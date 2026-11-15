// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What self-healing knows between runs: the faults it has seen, what was
//! done about each, and the events a person is told about.
//!
//! The state is one JSON file (self-healing.md, section 6), by default
//! `/var/log/tur/heal/state.json`. It is a record of what happened, it holds
//! nothing of the person's (no file content, no address, no name), and it
//! may be read by anyone on the computer: P@x's read-only sandbox shows
//! `/var/log`, and the person's own notices are made from it. Copies of
//! replaced files, and the lock that keeps two runs apart, are in the data
//! directory, by default `/var/lib/tur/heal`, readable by root only.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::remedies::UndoData;
use crate::error::{DiagError, ErrorKind};

/// The state format this build writes.
pub const STATE_SCHEMA_VERSION: u32 = 1;

/// Where the state is kept by default.
pub const DEFAULT_STATE_FILE: &str = "/var/log/tur/heal/state.json";

/// Where copies and the lock are kept by default.
pub const DEFAULT_DATA_DIR: &str = "/var/lib/tur/heal";

/// How many events are kept.
pub const MAX_EVENTS: usize = 200;

/// How many resolved faults are kept.
pub const MAX_RESOLVED: usize = 50;

/// The whole state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HealState {
    /// [`STATE_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// When it was last written, UTC.
    pub updated_utc: String,
    /// The TUR build that last wrote it.
    pub build_id: Option<String>,
    /// The TUR version that last wrote it.
    pub version: Option<String>,
    /// The boot it was last written in.
    pub boot_id: Option<String>,
    /// How many systemd units were in the failed state at the last scan,
    /// counted as root. `tur-diag status` shows it where `systemctl` cannot
    /// be asked, as in P@x's sandbox (issue #57).
    #[serde(default)]
    pub failed_units: Option<u64>,
    /// When that count was made, UTC.
    #[serde(default)]
    pub failed_units_utc: Option<String>,
    /// Every fault seen, newest last.
    pub faults: Vec<FaultRecord>,
    /// What happened, newest last.
    pub events: Vec<Event>,
    /// The number the next event gets.
    pub next_event: u64,
}

/// Where a fault stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FaultState {
    /// Found; a remedy is waiting for the person, or none was tried yet.
    Present,
    /// A remedy was applied and the check passed afterwards. It is
    /// resolved once a later check still passes.
    Repaired,
    /// A remedy was applied and the check still finds the fault, or it came
    /// back.
    NotRepaired,
    /// No remedy helps; a problem report is offered.
    ReportOffered,
    /// The check passes again: see `resolved_how`.
    Resolved,
}

/// Who did something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Actor {
    /// TUR's healing service, by itself.
    Service,
    /// A person with administrator rights, at the command line.
    Administrator,
    /// A request through the healing service's socket: P@x's tool, or a
    /// person's own `tur-diag heal apply` without administrator rights.
    Request,
}

/// One fault, from when it was found until it was resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultRecord {
    /// The catalogue code.
    pub code: String,
    /// Where it stands.
    pub state: FaultState,
    /// When it was first found, UTC.
    pub first_seen_utc: String,
    /// When the check last found it, UTC.
    pub last_seen_utc: String,
    /// The TUR build it was (last) found in.
    pub build_id: Option<String>,
    /// The TUR version it was (last) found in.
    pub version: Option<String>,
    /// What the check found it about.
    pub subjects: Vec<String>,
    /// What the check saw.
    pub evidence: Vec<String>,
    /// Every remedy applied, in order.
    pub applied: Vec<AppliedRecord>,
    /// Whether a problem report was offered for it.
    pub report_offered: bool,
    /// Set when a person undid a remedy: the service does not apply its
    /// automatic remedy again until the fault is resolved.
    pub automatic_suppressed: bool,
    /// How it was resolved: `repair-held`, `fixed-by-update` or `cleared`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_how: Option<String>,
    /// When it was resolved, UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_utc: Option<String>,
}

/// One application of a remedy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedRecord {
    /// The application's own name, `a<N>`.
    pub id: String,
    /// The remedy.
    pub remedy: String,
    /// Who applied it.
    pub by: Actor,
    /// The account that asked, for a request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// When, UTC.
    pub at_utc: String,
    /// The boot it was applied in.
    pub boot_id: Option<String>,
    /// `repaired`, `not-repaired` or `failed` (a step did not run).
    pub outcome: String,
    /// What was done.
    pub detail: String,
    /// How to undo it.
    pub undo: UndoData,
    /// When it was undone, UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undone_utc: Option<String>,
}

/// What happened, in the order it happened. Notices are made from events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Its number, unique and increasing.
    pub seq: u64,
    /// When, UTC.
    pub at_utc: String,
    /// The fault's code.
    pub code: String,
    /// What happened.
    pub kind: EventKind,
    /// What it was about (units, programs).
    pub subjects: Vec<String>,
    /// The TUR version at the time.
    pub version: Option<String>,
    /// The remedy concerned, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
    /// Who acted, if anyone did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<Actor>,
}

/// The kinds of event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    /// Found, and waiting for the person.
    Detected,
    /// A remedy repaired it.
    Repaired,
    /// A remedy did not repair it, or it came back after a repair.
    NotRepaired,
    /// A later check still passed after a repair.
    Held,
    /// A problem report is offered.
    ReportOffered,
    /// It is gone, by itself or by the person's hand.
    Cleared,
    /// It is gone after an update that names its code.
    FixedByUpdate,
    /// It came back after an update that names its code.
    RecurredAfterUpdate,
    /// A remedy was undone.
    Undone,
}

