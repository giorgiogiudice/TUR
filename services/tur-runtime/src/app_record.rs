// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The record of an installed Windows application.
//!
//! `tur-runtime app install` writes one record per application, beside its
//! prefix and outside what the application can see:
//! `prefixes/<app-id>/application.json`. It says what was installed, from
//! which file (by SHA-256), how, what the launcher entry starts, and how the
//! application is removed again. `schemas/app-record.schema.json` describes
//! the file; a test keeps the two in step.
//!
//! The record is written by this program only. It is still read back with
//! the discipline of an inspected file, because another process of the same
//! user can change it: a size cap, unknown fields refused, every path a
//! plain absolute path, every string bounded and free of control
//! characters.

use std::fmt;
use std::fs;
use std::io::Read;

use serde::{Deserialize, Serialize};

use crate::codes::Code;
use crate::installer::InstallerKind;
use crate::metadata::RuntimeKind;
use crate::paths::{AppPaths, is_plain_absolute};
use crate::profile::{is_app_id, is_display_name, is_slug, is_version};
use crate::timefmt::Timestamp;

/// The record schema version this build reads and writes.
pub const APP_RECORD_SCHEMA_VERSION: u32 = 1;

/// Largest application record accepted, in bytes.
pub const MAX_RECORD_BYTES: usize = 256 * 1024;

/// Programs, shortcuts and arguments kept in one record.
pub const MAX_RECORDED_ITEMS: usize = 64;

/// Characters kept from a free-text field of a record.
const MAX_TEXT_CHARS: usize = 4096;

/// How the application got into its prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallMethod {
    /// Its installer was run in its unattended mode.
    UnattendedInstaller,
    /// An archive was unpacked into the prefix.
    Unpacked,
    /// The program file was copied into the prefix.
    Copied,
}

impl InstallMethod {
    /// Every method, in schema order.
    pub const ALL: [InstallMethod; 3] = [
        InstallMethod::UnattendedInstaller,
        InstallMethod::Unpacked,
        InstallMethod::Copied,
    ];
}

/// How the application is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UninstallMethod {
    /// Its own uninstaller is run first, then the prefix is deleted.
    Uninstaller,
    /// Windows Installer removes the product, then the prefix is deleted.
    MsiProduct,
    /// There is no uninstaller; the prefix is deleted.
    RemovePrefix,
}

impl UninstallMethod {
    /// Every method, in schema order.
    pub const ALL: [UninstallMethod; 3] = [
        UninstallMethod::Uninstaller,
        UninstallMethod::MsiProduct,
        UninstallMethod::RemovePrefix,
    ];
}

/// The file an application was installed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    /// The file's name, without its directory.
    pub file_name: String,
    /// The SHA-256 digest of the file, in lower-case hexadecimal.
    pub sha256: String,
    /// The file's size in bytes.
    pub size: u64,
}

/// How an application was installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    /// The kind of installer, identified from the file's content.
    pub kind: InstallerKind,
    /// What was done with it.
    pub method: InstallMethod,
    /// The arguments the installer was run with, after the runtime's own.
    pub arguments: Vec<String>,
    /// The file it was installed from.
    pub source: SourceRecord,
}

/// How an application is removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UninstallRecord {
    /// What is done.
    pub method: UninstallMethod,
    /// The uninstaller, as a Windows path, or the product code of a Windows
    /// Installer package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// The arguments the uninstaller is run with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    /// The uninstall entry in the prefix's registry the above came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_key: Option<String>,
}

/// The launcher entry generated for an application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LauncherRecord {
    /// The desktop entry file.
    pub desktop_entry: String,
    /// The icon file, when the program had an icon that could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Whether the entry is installed where the desktop looks for entries.
    /// `false` in this build: the entry is staged in the runtime manager's
    /// own directory.
    pub installed: bool,
}

