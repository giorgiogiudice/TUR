// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What is kept about an installed macOS application:
//! `prefixes/<app-id>/mac-application.json`, schema version 1,
//! `schemas/mac-app-record.schema.json`.
//!
//! It sits beside the application's prefix and private home, outside the
//! sandbox, like a Windows application's `application.json`; it has another
//! name so that the two kinds of record are never taken for each other. It
//! is read back with a cap and must name the application of the directory it
//! is in, a bundle name that stays inside the prefix and a main program
//! inside that bundle: a record edited by hand cannot start something else
//! in an application's name.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::MacError;
use super::darling::{DarlingMode, PrefixLayout};
use crate::codes::Code;
use crate::paths::{AppPaths, RuntimeDirs};
use crate::profile::is_app_id;

/// The version of the record's format.
pub const RECORD_SCHEMA_VERSION: u32 = 1;

/// The record's file name in the application's directory.
pub const RECORD_FILE: &str = "mac-application.json";

/// The largest record that is read.
pub const MAX_RECORD_BYTES: u64 = 256 * 1024;

/// The Darling an application was installed with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DarlingRecord {
    /// The build's identifier in the manifest.
    pub artifact: String,
    /// Darling's commit.
    pub commit: String,
    /// How Darling runs.
    pub mode: DarlingMode,
    /// How the prefix is made.
    pub layout: PrefixLayout,
}

/// The file the application was installed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    /// What it was: `app-bundle`, `disk-image`, `installer-package`,
    /// `zip-archive`.
    pub kind: String,
    /// Its name.
    pub name: String,
    /// Its size in bytes; zero for a bundle directory.
    pub size: u64,
    /// Its SHA-256; absent for a bundle directory.
    pub sha256: Option<String>,
}

/// A finding of the analysis, as it was when the application was installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedFinding {
    /// The finding's stable identifier.
    pub reason: String,
    /// `refusal`, `refused-unless-allowed`, `warning` or `note`.
    pub severity: String,
    /// Its code, when it has one.
    pub code: Option<String>,
}

/// The staged launcher entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LauncherRecord {
    /// The entry's file.
    pub desktop_entry: String,
    /// Whether it was put where the desktop looks. Always `false` here: that
    /// is the desktop bridge's work.
    pub installed: bool,
}

/// The record of one installed macOS application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacAppRecord {
    /// [`RECORD_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The application identifier, also its directory's name.
    pub app_id: String,
    /// Its name.
    pub name: String,
    /// Its version, as the bundle states it.
    pub version: Option<String>,
    /// `CFBundleIdentifier`.
    pub bundle_identifier: Option<String>,
    /// The bundle's directory name in the prefix's `/Applications`.
    pub bundle: String,
    /// The main program, relative to the bundle.
    pub executable: String,
    /// The architecture of the program Darling runs.
    pub slice: String,
    /// Always `darling`.
    pub runtime: String,
    /// The Darling it was installed with.
    pub darling: DarlingRecord,
    /// Whether the application shares the host's network.
    pub network: bool,
    /// Whether the person allowed a Swift application to be tried.
    pub allowed_swift: bool,
    /// When it was installed, in UTC.
    pub installed_at: String,
    /// The file it came from.
    pub source: SourceRecord,
    /// How it was put into the prefix: `copied-bundle`, `unpacked` or
    /// `installer-package`.
    pub method: String,
    /// What the analysis found.
    pub findings: Vec<RecordedFinding>,
    /// The launcher entry.
    pub launcher: LauncherRecord,
}

/// Where an application's record is.
pub fn record_path(app: &AppPaths) -> PathBuf {
    app.app_dir.join(RECORD_FILE)
}

fn invalid(app: &AppPaths, why: &str) -> MacError {
    MacError::new(
        Code::MacosRecordInvalid,
        format!("the record of {} is not valid: {why}", app.app_id),
    )
}

/// Whether `name` is one path component without control characters.
fn plain_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.chars().any(char::is_control)
}

impl MacAppRecord {
    /// Checks the record against the directory it was read from.
    ///
    /// # Errors
    ///
    /// [`Code::MacosRecordInvalid`] with the reason.
    pub fn check(&self, app: &AppPaths) -> Result<(), MacError> {
        if self.schema_version != RECORD_SCHEMA_VERSION {
            return Err(invalid(app, "unknown schema version"));
        }
        if self.app_id != app.app_id || !is_app_id(&self.app_id) {
            return Err(invalid(app, "it names another application"));
        }
        if self.runtime != "darling" {
            return Err(invalid(app, "its runtime is not darling"));
        }
        if !plain_component(&self.bundle) || !self.bundle.ends_with(".app") {
            return Err(invalid(app, "the bundle is not a plain .app name"));
        }
        let program = self
            .executable
            .strip_prefix("Contents/MacOS/")
            .ok_or_else(|| invalid(app, "the main program is not in Contents/MacOS"))?;
        if !plain_component(program) {
            return Err(invalid(app, "the main program is not a plain file name"));
        }
        if self.name.is_empty()
            || self.name.chars().count() > 200
            || self.name.chars().any(char::is_control)
        {
            return Err(invalid(app, "the name is not allowed"));
        }
        Ok(())
    }
}

