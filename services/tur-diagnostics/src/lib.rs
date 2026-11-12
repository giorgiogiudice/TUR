// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-diag`: local, redacted diagnostic bundles for TUR.
//!
//! This library holds all of the logic behind the `tur-diag` command
//! (specification §6.4, §6.5 and §29):
//!
//! * [`host`] is the only door to the machine: run a command or read a file,
//!   both with limits. Tests replace it with a table.
//! * [`collectors`] is the table of what is gathered, and the guarantee that
//!   nothing under a home directory is ever read.
//! * [`redact`] replaces personal and machine identifiers with stable
//!   pseudonyms before anything enters a bundle.
//! * [`manifest`] defines `manifest.json`.
//! * [`bundle`] builds, writes and verifies the archive.
//! * [`identity`] reads what the machine is running, for `tur-diag status`.
//! * [`error`] is the registry of stable `TUR-DIAG-*` error codes.
//!
//! Nothing here uploads anything. A bundle is a local file, created only when
//! a person asks for one.

#![forbid(unsafe_code)]

pub mod bundle;
pub mod collectors;
pub mod error;
pub mod host;
pub mod identity;
pub mod manifest;
pub mod redact;

pub use bundle::{
    BuiltBundle, CollectRequest, Verified, build_bundle, verify_archive, verify_file, write_bundle,
};
pub use collectors::{COLLECTORS, Collector, DEFAULT_MAX_BYTES};
pub use error::{DiagError, ErrorKind};
pub use host::{FakeHost, Host, SystemHost};
pub use identity::{Status, gather_known_identifiers, gather_status, render_status};
pub use manifest::Manifest;
pub use redact::{KnownIdentifiers, Profile, RedactionReport, Redactor};
