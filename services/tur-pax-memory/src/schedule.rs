// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Reminders and scheduled tasks, their standing approvals and their run
//! records.
//!
//! # A proposal is not a schedule
//!
//! Addendum A sections 5.6, 16.2 and 17: the assistant proposes a schedule;
//! creating, changing or approving one is the user's act. The two are
//! different types here.
//!
//! - A [`Proposal`] is plain data. The assistant, or anything else, can build
//!   one. It fires nothing and approves nothing.
//! - A [`Schedule`] has no public constructor and cannot be deserialised from
//!   outside this crate. One comes into being in two ways only:
//!   [`ScheduleBook::create`] or [`ScheduleBook::change`], which take a
//!   proposal **and** a [`UserConfirmation`] that covers exactly that
//!   proposal; or reading a schedule file back from the store.
//!
//! This library cannot produce a [`UserConfirmation`] (see
//! [`crate::confirm`]), so no code path in it turns a proposal into a
//! schedule on its own. The second way, the store, is as trustworthy as the
//! directory: addendum A section 17 item 5 requires that the assistant's own
//! process cannot write it. This crate does not enforce file ownership.
//!
//! A reminder notifies and performs no action, so it cannot carry a standing
//! approval: the approvals are a field of the task variant only.
//!
//! # Runs
//!
//! Each firing leaves a [`RunRecord`]: what ran, what was deferred and why,
//! and the outcome. Deferred actions stay listed by
//! [`ScheduleBook::unpresented_deferrals`] until
//! [`ScheduleBook::mark_presented`] records that the user has been shown them.
//! The time of a schedule's last run is read from its run records, so the
//! schedule file itself is never rewritten by a run.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::approval::{ApprovalRequest, MAX_STANDING_APPROVALS, StandingApproval};
use crate::codes::{Code, Problem};
use crate::confirm::UserConfirmation;
use crate::entry::{check_text, is_service_name, is_tool_name};
use crate::ids::{RunId, ScheduleId};
use crate::json::{describe_error, to_document};
use crate::memory::DamagedFile;
use crate::secret;
use crate::store::{Collection, STATE_ID, Store, StoreError, is_oversized};
use crate::time::Timestamp;
use crate::trigger::{MissedRunPolicy, NextRun, SystemEvent, Trigger, next_run};

/// The schema version of a stored schedule.
pub const SCHEDULE_SCHEMA_VERSION: u32 = 1;
/// The schema version of a stored run record.
pub const RUN_SCHEMA_VERSION: u32 = 1;
/// The schema version of the schedule state file.
pub const SCHEDULE_STATE_SCHEMA_VERSION: u32 = 1;
/// The largest number of schedules. A proposal beyond it is refused, so a
/// runaway or hostile series of proposals cannot create timers without end.
pub const MAX_SCHEDULES: usize = 64;
/// The longest schedule name, in bytes.
pub const MAX_NAME_BYTES: usize = 80;
/// The longest reminder message or task request, in bytes.
pub const MAX_REQUEST_BYTES: usize = 2000;
/// How many run records are kept for one schedule. Older ones are removed.
pub const MAX_RUNS_KEPT: usize = 50;
/// The largest number of actions, performed and deferred together, in one run
/// record.
pub const MAX_RUN_ACTIONS: usize = 200;
/// The longest free-text detail in a run record, in bytes.
pub const MAX_DETAIL_BYTES: usize = 500;

/// What a proposed schedule would do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProposedAction {
    /// Notify the user. Performs no action.
    Reminder {
        /// The text of the notification.
        message: String,
    },
    /// Run a request, unattended.
    Task {
        /// The request, in the user's words.
        request: String,
        /// The class 2 calls the task will need, for the user to approve.
        #[serde(default)]
        approvals: Vec<ApprovalRequest>,
    },
}

/// A schedule as the assistant proposes it. It has no effect until the user
/// creates a schedule from it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// A short name, shown in settings and in the timer's description.
    pub name: String,
    /// What it would do.
    pub action: ProposedAction,
    /// When it would fire.
    pub trigger: Trigger,
    /// What happens to occurrences missed while the machine was off.
    pub missed_runs: MissedRunPolicy,
    /// The task during which the assistant proposed it, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_by_task: Option<String>,
}

fn invalid_proposal(reason: String) -> Problem {
    Problem::new(Code::ScheduleInvalidProposal, reason)
}

impl Proposal {
    // The limits on shape and size, without the secret check.
    fn check_shape(&self) -> Result<(), Problem> {
        check_text("name", &self.name, MAX_NAME_BYTES, false).map_err(invalid_proposal)?;
        match &self.action {
            ProposedAction::Reminder { message } => {
                check_text("message", message, MAX_REQUEST_BYTES, true)
                    .map_err(invalid_proposal)?;
            }
            ProposedAction::Task { request, approvals } => {
                check_text("request", request, MAX_REQUEST_BYTES, true)
                    .map_err(invalid_proposal)?;
                if approvals.len() > MAX_STANDING_APPROVALS {
                    return Err(invalid_proposal(format!(
                        "more than {MAX_STANDING_APPROVALS} approvals"
                    )));
                }
                for approval in approvals {
                    approval.validate()?;
                }
            }
        }
        if let Some(task) = &self.proposed_by_task {
            check_text("task", task, crate::entry::MAX_TASK_BYTES, false)
                .map_err(invalid_proposal)?;
        }
        self.trigger.validate()
    }

