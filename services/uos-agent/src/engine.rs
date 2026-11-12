// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The policy engine.
//!
//! [`decide`] is a pure function. It performs no I/O and reads no clock: the
//! time is a parameter. It takes no input from the assistant's memory, and the
//! only input that originates from the model, besides the proposal itself, is
//! the "consequential" hint, which can only raise the class.
//!
//! The rules are evaluated in a fixed order and the first that applies decides.
//! `docs/architecture/agent-policy.md` holds the same table in prose.

use serde_json::Value;

use crate::action::{Action, CommandAction, Sandbox, UiAction, UiActionKind, WorkingContext};
use crate::class::ActionClass;
use crate::context::{Context, Presence, Timestamp};
use crate::decision::{Decision, Reason, Refusal};
use crate::patterns::match_consequential;
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

    // 3. Accessibility and screen actions: privacy exclusions and the lock.
    let ui = match action {
        Action::Accessibility(ui) | Action::ScreenInput(ui) => Some(ui),
        Action::Tool(_) | Action::Command(_) | Action::Agent(_) => None,
    };
    if let Some(ui) = ui {
        if policy.is_excluded(&ui.application) {
            return Decision::refuse(class, Refusal::ExcludedApplication);
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
    if !context.trusted_path_available && class >= ActionClass::Consequential {
        return Decision::refuse(class, Refusal::TrustedPathUnavailable);
    }

    // A locked session has nobody at the screen, whatever the task says.
    let present = context.presence == Presence::Attended && !context.session_locked;

    // 6. Class 3: its own authentication, every time. Never as a command,
    //    never without a person present, never covered by a repair session.
    if class == ActionClass::SystemCritical && present {
        if matches!(action, Action::Command(_)) {
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

    // 7. A privileged command: only with a person present, only inside an open
    //    repair session. A lock closes the repair session.
    if let Action::Command(command) = action
        && matches!(command.sandbox, Sandbox::Privileged(_))
    {
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
        return Decision::allow(class, Reason::RepairSessionOpen);
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
        if class <= ActionClass::Reversible {
            return Decision::allow(class, permitted(class));
        }
        if let Action::Tool(call) = action
            && context
                .standing_approvals
                .iter()
                .any(|approval| approval.matches(call))
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
    if let Some(ui) = ui
        && !policy.is_sensitive(&ui.application)
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
/// the UI rule set or the built-in table), raised to class 2 if the model
/// flagged the action as consequential. Nothing lowers it.
pub fn classify(action: &Action, context: &Context, policy: &UserPolicy) -> ActionClass {
    let base = match action {
        Action::Tool(call) => call.declared_class,
        Action::Accessibility(ui) | Action::ScreenInput(ui) => ui_class(ui, policy),
        Action::Command(command) => command_class(command),
        Action::Agent(operation) => operation.class(),
    };
    if context.model_flags_consequential {
        base.max(ActionClass::Consequential)
    } else {
        base
    }
}

/// The class of an accessibility or screen action (addendum section 5.1).
///
/// Never above class 2: privileged prompts reject emulated input.
fn ui_class(ui: &UiAction, policy: &UserPolicy) -> ActionClass {
    if policy.is_sensitive(&ui.application) {
        return ActionClass::Consequential;
    }
    match ui.kind {
        UiActionKind::Observe => ActionClass::Observe,
        UiActionKind::Act => {
            let unidentified = ui.control.trim().is_empty();
            let matches_pattern = match_consequential(&ui.control).is_some()
                || match_consequential(&ui.window_title).is_some();
            if unidentified || matches_pattern {
                ActionClass::Consequential
            } else {
                ActionClass::Reversible
            }
        }
    }
}

/// The class of a command. It comes from the sandbox, never from the text.
fn command_class(command: &CommandAction) -> ActionClass {
    match command.sandbox {
        Sandbox::ReadOnly => ActionClass::Observe,
        Sandbox::User => match command.working_context {
            WorkingContext::UndoableWorkspace => ActionClass::Reversible,
            WorkingContext::Session => ActionClass::Consequential,
        },
        Sandbox::Privileged(operation) => {
            if operation.repair_session_may_cover() {
                ActionClass::Consequential
            } else {
                ActionClass::SystemCritical
            }
        }
    }
}

fn is_well_formed(action: &Action) -> bool {
    match action {
        Action::Tool(call) => !call.service.is_empty() && !call.tool.is_empty(),
        Action::Accessibility(ui) | Action::ScreenInput(ui) => !ui.application.trim().is_empty(),
        Action::Command(command) => command
            .argv
            .first()
            .is_some_and(|program| !program.is_empty()),
        Action::Agent(_) => true,
    }
}

fn permitted(class: ActionClass) -> Reason {
    if class == ActionClass::Observe {
        Reason::ObservePermitted
    } else {
        Reason::ReversiblePermitted
    }
}

/// A statement of the exact action, for a confirmation prompt.
///
/// Every piece of text that did not come from this crate is escaped: anything
/// other than printable ASCII appears as a `\u{...}` escape, so the text
/// cannot hide characters, change direction or break out of its line. A
/// command is shown argument by argument and is never shortened.
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
        Action::Agent(operation) => format!("Agent operation {}", operation.as_str()),
    }
}

fn describe_ui(ui: &UiAction, tier: &str) -> String {
    let verb = match ui.kind {
        UiActionKind::Observe => "Read",
        UiActionKind::Act => "Act on",
    };
    format!(
        "{verb} control {} in window {} of application {}, {tier}",
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

#[cfg(any())]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::action::{PrivilegedOperation, ToolCall};
    use crate::context::{RepairSession, StandingApproval};

    fn tool(class: ActionClass) -> Action {
        Action::Tool(ToolCall {
            service: "org.uos.Test1".to_owned(),
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

    fn trusted() -> Context {
        Context {
            trusted_path_available: true,
            ..Context::attended()
        }
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
        assert_eq!(
            printable("{\"k\":\"é\u{7f}\"}"),
            "{\"k\":\"\\u{e9}\\u{7f}\"}"
        );
    }

    #[test]
    fn a_command_prompt_shows_every_argument_exactly() {
        let action = command(Sandbox::User, &["rm", "-rf", "my files", "", "x\ny"]);
        let decision = decide(&action, &trusted(), &UserPolicy::default(), Timestamp(0));
        assert_eq!(
            decision.prompt(),
            Some(
                "Run command [\"rm\" \"-rf\" \"my files\" \"\" \"x\\u{a}y\"] with your privileges, on your files"
            )
        );
    }

    #[test]
    fn a_tool_prompt_shows_the_exact_arguments() {
        let decision = decide(
            &tool(ActionClass::Consequential),
            &trusted(),
            &UserPolicy::default(),
            Timestamp(0),
        );
        assert_eq!(
            decision.prompt(),
            Some(
                "Call tool \"test_tool\" of service \"org.uos.Test1\" with arguments {\"name\":\"value\"}"
            )
        );
    }

    #[test]
    fn the_repair_session_prompt_names_the_operation_type() {
        let action = command(
            Sandbox::Privileged(PrivilegedOperation::ServiceControl),
            &["systemctl", "restart", "cups"],
        );
        let decision = decide(&action, &trusted(), &UserPolicy::default(), Timestamp(0));
        assert_eq!(decision.reason(), Reason::RepairSessionRequired);
        assert_eq!(
            decision.prompt(),
            Some(
                "Open a repair session to run privileged commands. Requested: Run command [\"systemctl\" \"restart\" \"cups\"] as root through the privileged helper (operation type service_control)"
            )
        );
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
        });
        let mut no_tool = tool(ActionClass::Observe);
        if let Action::Tool(call) = &mut no_tool {
            call.tool.clear();
        }
        for action in [empty_argv, empty_program, no_application, no_tool] {
            let decision = decide(&action, &trusted(), &UserPolicy::default(), Timestamp(0));
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
                    for trusted_path_available in [false, true] {
                        for model_flags_consequential in [false, true] {
                            for mode in Mode::ALL {
                                let context = Context {
                                    presence,
                                    session_locked,
                                    trusted_path_available,
                                    repair_session: Some(RepairSession {
                                        purpose: "repair".to_owned(),
                                        expires_at: Timestamp(100),
                                    }),
                                    standing_approvals: vec![StandingApproval {
                                        service: "org.uos.Test1".to_owned(),
                                        tool: "test_tool".to_owned(),
                                        arguments: arguments.clone(),
                                    }],
                                    model_flags_consequential,
                                };
                                let policy = UserPolicy {
                                    mode,
                                    ..UserPolicy::default()
                                };
                                let decision =
                                    decide(&tool(class), &context, &policy, Timestamp(0));
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
}
