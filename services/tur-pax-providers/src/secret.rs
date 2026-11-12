// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The API key and where it comes from.
//!
//! An [`ApiKey`] cannot be printed: `Debug` and `Display` give a fixed
//! placeholder, and the type implements neither `Clone` nor `Serialize`. Its
//! memory is overwritten when it is dropped. The key text can be read only
//! inside this crate, and is read in two places: the HTTP transport puts it in
//! a request header, and a stream uses it to remove the key from any error text
//! a provider sends back.
//!
//! Keys arrive through the [`KeySource`] trait. The system keyring is the real
//! source and is implemented elsewhere. [`EnvKeySource`] reads an environment
//! variable and is for development only.

use std::fmt;

use zeroize::Zeroizing;

use crate::error::{ErrorKind, ProviderError};
use crate::model::ProviderId;

/// The longest key accepted, in bytes.
pub const KEY_MAX: usize = 4096;

/// The text printed in place of a key.
pub const REDACTED: &str = "[redacted]";

/// A provider API key.
pub struct ApiKey {
    text: Zeroizing<String>,
}

/// Whether `text`, with surrounding whitespace removed, has the form of a
/// key: 1 to [`KEY_MAX`] printable ASCII characters with no spaces, which is
/// what can be sent in an HTTP header. It says nothing about whether a
/// provider accepts the key.
pub fn is_usable_key(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.len() <= KEY_MAX && trimmed.bytes().all(|b| b.is_ascii_graphic())
}

impl ApiKey {
    /// Take ownership of a key.
    ///
    /// Surrounding whitespace is removed. The key must then be 1 to [`KEY_MAX`]
    /// printable ASCII characters with no spaces, which is what can be sent in
    /// an HTTP header. The given string is overwritten before it is freed.
    pub fn new(text: String) -> Result<Self, ProviderError> {
        let given = Zeroizing::new(text);
        let trimmed = given.trim();
        let acceptable = is_usable_key(trimmed);
        if !acceptable {
            return Err(ProviderError::new(ErrorKind::KeyUnavailable).with_detail(
                "the key is empty, too long, or has characters that cannot be sent in a header",
            ));
        }
        Ok(ApiKey {
            text: Zeroizing::new(trimmed.to_owned()),
        })
    }

    /// The key text. Read only to send it and to remove it from error text.
    pub(crate) fn expose(&self) -> &str {
        self.text.as_str()
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ApiKey({REDACTED})")
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// Where API keys come from.
pub trait KeySource {
    /// The key for the given provider.
    fn api_key(&self, provider: ProviderId) -> Result<ApiKey, ProviderError>;
}

/// Reads the key from an environment variable. For development only: the
/// environment of a process is readable by other processes of the same user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvKeySource {
    variable: String,
}

impl EnvKeySource {
    /// A source that reads the named environment variable.
    pub fn new(variable: impl Into<String>) -> Self {
        EnvKeySource {
            variable: variable.into(),
        }
    }

    /// The name of the environment variable.
    pub fn variable(&self) -> &str {
        &self.variable
    }
}

impl KeySource for EnvKeySource {
    fn api_key(&self, provider: ProviderId) -> Result<ApiKey, ProviderError> {
        key_from_lookup(&self.variable, std::env::var(&self.variable).ok())
            .map_err(|error| error.with_provider(provider))
    }
}

/// Turn the result of looking up `variable` into a key. The error names the
/// variable and never its value.
fn key_from_lookup(variable: &str, value: Option<String>) -> Result<ApiKey, ProviderError> {
    let Some(value) = value else {
        return Err(ProviderError::new(ErrorKind::KeyUnavailable)
            .with_detail(format!("the environment variable {variable} is not set")));
    };
    ApiKey::new(value).map_err(|_| {
        ProviderError::new(ErrorKind::KeyUnavailable).with_detail(format!(
            "the environment variable {variable} does not hold a usable key"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-test-0123456789abcdef";

    #[test]
    fn the_form_of_a_key_is_checked_without_keeping_it() {
        assert!(is_usable_key(KEY));
        assert!(is_usable_key("  sk-test-0123456789abcdef\n"));
        assert!(!is_usable_key(""));
        assert!(!is_usable_key("   "));
        assert!(!is_usable_key("two words"));
        assert!(!is_usable_key("line\nbreak"));
        assert!(!is_usable_key("k\u{e9}y"));
        assert!(!is_usable_key(&"k".repeat(KEY_MAX + 1)));
        assert!(is_usable_key(&"k".repeat(KEY_MAX)));
    }

    #[test]
    fn a_key_cannot_be_printed() {
        let key = ApiKey::new(KEY.to_owned()).unwrap();
        assert_eq!(format!("{key}"), REDACTED);
        assert_eq!(format!("{key:?}"), "ApiKey([redacted])");
        assert_eq!(format!("{key:#?}"), "ApiKey([redacted])");
        assert_eq!(key.expose(), KEY);
    }

    #[test]
    fn surrounding_whitespace_is_removed() {
        let key = ApiKey::new(format!("  {KEY}\n")).unwrap();
        assert_eq!(key.expose(), KEY);
    }

    #[test]
    fn unusable_keys_are_rejected_without_repeating_them() {
        let too_long = "k".repeat(KEY_MAX + 1);
        for bad in [
            "",
            "   ",
            "has space",
            "line\nbreak",
            "naïve-key",
            too_long.as_str(),
        ] {
            let error = ApiKey::new(bad.to_owned()).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::KeyUnavailable);
            if !bad.trim().is_empty() {
                assert!(!error.to_string().contains(bad.trim()), "{error}");
            }
        }
    }

    #[test]
    fn the_environment_source_names_the_variable_never_the_value() {
        let key = key_from_lookup("TUR_TEST_KEY", Some(KEY.to_owned())).unwrap();
        assert_eq!(key.expose(), KEY);

        let missing = key_from_lookup("TUR_TEST_KEY", None).unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::KeyUnavailable);
        assert!(missing.to_string().contains("TUR_TEST_KEY"));

        let unusable =
            key_from_lookup("TUR_TEST_KEY", Some("bad key value".to_owned())).unwrap_err();
        assert_eq!(unusable.kind(), ErrorKind::KeyUnavailable);
        assert!(unusable.to_string().contains("TUR_TEST_KEY"));
        assert!(!unusable.to_string().contains("bad key value"));
    }

    #[test]
    fn an_unset_variable_gives_a_typed_error() {
        let source = EnvKeySource::new("TUR_PAX_PROVIDERS_TEST_VARIABLE_THAT_IS_NEVER_SET");
        assert_eq!(
            source.variable(),
            "TUR_PAX_PROVIDERS_TEST_VARIABLE_THAT_IS_NEVER_SET"
        );
        let error = source.api_key(ProviderId::OpenAi).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::KeyUnavailable);
        assert_eq!(error.provider(), Some(ProviderId::OpenAi));
    }
}
