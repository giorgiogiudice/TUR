// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Scripted Chat Completions responses of an OpenAI-compatible service,
//! written by hand from OpenAI's Chat Completions reference, its
//! function-calling guide (`?api-mode=chat`), the chunk types of the official
//! Python SDK (`src/openai/types/chat/chat_completion_chunk.py`), and
//! DeepSeek's API documentation (`api-docs.deepseek.com`: the
//! `reasoning_content` field, usage on the last chunk, `: keep-alive`
//! comments, its error codes), all read on 2026-11-12. They are not
//! recordings of a real service: no request was sent to one.

use serde_json::{Value, json};
use tur_pax_providers::ProviderId;

use super::{ErrorReply, FollowUp, Recorded, TEST_KEY, Wire};

/// A plain answer: a role chunk, a keep-alive comment, a field added later
/// (`obfuscation`), a finish chunk, then the usage chunk with no choices that
/// `stream_options.include_usage` asks for.
const PLAIN: &str = r#"data: {"id":"chatcmpl-01","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"","refusal":null},"finish_reason":null}],"usage":null}

: keep-alive

data: {"id":"chatcmpl-01","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"content":"Hello, "},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-01","object":"chat.completion.chunk","created":1794477600,"model":"test-model","obfuscation":"q7Xz","choices":[{"index":0,"delta":{"content":"wörld!"},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-01","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":null}

data: {"id":"chatcmpl-01","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[],"usage":{"prompt_tokens":25,"completion_tokens":15,"total_tokens":40,"prompt_tokens_details":{"cached_tokens":0},"completion_tokens_details":{"reasoning_tokens":0}}}

data: [DONE]

"#;

/// DeepSeek's thinking mode: reasoning in `reasoning_content`, then the
/// answer, then one call whose argument text is cut inside a key and inside
/// a value. The usage rides on the last chunk, with the finish, as DeepSeek
/// documents.
const TOOL_CALL: &str = r#"data: {"id":"chatcmpl-02","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":null,"reasoning_content":"The person "},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-02","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"content":null,"reasoning_content":"wants it unmounted."},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-02","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"content":"I will unmount it."},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-02","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_01A","type":"function","function":{"name":"storage_unmount","arguments":""}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-02","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"volu"}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-02","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"me_id\": \"vo"}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-02","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"l-1\"}"}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-02","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"content":""},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":40,"completion_tokens":30,"total_tokens":70,"prompt_tokens_details":{"cached_tokens":0},"prompt_cache_hit_tokens":0,"prompt_cache_miss_tokens":40,"completion_tokens_details":{"reasoning_tokens":12}}}

data: [DONE]

"#;

/// Two calls whose deltas interleave. The first has no arguments at all.
const TWO_TOOL_CALLS: &str = r#"data: {"id":"chatcmpl-03","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":null,"tool_calls":[{"index":0,"id":"call_02A","type":"function","function":{"name":"storage_list_volumes","arguments":""}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-03","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_02B","type":"function","function":{"name":"storage_unmount","arguments":"{\"volume_id\": \"vo"}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-03","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"l-2\"}"}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-03","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":null}

data: {"id":"chatcmpl-03","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[],"usage":{"prompt_tokens":40,"completion_tokens":30,"total_tokens":70}}

data: [DONE]

"#;

/// The model refuses in the `refusal` field.
const REFUSAL: &str = r#"data: {"id":"chatcmpl-04","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":null,"refusal":"I can't help "},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-04","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"refusal":"with that."},"finish_reason":"stop"}],"usage":null}

data: {"id":"chatcmpl-04","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[],"usage":{"prompt_tokens":25,"completion_tokens":8,"total_tokens":33}}

data: [DONE]

"#;

/// The beginning of an answer. The connection is cut after it.
const CUT_SHORT: &str = r#"data: {"id":"chatcmpl-05","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello, "},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-05","object":"chat.completion.chunk","created":17944"#;

/// The beginning of an answer, then an error chunk, as the reference says an
/// error after streaming began can arrive.
const OVERLOADED_MID_STREAM: &str = r#"data: {"id":"chatcmpl-06","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello, "},"finish_reason":null}],"usage":null}

data: {"error":{"message":"The model is temporarily overloaded.","type":"server_error","param":null,"code":"server_is_overloaded"}}

"#;

/// An answer that stops at the output limit.
const OUTPUT_LIMIT: &str = r#"data: {"id":"chatcmpl-07","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello, "},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-07","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"length"}],"usage":null}

data: {"id":"chatcmpl-07","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[],"usage":{"prompt_tokens":25,"completion_tokens":15,"total_tokens":40}}

data: [DONE]

"#;

/// A call whose streamed argument text names `volume_id` twice; the
/// response then ends normally.
const REPEATED_KEY: &str = r#"data: {"id":"chatcmpl-08","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":null,"tool_calls":[{"index":0,"id":"call_08A","type":"function","function":{"name":"storage_unmount","arguments":"{\"volume_id\": \"vol-1\","}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-08","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":" \"volume_id\": \"vol-2\"}"}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-08","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":40,"completion_tokens":30,"total_tokens":70}}

data: [DONE]

"#;

/// A call that arrives in one delta, name and complete arguments together,
/// as xAI documents for its streams ("returned in whole in a single chunk").
const WHOLE_ARGUMENTS: &str = r#"data: {"id":"chatcmpl-09","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":null,"tool_calls":[{"index":0,"id":"call_09A","type":"function","function":{"name":"storage_unmount","arguments":"{\"volume_id\":\"vol-1\",\"options\":{\"force\":false}}"}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-09","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":40,"completion_tokens":30,"total_tokens":70}}

data: [DONE]

"#;

/// As [`WHOLE_ARGUMENTS`], with `force` named twice inside `options`.
const WHOLE_REPEATED_KEY: &str = r#"data: {"id":"chatcmpl-10","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":null,"tool_calls":[{"index":0,"id":"call_10A","type":"function","function":{"name":"storage_unmount","arguments":"{\"volume_id\":\"vol-1\",\"options\":{\"force\":false,\"force\":true}}"}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-10","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":40,"completion_tokens":30,"total_tokens":70}}

data: [DONE]

"#;

/// The model calls the tool of the tool probe.
pub const PROBE_CALLED: &str = r#"data: {"id":"chatcmpl-11","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":null,"tool_calls":[{"index":0,"id":"call_11A","type":"function","function":{"name":"tur_probe","arguments":"{\"word\":"}}]},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-11","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":" \"ok\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":60,"completion_tokens":9,"total_tokens":69}}

data: [DONE]

"#;

/// A plain answer from a service that sends no usage at all, though asked.
pub const NO_USAGE: &str = r#"data: {"id":"chatcmpl-12","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello, wörld!"},"finish_reason":null}]}

data: {"id":"chatcmpl-12","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]

"#;

/// The beginning of an answer from a service that sends no usage, cut off.
pub const NO_USAGE_CUT: &str = r#"data: {"id":"chatcmpl-13","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello, "},"finish_reason":null}]}

data: {"id":"chatcmpl-13","object":"chat.completion.chunk","created":17944"#;

/// The service's content filter stops the answer.
pub const CONTENT_FILTER: &str = r#"data: {"id":"chatcmpl-14","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Here is how to "},"finish_reason":null}]}

data: {"id":"chatcmpl-14","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{},"finish_reason":"content_filter"}],"usage":{"prompt_tokens":25,"completion_tokens":4,"total_tokens":29}}

data: [DONE]

"#;

/// Ollama's form, from its source: reasoning in `reasoning`, a call whose
/// arguments arrive whole with an index, then a separate finish chunk.
pub const OLLAMA_STYLE: &str = r#"data: {"id":"chatcmpl-15","object":"chat.completion.chunk","created":1794477600,"model":"test-model","system_fingerprint":"fp_ollama","choices":[{"index":0,"delta":{"role":"assistant","content":"","reasoning":"Look at the volumes."},"finish_reason":null}]}

data: {"id":"chatcmpl-15","object":"chat.completion.chunk","created":1794477600,"model":"test-model","system_fingerprint":"fp_ollama","choices":[{"index":0,"delta":{"role":"assistant","content":"","tool_calls":[{"id":"call_abc","index":0,"type":"function","function":{"name":"storage_list_volumes","arguments":"{}"}}]},"finish_reason":null}]}

data: {"id":"chatcmpl-15","object":"chat.completion.chunk","created":1794477600,"model":"test-model","system_fingerprint":"fp_ollama","choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":"tool_calls"}]}

data: {"id":"chatcmpl-15","object":"chat.completion.chunk","created":1794477600,"model":"test-model","system_fingerprint":"fp_ollama","choices":[],"usage":{"prompt_tokens":31,"completion_tokens":12,"total_tokens":43}}

data: [DONE]

"#;

/// OpenRouter's form: a processing comment, the finish repeated on the
/// usage chunk, which has one content-free choice.
pub const OPENROUTER_STYLE: &str = r#": OPENROUTER PROCESSING

data: {"id":"gen-16","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}

data: {"id":"gen-16","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"content":""},"finish_reason":"stop"}]}

data: {"id":"gen-16","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"content":""},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":4}}}

data: [DONE]

"#;

/// OpenRouter's mid-stream error: status 200, a chunk with `error` and the
/// finish reason `error`.
pub const ERROR_CHUNK_WITH_STATUS: &str = r#"data: {"id":"gen-17","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}

data: {"id":"gen-17","object":"chat.completion.chunk","created":1794477600,"model":"test-model","error":{"code":502,"message":"Provider disconnected"},"choices":[{"index":0,"delta":{"content":""},"finish_reason":"error"}]}

data: [DONE]

"#;

/// The answer ends with DeepSeek's `insufficient_system_resource`.
pub const INSUFFICIENT_RESOURCE: &str = r#"data: {"id":"chatcmpl-18","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-18","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"content":""},"finish_reason":"insufficient_system_resource"}],"usage":{"prompt_tokens":25,"completion_tokens":1,"total_tokens":26}}

data: [DONE]

"#;

/// A finish reason, and then the stream closes without `[DONE]`.
pub const NO_DONE: &str = r#"data: {"id":"chatcmpl-19","object":"chat.completion.chunk","created":1794477600,"model":"test-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":1,"total_tokens":6}}

"#;

const RATE_LIMITED: &str = r#"{"error":{"message":"Rate limit reached for requests.","type":"rate_limit_error","param":null,"code":"rate_limit_exceeded"}}"#;

const AUTH_FAILED: &str = r#"{"error":{"message":"Authentication Fails, Your api key is invalid","type":"authentication_error","param":null,"code":"invalid_request_error"}}"#;

const REJECTED: &str = r#"{"error":{"message":"Invalid value for 'messages'.","type":"invalid_request_error","param":"messages","code":null}}"#;

const OVERLOADED: &str = r#"{"error":{"message":"Server overloaded, please retry shortly.","type":"server_error","param":null,"code":null}}"#;

/// The reasoning of [`TOOL_CALL`], as it is kept.
fn opaque() -> Value {
    json!({ "reasoning_content": "The person wants it unmounted." })
}

fn check_request(request: &Recorded) {
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/chat/completions");
    let bearer = format!("Bearer {TEST_KEY}");
    assert_eq!(request.header("authorization"), Some(bearer.as_str()));
    assert_eq!(request.header("x-api-key"), None);
    let body = request.json();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["max_tokens"], 256);
    assert_eq!(body["stream"], true);
    // Usage is asked for: the day's limit rests on it.
    assert_eq!(body["stream_options"], json!({ "include_usage": true }));
    assert_eq!(
        body["messages"],
        json!([
            { "role": "system", "content": "You are P@x." },
            { "role": "user", "content": "Hello" }
        ])
    );
    for absent in ["store", "n", "parallel_tool_calls", "reasoning_effort"] {
        assert!(body.get(absent).is_none(), "{absent} was sent");
    }
}

fn check_follow_up(body: &Value, expected: &FollowUp) {
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 4, "{messages:?}");
    // The system text is the system message and nothing else.
    assert_eq!(
        messages[0],
        json!({ "role": "system", "content": expected.system })
    );
    // The assistant turn as it came: its text and its call. Its reasoning is
    // not sent back unless the configuration says the service needs it.
    let assistant = &messages[2];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(assistant["content"], "I will unmount it.");
    assert!(assistant.get("reasoning_content").is_none());
    let call = &assistant["tool_calls"][0];
    assert_eq!(call["id"], expected.call_id);
    assert_eq!(call["type"], "function");
    assert_eq!(call["function"]["name"], "storage_unmount");
    let arguments: Value =
        serde_json::from_str(call["function"]["arguments"].as_str().expect("a JSON string"))
            .unwrap();
    assert_eq!(arguments, json!({ "volume_id": "vol-1" }));
    // The untrusted output is a tool message tied to the call.
    assert_eq!(
        messages[3],
        json!({ "role": "tool", "tool_call_id": expected.call_id, "content": expected.untrusted })
    );
}

/// The Chat Completions scripts and checks.
pub fn wire() -> Wire {
    Wire {
        provider: ProviderId::OpenAiChat,
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
