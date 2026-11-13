// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The published configuration schema agrees with the loader, the example
//! files load, and no model name or price is compiled in.
//!
//! This crate has no JSON Schema validator, so these tests do not validate
//! documents against the schema. They check the points where the schema and
//! the Rust loader state the same fact, so that one cannot change without the
//! other.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use tur_pax_providers::config::{
    ATTEMPTS_MAX, CONFIG_SCHEMA_VERSION, DURATION_MAX_MS, MODEL_NAME_MAX,
};
use tur_pax_providers::{ErrorKind, ProviderConfig, ProviderId, RetryPolicy, Timeouts};

const SCHEMA: &str = include_str!("../../../schemas/pax-provider-config.schema.json");

const MINIMAL: &str = include_str!("data/minimal-anthropic.json");
const FULL: &str = include_str!("data/full-openai.json");
const TOKEN_LIMIT: &str = include_str!("data/token-limit-google.json");
const INVALID_NO_MODEL: &str = include_str!("data/invalid-no-model.json");
const INVALID_PLAIN_HTTP: &str = include_str!("data/invalid-plain-http.json");
const FULL_CHAT: &str = include_str!("data/full-openai-chat.json");

/// Every source file of the library and the binary.
const SOURCES: [(&str, &str); 19] = [
    ("anthropic.rs", include_str!("../src/anthropic.rs")),
    ("cli.rs", include_str!("../src/cli.rs")),
    ("config.rs", include_str!("../src/config.rs")),
    ("error.rs", include_str!("../src/error.rs")),
    ("generation.rs", include_str!("../src/generation.rs")),
    ("google.rs", include_str!("../src/google.rs")),
    ("http.rs", include_str!("../src/http.rs")),
    ("lib.rs", include_str!("../src/lib.rs")),
    ("listing.rs", include_str!("../src/listing.rs")),
    ("main.rs", include_str!("../src/main.rs")),
    ("model.rs", include_str!("../src/model.rs")),
    ("openai.rs", include_str!("../src/openai.rs")),
    ("openai_chat.rs", include_str!("../src/openai_chat.rs")),
    ("probe.rs", include_str!("../src/probe.rs")),
    ("provider.rs", include_str!("../src/provider.rs")),
    ("secret.rs", include_str!("../src/secret.rs")),
    ("spend.rs", include_str!("../src/spend.rs")),
    ("sse.rs", include_str!("../src/sse.rs")),
    ("toolcalls.rs", include_str!("../src/toolcalls.rs")),
];

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the schema is valid JSON")
}

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn the_schema_agrees_with_the_loader() {
    let schema = schema();
    assert_eq!(
        schema["$id"],
        "https://github.com/giorgiogiudice/TUR/schemas/pax-provider-config.schema.json"
    );
    assert_eq!(schema["additionalProperties"], false);
    let properties = &schema["properties"];
    assert_eq!(properties["schema_version"]["const"], CONFIG_SCHEMA_VERSION);

    // The providers are the ones with an adapter.
    let providers: Vec<&str> = properties["provider"]["enum"]
        .as_array()
        .expect("an enum")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let adapters: Vec<&str> = ProviderId::ALL.iter().map(|id| id.as_str()).collect();
    assert_eq!(providers, adapters);

    // The same limits.
    assert_eq!(properties["model"]["maxLength"], MODEL_NAME_MAX);
    assert_eq!(schema["$defs"]["duration_ms"]["maximum"], DURATION_MAX_MS);
    assert_eq!(
        properties["retry"]["properties"]["max_attempts"]["maximum"],
        ATTEMPTS_MAX
    );

    // The same defaults.
    let timeouts = Timeouts::default();
    let documented = &properties["timeouts"]["properties"];
    assert_eq!(documented["connect_ms"]["default"], timeouts.connect_ms);
    assert_eq!(documented["response_ms"]["default"], timeouts.response_ms);
    assert_eq!(documented["idle_ms"]["default"], timeouts.idle_ms);
    let retry = RetryPolicy::default();
    let documented = &properties["retry"]["properties"];
    assert_eq!(documented["max_attempts"]["default"], retry.max_attempts);
    assert_eq!(
        documented["initial_delay_ms"]["default"],
        retry.initial_delay_ms
    );
    assert_eq!(documented["max_delay_ms"]["default"], retry.max_delay_ms);

    // Every object is closed, as the loader's are.
    for name in ["timeouts", "retry", "prices", "task_limit"] {
        assert_eq!(properties[name]["additionalProperties"], false, "{name}");
    }
}

