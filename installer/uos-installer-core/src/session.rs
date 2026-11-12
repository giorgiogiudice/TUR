// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The durable session directory and the resume rules (specification §6.2,
//! §28.1 and §28.3).
//!
//! A session directory holds:
//!
//! * `session.json`: the session's identity, its target, and a snapshot of
//!   the phase table. Rewritten atomically after every transition.
//! * `phases.jsonl`: an append-only log with one [`TransitionRecord`] per
//!   line. This is the authority on what happened.
//! * `storage-inventory.json`: the inventory the plan was made from.
//! * `partition-plan.json`: the plan.
//!
//! # Crash safety
//!
//! Whole files are replaced with [`write_atomic`]: write a temporary file in
//! the same directory, flush it to stable storage, rename it over the
//! destination, flush the directory. A crash leaves either the old file or
//! the new one, never a mixture. The log is only ever appended to, and each
//! append is flushed before the transition is reported as done, so a crash
//! can damage at most the final, unacknowledged line. [`load`] tolerates
//! such a line.
//!
//! Before the first record of a phase that writes to the target is appended,
//! `session.json` is rewritten with `target_writes_started: true`. That flag
//! is never cleared. So if the log is later found shorter than it should be,
//! the session still says that the target may have been written.
//!
//! # Resume rules
//!
//! [`load`] never trusts the session's files further than this:
//!
//! * If there is **any** evidence that a destructive phase started (the
//!   flag, any log record, or the snapshot), the decision is
//!   [`ResumeDecision::NeedsReconciliation`], unless the log is intact and
//!   shows the installation complete. It is never "resume".
//!   [`LoadedSession::reconcile`] then requires a fresh inventory and
//!   revalidates the target's identity against it.
//! * Otherwise nothing was written to the target, and the decision is
//!   [`ResumeDecision::Resume`] from storage discovery or earlier. Discovery,
//!   target selection, planning and confirmation always run again after a
//!   restart: the inventory in the session is stale, and a confirmation
//!   lives only in memory.
//! * If the log is inconsistent and nothing suggests the target was written,
//!   the decision is [`ResumeDecision::Restart`].
//!
//! File-system access goes through [`FileSystem`] and time through
//! [`crate::time::Clock`], so the tests can inject failures at every step.

use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codes::{Fault, InstallCode};
use crate::identity::{Identity, IdentityMismatch, revalidate};
use crate::inventory::{BlockDevice, Inventory};
use crate::phase::{
    Completion, PhaseId, PhaseRecord, PhaseState, PhaseTable, Progress, RepeatClass,
    TransitionError,
};
use crate::plan::{InstallMode, Plan, ValidatedTarget};
use crate::time::{Clock, Timestamp};

/// Version of the `session.json` and `phases.jsonl` formats.
pub const SESSION_SCHEMA_VERSION: u32 = 1;
/// The session's identity, target and phase snapshot.
pub const SESSION_FILE: &str = "session.json";
/// The append-only transition log.
pub const PHASES_FILE: &str = "phases.jsonl";
/// The inventory the plan was made from.
pub const INVENTORY_FILE: &str = "storage-inventory.json";
/// The partition plan.
pub const PLAN_FILE: &str = "partition-plan.json";

/// Session files and directories are readable by their owner only: they
/// hold serial numbers.
const FILE_MODE: u32 = 0o600;
const DIRECTORY_MODE: u32 = 0o700;
const MAX_SESSION_ID_LENGTH: usize = 64;

/// The file-system operations the session needs.
///
/// Every operation that changes something must be durable when it returns.
pub trait FileSystem {
    /// Creates a directory and its missing parents.
    fn create_dir_all(&self, directory: &Path) -> io::Result<()>;

    /// Reads a whole file. A missing file is `ErrorKind::NotFound`.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Creates or truncates `path`, writes `data` and flushes it to stable
    /// storage.
    fn write_and_sync(&self, path: &Path, data: &[u8]) -> io::Result<()>;

    /// Renames `from` to `to`, replacing `to` atomically.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Flushes a directory, making renames and new files in it durable.
    fn sync_dir(&self, directory: &Path) -> io::Result<()>;

    /// Appends `data` to `path`, creating it if needed, and flushes it to
    /// stable storage.
    fn append_and_sync(&self, path: &Path, data: &[u8]) -> io::Result<()>;
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsFileSystem;

impl FileSystem for OsFileSystem {
    fn create_dir_all(&self, directory: &Path) -> io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(DIRECTORY_MODE)
            .create(directory)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn write_and_sync(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(FILE_MODE)
            .open(path)?;
        file.write_all(data)?;
        file.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn sync_dir(&self, directory: &Path) -> io::Result<()> {
        File::open(directory)?.sync_all()
    }

    fn append_and_sync(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(FILE_MODE)
            .open(path)?;
        file.write_all(data)?;
        file.sync_all()
    }
}

/// Replaces `path` with `data` so that a crash at any point leaves either
/// the old contents or the new, never a mixture.
///
/// The steps are: write `<name>.tmp` beside the destination and flush it,
/// rename it over the destination, flush the directory.
pub fn write_atomic(file_system: &dyn FileSystem, path: &Path, data: &[u8]) -> io::Result<()> {
    let (Some(directory), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path has no directory or no file name",
        ));
    };
    let mut temporary_name = name.to_os_string();
    temporary_name.push(".tmp");
    let temporary = directory.join(temporary_name);
    file_system.write_and_sync(&temporary, data)?;
    file_system.rename(&temporary, path)?;
    file_system.sync_dir(directory)
}

/// The contents of `session.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    /// Format version.
    pub schema_version: u32,
    /// The session's identifier.
    pub session_id: String,
    /// The version of the installer that created the session.
    pub installer_version: String,
    /// When the session was created.
    pub created_at: Timestamp,
    /// When the session was last written.
    pub updated_at: Timestamp,
    /// The installation mode, once a plan is saved.
    pub mode: Option<InstallMode>,
    /// The identity of the target disk, once a plan is saved.
    pub target: Option<Identity>,
    /// True from just before the first record of a phase that writes to the
    /// target is appended to the log. Never cleared.
    pub target_writes_started: bool,
    /// The sequence number of the last log record this snapshot reflects.
    pub records_written: u64,
    /// A snapshot of the phase table.
    pub phases: PhaseTable,
}

/// One line of `phases.jsonl`: the state of one phase after a transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionRecord {
    /// Position in the log, starting at 1 and increasing by 1.
    pub seq: u64,
    /// When the record was written.
    pub recorded_at: Timestamp,
    /// The phase's record after the transition.
    pub phase: PhaseRecord,
}

/// Why a session operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// A file-system operation failed.
    Io {
        /// The file or directory.
        path: PathBuf,
        /// What was being done, for example `write`.
        action: &'static str,
        /// The underlying reason.
        reason: String,
    },
    /// The directory holds no session.
    NotFound {
        /// The directory.
        directory: PathBuf,
    },
    /// The directory already holds a session.
    AlreadyExists {
        /// The directory.
        directory: PathBuf,
    },
    /// A session file cannot be understood.
    Corrupt {
        /// The file's name.
        file: &'static str,
        /// What is wrong with it.
        reason: String,
    },
    /// The session was written in a format this installer does not read.
    UnsupportedVersion {
        /// The version found, when it could be read.
        found: Option<u64>,
    },
    /// The session identifier has characters that are not allowed.
    InvalidSessionId {
        /// The identifier.
        id: String,
    },
    /// The phase model refused the transition.
    Transition(TransitionError),
    /// A phase that writes to the target was started without a revalidated
    /// target.
    ValidatedTargetRequired {
        /// The phase.
        phase: PhaseId,
    },
    /// The revalidated target belongs to a plan other than the session's.
    TargetNotInPlan,
    /// The session has no plan.
    NoPlan,
    /// The plan cannot change once the target may have been written.
    PlanLocked,
    /// The session wrote to the target; it must be reconciled, not resumed.
    ReconciliationRequired {
        /// The last phase known to have written to the target.
        phase: PhaseId,
    },
    /// The session cannot be resumed.
    NotResumable {
        /// Why.
        reason: String,
    },
    /// An earlier write failed, so this object no longer matches the
    /// directory. Load the session again.
    Unusable,
}

fn io_error(path: &Path, action: &'static str, error: &io::Error) -> SessionError {
    SessionError::Io {
        path: path.to_path_buf(),
        action,
        reason: error.to_string(),
    }
}

