// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What the OpenAI-compatible family must do beyond the shared conformance
//! suite: the services that speak it differ in where they put reasoning and
//! usage, and some send no usage at all.
//!
//! Every script is in `tests/common/openai_chat.rs`, written by hand from the
//! services' documentation; no test talks to a real service.

mod common;

use std::time::Duration;

use serde_json::{Value, json};
use tur_pax_providers::{
    ErrorKind, FinishReason, Message, OpaquePart, Part, ProviderId, SpendMeter, StreamEvent,
    TaskLimit, ToolDefinition, ToolResult, Turn, Usage, connect_without_key,
};

use common::openai_chat::{
    CONTENT_FILTER, ERROR_CHUNK_WITH_STATUS, INSUFFICIENT_RESOURCE, NO_DONE, NO_USAGE,
    NO_USAGE_CUT, OLLAMA_STYLE, OPENROUTER_STYLE,
};
use common::{FakeServer, Reply, SYSTEM, config_with, hello, provider, run};

const CHAT: ProviderId = ProviderId::OpenAiChat;

fn tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "storage_list_volumes",
            "List detected volumes.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        )
        .expect("a valid tool"),
        ToolDefinition::new(
            "storage_unmount",
            "Unmount a volume.",
            json!({
                "type": "object",
                "properties": { "volume_id": { "type": "string" } },
                "required": ["volume_id"]
            }),
        )
        .expect("a valid tool"),
    ]
}

fn finish(events: &[StreamEvent]) -> &FinishReason {
    let finishes = events
        .iter()
        .filter(|event| matches!(event, StreamEvent::Finish(_)))
        .count();
    assert_eq!(finishes, 1, "{events:?}");
    match events.last() {
        Some(StreamEvent::Finish(reason)) => reason,
        other => panic!("the last event is not a finish: {other:?}"),
    }
}

fn last_usage(events: &[StreamEvent]) -> Option<Usage> {
    events.iter().rev().find_map(|event| match event {
        StreamEvent::Usage(usage) => Some(*usage),
        _ => None,
    })
}

