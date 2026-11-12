// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The task loop through each provider's adapter.
//!
//! The other integration tests run the loop through the Anthropic adapter.
//! These run the same loop through the OpenAI and the Google adapter as well:
//! the scripted provider (`tests/common`) replays one script in each of the
//! three wire formats, and what the loop does must not depend on which one
//! it was.
//!
//! Each case is also run with the arguments of every call delivered in one
//! piece instead of streamed in fragments. The Google adapter delivers calls
//! that way when a step starts with its arguments, and the loop used to
//! refuse every such call, because it rebuilt the argument text from the
//! fragments and there were none.
//!
//! The formats are written by hand from the providers' documentation. No
//! real model is called by any test.

mod common;

use serde_json::json;
use tur_pax::COMMAND_TOOL;
use tur_pax::orchestrator::{ReportItem, SYSTEM_TEXT, TaskOutcome, UNTRUSTED_NOTICE};
use tur_pax::trail::EventKind;

use common::{
    FakeServer, Harness, Turn, Wire, call, calls, offered_tools, result_frames, says, system_of,
};

/// Every wire format, with the arguments streamed and delivered whole.
fn deliveries() -> Vec<(Wire, bool)> {
    let mut cases = Vec::new();
    for wire in Wire::ALL {
        for whole in [false, true] {
            cases.push((wire, whole));
        }
    }
    cases
}

fn delivered(turn: Turn, whole: bool) -> Turn {
    if whole { turn.whole() } else { turn }
}

