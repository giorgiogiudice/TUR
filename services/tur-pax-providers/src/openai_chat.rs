// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The OpenAI-compatible adapter family: the Chat Completions API with
//! server-sent events, as DeepSeek, other cloud services and model servers
//! on the computer itself offer it (ADR-0004, amended 12 November 2026).
//!
//! Written against OpenAI's Chat Completions reference and function-calling
//! guide (`developers.openai.com/api/reference/resources/chat/...`,
//! `.../docs/guides/function-calling?api-mode=chat`), the type definitions of
//! the official Python SDK (`src/openai/types/chat/`), and DeepSeek's API
//! documentation (`api-docs.deepseek.com`), all read on 2026-11-12. What each
//! service documents differently, and what was not verified, is in
//! `docs/architecture/pax-providers.md`.
//!
//! The family has no address of its own: the configuration names the base
//! URL the service documents (`https://api.deepseek.com`, or
//! `http://127.0.0.1:11434/v1` for a server on this computer), and requests
//! go to `<base URL>/chat/completions`.
//!
//! **Strict reading.** The data of every event is read strictly, and so is
//! the argument text of every tool call: a key repeated at any depth, or
//! arguments that are not one JSON object, make the call undeliverable and a
//! normal end of the response malformed, so that the whole response is
//! discarded (the same rule as the other families). So does a call of which
//! the service says two things: two names, two identifiers, or arguments that
//! are not text.
//!
//! **Usage.** Every request asks for usage in the stream
//! (`stream_options.include_usage`), because the day's limit rests on it. A
//! service that sends none anyway is counted at an estimate, marked as one
//! ([`crate::Usage::estimated`]): one token for every two bytes of the
//! request, a fixed figure for each image, and the whole output limit. That
//! is meant to be high, never low.
//!
//! **Reasoning.** Some services stream the model's reasoning beside its
//! answer (`reasoning_content` at DeepSeek and llama.cpp, `reasoning` at
//! Ollama and others). It is never text of the answer, never an instruction,
//! and never shown as P@x's words: it is kept as an opaque part of the
//! assistant turn. It is sent back only when the configuration says so
//! (`send_back_reasoning`), for a service whose documentation requires it:
//! DeepSeek returns an error when the reasoning of earlier turns is missing
//! from a request that offers tools.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::error::{ErrorKind, ProviderError};
use crate::http::{Auth, kind_for_status};
use crate::model::{
    FinishReason, Image, OpaquePart, Part, ProviderId, Request, ResultContent, Role, StreamEvent,
    ToolResult, Usage, rejected,
};
use crate::provider::{
    Capabilities, Decoder, Endpoint, EventStream, NativeComputerControl, Provider, Shared,
    parse_data, provider_error,
};
use crate::sse::SseEvent;
use crate::toolcalls::ToolCalls;

/// What this adapter can carry. Whether a given model of a given service
/// can call tools or see images is that service's to declare: the session
/// service reads it from the provider's entry.
pub const CAPABILITIES: Capabilities = Capabilities {
    streaming: true,
    tool_calling: true,
    parallel_tool_calls: true,
    image_input: true,
    // The reference says of a tool message: "only type `text` is supported".
    image_in_tool_results: false,
    native_computer_control: NativeComputerControl {
        provider_tool: None,
        implemented: false,
    },
    model_discovery: false,
    speech_services: false,
};

pub(crate) const ENDPOINT: Endpoint = Endpoint {
    provider: ProviderId::OpenAiChat,
    path: "/chat/completions",
    auth: Auth::Bearer,
    headers: &[],
    classify,
};

/// The fields in which services stream a model's reasoning.
pub const REASONING_FIELDS: [&str; 2] = ["reasoning_content", "reasoning"];

/// Bytes of request text counted as one token when a service reports no
/// usage. Tokenisers put several bytes in a token for ordinary text; two is
/// meant to be high.
pub const ESTIMATE_BYTES_PER_TOKEN: u64 = 2;

/// Tokens counted for each image when a service reports no usage.
pub const ESTIMATE_TOKENS_PER_IMAGE: u64 = 3_000;

/// The key a call gets when the service gave it no index: above any index a
/// service could send for the calls of one response.
const UNINDEXED_KEYS: u64 = 1 << 40;

/// The OpenAI-compatible adapter. Built by [`crate::provider::connect`] or
/// [`crate::provider::connect_without_key`].
pub struct OpenAiChatProvider {
    shared: Arc<Shared>,
    send_back_reasoning: bool,
}

impl OpenAiChatProvider {
    pub(crate) fn new(shared: Arc<Shared>, send_back_reasoning: bool) -> Self {
        OpenAiChatProvider {
            shared,
            send_back_reasoning,
        }
    }
}

