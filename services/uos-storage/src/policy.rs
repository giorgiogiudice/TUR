// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The filesystem policy table (specification §11.3) and the policy overrides.
//!
//! [`POLICY_TABLE`] is the single source of truth for what UOS does with each
//! filesystem: the default access, the implementation and the conditions for
//! writing. The mount decision reads this table; it holds no per-filesystem
//! policy of its own.
//!
//! [`PolicyOverrides`] is the whole set of knobs an administrator or caller
//! can turn. None of them can make APFS writable: the APFS row has
//! `writable: false`, no override refers to APFS writing, and the experimental
//! write driver appears only in [`EXCLUDED_IMPLEMENTATIONS`].

use serde::Serialize;

use crate::filesystem::Filesystem;

string_enum! {
    /// What UOS does with a filesystem when nothing else is known.
    pub enum DefaultAccess {
        /// Mounted read/write, subject to the row's write conditions.
        ReadWrite => "read-write",
        /// Mounted read-only unless every write condition holds.
        ReadOnly => "read-only",
        /// Never mounted directly: the device is a container for other volumes.
        Refuse => "refuse",
        /// Not supported; never mounted.
        Unsupported => "unsupported",
    }
}

string_enum! {
    /// Where the filesystem implementation runs.
    pub enum ImplementationKind {
        /// A driver in the Linux kernel.
        Kernel => "kernel",
        /// A userspace (FUSE) driver.
        Userspace => "userspace",
        /// No implementation is used.
        Absent => "none",
    }
}

string_enum! {
    /// A condition that must hold before a filesystem is mounted read/write.
    pub enum WriteCondition {
        /// The NTFS volume state is `Clean`.
        NtfsClean => "ntfs-clean",
        /// The HFS+ volume is known not to be journalled.
        HfsPlusNotJournaled => "hfsplus-not-journalled",
        /// The policy override that allows HFS+ writing is on.
        HfsPlusWriteOptIn => "hfsplus-write-opt-in",
    }
}

string_enum! {
    /// A condition that must hold before a filesystem is mounted at all.
    pub enum ReadCondition {
        /// The optional APFS feature is enabled (deferred by ADR-0007).
        ApfsFeatureEnabled => "apfs-feature-enabled",
    }
}

/// One row of the filesystem policy table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsPolicy {
    /// The filesystem this row applies to.
    pub filesystem: Filesystem,
    /// The name used in the specification and in user messages.
    pub spec_name: &'static str,
    /// Whether the row comes from specification §11.3. Rows that do not are
    /// additions needed to classify real devices and are listed in the design
    /// note.
    pub in_spec: bool,
    /// What happens when nothing else is known.
    pub default_access: DefaultAccess,
    /// Whether any combination of volume state and overrides can produce a
    /// read/write mount. `false` is final.
    pub writable: bool,
    /// Where the implementation runs.
    pub implementation: ImplementationKind,
    /// The driver, for example `ntfs3` or `apfs-fuse`.
    pub driver: Option<&'static str>,
    /// The filesystem type passed to the mount call, for kernel drivers.
    pub mount_type: Option<&'static str>,
    /// Every one of these must hold for a read/write mount.
    pub write_conditions: &'static [WriteCondition],
    /// Every one of these must hold for any mount.
    pub read_conditions: &'static [ReadCondition],
    /// The specification's note, or the reason for an added row.
    pub notes: &'static str,
}

const UNKNOWN_ROW: FsPolicy = FsPolicy {
    filesystem: Filesystem::Unknown,
    spec_name: "unrecognised filesystem",
    in_spec: false,
    default_access: DefaultAccess::Unsupported,
    writable: false,
    implementation: ImplementationKind::Absent,
    driver: None,
    mount_type: None,
    write_conditions: &[],
    read_conditions: &[],
    notes: "No signature, or a signature that is not in this table. Never mounted.",
};

