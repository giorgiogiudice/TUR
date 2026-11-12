// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The Google adapter: the Gemini Interactions API with server-sent events.
//!
//! Written against the documentation at `aistudio.google.com/docs` (where the
//! older `ai.google.dev` documentation now redirects) as read on 2026-11-12.
//! The mapping, and what was and was not verified, is in
//! `docs/architecture/pax-providers.md`.
//!
//! The Interactions API is used because the provider has made it the
//! recommended interface for all new development and calls `generateContent`
//! legacy. It is used statelessly: every request carries the whole
//! conversation and sets `store` to `false`, so the provider keeps no copy of
//! the conversation for later retrieval.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::error::{ErrorKind, ProviderError};
use crate::http::{Auth, kind_for_status};
use crate::model::{
    FinishReason, Image, OpaquePart, Part, ProviderId, Request, ResultContent, Role, StreamEvent,
    Usage, rejected,
};
use crate::provider::{
    Capabilities, Decoder, Endpoint, EventStream, NativeComputerControl, Provider, Shared,
    event_type, parse_data, provider_error,
};
use crate::sse::SseEvent;
use crate::toolcalls::ToolCalls;

/// The API version in the request path.
pub const API_VERSION: &str = "v1beta";

/// The provider's computer-control tool type. Recorded as a capability; this
/// adapter does not drive it.
pub const COMPUTER_CONTROL_TOOL: &str = "computer_use";

/// What this adapter can do.
pub const CAPABILITIES: Capabilities = Capabilities {
    streaming: true,
    tool_calling: true,
    parallel_tool_calls: true,
    image_input: true,
    image_in_tool_results: true,
    native_computer_control: NativeComputerControl {
        provider_tool: Some(COMPUTER_CONTROL_TOOL),
        implemented: false,
    },
    model_discovery: false,
    speech_services: false,
};

pub(crate) const ENDPOINT: Endpoint = Endpoint {
    provider: ProviderId::Google,
    path: "/v1beta/interactions",
    auth: Auth::Header("x-goog-api-key"),
    headers: &[],
    classify,
};

/// The Google adapter. Built by [`crate::provider::connect`].
pub struct GoogleProvider {
    shared: Arc<Shared>,
}

impl GoogleProvider {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        GoogleProvider { shared }
    }
}

impl Provider for GoogleProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Google
    }

    fn capabilities(&self) -> Capabilities {
        CAPABILITIES
    }

    fn stream(&self, request: &Request) -> EventStream {
        self.shared.stream(
            ENDPOINT,
            request_body(request),
            Box::new(GoogleDecoder::default()),
        )
    }
}