impl Provider for OpenAiChatProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAiChat
    }

    fn capabilities(&self) -> Capabilities {
        CAPABILITIES
    }

    fn stream(&self, request: &Request) -> EventStream {
        let body = request_body(request, self.send_back_reasoning);
        let estimate = body
            .as_ref()
            .map(|body| input_estimate(body, request))
            .unwrap_or(0);
        self.shared.stream(
            ENDPOINT,
            body,
            Box::new(ChatDecoder::new(estimate, request.max_output_tokens)),
        )
    }
}

/// The JSON body of a Chat Completions request.
fn request_body(request: &Request, send_back_reasoning: bool) -> Result<Value, ProviderError> {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(system) = request.system.as_deref()
        && !system.trim().is_empty()
    {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &request.messages {
        match message.role {
            Role::User => user_messages(&message.content, &mut messages)?,
            Role::Assistant => {
                assistant_message(&message.content, send_back_reasoning, &mut messages)?;
            }
        }
    }
    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": request.max_output_tokens,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name(),
                        "description": tool.description(),
                        "parameters": tool.input_schema(),
                    },
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
        // The model decides whether to call a tool. A forced choice is
        // refused by DeepSeek in thinking mode, its default.
        body["tool_choice"] = json!("auto");
    }
    Ok(body)
}

/// The messages of one user message: a `tool` message for each tool result,
/// which must follow the assistant message that called the tools, then one
/// `user` message with the text and images.
fn user_messages(parts: &[Part], messages: &mut Vec<Value>) -> Result<(), ProviderError> {
    let mut texts: Vec<&str> = Vec::new();
    let mut images: Vec<Value> = Vec::new();
    for part in parts {
        match part {
            Part::Text { text } => {
                if !text.is_empty() {
                    texts.push(text);
                }
            }
            Part::Image(image) => images.push(image_part(image)?),
            Part::ToolResult(result) => messages.push(json!({
                "role": "tool",
                "tool_call_id": result.call_id,
                "content": tool_output(result)?,
            })),
            Part::ToolCall(_) | Part::Opaque(_) => {
                return Err(rejected(
                    "a tool call or provider content cannot be part of a user message",
                ));
            }
        }
    }
    if images.is_empty() {
        if !texts.is_empty() {
            messages.push(json!({ "role": "user", "content": texts.join("\n\n") }));
        }
    } else {
        let mut content: Vec<Value> = texts
            .iter()
            .map(|text| json!({ "type": "text", "text": text }))
            .collect();
        content.extend(images);
        messages.push(json!({ "role": "user", "content": content }));
    }
    Ok(())
}

/// The content of a tool message: text only. The interface has no field that
/// marks a failed call, so the text has to say that it failed; and none for
/// an image in a tool result, so a result with one is refused before
/// anything is sent.
fn tool_output(result: &ToolResult) -> Result<String, ProviderError> {
    let mut texts: Vec<&str> = Vec::new();
    for piece in &result.content {
        match piece {
            ResultContent::Text { text } => texts.push(text),
            ResultContent::Image(_) => {
                return Err(rejected(
                    "this interface cannot carry an image in a tool result",
                ));
            }
        }
    }
    Ok(texts.join("\n"))
}

fn image_part(image: &Image) -> Result<Value, ProviderError> {
    image.check()?;
    Ok(json!({
        "type": "image_url",
        "image_url": {
            "url": format!("data:{};base64,{}", image.media_type, image.data_base64),
        },
    }))
}

/// One assistant message: its text, its tool calls with their arguments as a
/// JSON string, and, if the service needs it back, its reasoning.
fn assistant_message(
    parts: &[Part],
    send_back_reasoning: bool,
    messages: &mut Vec<Value>,
) -> Result<(), ProviderError> {
    let mut text = String::new();
    let mut calls: Vec<Value> = Vec::new();
    let mut reasoning: Map<String, Value> = Map::new();
    for part in parts {
        match part {
            Part::Text { text: piece } => text.push_str(piece),
            Part::ToolCall(call) => calls.push(json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": Value::Object(call.arguments.clone()).to_string(),
                },
            })),
            Part::Opaque(opaque) => {
                // Only this family's own reasoning, and only where the
                // service's documentation requires it back.
                if send_back_reasoning && opaque.provider == ProviderId::OpenAiChat {
                    for field in REASONING_FIELDS {
                        if let Some(piece) = opaque.data[field].as_str() {
                            let joined = match reasoning.get(field).and_then(Value::as_str) {
                                Some(before) => format!("{before}{piece}"),
                                None => piece.to_owned(),
                            };
                            reasoning.insert(field.to_owned(), Value::String(joined));
                        }
                    }
                }
            }
            Part::Image(_) | Part::ToolResult(_) => {
                return Err(rejected(
                    "an image or a tool result cannot be part of an assistant message",
                ));
            }
        }
    }
    if text.is_empty() && calls.is_empty() && reasoning.is_empty() {
        return Ok(());
    }
    let mut message = Map::new();
    message.insert("role".to_owned(), json!("assistant"));
    message.insert(
        "content".to_owned(),
        if text.is_empty() && !calls.is_empty() {
            // "Required unless `tool_calls` ... is specified."
            Value::Null
        } else {
            Value::String(text)
        },
    );
    if !calls.is_empty() {
        message.insert("tool_calls".to_owned(), Value::Array(calls));
    }
    message.extend(reasoning);
    messages.push(Value::Object(message));
    Ok(())
}

