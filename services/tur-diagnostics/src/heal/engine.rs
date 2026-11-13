// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The healing loop: detect, repair what may be repaired, verify, offer a
//! report, and link a fix that arrives in an update back to the fault.
//!
//! [`scan`] is what TUR's healing service runs on a timer, as root, without
//! P@x. [`apply`] and [`undo`] are what a person, or a request through the
//! service's socket ([`handle_request`]), asks for. Every function takes the
//! state and changes it; the caller holds the lock and saves it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::catalogue::{Catalogue, Fault, Remedy, ReportPolicy};
use super::checks::{Finding, run_check};
use super::machine::Machine;
use super::remedies::{self, UndoData};
use super::state::{
    Actor, AppliedRecord, DEFAULT_DATA_DIR, DEFAULT_STATE_FILE, EventKind, FaultRecord,
    FaultState, HealState, Lock,
};
use crate::error::{DiagError, ErrorKind};
use crate::manifest::format_utc;

/// Where the image says which fault codes it fixes.
pub const DEFAULT_FIXES_FILE: &str = "/usr/share/tur/heal/fixes.json";

/// The socket of TUR's healing service.
pub const DEFAULT_SOCKET: &str = "/run/tur/heal.socket";

/// The request format this build reads and answers.
pub const REQUEST_SCHEMA_VERSION: u32 = 1;

/// How often one remedy may be requested in one boot, unless the catalogue
/// says less.
pub const REQUESTS_PER_BOOT: u32 = 3;

/// The longest request read, in bytes.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// Where self-healing keeps what it keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The state file.
    pub state: PathBuf,
    /// Copies of replaced files and the lock.
    pub data_dir: PathBuf,
    /// The image's list of fixed codes.
    pub fixes: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Paths {
            state: PathBuf::from(DEFAULT_STATE_FILE),
            data_dir: PathBuf::from(DEFAULT_DATA_DIR),
            fixes: PathBuf::from(DEFAULT_FIXES_FILE),
        }
    }
}

/// What is running.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Running {
    /// The TUR build, from `/usr/lib/tur/release`.
    pub build_id: Option<String>,
    /// The TUR version.
    pub version: Option<String>,
    /// The kernel's ID of this boot.
    pub boot_id: Option<String>,
}

impl Running {
    /// Reads what is running from the machine.
    pub fn read(machine: &dyn Machine) -> Running {
        let release = machine
            .read_file(crate::identity::RELEASE_FILE)
            .map(|data| String::from_utf8_lossy(&data).into_owned())
            .unwrap_or_default();
        let value = |name: &str| {
            release.lines().find_map(|line| {
                let value = line.strip_prefix(name)?.strip_prefix('=')?;
                let value = value.trim().trim_matches('"');
                (!value.is_empty()).then(|| value.to_owned())
            })
        };
        let boot_id = machine
            .read_file("/proc/sys/kernel/random/boot_id")
            .ok()
            .map(|data| String::from_utf8_lossy(&data).trim().to_owned())
            .filter(|text| !text.is_empty());
        Running {
            build_id: value("TUR_BUILD_ID"),
            version: value("TUR_VERSION"),
            boot_id,
        }
    }
}

/// The image's list of the fault codes it fixes
/// (`schemas/heal-fixes.schema.json`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixes {
    /// 1.
    pub schema_version: u32,
    /// What the file is.
    #[serde(default)]
    pub about: String,
    /// One entry for each code a change in this image addresses.
    pub fixes: Vec<Fix>,
}

/// One fixed code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fix {
    /// The fault code.
    pub code: String,
    /// What was changed, in a line.
    pub summary: String,
    /// The change that fixed it, for example a pull request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<String>,
}

impl Fixes {
    /// Reads the list; a missing or unusable file lists nothing.
    pub fn load(path: &Path) -> Fixes {
        std::fs::read(path)
            .ok()
            .and_then(|text| serde_json::from_slice::<Fixes>(&text).ok())
            .filter(|fixes| fixes.schema_version == 1)
            .unwrap_or_default()
    }

    /// Whether `code` is listed.
    pub fn fixes(&self, code: &str) -> bool {
        self.fixes.iter().any(|fix| fix.code == code)
    }
}

