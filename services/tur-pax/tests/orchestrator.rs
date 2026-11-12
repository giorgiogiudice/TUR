// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The task loop, end to end against a scripted provider.
//!
//! Each test starts a local server that replays a scripted model
//! (`tests/common`), connects the real Anthropic adapter to it, and runs the
//! real loop: the registry, the policy engine, the audit trail and the output
//! filter are the shipped ones. Only two things are test doubles: the
//! executor, which records the sandbox plans it is handed instead of starting
//! bubblewrap, and the clock. The sandbox itself is exercised for real by the
//! `pax` workflow (`tests/pax`).
//!
//! No real model is called by any test.

mod common;

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use tur_pax::adapter::FEED_OUTPUT_BYTES;
use tur_pax::exec::{ExecOutput, ExecStatus};
use tur_pax::orchestrator::{
    ReportItem, SYSTEM_TEXT, Signal, Situation, TaskOutcome, UNTRUSTED_NOTICE,
};
use tur_pax::redact::WITHHELD;
use tur_pax::sandbox::{BWRAP, GrantRefusal, GrantRules, MountKind};
use tur_pax::trail::EventKind;
use tur_pax::{
    ActionClass, COMMAND_TOOL, Mode, Presence, Timestamp, ToolRegistry, UserPolicy, decide,
};

use common::{
    FakeServer, Harness, ManualClock, ScriptedControl, TEST_MANIFEST, call, calls,
    first_user_texts, frame, printed, says, tool_results,
};

const APPROVAL_MESSAGE: &str = "P@x would need your approval for this, which is not available yet.";

fn policy(mode: &str) -> UserPolicy {
    UserPolicy::from_json(&format!(r#"{{"schema_version":1,"mode":"{mode}"}}"#))
        .expect("a valid policy")
}

fn proposals(items: &[ReportItem]) -> Vec<&tur_pax::ProposalReport> {
    items
        .iter()
        .filter_map(|item| match item {
            ReportItem::Proposal(proposal) => Some(proposal),
            _ => None,
        })
        .collect()
}

#[test]
fn a_question_that_needs_no_tool_is_answered() {
    let server = FakeServer::start(vec![says("The answer is 42.")]);
    let mut harness = Harness::new();
    let report = harness.run(&server, "What is six times seven?");

    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(report.answer, "The answer is 42.");
    assert_eq!(report.error_code, None);
    assert_eq!((report.steps, report.requests), (1, 1));
    assert_eq!(
        (report.usage.input_tokens, report.usage.output_tokens),
        (40, 30)
    );
    assert!(report.items.is_empty());

    let bodies = server.bodies();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["system"], SYSTEM_TEXT);
    assert_eq!(first_user_texts(&bodies[0]), ["What is six times seven?"]);
    let tools = bodies[0]["tools"].as_array().expect("tools were offered");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "test_observe",
            "test_reversible",
            "test_consequential",
            "test_system_critical",
            COMMAND_TOOL
        ]
    );
    // A provider is told a tool's name, description and input schema. It is
    // not told the class, and nothing asks it to choose one.
    for tool in tools {
        let keys: Vec<&String> = tool.as_object().expect("an object").keys().collect();
        assert_eq!(keys, ["description", "input_schema", "name"]);
    }

    assert!(harness.executor.plans.is_empty());
    assert_eq!(harness.trail.events(EventKind::TaskStarted).len(), 1);
    assert_eq!(
        harness.events(EventKind::TaskFinished, "TUR_AI_OUTCOME"),
        ["completed"]
    );
    assert_eq!(harness.history.len(), 2);
}

#[test]
fn tool_calls_of_one_response_are_decided_one_by_one_and_performed() {
    let server = FakeServer::start(vec![
        calls(
            "I will look, then set it.",
            vec![
                call("toolu_a", "test_observe", "{}"),
                call("toolu_b", "test_reversible", r#"{"value": "night"}"#),
            ],
        ),
        says("Done: the value is now night."),
    ]);
    let mut harness = Harness::new();
    let report = harness.run(&server, "Set the test value to night.");

    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(report.answer, "Done: the value is now night.");
    assert_eq!((report.steps, report.requests), (2, 2));

    // One decision each, with the class of the manifest.
    assert_eq!(
        harness.decisions("TUR_AI_ACTION"),
        [
            "org.tur.Test1:test_observe",
            "org.tur.Test1:test_reversible"
        ]
    );
    assert_eq!(harness.decisions("TUR_AI_CLASS"), ["0", "1"]);
    assert_eq!(harness.decisions("TUR_AI_DECISION"), ["allowed", "allowed"]);
    assert_eq!(harness.decisions("TUR_AI_TIER"), ["1", "1"]);
    assert_eq!(
        harness.decisions("TUR_AI_PROVIDER"),
        ["anthropic", "anthropic"]
    );
    assert_eq!(harness.decisions("TUR_AI_ATTENDED"), ["yes", "yes"]);

    // Performed through the adapters, as argument vectors.
    assert_eq!(
        harness.executor.commands(),
        [
            vec!["/usr/bin/tur-pax-test-tool", "observe"],
            vec!["/usr/bin/tur-pax-test-tool", "set", "--", "night"]
        ]
    );
    for plan in &harness.executor.plans {
        assert_eq!(plan.program, BWRAP);
    }

    // The record of a decision comes before the action, and the record of
    // what it returned before the next decision.
    let order: Vec<String> = harness
        .trail
        .entries()
        .iter()
        .map(|entry| entry.field("TUR_AI_EVENT").unwrap_or_default())
        .collect();
    assert_eq!(
        order,
        [
            "task_started",
            "decision",
            "executed",
            "decision",
            "executed",
            "task_finished"
        ]
    );
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_AI_RESULT"),
        ["ok", "ok"]
    );

    // The results go back in the tool-result position, each tied to its
    // call, framed as untrusted data. The system text is untouched.
    let bodies = server.bodies();
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[1]["system"], SYSTEM_TEXT);
    let results = tool_results(&bodies[1]);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["tool_use_id"], "toolu_a");
    assert_eq!(results[1]["tool_use_id"], "toolu_b");
    for block in &results {
        let frame = frame(block);
        assert_eq!(frame["notice"], UNTRUSTED_NOTICE);
        assert_eq!(frame["status"], "ok");
        assert_eq!(frame["output"], json!({ "ok": true }));
        assert!(block.get("is_error").is_none());
    }

    // What the user is shown.
    assert_eq!(
        report.items[0],
        ReportItem::Said {
            text: "I will look, then set it.".to_owned()
        }
    );
    let shown = proposals(&report.items);
    assert_eq!(shown.len(), 2);
    assert_eq!(
        (shown[0].tool.as_str(), shown[0].class, shown[0].decision),
        ("test_observe", Some(0), "allowed")
    );
    assert_eq!(
        (shown[1].tool.as_str(), shown[1].class, shown[1].decision),
        ("test_reversible", Some(1), "allowed")
    );
    assert!(shown.iter().all(|proposal| proposal.executed));
    assert!(shown[1].description.contains("test_reversible"));
}

