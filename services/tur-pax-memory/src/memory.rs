// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The memory: what the assistant has been told and has learned, for one user.
//!
//! | Operation | Who | While memory is off |
//! |---|---|---|
//! | [`Memory::add`] | the assistant, through the orchestrator | refused |
//! | [`Memory::recall`] | the assistant, through the orchestrator | returns nothing |
//! | [`Memory::confirm`] | the user | refused |
//! | [`Memory::correct`] | the user | refused |
//! | [`Memory::set_enabled`] | the user | allowed |
//! | [`Memory::forget`] | either | allowed |
//! | [`Memory::get`], [`Memory::list`], [`Memory::export`] | the user | allowed |
//!
//! The operations marked "the user" take a [`UserConfirmation`], which this
//! library cannot produce. Forgetting and reading stay available while memory
//! is off: the user can always see and delete what was kept.
//!
//! A new store starts with memory **off**: the AI layer is optional and off by
//! default (addendum A section 2), and addendum A leaves open which categories
//! of memory are on by default (section 15, question 10).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::codes::{Code, Problem};
use crate::confirm::UserConfirmation;
use crate::entry::{EntryKind, EntryState, MemoryEntry, NewEntry};
use crate::ids::EntryId;
use crate::json::{describe_error, to_document};
use crate::recall::{RecallBudget, RecalledNotes, recall_from};
use crate::secret::{self, SecretKind};
use crate::store::{Collection, STATE_ID, Store, StoreError, is_oversized};
use crate::time::Timestamp;

/// The schema version of the memory state file.
pub const MEMORY_STATE_SCHEMA_VERSION: u32 = 1;

/// The largest number of entries the store accepts. A flood of entries,
/// whether from a runaway task or from content written to be remembered,
/// stops here.
pub const MAX_ENTRIES: usize = 2000;

/// The memory switch, as the subject of a user confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySwitch {
    /// Whether memory is to be on.
    pub enabled: bool,
}

// The memory state file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryState {
    schema_version: u32,
    enabled: bool,
    next_entry: u64,
}

impl Default for MemoryState {
    fn default() -> Self {
        MemoryState {
            schema_version: MEMORY_STATE_SCHEMA_VERSION,
            enabled: false,
            next_entry: 1,
        }
    }
}

/// A file in the store that could not be read as a record. It is left where
/// it is; nothing deletes it but [`Memory::forget`] on its identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DamagedFile {
    /// The identifier the file is named after.
    pub id: String,
    /// Why it could not be read. Never quotes the file.
    pub reason: String,
}

/// The entries of a store, and the files that could not be read.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    /// The readable entries that passed the filter, by identifier.
    pub entries: Vec<MemoryEntry>,
    /// Every damaged file, whatever the filter.
    pub damaged: Vec<DamagedFile>,
}

/// Which entries to list. Every condition that is set must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Only this kind.
    pub kind: Option<EntryKind>,
    /// Only this state.
    pub state: Option<EntryState>,
    /// Only entries with this tag.
    pub tag: Option<String>,
    /// Only entries about this application.
    pub application: Option<String>,
    /// Only entries whose text contains this, without regard to case.
    pub contains: Option<String>,
}

impl Filter {
    /// Whether an entry passes the filter.
    pub fn matches(&self, entry: &MemoryEntry) -> bool {
        self.kind.is_none_or(|kind| entry.kind() == kind)
            && self.state.is_none_or(|state| entry.state() == state)
            && self
                .tag
                .as_ref()
                .is_none_or(|tag| entry.tags().iter().any(|own| own == tag))
            && self
                .application
                .as_deref()
                .is_none_or(|application| entry.application() == Some(application))
            && self
                .contains
                .as_ref()
                .is_none_or(|needle| entry.text().to_lowercase().contains(&needle.to_lowercase()))
    }
}

/// One user's memory, kept in a [`Store`].
#[derive(Debug, Clone)]
pub struct Memory<S: Store> {
    store: S,
}

fn store_problem(err: StoreError) -> Problem {
    Problem::new(Code::MemoryStore, err.detail)
}

