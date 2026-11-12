// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The partition plan for the "erase target disk" mode, its consequences
//! and the confirmation gate (specification §5.2, §14.2, §28.5).
//!
//! A [`Plan`] is a value. It describes what would be written and what would
//! be lost; nothing in this module writes anything.
//!
//! # The layout is an unverified assumption
//!
//! The partition scheme the UOS base image's `bootc install` expects has not
//! been verified yet. It will be, by the boot-test work. Until then the
//! scheme lives in one place, [`ERASE_DISK_SCHEME`], is marked
//! `verified: false`, and every plan carries that flag. No size or type
//! appears anywhere else in the code.
//!
//! # The chain that guards the disk
//!
//! The types make the steps of specification §5.2 hard to skip:
//!
//! 1. [`crate::identity::select_target`] is the only way to obtain a
//!    [`SelectedTarget`], and it never accepts a device path.
//! 2. [`build_plan`] takes a `SelectedTarget` and refuses the live medium, a
//!    disk in use and a read-only disk.
//! 3. [`Plan::confirm`] is the only way to obtain a [`ConfirmedPlan`]. It
//!    needs the exact typed phrase and a preflight report for the same disk
//!    with no blocking failure. A `ConfirmedPlan` cannot be read from a file.
//! 4. [`ConfirmedPlan::revalidate`] is the only way to obtain a
//!    [`ValidatedTarget`]. It needs a fresh inventory, and fails if the
//!    disk's identity, safety or contents differ from what was confirmed.
//! 5. [`crate::session::Session::begin_destructive`] needs a
//!    `ValidatedTarget` to record the start of any phase that writes to the
//!    target.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::codes::{Fault, InstallCode};
use crate::gpt;
use crate::identity::{
    Identity, IdentityMismatch, IdentityStrength, SelectedTarget, revalidate as revalidate_identity,
};
use crate::inventory::{ActiveUse, BlockDevice, BlockKind, DetectedOs, Inventory};
use crate::preflight::PreflightReport;
use crate::time::Timestamp;
use crate::units::{GIB, MIB, align_down, align_up, format_size};

/// Version of the `partition-plan.json` format.
pub const PLAN_SCHEMA_VERSION: u32 = 1;

/// How many trailing characters of the serial number or WWN go into the
/// typed confirmation phrase.
const PHRASE_TOKEN_CHARS: usize = 8;

/// The installation modes of specification §14.2 that change a disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallMode {
    /// Erase the target disk and install UOS on all of it.
    EraseDisk,
    /// Install next to an existing operating system. Not implemented yet.
    Alongside,
    /// Manual partitioning. Not implemented yet.
    Manual,
    /// Reinstall UOS and keep a supported user-data partition. Not implemented yet.
    ReinstallPreservingData,
}

impl InstallMode {
    /// The mode as shown to the user.
    pub const fn label(self) -> &'static str {
        match self {
            InstallMode::EraseDisk => "erase the target disk and install UOS",
            InstallMode::Alongside => "install alongside an existing operating system",
            InstallMode::Manual => "manual partitioning",
            InstallMode::ReinstallPreservingData => "reinstall UOS and preserve user data",
        }
    }
}

/// What a planned partition is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionRole {
    /// EFI System Partition.
    Esp,
    /// Unencrypted boot partition.
    Boot,
    /// The operating system root.
    Root,
    /// Separate user data.
    Data,
}

/// A filesystem a planned partition is to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filesystem {
    /// FAT32.
    Vfat,
    /// ext4.
    Ext4,
    /// XFS.
    Xfs,
    /// Btrfs.
    Btrfs,
}

impl Filesystem {
    /// The filesystem's usual name.
    pub const fn label(self) -> &'static str {
        match self {
            Filesystem::Vfat => "FAT32",
            Filesystem::Ext4 => "ext4",
            Filesystem::Xfs => "XFS",
            Filesystem::Btrfs => "Btrfs",
        }
    }
}

/// The partition table type a plan creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionTable {
    /// GUID Partition Table.
    Gpt,
}

/// The encryption applied to a planned partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionKind {
    /// LUKS version 2.
    Luks2,
}

/// When a scheme creates its separate boot partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootPartitionPolicy {
    /// Always.
    Always,
    /// Only when the root is encrypted.
    WhenEncrypted,
    /// Never.
    Never,
}

impl BootPartitionPolicy {
    const fn applies(self, encrypt: bool) -> bool {
        match self {
            BootPartitionPolicy::Always => true,
            BootPartitionPolicy::WhenEncrypted => encrypt,
            BootPartitionPolicy::Never => false,
        }
    }
}

/// The fixed properties of one partition in a scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionSpec {
    /// What the partition is for.
    pub role: PartitionRole,
    /// GPT partition name.
    pub label: &'static str,
    /// GPT partition type GUID.
    pub type_guid: &'static str,
    /// Filesystem to create.
    pub filesystem: Filesystem,
    /// Where the installed system mounts it.
    pub mount_point: &'static str,
    /// Whether the partition is encrypted when the user asks for encryption.
    pub encryptable: bool,
}

/// A partition scheme: every number and type the layout depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scheme {
    /// Name recorded in every plan made from this scheme.
    pub name: &'static str,
    /// True once the scheme has been verified against the base image's
    /// install tooling in a boot test.
    pub verified: bool,
    /// Every partition starts and ends on a multiple of this many bytes.
    pub alignment_bytes: u64,
    /// Space kept free at the start of the disk for the primary GPT.
    pub leading_reserved_bytes: u64,
    /// Space kept free at the end of the disk for the backup GPT.
    pub trailing_reserved_bytes: u64,
    /// The EFI System Partition.
    pub esp: PartitionSpec,
    /// Size of the EFI System Partition.
    pub esp_bytes: u64,
    /// The separate boot partition.
    pub boot: PartitionSpec,
    /// Size of the boot partition.
    pub boot_bytes: u64,
    /// When the boot partition is created.
    pub boot_policy: BootPartitionPolicy,
    /// The root partition. It takes all remaining space unless a data
    /// partition is requested.
    pub root: PartitionSpec,
    /// Smallest acceptable root partition.
    pub root_min_bytes: u64,
    /// Size of the root partition when a data partition is requested and the
    /// disk is large enough.
    pub root_bytes_with_data: u64,
    /// The optional user-data partition. It takes all space after the root.
    pub data: PartitionSpec,
    /// Smallest acceptable data partition.
    pub data_min_bytes: u64,
}

/// The scheme for the "erase target disk" mode.
///
/// **Unverified assumption.** See the module documentation and
/// `docs/architecture/installer.md`.
pub const ERASE_DISK_SCHEME: Scheme = Scheme {
    name: "uos-erase-disk-gpt-draft-1",
    verified: false,
    alignment_bytes: MIB,
    leading_reserved_bytes: MIB,
    trailing_reserved_bytes: MIB,
    esp: PartitionSpec {
        role: PartitionRole::Esp,
        label: "EFI-SYSTEM",
        type_guid: gpt::ESP,
        filesystem: Filesystem::Vfat,
        mount_point: "/boot/efi",
        encryptable: false,
    },
    esp_bytes: GIB,
    boot: PartitionSpec {
        role: PartitionRole::Boot,
        label: "boot",
        type_guid: gpt::XBOOTLDR,
        filesystem: Filesystem::Ext4,
        mount_point: "/boot",
        encryptable: false,
    },
    boot_bytes: GIB,
    boot_policy: BootPartitionPolicy::Always,
    root: PartitionSpec {
        role: PartitionRole::Root,
        label: "root",
        type_guid: gpt::LINUX_ROOT_X86_64,
        filesystem: Filesystem::Btrfs,
        mount_point: "/",
        encryptable: true,
    },
    root_min_bytes: 20 * GIB,
    root_bytes_with_data: 64 * GIB,
    data: PartitionSpec {
        role: PartitionRole::Data,
        label: "uos-data",
        type_guid: gpt::LINUX_HOME,
        filesystem: Filesystem::Btrfs,
        mount_point: "/var/home",
        encryptable: true,
    },
    data_min_bytes: 8 * GIB,
};

/// The user's layout choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutOptions {
    /// Encrypt the root and data partitions with LUKS2.
    pub encrypt: bool,
    /// Create a separate user-data partition.
    pub data_partition: bool,
}

/// What the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanRequest {
    /// The installation mode.
    pub mode: InstallMode,
    /// The layout choices.
    pub options: LayoutOptions,
}

/// One partition the plan would create.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedPartition {
    /// Partition number, starting at 1.
    pub number: u32,
    /// What the partition is for.
    pub role: PartitionRole,
    /// GPT partition name.
    pub label: String,
    /// GPT partition type GUID.
    pub type_guid: String,
    /// Offset of the first byte from the start of the disk.
    pub offset_bytes: u64,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Filesystem to create. With encryption it is created inside the
    /// encrypted container.
    pub filesystem: Filesystem,
    /// Where the installed system mounts it.
    pub mount_point: String,
    /// Encryption applied to the partition, if any.
    pub encryption: Option<EncryptionKind>,
}

