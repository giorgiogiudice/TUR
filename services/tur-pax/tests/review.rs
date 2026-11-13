// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Regression tests for the findings of the first independent review of the
//! policy engine (`docs/reviews/policy-engine-review-1.md`).
//!
//! Each test carries the number of the finding it demonstrates. Every one of
//! them failed against the engine as it was reviewed, except `f04`, which
//! could not be written before the context could name the owner of a window;
//! the review document records the run.

use std::process::Command;

use serde_json::{Map, Value, json};
use tur_pax::{
    Action, ActionClass, AuditMeta, CommandAction, Context, MemorySink, PrivilegedOperation,
    Reason, Refusal, Relaxation, RelaxationLevel, RepairSession, Sandbox, StandingApproval,
    Timestamp, ToolManifest, UiAction, UiActionKind, UserPolicy, Verdict, WindowOwner,
    WorkingContext, decide, decide_and_record,
};

const NOW: Timestamp = Timestamp(1_000_000);

const TEST_SERVICE_MANIFEST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/test-service.manifest.json"
);

/// Attended, unlocked, with the trusted path available, and a target window
/// that is known to belong to the user.
fn trusted() -> Context {
    Context {
        trusted_session_proven: true,
        target_window_owner: WindowOwner::User,
        ..Context::attended()
    }
}

fn open_repair_session() -> RepairSession {
    RepairSession {
        purpose: "fix printing".to_owned(),
        opened_at: Timestamp(NOW.0 - 300),
        expires_at: Timestamp(NOW.0 + 600),
        ended: false,
    }
}

/// An action on a control, through the screen.
fn act(application: &str, window_title: &str, control: &str) -> Action {
    Action::ScreenInput(UiAction {
        application: application.to_owned(),
        window_title: window_title.to_owned(),
        control: control.to_owned(),
        kind: UiActionKind::Act,
        detail: tur_pax::UiDetail::default(),
    })
}

fn observe(application: &str) -> Action {
    Action::ScreenInput(UiAction {
        application: application.to_owned(),
        window_title: "Window".to_owned(),
        control: String::new(),
        kind: UiActionKind::Observe,
        detail: tur_pax::UiDetail::default(),
    })
}

fn command(sandbox: Sandbox, working_context: WorkingContext, argv: &[&str]) -> Action {
    Action::Command(CommandAction {
        sandbox,
        argv: argv.iter().map(|argument| (*argument).to_owned()).collect(),
        working_context,
    })
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

/// A manifest with one tool that has the given input schema properties.
fn manifest_text(properties: &str) -> String {
    format!(
        r#"{{
            "schema_version": 1,
            "service": "org.tur.Test1",
            "tools": [{{
                "name": "test_tool",
                "description": "A tool for tests.",
                "class": 1,
                "method": "Test",
                "input_schema": {{
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {properties}
                }}
            }}]
        }}"#
    )
}

/// The journal fields of the record written for one decision.
fn recorded_fields(action: &Action, context: &Context) -> Vec<(&'static str, String)> {
    let mut sink = MemorySink::new();
    let meta = AuditMeta {
        task: "task-0001",
        provider: "none",
    };
    let _ = decide_and_record(
        action,
        context,
        &UserPolicy::default(),
        NOW,
        &meta,
        &mut sink,
    );
    sink.records()
        .first()
        .expect("one record is written for each decision")
        .journal_fields()
}

