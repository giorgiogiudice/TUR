// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What the session service keeps about P@x itself: whether it is switched
//! on, which consent was given, which provider and model are in use, and the
//! spending limits. And the model list: the names and prices the service
//! offers, which is configuration and never compiled in.
//!
//! Two files:
//!
//! * the **settings**, `$XDG_CONFIG_HOME/tur/pax/settings.json`
//!   (`schemas/pax-settings.schema.json`), written only by the service, on
//!   the person's request;
//! * the **model list**, `/usr/share/tur/pax/models.json`, or
//!   `/etc/tur/pax/models.json` if the administrator put one there
//!   (`schemas/pax-models.schema.json`), read and never written. The file in
//!   this repository is an example with the date its prices were read.
//!
//! Neither file ever holds a key. The settings format has no field for one
//! and rejects fields it does not know.
//!
//! A settings file that cannot be read is not repaired or guessed at: the
//! service treats P@x as switched off and says why. That is the safe
//! reading of "off until switched on".
//!
//! # Who may change the settings
//!
//! The person. The design says more: loosening a setting, such as raising a
//! spending limit, is confirmed on the trusted screen and the file belongs
//! to a separate component (addendum A section 17, item 5). Neither exists
//! yet. What holds today is weaker and is stated in
//! `docs/architecture/pax-service.md`: nothing the model can do reaches
//! these functions, and any program running as the person can.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tur_pax_memory::default_store_dir;
use tur_pax_providers::{Prices, ProviderConfig, ProviderId, RetryPolicy, TaskLimit, Timeouts};

use crate::consent::{CONSENT_TEXT_VERSION, family_name};
use crate::faults::{Fault, TaskCode};

/// The settings schema version this build reads and writes.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

/// The model list schema version this build reads.
pub const MODELS_SCHEMA_VERSION: u32 = 1;

/// The output limit of one response when neither the settings nor the model
/// list gives one.
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 4096;

/// The limit of one task, in the currency of the prices, when the person set
/// none. The figure is the one the setup flow of the desktop design shows.
pub const DEFAULT_TASK_COST: f64 = 1.0;

/// The limit of one day, in the currency of the prices, when the person set
/// none. The figure is the one the setup flow of the desktop design shows.
pub const DEFAULT_DAY_COST: f64 = 5.0;

/// The limit of one task in tokens, for a model whose price is not known,
/// when the person set none.
pub const DEFAULT_TASK_TOKENS: u64 = 400_000;

/// The limit of one day in tokens, for a model whose price is not known,
/// when the person set none.
pub const DEFAULT_DAY_TOKENS: u64 = 2_000_000;

/// The largest amount a limit may be set to, in currency units.
pub const MAX_COST_LIMIT: f64 = 100_000.0;

/// The largest limit in tokens.
pub const MAX_TOKEN_LIMIT: u64 = 100_000_000_000;

/// The longest model name, in characters. The same as the provider layer's.
const MODEL_NAME_MAX: usize = 200;

/// The acceptance of the consent text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentRecord {
    /// The version of the text that was accepted.
    pub text_version: String,
    /// The identifier of the provider the text named.
    pub provider: String,
    /// When it was accepted, in seconds since the Unix epoch.
    pub accepted_at: u64,
}

/// The spending limits the person set. A limit that is absent has its
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The most one task may cost, in the currency of the prices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_cost: Option<f64>,
    /// The most one day may cost, in the currency of the prices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_cost: Option<f64>,
    /// The most tokens one task may use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_tokens: Option<u64>,
    /// The most tokens one day may use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_tokens: Option<u64>,
}

impl Limits {
    /// Check the limits: each amount finite, above zero and within range.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006`, naming the limit and never a value.
    pub fn validate(&self) -> Result<(), Fault> {
        for (name, amount) in [("task_cost", self.task_cost), ("day_cost", self.day_cost)] {
            if let Some(amount) = amount
                && !(amount.is_finite() && amount > 0.0 && amount <= MAX_COST_LIMIT)
            {
                return Err(invalid(format!(
                    "the limit {name} must be above 0 and at most {MAX_COST_LIMIT}"
                )));
            }
        }
        for (name, tokens) in [
            ("task_tokens", self.task_tokens),
            ("day_tokens", self.day_tokens),
        ] {
            if let Some(tokens) = tokens
                && !(1..=MAX_TOKEN_LIMIT).contains(&tokens)
            {
                return Err(invalid(format!(
                    "the limit {name} must be at least 1 and at most {MAX_TOKEN_LIMIT}"
                )));
            }
        }
        Ok(())
    }
}

/// The limit of one day as it is enforced: the person's, or the default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayLimit {
    /// The most the day may cost. `None` if the price of the model is not
    /// known, because then nothing can be priced.
    pub cost: Option<f64>,
    /// The most tokens the day may use, if that is limited.
    pub tokens: Option<u64>,
}

/// What a provider entry declares the service can do. Recorded and
/// reported; whether it is true is the adapter's to find out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredCapabilities {
    /// Tool calling: without it a model can converse and not act.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calling: Option<bool>,
    /// Tool calls streamed in fragments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming_tool_calls: Option<bool>,
    /// Image input: without it a model cannot control applications by sight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_input: Option<bool>,
    /// The context size, in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
}

impl DeclaredCapabilities {
    /// These declarations, with what `over` declares in their place: a
    /// model's own declarations over its provider's.
    #[must_use]
    pub fn overlaid(self, over: Option<DeclaredCapabilities>) -> DeclaredCapabilities {
        let Some(over) = over else {
            return self;
        };
        DeclaredCapabilities {
            tool_calling: over.tool_calling.or(self.tool_calling),
            streaming_tool_calls: over.streaming_tool_calls.or(self.streaming_tool_calls),
            image_input: over.image_input.or(self.image_input),
            context_tokens: over.context_tokens.or(self.context_tokens),
        }
    }
}

/// What P@x may attempt with a model, from what its entry declares
/// (ADR-0004, amended 12 November 2026): a model without tool calling can
/// converse and not act; a model without image input cannot control
/// applications by sight. `None` where nothing is declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abilities {
    /// Whether P@x may offer it tools.
    pub act: Option<bool>,
    /// Whether P@x may offer it screen control.
    pub see: Option<bool>,
}

impl Abilities {
    /// What the person is told, in plain words, about what P@x will not
    /// attempt with the model. Empty when nothing is withheld.
    pub fn notices(&self) -> Vec<&'static str> {
        let mut said = Vec::new();
        if self.act == Some(false) {
            said.push(MODEL_CANNOT_ACT);
        }
        if self.see == Some(false) {
            said.push(MODEL_CANNOT_SEE);
        }
        said
    }
}

/// What the person is told when the model in use cannot call tools.
pub const MODEL_CANNOT_ACT: &str = "This model cannot call tools, as its provider's entry \
     declares, so P@x can talk with it but cannot act with it: no tool or command is offered to it.";

/// What the person is told when the model in use takes no images.
pub const MODEL_CANNOT_SEE: &str = "This model takes no images, as its provider's entry \
     declares, so P@x will not offer it control of the screen.";

/// What a provider says about the data sent to it, as TUR read it.
///
/// The statements are TUR's summary in plain words of the provider's own
/// pages, read on `reviewed_on` from `sources`. Without `reviewed_on`
/// nothing has been reviewed, and the consent text says "not reviewed by
/// TUR". A provider the person adds by hand is never shown as reviewed,
/// whatever its entry says.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataTerms {
    /// The day TUR read the provider's terms, as `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_on: Option<String>,
    /// The model runs on this computer: nothing is sent to a company. Only
    /// true of an entry whose address is this computer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub runs_locally: bool,
    /// Whether the provider keeps what it is sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stores_requests: Option<String>,
    /// For how long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<String>,
    /// Whether it trains models on what it is sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trains_on_data: Option<String>,
    /// Where it processes and keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing_location: Option<String>,
    /// Where its terms are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms_url: Option<String>,
    /// The pages the statements were read from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
}

/// The longest statement of [`DataTerms`], in characters.
pub const TERMS_STATEMENT_MAX: usize = 400;

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_true(value: &bool) -> bool {
    *value
}

fn yes() -> bool {
    true
}