fn corrupt(file: &'static str, error: &serde_json::Error) -> SessionError {
    SessionError::Corrupt {
        file,
        reason: error.to_string(),
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            SessionError::Io { .. } => {
                "The installer could not read or write its session directory. Check that \
                 the live environment has free space, then export the diagnostics."
            }
            SessionError::NotFound { .. } => "There is no installer session in this directory.",
            SessionError::AlreadyExists { .. } => {
                "This directory already holds an installer session. Each installation \
                 needs a new, empty session directory."
            }
            SessionError::Corrupt { .. } => {
                "The installer session's files are damaged and cannot be read. Start the \
                 installation again; the damaged session is kept for diagnosis."
            }
            SessionError::UnsupportedVersion { .. } => {
                "This installer session was written by a different version of the \
                 installer and cannot be read by this one."
            }
            SessionError::InvalidSessionId { .. } => {
                "The session identifier may contain only letters, digits, dots, hyphens \
                 and underscores."
            }
            SessionError::Transition(error) => return fmt::Display::fmt(error, f),
            SessionError::ValidatedTargetRequired { .. }
            | SessionError::TargetNotInPlan
            | SessionError::NoPlan => {
                "The installer stopped before changing the disk, because the disk had not \
                 been checked again against the confirmed plan. Nothing was written. \
                 Export the diagnostics and report this."
            }
            SessionError::PlanLocked => {
                "The plan cannot be changed, because the installer may already have \
                 written to the disk. Start the installation again to choose differently."
            }
            SessionError::ReconciliationRequired { .. } => {
                "An earlier installation was interrupted after it started changing the \
                 disk. It cannot simply continue. The disk must be examined again and you \
                 must confirm again before anything more is written."
            }
            SessionError::NotResumable { .. } => {
                "This installer session cannot be continued. Start the installation again."
            }
            SessionError::Unusable => {
                "The installer could not save its progress and stopped. Nothing further \
                 was done. Export the diagnostics and start the installation again."
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for SessionError {}

impl Fault for SessionError {
    fn code(&self) -> InstallCode {
        match self {
            SessionError::Io { .. } => InstallCode::SessionIo,
            SessionError::NotFound { .. } => InstallCode::SessionNotFound,
            SessionError::AlreadyExists { .. } => InstallCode::SessionAlreadyExists,
            SessionError::Corrupt { .. } => InstallCode::SessionCorrupt,
            SessionError::UnsupportedVersion { .. } => InstallCode::SessionUnsupportedVersion,
            SessionError::InvalidSessionId { .. } => InstallCode::SessionInvalidId,
            SessionError::Transition(error) => error.code(),
            SessionError::ValidatedTargetRequired { .. } | SessionError::TargetNotInPlan => {
                InstallCode::SessionTargetNotValidated
            }
            SessionError::NoPlan => InstallCode::SessionNoPlan,
            SessionError::PlanLocked => InstallCode::SessionPlanLocked,
            SessionError::ReconciliationRequired { .. } => {
                InstallCode::SessionReconciliationRequired
            }
            SessionError::NotResumable { .. } => InstallCode::SessionNotResumable,
            SessionError::Unusable => InstallCode::SessionUnusable,
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            SessionError::Io {
                path,
                action,
                reason,
            } => format!("could not {action} {}: {reason}", path.display()),
            SessionError::NotFound { directory } => {
                format!("{} has no {SESSION_FILE}", directory.display())
            }
            SessionError::AlreadyExists { directory } => {
                format!("{} already has a {SESSION_FILE}", directory.display())
            }
            SessionError::Corrupt { file, reason } => format!("{file}: {reason}"),
            SessionError::UnsupportedVersion { found } => match found {
                Some(version) => format!(
                    "schema_version {version}; this installer reads {SESSION_SCHEMA_VERSION}"
                ),
                None => "schema_version is missing or not a number".to_owned(),
            },
            SessionError::InvalidSessionId { id } => format!("session id: \"{id}\""),
            SessionError::Transition(error) => error.technical_detail(),
            SessionError::ValidatedTargetRequired { phase } => {
                format!(
                    "phase {phase} writes to the target and was begun without a ValidatedTarget"
                )
            }
            SessionError::TargetNotInPlan => {
                "the ValidatedTarget was produced from a plan other than the session's".to_owned()
            }
            SessionError::NoPlan => "no plan has been saved in this session".to_owned(),
            SessionError::PlanLocked => "target_writes_started is true".to_owned(),
            SessionError::ReconciliationRequired { phase } => {
                format!("phase {phase} has started; resume is refused")
            }
            SessionError::NotResumable { reason } => reason.clone(),
            SessionError::Unusable => {
                "an earlier append or atomic write returned an error".to_owned()
            }
        }
    }
}

fn validate_session_id(id: &str) -> Result<(), SessionError> {
    let allowed =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_');
    if id.is_empty()
        || id.len() > MAX_SESSION_ID_LENGTH
        || id.starts_with('.')
        || !id.chars().all(allowed)
    {
        return Err(SessionError::InvalidSessionId { id: id.to_owned() });
    }
    Ok(())
}

fn pretty_json<T: Serialize>(file: &'static str, value: &T) -> Result<Vec<u8>, SessionError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| corrupt(file, &error))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// A session that is being written.
///
/// Every transition is appended to the log and flushed before the method
/// returns. If any write fails the session becomes unusable: the caller must
/// stop and [`load`] the directory again, which applies the resume rules.
pub struct Session<'a> {
    file_system: &'a dyn FileSystem,
    clock: &'a dyn Clock,
    directory: PathBuf,
    meta: SessionMeta,
    phases: PhaseTable,
    plan: Option<Plan>,
    next_seq: u64,
    log_needs_newline: bool,
    unusable: bool,
}

impl fmt::Debug for Session<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("directory", &self.directory)
            .field("session_id", &self.meta.session_id)
            .field("next_seq", &self.next_seq)
            .field("unusable", &self.unusable)
            .finish_non_exhaustive()
    }
}

impl<'a> Session<'a> {
    /// Creates a session in `directory`, which must not already hold one,
    /// and records the `START` phase.
    pub fn create(
        file_system: &'a dyn FileSystem,
        clock: &'a dyn Clock,
        directory: &Path,
        session_id: &str,
    ) -> Result<Session<'a>, SessionError> {
        validate_session_id(session_id)?;
        let meta_path = directory.join(SESSION_FILE);
        match file_system.read(&meta_path) {
            Ok(_) => {
                return Err(SessionError::AlreadyExists {
                    directory: directory.to_path_buf(),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(&meta_path, "read", &error)),
        }
        file_system
            .create_dir_all(directory)
            .map_err(|error| io_error(directory, "create", &error))?;
        let now = clock.now();
        let phases = PhaseTable::new();
        let meta = SessionMeta {
            schema_version: SESSION_SCHEMA_VERSION,
            session_id: session_id.to_owned(),
            installer_version: env!("CARGO_PKG_VERSION").to_owned(),
            created_at: now,
            updated_at: now,
            mode: None,
            target: None,
            target_writes_started: false,
            records_written: 0,
            phases: phases.clone(),
        };
        let mut session = Session {
            file_system,
            clock,
            directory: directory.to_path_buf(),
            meta,
            phases,
            plan: None,
            next_seq: 1,
            log_needs_newline: false,
            unusable: false,
        };
        session.write_meta()?;
        session.begin(PhaseId::Start)?;
        session.finish(
            PhaseId::Start,
            Completion::succeeded("Session directory created"),
        )?;
        Ok(session)
    }

    /// The session directory.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The session's metadata as last written.
    pub fn meta(&self) -> &SessionMeta {
        &self.meta
    }

    /// The current phase table.
    pub fn phases(&self) -> &PhaseTable {
        &self.phases
    }

    /// The plan saved in this session, if any.
    pub fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    fn ensure_usable(&self) -> Result<(), SessionError> {
        if self.unusable {
            Err(SessionError::Unusable)
        } else {
            Ok(())
        }
    }

    fn write_file(&mut self, name: &'static str, bytes: &[u8]) -> Result<(), SessionError> {
        let path = self.directory.join(name);
        let result = write_atomic(self.file_system, &path, bytes)
            .map_err(|error| io_error(&path, "write", &error));
        if result.is_err() {
            self.unusable = true;
        }
        result
    }

    fn write_meta(&mut self) -> Result<(), SessionError> {
        let bytes = pretty_json(SESSION_FILE, &self.meta)?;
        self.write_file(SESSION_FILE, &bytes)
    }

    /// Writes `storage-inventory.json`.
    pub fn save_inventory(&mut self, inventory: &Inventory) -> Result<(), SessionError> {
        self.ensure_usable()?;
        let bytes = pretty_json(INVENTORY_FILE, inventory)?;
        self.write_file(INVENTORY_FILE, &bytes)
    }

    /// Writes `partition-plan.json` and records the plan's mode and target
    /// in `session.json`.
    ///
    /// Refused once the target may have been written.
    pub fn save_plan(&mut self, plan: &Plan) -> Result<(), SessionError> {
        self.ensure_usable()?;
        if self.meta.target_writes_started {
            return Err(SessionError::PlanLocked);
        }
        let bytes = pretty_json(PLAN_FILE, plan)?;
        self.write_file(PLAN_FILE, &bytes)?;
        self.meta.mode = Some(plan.mode);
        self.meta.target = Some(plan.target.identity.clone());
        self.meta.updated_at = self.clock.now();
        self.plan = Some(plan.clone());
        self.write_meta()
    }

    fn apply<F>(&mut self, change: F) -> Result<(), SessionError>
    where
        F: FnOnce(&mut PhaseTable, Timestamp) -> Result<Vec<PhaseRecord>, TransitionError>,
    {
        self.ensure_usable()?;
        let now = self.clock.now();
        let mut next = self.phases.clone();
        let records = change(&mut next, now).map_err(SessionError::Transition)?;
        if records.is_empty() {
            return Ok(());
        }
        let result = self.persist(next, &records, now);
        if result.is_err() {
            self.unusable = true;
        }
        result
    }

    fn persist(
        &mut self,
        next: PhaseTable,
        records: &[PhaseRecord],
        now: Timestamp,
    ) -> Result<(), SessionError> {
        let mut buffer = Vec::new();
        if self.log_needs_newline {
            buffer.push(b'\n');
        }
        let mut seq = self.next_seq;
        for record in records {
            let entry = TransitionRecord {
                seq,
                recorded_at: now,
                phase: record.clone(),
            };
            let line = serde_json::to_vec(&entry).map_err(|error| corrupt(PHASES_FILE, &error))?;
            buffer.extend_from_slice(&line);
            buffer.push(b'\n');
            seq = seq.saturating_add(1);
        }
        let log_path = self.directory.join(PHASES_FILE);
        self.file_system
            .append_and_sync(&log_path, &buffer)
            .map_err(|error| io_error(&log_path, "append to", &error))?;
        if self.next_seq == 1 {
            // The log file may just have been created: make its directory
            // entry durable too.
            self.file_system
                .sync_dir(&self.directory)
                .map_err(|error| io_error(&self.directory, "flush", &error))?;
        }
        self.log_needs_newline = false;
        self.next_seq = seq;
        self.phases = next;
        self.meta.records_written = seq.saturating_sub(1);
        self.meta.updated_at = now;
        self.meta.phases = self.phases.clone();
        self.write_meta()
    }

    /// Starts a phase that does not write to the target, or retries one.
    ///
    /// A destructive phase is refused here: use
    /// [`Session::begin_destructive`].
    pub fn begin(&mut self, phase: PhaseId) -> Result<(), SessionError> {
        if phase.destructive() {
            return Err(SessionError::ValidatedTargetRequired { phase });
        }
        self.apply(|table, now| table.begin(phase, now).map(|record| vec![record]))
    }

    /// Starts a phase that writes to the target.
    ///
    /// `target` is proof that the confirmed plan's disk was found again,
    /// unchanged, in a fresh inventory. It must come from this session's
    /// plan. It is consumed: each phase that writes needs its own proof, so
    /// the disk is revalidated immediately before every such phase. Before
    /// the phase's first record is appended, `session.json` is rewritten
    /// with `target_writes_started: true`.
    pub fn begin_destructive(
        &mut self,
        phase: PhaseId,
        target: ValidatedTarget<'_>,
    ) -> Result<(), SessionError> {
        self.ensure_usable()?;
        match &self.plan {
            None => return Err(SessionError::NoPlan),
            Some(plan) if plan != target.plan() => return Err(SessionError::TargetNotInPlan),
            Some(_) => {}
        }
        // Check the transition first, so that an illegal request does not
        // mark the session as having written to the target.
        self.phases
            .clone()
            .begin(phase, Timestamp::from_unix_millis(0))
            .map_err(SessionError::Transition)?;
        if phase.destructive() && !self.meta.target_writes_started {
            self.meta.target_writes_started = true;
            self.write_meta()?;
        }
        self.apply(|table, now| table.begin(phase, now).map(|record| vec![record]))
    }

    /// Skips an optional phase.
    pub fn skip(&mut self, phase: PhaseId, summary: &str) -> Result<(), SessionError> {
        self.apply(|table, now| table.skip(phase, now, summary).map(|record| vec![record]))
    }

    /// Ends the running phase `phase`.
    pub fn finish(&mut self, phase: PhaseId, completion: Completion) -> Result<(), SessionError> {
        self.apply(|table, now| {
            table
                .finish(phase, now, completion)
                .map(|record| vec![record])
        })
    }

    /// Sets the progress of the running phase. Progress is kept in memory
    /// and written with the next transition.
    pub fn set_progress(&mut self, phase: PhaseId, progress: Progress) -> Result<(), SessionError> {
        self.ensure_usable()?;
        self.phases
            .set_progress(phase, progress)
            .map_err(SessionError::Transition)
    }

    /// Returns `phase` and every later phase to pending, so the user can go
    /// back. Refused once any destructive phase has started.
    pub fn reopen(&mut self, phase: PhaseId) -> Result<(), SessionError> {
        self.apply(|table, _| table.reopen(phase))
    }
}

/// Something unexpected found while loading a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Anomaly {
    /// A log line could not be parsed. At the end of the log this is the
    /// mark of a crash during an append, and the record was never
    /// acknowledged.
    UnreadableRecord {
        /// The line number, starting at 1.
        line: usize,
        /// True when it is the last line and has no line terminator.
        at_end: bool,
    },
    /// A record's sequence number is not the previous one plus one: records
    /// are missing or out of order.
    SequenceGap {
        /// The line number, starting at 1.
        line: usize,
        /// The sequence number expected.
        expected: u64,
        /// The sequence number found.
        found: u64,
    },
    /// `session.json` reflects more records than the log holds: the end of
    /// the log is missing.
    LogBehindSnapshot {
        /// The number of records the snapshot reflects.
        snapshot_records: u64,
        /// The last sequence number in the log.
        log_records: u64,
    },
    /// The plan file is missing, unreadable, or not for the session's target.
    UnreadablePlan {
        /// What is wrong.
        reason: String,
    },
}

/// What may be done with a loaded session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ResumeDecision {
    /// Nothing was written to the target. The session may continue from
    /// `from_phase`, which is storage discovery or earlier and never a
    /// destructive phase.
    Resume {
        /// The phase to run next.
        from_phase: PhaseId,
    },
    /// The log is inconsistent, and nothing suggests the target was written.
    /// Start a new session.
    Restart {
        /// What is inconsistent.
        reason: String,
    },
    /// A phase that writes to the target started. The session must not be
    /// resumed. Take a fresh inventory and call
    /// [`LoadedSession::reconcile`].
    NeedsReconciliation {
        /// The last phase known to have written to the target.
        phase: PhaseId,
        /// That phase's last known state.
        state: PhaseState,
        /// That phase's repeat class.
        class: RepeatClass,
    },
    /// The log is intact and every phase finished.
    Completed,
}

