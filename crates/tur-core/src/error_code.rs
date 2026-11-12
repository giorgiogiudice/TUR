// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Stable, namespaced TUR error codes such as `TUR-STORAGE-NTFS-004`.
//!
//! A code has the form `TUR-<FAMILY>[-<TOPIC>...]-<NNN>`:
//!
//! * `FAMILY` is one of the registered families ([`Family`]).
//! * Zero or more `TOPIC` segments of upper-case letters and digits narrow it.
//! * `NNN` is a three-digit number.
//!
//! The meaning of a released code never changes (specification §29.3).

use std::borrow::Cow;
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
    /// The runtime manager: file inspection, runtime selection and
    /// application profiles, where a condition is not specific to one
    /// ecosystem. Conditions specific to Windows, macOS or Android software
    /// stay in their own families.
    Runtime,
    /// The application catalogue and the router. Registered for the
    /// catalogue's codes; see `services/tur-catalogue/src/codes.rs` for why
    /// they have not moved to it yet.
    Catalogue,
}

impl Family {
    /// Every registered family, in registry order.
    pub const ALL: [Family; 17] = [
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
        Family::Runtime,
        Family::Catalogue,
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
            Family::Runtime => "RUNTIME",
            Family::Catalogue => "CATALOGUE",
        }
    }

    fn from_segment(segment: &str) -> Option<Family> {
        Family::ALL
            .into_iter()
            .find(|family| family.as_str() == segment)
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A validated TUR error code.
///
/// ```
/// use tur_core::{ErrorCode, Family};
///
/// let code: ErrorCode = "TUR-STORAGE-NTFS-004".parse().unwrap();
/// assert_eq!(code.family(), Family::Storage);
/// assert_eq!(code.number(), 4);
/// assert_eq!(code.to_string(), "TUR-STORAGE-NTFS-004");
/// ```
///
/// A code that is known when the program is written can be built in a constant
/// with [`ErrorCode::from_static`], so that a misspelt code is a compile error.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ErrorCode {
    // Borrowed for a code built from a string literal, owned for a parsed one.
    // The two compare, order and hash by their text, so they are interchangeable.
    text: Cow<'static, str>,
    family: Family,
    number: u16,
}

impl ErrorCode {
    /// The code for a string literal, checked by the same rules as parsing.
    ///
    /// Used in a constant, the check happens when the program is compiled: a
    /// code that is not valid stops the build.
    ///
    /// ```
    /// use tur_core::{ErrorCode, Family};
    ///
    /// const NOT_MOUNTED: ErrorCode = ErrorCode::from_static("TUR-STORAGE-NTFS-004");
    ///
    /// assert_eq!(NOT_MOUNTED.family(), Family::Storage);
    /// assert_eq!(NOT_MOUNTED, "TUR-STORAGE-NTFS-004".parse().unwrap());
    /// ```
    ///
    /// ```compile_fail
    /// use tur_core::ErrorCode;
    ///
    /// const MISSPELT: ErrorCode = ErrorCode::from_static("TUR-STORAGE-4");
    /// assert_eq!(MISSPELT.number(), 4);
    /// ```
    ///
    /// # Panics
    ///
    /// If `text` is not a valid code. In a constant that is a compile error;
    /// called at run time it is a panic, so parse text that is not a literal
    /// with [`str::parse`] instead.
    pub const fn from_static(text: &'static str) -> ErrorCode {
        match validate(text.as_bytes()) {
            Some((family, number)) => ErrorCode {
                text: Cow::Borrowed(text),
                family,
                number,
            },
            None => panic!("not a valid TUR error code"),
        }
    }

    /// The family this code belongs to.
    pub fn family(&self) -> Family {
        self.family
    }

    /// The numeric suffix, `4` for `TUR-STORAGE-NTFS-004`.
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

/// Why a string is not a valid TUR error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCodeParseError {
    /// The code does not start with `TUR-`.
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
            Self::MissingPrefix => f.write_str("error code must start with \"TUR-\""),
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
        let rest = s
            .strip_prefix("TUR-")
            .ok_or(ErrorCodeParseError::MissingPrefix)?;
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
        let number: u16 = last
            .parse()
            .map_err(|_| ErrorCodeParseError::InvalidNumber(last.to_owned()))?;

        for topic in &segments[1..segments.len() - 1] {
            let valid = !topic.is_empty()
                && topic
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
            if !valid {
                return Err(ErrorCodeParseError::InvalidTopic((*topic).to_owned()));
            }
        }

        Ok(ErrorCode {
            text: Cow::Owned(s.to_owned()),
            family,
            number,
        })
    }
}

