// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The metadata capability registry (specification §11.5).
//!
//! For each filesystem UOS mounts, the registry records which metadata
//! features it can represent *as mounted by the driver UOS uses*, which is
//! not always what the on-disk format can hold. [`copy_losses`] compares two
//! filesystems and lists what a copy from one to the other would lose, so
//! that a caller can warn instead of discarding metadata silently.
//!
//! The entries come from kernel documentation and general knowledge of the
//! formats. They have not been validated against real filesystems. An entry
//! that could not be established is [`Support::Unverified`], and a loss that
//! depends on one is reported as [`LossKind::Unverified`] rather than hidden.

use serde::Serialize;

use crate::filesystem::Filesystem;
use crate::policy::policy_for;

string_enum! {
    /// A metadata feature tracked by the registry.
    pub enum Feature {
        /// POSIX permissions and ownership.
        PosixPermissions => "posix-permissions",
        /// POSIX access control lists.
        Acls => "acls",
        /// Extended attributes.
        ExtendedAttributes => "extended-attributes",
        /// Windows alternate data streams.
        AlternateDataStreams => "alternate-data-streams",
        /// Windows security descriptors.
        SecurityDescriptors => "windows-security-descriptors",
        /// Apple resource forks.
        ResourceForks => "apple-resource-forks",
        /// Names that differ only in case are distinct files.
        CaseSensitiveNames => "case-sensitive-names",
        /// Symbolic links.
        SymbolicLinks => "symbolic-links",
        /// Hard links.
        HardLinks => "hard-links",
        /// Sparse files.
        SparseFiles => "sparse-files",
        /// Transparent compression.
        Compression => "compression",
        /// Snapshots.
        Snapshots => "snapshots",
        /// Copy-on-write clones of file data.
        CopyOnWrite => "copy-on-write",
    }
}

string_enum! {
    /// How well a filesystem represents a feature.
    pub enum Support {
        /// Represented.
        Yes => "yes",
        /// Represented with limits: an emulation, a mount option, or only
        /// some variants of the format.
        Partial => "partial",
        /// Not represented.
        No => "no",
        /// Not established. Treated as possibly present in a source and
        /// possibly absent in a destination.
        Unverified => "unverified",
    }
}

const FEATURE_COUNT: usize = Feature::all().len();

const Y: Support = Support::Yes;
const P: Support = Support::Partial;
const N: Support = Support::No;
const U: Support = Support::Unverified;

/// The registry. Columns are in [`Feature`] order: permissions, ACLs,
/// extended attributes, alternate data streams, security descriptors,
/// resource forks, case-sensitive names, symbolic links, hard links, sparse
/// files, compression, snapshots, copy-on-write.
const REGISTRY: &[(Filesystem, [Support; FEATURE_COUNT])] = &[
    (Filesystem::Ext4, [Y, Y, Y, N, N, N, Y, Y, Y, Y, N, N, N]),
    (Filesystem::Btrfs, [Y, Y, Y, N, N, N, Y, Y, Y, Y, Y, Y, Y]),
    // Copy-on-write (reflink) depends on how the filesystem was created.
    (Filesystem::Xfs, [Y, Y, Y, N, N, N, Y, Y, Y, Y, N, N, P]),
    // Through ntfs3: ownership is fixed by mount options, POSIX ACLs need a
    // mount option UOS does not set, lookups are case-sensitive on Linux but
    // not in Windows, and symbolic links are reparse points.
    (Filesystem::Ntfs, [P, P, P, Y, Y, N, P, P, Y, Y, Y, N, N]),
    (Filesystem::Exfat, [N, N, N, N, N, N, N, N, N, N, N, N, N]),
    (Filesystem::Vfat, [N, N, N, N, N, N, N, N, N, N, N, N, N]),
    // Through the Linux hfsplus driver. HFS+ is case-insensitive unless
    // created as HFSX.
    (Filesystem::HfsPlus, [Y, U, P, N, N, U, P, Y, U, N, U, N, N]),
    // Through apfs-fuse, read-only, so only ever a copy source.
    (Filesystem::Apfs, [Y, U, U, N, N, U, P, Y, U, U, P, U, U]),
    // Permissions and symbolic links need Rock Ridge; case depends on the
    // extensions in use.
    (Filesystem::Iso9660, [P, N, N, N, N, N, P, P, N, N, N, N, N]),
];