impl ResumeDecision {
    /// The decision in one sentence, for display.
    pub fn summary(&self) -> String {
        match self {
            ResumeDecision::Resume { from_phase } => format!(
                "Nothing was written to the target disk. The session can continue from \
                 {from_phase}; the disk will be discovered, chosen and confirmed again."
            ),
            ResumeDecision::Restart { reason } => format!(
                "The session log is inconsistent ({reason}). Nothing suggests the target \
                 disk was written. Start a new session."
            ),
            ResumeDecision::NeedsReconciliation {
                phase,
                state,
                class,
            } => format!(
                "Needs reconciliation: phase {phase} ({}) is recorded as {state}. The \
                 target disk may have been written. Do not resume: take a fresh \
                 inventory, revalidate the disk's identity and confirm again.",
                class.as_str()
            ),
            ResumeDecision::Completed => "The installation completed.".to_owned(),
        }
    }
}

struct Replay {
    table: PhaseTable,
    last_seq: u64,
    anomalies: Vec<Anomaly>,
    target_write_seen: Option<PhaseId>,
    unterminated: bool,
}

/// Replays the log. Unparseable lines are noted and skipped; every parseable
/// record is applied, and any record of a destructive phase that has left
/// the pending state is remembered.
fn replay_log(log: &[u8]) -> Replay {
    let mut replay = Replay {
        table: PhaseTable::new(),
        last_seq: 0,
        anomalies: Vec::new(),
        target_write_seen: None,
        unterminated: !log.is_empty() && !log.ends_with(b"\n"),
    };
    let chunks: Vec<&[u8]> = log.split(|byte| *byte == b'\n').collect();
    let last_index = chunks.len().saturating_sub(1);
    for (index, chunk) in chunks.iter().enumerate() {
        if chunk.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let line = index.saturating_add(1);
        match serde_json::from_slice::<TransitionRecord>(chunk) {
            Ok(entry) => {
                let expected = replay.last_seq.saturating_add(1);
                if entry.seq != expected {
                    replay.anomalies.push(Anomaly::SequenceGap {
                        line,
                        expected,
                        found: entry.seq,
                    });
                }
                replay.last_seq = entry.seq;
                let phase = entry.phase.phase_id;
                if phase.destructive() && entry.phase.state != PhaseState::Pending {
                    replay.target_write_seen = replay.target_write_seen.max(Some(phase));
                }
                replay.table.restore(entry.phase);
            }
            Err(_) => replay.anomalies.push(Anomaly::UnreadableRecord {
                line,
                at_end: index == last_index && replay.unterminated,
            }),
        }
    }
    replay
}

/// Applies the resume rules. See the module documentation.
fn decide(meta: &SessionMeta, replay: &Replay) -> ResumeDecision {
    let log_behind = meta.records_written > replay.last_seq;
    let gap = replay
        .anomalies
        .iter()
        .any(|anomaly| matches!(anomaly, Anomaly::SequenceGap { .. }));
    let untrusted = log_behind || gap;

    let evidence = [
        replay.table.latest_target_write(),
        replay.target_write_seen,
        meta.phases.latest_target_write(),
    ]
    .into_iter()
    .flatten()
    .max();
    let written = evidence.or(meta.target_writes_started.then_some(PhaseId::Partitioning));

    if let Some(phase) = written {
        if !untrusted && replay.table.is_complete() {
            return ResumeDecision::Completed;
        }
        let record = if log_behind {
            meta.phases.get(phase)
        } else {
            replay.table.get(phase)
        };
        return ResumeDecision::NeedsReconciliation {
            phase,
            state: record.state,
            class: phase.class(),
        };
    }
    if untrusted {
        let reason = if log_behind {
            "the log is shorter than the session snapshot"
        } else {
            "log records are missing or out of order"
        };
        return ResumeDecision::Restart {
            reason: reason.to_owned(),
        };
    }
    match replay.table.first_incomplete() {
        Some(first) => ResumeDecision::Resume {
            from_phase: first.min(PhaseId::StorageDiscovery),
        },
        None => ResumeDecision::Restart {
            reason: "every phase is finished but no phase wrote to the target".to_owned(),
        },
    }
}

/// A session read back from its directory, with the decision on what may be
/// done with it.
#[derive(Debug)]
pub struct LoadedSession {
    directory: PathBuf,
    meta: SessionMeta,
    phases: PhaseTable,
    plan: Option<Plan>,
    anomalies: Vec<Anomaly>,
    decision: ResumeDecision,
    last_seq: u64,
    log_unterminated: bool,
}

/// Reads the session in `directory` and decides what may be done with it.
///
/// This never panics on damaged files. A damaged or missing `session.json`
/// is an error; a damaged log is tolerated and reflected in the decision.
pub fn load(file_system: &dyn FileSystem, directory: &Path) -> Result<LoadedSession, SessionError> {
    let meta_path = directory.join(SESSION_FILE);
    let meta_bytes = match file_system.read(&meta_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(SessionError::NotFound {
                directory: directory.to_path_buf(),
            });
        }
        Err(error) => return Err(io_error(&meta_path, "read", &error)),
    };
    let meta_value: Value =
        serde_json::from_slice(&meta_bytes).map_err(|error| corrupt(SESSION_FILE, &error))?;
    let version = meta_value.get("schema_version").and_then(Value::as_u64);
    if version != Some(u64::from(SESSION_SCHEMA_VERSION)) {
        return Err(SessionError::UnsupportedVersion { found: version });
    }
    let meta: SessionMeta =
        serde_json::from_value(meta_value).map_err(|error| corrupt(SESSION_FILE, &error))?;

    let log_path = directory.join(PHASES_FILE);
    let log = match file_system.read(&log_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(io_error(&log_path, "read", &error)),
    };
    let mut replay = replay_log(&log);
    if meta.records_written > replay.last_seq {
        replay.anomalies.push(Anomaly::LogBehindSnapshot {
            snapshot_records: meta.records_written,
            log_records: replay.last_seq,
        });
    }

    let mut plan = None;
    if let Some(target) = &meta.target {
        match read_plan(file_system, directory, target) {
            Ok(found) => plan = Some(found),
            Err(reason) => replay.anomalies.push(Anomaly::UnreadablePlan { reason }),
        }
    }

    let decision = decide(&meta, &replay);
    Ok(LoadedSession {
        directory: directory.to_path_buf(),
        meta,
        phases: replay.table,
        plan,
        anomalies: replay.anomalies,
        decision,
        last_seq: replay.last_seq,
        log_unterminated: replay.unterminated,
    })
}