#[test]
fn a_command_is_a_read_only_command_whatever_it_says() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![call(
                "toolu_c",
                COMMAND_TOOL,
                r#"{"argv": ["/usr/bin/sudo", "rm", "-rf", "/home"]}"#,
            )],
        ),
        says("That did not work."),
    ]);
    let mut harness = Harness::new();
    harness.executor.default = ExecOutput {
        status: ExecStatus::Exited(1),
        stdout: Vec::new(),
        stderr: b"sudo: not permitted\n".to_vec(),
        stdout_truncated: false,
        stderr_truncated: false,
    };
    let report = harness.run(&server, "Clean up my home folder.");
    assert_eq!(report.outcome, TaskOutcome::Completed);

    // The class comes from the sandbox: class 0, read-only, tier 4. The text
    // of the command plays no part.
    assert_eq!(harness.decisions("TUR_AI_ACTION"), ["command:read_only"]);
    assert_eq!(harness.decisions("TUR_AI_CLASS"), ["0"]);
    assert_eq!(harness.decisions("TUR_AI_TIER"), ["4"]);
    assert_eq!(harness.decisions("TUR_AI_SANDBOX"), ["read-only"]);
    assert_eq!(harness.decisions("TUR_AI_DECISION"), ["allowed"]);

    // It runs in the read-only sandbox, as an argument vector.
    assert_eq!(harness.executor.plans.len(), 1);
    let plan = &harness.executor.plans[0];
    assert_eq!(plan.argv, ["/usr/bin/sudo", "rm", "-rf", "/home"]);
    assert_eq!(plan.program, BWRAP);
    for option in [
        "--unshare-net",
        "--unshare-user",
        "--clearenv",
        "--new-session",
    ] {
        assert!(plan.arguments.iter().any(|argument| argument == option));
    }
    assert!(!plan.arguments.iter().any(|argument| argument == "--bind"));
    assert!(!plan.shows("/home"));
    assert!(!plan.shows("/home/ada"));
    assert!(plan.stdin.is_none());

    // Its failure is reported to the model as a failure.
    let results = tool_results(&server.bodies()[1]);
    assert_eq!(results[0]["is_error"], true);
    let frame = frame(&results[0]);
    assert_eq!(frame["status"], "failed");
    assert_eq!(frame["exit_code"], 1);
    assert_eq!(frame["stderr"], "sudo: not permitted\n");
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_ERROR_CODE"),
        ["TUR-AI-TASK-008"]
    );
}

#[test]
fn an_action_above_class_one_is_refused_and_the_model_is_told_why() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call(
                    "toolu_1",
                    "test_consequential",
                    r#"{"recipient": "a@example.org"}"#,
                ),
                call(
                    "toolu_2",
                    "test_system_critical",
                    r#"{"volume_id": "vol-1"}"#,
                ),
            ],
        ),
        says("I am not able to do either of those."),
    ]);
    let mut harness = Harness::new();
    let report = harness.run(&server, "Send the report and format the volume.");

    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.decisions("TUR_AI_CLASS"), ["2", "3"]);
    assert_eq!(harness.decisions("TUR_AI_DECISION"), ["refused", "refused"]);
    assert_eq!(
        harness.decisions("TUR_ERROR_CODE"),
        ["TUR-AI-POLICY-006", "TUR-AI-POLICY-006"]
    );
    assert!(harness.executor.plans.is_empty());
    assert!(harness.trail.events(EventKind::Executed).is_empty());

    let results = tool_results(&server.bodies()[1]);
    assert_eq!(results.len(), 2);
    for block in &results {
        assert_eq!(block["is_error"], true);
        let frame = frame(block);
        assert_eq!(frame["status"], "refused");
        assert_eq!(frame["reason"], "trusted_path_unavailable");
        assert_eq!(frame["error_code"], "TUR-AI-POLICY-006");
    }
    let shown = proposals(&report.items);
    assert!(shown.iter().all(|proposal| !proposal.executed));
    assert!(shown.iter().all(|proposal| proposal.decision == "refused"));
    assert!(shown.iter().all(|proposal| proposal.message.is_some()));
}