fn refusal(kind: SecretKind, field: &str) -> Problem {
    Problem::new(
        kind.code(),
        format!(
            "the {field} looks like it holds {}; nothing was stored",
            kind.describe()
        ),
    )
}

// Refuse an entry any of whose texts looks like a secret.
fn refuse_secrets(entry: &MemoryEntry) -> Result<(), Problem> {
    for (field, text) in entry.texts() {
        if let Some(kind) = secret::detect(text) {
            return Err(refusal(kind, field));
        }
    }
    if let Some(procedure) = entry.procedure() {
        for step in &procedure.steps {
            let found = secret::detect(&step.service)
                .or_else(|| secret::detect(&step.tool))
                .or_else(|| secret::detect_in_json(&Value::Object(step.arguments.clone())));
            if let Some(kind) = found {
                return Err(refusal(kind, "procedure step"));
            }
        }
    }
    Ok(())
}

impl<S: Store> Memory<S> {
    /// The memory kept in `store`.
    pub fn new(store: S) -> Self {
        Memory { store }
    }

    /// The store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Give the store back.
    pub fn into_store(self) -> S {
        self.store
    }

    fn read_state(&self) -> Result<MemoryState, Problem> {
        let bytes = self
            .store
            .get(Collection::MemoryState, STATE_ID)
            .map_err(store_problem)?;
        let Some(bytes) = bytes else {
            return Ok(MemoryState::default());
        };
        let damaged = |reason: String| {
            Problem::new(
                Code::MemoryDamaged,
                format!("the memory state file is damaged: {reason}"),
            )
        };
        if is_oversized(&bytes) {
            return Err(damaged("the file is larger than the limit".to_owned()));
        }
        let state: MemoryState =
            serde_json::from_slice(&bytes).map_err(|err| damaged(describe_error(&err)))?;
        if state.schema_version != MEMORY_STATE_SCHEMA_VERSION {
            return Err(damaged("unsupported schema version".to_owned()));
        }
        Ok(state)
    }

    fn write_state(&mut self, state: &MemoryState) -> Result<(), Problem> {
        let bytes = to_document(state).map_err(|reason| Problem::new(Code::MemoryStore, reason))?;
        self.store
            .put(Collection::MemoryState, STATE_ID, &bytes)
            .map_err(store_problem)
    }

    fn write_entry(&mut self, entry: &MemoryEntry) -> Result<(), Problem> {
        let bytes = to_document(entry).map_err(|reason| Problem::new(Code::MemoryStore, reason))?;
        self.store
            .put(Collection::MemoryEntries, entry.id().as_str(), &bytes)
            .map_err(store_problem)
    }

    // Remove what interrupted writes left behind.
    fn sweep(&mut self) -> Result<(), Problem> {
        self.store
            .sweep(Collection::MemoryEntries)
            .map_err(store_problem)?;
        self.store
            .sweep(Collection::MemoryState)
            .map_err(store_problem)?;
        Ok(())
    }

    fn require_enabled(&self) -> Result<MemoryState, Problem> {
        let state = self.read_state()?;
        if state.enabled {
            Ok(state)
        } else {
            Err(Problem::new(
                Code::MemoryOff,
                "memory is turned off; the user turns it on in settings",
            ))
        }
    }

    // One entry by the name of its file. `Ok(None)` if there is no such file;
    // a `MemoryDamaged` problem if the file cannot be read as an entry.
    fn read_entry(&self, id: &str) -> Result<Option<MemoryEntry>, Problem> {
        let bytes = self
            .store
            .get(Collection::MemoryEntries, id)
            .map_err(store_problem)?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        let damaged = |reason: String| Problem::new(Code::MemoryDamaged, reason);
        if is_oversized(&bytes) {
            return Err(damaged("the file is larger than the limit".to_owned()));
        }
        let entry = MemoryEntry::from_json(&bytes).map_err(damaged)?;
        if entry.id().as_str() != id {
            return Err(damaged(
                "the identifier inside the file is not the one it is named after".to_owned(),
            ));
        }
        Ok(Some(entry))
    }

