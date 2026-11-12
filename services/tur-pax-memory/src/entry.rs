// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! A memory entry: what was remembered, where it came from and how far it is
//! to be trusted.
//!
//! A [`MemoryEntry`] cannot be built field by field. It comes from
//! [`crate::Memory::add`], which derives the trust level and the state from
//! the kind, or from a stored file, which is checked against the same rules on
//! the way in. A file that claims `user_stated` trust for an entry taken from
//! content is rejected as damaged, not believed.
//!
//! This type is the user's view of their memory: the text is readable, for
//! listing and review. What goes to a model is produced by
//! [`crate::Memory::recall`] and is a different type.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ids::EntryId;
use crate::time::Timestamp;

/// The schema version of a stored memory entry.
pub const ENTRY_SCHEMA_VERSION: u32 = 1;
/// The longest entry text, in bytes.
pub const MAX_TEXT_BYTES: usize = 4000;
/// The largest number of tags on an entry.
pub const MAX_TAGS: usize = 16;
/// The longest tag, in bytes.
pub const MAX_TAG_BYTES: usize = 64;
/// The longest application identifier, in bytes.
pub const MAX_APPLICATION_BYTES: usize = 255;
/// The longest description of where an entry came from, in bytes.
pub const MAX_ORIGIN_BYTES: usize = 512;
/// The longest task identifier, in bytes.
pub const MAX_TASK_BYTES: usize = 128;
/// The longest procedure name, in bytes.
pub const MAX_PROCEDURE_NAME_BYTES: usize = 120;
/// The largest number of steps in a procedure.
pub const MAX_PROCEDURE_STEPS: usize = 64;
/// The largest size of one step's arguments, as JSON, in bytes.
pub const MAX_STEP_ARGUMENT_BYTES: usize = 8192;

/// What kind of thing was remembered (addendum A section 16.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// The user said it.
    Stated,
    /// The assistant observed it from its own completed actions.
    Observed,
    /// A sequence of steps saved from a task the user accepted.
    Procedure,
    /// Taken from content the assistant read: a screen, a file, a tool result.
    FromContent,
}

impl EntryKind {
    /// Every kind, in the order recall prefers them at equal relevance.
    pub const ALL: [EntryKind; 4] = [
        EntryKind::Stated,
        EntryKind::Procedure,
        EntryKind::Observed,
        EntryKind::FromContent,
    ];

    /// The kind as it appears in JSON and on the command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            EntryKind::Stated => "stated",
            EntryKind::Observed => "observed",
            EntryKind::Procedure => "procedure",
            EntryKind::FromContent => "from_content",
        }
    }

    /// Parse a kind from its text form.
    pub fn parse(text: &str) -> Option<EntryKind> {
        EntryKind::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// The trust level of an entry of this kind. The kind alone decides it;
    /// nothing can raise it afterwards, not even the user's confirmation that
    /// an entry taken from content may be kept.
    pub const fn trust(self) -> Trust {
        match self {
            EntryKind::Stated => Trust::UserStated,
            EntryKind::Observed | EntryKind::Procedure => Trust::AssistantObserved,
            EntryKind::FromContent => Trust::Untrusted,
        }
    }

    /// The state a new entry of this kind starts in. Entries taken from
    /// content are quarantined until the user confirms them.
    pub const fn initial_state(self) -> EntryState {
        match self {
            EntryKind::FromContent => EntryState::Pending,
            EntryKind::Stated | EntryKind::Observed | EntryKind::Procedure => EntryState::Active,
        }
    }

    /// The position of the kind in recall's order of preference: lower is
    /// preferred.
    pub const fn recall_rank(self) -> u8 {
        match self {
            EntryKind::Stated => 0,
            EntryKind::Procedure => 1,
            EntryKind::Observed => 2,
            EntryKind::FromContent => 3,
        }
    }
}

/// How far an entry is to be trusted as a statement of fact. It never makes an
/// entry an instruction: no trust level carries authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    /// The user said it.
    UserStated,
    /// The assistant recorded it from what it did itself.
    AssistantObserved,
    /// It came from content. It may be false or planted.
    Untrusted,
}

impl Trust {
    /// The trust level as it appears in JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Trust::UserStated => "user_stated",
            Trust::AssistantObserved => "assistant_observed",
            Trust::Untrusted => "untrusted",
        }
    }
}

