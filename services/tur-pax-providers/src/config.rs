// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Provider configuration.
//!
//! The file format is described by `schemas/pax-provider-config.schema.json`.
//! [`ProviderConfig::from_json`] is authoritative: it parses the file and then
//! checks every rule, including the ones a JSON Schema cannot express.
//!
//! No model name and no price has a default in this crate. They are
//! configuration (ADR-0004). The API key is not part of the configuration: it
//! comes from a [`crate::secret::KeySource`].

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorKind, ProviderError};
use crate::generation::Generation;
use crate::model::{Message, ProviderId, Request, ToolDefinition};

/// The configuration schema version this build reads.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// The longest model name, in characters.
pub const MODEL_NAME_MAX: usize = 200;

/// The longest timeout or delay, in milliseconds: one hour.
pub const DURATION_MAX_MS: u64 = 3_600_000;

/// The most attempts a request may be given.
pub const ATTEMPTS_MAX: u32 = 10;

/// The configuration of one provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    /// The schema version. Must be [`CONFIG_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The adapter family, which selects the adapter.
    pub provider: ProviderId,
    /// The base URL of the provider's API. Without it the family's public
    /// endpoint is used. Set it to use a compatible gateway. The
    /// OpenAI-compatible family has no public endpoint and needs one: the
    /// address up to the path `/chat/completions`, as the service documents
    /// it (for example `https://api.example.org/v1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Whether the model's reasoning text, which the service streams beside
    /// the answer, is sent back with the assistant turns of later requests.
    /// Only the OpenAI-compatible family reads it, and only for a service
    /// whose documentation requires it (DeepSeek, when tools are offered).
    /// Off by default: the reasoning is then kept with the conversation and
    /// never sent.
    #[serde(default, skip_serializing_if = "is_false")]
    pub send_back_reasoning: bool,
    /// The model name, as the provider spells it.
    pub model: String,
    /// The most output tokens the model may produce for one request.
    pub max_output_tokens: u32,
    /// Timeouts.
    #[serde(default)]
    pub timeouts: Timeouts,
    /// Retry behaviour.
    #[serde(default)]
    pub retry: RetryPolicy,
    /// Prices, for showing what a task cost. Without them only tokens are counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prices: Option<Prices>,
    /// The limit for one task. Without it a task is not limited by this layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_limit: Option<TaskLimit>,
    /// Generation settings: temperature, top-p, top-k, stop sequences, how
    /// much the model reasons. Only what is set is sent; a setting the
    /// family's interface has no place for is refused
    /// ([`Generation::check`]).
    #[serde(default, skip_serializing_if = "Generation::is_empty")]
    pub generation: Generation,
}

/// Timeouts, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Timeouts {
    /// The longest wait for the connection to be established.
    pub connect_ms: u64,
    /// The longest wait for the provider to start answering.
    pub response_ms: u64,
    /// The longest silence tolerated in the middle of a stream.
    pub idle_ms: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            connect_ms: 10_000,
            response_ms: 120_000,
            idle_ms: 120_000,
        }
    }
}

/// How failed requests are retried.
///
/// Only failures the providers document as transient are retried: a rate
/// limit, an overloaded or unavailable service, a connection that could not be
/// made and a timeout. A request is never retried once any of its response has
/// been delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RetryPolicy {
    /// The most times one request is sent, the first attempt included.
    pub max_attempts: u32,
    /// The delay before the second attempt. It doubles for each later attempt.
    pub initial_delay_ms: u64,
    /// The longest delay between two attempts. If the provider asks for a
    /// longer delay, the request is not retried and the delay is reported.
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 3,
            initial_delay_ms: 500,
            max_delay_ms: 30_000,
        }
    }
}

/// Prices per million tokens, for showing what a task cost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prices {
    /// The currency the prices are in, for display, for example `USD`.
    pub currency: String,
    /// The price of a million input tokens.
    pub input_per_million: f64,
    /// The price of a million output tokens.
    pub output_per_million: f64,
    /// The price of a million input tokens read from the provider's cache.
    /// Without it they are priced as ordinary input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_per_million: Option<f64>,
}

/// The limit for one task. A stream is stopped when the task goes over it.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskLimit {
    /// The most a task may cost, in the currency of the prices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost: Option<f64>,
    /// The most tokens a task may use, input and output together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

