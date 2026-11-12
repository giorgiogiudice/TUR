// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The audit trail of a task.
//!
//! The policy engine writes one [`AuditRecord`] for every decision
//! ([`crate::audit`]). A task leaves more to record than its decisions: a
//! tool call that never became an action, an action that was allowed and
//! then ran or failed, output that was withheld from the provider, a
//! response that was discarded, and how the task ended. Those are
//! [`TrailEvent`]s. A [`Trail`] takes both, in the order they happened.
//!
//! What an event contains is restricted the same way a record is. An event
//! never holds a request, an answer, an argument value, a command line or
//! any output. It holds identifiers, counts, statuses and codes. When
//! something was withheld it says how many pieces and of which kinds, never
//! the text.
//!
//! The rule of the engine holds here too: no record, no action. If a
//! decision cannot be written the engine refuses the action; if the record
//! of what an action returned cannot be written, the orchestrator does not
//! pass the result on and stops the task.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use crate::audit::{
    ACTION_OUTCOME_EVENT, ActionOutcome, AuditRecord, AuditSink, AuditWriteError,
    OUTPUT_WITHHELD_EVENT, OutputWithheld,
};

/// The value of `TUR_AI_EVENT` for a decision of the policy engine.
pub const DECISION_EVENT: &str = "decision";

/// What happened, other than a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// A task began.
    TaskStarted,
    /// A tool call of the model did not become an action: the tool is not
    /// offered, or the arguments are not valid.
    ProposalRejected,
    /// The engine asked for a confirmation, an authentication or the user's
    /// return, and no way to give one exists.
    ApprovalUnavailable,
    /// An allowed action was performed, or was attempted and failed: the
    /// action-outcome event of the audit vocabulary
    /// ([`crate::audit::ActionOutcome`]).
    Executed,
    /// Part of what an action returned was withheld before it left the
    /// machine ([`crate::audit::OutputWithheld`]).
    OutputWithheld,
    /// A response of the model was discarded whole.
    ResponseDiscarded,
    /// A task ended.
    TaskFinished,
    /// An unattended task proposed an action it may not take with nobody
    /// there. The action was not performed; it is kept for the person's
    /// return (design sections 5.6 and 16.2).
    HeldForReturn,
    /// The schedule runner handled one schedule that was due: it showed a
    /// reminder, ran a scheduled task, or passed over a missed occurrence.
    ScheduleRun,
    /// Something about the assistant was changed by the person: the switch,
    /// the provider, the key, a limit, a schedule, a memory entry. The event
    /// names what was changed, never a value.
    SettingChanged,
}

impl EventKind {
    /// Every kind.
    pub const ALL: [EventKind; 10] = [
        EventKind::TaskStarted,
        EventKind::ProposalRejected,
        EventKind::ApprovalUnavailable,
        EventKind::Executed,
        EventKind::OutputWithheld,
        EventKind::ResponseDiscarded,
        EventKind::TaskFinished,
        EventKind::HeldForReturn,
        EventKind::ScheduleRun,
        EventKind::SettingChanged,
    ];

    /// The value of the `TUR_AI_EVENT` field.
    pub const fn as_str(self) -> &'static str {
        match self {
            EventKind::TaskStarted => "task_started",
            EventKind::ProposalRejected => "proposal_rejected",
            EventKind::ApprovalUnavailable => "approval_unavailable",
            EventKind::Executed => ACTION_OUTCOME_EVENT,
            EventKind::OutputWithheld => OUTPUT_WITHHELD_EVENT,
            EventKind::ResponseDiscarded => "response_discarded",
            EventKind::TaskFinished => "task_finished",
            EventKind::HeldForReturn => "held_for_return",
            EventKind::ScheduleRun => "schedule_run",
            EventKind::SettingChanged => "setting_changed",
        }
    }
}

/// One event of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrailEvent {
    kind: EventKind,
    task: String,
    provider: String,
    fields: Vec<(&'static str, String)>,
}

