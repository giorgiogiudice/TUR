// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Where volume facts come from.
//!
//! The policy core never looks at a device. It asks a [`Probe`] for two
//! things: the block-device tree in the shape of
//! `lsblk --json --bytes --output-all`, and `blkid`-style `KEY=value`
//! properties for a device. [`SnapshotProbe`] answers from text captured
//! earlier, which is what the tests and the command line use. A later change
//! adds a live probe behind the same trait.
//!
//! Facts that neither tool reports are passed as extra properties with a
//! `UOS_` prefix:
//!
//! * `UOS_NTFS_STATE` = `clean` | `dirty` | `hibernated` | `unknown`
//! * `UOS_HFS_JOURNAL` = `journalled` | `not-journalled` | `unknown`
//!
//! A missing property means `unknown`, which the decision treats as unsafe.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer};

/// Property carrying the NTFS volume state.
pub const PROP_NTFS_STATE: &str = "UOS_NTFS_STATE";
/// Property carrying the HFS+ journal state.
pub const PROP_HFS_JOURNAL: &str = "UOS_HFS_JOURNAL";
/// blkid property carrying the per-device identifier of a Btrfs member.
pub const PROP_UUID_SUB: &str = "UUID_SUB";

/// A source of volume facts.
pub trait Probe {
    /// The block-device tree, as `lsblk --json --bytes --output-all` reports it.
    fn block_devices(&self) -> Result<Vec<LsblkDevice>, ProbeError>;

    /// `blkid`-style properties for a device path such as `/dev/sda1`. An
    /// unknown device has no properties.
    fn properties(&self, device: &str) -> BTreeMap<String, String>;
}