/// How well a filesystem represents a feature, or `None` when the registry
/// has no entry for the filesystem.
pub fn support(filesystem: Filesystem, feature: Feature) -> Option<Support> {
    let position = Feature::all()
        .iter()
        .position(|candidate| *candidate == feature)?;
    REGISTRY
        .iter()
        .find(|(entry, _)| *entry == filesystem)
        .map(|(_, row)| row[position])
}

string_enum! {
    /// What happens to a feature in a copy.
    pub enum LossKind {
        /// The destination cannot represent it.
        Lost => "lost",
        /// The destination represents it only in part.
        Degraded => "degraded",
        /// It could not be established whether it survives.
        Unverified => "unverified",
    }
}

/// One feature that a copy would not preserve in full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Loss {
    /// The feature.
    pub feature: Feature,
    /// What happens to it.
    pub kind: LossKind,
    /// How well the source represents it.
    pub source: Support,
    /// How well the destination represents it.
    pub destination: Support,
}

string_enum! {
    /// Whether a copy between two filesystems could be assessed.
    pub enum CapabilityAssessment {
        /// Both filesystems are in the registry and the destination can be
        /// written.
        Complete => "complete",
        /// UOS never writes to the destination filesystem.
        DestinationNotWritable => "destination-not-writable",
        /// The registry has no entry for one of the filesystems.
        NoData => "no-data",
    }
}