/// The filesystem policy table. One row per [`Filesystem`].
pub const POLICY_TABLE: &[FsPolicy] = &[
    FsPolicy {
        filesystem: Filesystem::Ext4,
        spec_name: "ext4",
        in_spec: true,
        default_access: DefaultAccess::ReadWrite,
        writable: true,
        implementation: ImplementationKind::Kernel,
        driver: Some("ext4"),
        mount_type: Some("ext4"),
        write_conditions: &[],
        read_conditions: &[],
        notes: "Primary supported Linux filesystem.",
    },
    FsPolicy {
        filesystem: Filesystem::Btrfs,
        spec_name: "Btrfs",
        in_spec: true,
        default_access: DefaultAccess::ReadWrite,
        writable: true,
        implementation: ImplementationKind::Kernel,
        driver: Some("btrfs"),
        mount_type: Some("btrfs"),
        write_conditions: &[],
        read_conditions: &[],
        notes: "Good fit for snapshots and subvolumes.",
    },
    FsPolicy {
        filesystem: Filesystem::Xfs,
        spec_name: "XFS",
        in_spec: true,
        default_access: DefaultAccess::ReadWrite,
        writable: true,
        implementation: ImplementationKind::Kernel,
        driver: Some("xfs"),
        mount_type: Some("xfs"),
        write_conditions: &[],
        read_conditions: &[],
        notes: "Supported data volume.",
    },
    FsPolicy {
        filesystem: Filesystem::Ntfs,
        spec_name: "NTFS",
        in_spec: true,
        default_access: DefaultAccess::ReadWrite,
        writable: true,
        implementation: ImplementationKind::Kernel,
        driver: Some("ntfs3"),
        mount_type: Some("ntfs3"),
        write_conditions: &[WriteCondition::NtfsClean],
        read_conditions: &[],
        notes: "Read/write when clean. Unsafe writes to hibernated or dirty volumes are refused.",
    },
    FsPolicy {
        filesystem: Filesystem::Exfat,
        spec_name: "exFAT",
        in_spec: true,
        default_access: DefaultAccess::ReadWrite,
        writable: true,
        implementation: ImplementationKind::Kernel,
        driver: Some("exfat"),
        mount_type: Some("exfat"),
        write_conditions: &[],
        read_conditions: &[],
        notes: "Good removable-media format.",
    },
    FsPolicy {
        filesystem: Filesystem::Vfat,
        spec_name: "FAT32/VFAT",
        in_spec: true,
        default_access: DefaultAccess::ReadWrite,
        writable: true,
        implementation: ImplementationKind::Kernel,
        driver: Some("vfat"),
        mount_type: Some("vfat"),
        write_conditions: &[],
        read_conditions: &[],
        notes: "EFI and removable media.",
    },
    FsPolicy {
        filesystem: Filesystem::HfsPlus,
        spec_name: "HFS+",
        in_spec: true,
        default_access: DefaultAccess::ReadOnly,
        writable: true,
        implementation: ImplementationKind::Kernel,
        driver: Some("hfsplus"),
        mount_type: Some("hfsplus"),
        // Checked in order: the journal state is reported before the opt-in.
        write_conditions: &[
            WriteCondition::HfsPlusNotJournaled,
            WriteCondition::HfsPlusWriteOptIn,
        ],
        read_conditions: &[],
        notes: "Policy-dependent. Caution with journalled volumes; validate before write.",
    },
    FsPolicy {
        filesystem: Filesystem::Apfs,
        spec_name: "APFS",
        in_spec: true,
        default_access: DefaultAccess::ReadOnly,
        writable: false,
        implementation: ImplementationKind::Userspace,
        driver: Some("apfs-fuse"),
        mount_type: None,
        write_conditions: &[],
        read_conditions: &[ReadCondition::ApfsFeatureEnabled],
        notes: "Read-only. Incomplete feature coverage; FUSE overhead. Deferred by ADR-0007.",
    },
    FsPolicy {
        filesystem: Filesystem::Refs,
        spec_name: "ReFS",
        in_spec: true,
        default_access: DefaultAccess::Unsupported,
        writable: false,
        implementation: ImplementationKind::Absent,
        driver: None,
        mount_type: None,
        write_conditions: &[],
        read_conditions: &[],
        notes: "No mature general-purpose path assumed. Not advertised as supported.",
    },
    FsPolicy {
        filesystem: Filesystem::Zfs,
        spec_name: "ZFS",
        in_spec: true,
        default_access: DefaultAccess::Unsupported,
        writable: false,
        implementation: ImplementationKind::Absent,
        driver: None,
        mount_type: None,
        write_conditions: &[],
        read_conditions: &[],
        notes: "Optional in the specification (OpenZFS). Not included until the licensing and \
                kernel-compatibility review is done.",
    },
    FsPolicy {
        filesystem: Filesystem::Iso9660,
        spec_name: "ISO 9660",
        in_spec: false,
        default_access: DefaultAccess::ReadOnly,
        writable: false,
        implementation: ImplementationKind::Kernel,
        driver: Some("iso9660"),
        mount_type: Some("iso9660"),
        write_conditions: &[],
        read_conditions: &[],
        notes: "Added row. Optical media and disc images; a read-only format.",
    },
    FsPolicy {
        filesystem: Filesystem::Luks,
        spec_name: "LUKS container",
        in_spec: false,
        default_access: DefaultAccess::Refuse,
        writable: false,
        implementation: ImplementationKind::Absent,
        driver: None,
        mount_type: None,
        write_conditions: &[],
        read_conditions: &[],
        notes: "Added row. A locked container needs unlocking; the filesystem inside the \
                unlocked mapping is the volume that is mounted.",
    },
    FsPolicy {
        filesystem: Filesystem::BitLocker,
        spec_name: "BitLocker volume",
        in_spec: false,
        default_access: DefaultAccess::Unsupported,
        writable: false,
        implementation: ImplementationKind::Absent,
        driver: None,
        mount_type: None,
        write_conditions: &[],
        read_conditions: &[],
        notes: "Added row. Unlocking BitLocker volumes is not implemented.",
    },
    FsPolicy {
        filesystem: Filesystem::LvmMember,
        spec_name: "LVM physical volume",
        in_spec: false,
        default_access: DefaultAccess::Refuse,
        writable: false,
        implementation: ImplementationKind::Absent,
        driver: None,
        mount_type: None,
        write_conditions: &[],
        read_conditions: &[],
        notes: "Added row. The logical volumes inside it are the volumes that are mounted.",
    },
    FsPolicy {
        filesystem: Filesystem::RaidMember,
        spec_name: "MD RAID member",
        in_spec: false,
        default_access: DefaultAccess::Refuse,
        writable: false,
        implementation: ImplementationKind::Absent,
        driver: None,
        mount_type: None,
        write_conditions: &[],
        read_conditions: &[],
        notes: "Added row. The assembled array is the volume that is mounted.",
    },
    UNKNOWN_ROW,
];