fn field<'a>(fields: &'a [(&'static str, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.as_str())
}

/// Assert that acting on each control, in an ordinary application and an
/// ordinary window, is asked first.
fn assert_asked(controls: &[&str]) {
    for control in controls {
        let action = act("org.example.Editor", "Dialog", control);
        let decision = decide(&action, &trusted(), &UserPolicy::default(), NOW);
        assert!(
            decision.class() >= ActionClass::Consequential,
            "{control:?} is treated as class {}",
            decision.class()
        );
        assert!(
            matches!(decision.verdict(), Verdict::Ask { .. }),
            "{control:?} => {decision:?}"
        );
    }
}

#[test]
fn f01_acting_in_a_terminal_or_a_run_dialog_is_asked_with_an_empty_user_policy() {
    for application in [
        "org.kde.konsole",
        "org.kde.yakuake",
        "org.kde.krunner",
        "org.gnome.Terminal",
        "org.gnome.Ptyxis",
        "xterm",
        "ORG.KDE.KONSOLE",
    ] {
        // Neither the title nor the control says anything: only the
        // application identifies the surface.
        let action = act(application, "user@host: ~", "Text");
        let decision = decide(&action, &trusted(), &UserPolicy::default(), NOW);
        assert!(
            decision.class() >= ActionClass::Consequential,
            "{application} is treated as class {}",
            decision.class()
        );
        assert!(
            matches!(decision.verdict(), Verdict::Ask { .. }),
            "{application} => {decision:?}"
        );
    }
}

#[test]
fn f02_a_free_text_privileged_command_is_asked_inside_a_repair_session() {
    let context = Context {
        repair_session: Some(open_repair_session()),
        ..trusted()
    };
    let action = command(
        Sandbox::Privileged(PrivilegedOperation::ServiceControl),
        WorkingContext::Session,
        &["systemctl", "restart", "cups"],
    );
    let decision = decide(&action, &context, &UserPolicy::default(), NOW);
    assert!(
        matches!(decision.verdict(), Verdict::Ask { .. }),
        "{decision:?}"
    );
    assert!(
        decision
            .prompt()
            .is_some_and(|prompt| prompt.contains("\"systemctl\" \"restart\" \"cups\"")),
        "{decision:?}"
    );
}

#[test]
fn f03_a_repair_session_of_more_than_an_hour_is_closed() {
    let session = |seconds: u64| RepairSession {
        purpose: "fix printing".to_owned(),
        opened_at: NOW,
        expires_at: Timestamp(NOW.0 + seconds),
        ended: false,
    };
    assert!(session(3_600).is_open_at(NOW));
    assert!(!session(3_601).is_open_at(NOW));
    assert!(!session(3_601).is_open_at(Timestamp(NOW.0 + 1)));
    // The bound the engine had before: 24 hours.
    assert!(!session(86_400).is_open_at(NOW));
}

#[test]
fn f04_a_window_of_a_process_with_administrator_rights_is_refused() {
    // An ordinary control in an ordinary application: class 1 by every other
    // rule. What it does in a root window is not bounded by anything.
    let administrator = Context {
        target_window_owner: WindowOwner::Administrator,
        ..trusted()
    };
    for action in [
        act("org.example.Partitions", "Devices", "Next"),
        observe("org.example.Partitions"),
    ] {
        let decision = decide(&action, &administrator, &UserPolicy::default(), NOW);
        assert_eq!(
            decision.reason(),
            Reason::Refused(Refusal::PrivilegedWindow),
            "{action:?}"
        );
        assert!(decide(&action, &trusted(), &UserPolicy::default(), NOW).is_allow());
    }
}

#[test]
fn f05_a_control_named_only_by_a_symbol_is_asked() {
    assert_asked(&[
        "\u{1f5d1}",
        "\u{2192}",
        "\u{2713}",
        "\u{2026}",
        "\u{2022}\u{2022}\u{2022}",
        "\u{2699}",
        "?",
        "+",
    ]);
}

#[test]
fn f06_a_control_name_with_look_alike_or_hidden_characters_is_asked() {
    assert_asked(&[
        // "Send" with a Cyrillic first letter.
        "\u{405}end",
        // "Send" with a zero-width space inside.
        "S\u{200b}end",
        // "Send" in full-width letters.
        "\u{ff33}\u{ff45}\u{ff4e}\u{ff44}",
        // "Delete" with a soft hyphen inside.
        "De\u{ad}lete",
        // "Pay now" with a Cyrillic "a".
        "P\u{430}y now",
        // "Send" with a combining accent.
        "Se\u{301}nd",
        // "Delete" in German, Russian and Chinese.
        "L\u{f6}schen",
        "\u{423}\u{434}\u{430}\u{43b}\u{438}\u{442}\u{44c}",
        "\u{5220}\u{9664}",
        // A right-to-left override.
        "\u{202e}dneS",
    ]);
}

#[test]
fn f07_a_control_that_only_affirms_is_asked() {
    assert_asked(&[
        "OK", "Ok", "Okay", "Yes", "Continue", "Proceed", "Apply", "Allow", "Done", "Finish",
    ]);
}

#[test]
fn f08_destructive_and_granting_words_are_asked() {
    assert_asked(&[
        "Format",
        "Format disk\u{2026}",
        "Overwrite",
        "Reset to defaults",
        "Restore",
        "Encrypt",
        "Approve",
        "Authorize",
        "Authorise",
        "Grant access",
        "Manage subscription",
        "Unsubscribe",
        "Run",
        "Execute",
    ]);
}

#[test]
fn f09_a_locked_unattended_task_runs_no_command_outside_the_read_only_sandbox() {
    let context = Context {
        session_locked: true,
        trusted_session_proven: true,
        ..Context::unattended()
    };
    let policy = UserPolicy::default();

    let workspace = command(Sandbox::User, WorkingContext::UndoableWorkspace, &["make"]);
    let decision = decide(&workspace, &context, &policy, NOW);
    assert!(!decision.is_allow(), "{decision:?}");

    // What section 5.6 does permit while locked still runs.
    let read_only = command(Sandbox::ReadOnly, WorkingContext::Session, &["journalctl"]);
    assert!(decide(&read_only, &context, &policy, NOW).is_allow());
}

#[test]
fn f10_the_record_says_whether_the_task_was_attended_and_which_sandbox_was_used() {
    let user = command(Sandbox::User, WorkingContext::Session, &["touch", "x"]);
    let fields = recorded_fields(&user, &trusted());
    assert_eq!(field(&fields, "TUR_AI_ATTENDED"), Some("yes"));
    assert_eq!(field(&fields, "TUR_AI_SANDBOX"), Some("user"));

    let unattended = Context {
        trusted_session_proven: true,
        ..Context::unattended()
    };
    let workspace = command(Sandbox::User, WorkingContext::UndoableWorkspace, &["make"]);
    let fields = recorded_fields(&workspace, &unattended);
    assert_eq!(field(&fields, "TUR_AI_ATTENDED"), Some("no"));
    assert_eq!(field(&fields, "TUR_AI_SANDBOX"), Some("workspace"));

    let read_only = command(Sandbox::ReadOnly, WorkingContext::Session, &["journalctl"]);
    let fields = recorded_fields(&read_only, &trusted());
    assert_eq!(field(&fields, "TUR_AI_SANDBOX"), Some("read-only"));

    // An action that is not a command has no sandbox.
    let fields = recorded_fields(&observe("org.example.Editor"), &trusted());
    assert_eq!(field(&fields, "TUR_AI_ATTENDED"), Some("yes"));
    assert_eq!(field(&fields, "TUR_AI_SANDBOX"), None);
}

#[test]
fn f11_a_privileged_command_is_recorded_in_full() {
    let action = command(
        Sandbox::Privileged(PrivilegedOperation::ServiceControl),
        WorkingContext::Session,
        &["/usr/bin/systemctl", "restart", "cups.service"],
    );
    let fields = recorded_fields(&action, &trusted());
    let message = field(&fields, "MESSAGE").unwrap_or_default();
    for part in ["/usr/bin/systemctl", "restart", "cups.service"] {
        assert!(message.contains(part), "{part} is missing from {message}");
    }
    assert_eq!(field(&fields, "TUR_AI_SANDBOX"), Some("privileged"));
}

#[test]
fn f12_the_record_of_a_command_names_the_program_by_its_path() {
    let genuine = command(
        Sandbox::User,
        WorkingContext::Session,
        &["/usr/bin/systemctl", "--user", "restart", "x"],
    );
    let planted = command(
        Sandbox::User,
        WorkingContext::Session,
        &["/tmp/x/systemctl", "--user", "restart", "x"],
    );
    let genuine = recorded_fields(&genuine, &trusted());
    let planted = recorded_fields(&planted, &trusted());
    assert_ne!(field(&genuine, "MESSAGE"), field(&planted, "MESSAGE"));
    assert!(
        field(&planted, "MESSAGE")
            .unwrap_or_default()
            .contains("/tmp/x/systemctl")
    );
}

#[test]
fn f13_a_method_cannot_be_declared_with_two_classes() {
    let tool = |name: &str, class: u8| {
        json!({
            "name": name,
            "description": "A tool for tests.",
            "class": class,
            "method": "Format",
            "input_schema": { "type": "object", "additionalProperties": false }
        })
    };
    let conflicting = json!({
        "schema_version": 1,
        "service": "org.tur.Test1",
        "tools": [tool("storage_format", 3), tool("storage_tidy", 1)]
    });
    assert!(ToolManifest::from_json(&conflicting.to_string()).is_err());

    // Two names for one method are accepted when the class is the same.
    let consistent = json!({
        "schema_version": 1,
        "service": "org.tur.Test1",
        "tools": [tool("storage_format", 3), tool("storage_wipe", 3)]
    });
    assert!(ToolManifest::from_json(&consistent.to_string()).is_ok());
}

#[test]
fn f14_a_repeated_key_inside_an_input_schema_is_rejected() {
    let once = manifest_text(r#"{ "count": { "type": "integer", "maximum": 10 } }"#);
    assert!(ToolManifest::from_json(&once).is_ok());

    let twice =
        manifest_text(r#"{ "count": { "type": "integer", "maximum": 10, "maximum": 1000000 } }"#);
    assert!(ToolManifest::from_json(&twice).is_err());

    let nested = manifest_text(
        r#"{ "options": { "type": "object", "additionalProperties": true, "additionalProperties": false } }"#,
    );
    assert!(ToolManifest::from_json(&nested).is_err());
}

#[test]
fn f15_an_integer_bound_is_exact_above_two_to_the_53() {
    let manifest = ToolManifest::from_json(&manifest_text(
        r#"{ "amount": { "type": "integer", "minimum": -9007199254740992, "maximum": 9007199254740992 } }"#,
    ))
    .expect("the manifest is valid");
    let call = |text: &str| {
        let arguments: Value = serde_json::from_str(text).expect("the arguments are JSON");
        manifest.tool_call("test_tool", object(arguments))
    };
    assert!(call(r#"{ "amount": 9007199254740992 }"#).is_ok());
    assert!(call(r#"{ "amount": -9007199254740992 }"#).is_ok());
    // One more than the maximum, and one less than the minimum. As 64-bit
    // floating-point values these are equal to the bounds.
    assert!(call(r#"{ "amount": 9007199254740993 }"#).is_err());
    assert!(call(r#"{ "amount": -9007199254740993 }"#).is_err());
}

#[test]
fn f16_a_policy_file_cannot_name_an_application_that_could_never_match() {
    for application in [
        // A zero-width space after the identifier.
        "org.keepassxc.KeePassXC\u{200b}",
        // A Cyrillic "a".
        "org.keep\u{430}ssxc.KeePassXC",
        // A byte-order mark in front.
        "\u{feff}org.keepassxc.KeePassXC",
    ] {
        let text = json!({
            "schema_version": 1,
            "mode": "normal",
            "excluded_applications": [application]
        });
        assert!(
            UserPolicy::from_json(&text.to_string()).is_err(),
            "{application:?} is accepted"
        );
    }
}

#[test]
fn f17_an_exclusion_also_covers_the_desktop_file_form_of_the_identifier() {
    let policy = UserPolicy {
        excluded_applications: vec!["org.example.Vault".to_owned()],
        sensitive_applications: vec!["org.example.Bank".to_owned()],
        ..UserPolicy::default()
    };
    assert!(policy.is_excluded("org.example.Vault.desktop"));
    assert!(policy.is_sensitive("org.example.Bank.desktop"));
    assert!(!policy.is_excluded("org.example.Vault.desktop.other"));

    let decision = decide(
        &observe("org.example.Vault.desktop"),
        &trusted(),
        &policy,
        NOW,
    );
    assert_eq!(decision.verdict(), &Verdict::Refuse, "{decision:?}");
}

#[test]
fn f18_password_managers_are_excluded_with_an_empty_user_policy() {
    let policy = UserPolicy::default();
    for application in [
        "org.keepassxc.KeePassXC",
        "org.kde.kwalletmanager5",
        "org.gnome.seahorse.Application",
        "com.bitwarden.desktop",
    ] {
        assert!(policy.is_excluded(application), "{application}");
        let decision = decide(&observe(application), &trusted(), &policy, NOW);
        assert_eq!(decision.verdict(), &Verdict::Refuse, "{application}");
    }
}

#[test]
fn f19_the_model_hint_adds_a_confirmation_in_a_relaxed_application() {
    let policy = UserPolicy {
        relaxations: vec![Relaxation {
            application: "org.example.Mail".to_owned(),
            level: RelaxationLevel::Consequential,
        }],
        ..UserPolicy::default()
    };
    let action = act("org.example.Mail", "Inbox", "Send");

    // The relaxation applies: the user is not asked.
    let plain = decide(&action, &trusted(), &policy, NOW);
    assert!(plain.is_allow(), "{plain:?}");

    // The model itself calls the action consequential: the user is asked.
    let hinted = Context {
        model_flags_consequential: true,
        ..trusted()
    };
    let decision = decide(&action, &hinted, &policy, NOW);
    assert!(
        matches!(decision.verdict(), Verdict::Ask { .. }),
        "{decision:?}"
    );
}

#[test]
fn f20_a_class_1_relaxation_is_not_part_of_the_file_format() {
    let text = json!({
        "schema_version": 1,
        "mode": "normal",
        "relaxations": [{ "application": "org.example.Notes", "class": 1 }]
    });
    assert!(UserPolicy::from_json(&text.to_string()).is_err());
}

#[test]
fn f21_a_repeated_key_inside_a_standing_approval_is_rejected() {
    let once =
        r#"{"service":"org.tur.Test1","tool":"mail_send","arguments":{"to":"a@example.org"}}"#;
    assert!(serde_json::from_str::<StandingApproval>(once).is_ok());

    let twice = r#"{"service":"org.tur.Test1","tool":"mail_send","arguments":{"to":"a@example.org","to":"b@example.org"}}"#;
    assert!(serde_json::from_str::<StandingApproval>(twice).is_err());

    let nested = r#"{"service":"org.tur.Test1","tool":"mail_send","arguments":{"options":{"cc":"a@example.org","cc":"b@example.org"}}}"#;
    assert!(serde_json::from_str::<StandingApproval>(nested).is_err());
}

#[test]
fn f22_two_relaxations_that_differ_only_in_letter_case_are_rejected() {
    let text = json!({
        "schema_version": 1,
        "mode": "normal",
        "relaxations": [
            { "application": "org.example.Mail", "class": 2 },
            { "application": "org.example.mail", "class": 2 }
        ]
    });
    assert!(UserPolicy::from_json(&text.to_string()).is_err());
}

#[test]
fn f23_repeated_keys_in_the_arguments_of_a_call_are_an_input_error() {
    let run = |arguments: &str| {
        Command::new(env!("CARGO_BIN_EXE_tur-pax"))
            .args([
                "policy",
                "check",
                "--manifest",
                TEST_SERVICE_MANIFEST,
                "--tool",
                "test_reversible",
                "--args",
                arguments,
            ])
            .output()
            .expect("tur-pax starts")
            .status
            .code()
    };
    assert_eq!(run(r#"{"value":"a"}"#), Some(0));
    // Exit status 1: an input was invalid and nothing was decided.
    assert_eq!(run(r#"{"value":"a","value":"b"}"#), Some(1));
}
