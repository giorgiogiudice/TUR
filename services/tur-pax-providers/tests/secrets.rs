// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The API key never appears in an error, an event or a debug rendering.
//!
//! The tests run every kind of outcome against every adapter with a known key
//! in play, format everything the layer produced with both `Display` and
//! `Debug`, and assert that the key text is nowhere in it. The fake server is
//! hostile on purpose: it repeats the key it was sent inside its error messages,
//! as a provider or a gateway might.

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::json;
use tur_pax_providers::error::REGISTRY;
use tur_pax_providers::{
    ApiKey, EnvKeySource, ErrorKind, FinishReason, KeySource, ProviderConfig, ProviderId,
    SpendMeter, StreamEvent, TaskLimit,
};

use common::{FakeServer, Reply, TEST_KEY, Wire, config_with, hello};

/// Everything the layer produced, rendered as text, and what it covered.
#[derive(Default)]
struct Seen {
    renderings: Vec<String>,
    kinds: BTreeSet<ErrorKind>,
    events: BTreeSet<&'static str>,
}

impl Seen {
    fn event(&mut self, event: &StreamEvent) {
        self.renderings.push(format!("{event:?}"));
        self.renderings.push(format!("{event:#?}"));
        let name = match event {
            StreamEvent::TextDelta { .. } => "text_delta",
            StreamEvent::ToolCallStart { .. } => "tool_call_start",
            StreamEvent::ToolCallArgumentsDelta { .. } => "tool_call_arguments_delta",
            StreamEvent::ToolCallEnd { .. } => "tool_call_end",
            StreamEvent::Opaque(_) => "opaque",
            StreamEvent::Usage(_) => "usage",
            StreamEvent::Finish(reason) => {
                if let Some(error) = reason.error() {
                    self.kinds.insert(error.kind());
                    self.renderings.push(error.to_string());
                    self.renderings.push(format!("{error:?}"));
                    self.renderings
                        .push(error.detail().unwrap_or_default().to_owned());
                }
                "finish"
            }
        };
        self.events.insert(name);
    }

    fn events(&mut self, events: &[StreamEvent]) {
        assert!(
            matches!(events.last(), Some(StreamEvent::Finish(_))),
            "{events:?}"
        );
        for event in events {
            self.event(event);
        }
    }

    fn assert_no_key(&self) {
        assert!(!self.renderings.is_empty());
        for rendering in &self.renderings {
            assert!(
                !rendering.contains(TEST_KEY),
                "the key appears in: {rendering}"
            );
        }
    }
}

/// An error body in the provider's documented shape whose message repeats the key.
fn echoing_error(provider: ProviderId) -> String {
    let message = format!("The credential {TEST_KEY} was not accepted ({TEST_KEY}).");
    let body = match provider {
        ProviderId::Anthropic => {
            json!({ "type": "error", "error": { "type": "a_type_added_later", "message": message } })
        }
        ProviderId::OpenAi => {
            json!({ "error": { "message": message, "type": "a_type_added_later", "code": null } })
        }
        ProviderId::Google => json!({ "error": { "code": "a_code_added_later", "message": message } }),
    };
    body.to_string()
}

/// An error event in the provider's documented shape whose message repeats the key.
fn echoing_error_event(provider: ProviderId) -> String {
    let message = format!("Something failed for {TEST_KEY}.");
    let data = match provider {
        ProviderId::Anthropic => {
            json!({ "type": "error", "error": { "type": "api_error", "message": message } })
        }
        ProviderId::OpenAi => {
            json!({ "type": "error", "code": "server_error", "message": message, "param": null })
        }
        ProviderId::Google => {
            json!({ "event_type": "error", "error": { "code": "api_error", "message": message } })
        }
    };
    format!("event: error\ndata: {data}\n\n")
}

