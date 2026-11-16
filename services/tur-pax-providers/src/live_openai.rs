// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `openai-realtime` live family: OpenAI's Realtime API over WebSocket.
//!
//! Written from OpenAI's documentation as read on 16 November 2026 (the
//! generally available interface; the `OpenAI-Beta: realtime=v1` header is
//! not sent). Never run against OpenAI.
//!
//! * Address: `wss://api.openai.com/v1/realtime?model=<model>`; the model
//!   is configuration.
//! * Key: `Authorization: Bearer <key>`, or another header by configuration
//!   (Azure OpenAI: `api-key`).
//! * Setup: one `session.update`: P@x's instructions, audio out only, 16-bit
//!   PCM at 24 kHz both ways, the server's voice detection (which also stops
//!   a response when the person speaks), transcription of what the person
//!   says, the tools as function tools.
//! * Out: `input_audio_buffer.append`, a user message plus
//!   `response.create`, a `function_call_output` item plus `response.create`,
//!   and `response.cancel` with `conversation.item.truncate` for barge-in.
//! * In: `response.output_audio.delta`, `response.output_audio_transcript.*`,
//!   `response.output_text.*`, `conversation.item.input_audio_transcription.*`,
//!   `input_audio_buffer.speech_started`, `response.function_call_arguments.done`,
//!   `response.done` (with its usage), `error`. Anything else is passed over.
//!
//! The message builders and the reader are plain functions, tested without a
//! network (`tests/live.rs`).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::error::{ErrorKind, ProviderError};
use crate::live::{
    LiveConfig, LiveEvent, LiveFamily, LiveSession, LiveUsage, audio_from_base64,
    audio_to_base64, key_header, malformed, read_call, resample, tool_result_text,
};
use crate::model::ToolResult;
use crate::secret::ApiKey;
use crate::ws::{Incoming, WebSocket, parse_url};

/// OpenAI's address of the Realtime API.
pub const DEFAULT_URL: &str = "wss://api.openai.com/v1/realtime";

/// The sample rate the family sends and receives.
pub const RATE: u32 = 24_000;

/// How long the handshake may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// The address of a session: the configured or public one, with the model.
pub fn session_url(config: &LiveConfig) -> String {
    let base = config.base_url.as_deref().unwrap_or(DEFAULT_URL);
    let joiner = if base.contains('?') { '&' } else { '?' };
    format!("{base}{joiner}model={}", config.model)
}

/// The `session.update` that sets the session up.
pub fn session_update(config: &LiveConfig) -> Value {
    let mut output = json!({ "format": { "type": "audio/pcm", "rate": RATE } });
    if let Some(voice) = &config.voice {
        output["voice"] = json!(voice);
    }
    let tools: Vec<Value> = config
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
    let mut session = json!({
        "type": "realtime",
        "model": config.model,
        "instructions": config.instructions,
        "output_modalities": ["audio"],
        "audio": {
            "input": {
                "format": { "type": "audio/pcm", "rate": RATE },
                "transcription": config
                    .transcription_model
                    .as_ref()
                    .map(|model| json!({ "model": model })),
                "turn_detection": {
                    "type": "server_vad",
                    "create_response": true,
                    "interrupt_response": true
                }
            },
            "output": output
        }
    });
    if !tools.is_empty() {
        session["tools"] = Value::Array(tools);
        session["tool_choice"] = json!("auto");
    }
    json!({ "type": "session.update", "session": session })
}

/// Audio at the family's rate, as one append message.
pub fn audio_message(samples_at_rate: &[i16]) -> Value {
    json!({ "type": "input_audio_buffer.append", "audio": audio_to_base64(samples_at_rate) })
}

/// The messages that send the person's typed words and ask for an answer.
pub fn text_messages(text: &str) -> Vec<Value> {
    vec![
        json!({
            "type": "conversation.item.create",
            "item": {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": text }]
            }
        }),
        json!({ "type": "response.create" }),
    ]
}

/// The messages that answer a tool call and let the model go on. A failed
/// call is sent as `{"error": …}`, so the model can tell it from a result.
pub fn tool_result_messages(result: &ToolResult) -> Vec<Value> {
    let text = tool_result_text(result);
    let output = if result.is_error {
        json!({ "error": text }).to_string()
    } else {
        text
    };
    vec![
        json!({
            "type": "conversation.item.create",
            "item": {
                "type": "function_call_output",
                "call_id": result.call_id,
                "output": output
            }
        }),
        json!({ "type": "response.create" }),
    ]
}

/// What the reader knows of the session between messages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReaderState {
    /// A response is being made.
    pub responding: bool,
    /// The item whose audio is being received, for a truncation.
    pub audio_item: Option<String>,
}

