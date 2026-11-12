// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! `manifest.json`: what a bundle is, what it holds and what it lacks.
//!
//! The types here and `schemas/diag-bundle-manifest.schema.json` describe the
//! same document (specification §29.2). A test in this module reads the schema
//! and fails when the two disagree.

use serde::{Deserialize, Serialize};

use crate::redact::Profile;

/// The manifest schema version this build writes and verifies.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
/// The collector's name as recorded in the manifest.
pub const COLLECTOR_NAME: &str = "uos-diag";
/// The collector's version as recorded in the manifest.
pub const COLLECTOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The content of `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Version of this document's format.
    pub schema_version: u32,
    /// The program that made the bundle.
    pub collector: CollectorInfo,
    /// When the bundle was made, as `YYYY-MM-DDTHH:MM:SSZ`.
    pub created_utc: String,
    /// Which UOS build and deployment the machine was running.
    pub uos: UosIdentity,
    /// The kernel's boot ID, or `null` when it could not be read.
    pub boot_id: Option<String>,
    /// The machine architecture, for example `x86_64`.
    pub architecture: String,
    /// The running kernel's release, or `null` when it could not be read.
    pub kernel_release: Option<String>,
    /// The installer session the bundle belongs to. Always `null` for now.
    pub installer_session_id: Option<String>,
    /// The redaction profile applied to every file.
    pub redaction_profile: Profile,
    /// Whether collection was limited to the current boot.
    pub since_boot: bool,
    /// Every file in the bundle except `manifest.json` and `checksums.sha256`.
    pub files: Vec<FileEntry>,
    /// Everything that was wanted but is not in the bundle, or is incomplete.
    pub missing: Vec<MissingEntry>,
}

/// The program that made a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorInfo {
    /// Program name.
    pub name: String,
    /// Program version.
    pub version: String,
}

/// Which UOS build and deployment a machine runs. Every value is `null` on a
/// host that is not UOS or when it could not be determined.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UosIdentity {
    /// `UOS_BUILD_ID` from `/usr/lib/uos/release`.
    pub build_id: Option<String>,
    /// `UOS_VERSION` from `/usr/lib/uos/release`.
    pub version: Option<String>,
    /// `UOS_SOURCE_REVISION` from `/usr/lib/uos/release`.
    pub source_revision: Option<String>,
    /// The booted deployment's ostree checksum, from `bootc status`.
    pub deployment_checksum: Option<String>,
    /// The booted image's digest, from `bootc status`.
    pub image_digest: Option<String>,
}

/// One file in a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Path inside the archive.
    pub path: String,
    /// Lower-case hexadecimal SHA-256 of the file as stored.
    pub sha256: String,
    /// Size in bytes of the file as stored.
    pub size: u64,
}

/// Why something is not in a bundle, or not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingReason {
    /// The command is not installed.
    CommandNotFound,
    /// The command exited with a non-zero code or was ended by a signal.
    NonZeroExit,
    /// The command ran past its time limit and was killed.
    TimedOut,
    /// The file does not exist.
    FileNotFound,
    /// The command or file may not be accessed by the calling user.
    PermissionDenied,
    /// Another I/O failure.
    IoError,
    /// The output exceeded the size limit. The file is present but cut short.
    Truncated,
    /// A command-line option excluded this item.
    ExcludedByOption,
    /// The collector definition names a path outside the allowed locations.
    ForbiddenPath,
}

impl MissingReason {
    /// Every reason, in declaration order.
    pub const ALL: [MissingReason; 9] = [
        MissingReason::CommandNotFound,
        MissingReason::NonZeroExit,
        MissingReason::TimedOut,
        MissingReason::FileNotFound,
        MissingReason::PermissionDenied,
        MissingReason::IoError,
        MissingReason::Truncated,
        MissingReason::ExcludedByOption,
        MissingReason::ForbiddenPath,
    ];

    /// The reason as it appears in the manifest.
    pub const fn as_str(self) -> &'static str {
        match self {
            MissingReason::CommandNotFound => "command_not_found",
            MissingReason::NonZeroExit => "non_zero_exit",
            MissingReason::TimedOut => "timed_out",
            MissingReason::FileNotFound => "file_not_found",
            MissingReason::PermissionDenied => "permission_denied",
            MissingReason::IoError => "io_error",
            MissingReason::Truncated => "truncated",
            MissingReason::ExcludedByOption => "excluded_by_option",
            MissingReason::ForbiddenPath => "forbidden_path",
        }
    }
}

/// One item that was wanted but is absent from the bundle or incomplete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingEntry {
    /// The path the item has, or would have had, inside the archive.
    pub path: String,
    /// The command line or file the item comes from.
    pub source: String,
    /// The reason, from a fixed list.
    pub reason: MissingReason,
    /// Further detail, redacted like every other file.
    pub detail: String,
}

/// Formats seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// ```
/// assert_eq!(uos_diag::manifest::format_utc(0), "1970-01-01T00:00:00Z");
/// assert_eq!(uos_diag::manifest::format_utc(1_700_000_000), "2023-11-14T22:13:20Z");
/// ```
pub fn format_utc(epoch_seconds: u64) -> String {
    let days = epoch_seconds / 86_400;
    let second_of_day = epoch_seconds % 86_400;
    let hour = second_of_day / 3600;
    let minute = second_of_day % 3600 / 60;
    let second = second_of_day % 60;

    // Days since 1970-01-01 to a proleptic Gregorian date, following Howard
    // Hinnant's "civil_from_days". All values are non-negative here.
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::*;

    const SCHEMA: &str = include_str!("../../../schemas/diag-bundle-manifest.schema.json");

    fn sample() -> Manifest {
        Manifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            collector: CollectorInfo {
                name: COLLECTOR_NAME.to_owned(),
                version: COLLECTOR_VERSION.to_owned(),
            },
            created_utc: format_utc(1_700_000_000),
            uos: UosIdentity {
                build_id: Some("20261112.g0123456789ab".to_owned()),
                version: Some("0.1.0".to_owned()),
                source_revision: None,
                deployment_checksum: None,
                image_digest: None,
            },
            boot_id: Some("0f0e0d0c-0b0a-4909-8807-060504030201".to_owned()),
            architecture: "x86_64".to_owned(),
            kernel_release: Some("6.17.1-300.fc45.x86_64".to_owned()),
            installer_session_id: None,
            redaction_profile: Profile::Default,
            since_boot: false,
            files: vec![FileEntry {
                path: "system/kernel.txt".to_owned(),
                sha256: "0".repeat(64),
                size: 12,
            }],
            missing: vec![MissingEntry {
                path: "hardware/firmware.txt".to_owned(),
                source: "fwupdmgr get-devices".to_owned(),
                reason: MissingReason::CommandNotFound,
                detail: String::new(),
            }],
        }
    }

    fn keys(value: &Value) -> BTreeSet<String> {
        value
            .as_object()
            .expect("value is a JSON object")
            .keys()
            .cloned()
            .collect()
    }

    fn strings(value: &Value) -> BTreeSet<String> {
        value
            .as_array()
            .expect("value is a JSON array")
            .iter()
            .map(|item| item.as_str().expect("item is a string").to_owned())
            .collect()
    }

    /// Checks that a schema object node and a serialised Rust value have
    /// exactly the same keys, that every key is required, and that nothing
    /// else is allowed.
    fn assert_same_shape(schema_node: &Value, instance: &Value, what: &str) {
        assert_eq!(schema_node["type"], "object", "{what}: type");
        assert_eq!(schema_node["additionalProperties"], false, "{what}: additionalProperties");
        let properties = keys(&schema_node["properties"]);
        assert_eq!(properties, keys(instance), "{what}: properties differ from the Rust type");
        assert_eq!(properties, strings(&schema_node["required"]), "{what}: required");
    }

    #[test]
    fn utc_formatting_handles_leap_years_and_month_ends() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(86_399), "1970-01-01T23:59:59Z");
        assert_eq!(format_utc(86_400), "1970-01-02T00:00:00Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc(951_868_800), "2000-03-01T00:00:00Z");
        assert_eq!(format_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_utc(1_735_689_599), "2024-12-31T23:59:59Z");
        assert_eq!(format_utc(1_735_689_600), "2025-01-01T00:00:00Z");
        assert_eq!(format_utc(4_102_444_800), "2100-01-01T00:00:00Z");
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let manifest = sample();
        let text = serde_json::to_string_pretty(&manifest).unwrap();
        let back: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(back, manifest);
    }

    #[test]
    fn manifest_has_every_field_the_specification_requires() {
        let value = serde_json::to_value(sample()).unwrap();
        let expected: BTreeSet<String> = [
            "schema_version",
            "collector",
            "created_utc",
            "uos",
            "boot_id",
            "architecture",
            "kernel_release",
            "installer_session_id",
            "redaction_profile",
            "since_boot",
            "files",
            "missing",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(keys(&value), expected);
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["redaction_profile"], "default");
        assert_eq!(value["collector"]["name"], "uos-diag");
        assert_eq!(value["uos"]["source_revision"], Value::Null);
        assert_eq!(value["installer_session_id"], Value::Null);
        assert_eq!(value["missing"][0]["reason"], "command_not_found");
        assert_eq!(value["files"][0]["size"], 12);
    }

    #[test]
    fn rust_types_and_json_schema_agree() {
        let schema: Value = serde_json::from_str(SCHEMA).expect("the schema is valid JSON");
        let instance = serde_json::to_value(sample()).unwrap();

        assert_same_shape(&schema, &instance, "manifest");
        let properties = &schema["properties"];
        assert_same_shape(&properties["collector"], &instance["collector"], "collector");
        assert_same_shape(&properties["uos"], &instance["uos"], "uos");
        assert_same_shape(&properties["files"]["items"], &instance["files"][0], "files[]");
        assert_same_shape(&properties["missing"]["items"], &instance["missing"][0], "missing[]");

        assert_eq!(properties["schema_version"]["const"], MANIFEST_SCHEMA_VERSION);

        let reasons: BTreeSet<String> = MissingReason::ALL
            .into_iter()
            .map(|reason| reason.as_str().to_owned())
            .collect();
        assert_eq!(
            strings(&properties["missing"]["items"]["properties"]["reason"]["enum"]),
            reasons
        );

        let profiles: BTreeSet<String> = Profile::ALL
            .into_iter()
            .map(|profile| profile.as_str().to_owned())
            .collect();
        assert_eq!(strings(&properties["redaction_profile"]["enum"]), profiles);
    }

    #[test]
    fn reason_names_match_their_serialised_form() {
        for reason in MissingReason::ALL {
            let value = serde_json::to_value(reason).unwrap();
            assert_eq!(value, reason.as_str());
        }
        for profile in Profile::ALL {
            let value = serde_json::to_value(profile).unwrap();
            assert_eq!(value, profile.as_str());
        }
    }
}
