// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The installer phase model (specification §6.2, §28.1 and §28.2).
//!
//! An installation is sixteen phases in a fixed order. Each phase has a
//! [`PhaseRecord`] with the fields §6.2 lists, and moves between states only
//! along the transitions [`PhaseTable`] allows. Anything else is a typed
//! [`TransitionError`].
//!
//! Two properties of a phase decide what may happen after a failure or a
//! crash:
//!
//! * its [`RepeatClass`]: `idempotent`, `reconcilable` or `non_repeatable`;
//! * whether it is **destructive**, which here means it writes to the target
//!   disk or to the firmware's boot configuration.
//!
//! This module only keeps the books. It does not perform any phase.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::codes::{Code, Fault, InstallCode};
use crate::time::Timestamp;

/// How many phases there are.
pub const PHASE_COUNT: usize = 16;

/// The installer phases, in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PhaseId {
    /// The session directory exists and is durable.
    Start,
    /// Boot mode, Secure Boot, CPU and memory checks.
    EnvironmentCheck,
    /// Detection of the machine's hardware.
    HardwareInventory,
    /// Enumeration of block devices and stable identifiers.
    StorageDiscovery,
    /// Choice of the target disk.
    TargetSelection,
    /// Computation of the partition plan and its consequences.
    PartitionPlan,
    /// The user's explicit confirmation of the plan.
    UserConfirmation,
    /// Writing the partition table.
    Partitioning,
    /// Creating encrypted containers. Skipped when encryption is not chosen.
    EncryptionSetup,
    /// Creating filesystems.
    FilesystemCreation,
    /// Deploying the operating system image to the target.
    ImageDeployment,
    /// Installing the bootloader and boot entries.
    BootloaderInstall,
    /// Configuring the initramfs of the installed system.
    InitramfsConfiguration,
    /// Writing first-boot configuration to the installed system.
    FirstBootConfiguration,
    /// Checking the installed system.
    InstallValidation,
    /// The installation is complete.
    Complete,
}

/// What may be done with a phase after a failure or interruption
/// (specification §28.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatClass {
    /// Safe to repeat after verifying current state.
    Idempotent,
    /// An incomplete run can be inspected and reconciled.
    Reconcilable,
    /// Needs a fresh confirmation and a new operation record.
    NonRepeatable,
}

impl RepeatClass {
    /// The class as written in files and documentation.
    pub const fn as_str(self) -> &'static str {
        match self {
            RepeatClass::Idempotent => "idempotent",
            RepeatClass::Reconcilable => "reconcilable",
            RepeatClass::NonRepeatable => "non_repeatable",
        }
    }
}

impl PhaseId {
    /// Every phase, in execution order.
    pub const ALL: [PhaseId; PHASE_COUNT] = [
        PhaseId::Start,
        PhaseId::EnvironmentCheck,
        PhaseId::HardwareInventory,
        PhaseId::StorageDiscovery,
        PhaseId::TargetSelection,
        PhaseId::PartitionPlan,
        PhaseId::UserConfirmation,
        PhaseId::Partitioning,
        PhaseId::EncryptionSetup,
        PhaseId::FilesystemCreation,
        PhaseId::ImageDeployment,
        PhaseId::BootloaderInstall,
        PhaseId::InitramfsConfiguration,
        PhaseId::FirstBootConfiguration,
        PhaseId::InstallValidation,
        PhaseId::Complete,
    ];

