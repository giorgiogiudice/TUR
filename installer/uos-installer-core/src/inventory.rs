// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Storage inventory built from data, never from the live system directly.
//!
//! The inventory is parsed from the JSON that `lsblk --json --bytes
//! --output-all` prints, together with a map of the `/dev/disk/by-id` and
//! `/dev/disk/by-path` links. Both arrive through the [`Probe`] trait, so the
//! same code runs on fixture files in tests and on a real machine later.
//!
//! The parser is lenient about value types because lsblk versions differ:
//! numbers may be JSON numbers or numeric strings, and booleans may be
//! `true`, `1` or `"1"`. It is strict about what it cannot do without: every
//! entry needs a name and a size in bytes.
//!
//! Kernel names and device paths (`sda`, `/dev/sda`) are recorded for display
//! only. They change between boots and are never an identity; see
//! [`crate::identity`].

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::codes::{Fault, InstallCode};
use crate::gpt;

/// Version of the `storage-inventory.json` format.
pub const INVENTORY_SCHEMA_VERSION: u32 = 1;

/// Largest input file the installer core will read, in bytes.
pub const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

/// Directory of stable device links derived from model, serial and WWN.
pub const BY_ID_DIR: &str = "/dev/disk/by-id";

/// Directory of device links derived from the physical connection.
pub const BY_PATH_DIR: &str = "/dev/disk/by-path";

/// Mount points that mark a device as the medium the live environment runs
/// from. This is an assumption about the UOS live image (a dracut live boot)
/// and is recorded in `docs/architecture/installer.md`.
pub const LIVE_MEDIUM_MOUNTPOINTS: [&str; 3] = [
    "/run/initramfs/live",
    "/run/initramfs/isoscan",
    "/run/install/repo",
];

/// How lsblk shows active swap in its mount point column.
const SWAP_MARKER: &str = "[SWAP]";

/// Deepest device tree the parser accepts (disk, partition, mappings).
const MAX_DEPTH: usize = 16;

/// What kind of block device a node is, from lsblk's `TYPE` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    /// A whole disk.
    Disk,
    /// A partition of a disk.
    Partition,
    /// A loop device.
    Loop,
    /// An optical drive.
    Rom,
    /// An opened encrypted volume.
    Crypt,
    /// An active LVM logical volume.
    Lvm,
    /// A multipath map over several paths to one disk.
    Multipath,
    /// A software RAID array.
    Raid,
    /// A compressed RAM device (zram), which lsblk reports as a disk.
    CompressedRam,
    /// Anything else.
    Other,
}

impl BlockKind {
    fn from_lsblk(kind: Option<&str>, name: &str) -> BlockKind {
        match kind {
            Some("disk") if name.starts_with("zram") => BlockKind::CompressedRam,
            Some("disk") => BlockKind::Disk,
            Some("part") => BlockKind::Partition,
            Some("loop") => BlockKind::Loop,
            Some("rom") => BlockKind::Rom,
            Some("crypt") => BlockKind::Crypt,
            Some("lvm") => BlockKind::Lvm,
            Some("mpath") => BlockKind::Multipath,
            Some(other) if other.starts_with("raid") || other == "md" => BlockKind::Raid,
            _ => BlockKind::Other,
        }
    }

    /// A short human-readable name.
    pub const fn label(self) -> &'static str {
        match self {
            BlockKind::Disk => "disk",
            BlockKind::Partition => "partition",
            BlockKind::Loop => "loop device",
            BlockKind::Rom => "optical drive",
            BlockKind::Crypt => "encrypted volume",
            BlockKind::Lvm => "LVM volume",
            BlockKind::Multipath => "multipath map",
            BlockKind::Raid => "RAID array",
            BlockKind::CompressedRam => "compressed RAM device",
            BlockKind::Other => "block device",
        }
    }

    /// True for kinds that exist only while something holds the device below
    /// them open: an unlocked encrypted volume, an active logical volume, a
    /// multipath map or an assembled array.
    pub const fn is_active_mapping(self) -> bool {
        matches!(
            self,
            BlockKind::Crypt
                | BlockKind::Lvm
                | BlockKind::Multipath
                | BlockKind::Raid
                | BlockKind::Other
        )
    }
}

/// How a disk is connected, from lsblk's `TRAN` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// NVM Express.
    Nvme,
    /// Serial ATA.
    Sata,
    /// Parallel ATA.
    Ata,
    /// USB mass storage.
    Usb,
    /// Parallel SCSI or an unspecified SCSI transport.
    Scsi,
    /// Serial Attached SCSI.
    Sas,
    /// SD or eMMC.
    Mmc,
    /// A virtio disk in a virtual machine.
    Virtio,
    /// Fibre Channel.
    FibreChannel,
    /// iSCSI.
    Iscsi,
    /// Not reported, or not one of the above.
    Unknown,
}

impl Transport {
    fn from_lsblk(transport: Option<&str>) -> Transport {
        match transport {
            Some("nvme") => Transport::Nvme,
            Some("sata") => Transport::Sata,
            Some("ata" | "ide") => Transport::Ata,
            Some("usb") => Transport::Usb,
            Some("scsi" | "spi") => Transport::Scsi,
            Some("sas") => Transport::Sas,
            Some("mmc") => Transport::Mmc,
            Some("virtio") => Transport::Virtio,
            Some("fc") => Transport::FibreChannel,
            Some("iscsi") => Transport::Iscsi,
            _ => Transport::Unknown,
        }
    }

