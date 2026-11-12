// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The metadata file kept beside every prefix (specification §12.2, §15.2).
//!
//! It records which application the prefix belongs to, the runtime and the
//! exact runtime version it was created with, and the settings needed to
//! reproduce a launch. The interchange format is JSON, described by
//! `schemas/prefix-metadata.schema.json`; a test keeps the two in step.
//!
//! The file is written by `tur-runtime` only and is never visible to the
//! application, which could otherwise grant itself directories.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::codes::Code;
use crate::limits::{MAX_GRANTS, MAX_METADATA_BYTES, MAX_MIGRATIONS};
use crate::paths::is_plain_absolute;
use crate::profile::{GraphicsBackend, SyncBackend, is_app_id, is_slug, is_version};
use crate::timefmt::Timestamp;

/// The metadata schema version this build reads and writes.
pub const PREFIX_SCHEMA_VERSION: u32 = 1;

/// The runtime a prefix belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    /// Wine, started directly.
    Wine,
    /// Proton, started through `umu-run`.
    Proton,
}

impl RuntimeKind {
    /// Every kind, in schema order.
    pub const ALL: [RuntimeKind; 2] = [RuntimeKind::Wine, RuntimeKind::Proton];

    /// The kind's machine-readable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            RuntimeKind::Wine => "wine",
            RuntimeKind::Proton => "proton",
        }
    }

    /// Parses a machine-readable name.
    pub fn from_name(name: &str) -> Option<RuntimeKind> {
        RuntimeKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == name)
    }

    /// A label for messages.
    pub const fn label(self) -> &'static str {
        match self {
            RuntimeKind::Wine => "Wine",
            RuntimeKind::Proton => "Proton",
        }
    }
}

/// The Windows architecture of a prefix.
///
/// Only 64-bit prefixes are created. Wine 11.0 deprecates pure 32-bit
/// prefixes; a 64-bit prefix hosts 32-bit programs where the Wine build can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrefixArch {
    /// A 64-bit prefix (`WINEARCH=win64`).
    Win64,
}

impl PrefixArch {
    /// Every architecture, in schema order.
    pub const ALL: [PrefixArch; 1] = [PrefixArch::Win64];

    /// The value of `WINEARCH` that creates this kind of prefix.
    pub const fn winearch(self) -> &'static str {
        match self {
            PrefixArch::Win64 => "win64",
        }
    }
}

/// How much access a grant gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrantAccess {
    /// The application can read the directory.
    ReadOnly,
    /// The application can read and change the directory.
    ReadWrite,
}

impl GrantAccess {
    /// Every access level, in schema order.
    pub const ALL: [GrantAccess; 2] = [GrantAccess::ReadOnly, GrantAccess::ReadWrite];
}

/// A host directory the user has made visible to the application.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// The directory, as an absolute path with every symbolic link resolved.
    pub path: String,
    /// Read-only or read-write.
    pub access: GrantAccess,
}

/// The synchronisation implementation a launch ended up with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncMode {
    /// The kernel's `ntsync` driver.
    Ntsync,
    /// Eventfd-based synchronisation (Proton builds).
    Esync,
    /// Futex-based synchronisation (Proton builds).
    Fsync,
    /// Synchronisation through the Wine server, which every Wine build has.
    Server,
    /// Left to the runtime; `tur-runtime` did not choose.
    RuntimeDefault,
}

impl SyncMode {
    /// Every mode, in schema order.
    pub const ALL: [SyncMode; 5] = [
        SyncMode::Ntsync,
        SyncMode::Esync,
        SyncMode::Fsync,
        SyncMode::Server,
        SyncMode::RuntimeDefault,
    ];
}

/// What was found when the `ntsync` device was last looked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NtsyncDevice {
    /// `/dev/ntsync` is a character device and could be opened.
    Present,
    /// There is no `/dev/ntsync`.
    Absent,
    /// `/dev/ntsync` exists and could not be opened.
    Inaccessible,
    /// Nobody has looked yet: the prefix has not been launched.
    NotChecked,
}

impl NtsyncDevice {
    /// Every state, in schema order.
    pub const ALL: [NtsyncDevice; 4] = [
        NtsyncDevice::Present,
        NtsyncDevice::Absent,
        NtsyncDevice::Inaccessible,
        NtsyncDevice::NotChecked,
    ];
}

