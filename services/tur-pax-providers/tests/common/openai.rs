// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Scripted OpenAI responses, written by hand from the Responses API
//! documentation (`developers.openai.com/api/docs/guides/function-calling`,
//! `.../streaming-responses`, `.../rate-limits`, `.../error-codes`) and the
//! event and item types of the official SDK, read on 2026-11-12. They are not
//! recordings of a real provider.

use serde_json::{Value, json};
use tur_pax_providers::ProviderId;

use super::{ErrorReply, FollowUp, Recorded, TEST_KEY, Wire};

/// A plain answer, with a keep-alive comment and an event type that does not
/// exist yet.
const PLAIN: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_01","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.in_progress
data: {"type":"response.in_progress","sequence_number":1,"response":{"id":"resp_01","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":2,"output_index":0,"item":{"id":"msg_01","type":"message","status":"in_progress","role":"assistant","content":[]}}

event: response.content_part.added
data: {"type":"response.content_part.added","sequence_number":3,"item_id":"msg_01","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}

: keep-alive

event: response.output_text.delta
data: {"type":"response.output_text.delta","sequence_number":4,"item_id":"msg_01","output_index":0,"content_index":0,"delta":"Hello, ","logprobs":[]}

event: response.an_event_added_later
data: {"type":"response.an_event_added_later","sequence_number":5,"payload":{"anything":[1,2,3]}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","sequence_number":6,"item_id":"msg_01","output_index":0,"content_index":0,"delta":"wörld!","logprobs":[]}

event: response.output_text.done
data: {"type":"response.output_text.done","sequence_number":7,"item_id":"msg_01","output_index":0,"content_index":0,"text":"Hello, wörld!","logprobs":[]}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":8,"output_index":0,"item":{"id":"msg_01","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":"Hello, wörld!","annotations":[]}]}}

event: response.completed
data: {"type":"response.completed","sequence_number":9,"response":{"id":"resp_01","object":"response","status":"completed","model":"test-model","output":[],"usage":{"input_tokens":25,"input_tokens_details":{"cached_tokens":0},"output_tokens":15,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":40}}}

"#;

/// A reasoning item, some text, then one function call whose arguments arrive
/// in several fragments.
const TOOL_CALL: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_02","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"id":"rs_01","type":"reasoning","summary":[]}}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":2,"output_index":0,"item":{"id":"rs_01","type":"reasoning","summary":[],"encrypted_content":"ZW5jLW9wZW5haS0x"}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":3,"output_index":1,"item":{"id":"msg_02","type":"message","status":"in_progress","role":"assistant","content":[]}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","sequence_number":4,"item_id":"msg_02","output_index":1,"content_index":0,"delta":"I will unmount it.","logprobs":[]}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":5,"output_index":1,"item":{"id":"msg_02","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":"I will unmount it.","annotations":[]}]}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":6,"output_index":2,"item":{"type":"function_call","id":"fc_01","call_id":"call_01A","name":"storage_unmount","arguments":""}}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","sequence_number":7,"item_id":"fc_01","output_index":2,"delta":"{\"volume_id\":"}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","sequence_number":8,"item_id":"fc_01","output_index":2,"delta":" \"vol"}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","sequence_number":9,"item_id":"fc_01","output_index":2,"delta":"-1\"}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","sequence_number":10,"item_id":"fc_01","output_index":2,"arguments":"{\"volume_id\": \"vol-1\"}"}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":11,"output_index":2,"item":{"type":"function_call","id":"fc_01","call_id":"call_01A","name":"storage_unmount","arguments":"{\"volume_id\": \"vol-1\"}","status":"completed"}}

event: response.completed
data: {"type":"response.completed","sequence_number":12,"response":{"id":"resp_02","object":"response","status":"completed","model":"test-model","output":[],"usage":{"input_tokens":40,"input_tokens_details":{"cached_tokens":0},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":12},"total_tokens":70}}}

"#;

/// Two function calls in one response. The first has no arguments.
const TWO_TOOL_CALLS: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_03","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"function_call","id":"fc_02","call_id":"call_02A","name":"storage_list_volumes","arguments":""}}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","sequence_number":2,"item_id":"fc_02","output_index":0,"delta":"{}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","sequence_number":3,"item_id":"fc_02","output_index":0,"arguments":"{}"}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":4,"output_index":0,"item":{"type":"function_call","id":"fc_02","call_id":"call_02A","name":"storage_list_volumes","arguments":"{}","status":"completed"}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":5,"output_index":1,"item":{"type":"function_call","id":"fc_03","call_id":"call_02B","name":"storage_unmount","arguments":""}}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","sequence_number":6,"item_id":"fc_03","output_index":1,"delta":"{\"volume_id\": \"vo"}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","sequence_number":7,"item_id":"fc_03","output_index":1,"delta":"l-2\"}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","sequence_number":8,"item_id":"fc_03","output_index":1,"arguments":"{\"volume_id\": \"vol-2\"}"}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":9,"output_index":1,"item":{"type":"function_call","id":"fc_03","call_id":"call_02B","name":"storage_unmount","arguments":"{\"volume_id\": \"vol-2\"}","status":"completed"}}

