// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Live sessions: speech to speech, with tool calls.
//!
//! A live session is a conversation in sound: the person's audio goes in as
//! it is spoken, the model's audio comes out as it is made, both sides'
//! words come back as transcripts, the person can interrupt, and the model
//! can call tools. This module is the one interface P@x speaks for every
//! live engine, as [`crate::provider::Provider`] is for text:
//!
//! * [`LiveSession`]: audio in, text in, tool results in, interruptions in;
//!   [`LiveEvent`]s out.
//! * [`connect_live`]: one session of a [`LiveFamily`], from a
//!   [`LiveConfig`] and the person's key.
//! * [`ScriptedLive`]: a session that plays a script, for tests.
//!
//! **The model never acts.** A [`LiveEvent::ToolCall`] is a proposal, read
//! strictly (a repeated key anywhere makes the message malformed, as for the
//! text families); the caller decides it with the policy engine, performs it
//! only if allowed, and answers with [`LiveSession::send_tool_result`].
//!
//! Families, from the providers' documentation read on 16 November 2026
//! (`docs/architecture/pax-providers.md`, section "Live sessions"):
//!
//! | Family | Transport | Who |
//! |---|---|---|
//! | `openai-realtime` | WebSocket | OpenAI's Realtime API; by configuration Azure OpenAI's (`key_header` `api-key`), xAI's voice agent and llama.cpp's `llama-server` on this computer (`ws://` to a loopback address only) |
//! | `gemini-live` | WebSocket | Google's Gemini Live API |
//! | `scripted` | none | tests |
//!
//! Nothing in this module has been run against a real provider.

use std::collections::VecDeque;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::{ErrorKind, ProviderError};
use crate::model::{ToolCall, ToolDefinition, ToolResult};
use crate::secret::ApiKey;

/// The live API families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiveFamily {
    /// OpenAI's Realtime API and servers that speak it.
    OpenAiRealtime,
    /// Google's Gemini Live API.
    GeminiLive,
    /// A scripted session, for tests. [`connect_live`] refuses it.
    Scripted,
}

impl LiveFamily {
    /// Every family.
    pub const ALL: [LiveFamily; 3] = [
        LiveFamily::OpenAiRealtime,
        LiveFamily::GeminiLive,
        LiveFamily::Scripted,
    ];

    /// The family's name in configuration.
    pub const fn as_str(self) -> &'static str {
        match self {
            LiveFamily::OpenAiRealtime => "openai-realtime",
            LiveFamily::GeminiLive => "gemini-live",
            LiveFamily::Scripted => "scripted",
        }
    }

    /// The sample rate of the audio the family is sent, in hertz.
    pub const fn input_rate(self) -> u32 {
        match self {
            LiveFamily::OpenAiRealtime => 24_000,
            LiveFamily::GeminiLive | LiveFamily::Scripted => 16_000,
        }
    }

    /// The audio tokens a minute of sound counts, as the providers' pages
    /// state them: in (the person speaking) and out (the model speaking).
    /// OpenAI: one token per 100 ms in, one per 50 ms out. Google: 32 tokens
    /// a second either way.
    pub const fn audio_tokens_per_minute(self) -> (u64, u64) {
        match self {
            LiveFamily::OpenAiRealtime => (600, 1200),
            LiveFamily::GeminiLive => (1920, 1920),
            LiveFamily::Scripted => (0, 0),
        }
    }
}

impl fmt::Display for LiveFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LiveFamily {
    type Err = ProviderError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        LiveFamily::ALL
            .into_iter()
            .find(|family| family.as_str() == text)
            .ok_or_else(|| {
                ProviderError::new(ErrorKind::ConfigInvalid)
                    .with_detail("the live family is not openai-realtime or gemini-live")
            })
    }
}

