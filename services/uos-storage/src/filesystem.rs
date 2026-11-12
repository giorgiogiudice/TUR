// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Filesystem and container signatures that the storage policy recognises.
//!
//! A signature is the `TYPE` that libblkid reports for a block device and that
//! `lsblk` prints as `FSTYPE`. Anything not listed here is
//! [`Filesystem::Unknown`] and is never mounted.

string_enum! {
    /// A recognised filesystem or container signature.
    pub enum Filesystem {
        /// ext4.
        Ext4 => "ext4",
        /// Btrfs.
        Btrfs => "btrfs",
        /// XFS.
        Xfs => "xfs",
        /// NTFS.
        Ntfs => "ntfs",
        /// exFAT.
        Exfat => "exfat",
        /// FAT12, FAT16 or FAT32.
        Vfat => "vfat",
        /// HFS+ (Mac OS Extended).
        HfsPlus => "hfsplus",
        /// APFS.
        Apfs => "apfs",
        /// ReFS.
        Refs => "refs",
        /// A ZFS pool member.
        Zfs => "zfs",
        /// ISO 9660 (optical media and disc images).
        Iso9660 => "iso9660",
        /// A LUKS encrypted container.
        Luks => "luks",
        /// A BitLocker encrypted volume.
        BitLocker => "bitlocker",
        /// An LVM physical volume.
        LvmMember => "lvm-member",
        /// A Linux MD RAID member.
        RaidMember => "raid-member",
        /// No signature, or a signature the policy table does not know.
        Unknown => "unknown",
    }
}

impl Filesystem {
    /// Maps a libblkid `TYPE` value to a filesystem.
    ///
    /// The comparison ignores ASCII case because blkid's own spelling is mixed
    /// (`crypto_LUKS`, `ReFS`, `BitLocker`).
    pub fn from_signature(signature: Option<&str>) -> Filesystem {
        let Some(signature) = signature else {
            return Filesystem::Unknown;
        };
        match signature.trim().to_ascii_lowercase().as_str() {
            "ext4" => Filesystem::Ext4,
            "btrfs" => Filesystem::Btrfs,
            "xfs" => Filesystem::Xfs,
            "ntfs" => Filesystem::Ntfs,
            "exfat" => Filesystem::Exfat,
            "vfat" => Filesystem::Vfat,
            "hfsplus" => Filesystem::HfsPlus,
            "apfs" => Filesystem::Apfs,
            "refs" => Filesystem::Refs,
            "zfs_member" => Filesystem::Zfs,
            "iso9660" => Filesystem::Iso9660,
            "crypto_luks" => Filesystem::Luks,
            "bitlocker" => Filesystem::BitLocker,
            "lvm2_member" => Filesystem::LvmMember,
            "linux_raid_member" => Filesystem::RaidMember,
            _ => Filesystem::Unknown,
        }
    }

    /// Parses a filesystem name given by a person on the command line.
    ///
    /// Accepts the stable text form, the blkid signature and a few common
    /// spellings such as `fat32` and `hfs+`. Returns `None` when the name is
    /// not recognised; it never falls back to [`Filesystem::Unknown`].
    pub fn from_name(name: &str) -> Option<Filesystem> {
        let lowered = name.trim().to_ascii_lowercase();
        if let Some(found) = Filesystem::from_text(&lowered) {
            return Some(found);
        }
        let alias = match lowered.as_str() {
            "fat" | "fat12" | "fat16" | "fat32" | "msdos" => Some(Filesystem::Vfat),
            "hfs+" | "hfsx" => Some(Filesystem::HfsPlus),
            "iso" | "cdfs" => Some(Filesystem::Iso9660),
            "ntfs3" => Some(Filesystem::Ntfs),
            _ => None,
        };
        alias.or_else(|| match Filesystem::from_signature(Some(&lowered)) {
            Filesystem::Unknown => None,
            found => Some(found),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blkid_signatures_map_to_filesystems() {
        let cases = [
            ("ext4", Filesystem::Ext4),
            ("btrfs", Filesystem::Btrfs),
            ("xfs", Filesystem::Xfs),
            ("ntfs", Filesystem::Ntfs),
            ("exfat", Filesystem::Exfat),
            ("vfat", Filesystem::Vfat),
            ("hfsplus", Filesystem::HfsPlus),
            ("apfs", Filesystem::Apfs),
            ("ReFS", Filesystem::Refs),
            ("zfs_member", Filesystem::Zfs),
            ("iso9660", Filesystem::Iso9660),
            ("crypto_LUKS", Filesystem::Luks),
            ("BitLocker", Filesystem::BitLocker),
            ("LVM2_member", Filesystem::LvmMember),
            ("linux_raid_member", Filesystem::RaidMember),
        ];
        for (signature, expected) in cases {
            assert_eq!(
                Filesystem::from_signature(Some(signature)),
                expected,
                "{signature}"
            );
        }
    }

    #[test]
    fn unknown_and_missing_signatures_are_unknown() {
        assert_eq!(Filesystem::from_signature(None), Filesystem::Unknown);
        assert_eq!(Filesystem::from_signature(Some("")), Filesystem::Unknown);
        assert_eq!(
            Filesystem::from_signature(Some("bcachefs")),
            Filesystem::Unknown
        );
        assert_eq!(
            Filesystem::from_signature(Some("ext3")),
            Filesystem::Unknown
        );
    }

    #[test]
    fn text_form_round_trips() {
        for &filesystem in Filesystem::all() {
            assert_eq!(Filesystem::from_text(filesystem.as_str()), Some(filesystem));
            assert_eq!(filesystem.to_string(), filesystem.as_str());
        }
    }

    #[test]
    fn names_accept_common_spellings_and_reject_nonsense() {
        assert_eq!(Filesystem::from_name("FAT32"), Some(Filesystem::Vfat));
        assert_eq!(Filesystem::from_name("hfs+"), Some(Filesystem::HfsPlus));
        assert_eq!(Filesystem::from_name("NTFS"), Some(Filesystem::Ntfs));
        assert_eq!(Filesystem::from_name("crypto_LUKS"), Some(Filesystem::Luks));
        assert_eq!(Filesystem::from_name("unknown"), Some(Filesystem::Unknown));
        assert_eq!(Filesystem::from_name("nonsense"), None);
    }
}