#[test]
fn a_task_with_tool_calls_runs_the_same_through_every_adapter() {
    for (wire, whole) in deliveries() {
        let case = format!("{wire:?}, whole: {whole}");
        let server = FakeServer::start_on(
            wire,
            vec![
                delivered(
                    calls(
                        "I will look, then set it.",
                        vec![
                            call("call_a", "test_observe", "{}"),
                            call("call_b", "test_reversible", r#"{"value": "night"}"#),
                        ],
                    ),
                    whole,
                ),
                says("Done: the value is now night."),
            ],
        );
        assert_eq!(server.wire(), wire);
        let mut harness = Harness::new();
        let report = harness.run(&server, "Set the test value to night.");

        assert_eq!(report.outcome, TaskOutcome::Completed, "{case}");
        assert_eq!(report.answer, "Done: the value is now night.", "{case}");
        assert_eq!((report.steps, report.requests), (2, 2), "{case}");
        assert_eq!(
            report.items[0],
            ReportItem::Said {
                text: "I will look, then set it.".to_owned()
            },
            "{case}"
        );

        // One decision each, with the class of the manifest, recorded under
        // the adapter that was used.
        assert_eq!(
            harness.decisions("TUR_AI_ACTION"),
            [
                "org.tur.Test1:test_observe",
                "org.tur.Test1:test_reversible"
            ],
            "{case}"
        );
        assert_eq!(harness.decisions("TUR_AI_CLASS"), ["0", "1"], "{case}");
        assert_eq!(
            harness.decisions("TUR_AI_DECISION"),
            ["allowed", "allowed"],
            "{case}"
        );
        assert_eq!(
            harness.decisions("TUR_AI_PROVIDER"),
            [wire.provider(), wire.provider()],
            "{case}"
        );

        // No call was rejected for the way its arguments arrived, and both
        // were performed with the arguments the script wrote.
        assert!(
            harness.trail.events(EventKind::ProposalRejected).is_empty(),
            "{case}"
        );
        assert_eq!(
            harness.executor.commands(),
            [
                vec!["/usr/bin/tur-pax-test-tool", "observe"],
                vec!["/usr/bin/tur-pax-test-tool", "set", "--", "night"]
            ],
            "{case}"
        );
        assert_eq!(
            harness.events(EventKind::Executed, "TUR_AI_RESULT"),
            ["ok", "ok"],
            "{case}"
        );

        // What the adapter sent: the fixed system text where the format puts
        // it, the tools by name, and each result in the format's tool-result
        // position, framed as untrusted data.
        let bodies = server.bodies();
        assert_eq!(bodies.len(), 2, "{case}");
        for body in &bodies {
            assert_eq!(system_of(wire, body), json!(SYSTEM_TEXT), "{case}");
            let offered = offered_tools(body);
            for tool in ["test_observe", "test_reversible", COMMAND_TOOL] {
                assert!(offered.iter().any(|name| name == tool), "{case}: {tool}");
            }
        }
        assert!(result_frames(wire, &bodies[0]).is_empty(), "{case}");
        let frames = result_frames(wire, &bodies[1]);
        assert_eq!(frames.len(), 2, "{case}");
        for frame in &frames {
            assert_eq!(frame["notice"], UNTRUSTED_NOTICE, "{case}");
            assert_eq!(frame["status"], "ok", "{case}");
            assert_eq!(frame["output"], json!({ "ok": true }), "{case}");
        }
    }
}

#[test]
fn nothing_above_class_one_runs_through_any_adapter() {
    for (wire, whole) in deliveries() {
        let case = format!("{wire:?}, whole: {whole}");
        let server = FakeServer::start_on(
            wire,
            vec![
                delivered(
                    calls(
                        "",
                        vec![
                            call(
                                "call_send",
                                "test_consequential",
                                r#"{"recipient": "a@example.org"}"#,
                            ),
                            call(
                                "call_format",
                                "test_system_critical",
                                r#"{"volume_id": "vol-1"}"#,
                            ),
                            call("call_read", "test_observe", "{}"),
                        ],
                    ),
                    whole,
                ),
                says("Only the reading was possible."),
            ],
        );
        let mut harness = Harness::new();
        let report = harness.run(&server, "Send it, format the volume, then read the value.");
        assert_eq!(report.outcome, TaskOutcome::Completed, "{case}");

        assert_eq!(harness.decisions("TUR_AI_CLASS"), ["2", "3", "0"], "{case}");
        assert_eq!(
            harness.decisions("TUR_AI_DECISION"),
            ["refused", "refused", "allowed"],
            "{case}"
        );
        // Only the observation was handed to the executor.
        assert_eq!(
            harness.executor.commands(),
            [vec!["/usr/bin/tur-pax-test-tool", "observe"]],
            "{case}"
        );
        let frames = result_frames(wire, &server.bodies()[1]);
        let statuses: Vec<&str> = frames
            .iter()
            .map(|frame| frame["status"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(statuses, ["refused", "refused", "ok"], "{case}");
    }
}

#[test]
fn a_repeated_key_discards_the_whole_response_through_every_adapter() {
    // The second call names `value` twice: one value to show, another to act
    // on. The first call of the same response is unremarkable.
    for (wire, whole) in deliveries() {
        let case = format!("{wire:?}, whole: {whole}");
        let server = FakeServer::start_on(
            wire,
            vec![
                delivered(
                    calls(
                        "I will read it and then set it.",
                        vec![
                            call("call_a", "test_observe", "{}"),
                            call(
                                "call_b",
                                "test_reversible",
                                r#"{"value": "harmless", "value": "harmful"}"#,
                            ),
                        ],
                    ),
                    whole,
                ),
                says("This answer is never asked for."),
            ],
        );
        let mut harness = Harness::new();
        let report = harness.run(&server, "Read the value and set it.");

        // The provider layer delivers no such call: the response is
        // malformed, and a malformed response is discarded whole.
        assert_eq!(report.outcome, TaskOutcome::ProviderError, "{case}");
        assert_eq!(
            report.error_code.as_deref(),
            Some("TUR-AI-PROVIDER-007"),
            "{case}"
        );
        assert!(report.answer.is_empty(), "{case}");
        // Neither value was acted on, and neither was the call before it.
        assert!(harness.trail.decisions().is_empty(), "{case}");
        assert!(harness.executor.plans.is_empty(), "{case}");
        assert!(
            harness.trail.events(EventKind::Executed).is_empty(),
            "{case}"
        );
        assert_eq!(
            harness.events(EventKind::ResponseDiscarded, "TUR_AI_REASON"),
            ["error"],
            "{case}"
        );
        assert_eq!(
            harness.events(EventKind::ResponseDiscarded, "TUR_ERROR_CODE"),
            ["TUR-AI-PROVIDER-007"],
            "{case}"
        );
        // The model is not asked again, and the conversation keeps nothing.
        assert_eq!(server.requests().len(), 1, "{case}");
        assert!(harness.history.is_empty(), "{case}");
        // Neither value reached the trail.
        let trail = harness.trail_text();
        assert!(
            !trail.contains("harmless") && !trail.contains("harmful"),
            "{case}"
        );
    }
}

#[test]
fn a_repeated_key_at_depth_is_refused_through_every_adapter() {
    // The command tool takes an array; a nested object is the only place a
    // key can repeat at depth. The arguments are outside the tool's schema as
    // well, but the response never gets that far.
    for (wire, whole) in deliveries() {
        let case = format!("{wire:?}, whole: {whole}");
        let server = FakeServer::start_on(
            wire,
            vec![delivered(
                calls(
                    "",
                    vec![call(
                        "call_a",
                        COMMAND_TOOL,
                        r#"{"argv": ["/usr/bin/true"], "options": {"sandbox": "read-only", "sandbox": "privileged"}}"#,
                    )],
                ),
                whole,
            )],
        );
        let mut harness = Harness::new();
        let report = harness.run(&server, "Run it.");
        assert_eq!(report.outcome, TaskOutcome::ProviderError, "{case}");
        assert_eq!(
            report.error_code.as_deref(),
            Some("TUR-AI-PROVIDER-007"),
            "{case}"
        );
        assert!(harness.trail.decisions().is_empty(), "{case}");
        assert!(
            harness.trail.events(EventKind::ProposalRejected).is_empty(),
            "{case}"
        );
        assert!(harness.executor.plans.is_empty(), "{case}");
    }
}

#[test]
fn a_response_that_does_not_end_well_is_discarded_through_every_adapter() {
    let endings = [
        ("refusal", TaskOutcome::ProviderRefusal),
        ("error", TaskOutcome::ProviderError),
        ("max_tokens", TaskOutcome::OutputLimit),
    ];
    for wire in Wire::ALL {
        for (stop, outcome) in endings {
            let case = format!("{wire:?}, {stop}");
            let server = FakeServer::start_on(
                wire,
                vec![
                    calls(
                        "First I will read the value.",
                        vec![call("call_a", "test_observe", "{}")],
                    )
                    .stopping(stop),
                ],
            );
            let mut harness = Harness::new();
            let report = harness.run(&server, "Read the value.");
            assert_eq!(report.outcome, outcome, "{case}");
            assert!(report.answer.is_empty(), "{case}");
            assert!(harness.trail.decisions().is_empty(), "{case}");
            assert!(harness.executor.plans.is_empty(), "{case}");
            assert_eq!(
                harness.trail.events(EventKind::ResponseDiscarded).len(),
                1,
                "{case}"
            );
            assert_eq!(server.requests().len(), 1, "{case}");
        }
    }
}
