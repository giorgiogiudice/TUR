// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The Anthropic adapter: the Messages API with server-sent events.
//!
//! Written against the documentation at `platform.claude.com/docs` as read on
//! 2026-11-12. The mapping, and what was and was not verified, is in
//! `docs/architecture/pax-providers.md`.
//!
//! The request carries only what the documentation requires: the model, the
//! output limit, the messages, and the system text and tools when there are
//! any. No sampling parameter and no thinking setting is sent.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::error::{ErrorKind, ProviderError};
use crate::http::{Auth, kind_for_status};
use crate::model::{
    FinishReason, Image, Message, OpaquePart, Part, ProviderId, Request, ResultContent, Role,
    StreamEvent, Usage, rejected,
};
use crate::provider::{
    Capabilities, Decoder, Endpoint, EventStream, NativeComputerControl, Provider, Shared,
    event_type, parse_data, provider_error,
};
use crate::sse::SseEvent;
use crate::toolcalls::ToolCalls;

/// The API version this adapter is pinned to, sent as `anthropic-version`.
pub const API_VERSION: &str = "2026-10-21";

/// The provider's computer-control tool type. Recorded as a capability; this
/// adapter does not drive it.
pub const COMPUTER_CONTROL_TOOL: &str = "computer_toolset_20260801";

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
    provider: ProviderId::Anthropic,
    path: "/v1/messages",
    auth: Auth::Header("x-api-key"),
    headers: &[("anthropic-version", API_VERSION)],
    classify,
};

/// The Anthropic adapter. Built by [`crate::provider::connect`].
pub struct AnthropicProvider {
    shared: Arc<Shared>,
}

impl AnthropicProvider {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        AnthropicProvider { shared }
    }
}

impl Provider for AnthropicProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Anthropic
    }

    fn capabilities(&self) -> Capabilities {
        CAPABILITIES
    }

    fn stream(&self, request: &Request) -> EventStream {
        self.shared.stream(
            ENDPOINT,
            request_body(request),
            Box::new(AnthropicDecoder::default()),
        )
    }
}

/// The JSON body of a Messages request.
fn request_body(request: &Request) -> Result<Value, ProviderError> {
    // The API takes alternating turns, so neighbouring messages of one role
    // are sent as one.
    let mut turns: Vec<(Role, Vec<Value>)> = Vec::new();
    for message in &request.messages {
        let blocks = blocks(message)?;
        if blocks.is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some((role, existing)) if *role == message.role => existing.extend(blocks),
            _ => turns.push((message.role, blocks)),
        }
    }
    let messages: Vec<Value> = turns
        .into_iter()
        .map(|(role, blocks)| match role {
            Role::User => json!({ "role": "user", "content": results_first(blocks) }),
            Role::Assistant => json!({ "role": "assistant", "content": blocks }),
        })
        .collect();

    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_output_tokens,
        "stream": true,
        "messages": messages,
    });
    if let Some(system) = request.system.as_deref()
        && !system.trim().is_empty()
    {
        body["system"] = json!(system);
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.input_schema(),
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    Ok(body)
}

/// The content blocks of one message.
fn blocks(message: &Message) -> Result<Vec<Value>, ProviderError> {
    let mut blocks = Vec::new();
    for part in &message.content {
        match (message.role, part) {
            (_, Part::Text { text }) => {
                // The API does not accept an empty text block.
                if !text.is_empty() {
                    blocks.push(json!({ "type": "text", "text": text }));
                }
            }
            (Role::User, Part::Image(image)) => blocks.push(image_block(image)?),
            (Role::User, Part::ToolResult(result)) => {
                let mut content = Vec::new();
                for piece in &result.content {
                    match piece {
                        ResultContent::Text { text } => {
                            if !text.is_empty() {
                                content.push(json!({ "type": "text", "text": text }));
                            }
                        }
                        ResultContent::Image(image) => content.push(image_block(image)?),
                    }
                }
                let mut block = json!({ "type": "tool_result", "tool_use_id": result.call_id });
                if !content.is_empty() {
                    block["content"] = Value::Array(content);
                }
                if result.is_error {
                    block["is_error"] = json!(true);
                }
                blocks.push(block);
            }
            (Role::Assistant, Part::ToolCall(call)) => blocks.push(json!({
                "type": "tool_use",
                "id": call.id,
                "name": call.name,
                "input": call.arguments,
            })),
            (Role::Assistant, Part::Opaque(opaque)) => {
                // Thinking blocks go back exactly as they came. Content from
                // another provider means nothing here and is left out.
                if opaque.provider == ProviderId::Anthropic {
                    blocks.push(opaque.data.clone());
                }
            }
            (Role::User, Part::ToolCall(_) | Part::Opaque(_)) => {
                return Err(rejected(
                    "a tool call or provider content cannot be part of a user message",
                ));
            }
            (Role::Assistant, Part::Image(_) | Part::ToolResult(_)) => {
                return Err(rejected(
                    "an image or a tool result cannot be part of an assistant message",
                ));
            }
        }
    }
    Ok(blocks)
}