/// Whether an entry takes part in recall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    /// The entry can be recalled.
    Active,
    /// The entry was taken from content and waits for the user. It is never
    /// recalled.
    Pending,
}

impl EntryState {
    /// The state as it appears in JSON and on the command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            EntryState::Active => "active",
            EntryState::Pending => "pending",
        }
    }

    /// Parse a state from its text form.
    pub fn parse(text: &str) -> Option<EntryState> {
        [EntryState::Active, EntryState::Pending]
            .into_iter()
            .find(|state| state.as_str() == text)
    }
}

/// Where an entry came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// Where the entry came from, in words: "conversation", a window title, a
    /// file path, a tool name. For an entry taken from content this text is
    /// itself content and is never trusted.
    pub origin: String,
    /// The task during which the entry was recorded, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// When the entry was recorded.
    pub recorded_at: Timestamp,
}

/// One step of a saved procedure: a tool and a template for its arguments.
///
/// This crate does not interpret the template. A string such as
/// `{{recipient}}` is stored and returned as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcedureStep {
    /// The D-Bus name of the service that owns the tool.
    pub service: String,
    /// The tool name.
    pub tool: String,
    /// The argument template.
    pub arguments: Map<String, Value>,
}

/// A saved procedure: a named sequence of steps recorded from a task the user
/// accepted.
///
/// A procedure is a note about what worked. It is not a permission: replaying
/// it is an ordinary task, and every step goes through the policy engine again
/// (addendum A section 16.1). There is deliberately no conversion from a
/// procedure step to a standing approval or to an approval request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Procedure {
    /// The name of the procedure.
    pub name: String,
    /// The steps, in order.
    pub steps: Vec<ProcedureStep>,
}

/// What the caller supplies to remember something new.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEntry {
    /// The kind. The orchestrator sets it from what happened in the task, not
    /// from anything the model says about itself.
    pub kind: EntryKind,
    /// The text to remember. For a procedure, a sentence saying what it does.
    pub text: String,
    /// The steps, for a procedure; `None` otherwise.
    pub procedure: Option<Procedure>,
    /// Where it came from.
    pub origin: String,
    /// The task during which it was recorded.
    pub task: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
    /// The application it concerns.
    pub application: Option<String>,
}

impl NewEntry {
    fn new(kind: EntryKind, text: &str, origin: &str) -> NewEntry {
        NewEntry {
            kind,
            text: text.to_owned(),
            procedure: None,
            origin: origin.to_owned(),
            task: None,
            tags: Vec::new(),
            application: None,
        }
    }

    /// Something the user said.
    pub fn stated(text: &str, origin: &str) -> NewEntry {
        NewEntry::new(EntryKind::Stated, text, origin)
    }

    /// Something the assistant observed from its own completed actions.
    pub fn observed(text: &str, origin: &str) -> NewEntry {
        NewEntry::new(EntryKind::Observed, text, origin)
    }

    /// Something taken from content. It will wait for the user's confirmation.
    pub fn from_content(text: &str, origin: &str) -> NewEntry {
        NewEntry::new(EntryKind::FromContent, text, origin)
    }

    /// A procedure saved from a task the user accepted.
    pub fn procedure(text: &str, procedure: Procedure, origin: &str) -> NewEntry {
        NewEntry {
            procedure: Some(procedure),
            ..NewEntry::new(EntryKind::Procedure, text, origin)
        }
    }

    /// Record the task.
    pub fn with_task(mut self, task: &str) -> NewEntry {
        self.task = Some(task.to_owned());
        self
    }

    /// Add tags.
    pub fn with_tags(mut self, tags: &[&str]) -> NewEntry {
        self.tags.extend(tags.iter().map(|tag| (*tag).to_owned()));
        self
    }

    /// Record the application the entry concerns.
    pub fn with_application(mut self, application: &str) -> NewEntry {
        self.application = Some(application.to_owned());
        self
    }
}

/// One stored memory entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawEntry", into = "RawEntry")]
pub struct MemoryEntry {
    id: EntryId,
    kind: EntryKind,
    state: EntryState,
    text: String,
    procedure: Option<Procedure>,
    provenance: Provenance,
    created_at: Timestamp,
    updated_at: Timestamp,
    confirmed_at: Option<Timestamp>,
    tags: Vec<String>,
    application: Option<String>,
}