/// Why probe input could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// The lsblk document is not valid JSON or does not have the expected shape.
    Lsblk(String),
    /// The property text is malformed.
    Properties {
        /// One-based line number.
        line: usize,
        /// What is wrong with the line.
        message: String,
    },
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeError::Lsblk(message) => write!(f, "lsblk JSON: {message}"),
            ProbeError::Properties { line, message } => {
                write!(f, "property file line {line}: {message}")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

/// One block device as `lsblk` reports it. Only the columns the policy core
/// uses are kept; every other column is ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct LsblkDevice {
    /// Device name, for example `sda1` or `luks-…`.
    #[serde(default)]
    pub name: Option<String>,
    /// Kernel name, for example `dm-0`.
    #[serde(default)]
    pub kname: Option<String>,
    /// Device path, for example `/dev/sda1`.
    #[serde(default)]
    pub path: Option<String>,
    /// Device type: `disk`, `part`, `crypt`, `lvm`, `rom`, `loop`, `raid1`, ….
    #[serde(default, rename = "type")]
    pub device_type: Option<String>,
    /// Filesystem or container signature (blkid `TYPE`).
    #[serde(default)]
    pub fstype: Option<String>,
    /// Filesystem version, for example `FAT32` or `2` for LUKS2.
    #[serde(default)]
    pub fsver: Option<String>,
    /// Filesystem label.
    #[serde(default)]
    pub label: Option<String>,
    /// Filesystem UUID.
    #[serde(default)]
    pub uuid: Option<String>,
    /// Partition UUID.
    #[serde(default)]
    pub partuuid: Option<String>,
    /// Partition type code or GUID.
    #[serde(default)]
    pub parttype: Option<String>,
    /// Partition type name.
    #[serde(default)]
    pub parttypename: Option<String>,
    /// Size in bytes.
    #[serde(default, deserialize_with = "flexible_u64")]
    pub size: Option<u64>,
    /// Start offset in sectors.
    #[serde(default, deserialize_with = "flexible_u64")]
    pub start: Option<u64>,
    /// Removable-media flag.
    #[serde(default, deserialize_with = "flexible_bool")]
    pub rm: Option<bool>,
    /// Hot-pluggable flag.
    #[serde(default, deserialize_with = "flexible_bool")]
    pub hotplug: Option<bool>,
    /// Read-only flag.
    #[serde(default, deserialize_with = "flexible_bool")]
    pub ro: Option<bool>,
    /// Transport, for example `usb`, `nvme` or `sata`.
    #[serde(default)]
    pub tran: Option<String>,
    /// Hardware serial number.
    #[serde(default)]
    pub serial: Option<String>,
    /// World-wide name.
    #[serde(default)]
    pub wwn: Option<String>,
    /// Hardware model.
    #[serde(default)]
    pub model: Option<String>,
    /// Every mount point of the device.
    #[serde(default, deserialize_with = "string_list")]
    pub mountpoints: Vec<String>,
    /// The single mount point reported by older `lsblk` versions.
    #[serde(default)]
    pub mountpoint: Option<String>,
    /// Filesystem roots of the mounts, which for Btrfs are subvolume paths.
    #[serde(default, deserialize_with = "string_list")]
    pub fsroots: Vec<String>,
    /// Devices stacked on this one: partitions, mappings, logical volumes.
    #[serde(default)]
    pub children: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDocument {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FlexibleNumber {
    Number(u64),
    Text(String),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FlexibleBool {
    Bool(bool),
    Number(u64),
    Text(String),
}

/// Accepts a JSON number, a decimal string or null. `lsblk` prints byte sizes
/// as numbers only with `--bytes`; a human-readable size such as `"465.8G"`
/// is rejected rather than guessed.
fn flexible_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    match Option::<FlexibleNumber>::deserialize(deserializer)? {
        None => Ok(None),
        Some(FlexibleNumber::Number(number)) => Ok(Some(number)),
        Some(FlexibleNumber::Text(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed.parse::<u64>().map(Some).map_err(|_| {
                serde::de::Error::custom(format!(
                    "\"{trimmed}\" is not a whole number; run lsblk with --bytes"
                ))
            })
        }
    }
}

/// Accepts `true`/`false`, `0`/`1` as number or string, or null. Older
/// `lsblk` versions print flags as strings.
fn flexible_bool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    match Option::<FlexibleBool>::deserialize(deserializer)? {
        None => Ok(None),
        Some(FlexibleBool::Bool(value)) => Ok(Some(value)),
        Some(FlexibleBool::Number(0)) => Ok(Some(false)),
        Some(FlexibleBool::Number(1)) => Ok(Some(true)),
        Some(FlexibleBool::Number(other)) => {
            Err(serde::de::Error::custom(format!("{other} is not a flag value")))
        }
        Some(FlexibleBool::Text(text)) => match text.trim() {
            "" => Ok(None),
            "0" | "false" => Ok(Some(false)),
            "1" | "true" => Ok(Some(true)),
            other => Err(serde::de::Error::custom(format!("\"{other}\" is not a flag value"))),
        },
    }
}

/// Accepts an array of strings and nulls, or null. `lsblk` prints `[null]`
/// for a device that is not mounted.
fn string_list<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let raw = Option::<Vec<Option<String>>>::deserialize(deserializer)?;
    Ok(raw.unwrap_or_default().into_iter().flatten().collect())
}

/// Parses the JSON printed by `lsblk --json --bytes --output-all`.
pub fn parse_lsblk_json(json: &str) -> Result<Vec<LsblkDevice>, ProbeError> {
    serde_json::from_str::<LsblkDocument>(json)
        .map(|document| document.blockdevices)
        .map_err(|error| ProbeError::Lsblk(error.to_string()))
}

/// Parses `blkid --output export` text: blocks of `KEY=value` lines separated
/// by blank lines, each block starting with `DEVNAME=<device path>`.
///
/// Lines starting with `#` are comments. Values are taken literally; blkid's
/// escaping of unsafe characters is not undone, so labels are read from the
/// lsblk JSON, never from here.
pub fn parse_properties(
    text: &str,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProbeError> {
    let mut devices: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            current = None;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let malformed = |message: &str| ProbeError::Properties {
            line: index + 1,
            message: message.to_owned(),
        };
        let (key, value) = line.split_once('=').ok_or_else(|| malformed("expected KEY=value"))?;
        let key = key.trim();
        if key.is_empty() {
            return Err(malformed("empty key"));
        }
        if key == "DEVNAME" {
            if value.is_empty() {
                return Err(malformed("empty DEVNAME"));
            }
            devices.entry(value.to_owned()).or_default();
            current = Some(value.to_owned());
            continue;
        }
        let Some(device) = current.as_ref() else {
            return Err(malformed("property before DEVNAME"));
        };
        devices.entry(device.clone()).or_default().insert(key.to_owned(), value.to_owned());
    }
    Ok(devices)
}

/// A probe that answers from text captured earlier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotProbe {
    devices: Vec<LsblkDevice>,
    properties: BTreeMap<String, BTreeMap<String, String>>,
}

impl SnapshotProbe {
    /// Builds a probe from `lsblk --json --bytes --output-all` output.
    pub fn from_lsblk_json(json: &str) -> Result<SnapshotProbe, ProbeError> {
        Ok(SnapshotProbe { devices: parse_lsblk_json(json)?, properties: BTreeMap::new() })
    }

    /// Adds `blkid --output export` style properties. Later calls add to and
    /// replace earlier properties for the same device.
    pub fn with_properties(mut self, text: &str) -> Result<SnapshotProbe, ProbeError> {
        for (device, properties) in parse_properties(text)? {
            self.properties.entry(device).or_default().extend(properties);
        }
        Ok(self)
    }
}

impl Probe for SnapshotProbe {
    fn block_devices(&self) -> Result<Vec<LsblkDevice>, ProbeError> {
        Ok(self.devices.clone())
    }