/// Whether `text` is written `YYYY-MM-DD`.
fn is_date(text: &str) -> bool {
    let date = text.as_bytes();
    date.len() == 10
        && date.iter().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

impl DataTerms {
    /// Check the statements. The reason quotes nothing.
    fn check(&self) -> Result<(), &'static str> {
        if self
            .reviewed_on
            .as_deref()
            .is_some_and(|date| !is_date(date))
        {
            return Err("the date its terms were read is not written YYYY-MM-DD");
        }
        for statement in [
            &self.stores_requests,
            &self.retention,
            &self.trains_on_data,
            &self.processing_location,
        ]
        .into_iter()
        .flatten()
        {
            let fine = !statement.trim().is_empty()
                && statement.chars().count() <= TERMS_STATEMENT_MAX
                && !statement.chars().any(char::is_control);
            if !fine {
                return Err("a statement of its data terms is empty, too long or not plain text");
            }
        }
        for url in self.terms_url.iter().chain(&self.sources) {
            if !is_https_url(url) {
                return Err("an address of its data terms is not an https address");
            }
        }
        if self.reviewed_on.is_some() && self.sources.is_empty() && !self.runs_locally {
            return Err("reviewed data terms name no page they were read from");
        }
        Ok(())
    }
}

/// Whether an address is on this computer: `http` or `https` to
/// `localhost` or a loopback address.
pub fn is_loopback_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https") {
        return false;
    }
    let authority = rest.split('/').next().unwrap_or("");
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// One model of a provider entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEntry {
    /// The model name, as the provider spells it.
    pub model: String,
    /// What this model declares it can do, in place of what its provider
    /// declares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<DeclaredCapabilities>,
    /// A name for a person to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The output limit of one response to use with this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Whether the project has run its conformance tests against this model.
    #[serde(default)]
    pub tested_with_tur: bool,
    /// The prices, if they are known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prices: Option<Prices>,
}

/// One provider: a service P@x can talk to, by an identifier of its own.
///
/// A provider is configuration (ADR-0004, amendment of 12 November 2026):
/// which adapter family speaks its interface, where it is, which item of
/// the password store holds its key, what it declares it can do, and its
/// models and prices. The three companies the adapter families are named
/// after are providers like any other; when no list names them, they exist
/// with their family's public address and no prices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEntry {
    /// The identifier: 1 to 32 lower-case letters, digits, `-` and `_`,
    /// starting with a letter.
    pub id: String,
    /// The adapter family that speaks its interface.
    pub adapter: ProviderId,
    /// The provider's name, for a person to read.
    pub name: String,
    /// Its address, if it is not the public address of its adapter family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The name of the password store item that holds its key; the
    /// identifier when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyring_item: Option<String>,
    /// Whether it needs a key. Only a service of the OpenAI-compatible
    /// family, such as a model server on this computer, can do without one.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub key_required: bool,
    /// Whether the model's reasoning is sent back with later requests,
    /// because the service's documentation requires it (OpenAI-compatible
    /// family only; DeepSeek).
    #[serde(default, skip_serializing_if = "is_false")]
    pub send_back_reasoning: bool,
    /// What it declares its models can do, unless a model declares otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<DeclaredCapabilities>,
    /// What it says about the data sent to it, as TUR read it, or nothing
    /// when TUR has not reviewed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_terms: Option<DataTerms>,
    /// Where the provider says how it handles data sent to its interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_terms_url: Option<String>,
    /// Where a person gets a key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_url: Option<String>,
    /// Where the prices below were read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prices_url: Option<String>,
    /// The models.
    #[serde(default)]
    pub models: Vec<ModelEntry>,
}

/// Where a provider entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The model list, shipped or put in place by the administrator.
    Listed,
    /// The person's own entry, in the settings file.
    ByHand,
    /// No entry: one of the adapter families' own companies, at its public
    /// address.
    Implicit,
}

impl Origin {
    /// The origin as the interface writes it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Origin::Listed => "listed",
            Origin::ByHand => "by_hand",
            Origin::Implicit => "implicit",
        }
    }
}

/// Whether `id` can be the identifier of a provider: 1 to 32 lower-case
/// ASCII letters, digits, `-` and `_`, starting with a letter. Every
/// identifier is also a valid password store item name
/// ([`is_keyring_item`]), so a provider's key can always be kept under its
/// identifier.
pub fn is_provider_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id.as_bytes()[0].is_ascii_lowercase()
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

/// The longest password store item name, in bytes.
pub const KEYRING_ITEM_MAX: usize = 64;

/// Whether `name` can name the password store item of a provider's key: 1
/// to 64 lower-case ASCII letters, digits, `-`, `_` and `.`, starting with a
/// letter or a digit, with no two dots in a row. A safe set: it goes into a
/// Secret Service attribute and a label, and is never a path. Every provider
/// identifier is one.
pub fn is_keyring_item(name: &str) -> bool {
    (1..=KEYRING_ITEM_MAX).contains(&name.len())
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && !name.contains("..")
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

impl ProviderEntry {
    /// The entry of an adapter family's own company, when no list names it.
    pub fn implicit(adapter: ProviderId) -> Self {
        ProviderEntry {
            id: adapter.as_str().to_owned(),
            adapter,
            name: family_name(adapter).to_owned(),
            base_url: None,
            keyring_item: None,
            key_required: true,
            send_back_reasoning: false,
            capabilities: None,
            data_terms: None,
            data_terms_url: None,
            key_url: None,
            prices_url: None,
            models: Vec::new(),
        }
    }

    /// What a model of this provider declares it can do: the model's own
    /// declarations over the provider's.
    pub fn declared(&self, model: &str) -> DeclaredCapabilities {
        self.capabilities
            .unwrap_or_default()
            .overlaid(self.model(model).and_then(|entry| entry.capabilities))
    }

    /// What P@x may attempt with a model of this provider.
    pub fn abilities(&self, model: &str) -> Abilities {
        let declared = self.declared(model);
        Abilities {
            act: declared.tool_calling,
            see: declared.image_input,
        }
    }

    /// The data terms TUR reviewed, if the entry is TUR's (not added by
    /// hand) and the address P@x uses is the entry's own: a review says
    /// nothing about another address.
    pub fn reviewed_terms(&self, origin: Origin, address_changed: bool) -> Option<&DataTerms> {
        if origin == Origin::ByHand || address_changed {
            return None;
        }
        self.data_terms
            .as_ref()
            .filter(|terms| terms.reviewed_on.is_some() || terms.runs_locally)
    }

    /// Whether what P@x sends this provider stays on this computer: its
    /// entry says the model runs locally and the address is this computer.
    pub fn runs_locally(&self, origin: Origin, address: Option<&str>) -> bool {
        let address = address.or(self.base_url.as_deref());
        origin != Origin::ByHand
            && self
                .data_terms
                .as_ref()
                .is_some_and(|terms| terms.runs_locally)
            && address.is_some_and(is_loopback_url)
    }

    /// The name of the password store item that holds the key.
    pub fn keyring_item(&self) -> &str {
        self.keyring_item.as_deref().unwrap_or(&self.id)
    }

    /// Whether this is the adapter family's own company at its public
    /// address, the interface the adapter was written for.
    pub fn is_native(&self) -> bool {
        self.base_url.is_none() && self.id == self.adapter.as_str()
    }

    /// The model of this provider with that name.
    pub fn model(&self, model: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|entry| entry.model == model)
    }

    /// A configuration of the provider layer for this provider: what its own
    /// rules accept is what an entry may say.
    fn probe(&self, model: &str, prices: Option<Prices>) -> ProviderConfig {
        ProviderConfig {
            schema_version: tur_pax_providers::config::CONFIG_SCHEMA_VERSION,
            provider: self.adapter,
            base_url: self.base_url.clone(),
            send_back_reasoning: self.send_back_reasoning,
            model: model.to_owned(),
            max_output_tokens: 1,
            timeouts: Timeouts::default(),
            retry: RetryPolicy::default(),
            prices,
            task_limit: None,
        }
    }

    /// Check the entry. The reason names what is wrong and quotes nothing.
    fn check(&self) -> Result<(), String> {
        if !is_provider_id(&self.id) {
            return Err("the identifier of a provider is not valid".to_owned());
        }
        if self
            .keyring_item
            .as_deref()
            .is_some_and(|item| !is_keyring_item(item))
        {
            return Err(format!("the keyring item of {} is not valid", self.id));
        }
        if !self.key_required && self.adapter != ProviderId::OpenAiChat {
            return Err(format!(
                "{} cannot do without a key: only the openai-chat family can",
                self.id
            ));
        }
        if let Some(terms) = &self.data_terms {
            terms
                .check()
                .map_err(|reason| format!("{}: {reason}", self.id))?;
            if terms.runs_locally && !self.base_url.as_deref().is_some_and(is_loopback_url) {
                return Err(format!(
                    "{} is said to run on this computer, and its address is not this computer",
                    self.id
                ));
            }
        }
        let contexts = self.capabilities.iter().chain(
            self.models
                .iter()
                .filter_map(|model| model.capabilities.as_ref()),
        );
        for declared in contexts {
            if declared.context_tokens == Some(0) {
                return Err(format!("a context size of {} is zero", self.id));
            }
        }
        let name_ok = !self.name.trim().is_empty()
            && self.name.len() <= 80
            && !self.name.chars().any(char::is_control);
        if !name_ok {
            return Err(format!("the name of {} is not valid", self.id));
        }
        for url in [&self.data_terms_url, &self.key_url, &self.prices_url]
            .into_iter()
            .flatten()
        {
            if !is_https_url(url) {
                return Err(format!("an address of {} is not an https address", self.id));
            }
        }
        // The address, by the provider layer's rules: https, or http for a
        // loopback host only.
        if self.probe("probe", None).validate().is_err() {
            return Err(format!("the address of {} is not valid", self.id));
        }
        let mut names: Vec<&str> = Vec::new();
        for model in &self.models {
            if !is_model_name(&model.model) || names.contains(&model.model.as_str()) {
                return Err(format!(
                    "a model name of {} is not valid or is there twice",
                    self.id
                ));
            }
            names.push(&model.model);
            if model.max_output_tokens == Some(0) {
                return Err(format!("an output limit of {} is zero", self.id));
            }
            if let Some(prices) = &model.prices
                && self
                    .probe(&model.model, Some(prices.clone()))
                    .validate()
                    .is_err()
            {
                return Err(format!(
                    "the prices of a model of {} are not valid",
                    self.id
                ));
            }
        }
        Ok(())
    }
}

