// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The provider-neutral conversation model.
//!
//! A conversation is a list of [`Message`]s, each a list of [`Part`]s. A
//! [`Request`] adds the model name, the system text, the tool definitions and
//! the output limit. A provider answers with a stream of [`StreamEvent`]s that
//! always ends with exactly one [`StreamEvent::Finish`].
//!
//! The shape was chosen after reading the three providers' current interfaces,
//! so that each maps onto it without loss. The reasons are in
//! `docs/architecture/pax-providers.md`.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{ErrorKind, ProviderError};

/// The longest tool name every supported provider accepts, in bytes.
pub const TOOL_NAME_MAX: usize = 64;

/// An adapter family of this crate: one wire format, spoken by the adapter
/// written for it.
///
/// The name is historical. Three families are one company's own interface
/// and carry its name; the fourth, [`ProviderId::OpenAiChat`], is the
/// OpenAI-compatible Chat Completions interface that many services and
/// local servers offer (ADR-0004, amended 12 November 2026). Which
/// *service* a configuration talks to is data: its base URL here, and in the
/// session service a provider entry with an identifier of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// Anthropic (Claude), through the Messages API.
    Anthropic,
    /// OpenAI, through the Responses API.
    OpenAi,
    /// Google (Gemini), through the Interactions API.
    Google,
    /// Any service that offers the OpenAI-compatible Chat Completions API:
    /// DeepSeek, other cloud services, a server on this computer. It has no
    /// public address of its own: a configuration must name one.
    #[serde(rename = "openai-chat")]
    OpenAiChat,
}

impl ProviderId {
    /// Every adapter family, in the priority order of the adapters.
    pub const ALL: [ProviderId; 4] = [
        ProviderId::Anthropic,
        ProviderId::OpenAi,
        ProviderId::Google,
        ProviderId::OpenAiChat,
    ];

    /// The family's name as it appears in configuration.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProviderId::Anthropic => "anthropic",
            ProviderId::OpenAi => "openai",
            ProviderId::Google => "google",
            ProviderId::OpenAiChat => "openai-chat",
        }
    }

    /// Whether the family is one company's own interface, with a public
    /// address of its own. The compatible family is not.
    pub const fn is_company_interface(self) -> bool {
        !matches!(self, ProviderId::OpenAiChat)
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who a message is from.
///
/// There is no system role: the system text is a field of the [`Request`], which
/// is where every provider takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The person, and everything the orchestrator sends on their behalf,
    /// including tool results.
    User,
    /// The model.
    Assistant,
}

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Who the message is from.
    pub role: Role,
    /// The parts, in order.
    pub content: Vec<Part>,
}

impl Message {
    /// A user message with the given parts.
    pub fn user(content: Vec<Part>) -> Self {
        Message {
            role: Role::User,
            content,
        }
    }

    /// A user message holding one text part.
    pub fn user_text(text: impl Into<String>) -> Self {
        Message::user(vec![Part::text(text)])
    }

    /// An assistant message with the given parts.
    pub fn assistant(content: Vec<Part>) -> Self {
        Message {
            role: Role::Assistant,
            content,
        }
    }
}

/// One part of a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    /// Plain text.
    Text {
        /// The text.
        text: String,
    },
    /// An image. Valid in a user message.
    Image(Image),
    /// A tool call the model made. Valid in an assistant message.
    ToolCall(ToolCall),
    /// The result of a tool call. Valid in a user message. This is the only
    /// place untrusted content (file text, command output, screen text) goes.
    ToolResult(ToolResult),
    /// Provider-specific content that must be sent back unchanged. Valid in an
    /// assistant message.
    Opaque(OpaquePart),
}

impl Part {
    /// A text part.
    pub fn text(text: impl Into<String>) -> Self {
        Part::Text { text: text.into() }
    }
}

/// An image, held as base64 text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    /// The media type, for example `image/png`.
    pub media_type: String,
    /// The image bytes in standard base64, without line breaks.
    pub data_base64: String,
}

impl Image {
    /// An image from its media type and base64 data.
    pub fn new(media_type: impl Into<String>, data_base64: impl Into<String>) -> Self {
        Image {
            media_type: media_type.into(),
            data_base64: data_base64.into(),
        }
    }

    /// Check that the media type and the data are safe to place in a request.
    pub(crate) fn check(&self) -> Result<(), ProviderError> {
        let subtype = self.media_type.strip_prefix("image/").unwrap_or("");
        let media_ok = !subtype.is_empty()
            && subtype.len() <= 32
            && subtype
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
        let data_ok = !self.data_base64.is_empty()
            && self
                .data_base64
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
        if media_ok && data_ok {
            Ok(())
        } else {
            Err(rejected(
                "an image part needs an image/* media type and standard base64 data",
            ))
        }
    }
}

