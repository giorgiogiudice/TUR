// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Reading Windows shortcut files (`.lnk`, the Shell Link format).
//!
//! An installer tells the user where the application is by putting
//! shortcuts in the Start menu. Wine writes them as ordinary shortcut files
//! inside the prefix. Reading where they point is how the main program of an
//! installed application is found.
//!
//! Only what is needed is read: the target path, the arguments, the working
//! directory, the description and the icon location. A shortcut is written
//! by an installer and is not trusted: every offset is checked, and the
//! strings that are kept are stripped of control characters and bounded.

use crate::arch::Endianness;
use crate::reader::{u16_at, u32_at};

const LITTLE: Endianness = Endianness::Little;

/// The size of the header, which is also its first field.
const HEADER_LEN: usize = 76;
/// The class identifier every shortcut carries,
/// `{00021401-0000-0000-C000-000000000046}`.
const CLSID_SHELL_LINK: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];
/// Characters kept from one string of a shortcut.
const MAX_STRING_CHARS: usize = 1024;

const HAS_ID_LIST: u32 = 0x01;
const HAS_LINK_INFO: u32 = 0x02;
const HAS_NAME: u32 = 0x04;
const HAS_RELATIVE_PATH: u32 = 0x08;
const HAS_WORKING_DIR: u32 = 0x10;
const HAS_ARGUMENTS: u32 = 0x20;
const HAS_ICON_LOCATION: u32 = 0x40;
const IS_UNICODE: u32 = 0x80;

/// What a shortcut says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellLink {
    /// The file the shortcut points to, as a Windows path.
    pub target: Option<String>,
    /// The arguments it passes.
    pub arguments: Option<String>,
    /// The directory it starts the target in.
    pub working_directory: Option<String>,
    /// Its description.
    pub description: Option<String>,
    /// The file its icon is taken from.
    pub icon_location: Option<String>,
}

fn clean(text: &str) -> Option<String> {
    let cleaned: String = text
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_STRING_CHARS)
        .collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// A NUL-terminated string of single bytes at `at`. The bytes are in the
/// Windows code page of whoever wrote the file; they are read as Latin-1,
/// which is exact for the ASCII paths installers use.
fn narrow_string(data: &[u8], at: usize) -> Option<String> {
    let text: String = data
        .get(at..)?
        .iter()
        .take_while(|byte| **byte != 0)
        .take(MAX_STRING_CHARS)
        .map(|byte| char::from(*byte))
        .collect();
    clean(&text)
}

/// A NUL-terminated UTF-16 string at `at`.
fn wide_string(data: &[u8], at: usize) -> Option<String> {
    let units: Vec<u16> = data
        .get(at..)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .take(MAX_STRING_CHARS)
        .collect();
    clean(&String::from_utf16_lossy(&units))
}

/// Parses a shortcut file. Returns `None` when `data` is not one or is cut
/// short before its strings.
pub fn parse_shell_link(data: &[u8]) -> Option<ShellLink> {
    if u32_at(data, 0, LITTLE)? != 0x4c || data.get(4..20)? != CLSID_SHELL_LINK {
        return None;
    }
    let flags = u32_at(data, 20, LITTLE)?;
    let mut at = HEADER_LEN;
    data.get(..at)?;

    if flags & HAS_ID_LIST != 0 {
        let size = usize::from(u16_at(data, at, LITTLE)?);
        at = at.checked_add(2)?.checked_add(size)?;
    }

    let mut link = ShellLink::default();
    if flags & HAS_LINK_INFO != 0 {
        let size = usize::try_from(u32_at(data, at, LITTLE)?).ok()?;
        let header_size = u32_at(data, at + 4, LITTLE)?;
        let info_flags = u32_at(data, at + 8, LITTLE)?;
        let end = at.checked_add(size)?;
        let info = data.get(at..end)?;
        // Bit 0: the target is on a local volume and its path follows.
        if info_flags & 1 != 0 {
            // Newer writers add the path in UTF-16 as well; it is exact
            // where the single-byte one is not.
            let wide = (header_size >= 0x24)
                .then(|| u32_at(info, 28, LITTLE))
                .flatten()
                .and_then(|offset| usize::try_from(offset).ok())
                .filter(|offset| *offset != 0)
                .and_then(|offset| wide_string(info, offset));
            let narrow = u32_at(info, 16, LITTLE)
                .and_then(|offset| usize::try_from(offset).ok())
                .filter(|offset| *offset != 0)
                .and_then(|offset| narrow_string(info, offset));
            link.target = wide.or(narrow);
        }
        at = end;
    }

    // The strings, each present when its flag is set, in this order.
    let unicode = flags & IS_UNICODE != 0;
    let mut relative_path = None;
    for flag in [
        HAS_NAME,
        HAS_RELATIVE_PATH,
        HAS_WORKING_DIR,
        HAS_ARGUMENTS,
        HAS_ICON_LOCATION,
    ] {
        if flags & flag == 0 {
            continue;
        }
        let count = usize::from(u16_at(data, at, LITTLE)?);
        let length = if unicode {
            count.checked_mul(2)?
        } else {
            count
        };
        let start = at.checked_add(2)?;
        let end = start.checked_add(length)?;
        let bytes = data.get(start..end)?;
        // A string's count may or may not include a terminator; Wine's
        // does. Either way the string ends at the first NUL.
        let text = if unicode {
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .take_while(|unit| *unit != 0)
                .collect();
            clean(&String::from_utf16_lossy(&units))
        } else {
            narrow_string(bytes, 0)
        };
        match flag {
            HAS_NAME => link.description = text,
            HAS_RELATIVE_PATH => relative_path = text,
            HAS_WORKING_DIR => link.working_directory = text,
            HAS_ARGUMENTS => link.arguments = text,
            _ => link.icon_location = text,
        }
        at = end;
    }
    // A relative path alone does not say where the target is; it is only
    // used when it is in fact absolute.
    if link.target.is_none()
        && let Some(path) = relative_path
        && is_drive_path(&path)
    {
        link.target = Some(path);
    }
    Some(link)
}

