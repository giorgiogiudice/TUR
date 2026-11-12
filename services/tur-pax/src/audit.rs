// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The audit record (addendum section 11).
//!
//! One record is written for every proposed action and its decision. A record
//! is rendered to journal fields by [`AuditRecord::journal_fields`] and written
//! through an [`AuditSink`]. This crate has no journald dependency; the sink
//! for the journal belongs to the service.
//!
//! What a record contains is restricted by construction:
//!
//! * The argument values of a tool call are never written. Tool arguments are
//!   summarised as the property names with the kind of each value.
//! * A property name that resembles a secret has its kind withheld too; a name
//!   that is not a plain identifier is not written at all.
//! * A command outside the privileged sandbox is summarised as the path of its
//!   program and its argument count.
//! * A privileged request, whether a free-text command or a typed operation,
//!   is written in full, because it is shown to the user anyway and must be
//!   reviewable (addendum section 17, item 9). A value that is marked secret
//!   is replaced. Nothing else is: a rule that withheld what merely looks like
//!   a secret would let the model keep part of a command out of the record.
//! * Window titles, control names and the prompts of other actions are never
//!   written.
//! * Identifiers (task, provider, service, tool, application) are written only
//!   if they use a restricted character set. They are not scanned for secrets:
//!   service and tool names come from a validated manifest, and the task and
//!   provider names from the orchestrator.

use serde_json::{Map, Value};
use tur_core::ErrorCode;

use crate::action::{Action, CommandAction, Sandbox, UiAction, UiActionKind, WorkingContext};
use crate::class::ActionClass;
use crate::context::{Context, Presence, Timestamp};
use crate::decision::{Decision, Refusal, Verdict};
use crate::engine::{decide, describe};
use crate::patterns::match_consequential;
use crate::policy::UserPolicy;

/// Substrings that mark a name as resembling a secret. Compared in lower case.
pub const SECRET_MARKERS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "credential",
    "apikey",
    "api_key",
    "api-key",
    "private",
    "bearer",
    "cookie",
    "authorization",
    "session",
];

/// The length of an unbroken run of key-like characters that is treated as an
/// opaque token.
pub const OPAQUE_RUN: usize = 20;

/// The most argument names one record lists.
pub const MAX_SUMMARY_KEYS: usize = 16;

const INVALID: &str = "<invalid>";
const REDACTED: &str = "<redacted>";

/// Whether a piece of text resembles a secret.
///
/// True if it contains one of the [`SECRET_MARKERS`], or a run of at least
/// [`OPAQUE_RUN`] characters from the base64 and hexadecimal alphabets that
/// contains both a letter and a digit. This is a heuristic: it is applied to
/// names, and values are never written at all.
///
/// ```
/// use tur_pax::audit::resembles_secret;
///
/// assert!(resembles_secret("api_key"));
/// assert!(resembles_secret("sk-live-4f9a8b7c6d5e4f3a2b1c"));
/// assert!(!resembles_secret("volume_id"));
/// ```
pub fn resembles_secret(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    SECRET_MARKERS.iter().any(|marker| lowered.contains(marker)) || has_opaque_run(text)
}

fn has_opaque_run(text: &str) -> bool {
    let mut length = 0_usize;
    let mut letters = false;
    let mut digits = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-') {
            length += 1;
            letters |= c.is_ascii_alphabetic();
            digits |= c.is_ascii_digit();
            if length >= OPAQUE_RUN && letters && digits {
                return true;
            }
        } else {
            length = 0;
            letters = false;
            digits = false;
        }
    }
    false
}

/// The outcome recorded for an action: the values of `TUR_AI_DECISION`
/// (addendum sections 11 and 17, item 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditOutcome {
    /// The engine allowed the action.
    Allowed,
    /// The engine asked for the user's confirmation.
    ConfirmationRequired,
    /// The engine asked for authentication on the trusted path.
    AuthenticationRequired,
    /// The engine deferred the action until the user returns.
    Deferred,
    /// The engine refused the action.
    Refused,
    /// The user approved after a prompt.
    Confirmed,
    /// The user declined after a prompt.
    Denied,
}

impl AuditOutcome {
    /// The value of the `TUR_AI_DECISION` field.
    pub const fn as_str(self) -> &'static str {
        match self {
            AuditOutcome::Allowed => "allowed",
            AuditOutcome::ConfirmationRequired => "confirmation_required",
            AuditOutcome::AuthenticationRequired => "authentication_required",
            AuditOutcome::Deferred => "deferred",
            AuditOutcome::Refused => "refused",
            AuditOutcome::Confirmed => "confirmed",
            AuditOutcome::Denied => "denied",
        }
    }

    fn of(verdict: &Verdict) -> Self {
        match verdict {
            Verdict::Allow => AuditOutcome::Allowed,
            Verdict::Ask { .. } => AuditOutcome::ConfirmationRequired,
            Verdict::Authenticate { .. } => AuditOutcome::AuthenticationRequired,
            Verdict::Defer => AuditOutcome::Deferred,
            Verdict::Refuse => AuditOutcome::Refused,
        }
    }
}