/// A tool call made by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The provider's identifier for this call. The result refers to it.
    pub id: String,
    /// The name of the tool.
    pub name: String,
    /// The arguments, always a JSON object.
    pub arguments: Map<String, Value>,
}

/// The result of a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// The identifier of the call this answers.
    pub call_id: String,
    /// The name of the tool that was called. One provider requires it.
    pub name: String,
    /// What the tool returned.
    pub content: Vec<ResultContent>,
    /// Whether the call failed.
    pub is_error: bool,
}

impl ToolResult {
    /// A successful result holding one text.
    pub fn text(
        call_id: impl Into<String>,
        name: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        ToolResult {
            call_id: call_id.into(),
            name: name.into(),
            content: vec![ResultContent::Text { text: text.into() }],
            is_error: false,
        }
    }

    /// A failed result holding one text that says what went wrong.
    pub fn error(
        call_id: impl Into<String>,
        name: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        ToolResult {
            is_error: true,
            ..ToolResult::text(call_id, name, text)
        }
    }
}

/// One piece of a tool result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResultContent {
    /// Text.
    Text {
        /// The text.
        text: String,
    },
    /// An image, for example a screenshot.
    Image(Image),
}

/// Provider-specific content that must be sent back unchanged.
///
/// Each provider returns some record of the model's reasoning that it expects
/// in the next request: a thinking block, a reasoning item or a thought step.
/// The orchestrator does not interpret it. An adapter sends back the parts of
/// its own provider and leaves out the parts of another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpaquePart {
    /// The provider that produced the content.
    pub provider: ProviderId,
    /// The content, as the provider sent it.
    pub data: Value,
}

/// A tool offered to the model: a name, a description and a JSON Schema for the
/// input.
///
/// These are the three fields of a tool in the reviewed tool manifest
/// (`services/tur-pax`), so a definition is built from a manifest tool with
/// `ToolDefinition::new(tool.name(), tool.description(), tool.input_schema().clone())`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

impl ToolDefinition {
    /// A tool definition.
    ///
    /// The name must be acceptable to every supported provider: 1 to
    /// [`TOOL_NAME_MAX`] characters from `A-Z`, `a-z`, `0-9`, `_` and `-`. The
    /// input schema must be a JSON Schema of type `object`.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Result<Self, ProviderError> {
        let name = name.into();
        let description = description.into();
        let name_ok = !name.is_empty()
            && name.len() <= TOOL_NAME_MAX
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !name_ok {
            return Err(rejected(
                "a tool name must be 1 to 64 characters from A-Z, a-z, 0-9, underscore and dash",
            ));
        }
        if description.trim().is_empty() {
            return Err(rejected("a tool needs a description"));
        }
        if input_schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err(rejected(
                "a tool input schema must be a JSON Schema of type \"object\"",
            ));
        }
        Ok(ToolDefinition {
            name,
            description,
            input_schema,
        })
    }

    /// The tool name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The description given to the model.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The JSON Schema of the input.
    pub fn input_schema(&self) -> &Value {
        &self.input_schema
    }
}

/// One request to a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The model name, from configuration.
    pub model: String,
    /// The system text: the instructions. Never contains untrusted content.
    pub system: Option<String>,
    /// The conversation so far.
    pub messages: Vec<Message>,
    /// The tools offered to the model.
    pub tools: Vec<ToolDefinition>,
    /// The most output tokens the model may produce for this request.
    pub max_output_tokens: u32,
}

/// Token usage of one response.
///
/// Each [`StreamEvent::Usage`] is a snapshot of the response so far, not an
/// increment: the last one of a stream is the usage of the whole response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Every input token, including the ones read from the provider's cache.
    pub input_tokens: u64,
    /// Every output token, including the model's reasoning.
    pub output_tokens: u64,
    /// The part of `input_tokens` that was read from the provider's cache.
    pub cached_input_tokens: u64,
    /// The counts are not the provider's. The service sent no usage, so the
    /// adapter estimated them high, from the size of the request and its
    /// output limit, rather than count a response as costing nothing. Shown
    /// as an estimate wherever it is shown.
    #[serde(default, skip_serializing_if = "is_false")]
    pub estimated: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Usage {
    /// Input and output tokens together.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// The sum of two usages. A sum with an estimate in it is an estimate.
    pub fn plus(&self, other: &Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens.saturating_add(other.input_tokens),
            output_tokens: self.output_tokens.saturating_add(other.output_tokens),
            cached_input_tokens: self
                .cached_input_tokens
                .saturating_add(other.cached_input_tokens),
            estimated: self.estimated || other.estimated,
        }
    }
}