    fn properties(&self, device: &str) -> BTreeMap<String, String> {
        self.properties.get(device).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_flags_and_null_lists() {
        let json = r#"{"blockdevices":[
            {"name":"sda","type":"disk","size":1000,"rm":true,"ro":false,"hotplug":"1",
             "mountpoints":[null],"fsroots":[null],
             "children":[{"name":"sda1","path":"/dev/sda1","type":"part","size":"900",
                          "fstype":"ext4","rm":"0","mountpoints":["/mnt/a",null,"/mnt/b"]}]}
        ]}"#;
        let devices = parse_lsblk_json(json).unwrap();
        assert_eq!(devices.len(), 1);
        let disk = &devices[0];
        assert_eq!(disk.size, Some(1000));
        assert_eq!(disk.rm, Some(true));
        assert_eq!(disk.ro, Some(false));
        assert_eq!(disk.hotplug, Some(true));
        assert!(disk.mountpoints.is_empty());
        assert!(disk.fsroots.is_empty());
        let part = &disk.children[0];
        assert_eq!(part.size, Some(900));
        assert_eq!(part.rm, Some(false));
        assert_eq!(part.device_type.as_deref(), Some("part"));
        assert_eq!(part.mountpoints, vec!["/mnt/a".to_owned(), "/mnt/b".to_owned()]);
    }

    #[test]
    fn missing_columns_default_to_absent() {
        let devices = parse_lsblk_json(r#"{"blockdevices":[{"name":"sdb"}]}"#).unwrap();
        let disk = &devices[0];
        assert_eq!(disk.size, None);
        assert_eq!(disk.rm, None);
        assert!(disk.children.is_empty());
        assert!(disk.mountpoints.is_empty());
    }

    #[test]
    fn unknown_columns_are_ignored() {
        let json = r#"{"blockdevices":[{"name":"sdb","maj:min":"8:16","fsuse%":null,"zoned":"none"}]}"#;
        assert_eq!(parse_lsblk_json(json).unwrap().len(), 1);
    }

    #[test]
    fn human_readable_sizes_are_rejected() {
        let error = parse_lsblk_json(r#"{"blockdevices":[{"name":"sdb","size":"465.8G"}]}"#)
            .unwrap_err();
        assert!(error.to_string().contains("--bytes"), "{error}");
    }

    #[test]
    fn malformed_documents_are_rejected() {
        assert!(matches!(parse_lsblk_json("not json"), Err(ProbeError::Lsblk(_))));
        assert!(matches!(parse_lsblk_json("{}"), Err(ProbeError::Lsblk(_))));
        assert!(matches!(parse_lsblk_json(r#"{"blockdevices":{}}"#), Err(ProbeError::Lsblk(_))));
        assert!(matches!(
            parse_lsblk_json(r#"{"blockdevices":[{"name":"sdb","rm":"maybe"}]}"#),
            Err(ProbeError::Lsblk(_))
        ));
    }

    #[test]
    fn parses_property_blocks() {
        let text = "# captured by hand\nDEVNAME=/dev/sda1\nUUID=AAAA\nUOS_NTFS_STATE=clean\n\n\
                    DEVNAME=/dev/sdb1\nUUID_SUB=1234\n";
        let devices = parse_properties(text).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices["/dev/sda1"]["UOS_NTFS_STATE"], "clean");
        assert_eq!(devices["/dev/sda1"]["UUID"], "AAAA");
        assert_eq!(devices["/dev/sdb1"]["UUID_SUB"], "1234");
    }

    #[test]
    fn property_errors_carry_the_line_number() {
        assert_eq!(
            parse_properties("UUID=AAAA\n"),
            Err(ProbeError::Properties { line: 1, message: "property before DEVNAME".to_owned() })
        );
        assert_eq!(
            parse_properties("DEVNAME=/dev/sda1\nnonsense\n"),
            Err(ProbeError::Properties { line: 2, message: "expected KEY=value".to_owned() })
        );
        assert_eq!(
            parse_properties("DEVNAME=/dev/sda1\n\nUUID=AAAA\n"),
            Err(ProbeError::Properties { line: 3, message: "property before DEVNAME".to_owned() })
        );
    }

    #[test]
    fn snapshot_probe_returns_properties_per_device() {
        let probe = SnapshotProbe::from_lsblk_json(r#"{"blockdevices":[]}"#)
            .unwrap()
            .with_properties("DEVNAME=/dev/sda1\nUOS_NTFS_STATE=dirty\n")
            .unwrap()
            .with_properties("DEVNAME=/dev/sda1\nUOS_NTFS_STATE=clean\n")
            .unwrap();
        assert_eq!(probe.properties("/dev/sda1")[PROP_NTFS_STATE], "clean");
        assert!(probe.properties("/dev/sdz9").is_empty());
        assert!(probe.block_devices().unwrap().is_empty());
    }
}