    /// Whether memory is on.
    ///
    /// Fails with `UOS-AI-MEMORY-006` if the state file is damaged: it is then
    /// not known whether the user turned memory off, so nothing is written or
    /// recalled until the user sets the switch again.
    pub fn is_enabled(&self) -> Result<bool, Problem> {
        Ok(self.read_state()?.enabled)
    }

    /// Turn memory on or off. The user's act.
    ///
    /// Turning memory off stops writing and recall. It does not delete
    /// anything: the entries stay readable and can be exported or forgotten.
    /// Setting the switch also repairs a damaged state file.
    pub fn set_enabled(
        &mut self,
        switch: MemorySwitch,
        confirmation: &dyn UserConfirmation<MemorySwitch>,
    ) -> Result<(), Problem> {
        if !confirmation.covers(&switch) {
            return Err(Problem::new(
                Code::MemoryNotConfirmed,
                "the confirmation does not cover the memory switch",
            ));
        }
        let mut state = match self.read_state() {
            Ok(state) => state,
            Err(problem) if problem.code == Code::MemoryDamaged => MemoryState {
                next_entry: self.next_unused_sequence()?,
                ..MemoryState::default()
            },
            Err(problem) => return Err(problem),
        };
        state.enabled = switch.enabled;
        self.write_state(&state)
    }

    // One more than the largest sequence number among the entry files.
    fn next_unused_sequence(&self) -> Result<u64, Problem> {
        let ids = self
            .store
            .ids(Collection::MemoryEntries)
            .map_err(store_problem)?;
        let largest = ids
            .iter()
            .filter_map(|id| id.strip_prefix(EntryId::PREFIX))
            .filter_map(|digits| u64::from_str_radix(digits, 16).ok())
            .max()
            .unwrap_or(0);
        Ok(largest.saturating_add(1))
    }

    /// Remember something.
    ///
    /// The entry's trust level and state follow from its kind: an entry taken
    /// from content is stored as pending and is not recalled until the user
    /// confirms it. Refused with a `UOS-AI-MEMORY-SECRET-*` code if any of its
    /// texts looks like a secret, and with `UOS-AI-MEMORY-001` while memory is
    /// off.
    pub fn add(&mut self, new: NewEntry, now: Timestamp) -> Result<MemoryEntry, Problem> {
        let mut state = self.require_enabled()?;
        let existing = self
            .store
            .ids(Collection::MemoryEntries)
            .map_err(store_problem)?;
        if existing.len() >= MAX_ENTRIES {
            return Err(Problem::new(
                Code::MemoryFull,
                format!("the store already holds {MAX_ENTRIES} entries"),
            ));
        }
        // The counter normally points past every entry. If the state file was
        // lost, skip identifiers that are in use rather than replace an entry.
        let mut sequence = state.next_entry;
        let mut id = EntryId::from_sequence(sequence);
        while existing.iter().any(|known| known == id.as_str()) {
            sequence = sequence.saturating_add(1);
            id = EntryId::from_sequence(sequence);
        }
        let entry = MemoryEntry::create(id, new, now)
            .map_err(|reason| Problem::new(Code::MemoryInvalidEntry, reason))?;
        refuse_secrets(&entry)?;

        self.sweep()?;
        state.next_entry = sequence.saturating_add(1);
        self.write_state(&state)?;
        self.write_entry(&entry)?;
        Ok(entry)
    }

    /// Make a pending entry active. The user's act.
    ///
    /// The confirmation must cover the entry exactly as it is stored: it is
    /// what the user was shown. The entry stays untrusted; confirming says it
    /// may be kept and recalled, not that it is true or that it is an
    /// instruction.
    pub fn confirm(
        &mut self,
        id: &EntryId,
        confirmation: &dyn UserConfirmation<MemoryEntry>,
    ) -> Result<MemoryEntry, Problem> {
        self.require_enabled()?;
        let entry = self.get(id)?;
        if entry.state() != EntryState::Pending {
            return Err(Problem::new(
                Code::MemoryNotPending,
                format!("{id} is not waiting for confirmation"),
            ));
        }
        if !confirmation.covers(&entry) {
            return Err(Problem::new(
                Code::MemoryNotConfirmed,
                format!("the confirmation does not cover {id} as it is stored"),
            ));
        }
        let confirmed = entry
            .confirmed(confirmation.confirmed_at())
            .map_err(|reason| Problem::new(Code::MemoryInvalidEntry, reason))?;
        self.sweep()?;
        self.write_entry(&confirmed)?;
        Ok(confirmed)
    }

