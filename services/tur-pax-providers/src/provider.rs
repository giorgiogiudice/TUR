// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The provider interface.
//!
//! A [`Provider`] declares its [`Capabilities`] and turns a [`Request`] into
//! an [`EventStream`]. The interface is blocking: the stream is an iterator,
//! and the orchestrator runs a call on a thread of its own.
//!
//! An [`EventStream`] has one contract, the same for every adapter and every
//! outcome: it yields zero or more events and then exactly one
//! [`StreamEvent::Finish`], after which it yields nothing. A refusal, a rate
//! limit, a timeout, a dropped connection and a malformed response all arrive
//! as that last event, never as a panic and never as a stream that just stops.
//! Nothing is sent until the stream is first polled.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::config::ProviderConfig;
use crate::error::{ErrorKind, ProviderError};
use crate::http::{Auth, BodyReader, Call, Classifier, Transport, read_error};
use crate::model::{FinishReason, ProviderId, Request, StreamEvent};
use crate::secret::{ApiKey, REDACTED};
use crate::sse::{SseError, SseEvent, SseReader};
use crate::{anthropic, google, openai, openai_chat, strict};

/// What an adapter can do. The orchestrator adapts to this instead of assuming
/// that providers are alike (AI control layer design, section 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Responses are streamed.
    pub streaming: bool,
    /// Tools with JSON Schema inputs can be offered and called.
    pub tool_calling: bool,
    /// The model may make several tool calls in one response.
    pub parallel_tool_calls: bool,
    /// Images can be sent in a user message.
    pub image_input: bool,
    /// Images can be sent inside a tool result, for example a screenshot.
    pub image_in_tool_results: bool,
    /// The provider's own computer-control tool.
    pub native_computer_control: NativeComputerControl,
    /// The adapter can list the provider's models.
    pub model_discovery: bool,
    /// The adapter offers the provider's speech services.
    pub speech_services: bool,
}

/// A provider's own computer-control tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeComputerControl {
    /// The tool type the provider documents, if it offers one.
    pub provider_tool: Option<&'static str>,
    /// Whether this adapter can drive it. Where it cannot, the orchestrator
    /// uses the generic screenshot-and-tools loop.
    pub implemented: bool,
}

/// One model provider behind the neutral interface.
pub trait Provider: Send + Sync {
    /// Which provider this is.
    fn id(&self) -> ProviderId;

    /// What this adapter can do.
    fn capabilities(&self) -> Capabilities;

    /// Send a request and stream the response.
    ///
    /// The call returns at once. The request is sent, and retried if need be,
    /// when the stream is first polled.
    fn stream(&self, request: &Request) -> EventStream;
}

/// The capabilities the adapter of a family declares: what the adapter can
/// carry. What one model of one service can do is the service's entry's to
/// declare (the session service checks it before it offers tools).
pub fn capabilities_of(provider: ProviderId) -> Capabilities {
    match provider {
        ProviderId::Anthropic => anthropic::CAPABILITIES,
        ProviderId::OpenAi => openai::CAPABILITIES,
        ProviderId::Google => google::CAPABILITIES,
        ProviderId::OpenAiChat => openai_chat::CAPABILITIES,
    }
}

/// The path, below the base URL, that an adapter posts conversations to.
pub fn endpoint_path(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Anthropic => anthropic::ENDPOINT.path,
        ProviderId::OpenAi => openai::ENDPOINT.path,
        ProviderId::Google => google::ENDPOINT.path,
        ProviderId::OpenAiChat => openai_chat::ENDPOINT.path,
    }
}

/// Build the adapter a configuration names.
///
/// The configuration is validated first. No network call is made.
pub fn connect(config: &ProviderConfig, key: ApiKey) -> Result<Box<dyn Provider>, ProviderError> {
    build(config, Some(key))
}

/// Build the adapter of a configuration for a service that takes no key,
/// such as a model server on this computer. No credential header is sent.
///
/// Only the OpenAI-compatible family can be used without a key: the three
/// companies' interfaces all require one.
///
/// # Errors
///
/// `TUR-AI-PROVIDER-011` for another family or a configuration that is not
/// valid.
pub fn connect_without_key(config: &ProviderConfig) -> Result<Box<dyn Provider>, ProviderError> {
    if config.provider != ProviderId::OpenAiChat {
        return Err(ProviderError::new(ErrorKind::ConfigInvalid)
            .with_detail("only the openai-chat family can be used without a key")
            .with_provider(config.provider));
    }
    build(config, None)
}