    /// Check the proposal: the limits on its texts and approvals, the trigger
    /// (including the frequency cap), and that nothing in it looks like a
    /// secret.
    pub fn validate(&self) -> Result<(), Problem> {
        self.check_shape()?;
        let refused = |kind: secret::SecretKind, field: &str| {
            Problem::new(
                Code::ScheduleSecret,
                format!(
                    "the {field} looks like it holds {}; nothing was stored",
                    kind.describe()
                ),
            )
        };
        if let Some(kind) = secret::detect(&self.name) {
            return Err(refused(kind, "name"));
        }
        match &self.action {
            ProposedAction::Reminder { message } => {
                if let Some(kind) = secret::detect(message) {
                    return Err(refused(kind, "message"));
                }
            }
            ProposedAction::Task { request, approvals } => {
                if let Some(kind) = secret::detect(request) {
                    return Err(refused(kind, "request"));
                }
                for approval in approvals {
                    let arguments = Value::Object(approval.arguments.clone());
                    if let Some(kind) = secret::detect_in_json(&arguments) {
                        return Err(refused(kind, "arguments of an approval"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// What a schedule does.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduledAction {
    /// Notify the user. Performs no action and has no standing approvals.
    Reminder {
        /// The text of the notification.
        message: String,
    },
    /// Run a request, unattended.
    Task {
        /// The request.
        request: String,
        /// The calls the user approved in advance for this schedule.
        standing_approvals: Vec<StandingApproval>,
    },
}

/// A reminder or scheduled task the user created.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(into = "RawSchedule")]
pub struct Schedule {
    id: ScheduleId,
    name: String,
    action: ScheduledAction,
    trigger: Trigger,
    missed_runs: MissedRunPolicy,
    created_at: Timestamp,
    confirmed_at: Timestamp,
    proposed_by_task: Option<String>,
}

impl Schedule {
    // Put a schedule together from a checked proposal and its two times.
    fn assemble(
        id: ScheduleId,
        proposal: Proposal,
        created_at: Timestamp,
        confirmed_at: Timestamp,
    ) -> Result<Schedule, Problem> {
        if !created_at.is_supported() || !confirmed_at.is_supported() || confirmed_at < created_at
        {
            return Err(invalid_proposal(
                "the creation and confirmation times are not in order".to_owned(),
            ));
        }
        let action = match proposal.action {
            ProposedAction::Reminder { message } => ScheduledAction::Reminder { message },
            ProposedAction::Task { request, approvals } => ScheduledAction::Task {
                request,
                standing_approvals: approvals
                    .into_iter()
                    .map(StandingApproval::granted)
                    .collect::<Result<Vec<_>, _>>()?,
            },
        };
        Ok(Schedule {
            id,
            name: proposal.name,
            action,
            trigger: proposal.trigger,
            missed_runs: proposal.missed_runs,
            created_at,
            confirmed_at,
            proposed_by_task: proposal.proposed_by_task,
        })
    }

    // The only way from a proposal to a schedule: with the user's
    // confirmation of exactly this proposal.
    fn from_confirmed(
        id: ScheduleId,
        proposal: Proposal,
        confirmation: &dyn UserConfirmation<Proposal>,
        created_at: Option<Timestamp>,
    ) -> Result<Schedule, Problem> {
        proposal.validate()?;
        if !confirmation.covers(&proposal) {
            return Err(Problem::new(
                Code::ScheduleNotConfirmed,
                "the confirmation does not cover this proposal",
            ));
        }
        let confirmed_at = confirmation.confirmed_at();
        if let Trigger::Once { at } = &proposal.trigger
            && *at <= confirmed_at
        {
            return Err(Problem::new(
                Code::ScheduleInvalidTrigger,
                "the time of the trigger has already passed",
            ));
        }
        Schedule::assemble(
            id,
            proposal,
            created_at.unwrap_or(confirmed_at),
            confirmed_at,
        )
    }

    // Read a schedule from the bytes of its file. The reason for a failure
    // never quotes the file.
    fn from_json(bytes: &[u8]) -> Result<Schedule, String> {
        let raw: RawSchedule = serde_json::from_slice(bytes).map_err(|err| describe_error(&err))?;
        if raw.schema_version != SCHEDULE_SCHEMA_VERSION {
            return Err("unsupported schema version".to_owned());
        }
        let action = match raw.action {
            RawAction::Reminder { message } => ProposedAction::Reminder { message },
            RawAction::Task {
                request,
                standing_approvals,
            } => ProposedAction::Task {
                request,
                approvals: standing_approvals,
            },
        };
        let proposal = Proposal {
            name: raw.name,
            action,
            trigger: raw.trigger,
            missed_runs: raw.missed_runs,
            proposed_by_task: raw.proposed_by_task,
        };
        proposal.check_shape().map_err(|problem| problem.detail)?;
        Schedule::assemble(raw.id, proposal, raw.created_at, raw.confirmed_at)
            .map_err(|problem| problem.detail)
    }

    /// The identifier.
    pub fn id(&self) -> &ScheduleId {
        &self.id
    }

    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the schedule does.
    pub fn action(&self) -> &ScheduledAction {
        &self.action
    }

    /// Whether the schedule is a reminder, which performs no action.
    pub fn is_reminder(&self) -> bool {
        matches!(self.action, ScheduledAction::Reminder { .. })
    }

    /// When the schedule fires.
    pub fn trigger(&self) -> &Trigger {
        &self.trigger
    }

    /// What happens to missed occurrences.
    pub fn missed_runs(&self) -> MissedRunPolicy {
        self.missed_runs
    }

    /// When the user first created the schedule.
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// When the user created or last changed the schedule.
    pub fn confirmed_at(&self) -> Timestamp {
        self.confirmed_at
    }

    /// The task during which the assistant proposed the schedule, if any.
    pub fn proposed_by_task(&self) -> Option<&str> {
        self.proposed_by_task.as_deref()
    }

    /// The standing approvals. Always empty for a reminder.
    pub fn standing_approvals(&self) -> &[StandingApproval] {
        match &self.action {
            ScheduledAction::Reminder { .. } => &[],
            ScheduledAction::Task {
                standing_approvals, ..
            } => standing_approvals,
        }
    }

    /// The standing approval that covers exactly this call, if there is one.
    pub fn approval_for(
        &self,
        service: &str,
        tool: &str,
        arguments: &Map<String, Value>,
    ) -> Option<&StandingApproval> {
        self.standing_approvals()
            .iter()
            .find(|approval| approval.matches(service, tool, arguments))
    }

    /// Why a call that has no matching approval is deferred: the schedule has
    /// an approval for the tool but with other arguments, or none for the tool
    /// at all. For the record shown to the user; it allows nothing.
    pub fn why_deferred(&self, service: &str, tool: &str) -> DeferReason {
        if self
            .standing_approvals()
            .iter()
            .any(|approval| approval.names(service, tool))
        {
            DeferReason::ArgumentsDiffer
        } else {
            DeferReason::NoStandingApproval
        }
    }

    /// What the schedule is to do at `now`, given the time of its last run.
    /// See [`crate::trigger::next_run`].
    pub fn next_run(&self, last_run: Option<Timestamp>, now: Timestamp) -> Result<NextRun, Problem> {
        let since = last_run.map_or(self.confirmed_at, |last| last.max(self.confirmed_at));
        next_run(&self.trigger, self.missed_runs, since, now)
    }
}

// The stored form of a schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSchedule {
    schema_version: u32,
    id: ScheduleId,
    name: String,
    action: RawAction,
    trigger: Trigger,
    missed_runs: MissedRunPolicy,
    created_at: Timestamp,
    confirmed_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proposed_by_task: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum RawAction {
    Reminder {
        message: String,
    },
    Task {
        request: String,
        #[serde(default)]
        standing_approvals: Vec<ApprovalRequest>,
    },
}

impl From<Schedule> for RawSchedule {
    fn from(schedule: Schedule) -> RawSchedule {
        let action = match schedule.action {
            ScheduledAction::Reminder { message } => RawAction::Reminder { message },
            ScheduledAction::Task {
                request,
                standing_approvals,
            } => RawAction::Task {
                request,
                standing_approvals: standing_approvals
                    .iter()
                    .map(StandingApproval::to_request)
                    .collect(),
            },
        };
        RawSchedule {
            schema_version: SCHEDULE_SCHEMA_VERSION,
            id: schedule.id,
            name: schedule.name,
            action,
            trigger: schedule.trigger,
            missed_runs: schedule.missed_runs,
            created_at: schedule.created_at,
            confirmed_at: schedule.confirmed_at,
            proposed_by_task: schedule.proposed_by_task,
        }
    }
}

/// A change to an existing schedule that is not a new proposal. The user's
/// act, like every change.
#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleEdit {
    /// Remove the schedule and its run records.
    Remove {
        /// The schedule.
        id: ScheduleId,
    },
    /// Revoke one standing approval, named by the exact call it covers.
    RevokeApproval {
        /// The schedule.
        id: ScheduleId,
        /// The service of the approval.
        service: String,
        /// The tool of the approval.
        tool: String,
        /// The arguments of the approval.
        arguments: Map<String, Value>,
    },
}

/// One tool call in a run record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRecord {
    /// The D-Bus name of the service that owns the tool.
    pub service: String,
    /// The tool name.
    pub tool: String,
    /// The arguments of the call. Empty if they were withheld.
    pub arguments: Map<String, Value>,
    /// Whether the arguments were left out because they looked like they held
    /// a secret.
    #[serde(default)]
    pub arguments_withheld: bool,
}

/// Why an action was deferred instead of run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferReason {
    /// The schedule has no standing approval for the tool.
    NoStandingApproval,
    /// The schedule has a standing approval for the tool, but for other
    /// arguments.
    ArgumentsDiffer,
    /// The user's policy asks before every action, and nobody was there to
    /// ask.
    AskEveryAction,
    /// The action was not a tool call, and only tool calls can be approved in
    /// advance.
    NotAToolCall,
    /// Another reason, given in the detail.
    Other,
}

/// An action an unattended run needed and had no approval for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeferredAction {
    /// The call that was not made.
    pub action: ActionRecord,
    /// Why it was deferred.
    pub reason: DeferReason,
    /// More about the reason, for the user.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    /// The run finished and nothing was deferred.
    Completed,
    /// The run stopped because it needed something it had no approval for.
    Deferred,
    /// The run failed.
    Failed,
    /// An occurrence was missed and, by the schedule's policy, not run.
    SkippedMissed,
}

/// What the orchestrator reports about one firing.
#[derive(Debug, Clone, PartialEq)]
pub struct NewRun {
    /// The schedule that fired.
    pub schedule: ScheduleId,
    /// The occurrence the run was for, for a time trigger.
    pub scheduled_for: Option<Timestamp>,
    /// The event that fired it, for an event trigger.
    pub event: Option<SystemEvent>,
    /// When the run started.
    pub started_at: Timestamp,
    /// How it ended.
    pub outcome: RunOutcome,
    /// The calls that were made.
    pub performed: Vec<ActionRecord>,
    /// The calls that were deferred.
    pub deferred: Vec<DeferredAction>,
    /// A short account of the outcome, for the user.
    pub detail: String,
}

/// The record of one firing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    /// The schema version, 1.
    pub schema_version: u32,
    /// The identifier of the record.
    pub id: RunId,
    /// The schedule that fired.
    pub schedule: ScheduleId,
    /// The occurrence the run was for, for a time trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<Timestamp>,
    /// The event that fired it, for an event trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<SystemEvent>,
    /// When the run started.
    pub started_at: Timestamp,
    /// When the run was recorded as finished.
    pub finished_at: Timestamp,
    /// How it ended.
    pub outcome: RunOutcome,
    /// The calls that were made.
    #[serde(default)]
    pub performed: Vec<ActionRecord>,
    /// The calls that were deferred, for the user to see on return.
    #[serde(default)]
    pub deferred: Vec<DeferredAction>,
    /// A short account of the outcome.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// When the user was shown the record, if they have been.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presented_at: Option<Timestamp>,
}

/// The schedules of a store, and the files that could not be read.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleListing {
    /// The readable schedules, by identifier.
    pub schedules: Vec<Schedule>,
    /// The damaged files.
    pub damaged: Vec<DamagedFile>,
}

/// The run records of a store, and the files that could not be read.
#[derive(Debug, Clone, PartialEq)]
pub struct RunListing {
    /// The readable records, oldest first.
    pub runs: Vec<RunRecord>,
    /// The damaged files.
    pub damaged: Vec<DamagedFile>,
}

// The schedule state file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleState {
    schema_version: u32,
    next_schedule: u64,
    next_run: u64,
}