fn read_plan(
    file_system: &dyn FileSystem,
    directory: &Path,
    target: &Identity,
) -> Result<Plan, String> {
    let bytes = file_system
        .read(&directory.join(PLAN_FILE))
        .map_err(|error| format!("{PLAN_FILE}: {error}"))?;
    let plan: Plan =
        serde_json::from_slice(&bytes).map_err(|error| format!("{PLAN_FILE}: {error}"))?;
    if &plan.target.identity != target {
        return Err(format!(
            "{PLAN_FILE} is for a different disk than {SESSION_FILE} records"
        ));
    }
    Ok(plan)
}

impl LoadedSession {
    /// The session directory.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The contents of `session.json`.
    pub fn meta(&self) -> &SessionMeta {
        &self.meta
    }

    /// The phase table rebuilt from the log.
    pub fn phases(&self) -> &PhaseTable {
        &self.phases
    }

    /// The plan, when the session has one and it could be read.
    pub fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    /// Everything unexpected found while loading.
    pub fn anomalies(&self) -> &[Anomaly] {
        &self.anomalies
    }

    /// What may be done with the session.
    pub fn decision(&self) -> &ResumeDecision {
        &self.decision
    }

    /// Continues the session, when the decision is [`ResumeDecision::Resume`].
    ///
    /// A phase that was running when the installer stopped is recorded as
    /// failed with `UOS-INSTALL-SESSION-005`. The phase to continue from and
    /// every later phase return to pending, and the session forgets its plan
    /// and target: discovery, selection, planning and confirmation run again.
    ///
    /// Any other decision is an error. In particular a session that needs
    /// reconciliation returns [`SessionError::ReconciliationRequired`].
    pub fn resume<'a>(
        self,
        file_system: &'a dyn FileSystem,
        clock: &'a dyn Clock,
    ) -> Result<Session<'a>, SessionError> {
        let from_phase = match &self.decision {
            ResumeDecision::Resume { from_phase } => *from_phase,
            ResumeDecision::NeedsReconciliation { phase, .. } => {
                return Err(SessionError::ReconciliationRequired { phase: *phase });
            }
            ResumeDecision::Restart { reason } => {
                return Err(SessionError::NotResumable {
                    reason: reason.clone(),
                });
            }
            ResumeDecision::Completed => {
                return Err(SessionError::NotResumable {
                    reason: "the installation completed".to_owned(),
                });
            }
        };
        let mut meta = self.meta;
        meta.mode = None;
        meta.target = None;
        let mut session = Session {
            file_system,
            clock,
            directory: self.directory,
            meta,
            phases: self.phases,
            plan: None,
            next_seq: self.last_seq.saturating_add(1),
            log_needs_newline: self.log_unterminated,
            unusable: false,
        };
        if let Some(running) = session.phases.running() {
            let interrupted = Completion::failed(
                InstallCode::SessionInterrupted.into(),
                "The installer stopped while this phase was running",
            )
            .with_recovery("The phase will be run again");
            session.finish(running, interrupted)?;
        }
        session.reopen(from_phase)?;
        Ok(session)
    }

    /// Examines the target of a session that needs reconciliation.
    ///
    /// `current` must be an inventory taken now. The target's identity is
    /// revalidated against it; if the disk is missing, changed or ambiguous
    /// the result is an error and nothing more may be done to it.
    ///
    /// On success the result says what the disk looks like compared with the
    /// plan. It never permits continuing: every route to a phase that writes
    /// to the target needs a new typed confirmation.
    pub fn reconcile<'a>(
        &self,
        current: &'a Inventory,
    ) -> Result<Reconciliation<'a>, ReconcileError> {
        let ResumeDecision::NeedsReconciliation {
            phase,
            state,
            class,
        } = &self.decision
        else {
            return Err(ReconcileError::NotRequired);
        };
        let plan = self.plan.as_ref().ok_or(ReconcileError::NoPlan)?;
        let device =
            revalidate(&plan.target.identity, current).map_err(ReconcileError::Identity)?;
        let layout = if plan.existing_contents_unchanged(device) {
            ObservedLayout::Unchanged
        } else if plan.layout_present_on(device) {
            ObservedLayout::MatchesPlan
        } else {
            ObservedLayout::Unrecognised
        };
        Ok(Reconciliation {
            device,
            phase: *phase,
            state: *state,
            class: *class,
            layout,
        })
    }
}

/// What the target of an interrupted session looks like now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedLayout {
    /// The disk still holds exactly what the plan said would be destroyed.
    Unchanged,
    /// The disk carries the planned partition table.
    MatchesPlan,
    /// Neither: the disk is in an intermediate or unknown state.
    Unrecognised,
}

/// The result of examining an interrupted session's target.
#[derive(Debug)]
pub struct Reconciliation<'a> {
    /// The target disk in the fresh inventory.
    pub device: &'a BlockDevice,
    /// The last phase known to have written to the target.
    pub phase: PhaseId,
    /// That phase's last recorded state.
    pub state: PhaseState,
    /// That phase's repeat class.
    pub class: RepeatClass,
    /// What the disk looks like compared with the plan.
    pub layout: ObservedLayout,
}

impl Reconciliation<'_> {
    /// Always true: nothing more may be written to the target without a new
    /// typed confirmation and a new session. Continuing an interrupted
    /// installation in place is not implemented.
    pub fn fresh_confirmation_required(&self) -> bool {
        true
    }
}

/// Why a session's target could not be examined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileError {
    /// The session does not need reconciliation.
    NotRequired,
    /// The session's plan is missing or unreadable, so the target's identity
    /// is unknown.
    NoPlan,
    /// The target's identity does not match exactly one disk.
    Identity(IdentityMismatch),
}

impl fmt::Display for ReconcileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReconcileError::NotRequired => {
                f.write_str("This installer session does not need to be reconciled.")
            }
            ReconcileError::NoPlan => f.write_str(
                "The interrupted installation's plan cannot be read, so the installer \
                 cannot tell which disk it was working on. Do not continue it. Start the \
                 installation again and check the disk before confirming.",
            ),
            ReconcileError::Identity(mismatch) => fmt::Display::fmt(mismatch, f),
        }
    }
}

impl std::error::Error for ReconcileError {}

