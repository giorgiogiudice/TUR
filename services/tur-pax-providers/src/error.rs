// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Typed provider errors with stable `UOS-AI-PROVIDER-*` codes.
//!
//! Every failure of a provider call is a [`ProviderError`]. It carries an
//! [`ErrorKind`], which fixes the stable code and the plain message shown to the
//! user, and optional facts: the provider, the HTTP status, the retry delay the
//! provider gave, and a short detail taken from the provider's own message.
//!
//! An error never contains the API key. The detail is the only free text, and
//! the transport removes the key from it before the error leaves the crate.

use std::fmt;
use std::time::Duration;

use uos_core::ErrorCode;

use crate::model::ProviderId;

/// The longest detail kept, in characters.
pub const DETAIL_MAX: usize = 300;

// Declares the `ErrorKind` enum, the `REGISTRY` table and the lookups from one
// list, so a kind cannot exist without a code and a user message.
macro_rules! error_kinds {
    ($( $variant:ident => $code:literal, $message:literal; )+) => {
        /// What went wrong, independent of the provider.
        ///
        /// The documentation of each variant is the plain message shown to the user.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ErrorKind {
            $(
                #[doc = $message]
                $variant,
            )+
        }

        /// The registry: every kind with its code and its user message, in registry order.
        pub const REGISTRY: &[(ErrorKind, &str, &str)] = &[
            $( (ErrorKind::$variant, $code, $message), )+
        ];

        impl ErrorKind {
            /// The stable code as text, for example `UOS-AI-PROVIDER-002`.
            pub const fn code(self) -> &'static str {
                match self {
                    $( ErrorKind::$variant => $code, )+
                }
            }

            /// The plain message shown to the user.
            pub const fn user_message(self) -> &'static str {
                match self {
                    $( ErrorKind::$variant => $message, )+
                }
            }
        }
    };
}

error_kinds! {
    AuthenticationFailed => "UOS-AI-PROVIDER-001", "The provider did not accept the API key, or the key is not permitted to do this.";
    RateLimited => "UOS-AI-PROVIDER-002", "The provider is limiting requests for this key. Try again in a moment.";
    Overloaded => "UOS-AI-PROVIDER-003", "The provider is overloaded or temporarily unavailable.";
    RequestRejected => "UOS-AI-PROVIDER-004", "The provider rejected the request.";
    Refusal => "UOS-AI-PROVIDER-005", "The model declined to respond to this request.";
    Network => "UOS-AI-PROVIDER-006", "The provider could not be reached, or the connection was lost.";
    MalformedResponse => "UOS-AI-PROVIDER-007", "The provider sent a response that could not be understood.";
    TimedOut => "UOS-AI-PROVIDER-008", "The provider did not respond in time.";
    QuotaExhausted => "UOS-AI-PROVIDER-009", "The provider account has no remaining credit or quota.";
    SpendLimitReached => "UOS-AI-PROVIDER-010", "The spending limit set for this task was reached.";
    ConfigInvalid => "UOS-AI-PROVIDER-011", "The provider configuration is not valid.";
    KeyUnavailable => "UOS-AI-PROVIDER-012", "No usable API key is available for this provider.";
}

impl ErrorKind {
    /// The code as a validated [`ErrorCode`].
    ///
    /// # Panics
    ///
    /// Never for a registered kind: a unit test parses every registry entry.
    pub fn error_code(self) -> ErrorCode {
        self.code()
            .parse()
            .expect("every registry entry is a valid UOS error code")
    }

    /// Whether the same request may succeed if it is sent again later.
    ///
    /// A rejected request, a refusal, an authentication failure and an exhausted
    /// quota are never retried.
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::RateLimited
                | ErrorKind::Overloaded
                | ErrorKind::Network
                | ErrorKind::TimedOut
        )
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One failure of a provider call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    kind: ErrorKind,
    provider: Option<ProviderId>,
    http_status: Option<u16>,
    retry_after: Option<Duration>,
    detail: Option<String>,
}

impl ProviderError {
    /// An error of the given kind with no further facts.
    pub fn new(kind: ErrorKind) -> Self {
        ProviderError {
            kind,
            provider: None,
            http_status: None,
            retry_after: None,
            detail: None,
        }
    }