fn up(value: u64, alignment: u64) -> Result<u64, PlanError> {
    align_up(value, alignment).ok_or(PlanError::Arithmetic)
}

fn add(left: u64, right: u64) -> Result<u64, PlanError> {
    left.checked_add(right).ok_or(PlanError::Arithmetic)
}

fn sub(left: u64, right: u64) -> Result<u64, PlanError> {
    left.checked_sub(right).ok_or(PlanError::Arithmetic)
}

/// The smallest disk, in bytes, that `scheme` fits on with `options`.
pub fn minimum_disk_bytes(scheme: &Scheme, options: LayoutOptions) -> Result<u64, PlanError> {
    let alignment = scheme.alignment_bytes;
    let mut parts = vec![
        scheme.leading_reserved_bytes,
        scheme.esp_bytes,
        scheme.root_min_bytes,
        scheme.trailing_reserved_bytes,
    ];
    if scheme.boot_policy.applies(options.encrypt) {
        parts.push(scheme.boot_bytes);
    }
    if options.data_partition {
        parts.push(scheme.data_min_bytes);
    }
    let mut total = 0u64;
    for bytes in parts {
        total = add(total, up(bytes, alignment)?)?;
    }
    Ok(total)
}

/// Computes the partitions `scheme` puts on a disk of `disk_bytes`.
///
/// All arithmetic is checked. Every offset and size is a multiple of the
/// scheme's alignment, the first partition starts after the leading reserve,
/// and the last ends before the trailing reserve.
pub fn compute_layout(
    scheme: &Scheme,
    disk_bytes: u64,
    logical_sector_size: u32,
    options: LayoutOptions,
) -> Result<Vec<PlannedPartition>, PlanError> {
    let alignment = scheme.alignment_bytes;
    let sector = u64::from(logical_sector_size);
    if sector == 0
        || !sector.is_power_of_two()
        || alignment == 0
        || !alignment.is_multiple_of(sector)
    {
        return Err(PlanError::UnsupportedSectorSize {
            logical_sector_size,
        });
    }
    let too_small = PlanError::DiskTooSmall {
        required_bytes: minimum_disk_bytes(scheme, options)?,
        available_bytes: disk_bytes,
    };
    let start = up(scheme.leading_reserved_bytes, alignment)?;
    let tail = up(scheme.trailing_reserved_bytes, alignment)?;
    let usable_end = align_down(disk_bytes, alignment)
        .ok_or(PlanError::Arithmetic)?
        .checked_sub(tail);
    let Some(end) = usable_end else {
        return Err(too_small);
    };

    let mut fixed = vec![(scheme.esp, scheme.esp_bytes)];
    if scheme.boot_policy.applies(options.encrypt) {
        fixed.push((scheme.boot, scheme.boot_bytes));
    }
    let mut partitions = Vec::new();
    let mut cursor = start;
    for (spec, bytes) in fixed {
        let size = up(bytes, alignment)?;
        partitions.push(planned(partitions.len(), &spec, cursor, size, options));
        cursor = add(cursor, size)?;
    }

    let Some(remaining) = end.checked_sub(cursor) else {
        return Err(too_small);
    };
    let root_min = up(scheme.root_min_bytes, alignment)?;
    if options.data_partition {
        let data_min = up(scheme.data_min_bytes, alignment)?;
        if remaining < add(root_min, data_min)? {
            return Err(too_small);
        }
        let preferred = up(scheme.root_bytes_with_data, alignment)?.max(root_min);
        let root_size = preferred.min(sub(remaining, data_min)?);
        partitions.push(planned(partitions.len(), &scheme.root, cursor, root_size, options));
        let data_offset = add(cursor, root_size)?;
        let data_size = sub(remaining, root_size)?;
        partitions.push(planned(partitions.len(), &scheme.data, data_offset, data_size, options));
    } else {
        if remaining < root_min {
            return Err(too_small);
        }
        partitions.push(planned(partitions.len(), &scheme.root, cursor, remaining, options));
    }
    validate_layout(&partitions, disk_bytes, alignment)?;
    Ok(partitions)
}

fn planned(
    existing: usize,
    spec: &PartitionSpec,
    offset_bytes: u64,
    size_bytes: u64,
    options: LayoutOptions,
) -> PlannedPartition {
    PlannedPartition {
        number: u32::try_from(existing + 1).unwrap_or(u32::MAX),
        role: spec.role,
        label: spec.label.to_owned(),
        type_guid: spec.type_guid.to_owned(),
        offset_bytes,
        size_bytes,
        filesystem: spec.filesystem,
        mount_point: spec.mount_point.to_owned(),
        encryption: (options.encrypt && spec.encryptable).then_some(EncryptionKind::Luks2),
    }
}

/// Checks that a list of planned partitions is consistent: numbered from 1,
/// aligned, non-empty, in order, not overlapping, clear of the first and last
/// `alignment_bytes` of the disk, and with exactly one ESP and one root.
pub fn validate_layout(
    partitions: &[PlannedPartition],
    disk_bytes: u64,
    alignment_bytes: u64,
) -> Result<(), PlanError> {
    let invalid = |reason: String| PlanError::Invalid { reason };
    if alignment_bytes == 0 {
        return Err(invalid("the alignment is zero".to_owned()));
    }
    let limit = align_down(disk_bytes, alignment_bytes)
        .and_then(|aligned| aligned.checked_sub(alignment_bytes))
        .ok_or_else(|| invalid("the disk is smaller than two alignment units".to_owned()))?;
    let mut previous_end = alignment_bytes;
    for (index, partition) in partitions.iter().enumerate() {
        let number = partition.number;
        if u32::try_from(index + 1).ok() != Some(number) {
            return Err(invalid(format!("partition {number} is out of sequence")));
        }
        if partition.size_bytes == 0 {
            return Err(invalid(format!("partition {number} is empty")));
        }
        if !partition.offset_bytes.is_multiple_of(alignment_bytes)
            || !partition.size_bytes.is_multiple_of(alignment_bytes)
        {
            return Err(invalid(format!("partition {number} is not aligned")));
        }
        if partition.offset_bytes < previous_end {
            return Err(invalid(format!(
                "partition {number} overlaps the space before it"
            )));
        }
        let end = partition
            .offset_bytes
            .checked_add(partition.size_bytes)
            .ok_or_else(|| invalid(format!("partition {number} overflows")))?;
        if end > limit {
            return Err(invalid(format!(
                "partition {number} extends past the usable end of the disk"
            )));
        }
        previous_end = end;
    }
    for role in [PartitionRole::Esp, PartitionRole::Root] {
        let count = partitions
            .iter()
            .filter(|partition| partition.role == role)
            .count();
        if count != 1 {
            return Err(invalid(format!(
                "the layout has {count} partitions with role {role:?}; exactly one is required"
            )));
        }
    }
    Ok(())
}

/// Why a disk may not be an erase target, whatever its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetRefusal {
    /// The live environment runs from this disk.
    LiveMedium,
    /// The disk is read-only.
    ReadOnly,
    /// Something on the disk is mounted, used as swap or held open.
    InUse {
        /// Each use that was found.
        uses: Vec<ActiveUse>,
    },
}

/// Says whether `device` must be refused as an erase target.
///
/// This is the single predicate behind the plan refusal, the matching
/// preflight checks and the revalidation before any write.
pub fn target_refusal(device: &BlockDevice) -> Option<TargetRefusal> {
    if device.is_live_medium() {
        return Some(TargetRefusal::LiveMedium);
    }
    if device.read_only {
        return Some(TargetRefusal::ReadOnly);
    }
    let uses = device.active_uses();
    if uses.is_empty() {
        None
    } else {
        Some(TargetRefusal::InUse { uses })
    }
}