fn image_block(image: &Image) -> Result<Value, ProviderError> {
    image.check()?;
    Ok(json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": image.media_type,
            "data": image.data_base64,
        },
    }))
}

/// Tool results must be the first blocks of a user message.
fn results_first(blocks: Vec<Value>) -> Vec<Value> {
    let (mut results, others): (Vec<Value>, Vec<Value>) = blocks
        .into_iter()
        .partition(|block| block["type"] == "tool_result");
    results.extend(others);
    results
}

/// The token counts the API reports, as it reports them.
#[derive(Default)]
struct Counts {
    input: u64,
    cache_creation: u64,
    cache_read: u64,
    output: u64,
}

#[derive(Default)]
struct AnthropicDecoder {
    calls: ToolCalls,
    /// Thinking blocks under construction, by content block index.
    thinking: BTreeMap<u64, Value>,
    counts: Counts,
    stop: Option<FinishReason>,
}

impl Decoder for AnthropicDecoder {
    fn decode(&mut self, event: &SseEvent, out: &mut VecDeque<StreamEvent>) {
        let Some(data) = parse_data(event, out) else {
            return;
        };
        match event_type(&data, "type", event) {
            "message_start" => self.usage(&data["message"]["usage"], out),
            "content_block_start" => self.block_start(&data, out),
            "content_block_delta" => self.block_delta(&data, out),
            "content_block_stop" => self.block_stop(&data, out),
            "message_delta" => self.message_delta(&data, out),
            "message_stop" => self.message_stop(out),
            "error" => out.push_back(StreamEvent::Finish(FinishReason::Error(error_from(
                &data["error"],
                None,
            )))),
            // `ping`, and any event type added later.
            _ => {}
        }
    }
}

impl AnthropicDecoder {
    /// Take the counts a usage object gives and report the usage so far.
    fn usage(&mut self, usage: &Value, out: &mut VecDeque<StreamEvent>) {
        if !usage.is_object() {
            return;
        }
        if let Some(count) = usage["input_tokens"].as_u64() {
            self.counts.input = count;
        }
        if let Some(count) = usage["cache_creation_input_tokens"].as_u64() {
            self.counts.cache_creation = count;
        }
        if let Some(count) = usage["cache_read_input_tokens"].as_u64() {
            self.counts.cache_read = count;
        }
        if let Some(count) = usage["output_tokens"].as_u64() {
            self.counts.output = count;
        }
        // `input_tokens` excludes what was written to or read from the cache.
        out.push_back(StreamEvent::Usage(Usage {
            input_tokens: self
                .counts
                .input
                .saturating_add(self.counts.cache_creation)
                .saturating_add(self.counts.cache_read),
            output_tokens: self.counts.output,
            cached_input_tokens: self.counts.cache_read,
        }));
    }

    fn block_start(&mut self, data: &Value, out: &mut VecDeque<StreamEvent>) {
        let Some(index) = data["index"].as_u64() else {
            return;
        };
        let block = &data["content_block"];
        match block["type"].as_str().unwrap_or("") {
            "tool_use" => {
                self.calls.start(
                    index,
                    block["id"].as_str().unwrap_or(""),
                    block["name"].as_str().unwrap_or(""),
                    out,
                );
                // A streamed block starts with an empty input, and the
                // arguments arrive in deltas. A block that starts with its
                // input already has all of it. The event was parsed strictly,
                // so writing the object out loses nothing.
                if block["input"]
                    .as_object()
                    .is_some_and(|map| !map.is_empty())
                {
                    self.calls.set_arguments(index, &block["input"].to_string());
                }
            }
            "thinking" | "redacted_thinking" => {
                self.thinking.insert(index, block.clone());
            }
            // A text block starts empty; its text arrives in deltas.
            _ => {}
        }
    }

