// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Scripted Google responses, written by hand from the Interactions API
//! documentation (`aistudio.google.com/docs/streaming`,
//! `.../docs/function-calling`, `.../docs/api-errors`,
//! `.../api-reference/interactions`), read on 2026-11-12. They are not
//! recordings of a real provider.

use serde_json::{Value, json};
use tur_pax_providers::ProviderId;

use super::{ErrorReply, FollowUp, Recorded, TEST_KEY, Wire};

/// A plain answer, with a keep-alive comment, an event type that does not
/// exist yet, and the closing `[DONE]` marker.
const PLAIN: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test01","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: interaction.status_update
data: {"interaction_id":"v1_test01","status":"in_progress","event_type":"interaction.status_update"}

event: step.start
data: {"index":0,"step":{"type":"model_output"},"event_type":"step.start"}

: keep-alive

event: step.delta
data: {"index":0,"delta":{"text":"Hello, ","type":"text"},"event_type":"step.delta"}

event: step.an_event_added_later
data: {"index":0,"payload":{"anything":[1,2,3]},"event_type":"step.an_event_added_later"}

event: step.delta
data: {"index":0,"delta":{"text":"wörld!","type":"text"},"event_type":"step.delta"}

event: step.stop
data: {"index":0,"event_type":"step.stop"}

event: interaction.completed
data: {"interaction":{"id":"v1_test01","status":"completed","usage":{"total_tokens":40,"total_input_tokens":25,"input_tokens_by_modality":[{"modality":"text","tokens":25}],"total_cached_tokens":0,"total_output_tokens":15,"total_tool_use_tokens":0,"total_thought_tokens":0},"object":"interaction","model":"test-model"},"event_type":"interaction.completed"}

event: done
data: [DONE]

"#;

/// A thought step, some text, then one function call whose arguments arrive
/// in several fragments.
const TOOL_CALL: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test02","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: step.start
data: {"index":0,"step":{"type":"thought"},"event_type":"step.start"}

event: step.delta
data: {"index":0,"delta":{"signature":"c2lnLWdvb2dsZS0x","type":"thought_signature"},"event_type":"step.delta"}

event: step.stop
data: {"index":0,"event_type":"step.stop"}

event: step.start
data: {"index":1,"step":{"type":"model_output"},"event_type":"step.start"}

event: step.delta
data: {"index":1,"delta":{"text":"I will unmount it.","type":"text"},"event_type":"step.delta"}

event: step.stop
data: {"index":1,"event_type":"step.stop"}

event: step.start
data: {"index":2,"step":{"id":"fc_g01A","type":"function_call","name":"storage_unmount","arguments":{}},"event_type":"step.start"}

event: step.delta
data: {"index":2,"delta":{"arguments":"{\"volume_id\":","type":"arguments_delta"},"event_type":"step.delta"}

event: step.delta
data: {"index":2,"delta":{"arguments":" \"vol","type":"arguments_delta"},"event_type":"step.delta"}

event: step.delta
data: {"index":2,"delta":{"arguments":"-1\"}","type":"arguments_delta"},"event_type":"step.delta"}

event: step.stop
data: {"index":2,"event_type":"step.stop"}

event: interaction.completed
data: {"interaction":{"id":"v1_test02","status":"requires_action","usage":{"total_tokens":70,"total_input_tokens":40,"input_tokens_by_modality":[{"modality":"text","tokens":40}],"total_cached_tokens":0,"total_output_tokens":18,"total_tool_use_tokens":0,"total_thought_tokens":12},"object":"interaction","model":"test-model"},"event_type":"interaction.completed"}

event: done
data: [DONE]

"#;

/// Two function calls in one response. The first has no arguments.
const TWO_TOOL_CALLS: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test03","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: step.start
data: {"index":0,"step":{"id":"fc_g02A","type":"function_call","name":"storage_list_volumes","arguments":{}},"event_type":"step.start"}

event: step.delta
data: {"index":0,"delta":{"arguments":"{}","type":"arguments_delta"},"event_type":"step.delta"}

event: step.stop
data: {"index":0,"event_type":"step.stop"}

event: step.start
data: {"index":1,"step":{"id":"fc_g02B","type":"function_call","name":"storage_unmount","arguments":{}},"event_type":"step.start"}

event: step.delta
data: {"index":1,"delta":{"arguments":"{\"volume_id\": \"vo","type":"arguments_delta"},"event_type":"step.delta"}

