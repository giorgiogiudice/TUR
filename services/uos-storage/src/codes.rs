// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The `UOS-STORAGE-*` error-code registry (specification §29.3).
//!
//! Every code has one registry entry with a title, severity, detection
//! criteria, user message, technical explanation and recovery action.
//! [`StorageCode::entry`] is an exhaustive `match`, so a code without an entry
//! does not compile. The meaning of a released code never changes.
//!
//! No text in this registry tells a user to override a filesystem safety
//! check; a test scans the registry for such wording.

use uos_core::{ErrorCode, Family};

string_enum! {
    /// How serious the condition behind a code is.
    pub enum Severity {
        /// Nothing is wrong; the code records a normal outcome.
        Info => "info",
        /// The user did not get everything they asked for, by design.
        Notice => "notice",
        /// Data was protected from a risky operation.
        Warning => "warning",
        /// The operation could not be carried out.
        Error => "error",
    }
}

string_enum! {
    /// Where a code is produced.
    pub enum CodeOrigin {
        /// Returned by the mount decision.
        Decision => "decision",
        /// Returned when the input (probe data or a volume identifier) is bad.
        Input => "input",
    }
}

string_enum! {
    /// A stable storage error code.
    pub enum StorageCode {
        /// A read/write mount is permitted.
        MountReadWrite => "UOS-STORAGE-MOUNT-001",
        /// A read-only mount was requested and is permitted.
        MountReadOnlyRequested => "UOS-STORAGE-MOUNT-002",
        /// The volume is already mounted outside the UOS volume namespace.
        MountedElsewhere => "UOS-STORAGE-MOUNT-003",
        /// The volume belongs to the running system.
        SystemVolume => "UOS-STORAGE-SYSTEM-001",
        /// Two devices report the same filesystem identifier.
        DuplicateIdentity => "UOS-STORAGE-ID-001",
        /// An NTFS volume is hibernated or in an unsafe state.
        NtfsUnsafe => "UOS-STORAGE-NTFS-004",
        /// An NTFS volume is marked dirty.
        NtfsDirty => "UOS-STORAGE-NTFS-005",
        /// An HFS+ volume is journalled, or its journal state is unknown.
        HfsPlusJournaled => "UOS-STORAGE-HFSPLUS-001",
        /// HFS+ writing is switched off by policy.
        HfsPlusWriteDisabled => "UOS-STORAGE-HFSPLUS-002",
        /// APFS is read-only.
        ApfsReadOnly => "UOS-STORAGE-APFS-001",
        /// APFS support is not enabled.
        ApfsNotEnabled => "UOS-STORAGE-APFS-002",
        /// The filesystem is known and not supported.
        FilesystemUnsupported => "UOS-STORAGE-FS-001",
        /// No supported filesystem was recognised.
        FilesystemUnknown => "UOS-STORAGE-FS-002",
        /// The filesystem is a read-only format.
        FilesystemReadOnlyByDesign => "UOS-STORAGE-FS-003",
        /// The device or medium is write-protected.
        DeviceReadOnly => "UOS-STORAGE-MEDIA-001",
        /// An encrypted volume is locked.
        EncryptedLocked => "UOS-STORAGE-LUKS-001",
        /// The device is a container for other volumes.
        ContainerNotMountable => "UOS-STORAGE-CONTAINER-001",
        /// The volume is encrypted with BitLocker.
        BitLockerUnsupported => "UOS-STORAGE-BITLOCKER-001",
        /// Probe input could not be read or parsed.
        ProbeInputInvalid => "UOS-STORAGE-PROBE-001",
        /// The requested volume identifier is not in the inventory.
        NoSuchVolume => "UOS-STORAGE-VOLUME-001",
    }
}

/// A registry entry: everything specification §29.3 requires for a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeEntry {
    /// The code.
    pub code: StorageCode,
    /// A short title.
    pub title: &'static str,
    /// How serious the condition is.
    pub severity: Severity,
    /// Where the code is produced.
    pub origin: CodeOrigin,
    /// The facts that cause this code to be reported.
    pub detection: &'static str,
    /// What the user is told happened.
    pub user_message: &'static str,
    /// Why, in technical terms.
    pub technical_explanation: &'static str,
    /// What the user can safely do about it.
    pub recovery_action: &'static str,
}