    fn block_delta(&mut self, data: &Value, out: &mut VecDeque<StreamEvent>) {
        let Some(index) = data["index"].as_u64() else {
            return;
        };
        let delta = &data["delta"];
        match delta["type"].as_str().unwrap_or("") {
            "text_delta" => {
                if let Some(text) = delta["text"].as_str()
                    && !text.is_empty()
                {
                    out.push_back(StreamEvent::TextDelta {
                        text: text.to_owned(),
                    });
                }
            }
            "input_json_delta" => {
                self.calls
                    .fragment(index, delta["partial_json"].as_str().unwrap_or(""), out);
            }
            "thinking_delta" => {
                if let Some(block) = self.thinking.get_mut(&index)
                    && let Some(more) = delta["thinking"].as_str()
                {
                    let mut text = block["thinking"].as_str().unwrap_or("").to_owned();
                    text.push_str(more);
                    block["thinking"] = json!(text);
                }
            }
            "signature_delta" => {
                if let Some(block) = self.thinking.get_mut(&index) {
                    block["signature"] = delta["signature"].clone();
                }
            }
            _ => {}
        }
    }

    fn block_stop(&mut self, data: &Value, out: &mut VecDeque<StreamEvent>) {
        let Some(index) = data["index"].as_u64() else {
            return;
        };
        if self.calls.is_open(index) {
            self.calls.end(index, out);
        } else if let Some(block) = self.thinking.remove(&index) {
            out.push_back(StreamEvent::Opaque(OpaquePart {
                provider: ProviderId::Anthropic,
                data: block,
            }));
        }
    }

    fn message_delta(&mut self, data: &Value, out: &mut VecDeque<StreamEvent>) {
        self.usage(&data["usage"], out);
        let delta = &data["delta"];
        let Some(reason) = delta["stop_reason"].as_str() else {
            return;
        };
        let details = if delta["stop_details"].is_object() {
            &delta["stop_details"]
        } else {
            &data["stop_details"]
        };
        self.stop = Some(finish_reason(reason, details));
    }

    fn message_stop(&mut self, out: &mut VecDeque<StreamEvent>) {
        let reason = self.stop.take().unwrap_or_else(|| {
            FinishReason::Error(
                ProviderError::new(ErrorKind::MalformedResponse)
                    .with_detail("the response ended without a stop reason"),
            )
        });
        out.push_back(StreamEvent::Finish(self.calls.settle(reason)));
    }
}

fn finish_reason(reason: &str, details: &Value) -> FinishReason {
    match reason {
        "end_turn" | "stop_sequence" => FinishReason::EndTurn,
        "tool_use" => FinishReason::ToolUse,
        "max_tokens" | "model_context_window_exceeded" => FinishReason::OutputLimit,
        "refusal" => {
            let detail = match (
                details["category"].as_str(),
                details["explanation"].as_str(),
            ) {
                (Some(category), Some(explanation)) => Some(format!("{category}: {explanation}")),
                (Some(text), None) | (None, Some(text)) => Some(text.to_owned()),
                (None, None) => None,
            };
            FinishReason::Refusal(provider_error(ErrorKind::Refusal, detail.as_deref()))
        }
        // `pause_turn` belongs to server-side tools, which this adapter never
        // requests. It and any reason added later are not guessed at.
        other => FinishReason::Error(
            ProviderError::new(ErrorKind::MalformedResponse)
                .with_detail(format!("unrecognised stop reason {other}")),
        ),
    }
}