    /// Replace the text of an entry. The user's act.
    ///
    /// The confirmation must cover the entry as it will be stored. The new
    /// text passes the secret detector like any other. The kind, the trust
    /// level and the state do not change.
    pub fn correct(
        &mut self,
        id: &EntryId,
        text: &str,
        confirmation: &dyn UserConfirmation<MemoryEntry>,
    ) -> Result<MemoryEntry, Problem> {
        self.require_enabled()?;
        let entry = self.get(id)?;
        let corrected = entry
            .corrected(text, confirmation.confirmed_at())
            .map_err(|reason| Problem::new(Code::MemoryInvalidEntry, reason))?;
        refuse_secrets(&corrected)?;
        if !confirmation.covers(&corrected) {
            return Err(Problem::new(
                Code::MemoryNotConfirmed,
                format!("the confirmation does not cover the corrected {id}"),
            ));
        }
        self.sweep()?;
        self.write_entry(&corrected)?;
        Ok(corrected)
    }

    /// Forget an entry: remove its file, and anything an interrupted write of
    /// it left behind. Works on a damaged file too, and while memory is off.
    ///
    /// The bytes are removed from the store's files. What the storage device
    /// or a backup still holds is outside this crate's reach.
    pub fn forget(&mut self, id: &EntryId) -> Result<(), Problem> {
        self.sweep()?;
        let existed = self
            .store
            .remove(Collection::MemoryEntries, id.as_str())
            .map_err(store_problem)?;
        if existed {
            Ok(())
        } else {
            Err(Problem::new(
                Code::MemoryNotFound,
                format!("there is no entry {id}"),
            ))
        }
    }

    /// One entry, for showing to the user.
    pub fn get(&self, id: &EntryId) -> Result<MemoryEntry, Problem> {
        match self.read_entry(id.as_str()) {
            Ok(Some(entry)) => Ok(entry),
            Ok(None) => Err(Problem::new(
                Code::MemoryNotFound,
                format!("there is no entry {id}"),
            )),
            Err(problem) if problem.code == Code::MemoryDamaged => Err(Problem::new(
                Code::MemoryDamaged,
                format!("the file of {id} is damaged: {}", problem.detail),
            )),
            Err(problem) => Err(problem),
        }
    }

    /// The entries that pass a filter, and every damaged file.
    ///
    /// A file that cannot be read never hides the others: it is reported in
    /// [`Listing::damaged`] and the rest are returned.
    pub fn list(&self, filter: &Filter) -> Result<Listing, Problem> {
        let ids = self
            .store
            .ids(Collection::MemoryEntries)
            .map_err(store_problem)?;
        let mut listing = Listing {
            entries: Vec::new(),
            damaged: Vec::new(),
        };
        for id in ids {
            match self.read_entry(&id) {
                Ok(Some(entry)) => {
                    if filter.matches(&entry) {
                        listing.entries.push(entry);
                    }
                }
                // Removed between listing and reading.
                Ok(None) => {}
                Err(problem) if problem.code == Code::MemoryDamaged => {
                    listing.damaged.push(DamagedFile {
                        id,
                        reason: problem.detail,
                    });
                }
                Err(problem) => return Err(problem),
            }
        }
        Ok(listing)
    }