/// One event of a streamed response.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// More text of the answer.
    TextDelta {
        /// The new text.
        text: String,
    },
    /// The model started a tool call.
    ToolCallStart {
        /// The position of the call among the calls of this response, from 0.
        index: usize,
        /// The provider's identifier for the call.
        id: String,
        /// The name of the tool.
        name: String,
    },
    /// More of the arguments of a tool call, as a fragment of JSON text.
    ToolCallArgumentsDelta {
        /// The position of the call among the calls of this response.
        index: usize,
        /// The new fragment.
        fragment: String,
    },
    /// A tool call is complete. Its arguments were read strictly
    /// ([`crate::strict`]): they are one JSON object in which no key is
    /// repeated at any depth. A call whose arguments are anything else is
    /// never delivered, and makes the response malformed.
    ToolCallEnd {
        /// The position of the call among the calls of this response.
        index: usize,
        /// The complete call.
        call: ToolCall,
        /// The text the arguments were read from: what the provider sent,
        /// whether in fragments or in one piece, or `{}` if it sent none.
        /// Where a provider sends the arguments as a JSON object and not as
        /// text, this is that object written out. Read strictly, it gives
        /// exactly the arguments of `call`.
        arguments_text: String,
    },
    /// Provider-specific content to send back unchanged in the next request.
    Opaque(OpaquePart),
    /// The usage of the response so far.
    Usage(Usage),
    /// The response is over. Always the last event, and always present.
    Finish(FinishReason),
}

/// Why a response ended.
#[derive(Debug, Clone, PartialEq)]
pub enum FinishReason {
    /// The model finished its answer.
    EndTurn,
    /// The model made one or more tool calls and waits for their results.
    ToolUse,
    /// The output limit, or the model's context limit, was reached. The answer
    /// is cut short.
    OutputLimit,
    /// The model or the provider's safety system declined. Anything streamed in
    /// this response is incomplete and must be discarded.
    Refusal(ProviderError),
    /// The call failed. Anything streamed in this response is incomplete and
    /// must be discarded.
    Error(ProviderError),
}

impl FinishReason {
    /// The error carried by a refusal or a failure.
    pub fn error(&self) -> Option<&ProviderError> {
        match self {
            FinishReason::Refusal(error) | FinishReason::Error(error) => Some(error),
            FinishReason::EndTurn | FinishReason::ToolUse | FinishReason::OutputLimit => None,
        }
    }

    /// Remove every occurrence of `secret` from the detail of a carried error.
    pub(crate) fn scrubbed(self, secret: &str) -> FinishReason {
        match self {
            FinishReason::Refusal(error) => FinishReason::Refusal(error.scrubbed(secret)),
            FinishReason::Error(error) => FinishReason::Error(error.scrubbed(secret)),
            other => other,
        }
    }

    /// A short name for a log or a terminal.
    pub fn name(&self) -> &'static str {
        match self {
            FinishReason::EndTurn => "end_turn",
            FinishReason::ToolUse => "tool_use",
            FinishReason::OutputLimit => "output_limit",
            FinishReason::Refusal(_) => "refusal",
            FinishReason::Error(_) => "error",
        }
    }
}

/// A whole response, assembled from its events.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// The assistant message, with its parts in the order they arrived. It can
    /// be appended to the conversation as it is.
    pub message: Message,
    /// The usage of the response.
    pub usage: Usage,
    /// Why the response ended.
    pub finish: FinishReason,
}

impl Turn {
    /// Assemble a response from its events.
    ///
    /// Text deltas are joined, completed tool calls and opaque parts are kept in
    /// order, and the last usage snapshot is taken. Events after the finish are
    /// not read.
    pub fn collect(events: impl IntoIterator<Item = StreamEvent>) -> Turn {
        let mut content: Vec<Part> = Vec::new();
        let mut usage = Usage::default();
        let mut finish = None;
        for event in events {
            match event {
                StreamEvent::TextDelta { text } => match content.last_mut() {
                    Some(Part::Text { text: existing }) => existing.push_str(&text),
                    _ => content.push(Part::Text { text }),
                },
                StreamEvent::ToolCallEnd { call, .. } => content.push(Part::ToolCall(call)),
                StreamEvent::Opaque(part) => content.push(Part::Opaque(part)),
                StreamEvent::Usage(latest) => usage = latest,
                StreamEvent::Finish(reason) => {
                    finish = Some(reason);
                    break;
                }
                StreamEvent::ToolCallStart { .. } | StreamEvent::ToolCallArgumentsDelta { .. } => {}
            }
        }
        let finish = finish.unwrap_or_else(|| {
            FinishReason::Error(
                ProviderError::new(ErrorKind::MalformedResponse)
                    .with_detail("the stream ended without a finish event"),
            )
        });
        Turn {
            message: Message::assistant(content),
            usage,
            finish,
        }
    }