/// The messages of a barge-in: cancel the response in progress, and cut
/// the item being heard at what was actually played.
pub fn interrupt_messages(state: &ReaderState, played_ms: u64) -> Vec<Value> {
    let mut messages = Vec::new();
    if state.responding {
        messages.push(json!({ "type": "response.cancel" }));
    }
    if let Some(item) = &state.audio_item {
        messages.push(json!({
            "type": "conversation.item.truncate",
            "item_id": item,
            "content_index": 0,
            "audio_end_ms": played_ms
        }));
    }
    messages
}

fn count(value: &Value, path: &[&str]) -> u64 {
    let mut at = value;
    for key in path {
        match at.get(key) {
            Some(next) => at = next,
            None => return 0,
        }
    }
    at.as_u64().unwrap_or(0)
}

/// The usage of a `response.done`, with cached input apart.
pub fn usage_of(usage: &Value) -> LiveUsage {
    let audio_in = count(usage, &["input_token_details", "audio_tokens"]);
    let text_in = count(usage, &["input_token_details", "text_tokens"]);
    let cached = count(usage, &["input_token_details", "cached_tokens"]);
    let cached_audio = count(
        usage,
        &["input_token_details", "cached_tokens_details", "audio_tokens"],
    );
    let cached_text = count(
        usage,
        &["input_token_details", "cached_tokens_details", "text_tokens"],
    );
    // Without the breakdown, cached input is taken from text first.
    let (cached_audio, cached_text) = if cached_audio + cached_text == cached {
        (cached_audio, cached_text)
    } else {
        let from_text = cached.min(text_in);
        (cached - from_text, from_text)
    };
    LiveUsage {
        audio_input_tokens: audio_in.saturating_sub(cached_audio),
        audio_output_tokens: count(usage, &["output_token_details", "audio_tokens"]),
        text_input_tokens: text_in.saturating_sub(cached_text),
        text_output_tokens: count(usage, &["output_token_details", "text_tokens"]),
        cached_input_tokens: cached,
    }
}

fn error_event(error: &Value) -> Option<LiveEvent> {
    let code = error.get("code").and_then(Value::as_str).unwrap_or_default();
    let kind_text = error.get("type").and_then(Value::as_str).unwrap_or_default();
    // A cancel that arrives after the response ended is harmless: barge-in
    // races the model finishing.
    if code == "response_cancel_not_active" {
        return None;
    }
    let kind = match (kind_text, code) {
        (_, "invalid_api_key") | ("authentication_error", _) => ErrorKind::AuthenticationFailed,
        (_, "insufficient_quota") => ErrorKind::QuotaExhausted,
        (_, "rate_limit_exceeded") | ("rate_limit_error", _) => ErrorKind::RateLimited,
        ("server_error", _) => ErrorKind::Overloaded,
        _ => ErrorKind::RequestRejected,
    };
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("the live service reported an error");
    Some(LiveEvent::Closed {
        error: Some(ProviderError::new(kind).with_detail(message)),
    })
}

fn text_of(event: &Map<String, Value>, field: &str) -> String {
    event
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Read one server message. Every message is read strictly: a repeated key
/// anywhere makes it malformed, and the session ends.
pub fn parse_server_event(text: &str, state: &mut ReaderState) -> Vec<LiveEvent> {
    let Ok(Value::Object(event)) = crate::strict::value_from_str(text) else {
        return vec![malformed("a live message is not one JSON object with each key once")];
    };
    let kind = event.get("type").and_then(Value::as_str).unwrap_or_default();
    match kind {
        "response.created" => {
            state.responding = true;
            Vec::new()
        }
        "response.output_audio.delta" => {
            state.responding = true;
            if let Some(item) = event.get("item_id").and_then(Value::as_str) {
                state.audio_item = Some(item.to_owned());
            }
            match event.get("delta").and_then(Value::as_str).map(audio_from_base64) {
                Some(Some(samples)) => vec![LiveEvent::Audio { samples, rate: RATE }],
                _ => vec![malformed("a piece of the model's sound is not base64")],
            }
        }
        "response.output_audio_transcript.delta" | "response.output_text.delta" => {
            vec![LiveEvent::Said {
                text: text_of(&event, "delta"),
                final_text: false,
            }]
        }
        "response.output_audio_transcript.done" => vec![LiveEvent::Said {
            text: text_of(&event, "transcript"),
            final_text: true,
        }],
        "response.output_text.done" => vec![LiveEvent::Said {
            text: text_of(&event, "text"),
            final_text: true,
        }],
        "conversation.item.input_audio_transcription.delta" => vec![LiveEvent::Heard {
            text: text_of(&event, "delta"),
            final_text: false,
        }],
        "conversation.item.input_audio_transcription.completed" => vec![LiveEvent::Heard {
            text: text_of(&event, "transcript"),
            final_text: true,
        }],
        "input_audio_buffer.speech_started" => vec![LiveEvent::SpeechStarted],
        "response.function_call_arguments.done" => {
            let id = text_of(&event, "call_id");
            let name = text_of(&event, "name");
            let arguments = text_of(&event, "arguments");
            vec![read_call(&id, &name, &arguments)]
        }
        "response.done" => {
            state.responding = false;
            state.audio_item = None;
            let response = event.get("response").cloned().unwrap_or(Value::Null);
            let mut events = Vec::new();
            if let Some(usage) = response.get("usage") {
                events.push(LiveEvent::Usage(usage_of(usage)));
            }
            if response.get("status").and_then(Value::as_str) == Some("failed") {
                let error = response
                    .get("status_details")
                    .and_then(|details| details.get("error"))
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "server_error" }));
                events.extend(error_event(&error));
            } else {
                events.push(LiveEvent::TurnDone);
            }
            events
        }
        "error" => event
            .get("error")
            .and_then(error_event)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// A session of the family.
