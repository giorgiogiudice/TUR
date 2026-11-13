// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The policy engine.
//!
//! [`decide`] is a pure function. It performs no I/O and reads no clock: the
//! time is a parameter. It takes no input from the assistant's memory, and the
//! only input that originates from the model, besides the proposal itself, is
//! the "consequential" hint, which can only raise the class and add a
//! confirmation.
//!
//! The rules are evaluated in a fixed order and the first that applies decides.
//! `docs/architecture/agent-policy.md` holds the same table in prose.

use serde_json::Value;

use crate::action::{
    Action, CommandAction, PrivilegedOperation, PrivilegedRequestKind, SECRET_PLACEHOLDER, Sandbox,
    TypedOperation, UiAction, UiActionKind, UiOperation, UiRole, WorkingContext,
};
use crate::class::ActionClass;
use crate::context::{Context, Presence, Timestamp, WindowOwner};
use crate::decision::{Decision, Reason, Refusal};
use crate::patterns::{is_legible, match_consequential};
use crate::policy::{Mode, RelaxationLevel, UserPolicy};

/// Decide what happens to one proposed action.
///
/// The decision depends only on the arguments. Calling it again with the same
/// arguments gives the same decision; an earlier approval is never remembered.
pub fn decide(action: &Action, context: &Context, policy: &UserPolicy, now: Timestamp) -> Decision {
    let class = classify(action, context, policy);

    // 1. An action that cannot be classified does not run.
    if !is_well_formed(action) {
        return Decision::refuse(class, Refusal::MalformedAction);
    }

    // 2. Class 4 is refused, whatever else is true.
    if class == ActionClass::Prohibited {
        return Decision::refuse(class, Refusal::Prohibited);
    }

    // 3. Accessibility and screen actions: privacy exclusions, windows of
    //    processes with administrator rights, and the lock.
    let ui = match action {
        Action::Accessibility(ui) | Action::ScreenInput(ui) => Some(ui),
        Action::Tool(_) | Action::Command(_) | Action::Operation(_) | Action::Agent(_) => None,
    };
    if let Some(ui) = ui {
        if ui.identities().any(|identity| policy.is_excluded(identity)) {
            return Decision::refuse(class, Refusal::ExcludedApplication);
        }
        if context.target_window_owner == WindowOwner::Administrator {
            return Decision::refuse(class, Refusal::PrivilegedWindow);
        }
        if context.session_locked {
            return Decision::refuse(class, Refusal::SessionLocked);
        }
    }

    // 4. Observe-only mode: nothing above class 0.
    if policy.mode == Mode::ObserveOnly && class > ActionClass::Observe {
        return Decision::refuse(class, Refusal::ObserveOnly);
    }

    // 5. Without the trusted path, nothing above class 1, and no request to
    //    authenticate, because it could not be answered safely.
    if !context.trusted_session_proven && class >= ActionClass::Consequential {
        return Decision::refuse(class, Refusal::TrustedPathUnavailable);
    }

    // A locked session has nobody at the screen, whatever the task says.
    let present = context.presence == Presence::Attended && !context.session_locked;
    let privileged = action.privileged_request();

    // 6. Class 3: its own authentication, every time. Never as a privileged
    //    request, never without a person present, never covered by a repair
    //    session.
    if class == ActionClass::SystemCritical {
        if privileged.is_some() {
            return Decision::refuse(class, Refusal::SystemCriticalAsCommand);
        }
        if !present {
            return Decision::refuse(class, Refusal::SystemCriticalUnattended);
        }
        return Decision::authenticate(
            class,
            Reason::SystemCriticalNeedsAuthentication,
            describe(action),
        );
    }

    // 7. A privileged request: only with a person present, only inside an open
    //    repair session. A lock closes the repair session. Inside one, a typed
    //    operation runs; a free-text command is still approved one at a time.
    if let Some(kind) = privileged {
        if !present {
            return Decision::refuse(class, Refusal::PrivilegedUnattended);
        }
        let session_open = context
            .repair_session
            .as_ref()
            .is_some_and(|session| session.is_open_at(now));
        if !session_open {
            return Decision::authenticate(
                class,
                Reason::RepairSessionRequired,
                format!(
                    "Open a repair session to run privileged commands. Requested: {}",
                    describe(action)
                ),
            );
        }
        if policy.mode == Mode::AskEveryAction {
            return Decision::ask(class, Reason::AskEveryActionMode, describe(action));
        }
        return match kind {
            PrivilegedRequestKind::FreeText => Decision::ask(
                class,
                Reason::PrivilegedCommandNeedsApproval,
                describe(action),
            ),
            PrivilegedRequestKind::Typed if context.model_flags_consequential => Decision::ask(
                class,
                Reason::ConsequentialNeedsConfirmation,
                describe(action),
            ),
            PrivilegedRequestKind::Typed => Decision::allow(class, Reason::RepairSessionOpen),
        };
    }

    // 8. An attended task pauses while the session is locked. The standing
    //    approvals of a schedule are not for it.
    if context.presence == Presence::Attended && context.session_locked {
        return Decision::defer(class, Reason::PausedWhileLocked);
    }

    // 9. Unattended: nobody can be asked.
    if context.presence == Presence::Unattended {
        if policy.mode == Mode::AskEveryAction {
            return Decision::defer(class, Reason::UnattendedAskEveryActionMode);
        }
        // An unattended task records only what it observed. An entry on the
        // user's word waits for the user (addendum section 18, item 4).
        if let Action::Agent(operation) = action
            && operation.needs_the_user_present()
        {
            return Decision::defer(class, Reason::UnattendedMemoryNotObserved);
        }
        // While the session is locked: tools and read-only commands only.
        if context.session_locked
            && let Action::Command(command) = action
            && command.sandbox != Sandbox::ReadOnly
        {
            return Decision::defer(class, Reason::UnattendedCommandWhileLocked);
        }
        if class <= ActionClass::Reversible {
            return Decision::allow(class, permitted(class));
        }
        // A standing approval is honoured only as the policy store holds it
        // (`HeldApprovals`). No store exists yet, so outside tests nothing
        // is held and the action is deferred (addendum section 18, item 1).
        if let Action::Tool(call) = action
            && context.standing_approvals.covers(call)
        {
            return Decision::allow(class, Reason::StandingApprovalMatched);
        }
        return Decision::defer(class, Reason::UnattendedWithoutStandingApproval);
    }

    // 10. Attended, with the user present.
    if policy.mode == Mode::AskEveryAction {
        return Decision::ask(class, Reason::AskEveryActionMode, describe(action));
    }
    if class <= ActionClass::Reversible {
        return Decision::allow(class, permitted(class));
    }
    // A relaxation covers an application the engine can identify, in a window
    // known to be the user's, and never an action the model itself flagged.
    if let Some(ui) = ui
        && !context.model_flags_consequential
        && context.target_window_owner == WindowOwner::User
        && !is_sensitive_target(ui, policy)
        && policy.relaxation_for(&ui.application) == Some(RelaxationLevel::Consequential)
    {
        return Decision::allow(class, Reason::RelaxedByUserPolicy);
    }
    Decision::ask(
        class,
        Reason::ConsequentialNeedsConfirmation,
        describe(action),
    )
}

