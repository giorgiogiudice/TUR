// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The live engines P@x can talk through: the local cascade, and providers'
//! live models, as configuration (`schemas/pax-live-providers.schema.json`;
//! `docs/architecture/pax-voice.md`, section 5).
//!
//! An entry names the family that speaks the provider's live protocol
//! (`openai-realtime`, `gemini-live`), its address if it is not the
//! family's, the header its key goes in, the password-store item of the
//! key (the same as the provider's text models, so one key serves both),
//! its models with their audio prices, and its voices. No model name is
//! compiled in.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tur_pax_providers::{LiveFamily, LivePrices};

use crate::faults::{Fault, TaskCode};

/// The identifier of the live engine that runs on this computer.
pub const LOCAL_ENGINE: &str = "local";

/// The prices of one live model, per million tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListedPrices {
    /// The currency.
    pub currency: String,
    /// Audio the model hears.
    pub audio_input: f64,
    /// Audio the model speaks.
    pub audio_output: f64,
    /// Text in.
    pub text_input: f64,
    /// Text out.
    pub text_output: f64,
    /// Input read from the provider's cache, if it prices it apart.
    #[serde(default)]
    pub cached_input: Option<f64>,
}

impl ListedPrices {
    /// As the provider layer takes them.
    pub fn to_live(&self) -> LivePrices {
        LivePrices {
            currency: self.currency.clone(),
            audio_input_per_million: self.audio_input,
            audio_output_per_million: self.audio_output,
            text_input_per_million: self.text_input,
            text_output_per_million: self.text_output,
            cached_input_per_million: self.cached_input,
        }
    }
}

/// One live model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveModel {
    /// Its name, as the provider takes it.
    pub name: String,
    /// Its prices, or `None` if TUR does not know them.
    #[serde(default)]
    pub prices: Option<ListedPrices>,
}

/// One provider's live engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveEntry {
    /// Its identifier.
    pub id: String,
    /// Its name, as the person sees it.
    pub name: String,
    /// The company the person's voice is streamed to.
    pub company: String,
    /// The family that speaks its protocol.
    pub family: String,
    /// Its address, if not the family's own.
    #[serde(default)]
    pub base_url: Option<String>,
    /// The header the key goes in, if not the family's own.
    #[serde(default)]
    pub key_header: Option<String>,
    /// The password-store item of its key.
    pub keyring_item: String,
    /// The provider's terms, as TUR links them.
    #[serde(default)]
    pub terms_url: Option<String>,
    /// What TUR read of how the provider treats the audio, with the date.
    #[serde(default)]
    pub data_terms: Option<String>,
    /// Its models, the first being the default.
    pub models: Vec<LiveModel>,
    /// Its voices, the first being the default.
    #[serde(default)]
    pub voices: Vec<String>,
    /// The model that writes down what the person says, where the family
    /// asks for one by name.
    #[serde(default)]
    pub transcription_model: Option<String>,
    /// Whether TUR's adapter for it is built. An entry that is not is
    /// listed so that the person sees it exists, and cannot be chosen.
    #[serde(default = "built")]
    pub built: bool,
}

const fn built() -> bool {
    true
}

/// The list of live engines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveList {
    /// Always 1.
    pub schema_version: u32,
    /// `example` or `reviewed`.
    pub status: String,
    /// When the entries, and above all the prices, were read.
    pub as_of: String,
    /// A note for whoever reads the file, with its licence.
    #[serde(default)]
    pub note: Option<String>,
    /// The providers' live engines.
    pub providers: Vec<LiveEntry>,
}

fn invalid(detail: impl Into<String>) -> Fault {
    Fault::new(TaskCode::ServiceStorage, detail)
}

impl LiveList {
    /// No provider: the local engine only.
    pub fn empty() -> LiveList {
        LiveList {
            schema_version: 1,
            status: "none".to_owned(),
            as_of: String::new(),
            note: None,
            providers: Vec::new(),
        }
    }

    /// Read the list: the first of `paths` that exists. None existing is the
    /// empty list.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the file is there and not valid.
    pub fn load(paths: &[PathBuf]) -> Result<LiveList, Fault> {
        for path in paths {
            match std::fs::read_to_string(path) {
                Ok(text) => return LiveList::from_text(&text, path),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(invalid(format!("{}: {error}", path.display()))),
            }
        }
        Ok(LiveList::empty())
    }

    /// Read a list from its text.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` with what is wrong.
    pub fn from_text(text: &str, path: &Path) -> Result<LiveList, Fault> {
        let value = crate::strict::value_from_str(text)
            .map_err(|_| invalid(format!("{}: not one JSON object", path.display())))?;
        let list: LiveList = serde_json::from_value(value)
            .map_err(|error| invalid(format!("{}: {error}", path.display())))?;
        if list.schema_version != 1 {
            return Err(invalid(format!("{}: version is not 1", path.display())));
        }
        let mut seen = std::collections::BTreeSet::new();
        for entry in &list.providers {
            if !crate::settings::is_provider_id(&entry.id) || entry.id == LOCAL_ENGINE {
                return Err(invalid(format!("{}: an identifier is not valid", path.display())));
            }
            if !seen.insert(entry.id.clone()) {
                return Err(invalid(format!("{}: {} is listed twice", path.display(), entry.id)));
            }
            if entry.family.parse::<LiveFamily>().is_err()
                || entry.family == LiveFamily::Scripted.as_str()
            {
                return Err(invalid(format!("{}: {} has no known family", path.display(), entry.id)));
            }
            if entry.models.is_empty()
                || entry
                    .models
                    .iter()
                    .any(|model| !crate::settings::is_model_name(&model.name))
            {
                return Err(invalid(format!("{}: {} has no valid model", path.display(), entry.id)));
            }
            if let Some(url) = &entry.base_url
                && !(url.starts_with("wss://") || is_loopback_ws(url))
            {
                return Err(invalid(format!(
                    "{}: {}'s address is neither wss:// nor this computer",
                    path.display(),
                    entry.id
                )));
            }
        }
        Ok(list)
    }