#[test]
fn a_decision_that_needs_the_user_is_never_resolved() {
    // Ask (the user asked to approve everything), defer (nobody is there to
    // ask), defer (the session is locked).
    let cases = [
        (
            policy("ask_every_action"),
            Situation::attended(),
            "confirmation_required",
        ),
        (
            policy("ask_every_action"),
            Situation::unattended(),
            "deferred",
        ),
        (
            policy("normal"),
            Situation {
                presence: Presence::Attended,
                session_locked: true,
            },
            "deferred",
        ),
    ];
    for (policy, situation, expected) in cases {
        let server = FakeServer::start(vec![
            calls(
                "",
                vec![
                    call("toolu_1", "test_observe", "{}"),
                    call("toolu_2", COMMAND_TOOL, r#"{"argv": ["/usr/bin/id"]}"#),
                ],
            ),
            says("I would need your approval."),
        ]);
        let mut harness = Harness::new();
        harness.policy = policy;
        harness.situation = situation;
        let report = harness.run(&server, "What is the test value?");

        assert_eq!(report.outcome, TaskOutcome::Completed, "{expected}");
        assert_eq!(
            harness.decisions("TUR_AI_DECISION"),
            [expected, expected],
            "{expected}"
        );
        // Nothing ran, and the trail says the approval could not be asked for.
        assert!(harness.executor.plans.is_empty(), "{expected}");
        assert_eq!(
            harness.events(EventKind::ApprovalUnavailable, "TUR_ERROR_CODE"),
            ["TUR-AI-TASK-004", "TUR-AI-TASK-004"],
            "{expected}"
        );
        // The model is told it was refused, with the reason.
        for block in tool_results(&server.bodies()[1]) {
            assert_eq!(block["is_error"], true);
            let frame = frame(&block);
            assert_eq!(frame["status"], "refused");
            assert_eq!(frame["reason"], "approval_unavailable");
            assert_eq!(frame["message"], APPROVAL_MESSAGE);
        }
        // The user is told the same.
        for proposal in proposals(&report.items) {
            assert_eq!(proposal.message.as_deref(), Some(APPROVAL_MESSAGE));
            assert!(!proposal.executed);
        }
    }
}

#[test]
fn observe_only_lets_only_class_zero_run() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call("toolu_1", "test_observe", "{}"),
                call("toolu_2", "test_reversible", r#"{"value": "x"}"#),
            ],
        ),
        says("I read the value but may not change it."),
    ]);
    let mut harness = Harness::new();
    harness.policy = policy("observe_only");
    let report = harness.run(&server, "Read the value and set it.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.decisions("TUR_AI_DECISION"), ["allowed", "refused"]);
    assert_eq!(
        harness.decisions("TUR_ERROR_CODE"),
        ["", "TUR-AI-POLICY-005"]
    );
    assert_eq!(
        harness.executor.commands(),
        [vec!["/usr/bin/tur-pax-test-tool", "observe"]]
    );
}

/// Valid arguments for a tool of the test service, by name.
fn valid_arguments(tool: &str) -> &'static str {
    match tool {
        "test_observe" => "{}",
        "test_reversible" => r#"{"value": "x"}"#,
        "test_consequential" => r#"{"recipient": "a@example.org", "subject": "Report"}"#,
        "test_system_critical" => r#"{"volume_id": "vol-1"}"#,
        COMMAND_TOOL => r#"{"argv": ["/usr/bin/id"]}"#,
        other => panic!("the test has no arguments for the tool {other}"),
    }
}

#[test]
fn nothing_above_class_one_runs_whatever_the_policy_and_the_situation() {
    let policies = [
        r#"{"schema_version":1,"mode":"normal"}"#,
        r#"{"schema_version":1,"mode":"ask_every_action"}"#,
        r#"{"schema_version":1,"mode":"observe_only"}"#,
        // A relaxation is for one application's screen actions. It does not
        // reach a tool.
        r#"{"schema_version":1,"mode":"normal","relaxations":[{"application":"org.tur.Test1","class":2}]}"#,
    ];
    let mut runs = 0;
    for policy_text in policies {
        for presence in Presence::ALL {
            for session_locked in [false, true] {
                let context = format!("{policy_text} {presence:?} locked={session_locked}");
                let mut harness = Harness::new();
                harness.policy = UserPolicy::from_json(policy_text).expect("a valid policy");
                harness.situation = Situation {
                    presence,
                    session_locked,
                };

                // Every offered tool, in one response: the whole tool set.
                let offered = harness.registry.offered();
                assert!(
                    offered
                        .iter()
                        .any(|tool| tool.class == ActionClass::Consequential)
                );
                assert!(
                    offered
                        .iter()
                        .any(|tool| tool.class == ActionClass::SystemCritical)
                );
                let proposed: Vec<common::Call> = offered
                    .iter()
                    .enumerate()
                    .map(|(index, tool)| {
                        call(
                            &format!("toolu_{index}"),
                            &tool.name,
                            valid_arguments(&tool.name),
                        )
                    })
                    .collect();
                let server = FakeServer::start(vec![calls("", proposed), says("Finished.")]);
                let report = harness.run(&server, "Do everything you can.");
                assert_eq!(report.outcome, TaskOutcome::Completed, "{context}");
                runs += 1;

                // Every tool was decided, and none above class 1 was allowed.
                let classes = harness.decisions("TUR_AI_CLASS");
                let decisions = harness.decisions("TUR_AI_DECISION");
                assert_eq!(classes.len(), offered.len(), "{context}");
                for (class, decision) in classes.iter().zip(&decisions) {
                    if class.as_str() >= "2" {
                        assert_eq!(decision, "refused", "{context}: class {class}");
                    }
                }
                // What ran is among the class 0 and class 1 tools.
                for command in harness.executor.commands() {
                    let verb = command.get(1).map_or("", String::as_str);
                    assert!(
                        matches!(verb, "observe" | "set") || command[0] == "/usr/bin/id",
                        "{context}: {command:?} was run"
                    );
                }
                for action in harness.events(EventKind::Executed, "TUR_AI_ACTION") {
                    assert!(
                        [
                            "org.tur.Test1:test_observe",
                            "org.tur.Test1:test_reversible",
                            "command:read_only"
                        ]
                        .contains(&action.as_str()),
                        "{context}: {action} was performed"
                    );
                }
                for proposal in proposals(&report.items) {
                    if proposal.class.is_some_and(|class| class >= 2) {
                        assert!(!proposal.executed, "{context}: {}", proposal.tool);
                    }
                }
            }
        }
    }
    assert_eq!(runs, 16);
}

