// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! GPT partition type GUIDs the installer recognises or plans, in lower case.
//!
//! The Linux types are those of the Discoverable Partitions Specification.

/// EFI System Partition.
pub const ESP: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
/// Extended boot loader partition (`/boot`).
pub const XBOOTLDR: &str = "bc13c2ff-59e6-4262-a352-b275fd6f7172";
/// Linux root partition for x86-64.
pub const LINUX_ROOT_X86_64: &str = "4f68bce3-e8cd-4db1-96e7-fbcaf984b709";
/// Linux home partition.
pub const LINUX_HOME: &str = "933ac7e1-2eb4-4f13-b844-0e14e2aef915";
/// Generic Linux filesystem data.
pub const LINUX_FILESYSTEM: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";
/// Linux LVM physical volume.
pub const LINUX_LVM: &str = "e6d6d379-f507-44c2-a23c-238f2a3df928";
/// Linux swap.
pub const LINUX_SWAP: &str = "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f";
/// Microsoft reserved partition.
pub const MICROSOFT_RESERVED: &str = "e3c9e316-0b5c-4db8-817d-f92df00215ae";
/// Microsoft basic data.
pub const MICROSOFT_BASIC_DATA: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";
/// Windows recovery environment.
pub const WINDOWS_RECOVERY: &str = "de94bba4-06d1-4d40-a16a-bfd50179d6ac";
/// Apple APFS container.
pub const APPLE_APFS: &str = "7c3457ef-0000-11aa-aa11-00306543ecac";
/// Apple HFS+.
pub const APPLE_HFS: &str = "48465300-0000-11aa-aa11-00306543ecac";