    /// The connection type as shown to the user.
    pub const fn label(self) -> &'static str {
        match self {
            Transport::Nvme => "NVMe",
            Transport::Sata => "SATA",
            Transport::Ata => "ATA",
            Transport::Usb => "USB",
            Transport::Scsi => "SCSI",
            Transport::Sas => "SAS",
            Transport::Mmc => "SD/eMMC",
            Transport::Virtio => "virtio",
            Transport::FibreChannel => "Fibre Channel",
            Transport::Iscsi => "iSCSI",
            Transport::Unknown => "unknown connection",
        }
    }
}

/// A family of operating system found on a disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OsFamily {
    /// Microsoft Windows.
    Windows,
    /// A Linux distribution.
    Linux,
    /// Apple macOS.
    Macos,
}

impl OsFamily {
    /// The family as shown to the user.
    pub const fn label(self) -> &'static str {
        match self {
            OsFamily::Windows => "Windows",
            OsFamily::Linux => "Linux",
            OsFamily::Macos => "macOS",
        }
    }
}

/// How sure the detection is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// The disk carries structures only that operating system creates.
    Likely,
    /// The disk carries a filesystem that operating system uses, which may
    /// equally be a data volume.
    Possible,
}

/// An operating system detected on a disk from its partition types and
/// filesystems. Detection reads the inventory only; no volume is mounted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedOs {
    /// Which family was detected.
    pub family: OsFamily,
    /// How sure the detection is.
    pub confidence: Confidence,
    /// What was found, one entry per partition or filesystem.
    pub evidence: Vec<String>,
}

impl DetectedOs {
    /// One line for display, for example `Windows (likely)`.
    pub fn headline(&self) -> String {
        let confidence = match self.confidence {
            Confidence::Likely => "likely",
            Confidence::Possible => "possible",
        };
        format!("{} ({confidence})", self.family.label())
    }
}

/// One reason a device counts as in use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveUse {
    /// The device node in use, for display.
    pub path: String,
    /// Why it counts as in use, for example `mounted at /boot/efi`.
    pub reason: String,
}

/// A block device and everything layered on it.
///
/// One type serves disks, partitions and mappings; fields that do not apply
/// to a node are `None` or empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockDevice {
    /// The name lsblk shows, for example `nvme0n1` or `luks-…`.
    pub name: String,
    /// The kernel's name for the device, for example `sda` or `dm-0`. Not stable.
    pub kernel_name: String,
    /// The device node, for example `/dev/sda`. Not stable; display only.
    pub path: String,
    /// What kind of device this is.
    pub kind: BlockKind,
    /// Size in bytes. Zero for a drive with no medium.
    pub size_bytes: u64,
    /// Model string reported by the device.
    pub model: Option<String>,
    /// Vendor string reported by the device.
    pub vendor: Option<String>,
    /// Serial number reported by the device.
    pub serial: Option<String>,
    /// World Wide Name or equivalent unique identifier.
    pub wwn: Option<String>,
    /// How the device is connected.
    pub transport: Transport,
    /// Logical sector size in bytes; zero when not reported.
    pub logical_sector_size: u32,
    /// Physical sector size in bytes; zero when not reported.
    pub physical_sector_size: u32,
    /// True for removable media.
    pub removable: bool,
    /// True for hot-pluggable devices.
    pub hotplug: bool,
    /// True when the device is read-only.
    pub read_only: bool,
    /// True for rotating media.
    pub rotational: bool,
    /// Partition table type on this device, for example `gpt` or `dos`.
    pub partition_table: Option<String>,
    /// Partition number within its disk.
    pub partition_number: Option<u32>,
    /// First sector of the partition as lsblk reports it. Informational.
    pub start_sector: Option<u64>,
    /// Partition type: a lower-case GUID on GPT, a hexadecimal code on MBR.
    pub partition_type: Option<String>,
    /// Human-readable partition type name.
    pub partition_type_name: Option<String>,
    /// Partition label (GPT partition name).
    pub partition_label: Option<String>,
    /// Partition UUID.
    pub partition_uuid: Option<String>,
    /// Filesystem or container signature, for example `ntfs` or `crypto_LUKS`.
    pub filesystem: Option<String>,
    /// Filesystem label.
    pub filesystem_label: Option<String>,
    /// Filesystem UUID.
    pub filesystem_uuid: Option<String>,
    /// Where the device is mounted. Active swap appears as `[SWAP]`.
    pub mountpoints: Vec<String>,
    /// `/dev/disk/by-id` links that point at this device, sorted.
    pub by_id: Vec<String>,
    /// `/dev/disk/by-path` links that point at this device, sorted.
    pub by_path: Vec<String>,
    /// Devices layered on this one.
    pub children: Vec<BlockDevice>,
}

impl BlockDevice {
    /// This device followed by all its descendants, depth first.
    pub fn walk(&self) -> Vec<&BlockDevice> {
        let mut nodes = Vec::new();
        self.collect(&mut nodes);
        nodes
    }