/// An implementation that exists upstream and that UOS deliberately does not
/// use. It cannot be selected through this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExcludedImplementation {
    /// The filesystem the implementation is for.
    pub filesystem: Filesystem,
    /// The upstream project name.
    pub name: &'static str,
    /// The policy stated in specification §11.3.
    pub spec_policy: &'static str,
    /// Why it is excluded.
    pub reason: &'static str,
}

/// Implementations named in specification §11.3 that this crate never selects.
pub const EXCLUDED_IMPLEMENTATIONS: &[ExcludedImplementation] = &[ExcludedImplementation {
    filesystem: Filesystem::Apfs,
    name: "linux-apfs-rw",
    spec_policy: "Disabled by default",
    reason: "Reverse-engineered, with write risk and encryption gaps. There is no option in \
             uos-storage that selects it.",
}];

/// Returns the policy row for a filesystem.
///
/// The table has a row for every [`Filesystem`]; a unit test checks that. If
/// a row were ever missing, the lookup falls back to the unsupported row
/// rather than to anything that mounts.
pub fn policy_for(filesystem: Filesystem) -> &'static FsPolicy {
    POLICY_TABLE
        .iter()
        .find(|row| row.filesystem == filesystem)
        .unwrap_or(&UNKNOWN_ROW)
}

