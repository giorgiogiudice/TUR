// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The models a provider offers, from its own list.
//!
//! Every family's interface has a request that lists the models a key may
//! use. This module sends it, reads every page, and returns the names. It
//! is a request to the provider: it is made only when the person asks for
//! the list to be brought up to date, and the session service keeps the
//! answer so that it is not asked again for a day.
//!
//! | Family | Request | Key | Names in | Next page |
//! |---|---|---|---|---|
//! | Anthropic | `GET /v1/models?limit=1000` | `x-api-key`, with `anthropic-version` | `data[].id` (`display_name`) | `has_more`, then `after_id=<last_id>` |
//! | OpenAI | `GET /v1/models` | `Authorization: Bearer` | `data[].id` | none |
//! | Google | `GET /v1beta/models?pageSize=1000` | `x-goog-api-key` | `models[].name` without `models/` (`displayName`) | `nextPageToken`, then `pageToken` |
//! | OpenAI-compatible | `GET <base URL>/models` | `Authorization: Bearer`, or none | `data[].id`, or a bare list (Mistral's example) | none |
//!
//! What comes back is a name and, where the provider gives one, a label.
//! Nothing a provider says about a model's capabilities or prices is taken:
//! those stay what the model list declares. A name that is not a usable
//! model name (empty, too long, with control characters) is left out.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::config::{MODEL_NAME_MAX, ProviderConfig};
use crate::error::{ErrorKind, ProviderError};
use crate::http::{Fetch, Transport};
use crate::model::ProviderId;
use crate::secret::ApiKey;

/// The most pages read from one provider.
pub const PAGES_MAX: usize = 20;

/// The most models kept from one provider.
pub const MODELS_MAX: usize = 2000;

/// The longest label kept, in characters.
pub const LABEL_MAX: usize = 120;

/// One model a provider lists.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ListedModel {
    /// The name to send as `model`.
    pub model: String,
    /// The name for a person, where the provider gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

fn malformed(provider: ProviderId, detail: &str) -> ProviderError {
    ProviderError::new(ErrorKind::MalformedResponse)
        .with_detail(detail)
        .with_provider(provider)
}

/// Percent-encode a value for a query string.
fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn usable_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= MODEL_NAME_MAX
        && !name.chars().any(char::is_control)
        && name.trim() == name
}

fn label(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty() && !text.chars().any(char::is_control))
        .then(|| text.chars().take(LABEL_MAX).collect())
}

/// The models of one page: `items` is the array, `name` and `label` the
/// fields of an item, `prefix` what is taken off the front of a name.
fn read_items(
    items: &[Value],
    name: &str,
    label_field: &str,
    prefix: &str,
    out: &mut Vec<ListedModel>,
) {
    for item in items {
        let Some(model) = item.get(name).and_then(Value::as_str) else {
            continue;
        };
        let model = model.strip_prefix(prefix).unwrap_or(model);
        if usable_name(model) {
            out.push(ListedModel {
                model: model.to_owned(),
                label: label(item.get(label_field)),
            });
        }
    }
}