impl fmt::Display for TargetRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            TargetRefusal::LiveMedium => {
                "This disk is the medium the installer is running from, so it cannot be \
                 erased. Choose another disk."
            }
            TargetRefusal::ReadOnly => {
                "This disk is read-only, so UOS cannot be installed on it. Remove the \
                 write protection or choose another disk."
            }
            TargetRefusal::InUse { .. } => {
                "This disk is in use: a filesystem on it is mounted, or an encrypted or \
                 logical volume on it is open. Unmount and close everything on the disk, \
                 or choose another disk."
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for TargetRefusal {}

impl Fault for TargetRefusal {
    fn code(&self) -> InstallCode {
        match self {
            TargetRefusal::LiveMedium => InstallCode::PlanTargetLiveMedium,
            TargetRefusal::ReadOnly => InstallCode::PlanTargetReadOnly,
            TargetRefusal::InUse { .. } => InstallCode::PlanTargetInUse,
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            TargetRefusal::LiveMedium => {
                "a filesystem on the disk is mounted at a live-medium mount point".to_owned()
            }
            TargetRefusal::ReadOnly => "the kernel reports the disk as read-only".to_owned(),
            TargetRefusal::InUse { uses } => describe_uses(uses),
        }
    }
}

/// Formats a list of uses as `path: reason; path: reason`.
pub fn describe_uses(uses: &[ActiveUse]) -> String {
    uses.iter()
        .map(|active| format!("{}: {}", active.path, active.reason))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Why a plan could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The mode is defined but not implemented in this version.
    ModeNotImplemented {
        /// The requested mode.
        mode: InstallMode,
    },
    /// The disk may not be an erase target.
    Refused(TargetRefusal),
    /// The disk is too small for the scheme.
    DiskTooSmall {
        /// The smallest disk the scheme fits on.
        required_bytes: u64,
        /// The size of the chosen disk.
        available_bytes: u64,
    },
    /// The logical sector size is zero, not a power of two, or does not
    /// divide the alignment.
    UnsupportedSectorSize {
        /// The sector size the disk reports.
        logical_sector_size: u32,
    },
    /// A size calculation overflowed 64 bits.
    Arithmetic,
    /// The layout failed its own consistency check.
    Invalid {
        /// What is inconsistent.
        reason: String,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::ModeNotImplemented { mode } => write!(
                f,
                "The installation mode \"{}\" is not implemented yet. Only erasing the \
                 whole target disk is available in this version.",
                mode.label()
            ),
            PlanError::Refused(refusal) => fmt::Display::fmt(refusal, f),
            PlanError::DiskTooSmall {
                required_bytes,
                available_bytes,
            } => write!(
                f,
                "The disk is too small for UOS with the chosen options. It holds {}; at \
                 least {} is needed. Choose a larger disk or fewer options.",
                format_size(*available_bytes),
                format_size(*required_bytes)
            ),
            PlanError::UnsupportedSectorSize { .. } => f.write_str(
                "The disk reports a sector size the installer does not support. \
                 Choose another disk.",
            ),
            PlanError::Arithmetic => f.write_str(
                "The installer could not calculate a partition layout for this disk. \
                 Export the diagnostics and report this.",
            ),
            PlanError::Invalid { .. } => f.write_str(
                "The partition plan is not consistent, so it will not be used. \
                 Export the diagnostics and report this.",
            ),
        }
    }
}

impl std::error::Error for PlanError {}

impl Fault for PlanError {
    fn code(&self) -> InstallCode {
        match self {
            PlanError::ModeNotImplemented { .. } => InstallCode::PlanModeNotImplemented,
            PlanError::Refused(refusal) => refusal.code(),
            PlanError::DiskTooSmall { .. } => InstallCode::PlanDiskTooSmall,
            PlanError::UnsupportedSectorSize { .. } => InstallCode::PlanSectorSize,
            PlanError::Arithmetic => InstallCode::PlanArithmetic,
            PlanError::Invalid { .. } => InstallCode::PlanInvalid,
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            PlanError::ModeNotImplemented { mode } => format!("mode: {mode:?}"),
            PlanError::Refused(refusal) => refusal.technical_detail(),
            PlanError::DiskTooSmall {
                required_bytes,
                available_bytes,
            } => format!("required_bytes: {required_bytes}; available_bytes: {available_bytes}"),
            PlanError::UnsupportedSectorSize {
                logical_sector_size,
            } => format!("logical_sector_size: {logical_sector_size}"),
            PlanError::Arithmetic => "a checked 64-bit calculation overflowed".to_owned(),
            PlanError::Invalid { reason } => reason.clone(),
        }
    }
}

/// One existing partition, volume or filesystem the plan would destroy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestroyedItem {
    /// The device node when the plan was made. Display only.
    pub path: String,
    /// What kind of device it is.
    pub kind: BlockKind,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Partition number, for partitions.
    pub partition_number: Option<u32>,
    /// Human-readable partition type.
    pub partition_type_name: Option<String>,
    /// Partition label.
    pub partition_label: Option<String>,
    /// Partition UUID.
    pub partition_uuid: Option<String>,
    /// Filesystem or container signature.
    pub filesystem: Option<String>,
    /// Filesystem label.
    pub filesystem_label: Option<String>,
    /// Filesystem UUID.
    pub filesystem_uuid: Option<String>,
    /// One line for the confirmation screen.
    pub description: String,
}

impl DestroyedItem {
    fn of(node: &BlockDevice) -> DestroyedItem {
        DestroyedItem {
            path: node.path.clone(),
            kind: node.kind,
            size_bytes: node.size_bytes,
            partition_number: node.partition_number,
            partition_type_name: node.partition_type_name.clone(),
            partition_label: node.partition_label.clone(),
            partition_uuid: node.partition_uuid.clone(),
            filesystem: node.filesystem.clone(),
            filesystem_label: node.filesystem_label.clone(),
            filesystem_uuid: node.filesystem_uuid.clone(),
            description: format!(
                "{}  {}  {}",
                node.path,
                format_size(node.size_bytes),
                node.describe_contents()
            ),
        }
    }

    /// True when `other` describes the same content. Device nodes are not
    /// compared, because kernel names can change between probes.
    fn same_content(&self, other: &DestroyedItem) -> bool {
        self.kind == other.kind
            && self.size_bytes == other.size_bytes
            && self.partition_number == other.partition_number
            && self.partition_type_name == other.partition_type_name
            && self.partition_label == other.partition_label
            && self.partition_uuid == other.partition_uuid
            && self.filesystem == other.filesystem
            && self.filesystem_label == other.filesystem_label
            && self.filesystem_uuid == other.filesystem_uuid
    }
}

/// Everything on `device` that erasing it would destroy: every partition and
/// every volume layered on one, plus a filesystem written straight to the
/// disk.
pub fn destroyed_items(device: &BlockDevice) -> Vec<DestroyedItem> {
    device
        .walk()
        .into_iter()
        .filter(|node| !std::ptr::eq(*node, device) || node.filesystem.is_some())
        .map(DestroyedItem::of)
        .collect()
}

fn same_contents(left: &[DestroyedItem], right: &[DestroyedItem]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| a.same_content(b))
}

/// What carrying out the plan would do to what is on the disk now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consequences {
    /// The partition table on the disk now, which would be replaced.
    pub existing_partition_table: Option<String>,
    /// Every existing partition, volume and filesystem that would be destroyed.
    pub destroyed: Vec<DestroyedItem>,
    /// Other operating systems detected on the disk.
    pub other_operating_systems: Vec<DetectedOs>,
    /// Whether any user data on the disk is preserved. Always false for the
    /// erase mode.
    pub user_data_preserved: bool,
    /// The consequences in one sentence.
    pub summary: String,
}

impl Consequences {
    /// The consequences of erasing `device`.
    pub fn of_erasing(device: &BlockDevice) -> Consequences {
        let destroyed = destroyed_items(device);
        let other_operating_systems = device.detected_operating_systems();
        let summary = if destroyed.is_empty() {
            "The disk holds no partitions or recognised filesystems. Anything else on it \
             will still be overwritten."
                .to_owned()
        } else {
            format!(
                "Everything on the disk will be permanently destroyed: {} existing \
                 partitions or volumes. No user data is preserved.",
                destroyed.len()
            )
        };
        Consequences {
            existing_partition_table: device.partition_table.clone(),
            destroyed,
            other_operating_systems,
            user_data_preserved: false,
            summary,
        }
    }
}

/// The text of the final confirmation screen (specification §28.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Confirmation {
    /// The heading of the screen.
    pub heading: String,
    /// The body, one line per entry, in display order.
    pub lines: Vec<String>,
    /// The warning shown next to the serial number and WWN.
    pub privacy_notice: String,
    /// The encryption choice and what it means for key recovery.
    pub encryption_notice: String,
    /// The exact phrase the user must type.
    pub required_phrase: String,
    /// The instruction shown above the text field.
    pub phrase_instruction: String,
}

/// The target a plan was made for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTarget {
    /// The disk's identity. This, and nothing else, says which disk is meant.
    pub identity: Identity,
    /// Whether the identity rests on a serial number or WWN.
    pub identity_strength: IdentityStrength,
    /// True when the identity is weak and the user explicitly accepted that.
    pub weaker_identity_acknowledged: bool,
    /// The device node when the plan was made. Display only: it is never
    /// used to find the disk again.
    pub device_path_when_planned: String,
}

/// A complete plan for one installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Format version of this structure when written to a file.
    pub schema_version: u32,
    /// The installation mode.
    pub mode: InstallMode,
    /// The name of the scheme the layout was computed from.
    pub scheme: String,
    /// Whether that scheme has been verified against the base image's
    /// install tooling.
    pub scheme_verified: bool,
    /// The disk the plan is for.
    pub target: PlanTarget,
    /// The partition table to create.
    pub partition_table: PartitionTable,
    /// The alignment of every partition, in bytes.
    pub alignment_bytes: u64,
    /// The encryption the user chose, if any.
    pub encryption: Option<EncryptionKind>,
    /// The partitions to create, in disk order.
    pub partitions: Vec<PlannedPartition>,
    /// What would be lost.
    pub consequences: Consequences,
    /// The confirmation screen.
    pub confirmation: Confirmation,
}