    /// Everything in the memory as one JSON document: the switch, every
    /// entry, pending ones included, and the damaged files.
    pub fn export(&self) -> Result<Value, Problem> {
        let listing = self.list(&Filter::default())?;
        let enabled = match self.read_state() {
            Ok(state) => Value::Bool(state.enabled),
            Err(problem) if problem.code == Code::MemoryDamaged => Value::Null,
            Err(problem) => return Err(problem),
        };
        let entries = serde_json::to_value(&listing.entries)
            .map_err(|_| Problem::new(Code::MemoryStore, "the entries could not be serialised"))?;
        let damaged = serde_json::to_value(&listing.damaged)
            .map_err(|_| Problem::new(Code::MemoryStore, "the listing could not be serialised"))?;
        Ok(json!({
            "enabled": enabled,
            "entries": entries,
            "damaged": damaged,
        }))
    }

    /// The active entries most relevant to a task, within a budget.
    ///
    /// Returns nothing while memory is off. Pending entries and damaged files
    /// are never recalled. See [`crate::recall`] for the scoring and for the
    /// form in which the result may be given to a model.
    pub fn recall(&self, task: &str, budget: RecallBudget) -> Result<RecalledNotes, Problem> {
        if !self.read_state()?.enabled {
            return Ok(RecalledNotes::none());
        }
        let listing = self.list(&Filter::default())?;
        Ok(recall_from(&listing.entries, task, budget))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Procedure, ProcedureStep, Trust};
    use crate::store::MemoryStore;
    use crate::testing::TestUser;

    const ON: MemorySwitch = MemorySwitch { enabled: true };
    const OFF: MemorySwitch = MemorySwitch { enabled: false };

    fn memory() -> Memory<MemoryStore> {
        let mut memory = Memory::new(MemoryStore::new());
        memory
            .set_enabled(ON, &TestUser::agreeing(1))
            .expect("turns on");
        memory
    }

    fn id(sequence: u64) -> EntryId {
        EntryId::from_sequence(sequence)
    }

    #[test]
    fn a_new_store_has_memory_off_and_writes_nothing() {
        let mut memory = Memory::new(MemoryStore::new());
        assert_eq!(memory.is_enabled(), Ok(false));
        let refused = memory
            .add(NewEntry::stated("A fact.", "conversation"), Timestamp(5))
            .expect_err("memory is off");
        assert_eq!(refused.code, Code::MemoryOff);
        assert_eq!(memory.store(), &MemoryStore::new(), "nothing was written");
        assert!(
            memory
                .recall("fact", RecallBudget::default())
                .expect("recalls")
                .is_empty()
        );
    }

    #[test]
    fn only_the_user_turns_memory_on() {
        let mut memory = Memory::new(MemoryStore::new());
        let refused = memory
            .set_enabled(ON, &TestUser::refusing(1))
            .expect_err("not confirmed");
        assert_eq!(refused.code, Code::MemoryNotConfirmed);
        assert_eq!(memory.is_enabled(), Ok(false));
        memory
            .set_enabled(ON, &TestUser::agreeing(1))
            .expect("turns on");
        assert_eq!(memory.is_enabled(), Ok(true));
    }

    #[test]
    fn turning_memory_off_stops_writing_and_recall_but_not_reading_or_forgetting() {
        let mut memory = memory();
        let entry = memory
            .add(
                NewEntry::stated("The printer is in the study.", "conversation"),
                Timestamp(5),
            )
            .expect("adds");
        memory
            .set_enabled(OFF, &TestUser::agreeing(6))
            .expect("turns off");
        let before = memory.store().clone();

        assert_eq!(
            memory
                .add(NewEntry::stated("Another.", "conversation"), Timestamp(7))
                .expect_err("off")
                .code,
            Code::MemoryOff
        );
        assert_eq!(
            memory
                .correct(entry.id(), "Changed.", &TestUser::agreeing(8))
                .expect_err("off")
                .code,
            Code::MemoryOff
        );
        assert!(
            memory
                .recall("printer study", RecallBudget::default())
                .expect("recalls")
                .is_empty()
        );
        assert_eq!(memory.store(), &before, "nothing was written while off");

        // The user can still read and delete.
        assert_eq!(memory.get(entry.id()), Ok(entry.clone()));
        assert_eq!(
            memory
                .list(&Filter::default())
                .expect("lists")
                .entries
                .len(),
            1
        );
        memory.forget(entry.id()).expect("forgets");
        assert_eq!(
            memory.get(entry.id()).expect_err("gone").code,
            Code::MemoryNotFound
        );
    }