    fn collect<'a>(&'a self, nodes: &mut Vec<&'a BlockDevice>) {
        nodes.push(self);
        for child in &self.children {
            child.collect(nodes);
        }
    }

    /// False for a drive that reports no medium, such as an empty card reader.
    pub fn has_medium(&self) -> bool {
        self.size_bytes > 0
    }

    /// The model for display, or a placeholder when the device reports none.
    pub fn display_model(&self) -> String {
        match (&self.vendor, &self.model) {
            (_, Some(model)) => model.clone(),
            (Some(vendor), None) => format!("{vendor} (model not reported)"),
            (None, None) => "Unknown model".to_owned(),
        }
    }

    /// The `/dev/disk/by-id` link best suited for display.
    ///
    /// Links that carry the model and serial are preferred over bare WWN
    /// links; among equals the shortest wins.
    pub fn preferred_by_id(&self) -> Option<&str> {
        self.by_id
            .iter()
            .min_by(|a, b| {
                link_rank(a)
                    .cmp(&link_rank(b))
                    .then(a.len().cmp(&b.len()))
                    .then(a.cmp(b))
            })
            .map(String::as_str)
    }

    /// Every reason this device or anything layered on it is in use: mounted
    /// filesystems, active swap, and open encrypted, LVM, multipath or RAID
    /// mappings.
    pub fn active_uses(&self) -> Vec<ActiveUse> {
        let mut uses = Vec::new();
        for node in self.walk() {
            for mountpoint in &node.mountpoints {
                let reason = if mountpoint == SWAP_MARKER {
                    "active swap".to_owned()
                } else {
                    format!("mounted at {mountpoint}")
                };
                uses.push(ActiveUse {
                    path: node.path.clone(),
                    reason,
                });
            }
            if !std::ptr::eq(node, self) && node.kind.is_active_mapping() {
                uses.push(ActiveUse {
                    path: node.path.clone(),
                    reason: format!("active {}", node.kind.label()),
                });
            }
        }
        uses
    }

    /// True when the live environment runs from this device.
    pub fn is_live_medium(&self) -> bool {
        self.walk().iter().any(|node| {
            node.mountpoints
                .iter()
                .any(|mountpoint| LIVE_MEDIUM_MOUNTPOINTS.contains(&mountpoint.as_str()))
        })
    }

    /// What this node holds, for a list of existing contents.
    pub fn describe_contents(&self) -> String {
        let mut parts = Vec::new();
        if self.kind != BlockKind::Partition && self.kind != BlockKind::Disk {
            parts.push(self.kind.label().to_owned());
        }
        if let Some(name) = &self.partition_type_name {
            parts.push(name.clone());
        }
        match &self.filesystem {
            Some(filesystem) => parts.push(filesystem.clone()),
            None => parts.push("no recognised filesystem".to_owned()),
        }
        if let Some(label) = self
            .filesystem_label
            .as_ref()
            .or(self.partition_label.as_ref())
        {
            parts.push(format!("label \"{label}\""));
        }
        parts.join(", ")
    }

    /// Operating systems detected on this device from partition types and
    /// filesystem signatures.
    pub fn detected_operating_systems(&self) -> Vec<DetectedOs> {
        let mut windows = Evidence::default();
        let mut linux = Evidence::default();
        let mut macos = Evidence::default();
        let mut has_esp = false;
        for node in self.walk() {
            let path = &node.path;
            match node.partition_type.as_deref().unwrap_or("") {
                gpt::ESP => has_esp = true,
                gpt::MICROSOFT_RESERVED => {
                    windows.add(true, format!("Microsoft reserved partition {path}"));
                }
                gpt::WINDOWS_RECOVERY => {
                    windows.add(true, format!("Windows recovery partition {path}"));
                }
                gpt::LINUX_ROOT_X86_64 => linux.add(true, format!("Linux root partition {path}")),
                gpt::LINUX_LVM => linux.add(true, format!("Linux LVM partition {path}")),
                gpt::LINUX_HOME => linux.add(true, format!("Linux home partition {path}")),
                gpt::XBOOTLDR => linux.add(true, format!("Linux boot partition {path}")),
                gpt::APPLE_APFS => macos.add(true, format!("Apple APFS partition {path}")),
                gpt::APPLE_HFS => macos.add(true, format!("Apple HFS+ partition {path}")),
                _ => {}
            }
            match node.filesystem.as_deref().unwrap_or("") {
                "ntfs" => windows.add(false, format!("NTFS volume {path}")),
                "BitLocker" => windows.add(true, format!("BitLocker volume {path}")),
                filesystem @ ("ext2" | "ext3" | "ext4" | "xfs" | "btrfs" | "f2fs") => {
                    linux.add(false, format!("{filesystem} filesystem {path}"));
                }
                "LVM2_member" => linux.add(true, format!("LVM physical volume {path}")),
                "crypto_LUKS" => linux.add(false, format!("LUKS encrypted volume {path}")),
                "apfs" | "hfsplus" => macos.add(true, format!("Apple filesystem {path}")),
                _ => {}
            }
        }
        // A Linux filesystem next to an EFI System Partition is an installed
        // system rather than a data disk.
        linux.strong |= has_esp;
        [
            windows.into_detected(OsFamily::Windows),
            linux.into_detected(OsFamily::Linux),
            macos.into_detected(OsFamily::Macos),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[derive(Default)]
struct Evidence {
    strong: bool,
    items: Vec<String>,
}

impl Evidence {
    fn add(&mut self, strong: bool, item: String) {
        self.strong |= strong;
        self.items.push(item);
    }

    fn into_detected(self, family: OsFamily) -> Option<DetectedOs> {
        if self.items.is_empty() {
            return None;
        }
        let confidence = if self.strong {
            Confidence::Likely
        } else {
            Confidence::Possible
        };
        Some(DetectedOs {
            family,
            confidence,
            evidence: self.items,
        })
    }
}

/// Lower ranks are better for display. Links made only of a WWN or a SCSI
/// identifier say less to a person than links with the model and serial.
fn link_rank(link: &str) -> u8 {
    const OPAQUE_PREFIXES: [&str; 5] = ["wwn-", "nvme-eui.", "nvme-nvme.", "nvme-uuid.", "scsi-"];
    let name = link.rsplit('/').next().unwrap_or(link);
    u8::from(
        OPAQUE_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(*prefix)),
    )
}

/// The `/dev/disk/by-id` and `/dev/disk/by-path` links of a machine.
///
/// Each map goes from the link's file name to the device node it resolves
/// to, for example `"ata-Example_SSD_S123" -> "/dev/sda"`. Only the last
/// path component of the target is used, so `../../sda` works too.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskLinks {
    /// Links in `/dev/disk/by-id`.
    #[serde(default)]
    pub by_id: BTreeMap<String, String>,
    /// Links in `/dev/disk/by-path`.
    #[serde(default)]
    pub by_path: BTreeMap<String, String>,
}

