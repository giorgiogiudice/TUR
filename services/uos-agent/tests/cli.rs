// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The A0 exit criterion, run against the built binary: a scripted caller
//! exercises every class against a test service, and class 3 fails without
//! authentication.
//!
//! The test service is the manifest in `tests/data/`. No service runs: the
//! binary only decides. That a class 3 call also fails inside the service,
//! through polkit, is a test of each service (milestone A1).

#![cfg(any())]

use std::process::Command;

use serde_json::Value;

const MANIFEST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/test-service.manifest.json"
);
const CLASS_4_MANIFEST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/invalid-class-4.manifest.json"
);
const OBSERVE_ONLY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/policy-observe-only.json"
);
const ASK_EVERY_ACTION: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/policy-ask-every-action.json"
);
const INVALID_RELAXATION: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/policy-invalid-relaxation.json"
);

const EXIT_ALLOW: i32 = 0;
const EXIT_INPUT_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_ASK: i32 = 10;
const EXIT_AUTHENTICATE: i32 = 11;
const EXIT_DEFER: i32 = 12;
const EXIT_REFUSE: i32 = 13;

const OBSERVE: &str = "test_observe";
const REVERSIBLE: &str = "test_reversible";
const CONSEQUENTIAL: &str = "test_consequential";
const SYSTEM_CRITICAL: &str = "test_system_critical";

const CONSEQUENTIAL_ARGS: &str = r#"{"recipient":"a@example.org","subject":"Weekly report"}"#;
const EXACT_APPROVAL: &str = r#"{"service":"org.uos.Test1","tool":"test_consequential","arguments":{"recipient":"a@example.org","subject":"Weekly report"}}"#;

struct Run {
    status: i32,
    stdout: String,
    stderr: String,
}

fn uos_agent(args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_uos-agent"))
        .args(args)
        .output()
        .expect("uos-agent starts");
    Run {
        status: output.status.code().expect("uos-agent exits with a status"),
        stdout: String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    }
}

/// Valid arguments for each tool of the test service.
fn arguments_for(tool: &str) -> &'static str {
    match tool {
        REVERSIBLE => r#"{"value":"x"}"#,
        CONSEQUENTIAL => CONSEQUENTIAL_ARGS,
        SYSTEM_CRITICAL => r#"{"volume_id":"vol1"}"#,
        _ => "{}",
    }
}

/// Run `policy check --json` for a tool and return the exit status and the
/// decision document.
fn check(tool: &str, extra: &[&str]) -> (i32, Value) {
    let mut args = vec![
        "policy",
        "check",
        "--manifest",
        MANIFEST,
        "--tool",
        tool,
        "--args",
        arguments_for(tool),
        "--now",
        "1000",
        "--json",
    ];
    args.extend_from_slice(extra);
    let run = uos_agent(&args);
    let document: Value = serde_json::from_str(&run.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}); status {}, stdout {:?}, stderr {:?}",
            run.status, run.stdout, run.stderr
        )
    });
    assert!(run.stderr.is_empty(), "unexpected stderr: {}", run.stderr);
    (run.status, document)
}

fn text<'a>(document: &'a Value, field: &str) -> &'a str {
    document[field].as_str().unwrap_or("")
}

#[test]
fn manifest_validate_accepts_the_test_service() {
    let run = uos_agent(&["manifest", "validate", MANIFEST]);
    assert_eq!(run.status, EXIT_ALLOW, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "valid: org.uos.Test1 declares 4 tools\n  class 0  test_observe\n  class 1  test_reversible\n  class 2  test_consequential\n  class 3  test_system_critical\n"
    );
    assert!(run.stderr.is_empty());
}

#[test]
fn manifest_validate_rejects_a_class_4_tool() {
    let run = uos_agent(&["manifest", "validate", CLASS_4_MANIFEST]);
    assert_eq!(run.status, EXIT_INPUT_ERROR);
    assert!(run.stdout.is_empty());
    assert!(
        run.stderr.starts_with("error: UOS-AI-MANIFEST-009: "),
        "{}",
        run.stderr
    );
}