/// The synchronisation decision of the most recent launch (§8.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncRecord {
    /// What the profile asked for.
    pub requested: SyncBackend,
    /// What the launch was set up to use.
    pub selected: SyncMode,
    /// What the device probe found.
    pub ntsync_device: NtsyncDevice,
    /// When the device was probed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<Timestamp>,
    /// Why `selected` was chosen, in plain language.
    pub reason: String,
}

impl SyncRecord {
    /// The record of a prefix that has not been launched.
    pub fn not_checked() -> Self {
        SyncRecord {
            requested: SyncBackend::Auto,
            selected: SyncMode::Server,
            ntsync_device: NtsyncDevice::NotChecked,
            checked_at: None,
            reason: "The prefix has not been launched yet.".to_owned(),
        }
    }
}

/// One explicit move of a prefix from one runtime version to another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRecord {
    /// When the migration was planned.
    pub at: Timestamp,
    /// The runtime version the prefix was on.
    pub from_version: String,
    /// The runtime version it was moved to.
    pub to_version: String,
    /// The name of the backup taken first.
    pub backup: String,
}

/// The metadata of one prefix, schema version 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixMetadata {
    /// The schema version; always [`PREFIX_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The application the prefix belongs to.
    pub app_id: String,
    /// The prefix name used by the application's profile.
    pub prefix_id: String,
    /// The runtime the prefix belongs to.
    pub runtime: RuntimeKind,
    /// The runtime version the prefix is on, as a token such as `wine-9.0`.
    pub runtime_version: String,
    /// Everything the runtime reported about its build, for the record.
    pub runtime_build: String,
    /// Whether the prefix is pinned to `runtime_version`: a pinned prefix is
    /// never migrated.
    pub pinned: bool,
    /// The Windows architecture of the prefix.
    pub architecture: PrefixArch,
    /// When the prefix was created.
    pub created_at: Timestamp,
    /// When the application was last launched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<Timestamp>,
    /// How many times the application has been launched.
    pub launch_count: u64,
    /// The Direct3D translation setting.
    pub graphics_backend: GraphicsBackend,
    /// The synchronisation decision of the most recent launch.
    pub sync: SyncRecord,
    /// Whether the application shares the host's network.
    pub network: bool,
    /// The host directories the application may see.
    pub grants: Vec<Grant>,
    /// Earlier runtime versions of this prefix, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrations: Vec<MigrationRecord>,
}

/// Why a metadata file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    /// The text is larger than a metadata file may be.
    TooLarge {
        /// The size of the text in bytes.
        bytes: usize,
    },
    /// The text is not JSON, or not shaped like prefix metadata.
    Shape(String),
    /// A field holds a value that is not allowed.
    Invalid {
        /// The field.
        field: &'static str,
        /// What is wrong with it.
        problem: &'static str,
    },
}

impl MetadataError {
    /// The stable error code for this failure.
    pub fn code(&self) -> Code {
        Code::PrefixMetadataInvalid
    }
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetadataError::TooLarge { bytes } => write!(
                f,
                "the prefix metadata is {bytes} bytes; at most {MAX_METADATA_BYTES} are accepted"
            ),
            MetadataError::Shape(detail) => {
                write!(f, "the prefix metadata does not match the schema: {detail}")
            }
            MetadataError::Invalid { field, problem } => {
                write!(f, "prefix metadata field {field:?} {problem}")
            }
        }
    }
}

impl std::error::Error for MetadataError {}

fn check(holds: bool, field: &'static str, problem: &'static str) -> Result<(), MetadataError> {
    if holds {
        Ok(())
    } else {
        Err(MetadataError::Invalid { field, problem })
    }
}

/// Whether `text` can be stored as a runtime's build description: 1 to 200
/// characters, none of them a control character.
pub(crate) fn is_build_text(text: &str) -> bool {
    let length = text.chars().count();
    (1..=200).contains(&length) && !text.chars().any(char::is_control)
}

fn is_reason(text: &str) -> bool {
    text.chars().count() <= 500 && !text.chars().any(char::is_control)
}

