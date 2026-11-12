// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The session service, without the bus: everything `org.tur.Agent1` does,
//! as plain functions on one [`Core`].
//!
//! [`crate::dbus`] is a thin layer over this module: it checks who is
//! calling, reads the arguments and calls one function here. Keeping the
//! bus out makes the whole service testable with no bus at all
//! (`tests/service.rs`), and it makes one statement easy to check: **every
//! entry point of the interface ends in this module, and this module runs
//! a task in exactly one way.**
//!
//! # The one way a task runs
//!
//! [`Core::start_task`] (a person asked) and the schedule runner (a
//! schedule came due) both end in the private `run_job`, which builds an
//! [`Orchestrator`] and calls [`Orchestrator::run`]. What that loop may do
//! is fixed by the loop itself (`orchestrator.rs`, "What the loop cannot be
//! made to do"): the only thing this module tells it about the
//! circumstances is a [`Situation`], attended or unattended, locked or
//! not. There is no field here for the trusted confirmation path, a repair
//! session or a standing approval, so nothing above class 1 can run by any
//! path through the service. A source test holds this module and
//! [`crate::dbus`] to that (`tests/service.rs`).
//!
//! # Whose acts these are
//!
//! Switching P@x on, choosing a provider, setting a key or a limit,
//! creating or removing a schedule, keeping or forgetting a memory entry:
//! the design calls these the person's acts, and wants the loosening ones
//! confirmed on a trusted screen by a separate component (addendum A
//! sections 17 to 19). Neither exists. What this module can honestly claim
//! is narrower:
//!
//! * **The model cannot perform them.** No tool reaches these functions.
//!   A model can ask for a reminder or a note to be *proposed*; the
//!   proposal changes nothing until one of these functions is called from
//!   outside the task.
//! * **Any program running as the person can.** The service cannot tell
//!   the settings window from another process of the same user.
//!
//! Standing approvals are a consequence: a schedule created here never
//! carries one, and a proposal that asks for one is refused, because an
//! approval given over the session bus would be an approval anyone in the
//! session can give.

use std::collections::VecDeque;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;

use serde_json::{Map, Value, json};
use tur_pax_memory::{
    ActionRecord, Clock, DeferReason, DeferredAction, EntryId, EntryState, FileStore, Filter,
    LocalTime, Memory, MemoryEntry, MemorySwitch, MissedRunPolicy, NewEntry, Proposal,
    ProposedAction, RecallBudget, RecalledNotes, RunId, RunOutcome, Schedule, ScheduleBook,
    ScheduleEdit, ScheduleId, ScheduledAction, ShortMonth, StdFs, Timestamp, Trigger,
    UserConfirmation, is_known_zone, weekday_and_day_of,
};
use tur_pax_providers::{Message, ProviderConfig, SpendMeter, Usage, connect, connect_without_key};

use crate::audit::{AuditRecord, AuditWriteError};
use crate::codes::{Code, Problem};
use crate::consent::{CONSENT_TEXT_VERSION, Subject, consent_text};
use crate::exec::Executor;
use crate::faults::{Fault, TaskCode};
use crate::keyring::KeyStore;
use crate::notify::{self, AwayOutcome, Notifier};
use crate::orchestrator::{
    Assistant, AssistantOutcome, HeldAction, MEMORY_RECALL, Observer, Orchestrator, ProposalReport,
    ReportItem, SharedControl, Situation, SystemTaskClock, TaskLimits, TaskOutcome, TaskRequest,
};
use crate::policy::UserPolicy;
use crate::registry::{ASSISTANT_TOOLS, AssistantCall, AssistantTools, COMMAND_TOOL, ToolRegistry};
use crate::sandbox::{Grant, GrantRules};
use crate::scheduler::{Runner, TaskRun, TickReport, outcome_name, run_due};
use crate::settings::{
    Limits, MODEL_CANNOT_ACT, ModelList, Origin, Paths, ProviderEntry, Settings, find_provider,
    is_provider_id, providers,
};
use crate::spending::{Ledger, TaskBudget, Zone, day_of};
use crate::textui::{DEFAULT_TOOLS_DIR, read_json_dir, read_tools};
use crate::trail::{EventKind, FileTrail, TIME_FIELD, Trail, TrailEvent};
use crate::wake::Waker;

/// The version of the interface this service speaks.
pub const INTERFACE_VERSION: u32 = 1;

/// The longest request, in bytes.
pub const REQUEST_MAX_BYTES: usize = 8000;

/// How many finished tasks the service remembers.
pub const TASKS_KEPT: usize = 20;

/// How many proposals may wait for the person at one time.
pub const PROPOSALS_MAX: usize = 16;

/// How many messages a continued conversation may hold before a new one is
/// started.
pub const CONVERSATION_MAX_MESSAGES: usize = 200;

/// The most of the audit trail that is read for the activity list, in
/// bytes, counted from its end.
pub const ACTIVITY_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Something the service tells whoever listens: the signals of
/// `org.tur.Agent1`.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A task began.
    TaskStarted {
        /// The task.
        task: String,
        /// What is known about it.
        info: Value,
    },
    /// Text for the person: the model's words between steps (`interim`),
    /// its answer (`answer`), or a sentence of the system's (`notice`).
    TextAdded {
        /// The task.
        task: String,
        /// `interim`, `answer` or `notice`.
        kind: &'static str,
        /// The text.
        text: String,
    },
    /// A step was decided and, if it may run, is about to.
    ActionProposed {
        /// The task.
        task: String,
        /// The step.
        step: Value,
    },
    /// A step ended: it ran, failed, or was refused.
    ActionCompleted {
        /// The task.
        task: String,
        /// The step.
        step: Value,
    },
    /// A task ended.
    TaskFinished {
        /// The task.
        task: String,
        /// How it ended.
        result: Value,
    },
    /// P@x proposed a reminder or a scheduled task. Nothing is scheduled.
    ScheduleProposed {
        /// The proposal's identifier.
        proposal: String,
        /// The proposal.
        details: Value,
    },
    /// The schedules, their runs or the proposals changed.
    SchedulesChanged,
    /// The memory changed.
    MemoryChanged,
    /// The switch or the state changed.
    StateChanged,
}

/// Receives the service's events.
pub trait Events: Send + Sync {
    /// One event.
    fn emit(&self, event: &Event);
}

/// Events that go nowhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoEvents;

impl Events for NoEvents {
    fn emit(&self, _event: &Event) {}
}

/// Makes the executor of one task. The flag it is given is set when the
/// task is cancelled.
pub type ExecutorFactory = Arc<dyn Fn(Arc<AtomicBool>) -> Box<dyn Executor + Send> + Send + Sync>;

/// What the service is built from.
pub struct Parts {
    /// Where the files are.
    pub paths: Paths,
    /// The clock.
    pub clock: Arc<dyn Clock + Send + Sync>,
    /// The time zone the computer is set to.
    pub zone: Zone,
    /// The password store.
    pub keys: Arc<dyn KeyStore>,
    /// The notification service.
    pub notifier: Arc<dyn Notifier>,
    /// The timer that wakes the schedule runner.
    pub waker: Arc<dyn Waker>,
    /// Who is told what happens.
    pub events: Arc<dyn Events>,
    /// What runs sandbox plans.
    pub executors: ExecutorFactory,
}

/// A confirmation given by whoever called the service from outside a task,
/// for exactly what they were shown.
struct ByTheCaller<T> {
    shown: T,
    at: Timestamp,
}

impl<T: PartialEq> UserConfirmation<T> for ByTheCaller<T> {
    fn covers(&self, subject: &T) -> bool {
        self.shown == *subject
    }

    fn confirmed_at(&self) -> Timestamp {
        self.at
    }
}

/// A proposal P@x made, waiting for the person.
#[derive(Debug, Clone, PartialEq)]
struct PendingProposal {
    id: String,
    proposal: Proposal,
    proposed_at: Timestamp,
    attended: bool,
}

/// The task that is running.
struct Running {
    id: String,
    control: SharedControl,
    stop: Arc<AtomicBool>,
    attended: bool,
    schedule: Option<String>,
    paused_by_lock: bool,
    paused: bool,
}

/// What the service remembers of a task.
#[derive(Debug, Clone, PartialEq)]
struct TaskRecord {
    id: String,
    request: String,
    attended: bool,
    schedule: Option<String>,
    started_at: u64,
    finished_at: Option<u64>,
    outcome: Option<String>,
    error_code: Option<String>,
    message: String,
    answer: String,
    texts: Vec<Value>,
    steps: Vec<Value>,
    usage: Usage,
    cost: Option<f64>,
    currency: Option<String>,
}

impl TaskRecord {
    fn to_json(&self, paused: bool) -> Value {
        let state = match (&self.outcome, paused) {
            (Some(_), _) => "finished",
            (None, true) => "paused",
            (None, false) => "working",
        };
        json!({
            "task": self.id,
            "request": self.request,
            "attended": self.attended,
            "schedule": self.schedule,
            "state": state,
            "started_at": self.started_at,
            "finished_at": self.finished_at,
            "outcome": self.outcome,
            "error_code": self.error_code,
            "message": self.message,
            "answer": self.answer,
            "texts": self.texts,
            "steps": self.steps,
            "usage": {
                "input_tokens": self.usage.input_tokens,
                "output_tokens": self.usage.output_tokens,
                "cached_input_tokens": self.usage.cached_input_tokens,
            },
            "cost": self.cost,
            "currency": self.currency,
        })
    }
}

struct State {
    settings: Settings,
    /// Why the settings could not be read, if they could not: P@x is then
    /// off.
    settings_fault: Option<Fault>,
    models: ModelList,
    models_fault: Option<Fault>,
    running: Option<Running>,
    tasks: VecDeque<TaskRecord>,
    proposals: Vec<PendingProposal>,
    history: Vec<Message>,
    locked: Option<bool>,
    key_set: Option<bool>,
    counter: u64,
}

/// What a job needs to run.
struct Job {
    id: String,
    request: String,
    situation: Situation,
    grants: Vec<Grant>,
    resume: bool,
    control: SharedControl,
    stop: Arc<AtomicBool>,
}

/// How a job ended.
struct Ended {
    outcome: String,
    code: Option<String>,
    message: String,
    answer: String,
    usage: Usage,
    cost: Option<f64>,
    currency: Option<String>,
    performed: Vec<ActionRecord>,
    held: Vec<DeferredAction>,
}

/// The session service.
pub struct Core {
    parts: Parts,
    state: Mutex<State>,
    ledger: Result<Arc<Mutex<Ledger>>, Fault>,
    /// One look at the schedules at a time.
    ticking: Mutex<()>,
    /// What the person is to be told about the scheduled tasks of this look
    /// at the schedules, sent once their runs are recorded.
    to_tell: Mutex<Vec<notify::Notification>>,
    poked: Mutex<bool>,
    wake: Condvar,
}

fn fault(code: TaskCode, detail: impl Into<String>) -> Fault {
    Fault::new(code, detail)
}

fn coded(code: TaskCode) -> Fault {
    Fault::new(code, code.user_message())
}

fn invalid(detail: impl Into<String>) -> Fault {
    Fault::new(TaskCode::ServiceInvalidRequest, detail)
}

fn not_found(what: &str) -> Fault {
    Fault::new(
        TaskCode::ServiceNotFound,
        format!("there is no {what} with that identifier"),
    )
}

/// A problem of the memory layer as a fault. Its codes are `TUR-AI-*` codes
/// too; one that cannot be read as a code becomes a storage fault.
fn memory_fault(problem: tur_pax_memory::Problem) -> Fault {
    match problem.code.as_str().parse() {
        Ok(code) => Fault {
            code,
            detail: problem.detail,
        },
        Err(_) => fault(TaskCode::ServiceStorage, problem.detail),
    }
}

