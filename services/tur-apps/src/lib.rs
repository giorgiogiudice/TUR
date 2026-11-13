// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-apps`: the backend that installs applications for TUR's application
//! centre (ADR-0011, ADR-0019; docs/architecture/app-install-backend.md).
//!
//! An installation starts from a catalogue identifier. The catalogue's
//! router ([`tur_catalogue::route`]) says how the application would run on
//! this computer; [`plan::make`] turns that answer into a plan that says,
//! before anything is done, what is downloaded from where, what is
//! installed where, what the application may reach and how it is removed.
//! [`ops`] carries a plan out by one of two routes:
//!
//! * **Flatpak**: per-user, from a remote the user has switched on, through
//!   the `flatpak` command line ([`flatpak`]);
//! * **Windows**: the publisher's installer, downloaded only when the
//!   catalogue pins its address, size and SHA-256 for freely redistributable
//!   software, or a file the user supplies; checked before anything in it
//!   runs ([`download`], [`digest`]); and handed to `tur-runtime app
//!   install` ([`runtime`]).
//!
//! Every other route is refused with a stable code and a plain sentence.
//! Progress and results are events ([`events`]); what was installed is
//! recorded ([`records`]) and listed together with what Flatpak and
//! `tur-runtime` hold; removal keeps the user's data unless told otherwise.

#![forbid(unsafe_code)]

pub mod cancel;
pub mod cli;
pub mod codes;
pub mod digest;
pub mod download;
pub mod events;
pub mod exec;
pub mod flatpak;
pub mod ops;
pub mod paths;
pub mod plan;
pub mod records;
pub mod runtime;
pub mod tree;

pub use codes::{Code, Failure};