/// The class an action is treated as.
///
/// This is the class from the action's own source (the manifest, the sandbox,
/// the operation type, the UI rule set or the built-in table), raised to
/// class 2 if the model flagged the action as consequential. Nothing lowers
/// it.
pub fn classify(action: &Action, context: &Context, policy: &UserPolicy) -> ActionClass {
    let base = match action {
        Action::Tool(call) => call.declared_class,
        Action::Accessibility(ui) | Action::ScreenInput(ui) => ui_class(ui, context, policy),
        Action::Command(command) => command_class(command),
        Action::Operation(operation) => privileged_class(operation.operation_type()),
        Action::Agent(operation) => operation.class(),
    };
    if context.model_flags_consequential {
        base.max(ActionClass::Consequential)
    } else {
        base
    }
}

/// Whether the target application is sensitive under any of the identifiers
/// it is known by.
fn is_sensitive_target(ui: &UiAction, policy: &UserPolicy) -> bool {
    ui.identities().any(|identity| policy.is_sensitive(identity))
}

/// The class of an accessibility or screen action (addendum section 5.1).
///
/// Never above class 2: privileged prompts reject emulated input, and a window
/// of a process with administrator rights is refused before the class matters.
///
/// An action whose detail states an operation is judged further by what the
/// accessibility bus says about its target (`pax-accessibility.md`, section
/// 5). Every rule of that judgement can only raise the class.
fn ui_class(ui: &UiAction, context: &Context, policy: &UserPolicy) -> ActionClass {
    if is_sensitive_target(ui, policy) {
        return ActionClass::Consequential;
    }
    match ui.kind {
        UiActionKind::Observe => ActionClass::Observe,
        UiActionKind::Act => {
            if acting_is_consequential(ui, context) {
                ActionClass::Consequential
            } else {
                ActionClass::Reversible
            }
        }
    }
}

/// Whether acting on the target of `ui` is consequential: when in doubt,
/// ask.
fn acting_is_consequential(ui: &UiAction, context: &Context) -> bool {
    let detail = &ui.detail;
    // The rules for every action: a window whose owner the compositor did
    // not report, a window title, a name or a description that matches the
    // pattern tables, the default control of a dialog, and a terminal.
    if context.target_window_owner == WindowOwner::Unknown
        || match_consequential(&ui.window_title).is_some()
        || match_consequential(&ui.control).is_some()
        || match_consequential(&detail.description).is_some()
        || detail.default_control
        || detail.role == UiRole::Terminal
    {
        return true;
    }
    let named = !ui.control.is_empty();
    let legible = is_legible(&ui.control);
    match detail.operation {
        // A name that cannot be read (section 5.1, review item 1).
        UiOperation::Unspecified
        | UiOperation::Read
        | UiOperation::Expand
        | UiOperation::Collapse
        | UiOperation::Select => !legible,
        // Moving the keyboard or the view changes nothing in the
        // application: an element without a name may be focused or
        // scrolled, one with a name that cannot be read may not.
        UiOperation::Focus | UiOperation::Scroll => named && !legible,
        // A press is judged from the name of a control: only a control
        // whose role says it can be pressed, and whose name can be read.
        UiOperation::Press => !legible || !detail.role.is_pressable(),
        // Typing inserts text and submits nothing, as long as the text
        // holds no line break or other control character (Enter submits a
        // form or sends a message), the field takes text, and the field is
        // not for a secret. The field may have no name; a name it has must
        // be readable.
        UiOperation::Type => {
            detail.role != UiRole::Text
                || !detail.editable
                || detail.text.is_empty()
                || detail.text.chars().any(is_control_like)
                || (named && !legible)
        }
    }
}