/// Whether `text` can be a request: not empty, not too long, and free of
/// control characters other than a line break and a tab.
fn is_request(text: &str) -> bool {
    !text.trim().is_empty()
        && text.len() <= REQUEST_MAX_BYTES
        && !text
            .chars()
            .any(|character| character.is_control() && character != '\n' && character != '\t')
}

fn text_of<'a>(arguments: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    arguments.get(name).and_then(Value::as_str)
}

/// The first sentence of a text, on one line, of at most `most` characters.
fn first_sentence(text: &str, most: usize) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    let sentence = line.split_once(". ").map_or(line, |(first, _)| first);
    let sentence = sentence.trim_end_matches('.');
    if sentence.chars().count() <= most {
        sentence.to_owned()
    } else {
        let mut cut: String = sentence.chars().take(most.saturating_sub(1)).collect();
        cut.push('\u{2026}');
        cut
    }
}

/// Turn the arguments of one of the two "propose" tools into a proposal.
///
/// The words are the model's; what kind of thing they are is decided here.
/// The proposal has no standing approval and no way to ask for one.
///
/// # Errors
///
/// A sentence saying what is wrong with the time. It repeats nothing the
/// model wrote.
pub fn draft_proposal(
    reminder: bool,
    arguments: &Map<String, Value>,
    now: Timestamp,
    host_zone: &str,
    task: &str,
) -> Result<Proposal, String> {
    let text = text_of(arguments, if reminder { "message" } else { "request" })
        .ok_or("the text of the proposal is missing")?;
    let name = text_of(arguments, "name").map_or_else(|| first_sentence(text, 60), str::to_owned);
    let zone = text_of(arguments, "zone").unwrap_or(host_zone);
    if !is_known_zone(zone) {
        return Err("the time zone is not a name of the IANA time-zone database".to_owned());
    }
    let repeat = text_of(arguments, "repeat").unwrap_or("none");
    let minutes = arguments.get("in_minutes").and_then(Value::as_u64);
    let date = text_of(arguments, "date");
    let time = match text_of(arguments, "time") {
        Some(time) => Some(
            LocalTime::parse(time).ok_or("the time of day is not written HH:MM, 00:00 to 23:59")?,
        ),
        None => None,
    };
    let bad_date = || "the date is not a calendar date written YYYY-MM-DD".to_owned();
    let trigger = match (minutes, date, time, repeat) {
        (Some(minutes), None, None, "none") => Trigger::Once {
            at: minutes
                .checked_mul(60)
                .and_then(|seconds| now.checked_add(seconds))
                .ok_or("the time is too far away")?,
        },
        (Some(_), ..) => {
            return Err(
                "in_minutes cannot be combined with a date, a time of day or a repeat".to_owned(),
            );
        }
        (None, Some(date), Some(time), "none") => {
            let at = Timestamp::from_wall_clock(date, time.hour(), time.minute(), zone)
                .ok_or_else(bad_date)?;
            if at <= now {
                return Err("that time has already passed".to_owned());
            }
            Trigger::Once { at }
        }
        (None, _, Some(time), "daily") => Trigger::Daily {
            time,
            zone: zone.to_owned(),
        },
        (None, Some(date), Some(time), "weekly") => {
            let (weekday, _) = weekday_and_day_of(date).ok_or_else(bad_date)?;
            Trigger::Weekly {
                weekdays: vec![weekday],
                time,
                zone: zone.to_owned(),
            }
        }
        (None, Some(date), Some(time), "monthly") => {
            let (_, day) = weekday_and_day_of(date).ok_or_else(bad_date)?;
            Trigger::Monthly {
                day,
                time,
                zone: zone.to_owned(),
                if_day_missing: ShortMonth::LastDay,
            }
        }
        _ => {
            return Err(
                "give in_minutes, or a date and a time of day; a repeating schedule needs a \
                 time of day, and a weekly or monthly one a date as well"
                    .to_owned(),
            );
        }
    };
    let action = if reminder {
        ProposedAction::Reminder {
            message: text.to_owned(),
        }
    } else {
        ProposedAction::Task {
            request: text.to_owned(),
            approvals: Vec::new(),
        }
    };
    let proposal = Proposal {
        name,
        action,
        trigger,
        missed_runs: MissedRunPolicy::RunOnce,
        proposed_by_task: Some(task.to_owned()),
    };
    proposal
        .validate()
        .map_err(|problem| format!("{}: {}", problem.code.as_str(), problem.detail))?;
    Ok(proposal)
}

fn proposal_json(id: &str, pending: &PendingProposal) -> Value {
    let (kind, text) = match &pending.proposal.action {
        ProposedAction::Reminder { message } => ("reminder", message),
        ProposedAction::Task { request, .. } => ("task", request),
    };
    json!({
        "proposal": id,
        "kind": kind,
        "name": pending.proposal.name,
        "text": text,
        "trigger": pending.proposal.trigger,
        "trigger_text": pending.proposal.trigger.describe(),
        "missed_runs": pending.proposal.missed_runs.as_str(),
        "proposed_at": pending.proposed_at.seconds(),
        "proposed_by_task": pending.proposal.proposed_by_task,
        "attended": pending.attended,
        "definition": pending.proposal,
    })
}

fn entry_json(entry: &MemoryEntry) -> Value {
    json!({
        "id": entry.id().as_str(),
        "kind": entry.kind().as_str(),
        "state": match entry.state() {
            EntryState::Pending => "pending",
            EntryState::Active => "active",
        },
        "trusted": entry.kind().as_str() != "from_content",
        "text": entry.text(),
        "origin": entry.provenance().origin,
        "task": entry.provenance().task,
        "recorded_at": entry.provenance().recorded_at.seconds(),
        "confirmed_at": entry.confirmed_at().map(Timestamp::seconds),
        "tags": entry.tags(),
    })
}

/// A trail that counts what it wrote, so that a step can say which entry of
/// the trail is about it.
struct CountingTrail {
    inner: FileTrail,
    written: Arc<AtomicUsize>,
}

impl Trail for CountingTrail {
    fn decision(&mut self, record: &AuditRecord) -> Result<(), AuditWriteError> {
        self.inner.decision(record)?;
        self.written.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn event(&mut self, event: &TrailEvent) -> Result<(), AuditWriteError> {
        self.inner.event(event)?;
        self.written.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Performs P@x's own operations for a task.
struct CoreAssistant<'c> {
    core: &'c Core,
}

impl Assistant for CoreAssistant<'_> {
    fn perform(&mut self, task: &str, attended: bool, call: &AssistantCall) -> AssistantOutcome {
        match call {
            AssistantCall::ProposeReminder(arguments) => {
                self.core.propose(task, attended, true, arguments)
            }
            AssistantCall::ProposeTask(arguments) => {
                self.core.propose(task, attended, false, arguments)
            }
            AssistantCall::KeepNote(arguments) => self.core.keep_note(task, arguments),
        }
    }
}

/// Tells the service, and through it the bus, what a task does.
struct TaskObserver<'c> {
    core: &'c Core,
    task: &'c str,
    registry: &'c ToolRegistry,
    written: Arc<AtomicUsize>,
    unattended: bool,
    open: bool,
    performed: Vec<ActionRecord>,
    held: Vec<DeferredAction>,
}

impl TaskObserver<'_> {
    fn title(&self, proposal: &ProposalReport) -> String {
        if proposal.decision == "rejected" {
            return "A step P@x proposed was not valid".to_owned();
        }
        if proposal.tool == MEMORY_RECALL {
            return "Look at P@x's notes".to_owned();
        }
        self.registry
            .description_of(&proposal.tool)
            .map_or_else(|| proposal.tool.clone(), |text| first_sentence(text, 120))
    }

    fn step(&self, index: usize, proposal: &ProposalReport, before: bool) -> Value {
        let state = match proposal.decision {
            "rejected" => "rejected",
            "allowed" if before => "running",
            "allowed" => match proposal.result {
                Some("ok") => "done",
                Some(_) => "failed",
                None => "refused",
            },
            "refused" => "refused",
            _ => "needs_you",
        };
        let tier = if proposal.decision == "rejected" {
            None
        } else if proposal.tool == COMMAND_TOOL {
            Some(4)
        } else if proposal.tool == MEMORY_RECALL
            || ASSISTANT_TOOLS.contains(&proposal.tool.as_str())
        {
            Some(0)
        } else {
            Some(1)
        };
        json!({
            "index": index,
            "tool": proposal.tool,
            "service": proposal.service,
            "title": self.title(proposal),
            "detail": proposal.description,
            "class": proposal.class,
            "tier": tier,
            "decision": proposal.decision,
            "reason": proposal.reason,
            "state": state,
            "result": proposal.result,
            "exit_code": proposal.exit_code,
            "output_bytes": proposal.output_bytes,
            "truncated": proposal.truncated,
            "withheld": proposal.withheld,
            "error_code": proposal.error_code,
            "message": proposal.message,
            "audit_id": format!("{}#{}", self.task, self.written.load(Ordering::SeqCst)),
        })
    }
}

impl Observer for TaskObserver<'_> {
    fn decided(&mut self, proposal: &ProposalReport) {
        let event = {
            let mut state = self.core.state();
            let Some(record) = state
                .tasks
                .iter_mut()
                .find(|record| record.id == self.task && record.outcome.is_none())
            else {
                return;
            };
            let step = self.step(record.steps.len() + 1, proposal, true);
            record.steps.push(step.clone());
            Event::ActionProposed {
                task: self.task.to_owned(),
                step,
            }
        };
        self.open = true;
        self.core.parts.events.emit(&event);
    }

    fn held(&mut self, action: &HeldAction) {
        let reason = match action.reason.as_str() {
            "unattended_ask_every_action_mode" => DeferReason::AskEveryAction,
            "unattended_without_standing_approval" | "trusted_path_unavailable" => {
                DeferReason::NoStandingApproval
            }
            _ => DeferReason::Other,
        };
        let detail = match reason {
            DeferReason::NoStandingApproval => {
                "Nobody was there to approve this, and no approval given in advance is honoured \
                 in this version."
            }
            DeferReason::AskEveryAction => {
                "P@x is set to ask before every action, and nobody was there to ask."
            }
            _ => "This needs you, and nobody was there.",
        };
        self.held.push(DeferredAction {
            action: ActionRecord {
                service: action.service.clone(),
                tool: action.tool.clone(),
                arguments: action.arguments.clone(),
                arguments_withheld: false,
            },
            reason,
            detail: detail.to_owned(),
        });
    }

    fn item(&mut self, item: &ReportItem) {
        let event = {
            let mut state = self.core.state();
            let Some(record) = state
                .tasks
                .iter_mut()
                .find(|record| record.id == self.task && record.outcome.is_none())
            else {
                // The task was cancelled: what its thread still does is not
                // shown.
                return;
            };
            match item {
                ReportItem::Said { text } => {
                    record
                        .texts
                        .push(json!({ "kind": "interim", "text": text }));
                    Event::TextAdded {
                        task: self.task.to_owned(),
                        kind: "interim",
                        text: text.clone(),
                    }
                }
                ReportItem::Notice { code, message } => {
                    record
                        .texts
                        .push(json!({ "kind": "notice", "text": message, "error_code": code }));
                    Event::TextAdded {
                        task: self.task.to_owned(),
                        kind: "notice",
                        text: message.clone(),
                    }
                }
                ReportItem::Proposal(proposal) => {
                    let step = if self.open && !record.steps.is_empty() {
                        let index = record.steps.len();
                        let step = self.step(index, proposal, false);
                        record.steps[index - 1] = step.clone();
                        step
                    } else {
                        let step = self.step(record.steps.len() + 1, proposal, false);
                        record.steps.push(step.clone());
                        step
                    };
                    self.open = false;
                    if self.unattended
                        && proposal.executed
                        && let Some(service) = &proposal.service
                    {
                        self.performed.push(ActionRecord {
                            service: service.clone(),
                            tool: proposal.tool.clone(),
                            arguments: Map::new(),
                            arguments_withheld: false,
                        });
                    }
                    Event::ActionCompleted {
                        task: self.task.to_owned(),
                        step,
                    }
                }
            }
        };
        self.core.parts.events.emit(&event);
    }
}