fn kernel_name_of(target: &str) -> &str {
    target.rsplit('/').next().unwrap_or(target)
}

fn links_to(map: &BTreeMap<String, String>, kernel_name: &str, directory: &str) -> Vec<String> {
    map.iter()
        .filter(|(_, target)| kernel_name_of(target) == kernel_name)
        .map(|(name, _)| format!("{directory}/{name}"))
        .collect()
}

/// Everything the installer knows about the machine's block devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    /// Format version of this structure when written to a file.
    pub schema_version: u32,
    /// Top-level devices: disks, loop devices, optical drives.
    pub devices: Vec<BlockDevice>,
}

impl Inventory {
    /// Builds the inventory from what a probe supplies.
    pub fn probe(probe: &dyn Probe) -> Result<Inventory, InventoryError> {
        let lsblk = probe.lsblk_json()?;
        let links = probe.disk_links()?;
        Inventory::from_lsblk_json(&lsblk, &links)
    }

    /// Parses the output of `lsblk --json --bytes --output-all` and attaches
    /// the stable links.
    pub fn from_lsblk_json(text: &str, links: &DiskLinks) -> Result<Inventory, InventoryError> {
        let root: Value = serde_json::from_str(text)
            .map_err(|error| malformed("lsblk output", &error.to_string()))?;
        let entries = root
            .get("blockdevices")
            .and_then(Value::as_array)
            .ok_or_else(|| malformed("lsblk output", "there is no \"blockdevices\" array"))?;
        let mut devices = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let location = format!("blockdevices[{index}]");
            devices.push(parse_node(entry, links, &location, 0)?);
        }
        Ok(Inventory {
            schema_version: INVENTORY_SCHEMA_VERSION,
            devices,
        })
    }

    /// The whole disks: top-level devices of kind [`BlockKind::Disk`].
    pub fn disks(&self) -> Vec<&BlockDevice> {
        self.devices
            .iter()
            .filter(|device| device.kind == BlockKind::Disk)
            .collect()
    }

    /// Every node in the inventory, depth first.
    pub fn all_nodes(&self) -> Vec<&BlockDevice> {
        self.devices.iter().flat_map(BlockDevice::walk).collect()
    }

    /// True when `node` is one of this inventory's whole disks.
    pub fn is_whole_disk(&self, node: &BlockDevice) -> bool {
        node.kind == BlockKind::Disk && self.devices.iter().any(|device| std::ptr::eq(device, node))
    }
}