/// Prices of a live model, per million tokens, from the model list. The
/// provider's invoice is the authority.
#[derive(Debug, Clone, PartialEq)]
pub struct LivePrices {
    /// The currency, for example `USD`.
    pub currency: String,
    /// Audio the model hears.
    pub audio_input_per_million: f64,
    /// Audio the model speaks.
    pub audio_output_per_million: f64,
    /// Text the model reads: instructions, tool results, the conversation.
    pub text_input_per_million: f64,
    /// Text the model writes: tool calls, transcripts it is billed for.
    pub text_output_per_million: f64,
    /// Input the provider had cached, if it prices it apart.
    pub cached_input_per_million: Option<f64>,
}

impl LivePrices {
    /// What `usage` costs.
    pub fn cost(&self, usage: &LiveUsage) -> f64 {
        let cached = self
            .cached_input_per_million
            .unwrap_or(self.text_input_per_million);
        (usage.audio_input_tokens as f64 * self.audio_input_per_million
            + usage.audio_output_tokens as f64 * self.audio_output_per_million
            + usage.text_input_tokens as f64 * self.text_input_per_million
            + usage.text_output_tokens as f64 * self.text_output_per_million
            + usage.cached_input_tokens as f64 * cached)
            / 1_000_000.0
    }

    /// An estimate of one minute of the person speaking plus one minute of
    /// the model speaking, from the family's token rate. Text, and the
    /// conversation the providers count again with each response, come on
    /// top: the estimate is a floor, said so wherever it is shown.
    pub fn per_minute_estimate(&self, family: LiveFamily) -> f64 {
        let (input, output) = family.audio_tokens_per_minute();
        self.cost(&LiveUsage {
            audio_input_tokens: input,
            audio_output_tokens: output,
            ..LiveUsage::default()
        })
    }
}

/// What a live session used, as the provider reported it. Input counts do
/// not include what was cached; that is `cached_input_tokens`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveUsage {
    /// Audio heard, not cached.
    pub audio_input_tokens: u64,
    /// Audio spoken.
    pub audio_output_tokens: u64,
    /// Text read, not cached.
    pub text_input_tokens: u64,
    /// Text written.
    pub text_output_tokens: u64,
    /// Input of either kind the provider had cached.
    pub cached_input_tokens: u64,
}

impl LiveUsage {
    /// Every token, of every kind.
    pub fn total(&self) -> u64 {
        self.audio_input_tokens
            + self.audio_output_tokens
            + self.text_input_tokens
            + self.text_output_tokens
            + self.cached_input_tokens
    }
}

/// How to open one live session.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveConfig {
    /// The family.
    pub family: LiveFamily,
    /// The model, from configuration. Never compiled in.
    pub model: String,
    /// The service's address, `wss://…`, or `ws://` to a loopback address
    /// for a server on this computer. `None`: the family's public address.
    pub base_url: Option<String>,
    /// The header that carries the key. `None`: the family's way
    /// (`Authorization: Bearer` for OpenAI, the `key` query parameter for
    /// Google). `Authorization` is sent as a bearer token; any other header
    /// (`api-key` for Azure) carries the key as it is.
    pub key_header: Option<String>,
    /// The voice the model speaks with, by the provider's name for it.
    /// `None`: the provider's default.
    pub voice: Option<String>,
    /// The system text: P@x's fixed instructions.
    pub instructions: String,
    /// The tools offered: the same definitions a text task is offered.
    pub tools: Vec<ToolDefinition>,
    /// The model's prices, if known.
    pub prices: Option<LivePrices>,
    /// The model that writes down what the person says, where the family
    /// asks for one by name (OpenAI's Realtime API), from configuration.
    /// `None`: no transcript of the person is asked for.
    pub transcription_model: Option<String>,
}

/// The longest model name accepted.
pub const LIVE_MODEL_MAX: usize = 128;