/// Runs scheduled work for the schedule runner.
struct CoreRunner<'c> {
    core: &'c Core,
}

impl Runner for CoreRunner<'_> {
    fn remind(
        &mut self,
        schedule: &Schedule,
        message: &str,
        scheduled_for: Timestamp,
        late: bool,
    ) -> Result<(), String> {
        // A wall-clock schedule says its time in its own zone; any other in
        // the zone the computer is set to.
        let zone = match schedule.trigger() {
            Trigger::Daily { zone, .. }
            | Trigger::Weekly { zone, .. }
            | Trigger::Monthly { zone, .. } => zone.clone(),
            _ => (self.core.parts.zone)(),
        };
        let due = scheduled_for
            .wall_clock_in(&zone)
            .or_else(|| scheduled_for.wall_clock_in("UTC"));
        let late = match (&due, late) {
            (Some((date, time)), true) => Some((date.as_str(), time.as_str())),
            _ => None,
        };
        let shown = notify::reminder(schedule.name(), message, late);
        self.core.parts.notifier.notify(&shown).map(|_| ())
    }

    fn run_task(
        &mut self,
        schedule: &Schedule,
        request: &str,
        _scheduled_for: Timestamp,
        _late: bool,
    ) -> Option<TaskRun> {
        self.core.run_scheduled(schedule, request)
    }
}

