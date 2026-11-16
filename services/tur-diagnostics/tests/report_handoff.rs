// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! P@x never sends a problem report (ADR-0024; problem-reports.md,
//! section 2.3). It prepares one and shows it (class 0); the person sends
//! it. Every way P@x could reach a send is held here against P@x's own
//! policy engine (`tur-pax`, a development dependency of these tests only):
//!
//! * the tool `diag_report_send` is declared with class 2 and has no
//!   adapter, so it is never offered, and the engine never allows it, in
//!   any mode, presence or lock, with or without a proven trusted session;
//! * pressing a "Send" control or typing into a terminal through the screen
//!   or the accessibility tree is class 2 and refused without the trusted
//!   path;
//! * a read-only command that names `tur-diag report send` is class 0 by
//!   its sandbox, which has no network and a host name of its own, which
//!   `tur-diag` refuses to send from (TUR-DIAG-REPORT-013).

use serde_json::{Map, json};
use tur_diag::report::PAX_SANDBOX_HOSTNAME;
use tur_pax::registry::ToolRegistry;
use tur_pax::sandbox::{GrantRules, Limits, SandboxPlan};
use tur_pax::{
    Action, ActionClass, CommandAction, Context, Mode, Presence, Sandbox, Timestamp,
    ToolManifest, UiAction, UiActionKind, UiDetail, UiOperation, UserPolicy, Verdict,
    WorkingContext, decide,
};

const MANIFEST: &str = include_str!("../agent-tools.json");
const ADAPTERS: &str = include_str!("../agent-tools.adapter.json");

/// The tool that would send.
const SEND: &str = "diag_report_send";

const NOW: Timestamp = Timestamp(1_793_000_000);

fn manifest() -> ToolManifest {
    ToolManifest::from_json(MANIFEST).unwrap_or_else(|_| panic!("the manifest loads"))
}

/// A call of the send tool, built from the manifest as the registry would.
fn send_call() -> Action {
    let arguments = json!({ "title": "No sound after resume", "component": "audio" })
        .as_object()
        .cloned()
        .expect("an object");
    let call = manifest()
        .tool_call(SEND, arguments)
        .unwrap_or_else(|_| panic!("the arguments fit the schema"));
    Action::Tool(call)
}

/// Every context the engine distinguishes for a tool call, today's (no
/// trusted session) and tomorrow's (a proven one).
fn contexts() -> Vec<(String, Context, UserPolicy)> {
    let mut all = Vec::new();
    for mode in Mode::ALL {
        for presence in [Presence::Attended, Presence::Unattended] {
            for locked in [false, true] {
                for flagged in [false, true] {
                    for trusted in [false, true] {
                        let mut context = Context::attended();
                        context.presence = presence;
                        context.session_locked = locked;
                        context.model_flags_consequential = flagged;
                        context.trusted_session_proven = trusted;
                        let policy = UserPolicy {
                            mode,
                            ..UserPolicy::default()
                        };
                        let name = format!(
                            "{mode:?} {presence:?} locked={locked} flagged={flagged} \
                             trusted={trusted}"
                        );
                        all.push((name, context, policy));
                    }
                }
            }
        }
    }
    all
}

#[test]
fn sending_is_declared_class_2_and_never_offered() {
    let declared = manifest()
        .tools()
        .iter()
        .find(|tool| tool.name() == SEND)
        .map(|tool| tool.class())
        .expect("the manifest declares the send tool");
    assert_eq!(declared, ActionClass::Consequential);

    let registry = ToolRegistry::load(&[MANIFEST.to_owned()], &[ADAPTERS.to_owned()], false)
        .unwrap_or_else(|_| panic!("the tools load"));
    assert!(registry.unavailable().iter().any(|name| name == SEND));
    assert!(!registry.offered().iter().any(|tool| tool.name == SEND));
    let definitions = registry.definitions().expect("definitions");
    assert!(definitions.iter().all(|definition| definition.name() != SEND));
    // A model that names it anyway gets no action at all.
    assert!(registry.propose(SEND, Map::new()).is_err());
    // And no adapter may ever be written for it.
    let adapters: serde_json::Value = serde_json::from_str(ADAPTERS).expect("JSON");
    assert!(!adapters.to_string().contains(SEND));
}