impl LiveConfig {
    /// Check the configuration: a model name of 1 to [`LIVE_MODEL_MAX`]
    /// characters from `A-Z a-z 0-9 . _ - : /`, an address that is `wss://`
    /// or `ws://` to this computer, a header name that can be sent.
    pub fn validate(&self) -> Result<(), ProviderError> {
        let invalid = |detail: &str| ProviderError::new(ErrorKind::ConfigInvalid).with_detail(detail);
        let model_ok = !self.model.is_empty()
            && self.model.len() <= LIVE_MODEL_MAX
            && self
                .model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'/'));
        if !model_ok {
            return Err(invalid(
                "a live model name must be 1 to 128 characters from letters, digits, '.', '_', '-', ':' and '/'",
            ));
        }
        if let Some(url) = &self.base_url {
            let parsed = crate::ws::parse_url(url)?;
            if parsed.target.contains(['?', '#']) && self.family == LiveFamily::GeminiLive {
                return Err(invalid("a Gemini Live address takes no query: the key goes there"));
            }
        }
        if let Some(header) = &self.key_header {
            let ok = !header.is_empty()
                && header.len() <= 64
                && header
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if !ok {
                return Err(invalid("the key header must be a plain header name"));
            }
        }
        if let Some(voice) = &self.voice {
            let ok = !voice.is_empty()
                && voice.len() <= 64
                && voice
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
            if !ok {
                return Err(invalid("a voice name must be up to 64 letters, digits, '-', '_' or '.'"));
            }
        }
        Ok(())
    }
}

/// Something that happened in a live session.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    /// Sound the model speaks: 16-bit signed samples, mono, at `rate`.
    Audio {
        /// The samples.
        samples: Vec<i16>,
        /// Samples a second.
        rate: u32,
    },
    /// The model's words: a piece (`final_text` false) or the whole of what
    /// it said in this turn.
    Said {
        /// The words.
        text: String,
        /// Whether this is the whole text.
        final_text: bool,
    },
    /// The person's words, as the provider recognised them.
    Heard {
        /// The words.
        text: String,
        /// Whether this is the whole text.
        final_text: bool,
    },
    /// The provider heard the person start to speak. The caller stops the
    /// model's sound at once (barge-in).
    SpeechStarted,
    /// The model proposes a tool call. Nothing has been done.
    ToolCall {
        /// The call, its arguments read strictly.
        call: ToolCall,
        /// The text the arguments were read from.
        arguments_text: String,
    },
    /// What a response used.
    Usage(LiveUsage),
    /// The model finished its turn.
    TurnDone,
    /// The session ended: `None` when it was closed normally.
    Closed {
        /// Why, if it was not a normal end.
        error: Option<ProviderError>,
    },
}

/// One live session.
///
/// Blocking: a caller runs it on a thread of its own and alternates
/// [`LiveSession::send_audio`] with [`LiveSession::next_event`] with a short
/// wait. After [`LiveEvent::Closed`] no further event comes.
pub trait LiveSession: Send {
    /// The family.
    fn family(&self) -> LiveFamily;

    /// Send sound the person is making: 16-bit signed samples, mono, at
    /// `rate`. The adapter converts it to the rate its family takes.
    fn send_audio(&mut self, samples: &[i16], rate: u32) -> Result<(), ProviderError>;

    /// Send words the person typed, as their turn.
    fn send_text(&mut self, text: &str) -> Result<(), ProviderError>;

    /// Answer a tool call, and let the model go on.
    fn send_tool_result(&mut self, result: &ToolResult) -> Result<(), ProviderError>;

    /// The person interrupted: stop the response in progress. `played_ms`
    /// is how much of the model's current sound was actually played, so the
    /// provider can forget what was never heard.
    fn interrupt(&mut self, played_ms: u64) -> Result<(), ProviderError>;

    /// The next event, waiting at most `wait`. `None` when nothing arrived.
    fn next_event(&mut self, wait: Duration) -> Option<LiveEvent>;

    /// End the session.
    fn close(&mut self);
}

