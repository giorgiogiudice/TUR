// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The volume model and the inventory built from probe data.
//!
//! A [`Volume`] is one filesystem or container as the policy sees it: its
//! signature, identity, where it lives and the state facts that matter for
//! safety. An [`Inventory`] is every volume found in one probe snapshot, each
//! with a unique [`VolumeId`].
//!
//! Identity rules (specification §11.4):
//!
//! 1. `fs-<uuid>` from the filesystem UUID, when there is a usable one.
//! 2. `part-<partuuid>` from the partition UUID.
//! 3. `dev-<hash>` from the disk's WWN or serial number, the partition offset
//!    and the size.
//! 4. `tmp-<hash>` from the device path. This one is not stable across
//!    reconnects and is flagged as such.
//!
//! Two devices that claim the same identifier are never treated as one volume
//! unless they are members of one multi-device set (a Btrfs filesystem, an MD
//! RAID array or a ZFS pool) and each has its own `UUID_SUB`. Otherwise both
//! are flagged [`Volume::duplicate_identity`], which the decision refuses,
//! and each falls back to the next rule so that it can still be addressed.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use crate::filesystem::Filesystem;
use crate::probe::{
    LsblkDevice, PROP_HFS_JOURNAL, PROP_NTFS_STATE, PROP_UUID_SUB, Probe, ProbeError,
};

string_enum! {
    /// The state of an NTFS volume. Only `Clean` is safe to write.
    pub enum NtfsState {
        /// Cleanly shut down, not hibernated.
        Clean => "clean",
        /// The NTFS dirty flag is set.
        Dirty => "dirty",
        /// Windows is hibernated or was shut down with Fast Startup.
        Hibernated => "hibernated",
        /// The state could not be determined. Treated as unsafe.
        Unknown => "unknown",
    }
}

string_enum! {
    /// Whether an HFS+ volume has a journal.
    pub enum HfsJournal {
        /// The volume is journalled.
        Journaled => "journalled",
        /// The volume is known not to be journalled.
        NotJournaled => "not-journalled",
        /// The journal state could not be determined. Treated as journalled.
        Unknown => "unknown",
    }
}

string_enum! {
    /// How a volume relates to encryption.
    pub enum Encryption {
        /// Not encrypted at the block level, as far as the probe can tell.
        None => "none",
        /// An encrypted container with no unlocked mapping.
        Locked => "locked",
        /// An encrypted container whose mapping exists.
        Unlocked => "unlocked",
        /// The cleartext mapping of an encrypted container.
        Mapping => "mapping",
    }
}

string_enum! {
    /// What a volume identifier was derived from.
    pub enum IdSource {
        /// The filesystem UUID.
        FilesystemUuid => "filesystem-uuid",
        /// The partition UUID.
        PartitionUuid => "partition-uuid",
        /// The disk's WWN or serial number, the partition offset and the size.
        HardwareIdentity => "hardware-identity",
        /// The device path. Not stable across reconnects.
        Unstable => "unstable",
    }
}

impl IdSource {
    const fn prefix(self) -> &'static str {
        match self {
            IdSource::FilesystemUuid => "fs-",
            IdSource::PartitionUuid => "part-",
            IdSource::HardwareIdentity => "dev-",
            IdSource::Unstable => "tmp-",
        }
    }
}

/// A volume identifier: lower-case ASCII letters, digits and hyphens only, so
/// that it is always safe as a single path component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct VolumeId(String);

impl VolumeId {
    /// The longest identifier accepted, in bytes.
    pub const MAX_LEN: usize = 96;

    /// Validates an identifier. Returns `None` unless the text has a known
    /// prefix followed by at least one character, and consists only of
    /// lower-case ASCII letters, digits and hyphens.
    pub fn parse(text: &str) -> Option<VolumeId> {
        let known_prefix = IdSource::all().iter().any(|source| {
            text.strip_prefix(source.prefix()).is_some_and(|rest| !rest.is_empty())
        });
        let safe = text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        (known_prefix && safe && text.len() <= VolumeId::MAX_LEN).then(|| VolumeId(text.to_owned()))
    }

    /// The identifier as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VolumeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One filesystem or container, with the facts the policy needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Volume {
    /// The UOS volume identifier, unique within an inventory.
    pub id: VolumeId,
    /// What the identifier was derived from.
    pub id_source: IdSource,
    /// The block devices that hold the volume. More than one only for a
    /// multi-device set: a Btrfs filesystem, an MD RAID array or a ZFS pool.
    pub devices: Vec<String>,
    /// The recognised filesystem or container.
    pub filesystem: Filesystem,
    /// The raw signature the probe reported, if any.
    pub signature: Option<String>,
    /// The filesystem version, if reported.
    pub fs_version: Option<String>,
    /// The filesystem UUID, as reported.
    pub uuid: Option<String>,
    /// The filesystem label, exactly as reported. It may contain any
    /// character; use the display name for paths and terminals.
    pub label: Option<String>,
    /// The partition UUID, as reported.
    pub partition_uuid: Option<String>,
    /// The partition type name, as reported.
    pub partition_type: Option<String>,
    /// The size in bytes; for a multi-device volume, the sum of its devices.
    pub size_bytes: u64,
    /// The transport of the underlying disk, for example `usb` or `nvme`.
    pub transport: Option<String>,
    /// Whether the volume is on removable or hot-pluggable media.
    pub removable: bool,
    /// Whether the volume is in an optical drive.
    pub optical: bool,
    /// Whether the device, or a device beneath it, is read-only.
    pub device_read_only: bool,
    /// Whether the volume is part of the running system's own storage.
    pub system: bool,
    /// How the volume relates to encryption.
    pub encryption: Encryption,
    /// The NTFS state; present exactly when the filesystem is NTFS.
    pub ntfs_state: Option<NtfsState>,
    /// The HFS+ journal state; present exactly when the filesystem is HFS+.
    pub hfs_journal: Option<HfsJournal>,
    /// Whether another connected device claims the same identity.
    pub duplicate_identity: bool,
    /// Where the volume is mounted now.
    pub mountpoints: Vec<String>,
    /// Filesystem roots of the current mounts; for Btrfs, subvolume paths.
    pub subvolumes: Vec<String>,
}