/// A shortcut the installer created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortcutRecord {
    /// The shortcut's name.
    pub name: String,
    /// What it points to, as a Windows path.
    pub target: String,
    /// The arguments it passes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// The record of one installed application, schema version 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppRecord {
    /// The schema version; always [`APP_RECORD_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The application's identifier, which also names its prefix directory.
    pub app_id: String,
    /// The application's name.
    pub name: String,
    /// The application's version, as the installer or the program states
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The application's publisher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// The program the launcher entry starts, as a path on this system.
    pub main_executable: String,
    /// The same program as the application sees it, a Windows path.
    pub main_executable_windows: String,
    /// The rule by which that program was chosen.
    pub main_executable_reason: String,
    /// The application's prefix.
    pub prefix: String,
    /// The prefix name used by the application's profile.
    pub prefix_id: String,
    /// The runtime the application was installed with.
    pub runtime: RuntimeKind,
    /// The runtime version it was installed with.
    pub runtime_version: String,
    /// When it was installed.
    pub installed_at: Timestamp,
    /// How it was installed, and from which file.
    pub install: InstallRecord,
    /// How it is removed.
    pub uninstall: UninstallRecord,
    /// Its launcher entry.
    pub launcher: LauncherRecord,
    /// The profile it is launched from.
    pub profile: String,
    /// The programs the installer added, as Windows paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub executables: Vec<String>,
    /// The shortcuts the installer created.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shortcuts: Vec<ShortcutRecord>,
}

/// Why an application record was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// The text is larger than a record may be.
    TooLarge {
        /// The size of the text in bytes.
        bytes: usize,
    },
    /// The file could not be read, is not JSON, or is not shaped like a
    /// record.
    Shape(String),
    /// A field holds a value that is not allowed.
    Invalid {
        /// The field.
        field: &'static str,
        /// What is wrong with it.
        problem: &'static str,
    },
}

impl RecordError {
    /// The stable error code for this failure.
    pub fn code(&self) -> Code {
        Code::AppRecordInvalid
    }
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::TooLarge { bytes } => write!(
                f,
                "the application record is {bytes} bytes; at most {MAX_RECORD_BYTES} are accepted"
            ),
            RecordError::Shape(detail) => {
                write!(
                    f,
                    "the application record does not match the schema: {detail}"
                )
            }
            RecordError::Invalid { field, problem } => {
                write!(f, "application record field {field:?} {problem}")
            }
        }
    }
}

impl std::error::Error for RecordError {}

fn check(holds: bool, field: &'static str, problem: &'static str) -> Result<(), RecordError> {
    if holds {
        Ok(())
    } else {
        Err(RecordError::Invalid { field, problem })
    }
}

/// Whether `text` can be stored as free text: not empty, bounded, and
/// without control characters.
pub(crate) fn is_text(text: &str, max_chars: usize) -> bool {
    let length = text.chars().count();
    (1..=max_chars).contains(&length) && !text.chars().any(char::is_control)
}

/// `text` made storable as free text: control characters removed, cut to
/// `max_chars`, surrounding white space trimmed. `None` when nothing is
/// left.
pub(crate) fn as_text(text: &str, max_chars: usize) -> Option<String> {
    let cleaned: String = text
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_owned())
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_file_name(text: &str) -> bool {
    is_text(text, 255) && !text.contains('/') && text != "." && text != ".."
}

const NOT_TEXT: &str = "must be text without control characters, within the length limit";
const NOT_A_PATH: &str = "must be an absolute path without \"..\" or control characters";

