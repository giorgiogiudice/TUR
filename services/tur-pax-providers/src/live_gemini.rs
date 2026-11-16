// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `gemini-live` live family: Google's Gemini Live API over WebSocket.
//!
//! Written from Google's documentation as read on 16 November 2026. Never
//! run against Google.
//!
//! * Address: `wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent`;
//!   the key goes in the `key` query parameter, as the documentation shows,
//!   or in a header by configuration (`x-goog-api-key` is not confirmed by
//!   the pages read).
//! * Setup: the first message, `setup`, with the model (`models/<name>`),
//!   P@x's instructions, the tools as function declarations, automatic
//!   activity detection, and transcription both ways; then the server's
//!   `setupComplete` is awaited before anything else is sent.
//!   `responseModalities` and `speechConfig` are written inside
//!   `generationConfig`; the reference lists `generationConfig` as a setup
//!   field but the guides show the SDK's flattened form, so where these two
//!   go on the wire is **not verified**.
//! * Out: `realtimeInput.audio` (16-bit PCM at 16 kHz), `realtimeInput.text`,
//!   `toolResponse.functionResponses`.
//! * In: `serverContent` (the model's sound in `modelTurn.parts[].inlineData`
//!   at 24 kHz, `inputTranscription`, `outputTranscription`, `interrupted`,
//!   `turnComplete`), `toolCall.functionCalls`, `usageMetadata`, `goAway`.
//! * Barge-in: Google's activity detection stops the model's turn when the
//!   person speaks (`interrupted`); there is no client message to cancel a
//!   turn while automatic detection is on, so [`LiveSession::interrupt`]
//!   only forgets the turn locally and the caller stops playing it.
//!
//! The server may send its JSON in binary frames; both kinds are read.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::error::{ErrorKind, ProviderError};
use crate::live::{
    LiveConfig, LiveEvent, LiveFamily, LiveSession, LiveUsage, audio_from_base64,
    audio_to_base64, key_header, malformed, read_call, resample, tool_result_text,
};
use crate::model::ToolResult;
use crate::secret::ApiKey;
use crate::live_openai::closed_by_server;
use crate::ws::{Incoming, WebSocket, parse_url};

/// Google's address of the Live API.
pub const DEFAULT_URL: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

/// The sample rate the family is sent.
pub const INPUT_RATE: u32 = 16_000;

/// The sample rate of the model's sound when its message does not say.
pub const OUTPUT_RATE: u32 = 24_000;

/// How long the handshake and the setup may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// The model's name as the setup message carries it.
pub fn model_name(model: &str) -> String {
    if model.starts_with("models/") {
        model.to_owned()
    } else {
        format!("models/{model}")
    }
}

/// The address, with the key in the query when no header carries it.
pub fn session_url(config: &LiveConfig, key_in_query: Option<&str>) -> String {
    let base = config.base_url.as_deref().unwrap_or(DEFAULT_URL);
    match key_in_query {
        Some(key) => format!("{base}?key={key}"),
        None => base.to_owned(),
    }
}

/// The `setup` message.
pub fn setup_message(config: &LiveConfig) -> Value {
    let mut generation = json!({ "responseModalities": ["AUDIO"] });
    if let Some(voice) = &config.voice {
        generation["speechConfig"] =
            json!({ "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": voice } } });
    }
    let mut setup = json!({
        "model": model_name(&config.model),
        "generationConfig": generation,
        "systemInstruction": { "parts": [{ "text": config.instructions }] },
        "realtimeInputConfig": { "automaticActivityDetection": { "disabled": false } },
        "inputAudioTranscription": {},
        "outputAudioTranscription": {}
    });
    if !config.tools.is_empty() {
        let declarations: Vec<Value> = config
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.input_schema(),
                })
            })
            .collect();
        setup["tools"] = json!([{ "functionDeclarations": declarations }]);
    }
    json!({ "setup": setup })
}

/// Audio at 16 kHz as one message.
pub fn audio_message(samples_at_rate: &[i16]) -> Value {
    json!({
        "realtimeInput": {
            "audio": {
                "data": audio_to_base64(samples_at_rate),
                "mimeType": format!("audio/pcm;rate={INPUT_RATE}")
            }
        }
    })
}

/// The person's typed words.
pub fn text_message(text: &str) -> Value {
    json!({ "realtimeInput": { "text": text } })
}