/// A typed error from the API's error object, with the HTTP status if there
/// was one. An error event inside a stream has none.
fn error_from(error: &Value, status: Option<u16>) -> ProviderError {
    let message = error["message"].as_str();
    let kind = match error["type"].as_str().unwrap_or("") {
        "authentication_error" | "permission_error" => ErrorKind::AuthenticationFailed,
        "billing_error" => ErrorKind::QuotaExhausted,
        // The monthly spend cap is reported as a rate limit that no retry
        // clears. The API marks it with this code.
        "rate_limit_error" if error["details"]["error_code"] == "enforced_spend_limit_reached" => {
            ErrorKind::QuotaExhausted
        }
        "rate_limit_error" => ErrorKind::RateLimited,
        "overloaded_error" | "api_error" => ErrorKind::Overloaded,
        "timeout_error" => ErrorKind::TimedOut,
        // A spend limit the user set is reported as an invalid request.
        "invalid_request_error"
            if message.is_some_and(|text| text.starts_with("You have reached your specified")) =>
        {
            ErrorKind::QuotaExhausted
        }
        "invalid_request_error" | "not_found_error" | "request_too_large" | "conflict_error" => {
            ErrorKind::RequestRejected
        }
        _ => match status {
            Some(status) => kind_for_status(status),
            None => ErrorKind::Overloaded,
        },
    };
    provider_error(kind, message)
}

