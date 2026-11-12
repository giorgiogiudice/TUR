// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Reading the uninstall entries out of a prefix's registry files.
//!
//! Wine keeps a prefix's registry as text: `system.reg` for the machine hive
//! and `user.reg` for the user's. An installer records what it installed
//! under `Software\Microsoft\Windows\CurrentVersion\Uninstall\<key>`, in
//! either hive, and for 32-bit software under `Software\Wow6432Node\...`.
//! Those entries are the installer's own account of the application: its
//! name, version, publisher and how to remove it.
//!
//! The files are written by the Wine server on behalf of the installer, so
//! their content is untrusted. They are read with a size cap, parsed without
//! executing or expanding anything, and every string that is kept is
//! stripped of control characters and bounded.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Serialize;

/// Bytes read from one registry file. A fresh prefix's `system.reg` is a few
/// megabytes.
const MAX_REGISTRY_BYTES: u64 = 64 * 1024 * 1024;
/// Uninstall entries kept from one prefix.
const MAX_ENTRIES: usize = 256;
/// Characters kept from one registry string.
const MAX_VALUE_CHARS: usize = 1024;

/// The two places uninstall entries are kept, as key paths in the escaped
/// form the registry files use, in lower case.
const UNINSTALL_ROOTS: [(&str, bool); 2] = [
    (
        "software\\\\microsoft\\\\windows\\\\currentversion\\\\uninstall\\\\",
        false,
    ),
    (
        "software\\\\wow6432node\\\\microsoft\\\\windows\\\\currentversion\\\\uninstall\\\\",
        true,
    ),
];

/// A registry hive of a prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Hive {
    /// `HKEY_LOCAL_MACHINE`, stored in `system.reg`.
    Machine,
    /// `HKEY_CURRENT_USER`, stored in `user.reg`.
    User,
}

impl Hive {
    /// The hive's short Windows name.
    pub const fn label(self) -> &'static str {
        match self {
            Hive::Machine => "HKLM",
            Hive::User => "HKCU",
        }
    }
}

/// One uninstall entry: what an installer recorded about an application.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UninstallEntry {
    /// The hive the entry is in.
    pub hive: Option<Hive>,
    /// The entry's key name: a product code in braces for a Windows
    /// Installer package, otherwise a name the installer chose.
    pub key: String,
    /// Whether the entry is under `Wow6432Node`, where 32-bit software is
    /// recorded.
    pub wow64: bool,
    /// `DisplayName`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// `DisplayVersion`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_version: Option<String>,
    /// `Publisher`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// `UninstallString`: the command line of the uninstaller.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uninstall_string: Option<String>,
    /// `InstallLocation`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_location: Option<String>,
    /// `DisplayIcon`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_icon: Option<String>,
    /// Whether the entry belongs to a Windows Installer package.
    pub windows_installer: bool,
    /// Whether the entry is marked as a system component, which Windows
    /// does not list.
    pub system_component: bool,
}

impl UninstallEntry {
    /// A name that tells entries of one prefix apart: the hive, whether it
    /// is the 32-bit view, and the key.
    pub fn identity(&self) -> String {
        format!(
            "{}{}\\{}",
            self.hive.map_or("?", Hive::label),
            if self.wow64 { "\\Wow6432Node" } else { "" },
            self.key
        )
    }
}

/// Undoes the escaping of the registry files: `\\`, `\"`, the C escapes,
/// `\x` followed by up to four hexadecimal digits for a UTF-16 unit, and a
/// backslash followed by up to three octal digits. Control characters are
/// dropped and the result is bounded.
fn unescape(text: &str) -> String {
    let mut units: Vec<u16> = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' {
            let mut buffer = [0u16; 2];
            units.extend_from_slice(character.encode_utf16(&mut buffer));
            continue;
        }
        let Some(escaped) = characters.next() else {
            break;
        };
        match escaped {
            'x' => {
                let mut value: u32 = 0;
                let mut digits = 0;
                while digits < 4 {
                    let Some(digit) = characters.peek().and_then(|next| next.to_digit(16)) else {
                        break;
                    };
                    value = value * 16 + digit;
                    digits += 1;
                    characters.next();
                }
                units.push(u16::try_from(value).unwrap_or(0xfffd));
            }
            '0'..='7' => {
                let mut value = escaped.to_digit(8).unwrap_or(0);
                let mut digits = 1;
                while digits < 3 {
                    let Some(digit) = characters.peek().and_then(|next| next.to_digit(8)) else {
                        break;
                    };
                    value = value * 8 + digit;
                    digits += 1;
                    characters.next();
                }
                units.push(u16::try_from(value).unwrap_or(0xfffd));
            }
            // The C escapes all stand for control characters, which are not
            // kept.
            'a' | 'b' | 'e' | 'f' | 'n' | 'r' | 't' | 'v' => {}
            other => {
                let mut buffer = [0u16; 2];
                units.extend_from_slice(other.encode_utf16(&mut buffer));
            }
        }
    }
    String::from_utf16_lossy(&units)
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_VALUE_CHARS)
        .collect()
}