impl Core {
    /// The service, with its settings, the model list and the spending
    /// record read from their files.
    ///
    /// Nothing that cannot be read stops the service from answering: it
    /// stays up, switched off, and says what is wrong.
    pub fn new(parts: Parts) -> Arc<Core> {
        let (settings, settings_fault) = match Settings::load(&parts.paths.settings) {
            Ok(settings) => (settings, None),
            Err(problem) => (Settings::default(), Some(problem)),
        };
        let (models, models_fault) = match ModelList::load(&parts.paths.models) {
            Ok(models) => (models, None),
            Err(problem) => (ModelList::empty(), Some(problem)),
        };
        let ledger = Ledger::load(&parts.paths.spending).map(|ledger| Arc::new(Mutex::new(ledger)));
        Arc::new(Core {
            parts,
            state: Mutex::new(State {
                settings,
                settings_fault,
                models,
                models_fault,
                running: None,
                tasks: VecDeque::new(),
                proposals: Vec::new(),
                history: Vec::new(),
                locked: None,
                key_set: None,
                counter: 0,
            }),
            ledger,
            ticking: Mutex::new(()),
            to_tell: Mutex::new(Vec::new()),
            poked: Mutex::new(false),
            wake: Condvar::new(),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn now(&self) -> Timestamp {
        self.parts.clock.now()
    }

    fn emit(&self, event: Event) {
        self.parts.events.emit(&event);
    }

    fn memory(&self) -> Memory<FileStore<StdFs>> {
        Memory::new(FileStore::new(self.parts.paths.store.clone(), StdFs))
    }

    fn book(&self) -> ScheduleBook<FileStore<StdFs>> {
        ScheduleBook::new(FileStore::new(self.parts.paths.store.clone(), StdFs))
    }

    /// Write that the person changed something: what, never a value that is
    /// a secret. A trail that cannot be written does not undo the change;
    /// it is the person's own act, not an action of P@x.
    fn record_change(&self, what: &str, value: &str) {
        if let Ok(mut trail) = FileTrail::open(&self.parts.paths.audit) {
            let event = TrailEvent::new(EventKind::SettingChanged, "settings", "-")
                .with("TUR_AI_SETTING", crate::trail::identifier(what))
                .with("TUR_AI_VALUE", crate::trail::identifier(value));
            let _ = trail.event(&event);
        }
    }

    /// Change the settings and save them. If they cannot be saved, the
    /// change is not made.
    fn change_settings(
        &self,
        change: impl FnOnce(&mut Settings) -> Result<(), Fault>,
    ) -> Result<(), Fault> {
        let mut state = self.state();
        if let Some(problem) = &state.settings_fault {
            return Err(problem.clone());
        }
        let mut changed = state.settings.clone();
        change(&mut changed)?;
        changed.save(&self.parts.paths.settings)?;
        state.settings = changed;
        Ok(())
    }

    // ------------------------------------------------------------ the switch

    /// The consent text for a provider, and its version.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` if no provider has that identifier.
    pub fn consent(&self, provider: &str) -> Result<(String, String), Fault> {
        let (entry, origin) = self.provider_entry(provider)?;
        // A gateway address in the settings replaces the entry's for the
        // provider in use: TUR's review of the entry says nothing about it.
        let gateway = {
            let state = self.state();
            if state.settings.provider.as_deref() == Some(provider) {
                state.settings.base_url.clone()
            } else {
                None
            }
        };
        let terms = entry.reviewed_terms(origin, gateway.is_some());
        let subject = Subject {
            name: &entry.name,
            adapter: entry.adapter,
            native: entry.is_native(),
            terms_url: terms
                .and_then(|terms| terms.terms_url.as_deref())
                .or(entry.data_terms_url.as_deref()),
            by_hand: origin == Origin::ByHand,
            terms,
            runs_locally: entry.runs_locally(origin, gateway.as_deref()),
        };
        Ok((CONSENT_TEXT_VERSION.to_owned(), consent_text(&subject)))
    }

    /// The entry of a provider, by its identifier.
    fn provider_entry(&self, provider: &str) -> Result<(ProviderEntry, Origin), Fault> {
        let state = self.state();
        find_provider(&state.models, &state.settings, provider)
            .ok_or_else(|| invalid("no provider has that identifier"))
    }

    /// The name of the password store item that holds a provider's key: its
    /// entry's, or for an identifier no entry has yet, the identifier, so
    /// that a key can be stored before the provider is added.
    fn keyring_item(&self, provider: &str) -> Result<String, Fault> {
        match self.provider_entry(provider) {
            Ok((entry, _)) => Ok(entry.keyring_item().to_owned()),
            Err(_) if is_provider_id(provider) => Ok(provider.to_owned()),
            Err(fault) => Err(fault),
        }
    }

    /// Switch P@x on. The caller names the version of the consent text the
    /// person read.
    ///
    /// # Errors
    ///
    /// See [`Settings::enable`].
    pub fn enable(&self, consent_version: &str) -> Result<(), Fault> {
        let now = self.now().seconds();
        self.change_settings(|settings| settings.enable(consent_version, now))?;
        self.record_change("enabled", "on");
        self.emit(Event::StateChanged);
        self.poke();
        Ok(())
    }

    /// Switch P@x off. A running task is cancelled, and the timer is taken
    /// away: nothing fires while P@x is off.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the settings cannot be saved.
    pub fn disable(&self) -> Result<(), Fault> {
        self.change_settings(|settings| {
            settings.disable();
            Ok(())
        })?;
        self.record_change("enabled", "off");
        let _ = self.cancel_task("");
        let _ = self.parts.waker.arm(None);
        self.emit(Event::StateChanged);
        Ok(())
    }

    /// Choose the provider and the model.
    ///
    /// # Errors
    ///
    /// See [`Settings::set_provider`].
    pub fn set_provider(&self, provider: &str, model: &str) -> Result<(), Fault> {
        let models = self.state().models.clone();
        self.change_settings(|settings| settings.set_provider(provider, model, &models))?;
        self.state().key_set = None;
        self.record_change("provider", provider);
        self.emit(Event::StateChanged);
        Ok(())
    }

    /// Set the spending limits from a JSON object with any of `task_cost`,
    /// `day_cost`, `task_tokens` and `day_tokens`. A limit that is `null`
    /// goes back to its default; one that is absent is left as it is.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` if the text is not such an object or an amount
    /// is out of range.
    pub fn set_limits(&self, limits: &str) -> Result<(), Fault> {
        let value: Value = crate::strict::value_from_str(limits)
            .map_err(|_| invalid("the limits are not one JSON object"))?;
        let Some(given) = value.as_object() else {
            return Err(invalid("the limits are not one JSON object"));
        };
        self.change_settings(|settings| {
            let mut limits: Limits = settings.limits;
            for (name, amount) in given {
                let cost = || match amount {
                    Value::Null => Ok(None),
                    other => other
                        .as_f64()
                        .map(Some)
                        .ok_or_else(|| invalid(format!("the limit {name} is not a number"))),
                };
                let tokens = || match amount {
                    Value::Null => Ok(None),
                    other => other
                        .as_u64()
                        .map(Some)
                        .ok_or_else(|| invalid(format!("the limit {name} is not a whole number"))),
                };
                match name.as_str() {
                    "task_cost" => limits.task_cost = cost()?,
                    "day_cost" => limits.day_cost = cost()?,
                    "task_tokens" => limits.task_tokens = tokens()?,
                    "day_tokens" => limits.day_tokens = tokens()?,
                    _ => return Err(invalid("the limits name a limit that does not exist")),
                }
            }
            limits.validate()?;
            settings.limits = limits;
            Ok(())
        })?;
        self.record_change("limits", "changed");
        self.emit(Event::StateChanged);
        Ok(())
    }

    /// Every provider, by identifier, with its models: the model list's,
    /// the person's own, and the adapter families' companies no list names.
    pub fn models(&self) -> Value {
        let state = self.state();
        let providers: Vec<Value> = providers(&state.models, &state.settings)
            .iter()
            .map(|(entry, origin)| {
                let models: Vec<Value> = entry
                    .models
                    .iter()
                    .map(|model| {
                        let abilities = entry.abilities(&model.model);
                        json!({
                            "model": model.model,
                            "label": model.label,
                            "max_output_tokens": model.max_output_tokens,
                            "tested_with_tur": model.tested_with_tur,
                            "prices": model.prices,
                            "capabilities": entry.declared(&model.model),
                            "can_act": abilities.act,
                            "can_see": abilities.see,
                            "notices": abilities.notices(),
                        })
                    })
                    .collect();
                let terms = entry.reviewed_terms(*origin, false);
                json!({
                    "id": entry.id,
                    "adapter": entry.adapter.as_str(),
                    "name": entry.name,
                    "origin": origin.as_str(),
                    "base_url": entry.base_url,
                    "keyring_item": entry.keyring_item(),
                    "key_required": entry.key_required,
                    "capabilities": entry.capabilities,
                    "data_terms": terms,
                    "data_terms_reviewed": terms.is_some_and(|terms| terms.reviewed_on.is_some()),
                    "runs_locally": entry.runs_locally(*origin, None),
                    "data_terms_url": entry.data_terms_url,
                    "key_url": entry.key_url,
                    "prices_url": entry.prices_url,
                    "models": models,
                })
            })
            .collect();
        json!({
            "status": state.models.status,
            "prices_as_of": state.models.prices_as_of,
            "note": state.models.note,
            "problem": state.models_fault.as_ref().map(|problem| json!({
                "error_code": problem.code.as_str(),
                "message": problem.detail,
            })),
            "providers": providers,
        })
    }

    // --------------------------------------------------------------- the key

    /// Store the key of a provider in the password store, in place of any
    /// key already there.
    ///
    /// # Errors
    ///
    /// See [`KeyStore::set`].
    pub fn set_key(&self, provider: &str, key: &str) -> Result<(), Fault> {
        let item = self.keyring_item(provider)?;
        self.parts.keys.set(&item, key)?;
        self.state().key_set = None;
        self.record_change("key", "set");
        self.emit(Event::StateChanged);
        Ok(())
    }

    /// Remove the key of a provider. `Ok(false)` if there was none.
    ///
    /// # Errors
    ///
    /// See [`KeyStore::remove`].
    pub fn remove_key(&self, provider: &str) -> Result<bool, Fault> {
        let item = self.keyring_item(provider)?;
        let removed = self.parts.keys.remove(&item)?;
        self.state().key_set = None;
        self.record_change("key", "removed");
        self.emit(Event::StateChanged);
        Ok(removed)
    }

    /// Whether a key is stored for a provider.
    ///
    /// # Errors
    ///
    /// See [`KeyStore::is_set`].
    pub fn has_key(&self, provider: &str) -> Result<bool, Fault> {
        let item = self.keyring_item(provider)?;
        let set = self.parts.keys.is_set(&item)?;
        let mut state = self.state();
        if state.settings.provider.as_deref() == Some(provider) {
            state.key_set = Some(set);
        }
        Ok(set)
    }

    // ------------------------------------------------------------- the state

    /// The state of the service in one word: `off`, `idle`, `working` or
    /// `paused`.
    pub fn state_name(&self) -> &'static str {
        let state = self.state();
        match &state.running {
            _ if !state.settings.enabled || state.settings_fault.is_some() => "off",
            None => "idle",
            Some(running) if running.paused || running.paused_by_lock => "paused",
            Some(_) => "working",
        }
    }

    /// Whether P@x is switched on.
    pub fn is_enabled(&self) -> bool {
        let state = self.state();
        state.settings.enabled && state.settings_fault.is_none()
    }

    /// Everything a settings page or a panel header shows.
    pub fn status(&self) -> Value {
        // Whether a key is set is asked of the password store each time: a
        // key can be set or removed by another program. Asking never makes
        // the store prompt the person.
        let provider = self.state().settings.provider.clone();
        if let Some(provider) = provider {
            let found = self
                .keyring_item(&provider)
                .and_then(|item| self.parts.keys.is_set(&item))
                .ok();
            self.state().key_set = found;
        }

        let state_name = self.state_name();
        let state = self.state();
        let settings = &state.settings;
        let current = settings.current_provider(&state.models);
        let listed = match (&current, &settings.model) {
            (Some((entry, _)), Some(model)) => entry.model(model),
            _ => None,
        };
        let prices = listed.and_then(|entry| entry.prices.as_ref());
        let key_needed = current.as_ref().is_none_or(|(entry, _)| entry.key_required);
        let abilities = match (&current, &settings.model) {
            (Some((entry, _)), Some(model)) => Some(entry.abilities(model)),
            _ => None,
        };
        let day_limit = settings.day_limit(prices.is_some());
        let today = day_of(self.now(), &(self.parts.zone)());
        let (used_today, used_month, ledger_problem) = match &self.ledger {
            Ok(ledger) => {
                let ledger = ledger
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (
                    ledger.day(&today),
                    ledger.month(today.get(..7).unwrap_or_default()),
                    None,
                )
            }
            Err(problem) => (
                crate::spending::DayUse::default(),
                crate::spending::DayUse::default(),
                Some(problem.clone()),
            ),
        };
        let problem = state
            .settings_fault
            .clone()
            .or(ledger_problem)
            .or_else(|| settings.refusal());
        let config = settings.provider_config(&state.models).ok();
        let task_limit = config.as_ref().and_then(|config| config.task_limit);
        let used = |used: &crate::spending::DayUse| {
            json!({
                "cost": used.cost,
                "input_tokens": used.input_tokens,
                "output_tokens": used.output_tokens,
                "responses": used.responses,
            })
        };
        json!({
            "interface_version": INTERFACE_VERSION,
            "enabled": settings.enabled && state.settings_fault.is_none(),
            "state": state_name,
            "ready": problem.is_none() && (state.key_set == Some(true) || !key_needed),
            "problem": problem.as_ref().map(|problem| json!({
                "error_code": problem.code.as_str(),
                "message": problem.detail,
            })),
            "consent": {
                "current": settings.consent_is_current(),
                "text_version": CONSENT_TEXT_VERSION,
                "accepted_version": settings.consent.as_ref().map(|consent| consent.text_version.clone()),
                "accepted_for": settings.consent.as_ref().map(|consent| consent.provider.as_str()),
                "accepted_at": settings.consent.as_ref().map(|consent| consent.accepted_at),
            },
            "provider": settings.provider,
            "provider_name": current.as_ref().map(|(entry, _)| entry.name.clone()),
            "adapter": current.as_ref().map(|(entry, _)| entry.adapter.as_str()),
            "provider_origin": current.as_ref().map(|(_, origin)| origin.as_str()),
            "model": settings.model,
            "model_listed": listed.is_some(),
            "model_tested": listed.is_some_and(|entry| entry.tested_with_tur),
            "price_known": prices.is_some(),
            "key": match state.key_set {
                _ if !key_needed => "not_needed",
                Some(true) => "set",
                Some(false) => "not_set",
                None => "unknown",
            },
            "abilities": abilities.map(|abilities| json!({
                "act": abilities.act,
                "see": abilities.see,
                "notices": abilities.notices(),
            })),
            "session_locked": state.locked,
            "running": state.running.as_ref().map(|running| json!({
                "task": running.id,
                "attended": running.attended,
                "schedule": running.schedule,
                "paused": running.paused || running.paused_by_lock,
            })),
            "spending": {
                "currency": prices.map(|prices| prices.currency.clone()),
                "today": used(&used_today),
                "month": used(&used_month),
                "day": today,
                "day_limit": { "cost": day_limit.cost, "tokens": day_limit.tokens },
                "task_limit": {
                    "cost": task_limit.and_then(|limit| limit.max_cost),
                    "tokens": task_limit.and_then(|limit| limit.max_tokens),
                },
                "day_limit_reached": used_today.reaches(&day_limit),
                "prices_as_of": state.models.prices_as_of,
                "prices_status": state.models.status,
            },
            "proposals": state.proposals.len(),
        })
    }

    /// Tell the service whether the session is locked. A task a person
    /// asked for pauses while it is, and goes on when it no longer is; a
    /// task the person paused themselves stays paused.
    pub fn set_session_locked(&self, locked: Option<bool>) {
        let changed = {
            let mut state = self.state();
            state.locked = locked;
            match (&mut state.running, locked) {
                (Some(running), Some(true)) if running.attended && !running.paused_by_lock => {
                    running.paused_by_lock = true;
                    running.control.pause();
                    true
                }
                (Some(running), Some(false) | None) if running.paused_by_lock => {
                    running.paused_by_lock = false;
                    if !running.paused {
                        running.control.resume();
                    }
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.emit(Event::StateChanged);
        }
    }

    // ----------------------------------------------------------------- tasks

    /// Start a task for a person who is there.
    ///
    /// `options` is a JSON object, or empty: `grant_read`, a list of
    /// folders or files the commands of this task may read; `continue`,
    /// whether the task goes on from the last conversation.
    ///
    /// The task runs on a thread of its own; this returns its identifier at
    /// once.
    ///
    /// # Errors
    ///
    /// Why it cannot start: P@x is off (`TUR-AI-SERVICE-001`), the consent
    /// is not current (`-002`), nothing is configured (`-003`), a task is
    /// running (`-004`), the request or an option is not valid (`-006`),
    /// the session is locked (`-008`), a file cannot be read (`-009`), no
    /// key is set (`TUR-AI-KEY-002`), the password store is not available
    /// (`TUR-AI-KEY-001`), or a folder cannot be granted
    /// (`TUR-AI-SANDBOX-002`).
    pub fn start_task(self: &Arc<Self>, request: &str, options: &str) -> Result<String, Fault> {
        if !is_request(request) {
            return Err(invalid(
                "the request is empty, too long, or holds a control character",
            ));
        }
        let (grant_paths, resume) = Core::task_options(options)?;
        let provider = {
            let state = self.state();
            if let Some(problem) = &state.settings_fault {
                return Err(problem.clone());
            }
            if let Some(refusal) = state.settings.refusal() {
                return Err(refusal);
            }
            if state.locked == Some(true) {
                return Err(coded(TaskCode::ServiceSessionLocked));
            }
            if state.running.is_some() {
                return Err(coded(TaskCode::ServiceBusy));
            }
            state.settings.provider.clone()
        };
        if let Err(problem) = &self.ledger {
            return Err(problem.clone());
        }
        let Some(provider) = provider else {
            return Err(coded(TaskCode::ServiceNotConfigured));
        };
        // The key is looked for before anything starts, so that a missing
        // key is an answer to the call and not the end of a task. A server
        // that takes no key is sent none, and none is looked for.
        let key_needed = !matches!(
            self.provider_entry(&provider),
            Ok((entry, _)) if !entry.key_required
        );
        if key_needed {
            let item = self.keyring_item(&provider)?;
            let key_set = self.parts.keys.is_set(&item)?;
            self.state().key_set = Some(key_set);
            if !key_set {
                return Err(coded(TaskCode::KeyNotSet));
            }
        }
        let rules = GrantRules::new(self.parts.paths.home.as_deref());
        let mut grants = Vec::new();
        for path in &grant_paths {
            let grant = rules.admit(path).map_err(|refusal| {
                fault(
                    TaskCode::SandboxGrantRefused,
                    format!("a folder cannot be granted: {refusal}"),
                )
            })?;
            grants.push(grant);
        }

        let job = self.begin(request, Situation::attended(), None, grants, resume)?;
        let id = job.id.clone();
        let core = Arc::clone(self);
        thread::Builder::new()
            .name("tur-pax-task".to_owned())
            .spawn(move || {
                let ended = core.run_job(&job);
                core.finish(&job, ended);
            })
            .map_err(|error| {
                // The task never ran: the slot is given back.
                self.state().running = None;
                fault(
                    TaskCode::ServiceStorage,
                    format!("the task could not be started: {error}"),
                )
            })?;
        Ok(id)
    }

    fn task_options(options: &str) -> Result<(Vec<String>, bool), Fault> {
        if options.trim().is_empty() {
            return Ok((Vec::new(), false));
        }
        let value: Value = crate::strict::value_from_str(options)
            .map_err(|_| invalid("the options are not one JSON object"))?;
        let Some(given) = value.as_object() else {
            return Err(invalid("the options are not one JSON object"));
        };
        let mut grants = Vec::new();
        let mut resume = false;
        for (name, option) in given {
            match (name.as_str(), option) {
                ("grant_read", Value::Array(paths)) => {
                    for path in paths {
                        let path = path
                            .as_str()
                            .ok_or_else(|| invalid("a granted path is not a text"))?;
                        grants.push(path.to_owned());
                    }
                }
                ("continue", Value::Bool(value)) => resume = *value,
                _ => return Err(invalid("an option is not known or has the wrong type")),
            }
        }
        Ok((grants, resume))
    }

    /// Take the one slot for a task and make its record.
    fn begin(
        &self,
        request: &str,
        situation: Situation,
        schedule: Option<&str>,
        grants: Vec<Grant>,
        resume: bool,
    ) -> Result<Job, Fault> {
        let now = self.now().seconds();
        let attended = schedule.is_none();
        let (job, info) = {
            let mut state = self.state();
            if state.running.is_some() {
                return Err(coded(TaskCode::ServiceBusy));
            }
            state.counter += 1;
            let id = format!("t-{now}-{}", state.counter);
            let control = SharedControl::new();
            let stop = Arc::new(AtomicBool::new(false));
            state.running = Some(Running {
                id: id.clone(),
                control: control.clone(),
                stop: Arc::clone(&stop),
                attended,
                schedule: schedule.map(str::to_owned),
                paused_by_lock: false,
                paused: false,
            });
            let record = TaskRecord {
                id: id.clone(),
                request: request.to_owned(),
                attended,
                schedule: schedule.map(str::to_owned),
                started_at: now,
                finished_at: None,
                outcome: None,
                error_code: None,
                message: String::new(),
                answer: String::new(),
                texts: Vec::new(),
                steps: Vec::new(),
                usage: Usage::default(),
                cost: None,
                currency: None,
            };
            let info = record.to_json(false);
            state.tasks.push_back(record);
            while state.tasks.len() > TASKS_KEPT {
                state.tasks.pop_front();
            }
            (
                Job {
                    id,
                    request: request.to_owned(),
                    situation,
                    grants,
                    resume,
                    control,
                    stop,
                },
                info,
            )
        };
        self.emit(Event::TaskStarted {
            task: job.id.clone(),
            info,
        });
        self.emit(Event::StateChanged);
        Ok(job)
    }

    /// The tool set of a task: what each component ships, what the test
    /// directories add, the command tool, and P@x's own operations.
    fn registry(&self, notes: bool) -> Result<ToolRegistry, Fault> {
        let paths = &self.parts.paths;
        let default = paths.tools.to_str().unwrap_or(DEFAULT_TOOLS_DIR);
        let tools = read_tools(None, default)?;
        let mut manifests = tools.manifests;
        let mut adapters = tools.adapters;
        manifests.extend(read_json_dir(paths.extra_manifests.as_deref())?);
        adapters.extend(read_json_dir(paths.extra_adapters.as_deref())?);
        let registry = ToolRegistry::load(&manifests, &adapters, true).map_err(|problems| {
            problems.into_iter().next().unwrap_or_else(|| {
                fault(TaskCode::ServiceStorage, "the tool set could not be loaded")
            })
        })?;
        Ok(registry.with_assistant_tools(AssistantTools {
            schedules: true,
            notes,
        }))
    }

    /// The person's policy. A file that is there and cannot be used leaves
    /// P@x able to observe only.
    fn policy(&self) -> UserPolicy {
        match fs::read_to_string(&self.parts.paths.policy) {
            Ok(text) => UserPolicy::from_json(&text).unwrap_or_else(|_| UserPolicy::fail_closed()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => UserPolicy::default(),
            Err(_) => UserPolicy::fail_closed(),
        }
    }

    /// Run one job to its end. This is the only place a task loop is built.
    fn run_job(&self, job: &Job) -> Ended {
        let not_started = |problem: Fault| Ended {
            outcome: "not_started".to_owned(),
            code: Some(problem.code.to_string()),
            message: problem.detail,
            answer: String::new(),
            usage: Usage::default(),
            cost: None,
            currency: None,
            performed: Vec::new(),
            held: Vec::new(),
        };

        let (settings, models) = {
            let state = self.state();
            (state.settings.clone(), state.models.clone())
        };
        if let Some(refusal) = settings.refusal() {
            return not_started(refusal);
        }
        let ledger = match &self.ledger {
            Ok(ledger) => Arc::clone(ledger),
            Err(problem) => return not_started(problem.clone()),
        };
        let config: ProviderConfig = match settings.provider_config(&models) {
            Ok(config) => config,
            Err(problem) => return not_started(problem),
        };
        let Some((entry, _)) = settings.current_provider(&models) else {
            return not_started(coded(TaskCode::ServiceNotConfigured));
        };
        // A server that takes no key, such as a model server on this
        // computer, is sent none.
        let connected = if entry.key_required {
            match self.parts.keys.key(entry.keyring_item()) {
                Ok(key) => connect(&config, key),
                Err(problem) => return not_started(problem),
            }
        } else {
            connect_without_key(&config)
        };
        let provider = match connected {
            Ok(provider) => provider,
            Err(error) => {
                return not_started(Fault {
                    code: error.kind().error_code(),
                    detail: error.to_string(),
                });
            }
        };
        // What the model's entry declares decides what P@x offers it: a
        // model that cannot call tools is offered none, and the person is
        // told so in plain words.
        let abilities = settings
            .model
            .as_deref()
            .map(|model| entry.abilities(model));
        let can_act = abilities.is_none_or(|abilities| abilities.act != Some(false));
        let memory = self.memory();
        let memory_on = memory.is_enabled().unwrap_or(false);
        let recalled = if memory_on {
            memory
                .recall(&job.request, RecallBudget::default())
                .unwrap_or_else(|_| RecalledNotes::none())
        } else {
            RecalledNotes::none()
        };
        let registry = if can_act {
            self.registry(memory_on)
        } else {
            ToolRegistry::load(&[], &[], false).map_err(|problems| {
                problems.into_iter().next().unwrap_or_else(|| {
                    fault(TaskCode::ServiceStorage, "the tool set could not be loaded")
                })
            })
        };
        let registry = match registry {
            Ok(registry) => registry,
            Err(problem) => return not_started(problem),
        };
        let policy = self.policy();
        let rules = GrantRules::new(self.parts.paths.home.as_deref());
        let file = match FileTrail::open(&self.parts.paths.audit) {
            Ok(file) => file,
            Err(error) => {
                return not_started(Fault::from(Problem::new(
                    Code::AuditUnavailable,
                    format!("{}: {error}", self.parts.paths.audit.display()),
                )));
            }
        };

        let written = Arc::new(AtomicUsize::new(0));
        let mut trail = CountingTrail {
            inner: file,
            written: Arc::clone(&written),
        };
        let mut executor = (self.parts.executors)(Arc::clone(&job.stop));
        let clock = SystemTaskClock::new();
        let mut meter = SpendMeter::for_config(&config);
        let currency = config.prices.as_ref().map(|prices| prices.currency.clone());
        let mut budget = TaskBudget::new(
            ledger,
            settings.day_limit(config.prices.is_some()),
            Arc::clone(&self.parts.clock),
            Arc::clone(&self.parts.zone),
            currency,
        );
        let mut assistant = CoreAssistant { core: self };
        let unattended = job.situation.presence != crate::context::Presence::Attended;
        let mut observer = TaskObserver {
            core: self,
            task: &job.id,
            registry: &registry,
            written,
            unattended,
            open: false,
            performed: Vec::new(),
            held: Vec::new(),
        };
        if !can_act {
            observer.item(&ReportItem::Notice {
                code: None,
                message: MODEL_CANNOT_ACT.to_owned(),
            });
        }
        let mut history = if job.resume {
            let state = self.state();
            if state.history.len() > CONVERSATION_MAX_MESSAGES {
                Vec::new()
            } else {
                state.history.clone()
            }
        } else {
            Vec::new()
        };

        let report = {
            let mut orchestrator = Orchestrator {
                provider: &*provider,
                config: &config,
                registry: &registry,
                policy: &policy,
                situation: job.situation,
                limits: TaskLimits::default(),
                rules: &rules,
                grants: &job.grants,
                executor: &mut *executor,
                trail: &mut trail,
                clock: &clock,
                control: &job.control,
                observer: &mut observer,
                budget: &mut budget,
                assistant: &mut assistant,
            };
            let request = TaskRequest {
                task_id: &job.id,
                request: &job.request,
                recalled: &recalled,
            };
            orchestrator.run(&request, &mut history, &mut meter)
        };

        if report.outcome == TaskOutcome::Completed && !unattended {
            self.state().history = history;
        }
        Ended {
            outcome: report.outcome.as_str().to_owned(),
            code: report.error_code,
            message: report.message,
            answer: report.answer,
            usage: report.usage,
            cost: report.cost,
            currency: report.currency,
            performed: observer.performed,
            held: observer.held,
        }
    }

    /// Record how a job ended, give the slot back and tell the listeners.
    /// A task the person cancelled has been reported already: what its
    /// thread found afterwards only brings its usage up to date.
    fn finish(&self, job: &Job, ended: Ended) {
        let now = self.now().seconds();
        let events = {
            let mut state = self.state();
            if state
                .running
                .as_ref()
                .is_some_and(|running| running.id == job.id)
            {
                state.running = None;
            }
            let Some(record) = state.tasks.iter_mut().find(|record| record.id == job.id) else {
                return;
            };
            record.usage = ended.usage;
            record.cost = ended.cost;
            record.currency = ended.currency;
            if record.outcome.is_some() {
                Vec::new()
            } else {
                record.finished_at = Some(now);
                record.outcome = Some(ended.outcome);
                record.error_code = ended.code;
                record.message = ended.message;
                record.answer = ended.answer;
                let mut events = Vec::new();
                if !record.answer.is_empty() {
                    record
                        .texts
                        .push(json!({ "kind": "answer", "text": record.answer }));
                    events.push(Event::TextAdded {
                        task: record.id.clone(),
                        kind: "answer",
                        text: record.answer.clone(),
                    });
                }
                events.push(Event::TaskFinished {
                    task: record.id.clone(),
                    result: record.to_json(false),
                });
                events.push(Event::StateChanged);
                events
            }
        };
        for event in events {
            self.emit(event);
        }
        // A schedule that came due while the task ran can have its turn.
        self.poke();
    }

    /// Stop a task at once. With an empty identifier, the task that is
    /// running, whatever it is.
    ///
    /// The task is reported as cancelled now. Its thread stops at its next
    /// look at the control: a sandboxed program it was running is killed,
    /// a response still on its way is discarded when it arrives, and
    /// nothing further is proposed, decided or run.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if no such task is running.
    pub fn cancel_task(&self, id: &str) -> Result<(), Fault> {
        let now = self.now().seconds();
        let event = {
            let mut state = self.state();
            let Some(running) = state
                .running
                .take_if(|running| id.is_empty() || running.id == id)
            else {
                return Err(not_found("running task"));
            };
            running.control.cancel();
            running.stop.store(true, Ordering::SeqCst);
            let Some(record) = state
                .tasks
                .iter_mut()
                .find(|record| record.id == running.id)
            else {
                return Ok(());
            };
            record.finished_at = Some(now);
            record.outcome = Some(TaskOutcome::Cancelled.as_str().to_owned());
            record.error_code = Some(TaskCode::TaskCancelled.as_str().to_owned());
            record.message = TaskCode::TaskCancelled.user_message().to_owned();
            Event::TaskFinished {
                task: record.id.clone(),
                result: record.to_json(false),
            }
        };
        self.emit(event);
        self.emit(Event::StateChanged);
        self.poke();
        Ok(())
    }

    /// Pause the running task at its next step.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if no task is running.
    pub fn pause(&self) -> Result<(), Fault> {
        {
            let mut state = self.state();
            let Some(running) = &mut state.running else {
                return Err(not_found("running task"));
            };
            running.paused = true;
            running.control.pause();
        }
        self.emit(Event::StateChanged);
        Ok(())
    }

    /// Let a task the person paused go on. A task that is paused because
    /// the session is locked stays paused until it is unlocked.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if no task is running.
    pub fn resume(&self) -> Result<(), Fault> {
        {
            let mut state = self.state();
            let Some(running) = &mut state.running else {
                return Err(not_found("running task"));
            };
            running.paused = false;
            if !running.paused_by_lock {
                running.control.resume();
            }
        }
        self.emit(Event::StateChanged);
        Ok(())
    }

    /// What the service remembers of a task: its request, its steps, its
    /// texts and how it ended.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if the task is not among the last
    /// [`TASKS_KEPT`].
    pub fn task(&self, id: &str) -> Result<Value, Fault> {
        let state = self.state();
        let paused = state
            .running
            .as_ref()
            .is_some_and(|running| running.id == id && (running.paused || running.paused_by_lock));
        state
            .tasks
            .iter()
            .find(|record| record.id == id)
            .map(|record| record.to_json(paused))
            .ok_or_else(|| not_found("task"))
    }

    /// The audit trail as a list a person can read: the last `limit` tasks,
    /// newest first, each with every action, its class and its decision.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the trail cannot be read.
    pub fn activity(&self, limit: u32) -> Result<Value, Fault> {
        let path = &self.parts.paths.audit;
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(fault(
                    TaskCode::ServiceStorage,
                    format!("{}: {error}", path.display()),
                ));
            }
        };
        // A long trail is read from its end, starting at a whole line.
        let start = bytes.len().saturating_sub(ACTIVITY_MAX_BYTES);
        let tail = &bytes[start..];
        let tail = if start == 0 {
            tail
        } else {
            let first_line_end = tail
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(tail.len(), |at| at + 1);
            &tail[first_line_end..]
        };
        Ok(json!(activity_from(
            &String::from_utf8_lossy(tail),
            limit as usize
        )))
    }

    // ------------------------------------------------- P@x's own operations

    fn propose(
        &self,
        task: &str,
        attended: bool,
        reminder: bool,
        arguments: &Map<String, Value>,
    ) -> AssistantOutcome {
        let now = self.now();
        let zone = (self.parts.zone)();
        let proposal = match draft_proposal(reminder, arguments, now, &zone, task) {
            Ok(proposal) => proposal,
            Err(reason) => return AssistantOutcome::failed(reason),
        };
        let (id, details) = {
            let mut state = self.state();
            if state.proposals.len() >= PROPOSALS_MAX {
                return AssistantOutcome::failed(
                    "too many proposals are already waiting for the person",
                );
            }
            state.counter += 1;
            let id = format!("p-{}", state.counter);
            let pending = PendingProposal {
                id: id.clone(),
                proposal,
                proposed_at: now,
                attended,
            };
            let details = proposal_json(&id, &pending);
            state.proposals.push(pending);
            (id, details)
        };
        self.emit(Event::ScheduleProposed {
            proposal: id.clone(),
            details,
        });
        self.emit(Event::SchedulesChanged);
        AssistantOutcome {
            ok: true,
            output: json!({
                "proposal": id,
                "status": "waiting_for_the_person",
                "note": "Nothing is scheduled. The person has been shown the proposal and decides.",
            }),
            detail: None,
        }
    }

    /// Keep a note a model asked for: as an entry taken from content, which
    /// waits for the person and is not recalled until they keep it. The
    /// kind is fixed here. A model has no way to say that the person stated
    /// something.
    fn keep_note(&self, task: &str, arguments: &Map<String, Value>) -> AssistantOutcome {
        let Some(text) = text_of(arguments, "text") else {
            return AssistantOutcome::failed("the text of the note is missing");
        };
        let _guard = self.state();
        let mut memory = self.memory();
        let entry = NewEntry::from_content(text, "written by P@x during a task").with_task(task);
        match memory.add(entry, self.now()) {
            Ok(entry) => {
                drop(_guard);
                self.emit(Event::MemoryChanged);
                AssistantOutcome {
                    ok: true,
                    output: json!({
                        "note": entry.id().as_str(),
                        "status": "waiting_for_the_person",
                        "note_is_kept": false,
                    }),
                    detail: None,
                }
            }
            Err(problem) => AssistantOutcome::failed(format!(
                "{}: {}",
                problem.code.as_str(),
                problem.code.title()
            )),
        }
    }

    // ------------------------------------------------------------- schedules

    /// The proposals that wait for the person.
    pub fn proposals(&self) -> Value {
        let state = self.state();
        let list: Vec<Value> = state
            .proposals
            .iter()
            .map(|pending| proposal_json(&pending.id, pending))
            .collect();
        json!(list)
    }

    fn create(&self, proposal: Proposal) -> Result<Schedule, Fault> {
        let at = self.now();
        let confirmation = ByTheCaller {
            shown: proposal.clone(),
            at,
        };
        let schedule = {
            let _guard = self.state();
            self.book()
                .create(proposal, &confirmation)
                .map_err(memory_fault)?
        };
        self.record_change("schedule", "created");
        self.emit(Event::SchedulesChanged);
        self.poke();
        Ok(schedule)
    }

    /// Create the schedule P@x proposed. The proposal is used as it was
    /// shown; it goes away once the schedule exists.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if there is no such proposal; the memory
    /// layer's `TUR-AI-SCHEDULE-*` if it cannot become a schedule, for
    /// example because its time has passed. The proposal then stays.
    pub fn confirm_proposal(&self, id: &str) -> Result<String, Fault> {
        let proposal = {
            let state = self.state();
            state
                .proposals
                .iter()
                .find(|pending| pending.id == id)
                .map(|pending| pending.proposal.clone())
                .ok_or_else(|| not_found("proposal"))?
        };
        let schedule = self.create(proposal)?;
        self.state().proposals.retain(|pending| pending.id != id);
        self.emit(Event::SchedulesChanged);
        Ok(schedule.id().to_string())
    }

    /// Drop a proposal.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-007` if there is no such proposal.
    pub fn dismiss_proposal(&self, id: &str) -> Result<(), Fault> {
        {
            let mut state = self.state();
            let before = state.proposals.len();
            state.proposals.retain(|pending| pending.id != id);
            if state.proposals.len() == before {
                return Err(not_found("proposal"));
            }
        }
        self.emit(Event::SchedulesChanged);
        Ok(())
    }

    /// Create a schedule the person wrote or changed themselves, from the
    /// JSON of a proposal (`schemas/pax-schedule.schema.json`, the
    /// proposal's part).
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` if the text is not a proposal, or asks for a
    /// standing approval: none can be granted in this version. Otherwise
    /// the memory layer's `TUR-AI-SCHEDULE-*`.
    pub fn create_schedule(&self, proposal: &str) -> Result<String, Fault> {
        let value: Value = crate::strict::value_from_str(proposal)
            .map_err(|_| invalid("the proposal is not one JSON object"))?;
        let mut proposal: Proposal = serde_json::from_value(value)
            .map_err(|_| invalid("the proposal is not of the form of a schedule proposal"))?;
        if let ProposedAction::Task { approvals, .. } = &proposal.action
            && !approvals.is_empty()
        {
            return Err(invalid(
                "a standing approval cannot be granted in this version; leave the approvals out",
            ));
        }
        proposal.proposed_by_task = None;
        Ok(self.create(proposal)?.id().to_string())
    }

    /// Remove a schedule and its run records.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` if the text is not a schedule identifier; the
    /// memory layer's `TUR-AI-SCHEDULE-001` if there is no such schedule.
    pub fn remove_schedule(&self, id: &str) -> Result<(), Fault> {
        let id =
            ScheduleId::parse(id).ok_or_else(|| invalid("that is not a schedule identifier"))?;
        let edit = ScheduleEdit::Remove { id };
        let confirmation = ByTheCaller {
            shown: edit.clone(),
            at: self.now(),
        };
        {
            let _guard = self.state();
            self.book()
                .apply(&edit, &confirmation)
                .map_err(memory_fault)?;
        }
        self.record_change("schedule", "removed");
        self.emit(Event::SchedulesChanged);
        self.poke();
        Ok(())
    }

    /// The schedules, each with its next occurrence and its last run.
    ///
    /// # Errors
    ///
    /// The memory layer's `TUR-AI-SCHEDULE-*` if the store cannot be read.
    pub fn schedules(&self) -> Result<Value, Fault> {
        let now = self.now();
        let book = self.book();
        let listing = book.list().map_err(memory_fault)?;
        let mut list = Vec::new();
        for schedule in &listing.schedules {
            let runs = book.runs(Some(schedule.id())).map_err(memory_fault)?.runs;
            let last = runs.last().map(|run| {
                json!({
                    "run": run.id.as_str(),
                    "started_at": run.started_at.seconds(),
                    "finished_at": run.finished_at.seconds(),
                    "scheduled_for": run.scheduled_for.map(Timestamp::seconds),
                    "outcome": outcome_name(run.outcome),
                    "detail": run.detail,
                    "held": run.deferred.len(),
                })
            });
            let next = book.next_run(schedule.id(), now).ok();
            let (kind, text) = match schedule.action() {
                ScheduledAction::Reminder { message } => ("reminder", message),
                ScheduledAction::Task { request, .. } => ("task", request),
            };
            list.push(json!({
                "id": schedule.id().as_str(),
                "name": schedule.name(),
                "kind": kind,
                "text": text,
                "trigger": schedule.trigger(),
                "trigger_text": schedule.trigger().describe(),
                "missed_runs": schedule.missed_runs().as_str(),
                "created_at": schedule.created_at().seconds(),
                "standing_approvals": schedule.standing_approvals().len(),
                "next": next,
                "last_run": last,
                "runs": runs.len(),
            }));
        }
        let damaged: Vec<Value> = listing
            .damaged
            .iter()
            .map(|file| json!({ "id": file.id, "reason": file.reason }))
            .collect();
        Ok(json!({ "schedules": list, "damaged": damaged }))
    }

    /// "While you were away": the runs the person has not been shown yet.
    /// A reminder that was simply shown is not among them.
    ///
    /// # Errors
    ///
    /// The memory layer's `TUR-AI-SCHEDULE-*` if the store cannot be read.
    pub fn away(&self) -> Result<Value, Fault> {
        let book = self.book();
        let schedules = book.list().map_err(memory_fault)?.schedules;
        let runs = book.runs(None).map_err(memory_fault)?.runs;
        let mut list = Vec::new();
        for run in runs.iter().filter(|run| run.presented_at.is_none()) {
            let Some(schedule) = schedules
                .iter()
                .find(|schedule| *schedule.id() == run.schedule)
            else {
                continue;
            };
            if schedule.is_reminder() && run.outcome == RunOutcome::Completed {
                continue;
            }
            let held: Vec<Value> = run
                .deferred
                .iter()
                .map(|deferred| {
                    json!({
                        "service": deferred.action.service,
                        "tool": deferred.action.tool,
                        "arguments": deferred.action.arguments,
                        "arguments_withheld": deferred.action.arguments_withheld,
                        "reason": deferred.reason,
                        "detail": deferred.detail,
                    })
                })
                .collect();
            let performed: Vec<Value> = run
                .performed
                .iter()
                .map(|action| json!({ "service": action.service, "tool": action.tool }))
                .collect();
            list.push(json!({
                "run": run.id.as_str(),
                "schedule": run.schedule.as_str(),
                "name": schedule.name(),
                "kind": if schedule.is_reminder() { "reminder" } else { "task" },
                "scheduled_for": run.scheduled_for.map(Timestamp::seconds),
                "started_at": run.started_at.seconds(),
                "finished_at": run.finished_at.seconds(),
                "outcome": outcome_name(run.outcome),
                "detail": run.detail,
                "performed": performed,
                "held": held,
            }));
        }
        Ok(json!(list))
    }

    /// Record that the person has been shown some runs.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` if a text is not a run identifier; the memory
    /// layer's `TUR-AI-SCHEDULE-001` if there is no such run.
    pub fn mark_away_seen(&self, runs: &[String]) -> Result<(), Fault> {
        let now = self.now();
        {
            let _guard = self.state();
            let mut book = self.book();
            for run in runs {
                let id =
                    RunId::parse(run).ok_or_else(|| invalid("that is not a run identifier"))?;
                book.mark_presented(&id, now).map_err(memory_fault)?;
            }
        }
        self.emit(Event::SchedulesChanged);
        Ok(())
    }

    // ---------------------------------------------------------------- memory

    /// Whether memory is on, and every entry with where it came from.
    ///
    /// # Errors
    ///
    /// The memory layer's `TUR-AI-MEMORY-*` if the store cannot be read.
    pub fn memory_list(&self) -> Result<Value, Fault> {
        let memory = self.memory();
        let listing = memory.list(&Filter::default()).map_err(memory_fault)?;
        let entries: Vec<Value> = listing.entries.iter().map(entry_json).collect();
        let damaged: Vec<Value> = listing
            .damaged
            .iter()
            .map(|file| json!({ "id": file.id, "reason": file.reason }))
            .collect();
        Ok(json!({
            "enabled": memory.is_enabled().ok(),
            "entries": entries,
            "damaged": damaged,
        }))
    }

    /// Turn memory on or off.
    ///
    /// # Errors
    ///
    /// The memory layer's `TUR-AI-MEMORY-*` if the store cannot be written.
    pub fn set_memory_enabled(&self, enabled: bool) -> Result<(), Fault> {
        let switch = MemorySwitch { enabled };
        let confirmation = ByTheCaller {
            shown: switch,
            at: self.now(),
        };
        {
            let _guard = self.state();
            self.memory()
                .set_enabled(switch, &confirmation)
                .map_err(memory_fault)?;
        }
        self.record_change("memory", if enabled { "on" } else { "off" });
        self.emit(Event::MemoryChanged);
        Ok(())
    }

    /// Keep an entry that waits for the person: it becomes one P@x recalls.
    /// It stays marked as taken from content.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` if the text is not an entry identifier; the
    /// memory layer's `TUR-AI-MEMORY-*` otherwise.
    pub fn confirm_memory(&self, id: &str) -> Result<(), Fault> {
        let id =
            EntryId::parse(id).ok_or_else(|| invalid("that is not a memory entry identifier"))?;
        {
            let _guard = self.state();
            let mut memory = self.memory();
            let entry = memory.get(&id).map_err(memory_fault)?;
            let confirmation = ByTheCaller {
                shown: entry,
                at: self.now(),
            };
            memory.confirm(&id, &confirmation).map_err(memory_fault)?;
        }
        self.record_change("memory_entry", "kept");
        self.emit(Event::MemoryChanged);
        Ok(())
    }

    /// Forget an entry.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` if the text is not an entry identifier; the
    /// memory layer's `TUR-AI-MEMORY-*` otherwise.
    pub fn forget_memory(&self, id: &str) -> Result<(), Fault> {
        let id =
            EntryId::parse(id).ok_or_else(|| invalid("that is not a memory entry identifier"))?;
        {
            let _guard = self.state();
            self.memory().forget(&id).map_err(memory_fault)?;
        }
        self.record_change("memory_entry", "forgotten");
        self.emit(Event::MemoryChanged);
        Ok(())
    }

    // ------------------------------------------------------ the schedule runner

    /// Run a scheduled task with nobody there. `None` if another task has
    /// the slot: the schedule stays due and has its turn when that task
    /// ends.
    fn run_scheduled(&self, schedule: &Schedule, request: &str) -> Option<TaskRun> {
        let started_at = self.now();
        let locked = self.state().locked.unwrap_or(true);
        let situation = Situation {
            presence: crate::context::Presence::Unattended,
            session_locked: locked,
        };
        let job = self
            .begin(
                request,
                situation,
                Some(schedule.id().as_str()),
                Vec::new(),
                false,
            )
            .ok()?;
        let ended = self.run_job(&job);
        let completed = ended.outcome == TaskOutcome::Completed.as_str();
        let outcome = if !ended.held.is_empty() {
            RunOutcome::Deferred
        } else if completed {
            RunOutcome::Completed
        } else {
            RunOutcome::Failed
        };
        // The account of the outcome is the system's: an error code and its
        // sentence, never the model's answer.
        let detail = match (&ended.code, completed) {
            (_, true) => String::new(),
            (Some(code), false) => format!("{code}: {}", ended.message),
            (None, false) => ended.message.clone(),
        };
        let detail: String = detail
            .chars()
            .filter(|character| !character.is_control())
            .take(400)
            .collect();
        let run = TaskRun {
            started_at,
            outcome,
            performed: ended.performed.clone(),
            deferred: ended.held.clone(),
            detail: detail.clone(),
            task: job.id.clone(),
        };
        let away = match outcome {
            RunOutcome::Deferred => AwayOutcome::Waiting,
            RunOutcome::Completed => AwayOutcome::Done,
            RunOutcome::Failed | RunOutcome::SkippedMissed => AwayOutcome::Failed,
        };
        let shown = notify::scheduled_task(
            schedule.name(),
            away,
            run.performed.len(),
            run.deferred.len(),
            if completed { "" } else { &ended.message },
        );
        self.finish(&job, ended);
        // Told once the run is recorded, so that what the notification
        // points at is there when the person looks.
        self.to_tell
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(shown);
        Some(run)
    }

    /// Look at the schedules once: show what is due, run what is due, and
    /// set the timer for the next occurrence.
    ///
    /// While P@x is switched off nothing is shown or run and no timer is
    /// set. What came due in that time is found, late, when P@x is switched
    /// on again.
    pub fn tick(&self) -> TickReport {
        let _one_at_a_time = self
            .ticking
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.is_enabled() {
            let _ = self.parts.waker.arm(None);
            return TickReport::default();
        }
        let mut book = self.book();
        let mut runner = CoreRunner { core: self };
        let report = run_due(&mut book, &*self.parts.clock, &mut runner);
        let to_tell: Vec<notify::Notification> = self
            .to_tell
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect();
        for shown in &to_tell {
            let _ = self.parts.notifier.notify(shown);
        }

        if let Ok(mut trail) = FileTrail::open(&self.parts.paths.audit) {
            for fired in &report.fired {
                let task = fired
                    .task
                    .clone()
                    .unwrap_or_else(|| format!("schedule-{}", fired.schedule));
                let mut event = TrailEvent::new(EventKind::ScheduleRun, &task, "-")
                    .with("TUR_AI_SCHEDULE", crate::trail::identifier(&fired.schedule))
                    .with(
                        "TUR_AI_KIND",
                        if fired.reminder { "reminder" } else { "task" },
                    )
                    .with(
                        "TUR_AI_OUTCOME",
                        fired.outcome.map_or("still_due", outcome_name),
                    )
                    .with("TUR_AI_LATE", if fired.late { "yes" } else { "no" })
                    .with("TUR_AI_HELD", fired.held.to_string())
                    .with("TUR_AI_ATTENDED", "no");
                if let Some(at) = fired.scheduled_for {
                    event = event.with("TUR_AI_SCHEDULED_FOR", at.seconds().to_string());
                }
                if fired.outcome.is_none() && fired.reminder {
                    event = event.with("TUR_ERROR_CODE", TaskCode::RunnerNotification.as_str());
                }
                let _ = trail.event(&event);
            }
        }
        if self.parts.waker.arm(report.next_wake).is_err()
            && let Ok(mut trail) = FileTrail::open(&self.parts.paths.audit)
        {
            let event = TrailEvent::new(EventKind::ScheduleRun, "scheduler", "-")
                .with("TUR_AI_OUTCOME", "timer_not_set")
                .with("TUR_ERROR_CODE", TaskCode::RunnerWake.as_str());
            let _ = trail.event(&event);
        }
        if !report.fired.is_empty() {
            self.emit(Event::SchedulesChanged);
        }
        report
    }

    /// Ask the schedule runner to look at the schedules. It returns at
    /// once; the look happens on the runner's thread.
    pub fn poke(&self) {
        *self
            .poked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.wake.notify_all();
    }

    /// Start the thread of the schedule runner. It sleeps until it is
    /// poked, and looks at the schedules once for every poke it finds.
    ///
    /// # Errors
    ///
    /// The thread could not be started.
    pub fn spawn_runner(self: &Arc<Self>) -> std::io::Result<()> {
        let core = Arc::clone(self);
        thread::Builder::new()
            .name("tur-pax-schedules".to_owned())
            .spawn(move || {
                loop {
                    {
                        let mut poked = core
                            .poked
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        while !*poked {
                            poked = core
                                .wake
                                .wait(poked)
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                        }
                        *poked = false;
                    }
                    core.tick();
                }
            })
            .map(|_| ())
    }
}

/// One line of the trail as an entry of the activity list.
fn activity_entry(number: usize, line: &Map<String, Value>) -> Value {
    let field = |name: &str| line.get(name).and_then(Value::as_str);
    let number_of = |name: &str| field(name).and_then(|text| text.parse::<u64>().ok());
    json!({
        "n": number,
        "event": field("TUR_AI_EVENT"),
        "time": field(TIME_FIELD)
            .and_then(|text| text.parse::<u64>().ok())
            .map(|micros| micros / 1_000_000),
        "action": field("TUR_AI_ACTION"),
        "class": number_of("TUR_AI_CLASS"),
        "tier": number_of("TUR_AI_TIER"),
        "decision": field("TUR_AI_DECISION"),
        "sandbox": field("TUR_AI_SANDBOX"),
        "result": field("TUR_AI_RESULT"),
        "reason": field("TUR_AI_REASON"),
        "outcome": field("TUR_AI_OUTCOME"),
        "error_code": field("TUR_ERROR_CODE"),
        "withheld": number_of("TUR_AI_WITHHELD"),
        "setting": field("TUR_AI_SETTING"),
        "value": field("TUR_AI_VALUE"),
        "schedule": field("TUR_AI_SCHEDULE"),
        "message": field("MESSAGE"),
    })
}

/// The trail as the activity list: its tasks, newest first, at most `limit`
/// of them, each with its entries in order. A line that is not a record of
/// the trail is passed over.
pub fn activity_from(trail: &str, limit: usize) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<Map<String, Value>>> =
        std::collections::HashMap::new();
    for line in trail.lines() {
        let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(task) = fields.get("TUR_AI_TASK").and_then(Value::as_str) else {
            continue;
        };
        if !groups.contains_key(task) {
            order.push(task.to_owned());
        }
        groups.entry(task.to_owned()).or_default().push(fields);
    }
    let mut list = Vec::new();
    for task in order.iter().rev().take(limit) {
        let Some(lines) = groups.get(task) else {
            continue;
        };
        let find = |event: &str, name: &str| {
            lines
                .iter()
                .find(|line| line.get("TUR_AI_EVENT").and_then(Value::as_str) == Some(event))
                .and_then(|line| line.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let kind = if task == "settings" {
            "settings"
        } else if lines
            .iter()
            .any(|line| line.get("TUR_AI_EVENT").and_then(Value::as_str) == Some("schedule_run"))
            && find("task_started", "TUR_AI_TASK").is_none()
        {
            "schedule"
        } else {
            "task"
        };
        let entries: Vec<Value> = lines
            .iter()
            .enumerate()
            .map(|(index, line)| activity_entry(index + 1, line))
            .collect();
        let started = lines
            .first()
            .and_then(|line| line.get(TIME_FIELD))
            .and_then(Value::as_str)
            .and_then(|text| text.parse::<u64>().ok())
            .map(|micros| micros / 1_000_000);
        list.push(json!({
            "task": task,
            "kind": kind,
            "started_at": started,
            "attended": find("task_started", "TUR_AI_ATTENDED").map(|value| value == "yes"),
            "provider": lines
                .first()
                .and_then(|line| line.get("TUR_AI_PROVIDER"))
                .and_then(Value::as_str),
            "outcome": find("task_finished", "TUR_AI_OUTCOME"),
            "error_code": find("task_finished", "TUR_ERROR_CODE"),
            "schedule": find("schedule_run", "TUR_AI_SCHEDULE"),
            "entries": entries,
        }));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-12T10:00:00Z, a Thursday; 11:00 in Rome.
    const NOW: Timestamp = Timestamp(1_794_477_600);

    fn arguments(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("an object")
    }

    fn draft(reminder: bool, value: Value) -> Result<Proposal, String> {
        draft_proposal(reminder, &arguments(value), NOW, "Europe/Rome", "t-1")
    }

    #[test]
    fn a_reminder_in_minutes_is_one_instant_from_now() {
        let proposal = draft(
            true,
            json!({ "message": "Call Marta. Then lunch.", "in_minutes": 20 }),
        )
        .expect("a proposal");
        assert_eq!(proposal.name, "Call Marta");
        assert_eq!(
            proposal.action,
            ProposedAction::Reminder {
                message: "Call Marta. Then lunch.".to_owned()
            }
        );
        assert_eq!(
            proposal.trigger,
            Trigger::Once {
                at: Timestamp(NOW.0 + 1200)
            }
        );
        assert_eq!(proposal.missed_runs, MissedRunPolicy::RunOnce);
        assert_eq!(proposal.proposed_by_task.as_deref(), Some("t-1"));
    }

    #[test]
    fn a_date_and_a_time_are_read_in_the_zone_of_the_computer_or_the_one_named() {
        // 17:00 in Rome on that day is 16:00 UTC.
        let rome = draft(
            true,
            json!({ "message": "Tea", "name": "Tea time", "date": "2026-11-12", "time": "17:00" }),
        )
        .expect("a proposal");
        assert_eq!(rome.name, "Tea time");
        assert_eq!(
            rome.trigger,
            Trigger::Once {
                at: Timestamp(NOW.0 + 6 * 3600)
            }
        );
        // The same wall-clock time in New York is 22:00 UTC.
        let new_york = draft(
            true,
            json!({ "message": "Tea", "date": "2026-11-12", "time": "17:00", "zone": "America/New_York" }),
        )
        .expect("a proposal");
        assert_eq!(
            new_york.trigger,
            Trigger::Once {
                at: Timestamp(NOW.0 + 12 * 3600)
            }
        );
    }

    #[test]
    fn a_repeat_becomes_a_calendar_trigger_in_a_named_zone() {
        let daily = draft(
            false,
            json!({ "request": "Check the disks", "time": "07:30", "repeat": "daily" }),
        )
        .expect("a proposal");
        assert_eq!(
            daily.action,
            ProposedAction::Task {
                request: "Check the disks".to_owned(),
                approvals: Vec::new()
            }
        );
        assert_eq!(
            daily.trigger,
            Trigger::Daily {
                time: LocalTime::new(7, 30).expect("a time"),
                zone: "Europe/Rome".to_owned()
            }
        );
        let weekly = draft(
            true,
            json!({ "message": "Bins", "date": "2026-11-12", "time": "20:00", "repeat": "weekly" }),
        )
        .expect("a proposal");
        assert_eq!(
            weekly.trigger,
            Trigger::Weekly {
                weekdays: vec![tur_pax_memory::Weekday::Thursday],
                time: LocalTime::new(20, 0).expect("a time"),
                zone: "Europe/Rome".to_owned()
            }
        );
        let monthly = draft(
            true,
            json!({ "message": "Rent", "date": "2026-11-30", "time": "09:00", "repeat": "monthly", "zone": "UTC" }),
        )
        .expect("a proposal");
        assert_eq!(
            monthly.trigger,
            Trigger::Monthly {
                day: 30,
                time: LocalTime::new(9, 0).expect("a time"),
                zone: "UTC".to_owned(),
                if_day_missing: ShortMonth::LastDay
            }
        );
    }

    #[test]
    fn a_proposal_that_makes_no_sense_is_refused_without_repeating_it() {
        let secret = "hunter2-PLANTED";
        for value in [
            json!({ "message": secret }),
            json!({ "message": secret, "in_minutes": 5, "time": "10:00" }),
            json!({ "message": secret, "in_minutes": 5, "repeat": "daily" }),
            json!({ "message": secret, "date": "2026-11-12" }),
            json!({ "message": secret, "date": "2026-11-12", "time": "25:00" }),
            json!({ "message": secret, "date": "2026-02-30", "time": "10:00" }),
            json!({ "message": secret, "date": "2026-11-12", "time": "10:59" }),
            json!({ "message": secret, "date": "2020-01-01", "time": "10:00" }),
            json!({ "message": secret, "time": "10:00", "repeat": "weekly" }),
            json!({ "message": secret, "time": "10:00", "repeat": "monthly" }),
            json!({ "message": secret, "in_minutes": 5, "zone": "Mars/Olympus" }),
            json!({ "message": secret, "in_minutes": 18_446_744_073_709_551_615_u64 }),
            json!({ "request": secret, "in_minutes": 5 }),
        ] {
            let refused = draft(true, value.clone()).expect_err("refused");
            assert!(!refused.contains(secret), "{value}: {refused}");
        }
        // A request for a reminder is not a request for a task, and the
        // other way round.
        assert!(draft(false, json!({ "message": "x", "in_minutes": 5 })).is_err());
        // A text that looks like a secret is refused by the memory layer,
        // with its code.
        let refused = draft(
            true,
            json!({ "message": "my password: Zx81qwerty-door", "in_minutes": 5 }),
        )
        .expect_err("refused");
        assert!(refused.starts_with("TUR-AI-SCHEDULE-010"), "{refused}");
    }

    #[test]
    fn what_can_be_a_request() {
        assert!(is_request("Why is the fan loud?"));
        assert!(is_request("two\nlines\tand a tab"));
        assert!(!is_request(""));
        assert!(!is_request("   \n"));
        assert!(!is_request("bell\u{7}"));
        assert!(!is_request("escape\u{1b}[2J"));
        assert!(!is_request(&"x".repeat(REQUEST_MAX_BYTES + 1)));
        assert!(is_request(&"x".repeat(REQUEST_MAX_BYTES)));
    }

    #[test]
    fn the_first_sentence_of_a_description_is_its_title() {
        assert_eq!(
            first_sentence(
                "List detected volumes with filesystem, state and mount point.",
                120
            ),
            "List detected volumes with filesystem, state and mount point"
        );
        assert_eq!(
            first_sentence("Run one program. It cannot change anything.", 120),
            "Run one program"
        );
        assert_eq!(first_sentence("first line\nsecond line", 120), "first line");
        assert_eq!(first_sentence("", 120), "");
        let long = first_sentence(&"word ".repeat(100), 20);
        assert_eq!(long.chars().count(), 20);
        assert!(long.ends_with('\u{2026}'));
    }

    #[test]
    fn the_trail_becomes_a_list_of_tasks_newest_first() {
        let trail = [
            r#"{"MESSAGE":"m","TUR_AI_EVENT":"task_started","TUR_AI_TASK":"t-1","TUR_AI_PROVIDER":"anthropic","TUR_AI_ATTENDED":"yes","__REALTIME_TIMESTAMP":"1794477600000000"}"#,
            r#"{"MESSAGE":"m","TUR_AI_EVENT":"decision","TUR_AI_TASK":"t-1","TUR_AI_PROVIDER":"anthropic","TUR_AI_ACTION":"command:read_only","TUR_AI_CLASS":"0","TUR_AI_DECISION":"allowed","TUR_AI_TIER":"4","TUR_AI_SANDBOX":"read-only"}"#,
            r#"{"MESSAGE":"m","TUR_AI_EVENT":"setting_changed","TUR_AI_TASK":"settings","TUR_AI_PROVIDER":"-","TUR_AI_SETTING":"key","TUR_AI_VALUE":"set"}"#,
            r#"{"MESSAGE":"m","TUR_AI_EVENT":"task_finished","TUR_AI_TASK":"t-1","TUR_AI_PROVIDER":"anthropic","TUR_AI_OUTCOME":"completed"}"#,
            "not a record",
            r#"["neither"]"#,
            r#"{"MESSAGE":"no task"}"#,
            r#"{"MESSAGE":"m","TUR_AI_EVENT":"task_started","TUR_AI_TASK":"t-2","TUR_AI_PROVIDER":"anthropic","TUR_AI_ATTENDED":"no"}"#,
            r#"{"MESSAGE":"m","TUR_AI_EVENT":"decision","TUR_AI_TASK":"t-2","TUR_AI_PROVIDER":"anthropic","TUR_AI_ACTION":"org.tur.Test1:test_consequential","TUR_AI_CLASS":"2","TUR_AI_DECISION":"refused","TUR_AI_TIER":"1","TUR_ERROR_CODE":"TUR-AI-POLICY-006"}"#,
            r#"{"MESSAGE":"m","TUR_AI_EVENT":"task_finished","TUR_AI_TASK":"t-2","TUR_AI_PROVIDER":"anthropic","TUR_AI_OUTCOME":"spend_limit","TUR_ERROR_CODE":"TUR-AI-SPEND-001"}"#,
            r#"{"MESSAGE":"m","TUR_AI_EVENT":"schedule_run","TUR_AI_TASK":"schedule-s-00000001","TUR_AI_PROVIDER":"-","TUR_AI_SCHEDULE":"s-00000001","TUR_AI_OUTCOME":"completed"}"#,
        ]
        .join("\n");
        let list = activity_from(&trail, 10);
        let tasks: Vec<&str> = list
            .iter()
            .filter_map(|task| task["task"].as_str())
            .collect();
        assert_eq!(tasks, ["schedule-s-00000001", "t-2", "settings", "t-1"]);
        assert_eq!(list[0]["kind"], "schedule");
        assert_eq!(list[0]["schedule"], "s-00000001");
        assert_eq!(list[1]["kind"], "task");
        assert_eq!(list[1]["attended"], false);
        assert_eq!(list[1]["outcome"], "spend_limit");
        assert_eq!(list[1]["error_code"], "TUR-AI-SPEND-001");
        assert_eq!(list[1]["entries"][1]["class"], 2);
        assert_eq!(list[1]["entries"][1]["decision"], "refused");
        assert_eq!(list[1]["entries"][1]["n"], 2);
        assert_eq!(list[2]["kind"], "settings");
        assert_eq!(list[2]["entries"][0]["setting"], "key");
        assert_eq!(list[3]["attended"], true);
        assert_eq!(list[3]["started_at"], 1_794_477_600_u64);
        assert_eq!(list[3]["entries"].as_array().map(Vec::len), Some(3));
        assert_eq!(list[3]["entries"][1]["sandbox"], "read-only");
        assert_eq!(list[3]["entries"][1]["tier"], 4);

        // The limit counts tasks, from the newest.
        let newest = activity_from(&trail, 2);
        assert_eq!(newest.len(), 2);
        assert_eq!(newest[1]["task"], "t-2");
        assert!(activity_from("", 5).is_empty());
    }
}