impl MemoryEntry {
    /// A new entry. The state and the trust level follow from the kind.
    pub(crate) fn create(id: EntryId, new: NewEntry, now: Timestamp) -> Result<MemoryEntry, String> {
        let entry = MemoryEntry {
            id,
            kind: new.kind,
            state: new.kind.initial_state(),
            text: new.text,
            procedure: new.procedure,
            provenance: Provenance {
                origin: new.origin,
                task: new.task,
                recorded_at: now,
            },
            created_at: now,
            updated_at: now,
            confirmed_at: None,
            tags: new.tags,
            application: new.application,
        };
        entry.check()?;
        Ok(entry)
    }

    /// Read an entry from the bytes of its file.
    ///
    /// The reason for a failure names fields and positions only. It never
    /// quotes the file, so a damaged entry's text does not reach a log.
    pub(crate) fn from_json(bytes: &[u8]) -> Result<MemoryEntry, String> {
        let raw: RawEntry =
            serde_json::from_slice(bytes).map_err(|err| crate::json::describe_error(&err))?;
        MemoryEntry::try_from(raw)
    }

    /// The entry made active by the user's confirmation at `at`.
    pub(crate) fn confirmed(&self, at: Timestamp) -> Result<MemoryEntry, String> {
        let entry = MemoryEntry {
            state: EntryState::Active,
            confirmed_at: Some(at),
            updated_at: at.max(self.created_at),
            ..self.clone()
        };
        entry.check()?;
        Ok(entry)
    }

    /// The entry with its text replaced at `at`.
    pub(crate) fn corrected(&self, text: &str, at: Timestamp) -> Result<MemoryEntry, String> {
        let entry = MemoryEntry {
            text: text.to_owned(),
            updated_at: at.max(self.created_at),
            ..self.clone()
        };
        entry.check()?;
        Ok(entry)
    }

    /// The identifier.
    pub fn id(&self) -> &EntryId {
        &self.id
    }

    /// The kind.
    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    /// The trust level, which follows from the kind.
    pub fn trust(&self) -> Trust {
        self.kind.trust()
    }

    /// Whether the entry is active or waits for confirmation.
    pub fn state(&self) -> EntryState {
        self.state
    }

    /// The remembered text, for showing to the user.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The steps, if the entry is a procedure.
    pub fn procedure(&self) -> Option<&Procedure> {
        self.procedure.as_ref()
    }

    /// Where the entry came from.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// When the entry was created.
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// When the entry was last changed.
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }

    /// When the user confirmed an entry taken from content.
    pub fn confirmed_at(&self) -> Option<Timestamp> {
        self.confirmed_at
    }

    /// The tags.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// The application the entry concerns.
    pub fn application(&self) -> Option<&str> {
        self.application.as_deref()
    }

    /// Every text of the entry that the secret detector must see, with the
    /// name of the field it is in.
    pub(crate) fn texts(&self) -> Vec<(&'static str, &str)> {
        let mut texts = vec![
            ("text", self.text.as_str()),
            ("origin", self.provenance.origin.as_str()),
        ];
        if let Some(task) = &self.provenance.task {
            texts.push(("task", task.as_str()));
        }
        if let Some(application) = &self.application {
            texts.push(("application", application.as_str()));
        }
        for tag in &self.tags {
            texts.push(("tags", tag.as_str()));
        }
        if let Some(procedure) = &self.procedure {
            texts.push(("procedure name", procedure.name.as_str()));
        }
        texts
    }

    // The rules every entry obeys, new or loaded.
    fn check(&self) -> Result<(), String> {
        if !self.id.as_str().starts_with(EntryId::PREFIX) {
            return Err("the identifier is not a memory identifier".to_owned());
        }
        check_text("text", &self.text, MAX_TEXT_BYTES, true)?;
        check_text("origin", &self.provenance.origin, MAX_ORIGIN_BYTES, false)?;
        if let Some(task) = &self.provenance.task {
            check_text("task", task, MAX_TASK_BYTES, false)?;
        }
        if let Some(application) = &self.application {
            check_text("application", application, MAX_APPLICATION_BYTES, false)?;
        }
        if self.tags.len() > MAX_TAGS {
            return Err(format!("more than {MAX_TAGS} tags"));
        }
        for tag in &self.tags {
            check_text("tag", tag, MAX_TAG_BYTES, false)?;
        }
        match (&self.procedure, self.kind) {
            (Some(procedure), EntryKind::Procedure) => check_procedure(procedure)?,
            (None, EntryKind::Procedure) => {
                return Err("a procedure entry has no steps".to_owned());
            }
            (Some(_), _) => {
                return Err("only a procedure entry may carry steps".to_owned());
            }
            (None, _) => {}
        }
        match (self.kind, self.state, self.confirmed_at) {
            (EntryKind::FromContent, EntryState::Pending, None) => {}
            (EntryKind::FromContent, EntryState::Active, Some(_)) => {}
            (EntryKind::FromContent, EntryState::Active, None) => {
                return Err(
                    "an entry taken from content is active without the user's confirmation"
                        .to_owned(),
                );
            }
            (EntryKind::FromContent, EntryState::Pending, Some(_)) => {
                return Err("a pending entry carries a confirmation time".to_owned());
            }
            (_, EntryState::Active, None) => {}
            (_, EntryState::Pending, _) => {
                return Err("only an entry taken from content can be pending".to_owned());
            }
            (_, EntryState::Active, Some(_)) => {
                return Err(
                    "only an entry taken from content carries a confirmation time".to_owned(),
                );
            }
        }
        let times = [
            Some(self.created_at),
            Some(self.updated_at),
            Some(self.provenance.recorded_at),
            self.confirmed_at,
        ];
        if times.iter().flatten().any(|time| !time.is_supported()) {
            return Err("a time is outside the supported range".to_owned());
        }
        if self.updated_at < self.created_at {
            return Err("the entry was updated before it was created".to_owned());
        }
        Ok(())
    }
}