/// What a scan did about one fault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanLine {
    /// The fault's code.
    pub code: String,
    /// Whether the check found it.
    pub present: bool,
    /// Why the check could not be made, if it could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    /// Where the fault stands afterwards, if it has a record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<FaultState>,
    /// What was done, in words.
    pub action: String,
}

/// The result of applying or undoing a remedy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    /// The fault's code.
    pub code: String,
    /// The remedy.
    pub remedy: String,
    /// `repaired`, `not-repaired`, `failed` or `undone`.
    pub outcome: String,
    /// What was done.
    pub detail: String,
    /// Where the fault stands afterwards.
    pub state: FaultState,
    /// How to undo it, in words.
    pub undo: String,
}

/// One remedy application, as asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyRequest {
    /// The fault's code.
    pub code: String,
    /// The remedy's id.
    pub remedy: String,
    /// Who asks.
    pub actor: Actor,
    /// The account that asks, for a request.
    pub uid: Option<u32>,
    /// Whether a class 2 remedy was confirmed by the person.
    pub confirmed: bool,
}

fn error(kind: ErrorKind, detail: impl Into<String>) -> DiagError {
    DiagError::new(kind, detail)
}

fn new_record(code: &str, finding: &Finding, running: &Running, now: &str) -> FaultRecord {
    FaultRecord {
        code: code.to_owned(),
        state: FaultState::Present,
        first_seen_utc: now.to_owned(),
        last_seen_utc: now.to_owned(),
        build_id: running.build_id.clone(),
        version: running.version.clone(),
        subjects: finding.subjects.clone(),
        evidence: finding.evidence.clone(),
        applied: Vec::new(),
        report_offered: false,
        automatic_suppressed: false,
        resolved_how: None,
        resolved_utc: None,
    }
}