/// The settings of P@x.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The schema version. Must be [`SETTINGS_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Whether the person switched P@x on.
    #[serde(default)]
    pub enabled: bool,
    /// The acceptance of the consent text, if it was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<ConsentRecord>,
    /// The identifier of the provider in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// The model name, as the provider spells it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// An address that replaces the provider's, for a gateway. It cannot be
    /// set through the service's interface, only by editing the file,
    /// because whoever it names is sent the key. It is cleared when the
    /// provider changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The output limit of one response, if the person set one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// The spending limits.
    #[serde(default)]
    pub limits: Limits,
    /// Providers the person added by hand. An identifier the model list also
    /// has is the list's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderEntry>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema_version: SETTINGS_SCHEMA_VERSION,
            enabled: false,
            consent: None,
            provider: None,
            model: None,
            base_url: None,
            max_output_tokens: None,
            limits: Limits::default(),
            providers: Vec::new(),
        }
    }
}

fn invalid(detail: impl Into<String>) -> Fault {
    Fault::new(TaskCode::ServiceInvalidRequest, detail)
}

fn storage(detail: impl Into<String>) -> Fault {
    Fault::new(TaskCode::ServiceStorage, detail)
}

fn not_configured() -> Fault {
    Fault::new(
        TaskCode::ServiceNotConfigured,
        TaskCode::ServiceNotConfigured.user_message(),
    )
}

/// Whether `name` can be a model name: 1 to 200 characters, no control
/// character, no surrounding whitespace.
pub fn is_model_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= MODEL_NAME_MAX
        && !name.chars().any(char::is_control)
        && name.trim() == name
}

/// The adapter families that are one company's own interface, with a public
/// address of their own. Each of them is a provider even when no list names
/// it. A family for a kind of interface many services offer (the
/// OpenAI-compatible Chat Completions of ADR-0004's amendment) has no such
/// company and does not belong here.
pub const COMPANY_FAMILIES: [ProviderId; 3] = [
    ProviderId::Anthropic,
    ProviderId::OpenAi,
    ProviderId::Google,
];

/// Every provider there is, by identifier: the model list's, then the
/// person's own, then the adapter families' companies no list names.
pub fn providers(models: &ModelList, settings: &Settings) -> Vec<(ProviderEntry, Origin)> {
    let mut all: Vec<(ProviderEntry, Origin)> = models
        .providers
        .iter()
        .map(|entry| (entry.clone(), Origin::Listed))
        .collect();
    for entry in &settings.providers {
        if !all.iter().any(|(known, _)| known.id == entry.id) {
            all.push((entry.clone(), Origin::ByHand));
        }
    }
    for adapter in COMPANY_FAMILIES {
        if !all.iter().any(|(known, _)| known.id == adapter.as_str()) {
            all.push((ProviderEntry::implicit(adapter), Origin::Implicit));
        }
    }
    all
}

/// The provider with that identifier, and where its entry came from.
pub fn find_provider(
    models: &ModelList,
    settings: &Settings,
    id: &str,
) -> Option<(ProviderEntry, Origin)> {
    providers(models, settings)
        .into_iter()
        .find(|(entry, _)| entry.id == id)
}

impl Settings {
    /// Read settings from their JSON text.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the text is not settings of this version, or
    /// a value in it is not valid. The detail never quotes the file.
    pub fn from_json(text: &str) -> Result<Self, Fault> {
        let settings: Settings = serde_json::from_str(text).map_err(|error| {
            storage(format!(
                "the settings are not valid, at line {} column {}",
                error.line(),
                error.column()
            ))
        })?;
        if settings.schema_version != SETTINGS_SCHEMA_VERSION {
            return Err(storage(
                "the settings are of a version this build does not read",
            ));
        }
        if settings
            .model
            .as_deref()
            .is_some_and(|model| !is_model_name(model))
        {
            return Err(storage("the model name in the settings is not valid"));
        }
        if settings
            .provider
            .as_deref()
            .is_some_and(|provider| !is_provider_id(provider))
        {
            return Err(storage("the provider in the settings is not an identifier"));
        }
        if settings.max_output_tokens == Some(0) {
            return Err(storage("the output limit in the settings is zero"));
        }
        let mut seen: Vec<&str> = Vec::new();
        for entry in &settings.providers {
            entry
                .check()
                .map_err(|reason| storage(format!("a provider in the settings: {reason}")))?;
            if seen.contains(&entry.id.as_str()) {
                return Err(storage("a provider is in the settings twice"));
            }
            seen.push(&entry.id);
        }
        settings
            .limits
            .validate()
            .map_err(|fault| storage(fault.detail))?;
        Ok(settings)
    }

    /// Load the settings file. A file that does not exist is the settings
    /// of a system on which P@x was never switched on.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the file exists and cannot be read or used.
    pub fn load(path: &Path) -> Result<Self, Fault> {
        match fs::read_to_string(path) {
            Ok(text) => Settings::from_json(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(storage(format!("{}: {error}", path.display()))),
        }
    }

    /// Write the settings file: a new file beside it, flushed, then renamed
    /// over it, so that the file is always whole. The file is the person's
    /// alone.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if it cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), Fault> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|_| storage("the settings could not be written out"))?;
        write_private(path, text.as_bytes())
    }

    /// Whether the consent that was given is for the current text and the
    /// provider in use.
    pub fn consent_is_current(&self) -> bool {
        match (&self.consent, &self.provider) {
            (Some(consent), Some(provider)) => {
                consent.text_version == CONSENT_TEXT_VERSION && consent.provider == *provider
            }
            _ => false,
        }
    }

    /// Why a task cannot start, if it cannot: P@x is off, the consent is
    /// not current, or no provider and model are chosen.
    pub fn refusal(&self) -> Option<Fault> {
        if !self.enabled {
            return Some(Fault::new(
                TaskCode::ServiceDisabled,
                TaskCode::ServiceDisabled.user_message(),
            ));
        }
        if self.provider.is_none() || self.model.is_none() {
            return Some(not_configured());
        }
        if !self.consent_is_current() {
            return Some(Fault::new(
                TaskCode::ServiceConsentRequired,
                TaskCode::ServiceConsentRequired.user_message(),
            ));
        }
        None
    }