    /// The phase's position in the execution order, starting at zero.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The stable identifier, for example `PARTITIONING`.
    pub const fn as_str(self) -> &'static str {
        match self {
            PhaseId::Start => "START",
            PhaseId::EnvironmentCheck => "ENVIRONMENT_CHECK",
            PhaseId::HardwareInventory => "HARDWARE_INVENTORY",
            PhaseId::StorageDiscovery => "STORAGE_DISCOVERY",
            PhaseId::TargetSelection => "TARGET_SELECTION",
            PhaseId::PartitionPlan => "PARTITION_PLAN",
            PhaseId::UserConfirmation => "USER_CONFIRMATION",
            PhaseId::Partitioning => "PARTITIONING",
            PhaseId::EncryptionSetup => "ENCRYPTION_SETUP",
            PhaseId::FilesystemCreation => "FILESYSTEM_CREATION",
            PhaseId::ImageDeployment => "IMAGE_DEPLOYMENT",
            PhaseId::BootloaderInstall => "BOOTLOADER_INSTALL",
            PhaseId::InitramfsConfiguration => "INITRAMFS_CONFIGURATION",
            PhaseId::FirstBootConfiguration => "FIRST_BOOT_CONFIGURATION",
            PhaseId::InstallValidation => "INSTALL_VALIDATION",
            PhaseId::Complete => "COMPLETE",
        }
    }

    /// The name shown to the user.
    pub const fn name(self) -> &'static str {
        match self {
            PhaseId::Start => "Start",
            PhaseId::EnvironmentCheck => "Environment check",
            PhaseId::HardwareInventory => "Hardware inventory",
            PhaseId::StorageDiscovery => "Storage discovery",
            PhaseId::TargetSelection => "Target selection",
            PhaseId::PartitionPlan => "Partition plan",
            PhaseId::UserConfirmation => "User confirmation",
            PhaseId::Partitioning => "Partitioning",
            PhaseId::EncryptionSetup => "Encryption setup",
            PhaseId::FilesystemCreation => "Filesystem creation",
            PhaseId::ImageDeployment => "Image deployment",
            PhaseId::BootloaderInstall => "Bootloader installation",
            PhaseId::InitramfsConfiguration => "Initramfs configuration",
            PhaseId::FirstBootConfiguration => "First-boot configuration",
            PhaseId::InstallValidation => "Installation validation",
            PhaseId::Complete => "Complete",
        }
    }

    /// The phase's repeat class.
    ///
    /// Writing the partition table, creating encrypted containers and
    /// creating filesystems are non-repeatable. The later phases that write
    /// to the target can be inspected and reconciled. Everything else can be
    /// repeated. `USER_CONFIRMATION` is idempotent in the sense that asking
    /// again is always safe; a confirmation that was given is never reused
    /// after a restart, because it exists only in memory.
    pub const fn class(self) -> RepeatClass {
        match self {
            PhaseId::Partitioning | PhaseId::EncryptionSetup | PhaseId::FilesystemCreation => {
                RepeatClass::NonRepeatable
            }
            PhaseId::ImageDeployment
            | PhaseId::BootloaderInstall
            | PhaseId::InitramfsConfiguration
            | PhaseId::FirstBootConfiguration => RepeatClass::Reconcilable,
            PhaseId::Start
            | PhaseId::EnvironmentCheck
            | PhaseId::HardwareInventory
            | PhaseId::StorageDiscovery
            | PhaseId::TargetSelection
            | PhaseId::PartitionPlan
            | PhaseId::UserConfirmation
            | PhaseId::InstallValidation
            | PhaseId::Complete => RepeatClass::Idempotent,
        }
    }

    /// True when the phase writes to the target disk or to the firmware's
    /// boot configuration.
    pub const fn destructive(self) -> bool {
        matches!(
            self,
            PhaseId::Partitioning
                | PhaseId::EncryptionSetup
                | PhaseId::FilesystemCreation
                | PhaseId::ImageDeployment
                | PhaseId::BootloaderInstall
                | PhaseId::InitramfsConfiguration
                | PhaseId::FirstBootConfiguration
        )
    }

    /// True when the phase may be skipped.
    pub const fn optional(self) -> bool {
        matches!(self, PhaseId::EncryptionSetup)
    }
}

impl fmt::Display for PhaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The state of a phase (specification §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseState {
    /// Not started.
    Pending,
    /// In progress.
    Running,
    /// Finished without problems.
    Succeeded,
    /// Finished, with something the user should know.
    Warning,
    /// Finished unsuccessfully.
    Failed,
    /// Deliberately not run.
    Skipped,
    /// Stopped at the user's request.
    Cancelled,
}

impl PhaseState {
    /// The state as written in files.
    pub const fn as_str(self) -> &'static str {
        match self {
            PhaseState::Pending => "pending",
            PhaseState::Running => "running",
            PhaseState::Succeeded => "succeeded",
            PhaseState::Warning => "warning",
            PhaseState::Failed => "failed",
            PhaseState::Skipped => "skipped",
            PhaseState::Cancelled => "cancelled",
        }
    }

    /// True for the states that let the next phase start.
    pub const fn is_done_ok(self) -> bool {
        matches!(
            self,
            PhaseState::Succeeded | PhaseState::Warning | PhaseState::Skipped
        )
    }
}

impl fmt::Display for PhaseState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Progress within a running phase.
///
/// Specification §6.1 forbids fake precision: a phase with no reliable total
/// reports an activity, not a percentage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Progress {
    /// A reliable count of work done out of a known total.
    Determinate {
        /// Units of work completed.
        completed: u64,
        /// Total units of work.
        total: u64,
        /// What a unit is, for example `bytes`.
        unit: String,
    },
    /// No reliable total: only what is being done.
    Indeterminate {
        /// The concrete activity, for example `Copying the system image`.
        activity: String,
    },
}

/// Everything a phase emits (specification §6.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseRecord {
    /// The phase's stable identifier.
    pub phase_id: PhaseId,
    /// The phase's name as shown to the user.
    pub phase_name: String,
    /// The phase's state.
    pub state: PhaseState,
    /// How many times the phase has been started.
    pub attempt: u32,
    /// When the current attempt started.
    pub started_at: Option<Timestamp>,
    /// When the current attempt finished.
    pub finished_at: Option<Timestamp>,
    /// How long the current attempt took.
    pub duration_ms: Option<u64>,
    /// Progress, when meaningful.
    pub progress: Option<Progress>,
    /// The error code, when the phase failed.
    pub error_code: Option<Code>,
    /// One line saying what the phase did or is doing.
    pub summary: String,
    /// Technical details for a developer.
    pub technical_details: Option<String>,
    /// Where the phase's log is, relative to the session directory.
    pub log_reference: Option<String>,
    /// What the user can safely do next after a failure.
    pub recovery_action: Option<String>,
}

impl PhaseRecord {
    /// The record of a phase that has not started.
    pub fn pending(phase: PhaseId) -> PhaseRecord {
        PhaseRecord {
            phase_id: phase,
            phase_name: phase.name().to_owned(),
            state: PhaseState::Pending,
            attempt: 0,
            started_at: None,
            finished_at: None,
            duration_ms: None,
            progress: None,
            error_code: None,
            summary: "Not started".to_owned(),
            technical_details: None,
            log_reference: None,
            recovery_action: None,
        }
    }
}

/// How a running phase ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Without problems.
    Succeeded,
    /// Successfully, with something the user should know.
    Warning,
    /// Unsuccessfully, with a stable error code.
    Failed(Code),
    /// Stopped at the user's request.
    Cancelled,
}