/// Mount points that mark a device as the running system's own storage.
const SYSTEM_MOUNTPOINTS: &[&str] = &[
    "/",
    "/sysroot",
    "/boot",
    "/boot/efi",
    "/efi",
    "/usr",
    "/etc",
    "/var",
    "/var/home",
    "/home",
    "/root",
    "/var/roothome",
    "[SWAP]",
];

/// Whether a mount point belongs to the running system.
pub fn is_system_mountpoint(mountpoint: &str) -> bool {
    SYSTEM_MOUNTPOINTS.contains(&mountpoint) || mountpoint.starts_with("/sysroot/")
}

/// MBR partition types of extended-partition containers, which hold no data
/// of their own.
const EXTENDED_PARTITION_TYPES: &[&str] = &["0x5", "0xf", "0x85"];

/// Normalises a UUID-like identifier for use in a volume identifier.
///
/// Returns the lower-cased text if it is 4 to 64 characters of ASCII letters,
/// digits and hyphens and is not made only of zeros and hyphens. Anything
/// else is unusable and the caller falls back to the next identity rule.
pub fn normalise_identifier(raw: &str) -> Option<String> {
    let lowered = raw.trim().to_ascii_lowercase();
    let length_ok = (4..=64).contains(&lowered.len());
    let safe = lowered
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    let meaningful = lowered.bytes().any(|byte| byte != b'0' && byte != b'-');
    (length_ok && safe && meaningful).then_some(lowered)
}

/// 64-bit FNV-1a. Used only to derive identifiers; not a security boundary.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn non_empty(value: Option<&String>) -> Option<&str> {
    value.map(|text| text.trim()).filter(|text| !text.is_empty())
}

struct Node<'a> {
    device: &'a LsblkDevice,
    path: String,
    parents: Vec<usize>,
    children: Vec<usize>,
}

fn device_path(device: &LsblkDevice) -> String {
    if let Some(path) = non_empty(device.path.as_ref()) {
        return path.to_owned();
    }
    let name = non_empty(device.kname.as_ref()).or(non_empty(device.name.as_ref())).unwrap_or("");
    format!("/dev/{name}")
}

/// Flattens the lsblk tree. A device that appears under several parents (a
/// logical volume spanning two physical volumes, for example) becomes one
/// node with several parents.
fn flatten(roots: &[LsblkDevice]) -> Vec<Node<'_>> {
    let mut nodes: Vec<Node<'_>> = Vec::new();
    let mut index_by_path: BTreeMap<String, usize> = BTreeMap::new();
    let mut stack: Vec<(&LsblkDevice, Option<usize>)> =
        roots.iter().rev().map(|device| (device, None)).collect();
    while let Some((device, parent)) = stack.pop() {
        let path = device_path(device);
        let known = index_by_path.get(&path).copied();
        let index = match known {
            Some(index) => index,
            None => {
                let index = nodes.len();
                index_by_path.insert(path.clone(), index);
                nodes.push(Node { device, path, parents: Vec::new(), children: Vec::new() });
                for child in device.children.iter().rev() {
                    stack.push((child, Some(index)));
                }
                index
            }
        };
        if let Some(parent) = parent {
            if !nodes[index].parents.contains(&parent) {
                nodes[index].parents.push(parent);
            }
            if !nodes[parent].children.contains(&index) {
                nodes[parent].children.push(index);
            }
        }
    }
    nodes
}