/// `text` if it is 1 to 128 ASCII letters, digits, dots, underscores,
/// hyphens and colons; otherwise a placeholder. Names a model made up are
/// written to the trail only in this form.
pub fn identifier(text: &str) -> String {
    let valid = !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'));
    if valid {
        text.to_owned()
    } else {
        "<invalid>".to_owned()
    }
}

impl TrailEvent {
    /// An event of a task. The task and the provider are written as
    /// identifiers.
    pub fn new(kind: EventKind, task: &str, provider: &str) -> Self {
        TrailEvent {
            kind,
            task: identifier(task),
            provider: identifier(provider),
            fields: Vec::new(),
        }
    }

    /// The action-outcome event of a task: what became of an action the
    /// engine allowed.
    pub fn action_outcome(task: &str, provider: &str, outcome: &ActionOutcome) -> Self {
        let mut event = TrailEvent::new(EventKind::Executed, task, provider);
        event.fields = outcome.fields();
        event
    }

    /// The output-withheld event of a task: how many pieces of what an
    /// action returned were withheld, and of which kinds.
    pub fn output_withheld(task: &str, provider: &str, withheld: &OutputWithheld) -> Self {
        let mut event = TrailEvent::new(EventKind::OutputWithheld, task, provider);
        event.fields = withheld.fields();
        event
    }

    /// Add a field. The caller passes only identifiers, counts, statuses and
    /// codes.
    #[must_use]
    pub fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.fields.push((name, value.into()));
        self
    }

    /// What happened.
    pub fn kind(&self) -> EventKind {
        self.kind
    }

    /// The value of a field, if the event has it.
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| *field == name)
            .map(|(_, value)| value.as_str())
    }

    /// The human-readable `MESSAGE` field.
    pub fn message(&self) -> String {
        let mut message = format!("tur-pax: {}", self.kind.as_str());
        for (name, value) in &self.fields {
            message.push_str(&format!(
                " {}={value}",
                name.trim_start_matches("TUR_AI_").to_ascii_lowercase()
            ));
        }
        message
    }

    /// The event as journal fields, in a fixed order.
    pub fn journal_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("MESSAGE", self.message()),
            ("TUR_AI_EVENT", self.kind.as_str().to_owned()),
            ("TUR_AI_TASK", self.task.clone()),
            ("TUR_AI_PROVIDER", self.provider.clone()),
        ];
        fields.extend(self.fields.iter().cloned());
        fields
    }
}

/// One entry of a trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrailEntry {
    /// A decision of the policy engine.
    Decision(AuditRecord),
    /// Anything else.
    Event(TrailEvent),
}

impl TrailEntry {
    /// The entry as journal fields. A decision has `TUR_AI_EVENT=decision`
    /// after its message.
    pub fn journal_fields(&self) -> Vec<(&'static str, String)> {
        match self {
            TrailEntry::Decision(record) => {
                let mut fields = record.journal_fields();
                let at = usize::from(!fields.is_empty());
                fields.insert(at, ("TUR_AI_EVENT", DECISION_EVENT.to_owned()));
                fields
            }
            TrailEntry::Event(event) => event.journal_fields(),
        }
    }

    /// The value of a journal field of the entry.
    pub fn field(&self, name: &str) -> Option<String> {
        self.journal_fields()
            .into_iter()
            .find(|(field, _)| *field == name)
            .map(|(_, value)| value)
    }

    /// The entry as one JSON object of its journal fields.
    pub fn to_json(&self) -> Value {
        let map: Map<String, Value> = self
            .journal_fields()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), Value::String(value)))
            .collect();
        Value::Object(map)
    }
}

/// Where the audit trail of a task goes.
pub trait Trail {
    /// Write the record of one decision. An error means the record is not
    /// durable, and the action it is about must not run.
    fn decision(&mut self, record: &AuditRecord) -> Result<(), AuditWriteError>;

    /// Write one event. An error means the event is not durable.
    fn event(&mut self, event: &TrailEvent) -> Result<(), AuditWriteError>;
}

/// A [`Trail`] seen as the engine's [`AuditSink`], so that
/// [`crate::audit::decide_and_record`] writes into it.
pub struct DecisionSink<'a>(pub &'a mut dyn Trail);