impl Outcome {
    const fn state(&self) -> PhaseState {
        match self {
            Outcome::Succeeded => PhaseState::Succeeded,
            Outcome::Warning => PhaseState::Warning,
            Outcome::Failed(_) => PhaseState::Failed,
            Outcome::Cancelled => PhaseState::Cancelled,
        }
    }
}

/// What a phase reports when it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// How the phase ended.
    pub outcome: Outcome,
    /// One line saying what happened.
    pub summary: String,
    /// Technical details for a developer.
    pub technical_details: Option<String>,
    /// Where the phase's log is, relative to the session directory.
    pub log_reference: Option<String>,
    /// What the user can safely do next.
    pub recovery_action: Option<String>,
}

impl Completion {
    fn new(outcome: Outcome, summary: &str) -> Completion {
        Completion {
            outcome,
            summary: summary.to_owned(),
            technical_details: None,
            log_reference: None,
            recovery_action: None,
        }
    }

    /// The phase finished without problems.
    pub fn succeeded(summary: &str) -> Completion {
        Completion::new(Outcome::Succeeded, summary)
    }

    /// The phase finished, with something the user should know.
    pub fn warning(summary: &str) -> Completion {
        Completion::new(Outcome::Warning, summary)
    }

    /// The phase failed with `code`.
    pub fn failed(code: Code, summary: &str) -> Completion {
        Completion::new(Outcome::Failed(code), summary)
    }

    /// The phase was stopped at the user's request.
    pub fn cancelled(summary: &str) -> Completion {
        Completion::new(Outcome::Cancelled, summary)
    }

    /// Adds technical details.
    pub fn with_details(mut self, details: &str) -> Completion {
        self.technical_details = Some(details.to_owned());
        self
    }

    /// Adds a log reference.
    pub fn with_log(mut self, reference: &str) -> Completion {
        self.log_reference = Some(reference.to_owned());
        self
    }

    /// Adds a recovery action.
    pub fn with_recovery(mut self, action: &str) -> Completion {
        self.recovery_action = Some(action.to_owned());
        self
    }
}