    /// Choose the provider, by its identifier, and the model.
    ///
    /// Changing the provider withdraws what was agreed for the other one:
    /// the consent names a provider, so P@x is switched off until the text
    /// for the new provider is accepted. The gateway address goes too, so
    /// that one provider's key is never sent to another's address.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-006` if no provider has that identifier or the model
    /// name is not valid.
    pub fn set_provider(
        &mut self,
        provider: &str,
        model: &str,
        models: &ModelList,
    ) -> Result<(), Fault> {
        if find_provider(models, self, provider).is_none() {
            return Err(invalid("no provider has that identifier"));
        }
        if !is_model_name(model) {
            return Err(invalid(
                "the model name must be 1 to 200 characters with no control characters and no \
                 surrounding whitespace",
            ));
        }
        if self.provider.as_deref() != Some(provider) {
            self.enabled = false;
            self.consent = None;
            self.base_url = None;
        }
        self.provider = Some(provider.to_owned());
        self.model = Some(model.to_owned());
        Ok(())
    }

    /// Switch P@x on. The person has read the consent text of the version
    /// they name; it must be the current one.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-003` if no provider and model are chosen;
    /// `TUR-AI-SERVICE-002` if the version named is not the current one.
    pub fn enable(&mut self, consent_version: &str, now: u64) -> Result<(), Fault> {
        let (Some(provider), Some(_)) = (&self.provider, &self.model) else {
            return Err(not_configured());
        };
        if consent_version != CONSENT_TEXT_VERSION {
            return Err(Fault::new(
                TaskCode::ServiceConsentRequired,
                "the version named is not the version of the text in use",
            ));
        }
        self.consent = Some(ConsentRecord {
            text_version: CONSENT_TEXT_VERSION.to_owned(),
            provider: provider.clone(),
            accepted_at: now,
        });
        self.enabled = true;
        Ok(())
    }

    /// Switch P@x off. The consent record stays: it says what was agreed
    /// and when.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// The provider in use, its entry and where the entry came from.
    pub fn current_provider(&self, models: &ModelList) -> Option<(ProviderEntry, Origin)> {
        find_provider(models, self, self.provider.as_deref()?)
    }

    /// The provider configuration of a task: the adapter family and address
    /// of the provider in use, its model, the model's prices, the limit of
    /// one task.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-003` if no provider and model are chosen, or the
    /// provider is no longer known; the provider layer's
    /// `TUR-AI-PROVIDER-011` if the values do not make a valid configuration.
    pub fn provider_config(&self, models: &ModelList) -> Result<ProviderConfig, Fault> {
        let Some(model) = &self.model else {
            return Err(not_configured());
        };
        let Some((entry, _)) = self.current_provider(models) else {
            return Err(not_configured());
        };
        let listed = entry.model(model);
        let prices = listed.and_then(|entry| entry.prices.clone());
        let task_limit = if prices.is_some() {
            TaskLimit {
                max_cost: Some(self.limits.task_cost.unwrap_or(DEFAULT_TASK_COST)),
                max_tokens: self.limits.task_tokens,
            }
        } else {
            TaskLimit {
                max_cost: None,
                max_tokens: Some(self.limits.task_tokens.unwrap_or(DEFAULT_TASK_TOKENS)),
            }
        };
        let config = ProviderConfig {
            schema_version: tur_pax_providers::config::CONFIG_SCHEMA_VERSION,
            provider: entry.adapter,
            base_url: self.base_url.clone().or_else(|| entry.base_url.clone()),
            send_back_reasoning: entry.send_back_reasoning,
            model: model.clone(),
            max_output_tokens: self
                .max_output_tokens
                .or(listed.and_then(|entry| entry.max_output_tokens))
                .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS),
            timeouts: Timeouts::default(),
            retry: RetryPolicy::default(),
            prices,
            task_limit: Some(task_limit),
        };
        config.validate().map_err(|error| Fault {
            code: error.kind().error_code(),
            detail: error.to_string(),
        })?;
        Ok(config)
    }

    /// The limit of one day, given whether the price of the model is known.
    pub fn day_limit(&self, price_known: bool) -> DayLimit {
        if price_known {
            DayLimit {
                cost: Some(self.limits.day_cost.unwrap_or(DEFAULT_DAY_COST)),
                tokens: self.limits.day_tokens,
            }
        } else {
            DayLimit {
                cost: None,
                tokens: Some(self.limits.day_tokens.unwrap_or(DEFAULT_DAY_TOKENS)),
            }
        }
    }
}

/// Write a file that only its owner can read, whole or not at all.
///
/// # Errors
///
/// `TUR-AI-SERVICE-009` if the directory or the file cannot be written.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Fault> {
    let failed = |error: std::io::Error| storage(format!("{}: {error}", path.display()));
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| storage(format!("{}: no directory", path.display())))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(failed)?;
    let name = path
        .file_name()
        .ok_or_else(|| storage(format!("{}: no file name", path.display())))?;
    let temporary = directory.join(format!(".{}.tmp", name.to_string_lossy()));
    // Whatever is at the temporary name is not ours to write through.
    let _ = fs::remove_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(failed)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(failed)?;
    drop(file);
    fs::rename(&temporary, path).map_err(failed)?;
    // The rename is durable once the directory is.
    if let Ok(handle) = fs::File::open(directory) {
        let _ = handle.sync_all();
    }
    Ok(())
}

/// The model list: the providers, their models and what they cost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelList {
    /// The schema version. Must be [`MODELS_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// `example` for a list that shows the format with figures read on one
    /// day; `maintained` for a list its publisher keeps up to date.
    pub status: String,
    /// The day the prices were read, as `YYYY-MM-DD`.
    pub prices_as_of: String,
    /// A note for whoever reads the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The providers.
    pub providers: Vec<ProviderEntry>,
}

fn is_https_url(text: &str) -> bool {
    text.len() <= 500
        && text.starts_with("https://")
        && text.len() > "https://".len()
        && text.bytes().all(|byte| byte.is_ascii_graphic())
}

impl ModelList {
    /// A list with no provider: every model is one whose price is not known.
    pub fn empty() -> Self {
        ModelList {
            schema_version: MODELS_SCHEMA_VERSION,
            status: "example".to_owned(),
            prices_as_of: "1970-01-01".to_owned(),
            note: None,
            providers: Vec::new(),
        }
    }

    /// Read a model list from its JSON text.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the text is not a model list of this version
    /// or a value in it is not valid.
    pub fn from_json(text: &str) -> Result<Self, Fault> {
        let list: ModelList = serde_json::from_str(text).map_err(|error| {
            storage(format!(
                "the model list is not valid, at line {} column {}: {error}",
                error.line(),
                error.column()
            ))
        })?;
        if list.schema_version != MODELS_SCHEMA_VERSION {
            return Err(storage(
                "the model list is of a version this build does not read",
            ));
        }
        if !matches!(list.status.as_str(), "example" | "maintained") {
            return Err(storage("the status of the model list is not known"));
        }
        let date = list.prices_as_of.as_bytes();
        let dated = date.len() == 10
            && date.iter().enumerate().all(|(index, byte)| {
                if index == 4 || index == 7 {
                    *byte == b'-'
                } else {
                    byte.is_ascii_digit()
                }
            });
        if !dated {
            return Err(storage("the date of the prices is not written YYYY-MM-DD"));
        }
        let mut seen: Vec<&str> = Vec::new();
        for entry in &list.providers {
            entry
                .check()
                .map_err(|reason| storage(format!("the model list: {reason}")))?;
            if seen.contains(&entry.id.as_str()) {
                return Err(storage("a provider is in the model list twice"));
            }
            seen.push(&entry.id);
        }
        Ok(list)
    }