impl AuditSink for DecisionSink<'_> {
    fn write(&mut self, record: &AuditRecord) -> Result<(), AuditWriteError> {
        self.0.decision(record)
    }
}

/// A trail kept in memory.
#[derive(Debug, Default)]
pub struct MemoryTrail {
    entries: Vec<TrailEntry>,
}

impl MemoryTrail {
    /// An empty trail.
    pub fn new() -> Self {
        MemoryTrail::default()
    }

    /// The entries written so far, oldest first.
    pub fn entries(&self) -> &[TrailEntry] {
        &self.entries
    }

    /// The decisions written so far, oldest first.
    pub fn decisions(&self) -> Vec<&AuditRecord> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                TrailEntry::Decision(record) => Some(record),
                TrailEntry::Event(_) => None,
            })
            .collect()
    }

    /// The events of one kind written so far, oldest first.
    pub fn events(&self, kind: EventKind) -> Vec<&TrailEvent> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                TrailEntry::Event(event) if event.kind() == kind => Some(event),
                _ => None,
            })
            .collect()
    }
}

impl Trail for MemoryTrail {
    fn decision(&mut self, record: &AuditRecord) -> Result<(), AuditWriteError> {
        self.entries.push(TrailEntry::Decision(record.clone()));
        Ok(())
    }

    fn event(&mut self, event: &TrailEvent) -> Result<(), AuditWriteError> {
        self.entries.push(TrailEntry::Event(event.clone()));
        Ok(())
    }
}

/// The name of the field that says when an entry was written, in
/// microseconds since the Unix epoch. It is the name the journal gives the
/// time it adds to every record itself; the file stands in for the journal,
/// so [`FileTrail`] adds it.
pub const TIME_FIELD: &str = "__REALTIME_TIMESTAMP";

/// A trail appended to a file, one JSON object per line.
///
/// Each line is written whole and synchronised to the disk before the write
/// is reported as done. Each line also says when it was written
/// ([`TIME_FIELD`]). The file is only ever appended to by this type; it
/// is, however, a file of the user's, and nothing here stops another process
/// of the same user from changing it. The journal sink of the design is not
/// built yet.
#[derive(Debug)]
pub struct FileTrail {
    file: File,
}

impl FileTrail {
    /// Open the file for appending, creating it and its directory if they
    /// do not exist.
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(FileTrail { file })
    }

    fn append(&mut self, entry: &TrailEntry) -> Result<(), AuditWriteError> {
        let mut document = entry.to_json();
        let written = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_micros());
        if let Value::Object(fields) = &mut document {
            fields.insert(TIME_FIELD.to_owned(), Value::String(written.to_string()));
        }
        let mut line = document.to_string();
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.sync_data())
            .map_err(|error| AuditWriteError(error.to_string()))
    }
}

impl Trail for FileTrail {
    fn decision(&mut self, record: &AuditRecord) -> Result<(), AuditWriteError> {
        self.append(&TrailEntry::Decision(record.clone()))
    }

    fn event(&mut self, event: &TrailEvent) -> Result<(), AuditWriteError> {
        self.append(&TrailEntry::Event(event.clone()))
    }
}

/// Two trails written together: an entry is durable only if both took it.
pub struct BothTrails<'a>(pub &'a mut dyn Trail, pub &'a mut dyn Trail);