/// How often `remedy` was applied to `record` in this boot, by `actor`.
fn applied_this_boot(record: &FaultRecord, remedy: &str, boot: Option<&str>, actor: Actor) -> u32 {
    let count = record
        .applied
        .iter()
        .filter(|applied| {
            applied.remedy == remedy && applied.by == actor && applied.boot_id.as_deref() == boot
        })
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Runs the check until it passes or `verify_seconds` are up.
fn verify(machine: &dyn Machine, fault: &Fault) -> Finding {
    let deadline = machine.now() + fault.verify_seconds;
    loop {
        let finding = run_check(machine, &fault.check);
        if !finding.present || machine.now() >= deadline {
            return finding;
        }
        machine.sleep(1);
    }
}

/// Offers a report if the fault's policy says so in its present state.
fn offer_report(state: &mut HealState, index: usize, fault: &Fault, now: &str) {
    let record = &state.faults[index];
    let offer = match fault.report {
        ReportPolicy::Never => false,
        ReportPolicy::Always => true,
        ReportPolicy::WhenUnrepaired => record.state == FaultState::NotRepaired,
    };
    if offer && record.state != FaultState::ReportOffered && record.state != FaultState::Repaired {
        let subjects = record.subjects.clone();
        let record = &mut state.faults[index];
        record.state = FaultState::ReportOffered;
        record.report_offered = true;
        state.event(now, &fault.code, EventKind::ReportOffered, &subjects, None, None);
    }
}

/// Applies `remedy` to the fault at `index`, verifies, records.
#[allow(clippy::too_many_arguments)]
fn perform(
    machine: &dyn Machine,
    paths: &Paths,
    state: &mut HealState,
    index: usize,
    fault: &Fault,
    remedy: &Remedy,
    finding: &Finding,
    actor: Actor,
    uid: Option<u32>,
    running: &Running,
) -> Outcome {
    let now = format_utc(machine.now());
    let id = format!("a{}", state.next_event.max(1));
    let (outcome, detail, undo, after) =
        match remedies::apply(machine, &paths.data_dir, fault, remedy, finding, &id) {
            Ok(applied) => {
                let after = verify(machine, fault);
                let outcome = if after.present { "not-repaired" } else { "repaired" };
                (outcome, applied.detail, applied.undo, Some(after))
            }
            Err(detail) => ("failed", detail, UndoData::NothingChanged, None),
        };
    let now_after = format_utc(machine.now());
    let record = &mut state.faults[index];
    record.applied.push(AppliedRecord {
        id,
        remedy: remedy.id.clone(),
        by: actor,
        uid,
        at_utc: now,
        boot_id: running.boot_id.clone(),
        outcome: outcome.to_owned(),
        detail: detail.clone(),
        undo,
        undone_utc: None,
    });
    if let Some(after) = &after
        && after.present
    {
        record.evidence = after.evidence.clone();
    }
    record.state = if outcome == "repaired" {
        FaultState::Repaired
    } else {
        FaultState::NotRepaired
    };
    let kind = if outcome == "repaired" {
        EventKind::Repaired
    } else {
        EventKind::NotRepaired
    };
    let subjects = record.subjects.clone();
    state.event(&now_after, &fault.code, kind, &subjects, Some(&remedy.id), Some(actor));
    offer_report(state, index, fault, &now_after);
    Outcome {
        code: fault.code.clone(),
        remedy: remedy.id.clone(),
        outcome: outcome.to_owned(),
        detail,
        state: state.faults[index].state,
        undo: remedy.undo.clone(),
    }
}

/// Runs every check of the catalogue once. With `repair`, a fault's
/// automatic remedy is applied, at most as often per boot as the catalogue
/// allows. Faults that cleared are resolved; one that cleared, or came back,
/// after an update whose `fixes` name its code is linked to that update.
pub fn scan(
    machine: &dyn Machine,
    catalogue: &Catalogue,
    paths: &Paths,
    state: &mut HealState,
    running: &Running,
    fixes: &Fixes,
    repair: bool,
) -> Vec<ScanLine> {
    state.build_id.clone_from(&running.build_id);
    state.version.clone_from(&running.version);
    state.boot_id.clone_from(&running.boot_id);
    let mut lines = Vec::new();
    for fault in &catalogue.faults {
        let finding = run_check(machine, &fault.check);
        let now = format_utc(machine.now());
        if let Some(reason) = &finding.unavailable {
            lines.push(ScanLine {
                code: fault.code.clone(),
                present: false,
                unavailable: Some(reason.clone()),
                state: state.active(&fault.code).map(|index| state.faults[index].state),
                action: "the check could not be made; nothing was changed".to_owned(),
            });
            continue;
        }
        let mut actions = Vec::new();
        if finding.present {
            let index = match state.active(&fault.code) {
                Some(index) => {
                    let record = &mut state.faults[index];
                    record.last_seen_utc.clone_from(&now);
                    record.subjects.clone_from(&finding.subjects);
                    record.evidence.clone_from(&finding.evidence);
                    if record.state == FaultState::Repaired {
                        record.state = FaultState::NotRepaired;
                        let subjects = record.subjects.clone();
                        state.event(&now, &fault.code, EventKind::NotRepaired, &subjects, None, None);
                        actions.push("it came back after a repair".to_owned());
                    }
                    let record = &state.faults[index];
                    if fixes.fixes(&fault.code) && record.build_id != running.build_id {
                        let subjects = record.subjects.clone();
                        let record = &mut state.faults[index];
                        record.build_id.clone_from(&running.build_id);
                        record.version.clone_from(&running.version);
                        if record.state == FaultState::ReportOffered {
                            record.state = FaultState::Present;
                        }
                        state.event(
                            &now,
                            &fault.code,
                            EventKind::RecurredAfterUpdate,
                            &subjects,
                            None,
                            None,
                        );
                        actions.push("it happened again after an update meant to fix it".to_owned());
                    }
                    index
                }
                None => {
                    state
                        .faults
                        .push(new_record(&fault.code, &finding, running, &now));
                    let index = state.faults.len() - 1;
                    let waits_for_person = !(repair && fault.automatic_remedy().is_some())
                        && !fault.remedies.is_empty();
                    if waits_for_person {
                        state.event(
                            &now,
                            &fault.code,
                            EventKind::Detected,
                            &finding.subjects,
                            None,
                            None,
                        );
                    }
                    actions.push("found".to_owned());
                    index
                }
            };
            if repair
                && let Some(remedy) = fault.automatic_remedy()
                && !state.faults[index].automatic_suppressed
                && matches!(
                    state.faults[index].state,
                    FaultState::Present | FaultState::NotRepaired
                )
                && applied_this_boot(
                    &state.faults[index],
                    &remedy.id,
                    running.boot_id.as_deref(),
                    Actor::Service,
                ) < remedy.max_per_boot.unwrap_or(1)
            {
                let outcome = perform(
                    machine,
                    paths,
                    state,
                    index,
                    fault,
                    remedy,
                    &finding,
                    Actor::Service,
                    None,
                    running,
                );
                actions.push(format!("{} applied: {}", remedy.id, outcome.outcome));
            }
            offer_report(state, index, fault, &now);
            lines.push(ScanLine {
                code: fault.code.clone(),
                present: true,
                unavailable: None,
                state: Some(state.faults[index].state),
                action: actions.join("; "),
            });
        } else {
            let mut line = ScanLine {
                code: fault.code.clone(),
                present: false,
                unavailable: None,
                state: None,
                action: "not present".to_owned(),
            };
            if let Some(index) = state.active(&fault.code) {
                let record = &state.faults[index];
                let (how, kind) = if record.state == FaultState::Repaired {
                    ("repair-held", EventKind::Held)
                } else if fixes.fixes(&fault.code) && record.build_id != running.build_id {
                    ("fixed-by-update", EventKind::FixedByUpdate)
                } else {
                    ("cleared", EventKind::Cleared)
                };
                let subjects = record.subjects.clone();
                let record = &mut state.faults[index];
                record.state = FaultState::Resolved;
                record.resolved_how = Some(how.to_owned());
                record.resolved_utc = Some(now.clone());
                state.event(&now, &fault.code, kind, &subjects, None, None);
                line.state = Some(FaultState::Resolved);
                line.action = format!("resolved: {how}");
            }
            lines.push(line);
        }
    }
    state.updated_utc = format_utc(machine.now());
    lines
}

/// Applies a remedy a person or a request asks for.
///
/// # Errors
///
/// The fault or the remedy is unknown; the remedy needs an approval the
/// request does not carry; it was tried too often in this boot; the check
/// cannot be made; or the fault is not present (nothing is changed).
pub fn apply(
    machine: &dyn Machine,
    catalogue: &Catalogue,
    paths: &Paths,
    state: &mut HealState,
    running: &Running,
    request: &ApplyRequest,
) -> Result<Outcome, DiagError> {
    let fault = catalogue
        .fault(&request.code)
        .ok_or_else(|| error(ErrorKind::HealUnknown, format!("no fault {}", request.code)))?;
    let remedy = fault.remedy(&request.remedy).ok_or_else(|| {
        error(
            ErrorKind::HealUnknown,
            format!("{} has no remedy {}", request.code, request.remedy),
        )
    })?;
    if remedy.class > 1 {
        if request.actor != Actor::Administrator {
            return Err(error(
                ErrorKind::HealNeedsApproval,
                format!(
                    "{} is class {}: only a person with administrator rights can apply it, \
                     at the command line, after confirming",
                    remedy.id, remedy.class
                ),
            ));
        }
        if !request.confirmed {
            return Err(error(
                ErrorKind::HealNotConfirmed,
                format!("{} was not confirmed; nothing was changed", remedy.id),
            ));
        }
    }
    let limit = if request.actor == Actor::Request {
        REQUESTS_PER_BOOT
    } else {
        u32::MAX
    };
    if let Some(index) = state.latest(&request.code)
        && applied_this_boot(
            &state.faults[index],
            &remedy.id,
            running.boot_id.as_deref(),
            request.actor,
        ) >= limit
    {
        return Err(error(
            ErrorKind::HealTooOften,
            format!("{} was requested {limit} times in this boot", remedy.id),
        ));
    }
    let finding = run_check(machine, &fault.check);
    if let Some(reason) = finding.unavailable {
        return Err(error(ErrorKind::HealCheckUnavailable, reason));
    }
    if !finding.present {
        return Err(error(
            ErrorKind::HealNotPresent,
            format!("{}: {}", request.code, finding.evidence.join("; ")),
        ));
    }
    let now = format_utc(machine.now());
    state.build_id.clone_from(&running.build_id);
    state.version.clone_from(&running.version);
    state.boot_id.clone_from(&running.boot_id);
    let index = match state.active(&request.code) {
        Some(index) => {
            let record = &mut state.faults[index];
            record.last_seen_utc.clone_from(&now);
            record.subjects.clone_from(&finding.subjects);
            record.evidence.clone_from(&finding.evidence);
            index
        }
        None => {
            state
                .faults
                .push(new_record(&request.code, &finding, running, &now));
            state.faults.len() - 1
        }
    };
    state.faults[index].automatic_suppressed = false;
    let outcome = perform(
        machine,
        paths,
        state,
        index,
        fault,
        remedy,
        &finding,
        request.actor,
        request.uid,
        running,
    );
    state.updated_utc = format_utc(machine.now());
    Ok(outcome)
}

/// Undoes the newest application of `remedy` to the fault `code` that has
/// not been undone.
///
/// # Errors
///
/// Unknown fault or remedy; nothing to undo; the undo failed.
#[allow(clippy::too_many_arguments)]
pub fn undo(
    machine: &dyn Machine,
    catalogue: &Catalogue,
    paths: &Paths,
    state: &mut HealState,
    code: &str,
    remedy_id: &str,
    actor: Actor,
    uid: Option<u32>,
) -> Result<Outcome, DiagError> {
    let fault = catalogue
        .fault(code)
        .ok_or_else(|| error(ErrorKind::HealUnknown, format!("no fault {code}")))?;
    let remedy = fault
        .remedy(remedy_id)
        .ok_or_else(|| error(ErrorKind::HealUnknown, format!("{code} has no remedy {remedy_id}")))?;
    if remedy.class > 1 && actor != Actor::Administrator {
        return Err(error(
            ErrorKind::HealNeedsApproval,
            format!("{remedy_id} is class {}", remedy.class),
        ));
    }
    let found = state.faults.iter().enumerate().rev().find_map(|(index, record)| {
        if record.code != code {
            return None;
        }
        record
            .applied
            .iter()
            .rposition(|applied| {
                applied.remedy == remedy_id
                    && applied.undone_utc.is_none()
                    && applied.outcome != "failed"
            })
            .map(|position| (index, position))
    });
    let Some((index, position)) = found else {
        return Err(error(
            ErrorKind::HealNothingToUndo,
            format!("{remedy_id} has not been applied to {code}, or was undone"),
        ));
    };
    let data = state.faults[index].applied[position].undo.clone();
    let detail = remedies::undo(machine, &paths.data_dir, &data)
        .map_err(|detail| error(ErrorKind::HealNothingToUndo, detail))?;
    let now = format_utc(machine.now());
    let record = &mut state.faults[index];
    record.applied[position].undone_utc = Some(now.clone());
    record.automatic_suppressed = true;
    record.state = FaultState::Present;
    record.resolved_how = None;
    record.resolved_utc = None;
    let subjects = record.subjects.clone();
    // A resolved record is active again; an older active one of the same
    // code cannot exist, as only one record of a code is ever unresolved.
    state.event(&now, code, EventKind::Undone, &subjects, Some(remedy_id), Some(actor));
    if let Some(uid) = uid {
        state.faults[index].applied[position].detail.push_str(&format!("; undone at the request of UID {uid}"));
    }
    state.updated_utc = now;
    Ok(Outcome {
        code: code.to_owned(),
        remedy: remedy_id.to_owned(),
        outcome: "undone".to_owned(),
        detail,
        state: FaultState::Present,
        undo: remedy.undo.clone(),
    })
}

/// A request through the healing service's socket: one JSON object on one
/// line.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// [`REQUEST_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// `heal_apply_remedy` or `heal_undo_remedy`.
    pub tool: String,
    /// The tool's input.
    pub arguments: RequestArguments,
}