    /// Load the first model list that exists among `paths`. If none exists
    /// the list is empty: no model has a known price.
    ///
    /// # Errors
    ///
    /// `TUR-AI-SERVICE-009` if the first file that exists cannot be used.
    pub fn load(paths: &[PathBuf]) -> Result<Self, Fault> {
        for path in paths {
            match fs::read_to_string(path) {
                Ok(text) => {
                    return ModelList::from_json(&text)
                        .map_err(|fault| storage(format!("{}: {}", path.display(), fault.detail)));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(storage(format!("{}: {error}", path.display()))),
            }
        }
        Ok(ModelList::empty())
    }

    /// The entry of a provider the list names.
    pub fn provider(&self, id: &str) -> Option<&ProviderEntry> {
        self.providers.iter().find(|entry| entry.id == id)
    }

    /// The entry of a model of a provider the list names.
    pub fn find(&self, id: &str, model: &str) -> Option<&ModelEntry> {
        self.provider(id)?.model(model)
    }
}

/// Where P@x's files are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The settings file.
    pub settings: PathBuf,
    /// The user policy file. It is read if it exists.
    pub policy: PathBuf,
    /// The spending record.
    pub spending: PathBuf,
    /// The audit trail.
    pub audit: PathBuf,
    /// The store of memory and schedules.
    pub store: PathBuf,
    /// Where a model list is looked for, in order.
    pub models: Vec<PathBuf>,
    /// The directory of tool sets.
    pub tools: PathBuf,
    /// A further directory of tool manifests, for tests.
    pub extra_manifests: Option<PathBuf>,
    /// A further directory of tool adapters, for tests.
    pub extra_adapters: Option<PathBuf>,
    /// The home directory, for the rules on what a command may be shown.
    pub home: Option<PathBuf>,
}

/// The model list an administrator may put in place.
pub const ADMIN_MODELS: &str = "/etc/tur/pax/models.json";

/// The model list shipped with the system.
pub const SHIPPED_MODELS: &str = "/usr/share/tur/pax/models.json";