pub struct OpenAiRealtime {
    socket: WebSocket,
    key: ApiKey,
    state: ReaderState,
    pending: VecDeque<LiveEvent>,
    closed: bool,
}

impl std::fmt::Debug for OpenAiRealtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiRealtime")
            .field("state", &self.state)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl OpenAiRealtime {
    /// Connect and set the session up.
    pub fn connect(config: &LiveConfig, key: ApiKey) -> Result<OpenAiRealtime, ProviderError> {
        let url = parse_url(&session_url(config))?;
        let headers: Vec<(String, String)> = key_header(config, &key, Some("Authorization"))
            .into_iter()
            .collect();
        let socket = WebSocket::connect(&url, &headers, CONNECT_TIMEOUT)
            .map_err(|error| error.scrubbed(key.expose()))?;
        let mut session = OpenAiRealtime {
            socket,
            key,
            state: ReaderState::default(),
            pending: VecDeque::new(),
            closed: false,
        };
        session.send(&session_update(config))?;
        Ok(session)
    }

    fn send(&mut self, message: &Value) -> Result<(), ProviderError> {
        self.socket
            .send_text(&message.to_string())
            .map_err(|error| error.scrubbed(self.key.expose()))
    }

    /// Keep the events of one message; after the end of the session,
    /// nothing more.
    fn queue(&mut self, events: Vec<LiveEvent>) {
        for event in events {
            let end = matches!(event, LiveEvent::Closed { .. });
            self.pending.push_back(event);
            if end {
                self.closed = true;
                self.socket.close();
                break;
            }
        }
    }
}

/// The end of a session the server closed: normal for close code 1000.
pub(crate) fn closed_by_server(code: Option<u16>) -> LiveEvent {
    LiveEvent::Closed {
        error: (code != Some(1000)).then(|| {
            ProviderError::new(ErrorKind::Network).with_detail(format!(
                "the live service ended the session (close code {})",
                code.map_or_else(|| "none".to_owned(), |code| code.to_string())
            ))
        }),
    }
}

impl LiveSession for OpenAiRealtime {
    fn family(&self) -> LiveFamily {
        LiveFamily::OpenAiRealtime
    }

    fn send_audio(&mut self, samples: &[i16], rate: u32) -> Result<(), ProviderError> {
        if samples.is_empty() {
            return Ok(());
        }
        let converted = resample(samples, rate, RATE);
        self.send(&audio_message(&converted))
    }

    fn send_text(&mut self, text: &str) -> Result<(), ProviderError> {
        for message in text_messages(text) {
            self.send(&message)?;
        }
        Ok(())
    }

    fn send_tool_result(&mut self, result: &ToolResult) -> Result<(), ProviderError> {
        for message in tool_result_messages(result) {
            self.send(&message)?;
        }
        Ok(())
    }

    fn interrupt(&mut self, played_ms: u64) -> Result<(), ProviderError> {
        for message in interrupt_messages(&self.state, played_ms) {
            self.send(&message)?;
        }
        self.state.responding = false;
        self.state.audio_item = None;
        Ok(())
    }

    fn next_event(&mut self, wait: Duration) -> Option<LiveEvent> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            if self.closed {
                return None;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let incoming = match self.socket.read(left) {
                Ok(Some(incoming)) => incoming,
                Ok(None) => return None,
                Err(error) => {
                    self.closed = true;
                    return Some(LiveEvent::Closed {
                        error: Some(error.scrubbed(self.key.expose())),
                    });
                }
            };
            let events = match incoming {
                Incoming::Text(text) => parse_server_event(&text, &mut self.state),
                Incoming::Binary(_) => Vec::new(),
                Incoming::Close(code) => vec![closed_by_server(code)],
            };
            self.queue(events);
        }
    }

    fn close(&mut self) {
        self.socket.close();
        self.closed = true;
    }
}