fn every_outcome(wire: &Wire, seen: &mut Seen) {
    // The scripted streams: every event type and every way a stream can end.
    for script in [
        wire.plain,
        wire.tool_call,
        wire.two_tool_calls,
        wire.refusal,
        wire.overloaded_mid_stream,
        wire.output_limit,
    ] {
        let server = FakeServer::start(vec![Reply::stream(script)]);
        let config = common::config(wire.provider, &server);
        let provider = common::provider(&config);
        let stream = provider.stream(&hello(&config));
        seen.renderings.push(format!("{stream:?}"));
        let events: Vec<StreamEvent> = stream.collect();
        seen.events(&events);
    }

    // A stream that is cut, and bytes that are not a stream at all.
    for reply in [
        Reply::cut(wire.cut_short),
        Reply::Stream {
            body: b"\xff\xfe not a stream\n\n".to_vec(),
            piece: 3,
            cut: false,
        },
    ] {
        let server = FakeServer::start(vec![reply]);
        let config = common::config(wire.provider, &server);
        seen.events(&common::run(&config, &hello(&config)));
    }

    // Error statuses whose bodies repeat the key.
    for status in [400, 401, 402, 403, 404, 429, 500, 503] {
        let server = FakeServer::start(vec![Reply::status_retry_after(
            status,
            &echoing_error(wire.provider),
            "7",
        )]);
        let config = config_with(wire.provider, &server, |value| {
            value["retry"] = json!({ "max_attempts": 1 });
        });
        let events = common::run(&config, &hello(&config));
        match events.last() {
            Some(StreamEvent::Finish(FinishReason::Error(error))) => {
                // The message arrived, with the key taken out of it.
                let detail = error.detail().expect("the provider's message");
                assert!(detail.contains("[redacted]"), "{detail}");
                assert_eq!(error.http_status(), Some(status));
            }
            other => panic!("not an error: {other:?}"),
        }
        seen.events(&events);
    }

    // An error event, inside a stream, that repeats the key.
    let server = FakeServer::start(vec![Reply::stream(&echoing_error_event(wire.provider))]);
    let config = common::config(wire.provider, &server);
    let events = common::run(&config, &hello(&config));
    match events.last() {
        Some(StreamEvent::Finish(FinishReason::Error(error))) => {
            let detail = error.detail().expect("the provider's message");
            assert!(detail.contains("[redacted]"), "{detail}");
        }
        other => panic!("not an error: {other:?}"),
    }
    seen.events(&events);

    // No answer in time, and nobody listening.
    let server = FakeServer::start(vec![Reply::Silence(Duration::from_millis(700))]);
    let config = config_with(wire.provider, &server, |value| {
        value["timeouts"] = json!({ "connect_ms": 3000, "response_ms": 200, "idle_ms": 3000 });
        value["retry"] = json!({ "max_attempts": 1 });
    });
    seen.events(&common::run(&config, &hello(&config)));

    let server = FakeServer::start(Vec::new());
    let config = config_with(wire.provider, &server, |value| {
        value["retry"] = json!({ "max_attempts": 1 });
    });
    drop(server);
    seen.events(&common::run(&config, &hello(&config)));

    // A task that is over its limit.
    let server = FakeServer::start(vec![Reply::stream(wire.plain)]);
    let config = common::config(wire.provider, &server);
    let provider = common::provider(&config);
    let mut meter = SpendMeter::new(
        None,
        TaskLimit {
            max_cost: None,
            max_tokens: Some(1),
        },
    );
    let events: Vec<StreamEvent> = meter.meter(provider.stream(&hello(&config))).collect();
    seen.events(&events);
    seen.renderings.push(format!("{meter:?}"));

    // The configuration, which never holds the key.
    seen.renderings.push(format!("{config:?}"));
    seen.renderings
        .push(serde_json::to_string(&config).expect("a configuration serialises"));
}

fn local_outcomes(seen: &mut Seen) {
    // The key itself.
    let key = ApiKey::new(TEST_KEY.to_owned()).expect("a usable key");
    seen.renderings.push(format!("{key}"));
    seen.renderings.push(format!("{key:?}"));
    seen.renderings.push(format!("{key:#?}"));

    // A key that cannot be used is not repeated in the error.
    let unusable = ApiKey::new(format!("{TEST_KEY} with spaces")).expect_err("an unusable key");
    seen.kinds.insert(unusable.kind());
    seen.renderings.push(unusable.to_string());
    seen.renderings.push(format!("{unusable:?}"));

    // A key source that has nothing.
    let source = EnvKeySource::new("TUR_PAX_PROVIDERS_TEST_VARIABLE_THAT_IS_NEVER_SET");
    let missing = source
        .api_key(ProviderId::Anthropic)
        .expect_err("the variable is not set");
    seen.kinds.insert(missing.kind());
    seen.renderings.push(missing.to_string());
    seen.renderings.push(format!("{source:?}"));

    // A key pasted into a configuration file is rejected, and not repeated.
    let pasted = json!({
        "schema_version": 1,
        "provider": "openai",
        "model": "example-model",
        "max_output_tokens": 16,
        "api_key": TEST_KEY
    });
    let invalid = ProviderConfig::from_json(&pasted.to_string()).expect_err("an unknown field");
    seen.kinds.insert(invalid.kind());
    seen.renderings.push(invalid.to_string());
    seen.renderings.push(format!("{invalid:?}"));
}

#[test]
fn the_key_never_appears_in_any_error_or_event() {
    let mut seen = Seen::default();
    for wire in [
        common::anthropic::wire(),
        common::openai::wire(),
        common::google::wire(),
    ] {
        every_outcome(&wire, &mut seen);
    }
    local_outcomes(&mut seen);

    seen.assert_no_key();

    // The test means what it says only if it saw every error kind and every
    // event type.
    let every_kind: BTreeSet<ErrorKind> = REGISTRY.iter().map(|(kind, _, _)| *kind).collect();
    assert_eq!(seen.kinds, every_kind);
    assert_eq!(
        seen.events,
        BTreeSet::from([
            "text_delta",
            "tool_call_start",
            "tool_call_arguments_delta",
            "tool_call_end",
            "opaque",
            "usage",
            "finish",
        ])
    );
}

#[test]
fn the_request_carries_the_key_only_in_the_documented_header() {
    for wire in [
        common::anthropic::wire(),
        common::openai::wire(),
        common::google::wire(),
    ] {
        let server = FakeServer::start(vec![Reply::stream(wire.plain)]);
        let config = common::config(wire.provider, &server);
        let events = common::run(&config, &hello(&config));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::Finish(FinishReason::EndTurn))
        ));

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(!request.path.contains(TEST_KEY), "the key is in the URL");
        assert!(!request.body.contains(TEST_KEY), "the key is in the body");
        let carriers: Vec<&str> = request
            .headers
            .iter()
            .filter(|(_, value)| value.contains(TEST_KEY))
            .map(|(name, _)| name.as_str())
            .collect();
        let expected = match wire.provider {
            ProviderId::Anthropic => "x-api-key",
            ProviderId::OpenAi => "authorization",
            ProviderId::Google => "x-goog-api-key",
        };
        assert_eq!(carriers, [expected]);
    }
}