fn absolute(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

impl Paths {
    /// The paths for the given base directories, as the XDG Base Directory
    /// specification places them. `None` if neither a base directory nor a
    /// home directory is known for one of them.
    pub fn from_bases(
        config_home: Option<PathBuf>,
        state_home: Option<PathBuf>,
        data_home: Option<PathBuf>,
        home: Option<PathBuf>,
    ) -> Option<Paths> {
        let config = config_home.or_else(|| home.as_ref().map(|home| home.join(".config")))?;
        let state =
            state_home.or_else(|| home.as_ref().map(|home| home.join(".local").join("state")))?;
        let store = default_store_dir(data_home.as_deref(), home.as_deref())?;
        let config = config.join("tur").join("pax");
        let state = state.join("tur").join("pax");
        Some(Paths {
            settings: config.join("settings.json"),
            policy: config.join("policy.json"),
            spending: state.join("spending.json"),
            audit: state.join("audit.jsonl"),
            store,
            models: vec![PathBuf::from(ADMIN_MODELS), PathBuf::from(SHIPPED_MODELS)],
            tools: PathBuf::from(crate::textui::DEFAULT_TOOLS_DIR),
            extra_manifests: None,
            extra_adapters: None,
            home,
        })
    }

    /// The paths of the session this process runs in. This is one of the
    /// two places the session service reads the environment; the other
    /// reads the address of the session bus.
    pub fn from_environment() -> Option<Paths> {
        Paths::from_bases(
            absolute(std::env::var_os("XDG_CONFIG_HOME")),
            absolute(std::env::var_os("XDG_STATE_HOME")),
            absolute(std::env::var_os("XDG_DATA_HOME")),
            absolute(std::env::var_os("HOME")),
        )
    }
}

/// The IANA name of a time zone, from the target of the link
/// `/etc/localtime`.
fn zone_from_link(target: &Path) -> Option<String> {
    let text = target.to_str()?;
    let (_, name) = text.rsplit_once("zoneinfo/")?;
    let name = name
        .strip_prefix("posix/")
        .or_else(|| name.strip_prefix("right/"))
        .unwrap_or(name);
    tur_pax_memory::is_known_zone(name).then(|| name.to_owned())
}

/// The time zone the computer is set to, as an IANA name: where
/// `/etc/localtime` points, or what `/etc/timezone` says. `UTC` if neither
/// names a zone this build knows.
pub fn host_zone() -> String {
    if let Some(zone) = fs::read_link("/etc/localtime")
        .ok()
        .and_then(|target| zone_from_link(&target))
    {
        return zone;
    }
    fs::read_to_string("/etc/timezone")
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|name| tur_pax_memory::is_known_zone(name))
        .unwrap_or_else(|| "UTC".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODELS: &str = r#"{
        "schema_version": 1,
        "status": "example",
        "prices_as_of": "2026-11-12",
        "providers": [{
            "id": "anthropic",
            "adapter": "anthropic",
            "name": "Anthropic",
            "data_terms_url": "https://example.org/terms",
            "models": [
                { "model": "priced-model", "label": "Priced", "max_output_tokens": 2000,
                  "prices": { "currency": "USD", "input_per_million": 3.0, "output_per_million": 15.0 } },
                { "model": "unpriced-model" }
            ]
        }]
    }"#;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("tur-pax-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn p_a_x_is_off_until_it_is_switched_on_with_the_current_consent() {
        let mut settings = Settings::default();
        assert!(!settings.enabled);
        assert_eq!(
            settings
                .refusal()
                .map(|fault| fault.code.to_string())
                .as_deref(),
            Some("TUR-AI-SERVICE-001")
        );
        // It cannot be switched on before a provider and a model are chosen.
        assert_eq!(
            settings
                .enable(CONSENT_TEXT_VERSION, 100)
                .expect_err("not configured")
                .code
                .as_str(),
            "TUR-AI-SERVICE-003"
        );
        settings
            .set_provider("anthropic", "priced-model", &ModelList::empty())
            .expect("a provider");
        // Nor with another version of the text.
        assert_eq!(
            settings
                .enable("0", 100)
                .expect_err("another version")
                .code
                .as_str(),
            "TUR-AI-SERVICE-002"
        );
        assert!(!settings.enabled);
        settings.enable(CONSENT_TEXT_VERSION, 100).expect("enabled");
        assert!(settings.enabled);
        assert!(settings.consent_is_current());
        assert_eq!(settings.refusal(), None);
        let consent = settings.consent.clone().expect("recorded");
        assert_eq!(consent.text_version, CONSENT_TEXT_VERSION);
        assert_eq!(consent.provider, "anthropic");
        assert_eq!(consent.accepted_at, 100);

        // Switched off, the record of what was agreed stays.
        settings.disable();
        assert_eq!(
            settings
                .refusal()
                .map(|fault| fault.code.to_string())
                .as_deref(),
            Some("TUR-AI-SERVICE-001")
        );
        assert!(settings.consent.is_some());
    }

    #[test]
    fn consent_for_one_provider_or_one_text_does_not_cover_another() {
        let mut settings = Settings::default();
        settings
            .set_provider("anthropic", "m", &ModelList::empty())
            .expect("a provider");
        settings.base_url = Some("https://gateway.example.org".to_owned());
        settings.enable(CONSENT_TEXT_VERSION, 5).expect("enabled");

        // Another model of the same provider: nothing is withdrawn.
        settings
            .set_provider("anthropic", "m2", &ModelList::empty())
            .expect("a model");
        assert!(settings.enabled);
        assert!(settings.base_url.is_some());

        // Another provider: off, no consent, and the gateway address gone.
        settings
            .set_provider("openai", "m3", &ModelList::empty())
            .expect("a provider");
        assert!(!settings.enabled);
        assert_eq!(settings.consent, None);
        assert_eq!(settings.base_url, None);

        // A consent recorded for an older text is not current.
        settings.enable(CONSENT_TEXT_VERSION, 9).expect("enabled");
        settings.consent.as_mut().expect("recorded").text_version = "0".to_owned();
        assert!(!settings.consent_is_current());
        assert_eq!(
            settings
                .refusal()
                .map(|fault| fault.code.to_string())
                .as_deref(),
            Some("TUR-AI-SERVICE-002")
        );

        // What cannot be chosen.
        for (provider, model) in [
            ("claude", "m"),
            ("", "m"),
            ("anthropic", ""),
            ("anthropic", " m"),
            ("anthropic", "a\nb"),
        ] {
            assert_eq!(
                settings
                    .set_provider(provider, model, &ModelList::empty())
                    .expect_err("refused")
                    .code
                    .as_str(),
                "TUR-AI-SERVICE-006",
                "{provider:?} {model:?}"
            );
        }
    }

    #[test]
    fn the_settings_are_saved_whole_and_read_back() {
        let directory = scratch("save");
        let path = directory.join("tur").join("pax").join("settings.json");
        // No file: the settings of a system on which P@x was never on.
        assert_eq!(
            Settings::load(&path).expect("defaults"),
            Settings::default()
        );

        let mut settings = Settings::default();
        settings
            .set_provider("google", "some-model", &ModelList::empty())
            .expect("a provider");
        settings.enable(CONSENT_TEXT_VERSION, 77).expect("enabled");
        settings.limits.day_cost = Some(2.5);
        settings.save(&path).expect("saved");
        assert_eq!(Settings::load(&path).expect("read back"), settings);

        // The file is the person's alone, and so is its directory.
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).expect("the file").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let mode = fs::metadata(path.parent().expect("a directory"))
            .expect("the directory")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        // No temporary file is left.
        let names: Vec<String> = fs::read_dir(path.parent().expect("a directory"))
            .expect("listed")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["settings.json"]);

        // Saving again replaces it.
        settings.disable();
        settings.save(&path).expect("saved");
        assert!(!Settings::load(&path).expect("read back").enabled);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn settings_that_cannot_be_used_are_refused_not_guessed() {
        for text in [
            "",
            "not json",
            "[]",
            r#"{"enabled": true}"#,
            r#"{"schema_version": 2}"#,
            r#"{"schema_version": 1, "api_key": "x"}"#,
            r#"{"schema_version": 1, "key": "x"}"#,
            r#"{"schema_version": 1, "provider": "Claude!"}"#,
            r#"{"schema_version": 1, "model": ""}"#,
            r#"{"schema_version": 1, "max_output_tokens": 0}"#,
            r#"{"schema_version": 1, "limits": {"day_cost": 0}}"#,
            r#"{"schema_version": 1, "limits": {"day_cost": -1}}"#,
            r#"{"schema_version": 1, "limits": {"task_tokens": 0}}"#,
            r#"{"schema_version": 1, "limits": {"per_week": 1}}"#,
            r#"{"schema_version": 1, "consent": {"text_version": "1"}}"#,
        ] {
            let fault = Settings::from_json(text).expect_err(text);
            assert_eq!(fault.code.as_str(), "TUR-AI-SERVICE-009", "{text}");
            assert!(!fault.detail.contains("api_key"), "{text}");
        }
        assert_eq!(
            Settings::from_json(r#"{"schema_version": 1}"#).expect("minimal"),
            Settings::default()
        );
    }

    #[test]
    fn the_limits_have_defaults_and_bounds() {
        let models = ModelList::from_json(MODELS).expect("a model list");
        let mut settings = Settings::default();
        settings
            .set_provider("anthropic", "priced-model", &ModelList::empty())
            .expect("a provider");

        // A model with a known price: limits in its currency, with the
        // defaults of the setup flow.
        let config = settings.provider_config(&models).expect("a configuration");
        assert_eq!(config.model, "priced-model");
        assert_eq!(config.max_output_tokens, 2000);
        assert_eq!(
            config
                .prices
                .as_ref()
                .map(|prices| prices.currency.as_str()),
            Some("USD")
        );
        assert_eq!(
            config.task_limit.and_then(|limit| limit.max_cost),
            Some(DEFAULT_TASK_COST)
        );
        assert_eq!(
            settings.day_limit(true),
            DayLimit {
                cost: Some(DEFAULT_DAY_COST),
                tokens: None
            }
        );

        // The person's own limits and output limit replace them.
        settings.limits = Limits {
            task_cost: Some(0.25),
            day_cost: Some(2.0),
            task_tokens: Some(5000),
            day_tokens: Some(90_000),
        };
        settings.max_output_tokens = Some(512);
        let config = settings.provider_config(&models).expect("a configuration");
        assert_eq!(config.max_output_tokens, 512);
        let limit = config.task_limit.expect("a task limit");
        assert_eq!(limit.max_cost, Some(0.25));
        assert_eq!(limit.max_tokens, Some(5000));
        assert_eq!(
            settings.day_limit(true),
            DayLimit {
                cost: Some(2.0),
                tokens: Some(90_000)
            }
        );

        // A model whose price is not known: limits in tokens, and no cost.
        settings.limits = Limits::default();
        settings.max_output_tokens = None;
        for model in ["unpriced-model", "typed-by-hand"] {
            settings
                .set_provider("anthropic", model, &ModelList::empty())
                .expect("a model");
            let config = settings.provider_config(&models).expect("a configuration");
            assert_eq!(config.prices, None, "{model}");
            assert_eq!(config.max_output_tokens, DEFAULT_MAX_OUTPUT_TOKENS);
            let limit = config.task_limit.expect("a task limit");
            assert_eq!(limit.max_cost, None);
            assert_eq!(limit.max_tokens, Some(DEFAULT_TASK_TOKENS));
        }
        assert_eq!(
            settings.day_limit(false),
            DayLimit {
                cost: None,
                tokens: Some(DEFAULT_DAY_TOKENS)
            }
        );

        // Nothing chosen: no configuration.
        assert_eq!(
            Settings::default()
                .provider_config(&models)
                .expect_err("not configured")
                .code
                .as_str(),
            "TUR-AI-SERVICE-003"
        );

        // Limits out of range.
        for limits in [
            Limits {
                task_cost: Some(0.0),
                ..Limits::default()
            },
            Limits {
                day_cost: Some(f64::NAN),
                ..Limits::default()
            },
            Limits {
                day_cost: Some(f64::INFINITY),
                ..Limits::default()
            },
            Limits {
                day_cost: Some(MAX_COST_LIMIT * 2.0),
                ..Limits::default()
            },
            Limits {
                day_tokens: Some(0),
                ..Limits::default()
            },
            Limits {
                task_tokens: Some(MAX_TOKEN_LIMIT + 1),
                ..Limits::default()
            },
        ] {
            assert_eq!(
                limits.validate().expect_err("out of range").code.as_str(),
                "TUR-AI-SERVICE-006",
                "{limits:?}"
            );
        }
    }

    #[test]
    fn a_provider_is_an_identifier_and_any_identifier_can_be_configured() {
        // A compatible service in the model list, spoken to with the OpenAI
        // family at its own address, its key under an item of its own.
        let listed = r#"{
            "schema_version": 1, "status": "example", "prices_as_of": "2026-11-12",
            "providers": [{
                "id": "deepseek", "adapter": "openai", "name": "DeepSeek",
                "base_url": "https://api.example.org", "keyring_item": "deepseek-key",
                "capabilities": { "tool_calling": true, "image_input": false, "context_tokens": 64000 },
                "models": [{ "model": "chat-model",
                    "prices": { "currency": "USD", "input_per_million": 0.5, "output_per_million": 1.0 } }]
            }]
        }"#;
        let models = ModelList::from_json(listed).expect("a model list");
        let mut settings = Settings::default();
        settings
            .set_provider("deepseek", "chat-model", &models)
            .expect("a provider");
        let (entry, origin) = settings.current_provider(&models).expect("known");
        assert_eq!(origin, Origin::Listed);
        assert_eq!(entry.keyring_item(), "deepseek-key");
        assert!(!entry.is_native());
        let config = settings.provider_config(&models).expect("a configuration");
        assert_eq!(config.provider, ProviderId::OpenAi);
        assert_eq!(config.base_url.as_deref(), Some("https://api.example.org"));
        assert_eq!(
            config.prices.map(|prices| prices.output_per_million),
            Some(1.0)
        );

        // The adapter families' own companies exist without any list.
        for adapter in COMPANY_FAMILIES {
            let (entry, origin) =
                find_provider(&ModelList::empty(), &Settings::default(), adapter.as_str())
                    .expect("implicit");
            assert_eq!(origin, Origin::Implicit);
            assert!(entry.is_native());
            assert_eq!(entry.keyring_item(), adapter.as_str());
            assert_eq!(entry.name, family_name(adapter));
        }

        // A provider the person added by hand, in the settings file.
        let mut settings = Settings::from_json(
            r#"{ "schema_version": 1, "providers": [{
                "id": "local", "adapter": "openai", "name": "My local server",
                "base_url": "http://127.0.0.1:8080" }] }"#,
        )
        .expect("settings");
        settings
            .set_provider("local", "any-model", &ModelList::empty())
            .expect("a provider");
        let (entry, origin) = settings
            .current_provider(&ModelList::empty())
            .expect("known");
        assert_eq!(origin, Origin::ByHand);
        assert_eq!(entry.keyring_item(), "local");
        let config = settings
            .provider_config(&ModelList::empty())
            .expect("a configuration");
        assert_eq!(config.base_url.as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(config.prices, None);
        // An identifier the list has is the list's.
        let mut shadow = Settings::from_json(
            r#"{ "schema_version": 1, "providers": [{
                "id": "deepseek", "adapter": "google", "name": "Not it" }] }"#,
        )
        .expect("settings");
        shadow
            .set_provider("deepseek", "chat-model", &models)
            .expect("a provider");
        assert_eq!(
            shadow
                .current_provider(&models)
                .map(|(entry, origin)| (entry.name, origin)),
            Some(("DeepSeek".to_owned(), Origin::Listed))
        );
        // Unknown identifiers cannot be chosen; bad entries are refused.
        assert!(
            Settings::default()
                .set_provider("nobody", "m", &models)
                .is_err()
        );
        for text in [
            r#"{ "schema_version": 1, "providers": [{ "id": "x", "adapter": "openai", "name": "X", "base_url": "http://example.org" }] }"#,
            r#"{ "schema_version": 1, "providers": [{ "id": "X", "adapter": "openai", "name": "X" }] }"#,
            r#"{ "schema_version": 1, "providers": [{ "id": "x", "adapter": "openai", "name": "X" }, { "id": "x", "adapter": "google", "name": "Y" }] }"#,
            r#"{ "schema_version": 1, "provider": "Not An Id" }"#,
        ] {
            assert_eq!(
                Settings::from_json(text).expect_err(text).code.as_str(),
                "TUR-AI-SERVICE-009",
                "{text}"
            );
        }
        assert!(is_provider_id("deepseek"));
        assert!(is_provider_id("open-router_2"));
        let long = "x".repeat(33);
        for bad in ["", "Deep", "1st", "a b", "a.b", long.as_str()] {
            assert!(!is_provider_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_model_list_is_checked() {
        let models = ModelList::from_json(MODELS).expect("a model list");
        assert_eq!(models.status, "example");
        assert_eq!(models.prices_as_of, "2026-11-12");
        assert!(models.find("anthropic", "priced-model").is_some());
        assert!(models.find("anthropic", "nothing").is_none());
        assert!(models.find("openai", "priced-model").is_none());
        assert!(models.provider("google").is_none());
        assert!(ModelList::empty().providers.is_empty());

        let with = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut value: serde_json::Value = serde_json::from_str(MODELS).expect("JSON");
            change(&mut value);
            ModelList::from_json(&value.to_string())
        };
        assert!(with(&|_| {}).is_ok());
        type Change = Box<dyn Fn(&mut serde_json::Value)>;
        let bad: Vec<Change> = vec![
            Box::new(|value: &mut serde_json::Value| value["schema_version"] = 2.into()),
            Box::new(|value: &mut serde_json::Value| value["status"] = "official".into()),
            Box::new(|value: &mut serde_json::Value| value["prices_as_of"] = "12 November".into()),
            Box::new(|value: &mut serde_json::Value| value["unknown"] = 1.into()),
            Box::new(|value: &mut serde_json::Value| value["providers"][0]["name"] = "".into()),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["data_terms_url"] = "http://example.org".into()
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["models"][0]["model"] = "".into()
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["models"][1]["model"] = "priced-model".into();
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["models"][0]["prices"]["input_per_million"] = (-1).into();
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["models"][0]["prices"]["currency"] = "US$".into();
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["models"][0]["max_output_tokens"] = 0.into()
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["models"][0]["api_key"] = "x".into()
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["id"] = "Anthropic".into()
            }),
            Box::new(|value: &mut serde_json::Value| value["providers"][0]["id"] = "".into()),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["adapter"] = "claude".into()
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["keyring_item"] = "a b".into()
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["base_url"] = "http://example.org".into()
            }),
            Box::new(|value: &mut serde_json::Value| {
                value["providers"][0]["capabilities"] = serde_json::json!({ "telepathy": true })
            }),
            Box::new(|value: &mut serde_json::Value| {
                let first = value["providers"][0].clone();
                value["providers"]
                    .as_array_mut()
                    .expect("an array")
                    .push(first);
            }),
        ];
        for (index, change) in bad.iter().enumerate() {
            let fault = with(&**change).expect_err("refused");
            assert_eq!(fault.code.as_str(), "TUR-AI-SERVICE-009", "case {index}");
        }

        // The first list that exists is the one that is used; none at all
        // is an empty list.
        let directory = scratch("models");
        fs::create_dir_all(&directory).expect("a directory");
        let missing = directory.join("missing.json");
        let present = directory.join("models.json");
        fs::write(&present, MODELS).expect("written");
        assert_eq!(
            ModelList::load(std::slice::from_ref(&missing)).expect("empty"),
            ModelList::empty()
        );
        assert_eq!(
            ModelList::load(&[missing, present.clone()]).expect("loaded"),
            models
        );
        fs::write(&present, "{").expect("written");
        assert!(ModelList::load(&[present]).is_err());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_compatible_provider_is_data_with_terms_capabilities_and_an_optional_key() {
        let listed = r#"{
            "schema_version": 1, "status": "example", "prices_as_of": "2026-11-12",
            "providers": [{
                "id": "deepseek", "adapter": "openai-chat", "name": "DeepSeek",
                "base_url": "https://api.example.org", "send_back_reasoning": true,
                "capabilities": { "tool_calling": true, "image_input": false },
                "data_terms": {
                    "reviewed_on": "2026-11-12",
                    "stores_requests": "yes",
                    "trains_on_data": "yes, unless you opt out",
                    "processing_location": "the People's Republic of China",
                    "terms_url": "https://example.org/privacy",
                    "sources": ["https://example.org/privacy"]
                },
                "models": [
                    { "model": "sees", "capabilities": { "image_input": true, "context_tokens": 1000 } },
                    { "model": "talks", "capabilities": { "tool_calling": false } }
                ]
            }, {
                "id": "local", "adapter": "openai-chat", "name": "On this computer",
                "base_url": "http://127.0.0.1:11434/v1", "key_required": false,
                "data_terms": { "runs_locally": true }
            }]
        }"#;
        let models = ModelList::from_json(listed).expect("a model list");
        let (deepseek, origin) =
            find_provider(&models, &Settings::default(), "deepseek").expect("listed");
        assert_eq!(origin, Origin::Listed);
        assert!(deepseek.key_required);
        // A model's declarations over its provider's.
        assert_eq!(
            deepseek.abilities("sees"),
            Abilities {
                act: Some(true),
                see: Some(true)
            }
        );
        assert_eq!(deepseek.declared("sees").context_tokens, Some(1000));
        assert_eq!(
            deepseek.abilities("talks"),
            Abilities {
                act: Some(false),
                see: Some(false)
            }
        );
        assert_eq!(
            deepseek.abilities("typed-by-hand"),
            Abilities {
                act: Some(true),
                see: Some(false)
            }
        );
        assert_eq!(
            deepseek.abilities("talks").notices(),
            [MODEL_CANNOT_ACT, MODEL_CANNOT_SEE]
        );
        assert!(
            ProviderEntry::implicit(ProviderId::Anthropic)
                .abilities("any")
                .notices()
                .is_empty()
        );
        // Reviewed terms are shown for the entry at its own address only,
        // and never for an entry the person added.
        assert!(deepseek.reviewed_terms(Origin::Listed, false).is_some());
        assert!(deepseek.reviewed_terms(Origin::Listed, true).is_none());
        assert!(deepseek.reviewed_terms(Origin::ByHand, false).is_none());
        // The family and the reasoning setting reach the configuration.
        let mut settings = Settings::default();
        settings
            .set_provider("deepseek", "sees", &models)
            .expect("a provider");
        let config = settings.provider_config(&models).expect("a configuration");
        assert_eq!(config.provider, ProviderId::OpenAiChat);
        assert!(config.send_back_reasoning);

        // A server on this computer needs no key and runs locally.
        let (local, origin) =
            find_provider(&models, &Settings::default(), "local").expect("listed");
        assert!(!local.key_required);
        assert!(local.runs_locally(origin, None));
        assert!(!local.runs_locally(origin, Some("https://gateway.example.org")));
        assert!(!local.runs_locally(Origin::ByHand, None));

        // What an entry may not say.
        let entry = |fields: &str| {
            format!(
                r#"{{ "schema_version": 1, "status": "example", "prices_as_of": "2026-11-12",
                     "providers": [{{ "id": "x", "name": "X", {fields} }}] }}"#
            )
        };
        for fields in [
            // The compatible family has no address of its own.
            r#""adapter": "openai-chat""#,
            // Only it can do without a key, or send reasoning back.
            r#""adapter": "openai", "key_required": false"#,
            r#""adapter": "anthropic", "send_back_reasoning": true"#,
            // Local means this computer.
            r#""adapter": "openai-chat", "base_url": "https://api.example.org", "data_terms": { "runs_locally": true }"#,
            // Reviewed terms say when, from where, in plain words.
            r#""adapter": "openai-chat", "base_url": "https://a.example.org", "data_terms": { "reviewed_on": "12 November" , "sources": ["https://a.example.org"] }"#,
            r#""adapter": "openai-chat", "base_url": "https://a.example.org", "data_terms": { "reviewed_on": "2026-11-12" }"#,
            r#""adapter": "openai-chat", "base_url": "https://a.example.org", "data_terms": { "stores_requests": "" }"#,
            r#""adapter": "openai-chat", "base_url": "https://a.example.org", "data_terms": { "retention": "a\nb" }"#,
            r#""adapter": "openai-chat", "base_url": "https://a.example.org", "data_terms": { "terms_url": "http://a.example.org" }"#,
            r#""adapter": "openai-chat", "base_url": "https://a.example.org", "data_terms": { "unknown": 1 }"#,
            r#""adapter": "openai-chat", "base_url": "https://a.example.org", "models": [{ "model": "m", "capabilities": { "context_tokens": 0 } }]"#,
        ] {
            assert_eq!(
                ModelList::from_json(&entry(fields))
                    .expect_err(fields)
                    .code
                    .as_str(),
                "TUR-AI-SERVICE-009",
                "{fields}"
            );
        }
    }

    #[test]
    fn the_shipped_list_loads_with_its_presets() {
        let shipped = ModelList::from_json(include_str!("../data/models.json")).expect("loads");
        assert_eq!(shipped.status, "example");
        let compatible: Vec<&ProviderEntry> = shipped
            .providers
            .iter()
            .filter(|entry| entry.adapter == ProviderId::OpenAiChat)
            .collect();
        let ids: Vec<&str> = compatible.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "deepseek",
                "mistral",
                "xai",
                "groq",
                "openrouter",
                "together",
                "ollama",
                "llama-server"
            ]
        );
        for entry in compatible {
            let terms = entry.data_terms.as_ref().expect("data terms");
            if entry.key_required {
                // A cloud service: reviewed, with the pages it was read from,
                // at an https address, offering tools.
                assert_eq!(
                    terms.reviewed_on.as_deref(),
                    Some("2026-11-12"),
                    "{}",
                    entry.id
                );
                assert!(!terms.sources.is_empty(), "{}", entry.id);
                assert!(!terms.runs_locally, "{}", entry.id);
                assert!(
                    entry
                        .base_url
                        .as_deref()
                        .is_some_and(|url| url.starts_with("https://")),
                    "{}",
                    entry.id
                );
                assert_eq!(entry.declared("").tool_calling, Some(true), "{}", entry.id);
                assert!(!entry.models.is_empty(), "{}", entry.id);
            } else {
                // A server on this computer: no key, nothing declared.
                assert!(terms.runs_locally, "{}", entry.id);
                assert!(entry.runs_locally(Origin::Listed, None), "{}", entry.id);
                assert_eq!(entry.declared("").tool_calling, None, "{}", entry.id);
            }
            // Only DeepSeek's documentation requires its reasoning back.
            assert_eq!(entry.send_back_reasoning, entry.id == "deepseek");
        }
        // The three companies' terms were not reviewed.
        for id in ["anthropic", "openai", "google"] {
            let entry = shipped.provider(id).expect("listed");
            assert!(
                entry.reviewed_terms(Origin::Listed, false).is_none(),
                "{id}"
            );
        }
    }

    #[test]
    fn every_provider_identifier_is_a_keyring_item_and_the_set_is_safe() {
        let longest_id = "a".repeat(32);
        for id in [
            "deepseek",
            "open-router_2",
            "x",
            "llama-server",
            longest_id.as_str(),
        ] {
            assert!(is_provider_id(id), "{id}");
            assert!(is_keyring_item(id), "{id}");
        }
        let longest_item = "k".repeat(64);
        for item in ["deepseek.com", "groq-work-2", "0key", longest_item.as_str()] {
            assert!(is_keyring_item(item), "{item}");
        }
        let long = "k".repeat(65);
        for bad in [
            "",
            "Deep",
            ".hidden",
            "a..b",
            "a/b",
            "a b",
            "naïve",
            "-dash",
            long.as_str(),
        ] {
            assert!(!is_keyring_item(bad), "{bad:?}");
        }
        assert!(is_loopback_url("http://127.0.0.1:11434/v1"));
        assert!(is_loopback_url("http://localhost:9931/v1"));
        assert!(is_loopback_url("http://[::1]:8080"));
        assert!(!is_loopback_url("https://api.example.org"));
        assert!(!is_loopback_url("http://127.example.org"));
        assert!(!is_loopback_url("not a url"));
    }

    #[test]
    fn the_paths_follow_the_base_directories() {
        let home = PathBuf::from("/home/ada");
        let paths = Paths::from_bases(None, None, None, Some(home.clone())).expect("paths");
        assert_eq!(paths.settings, home.join(".config/tur/pax/settings.json"));
        assert_eq!(paths.policy, home.join(".config/tur/pax/policy.json"));
        assert_eq!(
            paths.spending,
            home.join(".local/state/tur/pax/spending.json")
        );
        assert_eq!(paths.audit, home.join(".local/state/tur/pax/audit.jsonl"));
        assert_eq!(paths.store, home.join(".local/share/tur/pax"));
        assert_eq!(
            paths.models,
            [PathBuf::from(ADMIN_MODELS), PathBuf::from(SHIPPED_MODELS)]
        );
        assert_eq!(paths.tools, PathBuf::from("/usr/share/tur/pax/tools"));

        let paths = Paths::from_bases(
            Some(PathBuf::from("/c")),
            Some(PathBuf::from("/s")),
            Some(PathBuf::from("/d")),
            None,
        )
        .expect("paths");
        assert_eq!(paths.settings, PathBuf::from("/c/tur/pax/settings.json"));
        assert_eq!(paths.audit, PathBuf::from("/s/tur/pax/audit.jsonl"));
        assert_eq!(paths.store, PathBuf::from("/d/tur/pax"));
        assert_eq!(Paths::from_bases(None, None, None, None), None);
    }

    #[test]
    fn the_time_zone_of_the_computer_is_read_from_where_the_link_points() {
        for (target, zone) in [
            ("/usr/share/zoneinfo/Europe/Rome", Some("Europe/Rome")),
            (
                "../usr/share/zoneinfo/America/New_York",
                Some("America/New_York"),
            ),
            ("/usr/share/zoneinfo/posix/Asia/Tokyo", Some("Asia/Tokyo")),
            ("/usr/share/zoneinfo/UTC", Some("UTC")),
            ("/usr/share/zoneinfo/Mars/Olympus", None),
            ("/etc/somewhere/else", None),
        ] {
            assert_eq!(
                zone_from_link(Path::new(target)).as_deref(),
                zone,
                "{target}"
            );
        }
        // Whatever this machine is set to, the answer is a zone that exists.
        assert!(tur_pax_memory::is_known_zone(&host_zone()));
    }
}