fn build(config: &ProviderConfig, key: Option<ApiKey>) -> Result<Box<dyn Provider>, ProviderError> {
    config
        .validate()
        .map_err(|error| error.with_provider(config.provider))?;
    let shared = Arc::new(Shared {
        base_url: config.effective_base_url().to_owned(),
        key,
        transport: Transport::new(config),
    });
    let provider: Box<dyn Provider> = match config.provider {
        ProviderId::Anthropic => Box::new(anthropic::AnthropicProvider::new(shared)),
        ProviderId::OpenAi => Box::new(openai::OpenAiProvider::new(shared)),
        ProviderId::Google => Box::new(google::GoogleProvider::new(shared)),
        ProviderId::OpenAiChat => Box::new(openai_chat::OpenAiChatProvider::new(
            shared,
            config.send_back_reasoning,
        )),
    };
    Ok(provider)
}

/// What an adapter needs to make a call.
pub(crate) struct Shared {
    base_url: String,
    /// `None` for a service that takes no key.
    key: Option<ApiKey>,
    transport: Transport,
}

impl Shared {
    /// The key text, to remove it from what a provider sends back. Empty
    /// without a key.
    fn secret(&self) -> &str {
        self.key.as_ref().map_or("", |key| key.expose())
    }
}

/// Where and how an adapter posts a conversation.
#[derive(Clone, Copy)]
pub(crate) struct Endpoint {
    pub(crate) provider: ProviderId,
    pub(crate) path: &'static str,
    pub(crate) auth: Auth,
    pub(crate) headers: &'static [(&'static str, &'static str)],
    pub(crate) classify: Classifier,
}

impl Shared {
    /// A stream for a request body, or a stream that reports why the body
    /// could not be built.
    pub(crate) fn stream(
        self: &Arc<Self>,
        endpoint: Endpoint,
        body: Result<Value, ProviderError>,
        decoder: Box<dyn Decoder>,
    ) -> EventStream {
        let body = match body {
            Ok(body) => body.to_string(),
            Err(error) => return EventStream::failed(endpoint.provider, error),
        };
        let shared = Arc::clone(self);
        let open: Opener = Box::new(move || {
            let url = format!("{}{}", shared.base_url, endpoint.path);
            let call = Call {
                provider: endpoint.provider,
                url: &url,
                auth: endpoint.auth,
                headers: endpoint.headers,
                body: &body,
                classify: endpoint.classify,
            };
            shared.transport.open(&call, shared.key.as_ref())
        });
        EventStream {
            provider: endpoint.provider,
            state: State::Pending(open),
            decoder: Some(decoder),
            queue: VecDeque::new(),
            received: false,
            shared: Some(Arc::clone(self)),
        }
    }
}

/// Turns one provider's stream events into neutral events.
pub(crate) trait Decoder: Send {
    /// Handle one event. Neutral events are appended to `out`; a
    /// [`StreamEvent::Finish`] ends the stream.
    fn decode(&mut self, event: &SseEvent, out: &mut VecDeque<StreamEvent>);

    /// The stream sent its `[DONE]` marker. For a format whose final event
    /// is that marker, this is where the finish is produced. By default the
    /// marker means nothing: the finish came before it.
    fn done(&mut self, _out: &mut VecDeque<StreamEvent>) {}

    /// The stream is about to end with a failure of its own (cut, unreadable)
    /// after some of the response was read. An adapter that has to account
    /// for what was used may add events before the failure.
    fn interrupted(&mut self, _out: &mut VecDeque<StreamEvent>) {}
}

type Opener = Box<dyn FnOnce() -> Result<BodyReader, ProviderError> + Send>;

enum State {
    /// Nothing has been sent yet.
    Pending(Opener),
    /// The response is being read.
    Open(Box<SseReader<BodyReader>>),
    /// The finish event has been produced.
    Done,
}

/// The events of one response. See the module documentation for the contract.
pub struct EventStream {
    provider: ProviderId,
    state: State,
    decoder: Option<Box<dyn Decoder>>,
    queue: VecDeque<StreamEvent>,
    received: bool,
    /// Holds the key, to remove it from any error text a provider sends.
    shared: Option<Arc<Shared>>,
}