/// Open a live session: the WebSocket handshake and the session's setup.
///
/// The key is sent only to the configured address, in the family's header
/// or query parameter, and never appears in an error.
pub fn connect_live(config: &LiveConfig, key: ApiKey) -> Result<Box<dyn LiveSession>, ProviderError> {
    config.validate()?;
    match config.family {
        LiveFamily::OpenAiRealtime => Ok(Box::new(crate::live_openai::OpenAiRealtime::connect(
            config, key,
        )?)),
        LiveFamily::GeminiLive => Ok(Box::new(crate::live_gemini::GeminiLive::connect(
            config, key,
        )?)),
        LiveFamily::Scripted => Err(ProviderError::new(ErrorKind::ConfigInvalid)
            .with_detail("the scripted family is for tests and cannot be connected")),
    }
}

/// The `Sec-WebSocket-Accept` answer to a handshake key: for a test server
/// that stands in for a live service.
pub fn websocket_accept(key: &str) -> String {
    crate::ws::accept_for(key)
}

/// Samples from `from` hertz to `to` hertz, by straight lines between
/// neighbours. Enough for speech going to a recogniser; each call is
/// converted on its own, so a stream cut in pieces may differ by a sample
/// at a joint.
pub fn resample(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || samples.is_empty() || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let length = (samples.len() as u64 * u64::from(to) / u64::from(from)).max(1) as usize;
    let step = f64::from(from) / f64::from(to);
    (0..length)
        .map(|index| {
            let position = index as f64 * step;
            let left = position.floor() as usize;
            let fraction = position - left as f64;
            let a = f64::from(samples[left.min(samples.len() - 1)]);
            let b = f64::from(samples[(left + 1).min(samples.len() - 1)]);
            (a + (b - a) * fraction).round().clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

/// Samples as little-endian bytes.
pub fn pcm16_to_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|sample| sample.to_le_bytes()).collect()
}

/// Little-endian bytes as samples; an odd last byte is dropped.
pub fn bytes_to_pcm16(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

/// Audio as standard base64, the way both families carry it in JSON.
pub fn audio_to_base64(samples: &[i16]) -> String {
    crate::ws::base64_encode(&pcm16_to_bytes(samples))
}

/// Base64 audio back to samples. `None` when it is not base64.
pub fn audio_from_base64(text: &str) -> Option<Vec<i16>> {
    crate::ws::base64_decode(text).map(|bytes| bytes_to_pcm16(&bytes))
}

/// The text of a tool result, as a live model is sent it. Images cannot go
/// into a live session: each is replaced by a note saying so.
pub fn tool_result_text(result: &ToolResult) -> String {
    let mut text = String::new();
    for content in &result.content {
        match content {
            crate::model::ResultContent::Text { text: part } => text.push_str(part),
            crate::model::ResultContent::Image(_) => {
                text.push_str("[an image was returned; it cannot be sent in a live session]");
            }
        }
    }
    text
}

/// Read the arguments of a tool call strictly: one JSON object, no key
/// repeated at any depth. A call that is anything else is a malformed
/// message, and the session ends with that error.
pub(crate) fn read_call(id: &str, name: &str, arguments_text: &str) -> LiveEvent {
    let name_ok = !name.is_empty()
        && name.len() <= crate::model::TOOL_NAME_MAX
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !name_ok || id.is_empty() {
        return LiveEvent::Closed {
            error: Some(
                ProviderError::new(ErrorKind::MalformedResponse)
                    .with_detail("a tool call has no identifier or an invalid name"),
            ),
        };
    }
    let text = if arguments_text.trim().is_empty() {
        "{}"
    } else {
        arguments_text
    };
    match crate::strict::object_from_str(text) {
        Ok(arguments) => LiveEvent::ToolCall {
            call: ToolCall {
                id: id.to_owned(),
                name: name.to_owned(),
                arguments,
            },
            arguments_text: text.to_owned(),
        },
        Err(_) => LiveEvent::Closed {
            error: Some(ProviderError::new(ErrorKind::MalformedResponse).with_detail(
                "the arguments of a tool call are not one JSON object with each key once",
            )),
        },
    }
}

/// The message of a server that could not be read: the session ends.
pub(crate) fn malformed(detail: &str) -> LiveEvent {
    LiveEvent::Closed {
        error: Some(ProviderError::new(ErrorKind::MalformedResponse).with_detail(detail)),
    }
}

/// The header and value that carry the key, or `None` for the query.
pub(crate) fn key_header(config: &LiveConfig, key: &ApiKey, default: Option<&str>) -> Option<(String, String)> {
    let name = config.key_header.as_deref().or(default)?;
    let value = if name.eq_ignore_ascii_case("authorization") {
        format!("Bearer {}", key.expose())
    } else {
        key.expose().to_owned()
    };
    Some((name.to_owned(), value))
}

// ----------------------------------------------------------- the scripted

/// One step of a [`ScriptedLive`] session's script.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptStep {
    /// Wait until at least this many samples have been sent since the step
    /// began.
    AwaitAudio {
        /// Samples.
        at_least_samples: usize,
    },
    /// Wait until a text has been sent since the step began.
    AwaitText,
    /// Wait until the result of this call has been sent.
    AwaitToolResult {
        /// The call's identifier.
        call_id: String,
    },
    /// Wait until an interruption has been sent since the step began.
    AwaitInterrupt,
    /// Give this event.
    Emit(LiveEvent),
}

/// What a [`ScriptedLive`] session was sent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptReceived {
    /// Every sample sent, whatever its rate.
    pub audio_samples: usize,
    /// The texts.
    pub texts: Vec<String>,
    /// The tool results.
    pub tool_results: Vec<ToolResult>,
    /// Each interruption: how much of the model's sound had been played.
    pub interrupts: Vec<u64>,
}