/// Whether `text` is a backup name: a compact timestamp, a hyphen and a
/// reason, such as `20261112T101500Z-manual`.
pub(crate) fn is_backup_name(text: &str) -> bool {
    let Some((time, reason)) = text.split_once('-') else {
        return false;
    };
    let time = time.as_bytes();
    let stamp = time.len() == 16
        && time.iter().enumerate().all(|(at, byte)| match at {
            8 => *byte == b'T',
            15 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
    stamp && is_slug(reason)
}

/// Parses and validates prefix metadata from JSON text.
///
/// # Errors
///
/// A [`MetadataError`] naming the first problem found.
pub fn parse_metadata(text: &str) -> Result<PrefixMetadata, MetadataError> {
    if text.len() > MAX_METADATA_BYTES {
        return Err(MetadataError::TooLarge { bytes: text.len() });
    }
    let metadata: PrefixMetadata =
        serde_json::from_str(text).map_err(|error| MetadataError::Shape(error.to_string()))?;
    metadata.validate()?;
    Ok(metadata)
}

impl PrefixMetadata {
    /// Checks every rule the types do not already enforce.
    ///
    /// Grants are checked for form only. Whether a grant is safe to honour is
    /// decided again at every launch, against the file system as it then is.
    ///
    /// # Errors
    ///
    /// [`MetadataError::Invalid`] naming the first field that breaks a rule.
    pub fn validate(&self) -> Result<(), MetadataError> {
        check(
            self.schema_version == PREFIX_SCHEMA_VERSION,
            "schema_version",
            "is not a schema version this build supports",
        )?;
        check(
            is_app_id(&self.app_id),
            "app_id",
            "must be lower-case segments joined by dots",
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
        check(
            is_build_text(&self.runtime_build),
            "runtime_build",
            "must be 1 to 200 characters with no control characters",
        )?;
        check(
            is_reason(&self.sync.reason),
            "sync",
            "has a reason that is too long or holds control characters",
        )?;
        check(
            self.grants.len() <= MAX_GRANTS,
            "grants",
            "has too many entries",
        )?;
        for (index, grant) in self.grants.iter().enumerate() {
            check(
                is_plain_absolute(&grant.path),
                "grants",
                "entries must be absolute paths without \"..\" or control characters",
            )?;
            let earlier = self.grants.get(..index).unwrap_or(&[]);
            check(
                !earlier.iter().any(|other| other.path == grant.path),
                "grants",
                "names the same directory twice",
            )?;
        }
        check(
            self.migrations.len() <= MAX_MIGRATIONS,
            "migrations",
            "has too many entries",
        )?;
        for migration in &self.migrations {
            check(
                is_version(&migration.from_version) && is_version(&migration.to_version),
                "migrations",
                "entries must name runtime versions",
            )?;
            check(
                is_backup_name(&migration.backup),
                "migrations",
                "entries must name a backup",
            )?;
        }
        Ok(())
    }

    /// The metadata as the JSON text stored on disk.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned());
        text.push('\n');
        text
    }

    /// Appends a migration, dropping the oldest when the list is full.
    pub(crate) fn remember_migration(&mut self, record: MigrationRecord) {
        if self.migrations.len() >= MAX_MIGRATIONS {
            self.migrations.remove(0);
        }
        self.migrations.push(record);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::Value;

    use super::*;

    const SCHEMA: &str = include_str!("../../../schemas/prefix-metadata.schema.json");

    /// A valid metadata record for `com.example.application`.
    pub(crate) fn sample() -> PrefixMetadata {
        PrefixMetadata {
            schema_version: PREFIX_SCHEMA_VERSION,
            app_id: "com.example.application".to_owned(),
            prefix_id: "example-application".to_owned(),
            runtime: RuntimeKind::Wine,
            runtime_version: "wine-9.0".to_owned(),
            runtime_build: "wine-9.0 (Ubuntu 9.0~repack-4build3)".to_owned(),
            pinned: false,
            architecture: PrefixArch::Win64,
            created_at: Timestamp::from_unix(1_794_478_500),
            last_run_at: None,
            launch_count: 0,
            graphics_backend: GraphicsBackend::Auto,
            sync: SyncRecord::not_checked(),
            network: true,
            grants: Vec::new(),
            migrations: Vec::new(),
        }
    }

    fn full() -> PrefixMetadata {
        PrefixMetadata {
            last_run_at: Some(Timestamp::from_unix(1_794_478_600)),
            launch_count: 3,
            grants: vec![
                Grant {
                    path: "/home/ada/Games".to_owned(),
                    access: GrantAccess::ReadOnly,
                },
                Grant {
                    path: "/home/ada/Saves".to_owned(),
                    access: GrantAccess::ReadWrite,
                },
            ],
            sync: SyncRecord {
                requested: SyncBackend::Ntsync,
                selected: SyncMode::Server,
                ntsync_device: NtsyncDevice::Absent,
                checked_at: Some(Timestamp::from_unix(1_794_478_600)),
                reason: "There is no /dev/ntsync on this system.".to_owned(),
            },
            migrations: vec![MigrationRecord {
                at: Timestamp::from_unix(1_794_478_550),
                from_version: "wine-8.0".to_owned(),
                to_version: "wine-9.0".to_owned(),
                backup: "20261112T101550Z-before-migration".to_owned(),
            }],
            ..sample()
        }
    }

    #[test]
    fn metadata_round_trips_through_json() {
        for metadata in [sample(), full()] {
            let text = metadata.to_json();
            assert!(text.ends_with("}\n"));
            assert_eq!(parse_metadata(&text).unwrap(), metadata);
        }
        let value: Value = serde_json::from_str(&sample().to_json()).unwrap();
        assert_eq!(value["created_at"], "2026-11-12T10:15:00Z");
        assert_eq!(value["runtime"], "wine");
        assert_eq!(value["architecture"], "win64");
        assert_eq!(value["sync"]["ntsync_device"], "not-checked");
        assert!(value.get("last_run_at").is_none());
        assert!(value.get("migrations").is_none());
    }

    #[test]
    fn shape_errors_are_rejected() {
        let mut unknown: Value = serde_json::from_str(&sample().to_json()).unwrap();
        unknown["colour"] = Value::from("blue");
        let mut wrong_runtime: Value = serde_json::from_str(&sample().to_json()).unwrap();
        wrong_runtime["runtime"] = Value::from("darling");
        let mut bad_time: Value = serde_json::from_str(&sample().to_json()).unwrap();
        bad_time["created_at"] = Value::from("yesterday");
        let mut missing: Value = serde_json::from_str(&sample().to_json()).unwrap();
        missing.as_object_mut().unwrap().remove("grants");
        for value in [unknown, wrong_runtime, bad_time, missing] {
            let error = parse_metadata(&value.to_string()).unwrap_err();
            assert!(matches!(error, MetadataError::Shape(_)), "{value}");
            assert_eq!(error.code(), Code::PrefixMetadataInvalid);
        }
        assert!(matches!(
            parse_metadata("not json"),
            Err(MetadataError::Shape(_))
        ));
        let huge = " ".repeat(MAX_METADATA_BYTES + 1);
        assert_eq!(
            parse_metadata(&huge),
            Err(MetadataError::TooLarge {
                bytes: MAX_METADATA_BYTES + 1
            })
        );
    }

    #[test]
    fn invalid_values_are_rejected_by_field() {
        let cases: Vec<(&str, PrefixMetadata)> = vec![
            (
                "schema_version",
                PrefixMetadata {
                    schema_version: 2,
                    ..sample()
                },
            ),
            (
                "app_id",
                PrefixMetadata {
                    app_id: "../escape".to_owned(),
                    ..sample()
                },
            ),
            (
                "prefix_id",
                PrefixMetadata {
                    prefix_id: "Has Capitals".to_owned(),
                    ..sample()
                },
            ),
            (
                "runtime_version",
                PrefixMetadata {
                    runtime_version: "wine 9.0".to_owned(),
                    ..sample()
                },
            ),
            (
                "runtime_build",
                PrefixMetadata {
                    runtime_build: "wine\u{1b}[31m".to_owned(),
                    ..sample()
                },
            ),
            (
                "grants",
                PrefixMetadata {
                    grants: vec![Grant {
                        path: "relative".to_owned(),
                        access: GrantAccess::ReadOnly,
                    }],
                    ..sample()
                },
            ),
            (
                "grants",
                PrefixMetadata {
                    grants: vec![Grant {
                        path: "/home/ada/Games/../.ssh".to_owned(),
                        access: GrantAccess::ReadOnly,
                    }],
                    ..sample()
                },
            ),
            (
                "grants",
                PrefixMetadata {
                    grants: vec![
                        Grant {
                            path: "/data".to_owned(),
                            access: GrantAccess::ReadOnly,
                        },
                        Grant {
                            path: "/data".to_owned(),
                            access: GrantAccess::ReadWrite,
                        },
                    ],
                    ..sample()
                },
            ),
            (
                "migrations",
                PrefixMetadata {
                    migrations: vec![MigrationRecord {
                        at: Timestamp::from_unix(0),
                        from_version: "wine-8.0".to_owned(),
                        to_version: "wine-9.0".to_owned(),
                        backup: "../../etc/passwd".to_owned(),
                    }],
                    ..sample()
                },
            ),
        ];
        for (field, metadata) in cases {
            match metadata.validate() {
                Err(MetadataError::Invalid { field: named, .. }) => assert_eq!(named, field),
                other => panic!("{field} gave {other:?}"),
            }
            // The same record is refused when read back from disk.
            assert!(parse_metadata(&metadata.to_json()).is_err());
        }

        let too_many = PrefixMetadata {
            grants: (0..=MAX_GRANTS)
                .map(|index| Grant {
                    path: format!("/data/{index}"),
                    access: GrantAccess::ReadOnly,
                })
                .collect(),
            ..sample()
        };
        assert!(too_many.validate().is_err());
    }

    #[test]
    fn backup_names() {
        assert!(is_backup_name("20261112T101500Z-manual"));
        assert!(is_backup_name("20261112T101500Z-before-migration"));
        for bad in [
            "",
            "manual",
            "20261112T101500Z",
            "20261112T101500Z-",
            "20261112T101500Z-Manual",
            "20261112T101500Z-a/b",
            "2026111xT101500Z-manual",
            "../20261112T101500Z-manual",
            "20261112T101500Z-manual.tar.gz",
        ] {
            assert!(!is_backup_name(bad), "{bad}");
        }
    }

    #[test]
    fn migrations_are_bounded() {
        let mut metadata = sample();
        for index in 0..(MAX_MIGRATIONS + 5) {
            metadata.remember_migration(MigrationRecord {
                at: Timestamp::from_unix(u64::try_from(index).unwrap()),
                from_version: format!("wine-{index}.0"),
                to_version: format!("wine-{}.0", index + 1),
                backup: "20261112T101500Z-before-migration".to_owned(),
            });
        }
        assert_eq!(metadata.migrations.len(), MAX_MIGRATIONS);
        assert_eq!(metadata.migrations[0].from_version, "wine-5.0");
        assert_eq!(metadata.validate(), Ok(()));
    }

    #[test]
    fn runtime_kind_names_round_trip() {
        for kind in RuntimeKind::ALL {
            assert_eq!(RuntimeKind::from_name(kind.as_str()), Some(kind));
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
            assert!(!kind.label().is_empty());
        }
        assert_eq!(RuntimeKind::from_name("darling"), None);
        assert_eq!(PrefixArch::Win64.winearch(), "win64");
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
            PREFIX_SCHEMA_VERSION
        );
        assert_eq!(
            schema_strings(&schema, "/properties/runtime/enum"),
            wire_names(&RuntimeKind::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/properties/architecture/enum"),
            wire_names(&PrefixArch::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/properties/graphics_backend/enum"),
            wire_names(&GraphicsBackend::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/$defs/sync/properties/requested/enum"),
            wire_names(&SyncBackend::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/$defs/sync/properties/selected/enum"),
            wire_names(&SyncMode::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/$defs/sync/properties/ntsync_device/enum"),
            wire_names(&NtsyncDevice::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/$defs/grant/properties/access/enum"),
            wire_names(&GrantAccess::ALL)
        );
        assert_eq!(schema["properties"]["grants"]["maxItems"], MAX_GRANTS);
        assert_eq!(
            schema["properties"]["migrations"]["maxItems"],
            MAX_MIGRATIONS
        );

        // The schema's properties are exactly the fields of the types.
        let everything = serde_json::to_value(full()).unwrap();
        assert_eq!(keys(&schema["properties"]), keys(&everything));
        assert_eq!(
            keys(&schema["$defs"]["sync"]["properties"]),
            keys(&everything["sync"])
        );
        assert_eq!(
            keys(&schema["$defs"]["grant"]["properties"]),
            keys(&everything["grants"][0])
        );
        assert_eq!(
            keys(&schema["$defs"]["migration"]["properties"]),
            keys(&everything["migrations"][0])
        );

        // The required lists are exactly the fields that are never left out.
        let minimal = serde_json::to_value(sample()).unwrap();
        let mut required = schema_strings(&schema, "/required");
        required.sort();
        assert_eq!(required, keys(&minimal));
        let mut required = schema_strings(&schema, "/$defs/sync/required");
        required.sort();
        assert_eq!(required, keys(&minimal["sync"]));
        let mut required = schema_strings(&schema, "/$defs/grant/required");
        required.sort();
        assert_eq!(required, keys(&everything["grants"][0]));
        let mut required = schema_strings(&schema, "/$defs/migration/required");
        required.sort();
        assert_eq!(required, keys(&everything["migrations"][0]));
    }
}