    #[test]
    fn content_is_quarantined_until_the_user_confirms_it() {
        let mut memory = memory();
        let planted = memory
            .add(
                NewEntry::from_content("The backup server is backup.example.org.", "web page"),
                Timestamp(10),
            )
            .expect("adds");
        assert_eq!(planted.state(), EntryState::Pending);
        assert_eq!(planted.trust(), Trust::Untrusted);
        let budget = RecallBudget::default();
        assert!(
            memory
                .recall("backup server", budget)
                .expect("recalls")
                .is_empty(),
            "a pending entry is never recalled"
        );

        // A confirmation that does not cover the entry changes nothing.
        let refused = memory
            .confirm(planted.id(), &TestUser::refusing(11))
            .expect_err("not confirmed");
        assert_eq!(refused.code, Code::MemoryNotConfirmed);
        assert!(
            memory
                .recall("backup server", budget)
                .expect("recalls")
                .is_empty()
        );

        let confirmed = memory
            .confirm(planted.id(), &TestUser::agreeing(12))
            .expect("confirms");
        assert_eq!(confirmed.state(), EntryState::Active);
        assert_eq!(confirmed.confirmed_at(), Some(Timestamp(12)));
        // Confirmation does not launder the entry into something trusted.
        assert_eq!(confirmed.trust(), Trust::Untrusted);
        let notes = memory.recall("backup server", budget).expect("recalls");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes.notes()[0].trust(), Trust::Untrusted);