/// Builds the plan for `target`.
///
/// Only [`InstallMode::EraseDisk`] is implemented; the other modes return
/// [`PlanError::ModeNotImplemented`]. The live medium, a disk in use and a
/// read-only disk are refused before any layout is computed.
pub fn build_plan(
    target: &SelectedTarget<'_>,
    request: &PlanRequest,
    scheme: &Scheme,
) -> Result<Plan, PlanError> {
    if request.mode != InstallMode::EraseDisk {
        return Err(PlanError::ModeNotImplemented { mode: request.mode });
    }
    let device = target.device();
    if let Some(refusal) = target_refusal(device) {
        return Err(PlanError::Refused(refusal));
    }
    let identity = target.identity().clone();
    let partitions = compute_layout(
        scheme,
        identity.size_bytes,
        identity.logical_sector_size,
        request.options,
    )?;
    let consequences = Consequences::of_erasing(device);
    let encryption = request.options.encrypt.then_some(EncryptionKind::Luks2);
    let confirmation = confirmation_text(
        &identity,
        target.weaker_identity_acknowledged(),
        &partitions,
        &consequences,
        encryption,
    );
    Ok(Plan {
        schema_version: PLAN_SCHEMA_VERSION,
        mode: request.mode,
        scheme: scheme.name.to_owned(),
        scheme_verified: scheme.verified,
        target: PlanTarget {
            identity_strength: identity.strength(),
            identity,
            weaker_identity_acknowledged: target.weaker_identity_acknowledged(),
            device_path_when_planned: device.path.clone(),
        },
        partition_table: PartitionTable::Gpt,
        alignment_bytes: scheme.alignment_bytes,
        encryption,
        partitions,
        consequences,
        confirmation,
    })
}

/// The phrase the user must type to confirm erasing the disk `identity`
/// describes.
///
/// It ends with the last characters of the serial number, or of the WWN when
/// there is no serial number, so that the phrase for one disk does not
/// confirm another. A disk with neither uses its size in bytes.
pub fn confirmation_phrase(identity: &Identity) -> String {
    for source in [identity.serial.as_deref(), identity.wwn.as_deref()]
        .into_iter()
        .flatten()
    {
        let cleaned: Vec<char> = source
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|character| character.to_ascii_uppercase())
            .collect();
        if !cleaned.is_empty() {
            let start = cleaned.len().saturating_sub(PHRASE_TOKEN_CHARS);
            let token: String = cleaned[start..].iter().collect();
            return format!("ERASE {token}");
        }
    }
    format!("ERASE {}", identity.size_bytes)
}

/// One line describing a planned partition.
pub fn describe_partition(partition: &PlannedPartition) -> String {
    let encryption = match partition.encryption {
        Some(EncryptionKind::Luks2) => ", encrypted with LUKS2",
        None => "",
    };
    format!(
        "{}  {}  {}  {}{}  mounted at {}",
        partition.number,
        partition.label,
        format_size(partition.size_bytes),
        partition.filesystem.label(),
        encryption,
        partition.mount_point
    )
}

fn confirmation_text(
    identity: &Identity,
    weaker_identity_acknowledged: bool,
    partitions: &[PlannedPartition],
    consequences: &Consequences,
    encryption: Option<EncryptionKind>,
) -> Confirmation {
    let mut lines = identity.display_lines();
    if weaker_identity_acknowledged {
        lines.push(
            "Warning: this drive reports no serial number or WWN. The installer can \
             recognise it only by model, size and port. You acknowledged this."
                .to_owned(),
        );
    }
    lines.push(format!(
        "Existing partition table: {}",
        consequences
            .existing_partition_table
            .as_deref()
            .unwrap_or("none recognised")
    ));
    if consequences.destroyed.is_empty() {
        lines.push("Existing partitions and filesystems: none recognised".to_owned());
    } else {
        lines.push("Existing partitions and filesystems, all of which will be destroyed:".to_owned());
        for item in &consequences.destroyed {
            lines.push(format!("  {}", item.description));
        }
    }
    if consequences.other_operating_systems.is_empty() {
        lines.push("Other operating systems detected on this drive: none".to_owned());
    } else {
        for system in &consequences.other_operating_systems {
            lines.push(format!(
                "Another operating system was detected on this drive and will be \
                 destroyed: {}",
                system.headline()
            ));
        }
    }
    lines.push("New partition layout:".to_owned());
    for partition in partitions {
        lines.push(format!("  {}", describe_partition(partition)));
    }
    lines.push(
        "User data: nothing on this drive is preserved. This cannot be undone.".to_owned(),
    );

    let encryption_notice = match encryption {
        Some(EncryptionKind::Luks2) => {
            "Encryption: the system and user data will be encrypted with LUKS2. The \
             passphrase you choose is the only key. The installer keeps no copy and \
             creates no recovery key: if the passphrase is lost, the data cannot be \
             recovered."
        }
        None => {
            "Encryption: none. Anyone with physical access to this drive will be able to \
             read what is on it."
        }
    };
    let required_phrase = confirmation_phrase(identity);
    Confirmation {
        heading: "Erase this drive and install UOS".to_owned(),
        lines,
        privacy_notice: "The serial number and WWN identify this physical drive. Treat \
                         screenshots and exported logs that show them as personal data."
            .to_owned(),
        encryption_notice: encryption_notice.to_owned(),
        phrase_instruction: format!("To confirm, type exactly: {required_phrase}"),
        required_phrase,
    }
}

impl Plan {
    /// True when what is on `device` now is exactly what this plan lists as
    /// destroyed: the disk still looks as it did when the plan was made.
    pub fn existing_contents_unchanged(&self, device: &BlockDevice) -> bool {
        same_contents(&destroyed_items(device), &self.consequences.destroyed)
    }

    /// True when `device` carries a GPT whose partitions have the sizes this
    /// plan lays out, in order. This compares what an inventory can show; it
    /// says nothing about what is inside the partitions.
    pub fn layout_present_on(&self, device: &BlockDevice) -> bool {
        let found: Vec<u64> = device
            .children
            .iter()
            .filter(|child| child.kind == BlockKind::Partition)
            .map(|child| child.size_bytes)
            .collect();
        let planned: Vec<u64> = self
            .partitions
            .iter()
            .map(|partition| partition.size_bytes)
            .collect();
        device.partition_table.as_deref() == Some("gpt") && found == planned
    }
}

/// A plan the user has confirmed by typing the exact phrase.
///
/// The only way to create one is [`Plan::confirm`]. It cannot be serialised
/// or deserialised, so a confirmation never survives in a file: after a
/// restart the user must confirm again.
#[derive(Debug)]
pub struct ConfirmedPlan {
    plan: Plan,
    confirmed_at: Timestamp,
}

impl Plan {
    /// Records the user's confirmation of this plan.
    ///
    /// `typed_phrase` must equal [`Confirmation::required_phrase`] exactly,
    /// apart from surrounding white space. `preflight` must be a report for
    /// this plan's target with no blocking failure.
    pub fn confirm(
        &self,
        typed_phrase: &str,
        preflight: &PreflightReport,
        now: Timestamp,
    ) -> Result<ConfirmedPlan, ConfirmError> {
        if self.mode != InstallMode::EraseDisk {
            return Err(ConfirmError::Plan(PlanError::ModeNotImplemented {
                mode: self.mode,
            }));
        }
        if !self.target.identity.is_stable() && !self.target.weaker_identity_acknowledged {
            return Err(ConfirmError::WeakIdentityNotAcknowledged);
        }
        if preflight.target.as_ref() != Some(&self.target.identity) {
            return Err(ConfirmError::PreflightMismatch);
        }
        let blocking: Vec<InstallCode> = preflight
            .blocking()
            .iter()
            .map(|result| result.code)
            .collect();
        if !blocking.is_empty() {
            return Err(ConfirmError::PreflightBlocked { codes: blocking });
        }
        if typed_phrase.trim() != confirmation_phrase(&self.target.identity) {
            return Err(ConfirmError::PhraseMismatch);
        }
        Ok(ConfirmedPlan {
            plan: self.clone(),
            confirmed_at: now,
        })
    }
}

impl ConfirmedPlan {
    /// The confirmed plan.
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// When the user confirmed.
    pub fn confirmed_at(&self) -> Timestamp {
        self.confirmed_at
    }