impl Default for ScheduleState {
    fn default() -> Self {
        ScheduleState {
            schema_version: SCHEDULE_STATE_SCHEMA_VERSION,
            next_schedule: 1,
            next_run: 1,
        }
    }
}

fn store_problem(err: StoreError) -> Problem {
    Problem::new(Code::ScheduleStore, err.detail)
}

fn invalid_run(detail: impl Into<String>) -> Problem {
    Problem::new(Code::ScheduleInvalidRun, detail)
}

const WITHHELD: &str = "(withheld: it looked like it held a secret)";

// A record of a call with anything secret-looking taken out.
fn cleaned(mut action: ActionRecord) -> Result<ActionRecord, Problem> {
    if !is_service_name(&action.service) || !is_tool_name(&action.tool) {
        return Err(invalid_run("an action does not name a service and a tool"));
    }
    let arguments = Value::Object(std::mem::take(&mut action.arguments));
    if secret::detect_in_json(&arguments).is_some() {
        action.arguments_withheld = true;
    } else if let Value::Object(map) = arguments {
        action.arguments = map;
    }
    Ok(action)
}

fn cleaned_detail(detail: String) -> Result<String, Problem> {
    if detail.len() > MAX_DETAIL_BYTES {
        return Err(invalid_run("a detail is too long"));
    }
    if detail.chars().any(|c| c.is_control() && c != '\n') {
        return Err(invalid_run("a detail contains a control character"));
    }
    if secret::detect(&detail).is_some() {
        Ok(WITHHELD.to_owned())
    } else {
        Ok(detail)
    }
}

// The sequence number in an identifier, for ordering.
fn sequence_of(id: &str) -> u64 {
    id.split_once('-')
        .and_then(|(_, digits)| u64::from_str_radix(digits, 16).ok())
        .unwrap_or(u64::MAX)
}

/// The schedules of one user, kept in a [`Store`].
#[derive(Debug, Clone)]
pub struct ScheduleBook<S: Store> {
    store: S,
}

impl<S: Store> ScheduleBook<S> {
    /// The schedules kept in `store`.
    pub fn new(store: S) -> Self {
        ScheduleBook { store }
    }

    /// The store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Give the store back.
    pub fn into_store(self) -> S {
        self.store
    }