fn text(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// A script of one chunk per delta, ending as the reference says.
fn stream_of(deltas: &[Value], finish_reason: &str) -> String {
    let mut text = String::new();
    for delta in deltas {
        let chunk = json!({
            "id": "chatcmpl-x", "object": "chat.completion.chunk", "created": 1_794_477_600,
            "model": "test-model",
            "choices": [{ "index": 0, "delta": delta, "finish_reason": null }],
            "usage": null
        });
        text.push_str(&format!("data: {chunk}\n\n"));
    }
    let end = json!({
        "id": "chatcmpl-x", "object": "chat.completion.chunk", "created": 1_794_477_600,
        "model": "test-model",
        "choices": [{ "index": 0, "delta": {}, "finish_reason": finish_reason }],
        "usage": { "prompt_tokens": 40, "completion_tokens": 30, "total_tokens": 70 }
    });
    text.push_str(&format!("data: {end}\n\ndata: [DONE]\n\n"));
    text
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

#[test]
fn reasoning_is_never_text_and_goes_back_only_where_the_service_requires_it() {
    let wire = common::openai_chat::wire();
    for send_back in [false, true] {
        let server = FakeServer::start(vec![
            Reply::stream(wire.tool_call),
            Reply::stream(wire.plain),
        ]);
        let config = config_with(CHAT, &server, |value| {
            value["send_back_reasoning"] = json!(send_back);
        });
        let provider = provider(&config);
        let mut request = config.request(
            Some(SYSTEM.to_owned()),
            vec![Message::user_text("Unmount the volume.")],
            tools(),
        );
        let events: Vec<StreamEvent> = provider.stream(&request).collect();
        // The reasoning is not the answer.
        assert_eq!(text(&events), "I will unmount it.");
        let first = Turn::collect(events);
        assert_eq!(first.text(), "I will unmount it.");
        assert_eq!(
            first.message.content[0],
            Part::Opaque(OpaquePart {
                provider: CHAT,
                data: json!({ "reasoning_content": "The person wants it unmounted." }),
            })
        );
        let call = first.tool_calls()[0].clone();
        request.messages.push(first.message);
        request
            .messages
            .push(Message::user(vec![Part::ToolResult(ToolResult::text(
                call.id, call.name, "done",
            ))]));
        let second = Turn::collect(provider.stream(&request));
        assert_eq!(second.finish, FinishReason::EndTurn);

        let body = server.requests()[1].json();
        let assistant = &body["messages"][2];
        if send_back {
            // DeepSeek: "the reasoning_content must be fully passed back to
            // the API in all subsequent requests" when tools are offered.
            assert_eq!(
                assistant["reasoning_content"],
                "The person wants it unmounted."
            );
        } else {
            assert!(assistant.get("reasoning_content").is_none());
            assert!(!server.requests()[1].body.contains("The person wants"));
        }
        // Never as text of the answer, never as an instruction.
        assert_eq!(assistant["content"], "I will unmount it.");
        assert_eq!(body["messages"][0]["content"], SYSTEM);
    }
}

#[test]
fn the_reasoning_field_of_other_servers_is_kept_the_same_way() {
    let server = FakeServer::start(vec![Reply::stream(OLLAMA_STYLE)]);
    let config = common::config(CHAT, &server);
    let mut request = hello(&config);
    request.tools = tools();
    let events = run(&config, &request);
    assert_eq!(text(&events), "");
    assert_eq!(finish(&events), &FinishReason::ToolUse);
    let turn = Turn::collect(events);
    assert_eq!(
        turn.message.content[0],
        Part::Opaque(OpaquePart {
            provider: CHAT,
            data: json!({ "reasoning": "Look at the volumes." }),
        })
    );
    assert_eq!(turn.tool_calls()[0].name, "storage_list_volumes");
    assert_eq!(turn.usage.input_tokens, 31);
    assert!(!turn.usage.estimated);
}

#[test]
fn tool_call_text_cut_inside_an_escape_and_bytes_cut_inside_a_character() {
    let script = leak(stream_of(
        &[
            json!({ "role": "assistant", "content": null, "tool_calls": [{ "index": 0, "id": "call_x",
                "type": "function", "function": { "name": "storage_unmount", "arguments": "{\"volume_id\": \"a\\" } }] }),
            json!({ "tool_calls": [{ "index": 0, "function": { "arguments": "\"b\\u00f6" } }] }),
            json!({ "tool_calls": [{ "index": 0, "function": { "arguments": "\"}" } }] }),
        ],
        "tool_calls",
    ));
    // One byte at a time: every boundary of the stream is tried.
    let server = FakeServer::start(vec![Reply::Stream {
        body: script.as_bytes().to_vec(),
        piece: 1,
        cut: false,
    }]);
    let config = common::config(CHAT, &server);
    let mut request = hello(&config);
    request.tools = tools();
    let turn = Turn::collect(run(&config, &request));
    assert_eq!(turn.finish, FinishReason::ToolUse);
    assert_eq!(
        Value::Object(turn.tool_calls()[0].arguments.clone()),
        json!({ "volume_id": "a\"bö" })
    );

    let server = FakeServer::start(vec![Reply::Stream {
        body: common::openai_chat::wire().plain.as_bytes().to_vec(),
        piece: 1,
        cut: false,
    }]);
    let config = common::config(CHAT, &server);
    let events = run(&config, &hello(&config));
    assert_eq!(text(&events), "Hello, wörld!");
}

#[test]
fn a_repeated_key_split_across_deltas_at_any_depth_discards_the_response() {
    let script = leak(stream_of(
        &[
            json!({ "role": "assistant", "content": "Two calls.", "tool_calls": [{ "index": 0, "id": "call_a",
                "type": "function", "function": { "name": "storage_list_volumes", "arguments": "{}" } }] }),
            json!({ "tool_calls": [{ "index": 1, "id": "call_b", "type": "function",
                "function": { "name": "storage_unmount", "arguments": "{\"volume_id\": \"v\", \"o\": {\"f\": 1, \"f" } }] }),
            json!({ "tool_calls": [{ "index": 1, "function": { "arguments": "\": 2}}" } }] }),
        ],
        "tool_calls",
    ));
    let server = FakeServer::start(vec![Reply::stream(script)]);
    let config = common::config(CHAT, &server);
    let mut request = hello(&config);
    request.tools = tools();
    let events = run(&config, &request);
    match finish(&events) {
        FinishReason::Error(error) => assert_eq!(error.kind(), ErrorKind::MalformedResponse),
        other => panic!("not discarded: {other:?}"),
    }
    // The unremarkable call was delivered, and goes down with the response:
    // a malformed response is discarded whole by the task loop.
    let turn = Turn::collect(events);
    assert!(turn.finish.error().is_some());
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn a_service_that_sends_no_usage_is_counted_at_a_marked_estimate() {
    let server = FakeServer::start(vec![Reply::stream(NO_USAGE)]);
    let config = common::config(CHAT, &server);
    let events = run(&config, &hello(&config));
    assert_eq!(text(&events), "Hello, wörld!");
    assert_eq!(finish(&events), &FinishReason::EndTurn);
    let usage = last_usage(&events).expect("a usage");
    assert!(usage.estimated);
    // The whole output limit, and the request at two bytes a token.
    assert_eq!(usage.output_tokens, 256);
    let sent = server.requests()[0].body.len() as u64;
    assert_eq!(usage.input_tokens, sent.div_ceil(2));

    // The estimate counts against the limit of a task like any usage.
    let server = FakeServer::start(vec![Reply::stream(NO_USAGE)]);
    let config = common::config(CHAT, &server);
    let mut meter = SpendMeter::new(
        None,
        TaskLimit {
            max_cost: None,
            max_tokens: Some(100),
        },
    );
    let events: Vec<StreamEvent> = meter
        .meter(provider(&config).stream(&hello(&config)))
        .collect();
    match finish(&events) {
        FinishReason::Error(error) => assert_eq!(error.kind(), ErrorKind::SpendLimitReached),
        other => panic!("not stopped: {other:?}"),
    }
    assert!(meter.usage().estimated);

    // Cut off before it ended: still counted.
    let server = FakeServer::start(vec![Reply::cut(NO_USAGE_CUT)]);
    let config = common::config(CHAT, &server);
    let events = run(&config, &hello(&config));
    match finish(&events) {
        FinishReason::Error(error) => assert_eq!(error.kind(), ErrorKind::Network),
        other => panic!("not an error: {other:?}"),
    }
    assert!(last_usage(&events).expect("a usage").estimated);
}

#[test]
fn a_reported_usage_is_taken_as_it_is_wherever_the_service_puts_it() {
    let server = FakeServer::start(vec![Reply::stream(OPENROUTER_STYLE)]);
    let config = common::config(CHAT, &server);
    let events = run(&config, &hello(&config));
    assert_eq!(text(&events), "Hello");
    // The finish is repeated on the usage chunk; it is read once.
    assert_eq!(finish(&events), &FinishReason::EndTurn);
    assert_eq!(
        last_usage(&events),
        Some(Usage {
            input_tokens: 10,
            output_tokens: 2,
            cached_input_tokens: 4,
            estimated: false,
        })
    );
    let usages = events
        .iter()
        .filter(|event| matches!(event, StreamEvent::Usage(_)))
        .count();
    assert_eq!(usages, 1);
}

#[test]
fn the_endings_a_service_can_give() {
    for (script, expected) in [
        (CONTENT_FILTER, ErrorKind::Refusal),
        (ERROR_CHUNK_WITH_STATUS, ErrorKind::Overloaded),
        (INSUFFICIENT_RESOURCE, ErrorKind::Overloaded),
        (NO_DONE, ErrorKind::Network),
    ] {
        let server = FakeServer::start(vec![Reply::stream(script)]);
        let config = common::config(CHAT, &server);
        let events = run(&config, &hello(&config));
        let reason = finish(&events);
        let error = reason.error().expect("not a normal end");
        assert_eq!(error.kind(), expected, "{script}");
        if expected == ErrorKind::Refusal {
            assert!(matches!(reason, FinishReason::Refusal(_)));
        }
        // Not asked again: something was delivered.
        assert_eq!(server.requests().len(), 1);
    }
}

#[test]
fn a_rate_limit_with_retry_after_is_waited_for_then_succeeds() {
    let wire = common::openai_chat::wire();
    let server = FakeServer::start(vec![
        Reply::status_retry_after(429, wire.rate_limited.body, "1"),
        Reply::stream(wire.plain),
    ]);
    let config = config_with(CHAT, &server, |value| {
        value["retry"] = json!({ "max_attempts": 2, "initial_delay_ms": 5, "max_delay_ms": 2000 });
    });
    let started = std::time::Instant::now();
    let events = run(&config, &hello(&config));
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert_eq!(text(&events), "Hello, wörld!");
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn a_server_that_takes_no_key_is_sent_no_credential() {
    let wire = common::openai_chat::wire();
    let server = FakeServer::start(vec![Reply::stream(wire.plain)]);
    let config = common::config(CHAT, &server);
    let provider = connect_without_key(&config).expect("an adapter");
    let events: Vec<StreamEvent> = provider.stream(&hello(&config)).collect();
    assert_eq!(text(&events), "Hello, wörld!");
    let requests = server.requests();
    assert_eq!(requests[0].header("authorization"), None);
    assert_eq!(requests[0].path, "/v1/chat/completions");

    // The companies' interfaces cannot be used without a key.
    let other = config_with(ProviderId::Anthropic, &server, |_| {});
    let error = connect_without_key(&other).err().expect("refused");
    assert_eq!(error.kind(), ErrorKind::ConfigInvalid);
}