fn parse_node(
    entry: &Value,
    links: &DiskLinks,
    location: &str,
    depth: usize,
) -> Result<BlockDevice, InventoryError> {
    if depth > MAX_DEPTH {
        return Err(malformed(location, "the device tree is nested too deeply"));
    }
    let object = entry
        .as_object()
        .ok_or_else(|| malformed(location, "the entry is not a JSON object"))?;
    let name =
        text(object, "name").ok_or_else(|| malformed(location, "the entry has no \"name\""))?;
    let size_bytes = number(object, "size").ok_or_else(|| {
        malformed(
            location,
            "the entry has no size in bytes; lsblk must be run with --bytes",
        )
    })?;
    let kernel_name = text(object, "kname").unwrap_or_else(|| name.clone());
    let path = text(object, "path").unwrap_or_else(|| format!("/dev/{kernel_name}"));
    let kind = BlockKind::from_lsblk(text(object, "type").as_deref(), &name);

    let mut mountpoints = Vec::new();
    if let Some(Value::Array(items)) = object.get("mountpoints") {
        for item in items {
            let mountpoint = item.as_str().map(str::trim).unwrap_or("");
            if !mountpoint.is_empty() {
                mountpoints.push(mountpoint.to_owned());
            }
        }
    }
    if let Some(single) = text(object, "mountpoint")
        && !mountpoints.contains(&single)
    {
        mountpoints.push(single);
    }

    let mut children = Vec::new();
    if let Some(Value::Array(items)) = object.get("children") {
        for (index, item) in items.iter().enumerate() {
            let child_location = format!("{location}.children[{index}]");
            children.push(parse_node(item, links, &child_location, depth + 1)?);
        }
    }

    let mut by_id = links_to(&links.by_id, &kernel_name, BY_ID_DIR);
    if let Some(id_link) = text(object, "id-link") {
        by_id.push(format!("{BY_ID_DIR}/{id_link}"));
    }
    by_id.sort();
    by_id.dedup();
    let mut by_path = links_to(&links.by_path, &kernel_name, BY_PATH_DIR);
    by_path.sort();

    Ok(BlockDevice {
        kind,
        size_bytes,
        model: text(object, "model"),
        vendor: text(object, "vendor"),
        serial: text(object, "serial"),
        wwn: text(object, "wwn"),
        transport: Transport::from_lsblk(text(object, "tran").as_deref()),
        logical_sector_size: sector_size(object, "log-sec"),
        physical_sector_size: sector_size(object, "phy-sec"),
        removable: flag(object, "rm"),
        hotplug: flag(object, "hotplug"),
        read_only: flag(object, "ro"),
        rotational: flag(object, "rota"),
        partition_table: text(object, "pttype"),
        partition_number: number(object, "partn").and_then(|value| u32::try_from(value).ok()),
        start_sector: number(object, "start"),
        partition_type: text(object, "parttype").map(|value| value.to_ascii_lowercase()),
        partition_type_name: text(object, "parttypename"),
        partition_label: text(object, "partlabel"),
        partition_uuid: text(object, "partuuid"),
        filesystem: text(object, "fstype"),
        filesystem_label: text(object, "label"),
        filesystem_uuid: text(object, "uuid"),
        mountpoints,
        by_id,
        by_path,
        children,
        name,
        kernel_name,
        path,
    })
}

fn text(object: &Map<String, Value>, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn number(object: &Map<String, Value>, key: &str) -> Option<u64> {
    match object.get(key) {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value)) => value.trim().parse().ok(),
        _ => None,
    }
}

fn flag(object: &Map<String, Value>, key: &str) -> bool {
    match object.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_u64() == Some(1),
        Some(Value::String(value)) => matches!(value.trim(), "1" | "true"),
        _ => false,
    }
}

fn sector_size(object: &Map<String, Value>, key: &str) -> u32 {
    number(object, key)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

fn malformed(location: &str, reason: &str) -> InventoryError {
    InventoryError::Malformed {
        location: location.to_owned(),
        reason: reason.to_owned(),
    }
}

/// Why the inventory could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    /// The probe could not supply its data.
    Unreadable {
        /// What was being read: a file name or a command.
        what: String,
        /// The underlying reason.
        reason: String,
    },
    /// The data is not lsblk JSON in the expected shape.
    Malformed {
        /// Where in the data the problem is.
        location: String,
        /// What is wrong.
        reason: String,
    },
    /// Another input file (links or host facts) is not in the expected format.
    InvalidFile {
        /// The file.
        what: String,
        /// What is wrong.
        reason: String,
    },
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InventoryError::Unreadable { what, .. } => write!(
                f,
                "The installer could not read {what}. Check that it exists and is a regular file."
            ),
            InventoryError::Malformed { .. } => f.write_str(
                "The storage inventory data is not in the expected format. \
                 It must be the output of lsblk with --json, --bytes and --output-all.",
            ),
            InventoryError::InvalidFile { what, .. } => write!(
                f,
                "The file {what} is not in the expected format. Check it against the \
                 installer documentation."
            ),
        }
    }
}

impl std::error::Error for InventoryError {}

impl Fault for InventoryError {
    fn code(&self) -> InstallCode {
        match self {
            InventoryError::Unreadable { .. } => InstallCode::InventoryUnreadable,
            InventoryError::Malformed { .. } | InventoryError::InvalidFile { .. } => {
                InstallCode::InventoryMalformed
            }
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            InventoryError::Unreadable { what, reason } => format!("source: {what}; {reason}"),
            InventoryError::Malformed { location, reason } => format!("at {location}: {reason}"),
            InventoryError::InvalidFile { what, reason } => format!("{what}: {reason}"),
        }
    }
}

/// Where the inventory's raw data comes from.
///
/// Implementations supply data; they never interpret it. Tests use
/// [`StaticProbe`] and [`FileProbe`]. The probe that asks the running system
/// is [`crate::system_probe::SystemProbe`].
pub trait Probe {
    /// The JSON text printed by `lsblk --json --bytes --output-all`.
    fn lsblk_json(&self) -> Result<String, InventoryError>;

    /// The stable device links.
    fn disk_links(&self) -> Result<DiskLinks, InventoryError>;
}