string_enum! {
    /// What to do with an NTFS volume that is hibernated, dirty or of unknown
    /// state. Both choices protect the volume; neither allows writing.
    pub enum UnsafeNtfsPolicy {
        /// Mount it read-only.
        ReadOnly => "read-only",
        /// Do not mount it at all.
        Refuse => "refuse",
    }
}

/// The policy overrides: every knob that can change a mount decision.
///
/// The defaults are the UOS defaults. Each override either tightens the policy
/// or enables something the policy table already bounds. There is no field
/// that enables APFS writing or relaxes the NTFS state check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct PolicyOverrides {
    /// Handling of NTFS volumes that are not known to be clean.
    pub unsafe_ntfs: UnsafeNtfsPolicy,
    /// Allow read/write mounts of HFS+ volumes that are known not to be
    /// journalled. Off by default.
    pub hfsplus_write: bool,
    /// Enable read-only APFS mounts through `apfs-fuse`. Off by default
    /// because APFS is deferred (ADR-0007).
    pub apfs_read_only: bool,
    /// Add `noexec` to mounts of removable media. Off by default because the
    /// Windows runtime cannot start programs from a `noexec` mount.
    pub noexec_removable: bool,
}

impl Default for PolicyOverrides {
    fn default() -> Self {
        PolicyOverrides {
            unsafe_ntfs: UnsafeNtfsPolicy::ReadOnly,
            hfsplus_write: false,
            apfs_read_only: false,
            noexec_removable: false,
        }
    }
}

impl PolicyOverrides {
    /// Every value of the override space.
    ///
    /// The struct literal below names every field, so adding an override
    /// without extending this enumeration does not compile. Safety tests run
    /// over this whole space.
    pub fn all() -> Vec<PolicyOverrides> {
        let mut space = Vec::new();
        for &unsafe_ntfs in UnsafeNtfsPolicy::all() {
            for hfsplus_write in [false, true] {
                for apfs_read_only in [false, true] {
                    for noexec_removable in [false, true] {
                        space.push(PolicyOverrides {
                            unsafe_ntfs,
                            hfsplus_write,
                            apfs_read_only,
                            noexec_removable,
                        });
                    }
                }
            }
        }
        space
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(filesystem: Filesystem) -> &'static FsPolicy {
        policy_for(filesystem)
    }

    #[test]
    fn every_filesystem_has_exactly_one_row() {
        for &filesystem in Filesystem::all() {
            let rows = POLICY_TABLE
                .iter()
                .filter(|row| row.filesystem == filesystem)
                .count();
            assert_eq!(rows, 1, "{filesystem}");
            assert_eq!(policy_for(filesystem).filesystem, filesystem);
        }
        assert_eq!(POLICY_TABLE.len(), Filesystem::all().len());
    }

    #[test]
    fn table_matches_specification_section_11_3() {
        // Filesystem, default access, implementation, driver: one line per
        // specification row that maps to a filesystem.
        let expected = [
            (
                Filesystem::Ext4,
                DefaultAccess::ReadWrite,
                ImplementationKind::Kernel,
                Some("ext4"),
            ),
            (
                Filesystem::Btrfs,
                DefaultAccess::ReadWrite,
                ImplementationKind::Kernel,
                Some("btrfs"),
            ),
            (
                Filesystem::Xfs,
                DefaultAccess::ReadWrite,
                ImplementationKind::Kernel,
                Some("xfs"),
            ),
            (
                Filesystem::Ntfs,
                DefaultAccess::ReadWrite,
                ImplementationKind::Kernel,
                Some("ntfs3"),
            ),
            (
                Filesystem::Exfat,
                DefaultAccess::ReadWrite,
                ImplementationKind::Kernel,
                Some("exfat"),
            ),
            (
                Filesystem::Vfat,
                DefaultAccess::ReadWrite,
                ImplementationKind::Kernel,
                Some("vfat"),
            ),
            (
                Filesystem::HfsPlus,
                DefaultAccess::ReadOnly,
                ImplementationKind::Kernel,
                Some("hfsplus"),
            ),
            (
                Filesystem::Apfs,
                DefaultAccess::ReadOnly,
                ImplementationKind::Userspace,
                Some("apfs-fuse"),
            ),
            (
                Filesystem::Refs,
                DefaultAccess::Unsupported,
                ImplementationKind::Absent,
                None,
            ),
            (
                Filesystem::Zfs,
                DefaultAccess::Unsupported,
                ImplementationKind::Absent,
                None,
            ),
        ];
        for (filesystem, access, implementation, driver) in expected {
            let row = row(filesystem);
            assert!(row.in_spec, "{filesystem}");
            assert_eq!(row.default_access, access, "{filesystem}");
            assert_eq!(row.implementation, implementation, "{filesystem}");
            assert_eq!(row.driver, driver, "{filesystem}");
        }
        let spec_rows = POLICY_TABLE.iter().filter(|row| row.in_spec).count();
        assert_eq!(spec_rows, expected.len());
    }