#[test]
fn the_engine_allows_no_offered_tool_above_class_one_in_any_situation() {
    let registry = ToolRegistry::load(
        &[TEST_MANIFEST.to_owned()],
        &[common::TEST_ADAPTERS.to_owned()],
        true,
    )
    .expect("a valid tool set");
    let mut checked = 0;
    for tool in registry.offered() {
        let arguments =
            tur_pax::strict::object_from_str(valid_arguments(&tool.name)).expect("valid arguments");
        let proposal = registry.propose(&tool.name, arguments).expect("a proposal");
        for mode in Mode::ALL {
            for presence in Presence::ALL {
                for session_locked in [false, true] {
                    let situation = Situation {
                        presence,
                        session_locked,
                    };
                    let policy = UserPolicy {
                        mode,
                        ..UserPolicy::default()
                    };
                    let decision = decide(
                        &proposal.action,
                        &situation.context(),
                        &policy,
                        Timestamp(0),
                    );
                    if decision.class() >= ActionClass::Consequential {
                        assert!(!decision.is_allow(), "{} {mode:?}", tool.name);
                        assert!(decision.code().is_some(), "{} {mode:?}", tool.name);
                    }
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 5 * 3 * 2 * 2);
}

#[test]
fn a_model_that_never_stops_is_cut_off_at_the_step_limit() {
    let server = FakeServer::repeating(vec![calls(
        "Once more.",
        vec![call("toolu_r", "test_observe", "{}")],
    )]);
    let mut harness = Harness::new();
    harness.limits.max_steps = 3;
    let report = harness.run(&server, "Keep going.");

    assert_eq!(report.outcome, TaskOutcome::StepLimit);
    assert_eq!(report.error_code.as_deref(), Some("TUR-AI-TASK-001"));
    assert_eq!(report.steps, 3);
    assert_eq!(server.requests().len(), 3);
    assert_eq!(harness.executor.plans.len(), 3);
    assert!(report.answer.is_empty());
    assert!(!report.message.is_empty());
    assert_eq!(
        harness.events(EventKind::TaskFinished, "TUR_AI_OUTCOME"),
        ["step_limit"]
    );
    assert_eq!(
        harness.events(EventKind::TaskFinished, "TUR_AI_STEPS"),
        ["3"]
    );
    // A task that did not complete leaves the conversation as it was.
    assert!(harness.history.is_empty());
}

#[test]
fn a_model_that_never_stops_is_cut_off_at_the_spend_limit() {
    let server =
        FakeServer::repeating(vec![calls("", vec![call("toolu_s", "test_observe", "{}")])]);
    let config = server.config_with(|config| {
        config["task_limit"] = json!({ "max_tokens": 100 });
    });
    let mut harness = Harness::new();
    harness.limits.max_steps = 50;
    let report = harness.run_configured(&config, "Keep going.");

    // The first response used 70 tokens. The second passes 100 as soon as the
    // provider reports its input, and is cut there and discarded.
    assert_eq!(report.outcome, TaskOutcome::SpendLimit);
    assert_eq!(report.error_code.as_deref(), Some("TUR-AI-PROVIDER-010"));
    assert_eq!(server.requests().len(), 2);
    assert_eq!(harness.executor.plans.len(), 1);
    assert_eq!(harness.trail.decisions().len(), 1);
    assert!(report.usage.total_tokens() > 100);
    assert_eq!(
        harness.events(EventKind::ResponseDiscarded, "TUR_ERROR_CODE"),
        ["TUR-AI-PROVIDER-010"]
    );
    assert_eq!(
        harness.events(EventKind::TaskFinished, "TUR_AI_OUTCOME"),
        ["spend_limit"]
    );
}

#[test]
fn a_task_is_stopped_at_its_time_limit() {
    let server =
        FakeServer::repeating(vec![calls("", vec![call("toolu_t", "test_observe", "{}")])]);
    let mut harness = Harness::new();
    // Every reading of the clock moves it ten seconds.
    harness.clock = ManualClock::new(Duration::from_secs(10));
    harness.limits.max_seconds = 25;
    let report = harness.run(&server, "Keep going.");

    assert_eq!(report.outcome, TaskOutcome::TimeLimit);
    assert_eq!(report.error_code.as_deref(), Some("TUR-AI-TASK-002"));
    assert_eq!(server.requests().len(), 1);
    assert_eq!(harness.executor.plans.len(), 1);
}

#[test]
fn a_cancellation_is_honoured_between_two_actions() {
    let server = FakeServer::start(vec![calls(
        "",
        vec![
            call("toolu_1", "test_observe", "{}"),
            call("toolu_2", "test_reversible", r#"{"value": "x"}"#),
        ],
    )]);
    let mut harness = Harness::new();
    // Go on before the request and before the first action; then cancel.
    harness.control = Box::new(ScriptedControl::new(
        vec![Signal::Proceed, Signal::Proceed],
        Signal::Cancel,
    ));
    let report = harness.run(&server, "Read the value and set it.");

    assert_eq!(report.outcome, TaskOutcome::Cancelled);
    assert_eq!(report.error_code.as_deref(), Some("TUR-AI-TASK-003"));
    assert_eq!(
        harness.executor.commands(),
        [vec!["/usr/bin/tur-pax-test-tool", "observe"]]
    );
    assert_eq!(harness.trail.decisions().len(), 1);
    assert_eq!(server.requests().len(), 1);
    assert!(harness.history.is_empty());
}

#[test]
fn a_paused_task_waits_and_its_waiting_counts_towards_the_time_limit() {
    // Paused three times, then let go.
    let server = FakeServer::start(vec![says("Here I am.")]);
    let mut harness = Harness::new();
    harness.control = Box::new(ScriptedControl::new(
        vec![Signal::Pause, Signal::Pause, Signal::Pause],
        Signal::Proceed,
    ));
    let report = harness.run(&server, "Hello.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.clock.sleeps.get(), 3);

    // Paused for good: the time limit ends the task, and nothing was sent.
    let server = FakeServer::start(vec![says("Never said.")]);
    let mut harness = Harness::new();
    harness.control = Box::new(ScriptedControl::new(Vec::new(), Signal::Pause));
    harness.limits.max_seconds = 1;
    let report = harness.run(&server, "Hello.");
    assert_eq!(report.outcome, TaskOutcome::TimeLimit);
    assert!(harness.clock.sleeps.get() >= 10);
    assert!(server.requests().is_empty());
}

#[test]
fn a_response_that_does_not_end_well_is_discarded_whole() {
    let cases = [
        (
            "refusal",
            TaskOutcome::ProviderRefusal,
            "TUR-AI-PROVIDER-005",
        ),
        ("error", TaskOutcome::ProviderError, "TUR-AI-PROVIDER-003"),
        ("max_tokens", TaskOutcome::OutputLimit, "TUR-AI-TASK-007"),
    ];
    for (stop, outcome, code) in cases {
        let server = FakeServer::start(vec![
            calls(
                "First I will read the value.",
                vec![
                    call("toolu_1", "test_observe", "{}"),
                    call("toolu_2", "test_reversible", r#"{"value": "x"}"#),
                ],
            )
            .stopping(stop),
        ]);
        let mut harness = Harness::new();
        let report = harness.run(&server, "Read the value and set it.");

        assert_eq!(report.outcome, outcome, "{stop}");
        assert_eq!(report.error_code.as_deref(), Some(code), "{stop}");
        // Nothing it proposed was looked at, and its text is not shown.
        assert!(harness.trail.decisions().is_empty(), "{stop}");
        assert!(harness.executor.plans.is_empty(), "{stop}");
        assert!(report.answer.is_empty(), "{stop}");
        assert!(proposals(&report.items).is_empty(), "{stop}");
        assert!(
            !report
                .items
                .iter()
                .any(|item| matches!(item, ReportItem::Said { .. })),
            "{stop}"
        );
        assert_eq!(
            harness.events(EventKind::ResponseDiscarded, "TUR_AI_DISCARDED_CALLS"),
            ["2"],
            "{stop}"
        );
        assert_eq!(
            harness.events(EventKind::ResponseDiscarded, "TUR_ERROR_CODE"),
            [code],
            "{stop}"
        );
        assert_eq!(server.requests().len(), 1, "{stop}");
        assert!(harness.history.is_empty(), "{stop}");
    }
}

#[test]
fn no_record_no_action() {
    let script = || {
        vec![
            calls("", vec![call("toolu_1", "test_observe", "{}")]),
            says("Done."),
        ]
    };

    // The trail cannot take the start of the task: nothing is sent.
    let server = FakeServer::start(script());
    let mut harness = Harness::new();
    harness.trail_capacity = Some(0);
    let report = harness.run(&server, "Read the value.");
    assert_eq!(report.outcome, TaskOutcome::AuditUnavailable);
    assert_eq!(report.error_code.as_deref(), Some("TUR-AI-AUDIT-001"));
    assert!(server.requests().is_empty());

    // The trail cannot take the decision: the action does not run.
    let server = FakeServer::start(script());
    let mut harness = Harness::new();
    harness.trail_capacity = Some(1);
    let report = harness.run(&server, "Read the value.");
    assert_eq!(report.outcome, TaskOutcome::AuditUnavailable);
    assert!(harness.executor.plans.is_empty());
    assert_eq!(server.requests().len(), 1);

    // The trail cannot take the record of what the action returned: the
    // result is not passed on to the provider.
    let server = FakeServer::start(script());
    let mut harness = Harness::new();
    harness.trail_capacity = Some(2);
    let report = harness.run(&server, "Read the value.");
    assert_eq!(report.outcome, TaskOutcome::AuditUnavailable);
    assert_eq!(harness.executor.plans.len(), 1);
    assert_eq!(server.requests().len(), 1);
    assert!(report.answer.is_empty());
}

#[test]
fn calls_beyond_the_limit_of_one_response_are_not_decided() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call("toolu_1", "test_observe", "{}"),
                call("toolu_2", "test_observe", "{}"),
                call("toolu_3", "test_observe", "{}"),
            ],
        ),
        says("Done."),
    ]);
    let mut harness = Harness::new();
    harness.limits.max_calls_per_response = 1;
    let report = harness.run(&server, "Read the value three times.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.trail.decisions().len(), 1);
    assert_eq!(harness.executor.plans.len(), 1);
    assert_eq!(
        harness.events(EventKind::ProposalRejected, "TUR_ERROR_CODE"),
        ["TUR-AI-TASK-006", "TUR-AI-TASK-006"]
    );
    let results = tool_results(&server.bodies()[1]);
    assert_eq!(results.len(), 3);
    assert_eq!(frame(&results[2])["reason"], "too_many_calls");
}

#[test]
fn a_tool_that_fails_times_out_or_cannot_start_is_reported_as_such() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call("toolu_1", "test_observe", "{}"),
                call("toolu_2", "test_observe", "{}"),
                call("toolu_3", "test_observe", "{}"),
            ],
        ),
        says("None of them worked."),
    ]);
    let mut harness = Harness::new();
    let failed = |status: ExecStatus| ExecOutput {
        status,
        stdout: b"partial".to_vec(),
        stderr: b"boom".to_vec(),
        stdout_truncated: false,
        stderr_truncated: false,
    };
    harness.executor.replies = [
        failed(ExecStatus::Exited(3)),
        failed(ExecStatus::TimedOut),
        ExecOutput::not_started("bubblewrap is not installed"),
    ]
    .into();
    let report = harness.run(&server, "Read the value three times.");
    assert_eq!(report.outcome, TaskOutcome::Completed);

    assert_eq!(
        harness.events(EventKind::Executed, "TUR_AI_RESULT"),
        ["failed", "timed_out", "not_started"]
    );
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_ERROR_CODE"),
        [
            "TUR-AI-TASK-008",
            "TUR-AI-SANDBOX-004",
            "TUR-AI-SANDBOX-001"
        ]
    );
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_AI_EXIT"),
        ["3", "none", "none"]
    );
    let results = tool_results(&server.bodies()[1]);
    let frames: Vec<Value> = results.iter().map(frame).collect();
    for block in &results {
        assert_eq!(block["is_error"], true);
    }
    assert_eq!(frames[0]["status"], "failed");
    assert_eq!(frames[0]["exit_code"], 3);
    assert_eq!(frames[0]["output"], "partial");
    assert_eq!(frames[0]["stderr"], "boom");
    assert_eq!(frames[1]["status"], "timed_out");
    assert_eq!(frames[2]["status"], "not_started");
    assert_eq!(frames[2]["detail"], "bubblewrap is not installed");
    let shown = proposals(&report.items);
    let executed: Vec<bool> = shown.iter().map(|proposal| proposal.executed).collect();
    assert_eq!(executed, [true, true, false]);
}

