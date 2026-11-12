// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Scripted Anthropic responses, written by hand from the Messages API
//! streaming documentation (`platform.claude.com/docs/en/build-with-claude/streaming`,
//! `.../handling-stop-reasons`, `.../refusals-and-fallback`, `.../api/errors`),
//! read on 2026-11-12. They are not recordings of a real provider.

use serde_json::{Value, json};
use tur_pax_providers::ProviderId;

use super::{ErrorReply, FollowUp, Recorded, TEST_KEY, Wire};

/// A plain answer, with a ping, a keep-alive comment and an event type that
/// does not exist yet.
const PLAIN: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_01","type":"message","role":"assistant","content":[],"model":"test-model","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: ping
data: {"type": "ping"}

: keep-alive

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello, "}}

event: an_event_added_later
data: {"type":"an_event_added_later","payload":{"anything":[1,2,3]}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"wörld!"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":15}}

event: message_stop
data: {"type":"message_stop"}

"#;

/// A thinking block, some text, then one tool call whose arguments arrive in
/// several fragments.
const TOOL_CALL: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_02","type":"message","role":"assistant","content":[],"model":"test-model","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":40,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"c2lnLWFudGhyb3BpYy0x"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"I will unmount it."}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: content_block_start
data: {"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_01A","name":"storage_unmount","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"volume_id\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":" \"vol"}}

event: content_block_delta
data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"-1\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":2}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":30}}

event: message_stop
data: {"type":"message_stop"}

"#;

/// Two tool calls in one response. The first has no arguments.
const TWO_TOOL_CALLS: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_03","type":"message","role":"assistant","content":[],"model":"test-model","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":40,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_02A","name":"storage_list_volumes","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":""}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_02B","name":"storage_unmount","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"volume_id\": \"vo"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"l-2\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":30}}

event: message_stop
data: {"type":"message_stop"}

"#;

/// A refusal by the provider's safety classifier, before any output.
const REFUSAL: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_04","type":"message","role":"assistant","content":[],"model":"test-model","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":0}}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"refusal","stop_sequence":null,"stop_details":{"type":"refusal","category":"cyber","explanation":"This request was declined because it could enable cyber harm."}},"usage":{"output_tokens":0}}

event: message_stop
data: {"type":"message_stop"}

"#;

/// The beginning of an answer. The connection is cut after it.
const CUT_SHORT: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_05","type":"message","role":"assistant","content":[],"model":"test-model","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello, "}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_del"#;

/// The beginning of an answer, then an error event.
const OVERLOADED_MID_STREAM: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_06","type":"message","role":"assistant","content":[],"model":"test-model","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello, "}}

event: error
data: {"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}

"#;

/// An answer that stops at the output limit.
const OUTPUT_LIMIT: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_07","type":"message","role":"assistant","content":[],"model":"test-model","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":25,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello, "}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"max_tokens","stop_sequence":null},"usage":{"output_tokens":15}}

event: message_stop
data: {"type":"message_stop"}

"#;

const RATE_LIMITED: &str = r#"{"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed your organization's rate limit of 50 requests per minute."},"request_id":"req_01"}"#;

const AUTH_FAILED: &str = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"},"request_id":"req_02"}"#;

const REJECTED: &str = r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages: at least one message is required"},"request_id":"req_03"}"#;

const OVERLOADED: &str = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"},"request_id":"req_04"}"#;

/// The thinking block of [`TOOL_CALL`], as it must be sent back.
fn opaque() -> Value {
    json!({ "type": "thinking", "thinking": "", "signature": "c2lnLWFudGhyb3BpYy0x" })
}

fn check_request(request: &Recorded) {
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(request.header("x-api-key"), Some(TEST_KEY));
    assert_eq!(request.header("anthropic-version"), Some("2026-10-21"));
    assert_eq!(request.header("authorization"), None);
    let body = request.json();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["max_tokens"], 256);
    assert_eq!(body["stream"], true);
    assert_eq!(body["system"], "You are P@x.");
    assert_eq!(
        body["messages"],
        json!([{ "role": "user", "content": [{ "type": "text", "text": "Hello" }] }])
    );
}

fn check_follow_up(body: &Value, expected: &FollowUp) {
    // The instructions are the system text and nothing else.
    assert_eq!(body["system"], expected.system);
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 3);

    // The assistant turn goes back with its thinking block first and unchanged.
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(
        messages[1]["content"],
        json!([
            opaque(),
            { "type": "text", "text": "I will unmount it." },
            { "type": "tool_use", "id": expected.call_id, "name": "storage_unmount",
              "input": { "volume_id": "vol-1" } }
        ])
    );

    // The untrusted tool output is a tool_result block, the first block of the
    // user turn that follows the call.
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(
        messages[2]["content"],
        json!([{
            "type": "tool_result",
            "tool_use_id": expected.call_id,
            "content": [{ "type": "text", "text": expected.untrusted }]
        }])
    );
}

/// The Anthropic scripts and checks.
pub fn wire() -> Wire {
    Wire {
        provider: ProviderId::Anthropic,
        plain: PLAIN,
        tool_call: TOOL_CALL,
        two_tool_calls: TWO_TOOL_CALLS,
        refusal: REFUSAL,
        cut_short: CUT_SHORT,
        overloaded_mid_stream: OVERLOADED_MID_STREAM,
        output_limit: OUTPUT_LIMIT,
        rate_limited: ErrorReply {
            status: 429,
            body: RATE_LIMITED,
        },
        auth_failed: ErrorReply {
            status: 401,
            body: AUTH_FAILED,
        },
        rejected: ErrorReply {
            status: 400,
            body: REJECTED,
        },
        overloaded: ErrorReply {
            status: 529,
            body: OVERLOADED,
        },
        opaque,
        check_request,
        check_follow_up,
    }
}