/// The JSON body of an Interactions request.
fn request_body(request: &Request) -> Result<Value, ProviderError> {
    let mut input: Vec<Value> = Vec::new();
    for message in &request.messages {
        match message.role {
            Role::User => user_steps(&message.content, &mut input)?,
            Role::Assistant => assistant_steps(&message.content, &mut input)?,
        }
    }
    let mut body = json!({
        "model": request.model,
        "input": input,
        "generation_config": { "max_output_tokens": request.max_output_tokens },
        "stream": true,
        "store": false,
    });
    if let Some(system) = request.system.as_deref()
        && !system.trim().is_empty()
    {
        body["system_instruction"] = json!(system);
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.input_schema(),
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    Ok(body)
}

/// The steps of one user message: a `function_result` step for each tool
/// result, then one `user_input` step with the text and images.
fn user_steps(parts: &[Part], input: &mut Vec<Value>) -> Result<(), ProviderError> {
    let mut content: Vec<Value> = Vec::new();
    for part in parts {
        match part {
            Part::Text { text } => {
                if !text.is_empty() {
                    content.push(json!({ "type": "text", "text": text }));
                }
            }
            Part::Image(image) => content.push(image_content(image)?),
            Part::ToolResult(result) => {
                let mut pieces = Vec::new();
                for piece in &result.content {
                    pieces.push(match piece {
                        ResultContent::Text { text } => json!({ "type": "text", "text": text }),
                        ResultContent::Image(image) => image_content(image)?,
                    });
                }
                let mut step = json!({
                    "type": "function_result",
                    "name": result.name,
                    "call_id": result.call_id,
                    "result": pieces,
                });
                if result.is_error {
                    step["is_error"] = json!(true);
                }
                input.push(step);
            }
            Part::ToolCall(_) | Part::Opaque(_) => {
                return Err(rejected(
                    "a tool call or provider content cannot be part of a user message",
                ));
            }
        }
    }
    if !content.is_empty() {
        input.push(json!({ "type": "user_input", "content": content }));
    }
    Ok(())
}

fn image_content(image: &Image) -> Result<Value, ProviderError> {
    image.check()?;
    Ok(json!({
        "type": "image",
        "mime_type": image.media_type,
        "data": image.data_base64,
    }))
}

/// The steps of one assistant message, in order: a `model_output` step for
/// text, a `function_call` step for a tool call, and thought steps as they
/// came.
fn assistant_steps(parts: &[Part], input: &mut Vec<Value>) -> Result<(), ProviderError> {
    for part in parts {
        match part {
            Part::Text { text } => {
                if !text.is_empty() {
                    input.push(json!({
                        "type": "model_output",
                        "content": [{ "type": "text", "text": text }],
                    }));
                }
            }
            Part::ToolCall(call) => input.push(json!({
                "type": "function_call",
                "id": call.id,
                "name": call.name,
                "arguments": call.arguments,
            })),
            Part::Opaque(opaque) => {
                // Thought steps go back exactly as they came. Content from
                // another provider means nothing here and is left out.
                if opaque.provider == ProviderId::Google {
                    input.push(opaque.data.clone());
                }
            }
            Part::Image(_) | Part::ToolResult(_) => {
                return Err(rejected(
                    "an image or a tool result cannot be part of an assistant message",
                ));
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct GoogleDecoder {
    calls: ToolCalls,
    /// Thought steps under construction, by step index.
    thoughts: BTreeMap<u64, Value>,
}

impl Decoder for GoogleDecoder {
    fn decode(&mut self, event: &SseEvent, out: &mut VecDeque<StreamEvent>) {
        let Some(data) = parse_data(event, out) else {
            return;
        };
        match event_type(&data, "event_type", event) {
            "step.start" => self.step_start(&data, out),
            "step.delta" => self.step_delta(&data, out),
            "step.stop" => self.step_stop(&data, out),
            "interaction.completed" => {
                let interaction = &data["interaction"];
                usage(&interaction["usage"], out);
                let status = interaction["status"].as_str().unwrap_or("");
                out.push_back(StreamEvent::Finish(self.finish(status, interaction)));
            }
            "interaction.status_update" => {
                // Only an interaction that will not continue ends the stream
                // here. Every other status is followed by more events.
                let status = data["status"].as_str().unwrap_or("");
                if status == "failed" || status == "cancelled" {
                    usage(&data["usage"], out);
                    out.push_back(StreamEvent::Finish(self.finish(status, &data)));
                }
            }
            "error" => out.push_back(StreamEvent::Finish(failure(error_from(
                &data["error"],
                None,
            )))),
            // `interaction.created`, and any event type added later.
            _ => {}
        }
    }
}

impl GoogleDecoder {
    fn step_start(&mut self, data: &Value, out: &mut VecDeque<StreamEvent>) {
        let Some(index) = data["index"].as_u64() else {
            return;
        };
        let step = &data["step"];
        match step["type"].as_str().unwrap_or("") {
            "function_call" => {
                self.calls.start(
                    index,
                    step["id"].as_str().unwrap_or(""),
                    step["name"].as_str().unwrap_or(""),
                    out,
                );
                // The arguments normally arrive in deltas. A step that starts
                // with them already has them all. The event was parsed
                // strictly, so writing the object out loses nothing: a key
                // repeated anywhere in it would have refused the event.
                if step["arguments"]
                    .as_object()
                    .is_some_and(|map| !map.is_empty())
                {
                    self.calls
                        .set_arguments(index, &step["arguments"].to_string());
                }
            }
            "thought" => {
                self.thoughts.insert(index, step.clone());
            }
            // A `model_output` step starts empty; its text arrives in deltas.
            _ => {}
        }
    }

    fn step_delta(&mut self, data: &Value, out: &mut VecDeque<StreamEvent>) {
        let Some(index) = data["index"].as_u64() else {
            return;
        };
        let delta = &data["delta"];
        match delta["type"].as_str().unwrap_or("") {
            "text" => {
                if let Some(text) = delta["text"].as_str()
                    && !text.is_empty()
                {
                    out.push_back(StreamEvent::TextDelta {
                        text: text.to_owned(),
                    });
                }
            }
            // The reference names the delta `arguments_delta` with the text in
            // `arguments`. One SDK sample reads `partial_arguments` from a
            // delta of type `arguments`. Both are accepted.
            "arguments_delta" | "arguments" => {
                let fragment = delta["arguments"]
                    .as_str()
                    .or(delta["partial_arguments"].as_str())
                    .unwrap_or("");
                self.calls.fragment(index, fragment, out);
            }
            "thought_signature" => {
                if let Some(step) = self.thoughts.get_mut(&index) {
                    step["signature"] = delta["signature"].clone();
                }
            }
            "thought_summary" => {
                if let Some(step) = self.thoughts.get_mut(&index) {
                    add_summary(step, &delta["content"]);
                }
            }
            _ => {}
        }
    }

    fn step_stop(&mut self, data: &Value, out: &mut VecDeque<StreamEvent>) {
        let Some(index) = data["index"].as_u64() else {
            return;
        };
        if self.calls.is_open(index) {
            self.calls.end(index, out);
        } else if let Some(step) = self.thoughts.remove(&index) {
            out.push_back(StreamEvent::Opaque(OpaquePart {
                provider: ProviderId::Google,
                data: step,
            }));
        }
    }

    /// The finish reason for the final status of an interaction. `holder` is
    /// the object that carries the errors of a failed interaction.
    fn finish(&self, status: &str, holder: &Value) -> FinishReason {
        let reason = match status {
            "completed" if self.calls.completed() > 0 => FinishReason::ToolUse,
            "completed" => FinishReason::EndTurn,
            "requires_action" if self.calls.completed() > 0 => FinishReason::ToolUse,
            "requires_action" => FinishReason::Error(
                ProviderError::new(ErrorKind::MalformedResponse)
                    .with_detail("the interaction asks for an action other than a tool call"),
            ),
            // `requires_continuation` means the response stopped at a limit and
            // more could be requested. This adapter does not continue it.
            "incomplete" | "requires_continuation" => FinishReason::OutputLimit,
            "failed" => {
                let error = if holder["errors"][0].is_object() {
                    &holder["errors"][0]
                } else {
                    &holder["error"]
                };
                failure(error_from(error, None))
            }
            "cancelled" => FinishReason::Error(
                ProviderError::new(ErrorKind::Network)
                    .with_detail("the provider cancelled the interaction"),
            ),
            other => FinishReason::Error(
                ProviderError::new(ErrorKind::MalformedResponse)
                    .with_detail(format!("unrecognised interaction status {other}")),
            ),
        };
        self.calls.settle(reason)
    }
}

/// Add a streamed piece of a thought summary to a thought step. Pieces of text
/// that follow each other are one text, as in the step the API returns when it
/// does not stream.
fn add_summary(step: &mut Value, content: &Value) {
    if !content.is_object() {
        return;
    }
    if !step["summary"].is_array() {
        step["summary"] = json!([]);
    }
    let Some(summary) = step["summary"].as_array_mut() else {
        return;
    };
    if content["type"] == "text"
        && let Some(more) = content["text"].as_str()
        && let Some(last) = summary.last_mut()
        && last["type"] == "text"
    {
        let mut text = last["text"].as_str().unwrap_or("").to_owned();
        text.push_str(more);
        last["text"] = json!(text);
        return;
    }
    summary.push(content.clone());
}

/// Report the usage of an interaction, if it has one.
fn usage(usage: &Value, out: &mut VecDeque<StreamEvent>) {
    if !usage.is_object() {
        return;
    }
    // Thought tokens are counted beside the output tokens, not within them.
    let output = usage["total_output_tokens"].as_u64().unwrap_or(0);
    let thought = usage["total_thought_tokens"].as_u64().unwrap_or(0);
    out.push_back(StreamEvent::Usage(Usage {
        input_tokens: usage["total_input_tokens"].as_u64().unwrap_or(0),
        output_tokens: output.saturating_add(thought),
        cached_input_tokens: usage["total_cached_tokens"].as_u64().unwrap_or(0),
    }));
}

/// A blocked generation is a refusal; anything else is a failure.
fn failure(error: ProviderError) -> FinishReason {
    if error.kind() == ErrorKind::Refusal {
        FinishReason::Refusal(error)
    } else {
        FinishReason::Error(error)
    }
}

/// A typed error from the API's error object, with the HTTP status if there
/// was one. An error event inside a stream has none.
fn error_from(error: &Value, status: Option<u16>) -> ProviderError {
    let kind = match error["code"].as_str().unwrap_or("") {
        "authentication" | "permission_denied" | "unauthorized" | "forbidden" => {
            ErrorKind::AuthenticationFailed
        }
        "payment_required" | "quota_exceeded" => ErrorKind::QuotaExhausted,
        "rate_limit_exceeded" | "too_many_requests" => ErrorKind::RateLimited,
        "api_error" | "service_unavailable" | "internal_server_error" | "bad_gateway" => {
            ErrorKind::Overloaded
        }
        "deadline_exceeded" | "gateway_timeout" | "request_timeout" => ErrorKind::TimedOut,
        // The documented "generation blocked" codes.
        "safety"
        | "recitation"
        | "language"
        | "prohibited_content"
        | "spii"
        | "blocklist"
        | "image_safety"
        | "image_prohibited_content"
        | "image_recitation"
        | "image_other"
        | "content_blocked" => ErrorKind::Refusal,
        // The documented "generation error" codes: the model's output was
        // structurally wrong.
        "malformed_function_call"
        | "malformed_tool_call"
        | "unexpected_tool_call"
        | "no_image"
        | "too_many_tool_calls"
        | "missing_thought_signature" => ErrorKind::MalformedResponse,
        "invalid_request"
        | "failed_precondition"
        | "out_of_range"
        | "parameter_unknown"
        | "not_found"
        | "model_not_found"
        | "already_exists"
        | "aborted"
        | "cancelled"
        | "unimplemented"
        | "bad_request" => ErrorKind::RequestRejected,
        _ => match status {
            Some(status) => kind_for_status(status),
            None => ErrorKind::Overloaded,
        },
    };
    provider_error(kind, error["message"].as_str())
}

fn classify(status: u16, body: &str) -> ProviderError {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    error_from(&parsed["error"], Some(status))
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;
    use crate::model::{Message, ToolCall, ToolDefinition, ToolResult};

    fn request(messages: Vec<Message>) -> Request {
        Request {
            model: "example-model".to_owned(),
            system: Some("Be brief.".to_owned()),
            messages,
            tools: Vec::new(),
            max_output_tokens: 512,
        }
    }

    #[test]
    fn a_plain_request_is_stateless_and_has_only_the_documented_fields() {
        let body = request_body(&request(vec![Message::user_text("Hello")])).unwrap();
        assert_eq!(
            body,
            json!({
                "model": "example-model",
                "system_instruction": "Be brief.",
                "input": [
                    { "type": "user_input", "content": [{ "type": "text", "text": "Hello" }] }
                ],
                "generation_config": { "max_output_tokens": 512 },
                "stream": true,
                "store": false
            })
        );
    }

    #[test]
    fn tools_are_function_tools_with_the_schema_unchanged() {
        let mut request = request(vec![Message::user_text("Hello")]);
        let schema = json!({ "type": "object", "properties": {}, "additionalProperties": false });
        request.tools = vec![
            ToolDefinition::new("storage_list_volumes", "List volumes.", schema.clone()).unwrap(),
        ];
        let body = request_body(&request).unwrap();
        assert_eq!(
            body["tools"],
            json!([{
                "type": "function",
                "name": "storage_list_volumes",
                "description": "List volumes.",
                "parameters": schema
            }])
        );
    }

    #[test]
    fn a_tool_round_trip_uses_call_and_result_steps() {
        let mut arguments = Map::new();
        arguments.insert("volume_id".to_owned(), json!("vol-1"));
        let thought = json!({ "type": "thought", "signature": "sig" });
        let messages = vec![
            Message::user_text("Unmount it."),
            Message::assistant(vec![
                Part::Opaque(OpaquePart {
                    provider: ProviderId::Google,
                    data: thought.clone(),
                }),
                Part::Opaque(OpaquePart {
                    provider: ProviderId::OpenAi,
                    data: json!({ "type": "reasoning" }),
                }),
                Part::text("One moment."),
                Part::ToolCall(ToolCall {
                    id: "call_1".to_owned(),
                    name: "storage_unmount".to_owned(),
                    arguments,
                }),
            ]),
            Message::user(vec![
                Part::Image(Image::new("image/png", "AAAA")),
                Part::ToolResult(ToolResult::error("call_1", "storage_unmount", "busy")),
            ]),
        ];
        let body = request_body(&request(messages)).unwrap();
        assert_eq!(
            body["input"],
            json!([
                { "type": "user_input", "content": [{ "type": "text", "text": "Unmount it." }] },
                thought,
                { "type": "model_output", "content": [{ "type": "text", "text": "One moment." }] },
                { "type": "function_call", "id": "call_1", "name": "storage_unmount",
                  "arguments": { "volume_id": "vol-1" } },
                { "type": "function_result", "name": "storage_unmount", "call_id": "call_1",
                  "is_error": true, "result": [{ "type": "text", "text": "busy" }] },
                { "type": "user_input", "content": [
                    { "type": "image", "mime_type": "image/png", "data": "AAAA" }
                ] }
            ])
        );
    }

    #[test]
    fn parts_in_the_wrong_role_are_rejected_before_anything_is_sent() {
        for message in [
            Message::user(vec![Part::ToolCall(ToolCall {
                id: "call_1".to_owned(),
                name: "tool".to_owned(),
                arguments: Map::new(),
            })]),
            Message::assistant(vec![Part::ToolResult(ToolResult::text(
                "call_1", "tool", "x",
            ))]),
            Message::user(vec![Part::Image(Image::new("application/pdf", "AAAA"))]),
        ] {
            let error = request_body(&request(vec![message])).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::RequestRejected);
        }
    }

    #[test]
    fn error_bodies_map_to_kinds() {
        let body = |code: &str| json!({ "error": { "code": code, "message": "m" } }).to_string();
        for (status, code, expected) in [
            (400, "invalid_request", ErrorKind::RequestRejected),
            (400, "failed_precondition", ErrorKind::RequestRejected),
            (401, "authentication", ErrorKind::AuthenticationFailed),
            (402, "payment_required", ErrorKind::QuotaExhausted),
            (403, "permission_denied", ErrorKind::AuthenticationFailed),
            (404, "model_not_found", ErrorKind::RequestRejected),
            (429, "rate_limit_exceeded", ErrorKind::RateLimited),
            (429, "too_many_requests", ErrorKind::RateLimited),
            (429, "quota_exceeded", ErrorKind::QuotaExhausted),
            (500, "api_error", ErrorKind::Overloaded),
            (503, "service_unavailable", ErrorKind::Overloaded),
            (504, "deadline_exceeded", ErrorKind::TimedOut),
            (418, "a_code_added_later", ErrorKind::RequestRejected),
        ] {
            let error = classify(status, &body(code));
            assert_eq!(error.kind(), expected, "{status} {code}");
            assert_eq!(error.detail(), Some("m"));
        }
        // The older error body has a numeric code; the status decides.
        let older =
            json!({ "error": { "code": 429, "message": "m", "status": "RESOURCE_EXHAUSTED" } });
        assert_eq!(
            classify(429, &older.to_string()).kind(),
            ErrorKind::RateLimited
        );
        assert_eq!(classify(503, "").kind(), ErrorKind::Overloaded);
    }

    #[test]
    fn blocked_generation_is_a_refusal_and_bad_output_is_malformed() {
        for code in [
            "safety",
            "recitation",
            "language",
            "prohibited_content",
            "spii",
            "blocklist",
            "content_blocked",
        ] {
            let error = error_from(&json!({ "code": code, "message": "blocked" }), None);
            assert!(matches!(failure(error), FinishReason::Refusal(_)), "{code}");
        }
        for code in [
            "malformed_function_call",
            "unexpected_tool_call",
            "missing_thought_signature",
        ] {
            let error = error_from(&json!({ "code": code }), None);
            assert_eq!(error.kind(), ErrorKind::MalformedResponse, "{code}");
            assert!(matches!(failure(error), FinishReason::Error(_)));
        }
        let timeout = error_from(&json!({ "code": "gateway_timeout" }), None);
        assert_eq!(timeout.kind(), ErrorKind::TimedOut);
    }

    #[test]
    fn statuses_map_to_finish_reasons() {
        let decoder = GoogleDecoder::default();
        let none = Value::Null;
        assert_eq!(decoder.finish("completed", &none), FinishReason::EndTurn);
        assert_eq!(
            decoder.finish("incomplete", &none),
            FinishReason::OutputLimit
        );
        assert_eq!(
            decoder.finish("requires_continuation", &none),
            FinishReason::OutputLimit
        );
        // An action is required, yet no tool call was made.
        assert!(matches!(
            decoder.finish("requires_action", &none),
            FinishReason::Error(_)
        ));
        assert!(matches!(
            decoder.finish("cancelled", &none),
            FinishReason::Error(_)
        ));
        assert!(matches!(
            decoder.finish("a_status_added_later", &none),
            FinishReason::Error(_)
        ));

        let blocked = json!({ "errors": [{ "code": "safety", "message": "blocked" }] });
        assert!(matches!(
            decoder.finish("failed", &blocked),
            FinishReason::Refusal(_)
        ));
        let failed = json!({ "error": { "code": "api_error", "message": "failed" } });
        match decoder.finish("failed", &failed) {
            FinishReason::Error(error) => assert_eq!(error.kind(), ErrorKind::Overloaded),
            other => panic!("unexpected reason: {other:?}"),
        }
    }

    #[test]
    fn thought_summaries_are_joined_into_one_text() {
        let mut step = json!({ "type": "thought" });
        add_summary(&mut step, &json!({ "type": "text", "text": "First, " }));
        add_summary(&mut step, &json!({ "type": "text", "text": "then." }));
        add_summary(&mut step, &json!("not an object"));
        assert_eq!(
            step,
            json!({ "type": "thought", "summary": [{ "type": "text", "text": "First, then." }] })
        );
    }
}