    fn read_state(&self) -> Result<ScheduleState, Problem> {
        let bytes = self
            .store
            .get(Collection::ScheduleState, STATE_ID)
            .map_err(store_problem)?;
        let Some(bytes) = bytes else {
            return Ok(ScheduleState::default());
        };
        let damaged = |reason: String| {
            Problem::new(
                Code::ScheduleDamaged,
                format!("the schedule state file is damaged: {reason}"),
            )
        };
        if is_oversized(&bytes) {
            return Err(damaged("the file is larger than the limit".to_owned()));
        }
        let state: ScheduleState =
            serde_json::from_slice(&bytes).map_err(|err| damaged(describe_error(&err)))?;
        if state.schema_version != SCHEDULE_STATE_SCHEMA_VERSION {
            return Err(damaged("unsupported schema version".to_owned()));
        }
        Ok(state)
    }

    fn write_state(&mut self, state: &ScheduleState) -> Result<(), Problem> {
        let bytes =
            to_document(state).map_err(|reason| Problem::new(Code::ScheduleStore, reason))?;
        self.store
            .put(Collection::ScheduleState, STATE_ID, &bytes)
            .map_err(store_problem)
    }

    fn write_schedule(&mut self, schedule: &Schedule) -> Result<(), Problem> {
        let bytes =
            to_document(schedule).map_err(|reason| Problem::new(Code::ScheduleStore, reason))?;
        self.store
            .put(Collection::Schedules, schedule.id().as_str(), &bytes)
            .map_err(store_problem)
    }

    fn write_run(&mut self, run: &RunRecord) -> Result<(), Problem> {
        let bytes = to_document(run).map_err(|reason| Problem::new(Code::ScheduleStore, reason))?;
        self.store
            .put(Collection::Runs, run.id.as_str(), &bytes)
            .map_err(store_problem)
    }

    fn sweep(&mut self) -> Result<(), Problem> {
        for collection in [
            Collection::ScheduleState,
            Collection::Schedules,
            Collection::Runs,
        ] {
            self.store.sweep(collection).map_err(store_problem)?;
        }
        Ok(())
    }

    // One schedule by the name of its file. `Ok(None)` if there is no such
    // file; a `ScheduleDamaged` problem if it cannot be read as a schedule.
    fn read_schedule(&self, id: &str) -> Result<Option<Schedule>, Problem> {
        let bytes = self
            .store
            .get(Collection::Schedules, id)
            .map_err(store_problem)?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        let damaged = |reason: String| Problem::new(Code::ScheduleDamaged, reason);
        if is_oversized(&bytes) {
            return Err(damaged("the file is larger than the limit".to_owned()));
        }
        let schedule = Schedule::from_json(&bytes).map_err(damaged)?;
        if schedule.id().as_str() != id {
            return Err(damaged(
                "the identifier inside the file is not the one it is named after".to_owned(),
            ));
        }
        Ok(Some(schedule))
    }

    fn read_run(&self, id: &str) -> Result<Option<RunRecord>, Problem> {
        let bytes = self.store.get(Collection::Runs, id).map_err(store_problem)?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        let damaged = |reason: String| Problem::new(Code::ScheduleDamaged, reason);
        if is_oversized(&bytes) {
            return Err(damaged("the file is larger than the limit".to_owned()));
        }
        let run: RunRecord =
            serde_json::from_slice(&bytes).map_err(|err| damaged(describe_error(&err)))?;
        if run.schema_version != RUN_SCHEMA_VERSION {
            return Err(damaged("unsupported schema version".to_owned()));
        }
        if run.id.as_str() != id {
            return Err(damaged(
                "the identifier inside the file is not the one it is named after".to_owned(),
            ));
        }
        Ok(Some(run))
    }

    /// Create a schedule from a proposal. The user's act: the confirmation
    /// must cover exactly this proposal.
    ///
    /// Refused if the proposal is not valid, if it would fire more often than
    /// the limit, if [`MAX_SCHEDULES`] schedules already exist, or if anything
    /// in it looks like a secret.
    pub fn create(
        &mut self,
        proposal: Proposal,
        confirmation: &dyn UserConfirmation<Proposal>,
    ) -> Result<Schedule, Problem> {
        let mut state = self.read_state()?;
        let existing = self
            .store
            .ids(Collection::Schedules)
            .map_err(store_problem)?;
        if existing.len() >= MAX_SCHEDULES {
            return Err(Problem::new(
                Code::ScheduleLimit,
                format!("{MAX_SCHEDULES} schedules already exist; remove one first"),
            ));
        }
        let mut sequence = state.next_schedule;
        let mut id = ScheduleId::from_sequence(sequence);
        while existing.iter().any(|known| known == id.as_str()) {
            sequence = sequence.saturating_add(1);
            id = ScheduleId::from_sequence(sequence);
        }
        let schedule = Schedule::from_confirmed(id, proposal, confirmation, None)?;
        self.sweep()?;
        state.next_schedule = sequence.saturating_add(1);
        self.write_state(&state)?;
        self.write_schedule(&schedule)?;
        Ok(schedule)
    }

    /// Replace a schedule's definition with a new proposal. The user's act:
    /// the confirmation must cover exactly the new proposal. The identifier
    /// and the creation time are kept; the standing approvals are those of the
    /// new proposal and no others.
    pub fn change(
        &mut self,
        id: &ScheduleId,
        proposal: Proposal,
        confirmation: &dyn UserConfirmation<Proposal>,
    ) -> Result<Schedule, Problem> {
        let existing = self.get(id)?;
        let schedule = Schedule::from_confirmed(
            id.clone(),
            proposal,
            confirmation,
            Some(existing.created_at()),
        )?;
        self.sweep()?;
        self.write_schedule(&schedule)?;
        Ok(schedule)
    }

    /// Remove a schedule or revoke one of its standing approvals. The user's
    /// act: the confirmation must cover exactly this edit.
    pub fn apply(
        &mut self,
        edit: &ScheduleEdit,
        confirmation: &dyn UserConfirmation<ScheduleEdit>,
    ) -> Result<(), Problem> {
        if !confirmation.covers(edit) {
            return Err(Problem::new(
                Code::ScheduleNotConfirmed,
                "the confirmation does not cover this change",
            ));
        }
        match edit {
            ScheduleEdit::Remove { id } => {
                self.sweep()?;
                let existed = self
                    .store
                    .remove(Collection::Schedules, id.as_str())
                    .map_err(store_problem)?;
                if !existed {
                    return Err(Problem::new(
                        Code::ScheduleNotFound,
                        format!("there is no schedule {id}"),
                    ));
                }
                for run in self.runs(Some(id))?.runs {
                    self.store
                        .remove(Collection::Runs, run.id.as_str())
                        .map_err(store_problem)?;
                }
                Ok(())
            }
            ScheduleEdit::RevokeApproval {
                id,
                service,
                tool,
                arguments,
            } => {
                let mut schedule = self.get(id)?;
                let ScheduledAction::Task {
                    standing_approvals, ..
                } = &mut schedule.action
                else {
                    return Err(Problem::new(
                        Code::ScheduleNotFound,
                        format!("{id} is a reminder and has no standing approvals"),
                    ));
                };
                let before = standing_approvals.len();
                standing_approvals.retain(|approval| !approval.matches(service, tool, arguments));
                if standing_approvals.len() == before {
                    return Err(Problem::new(
                        Code::ScheduleNotFound,
                        format!("{id} has no standing approval for exactly that call"),
                    ));
                }
                self.sweep()?;
                self.write_schedule(&schedule)
            }
        }
    }