    /// Attach a short detail for a log or a terminal.
    ///
    /// Control characters are replaced and the text is cut to [`DETAIL_MAX`]
    /// characters, because the detail usually comes from the provider.
    pub fn with_detail(mut self, detail: impl AsRef<str>) -> Self {
        let cleaned: String = detail
            .as_ref()
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(DETAIL_MAX)
            .collect();
        let cleaned = cleaned.trim();
        self.detail = if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.to_owned())
        };
        self
    }

    /// Record which provider the error came from, unless one is already recorded.
    pub fn with_provider(mut self, provider: ProviderId) -> Self {
        if self.provider.is_none() {
            self.provider = Some(provider);
        }
        self
    }

    pub(crate) fn with_status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }

    pub(crate) fn with_retry_after(mut self, delay: Option<Duration>) -> Self {
        self.retry_after = delay;
        self
    }

    /// Remove every occurrence of `secret` from the detail.
    pub(crate) fn scrubbed(mut self, secret: &str) -> Self {
        if !secret.is_empty()
            && let Some(detail) = self.detail.take()
        {
            self.detail = Some(detail.replace(secret, crate::secret::REDACTED));
        }
        self
    }

    /// What went wrong.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The stable code as text.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// The plain message shown to the user.
    pub fn user_message(&self) -> &'static str {
        self.kind.user_message()
    }

    /// The provider the error came from, if known.
    pub fn provider(&self) -> Option<ProviderId> {
        self.provider
    }

    /// The HTTP status of the provider's reply, if there was one.
    pub fn http_status(&self) -> Option<u16> {
        self.http_status
    }

    /// The delay the provider asked for before the next attempt, if it gave one.
    pub fn retry_after(&self) -> Option<Duration> {
        self.retry_after
    }

    /// A short detail, usually the provider's own message.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Whether the same request may succeed if it is sent again later.
    pub fn is_retryable(&self) -> bool {
        self.kind.is_retryable()
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.user_message())?;
        if let Some(provider) = self.provider {
            write!(f, " [{provider}]")?;
        }
        if let Some(status) = self.http_status {
            write!(f, " [HTTP {status}]")?;
        }
        if let Some(delay) = self.retry_after {
            write!(f, " [retry after {} s]", delay.as_secs())?;
        }
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ProviderError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use uos_core::Family;

    use super::*;

    #[test]
    fn every_kind_has_a_valid_ai_provider_code() {
        assert!(!REGISTRY.is_empty());
        for (kind, code, message) in REGISTRY {
            let parsed: ErrorCode = code.parse().unwrap_or_else(|err| panic!("{code}: {err}"));
            assert_eq!(parsed.family(), Family::Ai, "{code}");
            assert!(code.starts_with("UOS-AI-PROVIDER-"), "{code}");
            assert_eq!(kind.code(), *code);
            assert_eq!(kind.user_message(), *message);
            assert_eq!(kind.error_code(), parsed);
            assert!(
                message.ends_with('.'),
                "{code}: the message must be a sentence"
            );
        }
    }

    #[test]
    fn codes_and_messages_are_unique() {
        let mut codes = BTreeSet::new();
        let mut messages = BTreeSet::new();
        let mut kinds = BTreeSet::new();
        for (kind, code, message) in REGISTRY {
            assert!(codes.insert(*code), "duplicate code {code}");
            assert!(messages.insert(*message), "duplicate message for {code}");
            assert!(kinds.insert(*kind), "duplicate kind for {code}");
        }
    }

    #[test]
    fn only_transient_kinds_are_retryable() {
        let retryable: Vec<ErrorKind> = REGISTRY
            .iter()
            .map(|(kind, _, _)| *kind)
            .filter(|kind| kind.is_retryable())
            .collect();
        assert_eq!(
            retryable,
            [
                ErrorKind::RateLimited,
                ErrorKind::Overloaded,
                ErrorKind::Network,
                ErrorKind::TimedOut
            ]
        );
    }

    #[test]
    fn display_gives_code_message_and_facts() {
        let error = ProviderError::new(ErrorKind::RateLimited)
            .with_provider(ProviderId::Anthropic)
            .with_status(429)
            .with_retry_after(Some(Duration::from_secs(7)))
            .with_detail("slow down");
        assert_eq!(
            error.to_string(),
            "UOS-AI-PROVIDER-002: The provider is limiting requests for this key. Try again in a \
             moment. [anthropic] [HTTP 429] [retry after 7 s] (slow down)"
        );
    }

    #[test]
    fn detail_is_cleaned_and_bounded() {
        let error = ProviderError::new(ErrorKind::RequestRejected).with_detail("a\nb\u{7}c  ");
        assert_eq!(error.detail(), Some("a b c"));

        let long = "x".repeat(DETAIL_MAX * 2);
        let error = ProviderError::new(ErrorKind::RequestRejected).with_detail(long);
        assert_eq!(error.detail().map(|d| d.chars().count()), Some(DETAIL_MAX));

        let error = ProviderError::new(ErrorKind::RequestRejected).with_detail("  \n ");
        assert_eq!(error.detail(), None);
    }

    #[test]
    fn scrubbing_removes_the_secret_from_the_detail() {
        let error = ProviderError::new(ErrorKind::AuthenticationFailed)
            .with_detail("bad key sk-secret-123 given")
            .scrubbed("sk-secret-123");
        assert_eq!(error.detail(), Some("bad key [redacted] given"));
        assert!(!error.to_string().contains("sk-secret-123"));
        assert!(!format!("{error:?}").contains("sk-secret-123"));
    }

    #[test]
    fn the_first_recorded_provider_is_kept() {
        let error = ProviderError::new(ErrorKind::Network)
            .with_provider(ProviderId::Google)
            .with_provider(ProviderId::OpenAi);
        assert_eq!(error.provider(), Some(ProviderId::Google));
    }
}
