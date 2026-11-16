// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The record of a re-hosted application (`apps/<app-id>/record.json`,
//! `schemas/rehost-record.schema.json`) and its launcher entry.
//!
//! The record is written last: an application has one only when everything
//! else is in place. It is read back with a 256 KiB cap and must name the
//! application of the directory it is in.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::codes::{Code, Failure};
use crate::paths::AppId;

/// The record's schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// The largest record read back.
pub const MAX_RECORD_BYTES: u64 = 256 * 1024;

/// The runtime an application was re-hosted on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRecord {
    /// The family (`electron`).
    pub family: String,
    /// Its version, the payload's own.
    pub version: String,
    /// Where it was fetched from.
    pub url: String,
    /// Its SHA-256, as TUR's index pinned it.
    pub sha256: String,
    /// `upstream`: Electron's Linux build unchanged; `open-only`: its
    /// `libffmpeg.so` replaced by Electron's open-codecs build.
    pub codecs: String,
}

/// The package the person gave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRecord {
    /// Its file name.
    pub file_name: String,
    /// Its size.
    pub size: u64,
    /// Its SHA-256.
    pub sha256: String,
    /// The platform it was built for (`windows`, `macos`).
    pub platform: String,
    /// What was opened, outermost first.
    pub layers: Vec<String>,
}

/// One native module or helper package, as it was resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRecord {
    /// `name@version`, or `file:<name>` when it cannot be identified.
    pub module: String,
    /// The resolution (`carried`, `npm-package`, ..., `unresolved`).
    pub resolution: String,
    /// Where files were put.
    pub applied: Vec<String>,
}

/// What was fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchRecord {
    /// What it is.
    pub what: String,
    /// Where from.
    pub url: String,
    /// Its SHA-256.
    pub sha256: String,
    /// Its size.
    pub size: u64,
    /// Its licence, when the registry states one.
    pub licence: Option<String>,
}

/// The application record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppRecord {
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The application identifier.
    pub app_id: String,
    /// Its name.
    pub name: String,
    /// Its version, from the payload.
    pub app_version: Option<String>,
    /// The runtime.
    pub runtime: RuntimeRecord,
    /// The package.
    pub package: PackageRecord,
    /// The program started, relative to `app/`.
    pub program: String,
    /// The payload archive or directory, relative to `app/`.
    pub payload: String,
    /// The payload archive's SHA-256, checked before every start, because
    /// Electron's own check (`EnableEmbeddedAsarIntegrityValidation`) is
    /// off: its expected hash lives in the publisher's binary, which is not
    /// used. `None` for a directory payload.
    pub payload_sha256: Option<String>,
    /// The fuses of the publisher's binary.
    pub fuses_publisher: BTreeMap<String, String>,
    /// What was changed on the Linux binary (`RunAsNode: 1->0`).
    pub fuses_set: BTreeMap<String, String>,
    /// Native modules and helper packages.
    pub modules: Vec<ModuleRecord>,
    /// Helper programs replaced by the system's (`ffmpeg.exe -> /usr/bin/ffmpeg`).
    pub system_helpers: Vec<String>,
    /// Everything fetched.
    pub fetched: Vec<FetchRecord>,
    /// The application's own updater: always `off` (it would fetch a
    /// Windows or macOS update); TUR re-hosts a new version instead.
    pub updater: String,
    /// When it was set up, UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub installed_at: String,
    /// The staged launcher entry.
    pub desktop_entry: String,
}

impl AppRecord {
    /// Reads and checks the record of `id` at `path`.
    pub fn read(path: &Path, id: &AppId) -> Result<AppRecord, Failure> {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| {
            Failure::new(Code::NotInstalled, format!("no application {id} is re-hosted"))
        })?;
        if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES {
            return Err(Failure::new(
                Code::RecordInvalid,
                format!("the record of {id} is not a regular file of at most 256 KiB"),
            ));
        }
        let text = std::fs::read_to_string(path).map_err(|error| {
            Failure::new(Code::RecordInvalid, format!("cannot read the record of {id}: {error}"))
        })?;
        let record: AppRecord = serde_json::from_str(&text).map_err(|error| {
            Failure::new(Code::RecordInvalid, format!("the record of {id} is not valid: {error}"))
        })?;
        if record.schema_version != SCHEMA_VERSION || record.app_id != id.as_str() {
            return Err(Failure::new(
                Code::RecordInvalid,
                format!("the record in the directory of {id} is another application's, or of another version"),
            ));
        }
        for relative in [&record.program, &record.payload] {
            if crate::paths::safe_relative(relative).is_none() {
                return Err(Failure::new(
                    Code::RecordInvalid,
                    format!("the record of {id} names {relative:?}, which is not inside its directory"),
                ));
            }
        }
        Ok(record)
    }
}