/// A live session that plays a script: it gives the script's events in
/// order, and at each `Await…` step gives nothing until what it waits for
/// has been sent. No network, no clock: an unsatisfied wait returns `None`
/// at once. When the script is played out it gives `None` for ever.
#[derive(Debug)]
pub struct ScriptedLive {
    steps: VecDeque<ScriptStep>,
    record: Arc<Mutex<ScriptReceived>>,
    /// What had been received when the current step began.
    baseline: ScriptReceived,
    closed: bool,
}

impl ScriptedLive {
    /// A session that plays `steps`.
    pub fn new(steps: Vec<ScriptStep>) -> ScriptedLive {
        ScriptedLive {
            steps: steps.into(),
            record: Arc::new(Mutex::new(ScriptReceived::default())),
            baseline: ScriptReceived::default(),
            closed: false,
        }
    }

    /// What it receives, shared: a test keeps this before the session is
    /// boxed and handed over, and reads it afterwards.
    pub fn received(&self) -> Arc<Mutex<ScriptReceived>> {
        Arc::clone(&self.record)
    }

    fn snapshot(&self) -> ScriptReceived {
        self.record
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn update(&self, change: impl FnOnce(&mut ScriptReceived)) -> Result<(), ProviderError> {
        if self.closed {
            return Err(ProviderError::new(ErrorKind::Network).with_detail("the live session is closed"));
        }
        let mut received = self
            .record
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        change(&mut received);
        Ok(())
    }
}

impl LiveSession for ScriptedLive {
    fn family(&self) -> LiveFamily {
        LiveFamily::Scripted
    }

    fn send_audio(&mut self, samples: &[i16], _rate: u32) -> Result<(), ProviderError> {
        self.update(|received| received.audio_samples += samples.len())
    }

    fn send_text(&mut self, text: &str) -> Result<(), ProviderError> {
        self.update(|received| received.texts.push(text.to_owned()))
    }

    fn send_tool_result(&mut self, result: &ToolResult) -> Result<(), ProviderError> {
        self.update(|received| received.tool_results.push(result.clone()))
    }

    fn interrupt(&mut self, played_ms: u64) -> Result<(), ProviderError> {
        self.update(|received| received.interrupts.push(played_ms))
    }

