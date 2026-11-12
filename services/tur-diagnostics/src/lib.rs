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
//! * [`report`] prepares problem reports (ADR-0024), keeps the local list of
//!   sent ones, and sends one when `tur-diag report send` asks it to.
//!
//! A bundle is a local file, created only when a person asks for one. The one
//! thing that sends anything is [`report::send_prepared`], which only the
//! `report send` command calls, in the foreground, after the person has
//! confirmed, and only to the receiver configured for TUR. Nothing is sent
//! in the background or retried on its own.

#![forbid(unsafe_code)]

pub mod bundle;
pub mod collectors;
pub mod error;
pub mod host;
pub mod identity;
pub mod manifest;
pub mod redact;
pub mod report;

pub use bundle::{
    BuiltBundle, CollectRequest, ExtraDocument, Verified, build_bundle, build_bundle_with,
    verify_archive, verify_file, verify_members, write_bundle,
};
pub use collectors::{COLLECTORS, Collector, DEFAULT_MAX_BYTES};
pub use error::{DiagError, ErrorKind};
pub use host::{FakeHost, Host, SystemHost};
pub use identity::{Status, gather_known_identifiers, gather_status, render_status};
pub use manifest::Manifest;
pub use redact::{KnownIdentifiers, Profile, RedactionReport, Redactor};
