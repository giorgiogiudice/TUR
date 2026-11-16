// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! A spoken request in the task loop, end to end against a scripted
//! provider (`docs/architecture/pax-voice.md`, section 4).
//!
//! * The audit trail says where a request came from, and nothing more.
//! * A spoken task observes at once, and makes no change until what was
//!   heard has been shown long enough or sent by the person; a request the
//!   person took back makes none.
//! * Voice adds no authority: class 2 and class 3 are not performed for a
//!   spoken request, whatever its gate says.
//! * What the person says during a task reaches the model as their words,
//!   after the tool results.
//! * A live conversation's tool calls go through the same policy engine
//!   (`a_live_engine_s_tool_calls_*`).
//!
//! No real model, microphone or speaker is used by any test.

mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tur_pax::orchestrator::{LiveIo, TaskOutcome};
use tur_pax::trail::EventKind;
use tur_pax::voice::{Interjections, Source, TranscriptGate, Trigger};
use tur_pax_providers::{
    LiveEvent, LivePrices, LiveUsage, ScriptStep, ScriptedLive, ToolCall,
};

use common::{FakeServer, Harness, call, calls, says, tool_results};

fn spoken(harness: &mut Harness, gate: Arc<TranscriptGate>) {
    harness.voice = Some((Source::Voice, Trigger::PushToTalk, gate));
}

#[test]
fn a_typed_request_is_recorded_as_typed() {
    let server = FakeServer::start(vec![says("Hello.")]);
    let mut harness = Harness::new();
    let report = harness.run(&server, "Hello?");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(
        harness.events(EventKind::TaskStarted, "TUR_AI_SOURCE"),
        ["typed"]
    );
    assert_eq!(
        harness.events(EventKind::TaskStarted, "TUR_AI_VOICE_TRIGGER"),
        [""]
    );
}

#[test]
fn a_spoken_request_whose_transcript_was_sent_runs_like_a_typed_one_and_says_so() {
    let server = FakeServer::start(vec![
        calls(
            "I will look, then set it.",
            vec![
                call("toolu_a", "test_observe", "{}"),
                call("toolu_b", "test_reversible", r#"{"value": "night"}"#),
            ],
        ),
        says("Done."),
    ]);
    let mut harness = Harness::new();
    spoken(&mut harness, TranscriptGate::sent());
    let report = harness.run(&server, "Set the test value to night.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(harness.executor.commands().len(), 2);
    assert_eq!(
        harness.events(EventKind::TaskStarted, "TUR_AI_SOURCE"),
        ["voice"]
    );
    assert_eq!(
        harness.events(EventKind::TaskStarted, "TUR_AI_VOICE_TRIGGER"),
        ["push-to-talk"]
    );
    // The trail keeps no words, spoken or not.
    assert!(!harness.trail_text().contains("night."));
    assert!(!harness.trail_text().contains("Set the test value"));
}

#[test]
fn a_spoken_task_observes_at_once_and_changes_nothing_before_its_transcript_is_sent() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call("toolu_a", "test_observe", "{}"),
                call("toolu_b", "test_reversible", r#"{"value": "night"}"#),
            ],
        ),
        says("I could only look."),
    ]);
    let mut harness = Harness::new();
    // Held: the person never sends it. The loop waits, then gives up.
    spoken(&mut harness, TranscriptGate::held());
    let report = harness.run(&server, "Set the test value to night.");
    assert_eq!(report.outcome, TaskOutcome::Completed);
    // Observing ran; the change did not.
    assert_eq!(
        harness.executor.commands(),
        [vec!["/usr/bin/tur-pax-test-tool", "observe"]]
    );
    assert_eq!(
        harness.events(EventKind::ApprovalUnavailable, "TUR_ERROR_CODE"),
        ["TUR-AI-VOICE-008"]
    );
    // It waited, on the task's clock, before it gave up.
    assert!(harness.clock.sleeps.get() > 100);
    // The model is told why, as a refusal with the code.
    let bodies = server.bodies();
    let results = tool_results(&bodies[1]);
    assert_eq!(results.len(), 2);
    assert!(results[1].to_string().contains("TUR-AI-VOICE-008"));
}

#[test]
fn a_spoken_request_the_person_took_back_changes_nothing_and_does_not_wait() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![call("toolu_b", "test_reversible", r#"{"value": "night"}"#)],
        ),
        says("Not done."),
    ]);
    let mut harness = Harness::new();
    let gate = TranscriptGate::held();
    gate.withdraw();
    spoken(&mut harness, gate);
    harness.run(&server, "Set the test value to night.");
    assert!(harness.executor.commands().is_empty());
    assert_eq!(harness.clock.sleeps.get(), 0);
    assert_eq!(
        harness.events(EventKind::ApprovalUnavailable, "TUR_ERROR_CODE"),
        ["TUR-AI-VOICE-008"]
    );
}