impl EventStream {
    /// A stream that only reports a failure.
    pub(crate) fn failed(provider: ProviderId, error: ProviderError) -> Self {
        let mut queue = VecDeque::new();
        queue.push_back(StreamEvent::Finish(FinishReason::Error(
            error.with_provider(provider),
        )));
        EventStream {
            provider,
            state: State::Done,
            decoder: None,
            queue,
            received: false,
            shared: None,
        }
    }

    fn fail(&mut self, error: ProviderError) {
        self.queue
            .push_back(StreamEvent::Finish(FinishReason::Error(
                error.with_provider(self.provider),
            )));
    }

    /// The stream ends with a failure of its own after some of the response
    /// was read: the adapter may account for it first.
    fn interrupted(&mut self) {
        if self.received
            && let Some(decoder) = self.decoder.as_mut()
        {
            decoder.interrupted(&mut self.queue);
        }
    }

    /// The connection closed before the provider's own end of stream.
    fn ended_early(&self) -> ProviderError {
        if self.received {
            ProviderError::new(ErrorKind::Network)
                .with_detail("the connection closed before the response was complete")
        } else {
            ProviderError::new(ErrorKind::MalformedResponse)
                .with_detail("the response contained no events")
        }
    }
}

fn stream_error(error: SseError) -> ProviderError {
    match error {
        SseError::Io(source) => read_error(source),
        SseError::InvalidUtf8 => ProviderError::new(ErrorKind::MalformedResponse)
            .with_detail("the response was not valid UTF-8"),
        SseError::TooLong => ProviderError::new(ErrorKind::MalformedResponse)
            .with_detail("an event of the response was too large"),
    }
}

impl Iterator for EventStream {
    type Item = StreamEvent;

    fn next(&mut self) -> Option<StreamEvent> {
        loop {
            if let Some(event) = self.queue.pop_front() {
                let reason = match event {
                    StreamEvent::Finish(reason) => reason,
                    other => return Some(other),
                };
                // Dropping the reader closes the connection.
                self.state = State::Done;
                self.decoder = None;
                self.queue.clear();
                // A provider may repeat the key in an error message.
                let reason = match self.shared.take() {
                    Some(shared) => reason.scrubbed(shared.secret()),
                    None => reason,
                };
                return Some(StreamEvent::Finish(reason));
            }
            match std::mem::replace(&mut self.state, State::Done) {
                State::Done => return None,
                State::Pending(open) => match open() {
                    Ok(body) => self.state = State::Open(Box::new(SseReader::new(body))),
                    Err(error) => self.fail(error),
                },
                State::Open(mut reader) => match reader.next_event() {
                    Ok(Some(mut event)) => {
                        self.state = State::Open(reader);
                        // A provider may repeat the key it was sent. It is
                        // removed before anything is made of the event.
                        if let Some(shared) = &self.shared {
                            let secret = shared.secret();
                            if !secret.is_empty() && event.data.contains(secret) {
                                event.data = event.data.replace(secret, REDACTED);
                            }
                        }
                        // Three formats end a stream with this marker. Two
                        // have produced their finish by then; for the
                        // OpenAI-compatible one it is the end.
                        if event.data.trim() == "[DONE]" {
                            if let Some(decoder) = self.decoder.as_mut() {
                                decoder.done(&mut self.queue);
                            }
                            continue;
                        }
                        self.received = true;
                        if let Some(decoder) = self.decoder.as_mut() {
                            decoder.decode(&event, &mut self.queue);
                        }
                    }
                    Ok(None) => {
                        self.interrupted();
                        let error = self.ended_early();
                        self.fail(error);
                    }
                    Err(error) => {
                        self.interrupted();
                        self.fail(stream_error(error));
                    }
                },
            }
        }
    }
}

impl fmt::Debug for EventStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = match self.state {
            State::Pending(_) => "pending",
            State::Open(_) => "open",
            State::Done => "done",
        };
        f.debug_struct("EventStream")
            .field("provider", &self.provider)
            .field("state", &state)
            .finish_non_exhaustive()
    }
}

