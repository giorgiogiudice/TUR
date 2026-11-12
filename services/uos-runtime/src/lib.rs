// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The UOS Universal Runtime Manager library: inspection and recommendation.
//!
//! This first slice does three things and nothing else:
//!
//! * [`inspect()`] identifies a file or macOS application bundle from its
//!   content, never from its name (specification §2.3).
//! * [`recommend()`] turns an [`Inspection`] and the [`HostFacts`] into a
//!   [`Recommendation`]: native Linux, Wine, Darling, the Android runtime, an
//!   offer of a full Windows virtual machine, or unsupported (§7.4).
//! * [`profile`] defines the versioned application profile (§7.3), validates
//!   it and drafts one from an inspection.
//!
//! Nothing in this crate executes, loads or maps the inspected file. Inputs are
//! treated as untrusted downloads: every offset is checked against the file
//! size, every size is capped (see [`limits`]) and arithmetic on fields read
//! from the file is checked.
//!
//! ```
//! use uos_runtime::{Arch, Component, HostFacts, Runtime, inspect_bytes, recommend};
//!
//! // A shell script is identified by its first line, whatever it is called.
//! let inspection = inspect_bytes(b"#!/bin/sh\necho hello\n").unwrap();
//! let host = HostFacts::new(Arch::X86_64, &[Component::Wine]);
//! let recommendation = recommend(&inspection, &host);
//! assert_eq!(recommendation.runtime, Runtime::NativeLinux);
//! assert!(recommendation.is_ready());
//! ```

#![forbid(unsafe_code)]

pub mod android;
pub mod arch;
pub mod bundle;
pub mod codes;
pub mod elf;
pub mod error;
pub mod host;
pub mod inspect;
pub mod limits;
pub mod macho;
pub mod ole;
pub mod pe;
pub mod profile;
pub mod recommend;
pub mod report;
pub mod script;

mod plist;
mod reader;
mod zip;

#[cfg(test)]
mod testdata;

pub use arch::{Arch, Endianness};
pub use codes::Code;
pub use error::{FormatFamily, InspectError};
pub use host::{Component, HostFacts};
pub use inspect::{Detected, Inspection, inspect, inspect_bytes};
pub use recommend::{Reason, Recommendation, Runtime, recommend};