/// A character that is not text to insert: a control character, a line or
/// paragraph separator, or a character that changes the reading direction.
fn is_control_like(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{2028}' | '\u{2029}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

/// The class of a command. It comes from the sandbox, never from the text.
fn command_class(command: &CommandAction) -> ActionClass {
    match command.sandbox {
        Sandbox::ReadOnly => ActionClass::Observe,
        Sandbox::User => match command.working_context {
            WorkingContext::UndoableWorkspace => ActionClass::Reversible,
            WorkingContext::Session => ActionClass::Consequential,
        },
        Sandbox::Privileged(operation) => privileged_class(operation),
    }
}

/// The class of a privileged request, from its operation type: class 2 if a
/// repair session may cover the type, class 3 otherwise.
fn privileged_class(operation: PrivilegedOperation) -> ActionClass {
    if operation.repair_session_may_cover() {
        ActionClass::Consequential
    } else {
        ActionClass::SystemCritical
    }
}

fn is_well_formed(action: &Action) -> bool {
    match action {
        Action::Tool(call) => !call.service.is_empty() && !call.tool.is_empty(),
        Action::Accessibility(ui) | Action::ScreenInput(ui) => is_well_formed_ui(ui),
        Action::Command(command) => command
            .argv
            .first()
            .is_some_and(|program| !program.is_empty()),
        Action::Operation(operation) => operation.is_well_formed(),
        Action::Agent(_) => true,
    }
}

/// A UI action names its application; an operation it states goes with its
/// kind; text goes with typing, and only with typing.
fn is_well_formed_ui(ui: &UiAction) -> bool {
    let operation = ui.detail.operation;
    !ui.application.trim().is_empty()
        && operation.kind().is_none_or(|kind| kind == ui.kind)
        && (operation == UiOperation::Type) == !ui.detail.text.is_empty()
}

fn permitted(class: ActionClass) -> Reason {
    if class == ActionClass::Observe {
        Reason::ObservePermitted
    } else {
        Reason::ReversiblePermitted
    }
}

/// A statement of the exact action, for a confirmation prompt and, for a
/// privileged request, for the audit record.
///
/// Every piece of text that did not come from this crate is escaped: anything
/// other than printable ASCII appears as a `\u{...}` escape, so the text
/// cannot hide characters, change direction or break out of its line. A
/// command is shown argument by argument and is never shortened. A value that
/// is marked secret is replaced.
pub fn describe(action: &Action) -> String {
    match action {
        Action::Tool(call) => format!(
            "Call tool {} of service {} with arguments {}",
            quoted(&call.tool),
            quoted(&call.service),
            printable(&Value::Object(call.arguments.clone()).to_string())
        ),
        Action::Accessibility(ui) => describe_ui(ui, "through the accessibility tree"),
        Action::ScreenInput(ui) => describe_ui(ui, "through screen and input"),
        Action::Command(command) => describe_command(command),
        Action::Operation(operation) => describe_operation(operation),
        Action::Agent(operation) => format!("Agent operation {}", operation.as_str()),
    }
}

fn describe_ui(ui: &UiAction, tier: &str) -> String {
    let verb = match (ui.kind, ui.detail.operation) {
        (UiActionKind::Observe, _) => "Read",
        (UiActionKind::Act, UiOperation::Focus) => "Focus",
        (UiActionKind::Act, UiOperation::Scroll) => "Scroll to",
        (UiActionKind::Act, UiOperation::Expand) => "Expand",
        (UiActionKind::Act, UiOperation::Collapse) => "Collapse",
        (UiActionKind::Act, UiOperation::Select) => "Select",
        (UiActionKind::Act, UiOperation::Press) => "Press",
        (UiActionKind::Act, UiOperation::Type) => "Type into",
        (UiActionKind::Act, UiOperation::Unspecified | UiOperation::Read) => "Act on",
    };
    let role = match ui.detail.role {
        UiRole::Unknown => String::new(),
        role => format!(" ({})", role.as_str()),
    };
    let typed = if ui.detail.operation == UiOperation::Type {
        format!(" the text {}", quoted(&ui.detail.text))
    } else {
        String::new()
    };
    format!(
        "{verb} control {}{role}{typed} in window {} of application {}, {tier}",
        quoted(&ui.control),
        quoted(&ui.window_title),
        quoted(&ui.application)
    )
}

fn describe_command(command: &CommandAction) -> String {
    let argv: Vec<String> = command
        .argv
        .iter()
        .map(|argument| quoted(argument.as_str()))
        .collect();
    let place = match command.sandbox {
        Sandbox::ReadOnly => "in the read-only sandbox".to_owned(),
        Sandbox::User => match command.working_context {
            WorkingContext::Session => "with your privileges, on your files".to_owned(),
            WorkingContext::UndoableWorkspace => {
                "with your privileges, in a task workspace that can be undone".to_owned()
            }
        },
        Sandbox::Privileged(operation) => format!(
            "as root through the privileged helper (operation type {})",
            operation.as_str()
        ),
    };
    format!("Run command [{}] {place}", argv.join(" "))
}

fn describe_operation(operation: &TypedOperation) -> String {
    let spec = operation.kind.spec();
    let content = match &operation.content {
        None => String::new(),
        Some(content) if content.secret => format!(" with content {SECRET_PLACEHOLDER}"),
        Some(content) => format!(" with content {}", quoted(&content.text)),
    };
    format!(
        "Privileged operation {} on {} {}{content} as root through the privileged helper (operation type {})",
        spec.name,
        spec.target.noun(),
        quoted(&operation.target),
        operation.operation_type().as_str()
    )
}

/// `text` with everything except printable ASCII escaped.
fn printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, ' '..='~') {
            out.push(c);
        } else {
            out.extend(c.escape_unicode());
        }
    }
    out
}