impl EventKind {
    /// Whether the person is told about it.
    pub const fn is_notice(self) -> bool {
        !matches!(self, EventKind::Held | EventKind::Cleared)
    }
}

/// Makes `directory` (and its parents) exist, readable by root only.
///
/// # Errors
///
/// It cannot be created or its mode cannot be set.
pub fn private_dir(directory: &Path) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("{}: {error}", directory.display()))
}

fn unavailable(detail: String) -> DiagError {
    DiagError::new(ErrorKind::HealStateUnavailable, detail)
}

impl HealState {
    /// Reads the state at `path`; a missing file is an empty state.
    ///
    /// # Errors
    ///
    /// The file exists and cannot be read or is not a state of this format.
    pub fn load(path: &Path) -> Result<HealState, DiagError> {
        let text = match fs::read(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(HealState {
                    schema_version: STATE_SCHEMA_VERSION,
                    next_event: 1,
                    ..HealState::default()
                });
            }
            Err(error) => return Err(unavailable(format!("{}: {error}", path.display()))),
        };
        let state: HealState = serde_json::from_slice(&text)
            .map_err(|error| unavailable(format!("{}: {error}", path.display())))?;
        if state.schema_version != STATE_SCHEMA_VERSION {
            return Err(unavailable(format!(
                "{}: schema_version {} is not {STATE_SCHEMA_VERSION}",
                path.display(),
                state.schema_version
            )));
        }
        Ok(state)
    }

    /// Writes the state to `path` in one rename, readable by everyone.
    ///
    /// # Errors
    ///
    /// It cannot be written.
    pub fn save(&mut self, path: &Path) -> Result<(), DiagError> {
        self.trim();
        let parent = path
            .parent()
            .ok_or_else(|| unavailable(format!("{} has no directory", path.display())))?;
        fs::create_dir_all(parent)
            .map_err(|error| unavailable(format!("{}: {error}", parent.display())))?;
        let mut text = serde_json::to_vec_pretty(self)
            .map_err(|error| DiagError::new(ErrorKind::Internal, error.to_string()))?;
        text.push(b'\n');
        let temporary: PathBuf = parent.join(".state.json.tmp");
        let _ = fs::remove_file(&temporary);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&temporary)
            .map_err(|error| unavailable(format!("{}: {error}", temporary.display())))?;
        file.write_all(&text)
            .and_then(|()| file.sync_all())
            .map_err(|error| unavailable(format!("{}: {error}", temporary.display())))?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644))
            .map_err(|error| unavailable(format!("{}: {error}", temporary.display())))?;
        fs::rename(&temporary, path)
            .map_err(|error| unavailable(format!("{}: {error}", path.display())))
    }

    /// Keeps the newest [`MAX_EVENTS`] events and [`MAX_RESOLVED`] resolved
    /// faults.
    fn trim(&mut self) {
        if self.events.len() > MAX_EVENTS {
            let extra = self.events.len() - MAX_EVENTS;
            self.events.drain(..extra);
        }
        let resolved = self
            .faults
            .iter()
            .filter(|record| record.state == FaultState::Resolved)
            .count();
        let mut extra = resolved.saturating_sub(MAX_RESOLVED);
        self.faults.retain(|record| {
            if extra > 0 && record.state == FaultState::Resolved {
                extra -= 1;
                false
            } else {
                true
            }
        });
    }

    /// The index of the fault with `code` that is not resolved.
    pub fn active(&self, code: &str) -> Option<usize> {
        self.faults
            .iter()
            .position(|record| record.code == code && record.state != FaultState::Resolved)
    }

    /// The index of the newest record of `code`, resolved or not.
    pub fn latest(&self, code: &str) -> Option<usize> {
        self.faults.iter().rposition(|record| record.code == code)
    }

    /// Records an event and returns its number.
    pub fn event(
        &mut self,
        at_utc: &str,
        code: &str,
        kind: EventKind,
        subjects: &[String],
        remedy: Option<&str>,
        by: Option<Actor>,
    ) -> u64 {
        if self.next_event == 0 {
            self.next_event = 1;
        }
        let seq = self.next_event;
        self.next_event += 1;
        self.events.push(Event {
            seq,
            at_utc: at_utc.to_owned(),
            code: code.to_owned(),
            kind,
            subjects: subjects.to_vec(),
            version: self.version.clone(),
            remedy: remedy.map(str::to_owned),
            by,
        });
        seq
    }
}

/// The lock that keeps two runs of self-healing apart, held until dropped.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

impl Lock {
    /// Waits for the lock in `data_dir`.
    ///
    /// # Errors
    ///
    /// The data directory or the lock file cannot be used.
    pub fn take(data_dir: &Path) -> Result<Lock, DiagError> {
        private_dir(data_dir).map_err(unavailable)?;
        let path = data_dir.join("lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|error| unavailable(format!("{}: {error}", path.display())))?;
        file.lock()
            .map_err(|error| unavailable(format!("{}: {error}", path.display())))?;
        Ok(Lock { _file: file })
    }
}