/// Who proposed the action: the task and the provider adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditMeta<'a> {
    /// The task identifier, assigned by the orchestrator.
    pub task: &'a str,
    /// The provider adapter name, or `none` for a scripted caller.
    pub provider: &'a str,
}

/// The record of one proposed action and its decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    task: String,
    provider: String,
    action: String,
    class: ActionClass,
    tier: u8,
    outcome: AuditOutcome,
    reason: &'static str,
    error_code: Option<ErrorCode>,
    summary: String,
    sandbox: Option<&'static str>,
    attended: bool,
    request: Option<String>,
}

impl AuditRecord {
    /// The record for a proposed action, the context it was decided in, and
    /// the engine's decision.
    pub fn new(
        meta: &AuditMeta<'_>,
        action: &Action,
        context: &Context,
        decision: &Decision,
    ) -> Self {
        // A privileged request is recorded in full.
        let request = action.privileged_request().map(|_| describe(action));
        AuditRecord {
            task: identifier(meta.task),
            provider: identifier(meta.provider),
            action: action_name(action),
            class: decision.class(),
            tier: action.tier(),
            outcome: AuditOutcome::of(decision.verdict()),
            reason: decision.reason().as_str(),
            error_code: decision.error_code(),
            summary: request.clone().unwrap_or_else(|| summarise(action)),
            sandbox: sandbox_name(action),
            attended: context.presence == Presence::Attended,
            request,
        }
    }

    /// The follow-up record for the user's answer to a prompt.
    ///
    /// Returns `None` unless this record is waiting for a confirmation or an
    /// authentication.
    pub fn user_response(&self, approved: bool) -> Option<AuditRecord> {
        match self.outcome {
            AuditOutcome::ConfirmationRequired | AuditOutcome::AuthenticationRequired => {
                let mut record = self.clone();
                record.outcome = if approved {
                    AuditOutcome::Confirmed
                } else {
                    AuditOutcome::Denied
                };
                Some(record)
            }
            _ => None,
        }
    }

    /// The recorded outcome.
    pub fn outcome(&self) -> AuditOutcome {
        self.outcome
    }

    /// The class the action was treated as.
    pub fn class(&self) -> ActionClass {
        self.class
    }

    /// The error code, present when the action was refused.
    pub fn error_code(&self) -> Option<&ErrorCode> {
        self.error_code.as_ref()
    }

    /// Whether the task was attended: the value of `TUR_AI_ATTENDED`.
    pub fn attended(&self) -> bool {
        self.attended
    }

    /// The sandbox of a command or a typed operation: the value of
    /// `TUR_AI_SANDBOX`. `None` for every other action.
    pub fn sandbox(&self) -> Option<&'static str> {
        self.sandbox
    }

    /// The full statement of a privileged request: the value of
    /// `TUR_AI_REQUEST`. `None` for every other action.
    pub fn request(&self) -> Option<&str> {
        self.request.as_deref()
    }

    /// The human-readable `MESSAGE` field.
    pub fn message(&self) -> String {
        format!(
            "tur-pax: {} {} (class {}, tier {}): {}; {}",
            self.outcome.as_str(),
            self.action,
            self.class,
            self.tier,
            self.reason,
            self.summary
        )
    }

    /// The record as journal fields, in a fixed order.
    ///
    /// `TUR_AI_SANDBOX` is present only for a command or a typed operation,
    /// `TUR_AI_REQUEST` only for a privileged request, and `TUR_ERROR_CODE`
    /// only when the action was refused.
    pub fn journal_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("MESSAGE", self.message()),
            ("TUR_AI_TASK", self.task.clone()),
            ("TUR_AI_ACTION", self.action.clone()),
            ("TUR_AI_CLASS", self.class.to_string()),
            ("TUR_AI_DECISION", self.outcome.as_str().to_owned()),
            ("TUR_AI_TIER", self.tier.to_string()),
        ];
        if let Some(sandbox) = self.sandbox {
            fields.push(("TUR_AI_SANDBOX", sandbox.to_owned()));
        }
        let attended = if self.attended { "yes" } else { "no" };
        fields.push(("TUR_AI_ATTENDED", attended.to_owned()));
        fields.push(("TUR_AI_PROVIDER", self.provider.clone()));
        if let Some(request) = &self.request {
            fields.push(("TUR_AI_REQUEST", request.clone()));
        }
        if let Some(code) = &self.error_code {
            fields.push(("TUR_ERROR_CODE", code.to_string()));
        }
        fields
    }
}