/// The answer to a tool call. A failed call is sent as `{"error": …}`.
pub fn tool_result_message(result: &ToolResult) -> Value {
    let text = tool_result_text(result);
    let response = if result.is_error {
        json!({ "error": text })
    } else {
        json!({ "output": text })
    };
    json!({
        "toolResponse": {
            "functionResponses": [{
                "id": result.call_id,
                "name": result.name,
                "response": response
            }]
        }
    })
}

fn modality_counts(details: Option<&Value>) -> (u64, u64) {
    let mut audio = 0;
    let mut text = 0;
    for entry in details.and_then(Value::as_array).into_iter().flatten() {
        let tokens = entry.get("tokenCount").and_then(Value::as_u64).unwrap_or(0);
        match entry.get("modality").and_then(Value::as_str) {
            Some("AUDIO") => audio += tokens,
            _ => text += tokens,
        }
    }
    (audio, text)
}

/// The usage a message reports. Whether Google's counts are for the turn or
/// for the session so far is not stated in what was read; they are taken
/// as the turn's.
pub fn usage_of(metadata: &Value) -> LiveUsage {
    let (audio_in, text_in) = modality_counts(metadata.get("promptTokensDetails"));
    let (audio_out, text_out) = modality_counts(metadata.get("responseTokensDetails"));
    let prompt = metadata
        .get("promptTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let response = metadata
        .get("responseTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cached = metadata
        .get("cachedContentTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    // Without the breakdown by modality, every token is counted as text.
    let (audio_in, text_in) = if audio_in + text_in == 0 {
        (0, prompt)
    } else {
        (audio_in, text_in)
    };
    let (audio_out, text_out) = if audio_out + text_out == 0 {
        (0, response)
    } else {
        (audio_out, text_out)
    };
    let from_text = cached.min(text_in);
    let from_audio = (cached - from_text).min(audio_in);
    LiveUsage {
        audio_input_tokens: audio_in - from_audio,
        audio_output_tokens: audio_out,
        text_input_tokens: text_in - from_text,
        text_output_tokens: text_out,
        cached_input_tokens: cached,
    }
}

fn rate_of(mime: &str) -> u32 {
    mime.split(';')
        .find_map(|part| part.trim().strip_prefix("rate="))
        .and_then(|rate| rate.parse().ok())
        .unwrap_or(OUTPUT_RATE)
}

/// Read one server message strictly.
pub fn parse_server_message(text: &str) -> Vec<LiveEvent> {
    let Ok(Value::Object(message)) = crate::strict::value_from_str(text) else {
        return vec![malformed("a live message is not one JSON object with each key once")];
    };
    let mut events = Vec::new();
    if let Some(content) = message.get("serverContent") {
        if content.get("interrupted").and_then(Value::as_bool) == Some(true) {
            events.push(LiveEvent::SpeechStarted);
        }
        if let Some(heard) = content
            .get("inputTranscription")
            .and_then(|t| t.get("text"))
            .and_then(Value::as_str)
        {
            events.push(LiveEvent::Heard {
                text: heard.to_owned(),
                final_text: false,
            });
        }
        let parts = content
            .get("modelTurn")
            .and_then(|turn| turn.get("parts"))
            .and_then(Value::as_array);
        for part in parts.into_iter().flatten() {
            if let Some(inline) = part.get("inlineData") {
                let mime = inline.get("mimeType").and_then(Value::as_str).unwrap_or("");
                if !mime.starts_with("audio/pcm") {
                    continue;
                }
                match inline.get("data").and_then(Value::as_str).map(audio_from_base64) {
                    Some(Some(samples)) => events.push(LiveEvent::Audio {
                        samples,
                        rate: rate_of(mime),
                    }),
                    _ => return vec![malformed("a piece of the model's sound is not base64")],
                }
            }
        }
        if let Some(said) = content
            .get("outputTranscription")
            .and_then(|t| t.get("text"))
            .and_then(Value::as_str)
        {
            events.push(LiveEvent::Said {
                text: said.to_owned(),
                final_text: false,
            });
        }
        if content.get("turnComplete").and_then(Value::as_bool) == Some(true) {
            events.push(LiveEvent::TurnDone);
        }
    }
    if let Some(calls) = message
        .get("toolCall")
        .and_then(|call| call.get("functionCalls"))
        .and_then(Value::as_array)
    {
        for call in calls {
            let id = call.get("id").and_then(Value::as_str).unwrap_or_default();
            let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
            let arguments = match call.get("args") {
                None | Some(Value::Null) => "{}".to_owned(),
                Some(Value::Object(object)) => Value::Object(object.clone()).to_string(),
                Some(_) => return vec![malformed("the arguments of a tool call are not an object")],
            };
            let event = read_call(id, name, &arguments);
            let stop = matches!(event, LiveEvent::Closed { .. });
            events.push(event);
            if stop {
                return events;
            }
        }
    }
    if let Some(metadata) = message.get("usageMetadata") {
        events.push(LiveEvent::Usage(usage_of(metadata)));
    }
    events
}

/// A session of the family.
pub struct GeminiLive {
    socket: WebSocket,
    key: ApiKey,
    pending: VecDeque<LiveEvent>,
    closed: bool,
}

impl std::fmt::Debug for GeminiLive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiLive")
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

fn message_text(incoming: Incoming) -> Result<Option<String>, Option<u16>> {
    match incoming {
        Incoming::Text(text) => Ok(Some(text)),
        Incoming::Binary(bytes) => Ok(String::from_utf8(bytes).ok()),
        Incoming::Close(code) => Err(code),
    }
}

impl GeminiLive {
    /// Connect, send the setup and wait for `setupComplete`.
    pub fn connect(config: &LiveConfig, key: ApiKey) -> Result<GeminiLive, ProviderError> {
        let header = key_header(config, &key, None);
        let url_text = session_url(config, header.is_none().then(|| key.expose()));
        let url = parse_url(&url_text).map_err(|error| error.scrubbed(key.expose()))?;
        let headers: Vec<(String, String)> = header.into_iter().collect();
        let socket = WebSocket::connect(&url, &headers, CONNECT_TIMEOUT)
            .map_err(|error| error.scrubbed(key.expose()))?;
        let mut session = GeminiLive {
            socket,
            key,
            pending: VecDeque::new(),
            closed: false,
        };
        session.send(&setup_message(config))?;
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                session.close();
                return Err(ProviderError::new(ErrorKind::TimedOut)
                    .with_detail("the live service did not confirm the session's setup in time"));
            }
            let incoming = session
                .socket
                .read(left)
                .map_err(|error| error.scrubbed(session.key.expose()))?;
            let Some(incoming) = incoming else { continue };
            match message_text(incoming) {
                Ok(Some(text)) => {
                    let Ok(Value::Object(message)) = crate::strict::value_from_str(&text) else {
                        session.close();
                        return Err(ProviderError::new(ErrorKind::MalformedResponse)
                            .with_detail("the answer to the setup is not one JSON object"));
                    };
                    if message.contains_key("setupComplete") {
                        return Ok(session);
                    }
                }
                Ok(None) => {}
                Err(code) => {
                    session.closed = true;
                    return Err(ProviderError::new(ErrorKind::RequestRejected).with_detail(format!(
                        "the live service refused the setup (close code {})",
                        code.map_or_else(|| "none".to_owned(), |code| code.to_string())
                    )));
                }
            }
        }
    }

    fn send(&mut self, message: &Value) -> Result<(), ProviderError> {
        self.socket
            .send_text(&message.to_string())
            .map_err(|error| error.scrubbed(self.key.expose()))
    }
}

impl LiveSession for GeminiLive {
    fn family(&self) -> LiveFamily {
        LiveFamily::GeminiLive
    }

    fn send_audio(&mut self, samples: &[i16], rate: u32) -> Result<(), ProviderError> {
        if samples.is_empty() {
            return Ok(());
        }
        let converted = resample(samples, rate, INPUT_RATE);
        self.send(&audio_message(&converted))
    }

    fn send_text(&mut self, text: &str) -> Result<(), ProviderError> {
        self.send(&text_message(text))
    }

    fn send_tool_result(&mut self, result: &ToolResult) -> Result<(), ProviderError> {
        self.send(&tool_result_message(result))
    }

    fn interrupt(&mut self, _played_ms: u64) -> Result<(), ProviderError> {
        // Nothing to send while automatic activity detection is on (see the
        // module's documentation). What was queued of the model's turn is
        // dropped so that it is not played.
        self.pending
            .retain(|event| !matches!(event, LiveEvent::Audio { .. }));
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
            let events = match message_text(incoming) {
                Ok(Some(text)) => parse_server_message(&text),
                Ok(None) => Vec::new(),
                Err(code) => vec![closed_by_server(code)],
            };
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

    fn close(&mut self) {
        self.socket.close();
        self.closed = true;
    }
}