/// Lists what a copy from one filesystem to another would lose.
///
/// Returns an error instead of an empty list when the question cannot be
/// answered: a caller must not read "nothing is lost" into missing data.
pub fn copy_losses(
    source: Filesystem,
    destination: Filesystem,
) -> Result<Vec<Loss>, CapabilityAssessment> {
    let known = |filesystem: Filesystem| REGISTRY.iter().any(|(entry, _)| *entry == filesystem);
    if !known(source) || !known(destination) {
        return Err(CapabilityAssessment::NoData);
    }
    if !policy_for(destination).writable {
        return Err(CapabilityAssessment::DestinationNotWritable);
    }
    if source == destination {
        return Ok(Vec::new());
    }
    let mut losses = Vec::new();
    for &feature in Feature::all() {
        let (Some(from), Some(to)) = (support(source, feature), support(destination, feature))
        else {
            return Err(CapabilityAssessment::NoData);
        };
        let kind = match (from, to) {
            (Support::No, _) | (_, Support::Yes) => None,
            (Support::Unverified, _) | (_, Support::Unverified) => Some(LossKind::Unverified),
            (_, Support::No) => Some(LossKind::Lost),
            (Support::Yes, Support::Partial) => Some(LossKind::Degraded),
            // Two partial implementations need not cover the same ground.
            (Support::Partial, Support::Partial) => Some(LossKind::Unverified),
        };
        if let Some(kind) = kind {
            losses.push(Loss {
                feature,
                kind,
                source: from,
                destination: to,
            });
        }
    }
    Ok(losses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{DefaultAccess, POLICY_TABLE};

    fn kind(losses: &[Loss], feature: Feature) -> Option<LossKind> {
        losses
            .iter()
            .find(|loss| loss.feature == feature)
            .map(|loss| loss.kind)
    }

    #[test]
    fn every_mountable_filesystem_has_a_registry_row() {
        for row in POLICY_TABLE {
            let mountable = matches!(
                row.default_access,
                DefaultAccess::ReadWrite | DefaultAccess::ReadOnly
            );
            let registered = REGISTRY
                .iter()
                .filter(|(entry, _)| *entry == row.filesystem)
                .count();
            assert_eq!(registered, usize::from(mountable), "{}", row.filesystem);
        }
    }

    #[test]
    fn support_is_defined_for_every_feature_of_a_registered_filesystem() {
        for &feature in Feature::all() {
            assert!(support(Filesystem::Ext4, feature).is_some());
            assert!(support(Filesystem::Refs, feature).is_none());
        }
        assert_eq!(
            support(Filesystem::Ntfs, Feature::AlternateDataStreams),
            Some(Support::Yes)
        );
        assert_eq!(
            support(Filesystem::Btrfs, Feature::Snapshots),
            Some(Support::Yes)
        );
        assert_eq!(
            support(Filesystem::Exfat, Feature::PosixPermissions),
            Some(Support::No)
        );
    }

    #[test]
    fn copying_within_one_filesystem_loses_nothing() {
        for (filesystem, _) in REGISTRY {
            if policy_for(*filesystem).writable {
                assert_eq!(copy_losses(*filesystem, *filesystem), Ok(Vec::new()));
            }
        }
    }

    #[test]
    fn ext4_to_exfat_loses_all_posix_metadata() {
        let losses = copy_losses(Filesystem::Ext4, Filesystem::Exfat).unwrap();
        for feature in [
            Feature::PosixPermissions,
            Feature::Acls,
            Feature::ExtendedAttributes,
            Feature::CaseSensitiveNames,
            Feature::SymbolicLinks,
            Feature::HardLinks,
            Feature::SparseFiles,
        ] {
            assert_eq!(kind(&losses, feature), Some(LossKind::Lost), "{feature}");
        }
        assert_eq!(kind(&losses, Feature::AlternateDataStreams), None);
        assert_eq!(kind(&losses, Feature::Snapshots), None);
    }

    #[test]
    fn ntfs_to_ext4_loses_windows_metadata() {
        let losses = copy_losses(Filesystem::Ntfs, Filesystem::Ext4).unwrap();
        assert_eq!(
            kind(&losses, Feature::AlternateDataStreams),
            Some(LossKind::Lost)
        );
        assert_eq!(
            kind(&losses, Feature::SecurityDescriptors),
            Some(LossKind::Lost)
        );
        assert_eq!(kind(&losses, Feature::Compression), Some(LossKind::Lost));
        assert_eq!(kind(&losses, Feature::PosixPermissions), None);
        assert_eq!(kind(&losses, Feature::HardLinks), None);
    }

    #[test]
    fn ext4_to_ntfs_degrades_what_ntfs3_only_emulates() {
        let losses = copy_losses(Filesystem::Ext4, Filesystem::Ntfs).unwrap();
        assert_eq!(
            kind(&losses, Feature::PosixPermissions),
            Some(LossKind::Degraded)
        );
        assert_eq!(
            kind(&losses, Feature::CaseSensitiveNames),
            Some(LossKind::Degraded)
        );
        assert_eq!(kind(&losses, Feature::HardLinks), None);
    }

    #[test]
    fn btrfs_to_ext4_loses_snapshots_compression_and_clones() {
        let losses = copy_losses(Filesystem::Btrfs, Filesystem::Ext4).unwrap();
        let features: Vec<Feature> = losses.iter().map(|loss| loss.feature).collect();
        assert_eq!(
            features,
            [
                Feature::Compression,
                Feature::Snapshots,
                Feature::CopyOnWrite
            ]
        );
        assert!(losses.iter().all(|loss| loss.kind == LossKind::Lost));
    }

    #[test]
    fn unverified_entries_are_reported_not_hidden() {
        let losses = copy_losses(Filesystem::Apfs, Filesystem::Exfat).unwrap();
        assert_eq!(
            kind(&losses, Feature::ResourceForks),
            Some(LossKind::Unverified)
        );
        assert_eq!(
            kind(&losses, Feature::PosixPermissions),
            Some(LossKind::Lost)
        );
        let losses = copy_losses(Filesystem::Ext4, Filesystem::HfsPlus).unwrap();
        assert_eq!(kind(&losses, Feature::Acls), Some(LossKind::Unverified));
        assert_eq!(kind(&losses, Feature::SparseFiles), Some(LossKind::Lost));
    }

    #[test]
    fn destinations_that_are_never_written_are_not_assessed() {
        for destination in [Filesystem::Apfs, Filesystem::Iso9660] {
            assert_eq!(
                copy_losses(Filesystem::Ext4, destination),
                Err(CapabilityAssessment::DestinationNotWritable)
            );
        }
    }

    #[test]
    fn filesystems_without_data_are_not_assessed() {
        for other in [
            Filesystem::Refs,
            Filesystem::Zfs,
            Filesystem::Luks,
            Filesystem::Unknown,
        ] {
            assert_eq!(
                copy_losses(other, Filesystem::Ext4),
                Err(CapabilityAssessment::NoData)
            );
            assert_eq!(
                copy_losses(Filesystem::Ext4, other),
                Err(CapabilityAssessment::NoData)
            );
        }
    }

    #[test]
    fn a_feature_absent_from_the_source_is_never_a_loss() {
        for (source, _) in REGISTRY {
            for (destination, _) in REGISTRY {
                let Ok(losses) = copy_losses(*source, *destination) else {
                    continue;
                };
                for loss in losses {
                    assert_ne!(loss.source, Support::No);
                    assert_ne!(loss.destination, Support::Yes);
                    assert_eq!(support(*source, loss.feature), Some(loss.source));
                    assert_eq!(support(*destination, loss.feature), Some(loss.destination));
                }
            }
        }
    }
}
