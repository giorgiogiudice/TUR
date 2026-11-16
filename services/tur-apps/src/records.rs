// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The record `tur-apps` keeps of each application it installed:
//! `$XDG_DATA_HOME/tur/apps/installed/<id>.json`, schema
//! `schemas/app-installed.schema.json`.
//!
//! The record ties the catalogue's identifier to what was installed by which
//! route: the Flatpak reference and the references the installation brought
//! in, the identifier `tur-runtime` keeps the Windows application under, or
//! the identifier `tur-rehost` keeps a re-hosted application under.
//! Removing an application removes its record last.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::codes::{Code, Failure};
use crate::paths::{Dirs, is_app_id};

/// The version of the record format.
pub const RECORD_VERSION: u32 = 1;

/// The largest record read back, in bytes.
pub const MAX_RECORD_BYTES: u64 = 256 * 1024;

/// What an application was installed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// `flatpak-remote`, `publisher-download` or `user-file`.
    pub kind: String,
    /// For a Flatpak: the remote.
    #[serde(default)]
    pub remote: Option<String>,
    /// For a download: the address in the catalogue.
    #[serde(default)]
    pub url: Option<String>,
    /// For a download or a file: the file's name.
    #[serde(default)]
    pub file_name: Option<String>,
    /// For a download or a file: its SHA-256.
    #[serde(default)]
    pub sha256: Option<String>,
    /// For a download or a file: its size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
    /// Whether the file's digest matched one the catalogue pins.
    #[serde(default)]
    pub pinned: bool,
}

/// What a Flatpak installation consists of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlatpakPart {
    /// The application's Flatpak identifier.
    pub app_id: String,
    /// Its full reference.
    pub reference: String,
    /// The remote it came from.
    pub remote: String,
    /// The commit installed, when known.
    #[serde(default)]
    pub commit: Option<String>,
    /// Every reference this installation added, the application's among
    /// them: runtimes, extensions, translations. Removal takes away those
    /// no other application still uses.
    pub added_refs: Vec<String>,
}

/// What a Windows installation consists of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsPart {
    /// The identifier `tur-runtime` keeps it under.
    pub runtime_app_id: String,
    /// The program `tur-runtime` starts, as a Windows path.
    #[serde(default)]
    pub main_executable: Option<String>,
}

/// What a re-hosted installation consists of: the publisher's Windows or
/// macOS package, laid by `tur-rehost` under the Linux build of its own
/// runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RehostPart {
    /// The identifier `tur-rehost` keeps it under.
    pub app_id: String,
    /// The package's platform: `windows` or `macos`, when known.
    #[serde(default)]
    pub platform: Option<String>,
    /// The runtime it runs on, as `electron 44.10.2`, when known.
    #[serde(default)]
    pub runtime: Option<String>,
    /// Whether the package was the one TUR measured: its SHA-256 equals
    /// the one in the catalogue's record of TUR's test.
    pub measured: bool,
    /// Whether the person allowed native modules to be built from source
    /// on this computer.
    #[serde(default)]
    pub builds_allowed: bool,
    /// The native modules with no Linux build it was allowed to run
    /// without, because TUR's measurement of this package showed that it
    /// starts without them.
    #[serde(default)]
    pub allowed_unresolved: Vec<String>,
}

/// The record of one installed application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// [`RECORD_VERSION`].
    pub schema_version: u32,
    /// The catalogue's identifier.
    pub id: String,
    /// The name, as the catalogue gives it.
    pub name: String,
    /// The route: `flatpak`, `wine` or `rehost`.
    pub route: String,
    /// What it was installed from.
    pub source: Source,
    /// The version installed, when known.
    #[serde(default)]
    pub version: Option<String>,
    /// When, in UTC, RFC 3339.
    pub installed_at: String,
    /// For the Flatpak route.
    #[serde(default)]
    pub flatpak: Option<FlatpakPart>,
    /// For the Windows route.
    #[serde(default)]
    pub windows: Option<WindowsPart>,
    /// For the re-hosted route.
    #[serde(default)]
    pub rehost: Option<RehostPart>,
}

impl Record {
    /// Checks what a record read from disk must hold.
    pub fn check(&self, expected_id: &str) -> Result<(), String> {
        if self.schema_version != RECORD_VERSION {
            return Err(format!(
                "schema_version is {}; this program reads {RECORD_VERSION}",
                self.schema_version
            ));
        }
        if self.id != expected_id || !is_app_id(&self.id) {
            return Err(format!(
                "it names the application \"{}\", not the one its file is named after",
                self.id
            ));
        }
        match (self.route.as_str(), &self.flatpak, &self.windows, &self.rehost) {
            ("flatpak", Some(part), None, None) => {
                if !part.reference.starts_with("app/") {
                    return Err("its Flatpak reference is not an application's".to_owned());
                }
            }
            ("wine", None, Some(part), None) => {
                if !is_app_id(&part.runtime_app_id) {
                    return Err("its tur-runtime identifier is not valid".to_owned());
                }
            }
            ("rehost", None, None, Some(part)) => {
                if !is_app_id(&part.app_id) {
                    return Err("its tur-rehost identifier is not valid".to_owned());
                }
            }
            _ => {
                return Err(format!(
                    "route \"{}\" does not match what it holds",
                    self.route
                ));
            }
        }
        Ok(())
    }
}

