// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The audit record (addendum section 11).
//!
//! One record is written for every proposed action and its decision. A record
//! is rendered to journal fields by [`AuditRecord::journal_fields`] and written
//! through an [`AuditSink`]. This crate has no journald dependency; the sink
//! for the journal belongs to the service.
//!
//! What a record contains is restricted by construction:
//!
//! * Argument values are never written. Tool arguments are summarised as the
//!   property names with the kind of each value.
//! * A property name that resembles a secret has its kind withheld too; a name
//!   that is not a plain identifier is not written at all.
//! * A command is summarised as its program name and argument count.
//! * Window titles, control names and prompt texts are never written.
//! * Identifiers (task, provider, service, tool, application) are written only
//!   if they use a restricted character set. They are not scanned for secrets:
//!   service and tool names come from a validated manifest, and the task and
//!   provider names from the orchestrator.

use serde_json::{Map, Value};
use uos_core::ErrorCode;

use crate::action::{Action, CommandAction, Sandbox, UiAction, UiActionKind};
use crate::class::ActionClass;
use crate::context::{Context, Timestamp};
use crate::decision::{Decision, Refusal, Verdict};
use crate::engine::decide;
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
/// use uos_agent::audit::resembles_secret;
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

/// The outcome recorded for an action.
///
/// The addendum lists `allowed`, `confirmed`, `denied` and `refused`. The
/// record of a proposal also needs the outcomes that are not final yet.
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
    /// The value of the `UOS_AI_DECISION` field.
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
}