event: response.completed
data: {"type":"response.completed","sequence_number":10,"response":{"id":"resp_03","object":"response","status":"completed","model":"test-model","output":[],"usage":{"input_tokens":40,"input_tokens_details":{"cached_tokens":0},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":70}}}

"#;

/// The model refuses: the message holds a refusal part instead of text.
const REFUSAL: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_04","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"id":"msg_04","type":"message","status":"in_progress","role":"assistant","content":[]}}

event: response.content_part.added
data: {"type":"response.content_part.added","sequence_number":2,"item_id":"msg_04","output_index":0,"content_index":0,"part":{"type":"refusal","refusal":""}}

event: response.refusal.delta
data: {"type":"response.refusal.delta","sequence_number":3,"item_id":"msg_04","output_index":0,"content_index":0,"delta":"I can't help "}

event: response.refusal.delta
data: {"type":"response.refusal.delta","sequence_number":4,"item_id":"msg_04","output_index":0,"content_index":0,"delta":"with that."}

event: response.refusal.done
data: {"type":"response.refusal.done","sequence_number":5,"item_id":"msg_04","output_index":0,"content_index":0,"refusal":"I can't help with that."}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":6,"output_index":0,"item":{"id":"msg_04","type":"message","status":"completed","role":"assistant","content":[{"type":"refusal","refusal":"I can't help with that."}]}}

event: response.completed
data: {"type":"response.completed","sequence_number":7,"response":{"id":"resp_04","object":"response","status":"completed","model":"test-model","output":[],"usage":{"input_tokens":25,"input_tokens_details":{"cached_tokens":0},"output_tokens":8,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":33}}}

"#;

/// The beginning of an answer. The connection is cut after it.
const CUT_SHORT: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_05","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"id":"msg_05","type":"message","status":"in_progress","role":"assistant","content":[]}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_05","output_index":0,"content_index":0,"delta":"Hello, ","logprobs":[]}

event: response.output_text.delta
data: {"type":"response.output_text.delta","sequence_number":3,"item_id":"msg_05","output_in"#;

/// The beginning of an answer, then an error event.
const OVERLOADED_MID_STREAM: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_06","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"id":"msg_06","type":"message","status":"in_progress","role":"assistant","content":[]}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_06","output_index":0,"content_index":0,"delta":"Hello, ","logprobs":[]}

event: error
data: {"type":"error","sequence_number":3,"code":"server_is_overloaded","message":"The model is temporarily overloaded.","param":null}

"#;

/// An answer that stops at the output limit.
const OUTPUT_LIMIT: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_07","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"id":"msg_07","type":"message","status":"in_progress","role":"assistant","content":[]}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_07","output_index":0,"content_index":0,"delta":"Hello, ","logprobs":[]}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":3,"output_index":0,"item":{"id":"msg_07","type":"message","status":"incomplete","role":"assistant","content":[{"type":"output_text","text":"Hello, ","annotations":[]}]}}

event: response.incomplete
data: {"type":"response.incomplete","sequence_number":4,"response":{"id":"resp_07","object":"response","status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"model":"test-model","output":[],"usage":{"input_tokens":25,"input_tokens_details":{"cached_tokens":0},"output_tokens":15,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":40}}}

"#;

/// A function call whose argument text names `volume_id` twice, in the deltas
/// and in the finished item alike. The response then completes.
const REPEATED_KEY: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_08","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"function_call","id":"fc_08","call_id":"call_08A","name":"storage_unmount","arguments":""}}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","sequence_number":2,"item_id":"fc_08","output_index":0,"delta":"{\"volume_id\": \"vol-1\","}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","sequence_number":3,"item_id":"fc_08","output_index":0,"delta":" \"volume_id\": \"vol-2\"}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","sequence_number":4,"item_id":"fc_08","output_index":0,"arguments":"{\"volume_id\": \"vol-1\", \"volume_id\": \"vol-2\"}"}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":5,"output_index":0,"item":{"type":"function_call","id":"fc_08","call_id":"call_08A","name":"storage_unmount","arguments":"{\"volume_id\": \"vol-1\", \"volume_id\": \"vol-2\"}","status":"completed"}}