    #[test]
    fn ntfs_writes_require_a_clean_volume() {
        assert_eq!(
            row(Filesystem::Ntfs).write_conditions,
            &[WriteCondition::NtfsClean]
        );
    }

    #[test]
    fn hfsplus_writes_require_opt_in_and_no_journal() {
        let conditions = row(Filesystem::HfsPlus).write_conditions;
        assert!(conditions.contains(&WriteCondition::HfsPlusWriteOptIn));
        assert!(conditions.contains(&WriteCondition::HfsPlusNotJournaled));
    }

    #[test]
    fn apfs_row_is_never_writable_and_names_no_write_driver() {
        let apfs = row(Filesystem::Apfs);
        assert!(!apfs.writable);
        assert!(apfs.write_conditions.is_empty());
        assert_eq!(apfs.driver, Some("apfs-fuse"));
        assert_eq!(apfs.read_conditions, &[ReadCondition::ApfsFeatureEnabled]);
    }

    #[test]
    fn excluded_implementations_are_not_drivers_in_the_table() {
        assert!(
            EXCLUDED_IMPLEMENTATIONS
                .iter()
                .any(|excluded| excluded.name == "linux-apfs-rw")
        );
        for excluded in EXCLUDED_IMPLEMENTATIONS {
            for row in POLICY_TABLE {
                assert_ne!(row.driver, Some(excluded.name));
                assert_ne!(row.mount_type, Some(excluded.name));
            }
        }
    }

    #[test]
    fn rows_that_never_mount_have_no_driver_and_are_not_writable() {
        for row in POLICY_TABLE {
            let mounts = matches!(
                row.default_access,
                DefaultAccess::ReadWrite | DefaultAccess::ReadOnly
            );
            if mounts {
                assert!(row.driver.is_some(), "{}", row.filesystem);
            } else {
                assert!(row.driver.is_none(), "{}", row.filesystem);
                assert!(!row.writable, "{}", row.filesystem);
                assert_eq!(row.implementation, ImplementationKind::Absent);
            }
            if row.default_access == DefaultAccess::ReadWrite {
                assert!(row.writable, "{}", row.filesystem);
            }
            if !row.writable {
                assert!(row.write_conditions.is_empty(), "{}", row.filesystem);
            }
        }
    }

    #[test]
    fn override_space_is_complete_and_distinct() {
        let space = PolicyOverrides::all();
        assert_eq!(space.len(), 16);
        for (index, first) in space.iter().enumerate() {
            for second in &space[index + 1..] {
                assert_ne!(first, second);
            }
        }
        assert!(space.contains(&PolicyOverrides::default()));
    }

    #[test]
    fn defaults_are_the_conservative_choices() {
        let defaults = PolicyOverrides::default();
        assert_eq!(defaults.unsafe_ntfs, UnsafeNtfsPolicy::ReadOnly);
        assert!(!defaults.hfsplus_write);
        assert!(!defaults.apfs_read_only);
        assert!(!defaults.noexec_removable);
    }
}