/// Escapes a value for a desktop entry's string fields.
fn escape_value(text: &str) -> String {
    let mut out = String::new();
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Quotes one `Exec` argument by the Desktop Entry Specification's rules.
fn quote_argument(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(character);
            }
            '%' => out.push_str("%%"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    // A literal backslash is four in the file: escape for the string type.
    escape_value(&out)
}

/// The text of the launcher entry: the application starts through
/// `tur-rehost launch`, so a click gets the same sandbox and checks.
pub fn desktop_entry(id: &AppId, name: &str, platform: &str, launcher: &str) -> String {
    let edition = match platform {
        "macos" => "macOS",
        _ => "Windows",
    };
    format!(
        "[Desktop Entry]\nVersion=1.5\nType=Application\nName={}\nComment={}\nExec={} launch {}\nIcon=application-x-executable\nTerminal=false\nStartupNotify=true\nX-TUR-App-Id={}\nX-TUR-Runtime=rehost\n",
        escape_value(name),
        escape_value(&format!(
            "{edition} edition, running on Linux's own build of its runtime (not the publisher's Linux version)"
        )),
        quote_argument(launcher),
        quote_argument(id.as_str()),
        id.as_str()
    )
}

/// The current time in UTC, `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_utc() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format_utc(seconds)
}

/// Formats seconds since 1970 as `YYYY-MM-DDTHH:MM:SSZ` (the civil-from-days
/// algorithm, proleptic Gregorian calendar).
pub fn format_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let doe = shifted - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_index = (5 * doy + 2) / 153;
    let day = doy - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_in_utc() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_795_305_600), "2026-11-22T00:00:00Z");
        assert_eq!(format_utc(1_795_305_600 + 3_661), "2026-11-22T01:01:01Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn a_name_cannot_add_a_line_or_a_key_to_the_launcher_entry() {
        let id = AppId::parse("com.example.app").unwrap();
        let entry = desktop_entry(&id, "Evil\nExec=rm -rf ~", "windows", "/usr/bin/tur-rehost");
        assert_eq!(entry.lines().filter(|line| line.starts_with("Exec=")).count(), 1);
        assert!(entry.contains("Name=Evil\\nExec=rm -rf ~\n"));
        assert!(entry.contains("Exec=\"/usr/bin/tur-rehost\" launch \"com.example.app\"\n"));
        assert!(entry.contains("X-TUR-Runtime=rehost\n"));
    }

    #[test]
    fn the_schema_and_the_rust_types_name_the_same_fields() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../schemas/rehost-record.schema.json")).unwrap();
        let record = AppRecord {
            schema_version: SCHEMA_VERSION,
            app_id: "com.example.app".to_owned(),
            name: "Example".to_owned(),
            app_version: None,
            runtime: RuntimeRecord {
                family: "electron".to_owned(),
                version: "43.7.7".to_owned(),
                url: "https://example.org/e.zip".to_owned(),
                sha256: "0".repeat(64),
                codecs: "upstream".to_owned(),
            },
            package: PackageRecord {
                file_name: "x.zip".to_owned(),
                size: 1,
                sha256: "0".repeat(64),
                platform: "windows".to_owned(),
                layers: Vec::new(),
            },
            program: "x".to_owned(),
            payload: "resources/app.asar".to_owned(),
            payload_sha256: None,
            fuses_publisher: BTreeMap::new(),
            fuses_set: BTreeMap::new(),
            modules: vec![ModuleRecord {
                module: "x@1.0.0".to_owned(),
                resolution: "carried".to_owned(),
                applied: Vec::new(),
            }],
            system_helpers: Vec::new(),
            fetched: vec![FetchRecord {
                what: "x".to_owned(),
                url: "https://example.org/x".to_owned(),
                sha256: "0".repeat(64),
                size: 1,
                licence: None,
            }],
            updater: "off".to_owned(),
            installed_at: "2026-11-16T00:00:00Z".to_owned(),
            desktop_entry: "/x.desktop".to_owned(),
        };
        let written = serde_json::to_value(&record).unwrap();
        let keys = |value: &serde_json::Value| -> Vec<String> {
            let mut out: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
            out.sort();
            out
        };
        assert_eq!(keys(&written), keys(&schema["properties"]));
        let mut required: Vec<String> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect();
        required.sort();
        assert_eq!(keys(&written), required);
        for part in ["runtime", "package"] {
            assert_eq!(keys(&written[part]), keys(&schema["properties"][part]["properties"]), "{part}");
        }
        assert_eq!(
            keys(&written["modules"][0]),
            keys(&schema["properties"]["modules"]["items"]["properties"])
        );
        assert_eq!(
            keys(&written["fetched"][0]),
            keys(&schema["properties"]["fetched"]["items"]["properties"])
        );
    }

    #[test]
    fn a_record_of_another_application_is_refused() {
        let directory = std::env::temp_dir().join(format!("tur-rehost-record-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("record.json");
        std::fs::write(&path, "{\"schema_version\":1}").unwrap();
        let id = AppId::parse("com.example.app").unwrap();
        assert_eq!(AppRecord::read(&path, &id).unwrap_err().code, Code::RecordInvalid);
        assert_eq!(
            AppRecord::read(&directory.join("absent.json"), &id).unwrap_err().code,
            Code::NotInstalled
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
