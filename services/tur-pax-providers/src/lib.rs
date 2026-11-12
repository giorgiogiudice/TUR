// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The P@x provider layer: one provider-neutral interface and an adapter for
//! each of Anthropic, OpenAI and Google.
//!
//! P@x, the assistant, is provider-agnostic (ADR-0004). The orchestrator
//! speaks only the neutral model of this crate; everything specific to a
//! provider stays inside its adapter. The design is in
//! `docs/architecture/pax-providers.md`.
//!
//! * [`model`] is the neutral conversation model: messages, parts, tool
//!   definitions, requests, and the events of a streamed response.
//! * [`provider`] is the interface: a [`Provider`] declares its
//!   [`Capabilities`] and streams a response as an [`EventStream`].
//! * [`anthropic`], [`openai`] and [`google`] are the adapters.
//! * [`error`] is the typed error with its stable `UOS-AI-PROVIDER-*` code.
//! * [`secret`] holds the API key, which cannot be printed.
//! * [`config`] is the configuration file format.
//! * [`spend`] adds up what a task used and enforces its limit.
//!
//! The interface is blocking and has no async runtime: a stream is an iterator,
//! and a caller runs a provider call on a thread of its own.
//!
//! Nothing in this crate has been run against a real provider. The adapters are
//! written from the providers' documentation and tested against a local server
//! that replays hand-written responses.
//!
//! A configuration names the provider and the model; neither has a default:
//!
//! ```
//! use tur_pax_providers::{ProviderConfig, ProviderId, capabilities_of};
//!
//! let config = ProviderConfig::from_json(
//!     r#"{
//!         "schema_version": 1,
//!         "provider": "anthropic",
//!         "model": "the-model-name-from-configuration",
//!         "max_output_tokens": 1024
//!     }"#,
//! )
//! .unwrap();
//! assert_eq!(config.provider, ProviderId::Anthropic);
//! assert!(capabilities_of(config.provider).tool_calling);
//! ```
//!
//! A response is read as a stream that always ends with one finish event:
//!
//! ```no_run
//! use tur_pax_providers::{
//!     EnvKeySource, KeySource, Message, ProviderConfig, ProviderError, StreamEvent, connect,
//! };
//!
//! # fn demo() -> Result<(), ProviderError> {
//! let text = std::fs::read_to_string("provider.json").expect("a configuration file");
//! let config = ProviderConfig::from_json(&text)?;
//! let key = EnvKeySource::new("TUR_PAX_API_KEY").api_key(config.provider)?;
//! let provider = connect(&config, key)?;
//! let request = config.request(None, vec![Message::user_text("Hello")], Vec::new());
//! for event in provider.stream(&request) {
//!     match event {
//!         StreamEvent::TextDelta { text } => print!("{text}"),
//!         StreamEvent::Finish(reason) => println!("\n[{}]", reason.name()),
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod anthropic;
pub mod cli;
pub mod config;
pub mod error;
pub mod google;
mod http;
pub mod model;
pub mod openai;
pub mod provider;
pub mod secret;
pub mod spend;
mod sse;
mod toolcalls;

pub use config::{Prices, ProviderConfig, RetryPolicy, TaskLimit, Timeouts};
pub use error::{ErrorKind, ProviderError};
pub use model::{
    FinishReason, Image, Message, OpaquePart, Part, ProviderId, Request, ResultContent, Role,
    StreamEvent, ToolCall, ToolDefinition, ToolResult, Turn, Usage,
};
pub use provider::{
    Capabilities, EventStream, NativeComputerControl, Provider, capabilities_of, connect,
    endpoint_path,
};
pub use secret::{ApiKey, EnvKeySource, KeySource};
pub use spend::{MeteredStream, SpendMeter};