/// Splits a quoted string off the start of `text`: returns what is between
/// the quotes, still escaped, and what follows the closing quote.
fn quoted(text: &str) -> Option<(&str, &str)> {
    let inner = text.strip_prefix('"')?;
    let mut escaped = false;
    for (at, character) in inner.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some((inner.get(..at)?, inner.get(at + 1..)?));
        }
    }
    None
}

/// The uninstall key a `[key] time` header line names, when it names one
/// directly under an uninstall root.
fn uninstall_key(line: &str) -> Option<(String, bool)> {
    let inner = line.strip_prefix('[')?;
    let inner = inner.get(..inner.rfind(']')?)?;
    let lower = inner.to_ascii_lowercase();
    UNINSTALL_ROOTS.iter().find_map(|(root, wow64)| {
        if !lower.starts_with(root) {
            return None;
        }
        // Lower-casing ASCII keeps byte offsets.
        let name = inner.get(root.len()..)?;
        // A deeper key belongs to an entry; it is not one.
        if name.is_empty() || name.contains("\\\\") {
            return None;
        }
        let name = unescape(name);
        (!name.is_empty()).then_some((name, *wow64))
    })
}

/// A value of a registry line, as far as this module reads values.
enum Value {
    Text(String),
    Number(u32),
}

/// Parses `"Name"=value`. Only strings and double words are read.
fn value_line(line: &str) -> Option<(String, Value)> {
    let (name, rest) = quoted(line)?;
    let rest = rest.strip_prefix('=')?;
    let name = unescape(name);
    if let Some(number) = rest.strip_prefix("dword:") {
        let number = u32::from_str_radix(number.trim(), 16).ok()?;
        return Some((name, Value::Number(number)));
    }
    // Plain strings, and expandable strings, which are stored the same way
    // and are kept as they are: nothing is expanded.
    let text = rest
        .strip_prefix("str(2):")
        .or_else(|| rest.strip_prefix("str(1):"))
        .unwrap_or(rest);
    let (text, _) = quoted(text)?;
    Some((name, Value::Text(unescape(text))))
}

/// Reads the uninstall entries out of the text of a registry file.
pub fn parse_uninstall_entries(text: &str, hive: Hive) -> Vec<UninstallEntry> {
    let mut entries: Vec<UninstallEntry> = Vec::new();
    let mut current: Option<UninstallEntry> = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with('[') {
            if let Some(finished) = current.take() {
                entries.push(finished);
            }
            if entries.len() >= MAX_ENTRIES {
                break;
            }
            current = uninstall_key(line).map(|(key, wow64)| UninstallEntry {
                hive: Some(hive),
                key,
                wow64,
                ..UninstallEntry::default()
            });
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        let Some((name, value)) = value_line(line) else {
            continue;
        };
        let name = name.to_ascii_lowercase();
        match value {
            Value::Text(text) => {
                let slot = match name.as_str() {
                    "displayname" => &mut entry.display_name,
                    "displayversion" => &mut entry.display_version,
                    "publisher" => &mut entry.publisher,
                    "uninstallstring" => &mut entry.uninstall_string,
                    "installlocation" => &mut entry.install_location,
                    "displayicon" => &mut entry.display_icon,
                    _ => continue,
                };
                if !text.trim().is_empty() {
                    *slot = Some(text);
                }
            }
            Value::Number(number) => match name.as_str() {
                "windowsinstaller" => entry.windows_installer = number != 0,
                "systemcomponent" => entry.system_component = number != 0,
                _ => {}
            },
        }
    }
    if let Some(finished) = current.take()
        && entries.len() < MAX_ENTRIES
    {
        entries.push(finished);
    }
    entries
}