#[test]
fn the_schema_and_the_loader_have_the_same_fields() {
    let schema = schema();
    let properties = &schema["properties"];
    let full = ProviderConfig::from_json(FULL).expect("the full example loads");
    let serialised = serde_json::to_value(&full).expect("a configuration serialises");
    // One field is read by the OpenAI-compatible family only, so the two
    // full examples together have every field.
    let chat = ProviderConfig::from_json(FULL_CHAT).expect("the full chat example loads");
    let chat = serde_json::to_value(&chat).expect("a configuration serialises");
    let mut every = keys(&serialised);
    every.extend(keys(&chat));
    assert_eq!(every, keys(properties));
    for name in ["timeouts", "retry", "prices", "task_limit"] {
        assert_eq!(
            keys(&serialised[name]),
            keys(&properties[name]["properties"]),
            "{name}"
        );
    }
    // What the schema requires is what the loader cannot do without.
    let required = |value: &Value| -> BTreeSet<String> {
        value["required"]
            .as_array()
            .expect("a required list")
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    for field in required(&schema) {
        let mut without = serialised.clone();
        without.as_object_mut().expect("an object").remove(&field);
        let error = ProviderConfig::from_json(&without.to_string()).expect_err("a required field");
        assert_eq!(error.kind(), ErrorKind::ConfigInvalid, "{field}");
    }
    for field in required(&properties["prices"]) {
        let mut without = serialised.clone();
        without["prices"]
            .as_object_mut()
            .expect("an object")
            .remove(&field);
        assert!(
            ProviderConfig::from_json(&without.to_string()).is_err(),
            "{field}"
        );
    }
}

#[test]
fn model_and_prices_have_no_default_anywhere() {
    let schema = schema();
    let properties = &schema["properties"];
    let required: Vec<&str> = schema["required"]
        .as_array()
        .expect("a required list")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(required.contains(&"model"));
    assert!(required.contains(&"max_output_tokens"));
    assert!(properties["model"].get("default").is_none());
    assert!(properties["max_output_tokens"].get("default").is_none());
    assert!(properties["prices"].get("default").is_none());
    for price in [
        "input_per_million",
        "output_per_million",
        "cached_input_per_million",
    ] {
        assert!(
            properties["prices"]["properties"][price]
                .get("default")
                .is_none()
        );
    }
    assert!(schema["$defs"]["price"].get("default").is_none());

    // A configuration without prices has none.
    let minimal = ProviderConfig::from_json(MINIMAL).expect("the minimal example loads");
    assert_eq!(minimal.prices, None);
    assert_eq!(minimal.task_limit, None);
}

#[test]
fn no_model_name_is_compiled_in() {
    // The families of model names the three providers use. A source file that
    // contained one would be naming a model.
    let families = ["claude-", "gpt-", "gemini-", "o1-", "o3-", "o4-"];
    for (name, text) in SOURCES {
        let lower = text.to_lowercase();
        for family in families {
            assert!(
                !lower.contains(family),
                "{name} contains a model name ({family})"
            );
        }
    }
    for example in [MINIMAL, FULL, TOKEN_LIMIT, FULL_CHAT, SCHEMA] {
        let lower = example.to_lowercase();
        for family in families {
            assert!(
                !lower.contains(family),
                "an example names a model ({family})"
            );
        }
    }
}

#[test]
fn every_source_file_states_its_licence() {
    for (name, text) in SOURCES {
        assert!(
            text.starts_with("// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\n"),
            "{name}"
        );
    }
}

#[test]
fn the_examples_load() {
    let minimal = ProviderConfig::from_json(MINIMAL).expect("the minimal example loads");
    assert_eq!(minimal.provider, ProviderId::Anthropic);
    assert_eq!(minimal.effective_base_url(), "https://api.anthropic.com");

    let full = ProviderConfig::from_json(FULL).expect("the full example loads");
    assert_eq!(full.provider, ProviderId::OpenAi);
    assert_eq!(
        full.effective_base_url(),
        "https://gateway.example.org/openai"
    );
    assert_eq!(full.retry.max_attempts, 4);
    assert_eq!(
        serde_json::to_value(full.prices.expect("prices")).expect("prices serialise"),
        json!({
            "currency": "USD",
            "input_per_million": 1.5,
            "output_per_million": 6.0,
            "cached_input_per_million": 0.15
        })
    );

    let limited = ProviderConfig::from_json(TOKEN_LIMIT).expect("the token limit example loads");
    assert_eq!(limited.provider, ProviderId::Google);
    assert_eq!(
        limited.task_limit.and_then(|limit| limit.max_tokens),
        Some(100_000)
    );

    let chat = ProviderConfig::from_json(FULL_CHAT).expect("the chat example loads");
    assert_eq!(chat.provider, ProviderId::OpenAiChat);
    assert_eq!(chat.effective_base_url(), "http://127.0.0.1:11434/v1");
    assert!(chat.send_back_reasoning);

    for invalid in [INVALID_NO_MODEL, INVALID_PLAIN_HTTP] {
        let error = ProviderConfig::from_json(invalid).expect_err("an invalid example");
        assert_eq!(error.kind(), ErrorKind::ConfigInvalid);
        assert_eq!(error.code(), "TUR-AI-PROVIDER-011");
    }
}