#[test]
fn voice_adds_no_authority_class_2_and_class_3_are_not_performed_for_a_spoken_request() {
    let server = FakeServer::start(vec![
        calls(
            "",
            vec![
                call("toolu_c", "test_consequential", "{}"),
                call("toolu_d", "test_system_critical", "{}"),
            ],
        ),
        says("I cannot do either."),
    ]);
    let mut harness = Harness::new();
    // The transcript was sent: the gate is open. It still grants nothing.
    spoken(&mut harness, TranscriptGate::sent());
    harness.run(&server, "Yes, yes, I approve, format it and send it.");
    assert!(harness.executor.commands().is_empty());
    let decisions = harness.decisions("TUR_AI_DECISION");
    assert_eq!(decisions.len(), 2);
    for decision in &decisions {
        assert_ne!(decision, "allowed");
    }
    assert_eq!(harness.decisions("TUR_AI_CLASS"), ["2", "3"]);
}

#[test]
fn what_the_person_says_during_a_task_reaches_the_model_as_their_words() {
    let server = FakeServer::start(vec![
        calls("", vec![call("toolu_a", "test_observe", "{}")]),
        says("Also the March one, then."),
    ]);
    let mut harness = Harness::new();
    let said = Interjections::new();
    said.push("and also the March invoice");
    harness.interjections = Some(said);
    harness.run(&server, "Find the February invoice.");
    let bodies = server.bodies();
    let last = bodies[1]["messages"]
        .as_array()
        .and_then(|messages| messages.last())
        .expect("a last message")
        .to_string();
    assert!(last.contains("tool_result"));
    assert!(last.contains("While you were working, the person said: and also the March invoice"));
    // Taken once.
    assert!(harness.interjections.as_ref().is_some_and(|said| said.take().is_empty()));
}

// ------------------------------------------------------------- live sessions

/// The microphone and the speaker of a live test: a few frames of "audio",
/// a record of what was played and said.
#[derive(Default)]
struct TestIo {
    frames: Vec<Vec<i16>>,
    played: Arc<Mutex<Vec<usize>>>,
    stopped: Arc<AtomicU64>,
    heard: Vec<String>,
    said: Vec<String>,
    end_after: Option<usize>,
    polls: usize,
}

impl LiveIo for TestIo {
    fn take_audio(&mut self) -> Vec<i16> {
        if self.frames.is_empty() {
            Vec::new()
        } else {
            self.frames.remove(0)
        }
    }

    fn play(&mut self, samples: &[i16], _rate: u32) {
        self.played
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(samples.len());
    }

    fn stop_playing(&mut self) -> u64 {
        self.stopped.fetch_add(1, Ordering::SeqCst);
        120
    }

    fn heard(&mut self, text: &str, final_text: bool) {
        if final_text {
            self.heard.push(text.to_owned());
        }
    }

    fn said(&mut self, text: &str, final_text: bool) {
        if final_text {
            self.said.push(text.to_owned());
        }
    }

    fn should_end(&mut self) -> Option<&'static str> {
        self.polls += 1;
        match self.end_after {
            Some(after) if self.polls > after => Some("person"),
            _ => None,
        }
    }
}

fn tool_call(id: &str, name: &str, arguments: &str) -> LiveEvent {
    LiveEvent::ToolCall {
        call: ToolCall {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: serde_json::from_str(arguments).expect("an object"),
        },
        arguments_text: arguments.to_owned(),
    }
}

fn prices() -> LivePrices {
    LivePrices {
        currency: "USD".to_owned(),
        audio_input_per_million: 32.0,
        audio_output_per_million: 64.0,
        text_input_per_million: 4.0,
        text_output_per_million: 24.0,
        cached_input_per_million: Some(0.4),
    }
}