/// The input of a request as an estimate in tokens, for a service that
/// reports no usage: the request's bytes without its image data, two to a
/// token, and a fixed figure for each image.
fn input_estimate(body: &Value, request: &Request) -> u64 {
    let mut images: u64 = 0;
    let mut image_bytes: u64 = 0;
    let mut count = |image: &Image| {
        images += 1;
        image_bytes = image_bytes.saturating_add(image.data_base64.len() as u64);
    };
    for message in &request.messages {
        for part in &message.content {
            match part {
                Part::Image(image) => count(image),
                Part::ToolResult(result) => {
                    for piece in &result.content {
                        if let ResultContent::Image(image) = piece {
                            count(image);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let bytes = (body.to_string().len() as u64).saturating_sub(image_bytes);
    bytes
        .div_ceil(ESTIMATE_BYTES_PER_TOKEN)
        .saturating_add(images.saturating_mul(ESTIMATE_TOKENS_PER_IMAGE))
}

/// What has been said about one tool call while it streams.
#[derive(Default)]
struct Slot {
    id: String,
    name: String,
    /// Argument text that arrived before the name.
    pending: String,
    started: bool,
    broken: bool,
}

struct ChatDecoder {
    calls: ToolCalls,
    slots: BTreeMap<u64, Slot>,
    /// The keys of the calls, in the order they first appeared.
    order: Vec<u64>,
    /// The call the last delta without an index or an identifier belongs to.
    last_key: Option<u64>,
    next_unindexed: u64,
    /// Reasoning not yet delivered, by the field it came in.
    reasoning: Map<String, Value>,
    refusal: String,
    /// The first finish reason the service gave. One service repeats it on
    /// the chunk that carries the usage.
    finish: Option<String>,
    usage_seen: bool,
    finished: bool,
    input_estimate: u64,
    max_output_tokens: u32,
}

impl ChatDecoder {
    fn new(input_estimate: u64, max_output_tokens: u32) -> Self {
        ChatDecoder {
            calls: ToolCalls::default(),
            slots: BTreeMap::new(),
            order: Vec::new(),
            last_key: None,
            next_unindexed: UNINDEXED_KEYS,
            reasoning: Map::new(),
            refusal: String::new(),
            finish: None,
            usage_seen: false,
            finished: false,
            input_estimate,
            max_output_tokens,
        }
    }

    /// Deliver the reasoning that arrived so far as one opaque part, before
    /// what follows it.
    fn flush_reasoning(&mut self, out: &mut VecDeque<StreamEvent>) {
        if self.reasoning.is_empty() {
            return;
        }
        let data = std::mem::take(&mut self.reasoning);
        out.push_back(StreamEvent::Opaque(OpaquePart {
            provider: ProviderId::OpenAiChat,
            data: Value::Object(data),
        }));
    }

    fn add_reasoning(&mut self, field: &str, text: &str) {
        let joined = match self.reasoning.get(field).and_then(Value::as_str) {
            Some(before) => format!("{before}{text}"),
            None => text.to_owned(),
        };
        self.reasoning
            .insert(field.to_owned(), Value::String(joined));
    }

    /// The usage the service did not report, estimated high.
    fn estimate(&self, out: &mut VecDeque<StreamEvent>) {
        out.push_back(StreamEvent::Usage(Usage {
            input_tokens: self.input_estimate,
            output_tokens: u64::from(self.max_output_tokens),
            cached_input_tokens: 0,
            estimated: true,
        }));
    }

    /// The key of the call a delta belongs to.
    fn key_for(&mut self, index: Option<u64>, id: Option<&str>) -> u64 {
        if let Some(index) = index {
            return index;
        }
        if let Some(id) = id
            && let Some(key) = self
                .order
                .iter()
                .find(|key| self.slots.get(*key).is_some_and(|slot| slot.id == id))
        {
            return *key;
        }
        match (id, self.last_key) {
            (None, Some(last)) => last,
            _ => {
                let key = self.next_unindexed;
                self.next_unindexed += 1;
                key
            }
        }
    }

    /// One element of `delta.tool_calls`.
    fn tool_delta(&mut self, item: &Value, out: &mut VecDeque<StreamEvent>) {
        let id = item["id"].as_str().filter(|id| !id.is_empty());
        let key = self.key_for(item["index"].as_u64(), id);
        self.last_key = Some(key);
        let mut slot = match self.slots.remove(&key) {
            Some(slot) => slot,
            None => {
                self.order.push(key);
                Slot::default()
            }
        };
        if !slot.broken {
            self.fill(key, &mut slot, id, item, out);
        }
        self.slots.insert(key, slot);
    }

    fn fill(
        &mut self,
        key: u64,
        slot: &mut Slot,
        id: Option<&str>,
        item: &Value,
        out: &mut VecDeque<StreamEvent>,
    ) {
        let function = &item["function"];
        let name = function["name"].as_str().filter(|name| !name.is_empty());
        let arguments = match &function["arguments"] {
            Value::String(text) => Some(text.as_str()),
            Value::Null => None,
            // Arguments must be text the call is read from.
            _ => {
                slot.broken = true;
                self.calls.discard(key);
                return;
            }
        };
        let conflicting_id = match id {
            // An identifier that comes after the call was started under one
            // of this adapter's making would be a second one.
            Some(_) if slot.id.is_empty() => slot.started,
            Some(id) => slot.id != id,
            None => false,
        };
        let conflicting_name = name.is_some_and(|name| !slot.name.is_empty() && slot.name != name);
        if conflicting_id || conflicting_name {
            slot.broken = true;
            self.calls.discard(key);
            return;
        }
        if let Some(id) = id {
            id.clone_into(&mut slot.id);
        }
        if let Some(name) = name {
            name.clone_into(&mut slot.name);
        }
        if !slot.started && !slot.name.is_empty() {
            self.flush_reasoning(out);
            self.calls.start(key, &slot.id, &slot.name, out);
            slot.started = true;
            if !slot.pending.is_empty() {
                let pending = std::mem::take(&mut slot.pending);
                self.calls.fragment(key, &pending, out);
            }
        }
        if let Some(text) = arguments {
            if slot.started {
                self.calls.fragment(key, text, out);
            } else {
                slot.pending.push_str(text);
            }
        }
    }

    /// The end of the response: every call ends, and the finish reason is
    /// read.
    fn end(&mut self, out: &mut VecDeque<StreamEvent>) {
        self.finished = true;
        self.flush_reasoning(out);
        for key in std::mem::take(&mut self.order) {
            match self.slots.remove(&key) {
                Some(slot) if slot.started && !slot.broken => self.calls.end(key, out),
                // A call that never got a name cannot be delivered.
                Some(slot) if !slot.broken => self.calls.discard(key),
                _ => {}
            }
        }
        if !self.usage_seen {
            self.estimate(out);
        }
        let reason = match self.finish.take() {
            None => FinishReason::Error(
                ProviderError::new(ErrorKind::MalformedResponse)
                    .with_detail("the response ended without a finish reason"),
            ),
            Some(reason) => self.reason(&reason),
        };
        out.push_back(StreamEvent::Finish(self.calls.settle(reason)));
    }

    fn reason(&self, reason: &str) -> FinishReason {
        match reason {
            "stop" | "tool_calls" => {
                if !self.refusal.is_empty() {
                    FinishReason::Refusal(provider_error(
                        ErrorKind::Refusal,
                        Some(self.refusal.as_str()),
                    ))
                } else if self.calls.completed() > 0 {
                    FinishReason::ToolUse
                } else {
                    FinishReason::EndTurn
                }
            }
            "length" => FinishReason::OutputLimit,
            "content_filter" => FinishReason::Refusal(provider_error(
                ErrorKind::Refusal,
                Some("the service's content filter stopped the response"),
            )),
            // DeepSeek: "interrupted due to insufficient resource of the
            // inference system"; "generation was interrupted".
            "insufficient_system_resource" | "aborted" => FinishReason::Error(
                ProviderError::new(ErrorKind::Overloaded)
                    .with_detail(format!("the service ended the response: {reason}")),
            ),
            "error" => FinishReason::Error(
                ProviderError::new(ErrorKind::Overloaded)
                    .with_detail("the service reported an error in the stream"),
            ),
            other => FinishReason::Error(
                ProviderError::new(ErrorKind::MalformedResponse)
                    .with_detail(format!("the response ended for a reason not known: {other}")),
            ),
        }
    }
}

impl Decoder for ChatDecoder {
    fn decode(&mut self, event: &SseEvent, out: &mut VecDeque<StreamEvent>) {
        if self.finished {
            return;
        }
        let Some(data) = parse_data(event, out) else {
            self.finished = true;
            return;
        };
        // An error after the stream began arrives as a chunk with an `error`.
        if !data["error"].is_null() {
            self.finished = true;
            // What was produced before the error may be billed.
            if !self.usage_seen {
                self.estimate(out);
            }
            out.push_back(StreamEvent::Finish(failure(error_from(
                &data["error"],
                None,
                ErrorKind::Overloaded,
            ))));
            return;
        }
        for choice in data["choices"].as_array().into_iter().flatten() {
            // One choice is asked for; any other is not this conversation.
            if choice["index"].as_u64().unwrap_or(0) != 0 {
                continue;
            }
            let delta = &choice["delta"];
            for field in REASONING_FIELDS {
                if let Some(text) = delta[field].as_str()
                    && !text.is_empty()
                {
                    self.add_reasoning(field, text);
                }
            }
            match &delta["content"] {
                Value::String(text) if !text.is_empty() => {
                    self.flush_reasoning(out);
                    out.push_back(StreamEvent::TextDelta { text: text.clone() });
                }
                // A list of parts: its text is text; anything else, such as a
                // thinking part, is not the answer and is left out.
                Value::Array(parts) => {
                    for part in parts {
                        if part["type"] == "text"
                            && let Some(text) = part["text"].as_str()
                            && !text.is_empty()
                        {
                            self.flush_reasoning(out);
                            out.push_back(StreamEvent::TextDelta {
                                text: text.to_owned(),
                            });
                        }
                    }
                }
                _ => {}
            }
            if let Some(text) = delta["refusal"].as_str() {
                self.refusal.push_str(text);
            }
            for item in delta["tool_calls"].as_array().into_iter().flatten() {
                self.tool_delta(item, out);
            }
            if let Some(reason) = choice["finish_reason"].as_str()
                && self.finish.is_none()
            {
                self.finish = Some(reason.to_owned());
            }
        }
        if data["usage"].is_object() {
            self.usage_seen = true;
            out.push_back(StreamEvent::Usage(usage(&data["usage"])));
        }
    }

    fn done(&mut self, out: &mut VecDeque<StreamEvent>) {
        if !self.finished {
            self.end(out);
        }
    }

    fn interrupted(&mut self, out: &mut VecDeque<StreamEvent>) {
        if !self.finished && !self.usage_seen {
            self.estimate(out);
        }
    }
}

/// The neutral usage of a usage object. The cached part is where OpenAI and
/// DeepSeek put it, or where some services put it flat.
fn usage(usage: &Value) -> Usage {
    let cached = usage["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .or_else(|| usage["prompt_cache_hit_tokens"].as_u64())
        .or_else(|| usage["cached_tokens"].as_u64())
        .unwrap_or(0);
    Usage {
        input_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0),
        output_tokens: usage["completion_tokens"].as_u64().unwrap_or(0),
        cached_input_tokens: cached,
        estimated: false,
    }
}

/// A refusal stays a refusal; anything else is a failure.
fn failure(error: ProviderError) -> FinishReason {
    if error.kind() == ErrorKind::Refusal {
        FinishReason::Refusal(error)
    } else {
        FinishReason::Error(error)
    }
}

/// A typed error from an error object. `status` is the HTTP status, if
/// there was one; a service may also put a status number in `code`.
/// `fallback` is the kind when nothing more precise is known.
fn error_from(error: &Value, status: Option<u16>, fallback: ErrorKind) -> ProviderError {
    let message = error["message"].as_str().or_else(|| error.as_str());
    let code_number = error["code"]
        .as_u64()
        .and_then(|code| u16::try_from(code).ok());
    let kind = match error["code"].as_str().unwrap_or("") {
        "rate_limit_exceeded" | "slow_down" => ErrorKind::RateLimited,
        "insufficient_quota" | "credit_balance_exhausted" => ErrorKind::QuotaExhausted,
        "server_error" | "server_is_overloaded" => ErrorKind::Overloaded,
        _ => match (error["type"].as_str().unwrap_or(""), status.or(code_number)) {
            ("insufficient_quota", _) => ErrorKind::QuotaExhausted,
            (_, Some(status)) => kind_for_status(status),
            ("rate_limit_error", None) => ErrorKind::RateLimited,
            ("invalid_request_error", None) => ErrorKind::RequestRejected,
            (_, None) => fallback,
        },
    };
    provider_error(kind, message)
}

fn classify(status: u16, body: &str) -> ProviderError {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error = if parsed["error"].is_null() {
        &parsed
    } else {
        &parsed["error"]
    };
    error_from(error, Some(status), ErrorKind::RequestRejected)
}

#[cfg(test)]
mod tests {
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

    fn reasoning(text: &str) -> Part {
        Part::Opaque(OpaquePart {
            provider: ProviderId::OpenAiChat,
            data: json!({ "reasoning_content": text }),
        })
    }

    #[test]
    fn a_plain_request_has_only_the_documented_fields() {
        let body = request_body(&request(vec![Message::user_text("Hello")]), false).unwrap();
        assert_eq!(
            body,
            json!({
                "model": "example-model",
                "messages": [
                    { "role": "system", "content": "Be brief." },
                    { "role": "user", "content": "Hello" }
                ],
                "max_tokens": 512,
                "stream": true,
                "stream_options": { "include_usage": true }
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
        let body = request_body(&request, false).unwrap();
        assert_eq!(
            body["tools"],
            json!([{
                "type": "function",
                "function": {
                    "name": "storage_list_volumes",
                    "description": "List volumes.",
                    "parameters": schema
                }
            }])
        );
        assert_eq!(body["tool_choice"], "auto");
    }

    fn round_trip() -> Vec<Message> {
        let mut arguments = Map::new();
        arguments.insert("volume_id".to_owned(), json!("vol-1"));
        vec![
            Message::user_text("Unmount it."),
            Message::assistant(vec![
                reasoning("The person wants "),
                Part::Opaque(OpaquePart {
                    provider: ProviderId::Anthropic,
                    data: json!({ "type": "thinking" }),
                }),
                reasoning("it unmounted."),
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
        ]
    }

    #[test]
    fn a_tool_round_trip_uses_tool_calls_and_tool_messages() {
        let body = request_body(&request(round_trip()), false).unwrap();
        assert_eq!(
            body["messages"],
            json!([
                { "role": "system", "content": "Be brief." },
                { "role": "user", "content": "Unmount it." },
                { "role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_1", "type": "function",
                    "function": { "name": "storage_unmount", "arguments": "{\"volume_id\":\"vol-1\"}" }
                }] },
                { "role": "tool", "tool_call_id": "call_1", "content": "done" },
                { "role": "user", "content": "And then list them." }
            ])
        );
        // Reasoning is not sent back unless the service needs it.
        assert!(!body.to_string().contains("reasoning"));
    }

    #[test]
    fn reasoning_goes_back_only_where_the_service_requires_it() {
        let body = request_body(&request(round_trip()), true).unwrap();
        let assistant = &body["messages"][2];
        assert_eq!(assistant["reasoning_content"], "The person wants it unmounted.");
        assert_eq!(assistant["tool_calls"][0]["id"], "call_1");
        // Another family's content is never sent.
        assert!(!body.to_string().contains("thinking"));
        // A turn that is only reasoning still goes back.
        let body = request_body(
            &request(vec![
                Message::user_text("Hi"),
                Message::assistant(vec![reasoning("hm")]),
                Message::user_text("Well?"),
            ]),
            true,
        )
        .unwrap();
        assert_eq!(
            body["messages"][2],
            json!({ "role": "assistant", "content": "", "reasoning_content": "hm" })
        );
    }

    #[test]
    fn images_are_data_urls_and_not_allowed_in_tool_results() {
        let image = Image::new("image/png", "AAAA");
        let body = request_body(
            &request(vec![Message::user(vec![
                Part::text("What is this?"),
                Part::Image(image.clone()),
            ])]),
            false,
        )
        .unwrap();
        assert_eq!(
            body["messages"][1],
            json!({ "role": "user", "content": [
                { "type": "text", "text": "What is this?" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
            ] })
        );
        let error = request_body(
            &request(vec![Message::user(vec![Part::ToolResult(ToolResult {
                call_id: "call_1".to_owned(),
                name: "screen_capture".to_owned(),
                content: vec![ResultContent::Image(image)],
                is_error: false,
            })])]),
            false,
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::RequestRejected);
    }

    #[test]
    fn parts_in_the_wrong_role_are_rejected_before_anything_is_sent() {
        for message in [
            Message::user(vec![reasoning("x")]),
            Message::assistant(vec![Part::Image(Image::new("image/png", "AAAA"))]),
            Message::user(vec![Part::Image(Image::new("image/png", "not base64"))]),
        ] {
            let error = request_body(&request(vec![message]), false).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::RequestRejected);
        }
    }

    #[test]
    fn the_estimate_counts_text_high_and_images_at_a_fixed_figure() {
        let text_only = request(vec![Message::user_text("Hello")]);
        let body = request_body(&text_only, false).unwrap();
        let bytes = body.to_string().len() as u64;
        assert_eq!(input_estimate(&body, &text_only), bytes.div_ceil(2));

        let big = "A".repeat(40_000);
        let with_image = request(vec![Message::user(vec![Part::Image(Image::new(
            "image/png",
            big.as_str(),
        ))])]);
        let body = request_body(&with_image, false).unwrap();
        let estimate = input_estimate(&body, &with_image);
        // The image data itself is not counted as text.
        assert!(estimate < 3_000 + 200, "{estimate}");
        assert!(estimate >= ESTIMATE_TOKENS_PER_IMAGE);
    }

    fn decode(chunks: &[&str]) -> Vec<StreamEvent> {
        let mut decoder = ChatDecoder::new(100, 50);
        let mut out = VecDeque::new();
        for chunk in chunks {
            if *chunk == "[DONE]" {
                decoder.done(&mut out);
            } else {
                decoder.decode(
                    &SseEvent {
                        event: None,
                        data: (*chunk).to_owned(),
                    },
                    &mut out,
                );
            }
        }
        out.into_iter().collect()
    }

    fn finish_of(events: &[StreamEvent]) -> &FinishReason {
        match events.last() {
            Some(StreamEvent::Finish(reason)) => reason,
            other => panic!("not a finish: {other:?}"),
        }
    }

    fn ended_calls(events: &[StreamEvent]) -> Vec<(String, String, Value)> {
        events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ToolCallEnd { call, .. } => Some((
                    call.id.clone(),
                    call.name.clone(),
                    Value::Object(call.arguments.clone()),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn calls_without_an_index_are_told_apart_by_their_identifier() {
        let events = decode(&[
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"id":"a","type":"function","function":{"name":"one","arguments":"{\"x\":"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"function":{"arguments":"1}"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"id":"b","type":"function","function":{"name":"two","arguments":"{}"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}"#,
            "[DONE]",
        ]);
        assert_eq!(
            ended_calls(&events),
            vec![
                ("a".to_owned(), "one".to_owned(), json!({ "x": 1 })),
                ("b".to_owned(), "two".to_owned(), json!({})),
            ]
        );
        assert_eq!(finish_of(&events), &FinishReason::ToolUse);
    }

    #[test]
    fn a_call_the_service_describes_two_ways_is_never_delivered() {
        for second in [
            // Another name for the same call.
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"other","arguments":"1}"}}]}}]}"#,
            // Another identifier for the same call.
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"b","function":{"arguments":"1}"}}]}}]}"#,
            // Arguments that are not text.
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":{"x":1}}}]}}]}"#,
        ] {
            let events = decode(&[
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"one","arguments":"{\"x\":"}}]}}]}"#,
                second,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
                "[DONE]",
            ]);
            assert!(ended_calls(&events).is_empty(), "{second}");
            let error = finish_of(&events).error().expect("an error");
            assert_eq!(error.kind(), ErrorKind::MalformedResponse, "{second}");
        }
    }

    #[test]
    fn arguments_before_the_name_wait_for_it_and_a_call_without_one_is_not_delivered() {
        let events = decode(&[
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"a","function":{"arguments":"{\"x\":"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"one","arguments":"2}"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        assert_eq!(
            ended_calls(&events),
            vec![("a".to_owned(), "one".to_owned(), json!({ "x": 2 }))]
        );
        let events = decode(&[
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"a","function":{"arguments":"{}"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        assert!(ended_calls(&events).is_empty());
        assert!(finish_of(&events).error().is_some());
    }

    #[test]
    fn text_in_parts_is_text_and_a_thinking_part_is_not() {
        let events = decode(&[
            r#"{"choices":[{"index":0,"delta":{"content":[{"type":"thinking","thinking":[{"type":"text","text":"secret plan"}]}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"content":[{"type":"text","text":"Hello"}]}}]}"#,
            r#"{"choices":[{"index":1,"delta":{"content":"another choice"}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}"#,
            "[DONE]",
        ]);
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hello");
        assert_eq!(finish_of(&events), &FinishReason::EndTurn);
    }

    #[test]
    fn finish_reasons_map_to_the_neutral_ones() {
        for (reason, expected) in [
            ("stop", Some(FinishReason::EndTurn)),
            ("length", Some(FinishReason::OutputLimit)),
            ("content_filter", None),
            ("insufficient_system_resource", None),
            ("function_call", None),
            ("a_reason_added_later", None),
        ] {
            let chunk = format!(
                r#"{{"choices":[{{"index":0,"delta":{{"content":"Hi"}},"finish_reason":"{reason}"}}]}}"#
            );
            let events = decode(&[chunk.as_str(), "[DONE]"]);
            let finish = finish_of(&events);
            match expected {
                Some(expected) => assert_eq!(finish, &expected, "{reason}"),
                None => {
                    let error = finish.error().expect("an error");
                    let kind = match reason {
                        "content_filter" => ErrorKind::Refusal,
                        "insufficient_system_resource" => ErrorKind::Overloaded,
                        _ => ErrorKind::MalformedResponse,
                    };
                    assert_eq!(error.kind(), kind, "{reason}");
                    if reason == "content_filter" {
                        assert!(matches!(finish, FinishReason::Refusal(_)));
                    }
                }
            }
        }
        // No finish reason before the end is malformed.
        let events = decode(&[r#"{"choices":[{"index":0,"delta":{"content":"Hi"}}]}"#, "[DONE]"]);
        assert_eq!(
            finish_of(&events).error().map(ProviderError::kind),
            Some(ErrorKind::MalformedResponse)
        );
        // A refusal field makes a refusal.
        let events = decode(&[
            r#"{"choices":[{"index":0,"delta":{"refusal":"I can't."},"finish_reason":"stop"}]}"#,
            "[DONE]",
        ]);
        assert!(matches!(finish_of(&events), FinishReason::Refusal(_)));
    }

    #[test]
    fn a_response_without_usage_is_counted_at_a_marked_estimate() {
        let events = decode(&[
            r#"{"choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":"stop"}]}"#,
            "[DONE]",
        ]);
        let usage = events
            .iter()
            .find_map(|event| match event {
                StreamEvent::Usage(usage) => Some(*usage),
                _ => None,
            })
            .expect("a usage");
        assert_eq!(
            usage,
            Usage {
                input_tokens: 100,
                output_tokens: 50,
                cached_input_tokens: 0,
                estimated: true,
            }
        );
        // With usage, no estimate, and the cached part where DeepSeek puts it.
        let events = decode(&[
            r#"{"choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":30,"completion_tokens":4,"total_tokens":34,"prompt_cache_hit_tokens":20,"prompt_cache_miss_tokens":10}}"#,
            "[DONE]",
        ]);
        let usages: Vec<Usage> = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::Usage(usage) => Some(*usage),
                _ => None,
            })
            .collect();
        assert_eq!(
            usages,
            vec![Usage {
                input_tokens: 30,
                output_tokens: 4,
                cached_input_tokens: 20,
                estimated: false,
            }]
        );
    }

    #[test]
    fn error_bodies_map_to_kinds() {
        for (status, body, expected) in [
            (
                401,
                r#"{"error":{"message":"m","type":"authentication_error","code":"invalid_api_key"}}"#,
                ErrorKind::AuthenticationFailed,
            ),
            (
                402,
                r#"{"error":{"message":"m","type":"unknown_error","code":"invalid_request_error"}}"#,
                ErrorKind::QuotaExhausted,
            ),
            (
                429,
                r#"{"error":{"message":"m","type":"rate_limit_error","code":"rate_limit_exceeded"}}"#,
                ErrorKind::RateLimited,
            ),
            (
                429,
                r#"{"error":{"message":"m","type":"insufficient_quota","code":null}}"#,
                ErrorKind::QuotaExhausted,
            ),
            (422, r#"{"error":{"message":"m"}}"#, ErrorKind::RequestRejected),
            (503, r#"{"error":{"message":"m"}}"#, ErrorKind::Overloaded),
            (500, r#"{"error":"m"}"#, ErrorKind::Overloaded),
        ] {
            let error = classify(status, body);
            assert_eq!(error.kind(), expected, "{status} {body}");
            assert_eq!(error.detail(), Some("m"), "{status} {body}");
        }
        assert_eq!(classify(502, "bad gateway").kind(), ErrorKind::Overloaded);
        // In a stream, a status number in `code` is read as one.
        let error = error_from(
            &json!({ "code": 429, "message": "slow" }),
            None,
            ErrorKind::Overloaded,
        );
        assert_eq!(error.kind(), ErrorKind::RateLimited);
        let error = error_from(&json!({ "message": "?" }), None, ErrorKind::Overloaded);
        assert_eq!(error.kind(), ErrorKind::Overloaded);
    }
}