impl Fault for ReconcileError {
    fn code(&self) -> InstallCode {
        match self {
            ReconcileError::NotRequired => InstallCode::SessionNotResumable,
            ReconcileError::NoPlan => InstallCode::SessionNoPlan,
            ReconcileError::Identity(mismatch) => mismatch.code(),
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            ReconcileError::NotRequired => {
                "the session's decision is not needs_reconciliation".to_owned()
            }
            ReconcileError::NoPlan => format!("{PLAN_FILE} is missing or unreadable"),
            ReconcileError::Identity(mismatch) => mismatch.technical_detail(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{TargetSelector, WeakIdentityPolicy, select_target};
    use crate::plan::{ConfirmedPlan, ERASE_DISK_SCHEME, LayoutOptions, build_plan};
    use crate::preflight::{HostFacts, Requirements, TargetFacts, run_preflight};
    use crate::testing::{
        ManualClock, MemFs, NVME_SELECTOR, SATA_SELECTOR, erase_request, good_environment,
        workstation,
    };

    const DIR: &str = "/var/log/uos-installer/test-session";
    const ID: &str = "test-session";

    fn dir() -> &'static Path {
        Path::new(DIR)
    }

    fn confirmed_plan(inventory: &Inventory, selector: &str) -> ConfirmedPlan {
        let selector: TargetSelector = selector.parse().unwrap();
        let target = select_target(inventory, &selector, WeakIdentityPolicy::Refuse).unwrap();
        let options = LayoutOptions::default();
        let plan = build_plan(&target, &erase_request(options), &ERASE_DISK_SCHEME).unwrap();
        let facts = HostFacts {
            environment: good_environment(),
            target: Some(TargetFacts::of(target.device())),
        };
        let requirements = Requirements::for_layout(&ERASE_DISK_SCHEME, options).unwrap();
        let preflight = run_preflight(&facts, &requirements);
        plan.confirm(
            &plan.confirmation.required_phrase,
            &preflight,
            Timestamp::from_unix_millis(0),
        )
        .unwrap()
    }

    fn run_phase(session: &mut Session<'_>, phase: PhaseId) -> Result<(), SessionError> {
        session.begin(phase)?;
        session.finish(phase, Completion::succeeded("Done"))
    }

    const BEFORE_CONFIRMATION: [PhaseId; 5] = [
        PhaseId::EnvironmentCheck,
        PhaseId::HardwareInventory,
        PhaseId::StorageDiscovery,
        PhaseId::TargetSelection,
        PhaseId::PartitionPlan,
    ];
    const WRITING: [PhaseId; 7] = [
        PhaseId::Partitioning,
        PhaseId::EncryptionSetup,
        PhaseId::FilesystemCreation,
        PhaseId::ImageDeployment,
        PhaseId::BootloaderInstall,
        PhaseId::InitramfsConfiguration,
        PhaseId::FirstBootConfiguration,
    ];

    /// How far a scripted installation got before a write failed.
    #[derive(Debug, Default, Clone, Copy)]
    struct Reached {
        destructive_begun: bool,
        completed: bool,
    }

    /// Records a whole installation, stopping at `stop_before` if given.
    /// No phase does anything: only the bookkeeping is exercised.
    fn script(
        file_system: &MemFs,
        clock: &ManualClock,
        inventory: &Inventory,
        confirmed: &ConfirmedPlan,
        stop_before: Option<PhaseId>,
        reached: &mut Reached,
    ) -> Result<(), SessionError> {
        let stop = |phase: PhaseId| stop_before == Some(phase);
        let mut session = Session::create(file_system, clock, dir(), ID)?;
        session.save_inventory(inventory)?;
        for phase in BEFORE_CONFIRMATION {
            if stop(phase) {
                return Ok(());
            }
            run_phase(&mut session, phase)?;
        }
        session.save_plan(confirmed.plan())?;
        if stop(PhaseId::UserConfirmation) {
            return Ok(());
        }
        run_phase(&mut session, PhaseId::UserConfirmation)?;
        for phase in WRITING {
            if stop(phase) {
                return Ok(());
            }
            if phase.optional() {
                session.skip(phase, "Encryption was not chosen")?;
                continue;
            }
            // Each phase that writes needs its own fresh proof.
            let validated = confirmed
                .revalidate(inventory)
                .expect("the fixture revalidates");
            session.begin_destructive(phase, validated)?;
            reached.destructive_begun = true;
            session.finish(phase, Completion::succeeded("Done"))?;
        }
        for phase in [PhaseId::InstallValidation, PhaseId::Complete] {
            if stop(phase) {
                return Ok(());
            }
            run_phase(&mut session, phase)?;
        }
        reached.completed = true;
        Ok(())
    }

    /// A session recorded up to, but not including, `stop_before`.
    fn recorded(stop_before: Option<PhaseId>) -> (MemFs, Inventory, ConfirmedPlan) {
        let inventory = workstation();
        let confirmed = confirmed_plan(&inventory, SATA_SELECTOR);
        let file_system = MemFs::new();
        let mut reached = Reached::default();
        script(
            &file_system,
            &ManualClock::new(),
            &inventory,
            &confirmed,
            stop_before,
            &mut reached,
        )
        .unwrap();
        (file_system, inventory, confirmed)
    }

    /// Appends the record of `phase` starting, as a crash would leave it.
    fn leave_running(
        file_system: &MemFs,
        inventory: &Inventory,
        confirmed: &ConfirmedPlan,
        phase: PhaseId,
    ) {
        let clock = ManualClock::new();
        let loaded = load(file_system, dir()).unwrap();
        let mut session = Session {
            file_system,
            clock: &clock,
            directory: dir().to_path_buf(),
            meta: loaded.meta.clone(),
            phases: loaded.phases.clone(),
            plan: loaded.plan.clone(),
            next_seq: loaded.last_seq + 1,
            log_needs_newline: false,
            unusable: false,
        };
        if phase.destructive() {
            let validated = confirmed.revalidate(inventory).unwrap();
            session.begin_destructive(phase, validated).unwrap();
        } else {
            session.begin(phase).unwrap();
        }
    }

    #[test]
    fn creating_a_session_writes_its_files_and_records_start() {
        let file_system = MemFs::new();
        let clock = ManualClock::new();
        let session = Session::create(&file_system, &clock, dir(), ID).unwrap();
        assert_eq!(session.directory(), dir());
        assert_eq!(session.meta().session_id, ID);
        assert_eq!(session.meta().schema_version, SESSION_SCHEMA_VERSION);
        assert_eq!(session.meta().records_written, 2);
        assert!(!session.meta().target_writes_started);
        assert_eq!(
            session.phases().get(PhaseId::Start).state,
            PhaseState::Succeeded
        );
        assert!(session.plan().is_none());

        let log = file_system.file(&dir().join(PHASES_FILE)).unwrap();
        let text = String::from_utf8(log).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: TransitionRecord = serde_json::from_str(lines[0]).unwrap();
        let second: TransitionRecord = serde_json::from_str(lines[1]).unwrap();
        assert_eq!((first.seq, first.phase.state), (1, PhaseState::Running));
        assert_eq!((second.seq, second.phase.state), (2, PhaseState::Succeeded));
        assert!(text.ends_with('\n'));

        let meta: SessionMeta =
            serde_json::from_slice(&file_system.file(&dir().join(SESSION_FILE)).unwrap()).unwrap();
        assert_eq!(&meta, session.meta());
    }

    #[test]
    fn whole_files_are_replaced_atomically() {
        let file_system = MemFs::new();
        file_system.create_dir_all(dir()).unwrap();
        write_atomic(&file_system, &dir().join(SESSION_FILE), b"{}").unwrap();
        assert_eq!(
            file_system.operations(),
            [
                "create_dir_all test-session",
                "write_and_sync session.json.tmp",
                "rename session.json.tmp -> session.json",
                "sync_dir test-session"
            ]
        );
        assert_eq!(
            file_system.file(&dir().join(SESSION_FILE)).as_deref(),
            Some(b"{}".as_slice())
        );
        assert_eq!(file_system.file(&dir().join("session.json.tmp")), None);
        assert!(write_atomic(&file_system, Path::new("/"), b"x").is_err());
    }

    #[test]
    fn a_crash_inside_an_atomic_write_keeps_the_old_file() {
        // Fail at each of the three steps in turn.
        for failing_step in 0..3 {
            let file_system = MemFs::new();
            file_system.create_dir_all(dir()).unwrap();
            let path = dir().join(SESSION_FILE);
            write_atomic(&file_system, &path, b"old").unwrap();
            let done = file_system.operations().len();
            file_system.fail_from(done + failing_step, None);
            assert!(write_atomic(&file_system, &path, b"new").is_err());
            let survivor = file_system.survivors().file(&path).unwrap();
            if failing_step < 2 {
                assert_eq!(survivor, b"old", "step {failing_step}");
            } else {
                // The rename happened; only the directory flush failed.
                assert_eq!(survivor, b"new", "step {failing_step}");
            }
        }
    }

    #[test]
    fn session_ids_are_validated() {
        let file_system = MemFs::new();
        let clock = ManualClock::new();
        let too_long = "x".repeat(MAX_SESSION_ID_LENGTH + 1);
        for id in ["", ".hidden", "a/b", "a b", "../x", too_long.as_str()] {
            let error = Session::create(&file_system, &clock, dir(), id).unwrap_err();
            assert_eq!(error.code(), InstallCode::SessionInvalidId, "{id}");
        }
        assert!(file_system.operations().is_empty());
        Session::create(&file_system, &clock, dir(), "2026-11-12T06.17_a").unwrap();
    }

    #[test]
    fn a_directory_holds_one_session() {
        let file_system = MemFs::new();
        let clock = ManualClock::new();
        Session::create(&file_system, &clock, dir(), ID).unwrap();
        let error = Session::create(&file_system, &clock, dir(), ID).unwrap_err();
        assert_eq!(error.code(), InstallCode::SessionAlreadyExists);
    }

    #[test]
    fn loading_reports_missing_corrupt_and_foreign_sessions() {
        let file_system = MemFs::new();
        let missing = load(&file_system, dir()).unwrap_err();
        assert_eq!(missing.code(), InstallCode::SessionNotFound);

        file_system.create_dir_all(dir()).unwrap();
        let path = dir().join(SESSION_FILE);
        for (contents, code) in [
            ("", InstallCode::SessionCorrupt),
            ("{", InstallCode::SessionCorrupt),
            ("[]", InstallCode::SessionUnsupportedVersion),
            ("{}", InstallCode::SessionUnsupportedVersion),
            (
                r#"{"schema_version": 2}"#,
                InstallCode::SessionUnsupportedVersion,
            ),
            (
                r#"{"schema_version": "1"}"#,
                InstallCode::SessionUnsupportedVersion,
            ),
            (r#"{"schema_version": 1}"#, InstallCode::SessionCorrupt),
        ] {
            file_system.set_file(&path, contents.as_bytes());
            let error = load(&file_system, dir()).unwrap_err();
            assert_eq!(error.code(), code, "{contents}");
            assert!(!error.technical_detail().is_empty());
        }
    }

    #[test]
    fn a_complete_session_loads_as_completed() {
        let (file_system, _, confirmed) = recorded(None);
        let loaded = load(&file_system, dir()).unwrap();
        assert_eq!(loaded.decision(), &ResumeDecision::Completed);
        assert!(loaded.anomalies().is_empty());
        assert!(loaded.phases().is_complete());
        assert!(loaded.meta().target_writes_started);
        assert_eq!(loaded.plan(), Some(confirmed.plan()));
        assert_eq!(loaded.meta().phases, *loaded.phases());
        assert_eq!(loaded.directory(), dir());

        let clock = ManualClock::new();
        let error = loaded.resume(&file_system, &clock).unwrap_err();
        assert_eq!(error.code(), InstallCode::SessionNotResumable);
    }

    #[test]
    fn a_session_stopped_before_any_write_resumes_from_discovery_at_the_latest() {
        for (stop_before, expected) in [
            (PhaseId::EnvironmentCheck, PhaseId::EnvironmentCheck),
            (PhaseId::HardwareInventory, PhaseId::HardwareInventory),
            (PhaseId::StorageDiscovery, PhaseId::StorageDiscovery),
            (PhaseId::TargetSelection, PhaseId::StorageDiscovery),
            (PhaseId::PartitionPlan, PhaseId::StorageDiscovery),
            (PhaseId::UserConfirmation, PhaseId::StorageDiscovery),
            (PhaseId::Partitioning, PhaseId::StorageDiscovery),
        ] {
            let (file_system, _, _) = recorded(Some(stop_before));
            let loaded = load(&file_system, dir()).unwrap();
            assert_eq!(
                loaded.decision(),
                &ResumeDecision::Resume {
                    from_phase: expected
                },
                "stopped before {stop_before}"
            );
            assert!(!loaded.meta().target_writes_started);
        }
    }

    #[test]
    fn resuming_forgets_the_plan_and_reopens_from_discovery() {
        // Confirmation was given, then the installer stopped before writing.
        let (file_system, inventory, confirmed) = recorded(Some(PhaseId::Partitioning));
        let loaded = load(&file_system, dir()).unwrap();
        assert!(loaded.plan().is_some());
        let clock = ManualClock::new();
        let mut session = loaded.resume(&file_system, &clock).unwrap();

        assert_eq!(session.meta().target, None);
        assert_eq!(session.meta().mode, None);
        assert!(session.plan().is_none());
        for phase in [
            PhaseId::StorageDiscovery,
            PhaseId::TargetSelection,
            PhaseId::PartitionPlan,
            PhaseId::UserConfirmation,
        ] {
            assert_eq!(session.phases().get(phase).state, PhaseState::Pending);
        }
        assert_eq!(
            session.phases().get(PhaseId::HardwareInventory).state,
            PhaseState::Succeeded
        );

        // The recorded confirmation is not honoured: there is no plan, so no
        // phase that writes to the target can start.
        let validated = confirmed.revalidate(&inventory).unwrap();
        assert_eq!(
            session
                .begin_destructive(PhaseId::Partitioning, validated)
                .unwrap_err(),
            SessionError::NoPlan
        );

        run_phase(&mut session, PhaseId::StorageDiscovery).unwrap();
        assert_eq!(session.phases().get(PhaseId::StorageDiscovery).attempt, 2);
        // The resumed log is still a valid log.
        let again = load(&file_system, dir()).unwrap();
        assert!(again.anomalies().is_empty(), "{:?}", again.anomalies());
        assert_eq!(
            again.decision(),
            &ResumeDecision::Resume {
                from_phase: PhaseId::StorageDiscovery
            }
        );
    }

    #[test]
    fn a_phase_interrupted_before_any_write_is_marked_failed_and_run_again() {
        let (file_system, inventory, confirmed) = recorded(Some(PhaseId::TargetSelection));
        leave_running(
            &file_system,
            &inventory,
            &confirmed,
            PhaseId::TargetSelection,
        );
        let loaded = load(&file_system, dir()).unwrap();
        assert_eq!(
            loaded.phases().get(PhaseId::TargetSelection).state,
            PhaseState::Running
        );
        assert_eq!(
            loaded.decision(),
            &ResumeDecision::Resume {
                from_phase: PhaseId::StorageDiscovery
            }
        );
        let clock = ManualClock::new();
        let session = loaded.resume(&file_system, &clock).unwrap();
        assert_eq!(
            session.phases().get(PhaseId::TargetSelection).state,
            PhaseState::Pending
        );

        let log = String::from_utf8(file_system.file(&dir().join(PHASES_FILE)).unwrap()).unwrap();
        let interrupted = log
            .lines()
            .map(|line| serde_json::from_str::<TransitionRecord>(line).unwrap())
            .find(|entry| entry.phase.state == PhaseState::Failed)
            .expect("the interruption is recorded");
        assert_eq!(interrupted.phase.phase_id, PhaseId::TargetSelection);
        assert_eq!(
            interrupted
                .phase
                .error_code
                .as_ref()
                .map(|code| code.as_str()),
            Some("UOS-INSTALL-SESSION-005")
        );
    }

    #[test]
    fn a_running_destructive_phase_needs_reconciliation_and_never_resumes() {
        for phase in WRITING {
            if phase.optional() {
                continue;
            }
            let (file_system, inventory, confirmed) = recorded(Some(phase));
            leave_running(&file_system, &inventory, &confirmed, phase);
            let loaded = load(&file_system, dir()).unwrap();
            assert_eq!(
                loaded.decision(),
                &ResumeDecision::NeedsReconciliation {
                    phase,
                    state: PhaseState::Running,
                    class: phase.class()
                },
                "{phase}"
            );
            assert!(loaded.decision().summary().contains("Do not resume"));

            // Reconciliation needs a fresh inventory and revalidates identity.
            let fresh = workstation();
            let reconciliation = loaded.reconcile(&fresh).unwrap();
            assert_eq!(reconciliation.device.name, "sda");
            assert_eq!(reconciliation.phase, phase);
            assert_eq!(reconciliation.state, PhaseState::Running);
            assert_eq!(reconciliation.class, phase.class());
            assert_eq!(reconciliation.layout, ObservedLayout::Unchanged);
            assert!(reconciliation.fresh_confirmation_required());

            let clock = ManualClock::new();
            let error = loaded.resume(&file_system, &clock).unwrap_err();
            assert_eq!(error, SessionError::ReconciliationRequired { phase });
            assert_eq!(error.code(), InstallCode::SessionReconciliationRequired);
        }
    }

    #[test]
    fn a_finished_destructive_phase_still_needs_reconciliation() {
        // The installer stopped between two phases, after partitioning.
        let (file_system, _, _) = recorded(Some(PhaseId::EncryptionSetup));
        let loaded = load(&file_system, dir()).unwrap();
        assert_eq!(
            loaded.decision(),
            &ResumeDecision::NeedsReconciliation {
                phase: PhaseId::Partitioning,
                state: PhaseState::Succeeded,
                class: RepeatClass::NonRepeatable
            }
        );
        // And so does a session that only has validation left.
        let (file_system, _, _) = recorded(Some(PhaseId::InstallValidation));
        let loaded = load(&file_system, dir()).unwrap();
        assert!(matches!(
            loaded.decision(),
            ResumeDecision::NeedsReconciliation {
                phase: PhaseId::FirstBootConfiguration,
                ..
            }
        ));
    }

    #[test]
    fn reconciliation_stops_when_the_disk_is_not_the_same() {
        let (file_system, inventory, confirmed) = recorded(Some(PhaseId::Partitioning));
        leave_running(&file_system, &inventory, &confirmed, PhaseId::Partitioning);
        let loaded = load(&file_system, dir()).unwrap();

        let mut gone = workstation();
        gone.devices.retain(|device| device.name != "sda");
        let error = loaded.reconcile(&gone).unwrap_err();
        assert_eq!(error, ReconcileError::Identity(IdentityMismatch::Missing));
        assert_eq!(error.code(), InstallCode::IdentityMissing);

        let mut swapped = workstation();
        for device in &mut swapped.devices {
            if device.name == "sda" {
                device.serial = Some("S4EWNX0M799999Z".to_owned());
            }
        }
        assert_eq!(
            loaded.reconcile(&swapped).unwrap_err().code(),
            InstallCode::IdentitySerialChanged
        );

        let mut twin = workstation();
        let mut copy = twin.devices[1].clone();
        copy.path = "/dev/sdz".to_owned();
        twin.devices.push(copy);
        assert_eq!(
            loaded.reconcile(&twin).unwrap_err().code(),
            InstallCode::IdentityAmbiguous
        );
    }

    #[test]
    fn reconciliation_reports_what_the_disk_looks_like() {
        let (file_system, inventory, confirmed) = recorded(Some(PhaseId::Partitioning));
        leave_running(&file_system, &inventory, &confirmed, PhaseId::Partitioning);
        let loaded = load(&file_system, dir()).unwrap();

        // The planned table is on the disk.
        let mut partitioned = workstation();
        {
            let sda = &mut partitioned.devices[1];
            assert_eq!(sda.name, "sda");
            let template = sda.children[0].clone();
            sda.children = confirmed
                .plan()
                .partitions
                .iter()
                .map(|partition| {
                    let mut child = template.clone();
                    child.size_bytes = partition.size_bytes;
                    child.filesystem = None;
                    child
                })
                .collect();
        }
        assert_eq!(
            loaded.reconcile(&partitioned).unwrap().layout,
            ObservedLayout::MatchesPlan
        );

        // Something in between.
        let mut partial = workstation();
        partial.devices[1].children.truncate(1);
        assert_eq!(
            loaded.reconcile(&partial).unwrap().layout,
            ObservedLayout::Unrecognised
        );
    }

    #[test]
    fn reconciliation_is_refused_when_not_needed_or_when_the_plan_is_lost() {
        let (file_system, _, _) = recorded(Some(PhaseId::TargetSelection));
        let loaded = load(&file_system, dir()).unwrap();
        let fresh = workstation();
        assert_eq!(
            loaded.reconcile(&fresh).unwrap_err(),
            ReconcileError::NotRequired
        );

        let (file_system, inventory, confirmed) = recorded(Some(PhaseId::Partitioning));
        leave_running(&file_system, &inventory, &confirmed, PhaseId::Partitioning);
        file_system.set_file(&dir().join(PLAN_FILE), b"{ not json");
        let loaded = load(&file_system, dir()).unwrap();
        assert!(matches!(
            loaded.anomalies(),
            [Anomaly::UnreadablePlan { .. }]
        ));
        assert!(matches!(
            loaded.decision(),
            ResumeDecision::NeedsReconciliation { .. }
        ));
        let error = loaded.reconcile(&fresh).unwrap_err();
        assert_eq!(error, ReconcileError::NoPlan);
        assert_eq!(error.code(), InstallCode::SessionNoPlan);
    }

    #[test]
    fn destructive_phases_need_a_validated_target_from_this_sessions_plan() {
        let inventory = workstation();
        let confirmed = confirmed_plan(&inventory, SATA_SELECTOR);
        let other = confirmed_plan(&inventory, NVME_SELECTOR);
        let file_system = MemFs::new();
        let clock = ManualClock::new();
        let mut session = Session::create(&file_system, &clock, dir(), ID).unwrap();
        for phase in BEFORE_CONFIRMATION {
            run_phase(&mut session, phase).unwrap();
        }
        run_phase(&mut session, PhaseId::UserConfirmation).unwrap();

        // Without the proof, every destructive phase is refused.
        for phase in WRITING {
            let error = session.begin(phase).unwrap_err();
            assert_eq!(error, SessionError::ValidatedTargetRequired { phase });
            assert_eq!(error.code(), InstallCode::SessionTargetNotValidated);
        }

        // With a proof but no saved plan. A proof is used up by each attempt.
        let proof = || confirmed.revalidate(&inventory).unwrap();
        assert_eq!(
            session
                .begin_destructive(PhaseId::Partitioning, proof())
                .unwrap_err(),
            SessionError::NoPlan
        );

        // With a proof for another plan.
        session.save_plan(confirmed.plan()).unwrap();
        let wrong = other.revalidate(&inventory).unwrap();
        let error = session
            .begin_destructive(PhaseId::Partitioning, wrong)
            .unwrap_err();
        assert_eq!(error, SessionError::TargetNotInPlan);
        assert_eq!(error.code(), InstallCode::SessionTargetNotValidated);

        // An illegal transition does not mark the target as written.
        let error = session
            .begin_destructive(PhaseId::FilesystemCreation, proof())
            .unwrap_err();
        assert_eq!(error.code(), InstallCode::PhaseOutOfOrder);
        assert!(!session.meta().target_writes_started);
        assert!(
            !load(&file_system, dir())
                .unwrap()
                .meta()
                .target_writes_started
        );

        // With the right proof the phase starts, and the flag is durable.
        session
            .begin_destructive(PhaseId::Partitioning, proof())
            .unwrap();
        assert!(session.meta().target_writes_started);
        assert!(
            load(&file_system, dir())
                .unwrap()
                .meta()
                .target_writes_started
        );

        // From here the plan is locked and nothing can be reopened.
        assert_eq!(
            session.save_plan(other.plan()).unwrap_err(),
            SessionError::PlanLocked
        );
        assert_eq!(
            session.reopen(PhaseId::TargetSelection).unwrap_err().code(),
            InstallCode::PhaseReopenRefused
        );
    }

    #[test]
    fn the_write_intent_is_durable_before_the_first_destructive_record() {
        let inventory = workstation();
        let confirmed = confirmed_plan(&inventory, SATA_SELECTOR);
        let file_system = MemFs::new();
        let clock = ManualClock::new();
        let mut reached = Reached::default();
        script(
            &file_system,
            &clock,
            &inventory,
            &confirmed,
            Some(PhaseId::Partitioning),
            &mut reached,
        )
        .unwrap();
        let before = file_system.operations().len();
        leave_running(&file_system, &inventory, &confirmed, PhaseId::Partitioning);
        let operations = file_system.operations();
        assert_eq!(
            &operations[before..before + 4],
            [
                "write_and_sync session.json.tmp",
                "rename session.json.tmp -> session.json",
                "sync_dir test-session",
                "append phases.jsonl"
            ]
        );
    }

    #[test]
    fn a_failed_write_makes_the_session_unusable() {
        let file_system = MemFs::new();
        let clock = ManualClock::new();
        let mut session = Session::create(&file_system, &clock, dir(), ID).unwrap();
        file_system.fail_from(file_system.operations().len(), None);
        let error = session.begin(PhaseId::EnvironmentCheck).unwrap_err();
        assert_eq!(error.code(), InstallCode::SessionIo);
        // The in-memory state did not advance.
        assert_eq!(
            session.phases().get(PhaseId::EnvironmentCheck).state,
            PhaseState::Pending
        );
        for error in [
            session.begin(PhaseId::EnvironmentCheck).unwrap_err(),
            session
                .finish(PhaseId::EnvironmentCheck, Completion::succeeded("x"))
                .unwrap_err(),
            session.reopen(PhaseId::Start).unwrap_err(),
            session.save_inventory(&workstation()).unwrap_err(),
            session
                .set_progress(
                    PhaseId::EnvironmentCheck,
                    Progress::Indeterminate {
                        activity: "x".to_owned(),
                    },
                )
                .unwrap_err(),
        ] {
            assert_eq!(error, SessionError::Unusable);
        }
        assert!(format!("{session:?}").contains("unusable: true"));
    }

    #[test]
    fn an_illegal_transition_is_reported_and_leaves_the_session_usable() {
        let file_system = MemFs::new();
        let clock = ManualClock::new();
        let mut session = Session::create(&file_system, &clock, dir(), ID).unwrap();
        let before = file_system.operations().len();
        let error = session.begin(PhaseId::StorageDiscovery).unwrap_err();
        assert_eq!(error.code(), InstallCode::PhaseOutOfOrder);
        assert_eq!(
            session
                .skip(PhaseId::EnvironmentCheck, "x")
                .unwrap_err()
                .code(),
            InstallCode::PhaseNotOptional
        );
        assert_eq!(
            file_system.operations().len(),
            before,
            "nothing was written"
        );
        run_phase(&mut session, PhaseId::EnvironmentCheck).unwrap();
    }

    #[test]
    fn going_back_is_recorded_in_the_log() {
        let file_system = MemFs::new();
        let clock = ManualClock::new();
        let mut session = Session::create(&file_system, &clock, dir(), ID).unwrap();
        for phase in BEFORE_CONFIRMATION {
            run_phase(&mut session, phase).unwrap();
        }
        let before = session.meta().records_written;
        session.reopen(PhaseId::TargetSelection).unwrap();
        assert_eq!(session.meta().records_written, before + 2);
        session.begin(PhaseId::TargetSelection).unwrap();
        session
            .set_progress(
                PhaseId::TargetSelection,
                Progress::Indeterminate {
                    activity: "Waiting for a choice".to_owned(),
                },
            )
            .unwrap();
        session
            .finish(PhaseId::TargetSelection, Completion::succeeded("Chosen"))
            .unwrap();

        let loaded = load(&file_system, dir()).unwrap();
        assert!(loaded.anomalies().is_empty());
        assert_eq!(loaded.phases(), session.phases());
        let record = loaded.phases().get(PhaseId::TargetSelection);
        assert_eq!(record.attempt, 2);
        assert!(record.progress.is_some());
        assert_eq!(
            loaded.phases().get(PhaseId::PartitionPlan).state,
            PhaseState::Pending
        );
    }

    #[test]
    fn a_crash_at_any_write_leaves_a_loadable_session_that_never_resumes_past_a_write() {
        let inventory = workstation();
        let confirmed = confirmed_plan(&inventory, SATA_SELECTOR);
        let clean = MemFs::new();
        let mut reached = Reached::default();
        script(
            &clean,
            &ManualClock::new(),
            &inventory,
            &confirmed,
            None,
            &mut reached,
        )
        .unwrap();
        assert!(reached.completed);
        let total = clean.operations().len();
        assert!(total > 60, "the script performs {total} operations");

        let mut resumable = 0;
        let mut reconciled = 0;
        for crash_at in 0..total {
            // A crash can also tear an append: none, part or all of its
            // bytes reach the disk, and the append is never acknowledged.
            for torn in [None, Some(1), Some(60), Some(usize::MAX)] {
                let file_system = MemFs::new();
                file_system.fail_from(crash_at, torn);
                let mut reached = Reached::default();
                let result = script(
                    &file_system,
                    &ManualClock::new(),
                    &inventory,
                    &confirmed,
                    None,
                    &mut reached,
                );
                assert!(result.is_err(), "crash at {crash_at} was not noticed");
                assert!(!reached.completed);

                let after = file_system.survivors();
                let loaded = match load(&after, dir()) {
                    Ok(loaded) => loaded,
                    Err(SessionError::NotFound { .. }) => {
                        assert!(crash_at < 4, "session.json vanished at {crash_at}");
                        continue;
                    }
                    Err(error) => panic!("crash at {crash_at} ({torn:?}): {error:?}"),
                };
                let decision = loaded.decision().clone();
                let context = format!("crash at {crash_at} ({torn:?}): {decision:?}");
                match &decision {
                    ResumeDecision::Resume { from_phase } => {
                        assert!(!reached.destructive_begun, "{context}");
                        assert!(!from_phase.destructive(), "{context}");
                        assert!(*from_phase <= PhaseId::StorageDiscovery, "{context}");
                        assert_eq!(loaded.phases().first_target_write(), None, "{context}");
                        let clock = ManualClock::new();
                        let mut session = loaded.resume(&after, &clock).unwrap();
                        assert_eq!(session.phases().first_target_write(), None);
                        if *from_phase != PhaseId::Start {
                            run_phase(&mut session, *from_phase).unwrap();
                        }
                        let again = load(&after, dir()).unwrap();
                        assert!(
                            matches!(again.decision(), ResumeDecision::Resume { .. }),
                            "{context}"
                        );
                        resumable += 1;
                    }
                    ResumeDecision::NeedsReconciliation { phase, .. } => {
                        assert!(phase.destructive(), "{context}");
                        let fresh = workstation();
                        assert!(loaded.reconcile(&fresh).is_ok(), "{context}");
                        let clock = ManualClock::new();
                        assert!(loaded.resume(&after, &clock).is_err(), "{context}");
                        reconciled += 1;
                    }
                    ResumeDecision::Completed => {
                        // Only the final snapshot write was lost.
                        assert!(loaded.phases().is_complete(), "{context}");
                        assert!(crash_at + 4 >= total, "{context}");
                    }
                    ResumeDecision::Restart { .. } => {
                        panic!("a crash never makes the log inconsistent: {context}")
                    }
                }
                // Once the start of a destructive phase was acknowledged,
                // the session must never be reported as resumable.
                if reached.destructive_begun {
                    assert!(
                        matches!(
                            loaded_decision_kind(&after),
                            DecisionKind::NeedsReconciliation | DecisionKind::Completed
                        ),
                        "{context}"
                    );
                }
            }
        }
        assert!(resumable > 0 && reconciled > 0);
    }

    #[derive(Debug, PartialEq, Eq)]
    enum DecisionKind {
        Resume,
        Restart,
        NeedsReconciliation,
        Completed,
    }

    fn kind_of(decision: &ResumeDecision) -> DecisionKind {
        match decision {
            ResumeDecision::Resume { .. } => DecisionKind::Resume,
            ResumeDecision::Restart { .. } => DecisionKind::Restart,
            ResumeDecision::NeedsReconciliation { .. } => DecisionKind::NeedsReconciliation,
            ResumeDecision::Completed => DecisionKind::Completed,
        }
    }

    fn loaded_decision_kind(file_system: &MemFs) -> DecisionKind {
        kind_of(load(file_system, dir()).unwrap().decision())
    }

    /// Byte offsets just after each line terminator in `log`.
    fn record_boundaries(log: &[u8]) -> Vec<usize> {
        log.iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\n')
            .map(|(index, _)| index + 1)
            .collect()
    }

    #[test]
    fn truncating_the_log_at_every_byte_never_panics_and_never_resumes_past_a_write() {
        let (complete, _, _) = recorded(None);
        let log = complete.file(&dir().join(PHASES_FILE)).unwrap();
        let plan = complete.file(&dir().join(PLAN_FILE)).unwrap();
        let final_meta = complete.file(&dir().join(SESSION_FILE)).unwrap();
        // The snapshot as it was when the session had only just been planned:
        // it carries no hint that anything was written.
        let (planned, _, _) = recorded(Some(PhaseId::UserConfirmation));
        let early_meta = planned.file(&dir().join(SESSION_FILE)).unwrap();
        let early_records = load(&planned, dir()).unwrap().meta().records_written;

        let boundaries = record_boundaries(&log);
        assert_eq!(boundaries.last(), Some(&log.len()));
        // The first record of a destructive phase, and the byte at which it
        // becomes readable (its text is complete one byte before its newline).
        let first_write_line = String::from_utf8(log.clone())
            .unwrap()
            .lines()
            .position(|line| line.contains("\"phase_id\":\"PARTITIONING\""))
            .unwrap();
        let first_write_visible = boundaries[first_write_line] - 1;
        let complete_from = log.len() - 1;

        let check = |meta: &[u8], cut: usize, snapshot_has_evidence: bool| {
            let file_system = MemFs::new();
            file_system.create_dir_all(dir()).unwrap();
            file_system.set_file(&dir().join(SESSION_FILE), meta);
            file_system.set_file(&dir().join(PLAN_FILE), &plan);
            file_system.set_file(&dir().join(PHASES_FILE), &log[..cut]);
            let loaded =
                load(&file_system, dir()).unwrap_or_else(|error| panic!("cut at {cut}: {error:?}"));
            let kind = kind_of(loaded.decision());
            let context = format!("cut at {cut}: {:?}", loaded.decision());

            let write_visible = cut >= first_write_visible;
            if write_visible || snapshot_has_evidence {
                // The core rule: evidence of a write never yields "resume".
                if cut >= complete_from {
                    assert_eq!(kind, DecisionKind::Completed, "{context}");
                } else {
                    assert_eq!(kind, DecisionKind::NeedsReconciliation, "{context}");
                    let fresh = workstation();
                    assert!(loaded.reconcile(&fresh).is_ok(), "{context}");
                }
            } else {
                assert_ne!(kind, DecisionKind::NeedsReconciliation, "{context}");
                assert_ne!(kind, DecisionKind::Completed, "{context}");
            }
            if let ResumeDecision::Resume { from_phase } = loaded.decision() {
                assert!(!from_phase.destructive(), "{context}");
                assert!(*from_phase <= PhaseId::StorageDiscovery, "{context}");
                assert!(!write_visible, "{context}");
            }
            let clock = ManualClock::new();
            let resumed = loaded.resume(&file_system, &clock);
            assert_eq!(resumed.is_ok(), kind == DecisionKind::Resume, "{context}");
            kind
        };

        // Every byte offset, against the early snapshot: only the log can
        // reveal the write.
        let mut seen_restart = false;
        let mut seen_resume = false;
        for cut in 0..=log.len() {
            let kind = check(&early_meta, cut, false);
            let records_visible = boundaries.iter().filter(|end| **end <= cut + 1).count();
            if (records_visible as u64) < early_records {
                // The log is shorter than the snapshot says: not trusted.
                assert_eq!(kind, DecisionKind::Restart, "cut at {cut}");
                seen_restart = true;
            } else if cut < first_write_visible {
                assert_eq!(kind, DecisionKind::Resume, "cut at {cut}");
                seen_resume = true;
            }
        }
        assert!(seen_restart && seen_resume);

        // Every record boundary and the bytes around it, plus a stride
        // through the rest, against the final snapshot: the snapshot alone
        // forbids resuming.
        let mut cuts: Vec<usize> = (0..=log.len()).step_by(23).collect();
        for boundary in &boundaries {
            cuts.extend([boundary - 1, *boundary, (boundary + 1).min(log.len())]);
        }
        cuts.push(0);
        for cut in cuts {
            check(&final_meta, cut, true);
        }
    }

    #[test]
    fn a_torn_final_line_is_tolerated_and_later_appends_stay_readable() {
        let (file_system, _, _) = recorded(Some(PhaseId::TargetSelection));
        let log_path = dir().join(PHASES_FILE);
        let mut log = file_system.file(&log_path).unwrap();
        log.extend_from_slice(b"{\"seq\":9,\"recorded_at\":\"2026-11-1");
        file_system.set_file(&log_path, &log);

        let loaded = load(&file_system, dir()).unwrap();
        assert_eq!(
            loaded.anomalies(),
            [Anomaly::UnreadableRecord {
                line: 9,
                at_end: true
            }]
        );
        assert_eq!(
            loaded.decision(),
            &ResumeDecision::Resume {
                from_phase: PhaseId::StorageDiscovery
            }
        );
        let clock = ManualClock::new();
        let mut session = loaded.resume(&file_system, &clock).unwrap();
        run_phase(&mut session, PhaseId::StorageDiscovery).unwrap();

        let again = load(&file_system, dir()).unwrap();
        assert_eq!(
            again.anomalies(),
            [Anomaly::UnreadableRecord {
                line: 9,
                at_end: false
            }]
        );
        assert_eq!(again.phases(), session.phases());
        assert!(matches!(again.decision(), ResumeDecision::Resume { .. }));
    }

    #[test]
    fn missing_or_reordered_records_are_not_trusted() {
        let (file_system, _, _) = recorded(Some(PhaseId::PartitionPlan));
        let log_path = dir().join(PHASES_FILE);
        let log = String::from_utf8(file_system.file(&log_path).unwrap()).unwrap();
        let lines: Vec<&str> = log.lines().collect();

        // A record in the middle is gone.
        let mut holed = lines.clone();
        holed.remove(3);
        file_system.set_file(&log_path, format!("{}\n", holed.join("\n")).as_bytes());
        let loaded = load(&file_system, dir()).unwrap();
        assert!(loaded.anomalies().contains(&Anomaly::SequenceGap {
            line: 4,
            expected: 4,
            found: 5
        }));
        assert!(matches!(loaded.decision(), ResumeDecision::Restart { .. }));
        let clock = ManualClock::new();
        assert_eq!(
            loaded.resume(&file_system, &clock).unwrap_err().code(),
            InstallCode::SessionNotResumable
        );

        // The end of the log is gone, but the snapshot remembers it.
        let shortened = format!("{}\n", lines[..4].join("\n"));
        file_system.set_file(&log_path, shortened.as_bytes());
        let loaded = load(&file_system, dir()).unwrap();
        assert!(matches!(
            loaded.anomalies(),
            [Anomaly::LogBehindSnapshot { log_records: 4, .. }]
        ));
        assert!(matches!(loaded.decision(), ResumeDecision::Restart { .. }));
    }

    #[test]
    fn hostile_log_contents_never_panic() {
        let (file_system, _, _) = recorded(Some(PhaseId::TargetSelection));
        let log_path = dir().join(PHASES_FILE);
        let good = file_system.file(&log_path).unwrap();
        let first_line = good.split(|byte| *byte == b'\n').next().unwrap().to_vec();
        let huge_seq = String::from_utf8(first_line.clone())
            .unwrap()
            .replace("\"seq\":1,", "\"seq\":18446744073709551615,");
        let writes = String::from_utf8(first_line).unwrap().replace(
            "\"phase_id\":\"START\"",
            "\"phase_id\":\"FILESYSTEM_CREATION\"",
        );
        for contents in [
            b"\n\n\n".to_vec(),
            b"\xff\xfe\x00garbage".to_vec(),
            b"null\n[]\n{}\n42\n\"text\"\n".to_vec(),
            vec![b'{'; 4096],
            format!("{huge_seq}\n{huge_seq}\n").into_bytes(),
            format!("{writes}\n").into_bytes(),
        ] {
            file_system.set_file(&log_path, &contents);
            let loaded = load(&file_system, dir()).unwrap();
            assert!(!matches!(
                loaded.decision(),
                ResumeDecision::Resume { from_phase } if from_phase.destructive()
            ));
        }
        // The last variant holds a record of a destructive phase: that alone
        // is evidence of a write.
        let loaded = load(&file_system, dir()).unwrap();
        assert!(matches!(
            loaded.decision(),
            ResumeDecision::NeedsReconciliation {
                phase: PhaseId::FilesystemCreation,
                ..
            }
        ));
    }

    #[test]
    fn decisions_and_anomalies_serialise_for_the_command_line() {
        let decision = ResumeDecision::NeedsReconciliation {
            phase: PhaseId::Partitioning,
            state: PhaseState::Running,
            class: RepeatClass::NonRepeatable,
        };
        assert_eq!(
            serde_json::to_string(&decision).unwrap(),
            r#"{"decision":"needs_reconciliation","phase":"PARTITIONING","state":"running","class":"non_repeatable"}"#
        );
        assert_eq!(
            serde_json::to_string(&ResumeDecision::Completed).unwrap(),
            r#"{"decision":"completed"}"#
        );
        let anomaly = Anomaly::UnreadableRecord {
            line: 3,
            at_end: true,
        };
        assert_eq!(
            serde_json::to_string(&anomaly).unwrap(),
            r#"{"kind":"unreadable_record","line":3,"at_end":true}"#
        );
        assert_eq!(
            serde_json::to_string(&ObservedLayout::MatchesPlan).unwrap(),
            "\"matches_plan\""
        );
        for decision in [
            ResumeDecision::Resume {
                from_phase: PhaseId::Start,
            },
            ResumeDecision::Restart {
                reason: "x".to_owned(),
            },
            ResumeDecision::Completed,
        ] {
            assert!(!decision.summary().is_empty());
        }
    }
}
