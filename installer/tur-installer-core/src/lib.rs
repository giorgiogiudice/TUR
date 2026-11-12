// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Non-destructive core of the TUR installer.
//!
//! This crate models what the installer needs to know and decide before it is
//! allowed to change a disk, and how it records what it did:
//!
//! * [`inventory`]: block devices and partitions, built from data supplied
//!   through a [`inventory::Probe`], never by inspecting the machine directly.
//! * [`identity`]: the stable identity of a disk, target selection and
//!   revalidation. A device path such as `/dev/sda` is never an identity.
//! * [`preflight`]: pure checks over a [`preflight::HostFacts`] value.
//! * [`plan`]: the partition plan for the "erase target disk" mode, its
//!   consequences and the confirmation text.
//! * [`phase`]: the sixteen-phase state machine.
//! * [`session`]: the durable session directory and the resume rules.
//! * [`command`]: the record of one executed command, with secrets redacted.
//! * [`codes`]: the registry of `TUR-INSTALL-*` error codes.
//! * [`cli`]: the dry-run command line behind the `tur-installer` binary.
//!
//! Nothing in this crate writes to a block device. No partitioning,
//! formatting, encryption or bootloader tool is run or named in its code; the
//! test `tests/no_destructive_tools.rs` enforces that.

#![forbid(unsafe_code)]

pub mod cli;
pub mod codes;
pub mod command;
pub mod gpt;
pub mod identity;
pub mod inventory;
pub mod phase;
pub mod plan;
pub mod preflight;
pub mod session;
pub mod system_probe;
pub mod time;
pub mod units;

#[cfg(test)]
pub(crate) mod testing;