/// A transition the phase model does not allow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    /// The phase is not in a state the transition can start from.
    Illegal {
        /// The phase.
        phase: PhaseId,
        /// Its current state.
        from: PhaseState,
        /// The state that was asked for.
        to: PhaseState,
    },
    /// An earlier phase has not finished successfully.
    OutOfOrder {
        /// The phase that was to start.
        phase: PhaseId,
        /// The earlier phase in the way.
        blocked_by: PhaseId,
        /// That phase's state.
        blocked_state: PhaseState,
    },
    /// A failed or cancelled non-repeatable phase cannot simply run again.
    RetryNotAllowed {
        /// The phase.
        phase: PhaseId,
    },
    /// Phases cannot be reopened once a phase has written to the target.
    ReopenRefused {
        /// The phase that was to be reopened.
        phase: PhaseId,
        /// The first phase that wrote to the target.
        written_by: PhaseId,
    },
    /// Only optional phases can be skipped.
    NotOptional {
        /// The phase.
        phase: PhaseId,
    },
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            TransitionError::Illegal { .. }
            | TransitionError::OutOfOrder { .. }
            | TransitionError::NotOptional { .. } => {
                "The installer tried to perform its steps in an order that is not allowed, \
                 and stopped. Nothing was changed by this step. Export the diagnostics and \
                 report this."
            }
            TransitionError::RetryNotAllowed { .. } => {
                "This step changes the disk and cannot simply be run again. Start the \
                 installation again so that the disk is checked and you can confirm afresh."
            }
            TransitionError::ReopenRefused { .. } => {
                "The installer has already written to the disk, so earlier choices can no \
                 longer be changed in this session. Start the installation again to choose \
                 differently."
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for TransitionError {}

impl Fault for TransitionError {
    fn code(&self) -> InstallCode {
        match self {
            TransitionError::Illegal { .. } => InstallCode::PhaseIllegalTransition,
            TransitionError::OutOfOrder { .. } => InstallCode::PhaseOutOfOrder,
            TransitionError::RetryNotAllowed { .. } => InstallCode::PhaseRetryNotAllowed,
            TransitionError::ReopenRefused { .. } => InstallCode::PhaseReopenRefused,
            TransitionError::NotOptional { .. } => InstallCode::PhaseNotOptional,
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            TransitionError::Illegal { phase, from, to } => {
                format!("phase {phase}: transition from {from} to {to} is not allowed")
            }
            TransitionError::OutOfOrder {
                phase,
                blocked_by,
                blocked_state,
            } => format!("phase {phase} cannot start while {blocked_by} is {blocked_state}"),
            TransitionError::RetryNotAllowed { phase } => format!(
                "phase {phase} is {}; a retry needs a fresh confirmation and a new session",
                phase.class().as_str()
            ),
            TransitionError::ReopenRefused { phase, written_by } => {
                format!("phase {phase} cannot be reopened: {written_by} has written to the target")
            }
            TransitionError::NotOptional { phase } => {
                format!("phase {phase} is mandatory and cannot be skipped")
            }
        }
    }
}

/// The sixteen phase records and the transitions between their states.
///
/// The legal transitions are:
///
/// * `pending -> running` ([`PhaseTable::begin`]), once every earlier phase
///   has succeeded, finished with a warning or been skipped;
/// * `pending -> skipped` ([`PhaseTable::skip`]), for optional phases, under
///   the same ordering rule;
/// * `running -> succeeded | warning | failed | cancelled`
///   ([`PhaseTable::finish`]);
/// * `failed | cancelled -> running` (a retry through [`PhaseTable::begin`]),
///   for phases that are not non-repeatable;
/// * any state except `running` back to `pending` for a phase and all later
///   ones ([`PhaseTable::reopen`]), only while no destructive phase has ever
///   started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<PhaseRecord>", into = "Vec<PhaseRecord>")]
pub struct PhaseTable {
    records: Vec<PhaseRecord>,
}

impl Default for PhaseTable {
    fn default() -> Self {
        PhaseTable::new()
    }
}

impl From<PhaseTable> for Vec<PhaseRecord> {
    fn from(table: PhaseTable) -> Self {
        table.records
    }
}

impl TryFrom<Vec<PhaseRecord>> for PhaseTable {
    type Error = String;

    fn try_from(records: Vec<PhaseRecord>) -> Result<Self, Self::Error> {
        if records.len() != PHASE_COUNT {
            return Err(format!(
                "a phase table has {PHASE_COUNT} records, found {}",
                records.len()
            ));
        }
        for (record, expected) in records.iter().zip(PhaseId::ALL) {
            if record.phase_id != expected {
                return Err(format!(
                    "phase records are out of order: expected {expected}, found {}",
                    record.phase_id
                ));
            }
        }
        Ok(PhaseTable { records })
    }
}

fn is_target_write(record: &PhaseRecord) -> bool {
    record.phase_id.destructive() && record.state != PhaseState::Pending
}

impl PhaseTable {
    /// A table with every phase pending.
    pub fn new() -> PhaseTable {
        PhaseTable {
            records: PhaseId::ALL.into_iter().map(PhaseRecord::pending).collect(),
        }
    }

    /// The record of one phase.
    pub fn get(&self, phase: PhaseId) -> &PhaseRecord {
        &self.records[phase.index()]
    }

    /// All records, in execution order.
    pub fn records(&self) -> &[PhaseRecord] {
        &self.records
    }

    /// The phase that is running, if any.
    pub fn running(&self) -> Option<PhaseId> {
        self.records
            .iter()
            .find(|record| record.state == PhaseState::Running)
            .map(|record| record.phase_id)
    }

    /// The first phase that has not succeeded, warned or been skipped.
    pub fn first_incomplete(&self) -> Option<PhaseId> {
        self.records
            .iter()
            .find(|record| !record.state.is_done_ok())
            .map(|record| record.phase_id)
    }

    /// True when every phase has succeeded, warned or been skipped.
    pub fn is_complete(&self) -> bool {
        self.first_incomplete().is_none()
    }

    /// The first destructive phase that has ever left the pending state.
    pub fn first_target_write(&self) -> Option<PhaseId> {
        self.records
            .iter()
            .find(|record| is_target_write(record))
            .map(|record| record.phase_id)
    }

    /// The last destructive phase that has ever left the pending state.
    pub fn latest_target_write(&self) -> Option<PhaseId> {
        self.records
            .iter()
            .rev()
            .find(|record| is_target_write(record))
            .map(|record| record.phase_id)
    }

    fn ensure_predecessors_done(&self, phase: PhaseId) -> Result<(), TransitionError> {
        for record in &self.records[..phase.index()] {
            if !record.state.is_done_ok() {
                return Err(TransitionError::OutOfOrder {
                    phase,
                    blocked_by: record.phase_id,
                    blocked_state: record.state,
                });
            }
        }
        Ok(())
    }

    /// Starts a phase, or retries one that failed or was cancelled.
    ///
    /// Returns the updated record.
    pub fn begin(&mut self, phase: PhaseId, now: Timestamp) -> Result<PhaseRecord, TransitionError> {
        let state = self.get(phase).state;
        match state {
            PhaseState::Pending => {}
            PhaseState::Failed | PhaseState::Cancelled => {
                if phase.class() == RepeatClass::NonRepeatable {
                    return Err(TransitionError::RetryNotAllowed { phase });
                }
            }
            PhaseState::Running
            | PhaseState::Succeeded
            | PhaseState::Warning
            | PhaseState::Skipped => {
                return Err(TransitionError::Illegal {
                    phase,
                    from: state,
                    to: PhaseState::Running,
                });
            }
        }
        self.ensure_predecessors_done(phase)?;
        let record = &mut self.records[phase.index()];
        let attempt = record.attempt.saturating_add(1);
        *record = PhaseRecord::pending(phase);
        record.state = PhaseState::Running;
        record.attempt = attempt;
        record.started_at = Some(now);
        record.summary = "In progress".to_owned();
        Ok(record.clone())
    }

    /// Skips an optional phase.
    ///
    /// Returns the updated record.
    pub fn skip(
        &mut self,
        phase: PhaseId,
        now: Timestamp,
        summary: &str,
    ) -> Result<PhaseRecord, TransitionError> {
        if !phase.optional() {
            return Err(TransitionError::NotOptional { phase });
        }
        let state = self.get(phase).state;
        if state != PhaseState::Pending {
            return Err(TransitionError::Illegal {
                phase,
                from: state,
                to: PhaseState::Skipped,
            });
        }
        self.ensure_predecessors_done(phase)?;
        let record = &mut self.records[phase.index()];
        record.state = PhaseState::Skipped;
        record.finished_at = Some(now);
        record.summary = summary.to_owned();
        Ok(record.clone())
    }

    /// Ends the running phase `phase`.
    ///
    /// Returns the updated record.
    pub fn finish(
        &mut self,
        phase: PhaseId,
        now: Timestamp,
        completion: Completion,
    ) -> Result<PhaseRecord, TransitionError> {
        let to = completion.outcome.state();
        let record = &mut self.records[phase.index()];
        if record.state != PhaseState::Running {
            return Err(TransitionError::Illegal {
                phase,
                from: record.state,
                to,
            });
        }
        record.state = to;
        record.finished_at = Some(now);
        record.duration_ms = record.started_at.map(|started| now.millis_since(started));
        record.error_code = match completion.outcome {
            Outcome::Failed(code) => Some(code),
            Outcome::Succeeded | Outcome::Warning | Outcome::Cancelled => None,
        };
        record.summary = completion.summary;
        record.technical_details = completion.technical_details;
        record.log_reference = completion.log_reference;
        record.recovery_action = completion.recovery_action;
        Ok(record.clone())
    }

    /// Sets the progress of the running phase `phase`.
    pub fn set_progress(&mut self, phase: PhaseId, progress: Progress) -> Result<(), TransitionError> {
        let record = &mut self.records[phase.index()];
        if record.state != PhaseState::Running {
            return Err(TransitionError::Illegal {
                phase,
                from: record.state,
                to: PhaseState::Running,
            });
        }
        record.progress = Some(progress);
        Ok(())
    }

    /// Returns `phase` and every later phase to pending, so that the user can
    /// go back and choose again.
    ///
    /// Refused once any destructive phase has started, and while `phase` or a
    /// later phase is running. Returns the records that changed.
    pub fn reopen(&mut self, phase: PhaseId) -> Result<Vec<PhaseRecord>, TransitionError> {
        if let Some(written_by) = self.first_target_write() {
            return Err(TransitionError::ReopenRefused { phase, written_by });
        }
        if let Some(running) = self.records[phase.index()..]
            .iter()
            .find(|record| record.state == PhaseState::Running)
        {
            return Err(TransitionError::Illegal {
                phase: running.phase_id,
                from: PhaseState::Running,
                to: PhaseState::Pending,
            });
        }
        let mut changed = Vec::new();
        for record in &mut self.records[phase.index()..] {
            if record.state != PhaseState::Pending {
                let attempt = record.attempt;
                *record = PhaseRecord::pending(record.phase_id);
                record.attempt = attempt;
                changed.push(record.clone());
            }
        }
        Ok(changed)
    }

    /// Overwrites one phase's record with a record read from the log.
    ///
    /// This applies no transition rule. It exists for replaying a session's
    /// log, where the rules were applied when the record was written.
    pub fn restore(&mut self, record: PhaseRecord) {
        let index = record.phase_id.index();
        self.records[index] = record;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: u64) -> Timestamp {
        Timestamp::from_unix_millis(millis)
    }

    /// Runs every phase before `until` to success, skipping encryption.
    fn advance_to(table: &mut PhaseTable, until: PhaseId) {
        for phase in PhaseId::ALL {
            if phase == until {
                return;
            }
            if phase.optional() {
                table.skip(phase, at(0), "Not chosen").unwrap();
            } else {
                table.begin(phase, at(0)).unwrap();
                table
                    .finish(phase, at(10), Completion::succeeded("Done"))
                    .unwrap();
            }
        }
    }

    #[test]
    fn there_are_sixteen_phases_in_the_specified_order() {
        let names: Vec<&str> = PhaseId::ALL.iter().map(|phase| phase.as_str()).collect();
        assert_eq!(
            names,
            [
                "START",
                "ENVIRONMENT_CHECK",
                "HARDWARE_INVENTORY",
                "STORAGE_DISCOVERY",
                "TARGET_SELECTION",
                "PARTITION_PLAN",
                "USER_CONFIRMATION",
                "PARTITIONING",
                "ENCRYPTION_SETUP",
                "FILESYSTEM_CREATION",
                "IMAGE_DEPLOYMENT",
                "BOOTLOADER_INSTALL",
                "INITRAMFS_CONFIGURATION",
                "FIRST_BOOT_CONFIGURATION",
                "INSTALL_VALIDATION",
                "COMPLETE"
            ]
        );
        for (index, phase) in PhaseId::ALL.into_iter().enumerate() {
            assert_eq!(phase.index(), index);
            assert_eq!(
                serde_json::to_string(&phase).unwrap(),
                format!("\"{}\"", phase.as_str())
            );
            assert!(!phase.name().is_empty());
        }
    }

    #[test]
    fn classification_follows_the_specification() {
        use PhaseId as P;
        use RepeatClass as C;
        let expected = [
            (P::Start, C::Idempotent, false),
            (P::EnvironmentCheck, C::Idempotent, false),
            (P::HardwareInventory, C::Idempotent, false),
            (P::StorageDiscovery, C::Idempotent, false),
            (P::TargetSelection, C::Idempotent, false),
            (P::PartitionPlan, C::Idempotent, false),
            (P::UserConfirmation, C::Idempotent, false),
            (P::Partitioning, C::NonRepeatable, true),
            (P::EncryptionSetup, C::NonRepeatable, true),
            (P::FilesystemCreation, C::NonRepeatable, true),
            (P::ImageDeployment, C::Reconcilable, true),
            (P::BootloaderInstall, C::Reconcilable, true),
            (P::InitramfsConfiguration, C::Reconcilable, true),
            (P::FirstBootConfiguration, C::Reconcilable, true),
            (P::InstallValidation, C::Idempotent, false),
            (P::Complete, C::Idempotent, false),
        ];
        for (phase, class, destructive) in expected {
            assert_eq!(phase.class(), class, "{phase}");
            assert_eq!(phase.destructive(), destructive, "{phase}");
            // Nothing non-repeatable is ever non-destructive, and the
            // other way round nothing destructive is idempotent.
            if destructive {
                assert_ne!(class, C::Idempotent, "{phase}");
            } else {
                assert_eq!(class, C::Idempotent, "{phase}");
            }
        }
        assert_eq!(expected.len(), PHASE_COUNT);
    }

    #[test]
    fn a_new_table_is_all_pending() {
        let table = PhaseTable::new();
        assert_eq!(table.records().len(), PHASE_COUNT);
        for record in table.records() {
            assert_eq!(record.state, PhaseState::Pending);
            assert_eq!(record.attempt, 0);
            assert_eq!(record.phase_name, record.phase_id.name());
        }
        assert_eq!(table.first_incomplete(), Some(PhaseId::Start));
        assert_eq!(table.running(), None);
        assert_eq!(table.first_target_write(), None);
        assert!(!table.is_complete());
        assert_eq!(PhaseTable::default(), table);
    }

    #[test]
    fn a_phase_records_its_timing_and_outcome() {
        let mut table = PhaseTable::new();
        let started = table.begin(PhaseId::Start, at(1_000)).unwrap();
        assert_eq!(started.state, PhaseState::Running);
        assert_eq!(started.attempt, 1);
        assert_eq!(started.started_at, Some(at(1_000)));
        assert_eq!(started.finished_at, None);
        assert_eq!(table.running(), Some(PhaseId::Start));

        let completion = Completion::warning("Session created on a slow medium")
            .with_details("fsync took 900 ms")
            .with_log("logs/installer-ui.log")
            .with_recovery("None needed");
        let finished = table.finish(PhaseId::Start, at(1_900), completion).unwrap();
        assert_eq!(finished.state, PhaseState::Warning);
        assert_eq!(finished.finished_at, Some(at(1_900)));
        assert_eq!(finished.duration_ms, Some(900));
        assert_eq!(finished.error_code, None);
        assert_eq!(finished.summary, "Session created on a slow medium");
        assert_eq!(finished.technical_details.as_deref(), Some("fsync took 900 ms"));
        assert_eq!(finished.log_reference.as_deref(), Some("logs/installer-ui.log"));
        assert_eq!(finished.recovery_action.as_deref(), Some("None needed"));
        assert_eq!(table.get(PhaseId::Start), &finished);
        assert_eq!(table.running(), None);
    }

    #[test]
    fn a_clock_that_goes_backwards_gives_a_zero_duration() {
        let mut table = PhaseTable::new();
        table.begin(PhaseId::Start, at(5_000)).unwrap();
        let record = table
            .finish(PhaseId::Start, at(4_000), Completion::succeeded("Done"))
            .unwrap();
        assert_eq!(record.duration_ms, Some(0));
    }

    #[test]
    fn phases_must_run_in_order() {
        let mut table = PhaseTable::new();
        for phase in PhaseId::ALL.into_iter().skip(1) {
            let error = table.begin(phase, at(0)).unwrap_err();
            assert_eq!(
                error,
                TransitionError::OutOfOrder {
                    phase,
                    blocked_by: PhaseId::Start,
                    blocked_state: PhaseState::Pending
                }
            );
            assert_eq!(error.code(), InstallCode::PhaseOutOfOrder);
        }
        table.begin(PhaseId::Start, at(0)).unwrap();
        // A running predecessor blocks too.
        assert_eq!(
            table.begin(PhaseId::EnvironmentCheck, at(0)).unwrap_err(),
            TransitionError::OutOfOrder {
                phase: PhaseId::EnvironmentCheck,
                blocked_by: PhaseId::Start,
                blocked_state: PhaseState::Running
            }
        );
    }

    #[test]
    fn every_illegal_transition_is_a_typed_error() {
        // From each state, try each operation and compare with the table of
        // what is legal.
        let states = [
            PhaseState::Pending,
            PhaseState::Running,
            PhaseState::Succeeded,
            PhaseState::Warning,
            PhaseState::Failed,
            PhaseState::Cancelled,
        ];
        for from in states {
            let phase = PhaseId::StorageDiscovery;
            let prepare = || {
                let mut table = PhaseTable::new();
                advance_to(&mut table, phase);
                let failure = Completion::failed(InstallCode::InventoryUnreadable.into(), "Failed");
                match from {
                    PhaseState::Pending => {}
                    PhaseState::Running => {
                        table.begin(phase, at(0)).unwrap();
                    }
                    PhaseState::Succeeded => {
                        table.begin(phase, at(0)).unwrap();
                        table
                            .finish(phase, at(1), Completion::succeeded("Done"))
                            .unwrap();
                    }
                    PhaseState::Warning => {
                        table.begin(phase, at(0)).unwrap();
                        table
                            .finish(phase, at(1), Completion::warning("Done"))
                            .unwrap();
                    }
                    PhaseState::Failed => {
                        table.begin(phase, at(0)).unwrap();
                        table.finish(phase, at(1), failure).unwrap();
                    }
                    PhaseState::Cancelled => {
                        table.begin(phase, at(0)).unwrap();
                        table
                            .finish(phase, at(1), Completion::cancelled("Stopped"))
                            .unwrap();
                    }
                    PhaseState::Skipped => unreachable!("not in the list"),
                }
                assert_eq!(table.get(phase).state, from);
                table
            };

            let begin = prepare().begin(phase, at(2));
            let may_begin = matches!(
                from,
                PhaseState::Pending | PhaseState::Failed | PhaseState::Cancelled
            );
            assert_eq!(begin.is_ok(), may_begin, "begin from {from}");
            if let Err(error) = begin {
                assert_eq!(
                    error,
                    TransitionError::Illegal {
                        phase,
                        from,
                        to: PhaseState::Running
                    }
                );
                assert_eq!(error.code(), InstallCode::PhaseIllegalTransition);
            }

            for completion in [
                Completion::succeeded("x"),
                Completion::warning("x"),
                Completion::failed(InstallCode::SessionIo.into(), "x"),
                Completion::cancelled("x"),
            ] {
                let to = completion.outcome.state();
                let finish = prepare().finish(phase, at(2), completion);
                assert_eq!(
                    finish.is_ok(),
                    from == PhaseState::Running,
                    "finish from {from}"
                );
                if let Err(error) = finish {
                    assert_eq!(error, TransitionError::Illegal { phase, from, to });
                }
            }

            let progress = Progress::Indeterminate {
                activity: "Reading the device list".to_owned(),
            };
            assert_eq!(
                prepare().set_progress(phase, progress).is_ok(),
                from == PhaseState::Running,
                "progress from {from}"
            );
        }
    }

    #[test]
    fn a_failed_phase_records_its_error_code_and_can_be_retried_when_safe() {
        let mut table = PhaseTable::new();
        advance_to(&mut table, PhaseId::StorageDiscovery);
        table.begin(PhaseId::StorageDiscovery, at(100)).unwrap();
        table
            .set_progress(
                PhaseId::StorageDiscovery,
                Progress::Determinate {
                    completed: 1,
                    total: 4,
                    unit: "devices".to_owned(),
                },
            )
            .unwrap();
        let failure = Completion::failed(
            InstallCode::InventoryMalformed.into(),
            "The device list could not be read",
        )
        .with_recovery("Retry");
        let failed = table
            .finish(PhaseId::StorageDiscovery, at(150), failure)
            .unwrap();
        assert_eq!(failed.state, PhaseState::Failed);
        assert_eq!(
            failed.error_code.as_ref().map(Code::as_str),
            Some("UOS-INSTALL-INVENTORY-002")
        );
        assert!(failed.progress.is_some());

        let retried = table.begin(PhaseId::StorageDiscovery, at(200)).unwrap();
        assert_eq!(retried.state, PhaseState::Running);
        assert_eq!(retried.attempt, 2);
        assert_eq!(retried.error_code, None);
        assert_eq!(retried.recovery_action, None);
        assert_eq!(retried.progress, None);
        assert_eq!(retried.started_at, Some(at(200)));
    }

    #[test]
    fn non_repeatable_phases_cannot_be_retried() {
        for phase in [
            PhaseId::Partitioning,
            PhaseId::EncryptionSetup,
            PhaseId::FilesystemCreation,
        ] {
            for completion in [
                Completion::failed(InstallCode::SessionIo.into(), "Failed"),
                Completion::cancelled("Stopped"),
            ] {
                let mut table = PhaseTable::new();
                for earlier in PhaseId::ALL {
                    if earlier == phase {
                        break;
                    }
                    table.begin(earlier, at(0)).unwrap();
                    table
                        .finish(earlier, at(1), Completion::succeeded("Done"))
                        .unwrap();
                }
                table.begin(phase, at(2)).unwrap();
                table.finish(phase, at(3), completion).unwrap();
                let error = table.begin(phase, at(4)).unwrap_err();
                assert_eq!(error, TransitionError::RetryNotAllowed { phase });
                assert_eq!(error.code(), InstallCode::PhaseRetryNotAllowed);
            }
        }
    }

    #[test]
    fn reconcilable_phases_can_be_retried() {
        let mut table = PhaseTable::new();
        advance_to(&mut table, PhaseId::ImageDeployment);
        table.begin(PhaseId::ImageDeployment, at(0)).unwrap();
        table
            .finish(
                PhaseId::ImageDeployment,
                at(1),
                Completion::failed(InstallCode::SessionIo.into(), "Failed"),
            )
            .unwrap();
        assert_eq!(table.begin(PhaseId::ImageDeployment, at(2)).unwrap().attempt, 2);
    }

    #[test]
    fn only_the_optional_phase_can_be_skipped() {
        for phase in PhaseId::ALL {
            let mut table = PhaseTable::new();
            advance_to(&mut table, phase);
            let result = table.skip(phase, at(7), "Not chosen");
            if phase.optional() {
                let record = result.unwrap();
                assert_eq!(record.state, PhaseState::Skipped);
                assert_eq!(record.started_at, None);
                assert_eq!(record.finished_at, Some(at(7)));
                assert_eq!(record.summary, "Not chosen");
            } else {
                let error = result.unwrap_err();
                assert_eq!(error, TransitionError::NotOptional { phase });
                assert_eq!(error.code(), InstallCode::PhaseNotOptional);
            }
        }

        let mut table = PhaseTable::new();
        assert_eq!(
            table
                .skip(PhaseId::EncryptionSetup, at(0), "Not chosen")
                .unwrap_err()
                .code(),
            InstallCode::PhaseOutOfOrder
        );
        advance_to(&mut table, PhaseId::FilesystemCreation);
        assert_eq!(
            table
                .skip(PhaseId::EncryptionSetup, at(0), "Again")
                .unwrap_err(),
            TransitionError::Illegal {
                phase: PhaseId::EncryptionSetup,
                from: PhaseState::Skipped,
                to: PhaseState::Skipped
            }
        );
    }

    #[test]
    fn a_full_run_completes() {
        let mut table = PhaseTable::new();
        advance_to(&mut table, PhaseId::Complete);
        assert_eq!(table.first_incomplete(), Some(PhaseId::Complete));
        table.begin(PhaseId::Complete, at(0)).unwrap();
        table
            .finish(PhaseId::Complete, at(1), Completion::succeeded("Installed"))
            .unwrap();
        assert!(table.is_complete());
        assert_eq!(table.first_target_write(), Some(PhaseId::Partitioning));
        assert_eq!(
            table.latest_target_write(),
            Some(PhaseId::FirstBootConfiguration)
        );
    }

    #[test]
    fn reopening_returns_later_phases_to_pending_before_any_write() {
        let mut table = PhaseTable::new();
        advance_to(&mut table, PhaseId::Partitioning);
        let changed = table.reopen(PhaseId::TargetSelection).unwrap();
        let reopened: Vec<PhaseId> = changed.iter().map(|record| record.phase_id).collect();
        assert_eq!(
            reopened,
            [
                PhaseId::TargetSelection,
                PhaseId::PartitionPlan,
                PhaseId::UserConfirmation
            ]
        );
        for record in &changed {
            assert_eq!(record.state, PhaseState::Pending);
            assert_eq!(record.attempt, 1, "the attempt count is kept");
            assert_eq!(record.started_at, None);
        }
        assert_eq!(table.first_incomplete(), Some(PhaseId::TargetSelection));
        assert_eq!(
            table.begin(PhaseId::TargetSelection, at(0)).unwrap().attempt,
            2
        );
        // Reopening where everything is already pending changes nothing.
        assert!(table.reopen(PhaseId::PartitionPlan).unwrap().is_empty());
    }

    #[test]
    fn reopening_is_refused_while_running_and_after_a_target_write() {
        let mut table = PhaseTable::new();
        advance_to(&mut table, PhaseId::PartitionPlan);
        table.begin(PhaseId::PartitionPlan, at(0)).unwrap();
        assert_eq!(
            table.reopen(PhaseId::TargetSelection).unwrap_err(),
            TransitionError::Illegal {
                phase: PhaseId::PartitionPlan,
                from: PhaseState::Running,
                to: PhaseState::Pending
            }
        );

        let mut table = PhaseTable::new();
        advance_to(&mut table, PhaseId::Partitioning);
        table.begin(PhaseId::Partitioning, at(0)).unwrap();
        for phase in [
            PhaseId::Start,
            PhaseId::TargetSelection,
            PhaseId::UserConfirmation,
            PhaseId::Partitioning,
        ] {
            let error = table.reopen(phase).unwrap_err();
            assert_eq!(
                error,
                TransitionError::ReopenRefused {
                    phase,
                    written_by: PhaseId::Partitioning
                }
            );
            assert_eq!(error.code(), InstallCode::PhaseReopenRefused);
        }
    }

    #[test]
    fn records_and_tables_round_trip_through_json() {
        let mut table = PhaseTable::new();
        advance_to(&mut table, PhaseId::StorageDiscovery);
        table.begin(PhaseId::StorageDiscovery, at(1_794_464_220_123)).unwrap();
        table
            .set_progress(
                PhaseId::StorageDiscovery,
                Progress::Indeterminate {
                    activity: "Reading the device list".to_owned(),
                },
            )
            .unwrap();
        let json = serde_json::to_string(&table).unwrap();
        assert!(json.contains("\"phase_id\":\"STORAGE_DISCOVERY\""));
        assert!(json.contains("\"state\":\"running\""));
        assert!(json.contains("\"started_at\":\"2026-11-12T06:17:00.123Z\""));
        assert!(json.contains("\"kind\":\"indeterminate\""));
        let again: PhaseTable = serde_json::from_str(&json).unwrap();
        assert_eq!(again, table);
    }

    #[test]
    fn a_table_with_missing_or_misordered_phases_is_rejected() {
        let table = PhaseTable::new();
        let mut records: Vec<PhaseRecord> = table.clone().into();
        records.pop();
        let short = serde_json::to_string(&records).unwrap();
        assert!(serde_json::from_str::<PhaseTable>(&short).is_err());

        let mut records: Vec<PhaseRecord> = table.into();
        records.swap(3, 4);
        let misordered = serde_json::to_string(&records).unwrap();
        assert!(serde_json::from_str::<PhaseTable>(&misordered).is_err());

        assert!(serde_json::from_str::<PhaseTable>("[]").is_err());
        assert!(serde_json::from_str::<PhaseState>("\"exploded\"").is_err());
        assert!(serde_json::from_str::<PhaseId>("\"FORMAT_EVERYTHING\"").is_err());
    }

    #[test]
    fn restore_overwrites_one_record_without_checking_rules() {
        let mut table = PhaseTable::new();
        let mut record = PhaseRecord::pending(PhaseId::Partitioning);
        record.state = PhaseState::Running;
        table.restore(record.clone());
        assert_eq!(table.get(PhaseId::Partitioning), &record);
        assert_eq!(table.first_target_write(), Some(PhaseId::Partitioning));
    }
}