    /// All text of the response, joined.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for part in &self.message.content {
            if let Part::Text { text: piece } = part {
                text.push_str(piece);
            }
        }
        text
    }

    /// The completed tool calls, in order.
    pub fn tool_calls(&self) -> Vec<&ToolCall> {
        self.message
            .content
            .iter()
            .filter_map(|part| match part {
                Part::ToolCall(call) => Some(call),
                _ => None,
            })
            .collect()
    }
}

pub(crate) fn rejected(detail: &str) -> ProviderError {
    ProviderError::new(ErrorKind::RequestRejected).with_detail(detail)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn provider_names_match_their_serialised_form() {
        for provider in ProviderId::ALL {
            assert_eq!(json!(provider), json!(provider.as_str()));
            let parsed: ProviderId = serde_json::from_value(json!(provider.as_str())).unwrap();
            assert_eq!(parsed, provider);
            assert_eq!(provider.to_string(), provider.as_str());
        }
    }

    #[test]
    fn messages_round_trip_through_json() {
        let mut arguments = Map::new();
        arguments.insert("volume_id".to_owned(), json!("vol-1"));
        let message = Message::assistant(vec![
            Part::Opaque(OpaquePart {
                provider: ProviderId::Anthropic,
                data: json!({ "type": "thinking", "thinking": "", "signature": "s" }),
            }),
            Part::text("One moment."),
            Part::ToolCall(ToolCall {
                id: "call-1".to_owned(),
                name: "storage_unmount".to_owned(),
                arguments,
            }),
        ]);
        let text = serde_json::to_string(&message).unwrap();
        let back: Message = serde_json::from_str(&text).unwrap();
        assert_eq!(back, message);

        let result = Message::user(vec![
            Part::ToolResult(ToolResult {
                call_id: "call-1".to_owned(),
                name: "storage_unmount".to_owned(),
                content: vec![
                    ResultContent::Text {
                        text: "done".to_owned(),
                    },
                    ResultContent::Image(Image::new("image/png", "AAAA")),
                ],
                is_error: false,
            }),
            Part::Image(Image::new("image/jpeg", "BBBB")),
        ]);
        let text = serde_json::to_string(&result).unwrap();
        let back: Message = serde_json::from_str(&text).unwrap();
        assert_eq!(back, result);
    }

    #[test]
    fn tool_definitions_are_validated() {
        let schema = json!({ "type": "object", "properties": {}, "additionalProperties": false });
        let tool =
            ToolDefinition::new("storage_list_volumes", "List volumes.", schema.clone()).unwrap();
        assert_eq!(tool.name(), "storage_list_volumes");
        assert_eq!(tool.description(), "List volumes.");
        assert_eq!(tool.input_schema(), &schema);

        let too_long = "a".repeat(TOOL_NAME_MAX + 1);
        for name in [
            "",
            "has space",
            "dot.name",
            "naïve",
            "semi;colon",
            too_long.as_str(),
        ] {
            let error = ToolDefinition::new(name, "A tool.", schema.clone()).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::RequestRejected, "{name:?}");
        }
        assert!(ToolDefinition::new("ok", "  ", schema).is_err());
        for bad in [
            json!({}),
            json!({ "type": "string" }),
            json!([]),
            json!(true),
        ] {
            assert!(ToolDefinition::new("ok", "A tool.", bad).is_err());
        }
    }

    #[test]
    fn images_are_checked() {
        assert!(Image::new("image/png", "iVBORw0KGgo=").check().is_ok());
        assert!(Image::new("image/svg+xml", "AAAA").check().is_ok());
        for (media_type, data) in [
            ("text/plain", "AAAA"),
            ("image/", "AAAA"),
            ("image/png;base64,AAAA", "AAAA"),
            ("image/png", ""),
            ("image/png", "not base64!"),
            ("image/png", "AAAA\nBBBB"),
        ] {
            assert!(
                Image::new(media_type, data).check().is_err(),
                "{media_type} {data}"
            );
        }
    }

    #[test]
    fn a_turn_is_assembled_in_order() {
        let mut arguments = Map::new();
        arguments.insert("a".to_owned(), json!(1));
        let call = ToolCall {
            id: "c1".to_owned(),
            name: "tool".to_owned(),
            arguments,
        };
        let opaque = OpaquePart {
            provider: ProviderId::Google,
            data: json!({ "type": "thought", "signature": "s" }),
        };
        let events = vec![
            StreamEvent::Usage(Usage {
                input_tokens: 10,
                output_tokens: 1,
                cached_input_tokens: 0,
                estimated: false,
            }),
            StreamEvent::Opaque(opaque.clone()),
            StreamEvent::TextDelta {
                text: "Hello, ".to_owned(),
            },
            StreamEvent::TextDelta {
                text: "world".to_owned(),
            },
            StreamEvent::ToolCallStart {
                index: 0,
                id: "c1".to_owned(),
                name: "tool".to_owned(),
            },
            StreamEvent::ToolCallArgumentsDelta {
                index: 0,
                fragment: "{\"a\":1}".to_owned(),
            },
            StreamEvent::ToolCallEnd {
                index: 0,
                call: call.clone(),
                arguments_text: "{\"a\":1}".to_owned(),
            },
            StreamEvent::Usage(Usage {
                input_tokens: 10,
                output_tokens: 7,
                cached_input_tokens: 2,
                estimated: false,
            }),
            StreamEvent::Finish(FinishReason::ToolUse),
            StreamEvent::TextDelta {
                text: "never read".to_owned(),
            },
        ];
        let turn = Turn::collect(events);
        assert_eq!(
            turn.message,
            Message::assistant(vec![
                Part::Opaque(opaque),
                Part::text("Hello, world"),
                Part::ToolCall(call.clone()),
            ])
        );
        assert_eq!(turn.text(), "Hello, world");
        assert_eq!(turn.tool_calls(), vec![&call]);
        assert_eq!(turn.usage.output_tokens, 7);
        assert_eq!(turn.usage.total_tokens(), 17);
        assert_eq!(turn.finish, FinishReason::ToolUse);
        assert!(turn.finish.error().is_none());
    }

    #[test]
    fn a_stream_without_a_finish_is_an_error() {
        let turn = Turn::collect(vec![StreamEvent::TextDelta {
            text: "partial".to_owned(),
        }]);
        let error = turn.finish.error().expect("an error");
        assert_eq!(error.kind(), ErrorKind::MalformedResponse);
        assert_eq!(turn.finish.name(), "error");
    }

    #[test]
    fn usage_adds_and_saturates() {
        let a = Usage {
            input_tokens: 5,
            output_tokens: 6,
            cached_input_tokens: 1,
            estimated: false,
        };
        let b = Usage {
            input_tokens: u64::MAX,
            output_tokens: 1,
            cached_input_tokens: 2,
            estimated: false,
        };
        let sum = a.plus(&b);
        assert_eq!(sum.input_tokens, u64::MAX);
        assert_eq!(sum.output_tokens, 7);
        assert_eq!(sum.cached_input_tokens, 3);
        assert_eq!(sum.total_tokens(), u64::MAX);
        assert!(!sum.estimated);
        // A sum with an estimate in it is an estimate, and says so when it
        // is written out. A count that is the provider's says nothing more.
        let guessed = Usage {
            estimated: true,
            ..a
        };
        assert!(a.plus(&guessed).estimated);
        assert!(guessed.plus(&a).estimated);
        assert_eq!(serde_json::to_value(a).unwrap().get("estimated"), None);
        assert_eq!(serde_json::to_value(guessed).unwrap()["estimated"], true);
        let read: Usage = serde_json::from_value(
            json!({ "input_tokens": 1, "output_tokens": 2, "cached_input_tokens": 0 }),
        )
        .unwrap();
        assert!(!read.estimated);
    }

    #[test]
    fn the_compatible_family_has_its_own_name_and_no_company() {
        assert_eq!(ProviderId::OpenAiChat.as_str(), "openai-chat");
        assert_eq!(json!(ProviderId::OpenAiChat), json!("openai-chat"));
        assert!(!ProviderId::OpenAiChat.is_company_interface());
        for family in [ProviderId::Anthropic, ProviderId::OpenAi, ProviderId::Google] {
            assert!(family.is_company_interface(), "{family}");
        }
        assert!(serde_json::from_value::<ProviderId>(json!("openaichat")).is_err());
    }
}
