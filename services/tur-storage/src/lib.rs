// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Storage policy core for TUR.
//!
//! This crate decides how each volume may be mounted and how one mount is
//! presented to every application runtime. It is pure policy: nothing in it
//! mounts, unmounts, formats or writes to a device. Facts about volumes come
//! in through the [`Probe`] trait; decisions and plans go out as plain data.
//!
//! * [`filesystem`]: the filesystem and container signatures TUR recognises.
//! * [`policy`]: the filesystem policy table (specification §11.3).
//! * [`probe`] and [`volume`]: the volume model, built from `lsblk` JSON and
//!   `blkid`-style properties.
//! * [`decision`]: the mount decision, a pure function of volume facts,
//!   requested access and policy overrides.
//! * [`namespace`]: volume identifiers, mount paths and runtime visibility
//!   mappings (§11.4).
//! * [`capabilities`]: the metadata capability registry (§11.5).
//! * [`codes`]: the `TUR-STORAGE-*` error-code registry (§29.3).
//! * [`cli`]: the `tur-storage` command line.
//!
//! Everything here is tested against hand-written fixtures, not against real
//! filesystems or hardware.

#![forbid(unsafe_code)]

/// Defines a field-less enum with a stable text form.
///
/// The generated type has `all()`, `as_str()` and `from_text()`, implements
/// `Display`, and serialises as its text form.
macro_rules! string_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $text:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant
            ),+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const fn all() -> &'static [$name] {
                &[$($name::$variant),+]
            }

            /// The stable text form used in JSON output and on the command line.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }

            /// Parses the stable text form. Returns `None` for anything else.
            pub fn from_text(text: &str) -> Option<$name> {
                $name::all().iter().copied().find(|value| value.as_str() == text)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    };
}

pub mod capabilities;
pub mod cli;
pub mod codes;
pub mod decision;
pub mod filesystem;
pub mod namespace;
pub mod policy;
pub mod probe;
pub mod volume;

pub use capabilities::{CapabilityAssessment, Feature, Loss, LossKind, Support, copy_losses};
pub use codes::{CodeEntry, CodeOrigin, Severity, StorageCode};
pub use decision::{Access, Decision, MountRequest, Outcome, TechnicalDetail, decide};
pub use filesystem::Filesystem;
pub use namespace::{
    MountOperation, MountPlan, NamespaceContext, PlannedVolume, Runtime, UserName, View, ViewKind,
    canonical_path, plan, sanitise_label,
};
pub use policy::{
    DefaultAccess, EXCLUDED_IMPLEMENTATIONS, FsPolicy, ImplementationKind, POLICY_TABLE,
    PolicyOverrides, UnsafeNtfsPolicy, policy_for,
};
pub use probe::{LsblkDevice, Probe, ProbeError, SnapshotProbe};
pub use volume::{Encryption, HfsJournal, IdSource, Inventory, NtfsState, Volume, VolumeId};

/// Version of the JSON documents this crate emits
/// (`schemas/storage-volume.schema.json`).
pub const SCHEMA_VERSION: u32 = 1;