impl AppRecord {
    /// Checks every rule the types do not already enforce.
    ///
    /// # Errors
    ///
    /// [`RecordError::Invalid`] naming the first field that breaks a rule.
    pub fn validate(&self) -> Result<(), RecordError> {
        check(
            self.schema_version == APP_RECORD_SCHEMA_VERSION,
            "schema_version",
            "is not a schema version this build supports",
        )?;
        check(
            is_app_id(&self.app_id),
            "app_id",
            "must be lower-case segments joined by dots",
        )?;
        check(is_display_name(&self.name), "name", NOT_TEXT)?;
        check(
            self.version
                .as_deref()
                .is_none_or(|text| is_text(text, 100)),
            "version",
            NOT_TEXT,
        )?;
        check(
            self.publisher.as_deref().is_none_or(is_display_name),
            "publisher",
            NOT_TEXT,
        )?;
        check(is_plain_absolute(&self.prefix), "prefix", NOT_A_PATH)?;
        check(
            is_plain_absolute(&self.main_executable),
            "main_executable",
            NOT_A_PATH,
        )?;
        check(
            self.main_executable
                .strip_prefix(self.prefix.as_str())
                .is_some_and(|rest| rest.starts_with('/')),
            "main_executable",
            "must lie inside the application's prefix",
        )?;
        check(
            is_text(&self.main_executable_windows, MAX_TEXT_CHARS),
            "main_executable_windows",
            NOT_TEXT,
        )?;
        check(
            is_text(&self.main_executable_reason, 500),
            "main_executable_reason",
            NOT_TEXT,
        )?;
        check(
            is_slug(&self.prefix_id),
            "prefix_id",
            "must be 1 to 64 lower-case letters, digits or hyphens",
        )?;
        check(
            is_version(&self.runtime_version),
            "runtime_version",
            "must be a version token such as wine-9.0",
        )?;
        let arguments_ok = |arguments: &[String]| {
            arguments.len() <= MAX_RECORDED_ITEMS
                && arguments
                    .iter()
                    .all(|argument| is_text(argument, MAX_TEXT_CHARS))
        };
        check(arguments_ok(&self.install.arguments), "install", NOT_TEXT)?;
        check(
            is_file_name(&self.install.source.file_name),
            "install",
            "must name the source file without a directory",
        )?;
        check(
            is_sha256(&self.install.source.sha256),
            "install",
            "must hold a SHA-256 digest as 64 lower-case hexadecimal digits",
        )?;
        check(
            self.uninstall
                .program
                .as_deref()
                .is_none_or(|text| is_text(text, MAX_TEXT_CHARS))
                && self
                    .uninstall
                    .registry_key
                    .as_deref()
                    .is_none_or(|text| is_text(text, 1024))
                && arguments_ok(&self.uninstall.arguments),
            "uninstall",
            NOT_TEXT,
        )?;
        check(
            (self.uninstall.method == UninstallMethod::RemovePrefix)
                == self.uninstall.program.is_none(),
            "uninstall",
            "must name a program exactly when there is something to run",
        )?;
        check(
            is_plain_absolute(&self.launcher.desktop_entry)
                && self.launcher.icon.as_deref().is_none_or(is_plain_absolute),
            "launcher",
            NOT_A_PATH,
        )?;
        check(is_plain_absolute(&self.profile), "profile", NOT_A_PATH)?;
        check(
            self.executables.len() <= MAX_RECORDED_ITEMS
                && self
                    .executables
                    .iter()
                    .all(|path| is_text(path, MAX_TEXT_CHARS)),
            "executables",
            NOT_TEXT,
        )?;
        check(
            self.shortcuts.len() <= MAX_RECORDED_ITEMS
                && self.shortcuts.iter().all(|shortcut| {
                    is_text(&shortcut.name, 1024)
                        && is_text(&shortcut.target, MAX_TEXT_CHARS)
                        && shortcut
                            .arguments
                            .as_deref()
                            .is_none_or(|text| is_text(text, MAX_TEXT_CHARS))
                }),
            "shortcuts",
            NOT_TEXT,
        )?;
        Ok(())
    }

    /// The record as the JSON text stored on disk.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned());
        text.push('\n');
        text
    }
}

/// Parses and validates an application record from JSON text.
///
/// # Errors
///
/// A [`RecordError`] naming the first problem found.
pub fn parse_record(text: &str) -> Result<AppRecord, RecordError> {
    if text.len() > MAX_RECORD_BYTES {
        return Err(RecordError::TooLarge { bytes: text.len() });
    }
    let record: AppRecord =
        serde_json::from_str(text).map_err(|error| RecordError::Shape(error.to_string()))?;
    record.validate()?;
    Ok(record)
}