    /// Checks the confirmed plan against `current`, a fresh inventory, and
    /// returns the target device only if all of this holds:
    ///
    /// * exactly one disk still has the confirmed identity;
    /// * that disk is not the live medium, not read-only and not in use;
    /// * what is on it is what the user was told would be destroyed;
    /// * the planned layout is consistent and fits the disk.
    pub fn revalidate<'a>(
        &'a self,
        current: &'a Inventory,
    ) -> Result<ValidatedTarget<'a>, RevalidateError> {
        let device = revalidate_identity(&self.plan.target.identity, current)
            .map_err(RevalidateError::Identity)?;
        if let Some(refusal) = target_refusal(device) {
            return Err(RevalidateError::Refused(refusal));
        }
        if !same_contents(&destroyed_items(device), &self.plan.consequences.destroyed) {
            return Err(RevalidateError::ContentsChanged);
        }
        validate_layout(
            &self.plan.partitions,
            device.size_bytes,
            self.plan.alignment_bytes,
        )
        .map_err(RevalidateError::Plan)?;
        Ok(ValidatedTarget {
            device,
            plan: &self.plan,
        })
    }
}

/// Proof that a confirmed plan's target was found again, unchanged, in a
/// fresh inventory.
///
/// It borrows that inventory, so it cannot outlive it. The only way to
/// create one is [`ConfirmedPlan::revalidate`].
#[derive(Debug)]
pub struct ValidatedTarget<'a> {
    device: &'a BlockDevice,
    plan: &'a Plan,
}

impl<'a> ValidatedTarget<'a> {
    /// The target device in the fresh inventory.
    pub fn device(&self) -> &'a BlockDevice {
        self.device
    }

    /// The confirmed plan.
    pub fn plan(&self) -> &'a Plan {
        self.plan
    }
}

/// Why a plan could not be confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmError {
    /// The typed phrase is not the required phrase.
    PhraseMismatch,
    /// The preflight report has blocking failures.
    PreflightBlocked {
        /// The codes of the blocking checks.
        codes: Vec<InstallCode>,
    },
    /// The preflight report is for another disk, or for none.
    PreflightMismatch,
    /// The plan's target has a weak identity that was not acknowledged.
    WeakIdentityNotAcknowledged,
    /// The plan itself cannot be carried out.
    Plan(PlanError),
}