event: response.completed
data: {"type":"response.completed","sequence_number":6,"response":{"id":"resp_08","object":"response","status":"completed","model":"test-model","output":[],"usage":{"input_tokens":40,"input_tokens_details":{"cached_tokens":0},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":70}}}

"#;

/// A function call that arrives only as a finished item: no item was
/// announced and no argument delta was streamed.
const WHOLE_ARGUMENTS: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_09","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":1,"output_index":0,"item":{"type":"function_call","id":"fc_09","call_id":"call_09A","name":"storage_unmount","arguments":"{\"volume_id\":\"vol-1\",\"options\":{\"force\":false}}","status":"completed"}}

event: response.completed
data: {"type":"response.completed","sequence_number":2,"response":{"id":"resp_09","object":"response","status":"completed","model":"test-model","output":[],"usage":{"input_tokens":40,"input_tokens_details":{"cached_tokens":0},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":70}}}

"#;

/// As [`WHOLE_ARGUMENTS`], with `force` named twice inside `options`.
const WHOLE_REPEATED_KEY: &str = r#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_10","object":"response","status":"in_progress","model":"test-model","output":[]}}

event: response.output_item.done
data: {"type":"response.output_item.done","sequence_number":1,"output_index":0,"item":{"type":"function_call","id":"fc_10","call_id":"call_10A","name":"storage_unmount","arguments":"{\"volume_id\":\"vol-1\",\"options\":{\"force\":false,\"force\":true}}","status":"completed"}}

event: response.completed
data: {"type":"response.completed","sequence_number":2,"response":{"id":"resp_10","object":"response","status":"completed","model":"test-model","output":[],"usage":{"input_tokens":40,"input_tokens_details":{"cached_tokens":0},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":70}}}

"#;

const RATE_LIMITED: &str = r#"{"error":{"message":"Rate limit reached for requests. Please slow down.","type":"rate_limit_error","param":null,"code":"slow_down"}}"#;

const AUTH_FAILED: &str = r#"{"error":{"message":"Incorrect API key provided.","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}"#;

const REJECTED: &str = r#"{"error":{"message":"Invalid value for 'input'.","type":"invalid_request_error","param":"input","code":null}}"#;

const OVERLOADED: &str = r#"{"error":{"message":"The model is temporarily overloaded.","type":"service_unavailable_error","param":null,"code":"server_is_overloaded"}}"#;

/// The reasoning item of [`TOOL_CALL`], as it must be sent back.
fn opaque() -> Value {
    json!({ "id": "rs_01", "type": "reasoning", "summary": [], "encrypted_content": "ZW5jLW9wZW5haS0x" })
}

fn check_request(request: &Recorded) {
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/responses");
    let bearer = format!("Bearer {TEST_KEY}");
    assert_eq!(request.header("authorization"), Some(bearer.as_str()));
    assert_eq!(request.header("x-api-key"), None);
    let body = request.json();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["max_output_tokens"], 256);
    assert_eq!(body["stream"], true);
    // The provider is not to keep the conversation.
    assert_eq!(body["store"], false);
    assert_eq!(body["instructions"], "You are P@x.");
    assert_eq!(
        body["input"],
        json!([{ "role": "user", "content": [{ "type": "input_text", "text": "Hello" }] }])
    );
}

fn check_follow_up(body: &Value, expected: &FollowUp) {
    // The instructions are the system text and nothing else.
    assert_eq!(body["instructions"], expected.system);
    let input = body["input"].as_array().expect("input");
    assert_eq!(input.len(), 5);

    // The reasoning item goes back unchanged, before the call it led to.
    assert_eq!(input[1], opaque());
    assert_eq!(
        input[2],
        json!({ "role": "assistant", "content": "I will unmount it." })
    );
    assert_eq!(input[3]["type"], "function_call");
    assert_eq!(input[3]["call_id"], expected.call_id);
    assert_eq!(input[3]["name"], "storage_unmount");
    let arguments: Value =
        serde_json::from_str(input[3]["arguments"].as_str().expect("a JSON string")).unwrap();
    assert_eq!(arguments, json!({ "volume_id": "vol-1" }));

    // The untrusted tool output is a function_call_output item.
    assert_eq!(
        input[4],
        json!({
            "type": "function_call_output",
            "call_id": expected.call_id,
            "output": expected.untrusted
        })
    );
}

/// The OpenAI scripts and checks.
pub fn wire() -> Wire {
    Wire {
        provider: ProviderId::OpenAi,
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