/// A probe that returns data it was given.
#[derive(Debug, Clone, Default)]
pub struct StaticProbe {
    /// The lsblk JSON text.
    pub lsblk_json: String,
    /// The stable device links.
    pub links: DiskLinks,
}

impl Probe for StaticProbe {
    fn lsblk_json(&self) -> Result<String, InventoryError> {
        Ok(self.lsblk_json.clone())
    }

    fn disk_links(&self) -> Result<DiskLinks, InventoryError> {
        Ok(self.links.clone())
    }
}

/// A probe that reads saved lsblk output, and optionally a links file, from
/// regular files. It refuses device nodes, directories and oversized files.
#[derive(Debug, Clone)]
pub struct FileProbe {
    lsblk: PathBuf,
    links: Option<PathBuf>,
}

impl FileProbe {
    /// A probe over a saved lsblk JSON file and an optional links file.
    pub fn new(lsblk: &Path, links: Option<&Path>) -> FileProbe {
        FileProbe {
            lsblk: lsblk.to_path_buf(),
            links: links.map(Path::to_path_buf),
        }
    }
}

impl Probe for FileProbe {
    fn lsblk_json(&self) -> Result<String, InventoryError> {
        read_input_file(&self.lsblk)
    }

    fn disk_links(&self) -> Result<DiskLinks, InventoryError> {
        match &self.links {
            Some(path) => read_json_file(path),
            None => Ok(DiskLinks::default()),
        }
    }
}

/// Reads a regular file of bounded size as UTF-8 text.
///
/// Device nodes and directories are refused, so a mistyped argument can never
/// make the installer read a disk.
pub fn read_input_file(path: &Path) -> Result<String, InventoryError> {
    let unreadable = |reason: String| InventoryError::Unreadable {
        what: path.display().to_string(),
        reason,
    };
    let metadata = fs::metadata(path).map_err(|error| unreadable(error.to_string()))?;
    if !metadata.is_file() {
        return Err(unreadable(
            "not a regular file; device nodes and directories are never read".to_owned(),
        ));
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(unreadable(format!(
            "larger than the input limit of {MAX_INPUT_BYTES} bytes"
        )));
    }
    fs::read_to_string(path).map_err(|error| unreadable(error.to_string()))
}