    /// One schedule.
    pub fn get(&self, id: &ScheduleId) -> Result<Schedule, Problem> {
        match self.read_schedule(id.as_str()) {
            Ok(Some(schedule)) => Ok(schedule),
            Ok(None) => Err(Problem::new(
                Code::ScheduleNotFound,
                format!("there is no schedule {id}"),
            )),
            Err(problem) if problem.code == Code::ScheduleDamaged => Err(Problem::new(
                Code::ScheduleDamaged,
                format!("the file of {id} is damaged: {}", problem.detail),
            )),
            Err(problem) => Err(problem),
        }
    }

    /// Every schedule, and every damaged schedule file.
    pub fn list(&self) -> Result<ScheduleListing, Problem> {
        let ids = self
            .store
            .ids(Collection::Schedules)
            .map_err(store_problem)?;
        let mut listing = ScheduleListing {
            schedules: Vec::new(),
            damaged: Vec::new(),
        };
        for id in ids {
            match self.read_schedule(&id) {
                Ok(Some(schedule)) => listing.schedules.push(schedule),
                Ok(None) => {}
                Err(problem) if problem.code == Code::ScheduleDamaged => {
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

    /// The run records, oldest first: all of them, or those of one schedule.
    pub fn runs(&self, schedule: Option<&ScheduleId>) -> Result<RunListing, Problem> {
        let mut ids = self.store.ids(Collection::Runs).map_err(store_problem)?;
        ids.sort_by_key(|id| sequence_of(id));
        let mut listing = RunListing {
            runs: Vec::new(),
            damaged: Vec::new(),
        };
        for id in ids {
            match self.read_run(&id) {
                Ok(Some(run)) => {
                    if schedule.is_none_or(|wanted| *wanted == run.schedule) {
                        listing.runs.push(run);
                    }
                }
                Ok(None) => {}
                Err(problem) if problem.code == Code::ScheduleDamaged => {
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

    /// When a schedule last ran, from its run records. A record of a skipped
    /// occurrence counts: that occurrence is done with.
    pub fn last_run(&self, id: &ScheduleId) -> Result<Option<Timestamp>, Problem> {
        Ok(self
            .runs(Some(id))?
            .runs
            .iter()
            .map(|run| run.finished_at)
            .max())
    }

    /// What a schedule is to do at `now`. See [`crate::trigger::next_run`].
    pub fn next_run(&self, id: &ScheduleId, now: Timestamp) -> Result<NextRun, Problem> {
        let schedule = self.get(id)?;
        schedule.next_run(self.last_run(id)?, now)
    }

    /// Record one firing of a schedule.
    ///
    /// The record must be consistent: the schedule exists; the outcome is
    /// `deferred` exactly when something was deferred; a reminder performed
    /// and deferred nothing; the run did not start after `now`. Arguments and
    /// details that look like they hold a secret are withheld from the record,
    /// and the record says so.
    pub fn record_run(&mut self, new: NewRun, now: Timestamp) -> Result<RunRecord, Problem> {
        let schedule = self.get(&new.schedule)?;
        if !now.is_supported() || new.started_at > now {
            return Err(invalid_run("the run starts after it was recorded"));
        }
        if new.scheduled_for.is_some_and(|time| !time.is_supported()) {
            return Err(invalid_run("the scheduled time is out of range"));
        }
        if new.performed.len() + new.deferred.len() > MAX_RUN_ACTIONS {
            return Err(invalid_run(format!(
                "more than {MAX_RUN_ACTIONS} actions in one run"
            )));
        }
        let outcome_is_deferred = new.outcome == RunOutcome::Deferred;
        if outcome_is_deferred == new.deferred.is_empty() {
            return Err(invalid_run(
                "the outcome is deferred exactly when an action was deferred",
            ));
        }
        let acted = new.performed.len() + new.deferred.len() > 0;
        if schedule.is_reminder() && acted {
            return Err(invalid_run("a reminder performs no action"));
        }
        let performed = new
            .performed
            .into_iter()
            .map(cleaned)
            .collect::<Result<Vec<_>, _>>()?;
        let mut deferred = Vec::new();
        for item in new.deferred {
            deferred.push(DeferredAction {
                action: cleaned(item.action)?,
                reason: item.reason,
                detail: cleaned_detail(item.detail)?,
            });
        }
        let detail = cleaned_detail(new.detail)?;

        let mut state = self.read_state()?;
        let existing = self.store.ids(Collection::Runs).map_err(store_problem)?;
        let mut sequence = state.next_run;
        let mut id = RunId::from_sequence(sequence);
        while existing.iter().any(|known| known == id.as_str()) {
            sequence = sequence.saturating_add(1);
            id = RunId::from_sequence(sequence);
        }
        let record = RunRecord {
            schema_version: RUN_SCHEMA_VERSION,
            id,
            schedule: new.schedule,
            scheduled_for: new.scheduled_for,
            event: new.event,
            started_at: new.started_at,
            finished_at: now,
            outcome: new.outcome,
            performed,
            deferred,
            detail,
            presented_at: None,
        };
        self.sweep()?;
        state.next_run = sequence.saturating_add(1);
        self.write_state(&state)?;
        self.write_run(&record)?;

        // Keep the newest records of this schedule.
        let kept = self.runs(Some(&record.schedule))?.runs;
        if kept.len() > MAX_RUNS_KEPT {
            for old in &kept[..kept.len() - MAX_RUNS_KEPT] {
                self.store
                    .remove(Collection::Runs, old.id.as_str())
                    .map_err(store_problem)?;
            }
        }
        Ok(record)
    }

    /// The run records with deferred actions that the user has not yet been
    /// shown, oldest first.
    pub fn unpresented_deferrals(&self) -> Result<Vec<RunRecord>, Problem> {
        Ok(self
            .runs(None)?
            .runs
            .into_iter()
            .filter(|run| !run.deferred.is_empty() && run.presented_at.is_none())
            .collect())
    }

    /// Record that the user was shown a run record at `now`.
    pub fn mark_presented(&mut self, id: &RunId, now: Timestamp) -> Result<RunRecord, Problem> {
        let Some(run) = self.read_run(id.as_str())? else {
            return Err(Problem::new(
                Code::ScheduleNotFound,
                format!("there is no run record {id}"),
            ));
        };
        let presented = RunRecord {
            presented_at: Some(now),
            ..run
        };
        self.sweep()?;
        self.write_run(&presented)?;
        Ok(presented)
    }

    /// Every schedule and run record as one JSON document, with the damaged
    /// files.
    pub fn export(&self) -> Result<Value, Problem> {
        let schedules = self.list()?;
        let runs = self.runs(None)?;
        let unserialisable =
            |_| Problem::new(Code::ScheduleStore, "the records could not be serialised");
        let mut damaged = schedules.damaged;
        damaged.extend(runs.damaged);
        Ok(json!({
            "schedules": serde_json::to_value(&schedules.schedules).map_err(unserialisable)?,
            "runs": serde_json::to_value(&runs.runs).map_err(unserialisable)?,
            "damaged": serde_json::to_value(&damaged).map_err(unserialisable)?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;
    use crate::testing::TestUser;
    use crate::trigger::{IntervalUnit, LocalTime};

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn daily() -> Trigger {
        Trigger::Daily {
            time: LocalTime::new(7, 30).expect("a valid time"),
            zone: "UTC".to_owned(),
        }
    }

    fn reminder(name: &str) -> Proposal {
        Proposal {
            name: name.to_owned(),
            action: ProposedAction::Reminder {
                message: "Take the bins out.".to_owned(),
            },
            trigger: daily(),
            missed_runs: MissedRunPolicy::Skip,
            proposed_by_task: None,
        }
    }

    fn report_arguments() -> Map<String, Value> {
        object(json!({ "to": "manager@example.org", "subject": "Weekly report", "copies": 1 }))
    }

    fn task(name: &str) -> Proposal {
        Proposal {
            name: name.to_owned(),
            action: ProposedAction::Task {
                request: "Send the weekly report to my manager.".to_owned(),
                approvals: vec![ApprovalRequest {
                    service: "org.uos.Mail1".to_owned(),
                    tool: "mail_send".to_owned(),
                    arguments: report_arguments(),
                }],
            },
            trigger: daily(),
            missed_runs: MissedRunPolicy::RunOnce,
            proposed_by_task: Some("task-12".to_owned()),
        }
    }

    fn book() -> ScheduleBook<MemoryStore> {
        ScheduleBook::new(MemoryStore::new())
    }

    fn user() -> TestUser {
        TestUser::agreeing(1_000)
    }

    fn call(arguments: Value) -> ActionRecord {
        ActionRecord {
            service: "org.uos.Mail1".to_owned(),
            tool: "mail_send".to_owned(),
            arguments: object(arguments),
            arguments_withheld: false,
        }
    }

    fn run_of(schedule: &Schedule, outcome: RunOutcome) -> NewRun {
        NewRun {
            schedule: schedule.id().clone(),
            scheduled_for: Some(Timestamp(2_000)),
            event: None,
            started_at: Timestamp(2_000),
            outcome,
            performed: Vec::new(),
            deferred: Vec::new(),
            detail: String::new(),
        }
    }

    #[test]
    fn a_proposal_becomes_a_schedule_only_with_the_users_confirmation() {
        let mut book = book();
        let refused = book
            .create(task("Report"), &TestUser::refusing(1_000))
            .expect_err("not confirmed");
        assert_eq!(refused.code, Code::ScheduleNotConfirmed);
        assert_eq!(book.store(), &MemoryStore::new(), "nothing was written");

        let schedule = book.create(task("Report"), &user()).expect("creates");
        assert_eq!(schedule.id().as_str(), "s-00000001");
        assert_eq!(schedule.name(), "Report");
        assert_eq!(schedule.created_at(), Timestamp(1_000));
        assert_eq!(schedule.confirmed_at(), Timestamp(1_000));
        assert_eq!(schedule.proposed_by_task(), Some("task-12"));
        assert_eq!(schedule.standing_approvals().len(), 1);
        assert!(!schedule.is_reminder());
        assert_eq!(book.get(schedule.id()), Ok(schedule));
    }

    #[test]
    fn a_reminder_cannot_carry_an_approval() {
        let mut book = book();
        let schedule = book.create(reminder("Bins"), &user()).expect("creates");
        assert!(schedule.is_reminder());
        assert!(schedule.standing_approvals().is_empty());
        assert_eq!(
            schedule.approval_for("org.uos.Mail1", "mail_send", &report_arguments()),
            None
        );
        // Neither a proposal nor a stored file can attach one to a reminder.
        let text = r#"{"name":"x","action":{"type":"reminder","message":"m","approvals":[]},"trigger":{"type":"event","event":"update_available"},"missed_runs":"skip"}"#;
        assert!(serde_json::from_str::<Proposal>(text).is_err());
        let stored = serde_json::to_value(&schedule).expect("serialises");
        let mut tampered = stored.clone();
        tampered["action"]["standing_approvals"] = json!([
            { "service": "org.uos.Mail1", "tool": "mail_send", "arguments": {} }
        ]);
        let bytes = serde_json::to_vec(&tampered).expect("serialises");
        assert!(Schedule::from_json(&bytes).is_err());
        let bytes = serde_json::to_vec(&stored).expect("serialises");
        assert_eq!(Schedule::from_json(&bytes), Ok(schedule));
    }

    #[test]
    fn a_stored_schedule_keeps_its_approvals_exactly() {
        let mut book = book();
        let created = book.create(task("Report"), &user()).expect("creates");
        // Read back through the store, as a later process would.
        let loaded = ScheduleBook::new(book.into_store())
            .get(created.id())
            .expect("loads");
        assert_eq!(loaded, created);
        assert!(
            loaded
                .approval_for("org.uos.Mail1", "mail_send", &report_arguments())
                .is_some()
        );
        // Near misses after the round trip: one argument different, one
        // extra, and the number written as a float.
        let near_misses = [
            json!({ "to": "other@example.org", "subject": "Weekly report", "copies": 1 }),
            json!({ "to": "manager@example.org", "subject": "Weekly report", "copies": 1, "bcc": "x@example.org" }),
            json!({ "to": "manager@example.org", "subject": "Weekly report", "copies": 1.0 }),
            json!({ "to": "manager@example.org", "subject": "Weekly report" }),
        ];
        for arguments in near_misses {
            assert_eq!(
                loaded.approval_for("org.uos.Mail1", "mail_send", &object(arguments.clone())),
                None,
                "{arguments}"
            );
        }
        assert_eq!(
            loaded.approval_for("org.uos.Mail1", "mail_delete", &report_arguments()),
            None
        );
        assert_eq!(
            loaded.why_deferred("org.uos.Mail1", "mail_send"),
            DeferReason::ArgumentsDiffer
        );
        assert_eq!(
            loaded.why_deferred("org.uos.Mail1", "mail_delete"),
            DeferReason::NoStandingApproval
        );
    }

    #[test]
    fn proposals_are_checked() {
        let mut book = book();
        let mut code = |proposal: Proposal| {
            book.create(proposal, &user())
                .expect_err("an invalid proposal")
                .code
        };
        assert_eq!(code(reminder("")), Code::ScheduleInvalidProposal);
        assert_eq!(
            code(reminder(&"n".repeat(MAX_NAME_BYTES + 1))),
            Code::ScheduleInvalidProposal
        );
        assert_eq!(code(reminder("two\nlines")), Code::ScheduleInvalidProposal);

        let mut too_frequent = reminder("Often");
        too_frequent.trigger = Trigger::Every {
            every: 1,
            unit: IntervalUnit::Minutes,
            anchor: Timestamp(0),
        };
        assert_eq!(code(too_frequent), Code::ScheduleTooFrequent);

        let mut past = reminder("Past");
        past.trigger = Trigger::Once {
            at: Timestamp(1_000),
        };
        assert_eq!(code(past), Code::ScheduleInvalidTrigger);

        let mut unknown_zone = reminder("Zone");
        unknown_zone.trigger = Trigger::Daily {
            time: LocalTime::new(7, 30).expect("a valid time"),
            zone: "Mars/Olympus".to_owned(),
        };
        assert_eq!(code(unknown_zone), Code::ScheduleInvalidTrigger);

        let mut bad_approval = task("Bad approval");
        if let ProposedAction::Task { approvals, .. } = &mut bad_approval.action {
            approvals[0].tool = "Mail Send".to_owned();
        }
        assert_eq!(code(bad_approval), Code::ScheduleInvalidApproval);

        let mut many = task("Many approvals");
        if let ProposedAction::Task { approvals, .. } = &mut many.action {
            let one = approvals[0].clone();
            approvals.resize(MAX_STANDING_APPROVALS + 1, one);
        }
        assert_eq!(code(many), Code::ScheduleInvalidProposal);

        let mut secret_message = reminder("Secret");
        secret_message.action = ProposedAction::Reminder {
            message: "the wifi password is hunter2".to_owned(),
        };
        assert_eq!(code(secret_message), Code::ScheduleSecret);

        let mut secret_argument = task("Secret argument");
        if let ProposedAction::Task { approvals, .. } = &mut secret_argument.action {
            approvals[0]
                .arguments
                .insert("password".to_owned(), json!("hunter2"));
        }
        assert_eq!(code(secret_argument), Code::ScheduleSecret);
    }

    #[test]
    fn the_number_of_schedules_is_capped() {
        let mut book = book();
        for number in 0..MAX_SCHEDULES {
            book.create(reminder(&format!("Reminder {number}")), &user())
                .expect("creates");
        }
        let refused = book
            .create(reminder("One too many"), &user())
            .expect_err("the limit");
        assert_eq!(refused.code, Code::ScheduleLimit);
        assert_eq!(book.list().expect("lists").schedules.len(), MAX_SCHEDULES);
    }

    #[test]
    fn changing_a_schedule_is_the_users_act_and_replaces_its_approvals() {
        let mut book = book();
        let schedule = book.create(task("Report"), &user()).expect("creates");
        let id = schedule.id().clone();

        let refused = book
            .change(&id, reminder("Now a reminder"), &TestUser::refusing(2_000))
            .expect_err("not confirmed");
        assert_eq!(refused.code, Code::ScheduleNotConfirmed);
        assert_eq!(book.get(&id), Ok(schedule));

        let changed = book
            .change(&id, reminder("Now a reminder"), &TestUser::agreeing(2_000))
            .expect("changes");
        assert_eq!(changed.id(), &id);
        assert_eq!(changed.created_at(), Timestamp(1_000));
        assert_eq!(changed.confirmed_at(), Timestamp(2_000));
        assert!(changed.standing_approvals().is_empty());
        assert_eq!(
            book.change(
                &ScheduleId::from_sequence(99),
                reminder("x"),
                &TestUser::agreeing(2_000)
            )
            .expect_err("missing")
            .code,
            Code::ScheduleNotFound
        );
    }

    #[test]
    fn approvals_can_be_revoked_and_schedules_removed_by_the_user() {
        let mut book = book();
        let schedule = book.create(task("Report"), &user()).expect("creates");
        let id = schedule.id().clone();
        book.record_run(run_of(&schedule, RunOutcome::Completed), Timestamp(2_010))
            .expect("records");

        let revoke = ScheduleEdit::RevokeApproval {
            id: id.clone(),
            service: "org.uos.Mail1".to_owned(),
            tool: "mail_send".to_owned(),
            arguments: report_arguments(),
        };
        assert_eq!(
            book.apply(&revoke, &TestUser::refusing(3_000))
                .expect_err("not confirmed")
                .code,
            Code::ScheduleNotConfirmed
        );
        assert_eq!(book.get(&id).expect("loads").standing_approvals().len(), 1);
        book.apply(&revoke, &TestUser::agreeing(3_000))
            .expect("revokes");
        assert!(book.get(&id).expect("loads").standing_approvals().is_empty());
        assert_eq!(
            book.apply(&revoke, &TestUser::agreeing(3_000))
                .expect_err("already revoked")
                .code,
            Code::ScheduleNotFound
        );

        let remove = ScheduleEdit::Remove { id: id.clone() };
        assert_eq!(
            book.apply(&remove, &TestUser::refusing(3_000))
                .expect_err("not confirmed")
                .code,
            Code::ScheduleNotConfirmed
        );
        book.apply(&remove, &TestUser::agreeing(3_000))
            .expect("removes");
        assert_eq!(
            book.get(&id).expect_err("gone").code,
            Code::ScheduleNotFound
        );
        assert!(book.runs(None).expect("lists").runs.is_empty());
        assert_eq!(
            book.apply(&remove, &TestUser::agreeing(3_000))
                .expect_err("gone")
                .code,
            Code::ScheduleNotFound
        );
    }

    #[test]
    fn deferred_actions_are_kept_for_the_users_return() {
        let mut book = book();
        let schedule = book.create(task("Report"), &user()).expect("creates");
        let other = json!({ "to": "everyone@example.org", "subject": "Weekly report", "copies": 1 });
        let mut run = run_of(&schedule, RunOutcome::Deferred);
        run.performed = vec![call(Value::Object(report_arguments()))];
        run.deferred = vec![DeferredAction {
            action: call(other.clone()),
            reason: schedule.why_deferred("org.uos.Mail1", "mail_send"),
            detail: "The recipient is not the one that was approved.".to_owned(),
        }];
        let record = book.record_run(run, Timestamp(2_030)).expect("records");
        assert_eq!(record.id.as_str(), "r-00000001");
        assert_eq!(record.finished_at, Timestamp(2_030));
        assert_eq!(record.deferred[0].reason, DeferReason::ArgumentsDiffer);
        assert_eq!(record.deferred[0].action.arguments, object(other));

        let waiting = book.unpresented_deferrals().expect("lists");
        assert_eq!(waiting, [record.clone()]);
        let shown = book
            .mark_presented(&record.id, Timestamp(9_000))
            .expect("marks");
        assert_eq!(shown.presented_at, Some(Timestamp(9_000)));
        assert!(book.unpresented_deferrals().expect("lists").is_empty());
        // The record itself is still there.
        assert_eq!(book.runs(Some(schedule.id())).expect("lists").runs, [shown]);
        assert_eq!(
            book.mark_presented(&RunId::from_sequence(77), Timestamp(9_000))
                .expect_err("missing")
                .code,
            Code::ScheduleNotFound
        );
    }

    #[test]
    fn inconsistent_run_records_are_refused() {
        let mut book = book();
        let schedule = book.create(task("Report"), &user()).expect("creates");
        let bins = book.create(reminder("Bins"), &user()).expect("creates");
        let deferral = DeferredAction {
            action: call(json!({})),
            reason: DeferReason::NoStandingApproval,
            detail: String::new(),
        };

        // Deferred outcome without a deferred action, and the reverse.
        let run = run_of(&schedule, RunOutcome::Deferred);
        assert_eq!(
            book.record_run(run, Timestamp(2_010)).expect_err("empty").code,
            Code::ScheduleInvalidRun
        );
        let mut run = run_of(&schedule, RunOutcome::Completed);
        run.deferred = vec![deferral];
        assert!(book.record_run(run, Timestamp(2_010)).is_err());

        // A reminder that claims to have acted.
        let mut run = run_of(&bins, RunOutcome::Completed);
        run.performed = vec![call(json!({}))];
        assert!(book.record_run(run, Timestamp(2_010)).is_err());

        // A run that starts after it was recorded.
        let run = run_of(&schedule, RunOutcome::Completed);
        assert!(book.record_run(run, Timestamp(1_999)).is_err());

        // An action that does not name a tool.
        let mut run = run_of(&schedule, RunOutcome::Completed);
        run.performed = vec![ActionRecord {
            tool: "not a tool".to_owned(),
            ..call(json!({}))
        }];
        assert!(book.record_run(run, Timestamp(2_010)).is_err());

        // A schedule that does not exist.
        let mut run = run_of(&schedule, RunOutcome::Completed);
        run.schedule = ScheduleId::from_sequence(99);
        assert_eq!(
            book.record_run(run, Timestamp(2_010)).expect_err("missing").code,
            Code::ScheduleNotFound
        );
        assert!(book.runs(None).expect("lists").runs.is_empty());
    }

    #[test]
    fn secrets_are_withheld_from_run_records() {
        let mut book = book();
        let schedule = book.create(task("Report"), &user()).expect("creates");
        let mut run = run_of(&schedule, RunOutcome::Deferred);
        run.deferred = vec![DeferredAction {
            action: call(json!({ "user": "anna", "password": "hunter2" })),
            reason: DeferReason::NoStandingApproval,
            detail: "the password is hunter2".to_owned(),
        }];
        run.detail = "Stopped before logging in.".to_owned();
        let record = book.record_run(run, Timestamp(2_010)).expect("records");
        assert!(record.deferred[0].action.arguments_withheld);
        assert!(record.deferred[0].action.arguments.is_empty());
        assert_eq!(record.deferred[0].detail, WITHHELD);
        assert_eq!(record.detail, "Stopped before logging in.");
        let stored = serde_json::to_string(&book.export().expect("exports")).expect("serialises");
        assert!(!stored.contains("hunter2"));
    }

    #[test]
    fn the_last_run_drives_the_next_one() {
        let mut book = book();
        // Created at 1 000 seconds: the first 07:30 UTC is at 27 000.
        let schedule = book.create(task("Report"), &user()).expect("creates");
        let id = schedule.id().clone();
        let first = Timestamp(27_000);
        let second = Timestamp(27_000 + 86_400);
        assert_eq!(book.last_run(&id), Ok(None));
        assert_eq!(
            book.next_run(&id, Timestamp(5_000)),
            Ok(NextRun::Wait {
                next: first,
                skipped_missed: false
            })
        );
        assert_eq!(
            book.next_run(&id, Timestamp(27_010)),
            Ok(NextRun::Due {
                scheduled_for: first,
                late: false,
                next: Some(second)
            })
        );
        let mut run = run_of(&schedule, RunOutcome::Completed);
        run.scheduled_for = Some(first);
        run.started_at = Timestamp(27_010);
        book.record_run(run, Timestamp(27_040)).expect("records");
        assert_eq!(book.last_run(&id), Ok(Some(Timestamp(27_040))));
        // The same occurrence is not due again.
        assert_eq!(
            book.next_run(&id, Timestamp(27_050)),
            Ok(NextRun::Wait {
                next: second,
                skipped_missed: false
            })
        );
    }

    #[test]
    fn old_run_records_are_pruned_per_schedule() {
        let mut book = book();
        let report = book.create(task("Report"), &user()).expect("creates");
        let bins = book.create(reminder("Bins"), &user()).expect("creates");
        book.record_run(run_of(&bins, RunOutcome::Completed), Timestamp(2_000))
            .expect("records");
        let extra = 5;
        for number in 0..MAX_RUNS_KEPT + extra {
            let at = Timestamp(2_000 + u64::try_from(number).expect("fits"));
            book.record_run(run_of(&report, RunOutcome::Completed), at)
                .expect("records");
        }
        let kept = book.runs(Some(report.id())).expect("lists").runs;
        assert_eq!(kept.len(), MAX_RUNS_KEPT);
        // The oldest of this schedule went; the newest stayed.
        let newest = u64::try_from(MAX_RUNS_KEPT + extra + 1).expect("fits");
        assert_eq!(kept.last().map(|run| run.id.clone()), Some(RunId::from_sequence(newest)));
        // The other schedule's record is untouched.
        assert_eq!(book.runs(Some(bins.id())).expect("lists").runs.len(), 1);
    }

    #[test]
    fn a_damaged_schedule_file_is_reported_and_hides_nothing() {
        let mut book = book();
        let first = book.create(reminder("One"), &user()).expect("creates");
        let second = book.create(task("Two"), &user()).expect("creates");
        let mut store = book.into_store();
        store
            .put(Collection::Schedules, first.id().as_str(), b"{\"schema_ver")
            .expect("plants damage");
        let book = ScheduleBook::new(store);
        let listing = book.list().expect("lists");
        assert_eq!(listing.schedules, [second]);
        assert_eq!(listing.damaged.len(), 1);
        assert_eq!(listing.damaged[0].id, first.id().as_str());
        assert_eq!(
            book.get(first.id()).expect_err("damaged").code,
            Code::ScheduleDamaged
        );
    }
}