/// The input of both request tools.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestArguments {
    /// The fault's code.
    pub code: String,
    /// The remedy's id.
    pub remedy: String,
}

/// The account at the other end of a connection, from the instance name
/// systemd gives a per-connection service of a Unix socket: the last of its
/// `-`-separated fields is the peer's UID.
pub fn uid_of_connection(connection: &str) -> Option<u32> {
    connection.rsplit('-').next()?.parse().ok()
}

/// The answer to a request that was refused.
fn refusal(problem: &DiagError) -> Value {
    json!({
        "schema_version": REQUEST_SCHEMA_VERSION,
        "ok": false,
        "error": {
            "code": problem.code().to_string(),
            "message": problem.user_message(),
            "detail": problem.detail(),
        }
    })
}

/// Handles one request read from a connection to the service's socket, and
/// returns the answer. Only class 0 and class 1 remedies of the catalogue
/// can be applied or undone this way: the socket gives anyone on the
/// computer at most what the service itself would do.
pub fn handle_request(
    machine: &dyn Machine,
    catalogue: &Catalogue,
    paths: &Paths,
    input: &[u8],
    connection: Option<&str>,
) -> Value {
    if input.len() > MAX_REQUEST_BYTES {
        return refusal(&error(ErrorKind::HealRequestInvalid, "the request is too long"));
    }
    let request: Request = match serde_json::from_slice(input) {
        Ok(request) => request,
        Err(problem) => {
            return refusal(&error(ErrorKind::HealRequestInvalid, problem.to_string()));
        }
    };
    if request.schema_version != REQUEST_SCHEMA_VERSION {
        return refusal(&error(
            ErrorKind::HealRequestInvalid,
            format!("schema_version {} is not {REQUEST_SCHEMA_VERSION}", request.schema_version),
        ));
    }
    let uid = connection.and_then(uid_of_connection);
    let running = Running::read(machine);
    let _lock = match Lock::take(&paths.data_dir) {
        Ok(lock) => lock,
        Err(problem) => return refusal(&problem),
    };
    let mut state = match HealState::load(&paths.state) {
        Ok(state) => state,
        Err(problem) => return refusal(&problem),
    };
    let arguments = &request.arguments;
    let result = match request.tool.as_str() {
        "heal_apply_remedy" => apply(
            machine,
            catalogue,
            paths,
            &mut state,
            &running,
            &ApplyRequest {
                code: arguments.code.clone(),
                remedy: arguments.remedy.clone(),
                actor: Actor::Request,
                uid,
                confirmed: false,
            },
        ),
        "heal_undo_remedy" => undo(
            machine,
            catalogue,
            paths,
            &mut state,
            &arguments.code,
            &arguments.remedy,
            Actor::Request,
            uid,
        ),
        other => Err(error(
            ErrorKind::HealRequestInvalid,
            format!("{other:?} is not a request this service answers"),
        )),
    };
    match result {
        Ok(outcome) => {
            if let Err(problem) = state.save(&paths.state) {
                return refusal(&problem);
            }
            json!({
                "schema_version": REQUEST_SCHEMA_VERSION,
                "ok": outcome.outcome == "repaired" || outcome.outcome == "undone",
                "tool": request.tool,
                "result": outcome,
            })
        }
        Err(problem) => refusal(&problem),
    }
}