#[test]
fn credential_like_output_is_withheld_and_the_trail_says_so_without_saying_what() {
    const KEY: &str = "sk-live-abcdef0123456789abcdef0123456789";
    const AWS: &str = "AKIAIOSFODNN7EXAMPLE";
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call("toolu_1", "test_observe", "{}"),
                call(
                    "toolu_2",
                    COMMAND_TOOL,
                    r#"{"argv": ["/usr/bin/cat", "/etc/app.conf"]}"#,
                ),
            ],
        ),
        says("I read both."),
    ]);
    let mut harness = Harness::new();
    harness.executor.replies = [
        printed(&format!(
            r#"{{"user":"ada","api_key":"{KEY}","note":"fine"}}"#
        )),
        printed(&format!(
            "region = eu\naws_access_key_id = {AWS}\nretries = 3\n"
        )),
    ]
    .into();
    let report = harness.run(&server, "Show me the configuration.");
    assert_eq!(report.outcome, TaskOutcome::Completed);

    // Neither secret left the machine.
    let sent = server.requests()[1].body.clone();
    assert!(!sent.contains(KEY));
    assert!(!sent.contains(AWS));
    assert!(sent.contains("region = eu"));

    let results = tool_results(&server.bodies()[1]);
    let structured = frame(&results[0]);
    assert_eq!(structured["withheld"], 1);
    assert_eq!(
        structured["output"],
        json!({ "user": "ada", "api_key": WITHHELD, "note": "fine" })
    );
    let text = frame(&results[1]);
    assert_eq!(text["withheld"], 1);
    assert_eq!(
        text["output"],
        format!("region = eu\n{WITHHELD}\nretries = 3\n")
    );

    // The trail records that something was withheld, and of what kind.
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_AI_WITHHELD"),
        ["1", "1"]
    );
    // The outcome of each action is that it ran; that output was withheld
    // is an event of its own, with the count and the kinds.
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_AI_RESULT"),
        ["ok", "ok"]
    );
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_ERROR_CODE"),
        ["", ""]
    );
    assert_eq!(
        harness.events(EventKind::OutputWithheld, "TUR_AI_WITHHELD"),
        ["1", "1"]
    );
    assert_eq!(
        harness.events(EventKind::OutputWithheld, "TUR_AI_WITHHELD_KINDS"),
        ["password", "known_token"]
    );
    assert_eq!(
        harness.events(EventKind::OutputWithheld, "TUR_ERROR_CODE"),
        ["TUR-AI-OUTPUT-001", "TUR-AI-OUTPUT-001"]
    );
    // Each withheld event follows the outcome of its action, before the
    // next decision.
    let order: Vec<String> = harness
        .trail
        .entries()
        .iter()
        .map(|entry| entry.field("TUR_AI_EVENT").unwrap_or_default())
        .collect();
    assert_eq!(
        order,
        [
            "task_started",
            "decision",
            "executed",
            "output_withheld",
            "decision",
            "executed",
            "output_withheld",
            "task_finished"
        ]
    );
    // It never records what.
    let trail = harness.trail_text();
    assert!(!trail.contains(KEY));
    assert!(!trail.contains(AWS));
    assert!(!trail.contains("region"));
    let shown = proposals(&report.items);
    assert_eq!((shown[0].withheld, shown[1].withheld), (1, 1));
}