/// Reads the record of `id`, if there is one.
pub fn load(dirs: &Dirs, id: &str) -> Result<Option<Record>, Failure> {
    if !is_app_id(id) {
        return Err(Failure::new(
            Code::InputInvalid,
            format!("\"{id}\" is not an application identifier"),
        ));
    }
    let path = dirs.record(id);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Failure::new(
                Code::Unreadable,
                format!("cannot read {}: {error}", path.display()),
            ));
        }
    };
    let mut text = String::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| {
            Failure::new(
                Code::RecordInvalid,
                format!("cannot read {}: {error}", path.display()),
            )
        })?;
    if text.len() as u64 > MAX_RECORD_BYTES {
        return Err(Failure::new(
            Code::RecordInvalid,
            format!("{} is larger than a record may be", path.display()),
        ));
    }
    let record: Record = serde_json::from_str(&text).map_err(|error| {
        Failure::new(
            Code::RecordInvalid,
            format!("{} is not a valid record: {error}", path.display()),
        )
    })?;
    record.check(id).map_err(|reason| {
        Failure::new(
            Code::RecordInvalid,
            format!("{} is not a valid record: {reason}", path.display()),
        )
    })?;
    Ok(Some(record))
}

/// Every record, sorted by identifier, and the problems with files that
/// are not valid records.
pub fn load_all(dirs: &Dirs) -> (Vec<Record>, Vec<String>) {
    let mut records = Vec::new();
    let mut problems = Vec::new();
    let Ok(listing) = fs::read_dir(dirs.records()) else {
        return (records, problems);
    };
    let mut names: Vec<String> = listing
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    for name in names {
        let Some(id) = name.strip_suffix(".json") else {
            problems.push(format!("{name} is not a record"));
            continue;
        };
        match load(dirs, id) {
            Ok(Some(record)) => records.push(record),
            Ok(None) => {}
            Err(failure) => problems.push(failure.message),
        }
    }
    (records, problems)
}

/// Writes a record: to a file beside it first, then into place, so a
/// record is never half written.
pub fn save(dirs: &Dirs, record: &Record) -> Result<(), Failure> {
    let failed = |what: &str, error: std::io::Error| {
        Failure::new(
            Code::StepFailed,
            format!("cannot {what} the record: {error}"),
        )
    };
    let directory = dirs.records();
    fs::create_dir_all(&directory).map_err(|error| failed("create the directory of", error))?;
    let text = serde_json::to_string_pretty(record)
        .map_err(|error| Failure::new(Code::StepFailed, format!("cannot write JSON: {error}")))?;
    let target = dirs.record(&record.id);
    let partial = directory.join(format!(".{}.json.part", record.id));
    fs::write(&partial, format!("{text}\n")).map_err(|error| failed("write", error))?;
    fs::rename(&partial, &target).map_err(|error| failed("move into place", error))
}

/// Deletes a record.
pub fn remove(dirs: &Dirs, id: &str) -> Result<(), Failure> {
    match fs::remove_file(dirs.record(id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Failure::new(
            Code::StepFailed,
            format!("cannot delete the record of {id}: {error}"),
        )),
    }
}

/// The time now, in UTC, as RFC 3339 to the second.
pub fn now_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    rfc3339(seconds)
}