/// Parse the data of an event as JSON, strictly: an event in which an object
/// repeats a key, at any depth, has no single reading and is refused like
/// one that is not JSON. On failure the stream is ended with a
/// malformed-response error and `None` is returned.
///
/// This is what makes a tool call whose arguments arrive as a JSON object
/// inside an event (and not as text of their own) safe to take from the
/// parsed event: nothing was dropped on the way.
pub(crate) fn parse_data(event: &SseEvent, out: &mut VecDeque<StreamEvent>) -> Option<Value> {
    match strict::value_from_str(&event.data) {
        Ok(value) if value.is_object() => Some(value),
        _ => {
            out.push_back(StreamEvent::Finish(FinishReason::Error(
                ProviderError::new(ErrorKind::MalformedResponse).with_detail(
                    "an event of the response was not a JSON object, or repeated a key",
                ),
            )));
            None
        }
    }
}

/// The type of an event: the named field of its data, or else the name the
/// stream gave it.
pub(crate) fn event_type<'a>(data: &'a Value, field: &str, event: &'a SseEvent) -> &'a str {
    data[field]
        .as_str()
        .or(event.event.as_deref())
        .unwrap_or("")
}

/// A typed error built from a provider's error object: the kind, and the
/// provider's message as the detail.
pub(crate) fn provider_error(kind: ErrorKind, message: Option<&str>) -> ProviderError {
    let error = ProviderError::new(kind);
    match message {
        Some(message) => error.with_detail(message),
        None => error,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// Emits the data of each event as text and finishes on the word `stop`.
    struct Echo;

    impl Decoder for Echo {
        fn decode(&mut self, event: &SseEvent, out: &mut VecDeque<StreamEvent>) {
            if event.data == "stop" {
                out.push_back(StreamEvent::Finish(FinishReason::EndTurn));
            } else {
                out.push_back(StreamEvent::TextDelta {
                    text: event.data.clone(),
                });
            }
        }
    }

    fn stream_over(open: Opener) -> EventStream {
        EventStream {
            provider: ProviderId::Anthropic,
            state: State::Pending(open),
            decoder: Some(Box::new(Echo)),
            queue: VecDeque::new(),
            received: false,
            shared: None,
        }
    }

    fn body(text: &'static str) -> Opener {
        Box::new(move || Ok(Box::new(Cursor::new(text.as_bytes())) as BodyReader))
    }

    fn text(value: &str) -> StreamEvent {
        StreamEvent::TextDelta {
            text: value.to_owned(),
        }
    }

    fn only_error(events: &[StreamEvent]) -> &ProviderError {
        match events {
            [.., StreamEvent::Finish(reason)] => reason.error().expect("the finish is an error"),
            other => panic!("unexpected events: {other:?}"),
        }
    }

    #[test]
    fn a_stream_ends_with_exactly_one_finish_and_then_nothing() {
        let mut stream = stream_over(body("data: a\n\ndata: b\n\ndata: stop\n\ndata: late\n\n"));
        assert_eq!(
            format!("{stream:?}"),
            "EventStream { provider: Anthropic, state: \"pending\", .. }"
        );
        let events: Vec<StreamEvent> = stream.by_ref().collect();
        assert_eq!(
            events,
            vec![
                text("a"),
                text("b"),
                StreamEvent::Finish(FinishReason::EndTurn)
            ]
        );
        assert_eq!(stream.next(), None);
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn the_done_marker_is_skipped() {
        let events: Vec<StreamEvent> = stream_over(body(
            "data: a\n\nevent: done\ndata: [DONE]\n\ndata: stop\n\n",
        ))
        .collect();
        assert_eq!(
            events,
            vec![text("a"), StreamEvent::Finish(FinishReason::EndTurn)]
        );
    }

    #[test]
    fn a_stream_cut_short_is_a_network_error() {
        let events: Vec<StreamEvent> = stream_over(body("data: a\n\ndata: b")).collect();
        assert_eq!(events[0], text("a"));
        assert_eq!(events.len(), 2);
        let error = only_error(&events);
        assert_eq!(error.kind(), ErrorKind::Network);
        assert_eq!(error.provider(), Some(ProviderId::Anthropic));
    }

    #[test]
    fn a_response_without_events_is_malformed() {
        for garbage in [
            "",
            "<html>not a stream</html>\n\n",
            "\u{0}\u{1}\u{2}garbage\n\n",
        ] {
            let events: Vec<StreamEvent> = stream_over(body(garbage)).collect();
            assert_eq!(events.len(), 1, "{garbage:?}");
            assert_eq!(only_error(&events).kind(), ErrorKind::MalformedResponse);
        }
    }

    #[test]
    fn a_failure_to_open_is_the_only_event() {
        let open: Opener = Box::new(|| Err(ProviderError::new(ErrorKind::AuthenticationFailed)));
        let events: Vec<StreamEvent> = stream_over(open).collect();
        assert_eq!(events.len(), 1);
        assert_eq!(only_error(&events).kind(), ErrorKind::AuthenticationFailed);

        let events: Vec<StreamEvent> = EventStream::failed(
            ProviderId::Google,
            ProviderError::new(ErrorKind::RequestRejected),
        )
        .collect();
        assert_eq!(events.len(), 1);
        let error = only_error(&events);
        assert_eq!(error.kind(), ErrorKind::RequestRejected);
        assert_eq!(error.provider(), Some(ProviderId::Google));
    }

    #[test]
    fn event_data_that_is_not_a_json_object_is_malformed() {
        let event = SseEvent {
            event: Some("named".to_owned()),
            data: "{\"type\":\"a\"}".to_owned(),
        };
        let mut out = VecDeque::new();
        let data = parse_data(&event, &mut out).expect("an object");
        assert!(out.is_empty());
        assert_eq!(event_type(&data, "type", &event), "a");
        assert_eq!(event_type(&data, "event_type", &event), "named");

        for bad in ["not json", "[1]", "\"text\"", "{\"cut\":"] {
            let event = SseEvent {
                event: None,
                data: bad.to_owned(),
            };
            let mut out = VecDeque::new();
            assert!(parse_data(&event, &mut out).is_none());
            let events: Vec<StreamEvent> = out.into_iter().collect();
            assert_eq!(only_error(&events).kind(), ErrorKind::MalformedResponse);
        }
    }

    #[test]
    fn capabilities_cover_the_required_rows_for_every_provider() {
        for provider in ProviderId::ALL {
            let capabilities = capabilities_of(provider);
            assert!(capabilities.streaming, "{provider}");
            assert!(capabilities.tool_calling, "{provider}");
            assert!(capabilities.image_input, "{provider}");
            // Recorded, not implemented: screen control is out of scope here.
            // Only a company's own interface has a computer-control tool of
            // its own.
            assert_eq!(
                capabilities.native_computer_control.provider_tool.is_some(),
                provider.is_company_interface(),
                "{provider}"
            );
            assert!(!capabilities.native_computer_control.implemented);
            if provider.is_company_interface() {
                assert!(endpoint_path(provider).starts_with("/v1"));
            } else {
                // Below a base URL that already holds the version.
                assert_eq!(endpoint_path(provider), "/chat/completions");
            }
        }
    }

    /// Echoes the data, finishes on the marker, says when it is interrupted.
    struct Hooks;

    impl Decoder for Hooks {
        fn decode(&mut self, event: &SseEvent, out: &mut VecDeque<StreamEvent>) {
            out.push_back(StreamEvent::TextDelta {
                text: event.data.clone(),
            });
        }

        fn done(&mut self, out: &mut VecDeque<StreamEvent>) {
            out.push_back(StreamEvent::Finish(FinishReason::EndTurn));
        }

        fn interrupted(&mut self, out: &mut VecDeque<StreamEvent>) {
            out.push_back(StreamEvent::TextDelta {
                text: "interrupted".to_owned(),
            });
        }
    }

    fn hooked(text: &'static str) -> EventStream {
        EventStream {
            provider: ProviderId::OpenAiChat,
            state: State::Pending(body(text)),
            decoder: Some(Box::new(Hooks)),
            queue: VecDeque::new(),
            received: false,
            shared: None,
        }
    }

    #[test]
    fn the_done_marker_and_an_interruption_reach_the_decoder() {
        // The marker is where this decoder finishes.
        let events: Vec<StreamEvent> = hooked("data: a\n\ndata: [DONE]\n\ndata: late\n\n").collect();
        assert_eq!(
            events,
            vec![text("a"), StreamEvent::Finish(FinishReason::EndTurn)]
        );
        // Cut after something was read: the decoder is told before the
        // failure.
        let events: Vec<StreamEvent> = hooked("data: a\n\ndata: b").collect();
        assert_eq!(events[..2], [text("a"), text("interrupted")]);
        assert_eq!(only_error(&events).kind(), ErrorKind::Network);
        // Nothing read: nothing to account for.
        let events: Vec<StreamEvent> = hooked("").collect();
        assert_eq!(events.len(), 1);
        assert_eq!(only_error(&events).kind(), ErrorKind::MalformedResponse);
    }
}