impl Trail for BothTrails<'_> {
    fn decision(&mut self, record: &AuditRecord) -> Result<(), AuditWriteError> {
        self.0.decision(record)?;
        self.1.decision(record)
    }

    fn event(&mut self, event: &TrailEvent) -> Result<(), AuditWriteError> {
        self.0.event(event)?;
        self.1.event(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Action, AgentOperation};
    use crate::audit::AuditMeta;
    use crate::context::{Context, Timestamp};
    use crate::engine::decide;
    use crate::policy::UserPolicy;

    fn record() -> AuditRecord {
        let action = Action::Agent(AgentOperation::MemoryRead);
        let context = Context::attended();
        let decision = decide(&action, &context, &UserPolicy::default(), Timestamp(0));
        let meta = AuditMeta {
            task: "task-1",
            provider: "anthropic",
        };
        AuditRecord::new(&meta, &action, &context, &decision)
    }

    #[test]
    fn the_events_of_the_audit_vocabulary_become_trail_events() {
        let outcome = ActionOutcome {
            action: "command:read_only".to_owned(),
            result: crate::audit::ActionResult::Ok,
            exit_code: Some(0),
            output_bytes: 64,
            truncated: false,
            withheld: 2,
            error_code: None,
        };
        let event = TrailEvent::action_outcome("task-1", "google", &outcome);
        assert_eq!(event.kind(), EventKind::Executed);
        assert_eq!(event.field("TUR_AI_RESULT"), Some("ok"));
        assert_eq!(event.field("TUR_AI_WITHHELD"), Some("2"));
        // The kinds and the code are the withheld event's.
        assert_eq!(event.field("TUR_AI_WITHHELD_KINDS"), None);
        assert_eq!(event.field("TUR_ERROR_CODE"), None);
        let fields = event.journal_fields();
        assert_eq!(fields[1], ("TUR_AI_EVENT", "executed".to_owned()));
        assert_eq!(fields[3], ("TUR_AI_PROVIDER", "google".to_owned()));

        let withheld = OutputWithheld::new("command:read_only", &["password", "password"])
            .expect("something was withheld");
        let event = TrailEvent::output_withheld("task-1", "google", &withheld);
        assert_eq!(event.kind(), EventKind::OutputWithheld);
        let names: Vec<&str> = event
            .journal_fields()
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(
            names,
            [
                "MESSAGE",
                "TUR_AI_EVENT",
                "TUR_AI_TASK",
                "TUR_AI_PROVIDER",
                "TUR_AI_ACTION",
                "TUR_AI_WITHHELD",
                "TUR_AI_WITHHELD_KINDS",
                "TUR_ERROR_CODE"
            ]
        );
        assert_eq!(event.field("TUR_AI_WITHHELD"), Some("2"));
        assert_eq!(event.field("TUR_AI_WITHHELD_KINDS"), Some("password"));
        assert_eq!(event.field("TUR_ERROR_CODE"), Some("TUR-AI-OUTPUT-001"));
        assert_eq!(
            event.message(),
            "tur-pax: output_withheld action=command:read_only withheld=2 \
             withheld_kinds=password tur_error_code=TUR-AI-OUTPUT-001"
        );
    }

    #[test]
    fn an_event_renders_as_journal_fields_in_a_fixed_order() {
        let event = TrailEvent::new(EventKind::Executed, "task-1", "anthropic")
            .with("TUR_AI_ACTION", "org.tur.Test1:test_observe")
            .with("TUR_AI_RESULT", "ok")
            .with("TUR_AI_WITHHELD", "2");
        assert_eq!(event.kind(), EventKind::Executed);
        assert_eq!(event.field("TUR_AI_RESULT"), Some("ok"));
        assert_eq!(event.field("TUR_AI_MISSING"), None);
        let fields = event.journal_fields();
        let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "MESSAGE",
                "TUR_AI_EVENT",
                "TUR_AI_TASK",
                "TUR_AI_PROVIDER",
                "TUR_AI_ACTION",
                "TUR_AI_RESULT",
                "TUR_AI_WITHHELD"
            ]
        );
        assert_eq!(
            fields[0].1,
            "tur-pax: executed action=org.tur.Test1:test_observe result=ok withheld=2"
        );
        assert_eq!(fields[1].1, "executed");
    }

    #[test]
    fn names_that_are_not_identifiers_are_not_written() {
        let long = "x".repeat(129);
        for bad in ["", "a b", "a\nb", "tool;rm", "ünïcode", long.as_str()] {
            assert_eq!(identifier(bad), "<invalid>", "{bad:?}");
        }
        for good in [
            "task-1",
            "org.tur.Test1:test_observe",
            "run_readonly_command",
        ] {
            assert_eq!(identifier(good), good);
        }
        let event = TrailEvent::new(EventKind::TaskStarted, "task 1\nINJECTED=1", "x y");
        assert_eq!(event.journal_fields()[2].1, "<invalid>");
        assert_eq!(event.journal_fields()[3].1, "<invalid>");
    }

    #[test]
    fn a_decision_is_marked_as_one_and_keeps_its_fields() {
        let entry = TrailEntry::Decision(record());
        assert_eq!(entry.field("TUR_AI_EVENT").as_deref(), Some(DECISION_EVENT));
        assert_eq!(
            entry.field("TUR_AI_ACTION").as_deref(),
            Some("agent:memory_read")
        );
        assert_eq!(entry.field("TUR_AI_DECISION").as_deref(), Some("allowed"));
        assert_eq!(entry.field("TUR_AI_TIER").as_deref(), Some("0"));
        let json = entry.to_json();
        assert_eq!(json["TUR_AI_CLASS"], "0");
        assert_eq!(json["TUR_AI_PROVIDER"], "anthropic");
        assert_eq!(entry.journal_fields()[0].0, "MESSAGE");
        assert_eq!(entry.journal_fields()[1].0, "TUR_AI_EVENT");
    }

    #[test]
    fn a_memory_trail_keeps_decisions_and_events_in_order() {
        let mut trail = MemoryTrail::new();
        let started = TrailEvent::new(EventKind::TaskStarted, "task-1", "anthropic");
        trail.event(&started).expect("written");
        DecisionSink(&mut trail).write(&record()).expect("written");
        trail
            .event(&TrailEvent::new(
                EventKind::TaskFinished,
                "task-1",
                "anthropic",
            ))
            .expect("written");
        assert_eq!(trail.entries().len(), 3);
        assert_eq!(trail.decisions().len(), 1);
        assert_eq!(trail.events(EventKind::TaskStarted), [&started]);
        assert!(trail.events(EventKind::Executed).is_empty());
        assert!(matches!(trail.entries()[1], TrailEntry::Decision(_)));
    }

    #[test]
    fn a_file_trail_appends_one_json_object_per_line() {
        let directory = std::env::temp_dir().join(format!("tur-pax-trail-{}", std::process::id()));
        let path = directory.join("nested").join("audit.jsonl");
        let _ = std::fs::remove_dir_all(&directory);
        {
            let mut trail = FileTrail::open(&path).expect("the file opens");
            trail
                .event(&TrailEvent::new(
                    EventKind::TaskStarted,
                    "task-1",
                    "anthropic",
                ))
                .expect("written");
            trail.decision(&record()).expect("written");
        }
        {
            // Opening again appends.
            let mut trail = FileTrail::open(&path).expect("the file opens");
            trail
                .event(&TrailEvent::new(
                    EventKind::TaskFinished,
                    "task-1",
                    "anthropic",
                ))
                .expect("written");
        }
        let text = std::fs::read_to_string(&path).expect("the file reads");
        let lines: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("a JSON object"))
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["TUR_AI_EVENT"], "task_started");
        assert_eq!(lines[1]["TUR_AI_EVENT"], "decision");
        assert_eq!(lines[1]["TUR_AI_DECISION"], "allowed");
        assert_eq!(lines[2]["TUR_AI_EVENT"], "task_finished");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn two_trails_both_have_to_take_an_entry() {
        struct Broken;
        impl Trail for Broken {
            fn decision(&mut self, _record: &AuditRecord) -> Result<(), AuditWriteError> {
                Err(AuditWriteError("full".to_owned()))
            }
            fn event(&mut self, _event: &TrailEvent) -> Result<(), AuditWriteError> {
                Err(AuditWriteError("full".to_owned()))
            }
        }
        let mut memory = MemoryTrail::new();
        let mut broken = Broken;
        let mut both = BothTrails(&mut memory, &mut broken);
        assert!(both.decision(&record()).is_err());
        assert!(
            both.event(&TrailEvent::new(EventKind::TaskStarted, "t", "p"))
                .is_err()
        );
        let names: std::collections::BTreeSet<&str> =
            EventKind::ALL.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(names.len(), EventKind::ALL.len());
    }
}