#[test]
fn the_trail_never_holds_a_request_an_answer_or_an_argument() {
    let server = FakeServer::start(vec![
        calls(
            "My private thought about plums.",
            vec![
                call("toolu_1", "test_reversible", r#"{"value": "marmalade"}"#),
                call(
                    "toolu_2",
                    COMMAND_TOOL,
                    r#"{"argv": ["/usr/bin/echo", "quince"]}"#,
                ),
                call(
                    "toolu_3",
                    "made up tool; with spaces",
                    r#"{"value": "damson"}"#,
                ),
            ],
        ),
        says("The answer mentions apricots."),
    ]);
    let mut harness = Harness::new();
    harness.executor.default = printed("output about cherries");
    let report = harness.run(&server, "A request about rhubarb.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    let trail = harness.trail_text();
    for word in [
        "rhubarb",
        "plums",
        "marmalade",
        "quince",
        "damson",
        "apricots",
        "cherries",
        "made up tool",
    ] {
        assert!(!trail.contains(word), "{word} is in the trail");
    }
    // An invented name is recorded as invalid, not as it was written.
    assert_eq!(
        harness.events(EventKind::ProposalRejected, "TUR_AI_ACTION"),
        ["<invalid>"]
    );
}

#[test]
fn a_conversation_is_kept_between_tasks_that_completed() {
    let server = FakeServer::start(vec![says("Hello."), says("Yes, I said hello.")]);
    let mut harness = Harness::new();
    let first = harness.run(&server, "Say hello.");
    assert_eq!(first.outcome, TaskOutcome::Completed);
    assert_eq!(harness.history.len(), 2);
    let second = harness.run(&server, "Did you say hello?");
    assert_eq!(second.outcome, TaskOutcome::Completed);
    assert_eq!(harness.history.len(), 4);
    let bodies = server.bodies();
    assert_eq!(bodies[1]["messages"].as_array().map(Vec::len), Some(3));
    // Each task has its own records.
    assert_eq!(
        harness.events(EventKind::TaskFinished, "TUR_AI_OUTCOME"),
        ["completed", "completed"]
    );
}

const FEED_ADAPTERS: &str = r#"{
    "schema_version": 1,
    "service": "org.tur.Test1",
    "adapters": [{
        "tool": "test_observe",
        "kind": "command",
        "program": "/usr/bin/tur-pax-test-tool",
        "arguments": [
            { "literal": "volumes" },
            { "literal": "--from" },
            { "literal": "/dev/stdin" }
        ],
        "feed": { "program": "/usr/bin/lsblk", "arguments": ["--json"] },
        "timeout_seconds": 7,
        "max_output_bytes": 2048
    }]
}"#;

#[test]
fn a_feeding_command_runs_first_and_its_output_becomes_the_input() {
    let script = || {
        vec![
            calls("", vec![call("toolu_1", "test_observe", "{}")]),
            says("Done."),
        ]
    };
    let server = FakeServer::start(script());
    let mut harness = Harness::with_tools(&[TEST_MANIFEST], &[FEED_ADAPTERS]);
    // Tools without an adapter are not offered.
    assert_eq!(harness.registry.unavailable().len(), 3);
    harness.executor.replies = [
        printed("{\"blockdevices\":[]}"),
        printed("{\"volumes\":[]}"),
    ]
    .into();
    let report = harness.run(&server, "List the volumes.");
    assert_eq!(report.outcome, TaskOutcome::Completed);

    let plans = &harness.executor.plans;
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].argv, ["/usr/bin/lsblk", "--json"]);
    assert!(plans[0].stdin.is_none());
    assert_eq!(plans[0].limits.output_bytes, FEED_OUTPUT_BYTES);
    assert_eq!(
        plans[1].argv,
        [
            "/usr/bin/tur-pax-test-tool",
            "volumes",
            "--from",
            "/dev/stdin"
        ]
    );
    assert_eq!(
        plans[1].stdin.as_deref(),
        Some("{\"blockdevices\":[]}".as_bytes())
    );
    // The adapter's limits are the plan's limits.
    assert_eq!(plans[1].limits.wall_seconds, 7);
    assert_eq!(plans[1].limits.output_bytes, 2048);
    let results = tool_results(&server.bodies()[1]);
    assert_eq!(frame(&results[0])["output"], json!({ "volumes": [] }));
    // One action, one record of what it returned.
    assert_eq!(harness.trail.events(EventKind::Executed).len(), 1);

    // If the feeding command fails, the tool's program is not run.
    let server = FakeServer::start(script());
    let mut harness = Harness::with_tools(&[TEST_MANIFEST], &[FEED_ADAPTERS]);
    harness.executor.replies = [ExecOutput {
        status: ExecStatus::Exited(32),
        stdout: Vec::new(),
        stderr: b"lsblk: failed".to_vec(),
        stdout_truncated: false,
        stderr_truncated: false,
    }]
    .into();
    let report = harness.run(&server, "List the volumes.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.executor.plans.len(), 1);
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_AI_RESULT"),
        ["failed"]
    );
    assert_eq!(tool_results(&server.bodies()[1])[0]["is_error"], true);
}