fn classify(status: u16, body: &str) -> ProviderError {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    error_from(&parsed["error"], Some(status))
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;
    use crate::model::{ToolCall, ToolDefinition, ToolResult};

    fn request(messages: Vec<Message>) -> Request {
        Request {
            model: "example-model".to_owned(),
            system: Some("Be brief.".to_owned()),
            messages,
            tools: Vec::new(),
            max_output_tokens: 512,
        }
    }

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.to_owned(),
            name: "storage_list_volumes".to_owned(),
            arguments: Map::new(),
        }
    }

    #[test]
    fn a_plain_request_has_only_the_documented_fields() {
        let body = request_body(&request(vec![Message::user_text("Hello")])).unwrap();
        assert_eq!(
            body,
            json!({
                "model": "example-model",
                "max_tokens": 512,
                "stream": true,
                "system": "Be brief.",
                "messages": [
                    { "role": "user", "content": [{ "type": "text", "text": "Hello" }] }
                ]
            })
        );
    }

    #[test]
    fn tools_use_the_input_schema_field() {
        let mut request = request(vec![Message::user_text("Hello")]);
        request.system = None;
        let schema = json!({ "type": "object", "properties": {}, "additionalProperties": false });
        request.tools = vec![
            ToolDefinition::new("storage_list_volumes", "List volumes.", schema.clone()).unwrap(),
        ];
        let body = request_body(&request).unwrap();
        assert!(body.get("system").is_none());
        assert_eq!(
            body["tools"],
            json!([{
                "name": "storage_list_volumes",
                "description": "List volumes.",
                "input_schema": schema
            }])
        );
    }

    #[test]
    fn tool_results_come_first_and_neighbouring_turns_are_merged() {
        let thinking = json!({ "type": "thinking", "thinking": "", "signature": "sig" });
        let messages = vec![
            Message::user_text("List my volumes."),
            Message::assistant(vec![
                Part::Opaque(OpaquePart {
                    provider: ProviderId::Anthropic,
                    data: thinking.clone(),
                }),
                Part::Opaque(OpaquePart {
                    provider: ProviderId::OpenAi,
                    data: json!({ "type": "reasoning", "id": "rs_1" }),
                }),
                Part::text(""),
                Part::ToolCall(call("toolu_1")),
            ]),
            Message::user(vec![
                Part::text("Thanks."),
                Part::ToolResult(ToolResult::error(
                    "toolu_1",
                    "storage_list_volumes",
                    "no bus",
                )),
            ]),
            Message::user(vec![Part::Image(Image::new("image/png", "AAAA"))]),
        ];
        let body = request_body(&request(messages)).unwrap();
        assert_eq!(
            body["messages"],
            json!([
                { "role": "user", "content": [{ "type": "text", "text": "List my volumes." }] },
                { "role": "assistant", "content": [
                    thinking,
                    { "type": "tool_use", "id": "toolu_1", "name": "storage_list_volumes", "input": {} }
                ] },
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_1", "is_error": true,
                      "content": [{ "type": "text", "text": "no bus" }] },
                    { "type": "text", "text": "Thanks." },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } }
                ] }
            ])
        );
    }

    #[test]
    fn parts_in_the_wrong_role_are_rejected_before_anything_is_sent() {
        for message in [
            Message::user(vec![Part::ToolCall(call("toolu_1"))]),
            Message::assistant(vec![Part::Image(Image::new("image/png", "AAAA"))]),
            Message::assistant(vec![Part::ToolResult(ToolResult::text(
                "toolu_1", "tool", "x",
            ))]),
            Message::user(vec![Part::Image(Image::new("text/html", "AAAA"))]),
        ] {
            let error = request_body(&request(vec![message])).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::RequestRejected);
        }
    }

    #[test]
    fn error_bodies_map_to_kinds() {
        let body = |kind: &str| {
            json!({ "type": "error", "error": { "type": kind, "message": "m" } }).to_string()
        };
        for (status, kind, expected) in [
            (401, "authentication_error", ErrorKind::AuthenticationFailed),
            (403, "permission_error", ErrorKind::AuthenticationFailed),
            (402, "billing_error", ErrorKind::QuotaExhausted),
            (429, "rate_limit_error", ErrorKind::RateLimited),
            (400, "invalid_request_error", ErrorKind::RequestRejected),
            (404, "not_found_error", ErrorKind::RequestRejected),
            (413, "request_too_large", ErrorKind::RequestRejected),
            (500, "api_error", ErrorKind::Overloaded),
            (529, "overloaded_error", ErrorKind::Overloaded),
            (504, "timeout_error", ErrorKind::TimedOut),
            (500, "timeout_error", ErrorKind::TimedOut),
            (418, "a_type_added_later", ErrorKind::RequestRejected),
            (503, "a_type_added_later", ErrorKind::Overloaded),
        ] {
            let error = classify(status, &body(kind));
            assert_eq!(error.kind(), expected, "{status} {kind}");
            assert_eq!(error.detail(), Some("m"));
        }
        // A body that is not the documented JSON is classified by status alone.
        assert_eq!(classify(529, "<html>").kind(), ErrorKind::Overloaded);
        assert_eq!(classify(401, "").kind(), ErrorKind::AuthenticationFailed);
    }

    #[test]
    fn spend_limits_are_not_reported_as_retryable_rate_limits() {
        let cap = json!({
            "type": "error",
            "error": {
                "type": "rate_limit_error",
                "message": "You have reached your API usage limits.",
                "details": { "error_code": "enforced_spend_limit_reached" }
            }
        });
        assert_eq!(
            classify(429, &cap.to_string()).kind(),
            ErrorKind::QuotaExhausted
        );

        let own_limit = json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "You have reached your specified API usage limits."
            }
        });
        assert_eq!(
            classify(400, &own_limit.to_string()).kind(),
            ErrorKind::QuotaExhausted
        );
    }

    #[test]
    fn stop_reasons_map_to_finish_reasons() {
        let none = Value::Null;
        assert_eq!(finish_reason("end_turn", &none), FinishReason::EndTurn);
        assert_eq!(finish_reason("stop_sequence", &none), FinishReason::EndTurn);
        assert_eq!(finish_reason("tool_use", &none), FinishReason::ToolUse);
        assert_eq!(
            finish_reason("max_tokens", &none),
            FinishReason::OutputLimit
        );
        assert_eq!(
            finish_reason("model_context_window_exceeded", &none),
            FinishReason::OutputLimit
        );
        let details = json!({ "type": "refusal", "category": "cyber", "explanation": "Declined." });
        match finish_reason("refusal", &details) {
            FinishReason::Refusal(error) => {
                assert_eq!(error.kind(), ErrorKind::Refusal);
                assert_eq!(error.detail(), Some("cyber: Declined."));
            }
            other => panic!("unexpected reason: {other:?}"),
        }
        // A refusal with no named category is still a refusal.
        assert!(matches!(
            finish_reason("refusal", &none),
            FinishReason::Refusal(_)
        ));
        for unknown in ["pause_turn", "a_reason_added_later"] {
            let reason = finish_reason(unknown, &none);
            assert_eq!(
                reason.error().map(ProviderError::kind),
                Some(ErrorKind::MalformedResponse)
            );
        }
    }
}