    fn next_event(&mut self, _wait: Duration) -> Option<LiveEvent> {
        loop {
            if self.closed {
                return None;
            }
            let now = self.snapshot();
            let done = match self.steps.front()? {
                ScriptStep::Emit(event) => {
                    let event = event.clone();
                    self.steps.pop_front();
                    self.baseline = now;
                    if matches!(event, LiveEvent::Closed { .. }) {
                        self.closed = true;
                    }
                    return Some(event);
                }
                // Counted from the start of the session.
                ScriptStep::AwaitAudio { at_least_samples } => now.audio_samples >= *at_least_samples,
                ScriptStep::AwaitText => now.texts.len() > self.baseline.texts.len(),
                ScriptStep::AwaitToolResult { call_id } => now
                    .tool_results
                    .iter()
                    .any(|result| result.call_id == *call_id),
                ScriptStep::AwaitInterrupt => now.interrupts.len() > self.baseline.interrupts.len(),
            };
            if !done {
                return None;
            }
            // The wait is over; the next step may be an event to give now.
            self.steps.pop_front();
            self.baseline = now;
        }
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_have_stable_names() {
        for family in LiveFamily::ALL {
            assert_eq!(family.as_str().parse::<LiveFamily>(), Ok(family));
        }
        assert!("anthropic".parse::<LiveFamily>().is_err());
    }

    #[test]
    fn resampling_keeps_the_length_in_proportion_and_the_shape() {
        let ramp: Vec<i16> = (0..160).map(|i| i * 100).collect();
        let up = resample(&ramp, 16_000, 24_000);
        assert_eq!(up.len(), 240);
        assert_eq!(up[0], 0);
        assert!(up.windows(2).all(|pair| pair[1] >= pair[0]), "a ramp stays a ramp");
        let down = resample(&up, 24_000, 16_000);
        assert_eq!(down.len(), 160);
        assert_eq!(resample(&ramp, 16_000, 16_000), ramp);
        assert!(resample(&[], 16_000, 24_000).is_empty());
        assert_eq!(resample(&[i16::MAX, i16::MIN], 8_000, 48_000).len(), 12);
    }

    #[test]
    fn audio_survives_base64() {
        let samples = vec![0, 1, -1, i16::MAX, i16::MIN, 1234];
        assert_eq!(audio_from_base64(&audio_to_base64(&samples)), Some(samples));
        assert_eq!(audio_from_base64("not base64!"), None);
    }

    #[test]
    fn a_minute_is_priced_from_the_family_rates() {
        let prices = LivePrices {
            currency: "USD".to_owned(),
            audio_input_per_million: 32.0,
            audio_output_per_million: 64.0,
            text_input_per_million: 4.0,
            text_output_per_million: 24.0,
            cached_input_per_million: Some(0.4),
        };
        let minute = prices.per_minute_estimate(LiveFamily::OpenAiRealtime);
        assert!((minute - (600.0 * 32.0 + 1200.0 * 64.0) / 1e6).abs() < 1e-12);
        let usage = LiveUsage {
            audio_input_tokens: 1_000_000,
            cached_input_tokens: 1_000_000,
            ..LiveUsage::default()
        };
        assert!((prices.cost(&usage) - 32.4).abs() < 1e-9);
    }

    #[test]
    fn a_repeated_key_in_a_tool_call_ends_the_session() {
        assert!(matches!(
            read_call("c1", "list_volumes", r#"{"a": 1, "a": 2}"#),
            LiveEvent::Closed { error: Some(error) } if error.kind() == ErrorKind::MalformedResponse
        ));
        assert!(matches!(read_call("c1", "list volumes", "{}"), LiveEvent::Closed { .. }));
        assert!(matches!(read_call("c1", "x", "[1]"), LiveEvent::Closed { .. }));
        match read_call("c1", "list_volumes", "") {
            LiveEvent::ToolCall { call, arguments_text } => {
                assert!(call.arguments.is_empty());
                assert_eq!(arguments_text, "{}");
            }
            other => panic!("{other:?}"),
        }
    }
}