/// Reads a regular file of bounded size and parses it as JSON.
pub fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, InventoryError> {
    let text = read_input_file(path)?;
    serde_json::from_str(&text).map_err(|error| InventoryError::InvalidFile {
        what: path.display().to_string(),
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{links, oddities, workstation};

    fn disk<'a>(inventory: &'a Inventory, name: &str) -> &'a BlockDevice {
        inventory
            .devices
            .iter()
            .find(|device| device.name == name)
            .unwrap_or_else(|| panic!("fixture has no device {name}"))
    }

    #[test]
    fn parses_the_windows_nvme_disk() {
        let inventory = workstation();
        let nvme = disk(&inventory, "nvme0n1");
        assert_eq!(nvme.kind, BlockKind::Disk);
        assert_eq!(nvme.size_bytes, 1_000_204_886_016);
        assert_eq!(nvme.model.as_deref(), Some("Samsung SSD 980 PRO 1TB"));
        assert_eq!(nvme.serial.as_deref(), Some("S5GXNX0T123456A"));
        assert_eq!(nvme.wwn.as_deref(), Some("eui.002538b111b2c3d4"));
        assert_eq!(nvme.transport, Transport::Nvme);
        assert_eq!(nvme.logical_sector_size, 512);
        assert_eq!(nvme.physical_sector_size, 512);
        assert_eq!(nvme.partition_table.as_deref(), Some("gpt"));
        assert!(!nvme.removable);
        assert_eq!(nvme.children.len(), 4);
        assert_eq!(
            nvme.preferred_by_id(),
            Some("/dev/disk/by-id/nvme-Samsung_SSD_980_PRO_1TB_S5GXNX0T123456A")
        );
        assert_eq!(nvme.by_path, ["/dev/disk/by-path/pci-0000:01:00.0-nvme-1"]);

        let esp = &nvme.children[0];
        assert_eq!(esp.kind, BlockKind::Partition);
        assert_eq!(esp.partition_number, Some(1));
        assert_eq!(esp.partition_type.as_deref(), Some(gpt::ESP));
        assert_eq!(esp.filesystem.as_deref(), Some("vfat"));
        let msr = &nvme.children[1];
        assert_eq!(msr.partition_type.as_deref(), Some(gpt::MICROSOFT_RESERVED));
        assert_eq!(msr.filesystem, None);
        let windows = &nvme.children[2];
        assert_eq!(windows.filesystem.as_deref(), Some("ntfs"));
        assert_eq!(windows.filesystem_label.as_deref(), Some("Windows"));

        assert!(nvme.active_uses().is_empty());
        assert!(!nvme.is_live_medium());
    }

    #[test]
    fn detects_windows_and_linux_installations() {
        let inventory = workstation();
        let windows = disk(&inventory, "nvme0n1").detected_operating_systems();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].family, OsFamily::Windows);
        assert_eq!(windows[0].confidence, Confidence::Likely);
        assert_eq!(windows[0].headline(), "Windows (likely)");
        assert!(
            windows[0]
                .evidence
                .iter()
                .any(|item| item.contains("NTFS volume /dev/nvme0n1p3"))
        );

        let linux = disk(&inventory, "sda").detected_operating_systems();
        assert_eq!(linux.len(), 1);
        assert_eq!(linux[0].family, OsFamily::Linux);
        assert_eq!(linux[0].confidence, Confidence::Likely);

        assert!(
            disk(&inventory, "sdb")
                .detected_operating_systems()
                .is_empty()
        );
    }

    #[test]
    fn recognises_the_live_medium_and_its_mounts() {
        let inventory = workstation();
        let stick = disk(&inventory, "sdb");
        assert_eq!(stick.transport, Transport::Usb);
        assert!(stick.removable);
        assert!(stick.is_live_medium());
        let uses = stick.active_uses();
        assert!(
            uses.iter()
                .any(|active| active.reason == "mounted at /run/initramfs/live")
        );
        assert!(!disk(&inventory, "sda").is_live_medium());
    }

    #[test]
    fn keeps_the_empty_card_reader_but_marks_it_without_medium() {
        let inventory = workstation();
        let reader = disk(&inventory, "sdc");
        assert_eq!(reader.kind, BlockKind::Disk);
        assert!(!reader.has_medium());
        assert!(reader.children.is_empty());
        assert!(disk(&inventory, "nvme0n1").has_medium());
    }

    #[test]
    fn whole_disks_exclude_loop_and_compressed_ram_devices() {
        let inventory = workstation();
        let names: Vec<&str> = inventory
            .disks()
            .iter()
            .map(|device| device.name.as_str())
            .collect();
        assert_eq!(names, ["nvme0n1", "sda", "sdb", "sdc"]);
        assert_eq!(disk(&inventory, "loop0").kind, BlockKind::Loop);
        let zram = disk(&inventory, "zram0");
        assert_eq!(zram.kind, BlockKind::CompressedRam);
        assert_eq!(zram.active_uses()[0].reason, "active swap");
        assert!(!inventory.is_whole_disk(zram));
        assert!(!inventory.is_whole_disk(&disk(&inventory, "sda").children[0]));
        assert!(inventory.is_whole_disk(disk(&inventory, "sda")));
    }

    #[test]
    fn a_disk_without_a_serial_has_no_by_id_link() {
        let inventory = oddities();
        let plain = disk(&inventory, "sda");
        assert_eq!(plain.serial, None);
        assert_eq!(plain.wwn, None);
        assert!(plain.by_id.is_empty());
        assert_eq!(
            plain.by_path,
            ["/dev/disk/by-path/pci-0000:00:14.0-usb-0:3:1.0-scsi-0:0:0:0"]
        );
        assert_eq!(plain.display_model(), "Generic Mass Storage");
    }

    #[test]
    fn lvm_on_luks_is_parsed_as_a_tree_and_counts_as_in_use() {
        let inventory = oddities();
        let nvme = disk(&inventory, "nvme1n1");
        let luks_partition = &nvme.children[2];
        assert_eq!(luks_partition.filesystem.as_deref(), Some("crypto_LUKS"));
        let opened = &luks_partition.children[0];
        assert_eq!(opened.kind, BlockKind::Crypt);
        assert_eq!(opened.kernel_name, "dm-0");
        assert_eq!(opened.filesystem.as_deref(), Some("LVM2_member"));
        assert_eq!(opened.children.len(), 2);
        assert!(
            opened
                .children
                .iter()
                .all(|volume| volume.kind == BlockKind::Lvm)
        );
        assert_eq!(nvme.walk().len(), 7);

        let reasons: Vec<String> = nvme
            .active_uses()
            .into_iter()
            .map(|active| active.reason)
            .collect();
        assert_eq!(
            reasons,
            [
                "active encrypted volume",
                "active LVM volume",
                "active LVM volume"
            ]
        );
        let linux = nvme.detected_operating_systems();
        assert_eq!(linux[0].family, OsFamily::Linux);
        assert_eq!(linux[0].confidence, Confidence::Likely);
    }

    #[test]
    fn multipath_paths_share_identifiers_and_a_map() {
        let inventory = oddities();
        let first = disk(&inventory, "sdd");
        let second = disk(&inventory, "sde");
        assert_eq!(first.wwn, second.wwn);
        assert_eq!(first.serial, second.serial);
        assert_eq!(first.children[0].kind, BlockKind::Multipath);
        assert_eq!(first.children[0].name, second.children[0].name);
        assert_eq!(first.active_uses()[0].reason, "active multipath map");
    }

    #[test]
    fn accepts_older_lsblk_value_types() {
        let json = r#"{"blockdevices": [
            {"name": "sda", "size": "500107862016", "type": "disk", "rm": "0", "ro": "0",
             "rota": "1", "log-sec": "512", "phy-sec": "4096", "tran": "sata",
             "model": "  Old Disk  ", "serial": 12345, "mountpoint": null,
             "children": [
                {"name": "sda1", "size": "500106813440", "type": "part", "rm": 0, "ro": 1,
                 "fstype": "ext4", "mountpoint": "/mnt/old", "parttype": "0x83"}
             ]}
        ]}"#;
        let inventory = Inventory::from_lsblk_json(json, &DiskLinks::default()).unwrap();
        let sda = &inventory.devices[0];
        assert_eq!(sda.size_bytes, 500_107_862_016);
        assert_eq!(sda.model.as_deref(), Some("Old Disk"));
        assert_eq!(sda.serial.as_deref(), Some("12345"));
        assert!(sda.rotational);
        assert!(!sda.removable);
        assert_eq!(sda.physical_sector_size, 4096);
        assert_eq!(sda.path, "/dev/sda");
        assert_eq!(sda.kernel_name, "sda");
        let part = &sda.children[0];
        assert!(part.read_only);
        assert_eq!(part.mountpoints, ["/mnt/old"]);
        assert_eq!(part.partition_type.as_deref(), Some("0x83"));
        assert_eq!(part.logical_sector_size, 0);
    }

    #[test]
    fn rejects_data_that_is_not_lsblk_json() {
        let none = DiskLinks::default();
        for (input, expected_code) in [
            ("", InstallCode::InventoryMalformed),
            ("not json", InstallCode::InventoryMalformed),
            ("{}", InstallCode::InventoryMalformed),
            (r#"{"blockdevices": {}}"#, InstallCode::InventoryMalformed),
            (r#"{"blockdevices": [7]}"#, InstallCode::InventoryMalformed),
            (
                r#"{"blockdevices": [{"size": 5}]}"#,
                InstallCode::InventoryMalformed,
            ),
            (
                r#"{"blockdevices": [{"name": "sda"}]}"#,
                InstallCode::InventoryMalformed,
            ),
            (
                r#"{"blockdevices": [{"name": "sda", "size": "465.8G"}]}"#,
                InstallCode::InventoryMalformed,
            ),
            (
                r#"{"blockdevices": [{"name": "sda", "size": -1}]}"#,
                InstallCode::InventoryMalformed,
            ),
        ] {
            let error = Inventory::from_lsblk_json(input, &none).unwrap_err();
            assert_eq!(error.code(), expected_code, "{input}");
            assert!(!error.technical_detail().is_empty());
        }
    }

    #[test]
    fn rejects_a_tree_nested_beyond_the_limit() {
        let mut node = String::from(r#"{"name": "leaf", "size": 1}"#);
        for level in 0..=MAX_DEPTH {
            node = format!(r#"{{"name": "n{level}", "size": 1, "children": [{node}]}}"#);
        }
        let json = format!(r#"{{"blockdevices": [{node}]}}"#);
        let error = Inventory::from_lsblk_json(&json, &DiskLinks::default()).unwrap_err();
        assert!(error.technical_detail().contains("nested too deeply"));
    }

    #[test]
    fn empty_inventory_is_valid() {
        let inventory =
            Inventory::from_lsblk_json(r#"{"blockdevices": []}"#, &DiskLinks::default()).unwrap();
        assert!(inventory.devices.is_empty());
        assert!(inventory.disks().is_empty());
        assert_eq!(inventory.schema_version, INVENTORY_SCHEMA_VERSION);
    }

    #[test]
    fn links_attach_by_kernel_name_in_any_target_form() {
        let json = r#"{"blockdevices": [
            {"name": "sda", "size": 1000, "type": "disk", "id-link": "ata-Disk_A"},
            {"name": "sdab", "size": 1000, "type": "disk"}
        ]}"#;
        let mut links = DiskLinks::default();
        links
            .by_id
            .insert("ata-Disk_A".to_owned(), "../../sda".to_owned());
        links
            .by_id
            .insert("wwn-0x5000000000000001".to_owned(), "/dev/sda".to_owned());
        links
            .by_id
            .insert("ata-Disk_B".to_owned(), "sdab".to_owned());
        let inventory = Inventory::from_lsblk_json(json, &links).unwrap();
        assert_eq!(
            inventory.devices[0].by_id,
            [
                "/dev/disk/by-id/ata-Disk_A",
                "/dev/disk/by-id/wwn-0x5000000000000001"
            ]
        );
        assert_eq!(
            inventory.devices[0].preferred_by_id(),
            Some("/dev/disk/by-id/ata-Disk_A")
        );
        assert_eq!(inventory.devices[1].by_id, ["/dev/disk/by-id/ata-Disk_B"]);
    }

    #[test]
    fn inventory_round_trips_through_its_own_json() {
        let inventory = workstation();
        let json = serde_json::to_string(&inventory).unwrap();
        let again: Inventory = serde_json::from_str(&json).unwrap();
        assert_eq!(again, inventory);
    }

    #[test]
    fn static_probe_and_parser_agree() {
        let probe = StaticProbe {
            lsblk_json: crate::testing::WORKSTATION_LSBLK.to_owned(),
            links: links(crate::testing::WORKSTATION_LINKS),
        };
        assert_eq!(Inventory::probe(&probe).unwrap(), workstation());
    }

    #[test]
    fn links_file_rejects_unknown_keys() {
        assert!(serde_json::from_str::<DiskLinks>(r#"{"by_id": {}, "by_uuid": {}}"#).is_err());
        assert!(serde_json::from_str::<DiskLinks>("{}").is_ok());
    }
}