impl AuditRecord {
    /// The record for a proposed action and the engine's decision.
    pub fn new(meta: &AuditMeta<'_>, action: &Action, decision: &Decision) -> Self {
        AuditRecord {
            task: identifier(meta.task),
            provider: identifier(meta.provider),
            action: action_name(action),
            class: decision.class(),
            tier: action.tier(),
            outcome: AuditOutcome::of(decision.verdict()),
            reason: decision.reason().as_str(),
            error_code: decision.error_code(),
            summary: summarise(action),
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

    /// The human-readable `MESSAGE` field.
    pub fn message(&self) -> String {
        format!(
            "uos-agent: {} {} (class {}, tier {}): {}; {}",
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
    /// `UOS_ERROR_CODE` is present only when the action was refused.
    pub fn journal_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("MESSAGE", self.message()),
            ("UOS_AI_TASK", self.task.clone()),
            ("UOS_AI_ACTION", self.action.clone()),
            ("UOS_AI_CLASS", self.class.to_string()),
            ("UOS_AI_DECISION", self.outcome.as_str().to_owned()),
            ("UOS_AI_TIER", self.tier.to_string()),
            ("UOS_AI_PROVIDER", self.provider.clone()),
        ];
        if let Some(code) = &self.error_code {
            fields.push(("UOS_ERROR_CODE", code.to_string()));
        }
        fields
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
        Action::Agent(operation) => format!("agent:{}", operation.as_str()),
    }
}

fn summarise(action: &Action) -> String {
    match action {
        Action::Tool(call) => summarise_arguments(&call.arguments),
        Action::Accessibility(ui) | Action::ScreenInput(ui) => summarise_ui(ui),
        Action::Command(command) => summarise_command(command),
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

fn summarise_command(command: &CommandAction) -> String {
    let program = command
        .argv
        .first()
        .and_then(|path| path.rsplit('/').next())
        .unwrap_or("");
    let safe = !program.is_empty()
        && program.len() <= 64
        && program
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
        && !resembles_secret(program);
    let program = if safe { program } else { REDACTED };
    let count = command.argv.len().saturating_sub(1);
    match command.sandbox {
        Sandbox::Privileged(operation) => format!(
            "program {program}, {count} arguments, operation {}",
            operation.as_str()
        ),
        Sandbox::ReadOnly | Sandbox::User => format!("program {program}, {count} arguments"),
    }
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
    let record = AuditRecord::new(meta, action, &decision);
    match sink.write(&record) {
        Ok(()) => decision,
        Err(_) => Decision::refuse(decision.class(), Refusal::AuditUnavailable),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::action::{AgentOperation, PrivilegedOperation, ToolCall, WorkingContext};
    use crate::codes::Code;
    use crate::context::RepairSession;

    fn meta() -> AuditMeta<'static> {
        AuditMeta {
            task: "task-0001",
            provider: "none",
        }
    }

    fn trusted() -> Context {
        Context {
            trusted_path_available: true,
            ..Context::attended()
        }
    }

    fn tool(class: ActionClass, arguments: Value) -> Action {
        Action::Tool(ToolCall {
            service: "org.uos.Test1".to_owned(),
            tool: "test_tool".to_owned(),
            arguments: arguments.as_object().cloned().unwrap_or_default(),
            declared_class: class,
        })
    }

    fn record(action: &Action, context: &Context) -> AuditRecord {
        let decision = decide(action, context, &UserPolicy::default(), Timestamp(0));
        AuditRecord::new(&meta(), action, &decision)
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
                "UOS_AI_TASK",
                "UOS_AI_ACTION",
                "UOS_AI_CLASS",
                "UOS_AI_DECISION",
                "UOS_AI_TIER",
                "UOS_AI_PROVIDER"
            ]
        );
        assert_eq!(field(&fields, "UOS_AI_TASK"), Some("task-0001"));
        assert_eq!(
            field(&fields, "UOS_AI_ACTION"),
            Some("org.uos.Test1:test_tool")
        );
        assert_eq!(field(&fields, "UOS_AI_CLASS"), Some("1"));
        assert_eq!(field(&fields, "UOS_AI_DECISION"), Some("allowed"));
        assert_eq!(field(&fields, "UOS_AI_TIER"), Some("1"));
        assert_eq!(field(&fields, "UOS_AI_PROVIDER"), Some("none"));
        assert_eq!(
            field(&fields, "MESSAGE"),
            Some(
                "uos-agent: allowed org.uos.Test1:test_tool (class 1, tier 1): reversible_permitted; arguments force=boolean, volume_id=string"
            )
        );
    }

    #[test]
    fn a_refusal_carries_its_error_code() {
        let action = tool(ActionClass::SystemCritical, json!({}));
        // No trusted path: class 3 is refused.
        let fields = record(&action, &Context::attended()).journal_fields();
        assert_eq!(field(&fields, "UOS_AI_DECISION"), Some("refused"));
        assert_eq!(field(&fields, "UOS_AI_CLASS"), Some("3"));
        assert_eq!(
            field(&fields, "UOS_ERROR_CODE"),
            Some(Code::PolicyTrustedPathUnavailable.as_str())
        );
        assert_eq!(fields.last().map(|(name, _)| *name), Some("UOS_ERROR_CODE"));
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
            (Action::Accessibility(ui.clone()), "2", "accessibility:act"),
            (Action::ScreenInput(ui), "3", "screen:act"),
            (Action::Command(command), "4", "command:privileged"),
            (
                Action::Agent(AgentOperation::MemoryWrite),
                "0",
                "agent:memory_write",
            ),
        ];
        for (action, tier, name) in cases {
            let fields = record(&action, &trusted()).journal_fields();
            assert_eq!(field(&fields, "UOS_AI_TIER"), Some(tier));
            assert_eq!(field(&fields, "UOS_AI_ACTION"), Some(name));
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
            sandbox: Sandbox::Privileged(PrivilegedOperation::ServiceControl),
            argv: vec![
                "/usr/bin/systemctl".to_owned(),
                "restart".to_owned(),
                "cups".to_owned(),
            ],
            working_context: WorkingContext::Session,
        });
        let message = record(&command, &trusted()).message();
        assert!(
            message.ends_with("; program systemctl, 2 arguments, operation service_control"),
            "{message}"
        );
        assert!(!message.contains("cups"), "{message}");
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
                service: "org.uos.Test1".to_owned(),
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
                    let record = AuditRecord::new(&meta(), &action, &decision);
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
            task: "task 1\nUOS_AI_DECISION=allowed",
            provider: "",
        };
        let action = tool(ActionClass::Observe, json!({}));
        let decision = decide(&action, &trusted(), &UserPolicy::default(), Timestamp(0));
        let fields = AuditRecord::new(&meta, &action, &decision).journal_fields();
        assert_eq!(field(&fields, "UOS_AI_TASK"), Some("<invalid>"));
        assert_eq!(field(&fields, "UOS_AI_PROVIDER"), Some("<invalid>"));
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
        context.repair_session = Some(RepairSession {
            purpose: "fix printing".to_owned(),
            expires_at: Timestamp(100),
        });
        let privileged = Action::Command(CommandAction {
            sandbox: Sandbox::Privileged(PrivilegedOperation::ServiceControl),
            argv: vec!["systemctl".to_owned()],
            working_context: WorkingContext::Session,
        });
        let actions = [
            tool(ActionClass::Observe, json!({})),
            tool(ActionClass::Reversible, json!({})),
            privileged,
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