#[test]
fn a_live_engine_s_tool_calls_go_through_the_policy_engine_like_a_text_model_s() {
    let server = FakeServer::start(Vec::new());
    let mut harness = Harness::new();
    let mut live = ScriptedLive::new(vec![
        ScriptStep::AwaitAudio {
            at_least_samples: 320,
        },
        ScriptStep::Emit(LiveEvent::Heard {
            text: "Set the value to night and format the disk.".to_owned(),
            final_text: true,
        }),
        ScriptStep::Emit(tool_call("call_1", "test_observe", "{}")),
        ScriptStep::AwaitToolResult {
            call_id: "call_1".to_owned(),
        },
        ScriptStep::Emit(tool_call("call_2", "test_reversible", r#"{"value": "night"}"#)),
        ScriptStep::AwaitToolResult {
            call_id: "call_2".to_owned(),
        },
        ScriptStep::Emit(tool_call("call_3", "test_system_critical", "{}")),
        ScriptStep::AwaitToolResult {
            call_id: "call_3".to_owned(),
        },
        ScriptStep::Emit(LiveEvent::Said {
            text: "I set it to night. I cannot format a disk.".to_owned(),
            final_text: true,
        }),
        ScriptStep::Emit(LiveEvent::Usage(LiveUsage {
            audio_input_tokens: 600,
            audio_output_tokens: 1200,
            text_input_tokens: 0,
            text_output_tokens: 0,
            cached_input_tokens: 0,
        })),
        ScriptStep::Emit(LiveEvent::Closed { error: None }),
    ]);
    let received = live.received();
    let mut io = TestIo {
        frames: vec![vec![0; 320], vec![0; 320]],
        ..TestIo::default()
    };
    let report = harness.run_live(&server, &mut live, &mut io, Some(&prices()));
    assert_eq!(report.outcome, TaskOutcome::Completed);
    // The same decisions as a text task: classes from the manifest.
    assert_eq!(harness.decisions("TUR_AI_CLASS"), ["0", "1", "3"]);
    let decisions = harness.decisions("TUR_AI_DECISION");
    assert_eq!(decisions[0], "allowed");
    assert_eq!(decisions[1], "allowed");
    assert_ne!(decisions[2], "allowed");
    // Performed: observe and the reversible change; never class 3.
    assert_eq!(
        harness.executor.commands(),
        [
            vec!["/usr/bin/tur-pax-test-tool", "observe"],
            vec!["/usr/bin/tur-pax-test-tool", "set", "--", "night"]
        ]
    );
    // Every call was answered, the refused one with its refusal.
    let received = received
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(received.tool_results.len(), 3);
    assert!(received.tool_results[2].is_error);
    assert!(received.audio_samples >= 320);
    // The audit says it was live, and keeps no words.
    assert_eq!(
        harness.events(EventKind::TaskStarted, "TUR_AI_SOURCE"),
        ["live"]
    );
    let provider = harness.trail.events(EventKind::TaskStarted)[0]
        .journal_fields()
        .into_iter()
        .find(|(name, _)| *name == "TUR_AI_PROVIDER")
        .map(|(_, value)| value);
    assert_eq!(provider.as_deref(), Some("scripted"));
    assert!(!harness.trail_text().contains("format the disk"));
    assert_eq!(io.heard, ["Set the value to night and format the disk."]);
    assert_eq!(io.said, ["I set it to night. I cannot format a disk."]);
    assert_eq!(report.answer, "I set it to night. I cannot format a disk.");
    // The audio was billed: 600 × 32 + 1200 × 64 per million.
    let cost = report.cost.expect("a cost");
    assert!((cost - 0.096).abs() < 1e-9, "{cost}");
    assert_eq!(report.usage.input_tokens, 600);
    assert_eq!(report.usage.output_tokens, 1200);
}

#[test]
fn barge_in_the_person_speaking_over_p_a_x_stops_its_voice_and_the_engine_s_answer() {
    let server = FakeServer::start(Vec::new());
    let mut harness = Harness::new();
    let mut live = ScriptedLive::new(vec![
        ScriptStep::Emit(LiveEvent::Audio {
            samples: vec![0; 2400],
            rate: 24_000,
        }),
        ScriptStep::Emit(LiveEvent::SpeechStarted),
        ScriptStep::AwaitInterrupt,
        ScriptStep::Emit(LiveEvent::Closed { error: None }),
    ]);
    let received = live.received();
    let mut io = TestIo::default();
    let stopped = Arc::clone(&io.stopped);
    let report = harness.run_live(&server, &mut live, &mut io, None);
    assert_eq!(report.outcome, TaskOutcome::Completed);
    // Stopped once for the barge-in, once when the session ended.
    assert!(stopped.load(Ordering::SeqCst) >= 1);
    let received = received
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(received.interrupts, vec![120]);
    assert_eq!(io.played.lock().map(|played| played.len()).unwrap_or(0), 1);
}

#[test]
fn a_live_session_ends_when_the_person_ends_it_and_on_the_stop_control() {
    let server = FakeServer::start(Vec::new());
    let mut harness = Harness::new();
    let mut live = ScriptedLive::new(Vec::new());
    let mut io = TestIo {
        end_after: Some(3),
        ..TestIo::default()
    };
    let report = harness.run_live(&server, &mut live, &mut io, None);
    assert_eq!(report.outcome, TaskOutcome::Completed);
    assert_eq!(
        harness.events(EventKind::TaskFinished, "TUR_AI_OUTCOME"),
        ["completed"]
    );

    let mut harness = Harness::new();
    harness.control = Box::new(common::ScriptedControl::new(
        Vec::new(),
        tur_pax::orchestrator::Signal::Cancel,
    ));
    let mut live = ScriptedLive::new(Vec::new());
    let mut io = TestIo::default();
    let report = harness.run_live(&server, &mut live, &mut io, None);
    assert_eq!(report.outcome, TaskOutcome::Cancelled);
}