impl ProviderConfig {
    /// Load and validate a configuration from its JSON text.
    pub fn from_json(text: &str) -> Result<Self, ProviderError> {
        let config: ProviderConfig =
            serde_json::from_str(text).map_err(|error| invalid(&error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Check every rule of the format.
    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(invalid(&format!(
                "schema_version is {}, this build reads {CONFIG_SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        let model_ok = !self.model.trim().is_empty()
            && self.model.chars().count() <= MODEL_NAME_MAX
            && !self.model.chars().any(char::is_control)
            && self.model.trim() == self.model;
        if !model_ok {
            return Err(invalid(
                "model must be 1 to 200 characters with no control characters and no surrounding whitespace",
            ));
        }
        if self.max_output_tokens == 0 {
            return Err(invalid("max_output_tokens must be at least 1"));
        }
        match (&self.base_url, default_base_url(self.provider)) {
            (Some(base_url), _) => check_base_url(base_url)?,
            (None, Some(_)) => {}
            (None, None) => {
                return Err(invalid(&format!(
                    "base_url is needed: the {} family has no public address of its own",
                    self.provider
                )));
            }
        }
        if self.send_back_reasoning && self.provider != ProviderId::OpenAiChat {
            return Err(invalid(
                "send_back_reasoning is read by the openai-chat family only",
            ));
        }
        for (name, value) in [
            ("timeouts.connect_ms", self.timeouts.connect_ms),
            ("timeouts.response_ms", self.timeouts.response_ms),
            ("timeouts.idle_ms", self.timeouts.idle_ms),
            ("retry.max_delay_ms", self.retry.max_delay_ms),
        ] {
            if value == 0 || value > DURATION_MAX_MS {
                return Err(invalid(&format!(
                    "{name} must be between 1 and {DURATION_MAX_MS}"
                )));
            }
        }
        if self.retry.initial_delay_ms > self.retry.max_delay_ms {
            return Err(invalid(
                "retry.initial_delay_ms must not be larger than retry.max_delay_ms",
            ));
        }
        if self.retry.max_attempts == 0 || self.retry.max_attempts > ATTEMPTS_MAX {
            return Err(invalid(&format!(
                "retry.max_attempts must be between 1 and {ATTEMPTS_MAX}"
            )));
        }
        if let Some(prices) = &self.prices {
            let currency_ok = !prices.currency.is_empty()
                && prices.currency.len() <= 8
                && prices.currency.bytes().all(|b| b.is_ascii_alphabetic());
            if !currency_ok {
                return Err(invalid("prices.currency must be 1 to 8 ASCII letters"));
            }
            let mut amounts = vec![prices.input_per_million, prices.output_per_million];
            amounts.extend(prices.cached_input_per_million);
            if amounts.iter().any(|amount| !is_price(*amount)) {
                return Err(invalid(
                    "prices must be finite numbers that are not negative",
                ));
            }
        }
        self.generation
            .check(self.provider)
            .map_err(|detail| invalid(&format!("generation: {detail}")))?;
        if let Some(limit) = &self.task_limit {
            if limit.max_cost.is_none() && limit.max_tokens.is_none() {
                return Err(invalid("task_limit must set max_cost, max_tokens or both"));
            }
            if let Some(max_cost) = limit.max_cost {
                if !is_price(max_cost) || max_cost == 0.0 {
                    return Err(invalid(
                        "task_limit.max_cost must be a finite number above 0",
                    ));
                }
                if self.prices.is_none() {
                    return Err(invalid("task_limit.max_cost needs prices to be configured"));
                }
            }
            if limit.max_tokens == Some(0) {
                return Err(invalid("task_limit.max_tokens must be at least 1"));
            }
        }
        Ok(())
    }

    /// The base URL requests go to: the configured one, or the family's
    /// public endpoint. It has no trailing slash. Empty for a configuration
    /// that [`ProviderConfig::validate`] refuses because it names no address
    /// and its family has none.
    pub fn effective_base_url(&self) -> &str {
        match &self.base_url {
            Some(base_url) => base_url.trim_end_matches('/'),
            None => default_base_url(self.provider).unwrap_or(""),
        }
    }

    /// A request for this configuration's model and output limit.
    pub fn request(
        &self,
        system: Option<String>,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> Request {
        Request {
            model: self.model.clone(),
            system,
            messages,
            tools,
            max_output_tokens: self.max_output_tokens,
            generation: self.generation.clone(),
        }
    }
}

/// The public endpoint of an adapter family, if it has one. The
/// OpenAI-compatible family is spoken by many services and has none: every
/// configuration of it names its address.
pub const fn default_base_url(provider: ProviderId) -> Option<&'static str> {
    match provider {
        ProviderId::Anthropic => Some("https://api.anthropic.com"),
        ProviderId::OpenAi => Some("https://api.openai.com"),
        ProviderId::Google => Some("https://generativelanguage.googleapis.com"),
        ProviderId::OpenAiChat => None,
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_price(amount: f64) -> bool {
    amount.is_finite() && amount >= 0.0
}

fn invalid(detail: &str) -> ProviderError {
    ProviderError::new(ErrorKind::ConfigInvalid).with_detail(detail)
}

/// Check a base URL.
///
/// It must be `https://host[:port][/path]`. Plain `http` is accepted only for a
/// loopback host, so that a key is never sent unencrypted over a network. User
/// information, a query and a fragment are not accepted.
fn check_base_url(base_url: &str) -> Result<(), ProviderError> {
    let problem = || {
        invalid(
            "base_url must be https://host[:port][/path]; http is accepted for a loopback host only",
        )
    };
    let (scheme, rest) = base_url.split_once("://").ok_or_else(problem)?;
    let plain = match scheme {
        "https" => false,
        "http" => true,
        _ => return Err(problem()),
    };
    let authority = rest.split('/').next().unwrap_or("");
    let characters_ok = !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_graphic())
        && !rest.contains(['?', '#', '@', '\\']);
    if !characters_ok || authority.is_empty() {
        return Err(problem());
    }
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        // An IPv6 literal: [address] or [address]:port.
        let (address, after) = bracketed.split_once(']').ok_or_else(problem)?;
        if !(after.is_empty() || is_port(after.strip_prefix(':'))) {
            return Err(problem());
        }
        address
    } else {
        match authority.split_once(':') {
            Some((host, port)) => {
                if !is_port(Some(port)) {
                    return Err(problem());
                }
                host
            }
            None => authority,
        }
    };
    if host.is_empty() {
        return Err(problem());
    }
    let loopback = host == "localhost"
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if plain && !loopback {
        return Err(problem());
    }
    Ok(())
}

fn is_port(text: Option<&str>) -> bool {
    text.is_some_and(|port| !port.is_empty() && port.parse::<u16>().is_ok())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn minimal() -> Value {
        json!({
            "schema_version": 1,
            "provider": "anthropic",
            "model": "example-model",
            "max_output_tokens": 1024
        })
    }

    fn with(change: impl FnOnce(&mut Value)) -> Result<ProviderConfig, ProviderError> {
        let mut value = minimal();
        change(&mut value);
        ProviderConfig::from_json(&value.to_string())
    }

    fn rejected(change: impl FnOnce(&mut Value)) -> String {
        let error = with(change).expect_err("the configuration must be rejected");
        assert_eq!(error.kind(), ErrorKind::ConfigInvalid);
        error.detail().unwrap_or("").to_owned()
    }

    #[test]
    fn a_minimal_configuration_gets_the_documented_defaults() {
        let config = with(|_| {}).unwrap();
        assert_eq!(config.provider, ProviderId::Anthropic);
        assert_eq!(config.model, "example-model");
        assert_eq!(config.max_output_tokens, 1024);
        assert_eq!(config.base_url, None);
        assert_eq!(config.effective_base_url(), "https://api.anthropic.com");
        assert_eq!(config.timeouts, Timeouts::default());
        assert_eq!(config.retry, RetryPolicy::default());
        assert_eq!(config.prices, None);
        assert_eq!(config.task_limit, None);
    }

    #[test]
    fn every_company_family_has_a_public_endpoint_and_the_compatible_one_none() {
        for provider in ProviderId::ALL {
            match default_base_url(provider) {
                Some(public) => {
                    assert!(provider.is_company_interface(), "{provider}");
                    let config =
                        with(|value| value["provider"] = json!(provider.as_str())).unwrap();
                    assert_eq!(config.provider, provider);
                    assert!(config.effective_base_url().starts_with("https://"));
                    assert_eq!(config.effective_base_url(), public);
                }
                None => {
                    assert!(!provider.is_company_interface(), "{provider}");
                    // Without an address of its own a configuration is refused.
                    let detail = rejected(|value| value["provider"] = json!(provider.as_str()));
                    assert!(detail.contains("base_url"), "{detail}");
                    let config = with(|value| {
                        value["provider"] = json!(provider.as_str());
                        value["base_url"] = json!("https://api.example.org/v1/");
                    })
                    .unwrap();
                    assert_eq!(config.effective_base_url(), "https://api.example.org/v1");
                }
            }
        }
        assert_eq!(default_base_url(ProviderId::OpenAiChat), None);
    }

    #[test]
    fn reasoning_is_sent_back_only_by_the_family_that_reads_the_setting() {
        let config = with(|value| {
            value["provider"] = json!("openai-chat");
            value["base_url"] = json!("http://127.0.0.1:11434/v1");
            value["send_back_reasoning"] = json!(true);
        })
        .unwrap();
        assert!(config.send_back_reasoning);
        let text = serde_json::to_string(&config).unwrap();
        assert_eq!(ProviderConfig::from_json(&text).unwrap(), config);
        // Off by default, and not written out when off.
        let plain = with(|_| {}).unwrap();
        assert!(!plain.send_back_reasoning);
        assert!(
            !serde_json::to_string(&plain)
                .unwrap()
                .contains("send_back_reasoning")
        );
        let detail = rejected(|value| value["send_back_reasoning"] = json!(true));
        assert!(detail.contains("openai-chat"), "{detail}");
    }

    #[test]
    fn model_and_output_limit_have_no_default() {
        let detail = rejected(|value| {
            value.as_object_mut().unwrap().remove("model");
        });
        assert!(detail.contains("model"), "{detail}");
        let detail = rejected(|value| {
            value.as_object_mut().unwrap().remove("max_output_tokens");
        });
        assert!(detail.contains("max_output_tokens"), "{detail}");
        rejected(|value| value["model"] = json!(""));
        rejected(|value| value["model"] = json!(" padded "));
        rejected(|value| value["model"] = json!("line\nbreak"));
        rejected(|value| value["max_output_tokens"] = json!(0));
    }

    #[test]
    fn unknown_fields_versions_and_providers_are_rejected() {
        rejected(|value| value["api_key"] = json!("sk-should-never-be-here"));
        rejected(|value| value["schema_version"] = json!(2));
        rejected(|value| value["provider"] = json!("someone-else"));
        rejected(|value| value["timeouts"] = json!({ "total_ms": 5 }));
        rejected(|value| value["retry"] = json!({ "forever": true }));
    }

    #[test]
    fn a_full_configuration_round_trips() {
        let config = with(|value| {
            value["base_url"] = json!("https://gateway.example.org/anthropic/");
            value["timeouts"] = json!({ "connect_ms": 1000, "response_ms": 2000, "idle_ms": 3000 });
            value["retry"] =
                json!({ "max_attempts": 5, "initial_delay_ms": 10, "max_delay_ms": 100 });
            value["prices"] = json!({
                "currency": "USD",
                "input_per_million": 3.0,
                "output_per_million": 15.0,
                "cached_input_per_million": 0.3
            });
            value["task_limit"] = json!({ "max_cost": 2.5, "max_tokens": 500000 });
        })
        .unwrap();
        assert_eq!(
            config.effective_base_url(),
            "https://gateway.example.org/anthropic"
        );
        assert_eq!(config.timeouts.idle_ms, 3000);
        assert_eq!(config.retry.max_attempts, 5);
        let text = serde_json::to_string(&config).unwrap();
        assert_eq!(ProviderConfig::from_json(&text).unwrap(), config);
    }

    #[test]
    fn partial_timeouts_and_retry_keep_the_other_defaults() {
        let config = with(|value| {
            value["timeouts"] = json!({ "idle_ms": 5000 });
            value["retry"] = json!({ "max_attempts": 1 });
        })
        .unwrap();
        assert_eq!(config.timeouts.idle_ms, 5000);
        assert_eq!(config.timeouts.connect_ms, Timeouts::default().connect_ms);
        assert_eq!(config.retry.max_attempts, 1);
        assert_eq!(
            config.retry.max_delay_ms,
            RetryPolicy::default().max_delay_ms
        );
    }

    #[test]
    fn base_urls_must_be_https_unless_loopback() {
        for good in [
            "https://api.example.org",
            "https://api.example.org:8443/v2/",
            "http://127.0.0.1:8080",
            "http://localhost:1234/prefix",
            "http://[::1]:9000",
            "https://[2001:db8::1]/gateway",
        ] {
            assert!(
                with(|value| value["base_url"] = json!(good)).is_ok(),
                "{good}"
            );
        }
        for bad in [
            "",
            "api.example.org",
            "ftp://api.example.org",
            "http://api.example.org",
            "http://10.0.0.5:8080",
            "http://127.example.org",
            "http://localhost.example.org",
            "https://",
            "https://user:password@api.example.org",
            "https://api.example.org/?key=1",
            "https://api.example.org/#fragment",
            "https://api.example.org:notaport",
            "https://api example.org",
            "http://[::1",
        ] {
            rejected(|value| value["base_url"] = json!(bad));
        }
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        rejected(|value| value["timeouts"] = json!({ "connect_ms": 0 }));
        rejected(|value| value["timeouts"] = json!({ "idle_ms": DURATION_MAX_MS + 1 }));
        rejected(|value| value["retry"] = json!({ "max_attempts": 0 }));
        rejected(|value| value["retry"] = json!({ "max_attempts": ATTEMPTS_MAX + 1 }));
        rejected(|value| value["retry"] = json!({ "initial_delay_ms": 10, "max_delay_ms": 5 }));
        rejected(|value| {
            value["prices"] =
                json!({ "currency": "USD", "input_per_million": -1.0, "output_per_million": 1.0 });
        });
        rejected(|value| {
            value["prices"] =
                json!({ "currency": "U$D", "input_per_million": 1.0, "output_per_million": 1.0 });
        });
        rejected(|value| value["task_limit"] = json!({}));
        rejected(|value| value["task_limit"] = json!({ "max_tokens": 0 }));
        // A cost limit cannot be enforced without prices.
        rejected(|value| value["task_limit"] = json!({ "max_cost": 1.0 }));
        rejected(|value| {
            value["prices"] =
                json!({ "currency": "USD", "input_per_million": 1.0, "output_per_million": 1.0 });
            value["task_limit"] = json!({ "max_cost": 0.0 });
        });
    }

    #[test]
    fn a_request_takes_model_and_limit_from_the_configuration() {
        let config = with(|_| {}).unwrap();
        let request = config.request(
            Some("Be brief.".to_owned()),
            vec![Message::user_text("Hello")],
            Vec::new(),
        );
        assert_eq!(request.model, "example-model");
        assert_eq!(request.max_output_tokens, 1024);
        assert_eq!(request.system.as_deref(), Some("Be brief."));
        assert_eq!(request.messages.len(), 1);
        assert!(request.generation.is_empty());
    }

    #[test]
    fn generation_settings_are_checked_for_the_family_and_reach_the_request() {
        let config = with(|value| {
            value["generation"] = json!({ "effort": "high", "stop": ["END"] });
        })
        .unwrap();
        let request = config.request(None, vec![Message::user_text("Hello")], Vec::new());
        assert_eq!(request.generation, config.generation);
        assert_eq!(
            request.generation.effort,
            Some(crate::generation::Effort::High)
        );
        let text = serde_json::to_string(&config).unwrap();
        assert_eq!(ProviderConfig::from_json(&text).unwrap(), config);
        // Messages API version 2026-10-21 rejects sampling parameters.
        let detail = rejected(|value| value["generation"] = json!({ "temperature": 0.2 }));
        assert!(detail.contains("temperature"), "{detail}");
        rejected(|value| value["generation"] = json!({ "seed": 7 }));
        rejected(|value| value["generation"] = json!({ "effort": "ultra" }));
        rejected(|value| value["generation"] = json!({ "reasoning_style": "enable_thinking" }));
        // Nothing set is not written out.
        let plain = with(|_| {}).unwrap();
        assert!(!serde_json::to_string(&plain).unwrap().contains("generation"));
    }
}