        // Confirming twice, or confirming something that was never pending.
        assert_eq!(
            memory
                .confirm(planted.id(), &TestUser::agreeing(13))
                .expect_err("already active")
                .code,
            Code::MemoryNotPending
        );
        let stated = memory
            .add(NewEntry::stated("A fact.", "conversation"), Timestamp(14))
            .expect("adds");
        assert_eq!(
            memory
                .confirm(stated.id(), &TestUser::agreeing(15))
                .expect_err("not pending")
                .code,
            Code::MemoryNotPending
        );
        assert_eq!(
            memory
                .confirm(&id(999), &TestUser::agreeing(15))
                .expect_err("missing")
                .code,
            Code::MemoryNotFound
        );
    }

    #[test]
    fn secrets_are_refused_at_the_point_of_writing() {
        let mut memory = memory();
        let before = memory.store().clone();
        let cases = [
            (
                NewEntry::stated("my password is hunter2", "conversation"),
                Code::MemorySecretPassword,
            ),
            (
                NewEntry::stated("card 4111 1111 1111 1111", "conversation"),
                Code::MemorySecretPaymentCard,
            ),
            (
                NewEntry::from_content("your verification code is 482913", "sms"),
                Code::MemorySecretOneTimeCode,
            ),
            (
                NewEntry::observed("fine text", "task").with_tags(&["pin: 4821"]),
                Code::MemorySecretPassword,
            ),
            (
                NewEntry::observed("fine text", "window: password=hunter2"),
                Code::MemorySecretPassword,
            ),
        ];
        for (new, code) in cases {
            let refused = memory.add(new, Timestamp(5)).expect_err("a secret");
            assert_eq!(refused.code, code);
            assert!(!refused.detail.contains("hunter2"), "{}", refused.detail);
            assert!(!refused.detail.contains("4111"), "{}", refused.detail);
        }
        assert_eq!(memory.store(), &before, "a refused entry leaves no trace");

        // Look-alikes are kept.
        for text in [
            "order number 9000123456789012 was delivered",
            "call the office on +39 06 1234 5678",
            "the fix is commit 3f786850e387550fdab836ed7e6dc881de23001b",
        ] {
            memory
                .add(NewEntry::observed(text, "task"), Timestamp(5))
                .unwrap_or_else(|problem| panic!("{text}: {problem}"));
        }
    }

    #[test]
    fn a_commit_hash_in_a_procedure_is_kept_and_a_token_is_not() {
        let mut memory = memory();
        let hash = "3f786850e387550fdab836ed7e6dc881de23001b";
        let step = |arguments: Value| ProcedureStep {
            service: "org.uos.Build1".to_owned(),
            tool: "build_checkout".to_owned(),
            arguments: arguments.as_object().cloned().unwrap_or_default(),
        };
        let procedure = Procedure {
            name: "Return to the last good build".to_owned(),
            steps: vec![
                step(json!({ "commit": hash })),
                step(json!({ "command": format!("git checkout {hash}") })),
            ],
        };
        let entry = memory
            .add(
                NewEntry::procedure("Checks out commit 29a020e and rebuilds.", procedure, "task"),
                Timestamp(5),
            )
            .expect("a commit hash is not a secret");
        assert_eq!(entry.kind(), EntryKind::Procedure);

        // A token in a step's arguments is refused, wherever it sits.
        let token = ["q7Zr2LmX9vB4tKc8NpW3", "yHd6JfS1gAe5UoT0iRbM"].concat();
        let leaking = Procedure {
            name: "Call the build service".to_owned(),
            steps: vec![step(json!({ "headers": [format!("Authorization: Bearer {token}")] }))],
        };
        let refused = memory
            .add(NewEntry::procedure("Starts a build.", leaking, "task"), Timestamp(5))
            .expect_err("a token");
        assert_eq!(refused.code, Code::MemorySecretKnownToken);
        assert!(!refused.detail.contains(&token), "{}", refused.detail);
    }

    #[test]
    fn a_correction_is_the_users_and_is_checked_like_a_new_entry() {
        let mut memory = memory();
        let entry = memory
            .add(
                NewEntry::stated("The printer is in the hall.", "conversation"),
                Timestamp(5),
            )
            .expect("adds");

        assert_eq!(
            memory
                .correct(
                    entry.id(),
                    "The printer is in the study.",
                    &TestUser::refusing(6)
                )
                .expect_err("not confirmed")
                .code,
            Code::MemoryNotConfirmed
        );
        assert_eq!(
            memory
                .correct(
                    entry.id(),
                    "the printer password is hunter2",
                    &TestUser::agreeing(6)
                )
                .expect_err("a secret")
                .code,
            Code::MemorySecretPassword
        );
        assert_eq!(
            memory
                .correct(entry.id(), "  ", &TestUser::agreeing(6))
                .expect_err("empty")
                .code,
            Code::MemoryInvalidEntry
        );
        assert_eq!(
            memory.get(entry.id()),
            Ok(entry.clone()),
            "unchanged so far"
        );

        let corrected = memory
            .correct(
                entry.id(),
                "The printer is in the study.",
                &TestUser::agreeing(7),
            )
            .expect("corrects");
        assert_eq!(corrected.text(), "The printer is in the study.");
        assert_eq!(corrected.kind(), entry.kind());
        assert_eq!(corrected.created_at(), Timestamp(5));
        assert_eq!(corrected.updated_at(), Timestamp(7));
        assert_eq!(memory.get(entry.id()), Ok(corrected));
    }

    #[test]
    fn listing_filters() {
        let mut memory = memory();
        let now = Timestamp(5);
        memory
            .add(
                NewEntry::stated("Dark mode everywhere.", "conversation").with_tags(&["ui"]),
                now,
            )
            .expect("adds");
        memory
            .add(
                NewEntry::observed("Firefox is the default browser.", "task")
                    .with_application("org.mozilla.firefox"),
                now,
            )
            .expect("adds");
        memory
            .add(NewEntry::from_content("Seen on a page.", "web page"), now)
            .expect("adds");

        let count = |filter: Filter| memory.list(&filter).expect("lists").entries.len();
        assert_eq!(count(Filter::default()), 3);
        assert_eq!(
            count(Filter {
                kind: Some(EntryKind::Stated),
                ..Filter::default()
            }),
            1
        );
        assert_eq!(
            count(Filter {
                state: Some(EntryState::Pending),
                ..Filter::default()
            }),
            1
        );
        assert_eq!(
            count(Filter {
                tag: Some("ui".to_owned()),
                ..Filter::default()
            }),
            1
        );
        assert_eq!(
            count(Filter {
                application: Some("org.mozilla.firefox".to_owned()),
                ..Filter::default()
            }),
            1
        );
        assert_eq!(
            count(Filter {
                contains: Some("FIREFOX".to_owned()),
                ..Filter::default()
            }),
            1
        );
        assert_eq!(
            count(Filter {
                kind: Some(EntryKind::Stated),
                state: Some(EntryState::Pending),
                ..Filter::default()
            }),
            0
        );
    }

    #[test]
    fn identifiers_are_not_reused_after_forgetting() {
        let mut memory = memory();
        let now = Timestamp(5);
        let first = memory
            .add(NewEntry::stated("One.", "conversation"), now)
            .expect("adds");
        let second = memory
            .add(NewEntry::stated("Two.", "conversation"), now)
            .expect("adds");
        assert_eq!(first.id(), &id(1));
        assert_eq!(second.id(), &id(2));
        memory.forget(second.id()).expect("forgets");
        let third = memory
            .add(NewEntry::stated("Three.", "conversation"), now)
            .expect("adds");
        assert_eq!(third.id(), &id(3));
        assert_eq!(
            memory.forget(second.id()).expect_err("gone").code,
            Code::MemoryNotFound
        );
    }

    #[test]
    fn export_holds_everything() {
        let mut memory = memory();
        let now = Timestamp(5);
        memory
            .add(NewEntry::stated("One.", "conversation"), now)
            .expect("adds");
        memory
            .add(NewEntry::from_content("Two.", "web page"), now)
            .expect("adds");
        let exported = memory.export().expect("exports");
        assert_eq!(exported["enabled"], true);
        assert_eq!(exported["entries"].as_array().map(Vec::len), Some(2));
        assert_eq!(exported["entries"][1]["state"], "pending");
        assert_eq!(exported["damaged"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn a_damaged_state_file_fails_closed_and_the_user_can_repair_it() {
        let mut memory = memory();
        let entry = memory
            .add(NewEntry::stated("One.", "conversation"), Timestamp(5))
            .expect("adds");
        let mut store = memory.into_store();
        store
            .put(Collection::MemoryState, STATE_ID, b"{\"enabled\": tr")
            .expect("plants damage");
        let mut memory = Memory::new(store);

        assert_eq!(
            memory.is_enabled().expect_err("damaged").code,
            Code::MemoryDamaged
        );
        assert_eq!(
            memory
                .add(NewEntry::stated("Two.", "conversation"), Timestamp(6))
                .expect_err("damaged")
                .code,
            Code::MemoryDamaged
        );
        assert_eq!(
            memory
                .recall("one", RecallBudget::default())
                .expect_err("damaged")
                .code,
            Code::MemoryDamaged
        );
        // Reading, exporting and forgetting do not need the state file.
        assert_eq!(memory.get(entry.id()), Ok(entry.clone()));
        assert_eq!(memory.export().expect("exports")["enabled"], Value::Null);

        // The user sets the switch again; the counter moves past what exists.
        memory
            .set_enabled(ON, &TestUser::agreeing(9))
            .expect("repairs");
        let next = memory
            .add(NewEntry::stated("Two.", "conversation"), Timestamp(10))
            .expect("adds");
        assert_eq!(next.id(), &id(2));
    }

    #[test]
    fn the_store_is_capped() {
        let mut store = MemoryStore::new();
        for sequence in 1..=MAX_ENTRIES {
            let name = id(u64::try_from(sequence).expect("fits"));
            store
                .put(Collection::MemoryEntries, name.as_str(), b"{}")
                .expect("plants");
        }
        let mut memory = Memory::new(store);
        memory
            .set_enabled(ON, &TestUser::agreeing(1))
            .expect("turns on");
        assert_eq!(
            memory
                .add(NewEntry::stated("One more.", "conversation"), Timestamp(5))
                .expect_err("full")
                .code,
            Code::MemoryFull
        );
    }
}
