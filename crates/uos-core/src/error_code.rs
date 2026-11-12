// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Stable, namespaced UOS error codes such as `UOS-STORAGE-NTFS-004`.
//!
//! A code has the form `UOS-<FAMILY>[-<TOPIC>...]-<NNN>`:
//!
//! * `FAMILY` is one of the registered families ([`Family`]).
//! * Zero or more `TOPIC` segments of upper-case letters and digits narrow it.
//! * `NNN` is a three-digit number.
//!
//! The meaning of a released code never changes (specification §29.3).

use std::fmt;
use std::str::FromStr;

/// The registered error-code families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    /// Boot stages and boot health.
    Boot,
    /// Installer.
    Install,
    /// OS updates and rollback.
    Update,
    /// Volumes, filesystems and mounting.
    Storage,
    /// Devices, drivers and firmware.
    Device,
    /// GPU and display.
    Graphics,
    /// Audio.
    Audio,
    /// Networking.
    Network,
    /// Windows compatibility runtime.
    Windows,
    /// macOS compatibility runtime.
    Darwin,
    /// Android compatibility runtime.
    Android,
    /// Desktop integration.
    Desktop,
    /// Security policy and isolation.
    Security,
    /// Diagnostics and support bundles.
    Diag,
    /// AI control layer.
    Ai,
}

impl Family {
    /// Every registered family, in registry order.
    pub const ALL: [Family; 15] = [
        Family::Boot,
        Family::Install,
        Family::Update,
        Family::Storage,
        Family::Device,
        Family::Graphics,
        Family::Audio,
        Family::Network,
        Family::Windows,
        Family::Darwin,
        Family::Android,
        Family::Desktop,
        Family::Security,
        Family::Diag,
        Family::Ai,
    ];

    /// The family's segment as it appears in a code, for example `STORAGE`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Family::Boot => "BOOT",
            Family::Install => "INSTALL",
            Family::Update => "UPDATE",
            Family::Storage => "STORAGE",
            Family::Device => "DEVICE",
            Family::Graphics => "GRAPHICS",
            Family::Audio => "AUDIO",
            Family::Network => "NETWORK",
            Family::Windows => "WINDOWS",
            Family::Darwin => "DARWIN",
            Family::Android => "ANDROID",
            Family::Desktop => "DESKTOP",
            Family::Security => "SECURITY",
            Family::Diag => "DIAG",
            Family::Ai => "AI",
        }
    }

    fn from_segment(segment: &str) -> Option<Family> {
        Family::ALL.into_iter().find(|family| family.as_str() == segment)
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A validated UOS error code.
///
/// ```
/// use uos_core::{ErrorCode, Family};
///
/// let code: ErrorCode = "UOS-STORAGE-NTFS-004".parse().unwrap();
/// assert_eq!(code.family(), Family::Storage);
/// assert_eq!(code.number(), 4);
/// assert_eq!(code.to_string(), "UOS-STORAGE-NTFS-004");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ErrorCode {
    text: String,
    family: Family,
    number: u16,
}

impl ErrorCode {
    /// The family this code belongs to.
    pub fn family(&self) -> Family {
        self.family
    }

    /// The numeric suffix, `4` for `UOS-STORAGE-NTFS-004`.
    pub fn number(&self) -> u16 {
        self.number
    }

    /// The code as text.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Why a string is not a valid UOS error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCodeParseError {
    /// The code does not start with `UOS-`.
    MissingPrefix,
    /// The family segment is not registered.
    UnknownFamily(String),
    /// A topic segment is empty or has characters other than `A-Z` and `0-9`.
    InvalidTopic(String),
    /// The final segment is not exactly three digits.
    InvalidNumber(String),
    /// The code has too few segments.
    TooShort,
}

impl fmt::Display for ErrorCodeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPrefix => f.write_str("error code must start with \"UOS-\""),
            Self::UnknownFamily(s) => write!(f, "unknown error-code family \"{s}\""),
            Self::InvalidTopic(s) => write!(f, "invalid topic segment \"{s}\""),
            Self::InvalidNumber(s) => write!(f, "final segment \"{s}\" is not three digits"),
            Self::TooShort => f.write_str("error code needs a family and a number"),
        }
    }
}

impl std::error::Error for ErrorCodeParseError {}

impl FromStr for ErrorCode {
    type Err = ErrorCodeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rest = s.strip_prefix("UOS-").ok_or(ErrorCodeParseError::MissingPrefix)?;
        let segments: Vec<&str> = rest.split('-').collect();
        if segments.len() < 2 {
            return Err(ErrorCodeParseError::TooShort);
        }

        let family = Family::from_segment(segments[0])
            .ok_or_else(|| ErrorCodeParseError::UnknownFamily(segments[0].to_owned()))?;

        let last = segments[segments.len() - 1];
        if last.len() != 3 || !last.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ErrorCodeParseError::InvalidNumber(last.to_owned()));
        }
        let number: u16 =
            last.parse().map_err(|_| ErrorCodeParseError::InvalidNumber(last.to_owned()))?;

        for topic in &segments[1..segments.len() - 1] {
            let valid = !topic.is_empty()
                && topic.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
            if !valid {
                return Err(ErrorCodeParseError::InvalidTopic((*topic).to_owned()));
            }
        }

        Ok(ErrorCode {
            text: s.to_owned(),
            family,
            number,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_code_with_topic() {
        let code: ErrorCode = "UOS-STORAGE-NTFS-004".parse().unwrap();
        assert_eq!(code.family(), Family::Storage);
        assert_eq!(code.number(), 4);
        assert_eq!(code.as_str(), "UOS-STORAGE-NTFS-004");
    }

    #[test]
    fn parses_code_without_topic() {
        let code: ErrorCode = "UOS-BOOT-150".parse().unwrap();
        assert_eq!(code.family(), Family::Boot);
        assert_eq!(code.number(), 150);
    }

    #[test]
    fn parses_code_with_several_topics() {
        let code: ErrorCode = "UOS-AI-POLICY-CLASS3-001".parse().unwrap();
        assert_eq!(code.family(), Family::Ai);
    }

    #[test]
    fn every_family_round_trips() {
        for family in Family::ALL {
            let text = format!("UOS-{family}-001");
            let code: ErrorCode = text.parse().unwrap();
            assert_eq!(code.family(), family);
            assert_eq!(code.to_string(), text);
        }
    }

    #[test]
    fn rejects_malformed_codes() {
        use ErrorCodeParseError as E;
        assert_eq!("STORAGE-004".parse::<ErrorCode>(), Err(E::MissingPrefix));
        assert_eq!("UOS-004".parse::<ErrorCode>(), Err(E::TooShort));
        assert_eq!("UOS-NOPE-004".parse::<ErrorCode>(), Err(E::UnknownFamily("NOPE".into())));
        assert_eq!("UOS-BOOT-4".parse::<ErrorCode>(), Err(E::InvalidNumber("4".into())));
        assert_eq!("UOS-BOOT-0004".parse::<ErrorCode>(), Err(E::InvalidNumber("0004".into())));
        assert_eq!("UOS-BOOT-ntfs-004".parse::<ErrorCode>(), Err(E::InvalidTopic("ntfs".into())));
        assert_eq!("UOS-BOOT--004".parse::<ErrorCode>(), Err(E::InvalidTopic(String::new())));
    }
}
