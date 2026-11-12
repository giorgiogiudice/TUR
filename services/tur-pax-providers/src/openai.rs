// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The OpenAI adapter: the Responses API with server-sent events.
//!
//! Written against the documentation at `developers.openai.com/api/docs` and
//! the type definitions of the official SDK as read on 2026-11-12. The mapping,
//! and what was and was not verified, is in `docs/architecture/pax-providers.md`.
//!
//! The Responses API is used because the provider recommends it for all new
//! projects. It is used statelessly: every request carries the whole
//! conversation and sets `store` to `false`, so the provider keeps no copy of
//! the conversation for later retrieval.
//!
//! Not carried: the `phase` label the API may put on an assistant message
//! (`commentary` or `final_answer`). The SDK asks for it to be sent back on
//! later requests for some models; the neutral model has no place for it, so
//! assistant text goes back without it. It is listed as a known gap.

use std::collections::VecDeque;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::error::{ErrorKind, ProviderError};
use crate::http::{Auth, kind_for_status};
use crate::model::{
    FinishReason, Image, OpaquePart, Part, ProviderId, Request, ResultContent, Role, StreamEvent,
    ToolResult, Usage, rejected,
};
use crate::provider::{
    Capabilities, Decoder, Endpoint, EventStream, NativeComputerControl, Provider, Shared,
    event_type, parse_data, provider_error,
};
use crate::sse::SseEvent;
use crate::toolcalls::ToolCalls;

/// The provider's computer-control tool type. Recorded as a capability; this
/// adapter does not drive it.
pub const COMPUTER_CONTROL_TOOL: &str = "computer";

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
    provider: ProviderId::OpenAi,
    path: "/v1/responses",
    auth: Auth::Bearer,
    headers: &[],
    classify,
};

/// The OpenAI adapter. Built by [`crate::provider::connect`].
pub struct OpenAiProvider {
    shared: Arc<Shared>,
}

impl OpenAiProvider {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        OpenAiProvider { shared }
    }
}

impl Provider for OpenAiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAi
    }

    fn capabilities(&self) -> Capabilities {
        CAPABILITIES
    }

    fn stream(&self, request: &Request) -> EventStream {
        self.shared.stream(
            ENDPOINT,
            request_body(request),
            Box::new(OpenAiDecoder::default()),
        )
    }
}