impl StorageCode {
    /// The registry entry for this code.
    pub const fn entry(self) -> CodeEntry {
        match self {
            StorageCode::MountReadWrite => CodeEntry {
                code: self,
                title: "Read/write mount permitted",
                severity: Severity::Info,
                origin: CodeOrigin::Decision,
                detection: "Read/write access was requested, the filesystem policy allows writing, \
                            every write condition holds and the device is not write-protected.",
                user_message: "The volume can be mounted for reading and writing.",
                technical_explanation: "The filesystem policy table permits a read/write mount \
                                        with the listed driver and options.",
                recovery_action: "No action is needed.",
            },
            StorageCode::MountReadOnlyRequested => CodeEntry {
                code: self,
                title: "Read-only mount as requested",
                severity: Severity::Info,
                origin: CodeOrigin::Decision,
                detection: "Read-only access was requested and the filesystem can be mounted.",
                user_message: "The volume can be mounted read-only, as requested.",
                technical_explanation: "A read-only mount was requested. The decision does not \
                                        say whether writing would have been allowed.",
                recovery_action: "No action is needed. Request read/write access to see whether \
                                  writing is allowed.",
            },
            StorageCode::MountedElsewhere => CodeEntry {
                code: self,
                title: "Volume already mounted elsewhere",
                severity: Severity::Warning,
                origin: CodeOrigin::Decision,
                detection: "The volume is not system storage and has a mount point other than \
                            its canonical path under /run/uos/volumes.",
                user_message: "The volume is already in use and was not mounted again.",
                technical_explanation: "UOS keeps one mount per filesystem. A second mount of the \
                                        same filesystem is never planned.",
                recovery_action: "Unmount the volume where it is currently mounted, then retry.",
            },
            StorageCode::SystemVolume => CodeEntry {
                code: self,
                title: "Volume belongs to the running system",
                severity: Severity::Notice,
                origin: CodeOrigin::Decision,
                detection: "The volume, or a volume inside it, is mounted at a system path such \
                            as /, /sysroot, /boot, /boot/efi, /etc or /var, or is active swap.",
                user_message: "This volume is part of the running system and is not offered for \
                               mounting.",
                technical_explanation: "The running system's own storage is managed by the boot \
                                        and update stack. uos-storage never remounts it.",
                recovery_action: "No action is needed.",
            },
            StorageCode::DuplicateIdentity => CodeEntry {
                code: self,
                title: "Duplicate filesystem identifier",
                severity: Severity::Error,
                origin: CodeOrigin::Decision,
                detection: "Two or more connected devices report the same filesystem identifier \
                            and are not members of one multi-device Btrfs filesystem.",
                user_message: "Two connected devices look like the same volume, so neither was \
                               mounted.",
                technical_explanation: "Cloned filesystems share a UUID. Mounting both, or the \
                                        wrong one, can corrupt data on filesystems that identify \
                                        their devices by UUID.",
                recovery_action: "Disconnect one of the two devices, then retry.",
            },
            StorageCode::NtfsUnsafe => CodeEntry {
                code: self,
                title: "NTFS volume hibernated or unsafe",
                severity: Severity::Warning,
                origin: CodeOrigin::Decision,
                detection: "The filesystem is NTFS and its state is reported as hibernated, or \
                            its state could not be determined.",
                user_message: "The Windows volume was not mounted for writing.",
                technical_explanation: "Windows hibernation and Fast Startup leave the volume in \
                                        a state that Windows expects to find unchanged. Writing \
                                        to it from another system can destroy data. UOS never \
                                        overrides this check.",
                recovery_action: "Boot Windows, disable Fast Startup/hibernation, shut down \
                                  fully, then retry.",
            },
            StorageCode::NtfsDirty => CodeEntry {
                code: self,
                title: "NTFS volume marked dirty",
                severity: Severity::Warning,
                origin: CodeOrigin::Decision,
                detection: "The filesystem is NTFS and its dirty flag is set.",
                user_message: "The Windows volume was not mounted for writing.",
                technical_explanation: "The NTFS dirty flag means the volume was not shut down \
                                        cleanly or has errors that need a consistency check. The \
                                        ntfs3 driver rejects a normal read/write mount of such a \
                                        volume, and UOS never overrides that.",
                recovery_action: "Boot Windows, let it check the disk, shut down fully, then \
                                  retry.",
            },
            StorageCode::HfsPlusJournaled => CodeEntry {
                code: self,
                title: "HFS+ volume journalled",
                severity: Severity::Warning,
                origin: CodeOrigin::Decision,
                detection: "The filesystem is HFS+ and it is journalled, or its journal state \
                            could not be determined.",
                user_message: "The Mac volume was not mounted for writing.",
                technical_explanation: "The Linux hfsplus driver does not support writing to a \
                                        journalled volume and keeps it read-only. UOS never \
                                        overrides that.",
                recovery_action: "Use the volume read-only, or copy the data to a volume that \
                                  UOS can write.",
            },
            StorageCode::HfsPlusWriteDisabled => CodeEntry {
                code: self,
                title: "HFS+ writing disabled by policy",
                severity: Severity::Notice,
                origin: CodeOrigin::Decision,
                detection: "The filesystem is HFS+, it is known not to be journalled, and the \
                            policy option that allows HFS+ writing is off.",
                user_message: "The Mac volume was not mounted for writing.",
                technical_explanation: "Writing to HFS+ is policy-dependent and off by default \
                                        until it has been validated.",
                recovery_action: "Use the volume read-only. An administrator can allow writing \
                                  to HFS+ volumes that are not journalled.",
            },
            StorageCode::ApfsReadOnly => CodeEntry {
                code: self,
                title: "APFS is read-only",
                severity: Severity::Notice,
                origin: CodeOrigin::Decision,
                detection: "The filesystem is APFS, APFS support is enabled and read/write \
                            access was requested.",
                user_message: "The APFS volume is available read-only.",
                technical_explanation: "APFS is mounted through the read-only apfs-fuse driver. \
                                        Experimental APFS write support cannot be selected \
                                        through uos-storage.",
                recovery_action: "Copy the data you need to a volume that UOS can write.",
            },
            StorageCode::ApfsNotEnabled => CodeEntry {
                code: self,
                title: "APFS support not enabled",
                severity: Severity::Notice,
                origin: CodeOrigin::Decision,
                detection: "The filesystem is APFS and APFS support is not enabled.",
                user_message: "The APFS volume was not mounted.",
                technical_explanation: "APFS support is deferred (ADR-0007) and off by default. \
                                        When enabled it is read-only.",
                recovery_action: "Use the volume on a Mac, or copy its data there to a format \
                                  that UOS supports. UOS has not changed the volume.",
            },
            StorageCode::FilesystemUnsupported => CodeEntry {
                code: self,
                title: "Filesystem not supported",
                severity: Severity::Notice,
                origin: CodeOrigin::Decision,
                detection: "The signature identifies a filesystem that the policy table marks \
                            unsupported, such as ReFS or ZFS.",
                user_message: "This volume uses a format that UOS does not support.",
                technical_explanation: "The filesystem policy table has no driver for this \
                                        filesystem.",
                recovery_action: "Use the volume with the operating system that created it. UOS \
                                  has not changed the volume.",
            },
            StorageCode::FilesystemUnknown => CodeEntry {
                code: self,
                title: "No recognised filesystem",
                severity: Severity::Notice,
                origin: CodeOrigin::Decision,
                detection: "No filesystem signature was found, or the signature is not in the \
                            policy table.",
                user_message: "No supported filesystem was found on this volume.",
                technical_explanation: "Only filesystems in the policy table are mounted. An \
                                        unrecognised signature is never passed to a driver.",
                recovery_action: "If the volume holds data, use it with the system that created \
                                  it. UOS has not changed the volume.",
            },
            StorageCode::FilesystemReadOnlyByDesign => CodeEntry {
                code: self,
                title: "Read-only filesystem format",
                severity: Severity::Info,
                origin: CodeOrigin::Decision,
                detection: "The filesystem is a read-only format such as ISO 9660 and \
                            read/write access was requested.",
                user_message: "The volume is available read-only because its format cannot be \
                               written.",
                technical_explanation: "The policy table marks this filesystem as never writable.",
                recovery_action: "No action is needed.",
            },
            StorageCode::DeviceReadOnly => CodeEntry {
                code: self,
                title: "Device is write-protected",
                severity: Severity::Notice,
                origin: CodeOrigin::Decision,
                detection: "The block device, or a device beneath it, reports itself read-only, \
                            or it is an optical drive, and read/write access was requested.",
                user_message: "The volume is available read-only because the device is \
                               write-protected.",
                technical_explanation: "The kernel reports the block device as read-only.",
                recovery_action: "If the device has a write-protection switch, change it and \
                                  reconnect the device.",
            },
            StorageCode::EncryptedLocked => CodeEntry {
                code: self,
                title: "Encrypted volume is locked",
                severity: Severity::Info,
                origin: CodeOrigin::Decision,
                detection: "The signature is LUKS and no unlocked mapping exists.",
                user_message: "The volume is encrypted and must be unlocked first.",
                technical_explanation: "Unlocking is carried out by the storage service through \
                                        cryptsetup. It is not part of the policy core.",
                recovery_action: "Unlock the volume with its passphrase, then mount the volume \
                                  inside it.",
            },
            StorageCode::ContainerNotMountable => CodeEntry {
                code: self,
                title: "Container, not a filesystem",
                severity: Severity::Info,
                origin: CodeOrigin::Decision,
                detection: "The signature is an unlocked LUKS container, an LVM physical volume \
                            or an MD RAID member.",
                user_message: "This device holds other volumes and cannot be mounted itself.",
                technical_explanation: "The device has no filesystem of its own. The volumes \
                                        inside it appear in the inventory separately.",
                recovery_action: "Mount the volume inside it instead.",
            },
            StorageCode::BitLockerUnsupported => CodeEntry {
                code: self,
                title: "BitLocker volume not supported",
                severity: Severity::Notice,
                origin: CodeOrigin::Decision,
                detection: "The signature is BitLocker.",
                user_message: "The volume is encrypted with BitLocker and was not mounted.",
                technical_explanation: "Unlocking BitLocker volumes is not implemented in \
                                        uos-storage.",
                recovery_action: "Use the volume in Windows. UOS has not changed the volume.",
            },
            StorageCode::ProbeInputInvalid => CodeEntry {
                code: self,
                title: "Volume data could not be read",
                severity: Severity::Error,
                origin: CodeOrigin::Input,
                detection: "The lsblk JSON or the property file could not be read or parsed.",
                user_message: "The list of volumes could not be read.",
                technical_explanation: "The probe input is missing, is not valid JSON, or does \
                                        not have the shape of lsblk output with byte sizes.",
                recovery_action: "Check that the input was produced by lsblk --json --bytes \
                                  --output-all, then retry.",
            },
            StorageCode::NoSuchVolume => CodeEntry {
                code: self,
                title: "Unknown volume identifier",
                severity: Severity::Error,
                origin: CodeOrigin::Input,
                detection: "The requested volume identifier is not in the inventory.",
                user_message: "No volume with that identifier was found.",
                technical_explanation: "Volume identifiers are derived from the probe data. The \
                                        identifier may belong to a device that is no longer \
                                        connected.",
                recovery_action: "List the volumes and use an identifier from the list.",
            },
        }
    }