#[test]
fn the_policy_never_lets_pax_send_a_report() {
    let action = send_call();
    for (name, context, policy) in contexts() {
        let decision = decide(&action, &context, &policy, NOW);
        assert!(
            decision.class() >= ActionClass::Consequential,
            "{name}: {decision:?}"
        );
        assert!(
            !matches!(decision.verdict(), Verdict::Allow),
            "{name}: {decision:?}"
        );
        if !context.trusted_session_proven {
            // Today: refused outright, nobody is even asked.
            assert!(
                matches!(decision.verdict(), Verdict::Refuse),
                "{name}: {decision:?}"
            );
        }
    }
}

/// An action on a control through the accessibility tree.
fn ui(application: &str, window: &str, control: &str, operation: UiOperation) -> UiAction {
    UiAction {
        application: application.to_owned(),
        window_title: window.to_owned(),
        control: control.to_owned(),
        kind: UiActionKind::Act,
        detail: UiDetail {
            operation,
            ..UiDetail::default()
        },
    }
}

#[test]
fn pax_cannot_press_send_or_type_the_persons_command() {
    let mut typed = ui("org.kde.konsole", "Konsole", "Terminal", UiOperation::Type);
    typed.detail.text = "tur-diag report submit --title=x --component=audio".to_owned();
    typed.detail.editable = true;
    let surfaces = [
        ui(
            "org.tur.Settings",
            "Problem reports",
            "Send report",
            UiOperation::Press,
        ),
        ui(
            "org.kde.plasmashell",
            "Notification",
            "Send",
            UiOperation::Press,
        ),
        typed,
    ];
    let policy = UserPolicy::default();
    for surface in surfaces {
        // A screen action carries no detail: it is decided from the
        // application, the window and the control alone.
        let on_screen = UiAction {
            detail: UiDetail::default(),
            ..surface.clone()
        };
        for action in [
            Action::Accessibility(surface.clone()),
            Action::ScreenInput(on_screen),
        ] {
            let decision = decide(&action, &Context::attended(), &policy, NOW);
            assert!(
                decision.class() >= ActionClass::Consequential,
                "{surface:?}: {decision:?}"
            );
            assert!(
                matches!(decision.verdict(), Verdict::Refuse),
                "{surface:?}: {decision:?}"
            );
        }
    }
}

#[test]
fn a_command_that_names_send_runs_where_nothing_can_be_sent() {
    let argv: Vec<String> = [
        "/usr/bin/tur-diag",
        "report",
        "send",
        "--to",
        "https://reports.tur.example",
        "--yes",
        "/tmp/r.tar.zst",
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .collect();
    // The engine classes a command by its sandbox, never by its text: in
    // the read-only sandbox this is class 0 and allowed.
    let action = Action::Command(CommandAction {
        sandbox: Sandbox::ReadOnly,
        argv: argv.clone(),
        working_context: WorkingContext::Session,
    });
    let decision = decide(&action, &Context::attended(), &UserPolicy::default(), NOW);
    assert_eq!(decision.class(), ActionClass::Observe);

    // So the sandbox is what stops it: no network, and the host name
    // tur-diag refuses to send from.
    let plan = SandboxPlan::read_only(&GrantRules::new(None), &[], &argv, Limits::default())
        .expect("a plan");
    let line = plan.command_line();
    assert!(line.iter().any(|argument| argument == "--unshare-net"));
    assert!(line.iter().any(|argument| argument == "--unshare-uts"));
    assert!(
        line.windows(2)
            .any(|pair| pair[0] == "--hostname" && pair[1] == PAX_SANDBOX_HOSTNAME),
        "{line:?}"
    );
}