#[test]
fn a_program_outside_the_system_view_must_pass_the_grant_rules() {
    let adapters = common::TEST_ADAPTERS.replace(
        "/usr/bin/tur-pax-test-tool",
        "/home/ada/.local/bin/tur-pax-test-tool",
    );
    let server = FakeServer::start(vec![
        calls("", vec![call("toolu_1", "test_observe", "{}")]),
        says("It could not be started."),
    ]);
    let mut harness = Harness::with_tools(&[TEST_MANIFEST], &[adapters.as_str()]);
    let report = harness.run(&server, "Read the value.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    // Nothing was handed to the executor.
    assert!(harness.executor.plans.is_empty());
    assert_eq!(
        harness.events(EventKind::Executed, "TUR_AI_RESULT"),
        ["not_started"]
    );
    let results = tool_results(&server.bodies()[1]);
    assert_eq!(frame(&results[0])["status"], "not_started");
    assert!(!proposals(&report.items)[0].executed);
}

#[test]
fn a_grant_is_checked_against_the_file_system_and_shown_read_only() {
    // A stand-in home directory inside the build directory.
    let home = Path::new(env!("CARGO_TARGET_TMPDIR")).join("grant-test-home");
    let _ = std::fs::remove_dir_all(&home);
    let granted = home.join("Documents");
    let hidden = home.join(".ssh");
    let link = home.join("link-to-ssh");
    std::fs::create_dir_all(&granted).expect("a directory");
    std::fs::create_dir_all(&hidden).expect("a directory");
    std::fs::write(granted.join("note.txt"), "a note").expect("a file");
    std::os::unix::fs::symlink(&hidden, &link).expect("a link");
    let text = |path: &Path| path.to_str().expect("a UTF-8 path").to_owned();

    let rules = GrantRules::new(Some(home.as_path()));
    assert!(
        !rules.homes().is_empty(),
        "the build directory is a usable home"
    );
    let grant = rules
        .admit(&text(&granted))
        .expect("an ordinary folder is granted");
    assert_eq!(grant.path(), text(&granted));
    assert!(rules.admit(&text(&granted.join("note.txt"))).is_ok());

    // What the rules refuse, on a real file system.
    assert!(matches!(
        rules.admit(&text(&hidden)),
        Err(GrantRefusal::Protected { .. })
    ));
    assert!(matches!(
        rules.admit(&text(&link)),
        Err(GrantRefusal::Link { .. })
    ));
    assert_eq!(
        rules.admit(&text(&home.join("missing"))),
        Err(GrantRefusal::Missing)
    );
    assert_eq!(rules.admit(&text(&home)), Err(GrantRefusal::ContainsHome));
    assert_eq!(rules.admit("/"), Err(GrantRefusal::Root));
    assert_eq!(rules.admit("/etc"), Err(GrantRefusal::System));
    assert_eq!(rules.admit("relative/path"), Err(GrantRefusal::NotPlain));

    // The grant reaches the sandbox plan of a command, read-only.
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![call(
                "toolu_1",
                COMMAND_TOOL,
                r#"{"argv": ["/usr/bin/ls", "-la"]}"#,
            )],
        ),
        says("Listed."),
    ]);
    let mut harness = Harness::new();
    harness.rules = rules;
    harness.grants = vec![grant];
    let report = harness.run(&server, "List my documents.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    let plan = &harness.executor.plans[0];
    let path = text(&granted);
    assert!(
        plan.mounts
            .iter()
            .any(|mount| mount.kind == MountKind::Granted && mount.path == path)
    );
    assert!(plan.shows(&text(&granted.join("note.txt"))));
    assert!(!plan.shows(&text(&hidden)));
    assert!(!plan.shows(&text(&home)));
    let position = plan
        .arguments
        .iter()
        .position(|argument| *argument == path)
        .expect("the grant is on the command line");
    assert_eq!(plan.arguments[position - 1], "--ro-bind");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn the_orchestrator_has_no_switch_for_the_trusted_path() {
    let source = |name: &str| {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name))
            .expect("a source file")
    };
    let modules = [
        "orchestrator.rs",
        "registry.rs",
        "adapter.rs",
        "sandbox.rs",
        "exec.rs",
        "redact.rs",
        "trail.rs",
        "faults.rs",
        "textui.rs",
    ];
    for name in modules {
        let text = source(name);
        // Nothing sets the trusted path, a repair session or a standing
        // approval.
        for forbidden in [
            "trusted_session_proven: true",
            "trusted_session_proven = true",
            "RepairSession {",
            "RepairSession::open",
            "StandingApproval {",
        ] {
            assert!(!text.contains(forbidden), "{name} contains {forbidden:?}");
        }
        // Only the text interface reads the environment, and the loop reads
        // none of it.
        if name != "textui.rs" {
            assert!(!text.contains("env::var"), "{name} reads the environment");
        }
    }
    // What the text interface reads from the environment is where its files
    // are and where the key is. Nothing else.
    let text = source("textui.rs");
    let read: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("env::var"))
        .collect();
    assert_eq!(read.len(), 3, "{read:?}");
    assert!(read[0].contains("var_os(variable)"));
    assert!(read[1].contains("var_os(\"HOME\")"));
    assert!(read[2].contains("var_os(\"HOME\")"));
    // The context of the loop is the one that grants nothing.
    for presence in Presence::ALL {
        for session_locked in [false, true] {
            let context = Situation {
                presence,
                session_locked,
            }
            .context();
            assert!(!context.trusted_session_proven);
            assert!(context.repair_session.is_none());
            assert!(context.standing_approvals.is_empty());
        }
    }
}