impl fmt::Display for ConfirmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfirmError::PhraseMismatch => f.write_str(
                "The confirmation phrase was not typed exactly as shown. Nothing was \
                 written. Check the drive details and type the phrase again.",
            ),
            ConfirmError::PreflightBlocked { .. } => f.write_str(
                "Installation cannot start because a required check did not pass. \
                 Resolve the blocking checks listed in the preflight results.",
            ),
            ConfirmError::PreflightMismatch => f.write_str(
                "The checks were not run for the drive in this plan. Run the checks \
                 again for the chosen drive.",
            ),
            ConfirmError::WeakIdentityNotAcknowledged => f.write_str(
                "The drive in this plan has no stable identifier and that was not \
                 acknowledged. Choose the drive again.",
            ),
            ConfirmError::Plan(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl std::error::Error for ConfirmError {}

impl Fault for ConfirmError {
    fn code(&self) -> InstallCode {
        match self {
            ConfirmError::PhraseMismatch => InstallCode::ConfirmPhraseMismatch,
            ConfirmError::PreflightBlocked { .. } => InstallCode::ConfirmPreflightBlocked,
            ConfirmError::PreflightMismatch => InstallCode::ConfirmPreflightMismatch,
            ConfirmError::WeakIdentityNotAcknowledged => InstallCode::ConfirmWeakIdentity,
            ConfirmError::Plan(error) => error.code(),
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            ConfirmError::PhraseMismatch => {
                "the typed text differs from the required phrase; the typed text is not recorded"
                    .to_owned()
            }
            ConfirmError::PreflightBlocked { codes } => format!(
                "blocking checks: {}",
                codes
                    .iter()
                    .map(|code| code.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ConfirmError::PreflightMismatch => {
                "the preflight report's target identity is not the plan's target identity"
                    .to_owned()
            }
            ConfirmError::WeakIdentityNotAcknowledged => {
                "the plan's target has neither serial nor WWN and \
                 weaker_identity_acknowledged is false"
                    .to_owned()
            }
            ConfirmError::Plan(error) => error.technical_detail(),
        }
    }
}

/// Why a confirmed plan's target failed revalidation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevalidateError {
    /// The disk's identity no longer matches exactly one disk.
    Identity(IdentityMismatch),
    /// The disk became the live medium, read-only or in use.
    Refused(TargetRefusal),
    /// What is on the disk is no longer what the user confirmed destroying.
    ContentsChanged,
    /// The planned layout is inconsistent or does not fit the disk.
    Plan(PlanError),
}

impl fmt::Display for RevalidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RevalidateError::Identity(mismatch) => fmt::Display::fmt(mismatch, f),
            RevalidateError::Refused(refusal) => fmt::Display::fmt(refusal, f),
            RevalidateError::ContentsChanged => f.write_str(
                "The contents of the chosen disk changed after you confirmed. Nothing was \
                 written. Review the plan and confirm again.",
            ),
            RevalidateError::Plan(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl std::error::Error for RevalidateError {}

impl Fault for RevalidateError {
    fn code(&self) -> InstallCode {
        match self {
            RevalidateError::Identity(mismatch) => mismatch.code(),
            RevalidateError::Refused(refusal) => refusal.code(),
            RevalidateError::ContentsChanged => InstallCode::ConfirmContentsChanged,
            RevalidateError::Plan(error) => error.code(),
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            RevalidateError::Identity(mismatch) => mismatch.technical_detail(),
            RevalidateError::Refused(refusal) => refusal.technical_detail(),
            RevalidateError::ContentsChanged => {
                "the partitions, volumes and filesystems found on the disk differ from the \
                 plan's list of what would be destroyed"
                    .to_owned()
            }
            RevalidateError::Plan(error) => error.technical_detail(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{TargetSelector, WeakIdentityPolicy, select_target};
    use crate::preflight::{HostFacts, Requirements, TargetFacts, run_preflight};
    use crate::testing::{
        NVME_SELECTOR, SATA_SELECTOR, erase_request, good_environment, oddities, workstation,
    };

    const NO_OPTIONS: LayoutOptions = LayoutOptions {
        encrypt: false,
        data_partition: false,
    };
    const ENCRYPT: LayoutOptions = LayoutOptions {
        encrypt: true,
        data_partition: false,
    };
    const WITH_DATA: LayoutOptions = LayoutOptions {
        encrypt: false,
        data_partition: true,
    };
    const ENCRYPT_WITH_DATA: LayoutOptions = LayoutOptions {
        encrypt: true,
        data_partition: true,
    };
    const ALL_OPTIONS: [LayoutOptions; 4] = [NO_OPTIONS, ENCRYPT, WITH_DATA, ENCRYPT_WITH_DATA];

    fn target<'a>(inventory: &'a Inventory, selector: &str) -> SelectedTarget<'a> {
        let selector: TargetSelector = selector.parse().unwrap();
        select_target(inventory, &selector, WeakIdentityPolicy::Acknowledged).unwrap()
    }

    fn plan_for(inventory: &Inventory, selector: &str, options: LayoutOptions) -> Plan {
        build_plan(
            &target(inventory, selector),
            &erase_request(options),
            &ERASE_DISK_SCHEME,
        )
        .unwrap()
    }

    fn preflight_for(inventory: &Inventory, selector: &str) -> PreflightReport {
        let facts = HostFacts {
            environment: good_environment(),
            target: Some(TargetFacts::of(target(inventory, selector).device())),
        };
        let requirements = Requirements::for_layout(&ERASE_DISK_SCHEME, NO_OPTIONS).unwrap();
        run_preflight(&facts, &requirements)
    }

    fn assert_layout_invariants(partitions: &[PlannedPartition], disk_bytes: u64) {
        let alignment = ERASE_DISK_SCHEME.alignment_bytes;
        assert!(!partitions.is_empty());
        let mut previous_end = ERASE_DISK_SCHEME.leading_reserved_bytes;
        for (index, partition) in partitions.iter().enumerate() {
            assert_eq!(partition.number as usize, index + 1);
            assert!(partition.offset_bytes.is_multiple_of(alignment));
            assert!(partition.size_bytes.is_multiple_of(alignment));
            assert!(partition.size_bytes > 0);
            assert_eq!(partition.offset_bytes, previous_end, "no gaps, no overlap");
            previous_end = partition.offset_bytes + partition.size_bytes;
        }
        assert!(previous_end + ERASE_DISK_SCHEME.trailing_reserved_bytes <= disk_bytes);
        assert!(
            disk_bytes - previous_end < 2 * alignment,
            "the layout uses the whole disk up to the trailing reserve"
        );
    }

    #[test]
    fn default_layout_on_the_nvme_disk() {
        let inventory = workstation();
        let plan = plan_for(&inventory, NVME_SELECTOR, NO_OPTIONS);
        assert_eq!(plan.mode, InstallMode::EraseDisk);
        assert_eq!(plan.partition_table, PartitionTable::Gpt);
        assert_eq!(plan.scheme, ERASE_DISK_SCHEME.name);
        assert!(!plan.scheme_verified);
        assert_eq!(plan.encryption, None);
        assert_eq!(plan.alignment_bytes, MIB);

        let roles: Vec<PartitionRole> = plan.partitions.iter().map(|p| p.role).collect();
        assert_eq!(
            roles,
            [PartitionRole::Esp, PartitionRole::Boot, PartitionRole::Root]
        );
        let esp = &plan.partitions[0];
        assert_eq!(esp.offset_bytes, MIB);
        assert_eq!(esp.size_bytes, GIB);
        assert_eq!(esp.type_guid, gpt::ESP);
        assert_eq!(esp.filesystem, Filesystem::Vfat);
        assert_eq!(esp.encryption, None);
        let boot = &plan.partitions[1];
        assert_eq!(boot.offset_bytes, MIB + GIB);
        assert_eq!(boot.size_bytes, GIB);
        let root = &plan.partitions[2];
        assert_eq!(root.offset_bytes, MIB + 2 * GIB);
        // 1000204886016 bytes is 953869 MiB and 0.71 MiB; the last whole MiB
        // is kept for the backup GPT.
        assert_eq!(root.size_bytes, (953_869 - 1 - 1 - 2 * 1024) * MIB);
        assert_eq!(root.type_guid, gpt::LINUX_ROOT_X86_64);
        assert_eq!(root.encryption, None);
        assert_layout_invariants(&plan.partitions, 1_000_204_886_016);
    }

    #[test]
    fn encryption_applies_to_root_and_data_only() {
        let inventory = workstation();
        let plan = plan_for(&inventory, NVME_SELECTOR, ENCRYPT_WITH_DATA);
        assert_eq!(plan.encryption, Some(EncryptionKind::Luks2));
        let encrypted: Vec<(PartitionRole, bool)> = plan
            .partitions
            .iter()
            .map(|p| (p.role, p.encryption.is_some()))
            .collect();
        assert_eq!(
            encrypted,
            [
                (PartitionRole::Esp, false),
                (PartitionRole::Boot, false),
                (PartitionRole::Root, true),
                (PartitionRole::Data, true)
            ]
        );
        assert!(plan.confirmation.encryption_notice.contains("LUKS2"));
        assert!(
            plan.confirmation
                .encryption_notice
                .contains("cannot be recovered")
        );
        let unencrypted = plan_for(&inventory, NVME_SELECTOR, NO_OPTIONS);
        assert!(
            unencrypted
                .confirmation
                .encryption_notice
                .starts_with("Encryption: none")
        );
    }

    #[test]
    fn data_partition_takes_the_space_after_a_fixed_root() {
        let inventory = workstation();
        let plan = plan_for(&inventory, NVME_SELECTOR, WITH_DATA);
        assert_eq!(plan.partitions.len(), 4);
        let root = &plan.partitions[2];
        let data = &plan.partitions[3];
        assert_eq!(root.size_bytes, ERASE_DISK_SCHEME.root_bytes_with_data);
        assert_eq!(data.role, PartitionRole::Data);
        assert_eq!(data.offset_bytes, root.offset_bytes + root.size_bytes);
        assert_eq!(data.mount_point, "/var/home");
        assert_layout_invariants(&plan.partitions, 1_000_204_886_016);
    }

    #[test]
    fn root_shrinks_towards_its_minimum_on_a_small_disk_with_data() {
        // Room for the fixed partitions, the minimum data partition and 30 GiB.
        let disk = 2 * MIB + 2 * GIB + 30 * GIB + ERASE_DISK_SCHEME.data_min_bytes;
        let partitions = compute_layout(&ERASE_DISK_SCHEME, disk, 512, WITH_DATA).unwrap();
        assert_eq!(partitions[2].size_bytes, 30 * GIB);
        assert_eq!(partitions[3].size_bytes, ERASE_DISK_SCHEME.data_min_bytes);
        assert_layout_invariants(&partitions, disk);
    }

    #[test]
    fn exact_minimum_size_is_accepted_and_one_byte_less_is_refused() {
        for options in ALL_OPTIONS {
            let minimum = minimum_disk_bytes(&ERASE_DISK_SCHEME, options).unwrap();
            assert!(minimum.is_multiple_of(MIB));
            let partitions = compute_layout(&ERASE_DISK_SCHEME, minimum, 512, options).unwrap();
            assert_layout_invariants(&partitions, minimum);
            let root = partitions
                .iter()
                .find(|p| p.role == PartitionRole::Root)
                .unwrap();
            assert_eq!(root.size_bytes, ERASE_DISK_SCHEME.root_min_bytes);

            let error =
                compute_layout(&ERASE_DISK_SCHEME, minimum - 1, 512, options).unwrap_err();
            assert_eq!(
                error,
                PlanError::DiskTooSmall {
                    required_bytes: minimum,
                    available_bytes: minimum - 1
                }
            );
            assert_eq!(error.code(), InstallCode::PlanDiskTooSmall);
        }
        assert_eq!(
            minimum_disk_bytes(&ERASE_DISK_SCHEME, NO_OPTIONS).unwrap(),
            2 * MIB + 22 * GIB
        );
        assert_eq!(
            minimum_disk_bytes(&ERASE_DISK_SCHEME, WITH_DATA).unwrap(),
            2 * MIB + 30 * GIB
        );
    }

    #[test]
    fn layouts_hold_their_invariants_across_many_disk_sizes() {
        let mut checked = 0u32;
        for options in ALL_OPTIONS {
            let minimum = minimum_disk_bytes(&ERASE_DISK_SCHEME, options).unwrap();
            // Odd sizes on purpose: not multiples of the alignment or sector.
            let mut disk = minimum - 3 * MIB - 17;
            while disk < 3 * 1024 * GIB {
                for sector in [512, 4096] {
                    match compute_layout(&ERASE_DISK_SCHEME, disk, sector, options) {
                        Ok(partitions) => {
                            assert!(disk >= minimum);
                            assert_layout_invariants(&partitions, disk);
                            validate_layout(&partitions, disk, MIB).unwrap();
                        }
                        Err(error) => {
                            assert!(disk < minimum, "{error:?} at {disk}");
                            assert_eq!(error.code(), InstallCode::PlanDiskTooSmall);
                        }
                    }
                    checked += 1;
                }
                disk += disk / 7 + 4_097;
            }
        }
        assert!(checked > 200);
    }

    #[test]
    fn extreme_sizes_never_panic() {
        for options in ALL_OPTIONS {
            for disk in [0, 1, 511, MIB - 1, MIB, 2 * MIB, GIB, u64::MAX - 1, u64::MAX] {
                let result = compute_layout(&ERASE_DISK_SCHEME, disk, 512, options);
                if let Ok(partitions) = result {
                    assert_layout_invariants(&partitions, disk);
                }
            }
        }
        let huge = compute_layout(&ERASE_DISK_SCHEME, u64::MAX, 4096, NO_OPTIONS).unwrap();
        assert_layout_invariants(&huge, u64::MAX);
    }

    #[test]
    fn a_scheme_that_overflows_is_an_arithmetic_error() {
        let scheme = Scheme {
            root_min_bytes: u64::MAX - MIB,
            ..ERASE_DISK_SCHEME
        };
        assert_eq!(
            compute_layout(&scheme, 500 * GIB, 512, NO_OPTIONS).unwrap_err(),
            PlanError::Arithmetic
        );
        assert_eq!(PlanError::Arithmetic.code(), InstallCode::PlanArithmetic);
    }

    #[test]
    fn unsupported_sector_sizes_are_refused() {
        for sector in [0, 513, 520, 3 * 1024, 2 * 1024 * 1024] {
            let error = compute_layout(&ERASE_DISK_SCHEME, 500 * GIB, sector, NO_OPTIONS)
                .unwrap_err();
            assert_eq!(
                error,
                PlanError::UnsupportedSectorSize {
                    logical_sector_size: sector
                }
            );
            assert_eq!(error.code(), InstallCode::PlanSectorSize);
        }
        assert!(compute_layout(&ERASE_DISK_SCHEME, 500 * GIB, 4096, NO_OPTIONS).is_ok());
    }

    #[test]
    fn boot_partition_policy_is_data() {
        let when_encrypted = Scheme {
            boot_policy: BootPartitionPolicy::WhenEncrypted,
            ..ERASE_DISK_SCHEME
        };
        let plain = compute_layout(&when_encrypted, 500 * GIB, 512, NO_OPTIONS).unwrap();
        assert_eq!(plain.len(), 2);
        let encrypted = compute_layout(&when_encrypted, 500 * GIB, 512, ENCRYPT).unwrap();
        assert_eq!(encrypted.len(), 3);
        assert_eq!(encrypted[1].role, PartitionRole::Boot);

        let never = Scheme {
            boot_policy: BootPartitionPolicy::Never,
            ..ERASE_DISK_SCHEME
        };
        let without = compute_layout(&never, 500 * GIB, 512, ENCRYPT).unwrap();
        assert_eq!(without.len(), 2);
        assert_eq!(
            minimum_disk_bytes(&never, NO_OPTIONS).unwrap(),
            2 * MIB + 21 * GIB
        );
    }

    #[test]
    fn layout_validation_catches_inconsistent_plans() {
        let good = compute_layout(&ERASE_DISK_SCHEME, 500 * GIB, 512, WITH_DATA).unwrap();
        validate_layout(&good, 500 * GIB, MIB).unwrap();

        let mut overlapping = good.clone();
        overlapping[1].offset_bytes -= MIB;
        let mut misaligned = good.clone();
        misaligned[2].size_bytes -= 512;
        let mut renumbered = good.clone();
        renumbered[0].number = 2;
        let mut empty = good.clone();
        empty[3].size_bytes = 0;
        let mut no_root = good.clone();
        no_root[2].role = PartitionRole::Data;
        let mut into_reserve = good.clone();
        into_reserve[0].offset_bytes = 0;
        let mut overflowing = good.clone();
        overflowing[3].size_bytes = u64::MAX - (MIB - 1);

        for bad in [
            overlapping,
            misaligned,
            renumbered,
            empty,
            no_root,
            into_reserve,
            overflowing,
        ] {
            let error = validate_layout(&bad, 500 * GIB, MIB).unwrap_err();
            assert_eq!(error.code(), InstallCode::PlanInvalid);
        }
        // The same layout does not fit a smaller disk.
        assert!(validate_layout(&good, 400 * GIB, MIB).is_err());
        assert!(validate_layout(&good, 500 * GIB, 0).is_err());
        assert!(validate_layout(&[], 500 * GIB, MIB).is_err());
    }

    #[test]
    fn other_modes_are_defined_but_not_implemented() {
        let inventory = workstation();
        let selected = target(&inventory, NVME_SELECTOR);
        for mode in [
            InstallMode::Alongside,
            InstallMode::Manual,
            InstallMode::ReinstallPreservingData,
        ] {
            let request = PlanRequest {
                mode,
                options: NO_OPTIONS,
            };
            let error = build_plan(&selected, &request, &ERASE_DISK_SCHEME).unwrap_err();
            assert_eq!(error, PlanError::ModeNotImplemented { mode });
            assert_eq!(error.code(), InstallCode::PlanModeNotImplemented);
            assert!(error.to_string().contains("not implemented yet"));
        }
    }

    #[test]
    fn the_live_medium_is_refused() {
        let inventory = workstation();
        let stick = target(&inventory, "serial=4C530001230506112345");
        let error = build_plan(&stick, &erase_request(NO_OPTIONS), &ERASE_DISK_SCHEME).unwrap_err();
        assert_eq!(error, PlanError::Refused(TargetRefusal::LiveMedium));
        assert_eq!(error.code(), InstallCode::PlanTargetLiveMedium);
    }

    #[test]
    fn a_disk_with_mounted_partitions_is_refused() {
        let mut inventory = workstation();
        let sda = inventory
            .devices
            .iter_mut()
            .find(|device| device.name == "sda")
            .unwrap();
        sda.children[2].mountpoints = vec!["/mnt/old-root".to_owned()];
        let selected = target(&inventory, SATA_SELECTOR);
        let error =
            build_plan(&selected, &erase_request(NO_OPTIONS), &ERASE_DISK_SCHEME).unwrap_err();
        assert_eq!(error.code(), InstallCode::PlanTargetInUse);
        assert_eq!(
            error.technical_detail(),
            "/dev/sda3: mounted at /mnt/old-root"
        );
    }

    #[test]
    fn open_encrypted_and_logical_volumes_count_as_in_use() {
        let inventory = oddities();
        let selected = target(&inventory, "serial=LVMLUKS0001");
        let error =
            build_plan(&selected, &erase_request(NO_OPTIONS), &ERASE_DISK_SCHEME).unwrap_err();
        assert_eq!(error.code(), InstallCode::PlanTargetInUse);
    }

    #[test]
    fn a_read_only_disk_is_refused() {
        let mut inventory = workstation();
        inventory
            .devices
            .iter_mut()
            .find(|device| device.name == "sda")
            .unwrap()
            .read_only = true;
        let selected = target(&inventory, SATA_SELECTOR);
        let error =
            build_plan(&selected, &erase_request(NO_OPTIONS), &ERASE_DISK_SCHEME).unwrap_err();
        assert_eq!(error, PlanError::Refused(TargetRefusal::ReadOnly));
        assert_eq!(error.code(), InstallCode::PlanTargetReadOnly);
    }

    #[test]
    fn consequences_list_every_partition_and_the_detected_system() {
        let inventory = workstation();
        let plan = plan_for(&inventory, NVME_SELECTOR, NO_OPTIONS);
        let consequences = &plan.consequences;
        assert_eq!(consequences.existing_partition_table.as_deref(), Some("gpt"));
        assert!(!consequences.user_data_preserved);
        let paths: Vec<&str> = consequences
            .destroyed
            .iter()
            .map(|item| item.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "/dev/nvme0n1p1",
                "/dev/nvme0n1p2",
                "/dev/nvme0n1p3",
                "/dev/nvme0n1p4"
            ]
        );
        assert_eq!(consequences.destroyed[2].filesystem.as_deref(), Some("ntfs"));
        assert!(consequences.destroyed[2].description.contains("ntfs"));
        assert_eq!(consequences.other_operating_systems.len(), 1);
        assert_eq!(
            consequences.other_operating_systems[0].headline(),
            "Windows (likely)"
        );
        assert!(consequences.summary.contains("4 existing"));
    }

    #[test]
    fn consequences_descend_into_encrypted_and_logical_volumes() {
        let inventory = oddities();
        let nvme = inventory
            .devices
            .iter()
            .find(|device| device.name == "nvme1n1")
            .unwrap();
        let consequences = Consequences::of_erasing(nvme);
        let kinds: Vec<BlockKind> = consequences.destroyed.iter().map(|item| item.kind).collect();
        assert_eq!(
            kinds,
            [
                BlockKind::Partition,
                BlockKind::Partition,
                BlockKind::Partition,
                BlockKind::Crypt,
                BlockKind::Lvm,
                BlockKind::Lvm
            ]
        );
    }

    #[test]
    fn a_filesystem_written_straight_to_the_disk_is_listed() {
        let inventory = workstation();
        let stick = inventory
            .devices
            .iter()
            .find(|device| device.name == "sdb")
            .unwrap();
        let items = destroyed_items(stick);
        assert_eq!(items[0].path, "/dev/sdb");
        assert_eq!(items[0].filesystem.as_deref(), Some("iso9660"));
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn confirmation_text_covers_the_specified_items() {
        let inventory = workstation();
        let plan = plan_for(&inventory, NVME_SELECTOR, ENCRYPT);
        let confirmation = &plan.confirmation;
        let text = confirmation.lines.join("\n");
        for needle in [
            "Samsung SSD 980 PRO 1TB",
            "931.5 GiB (1000204886016 bytes)",
            "S5GXNX0T123456A",
            "eui.002538b111b2c3d4",
            "/dev/disk/by-id/nvme-Samsung_SSD_980_PRO_1TB_S5GXNX0T123456A",
            "Existing partition table: gpt",
            "/dev/nvme0n1p3",
            "ntfs",
            "will be destroyed",
            "Windows (likely)",
            "New partition layout:",
            "EFI-SYSTEM",
            "encrypted with LUKS2",
            "nothing on this drive is preserved",
        ] {
            assert!(text.contains(needle), "missing \"{needle}\" in:\n{text}");
        }
        assert_eq!(confirmation.required_phrase, "ERASE T123456A");
        assert!(confirmation.phrase_instruction.ends_with("ERASE T123456A"));
        assert!(confirmation.privacy_notice.contains("personal data"));
    }

    #[test]
    fn the_phrase_is_specific_to_the_disk() {
        let inventory = workstation();
        let nvme = plan_for(&inventory, NVME_SELECTOR, NO_OPTIONS);
        let sata = plan_for(&inventory, SATA_SELECTOR, NO_OPTIONS);
        assert_ne!(
            nvme.confirmation.required_phrase,
            sata.confirmation.required_phrase
        );
        assert_eq!(sata.confirmation.required_phrase, "ERASE M700001B");

        let mut identity = nvme.target.identity.clone();
        identity.serial = Some("ab-12".to_owned());
        assert_eq!(confirmation_phrase(&identity), "ERASE AB12");
        identity.serial = None;
        assert_eq!(confirmation_phrase(&identity), "ERASE 11B2C3D4");
        identity.wwn = Some("--".to_owned());
        assert_eq!(confirmation_phrase(&identity), "ERASE 1000204886016");
        identity.wwn = None;
        assert_eq!(confirmation_phrase(&identity), "ERASE 1000204886016");
    }

    #[test]
    fn a_weak_identity_is_recorded_in_the_plan() {
        let inventory = oddities();
        let plan = plan_for(
            &inventory,
            "model=Generic Mass Storage,size=64023257088",
            NO_OPTIONS,
        );
        assert!(plan.target.weaker_identity_acknowledged);
        assert_eq!(plan.target.identity_strength, IdentityStrength::Weak);
        assert_eq!(plan.confirmation.required_phrase, "ERASE 64023257088");
        assert!(
            plan.confirmation
                .lines
                .iter()
                .any(|line| line.contains("no serial number or WWN"))
        );

        let strong = plan_for(&workstation(), NVME_SELECTOR, NO_OPTIONS);
        assert!(!strong.target.weaker_identity_acknowledged);
        assert_eq!(strong.target.identity_strength, IdentityStrength::Stable);
    }

    #[test]
    fn plan_round_trips_through_json() {
        let inventory = workstation();
        let plan = plan_for(&inventory, NVME_SELECTOR, ENCRYPT_WITH_DATA);
        let json = serde_json::to_string_pretty(&plan).unwrap();
        let again: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(again, plan);
    }

    #[test]
    fn confirmation_requires_the_exact_phrase() {
        let inventory = workstation();
        let plan = plan_for(&inventory, SATA_SELECTOR, NO_OPTIONS);
        let preflight = preflight_for(&inventory, SATA_SELECTOR);
        let now = Timestamp::from_unix_millis(1_794_464_220_000);
        for wrong in [
            "",
            "ERASE",
            "erase m700001b",
            "ERASE M700001",
            "ERASE M700001B!",
            "ERASE T123456A",
            "yes",
            "ERASE  M700001B",
        ] {
            let error = plan.confirm(wrong, &preflight, now).unwrap_err();
            assert_eq!(error, ConfirmError::PhraseMismatch, "{wrong}");
            assert_eq!(error.code(), InstallCode::ConfirmPhraseMismatch);
        }
        let confirmed = plan.confirm("ERASE M700001B\n", &preflight, now).unwrap();
        assert_eq!(confirmed.plan(), &plan);
        assert_eq!(confirmed.confirmed_at(), now);
    }

    #[test]
    fn confirmation_requires_a_clean_preflight_for_the_same_disk() {
        let inventory = workstation();
        let plan = plan_for(&inventory, SATA_SELECTOR, NO_OPTIONS);
        let now = Timestamp::from_unix_millis(0);
        let phrase = plan.confirmation.required_phrase.clone();

        let other_disk = preflight_for(&inventory, NVME_SELECTOR);
        assert_eq!(
            plan.confirm(&phrase, &other_disk, now).unwrap_err(),
            ConfirmError::PreflightMismatch
        );

        let no_target = run_preflight(
            &HostFacts {
                environment: good_environment(),
                target: None,
            },
            &Requirements::for_layout(&ERASE_DISK_SCHEME, NO_OPTIONS).unwrap(),
        );
        assert_eq!(
            plan.confirm(&phrase, &no_target, now).unwrap_err(),
            ConfirmError::PreflightMismatch
        );

        let mut bios = good_environment();
        bios.boot_mode = Some(crate::preflight::BootMode::Bios);
        let blocked = run_preflight(
            &HostFacts {
                environment: bios,
                target: Some(TargetFacts::of(target(&inventory, SATA_SELECTOR).device())),
            },
            &Requirements::for_layout(&ERASE_DISK_SCHEME, NO_OPTIONS).unwrap(),
        );
        assert_eq!(
            plan.confirm(&phrase, &blocked, now).unwrap_err(),
            ConfirmError::PreflightBlocked {
                codes: vec![InstallCode::PreflightBootMode]
            }
        );
    }

    #[test]
    fn a_plan_read_from_a_file_cannot_skip_the_weak_identity_acknowledgement() {
        let inventory = oddities();
        let selector = "model=Generic Mass Storage,size=64023257088";
        let mut plan = plan_for(&inventory, selector, NO_OPTIONS);
        plan.target.weaker_identity_acknowledged = false;
        let preflight = preflight_for(&inventory, selector);
        let error = plan
            .confirm("ERASE 64023257088", &preflight, Timestamp::from_unix_millis(0))
            .unwrap_err();
        assert_eq!(error, ConfirmError::WeakIdentityNotAcknowledged);
        assert_eq!(error.code(), InstallCode::ConfirmWeakIdentity);
    }

    fn confirmed_sata_plan(inventory: &Inventory) -> ConfirmedPlan {
        let plan = plan_for(inventory, SATA_SELECTOR, NO_OPTIONS);
        let preflight = preflight_for(inventory, SATA_SELECTOR);
        plan.confirm(
            &plan.confirmation.required_phrase,
            &preflight,
            Timestamp::from_unix_millis(0),
        )
        .unwrap()
    }

    fn sata_mut(inventory: &mut Inventory) -> &mut BlockDevice {
        inventory
            .devices
            .iter_mut()
            .find(|device| device.name == "sda")
            .unwrap()
    }

    #[test]
    fn revalidation_returns_the_device_when_nothing_changed() {
        let inventory = workstation();
        let confirmed = confirmed_sata_plan(&inventory);
        let mut fresh = workstation();
        // The kernel renamed the disk between the two probes.
        {
            let sda = sata_mut(&mut fresh);
            sda.name = "sdf".to_owned();
            sda.kernel_name = "sdf".to_owned();
            sda.path = "/dev/sdf".to_owned();
            for (index, child) in sda.children.iter_mut().enumerate() {
                child.path = format!("/dev/sdf{}", index + 1);
            }
        }
        let validated = confirmed.revalidate(&fresh).unwrap();
        assert_eq!(validated.device().path, "/dev/sdf");
        assert_eq!(validated.plan(), confirmed.plan());
    }

    #[test]
    fn revalidation_stops_when_the_identity_changed() {
        let inventory = workstation();
        let confirmed = confirmed_sata_plan(&inventory);

        let mut gone = workstation();
        gone.devices.retain(|device| device.name != "sda");
        let error = confirmed.revalidate(&gone).unwrap_err();
        assert_eq!(error, RevalidateError::Identity(IdentityMismatch::Missing));
        assert_eq!(error.code(), InstallCode::IdentityMissing);

        let mut swapped = workstation();
        sata_mut(&mut swapped).serial = Some("S4EWNX0M799999Z".to_owned());
        assert_eq!(
            confirmed.revalidate(&swapped).unwrap_err().code(),
            InstallCode::IdentitySerialChanged
        );

        let mut resized = workstation();
        sata_mut(&mut resized).size_bytes += 512;
        assert_eq!(
            confirmed.revalidate(&resized).unwrap_err().code(),
            InstallCode::IdentitySizeChanged
        );
    }

    #[test]
    fn revalidation_stops_when_the_disk_became_unsafe_or_changed_contents() {
        let inventory = workstation();
        let confirmed = confirmed_sata_plan(&inventory);

        let mut mounted = workstation();
        sata_mut(&mut mounted).children[0].mountpoints = vec!["/mnt/efi".to_owned()];
        assert_eq!(
            confirmed.revalidate(&mounted).unwrap_err().code(),
            InstallCode::PlanTargetInUse
        );

        let mut read_only = workstation();
        sata_mut(&mut read_only).read_only = true;
        assert_eq!(
            confirmed.revalidate(&read_only).unwrap_err().code(),
            InstallCode::PlanTargetReadOnly
        );

        let mut live = workstation();
        sata_mut(&mut live).children[1].mountpoints = vec!["/run/initramfs/live".to_owned()];
        assert_eq!(
            confirmed.revalidate(&live).unwrap_err().code(),
            InstallCode::PlanTargetLiveMedium
        );

        let mut repartitioned = workstation();
        sata_mut(&mut repartitioned).children.pop();
        let error = confirmed.revalidate(&repartitioned).unwrap_err();
        assert_eq!(error, RevalidateError::ContentsChanged);
        assert_eq!(error.code(), InstallCode::ConfirmContentsChanged);

        let mut reformatted = workstation();
        sata_mut(&mut reformatted).children[2].filesystem_uuid =
            Some("00000000-1111-2222-3333-444444444444".to_owned());
        assert_eq!(
            confirmed.revalidate(&reformatted).unwrap_err(),
            RevalidateError::ContentsChanged
        );
    }

    #[test]
    fn target_refusal_is_none_for_an_idle_disk() {
        let inventory = workstation();
        let sda = inventory
            .devices
            .iter()
            .find(|device| device.name == "sda")
            .unwrap();
        assert_eq!(target_refusal(sda), None);
    }
}