/// Reads the record of the application whose paths are `app`.
///
/// Returns `Ok(None)` when there is no record: the application is not
/// installed, though it may have a prefix.
///
/// # Errors
///
/// A [`RecordError`] when the record exists and is unreadable, not valid, or
/// names another application, another prefix or another profile.
pub fn load_record(app: &AppPaths) -> Result<Option<AppRecord>, RecordError> {
    if fs::symlink_metadata(&app.record).is_err() {
        return Ok(None);
    }
    let unreadable = |error: std::io::Error| {
        RecordError::Shape(format!("cannot read {}: {error}", app.record.display()))
    };
    let file = fs::File::open(&app.record).map_err(unreadable)?;
    let limit = u64::try_from(MAX_RECORD_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut text = String::new();
    file.take(limit)
        .read_to_string(&mut text)
        .map_err(unreadable)?;
    let record = parse_record(&text)?;
    check(
        record.app_id == app.app_id,
        "app_id",
        "names another application than the directory it is in",
    )?;
    check(
        app.prefix.to_str() == Some(record.prefix.as_str()),
        "prefix",
        "is not this application's prefix",
    )?;
    // The profile is what `app launch` starts from. A record that names
    // another one would start something else in this application's name.
    check(
        app.profile.to_str() == Some(record.profile.as_str()),
        "profile",
        "is not this application's profile",
    )?;
    Ok(Some(record))
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::Value;

    use super::*;
    use crate::paths::RuntimeDirs;
    use crate::testdata::TempDir;

    const SCHEMA: &str = include_str!("../../../schemas/app-record.schema.json");

    /// A record with every optional field left out.
    pub(crate) fn minimal() -> AppRecord {
        AppRecord {
            schema_version: APP_RECORD_SCHEMA_VERSION,
            app_id: "com.example.app".to_owned(),
            name: "Example".to_owned(),
            version: None,
            publisher: None,
            main_executable: "/d/tur/runtime/prefixes/com.example.app/prefix/drive_c/App/app.exe"
                .to_owned(),
            main_executable_windows: "C:\\App\\app.exe".to_owned(),
            main_executable_reason: "it is the only program that was installed".to_owned(),
            prefix: "/d/tur/runtime/prefixes/com.example.app/prefix".to_owned(),
            prefix_id: "com-example-app".to_owned(),
            runtime: RuntimeKind::Wine,
            runtime_version: "wine-9.0".to_owned(),
            installed_at: Timestamp::from_unix(1_794_478_500),
            install: InstallRecord {
                kind: InstallerKind::PlainExe,
                method: InstallMethod::Copied,
                arguments: Vec::new(),
                source: SourceRecord {
                    file_name: "app.exe".to_owned(),
                    sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                        .to_owned(),
                    size: 3,
                },
            },
            uninstall: UninstallRecord {
                method: UninstallMethod::RemovePrefix,
                program: None,
                arguments: Vec::new(),
                registry_key: None,
            },
            launcher: LauncherRecord {
                desktop_entry:
                    "/d/tur/runtime/launchers/com.example.app/tur-wine-com.example.app.desktop"
                        .to_owned(),
                icon: None,
                installed: false,
            },
            profile: "/d/tur/runtime/prefixes/com.example.app/profile.json".to_owned(),
            executables: Vec::new(),
            shortcuts: Vec::new(),
        }
    }

    /// A record with every field present.
    pub(crate) fn full() -> AppRecord {
        AppRecord {
            version: Some("26.06".to_owned()),
            publisher: Some("Igor Pavlov".to_owned()),
            install: InstallRecord {
                kind: InstallerKind::Nsis,
                method: InstallMethod::UnattendedInstaller,
                arguments: vec!["/S".to_owned()],
                ..minimal().install
            },
            uninstall: UninstallRecord {
                method: UninstallMethod::Uninstaller,
                program: Some("C:\\App\\uninstall.exe".to_owned()),
                arguments: vec!["/S".to_owned()],
                registry_key: Some("HKLM\\App".to_owned()),
            },
            launcher: LauncherRecord {
                icon: Some(
                    "/d/tur/runtime/launchers/com.example.app/tur-wine-com.example.app.png"
                        .to_owned(),
                ),
                ..minimal().launcher
            },
            executables: vec!["C:\\App\\app.exe".to_owned()],
            shortcuts: vec![ShortcutRecord {
                name: "Example".to_owned(),
                target: "C:\\App\\app.exe".to_owned(),
                arguments: Some("--start".to_owned()),
            }],
            ..minimal()
        }
    }

    #[test]
    fn a_record_survives_a_round_trip() {
        for record in [minimal(), full()] {
            record.validate().unwrap();
            let text = record.to_json();
            assert!(text.ends_with("}\n"));
            assert_eq!(parse_record(&text).unwrap(), record);
        }
        let text = minimal().to_json();
        // Optional fields that are not set are left out, not written as null.
        assert!(!text.contains("null"));
        assert!(!text.contains("\"version\""));
        assert!(text.contains("\"installed_at\": \"2026-11-12T10:15:00Z\""));
        assert!(text.contains("\"kind\": \"plain-exe\""));
        assert!(text.contains("\"method\": \"remove-prefix\""));
    }

    type Case = (&'static str, Box<dyn Fn(&mut AppRecord)>);

    fn case(field: &'static str, change: impl Fn(&mut AppRecord) + 'static) -> Case {
        let change: Box<dyn Fn(&mut AppRecord)> = Box::new(change);
        (field, change)
    }

    #[test]
    fn values_that_are_not_allowed_are_refused() {
        let cases: Vec<Case> = vec![
            case("schema_version", |record| record.schema_version = 2),
            case("app_id", |record| record.app_id = "../escape".to_owned()),
            case("name", |record| record.name = String::new()),
            case("name", |record| record.name = "Evil\u{1b}[31m".to_owned()),
            case("version", |record| record.version = Some("1\n2".to_owned())),
            case("version", |record| record.version = Some("v".repeat(101))),
            case("publisher", |record| record.publisher = Some(String::new())),
            case("prefix", |record| record.prefix = "relative".to_owned()),
            case("main_executable", |record| {
                record.main_executable = "/usr/bin/true".to_owned();
            }),
            case("main_executable", |record| {
                record.main_executable = format!("{}/../../x.exe", record.prefix);
            }),
            // A sibling whose name merely starts like the prefix.
            case("main_executable", |record| {
                record.main_executable = format!("{}-other/x.exe", record.prefix);
            }),
            case("main_executable_windows", |record| {
                record.main_executable_windows = String::new();
            }),
            case("main_executable_reason", |record| {
                record.main_executable_reason = "r".repeat(501);
            }),
            case("prefix_id", |record| {
                record.prefix_id = "Has Space".to_owned()
            }),
            case("runtime_version", |record| {
                record.runtime_version = "wine 9".to_owned();
            }),
            case("install", |record| {
                record.install.arguments = vec![String::new()]
            }),
            case("install", |record| {
                record.install.arguments = vec!["/S".to_owned(); 65];
            }),
            case("install", |record| {
                record.install.source.file_name = "dir/setup.exe".to_owned();
            }),
            case("install", |record| {
                record.install.source.sha256 = "ABC".to_owned()
            }),
            case("install", |record| {
                record.install.source.sha256 = record.install.source.sha256.to_uppercase();
            }),
            // Something to run without saying what.
            case("uninstall", |record| {
                record.uninstall.method = UninstallMethod::Uninstaller;
            }),
            // Nothing to run, and a program named all the same.
            case("uninstall", |record| {
                record.uninstall.program = Some("C:\\x.exe".to_owned());
            }),
            case("launcher", |record| {
                record.launcher.desktop_entry = "entry.desktop".to_owned();
            }),
            case("launcher", |record| {
                record.launcher.icon = Some("/a/../b.png".to_owned());
            }),
            case("profile", |record| record.profile = String::new()),
            case("executables", |record| {
                record.executables = vec!["C:\\x.exe".to_owned(); 65];
            }),
            case("shortcuts", |record| {
                record.shortcuts = vec![ShortcutRecord {
                    name: "Name\n".to_owned(),
                    target: "C:\\x.exe".to_owned(),
                    arguments: None,
                }];
            }),
        ];
        for (field, change) in cases {
            let mut record = minimal();
            change(&mut record);
            let error = record.validate().unwrap_err();
            assert!(
                matches!(&error, RecordError::Invalid { field: found, .. } if *found == field),
                "{field}: {error}"
            );
            assert_eq!(error.code(), Code::AppRecordInvalid);
            // A record that is not valid is not read back either.
            assert!(parse_record(&record.to_json()).is_err(), "{field}");
        }
    }

    #[test]
    fn text_that_is_not_a_record_is_refused() {
        assert!(matches!(parse_record(""), Err(RecordError::Shape(_))));
        assert!(matches!(parse_record("[]"), Err(RecordError::Shape(_))));
        // An unknown field, at the top and inside a member.
        let mut value = serde_json::to_value(minimal()).unwrap();
        value["extra"] = Value::from(1);
        assert!(matches!(
            parse_record(&value.to_string()),
            Err(RecordError::Shape(_))
        ));
        let mut value = serde_json::to_value(minimal()).unwrap();
        value["install"]["source"]["url"] = Value::from("https://example.org/");
        assert!(matches!(
            parse_record(&value.to_string()),
            Err(RecordError::Shape(_))
        ));
        // An unknown installer kind.
        let mut value = serde_json::to_value(minimal()).unwrap();
        value["install"]["kind"] = Value::from("wise");
        assert!(matches!(
            parse_record(&value.to_string()),
            Err(RecordError::Shape(_))
        ));
        let huge = " ".repeat(MAX_RECORD_BYTES + 1);
        assert_eq!(
            parse_record(&huge),
            Err(RecordError::TooLarge {
                bytes: MAX_RECORD_BYTES + 1
            })
        );
    }

    #[test]
    fn a_record_is_loaded_for_its_own_application_only() {
        let temp = TempDir::new("app-record");
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let dirs = RuntimeDirs::new(&root.join("data"), &root.join("state"));
        let app = dirs.app("com.example.app").unwrap();
        assert_eq!(load_record(&app), Ok(None));

        std::fs::create_dir_all(&app.app_dir).unwrap();
        let prefix = app.prefix.to_str().unwrap().to_owned();
        let record = AppRecord {
            main_executable: format!("{prefix}/drive_c/App/app.exe"),
            prefix,
            profile: app.profile.to_str().unwrap().to_owned(),
            ..minimal()
        };
        std::fs::write(&app.record, record.to_json()).unwrap();
        assert_eq!(load_record(&app), Ok(Some(record.clone())));
        // A record that would launch from a profile kept somewhere else.
        let elsewhere = AppRecord {
            profile: root.join("other-profile.json").to_str().unwrap().to_owned(),
            ..record.clone()
        };
        std::fs::write(&app.record, elsewhere.to_json()).unwrap();
        assert!(matches!(
            load_record(&app),
            Err(RecordError::Invalid {
                field: "profile",
                ..
            })
        ));

        // The same file under another application's directory.
        let other = dirs.app("com.example.other").unwrap();
        std::fs::create_dir_all(&other.app_dir).unwrap();
        std::fs::write(&other.record, record.to_json()).unwrap();
        assert!(matches!(
            load_record(&other),
            Err(RecordError::Invalid {
                field: "app_id",
                ..
            })
        ));
        // A record that names a prefix elsewhere.
        std::fs::write(&app.record, minimal().to_json()).unwrap();
        assert!(matches!(
            load_record(&app),
            Err(RecordError::Invalid {
                field: "prefix",
                ..
            })
        ));
        // Not JSON, and too large.
        std::fs::write(&app.record, "not json").unwrap();
        assert!(matches!(load_record(&app), Err(RecordError::Shape(_))));
        std::fs::write(&app.record, " ".repeat(MAX_RECORD_BYTES + 10)).unwrap();
        assert!(matches!(
            load_record(&app),
            Err(RecordError::TooLarge { .. })
        ));
    }

    #[test]
    fn free_text_is_cleaned_before_it_is_recorded() {
        assert_eq!(
            as_text("  7-Zip 26.06 (x64) ", 100).as_deref(),
            Some("7-Zip 26.06 (x64)")
        );
        assert_eq!(
            as_text("Evil\u{1b}[31m\nname", 100).as_deref(),
            Some("Evil[31mname")
        );
        assert_eq!(as_text("abcdef", 3).as_deref(), Some("abc"));
        assert_eq!(as_text(" \t\n", 100), None);
        assert_eq!(as_text("", 100), None);
        assert!(is_text("x", 1));
        assert!(!is_text("xy", 1));
        assert!(!is_text("", 1));
        assert!(!is_text("a\tb", 10));
    }

    fn schema_strings(schema: &Value, pointer: &str) -> Vec<String> {
        schema
            .pointer(pointer)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{pointer} is not an array in the schema"))
            .iter()
            .map(|entry| entry.as_str().unwrap().to_owned())
            .collect()
    }

    fn wire_names<T: Serialize>(values: &[T]) -> Vec<String> {
        values
            .iter()
            .map(|value| {
                serde_json::to_value(value)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    fn keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }

    fn required(schema: &Value, pointer: &str) -> Vec<String> {
        let mut required = schema_strings(schema, pointer);
        required.sort();
        required
    }

    #[test]
    fn schema_file_matches_the_rust_types() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["properties"]["schema_version"]["const"],
            APP_RECORD_SCHEMA_VERSION
        );
        assert_eq!(
            schema_strings(&schema, "/properties/runtime/enum"),
            wire_names(&RuntimeKind::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/$defs/install/properties/kind/enum"),
            wire_names(&InstallerKind::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/$defs/install/properties/method/enum"),
            wire_names(&InstallMethod::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/$defs/uninstall/properties/method/enum"),
            wire_names(&UninstallMethod::ALL)
        );
        for list in ["executables", "shortcuts"] {
            assert_eq!(
                schema["properties"][list]["maxItems"], MAX_RECORDED_ITEMS,
                "{list}"
            );
        }

        // The schema's properties are exactly the fields of the types.
        let everything = serde_json::to_value(full()).unwrap();
        assert_eq!(keys(&schema["properties"]), keys(&everything));
        for (definition, value) in [
            ("install", &everything["install"]),
            ("source", &everything["install"]["source"]),
            ("uninstall", &everything["uninstall"]),
            ("launcher", &everything["launcher"]),
            ("shortcut", &everything["shortcuts"][0]),
        ] {
            assert_eq!(
                keys(&schema["$defs"][definition]["properties"]),
                keys(value),
                "{definition}"
            );
            assert_eq!(
                schema["$defs"][definition]["additionalProperties"], false,
                "{definition}"
            );
        }

        // The required lists are exactly the fields that are never left out.
        let least = serde_json::to_value(minimal()).unwrap();
        assert_eq!(required(&schema, "/required"), keys(&least));
        assert_eq!(
            required(&schema, "/$defs/install/required"),
            keys(&least["install"])
        );
        assert_eq!(
            required(&schema, "/$defs/source/required"),
            keys(&least["install"]["source"])
        );
        assert_eq!(
            required(&schema, "/$defs/uninstall/required"),
            keys(&least["uninstall"])
        );
        assert_eq!(
            required(&schema, "/$defs/launcher/required"),
            keys(&least["launcher"])
        );
        let bare_shortcut = serde_json::to_value(ShortcutRecord {
            name: "n".to_owned(),
            target: "t".to_owned(),
            arguments: None,
        })
        .unwrap();
        assert_eq!(
            required(&schema, "/$defs/shortcut/required"),
            keys(&bare_shortcut)
        );
    }
}