/// Whether `bytes[start..end]` equals `other`.
const fn segment_equals(bytes: &[u8], start: usize, end: usize, other: &[u8]) -> bool {
    if end - start != other.len() {
        return false;
    }
    let mut index = 0;
    while index < other.len() {
        if bytes[start + index] != other[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// The position in [`Family::ALL`] of the family whose segment is
/// `bytes[start..end]`, or the length of that list if there is none.
const fn family_index_of_segment(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut index = 0;
    while index < Family::ALL.len() {
        if segment_equals(bytes, start, end, Family::ALL[index].as_str().as_bytes()) {
            return index;
        }
        index += 1;
    }
    Family::ALL.len()
}

/// The family and number of a code, or `None` if `bytes` is not a valid code.
///
/// This states the rules of [`ErrorCode::from_str`] a second time, in a form
/// that can run while the program is compiled. A unit test holds the two
/// together over valid and invalid codes.
const fn validate(bytes: &[u8]) -> Option<(Family, u16)> {
    const PREFIX: &[u8] = b"TUR-";
    if bytes.len() < PREFIX.len() || !segment_equals(bytes, 0, PREFIX.len(), PREFIX) {
        return None;
    }

    // The family runs up to the next hyphen, and something must follow it.
    let family_start = PREFIX.len();
    let mut family_end = family_start;
    while family_end < bytes.len() && bytes[family_end] != b'-' {
        family_end += 1;
    }
    if family_end == bytes.len() {
        return None;
    }
    let family_index = family_index_of_segment(bytes, family_start, family_end);
    if family_index == Family::ALL.len() {
        return None;
    }
    let family = Family::ALL[family_index];

    // The number is the three digits after the last hyphen. There is a hyphen
    // at `family_end`, so the search stops there at the latest.
    let mut number_start = bytes.len();
    while bytes[number_start - 1] != b'-' {
        number_start -= 1;
    }
    if bytes.len() - number_start != 3 {
        return None;
    }
    let mut number: u16 = 0;
    let mut index = number_start;
    while index < bytes.len() {
        let digit = bytes[index];
        if !digit.is_ascii_digit() {
            return None;
        }
        number = number * 10 + (digit - b'0') as u16;
        index += 1;
    }

    // Whatever lies between the family and the number is topics, separated by
    // hyphens: each one or more of A-Z and 0-9.
    let topics_end = number_start - 1;
    if topics_end > family_end {
        let mut length = 0_usize;
        let mut index = family_end + 1;
        while index < topics_end {
            let byte = bytes[index];
            if byte == b'-' {
                if length == 0 {
                    return None;
                }
                length = 0;
            } else if byte.is_ascii_uppercase() || byte.is_ascii_digit() {
                length += 1;
            } else {
                return None;
            }
            index += 1;
        }
        if length == 0 {
            return None;
        }
    }

    Some((family, number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_code_with_topic() {
        let code: ErrorCode = "TUR-STORAGE-NTFS-004".parse().unwrap();
        assert_eq!(code.family(), Family::Storage);
        assert_eq!(code.number(), 4);
        assert_eq!(code.as_str(), "TUR-STORAGE-NTFS-004");
    }

    #[test]
    fn parses_code_without_topic() {
        let code: ErrorCode = "TUR-BOOT-150".parse().unwrap();
        assert_eq!(code.family(), Family::Boot);
        assert_eq!(code.number(), 150);
    }

    #[test]
    fn the_runtime_and_catalogue_families_are_registered() {
        let code: ErrorCode = "TUR-RUNTIME-PROFILE-004".parse().unwrap();
        assert_eq!(code.family(), Family::Runtime);
        assert_eq!(code.number(), 4);
        let code: ErrorCode = "TUR-CATALOGUE-011".parse().unwrap();
        assert_eq!(code.family(), Family::Catalogue);
        assert_eq!(ErrorCode::from_static("TUR-RUNTIME-MANAGER-001").family(), Family::Runtime);
        assert_eq!(ErrorCode::from_static("TUR-CATALOGUE-ENTRY-002").family(), Family::Catalogue);
        assert_eq!(Family::Runtime.as_str(), "RUNTIME");
        assert_eq!(Family::Catalogue.as_str(), "CATALOGUE");

        // The same words as a topic of another family are still topics: the
        // codes that existed before the families did keep their meaning.
        let parked: ErrorCode = "TUR-DESKTOP-CATALOGUE-011".parse().unwrap();
        assert_eq!(parked.family(), Family::Desktop);
        let windows: ErrorCode = "TUR-WINDOWS-RUNTIME-001".parse().unwrap();
        assert_eq!(windows.family(), Family::Windows);
        let android: ErrorCode = "TUR-ANDROID-RUNTIME-003".parse().unwrap();
        assert_eq!(android.family(), Family::Android);

        // The registry grew at its end: no earlier family changed place.
        assert_eq!(Family::ALL.len(), 17);
        assert_eq!(Family::ALL[14], Family::Ai);
        assert_eq!(Family::ALL[15], Family::Runtime);
        assert_eq!(Family::ALL[16], Family::Catalogue);
        // No family's segment is the beginning of another's, so the first
        // segment of a code names one family only.
        for family in Family::ALL {
            for other in Family::ALL {
                if family != other {
                    assert_ne!(family.as_str(), other.as_str());
                }
            }
        }
    }

    #[test]
    fn parses_code_with_several_topics() {
        let code: ErrorCode = "TUR-AI-POLICY-CLASS3-001".parse().unwrap();
        assert_eq!(code.family(), Family::Ai);
    }

    #[test]
    fn every_family_round_trips() {
        for family in Family::ALL {
            let text = format!("TUR-{family}-001");
            let code: ErrorCode = text.parse().unwrap();
            assert_eq!(code.family(), family);
            assert_eq!(code.to_string(), text);
        }
    }

    const POLICY_PROHIBITED: ErrorCode = ErrorCode::from_static("TUR-AI-POLICY-001");

    #[test]
    fn a_constant_code_equals_the_parsed_code() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let parsed: ErrorCode = "TUR-AI-POLICY-001".parse().unwrap();
        assert_eq!(POLICY_PROHIBITED, parsed);
        assert_eq!(POLICY_PROHIBITED.family(), Family::Ai);
        assert_eq!(POLICY_PROHIBITED.number(), 1);
        assert_eq!(POLICY_PROHIBITED.as_str(), "TUR-AI-POLICY-001");
        assert_eq!(POLICY_PROHIBITED.to_string(), parsed.to_string());
        assert_eq!(POLICY_PROHIBITED.cmp(&parsed), std::cmp::Ordering::Equal);
        assert_eq!(format!("{POLICY_PROHIBITED:?}"), format!("{parsed:?}"));

        let hash = |code: &ErrorCode| {
            let mut hasher = DefaultHasher::new();
            code.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(hash(&POLICY_PROHIBITED), hash(&parsed));
    }

    #[test]
    fn the_constant_constructor_accepts_what_parsing_accepts() {
        let valid = [
            "TUR-BOOT-150",
            "TUR-STORAGE-NTFS-004",
            "TUR-AI-POLICY-CLASS3-001",
            "TUR-AI-A-B-C-D-999",
            "TUR-DIAG-000",
            "TUR-DEVICE-USB3-042",
        ];
        for text in valid {
            let parsed: ErrorCode = text.parse().unwrap_or_else(|err| panic!("{text}: {err}"));
            let constant = ErrorCode::from_static(text);
            assert_eq!(constant, parsed, "{text}");
            assert_eq!(
                validate(text.as_bytes()),
                Some((parsed.family(), parsed.number())),
                "{text}"
            );
        }
        for family in Family::ALL {
            let text = format!("TUR-{family}-001");
            assert_eq!(validate(text.as_bytes()), Some((family, 1)), "{text}");
        }
    }

    #[test]
    fn the_constant_constructor_rejects_what_parsing_rejects() {
        let invalid = [
            "",
            "TUR",
            "TUR-",
            "TUR--",
            "TUR---",
            "TUR-004",
            "TUR--004",
            "TUR-BOOT",
            "TUR-BOOT-",
            "TUR-BOOT--",
            "TUR-BOOT-4",
            "TUR-BOOT-04",
            "TUR-BOOT-0004",
            "TUR-BOOT-00a",
            "TUR-BOOT-+04",
            "TUR-BOOT--004",
            "TUR-BOOT-A--004",
            "TUR-BOOT-A-004-",
            "TUR-BOOT-ntfs-004",
            "TUR-BOOT-NT FS-004",
            "TUR-BOOT-NTFS_1-004",
            "TUR-NOPE-004",
            "TUR-boot-004",
            "TUR-BOOTS-004",
            "TUR-BOO-004",
            "tur-BOOT-004",
            "STORAGE-004",
            " TUR-BOOT-004",
            "TUR-BOOT-004 ",
            "TUR-BOOT-004\n",
            "TUR-BOOT-\u{664}\u{664}\u{664}",
            "TUR-AI-P\u{d6}LICY-001",
        ];
        for text in invalid {
            assert!(text.parse::<ErrorCode>().is_err(), "{text:?} parses");
            assert_eq!(validate(text.as_bytes()), None, "{text:?}");
        }
    }

    #[test]
    #[should_panic(expected = "not a valid TUR error code")]
    fn the_constant_constructor_panics_at_run_time_on_an_invalid_code() {
        let _ = ErrorCode::from_static("TUR-BOOT-4");
    }

    #[test]
    fn rejects_malformed_codes() {
        use ErrorCodeParseError as E;
        assert_eq!("STORAGE-004".parse::<ErrorCode>(), Err(E::MissingPrefix));
        assert_eq!("TUR-004".parse::<ErrorCode>(), Err(E::TooShort));
        assert_eq!(
            "TUR-NOPE-004".parse::<ErrorCode>(),
            Err(E::UnknownFamily("NOPE".into()))
        );
        assert_eq!(
            "TUR-BOOT-4".parse::<ErrorCode>(),
            Err(E::InvalidNumber("4".into()))
        );
        assert_eq!(
            "TUR-BOOT-0004".parse::<ErrorCode>(),
            Err(E::InvalidNumber("0004".into()))
        );
        assert_eq!(
            "TUR-BOOT-ntfs-004".parse::<ErrorCode>(),
            Err(E::InvalidTopic("ntfs".into()))
        );
        assert_eq!(
            "TUR-BOOT--004".parse::<ErrorCode>(),
            Err(E::InvalidTopic(String::new()))
        );
    }
}