/// Seconds since 1970 in UTC as RFC 3339, `YYYY-MM-DDTHH:MM:SSZ`.
pub fn rfc3339(seconds: u64) -> String {
    let days = seconds / 86_400;
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// A name for a directory made now: `YYYYMMDDTHHMMSSZ`.
pub fn stamp() -> String {
    now_rfc3339().replace(['-', ':'], "")
}

/// The civil date of a day count since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`, for days on or after the epoch).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// Whether a path exists, without following a final symbolic link.
pub fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> Dirs {
        let root =
            std::env::temp_dir().join(format!("tur-apps-records-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let home = root.display().to_string();
        Dirs::from_env(|name| (name == "HOME").then(|| home.clone())).unwrap()
    }

    fn flatpak_record() -> Record {
        Record {
            schema_version: RECORD_VERSION,
            id: "net.sourceforge.klavaro".to_owned(),
            name: "Klavaro".to_owned(),
            route: "flatpak".to_owned(),
            source: Source {
                kind: "flatpak-remote".to_owned(),
                remote: Some("flathub".to_owned()),
                url: None,
                file_name: None,
                sha256: None,
                size: None,
                pinned: false,
            },
            version: Some("3.14".to_owned()),
            installed_at: "2026-11-12T10:00:00Z".to_owned(),
            flatpak: Some(FlatpakPart {
                app_id: "net.sourceforge.Klavaro".to_owned(),
                reference: "app/net.sourceforge.Klavaro/x86_64/stable".to_owned(),
                remote: "flathub".to_owned(),
                commit: None,
                added_refs: vec!["app/net.sourceforge.Klavaro/x86_64/stable".to_owned()],
            }),
            windows: None,
            rehost: None,
        }
    }

    fn rehost_record() -> Record {
        Record {
            schema_version: RECORD_VERSION,
            id: "com.jgraph.drawio".to_owned(),
            name: "draw.io".to_owned(),
            route: "rehost".to_owned(),
            source: Source {
                kind: "user-file".to_owned(),
                remote: None,
                url: None,
                file_name: Some("draw.io-32.6.10.msi".to_owned()),
                sha256: Some("d".repeat(64)),
                size: Some(149_065_728),
                pinned: true,
            },
            version: Some("32.6.10".to_owned()),
            installed_at: "2026-11-16T10:00:00Z".to_owned(),
            flatpak: None,
            windows: None,
            rehost: Some(RehostPart {
                app_id: "com.jgraph.drawio".to_owned(),
                platform: Some("windows".to_owned()),
                runtime: Some("electron 44.10.2".to_owned()),
                measured: true,
                builds_allowed: false,
                allowed_unresolved: Vec::new(),
            }),
        }
    }

    #[test]
    fn a_rehost_record_is_written_and_read_back() {
        let dirs = scratch("rehost");
        let record = rehost_record();
        save(&dirs, &record).unwrap();
        assert_eq!(load(&dirs, &record.id).unwrap(), Some(record.clone()));
        // A record of another route that also holds a re-host is refused.
        let mut mixed = rehost_record();
        mixed.route = "wine".to_owned();
        assert!(mixed.check("com.jgraph.drawio").is_err());
        let mut both = flatpak_record();
        both.rehost = record.rehost.clone();
        assert!(both.check("net.sourceforge.klavaro").is_err());
        // A record written before the re-hosted route existed has no
        // "rehost" field, and is still read.
        let path = dirs.record("net.sourceforge.klavaro");
        let mut old = serde_json::to_value(flatpak_record()).unwrap();
        old.as_object_mut().unwrap().remove("rehost");
        fs::write(&path, old.to_string()).unwrap();
        assert_eq!(
            load(&dirs, "net.sourceforge.klavaro").unwrap(),
            Some(flatpak_record())
        );
    }

    #[test]
    fn a_record_is_written_and_read_back() {
        let dirs = scratch("roundtrip");
        let record = flatpak_record();
        save(&dirs, &record).unwrap();
        assert_eq!(load(&dirs, &record.id).unwrap(), Some(record.clone()));
        let (all, problems) = load_all(&dirs);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], record);
        assert!(problems.is_empty());
        remove(&dirs, &record.id).unwrap();
        assert_eq!(load(&dirs, &record.id).unwrap(), None);
        remove(&dirs, &record.id).unwrap();
    }

    #[test]
    fn a_record_that_names_another_application_or_route_is_refused() {
        let dirs = scratch("refused");
        let mut record = flatpak_record();
        save(&dirs, &record).unwrap();
        let path = dirs.record(&record.id);
        let other = fs::read_to_string(&path).unwrap().replace(
            "\"id\": \"net.sourceforge.klavaro\"",
            "\"id\": \"org.example.other\"",
        );
        fs::write(&path, other).unwrap();
        assert_eq!(
            load(&dirs, &record.id).unwrap_err().code,
            Code::RecordInvalid
        );

        record.route = "wine".to_owned();
        assert!(record.check(&record.id.clone()).is_err());
        let unknown = serde_json::to_string(&flatpak_record())
            .unwrap()
            .replace("\"name\"", "\"colour\":1,\"name\"");
        fs::write(&path, unknown).unwrap();
        assert_eq!(
            load(&dirs, "net.sourceforge.klavaro").unwrap_err().code,
            Code::RecordInvalid
        );
        let (all, problems) = load_all(&dirs);
        assert!(all.is_empty());
        assert_eq!(problems.len(), 1);
        assert_eq!(load(&dirs, "../x").unwrap_err().code, Code::InputInvalid);
    }

    #[test]
    fn times_are_written_in_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_794_484_800), "2026-11-12T12:00:00Z");
        assert_eq!(stamp().len(), "20261112T120000Z".len());
    }
}