/// Reads an application's record. `Ok(None)` when it has none.
///
/// # Errors
///
/// [`Code::MacosRecordInvalid`] when the record cannot be read, is not
/// valid, or is another application's.
pub fn load(app: &AppPaths) -> Result<Option<MacAppRecord>, MacError> {
    let path = record_path(app);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(invalid(app, &error.to_string())),
    };
    let mut text = String::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| invalid(app, &error.to_string()))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_RECORD_BYTES {
        return Err(invalid(app, "it is too large"));
    }
    let record: MacAppRecord =
        serde_json::from_str(&text).map_err(|error| invalid(app, &error.to_string()))?;
    record.check(app)?;
    Ok(Some(record))
}

/// Every installed macOS application, and the problems met reading them.
pub fn installed(dirs: &RuntimeDirs) -> (Vec<MacAppRecord>, Vec<String>) {
    let mut records = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = fs::read_dir(dirs.prefixes()) else {
        return (records, problems);
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| is_app_id(name))
        .collect();
    names.sort();
    for name in names {
        let Ok(app) = dirs.app(&name) else {
            continue;
        };
        match load(&app) {
            Ok(Some(record)) => records.push(record),
            Ok(None) => {}
            Err(error) => problems.push(error.message),
        }
    }
    (records, problems)
}

/// The application's bundle inside the Darling prefix, on this system.
pub fn bundle_path(darling_prefix: &Path, record: &MacAppRecord) -> PathBuf {
    darling_prefix.join("Applications").join(&record.bundle)
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::Value;

    use super::*;
    use crate::testdata::TempDir;

    const SCHEMA: &str = include_str!("../../../../schemas/mac-app-record.schema.json");

    /// A record of a small application.
    pub(crate) fn sample(app_id: &str) -> MacAppRecord {
        MacAppRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            app_id: app_id.to_owned(),
            name: "Table Tool".to_owned(),
            version: Some("1.2.1".to_owned()),
            bundle_identifier: Some("ch.jakob.TableTool".to_owned()),
            bundle: "Table Tool.app".to_owned(),
            executable: "Contents/MacOS/Table Tool".to_owned(),
            slice: "x86_64".to_owned(),
            runtime: "darling".to_owned(),
            darling: DarlingRecord {
                artifact: "darling-754fe00baf67-q0-fc45-x86_64".to_owned(),
                commit: "754fe00baf67db29a39769a49abfb895490d8abc".to_owned(),
                mode: DarlingMode::Nonroot,
                layout: PrefixLayout::Layered,
            },
            network: false,
            allowed_swift: false,
            installed_at: "2026-11-16T12:00:00Z".to_owned(),
            source: SourceRecord {
                kind: "disk-image".to_owned(),
                name: "TableTool.dmg".to_owned(),
                size: 1_048_576,
                sha256: Some("a".repeat(64)),
            },
            method: "unpacked".to_owned(),
            findings: vec![RecordedFinding {
                reason: "minimum-macos".to_owned(),
                severity: "note".to_owned(),
                code: None,
            }],
            launcher: LauncherRecord {
                desktop_entry: format!("/d/tur/runtime/launchers/{app_id}/tur-darling-{app_id}.desktop"),
                installed: false,
            },
        }
    }

    fn keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }

    #[test]
    fn the_schema_and_the_type_name_the_same_fields() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        let record = serde_json::to_value(sample("ch.jakob.tabletool")).unwrap();
        assert_eq!(keys(&schema["properties"]), keys(&record));
        let mut required: Vec<String> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect();
        required.sort();
        assert_eq!(required, keys(&record));
        for (property, definition) in [
            ("darling", "darling"),
            ("source", "source"),
            ("launcher", "launcher"),
        ] {
            assert_eq!(
                keys(&schema["$defs"][definition]["properties"]),
                keys(&record[property]),
                "{property}"
            );
        }
        assert_eq!(
            keys(&schema["$defs"]["finding"]["properties"]),
            keys(&record["findings"][0])
        );
        assert_eq!(schema["properties"]["schema_version"]["const"], 1);
    }

    #[test]
    fn a_record_is_read_back_only_for_its_own_directory() {
        let scratch = TempDir::new("mac-record");
        let dirs = RuntimeDirs::new(&scratch.path().join("data"), &scratch.path().join("state"));
        let app = dirs.app("ch.jakob.tabletool").unwrap();
        assert_eq!(load(&app).unwrap(), None);
        fs::create_dir_all(&app.app_dir).unwrap();
        let record = sample("ch.jakob.tabletool");
        fs::write(record_path(&app), serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(load(&app).unwrap(), Some(record.clone()));
        let (found, problems) = installed(&dirs);
        assert_eq!(found, [record.clone()]);
        assert!(problems.is_empty());

        // Another application's record, a bundle that climbs out of the
        // prefix, a program outside the bundle: refused.
        let edits: [fn(&mut MacAppRecord); 5] = [
            |r| r.app_id = "org.other.app".to_owned(),
            |r| r.bundle = "../../escape.app".to_owned(),
            |r| r.bundle = "Table Tool".to_owned(),
            |r| r.executable = "Contents/MacOS/../../../bin/sh".to_owned(),
            |r| r.runtime = "wine".to_owned(),
        ];
        for edit in edits {
            let mut bad = record.clone();
            edit(&mut bad);
            fs::write(record_path(&app), serde_json::to_string(&bad).unwrap()).unwrap();
            assert_eq!(load(&app).unwrap_err().code, Code::MacosRecordInvalid);
        }
        fs::write(record_path(&app), "{\"schema_version\":1}").unwrap();
        assert!(load(&app).is_err());
        let (_, problems) = installed(&dirs);
        assert_eq!(problems.len(), 1);
    }
}