/// Whether `path` starts with a drive letter, a colon and a backslash.
pub(crate) fn is_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && matches!(bytes.get(2), Some(b'\\' | b'/'))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// What goes into a test shortcut.
    #[derive(Default)]
    pub(crate) struct LinkSpec<'a> {
        pub(crate) target: Option<&'a str>,
        pub(crate) wide_target: Option<&'a str>,
        pub(crate) id_list: bool,
        pub(crate) description: Option<&'a str>,
        pub(crate) relative_path: Option<&'a str>,
        pub(crate) working_directory: Option<&'a str>,
        pub(crate) arguments: Option<&'a str>,
        pub(crate) icon_location: Option<&'a str>,
        pub(crate) narrow_strings: bool,
    }

    impl LinkSpec<'_> {
        /// A shortcut to `target`, as Wine writes one.
        pub(crate) fn to(target: &str) -> LinkSpec<'_> {
            LinkSpec {
                target: Some(target),
                id_list: true,
                ..LinkSpec::default()
            }
        }

        /// The file's bytes, laid out the way Wine's shell32 writes them.
        pub(crate) fn build(&self) -> Vec<u8> {
            let mut flags = 0u32;
            if !self.narrow_strings {
                flags |= IS_UNICODE;
            }
            let strings = [
                (HAS_NAME, self.description),
                (HAS_RELATIVE_PATH, self.relative_path),
                (HAS_WORKING_DIR, self.working_directory),
                (HAS_ARGUMENTS, self.arguments),
                (HAS_ICON_LOCATION, self.icon_location),
            ];
            for (flag, text) in strings {
                if text.is_some() {
                    flags |= flag;
                }
            }
            if self.id_list {
                flags |= HAS_ID_LIST;
            }
            if self.target.is_some() || self.wide_target.is_some() {
                flags |= HAS_LINK_INFO;
            }
            let mut out = vec![0u8; HEADER_LEN];
            out[..4].copy_from_slice(&0x4cu32.to_le_bytes());
            out[4..20].copy_from_slice(&CLSID_SHELL_LINK);
            out[20..24].copy_from_slice(&flags.to_le_bytes());
            if self.id_list {
                // One item of four bytes and the terminating empty item.
                out.extend_from_slice(&8u16.to_le_bytes());
                out.extend_from_slice(&[6, 0, 0x1f, 0x50, 0, 0, 0, 0]);
            }
            if self.target.is_some() || self.wide_target.is_some() {
                let narrow = self.target.unwrap_or("");
                let header_size: u32 = if self.wide_target.is_some() {
                    0x24
                } else {
                    0x1c
                };
                let volume = [0u8; 17];
                let header_len = usize::try_from(header_size).unwrap();
                let path_at = header_len + volume.len();
                let mut body = Vec::new();
                body.extend_from_slice(&volume);
                body.extend(narrow.bytes());
                body.push(0);
                // The final path: empty.
                let final_at = header_len + body.len();
                body.push(0);
                let wide_at = header_len + body.len();
                if let Some(wide) = self.wide_target {
                    for unit in wide.encode_utf16() {
                        body.extend_from_slice(&unit.to_le_bytes());
                    }
                    body.extend_from_slice(&[0, 0]);
                }
                let total = header_len + body.len();
                for value in [
                    u32::try_from(total).unwrap(),
                    header_size,
                    1,
                    header_size,
                    u32::try_from(path_at).unwrap(),
                    0,
                    u32::try_from(final_at).unwrap(),
                ] {
                    out.extend_from_slice(&value.to_le_bytes());
                }
                if self.wide_target.is_some() {
                    out.extend_from_slice(&u32::try_from(wide_at).unwrap().to_le_bytes());
                    out.extend_from_slice(&u32::try_from(wide_at).unwrap().to_le_bytes());
                }
                out.extend_from_slice(&body);
            }
            for (_, text) in strings {
                let Some(text) = text else {
                    continue;
                };
                if self.narrow_strings {
                    out.extend_from_slice(&u16::try_from(text.len()).unwrap().to_le_bytes());
                    out.extend(text.bytes());
                } else {
                    // Wine counts the terminator and writes it.
                    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
                    out.extend_from_slice(&u16::try_from(units.len()).unwrap().to_le_bytes());
                    for unit in units {
                        out.extend_from_slice(&unit.to_le_bytes());
                    }
                }
            }
            out.extend_from_slice(&[0u8; 4]);
            out
        }
    }

    #[test]
    fn a_shortcut_written_the_way_wine_writes_one_is_read() {
        let bytes = LinkSpec {
            description: Some("7-Zip File Manager"),
            working_directory: Some("C:\\Program Files\\7-Zip"),
            arguments: Some("-open \"C:\\some file\""),
            icon_location: Some("C:\\Program Files\\7-Zip\\7zFM.exe"),
            ..LinkSpec::to("C:\\Program Files\\7-Zip\\7zFM.exe")
        }
        .build();
        let link = parse_shell_link(&bytes).unwrap();
        assert_eq!(
            link,
            ShellLink {
                target: Some("C:\\Program Files\\7-Zip\\7zFM.exe".to_owned()),
                arguments: Some("-open \"C:\\some file\"".to_owned()),
                working_directory: Some("C:\\Program Files\\7-Zip".to_owned()),
                description: Some("7-Zip File Manager".to_owned()),
                icon_location: Some("C:\\Program Files\\7-Zip\\7zFM.exe".to_owned()),
            }
        );
    }

    #[test]
    fn the_wide_path_is_preferred_and_narrow_strings_are_read() {
        let bytes = LinkSpec {
            wide_target: Some("C:\\Programme\\Caf\u{e9}\\caf\u{e9}.exe"),
            ..LinkSpec::to("C:\\Programme\\Caf?\\caf?.exe")
        }
        .build();
        assert_eq!(
            parse_shell_link(&bytes).unwrap().target.as_deref(),
            Some("C:\\Programme\\Caf\u{e9}\\caf\u{e9}.exe")
        );
        let bytes = LinkSpec {
            narrow_strings: true,
            id_list: false,
            arguments: Some("/safe"),
            ..LinkSpec::to("C:\\app\\app.exe")
        }
        .build();
        let link = parse_shell_link(&bytes).unwrap();
        assert_eq!(link.target.as_deref(), Some("C:\\app\\app.exe"));
        assert_eq!(link.arguments.as_deref(), Some("/safe"));
    }

    #[test]
    fn a_shortcut_without_a_location_uses_an_absolute_relative_path_only() {
        let bytes = LinkSpec {
            relative_path: Some("C:\\app\\app.exe"),
            ..LinkSpec::default()
        }
        .build();
        assert_eq!(
            parse_shell_link(&bytes).unwrap().target.as_deref(),
            Some("C:\\app\\app.exe")
        );
        let bytes = LinkSpec {
            relative_path: Some("..\\app\\app.exe"),
            ..LinkSpec::default()
        }
        .build();
        assert_eq!(parse_shell_link(&bytes).unwrap().target, None);
        assert!(is_drive_path("C:\\x"));
        assert!(is_drive_path("d:/x"));
        for path in ["", "C:", "C:x", "\\\\server\\share", "1:\\x", "/usr/bin"] {
            assert!(!is_drive_path(path), "{path}");
        }
    }

    #[test]
    fn control_characters_are_dropped_from_every_string() {
        let bytes = LinkSpec {
            description: Some("Name\u{1b}[31m\nsecond line"),
            ..LinkSpec::to("C:\\app\\a\u{7}pp.exe")
        }
        .build();
        let link = parse_shell_link(&bytes).unwrap();
        assert_eq!(link.description.as_deref(), Some("Name[31msecond line"));
        assert_eq!(link.target.as_deref(), Some("C:\\app\\app.exe"));
    }

    #[test]
    fn what_is_not_a_shortcut_or_is_cut_short_is_refused_without_a_panic() {
        assert_eq!(parse_shell_link(b""), None);
        assert_eq!(parse_shell_link(b"MZ"), None);
        assert_eq!(parse_shell_link(&[0u8; 200]), None);
        let whole = LinkSpec {
            description: Some("Example"),
            arguments: Some("--flag"),
            ..LinkSpec::to("C:\\Program Files\\Example\\example.exe")
        }
        .build();
        assert!(parse_shell_link(&whole).is_some());
        // Everything up to the trailing terminator is needed.
        for length in 0..whole.len() - 4 {
            assert_eq!(parse_shell_link(&whole[..length]), None, "{length} bytes");
        }
        // Sizes that point past the end.
        let mut bad = whole.clone();
        bad[HEADER_LEN..HEADER_LEN + 2].copy_from_slice(&0xffffu16.to_le_bytes());
        assert_eq!(parse_shell_link(&bad), None);
        let mut bad = whole.clone();
        bad[HEADER_LEN + 10..HEADER_LEN + 14].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        assert_eq!(parse_shell_link(&bad), None);
        // A wrong class identifier.
        let mut bad = whole;
        bad[4] = 0;
        assert_eq!(parse_shell_link(&bad), None);
    }
}