/// Ask the provider of a configuration which models it offers.
///
/// `key` is sent in the family's header; without one (a server on this
/// computer that takes none) no credential is sent, which only the
/// OpenAI-compatible family allows.
///
/// # Errors
///
/// The provider layer's errors: `TUR-AI-PROVIDER-011` for a configuration
/// that is not valid, `-012` for a missing key, the status classes of the
/// family for a refusal, `-007` for an answer that is not a list.
pub fn list_models(
    config: &ProviderConfig,
    key: Option<&ApiKey>,
) -> Result<Vec<ListedModel>, ProviderError> {
    config
        .validate()
        .map_err(|error| error.with_provider(config.provider))?;
    let provider = config.provider;
    if key.is_none() && provider != ProviderId::OpenAiChat {
        return Err(ProviderError::new(ErrorKind::KeyUnavailable)
            .with_detail("the list of models of this family needs a key")
            .with_provider(provider));
    }
    let transport = Transport::new(config);
    let base = config.effective_base_url();
    let (endpoint_auth, endpoint_headers, classify) = match provider {
        ProviderId::Anthropic => (
            crate::anthropic::ENDPOINT.auth,
            crate::anthropic::ENDPOINT.headers,
            crate::anthropic::ENDPOINT.classify,
        ),
        ProviderId::OpenAi => (
            crate::openai::ENDPOINT.auth,
            crate::openai::ENDPOINT.headers,
            crate::openai::ENDPOINT.classify,
        ),
        ProviderId::Google => (
            crate::google::ENDPOINT.auth,
            crate::google::ENDPOINT.headers,
            crate::google::ENDPOINT.classify,
        ),
        ProviderId::OpenAiChat => (
            crate::openai_chat::ENDPOINT.auth,
            crate::openai_chat::ENDPOINT.headers,
            crate::openai_chat::ENDPOINT.classify,
        ),
    };
    let fetch_page = |url: &str| -> Result<Value, ProviderError> {
        let text = transport.get_text(
            &Fetch {
                provider,
                url,
                auth: endpoint_auth,
                headers: endpoint_headers,
                classify,
            },
            key,
        )?;
        crate::strict::value_from_str(&text)
            .map_err(|_| malformed(provider, "the list of models is not one JSON document"))
    };

    let mut models: Vec<ListedModel> = Vec::new();
    match provider {
        ProviderId::Anthropic => {
            let mut after: Option<String> = None;
            for _ in 0..PAGES_MAX {
                let mut url = format!("{base}/v1/models?limit=1000");
                if let Some(after) = &after {
                    url.push_str(&format!("&after_id={}", encode(after)));
                }
                let page = fetch_page(&url)?;
                let items = page
                    .get("data")
                    .and_then(Value::as_array)
                    .ok_or_else(|| malformed(provider, "the list has no data array"))?;
                read_items(items, "id", "display_name", "", &mut models);
                let more = page.get("has_more").and_then(Value::as_bool) == Some(true);
                let last = page.get("last_id").and_then(Value::as_str);
                match (more, last) {
                    (true, Some(last)) if after.as_deref() != Some(last) => {
                        after = Some(last.to_owned());
                    }
                    _ => break,
                }
            }
        }
        ProviderId::Google => {
            let mut token: Option<String> = None;
            for _ in 0..PAGES_MAX {
                let mut url = format!("{base}/v1beta/models?pageSize=1000");
                if let Some(token) = &token {
                    url.push_str(&format!("&pageToken={}", encode(token)));
                }
                let page = fetch_page(&url)?;
                if let Some(items) = page.get("models").and_then(Value::as_array) {
                    read_items(items, "name", "displayName", "models/", &mut models);
                } else if page.get("nextPageToken").is_none() {
                    // An account with no model has an empty object; anything
                    // else without the list is not a list.
                    if page.as_object().is_none_or(|object| !object.is_empty()) {
                        return Err(malformed(provider, "the list has no models array"));
                    }
                }
                match page.get("nextPageToken").and_then(Value::as_str) {
                    Some(next) if !next.is_empty() && token.as_deref() != Some(next) => {
                        token = Some(next.to_owned());
                    }
                    _ => break,
                }
            }
        }
        ProviderId::OpenAi | ProviderId::OpenAiChat => {
            let url = if provider == ProviderId::OpenAi {
                format!("{base}/v1/models")
            } else {
                format!("{base}/models")
            };
            let page = fetch_page(&url)?;
            let items = page
                .get("data")
                .and_then(Value::as_array)
                .or_else(|| page.as_array())
                .ok_or_else(|| malformed(provider, "the list has no data array"))?;
            read_items(items, "id", "name", "", &mut models);
        }
    }
    let mut seen = BTreeSet::new();
    models.retain(|listed| seen.insert(listed.model.clone()));
    models.truncate(MODELS_MAX);
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(encode("abc-1_2.3~"), "abc-1_2.3~");
        assert_eq!(encode("a b/c&d=e"), "a%20b%2Fc%26d%3De");
    }

    #[test]
    fn items_without_a_usable_name_are_left_out() {
        let items: Vec<Value> = serde_json::from_str(
            r#"[{"name": "models/first", "displayName": "First"},
                {"name": ""}, {"name": "bad\nname"}, {"other": "x"},
                {"name": "models/second"}]"#,
        )
        .unwrap();
        let mut out = Vec::new();
        read_items(&items, "name", "displayName", "models/", &mut out);
        assert_eq!(
            out,
            vec![
                ListedModel {
                    model: "first".to_owned(),
                    label: Some("First".to_owned())
                },
                ListedModel {
                    model: "second".to_owned(),
                    label: None
                },
            ]
        );
    }
}