/// Reads at most [`MAX_REGISTRY_BYTES`] of a registry file as text. A file
/// that is missing, not a regular file or unreadable reads as empty: a
/// prefix without a registry has no uninstall entries.
fn read_registry(path: &Path) -> String {
    let regular = std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file());
    if !regular {
        return String::new();
    }
    let mut bytes = Vec::new();
    let read =
        File::open(path).and_then(|file| file.take(MAX_REGISTRY_BYTES).read_to_end(&mut bytes));
    if read.is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Reads the uninstall entries of the prefix at `prefix`, from both hives,
/// in a stable order.
pub fn read_uninstall_entries(prefix: &Path) -> Vec<UninstallEntry> {
    let mut entries =
        parse_uninstall_entries(&read_registry(&prefix.join("system.reg")), Hive::Machine);
    entries.extend(parse_uninstall_entries(
        &read_registry(&prefix.join("user.reg")),
        Hive::User,
    ));
    entries.sort_by_key(UninstallEntry::identity);
    entries
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testdata::TempDir;

    /// A machine hive as Wine writes it, with entries of the kinds the
    /// installers of the test matrix create.
    pub(crate) const SYSTEM_REG: &str = r#"WINE REGISTRY Version 2
;; All keys relative to \\Machine

#arch=win64

[Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall] 1794478500
#time=1dc5d0a0b0c0d0e

[Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\7-Zip] 1794478501
#time=1dc5d0a0b0c0d0f
"DisplayIcon"="C:\\Program Files\\7-Zip\\7zFM.exe"
"DisplayName"="7-Zip 26.06 (x64)"
"DisplayVersion"="26.06"
"EstimatedSize"=dword:0000166c
"InstallLocation"="C:\\Program Files\\7-Zip\\"
"NoModify"=dword:00000001
"Publisher"="Igor Pavlov"
"UninstallString"="\"C:\\Program Files\\7-Zip\\Uninstall.exe\""

[Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\7-Zip\\Deeper] 1794478501
"DisplayName"="not an entry"

[Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{E5C4B1A0-0000-4000-8000-0123456789AB}] 1794478502
"DisplayName"="PuTTY release 0.85 (64-bit)"
"DisplayVersion"="0.85.0.0"
"Publisher"="Simon Tatham"
"UninstallString"=str(2):"MsiExec.exe /X{E5C4B1A0-0000-4000-8000-0123456789AB}"
"WindowsInstaller"=dword:00000001

[Software\\Wow6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Caf\xe9 Tool] 1794478503
"DisplayName"="Caf\xe9 \x263a Tool"
"SystemComponent"=dword:00000001
"UninstallString"="C:\\Program Files (x86)\\Cafe\\unins000.exe"

[Software\\Other\\Key] 1794478504
"DisplayName"="unrelated"
"#;

    #[test]
    fn uninstall_entries_are_read_from_a_hive() {
        let entries = parse_uninstall_entries(SYSTEM_REG, Hive::Machine);
        let keys: Vec<&str> = entries.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "7-Zip",
                "{E5C4B1A0-0000-4000-8000-0123456789AB}",
                "Caf\u{e9} Tool"
            ]
        );
        let seven_zip = &entries[0];
        assert_eq!(seven_zip.hive, Some(Hive::Machine));
        assert!(!seven_zip.wow64);
        assert_eq!(seven_zip.display_name.as_deref(), Some("7-Zip 26.06 (x64)"));
        assert_eq!(seven_zip.display_version.as_deref(), Some("26.06"));
        assert_eq!(seven_zip.publisher.as_deref(), Some("Igor Pavlov"));
        assert_eq!(
            seven_zip.uninstall_string.as_deref(),
            Some("\"C:\\Program Files\\7-Zip\\Uninstall.exe\"")
        );
        assert_eq!(
            seven_zip.install_location.as_deref(),
            Some("C:\\Program Files\\7-Zip\\")
        );
        assert_eq!(
            seven_zip.display_icon.as_deref(),
            Some("C:\\Program Files\\7-Zip\\7zFM.exe")
        );
        assert!(!seven_zip.windows_installer);
        assert_eq!(seven_zip.identity(), "HKLM\\7-Zip");

        let putty = &entries[1];
        assert!(putty.windows_installer);
        assert_eq!(
            putty.uninstall_string.as_deref(),
            Some("MsiExec.exe /X{E5C4B1A0-0000-4000-8000-0123456789AB}")
        );

        let cafe = &entries[2];
        assert!(cafe.wow64);
        assert!(cafe.system_component);
        assert_eq!(
            cafe.display_name.as_deref(),
            Some("Caf\u{e9} \u{263a} Tool")
        );
        assert_eq!(cafe.identity(), "HKLM\\Wow6432Node\\Caf\u{e9} Tool");
    }

    #[test]
    fn escapes_are_undone_and_control_characters_dropped() {
        assert_eq!(unescape("plain"), "plain");
        assert_eq!(unescape(r"a\\b"), "a\\b");
        assert_eq!(unescape(r#"say \"hi\""#), "say \"hi\"");
        assert_eq!(unescape(r"caf\xe9"), "caf\u{e9}");
        // Four digits are taken when four follow.
        assert_eq!(unescape(r"\x263a!"), "\u{263a}!");
        // A surrogate pair written as two units is one character.
        assert_eq!(unescape(r"\xd83d\xde00"), "\u{1f600}");
        // Newlines, tabs, escapes and NULs do not survive.
        assert_eq!(unescape(r"a\nb\tc\e[31m\0d\33e"), "abc[31mde");
        assert_eq!(unescape("trailing\\"), "trailing");
        assert_eq!(unescape(&"x".repeat(5000)).chars().count(), 1024);
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let text = "[Software\\\\Microsoft\\\\Windows\\\\CurrentVersion\\\\Uninstall\\\\App] 1\n\
                    \"DisplayName\"\n\
                    \"DisplayName\"=\n\
                    \"DisplayName\"=\"unterminated\n\
                    \"DisplayVersion\"=hex:01,02\n\
                    \"EstimatedSize\"=dword:zz\n\
                    DisplayName=\"no quotes\"\n\
                    \"Publisher\"=\"   \"\n\
                    \"DisplayIcon\"=\"C:\\\\app\\\\app.exe,0\"\n";
        let entries = parse_uninstall_entries(text, Hive::User);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.key, "App");
        assert_eq!(entry.hive, Some(Hive::User));
        assert_eq!(entry.display_name, None);
        assert_eq!(entry.display_version, None);
        assert_eq!(entry.publisher, None);
        assert_eq!(entry.display_icon.as_deref(), Some("C:\\app\\app.exe,0"));
        assert_eq!(entry.identity(), "HKCU\\App");
        // Headers that are not uninstall keys, or are cut short.
        for text in [
            "",
            "[",
            "[]",
            "[Software\\\\Microsoft",
            "garbage\n\"a\"=\"b\"",
        ] {
            assert!(
                parse_uninstall_entries(text, Hive::Machine).is_empty(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_number_of_entries_is_bounded() {
        let mut text = String::new();
        for index in 0..400 {
            text.push_str(&format!(
                "[Software\\\\Microsoft\\\\Windows\\\\CurrentVersion\\\\Uninstall\\\\App{index}] 1\n\
                 \"DisplayName\"=\"App {index}\"\n\n"
            ));
        }
        assert_eq!(
            parse_uninstall_entries(&text, Hive::Machine).len(),
            MAX_ENTRIES
        );
    }

    #[test]
    fn both_hives_of_a_prefix_are_read() {
        let temp = TempDir::new("winreg");
        // A prefix without a registry has no entries.
        assert!(read_uninstall_entries(temp.path()).is_empty());
        std::fs::write(temp.path().join("system.reg"), SYSTEM_REG).unwrap();
        std::fs::write(
            temp.path().join("user.reg"),
            "WINE REGISTRY Version 2\n\n\
             [Software\\\\Microsoft\\\\Windows\\\\CurrentVersion\\\\Uninstall\\\\Mine] 1\n\
             \"DisplayName\"=\"Per-user application\"\n",
        )
        .unwrap();
        let entries = read_uninstall_entries(temp.path());
        let identities: Vec<String> = entries.iter().map(UninstallEntry::identity).collect();
        assert_eq!(
            identities,
            [
                "HKCU\\Mine",
                "HKLM\\7-Zip",
                "HKLM\\Wow6432Node\\Caf\u{e9} Tool",
                "HKLM\\{E5C4B1A0-0000-4000-8000-0123456789AB}",
            ]
        );
        // A registry file that is a link is not followed.
        std::fs::remove_file(temp.path().join("user.reg")).unwrap();
        std::os::unix::fs::symlink(temp.path().join("system.reg"), temp.path().join("user.reg"))
            .unwrap();
        assert_eq!(read_uninstall_entries(temp.path()).len(), 3);
    }
}