/// Every node reachable from `start`, excluding `start`: its ancestors when
/// `upwards` is true, otherwise its descendants.
fn reachable(nodes: &[Node<'_>], start: usize, upwards: bool) -> Vec<usize> {
    let mut seen: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = neighbours(nodes, start, upwards).to_vec();
    while let Some(index) = stack.pop() {
        if index != start && !seen.contains(&index) {
            seen.push(index);
            stack.extend_from_slice(neighbours(nodes, index, upwards));
        }
    }
    seen
}

/// The parents of a node when `upwards` is true, otherwise its children.
fn neighbours<'n>(nodes: &'n [Node<'_>], index: usize, upwards: bool) -> &'n [usize] {
    if upwards { &nodes[index].parents } else { &nodes[index].children }
}

fn mountpoints_of(device: &LsblkDevice) -> Vec<String> {
    let mut mountpoints = device.mountpoints.clone();
    if let Some(single) = non_empty(device.mountpoint.as_ref())
        && !mountpoints.iter().any(|existing| existing == single)
    {
        mountpoints.push(single.to_owned());
    }
    mountpoints
}

/// A volume under construction, with every identifier it could use.
struct Candidate {
    volume: Volume,
    /// Identifiers in order of preference. Never empty: the last one is
    /// derived from the device path.
    chain: Vec<(IdSource, String)>,
    level: usize,
    uuid_sub: Option<String>,
}

impl Candidate {
    fn current_id(&self) -> &str {
        &self.chain[self.level].1
    }
}

fn build_candidate(nodes: &[Node<'_>], index: usize, probe: &dyn Probe) -> Option<Candidate> {
    let node = &nodes[index];
    let device = node.device;
    let properties = probe.properties(&node.path);
    let device_type = non_empty(device.device_type.as_ref()).unwrap_or("");

    let signature = non_empty(device.fstype.as_ref())
        .or_else(|| non_empty(properties.get("TYPE")))
        .map(str::to_owned);
    if signature.as_deref() == Some("swap") {
        return None;
    }
    let has_partitions = node
        .children
        .iter()
        .any(|&child| non_empty(nodes[child].device.device_type.as_ref()) == Some("part"));
    if signature.is_none() {
        // Not a volume: a partitioned disk, an empty drive or loop device, or
        // an extended-partition container.
        let extended = non_empty(device.parttype.as_ref())
            .is_some_and(|code| EXTENDED_PARTITION_TYPES.contains(&code));
        let empty_drive = device.size == Some(0);
        if !node.children.is_empty() || device_type == "loop" || empty_drive || extended {
            return None;
        }
    } else if has_partitions {
        // A signature on a whole device that also has partitions (a hybrid
        // ISO image written to a USB stick) describes the same bytes as its
        // first partition. The partitions are the volumes.
        return None;
    }
    let filesystem = Filesystem::from_signature(signature.as_deref());

    let ancestors = reachable(nodes, index, true);
    let descendants = reachable(nodes, index, false);
    let lineage = || std::iter::once(index).chain(ancestors.iter().copied());

    let removable = lineage().any(|i| {
        let d = nodes[i].device;
        d.rm == Some(true) || d.hotplug == Some(true) || d.tran.as_deref() == Some("usb")
    });
    let optical = lineage().any(|i| nodes[i].device.device_type.as_deref() == Some("rom"));
    let device_read_only = optical || lineage().any(|i| nodes[i].device.ro == Some(true));
    let transport = lineage().find_map(|i| non_empty(nodes[i].device.tran.as_ref())).map(str::to_owned);

    let mountpoints = mountpoints_of(device);
    let system = std::iter::once(index)
        .chain(descendants.iter().copied())
        .any(|i| mountpoints_of(nodes[i].device).iter().any(|m| is_system_mountpoint(m)));

    let inside_container = ancestors.iter().any(|&i| {
        matches!(
            Filesystem::from_signature(non_empty(nodes[i].device.fstype.as_ref())),
            Filesystem::Luks | Filesystem::BitLocker
        )
    });
    let encryption = match filesystem {
        Filesystem::Luks if node.children.is_empty() => Encryption::Locked,
        Filesystem::Luks => Encryption::Unlocked,
        _ if device_type == "crypt" || inside_container => Encryption::Mapping,
        _ => Encryption::None,
    };

    let ntfs_state = (filesystem == Filesystem::Ntfs).then(|| {
        properties
            .get(PROP_NTFS_STATE)
            .and_then(|text| NtfsState::from_text(text.trim()))
            .unwrap_or(NtfsState::Unknown)
    });
    let hfs_journal = (filesystem == Filesystem::HfsPlus).then(|| {
        properties
            .get(PROP_HFS_JOURNAL)
            .and_then(|text| HfsJournal::from_text(text.trim()))
            .unwrap_or(HfsJournal::Unknown)
    });

    let uuid = non_empty(device.uuid.as_ref())
        .or_else(|| non_empty(properties.get("UUID")))
        .map(str::to_owned);
    let partition_uuid = non_empty(device.partuuid.as_ref()).map(str::to_owned);
    let size_bytes = device.size.unwrap_or(0);

    let mut chain: Vec<(IdSource, String)> = Vec::new();
    if let Some(value) = uuid.as_deref().and_then(normalise_identifier) {
        chain.push((IdSource::FilesystemUuid, format!("fs-{value}")));
    }
    if let Some(value) = partition_uuid.as_deref().and_then(normalise_identifier) {
        chain.push((IdSource::PartitionUuid, format!("part-{value}")));
    }
    let hardware = lineage().find_map(|i| {
        let d = nodes[i].device;
        non_empty(d.wwn.as_ref()).or_else(|| non_empty(d.serial.as_ref())).map(|identity| {
            let model = non_empty(d.model.as_ref()).unwrap_or("");
            format!("{identity}|{model}")
        })
    });
    if let Some(hardware) = hardware {
        let start = device.start.unwrap_or(0);
        let material = format!("{hardware}|{start}|{size_bytes}|{}", filesystem.as_str());
        chain.push((
            IdSource::HardwareIdentity,
            format!("dev-{:016x}", fnv1a64(material.as_bytes())),
        ));
    }
    chain.push((IdSource::Unstable, format!("tmp-{:016x}", fnv1a64(node.path.as_bytes()))));

    let (id_source, id_text) = chain[0].clone();
    let volume = Volume {
        id: VolumeId(id_text),
        id_source,
        devices: vec![node.path.clone()],
        filesystem,
        signature,
        fs_version: non_empty(device.fsver.as_ref()).map(str::to_owned),
        uuid,
        label: device.label.clone().filter(|label| !label.is_empty()),
        partition_uuid,
        partition_type: non_empty(device.parttypename.as_ref()).map(str::to_owned),
        size_bytes,
        transport,
        removable,
        optical,
        device_read_only,
        system,
        encryption,
        ntfs_state,
        hfs_journal,
        duplicate_identity: false,
        mountpoints,
        subvolumes: device.fsroots.clone(),
    };
    let uuid_sub = non_empty(properties.get(PROP_UUID_SUB)).map(str::to_ascii_lowercase);
    Some(Candidate { volume, chain, level: 0, uuid_sub })
}

/// Groups candidate indices by their current identifier.
fn groups(candidates: &[Candidate]) -> Vec<Vec<usize>> {
    let mut by_id: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        by_id.entry(candidate.current_id()).or_default().push(index);
    }
    by_id.into_values().collect()
}

/// Whether a signature can legitimately appear on several devices with one
/// UUID: the members of a Btrfs filesystem, an MD RAID array or a ZFS pool
/// share the set's UUID and differ in `UUID_SUB`.
fn spans_devices(filesystem: Filesystem) -> bool {
    matches!(filesystem, Filesystem::Btrfs | Filesystem::RaidMember | Filesystem::Zfs)
}

/// Whether a group of candidates with one identifier is a single multi-device
/// set: all of one signature that can span devices, identified by filesystem
/// UUID, each with its own distinct `UUID_SUB`. Clones of one device share
/// the `UUID_SUB` and do not qualify; neither do devices whose `UUID_SUB` is
/// not known.
fn is_one_set(candidates: &[Candidate], group: &[usize]) -> bool {
    let mut subs: Vec<&str> = Vec::new();
    let Some(&first) = group.first() else {
        return false;
    };
    let filesystem = candidates[first].volume.filesystem;
    for &index in group {
        let candidate = &candidates[index];
        let by_uuid = candidate.chain[candidate.level].0 == IdSource::FilesystemUuid;
        let Some(sub) = candidate.uuid_sub.as_deref() else {
            return false;
        };
        if candidate.volume.filesystem != filesystem || !by_uuid || subs.contains(&sub) {
            return false;
        }
        subs.push(sub);
    }
    group.len() > 1 && spans_devices(filesystem)
}

fn merge_set(mut members: Vec<Volume>) -> Volume {
    members.sort_by(|a, b| a.devices.cmp(&b.devices));
    let mut merged = members.remove(0);
    for member in members {
        merged.devices.extend(member.devices);
        merged.size_bytes = merged.size_bytes.saturating_add(member.size_bytes);
        merged.removable |= member.removable;
        merged.optical |= member.optical;
        merged.device_read_only |= member.device_read_only;
        merged.system |= member.system;
        merged.duplicate_identity |= member.duplicate_identity;
        for mountpoint in member.mountpoints {
            if !merged.mountpoints.contains(&mountpoint) {
                merged.mountpoints.push(mountpoint);
            }
        }
        for subvolume in member.subvolumes {
            if !merged.subvolumes.contains(&subvolume) {
                merged.subvolumes.push(subvolume);
            }
        }
    }
    merged.devices.sort();
    merged
}

/// Every volume found in one probe snapshot, sorted by identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    /// The volumes. Identifiers are unique.
    pub volumes: Vec<Volume>,
}

impl Inventory {
    /// Builds the inventory from a probe.
    pub fn from_probe(probe: &dyn Probe) -> Result<Inventory, ProbeError> {
        let roots = probe.block_devices()?;
        let nodes = flatten(&roots);
        let mut candidates: Vec<Candidate> =
            (0..nodes.len()).filter_map(|index| build_candidate(&nodes, index, probe)).collect();

        // Resolve identifier collisions. Each pass moves colliding candidates
        // one step down their chain, so the loop ends after at most as many
        // passes as the longest chain.
        loop {
            let mut changed = false;
            for group in groups(&candidates) {
                if group.len() < 2 || is_one_set(&candidates, &group) {
                    continue;
                }
                for index in group {
                    let candidate = &mut candidates[index];
                    candidate.volume.duplicate_identity = true;
                    if candidate.level + 1 < candidate.chain.len() {
                        candidate.level += 1;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }

        let final_groups = groups(&candidates);
        let merge_flags: Vec<bool> =
            final_groups.iter().map(|group| is_one_set(&candidates, group)).collect();
        let mut slots: Vec<Option<Candidate>> = candidates.into_iter().map(Some).collect();
        let mut volumes: Vec<Volume> = Vec::new();
        for (group, one_set) in final_groups.into_iter().zip(merge_flags) {
            let mut members: Vec<Volume> = Vec::new();
            for index in group {
                if let Some(candidate) = slots[index].take() {
                    let (source, id) = candidate.chain[candidate.level].clone();
                    let mut volume = candidate.volume;
                    volume.id = VolumeId(id);
                    volume.id_source = source;
                    members.push(volume);
                }
            }
            if one_set {
                volumes.push(merge_set(members));
                continue;
            }
            // More than one member here means two device paths hashed to the
            // same last-resort identifier. Keep them apart and refuse both.
            let collided = members.len() > 1;
            for (position, mut volume) in members.into_iter().enumerate() {
                if collided {
                    volume.duplicate_identity = true;
                }
                if position > 0 {
                    volume.id = VolumeId(format!("{}-{}", volume.id, position + 1));
                }
                volumes.push(volume);
            }
        }
        volumes.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Inventory { volumes })
    }

    /// Finds a volume by identifier.
    pub fn find(&self, id: &str) -> Option<&Volume> {
        self.volumes.iter().find(|volume| volume.id.as_str() == id)
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A plain internal volume of the given filesystem, for unit tests.
    pub(crate) fn volume(filesystem: Filesystem) -> Volume {
        Volume {
            id: VolumeId("fs-0000-test".to_owned()),
            id_source: IdSource::FilesystemUuid,
            devices: vec!["/dev/sdx1".to_owned()],
            filesystem,
            signature: Some(filesystem.as_str().to_owned()),
            fs_version: None,
            uuid: Some("0000-TEST".to_owned()),
            label: Some("Test".to_owned()),
            partition_uuid: None,
            partition_type: None,
            size_bytes: 1 << 30,
            transport: Some("sata".to_owned()),
            removable: false,
            optical: false,
            device_read_only: false,
            system: false,
            encryption: if filesystem == Filesystem::Luks {
                Encryption::Locked
            } else {
                Encryption::None
            },
            ntfs_state: (filesystem == Filesystem::Ntfs).then_some(NtfsState::Clean),
            hfs_journal: (filesystem == Filesystem::HfsPlus).then_some(HfsJournal::NotJournaled),
            duplicate_identity: false,
            mountpoints: Vec::new(),
            subvolumes: Vec::new(),
        }
    }

    /// The same volume with a different identifier.
    pub(crate) fn volume_with_id(filesystem: Filesystem, id: &str) -> Volume {
        let mut volume = volume(filesystem);
        volume.id = VolumeId::parse(id).expect("test identifier is valid");
        volume
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::SnapshotProbe;

    fn inventory(json: &str, properties: &str) -> Inventory {
        let probe = SnapshotProbe::from_lsblk_json(json).unwrap().with_properties(properties).unwrap();
        Inventory::from_probe(&probe).unwrap()
    }

    #[test]
    fn identifiers_are_validated() {
        assert!(VolumeId::parse("fs-1234-abcd").is_some());
        assert!(VolumeId::parse("part-0a1b").is_some());
        assert!(VolumeId::parse("dev-0123456789abcdef").is_some());
        assert!(VolumeId::parse("tmp-0123456789abcdef").is_some());
        for bad in ["", "fs-", "1234", "fs-ABCD", "fs-../etc", "fs-a/b", "fs-a b", "xx-1234"] {
            assert!(VolumeId::parse(bad).is_none(), "{bad:?}");
        }
        let long = format!("fs-{}", "a".repeat(VolumeId::MAX_LEN));
        assert!(VolumeId::parse(&long).is_none());
    }

    #[test]
    fn uuid_normalisation_rejects_unsafe_and_empty_values() {
        assert_eq!(normalise_identifier("A1B2C3D4E5F60718").as_deref(), Some("a1b2c3d4e5f60718"));
        assert_eq!(normalise_identifier(" 1234-ABCD ").as_deref(), Some("1234-abcd"));
        assert_eq!(normalise_identifier("2026-10-28-12-00-00-00").as_deref(), Some("2026-10-28-12-00-00-00"));
        for bad in ["", "abc", "../../etc", "a/b/c/d", "0000-0000", "has space", "ünïcode"] {
            assert_eq!(normalise_identifier(bad), None, "{bad:?}");
        }
        assert_eq!(normalise_identifier(&"a".repeat(65)), None);
    }

    #[test]
    fn identifier_prefers_filesystem_uuid() {
        let inventory = inventory(
            r#"{"blockdevices":[{"name":"sda","path":"/dev/sda","type":"disk","size":2000,
                "serial":"S1","children":[{"name":"sda1","path":"/dev/sda1","type":"part",
                "size":1000,"fstype":"ext4","uuid":"0F0E0D0C-1111-2222-3333-444455556666",
                "partuuid":"aaaa1111-01","start":2048}]}]}"#,
            "",
        );
        let volume = &inventory.volumes[0];
        assert_eq!(volume.id.as_str(), "fs-0f0e0d0c-1111-2222-3333-444455556666");
        assert_eq!(volume.id_source, IdSource::FilesystemUuid);
        assert!(!volume.duplicate_identity);
    }

    #[test]
    fn identifier_falls_back_in_the_documented_order() {
        let with_partuuid = inventory(
            r#"{"blockdevices":[{"name":"sda","path":"/dev/sda","type":"disk","size":2000,
                "serial":"S1","children":[{"name":"sda1","path":"/dev/sda1","type":"part",
                "size":1000,"partuuid":"AAAA1111-01","start":2048}]}]}"#,
            "",
        );
        assert_eq!(with_partuuid.volumes[0].id.as_str(), "part-aaaa1111-01");
        assert_eq!(with_partuuid.volumes[0].id_source, IdSource::PartitionUuid);

        let with_serial = inventory(
            r#"{"blockdevices":[{"name":"sda","path":"/dev/sda","type":"disk","size":2000,
                "serial":"S1","children":[{"name":"sda1","path":"/dev/sda1","type":"part",
                "size":1000,"start":2048}]}]}"#,
            "",
        );
        assert_eq!(with_serial.volumes[0].id_source, IdSource::HardwareIdentity);
        assert!(with_serial.volumes[0].id.as_str().starts_with("dev-"));

        let with_nothing = inventory(
            r#"{"blockdevices":[{"name":"sda","path":"/dev/sda","type":"disk","size":2000,
                "children":[{"name":"sda1","path":"/dev/sda1","type":"part","size":1000}]}]}"#,
            "",
        );
        assert_eq!(with_nothing.volumes[0].id_source, IdSource::Unstable);
        assert!(with_nothing.volumes[0].id.as_str().starts_with("tmp-"));
    }

    #[test]
    fn hardware_identifier_is_stable_across_device_renaming() {
        let first = inventory(
            r#"{"blockdevices":[{"name":"sda","path":"/dev/sda","type":"disk","size":2000,
                "wwn":"0x5000c500a1b2c3d4","children":[{"name":"sda1","path":"/dev/sda1",
                "type":"part","size":1000,"start":2048}]}]}"#,
            "",
        );
        let renamed = inventory(
            r#"{"blockdevices":[{"name":"sdq","path":"/dev/sdq","type":"disk","size":2000,
                "wwn":"0x5000c500a1b2c3d4","children":[{"name":"sdq1","path":"/dev/sdq1",
                "type":"part","size":1000,"start":2048}]}]}"#,
            "",
        );
        assert_eq!(first.volumes[0].id, renamed.volumes[0].id);
    }

    #[test]
    fn unusable_uuid_is_not_used_for_the_identifier() {
        let inventory = inventory(
            r#"{"blockdevices":[{"name":"sda1","path":"/dev/sda1","type":"part","size":1000,
                "fstype":"vfat","uuid":"../../../etc"}]}"#,
            "",
        );
        assert_eq!(inventory.volumes[0].id_source, IdSource::Unstable);
        assert!(VolumeId::parse(inventory.volumes[0].id.as_str()).is_some());
    }

    #[test]
    fn cloned_filesystems_are_flagged_and_kept_apart() {
        let inventory = inventory(
            r#"{"blockdevices":[
                {"name":"sda1","path":"/dev/sda1","type":"part","size":1000,"fstype":"ext4",
                 "uuid":"11111111-2222-3333-4444-555555555555","partuuid":"aaaa0001-01"},
                {"name":"sdb1","path":"/dev/sdb1","type":"part","size":1000,"fstype":"ext4",
                 "uuid":"11111111-2222-3333-4444-555555555555","partuuid":"bbbb0002-01"}]}"#,
            "",
        );
        assert_eq!(inventory.volumes.len(), 2);
        assert!(inventory.volumes.iter().all(|volume| volume.duplicate_identity));
        assert!(inventory.volumes.iter().all(|volume| volume.id_source == IdSource::PartitionUuid));
        assert_ne!(inventory.volumes[0].id, inventory.volumes[1].id);
    }

    #[test]
    fn whole_disk_clones_fall_through_to_distinct_identifiers() {
        // A disk cloned with its partition table: same UUID and same PARTUUID.
        let inventory = inventory(
            r#"{"blockdevices":[
                {"name":"sda","path":"/dev/sda","type":"disk","size":2000,"serial":"ONE",
                 "children":[{"name":"sda1","path":"/dev/sda1","type":"part","size":1000,
                 "fstype":"xfs","uuid":"11111111-2222-3333-4444-555555555555",
                 "partuuid":"aaaa0001-01","start":2048}]},
                {"name":"sdb","path":"/dev/sdb","type":"disk","size":2000,"serial":"TWO",
                 "children":[{"name":"sdb1","path":"/dev/sdb1","type":"part","size":1000,
                 "fstype":"xfs","uuid":"11111111-2222-3333-4444-555555555555",
                 "partuuid":"aaaa0001-01","start":2048}]}]}"#,
            "",
        );
        assert_eq!(inventory.volumes.len(), 2);
        assert!(inventory.volumes.iter().all(|volume| volume.duplicate_identity));
        assert!(inventory.volumes.iter().all(|volume| volume.id_source == IdSource::HardwareIdentity));
        assert_ne!(inventory.volumes[0].id, inventory.volumes[1].id);
    }

    #[test]
    fn multi_device_btrfs_is_one_volume() {
        let json = r#"{"blockdevices":[
            {"name":"sda","path":"/dev/sda","type":"disk","size":1000,"fstype":"btrfs",
             "uuid":"11111111-2222-3333-4444-555555555555","label":"pool"},
            {"name":"sdb","path":"/dev/sdb","type":"disk","size":3000,"fstype":"btrfs",
             "uuid":"11111111-2222-3333-4444-555555555555","label":"pool"}]}"#;
        let merged = inventory(
            json,
            "DEVNAME=/dev/sda\nUUID_SUB=aaaa\n\nDEVNAME=/dev/sdb\nUUID_SUB=bbbb\n",
        );
        assert_eq!(merged.volumes.len(), 1);
        let pool = &merged.volumes[0];
        assert_eq!(pool.devices, vec!["/dev/sda".to_owned(), "/dev/sdb".to_owned()]);
        assert_eq!(pool.size_bytes, 4000);
        assert!(!pool.duplicate_identity);
        assert_eq!(pool.id_source, IdSource::FilesystemUuid);

        // The same two devices with one UUID_SUB are clones, not one pool.
        let clones = inventory(
            json,
            "DEVNAME=/dev/sda\nUUID_SUB=aaaa\n\nDEVNAME=/dev/sdb\nUUID_SUB=aaaa\n",
        );
        assert_eq!(clones.volumes.len(), 2);
        assert!(clones.volumes.iter().all(|volume| volume.duplicate_identity));

        // Without UUID_SUB the two cannot be told apart from clones.
        let unknown = inventory(json, "");
        assert_eq!(unknown.volumes.len(), 2);
        assert!(unknown.volumes.iter().all(|volume| volume.duplicate_identity));
    }

    #[test]
    fn device_listed_under_two_parents_is_one_volume() {
        let lv = r#"{"name":"vg-data","path":"/dev/mapper/vg-data","type":"lvm","size":5000,
                     "fstype":"ext4","uuid":"99999999-2222-3333-4444-555555555555"}"#;
        let json = format!(
            r#"{{"blockdevices":[
                {{"name":"sda1","path":"/dev/sda1","type":"part","size":3000,
                  "fstype":"LVM2_member","uuid":"aaaaaa-bbbb-cccc-dddd-eeee-ffff-gggggg",
                  "children":[{lv}]}},
                {{"name":"sdb1","path":"/dev/sdb1","type":"part","size":3000,
                  "fstype":"LVM2_member","uuid":"hhhhhh-bbbb-cccc-dddd-eeee-ffff-gggggg",
                  "children":[{lv}]}}]}}"#
        );
        let inventory = inventory(&json, "");
        let logical: Vec<&Volume> =
            inventory.volumes.iter().filter(|volume| volume.filesystem == Filesystem::Ext4).collect();
        assert_eq!(logical.len(), 1);
        assert!(!logical[0].duplicate_identity);
        assert_eq!(inventory.volumes.len(), 3);
    }

    #[test]
    fn system_storage_is_recognised_through_containers() {
        let inventory = inventory(
            r#"{"blockdevices":[{"name":"nvme0n1","path":"/dev/nvme0n1","type":"disk",
                "size":9000,"children":[
                {"name":"nvme0n1p1","path":"/dev/nvme0n1p1","type":"part","size":1000,
                 "fstype":"vfat","uuid":"AAAA-0001","mountpoints":["/boot/efi"]},
                {"name":"nvme0n1p2","path":"/dev/nvme0n1p2","type":"part","size":4000,
                 "fstype":"crypto_LUKS","uuid":"11111111-aaaa-bbbb-cccc-000000000001",
                 "children":[{"name":"luks-root","path":"/dev/mapper/luks-root","type":"crypt",
                   "size":3990,"fstype":"btrfs","uuid":"11111111-aaaa-bbbb-cccc-000000000002",
                   "mountpoints":["/var","/sysroot","/etc"],"fsroots":["/var","/root","/etc"]}]},
                {"name":"nvme0n1p3","path":"/dev/nvme0n1p3","type":"part","size":4000,
                 "fstype":"ext4","uuid":"11111111-aaaa-bbbb-cccc-000000000003",
                 "mountpoints":["/var/mnt/data"]}]}]}"#,
            "",
        );
        let by_device = |device: &str| {
            inventory.volumes.iter().find(|volume| volume.devices[0] == device).unwrap()
        };
        assert!(by_device("/dev/nvme0n1p1").system);
        let container = by_device("/dev/nvme0n1p2");
        assert!(container.system);
        assert_eq!(container.encryption, Encryption::Unlocked);
        let root = by_device("/dev/mapper/luks-root");
        assert!(root.system);
        assert_eq!(root.encryption, Encryption::Mapping);
        assert_eq!(root.subvolumes, vec!["/var".to_owned(), "/root".to_owned(), "/etc".to_owned()]);
        let data = by_device("/dev/nvme0n1p3");
        assert!(!data.system);
        assert_eq!(data.mountpoints, vec!["/var/mnt/data".to_owned()]);
    }

    #[test]
    fn removable_and_read_only_flags_are_inherited_from_the_disk() {
        let inventory = inventory(
            r#"{"blockdevices":[
                {"name":"sdc","path":"/dev/sdc","type":"disk","size":2000,"rm":true,
                 "hotplug":true,"tran":"usb","ro":true,"children":[
                 {"name":"sdc1","path":"/dev/sdc1","type":"part","size":1000,"fstype":"exfat",
                  "uuid":"1234-ABCD","rm":false,"ro":false}]},
                {"name":"sr0","path":"/dev/sr0","type":"rom","size":700,"fstype":"iso9660",
                 "uuid":"2026-10-28-12-00-00-00","rm":true}]}"#,
            "",
        );
        let stick = inventory.find("fs-1234-abcd").unwrap();
        assert!(stick.removable);
        assert!(stick.device_read_only);
        assert_eq!(stick.transport.as_deref(), Some("usb"));
        assert!(!stick.optical);
        let disc = inventory.find("fs-2026-10-28-12-00-00-00").unwrap();
        assert!(disc.optical);
        assert!(disc.device_read_only);
        assert!(disc.removable);
    }

    #[test]
    fn non_volumes_are_left_out() {
        let inventory = inventory(
            r#"{"blockdevices":[
                {"name":"zram0","path":"/dev/zram0","type":"disk","size":8000,"fstype":"swap",
                 "mountpoints":["[SWAP]"]},
                {"name":"loop0","path":"/dev/loop0","type":"loop","size":100},
                {"name":"sr0","path":"/dev/sr0","type":"rom","size":0},
                {"name":"sda","path":"/dev/sda","type":"disk","size":9000,"children":[
                  {"name":"sda1","path":"/dev/sda1","type":"part","size":1,"parttype":"0x5"},
                  {"name":"sda5","path":"/dev/sda5","type":"part","size":4000}]},
                {"name":"sdb","path":"/dev/sdb","type":"disk","size":9000,"fstype":"iso9660",
                 "uuid":"2026-01-01-00-00-00-00","children":[
                  {"name":"sdb1","path":"/dev/sdb1","type":"part","size":4000,"fstype":"iso9660",
                   "uuid":"2026-01-01-00-00-00-00"}]}]}"#,
            "",
        );
        let devices: Vec<&str> =
            inventory.volumes.iter().map(|volume| volume.devices[0].as_str()).collect();
        assert_eq!(devices.len(), 2, "{devices:?}");
        assert!(devices.contains(&"/dev/sda5"));
        assert!(devices.contains(&"/dev/sdb1"));
        let hybrid = inventory.find("fs-2026-01-01-00-00-00-00").unwrap();
        assert!(!hybrid.duplicate_identity);
    }

    #[test]
    fn state_facts_come_from_properties_and_default_to_unknown() {
        let json = r#"{"blockdevices":[
            {"name":"sda1","path":"/dev/sda1","type":"part","size":1000,"fstype":"ntfs",
             "uuid":"AAAAAAAAAAAAAAA1"},
            {"name":"sda2","path":"/dev/sda2","type":"part","size":1000,"fstype":"ntfs",
             "uuid":"AAAAAAAAAAAAAAA2"},
            {"name":"sda3","path":"/dev/sda3","type":"part","size":1000,"fstype":"hfsplus",
             "uuid":"11111111-aaaa-bbbb-cccc-000000000003"},
            {"name":"sda4","path":"/dev/sda4","type":"part","size":1000,"fstype":"ext4",
             "uuid":"11111111-aaaa-bbbb-cccc-000000000004"}]}"#;
        let inventory = inventory(
            json,
            "DEVNAME=/dev/sda1\nUOS_NTFS_STATE=hibernated\n\nDEVNAME=/dev/sda2\n\
             UOS_NTFS_STATE=nonsense\n\nDEVNAME=/dev/sda3\nUOS_HFS_JOURNAL=not-journalled\n",
        );
        let state = |device: &str| {
            let volume =
                inventory.volumes.iter().find(|volume| volume.devices[0] == device).unwrap();
            (volume.ntfs_state, volume.hfs_journal)
        };
        assert_eq!(state("/dev/sda1"), (Some(NtfsState::Hibernated), None));
        assert_eq!(state("/dev/sda2"), (Some(NtfsState::Unknown), None));
        assert_eq!(state("/dev/sda3"), (None, Some(HfsJournal::NotJournaled)));
        assert_eq!(state("/dev/sda4"), (None, None));
    }

    #[test]
    fn inventory_is_sorted_and_identifiers_are_unique() {
        let inventory = inventory(
            r#"{"blockdevices":[
                {"name":"sdb1","path":"/dev/sdb1","type":"part","size":1000,"fstype":"ext4",
                 "uuid":"ffffffff-2222-3333-4444-555555555555"},
                {"name":"sda1","path":"/dev/sda1","type":"part","size":1000,"fstype":"ext4",
                 "uuid":"00000001-2222-3333-4444-555555555555"}]}"#,
            "",
        );
        let ids: Vec<&str> = inventory.volumes.iter().map(|volume| volume.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["fs-00000001-2222-3333-4444-555555555555", "fs-ffffffff-2222-3333-4444-555555555555"]
        );
    }
}
