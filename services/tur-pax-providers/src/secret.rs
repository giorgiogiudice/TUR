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
//! variable and is for development only. [`FileKeySource`] reads a key that a
//! model server on this computer wrote for the person, such as the access key
//! `tur-ai-local` writes each time it starts a model.

use std::fmt;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

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

/// Reads the key from a file that a service on this computer wrote for the
/// person, and that only the person can read.
///
/// This is for a model server on this computer that makes a new random key
/// each time it starts, and writes it to a file in the session's runtime
/// directory for the programs of the person's session: `tur-ai-local`
/// writes `$XDG_RUNTIME_DIR/tur-ai-local/api-key` (mode 0600). Such a key is
/// not the person's to keep in the password store: it changes with every
/// start, is worthless once the server stops, and is removed at log-out.
///
/// The file is refused unless it is a regular file (not a link), no larger
/// than a key can be, and readable by nobody but its owner: no permission
/// for the group or for others. Errors name the file and never its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileKeySource {
    path: PathBuf,
}

impl FileKeySource {
    /// A source that reads the named file.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileKeySource { path: path.into() }
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl KeySource for FileKeySource {
    fn api_key(&self, provider: ProviderId) -> Result<ApiKey, ProviderError> {
        key_from_file(&self.path).map_err(|error| error.with_provider(provider))
    }
}

/// Read a key from a file the person alone can read. The error names the
/// file and never its content.
fn key_from_file(path: &Path) -> Result<ApiKey, ProviderError> {
    let refused = |why: &str| {
        ProviderError::new(ErrorKind::KeyUnavailable)
            .with_detail(format!("the key file {} {why}", path.display()))
    };
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            refused("does not exist")
        } else {
            refused("cannot be read")
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(refused("is not a regular file"));
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(refused("can be read by others than its owner"));
    }
    // A key and a line break, at most.
    let limit = KEY_MAX + 2;
    if metadata.len() > limit as u64 {
        return Err(refused("is larger than a key"));
    }
    let file = std::fs::File::open(path).map_err(|_| refused("cannot be read"))?;
    let mut text = Zeroizing::new(String::with_capacity(limit + 1));
    file.take(limit as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|_| refused("cannot be read"))?;
    ApiKey::new(std::mem::take(&mut *text)).map_err(|_| refused("does not hold a usable key"))
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
    fn the_file_source_reads_a_key_only_its_owner_can_read() {
        use std::os::unix::fs::PermissionsExt;

        let directory =
            std::env::temp_dir().join(format!("tur-pax-providers-key-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("api-key");
        let write = |text: &str, mode: u32| {
            std::fs::write(&path, text).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        let source = FileKeySource::new(&path);
        assert_eq!(source.path(), path);

        write(&format!("{KEY}\n"), 0o600);
        let key = source.api_key(ProviderId::OpenAiChat).unwrap();
        assert_eq!(key.expose(), KEY);

        let refusals = [
            (
                KEY.to_owned(),
                0o640,
                "can be read by others than its owner",
            ),
            (
                KEY.to_owned(),
                0o604,
                "can be read by others than its owner",
            ),
            (
                "bad key value".to_owned(),
                0o600,
                "does not hold a usable key",
            ),
            (String::new(), 0o600, "does not hold a usable key"),
            ("k".repeat(KEY_MAX + 3), 0o600, "is larger than a key"),
        ];
        for (text, mode, why) in refusals {
            write(&text, mode);
            let error = source.api_key(ProviderId::OpenAiChat).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::KeyUnavailable);
            assert_eq!(error.provider(), Some(ProviderId::OpenAiChat));
            let said = error.to_string();
            assert!(said.contains(why), "{said}");
            assert!(said.contains("api-key"), "{said}");
            if !text.is_empty() && text.len() < 100 {
                assert!(!said.contains(text.trim()), "{said}");
            }
        }

        // A link is not followed, and a file that is not there is named.
        std::fs::remove_file(&path).unwrap();
        let missing = source.api_key(ProviderId::OpenAiChat).unwrap_err();
        assert!(missing.to_string().contains("does not exist"), "{missing}");
        let target = directory.join("elsewhere");
        std::fs::write(&target, KEY).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let linked = source.api_key(ProviderId::OpenAiChat).unwrap_err();
        assert!(
            linked.to_string().contains("is not a regular file"),
            "{linked}"
        );
        let _ = std::fs::remove_dir_all(&directory);
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