event: step.delta
data: {"index":1,"delta":{"arguments":"l-2\"}","type":"arguments_delta"},"event_type":"step.delta"}

event: step.stop
data: {"index":1,"event_type":"step.stop"}

event: interaction.completed
data: {"interaction":{"id":"v1_test03","status":"requires_action","usage":{"total_tokens":70,"total_input_tokens":40,"total_cached_tokens":0,"total_output_tokens":30,"total_tool_use_tokens":0,"total_thought_tokens":0},"object":"interaction","model":"test-model"},"event_type":"interaction.completed"}

event: done
data: [DONE]

"#;

/// Generation is blocked for safety: an error event with a "generation
/// blocked" code.
const REFUSAL: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test04","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: interaction.status_update
data: {"interaction_id":"v1_test04","status":"in_progress","event_type":"interaction.status_update"}

event: error
data: {"error":{"message":"Safety violations (harmful content) blocked the request.","code":"safety"},"event_type":"error"}

"#;

/// The beginning of an answer. The connection is cut after it.
const CUT_SHORT: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test05","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: step.start
data: {"index":0,"step":{"type":"model_output"},"event_type":"step.start"}

event: step.delta
data: {"index":0,"delta":{"text":"Hello, ","type":"text"},"event_type":"step.delta"}

event: step.delta
data: {"index":0,"delta":{"text":"wör"#;

/// The beginning of an answer, then an error event.
const OVERLOADED_MID_STREAM: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test06","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: step.start
data: {"index":0,"step":{"type":"model_output"},"event_type":"step.start"}

event: step.delta
data: {"index":0,"delta":{"text":"Hello, ","type":"text"},"event_type":"step.delta"}

event: error
data: {"error":{"message":"The service is temporarily overloaded.","code":"service_unavailable"},"event_type":"error"}

"#;

/// An answer that stops at the output limit.
const OUTPUT_LIMIT: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test07","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: step.start
data: {"index":0,"step":{"type":"model_output"},"event_type":"step.start"}

event: step.delta
data: {"index":0,"delta":{"text":"Hello, ","type":"text"},"event_type":"step.delta"}

event: step.stop
data: {"index":0,"event_type":"step.stop"}

event: interaction.completed
data: {"interaction":{"id":"v1_test07","status":"incomplete","usage":{"total_tokens":40,"total_input_tokens":25,"total_cached_tokens":0,"total_output_tokens":15,"total_tool_use_tokens":0,"total_thought_tokens":0},"object":"interaction","model":"test-model"},"event_type":"interaction.completed"}

event: done
data: [DONE]

"#;

/// A function call whose streamed argument text names `volume_id` twice. The
/// interaction then asks for the action as if nothing were wrong.
const REPEATED_KEY: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test08","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: step.start
data: {"index":0,"step":{"id":"fc_g08A","type":"function_call","name":"storage_unmount","arguments":{}},"event_type":"step.start"}

event: step.delta
data: {"index":0,"delta":{"arguments":"{\"volume_id\": \"vol-1\",","type":"arguments_delta"},"event_type":"step.delta"}

event: step.delta
data: {"index":0,"delta":{"arguments":" \"volume_id\": \"vol-2\"}","type":"arguments_delta"},"event_type":"step.delta"}

event: step.stop
data: {"index":0,"event_type":"step.stop"}

event: interaction.completed
data: {"interaction":{"id":"v1_test08","status":"requires_action","usage":{"total_tokens":70,"total_input_tokens":40,"total_cached_tokens":0,"total_output_tokens":30,"total_tool_use_tokens":0,"total_thought_tokens":0},"object":"interaction","model":"test-model"},"event_type":"interaction.completed"}

event: done
data: [DONE]

"#;

/// A function call whose step starts with its arguments complete, as a JSON
/// object and not as text, and has no delta.
const WHOLE_ARGUMENTS: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test09","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: step.start
data: {"index":0,"step":{"id":"fc_g09A","type":"function_call","name":"storage_unmount","arguments":{"volume_id":"vol-1","options":{"force":false}}},"event_type":"step.start"}

event: step.stop
data: {"index":0,"event_type":"step.stop"}

event: interaction.completed
data: {"interaction":{"id":"v1_test09","status":"requires_action","usage":{"total_tokens":70,"total_input_tokens":40,"total_cached_tokens":0,"total_output_tokens":30,"total_tool_use_tokens":0,"total_thought_tokens":0},"object":"interaction","model":"test-model"},"event_type":"interaction.completed"}

event: done
data: [DONE]

"#;

/// As [`WHOLE_ARGUMENTS`], with `force` named twice inside `options`.
const WHOLE_REPEATED_KEY: &str = r#"event: interaction.created
data: {"interaction":{"id":"v1_test10","status":"in_progress","object":"interaction","model":"test-model"},"event_type":"interaction.created"}

event: step.start
data: {"index":0,"step":{"id":"fc_g10A","type":"function_call","name":"storage_unmount","arguments":{"volume_id":"vol-1","options":{"force":false,"force":true}}},"event_type":"step.start"}

event: step.stop
data: {"index":0,"event_type":"step.stop"}

event: interaction.completed
data: {"interaction":{"id":"v1_test10","status":"requires_action","usage":{"total_tokens":70,"total_input_tokens":40,"total_cached_tokens":0,"total_output_tokens":30,"total_tool_use_tokens":0,"total_thought_tokens":0},"object":"interaction","model":"test-model"},"event_type":"interaction.completed"}

event: done
data: [DONE]

"#;

const RATE_LIMITED: &str = r#"{"error":{"code":"rate_limit_exceeded","message":"You have exceeded the per-minute request limit."}}"#;

const AUTH_FAILED: &str = r#"{"error":{"code":"authentication","message":"The API key is missing, invalid, or expired."}}"#;

const REJECTED: &str = r#"{"error":{"code":"invalid_request","message":"The value 'invalid_tool_type_xyz' is not supported for 'type' at 'tools[0]'."}}"#;

const OVERLOADED: &str = r#"{"error":{"code":"service_unavailable","message":"The service is temporarily overloaded or down."}}"#;

/// The thought step of [`TOOL_CALL`], as it must be sent back.
fn opaque() -> Value {
    json!({ "type": "thought", "signature": "c2lnLWdvb2dsZS0x" })
}

fn check_request(request: &Recorded) {
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1beta/interactions");
    assert_eq!(request.header("x-goog-api-key"), Some(TEST_KEY));
    assert_eq!(request.header("authorization"), None);
    let body = request.json();
    assert_eq!(body["model"], "test-model");
    assert_eq!(
        body["generation_config"],
        json!({ "max_output_tokens": 256 })
    );
    assert_eq!(body["stream"], true);
    // The provider is not to keep the conversation.
    assert_eq!(body["store"], false);
    assert_eq!(body["system_instruction"], "You are P@x.");
    assert_eq!(
        body["input"],
        json!([{ "type": "user_input", "content": [{ "type": "text", "text": "Hello" }] }])
    );
}

fn check_follow_up(body: &Value, expected: &FollowUp) {
    // The instructions are the system text and nothing else.
    assert_eq!(body["system_instruction"], expected.system);
    let input = body["input"].as_array().expect("input");
    assert_eq!(input.len(), 5);

    // The model's steps go back as they came, the thought step first.
    assert_eq!(input[1], opaque());
    assert_eq!(
        input[2],
        json!({ "type": "model_output", "content": [{ "type": "text", "text": "I will unmount it." }] })
    );
    assert_eq!(
        input[3],
        json!({
            "type": "function_call",
            "id": expected.call_id,
            "name": "storage_unmount",
            "arguments": { "volume_id": "vol-1" }
        })
    );

    // The untrusted tool output is a function_result step.
    assert_eq!(
        input[4],
        json!({
            "type": "function_result",
            "name": "storage_unmount",
            "call_id": expected.call_id,
            "result": [{ "type": "text", "text": expected.untrusted }]
        })
    );
}

/// The Google scripts and checks.
pub fn wire() -> Wire {
    Wire {
        provider: ProviderId::Google,
        plain: PLAIN,
        tool_call: TOOL_CALL,
        two_tool_calls: TWO_TOOL_CALLS,
        refusal: REFUSAL,
        cut_short: CUT_SHORT,
        overloaded_mid_stream: OVERLOADED_MID_STREAM,
        output_limit: OUTPUT_LIMIT,
        repeated_key: REPEATED_KEY,
        whole_arguments: WHOLE_ARGUMENTS,
        whole_repeated_key: WHOLE_REPEATED_KEY,
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
            status: 503,
            body: OVERLOADED,
        },
        opaque,
        check_request,
        check_follow_up,
    }
}