/// The JSON body of a Responses request.
fn request_body(request: &Request) -> Result<Value, ProviderError> {
    let mut input: Vec<Value> = Vec::new();
    for message in &request.messages {
        match message.role {
            Role::User => user_items(&message.content, &mut input)?,
            Role::Assistant => assistant_items(&message.content, &mut input)?,
        }
    }
    let mut body = json!({
        "model": request.model,
        "input": input,
        "max_output_tokens": request.max_output_tokens,
        "stream": true,
        "store": false,
    });
    if let Some(system) = request.system.as_deref()
        && !system.trim().is_empty()
    {
        body["instructions"] = json!(system);
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
                    // Without this the API tries to rewrite the schema for
                    // strict mode. The schema is sent as the manifest has it.
                    "strict": false,
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    Ok(body)
}

/// The input items of one user message: a `function_call_output` item for each
/// tool result, then one message item with the text and images.
fn user_items(parts: &[Part], input: &mut Vec<Value>) -> Result<(), ProviderError> {
    let mut content: Vec<Value> = Vec::new();
    for part in parts {
        match part {
            Part::Text { text } => {
                if !text.is_empty() {
                    content.push(json!({ "type": "input_text", "text": text }));
                }
            }
            Part::Image(image) => content.push(image_part(image)?),
            Part::ToolResult(result) => input.push(json!({
                "type": "function_call_output",
                "call_id": result.call_id,
                "output": output(result)?,
            })),
            Part::ToolCall(_) | Part::Opaque(_) => {
                return Err(rejected(
                    "a tool call or provider content cannot be part of a user message",
                ));
            }
        }
    }
    if !content.is_empty() {
        input.push(json!({ "role": "user", "content": content }));
    }
    Ok(())
}

/// The output of a tool: one string when it is all text, otherwise a list of
/// text and image parts. The API has no field that marks a failed call, so
/// `is_error` is not sent: the text has to say that the call failed.
fn output(result: &ToolResult) -> Result<Value, ProviderError> {
    let mut texts: Vec<&str> = Vec::new();
    let mut pieces: Vec<Value> = Vec::new();
    let mut has_image = false;
    for piece in &result.content {
        match piece {
            ResultContent::Text { text } => {
                texts.push(text);
                pieces.push(json!({ "type": "input_text", "text": text }));
            }
            ResultContent::Image(image) => {
                has_image = true;
                pieces.push(image_part(image)?);
            }
        }
    }
    if has_image {
        Ok(Value::Array(pieces))
    } else {
        Ok(json!(texts.join("\n")))
    }
}

fn image_part(image: &Image) -> Result<Value, ProviderError> {
    image.check()?;
    Ok(json!({
        "type": "input_image",
        "detail": "auto",
        "image_url": format!("data:{};base64,{}", image.media_type, image.data_base64),
    }))
}

/// The input items of one assistant message, in order: a message item for
/// text, a `function_call` item for a tool call, and reasoning items as they
/// came.
fn assistant_items(parts: &[Part], input: &mut Vec<Value>) -> Result<(), ProviderError> {
    for part in parts {
        match part {
            Part::Text { text } => {
                if !text.is_empty() {
                    input.push(json!({ "role": "assistant", "content": text }));
                }
            }
            Part::ToolCall(call) => input.push(json!({
                "type": "function_call",
                "call_id": call.id,
                "name": call.name,
                "arguments": Value::Object(call.arguments.clone()).to_string(),
            })),
            Part::Opaque(opaque) => {
                // Reasoning items go back exactly as they came. Content from
                // another provider means nothing here and is left out.
                if opaque.provider == ProviderId::OpenAi {
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
struct OpenAiDecoder {
    calls: ToolCalls,
    /// The text of a refusal, if the model refused.
    refusal: Option<String>,
}

impl Decoder for OpenAiDecoder {
    fn decode(&mut self, event: &SseEvent, out: &mut VecDeque<StreamEvent>) {
        let Some(data) = parse_data(event, out) else {
            return;
        };
        let index = data["output_index"].as_u64().unwrap_or(0);
        match event_type(&data, "type", event) {
            "response.output_item.added" => {
                let item = &data["item"];
                if item["type"] == "function_call" {
                    self.start_call(index, item, out);
                }
            }
            "response.output_text.delta" => {
                if let Some(text) = data["delta"].as_str()
                    && !text.is_empty()
                {
                    out.push_back(StreamEvent::TextDelta {
                        text: text.to_owned(),
                    });
                }
            }
            "response.refusal.delta" => {
                if let Some(text) = data["delta"].as_str() {
                    self.refusal.get_or_insert_default().push_str(text);
                }
            }
            "response.refusal.done" => {
                if let Some(text) = data["refusal"].as_str() {
                    self.refusal = Some(text.to_owned());
                }
            }
            "response.function_call_arguments.delta" => {
                self.calls
                    .fragment(index, data["delta"].as_str().unwrap_or(""), out);
            }
            "response.function_call_arguments.done" => {
                if let Some(arguments) = data["arguments"].as_str() {
                    self.calls.set_arguments(index, arguments);
                }
            }
            "response.output_item.done" => self.item_done(index, &data["item"], out),
            "response.completed" => {
                usage(&data["response"]["usage"], out);
                let reason = match self.refusal.take() {
                    Some(text) => FinishReason::Refusal(provider_error(
                        ErrorKind::Refusal,
                        Some(text.as_str()),
                    )),
                    None if self.calls.completed() > 0 => FinishReason::ToolUse,
                    None => FinishReason::EndTurn,
                };
                out.push_back(StreamEvent::Finish(self.calls.settle(reason)));
            }
            "response.incomplete" => {
                usage(&data["response"]["usage"], out);
                let reason = data["response"]["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or("");
                out.push_back(StreamEvent::Finish(incomplete(reason)));
            }
            "response.failed" => {
                usage(&data["response"]["usage"], out);
                let error =
                    error_from(&data["response"]["error"], None, ErrorKind::RequestRejected);
                out.push_back(StreamEvent::Finish(failure(error)));
            }
            "error" => {
                // The fields are on the event itself, or under `error`.
                let fields = if data["error"].is_object() {
                    &data["error"]
                } else {
                    &data
                };
                out.push_back(StreamEvent::Finish(failure(error_from(
                    fields,
                    None,
                    ErrorKind::Overloaded,
                ))));
            }
            // `response.created`, `response.in_progress`, the content part
            // events, and any event type added later.
            _ => {}
        }
    }
}

impl OpenAiDecoder {
    fn start_call(&mut self, index: u64, item: &Value, out: &mut VecDeque<StreamEvent>) {
        self.calls.start(
            index,
            item["call_id"].as_str().unwrap_or(""),
            item["name"].as_str().unwrap_or(""),
            out,
        );
    }

    /// An output item is complete.
    fn item_done(&mut self, index: u64, item: &Value, out: &mut VecDeque<StreamEvent>) {
        match item["type"].as_str().unwrap_or("") {
            "function_call" => {
                if !self.calls.is_open(index) {
                    self.start_call(index, item, out);
                }
                // The finished item carries the authoritative arguments.
                if let Some(arguments) = item["arguments"].as_str() {
                    self.calls.set_arguments(index, arguments);
                }
                self.calls.end(index, out);
            }
            // The finished reasoning item is the one to send back: the one in
            // `response.output_item.added` may be incomplete.
            "reasoning" => out.push_back(StreamEvent::Opaque(OpaquePart {
                provider: ProviderId::OpenAi,
                data: item.clone(),
            })),
            "message" => {
                let refused = item["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|part| part["type"] == "refusal");
                if let Some(part) = refused
                    && self.refusal.is_none()
                {
                    self.refusal = Some(part["refusal"].as_str().unwrap_or("").to_owned());
                }
            }
            _ => {}
        }
    }
}

/// Report the usage of a response object, if it has one.
fn usage(usage: &Value, out: &mut VecDeque<StreamEvent>) {
    if !usage.is_object() {
        return;
    }
    out.push_back(StreamEvent::Usage(Usage {
        input_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
        cached_input_tokens: usage["input_tokens_details"]["cached_tokens"]
            .as_u64()
            .unwrap_or(0),
        estimated: false,
    }));
}

/// The finish reason of a response that ended as incomplete.
fn incomplete(reason: &str) -> FinishReason {
    match reason {
        "max_output_tokens" => FinishReason::OutputLimit,
        "content_filter" => FinishReason::Refusal(provider_error(
            ErrorKind::Refusal,
            Some("the provider's content filter stopped the response"),
        )),
        other => FinishReason::Error(
            ProviderError::new(ErrorKind::MalformedResponse)
                .with_detail(format!("the response was left incomplete: {other}")),
        ),
    }
}

/// A policy block is a refusal; anything else is a failure.
fn failure(error: ProviderError) -> FinishReason {
    if error.kind() == ErrorKind::Refusal {
        FinishReason::Refusal(error)
    } else {
        FinishReason::Error(error)
    }
}

/// A typed error from the API's error object. `fallback` is the kind for a
/// code this adapter does not know when there is no HTTP status to go by.
fn error_from(error: &Value, status: Option<u16>, fallback: ErrorKind) -> ProviderError {
    let code = error["code"].as_str().unwrap_or("");
    let kind = match code {
        "rate_limit_exceeded" | "slow_down" => ErrorKind::RateLimited,
        "insufficient_quota" | "credit_balance_exhausted" => ErrorKind::QuotaExhausted,
        "server_error" | "server_is_overloaded" => ErrorKind::Overloaded,
        "bio_policy" | "misalignment_policy_violation" | "image_content_policy_violation" => {
            ErrorKind::Refusal
        }
        _ => match (error["type"].as_str().unwrap_or(""), status) {
            ("insufficient_quota", _) => ErrorKind::QuotaExhausted,
            (_, Some(status)) => kind_for_status(status),
            ("rate_limit_error", None) => ErrorKind::RateLimited,
            ("service_unavailable_error" | "server_error", None) => ErrorKind::Overloaded,
            ("invalid_request_error", None) => ErrorKind::RequestRejected,
            (_, None) => fallback,
        },
    };
    provider_error(kind, error["message"].as_str())
}

fn classify(status: u16, body: &str) -> ProviderError {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    error_from(&parsed["error"], Some(status), ErrorKind::RequestRejected)
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;
    use crate::model::{Message, ToolCall, ToolDefinition};

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
                "instructions": "Be brief.",
                "input": [
                    { "role": "user", "content": [{ "type": "input_text", "text": "Hello" }] }
                ],
                "max_output_tokens": 512,
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
                "parameters": schema,
                "strict": false
            }])
        );
    }

    #[test]
    fn a_tool_round_trip_uses_call_and_output_items() {
        let mut arguments = Map::new();
        arguments.insert("volume_id".to_owned(), json!("vol-1"));
        let reasoning =
            json!({ "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "enc" });
        let messages = vec![
            Message::user_text("Unmount it."),
            Message::assistant(vec![
                Part::Opaque(OpaquePart {
                    provider: ProviderId::OpenAi,
                    data: reasoning.clone(),
                }),
                Part::Opaque(OpaquePart {
                    provider: ProviderId::Anthropic,
                    data: json!({ "type": "thinking" }),
                }),
                Part::text("One moment."),
                Part::ToolCall(ToolCall {
                    id: "call_1".to_owned(),
                    name: "storage_unmount".to_owned(),
                    arguments,
                }),
            ]),
            Message::user(vec![
                Part::text("And then list them."),
                Part::ToolResult(ToolResult::text("call_1", "storage_unmount", "done")),
            ]),
        ];
        let body = request_body(&request(messages)).unwrap();
        assert_eq!(
            body["input"],
            json!([
                { "role": "user", "content": [{ "type": "input_text", "text": "Unmount it." }] },
                reasoning,
                { "role": "assistant", "content": "One moment." },
                { "type": "function_call", "call_id": "call_1", "name": "storage_unmount",
                  "arguments": "{\"volume_id\":\"vol-1\"}" },
                { "type": "function_call_output", "call_id": "call_1", "output": "done" },
                { "role": "user", "content": [{ "type": "input_text", "text": "And then list them." }] }
            ])
        );
    }

    #[test]
    fn images_are_data_urls_in_messages_and_in_tool_output() {
        let image = Image::new("image/png", "AAAA");
        let messages = vec![Message::user(vec![
            Part::Image(image.clone()),
            Part::ToolResult(ToolResult {
                call_id: "call_1".to_owned(),
                name: "screen_capture".to_owned(),
                content: vec![
                    ResultContent::Text {
                        text: "the screen".to_owned(),
                    },
                    ResultContent::Image(image),
                ],
                is_error: false,
            }),
        ])];
        let body = request_body(&request(messages)).unwrap();
        let picture = json!({ "type": "input_image", "detail": "auto", "image_url": "data:image/png;base64,AAAA" });
        assert_eq!(
            body["input"],
            json!([
                { "type": "function_call_output", "call_id": "call_1", "output": [
                    { "type": "input_text", "text": "the screen" },
                    picture
                ] },
                { "role": "user", "content": [picture] }
            ])
        );
    }

    #[test]
    fn parts_in_the_wrong_role_are_rejected_before_anything_is_sent() {
        for message in [
            Message::user(vec![Part::Opaque(OpaquePart {
                provider: ProviderId::OpenAi,
                data: json!({}),
            })]),
            Message::assistant(vec![Part::Image(Image::new("image/png", "AAAA"))]),
            Message::user(vec![Part::Image(Image::new("image/png", "not base64"))]),
        ] {
            let error = request_body(&request(vec![message])).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::RequestRejected);
        }
    }

    #[test]
    fn error_bodies_map_to_kinds() {
        let body = |kind: &str, code: Value| {
            json!({ "error": { "message": "m", "type": kind, "param": null, "code": code } })
                .to_string()
        };
        for (status, kind, code, expected) in [
            (
                401,
                "invalid_request_error",
                json!("invalid_api_key"),
                ErrorKind::AuthenticationFailed,
            ),
            (
                403,
                "invalid_request_error",
                json!(null),
                ErrorKind::AuthenticationFailed,
            ),
            (
                429,
                "rate_limit_error",
                json!("slow_down"),
                ErrorKind::RateLimited,
            ),
            (429, "rate_limit_error", json!(null), ErrorKind::RateLimited),
            (
                429,
                "insufficient_quota",
                json!("credit_balance_exhausted"),
                ErrorKind::QuotaExhausted,
            ),
            (
                429,
                "insufficient_quota",
                json!(null),
                ErrorKind::QuotaExhausted,
            ),
            (
                400,
                "invalid_request_error",
                json!(null),
                ErrorKind::RequestRejected,
            ),
            (
                404,
                "invalid_request_error",
                json!("model_not_found"),
                ErrorKind::RequestRejected,
            ),
            (500, "server_error", json!(null), ErrorKind::Overloaded),
            (
                503,
                "service_unavailable_error",
                json!("server_is_overloaded"),
                ErrorKind::Overloaded,
            ),
        ] {
            let error = classify(status, &body(kind, code.clone()));
            assert_eq!(error.kind(), expected, "{status} {kind} {code}");
            assert_eq!(error.detail(), Some("m"));
        }
        assert_eq!(classify(502, "bad gateway").kind(), ErrorKind::Overloaded);
    }

    #[test]
    fn failed_responses_separate_policy_blocks_from_failures() {
        let blocked = error_from(
            &json!({ "code": "bio_policy", "message": "blocked" }),
            None,
            ErrorKind::RequestRejected,
        );
        assert!(matches!(failure(blocked), FinishReason::Refusal(_)));

        let limited = error_from(
            &json!({ "code": "rate_limit_exceeded", "message": "slow" }),
            None,
            ErrorKind::RequestRejected,
        );
        assert_eq!(limited.kind(), ErrorKind::RateLimited);
        assert!(matches!(failure(limited), FinishReason::Error(_)));

        let unknown = error_from(
            &json!({ "code": "a_code_added_later" }),
            None,
            ErrorKind::Overloaded,
        );
        assert_eq!(unknown.kind(), ErrorKind::Overloaded);

        assert_eq!(incomplete("max_output_tokens"), FinishReason::OutputLimit);
        assert!(matches!(
            incomplete("content_filter"),
            FinishReason::Refusal(_)
        ));
        assert!(matches!(
            incomplete("a_reason_added_later"),
            FinishReason::Error(_)
        ));
    }
}