/// What `tur-diag heal status` shows: the faults that are not resolved,
/// with what may be done about each, and the recent events.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusView {
    /// The state file it was read from.
    pub state_file: String,
    /// When the state was last written.
    pub updated_utc: String,
    /// The faults that are not resolved.
    pub faults: Vec<FaultView>,
    /// The newest resolved faults, newest first, at most ten.
    pub resolved: Vec<FaultRecord>,
}

/// One fault in [`StatusView`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FaultView {
    /// The fault's record.
    #[serde(flatten)]
    pub record: FaultRecord,
    /// Its title.
    pub title: String,
    /// Its explanation.
    pub explanation: String,
    /// The remedies, as the catalogue describes them.
    pub remedies: Vec<Remedy>,
}

/// Builds the status view of `state`.
pub fn status_view(catalogue: &Catalogue, state: &HealState, state_file: &Path) -> StatusView {
    let faults = state
        .faults
        .iter()
        .filter(|record| record.state != FaultState::Resolved)
        .map(|record| {
            let fault = catalogue.fault(&record.code);
            FaultView {
                record: record.clone(),
                title: fault.map(|fault| fault.title.clone()).unwrap_or_default(),
                explanation: fault
                    .map(|fault| fault.explanation.clone())
                    .unwrap_or_default(),
                remedies: fault.map(|fault| fault.remedies.clone()).unwrap_or_default(),
            }
        })
        .collect();
    let resolved = state
        .faults
        .iter()
        .rev()
        .filter(|record| record.state == FaultState::Resolved)
        .take(10)
        .cloned()
        .collect();
    StatusView {
        state_file: state_file.display().to_string(),
        updated_utc: state.updated_utc.clone(),
        faults,
        resolved,
    }
}