#[test]
fn every_class_is_exercised_with_the_trusted_path() {
    let expected = [
        (OBSERVE, 0, "allow", EXIT_ALLOW),
        (REVERSIBLE, 1, "allow", EXIT_ALLOW),
        (CONSEQUENTIAL, 2, "ask", EXIT_ASK),
        (SYSTEM_CRITICAL, 3, "authenticate", EXIT_AUTHENTICATE),
    ];
    for (tool, class, decision, exit) in expected {
        let (status, document) = check(tool, &["--trusted-path"]);
        assert_eq!(status, exit, "{tool}");
        assert_eq!(text(&document, "decision"), decision, "{tool}");
        assert_eq!(document["class"], class, "{tool}");
        assert_eq!(document["error_code"], Value::Null, "{tool}");
        assert_eq!(
            document["audit"]["UOS_AI_CLASS"],
            class.to_string(),
            "{tool}"
        );
        assert_eq!(document["audit"]["UOS_AI_TIER"], "1", "{tool}");
        assert_eq!(document["audit"]["UOS_AI_TASK"], "cli", "{tool}");
    }
}

#[test]
fn without_the_trusted_path_only_classes_0_and_1_run() {
    let expected = [
        (OBSERVE, "allow", EXIT_ALLOW, Value::Null),
        (REVERSIBLE, "allow", EXIT_ALLOW, Value::Null),
        (
            CONSEQUENTIAL,
            "refuse",
            EXIT_REFUSE,
            Value::from("UOS-AI-POLICY-006"),
        ),
        (
            SYSTEM_CRITICAL,
            "refuse",
            EXIT_REFUSE,
            Value::from("UOS-AI-POLICY-006"),
        ),
    ];
    for (tool, decision, exit, code) in expected {
        // No context flags at all: the strict defaults apply.
        let (status, document) = check(tool, &[]);
        assert_eq!(status, exit, "{tool}");
        assert_eq!(text(&document, "decision"), decision, "{tool}");
        assert_eq!(document["error_code"], code, "{tool}");
        if code != Value::Null {
            assert_eq!(document["audit"]["UOS_ERROR_CODE"], code, "{tool}");
            assert_eq!(document["audit"]["UOS_AI_DECISION"], "refused", "{tool}");
        }
    }
}

#[test]
fn class_3_is_never_allowed_whatever_the_caller_claims() {
    let approval = r#"{"service":"org.uos.Test1","tool":"test_system_critical","arguments":{"volume_id":"vol1"}}"#;
    let flags: [&[&str]; 6] = [
        &["--trusted-path"],
        &["--unattended"],
        &["--locked"],
        &["--model-flags-consequential"],
        &[
            "--repair-session-until",
            "2000",
            "--repair-session-purpose",
            "repair the disk",
        ],
        &["--standing-approval", approval],
    ];
    let policies: [&[&str]; 3] = [
        &[],
        &["--policy", OBSERVE_ONLY],
        &["--policy", ASK_EVERY_ACTION],
    ];

    let mut runs = 0;
    for subset in 0_u32..(1 << flags.len()) {
        for policy in policies {
            let mut extra: Vec<&str> = policy.to_vec();
            for (index, flag) in flags.iter().enumerate() {
                if subset & (1 << index) != 0 {
                    extra.extend_from_slice(flag);
                }
            }
            let (status, document) = check(SYSTEM_CRITICAL, &extra);
            assert_ne!(status, EXIT_ALLOW, "{extra:?}");
            assert_ne!(text(&document, "decision"), "allow", "{extra:?}");
            assert_eq!(document["class"], 3, "{extra:?}");
            assert_ne!(document["audit"]["UOS_AI_DECISION"], "allowed", "{extra:?}");
            runs += 1;
        }
    }
    assert_eq!(runs, 64 * 3);
}

#[test]
fn class_3_asks_for_authentication_only_when_a_person_can_give_it() {
    let (status, document) = check(SYSTEM_CRITICAL, &["--trusted-path"]);
    assert_eq!(status, EXIT_AUTHENTICATE);
    assert_eq!(
        text(&document, "reason"),
        "system_critical_needs_authentication"
    );
    assert_eq!(
        text(&document, "prompt"),
        "Call tool \"test_system_critical\" of service \"org.uos.Test1\" with arguments {\"volume_id\":\"vol1\"}"
    );
    assert_eq!(
        document["audit"]["UOS_AI_DECISION"],
        "authentication_required"
    );

    let (status, document) = check(SYSTEM_CRITICAL, &["--trusted-path", "--unattended"]);
    assert_eq!(status, EXIT_REFUSE);
    assert_eq!(document["error_code"], "UOS-AI-POLICY-007");

    let (status, document) = check(SYSTEM_CRITICAL, &["--trusted-path", "--locked"]);
    assert_eq!(status, EXIT_REFUSE);
    assert_eq!(document["error_code"], "UOS-AI-POLICY-007");
}