/// The value of `TUR_AI_SANDBOX`: `read-only`, `workspace`, `user` or
/// `privileged` for a command, `privileged` for a typed operation.
fn sandbox_name(action: &Action) -> Option<&'static str> {
    match action {
        Action::Command(command) => Some(match command.sandbox {
            Sandbox::ReadOnly => "read-only",
            Sandbox::User => match command.working_context {
                WorkingContext::UndoableWorkspace => "workspace",
                WorkingContext::Session => "user",
            },
            Sandbox::Privileged(_) => "privileged",
        }),
        Action::Operation(_) => Some("privileged"),
        Action::Tool(_) | Action::Accessibility(_) | Action::ScreenInput(_) | Action::Agent(_) => {
            None
        }
    }
}

/// `text` if it is 1 to 128 ASCII letters, digits, dots, underscores and
/// hyphens; otherwise a placeholder.
fn identifier(text: &str) -> String {
    let valid = !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if valid {
        text.to_owned()
    } else {
        INVALID.to_owned()
    }
}

fn action_name(action: &Action) -> String {
    match action {
        Action::Tool(call) => format!("{}:{}", identifier(&call.service), identifier(&call.tool)),
        Action::Accessibility(ui) => format!("accessibility:{}", ui.kind.as_str()),
        Action::ScreenInput(ui) => format!("screen:{}", ui.kind.as_str()),
        Action::Command(command) => format!("command:{}", command.sandbox.as_str()),
        Action::Operation(operation) => format!("operation:{}", operation.kind.as_str()),
        Action::Agent(operation) => format!("agent:{}", operation.as_str()),
    }
}

/// The summary of an action that is not a privileged request.
fn summarise(action: &Action) -> String {
    match action {
        Action::Tool(call) => summarise_arguments(&call.arguments),
        Action::Accessibility(ui) | Action::ScreenInput(ui) => summarise_ui(ui),
        Action::Command(command) => summarise_command(command),
        // A privileged request is not summarised; see `AuditRecord::new`.
        Action::Operation(_) => describe(action),
        Action::Agent(_) => "no arguments".to_owned(),
    }
}

fn summarise_arguments(arguments: &Map<String, Value>) -> String {
    if arguments.is_empty() {
        return "no arguments".to_owned();
    }
    let mut parts: Vec<String> = arguments
        .iter()
        .take(MAX_SUMMARY_KEYS)
        .map(|(name, value)| summarise_argument(name, value))
        .collect();
    if arguments.len() > MAX_SUMMARY_KEYS {
        parts.push(format!("and {} more", arguments.len() - MAX_SUMMARY_KEYS));
    }
    format!("arguments {}", parts.join(", "))
}

fn summarise_argument(name: &str, value: &Value) -> String {
    if !is_plain_name(name) || has_opaque_run(name) {
        return format!("<name>={REDACTED}");
    }
    if resembles_secret(name) {
        return format!("{name}={REDACTED}");
    }
    let kind = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    format!("{name}={kind}")
}

/// A name that is safe to write: an ASCII identifier of at most 64 bytes.
fn is_plain_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 64
        && matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn summarise_ui(ui: &UiAction) -> String {
    let application = identifier(&ui.application);
    let matched = match ui.kind {
        UiActionKind::Observe => None,
        UiActionKind::Act => {
            match_consequential(&ui.control).or_else(|| match_consequential(&ui.window_title))
        }
    };
    match matched {
        Some(category) => format!("application {application}, pattern {}", category.as_str()),
        None => format!("application {application}"),
    }
}

/// The summary of a command outside the privileged sandbox: the program as it
/// was given, with its directory, and the number of arguments.
///
/// The path is written whole so that a program planted under a familiar name
/// in another directory does not read as the genuine one. It is withheld if it
/// is not a plain path or if a part of it resembles a secret.
fn summarise_command(command: &CommandAction) -> String {
    let program = command.argv.first().map_or("", String::as_str);
    let safe = !program.is_empty()
        && program.len() <= 255
        && program
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-' | b'/'))
        && !program.split('/').any(resembles_secret);
    let program = if safe { program } else { REDACTED };
    let count = command.argv.len().saturating_sub(1);
    format!("program {program}, {count} arguments")
}

/// The audit record could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditWriteError(pub String);

impl std::fmt::Display for AuditWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "audit record not written: {}", self.0)
    }
}

impl std::error::Error for AuditWriteError {}

