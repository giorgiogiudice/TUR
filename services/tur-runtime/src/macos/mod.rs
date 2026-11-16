// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The macOS route: macOS applications in Darling, one prefix per
//! application, inside the same sandbox as every other runtime
//! (docs/architecture/runtime-manager.md, "Running a macOS application").
//!
//! * [`package`] tells what a file is (an application bundle, a disk image,
//!   an installer package, a zip archive) and finds the bundles in an
//!   unpacked one.
//! * [`loadcmd`] reads a Mach-O image's load commands; [`analysis`] turns
//!   them, with the image's architectures, into findings: arm64-only,
//!   SwiftUI, Swift, encrypted, another Apple platform.
//! * [`darling`] is TUR's build of Darling: the manifest compiled into this
//!   program, where it is kept on this computer, and the plans that fetch,
//!   verify and remove it. It is fetched when a person first installs a
//!   macOS application, never with the system.
//! * [`plan`] makes the plans that install, launch and remove an
//!   application; [`record`] is what is kept about an installed one.
//! * [`route`] is the order the routes for a macOS package are tried in:
//!   a native Linux edition, then re-hosting a portable payload, then Darling
//!   for applications measured to work in it, otherwise a refusal.
//!
//! Darling itself is a separate program under its own licences
//! (GPL-3.0-or-later and others; `third-party/darling/README.md`). This
//! component only starts it, as it starts Wine.

use std::fmt;

use crate::codes::Code;

pub mod analysis;
pub mod darling;
pub mod loadcmd;
pub mod package;
pub mod plan;
pub mod record;
pub mod route;

/// Why a macOS operation could not be planned or carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacError {
    /// The stable code.
    pub code: Code,
    /// What happened, in plain language.
    pub message: String,
}

impl MacError {
    /// An error with `code` and `message`.
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        MacError {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for MacError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MacError {}