#[test]
fn unattended_class_2_needs_an_exactly_matching_standing_approval() {
    let unattended = ["--trusted-path", "--unattended"];

    let (status, document) = check(CONSEQUENTIAL, &unattended);
    assert_eq!(status, EXIT_DEFER);
    assert_eq!(
        text(&document, "reason"),
        "unattended_without_standing_approval"
    );
    assert_eq!(document["audit"]["UOS_AI_DECISION"], "deferred");

    let mut exact = unattended.to_vec();
    exact.extend(["--standing-approval", EXACT_APPROVAL]);
    let (status, document) = check(CONSEQUENTIAL, &exact);
    assert_eq!(status, EXIT_ALLOW);
    assert_eq!(text(&document, "reason"), "standing_approval_matched");

    let near_misses = [
        // One argument different.
        r#"{"service":"org.uos.Test1","tool":"test_consequential","arguments":{"recipient":"a@example.org","subject":"Weekly reports"}}"#,
        r#"{"service":"org.uos.Test1","tool":"test_consequential","arguments":{"recipient":"b@example.org","subject":"Weekly report"}}"#,
        // One argument missing.
        r#"{"service":"org.uos.Test1","tool":"test_consequential","arguments":{"recipient":"a@example.org"}}"#,
        // Another tool, another service.
        r#"{"service":"org.uos.Test1","tool":"test_reversible","arguments":{"recipient":"a@example.org","subject":"Weekly report"}}"#,
        r#"{"service":"org.uos.Other1","tool":"test_consequential","arguments":{"recipient":"a@example.org","subject":"Weekly report"}}"#,
    ];
    for approval in near_misses {
        let mut extra = unattended.to_vec();
        extra.extend(["--standing-approval", approval]);
        let (status, document) = check(CONSEQUENTIAL, &extra);
        assert_eq!(status, EXIT_DEFER, "{approval}");
        assert_eq!(text(&document, "decision"), "defer", "{approval}");
    }

    // An approval is for unattended tasks. Attended, the user is asked.
    let (status, _) = check(
        CONSEQUENTIAL,
        &["--trusted-path", "--standing-approval", EXACT_APPROVAL],
    );
    assert_eq!(status, EXIT_ASK);

    // And it does not replace the trusted path.
    let (status, document) = check(
        CONSEQUENTIAL,
        &["--unattended", "--standing-approval", EXACT_APPROVAL],
    );
    assert_eq!(status, EXIT_REFUSE);
    assert_eq!(document["error_code"], "UOS-AI-POLICY-006");
}

#[test]
fn the_model_hint_only_raises_the_class() {
    let (status, document) = check(OBSERVE, &["--trusted-path", "--model-flags-consequential"]);
    assert_eq!(status, EXIT_ASK);
    assert_eq!(document["class"], 2);

    let (status, document) = check(OBSERVE, &["--model-flags-consequential"]);
    assert_eq!(status, EXIT_REFUSE);
    assert_eq!(document["error_code"], "UOS-AI-POLICY-006");

    let (status, document) = check(
        SYSTEM_CRITICAL,
        &["--trusted-path", "--model-flags-consequential"],
    );
    assert_eq!(status, EXIT_AUTHENTICATE);
    assert_eq!(document["class"], 3);
}

#[test]
fn the_user_policy_file_is_applied() {
    let observe_only = ["--trusted-path", "--policy", OBSERVE_ONLY];
    let (status, _) = check(OBSERVE, &observe_only);
    assert_eq!(status, EXIT_ALLOW);
    for tool in [REVERSIBLE, CONSEQUENTIAL, SYSTEM_CRITICAL] {
        let (status, document) = check(tool, &observe_only);
        assert_eq!(status, EXIT_REFUSE, "{tool}");
        assert_eq!(document["error_code"], "UOS-AI-POLICY-005", "{tool}");
    }

    let ask_every = ["--trusted-path", "--policy", ASK_EVERY_ACTION];
    for tool in [OBSERVE, REVERSIBLE, CONSEQUENTIAL] {
        let (status, document) = check(tool, &ask_every);
        assert_eq!(status, EXIT_ASK, "{tool}");
        assert!(!text(&document, "prompt").is_empty(), "{tool}");
    }
    let (status, _) = check(SYSTEM_CRITICAL, &ask_every);
    assert_eq!(status, EXIT_AUTHENTICATE);

    let mut unattended = ask_every.to_vec();
    unattended.push("--unattended");
    let (status, _) = check(OBSERVE, &unattended);
    assert_eq!(status, EXIT_DEFER);
}