/// `text` in double quotes, with quotes, backslashes and everything except
/// printable ASCII escaped.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            _ => out.extend(c.escape_unicode()),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::action::{OperationContent, ToolCall, TypedOperationKind};
    use crate::context::{RepairSession, StandingApproval};
    use crate::store::{HeldApprovals, PolicyStore};

    /// A stand-in for the policy store, which does not exist yet.
    struct TestStore(Vec<StandingApproval>);

    impl PolicyStore for TestStore {
        fn standing_approvals(&self, _schedule: &str) -> Vec<StandingApproval> {
            self.0.clone()
        }
    }

    const NOW: Timestamp = Timestamp(1_000);

    fn tool(class: ActionClass) -> Action {
        Action::Tool(ToolCall {
            service: "org.tur.Test1".to_owned(),
            tool: "test_tool".to_owned(),
            arguments: json!({ "name": "value" })
                .as_object()
                .cloned()
                .unwrap_or_default(),
            declared_class: class,
        })
    }

    fn command(sandbox: Sandbox, argv: &[&str]) -> Action {
        Action::Command(CommandAction {
            sandbox,
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

    fn trusted() -> Context {
        Context {
            trusted_session_proven: true,
            target_window_owner: WindowOwner::User,
            ..Context::attended()
        }
    }

    fn open_session() -> Option<RepairSession> {
        RepairSession::open("fix printing", NOW)
    }

    /// Arguments that hold `text` as a name, as a value and inside a list.
    fn hostile_arguments(text: &str) -> serde_json::Map<String, Value> {
        let mut inner = serde_json::Map::new();
        inner.insert(text.to_owned(), json!(text));
        let mut arguments = serde_json::Map::new();
        arguments.insert(text.to_owned(), json!([text, Value::Object(inner)]));
        arguments
    }

    #[test]
    fn quoting_is_unambiguous_and_hides_nothing() {
        assert_eq!(quoted("plain"), "\"plain\"");
        assert_eq!(quoted("two words"), "\"two words\"");
        assert_eq!(quoted("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quoted("back\\slash"), "\"back\\\\slash\"");
        assert_eq!(quoted("line\nbreak"), "\"line\\u{a}break\"");
        // A right-to-left override cannot reorder the prompt.
        assert_eq!(quoted("a\u{202e}b"), "\"a\\u{202e}b\"");
        assert_eq!(quoted("é"), "\"\\u{e9}\"");
        assert_eq!(quoted("\u{7f}"), "\"\\u{7f}\"");
        assert_eq!(quoted("\u{0}"), "\"\\u{0}\"");
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(
            printable("{\"k\":\"é\u{7f}\"}"),
            "{\"k\":\"\\u{e9}\\u{7f}\"}"
        );
    }

    #[test]
    fn a_prompt_is_printable_ascii_whatever_the_action_holds() {
        let hostile = [
            "\u{202e}txt.exe",
            "a\u{200b}b",
            "line\nbreak\r\n",
            "tab\there",
            "nul\u{0}",
            "\u{1b}[2J\u{1b}[H",
            "\u{405}end",
            "\u{feff}",
            "\"; rm -rf ~; \"",
            "\\",
        ];
        for text in hostile {
            let actions = [
                command(Sandbox::User, &[text, text]),
                command(
                    Sandbox::Privileged(PrivilegedOperation::ServiceControl),
                    &["sh", "-c", text],
                ),
                Action::ScreenInput(UiAction {
                    application: text.to_owned(),
                    window_title: text.to_owned(),
                    control: text.to_owned(),
                    kind: UiActionKind::Act,
                    detail: crate::action::UiDetail::default(),
                }),
                typed(TypedOperationKind::NetworkProfileReset, text, None),
                typed(
                    TypedOperationKind::ConfigurationFileChange,
                    "/etc/cups/cupsd.conf",
                    Some((text, false)),
                ),
                Action::Tool(ToolCall {
                    service: "org.tur.Test1".to_owned(),
                    tool: "test_tool".to_owned(),
                    arguments: hostile_arguments(text),
                    declared_class: ActionClass::Consequential,
                }),
            ];
            for action in actions {
                let statement = describe(&action);
                assert!(
                    statement.chars().all(|c| matches!(c, ' '..='~')),
                    "{statement:?}"
                );
            }
        }
    }

    #[test]
    fn a_command_prompt_shows_every_argument_exactly() {
        let action = command(Sandbox::User, &["rm", "-rf", "my files", "", "x\ny"]);
        let decision = decide(&action, &trusted(), &UserPolicy::default(), NOW);
        assert_eq!(
            decision.prompt(),
            Some(
                "Run command [\"rm\" \"-rf\" \"my files\" \"\" \"x\\u{a}y\"] with your privileges, on your files"
            )
        );
    }

    #[test]
    fn a_long_command_is_never_shortened() {
        let padding = " ".repeat(10_000);
        let argument = format!("echo ok{padding}; rm -rf ~");
        let action = command(Sandbox::User, &["sh", "-c", argument.as_str()]);
        let decision = decide(&action, &trusted(), &UserPolicy::default(), NOW);
        let prompt = decision.prompt().expect("a class 2 command is asked");
        assert!(prompt.contains("rm -rf ~"));
        assert!(prompt.len() > 10_000);
    }

    #[test]
    fn a_tool_prompt_shows_the_exact_arguments() {
        let decision = decide(
            &tool(ActionClass::Consequential),
            &trusted(),
            &UserPolicy::default(),
            NOW,
        );
        assert_eq!(
            decision.prompt(),
            Some(
                "Call tool \"test_tool\" of service \"org.tur.Test1\" with arguments {\"name\":\"value\"}"
            )
        );
    }

    #[test]
    fn the_repair_session_prompt_names_the_operation_type() {
        let action = command(
            Sandbox::Privileged(PrivilegedOperation::ServiceControl),
            &["systemctl", "restart", "cups"],
        );
        let decision = decide(&action, &trusted(), &UserPolicy::default(), NOW);
        assert_eq!(decision.reason(), Reason::RepairSessionRequired);
        assert_eq!(
            decision.prompt(),
            Some(
                "Open a repair session to run privileged commands. Requested: Run command [\"systemctl\" \"restart\" \"cups\"] as root through the privileged helper (operation type service_control)"
            )
        );
    }

    #[test]
    fn a_typed_operation_prompt_states_the_operation_and_its_target() {
        let restart = typed(TypedOperationKind::ServiceRestart, "cups.service", None);
        assert_eq!(
            describe(&restart),
            "Privileged operation service_restart on unit \"cups.service\" as root through the privileged helper (operation type service_control)"
        );

        let change = typed(
            TypedOperationKind::ConfigurationFileChange,
            "/etc/cups/cupsd.conf",
            Some(("LogLevel debug\n", false)),
        );
        assert_eq!(
            describe(&change),
            "Privileged operation configuration_file_change on file \"/etc/cups/cupsd.conf\" with content \"LogLevel debug\\u{a}\" as root through the privileged helper (operation type system_configuration)"
        );

        // A protected target is named with the type it is treated as.
        let protected = typed(TypedOperationKind::ServiceStop, "firewalld.service", None);
        assert!(describe(&protected).ends_with("(operation type security_policy)"));
    }

    #[test]
    fn a_secret_content_is_replaced_in_the_prompt() {
        let secret = "hunter2-correct-horse";
        let change = typed(
            TypedOperationKind::ConfigurationFileChange,
            "/etc/samba/credentials",
            Some((secret, true)),
        );
        let statement = describe(&change);
        assert!(!statement.contains(secret), "{statement}");
        assert_eq!(
            statement,
            "Privileged operation configuration_file_change on file \"/etc/samba/credentials\" with content <secret> as root through the privileged helper (operation type system_configuration)"
        );
        // Outside a repair session the request is offered for authentication;
        // the secret is not in that prompt either.
        let decision = decide(&change, &trusted(), &UserPolicy::default(), NOW);
        assert_eq!(decision.reason(), Reason::RepairSessionRequired);
        assert!(!decision.prompt().unwrap_or_default().contains(secret));
    }

    #[test]
    fn malformed_actions_are_refused() {
        let empty_argv = command(Sandbox::ReadOnly, &[]);
        let empty_program = command(Sandbox::ReadOnly, &["", "x"]);
        let no_application = Action::ScreenInput(UiAction {
            application: "  ".to_owned(),
            window_title: String::new(),
            control: "OK".to_owned(),
            kind: UiActionKind::Observe,
            detail: crate::action::UiDetail::default(),
        });
        let mut no_tool = tool(ActionClass::Observe);
        if let Action::Tool(call) = &mut no_tool {
            call.tool.clear();
        }
        let mut no_service = tool(ActionClass::Observe);
        if let Action::Tool(call) = &mut no_service {
            call.service.clear();
        }
        let bad_unit = typed(
            TypedOperationKind::ServiceRestart,
            "cups.service; reboot",
            None,
        );
        let bad_path = typed(
            TypedOperationKind::ConfigurationFileChange,
            "/etc/../boot/loader/loader.conf",
            Some(("x", false)),
        );
        let missing_content = typed(
            TypedOperationKind::ConfigurationFileChange,
            "/etc/cups/cupsd.conf",
            None,
        );
        let surplus_content = typed(
            TypedOperationKind::ServiceRestart,
            "cups.service",
            Some(("x", false)),
        );
        let context = Context {
            repair_session: open_session(),
            ..trusted()
        };
        for action in [
            empty_argv,
            empty_program,
            no_application,
            no_tool,
            no_service,
            bad_unit,
            bad_path,
            missing_content,
            surplus_content,
        ] {
            let decision = decide(&action, &context, &UserPolicy::default(), NOW);
            assert_eq!(
                decision.reason(),
                Reason::Refused(Refusal::MalformedAction),
                "{action:?}"
            );
        }
    }

    #[test]
    fn a_tool_call_claiming_class_3_or_4_is_never_allowed() {
        // A manifest cannot declare class 4, so a class 4 tool call cannot be
        // built outside this crate. The engine refuses it all the same, with
        // everything that could be granted granted.
        let arguments = json!({ "name": "value" })
            .as_object()
            .cloned()
            .unwrap_or_default();
        for class in [ActionClass::SystemCritical, ActionClass::Prohibited] {
            for presence in Presence::ALL {
                for session_locked in [false, true] {
                    for trusted_session_proven in [false, true] {
                        for model_flags_consequential in [false, true] {
                            for target_window_owner in WindowOwner::ALL {
                                for mode in Mode::ALL {
                                    let context = Context {
                                        presence,
                                        session_locked,
                                        trusted_session_proven,
                                        repair_session: open_session(),
                                        standing_approvals: HeldApprovals::read(
                                            &TestStore(vec![StandingApproval {
                                                service: "org.tur.Test1".to_owned(),
                                                tool: "test_tool".to_owned(),
                                                arguments: arguments.clone(),
                                            }]),
                                            "test",
                                        ),
                                        model_flags_consequential,
                                        target_window_owner,
                                    };
                                    let policy = UserPolicy {
                                        mode,
                                        ..UserPolicy::default()
                                    };
                                    let decision = decide(&tool(class), &context, &policy, NOW);
                                    assert!(!decision.is_allow(), "{context:?} {policy:?}");
                                    assert_eq!(decision.class(), class);
                                    if class == ActionClass::Prohibited {
                                        assert_eq!(
                                            decision.reason(),
                                            Reason::Refused(Refusal::Prohibited)
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn classification_comes_from_the_sandbox_not_the_text() {
        let context = Context::attended();
        let policy = UserPolicy::default();
        // The same destructive text gets the class of its sandbox.
        let text = ["rm", "-rf", "/"];
        assert_eq!(
            classify(&command(Sandbox::ReadOnly, &text), &context, &policy),
            ActionClass::Observe
        );
        assert_eq!(
            classify(&command(Sandbox::User, &text), &context, &policy),
            ActionClass::Consequential
        );
        // A harmless text in the privileged sandbox is still privileged.
        assert_eq!(
            classify(
                &command(
                    Sandbox::Privileged(PrivilegedOperation::Formatting),
                    &["true"]
                ),
                &context,
                &policy
            ),
            ActionClass::SystemCritical
        );
    }

    #[test]
    fn inside_a_repair_session_a_typed_operation_runs_and_a_command_is_asked() {
        let context = Context {
            repair_session: open_session(),
            ..trusted()
        };
        let policy = UserPolicy::default();

        let restart = typed(TypedOperationKind::ServiceRestart, "cups.service", None);
        let decision = decide(&restart, &context, &policy, NOW);
        assert!(decision.is_allow(), "{decision:?}");
        assert_eq!(decision.reason(), Reason::RepairSessionOpen);
        assert_eq!(decision.class(), ActionClass::Consequential);

        let free_text = command(
            Sandbox::Privileged(PrivilegedOperation::ServiceControl),
            &["systemctl", "restart", "cups"],
        );
        let decision = decide(&free_text, &context, &policy, NOW);
        assert_eq!(decision.reason(), Reason::PrivilegedCommandNeedsApproval);
        assert_eq!(
            decision.prompt(),
            Some(
                "Run command [\"systemctl\" \"restart\" \"cups\"] as root through the privileged helper (operation type service_control)"
            )
        );

        // The model's own doubt adds a confirmation to the typed operation.
        let hinted = Context {
            model_flags_consequential: true,
            ..context.clone()
        };
        let decision = decide(&restart, &hinted, &policy, NOW);
        assert_eq!(decision.reason(), Reason::ConsequentialNeedsConfirmation);
        assert!(decision.prompt().is_some());

        // A protected target is class 3: refused, session or not.
        let firewall = typed(TypedOperationKind::ServiceStop, "firewalld.service", None);
        for context in [context, trusted()] {
            let decision = decide(&firewall, &context, &policy, NOW);
            assert_eq!(decision.class(), ActionClass::SystemCritical);
            assert_eq!(
                decision.reason(),
                Reason::Refused(Refusal::SystemCriticalAsCommand)
            );
        }
    }

    #[test]
    fn the_repair_session_is_judged_at_the_time_given() {
        let restart = typed(TypedOperationKind::ServiceRestart, "cups.service", None);
        let context = Context {
            repair_session: open_session(),
            ..trusted()
        };
        let policy = UserPolicy::default();
        let at = |seconds: u64| decide(&restart, &context, &policy, Timestamp(seconds));

        assert_eq!(at(0).reason(), Reason::RepairSessionRequired);
        assert_eq!(at(NOW.0 - 1).reason(), Reason::RepairSessionRequired);
        assert!(at(NOW.0).is_allow());
        assert!(at(NOW.0 + 899).is_allow());
        assert_eq!(at(NOW.0 + 900).reason(), Reason::RepairSessionRequired);
        assert_eq!(at(u64::MAX).reason(), Reason::RepairSessionRequired);
    }

    #[test]
    fn a_window_of_an_administrator_process_is_out_of_reach() {
        let policy = UserPolicy::default();
        for kind in UiActionKind::ALL {
            let target = UiAction {
                application: "org.example.Partitions".to_owned(),
                window_title: "Devices".to_owned(),
                control: "Bold".to_owned(),
                kind,
                detail: crate::action::UiDetail::default(),
            };
            for action in [
                Action::Accessibility(target.clone()),
                Action::ScreenInput(target),
            ] {
                let administrator = Context {
                    target_window_owner: WindowOwner::Administrator,
                    ..trusted()
                };
                let decision = decide(&action, &administrator, &policy, NOW);
                assert_eq!(
                    decision.reason(),
                    Reason::Refused(Refusal::PrivilegedWindow),
                    "{action:?}"
                );

                // A window known to be the user's: the ordinary rules.
                let decision = decide(&action, &trusted(), &policy, NOW);
                assert!(decision.is_allow(), "{action:?}");

                // A window nobody vouches for: observing is still class 0,
                // acting is asked.
                let unknown = Context {
                    target_window_owner: WindowOwner::Unknown,
                    ..trusted()
                };
                let decision = decide(&action, &unknown, &policy, NOW);
                match kind {
                    UiActionKind::Observe => assert!(decision.is_allow(), "{action:?}"),
                    UiActionKind::Act => assert_eq!(
                        decision.reason(),
                        Reason::ConsequentialNeedsConfirmation,
                        "{action:?}"
                    ),
                }
            }
        }
    }

    fn bridge(operation: UiOperation, role: UiRole, control: &str, text: &str) -> UiAction {
        UiAction {
            application: "org.example.Editor".to_owned(),
            window_title: "Notes".to_owned(),
            control: control.to_owned(),
            kind: operation.kind().unwrap_or(UiActionKind::Act),
            detail: crate::action::UiDetail {
                operation,
                role,
                editable: role == UiRole::Text,
                text: text.to_owned(),
                ..crate::action::UiDetail::default()
            },
        }
    }

    #[test]
    fn a_bridge_action_that_does_not_hold_together_is_refused_as_incomplete() {
        let mut read_that_acts = bridge(UiOperation::Read, UiRole::Window, "", "");
        read_that_acts.kind = UiActionKind::Act;
        let mut press_that_observes = bridge(UiOperation::Press, UiRole::Button, "Bold", "");
        press_that_observes.kind = UiActionKind::Observe;
        let typing_nothing = bridge(UiOperation::Type, UiRole::Text, "", "");
        let pressing_with_text = bridge(UiOperation::Press, UiRole::Button, "Bold", "hello");
        for target in [
            read_that_acts,
            press_that_observes,
            typing_nothing,
            pressing_with_text,
        ] {
            let decision = decide(
                &Action::Accessibility(target.clone()),
                &trusted(),
                &UserPolicy::default(),
                NOW,
            );
            assert_eq!(
                decision.reason(),
                Reason::Refused(Refusal::MalformedAction),
                "{target:?}"
            );
        }
        // An unspecified operation goes with either kind, as before.
        for kind in UiActionKind::ALL {
            let target = UiAction::new("org.example.Editor", "Notes", "Bold", kind);
            let decision = decide(
                &Action::Accessibility(target),
                &trusted(),
                &UserPolicy::default(),
                NOW,
            );
            assert!(decision.is_allow(), "{kind:?}");
        }
    }

    #[test]
    fn every_identifier_of_the_application_is_matched_against_the_lists() {
        let policy = UserPolicy::default();
        // A terminal known to the bus by its program name, and to the
        // built-in list by its desktop file: sensitive.
        let mut terminal = bridge(UiOperation::Type, UiRole::Text, "", "hello");
        terminal.application = "konsole".to_owned();
        terminal
            .detail
            .also_known_as
            .push("org.kde.konsole".to_owned());
        let decision = decide(&Action::Accessibility(terminal), &trusted(), &policy, NOW);
        assert_eq!(decision.class(), ActionClass::Consequential);
        assert!(!decision.is_allow());

        // A password manager under another name: excluded, even from being
        // read.
        let mut vault = bridge(UiOperation::Read, UiRole::Window, "", "");
        vault.application = "python3".to_owned();
        vault
            .detail
            .also_known_as
            .push("org.keepassxc.KeePassXC".to_owned());
        let decision = decide(&Action::Accessibility(vault), &trusted(), &policy, NOW);
        assert_eq!(
            decision.reason(),
            Reason::Refused(Refusal::ExcludedApplication)
        );

        // A relaxation of the main identifier does not reach an application
        // that is sensitive under another.
        let relaxed = UserPolicy::from_json(
            r#"{"schema_version":1,"mode":"normal","relaxations":[{"application":"konsole","class":2}]}"#,
        )
        .expect("a valid policy");
        let mut terminal = bridge(UiOperation::Press, UiRole::Button, "Send", "");
        terminal.application = "konsole".to_owned();
        terminal
            .detail
            .also_known_as
            .push("org.kde.konsole".to_owned());
        let decision = decide(&Action::Accessibility(terminal), &trusted(), &relaxed, NOW);
        assert_eq!(decision.reason(), Reason::ConsequentialNeedsConfirmation);
    }

    #[test]
    fn typing_a_line_break_or_a_direction_change_is_consequential() {
        for text in [
            "hello\n", "hello\r", "a\tb", "x\u{2028}", "x\u{2029}", "x\u{202e}", "x\u{2066}",
            "\u{1b}[A",
        ] {
            let target = bridge(UiOperation::Type, UiRole::Text, "", text);
            assert_eq!(
                classify(
                    &Action::Accessibility(target),
                    &trusted(),
                    &UserPolicy::default()
                ),
                ActionClass::Consequential,
                "{text:?}"
            );
        }
        for text in ["hello", "Grüße, ça va? 你好", "a — b “c”"] {
            let target = bridge(UiOperation::Type, UiRole::Text, "", text);
            assert_eq!(
                classify(
                    &Action::Accessibility(target),
                    &trusted(),
                    &UserPolicy::default()
                ),
                ActionClass::Reversible,
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_prompt_of_a_bridge_action_says_what_it_does_and_hides_nothing() {
        let target = bridge(UiOperation::Type, UiRole::Text, "To", "a\u{202e}b");
        assert_eq!(
            describe(&Action::Accessibility(target)),
            "Type into control \"To\" (text) the text \"a\\u{202e}b\" in window \"Notes\" of application \"org.example.Editor\", through the accessibility tree"
        );
        let target = bridge(UiOperation::Press, UiRole::Button, "Send", "");
        assert_eq!(
            describe(&Action::Accessibility(target)),
            "Press control \"Send\" (button) in window \"Notes\" of application \"org.example.Editor\", through the accessibility tree"
        );
    }

    #[test]
    fn every_atspi_role_has_a_category_and_the_ones_that_matter_are_right() {
        for role in 0..=140 {
            // Every number maps to something; nothing panics.
            let _ = UiRole::from_atspi(role);
        }
        for (number, role) in [
            (43, UiRole::Button),
            (62, UiRole::ToggleButton),
            (7, UiRole::CheckBox),
            (35, UiRole::MenuItem),
            (88, UiRole::Link),
            (37, UiRole::PageTab),
            (32, UiRole::ListItem),
            (61, UiRole::Text),
            (79, UiRole::Text),
            (94, UiRole::Text),
            (40, UiRole::PasswordText),
            (60, UiRole::Terminal),
            (23, UiRole::Window),
            (16, UiRole::Window),
            (67, UiRole::Unknown),
            (0, UiRole::Unknown),
            (39, UiRole::Other),
        ] {
            assert_eq!(UiRole::from_atspi(number), role, "{number}");
        }
        for role in UiRole::ALL {
            let pressable = role.is_pressable();
            let expected = !matches!(
                role,
                UiRole::Unknown
                    | UiRole::Slider
                    | UiRole::Text
                    | UiRole::PasswordText
                    | UiRole::Terminal
                    | UiRole::ScrollArea
                    | UiRole::Label
                    | UiRole::Window
                    | UiRole::Other
            );
            assert_eq!(pressable, expected, "{role:?}");
        }
    }
}