    /// The entry with this identifier.
    pub fn entry(&self, id: &str) -> Option<&LiveEntry> {
        self.providers.iter().find(|entry| entry.id == id)
    }
}

/// `ws://` to an address of this computer: a model server here.
pub fn is_loopback_ws(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("ws://") else {
        return false;
    };
    let host = rest.split(['/', '?']).next().unwrap_or_default();
    let host = if let Some(bracketed) = host.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    host == "localhost"
        || host == "::1"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|address| address.is_loopback())
}

impl LiveEntry {
    /// The model to use: the one named, which must be listed, or the first.
    ///
    /// # Errors
    ///
    /// `TUR-AI-VOICE-009` if the name is not listed.
    pub fn model(&self, name: Option<&str>) -> Result<&LiveModel, Fault> {
        match name {
            None => self.models.first(),
            Some(name) => self.models.iter().find(|model| model.name == name),
        }
        .ok_or_else(|| {
            Fault::new(
                TaskCode::VoiceLiveUnavailable,
                "that live model is not in the list of live engines",
            )
        })
    }

    /// The estimate of a minute of conversation, from the listed prices.
    pub fn per_minute(&self, model: &LiveModel) -> Option<f64> {
        let family: LiveFamily = self.family.parse().ok()?;
        model
            .prices
            .as_ref()
            .map(|prices| prices.to_live().per_minute_estimate(family))
    }

    /// The entry as `GetVoiceStatus` lists it.
    pub fn to_json(&self, consent_current: bool, key: &str) -> Value {
        let model = self.models.first();
        json!({
            "id": self.id,
            "name": self.name,
            "company": self.company,
            "kind": "provider",
            "family": self.family,
            "transport": "websocket",
            "paid": true,
            "built": self.built,
            "models": self.models.iter().map(|model| model.name.clone()).collect::<Vec<_>>(),
            "voices": self.voices,
            "consent_current": consent_current,
            "consent_text": crate::voice::live_consent_text(&self.company, self.terms_url.as_deref()),
            "consent_version": crate::voice::VOICE_CONSENT_VERSION,
            "data_terms": self.data_terms,
            "key": key,
            "price_per_minute": model.and_then(|model| self.per_minute(model)),
            "currency": model.and_then(|model| model.prices.as_ref().map(|prices| prices.currency.clone())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{
        "schema_version": 1, "status": "example", "as_of": "2026-11-16",
        "providers": [{
            "id": "openai-realtime", "name": "OpenAI Realtime", "company": "OpenAI",
            "family": "openai-realtime", "keyring_item": "openai",
            "models": [{"name": "gpt-realtime-2.1", "prices": {"currency": "USD",
                "audio_input": 32, "audio_output": 64, "text_input": 4, "text_output": 24,
                "cached_input": 0.4}}],
            "voices": ["marin"]
        }, {
            "id": "llama-realtime", "name": "A model server on this computer", "company": "this computer",
            "family": "openai-realtime", "base_url": "ws://127.0.0.1:39281/v1/realtime",
            "keyring_item": "local", "models": [{"name": "lfm3-audio-2b"}], "built": true
        }]
    }"#;

    #[test]
    fn the_shipped_list_and_this_one_are_valid() {
        let list = LiveList::from_text(LIST, Path::new("test")).expect("valid");
        assert_eq!(list.providers.len(), 2);
        let entry = list.entry("openai-realtime").expect("listed");
        let model = entry.model(None).expect("the first");
        let minute = entry.per_minute(model).expect("priced");
        assert!(minute > 0.0);
        assert!(entry.model(Some("not-listed")).is_err());
        let shipped = include_str!("../data/live.json");
        let list = LiveList::from_text(shipped, Path::new("data/live.json")).expect("valid");
        assert!(list.providers.iter().any(|entry| entry.family == "openai-realtime"));
        assert!(list.providers.iter().any(|entry| entry.family == "gemini-live"));
        assert!(list.entry(LOCAL_ENGINE).is_none());
    }

    #[test]
    fn an_address_is_wss_or_this_computer() {
        assert!(is_loopback_ws("ws://127.0.0.1:39281/v1/realtime"));
        assert!(is_loopback_ws("ws://localhost:8080/x"));
        assert!(is_loopback_ws("ws://[::1]:8080/x"));
        assert!(!is_loopback_ws("ws://192.168.1.4:8080/x"));
        assert!(!is_loopback_ws("ws://example.org/x"));
        assert!(!is_loopback_ws("wss://127.0.0.1/x"));
        let bad = LIST.replace("ws://127.0.0.1:39281", "ws://10.0.0.2:39281");
        assert!(LiveList::from_text(&bad, Path::new("test")).is_err());
        let twice = LIST.replace("llama-realtime", "openai-realtime");
        assert!(LiveList::from_text(&twice, Path::new("test")).is_err());
        let local = LIST.replace("llama-realtime", "local");
        assert!(LiveList::from_text(&local, Path::new("test")).is_err());
        let scripted = LIST.replace(r#""family": "openai-realtime", "keyring_item": "openai""#,
            r#""family": "scripted", "keyring_item": "openai""#);
        assert!(LiveList::from_text(&scripted, Path::new("test")).is_err());
    }
}