/// One notice for the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    /// The event it is made from.
    pub id: u64,
    /// When.
    pub at_utc: String,
    /// The fault's code.
    pub code: String,
    /// What happened (the event's kind).
    pub kind: EventKind,
    /// The heading: the fault's title.
    pub title: String,
    /// The text.
    pub body: String,
    /// Commands the person can run about it.
    pub actions: Vec<String>,
}

fn fill(text: &str, subjects: &[String]) -> String {
    let subjects = if subjects.is_empty() {
        "Something".to_owned()
    } else {
        subjects.join(", ")
    };
    text.replace("{subjects}", &subjects)
}

/// The notices of `state` not in `shown`, oldest first. `reported` holds
/// the codes of reports this person sent, so that a fix can say "the
/// problem you reported".
pub fn notices(
    catalogue: &Catalogue,
    state: &HealState,
    shown: &BTreeSet<u64>,
    reported: &BTreeSet<String>,
) -> Vec<Notice> {
    let mut found = Vec::new();
    for event in &state.events {
        if !event.kind.is_notice() || shown.contains(&event.seq) {
            continue;
        }
        let Some(fault) = catalogue.fault(&event.code) else {
            continue;
        };
        let text = &fault.notice;
        let version = event.version.as_deref().unwrap_or("the running version");
        let offered: Vec<&Remedy> = fault
            .remedies
            .iter()
            .filter(|remedy| !remedy.automatic)
            .collect();
        let mut actions = Vec::new();
        let body = match event.kind {
            EventKind::Detected => {
                for remedy in &offered {
                    actions.push(format!("tur-diag heal apply {} {}", fault.code, remedy.id));
                }
                fill(&text.found, &event.subjects)
            }
            EventKind::Repaired => {
                if let Some(remedy) = event.remedy.as_deref().and_then(|id| fault.remedy(id))
                    && remedy.reversible
                    && !remedy.automatic
                {
                    actions.push(format!("tur-diag heal undo {} {}", fault.code, remedy.id));
                }
                fill(text.repaired.as_deref().unwrap_or(&text.found), &event.subjects)
            }
            EventKind::NotRepaired => fill(
                text.unrepaired.as_deref().unwrap_or(&text.found),
                &event.subjects,
            ),
            EventKind::ReportOffered => {
                actions.push(format!("tur-diag heal report {}", fault.code));
                fill(text.report.as_deref().unwrap_or(&text.found), &event.subjects)
            }
            EventKind::FixedByUpdate => {
                if reported.contains(&fault.code) {
                    format!(
                        "The problem you reported is fixed: \"{}\" has not happened again since \
                         TUR was updated to {version}, which addresses it.",
                        fault.title
                    )
                } else {
                    format!(
                        "\"{}\" has not happened again since TUR was updated to {version}, \
                         which addresses it.",
                        fault.title
                    )
                }
            }
            EventKind::RecurredAfterUpdate => {
                actions.push(format!("tur-diag heal report {}", fault.code));
                format!(
                    "TUR {version} was meant to fix \"{}\", but it happened again. You can send \
                     a new report so that TUR's developers know.",
                    fault.title
                )
            }
            EventKind::Undone => format!(
                "The change made for \"{}\" was undone, as asked.",
                fault.title
            ),
            EventKind::Held | EventKind::Cleared => continue,
        };
        found.push(Notice {
            id: event.seq,
            at_utc: event.at_utc.clone(),
            code: event.code.clone(),
            kind: event.kind,
            title: fault.title.clone(),
            body,
            actions,
        });
    }
    found
}