/// Where audit records go.
pub trait AuditSink {
    /// Write one record. An error means the record is not durable.
    fn write(&mut self, record: &AuditRecord) -> Result<(), AuditWriteError>;
}

/// A sink that keeps records in memory, for tests.
#[derive(Debug, Default)]
pub struct MemorySink {
    records: Vec<AuditRecord>,
}

impl MemorySink {
    /// An empty sink.
    pub fn new() -> Self {
        MemorySink::default()
    }

    /// The records written so far, oldest first.
    pub fn records(&self) -> &[AuditRecord] {
        &self.records
    }
}

impl AuditSink for MemorySink {
    fn write(&mut self, record: &AuditRecord) -> Result<(), AuditWriteError> {
        self.records.push(record.clone());
        Ok(())
    }
}

/// Decide, and write the audit record before the decision is returned.
///
/// If the record cannot be written, the action is refused: an action that
/// leaves no record does not run.
pub fn decide_and_record(
    action: &Action,
    context: &Context,
    policy: &UserPolicy,
    now: Timestamp,
    meta: &AuditMeta<'_>,
    sink: &mut dyn AuditSink,
) -> Decision {
    let decision = decide(action, context, policy, now);
    let record = AuditRecord::new(meta, action, context, &decision);
    match sink.write(&record) {
        Ok(()) => decision,
        Err(_) => Decision::refuse(decision.class(), Refusal::AuditUnavailable),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::action::{
        AgentOperation, OperationContent, PrivilegedOperation, SECRET_PLACEHOLDER, ToolCall,
        TypedOperation, TypedOperationKind,
    };
    use crate::codes::Code;
    use crate::context::{RepairSession, WindowOwner};

    fn meta() -> AuditMeta<'static> {
        AuditMeta {
            task: "task-0001",
            provider: "none",
        }
    }

    fn trusted() -> Context {
        Context {
            trusted_path_available: true,
            target_window_owner: WindowOwner::User,
            ..Context::attended()
        }
    }

    fn privileged(argv: &[&str]) -> Action {
        Action::Command(CommandAction {
            sandbox: Sandbox::Privileged(PrivilegedOperation::ServiceControl),
            argv: argv.iter().map(|argument| (*argument).to_owned()).collect(),
            working_context: WorkingContext::Session,
        })
    }

    fn typed(kind: TypedOperationKind, target: &str, content: Option<(&str, bool)>) -> Action {
        Action::Operation(TypedOperation {
            kind,
            target: target.to_owned(),
            content: content.map(|(text, secret)| OperationContent {
                text: text.to_owned(),
                secret,
            }),
        })
    }

    fn tool(class: ActionClass, arguments: Value) -> Action {
        Action::Tool(ToolCall {
            service: "org.tur.Test1".to_owned(),
            tool: "test_tool".to_owned(),
            arguments: arguments.as_object().cloned().unwrap_or_default(),
            declared_class: class,
        })
    }

    fn record(action: &Action, context: &Context) -> AuditRecord {
        let decision = decide(action, context, &UserPolicy::default(), Timestamp(0));
        AuditRecord::new(&meta(), action, context, &decision)
    }

    fn field<'a>(fields: &'a [(&'static str, String)], name: &str) -> Option<&'a str> {
        fields
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn renders_the_journal_fields_of_the_addendum() {
        let action = tool(
            ActionClass::Reversible,
            json!({ "volume_id": "vol1", "force": true }),
        );
        let fields = record(&action, &trusted()).journal_fields();
        let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "MESSAGE",
                "TUR_AI_TASK",
                "TUR_AI_ACTION",
                "TUR_AI_CLASS",
                "TUR_AI_DECISION",
                "TUR_AI_TIER",
                "TUR_AI_ATTENDED",
                "TUR_AI_PROVIDER"
            ]
        );
        assert_eq!(field(&fields, "TUR_AI_ATTENDED"), Some("yes"));
        assert_eq!(field(&fields, "TUR_AI_TASK"), Some("task-0001"));
        assert_eq!(
            field(&fields, "TUR_AI_ACTION"),
            Some("org.tur.Test1:test_tool")
        );
        assert_eq!(field(&fields, "TUR_AI_CLASS"), Some("1"));
        assert_eq!(field(&fields, "TUR_AI_DECISION"), Some("allowed"));
        assert_eq!(field(&fields, "TUR_AI_TIER"), Some("1"));
        assert_eq!(field(&fields, "TUR_AI_PROVIDER"), Some("none"));
        assert_eq!(
            field(&fields, "MESSAGE"),
            Some(
                "tur-pax: allowed org.tur.Test1:test_tool (class 1, tier 1): reversible_permitted; arguments force=boolean, volume_id=string"
            )
        );
    }

    #[test]
    fn a_refusal_carries_its_error_code() {
        let action = tool(ActionClass::SystemCritical, json!({}));
        // No trusted path: class 3 is refused.
        let fields = record(&action, &Context::attended()).journal_fields();
        assert_eq!(field(&fields, "TUR_AI_DECISION"), Some("refused"));
        assert_eq!(field(&fields, "TUR_AI_CLASS"), Some("3"));
        assert_eq!(
            field(&fields, "TUR_ERROR_CODE"),
            Some(Code::PolicyTrustedPathUnavailable.as_str())
        );
        assert_eq!(fields.last().map(|(name, _)| *name), Some("TUR_ERROR_CODE"));
    }

    #[test]
    fn every_way_of_acting_has_a_tier_and_an_action_name() {
        let ui = UiAction {
            application: "org.example.Mail".to_owned(),
            window_title: "Compose".to_owned(),
            control: "Send".to_owned(),
            kind: UiActionKind::Act,
        };
        let command = CommandAction {
            sandbox: Sandbox::Privileged(PrivilegedOperation::ServiceControl),
            argv: vec![
                "/usr/bin/systemctl".to_owned(),
                "restart".to_owned(),
                "cups".to_owned(),
            ],
            working_context: WorkingContext::Session,
        };
        let cases = [
            (
                tool(ActionClass::Observe, json!({})),
                "1",
                "org.tur.Test1:test_tool",
            ),
            (Action::Accessibility(ui.clone()), "2", "accessibility:act"),
            (Action::ScreenInput(ui), "3", "screen:act"),
            (Action::Command(command), "4", "command:privileged"),
            (
                typed(TypedOperationKind::ServiceRestart, "cups.service", None),
                "4",
                "operation:service_restart",
            ),
            (
                Action::Agent(AgentOperation::MemoryWrite),
                "0",
                "agent:memory_write",
            ),
        ];
        for (action, tier, name) in cases {
            let fields = record(&action, &trusted()).journal_fields();
            assert_eq!(field(&fields, "TUR_AI_TIER"), Some(tier));
            assert_eq!(field(&fields, "TUR_AI_ACTION"), Some(name));
        }
    }

    #[test]
    fn summaries_name_the_pattern_and_the_program_but_no_content() {
        let ui = Action::Accessibility(UiAction {
            application: "org.example.Mail".to_owned(),
            window_title: "Re: salary review for Alex".to_owned(),
            control: "Send".to_owned(),
            kind: UiActionKind::Act,
        });
        let message = record(&ui, &trusted()).message();
        assert!(
            message.ends_with("; application org.example.Mail, pattern send"),
            "{message}"
        );
        assert!(!message.contains("salary"), "{message}");

        let command = Action::Command(CommandAction {
            sandbox: Sandbox::User,
            argv: vec![
                "/usr/bin/systemctl".to_owned(),
                "--user".to_owned(),
                "restart".to_owned(),
                "pipewire".to_owned(),
            ],
            working_context: WorkingContext::Session,
        });
        let message = record(&command, &trusted()).message();
        assert!(
            message.ends_with("; program /usr/bin/systemctl, 3 arguments"),
            "{message}"
        );
        assert!(!message.contains("pipewire"), "{message}");
    }

    #[test]
    fn a_command_is_named_by_the_path_it_was_given() {
        let summary = |program: &str| {
            summarise_command(&CommandAction {
                sandbox: Sandbox::User,
                argv: vec![program.to_owned(), "x".to_owned()],
                working_context: WorkingContext::Session,
            })
        };
        assert_eq!(summary("systemctl"), "program systemctl, 1 arguments");
        assert_eq!(
            summary("/usr/bin/systemctl"),
            "program /usr/bin/systemctl, 1 arguments"
        );
        // A program of the same name elsewhere reads differently.
        assert_eq!(
            summary("/tmp/x/systemctl"),
            "program /tmp/x/systemctl, 1 arguments"
        );
        assert_eq!(
            summary("/usr/lib64/qt6/bin/qdbus"),
            "program /usr/lib64/qt6/bin/qdbus, 1 arguments"
        );
        assert_eq!(summary("./run.sh"), "program ./run.sh, 1 arguments");
        // Not a plain path, or a part that resembles a secret: withheld.
        for program in [
            "",
            "my program",
            "/tmp/\u{202e}x",
            "x;y",
            "/home/u/sk-live-4f9a8b7c6d5e4f3a2b1c/run",
            "/home/u/password/run",
            "hunter2-correct-horse-battery",
        ] {
            assert_eq!(
                summary(program),
                "program <redacted>, 1 arguments",
                "{program:?}"
            );
        }
        let long = format!("/{}", "a".repeat(255));
        assert_eq!(summary(&long), "program <redacted>, 1 arguments");
    }

    #[test]
    fn the_record_states_the_sandbox_and_whether_the_task_was_attended() {
        let command = |sandbox, working_context| {
            Action::Command(CommandAction {
                sandbox,
                argv: vec!["true".to_owned()],
                working_context,
            })
        };
        let cases = [
            (
                command(Sandbox::ReadOnly, WorkingContext::Session),
                Some("read-only"),
            ),
            (
                command(Sandbox::ReadOnly, WorkingContext::UndoableWorkspace),
                Some("read-only"),
            ),
            (
                command(Sandbox::User, WorkingContext::UndoableWorkspace),
                Some("workspace"),
            ),
            (
                command(Sandbox::User, WorkingContext::Session),
                Some("user"),
            ),
            (
                command(
                    Sandbox::Privileged(PrivilegedOperation::Diagnostics),
                    WorkingContext::Session,
                ),
                Some("privileged"),
            ),
            (
                typed(TypedOperationKind::ServiceRestart, "cups.service", None),
                Some("privileged"),
            ),
            (tool(ActionClass::Observe, json!({})), None),
            (Action::Agent(AgentOperation::MemoryRead), None),
        ];
        for (action, sandbox) in cases {
            for (context, attended) in [
                (trusted(), "yes"),
                (Context::attended(), "yes"),
                (Context::unattended(), "no"),
                (
                    Context {
                        session_locked: true,
                        ..Context::unattended()
                    },
                    "no",
                ),
                (
                    // A locked session does not make an attended task unattended.
                    Context {
                        session_locked: true,
                        ..trusted()
                    },
                    "yes",
                ),
            ] {
                let record = record(&action, &context);
                let fields = record.journal_fields();
                assert_eq!(field(&fields, "TUR_AI_SANDBOX"), sandbox, "{action:?}");
                assert_eq!(
                    field(&fields, "TUR_AI_ATTENDED"),
                    Some(attended),
                    "{action:?}"
                );
                assert_eq!(record.sandbox(), sandbox);
                assert_eq!(record.attended(), attended == "yes");
            }
        }
    }

    #[test]
    fn a_privileged_request_is_recorded_in_full() {
        let command = privileged(&[
            "/usr/bin/systemctl",
            "restart",
            "cups.service",
            "--message=two words",
            "",
            "x\ny",
        ]);
        let statement = "Run command [\"/usr/bin/systemctl\" \"restart\" \"cups.service\" \"--message=two words\" \"\" \"x\\u{a}y\"] as root through the privileged helper (operation type service_control)";
        // Whatever the decision is: refused, offered for authentication, or
        // asked inside a session.
        let open = Context {
            repair_session: RepairSession::open("fix printing", Timestamp(0)),
            ..trusted()
        };
        for context in [Context::attended(), trusted(), open, Context::unattended()] {
            let record = record(&command, &context);
            assert_eq!(record.request(), Some(statement));
            let fields = record.journal_fields();
            assert_eq!(field(&fields, "TUR_AI_REQUEST"), Some(statement));
            assert!(
                field(&fields, "MESSAGE")
                    .unwrap_or_default()
                    .ends_with(&format!("; {statement}"))
            );
            for (name, value) in &fields {
                assert!(
                    value.chars().all(|c| matches!(c, ' '..='~')),
                    "{name} is not printable ASCII"
                );
            }
        }

        let restart = typed(TypedOperationKind::ServiceRestart, "cups.service", None);
        assert_eq!(
            record(&restart, &trusted()).request(),
            Some(
                "Privileged operation service_restart on unit \"cups.service\" as root through the privileged helper (operation type service_control)"
            )
        );
        let change = typed(
            TypedOperationKind::ConfigurationFileChange,
            "/etc/cups/cupsd.conf",
            Some(("LogLevel debug\n", false)),
        );
        assert_eq!(
            record(&change, &trusted()).request(),
            Some(
                "Privileged operation configuration_file_change on file \"/etc/cups/cupsd.conf\" with content \"LogLevel debug\\u{a}\" as root through the privileged helper (operation type system_configuration)"
            )
        );

        // Nothing else has the field, and nothing else is written in full.
        for action in [
            tool(ActionClass::Consequential, json!({ "to": "a@example.org" })),
            Action::Command(CommandAction {
                sandbox: Sandbox::User,
                argv: vec!["mail".to_owned(), "a@example.org".to_owned()],
                working_context: WorkingContext::Session,
            }),
            Action::Agent(AgentOperation::ScheduleWrite),
        ] {
            let record = record(&action, &trusted());
            assert_eq!(record.request(), None);
            let fields = record.journal_fields();
            assert_eq!(field(&fields, "TUR_AI_REQUEST"), None);
            for (name, value) in &fields {
                assert!(!value.contains("example.org"), "{name}={value}");
            }
        }
    }

    #[test]
    fn a_value_marked_secret_is_replaced_in_the_record_of_a_privileged_request() {
        let secret = "hunter2-correct-horse";
        let marked = typed(
            TypedOperationKind::ConfigurationFileChange,
            "/etc/samba/credentials",
            Some((secret, true)),
        );
        let open = Context {
            repair_session: RepairSession::open("fix the share", Timestamp(0)),
            ..trusted()
        };
        for context in [trusted(), open.clone(), Context::unattended()] {
            let record = record(&marked, &context);
            for (name, value) in record.journal_fields() {
                assert!(!value.contains(secret), "{name}={value} leaks the secret");
            }
            assert_eq!(
                record.request(),
                Some(
                    "Privileged operation configuration_file_change on file \"/etc/samba/credentials\" with content <secret> as root through the privileged helper (operation type system_configuration)"
                )
            );
            assert!(record.message().contains(SECRET_PLACEHOLDER));
            // The debug form of the record does not hold it either.
            assert!(!format!("{record:?}").contains(secret));
        }

        // The same value, not marked: recorded in full, as the design asks.
        // The marking is the orchestrator's, for a value the user entered; a
        // heuristic here would let the model hide what it writes.
        let unmarked = typed(
            TypedOperationKind::ConfigurationFileChange,
            "/etc/samba/credentials",
            Some((secret, false)),
        );
        assert!(
            record(&unmarked, &open)
                .request()
                .unwrap_or_default()
                .contains(secret)
        );
        // And a free-text command has nothing that could be marked.
        let command = privileged(&["smbpasswd", "-s", secret]);
        assert!(
            record(&command, &open)
                .request()
                .unwrap_or_default()
                .contains(secret)
        );
    }

    #[test]
    fn secrets_never_reach_the_record() {
        let secrets = [
            "hunter2-correct-horse",
            "sk-live-4f9a8b7c6d5e4f3a2b1c",
            "AKIAIOSFODNN7EXAMPLE",
            "s3cr3t value with spaces",
        ];
        for secret in secrets {
            let mut arguments = Map::new();
            arguments.insert("password".to_owned(), json!(secret));
            arguments.insert("note".to_owned(), json!(secret));
            arguments.insert(
                "nested".to_owned(),
                json!({ "api_key": secret, "list": [secret] }),
            );
            // The secret as a property name.
            arguments.insert(secret.to_owned(), json!(1));
            let tool_call = Action::Tool(ToolCall {
                service: "org.tur.Test1".to_owned(),
                tool: "test_tool".to_owned(),
                arguments,
                declared_class: ActionClass::Consequential,
            });
            let ui = Action::ScreenInput(UiAction {
                application: "org.example.Browser".to_owned(),
                window_title: format!("Sign in - {secret}"),
                control: secret.to_owned(),
                kind: UiActionKind::Act,
            });
            let command = Action::Command(CommandAction {
                sandbox: Sandbox::User,
                argv: vec![
                    "mysql".to_owned(),
                    format!("--password={secret}"),
                    secret.to_owned(),
                ],
                working_context: WorkingContext::Session,
            });
            let program_is_secret = Action::Command(CommandAction {
                sandbox: Sandbox::User,
                argv: vec![secret.to_owned()],
                working_context: WorkingContext::Session,
            });

            for action in [tool_call, ui, command, program_is_secret] {
                for context in [trusted(), Context::attended(), Context::unattended()] {
                    let decision = decide(&action, &context, &UserPolicy::default(), Timestamp(0));
                    let record = AuditRecord::new(&meta(), &action, &context, &decision);
                    for (name, value) in record.journal_fields() {
                        assert!(!value.contains(secret), "{name}={value} leaks {secret}");
                    }
                    // The prompt may show the content; the record must not hold it.
                    if let Some(prompt) = decision.prompt() {
                        assert!(!record.message().contains(prompt));
                    }
                }
            }
        }
    }

    #[test]
    fn argument_summary_withholds_what_resembles_a_secret() {
        let arguments = json!({
            "volume_id": "vol1",
            "password": "x",
            "auth_token": 5,
            "not a name": true,
            "a1b2c3d4e5f6a7b8c9d0e1f2": "opaque name"
        });
        let summary = summarise_arguments(arguments.as_object().expect("an object"));
        assert_eq!(
            summary,
            "arguments <name>=<redacted>, auth_token=<redacted>, <name>=<redacted>, password=<redacted>, volume_id=string"
        );
    }

    #[test]
    fn long_argument_lists_are_cut_short() {
        let mut arguments = Map::new();
        for index in 0..20 {
            arguments.insert(format!("key_{index:02}"), json!(index));
        }
        let summary = summarise_arguments(&arguments);
        assert!(
            summary.ends_with(", key_15=number, and 4 more"),
            "{summary}"
        );
    }

    #[test]
    fn identifiers_outside_the_character_set_are_not_written() {
        let meta = AuditMeta {
            task: "task 1\nTUR_AI_DECISION=allowed",
            provider: "",
        };
        let action = tool(ActionClass::Observe, json!({}));
        let decision = decide(&action, &trusted(), &UserPolicy::default(), Timestamp(0));
        let fields = AuditRecord::new(&meta, &action, &trusted(), &decision).journal_fields();
        assert_eq!(field(&fields, "TUR_AI_TASK"), Some("<invalid>"));
        assert_eq!(field(&fields, "TUR_AI_PROVIDER"), Some("<invalid>"));
        for (name, value) in &fields {
            assert!(!value.contains('\n'), "{name} spans lines");
        }
    }

    #[test]
    fn resembles_secret_recognises_markers_and_opaque_runs() {
        for text in [
            "password",
            "PASSWORD",
            "db_passwd",
            "client_secret",
            "access_token",
            "Authorization",
            "session_id",
            "ghp_16C7e42F292c6912E7710c838347Ae178B4a",
            "0123456789abcdef0123",
        ] {
            assert!(resembles_secret(text), "{text}");
        }
        for text in [
            "volume_id",
            "systemctl",
            "mode",
            "0123456789",
            "abcdefghijklmnopqrstuvwxyz",
            "",
        ] {
            assert!(!resembles_secret(text), "{text}");
        }
    }

    #[test]
    fn user_response_follows_only_a_prompt() {
        let asked = record(&tool(ActionClass::Consequential, json!({})), &trusted());
        assert_eq!(asked.outcome(), AuditOutcome::ConfirmationRequired);
        assert_eq!(
            asked.user_response(true).map(|record| record.outcome()),
            Some(AuditOutcome::Confirmed)
        );
        assert_eq!(
            asked.user_response(false).map(|record| record.outcome()),
            Some(AuditOutcome::Denied)
        );

        let authenticate = record(&tool(ActionClass::SystemCritical, json!({})), &trusted());
        assert_eq!(authenticate.outcome(), AuditOutcome::AuthenticationRequired);
        assert!(authenticate.user_response(true).is_some());

        let allowed = record(&tool(ActionClass::Observe, json!({})), &trusted());
        assert_eq!(allowed.user_response(true), None);
        let refused = record(&tool(ActionClass::Prohibited, json!({})), &trusted());
        assert_eq!(refused.user_response(true), None);
    }

    struct FailingSink;

    impl AuditSink for FailingSink {
        fn write(&mut self, _record: &AuditRecord) -> Result<(), AuditWriteError> {
            Err(AuditWriteError("disk full".to_owned()))
        }
    }

    #[test]
    fn decide_and_record_writes_one_record_per_decision() {
        let mut sink = MemorySink::new();
        let policy = UserPolicy::default();
        for class in ActionClass::ALL {
            let action = tool(class, json!({}));
            let decision = decide_and_record(
                &action,
                &trusted(),
                &policy,
                Timestamp(0),
                &meta(),
                &mut sink,
            );
            assert_eq!(decision, decide(&action, &trusted(), &policy, Timestamp(0)));
        }
        let outcomes: Vec<AuditOutcome> = sink.records().iter().map(AuditRecord::outcome).collect();
        assert_eq!(
            outcomes,
            [
                AuditOutcome::Allowed,
                AuditOutcome::Allowed,
                AuditOutcome::ConfirmationRequired,
                AuditOutcome::AuthenticationRequired,
                AuditOutcome::Refused
            ]
        );
    }

    #[test]
    fn an_action_that_cannot_be_recorded_is_refused() {
        let policy = UserPolicy::default();
        let mut context = trusted();
        context.repair_session = RepairSession::open("fix printing", Timestamp(0));
        let actions = [
            tool(ActionClass::Observe, json!({})),
            tool(ActionClass::Reversible, json!({})),
            typed(TypedOperationKind::ServiceRestart, "cups.service", None),
        ];
        for action in actions {
            assert!(decide(&action, &context, &policy, Timestamp(0)).is_allow());
            let decision = decide_and_record(
                &action,
                &context,
                &policy,
                Timestamp(0),
                &meta(),
                &mut FailingSink,
            );
            assert_eq!(decision.code(), Some(Code::AuditUnavailable));
            assert!(!decision.is_allow());
        }
    }
}