    /// The code as a validated [`ErrorCode`].
    ///
    /// Returns `None` only if the text form is not a valid UOS storage code; a
    /// unit test checks that this never happens for a registered code.
    pub fn error_code(self) -> Option<ErrorCode> {
        self.as_str().parse::<ErrorCode>().ok().filter(|code| code.family() == Family::Storage)
    }
}

/// The whole registry, in code order.
pub fn registry() -> Vec<CodeEntry> {
    StorageCode::all().iter().map(|code| code.entry()).collect()
}

/// Wording that must never appear in a message, option or registry entry: it
/// would steer a user towards overriding a filesystem safety check.
pub const FORBIDDEN_WORDING: &[&str] = &["force", "remove_hiberfile", "ntfsfix", "recover"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_code_parses_as_a_storage_code() {
        for &code in StorageCode::all() {
            let parsed = code.error_code().unwrap_or_else(|| panic!("{code} does not parse"));
            assert_eq!(parsed.family(), Family::Storage);
            assert_eq!(parsed.as_str(), code.as_str());
        }
    }

    #[test]
    fn codes_are_unique() {
        let texts: BTreeSet<&str> = StorageCode::all().iter().map(|code| code.as_str()).collect();
        assert_eq!(texts.len(), StorageCode::all().len());
    }

    #[test]
    fn every_entry_is_complete() {
        let entries = registry();
        assert_eq!(entries.len(), StorageCode::all().len());
        for entry in entries {
            assert_eq!(entry.code.entry(), entry);
            for text in [
                entry.title,
                entry.detection,
                entry.user_message,
                entry.technical_explanation,
                entry.recovery_action,
            ] {
                assert!(!text.trim().is_empty(), "{}", entry.code);
                assert!(!text.contains("  "), "{}: double space in {text:?}", entry.code);
            }
            assert!(entry.user_message.ends_with('.'), "{}", entry.code);
            assert!(entry.recovery_action.ends_with('.'), "{}", entry.code);
        }
    }

    #[test]
    fn ntfs_004_keeps_the_specification_meaning() {
        let entry = StorageCode::NtfsUnsafe.entry();
        assert_eq!(entry.code.as_str(), "UOS-STORAGE-NTFS-004");
        assert_eq!(entry.user_message, "The Windows volume was not mounted for writing.");
        assert_eq!(
            entry.recovery_action,
            "Boot Windows, disable Fast Startup/hibernation, shut down fully, then retry."
        );
        assert_eq!(entry.severity, Severity::Warning);
    }

    #[test]
    fn registry_never_suggests_overriding_a_safety_check() {
        for entry in registry() {
            for text in [
                entry.title,
                entry.detection,
                entry.user_message,
                entry.technical_explanation,
                entry.recovery_action,
            ] {
                let lowered = text.to_lowercase();
                for word in FORBIDDEN_WORDING {
                    assert!(!lowered.contains(word), "{}: {word:?} in {text:?}", entry.code);
                }
            }
        }
    }

    #[test]
    fn only_input_codes_are_outside_the_decision() {
        let input: Vec<StorageCode> = registry()
            .into_iter()
            .filter(|entry| entry.origin == CodeOrigin::Input)
            .map(|entry| entry.code)
            .collect();
        assert_eq!(input, vec![StorageCode::ProbeInputInvalid, StorageCode::NoSuchVolume]);
    }
}