/// Check a text field: not empty, within its size, no control characters
/// except, where `multiline` is set, line breaks and tabs.
pub(crate) fn check_text(
    field: &str,
    text: &str,
    max_bytes: usize,
    multiline: bool,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(format!("the {field} is empty"));
    }
    if text.len() > max_bytes {
        return Err(format!("the {field} is longer than {max_bytes} bytes"));
    }
    let allowed = |c: char| !c.is_control() || (multiline && (c == '\n' || c == '\t'));
    if !text.chars().all(allowed) {
        return Err(format!("the {field} contains a control character"));
    }
    Ok(())
}

/// Whether `name` has the form of a tool name: a lower-case letter, then up to
/// 63 lower-case letters, digits and underscores. The same form the tool
/// manifest schema requires.
pub(crate) fn is_tool_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// Whether `name` has the form of a D-Bus service name: 3 to 255 characters,
/// letters, digits, `_`, `-` and `.`, with at least one dot and no empty
/// element.
pub(crate) fn is_service_name(name: &str) -> bool {
    (3..=255).contains(&name.len())
        && name.contains('.')
        && name.split('.').all(|element| {
            !element.is_empty()
                && element
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
}

fn check_procedure(procedure: &Procedure) -> Result<(), String> {
    check_text(
        "procedure name",
        &procedure.name,
        MAX_PROCEDURE_NAME_BYTES,
        false,
    )?;
    if procedure.steps.is_empty() {
        return Err("the procedure has no steps".to_owned());
    }
    if procedure.steps.len() > MAX_PROCEDURE_STEPS {
        return Err(format!("more than {MAX_PROCEDURE_STEPS} steps"));
    }
    for (index, step) in procedure.steps.iter().enumerate() {
        let number = index + 1;
        if !is_service_name(&step.service) {
            return Err(format!("step {number} does not name a service"));
        }
        if !is_tool_name(&step.tool) {
            return Err(format!("step {number} does not name a tool"));
        }
        let size = serde_json::to_vec(&step.arguments).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_STEP_ARGUMENT_BYTES {
            return Err(format!(
                "the arguments of step {number} are larger than {MAX_STEP_ARGUMENT_BYTES} bytes"
            ));
        }
    }
    Ok(())
}

// The stored form. Every field of the JSON document, in the order it is
// written. `trust` is redundant with `kind`; it is written so that a reader of
// the file sees it, and checked on the way in so that it cannot disagree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    schema_version: u32,
    id: EntryId,
    kind: EntryKind,
    state: EntryState,
    trust: Trust,
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    procedure: Option<Procedure>,
    provenance: Provenance,
    created_at: Timestamp,
    updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confirmed_at: Option<Timestamp>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    application: Option<String>,
}

impl From<MemoryEntry> for RawEntry {
    fn from(entry: MemoryEntry) -> RawEntry {
        RawEntry {
            schema_version: ENTRY_SCHEMA_VERSION,
            trust: entry.kind.trust(),
            id: entry.id,
            kind: entry.kind,
            state: entry.state,
            text: entry.text,
            procedure: entry.procedure,
            provenance: entry.provenance,
            created_at: entry.created_at,
            updated_at: entry.updated_at,
            confirmed_at: entry.confirmed_at,
            tags: entry.tags,
            application: entry.application,
        }
    }
}

impl TryFrom<RawEntry> for MemoryEntry {
    type Error = String;

    fn try_from(raw: RawEntry) -> Result<MemoryEntry, String> {
        if raw.schema_version != ENTRY_SCHEMA_VERSION {
            return Err(format!(
                "unsupported schema version {}; this program reads version {ENTRY_SCHEMA_VERSION}",
                raw.schema_version
            ));
        }
        if raw.trust != raw.kind.trust() {
            return Err(format!(
                "the trust level \"{}\" does not follow from the kind \"{}\"",
                raw.trust.as_str(),
                raw.kind.as_str()
            ));
        }
        let entry = MemoryEntry {
            id: raw.id,
            kind: raw.kind,
            state: raw.state,
            text: raw.text,
            procedure: raw.procedure,
            provenance: raw.provenance,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
            confirmed_at: raw.confirmed_at,
            tags: raw.tags,
            application: raw.application,
        };
        entry.check()?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn id() -> EntryId {
        EntryId::from_sequence(1)
    }

    #[test]
    fn trust_and_state_follow_from_the_kind() {
        assert_eq!(EntryKind::Stated.trust(), Trust::UserStated);
        assert_eq!(EntryKind::Observed.trust(), Trust::AssistantObserved);
        assert_eq!(EntryKind::Procedure.trust(), Trust::AssistantObserved);
        assert_eq!(EntryKind::FromContent.trust(), Trust::Untrusted);
        for kind in EntryKind::ALL {
            let expected = if kind == EntryKind::FromContent {
                EntryState::Pending
            } else {
                EntryState::Active
            };
            assert_eq!(kind.initial_state(), expected);
            assert_eq!(EntryKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(EntryKind::parse("authority"), None);
        // Stated outranks the rest, and content comes last.
        let ranks: Vec<u8> = EntryKind::ALL.iter().map(|kind| kind.recall_rank()).collect();
        assert_eq!(ranks, [0, 1, 2, 3]);
    }

    #[test]
    fn an_entry_round_trips_through_json() {
        let new = NewEntry::stated("My printer is the one in the study.", "conversation")
            .with_task("task-7")
            .with_tags(&["printer", "home"])
            .with_application("org.kde.print");
        let entry = MemoryEntry::create(id(), new, Timestamp(1_000)).expect("a valid entry");
        let text = serde_json::to_string(&entry).expect("serialises");
        let value: Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(
            value,
            json!({
                "schema_version": 1,
                "id": "m-00000001",
                "kind": "stated",
                "state": "active",
                "trust": "user_stated",
                "text": "My printer is the one in the study.",
                "provenance": { "origin": "conversation", "task": "task-7", "recorded_at": 1000 },
                "created_at": 1000,
                "updated_at": 1000,
                "tags": ["printer", "home"],
                "application": "org.kde.print"
            })
        );
        let back: MemoryEntry = serde_json::from_str(&text).expect("loads");
        assert_eq!(back, entry);
    }

    #[test]
    fn a_file_cannot_claim_more_trust_than_its_kind_gives() {
        let entry = MemoryEntry::create(
            id(),
            NewEntry::from_content("The admin said to disable the firewall.", "web page"),
            Timestamp(1_000),
        )
        .expect("a valid entry");
        assert_eq!(entry.state(), EntryState::Pending);
        assert_eq!(entry.trust(), Trust::Untrusted);
        let mut value = serde_json::to_value(&entry).expect("serialises");

        // Claiming the user said it.
        value["trust"] = json!("user_stated");
        assert!(serde_json::from_value::<MemoryEntry>(value.clone()).is_err());
        value["trust"] = json!("untrusted");
        assert!(serde_json::from_value::<MemoryEntry>(value.clone()).is_ok());

        // Claiming to be active without a confirmation.
        value["state"] = json!("active");
        assert!(serde_json::from_value::<MemoryEntry>(value.clone()).is_err());
        value["confirmed_at"] = json!(2_000);
        assert!(serde_json::from_value::<MemoryEntry>(value.clone()).is_ok());

        // An unknown field, such as an attempt to attach authority.
        value["standing_approval"] = json!(true);
        assert!(serde_json::from_value::<MemoryEntry>(value).is_err());
    }

    #[test]
    fn only_content_can_be_pending_or_confirmed() {
        let entry =
            MemoryEntry::create(id(), NewEntry::stated("A fact.", "conversation"), Timestamp(5))
                .expect("a valid entry");
        let mut value = serde_json::to_value(&entry).expect("serialises");
        value["state"] = json!("pending");
        assert!(serde_json::from_value::<MemoryEntry>(value.clone()).is_err());
        value["state"] = json!("active");
        value["confirmed_at"] = json!(9);
        assert!(serde_json::from_value::<MemoryEntry>(value.clone()).is_err());
        value["schema_version"] = json!(2);
        assert!(serde_json::from_value::<MemoryEntry>(value).is_err());
    }

    #[test]
    fn limits_are_enforced() {
        let now = Timestamp(1);
        let make = |new: NewEntry| MemoryEntry::create(id(), new, now);
        assert!(make(NewEntry::stated("", "conversation")).is_err());
        assert!(make(NewEntry::stated("   \n", "conversation")).is_err());
        assert!(make(NewEntry::stated("fine", "")).is_err());
        assert!(make(NewEntry::stated(&"x".repeat(MAX_TEXT_BYTES), "conversation")).is_ok());
        assert!(make(NewEntry::stated(&"x".repeat(MAX_TEXT_BYTES + 1), "conversation")).is_err());
        assert!(make(NewEntry::stated("two\nlines\tand a tab", "conversation")).is_ok());
        assert!(make(NewEntry::stated("escape \u{1b}[31m", "conversation")).is_err());
        assert!(make(NewEntry::stated("fine", "line\nbreak")).is_err());
        assert!(make(NewEntry::stated("fine", "conversation").with_tags(&[""])).is_err());
        let many: Vec<String> = (0..=MAX_TAGS).map(|n| format!("t{n}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(make(NewEntry::stated("fine", "conversation").with_tags(&many)).is_err());
        assert!(
            MemoryEntry::create(id(), NewEntry::stated("fine", "conversation"), Timestamp(u64::MAX))
                .is_err()
        );
    }

    #[test]
    fn a_procedure_needs_steps_and_only_a_procedure_has_them() {
        let step = ProcedureStep {
            service: "org.uos.Network1".to_owned(),
            tool: "network_reset_profile".to_owned(),
            arguments: json!({ "profile": "{{profile}}" })
                .as_object()
                .cloned()
                .unwrap_or_default(),
        };
        let procedure = Procedure {
            name: "Reset the wireless profile".to_owned(),
            steps: vec![step.clone()],
        };
        let now = Timestamp(1);
        let good = NewEntry::procedure("Fixes wifi after suspend.", procedure.clone(), "task");
        let entry = MemoryEntry::create(id(), good, now).expect("a valid procedure");
        assert_eq!(entry.procedure(), Some(&procedure));
        assert_eq!(entry.trust(), Trust::AssistantObserved);

        let empty = Procedure {
            name: "Nothing".to_owned(),
            steps: Vec::new(),
        };
        assert!(MemoryEntry::create(id(), NewEntry::procedure("x", empty, "task"), now).is_err());

        let mut stated = NewEntry::stated("x", "conversation");
        stated.procedure = Some(procedure.clone());
        assert!(MemoryEntry::create(id(), stated, now).is_err());

        let mut bare = NewEntry::stated("x", "conversation");
        bare.kind = EntryKind::Procedure;
        assert!(MemoryEntry::create(id(), bare, now).is_err());

        for (service, tool) in [
            ("network", "network_reset_profile"),
            ("org.uos.Network1", "Reset"),
            ("org..Network1", "reset"),
            ("org.uos.Network1", "reset profile"),
        ] {
            let bad = Procedure {
                name: "Bad".to_owned(),
                steps: vec![ProcedureStep {
                    service: service.to_owned(),
                    tool: tool.to_owned(),
                    ..step.clone()
                }],
            };
            assert!(
                MemoryEntry::create(id(), NewEntry::procedure("x", bad, "task"), now).is_err(),
                "{service} {tool}"
            );
        }
    }
}