#[test]
fn the_audit_record_holds_no_argument_values() {
    let (_, document) = check(CONSEQUENTIAL, &["--trusted-path", "--task", "task-42"]);
    let audit = document["audit"].as_object().expect("audit is an object");
    assert_eq!(audit["UOS_AI_TASK"], "task-42");
    assert_eq!(audit["UOS_AI_ACTION"], "org.uos.Test1:test_consequential");
    assert_eq!(audit["UOS_AI_CLASS"], "2");
    assert_eq!(audit["UOS_AI_DECISION"], "confirmation_required");
    assert_eq!(audit["UOS_AI_PROVIDER"], "none");
    assert_eq!(
        audit["MESSAGE"],
        "uos-agent: confirmation_required org.uos.Test1:test_consequential (class 2, tier 1): consequential_needs_confirmation; arguments recipient=string, subject=string"
    );
    assert!(!audit.contains_key("UOS_ERROR_CODE"));
    for (name, value) in audit {
        let value = value.as_str().expect("journal fields are strings");
        assert!(!value.contains("example.org"), "{name}={value}");
        assert!(!value.contains("Weekly"), "{name}={value}");
    }
    // The prompt, which is shown and not logged, does state the arguments.
    assert!(text(&document, "prompt").contains("a@example.org"));
}

#[test]
fn the_plain_output_states_the_decision() {
    let run = uos_agent(&[
        "policy",
        "check",
        "--manifest",
        MANIFEST,
        "--tool",
        SYSTEM_CRITICAL,
        "--args",
        arguments_for(SYSTEM_CRITICAL),
    ]);
    assert_eq!(run.status, EXIT_REFUSE);
    assert_eq!(
        run.stdout,
        "decision: refuse\nclass: 3\nreason: trusted_path_unavailable\nerror_code: UOS-AI-POLICY-006\n"
    );

    let run = uos_agent(&["policy", "check", "--manifest", MANIFEST, "--tool", OBSERVE]);
    assert_eq!(run.status, EXIT_ALLOW);
    assert_eq!(
        run.stdout,
        "decision: allow\nclass: 0\nreason: observe_permitted\n"
    );
}

#[test]
fn invalid_input_is_reported_with_a_code_and_decides_nothing() {
    let base = ["policy", "check", "--manifest", MANIFEST];
    let cases: [(&[&str], &str); 8] = [
        (&["--tool", "no_such_tool"], "UOS-AI-TOOL-001"),
        (&["--tool", CONSEQUENTIAL], "UOS-AI-TOOL-002"),
        (
            &["--tool", OBSERVE, "--args", r#"{"extra":true}"#],
            "UOS-AI-TOOL-002",
        ),
        (
            &["--tool", OBSERVE, "--args", "not json"],
            "UOS-AI-INPUT-002",
        ),
        (&["--tool", OBSERVE, "--args", "[]"], "UOS-AI-INPUT-002"),
        (
            &["--tool", OBSERVE, "--standing-approval", r#"{"tool":"x"}"#],
            "UOS-AI-INPUT-002",
        ),
        (
            &[
                "--tool",
                OBSERVE,
                "--policy",
                "/nonexistent/uos-agent/policy.json",
            ],
            "UOS-AI-INPUT-001",
        ),
        (
            &["--tool", OBSERVE, "--policy", INVALID_RELAXATION],
            "UOS-AI-POLICYFILE-004",
        ),
    ];
    for (extra, code) in cases {
        let mut args = base.to_vec();
        args.extend_from_slice(extra);
        let run = uos_agent(&args);
        assert_eq!(run.status, EXIT_INPUT_ERROR, "{extra:?}");
        assert!(run.stdout.is_empty(), "{extra:?}: {}", run.stdout);
        assert!(
            run.stderr.starts_with(&format!("error: {code}: ")),
            "{extra:?}: {}",
            run.stderr
        );
    }

    let run = uos_agent(&[
        "policy",
        "check",
        "--manifest",
        CLASS_4_MANIFEST,
        "--tool",
        "x",
    ]);
    assert_eq!(run.status, EXIT_INPUT_ERROR);
    assert!(
        run.stderr.starts_with("error: UOS-AI-MANIFEST-009: "),
        "{}",
        run.stderr
    );
}

#[test]
fn a_usage_error_is_not_mistaken_for_a_decision() {
    let command_lines: [&[&str]; 5] = [
        &["policy", "check"],
        &["policy", "check", "--manifest", MANIFEST],
        &[
            "policy",
            "check",
            "--manifest",
            MANIFEST,
            "--tool",
            OBSERVE,
            "--allow",
        ],
        &["frobnicate"],
        &[],
    ];
    for args in command_lines {
        let run = uos_agent(args);
        assert_eq!(run.status, EXIT_USAGE, "{args:?}");
        assert!(run.stdout.is_empty(), "{args:?}");
    }
}
