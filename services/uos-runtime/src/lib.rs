// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The UOS Universal Runtime Manager library.
//!
//! Identifying a program:
//!
//! * [`inspect()`] identifies a file or macOS application bundle from its
//!   content, never from its name (specification §2.3).
//! * [`recommend()`] turns an [`Inspection`] and the [`HostFacts`] into a
//!   [`Recommendation`]: native Linux, Wine, Darling, the Android runtime, an
//!   offer of a full Windows virtual machine, or unsupported (§7.4).
//! * [`profile`] defines the versioned application profile (§7.3), validates
//!   it and drafts one from an inspection.
//!
//! Nothing in those modules executes, loads or maps the inspected file.
//! Inputs are treated as untrusted downloads: every offset is checked against
//! the file size, every size is capped (see [`limits`]) and arithmetic on
//! fields read from the file is checked.
//!
//! Running a Windows program:
//!
//! * [`prefix`] keeps one prefix per application and plans its operations:
//!   create, verify, repair, reset, back up, restore, remove, migrate.
//!   [`metadata`] is the record kept beside each prefix.
//! * [`launch`] turns a profile, that record and the facts in
//!   [`launch_host`] into a launch plan: the command, its environment and
//!   working directory, the synchronisation decision and the steps.
//! * [`sandbox`] wraps every runtime command in bubblewrap; [`grant`] decides
//!   which host directories may be shown to an application.
//! * [`plan`] is what all of these produce, and [`exec`] carries a plan out.
//! * [`desktop`] writes the text of a desktop entry.
//!
//! Every operation is a plan first. A plan can be printed, compared and
//! tested without starting anything; only [`exec::SystemExecutor`] touches
//! the system.
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
pub mod desktop;
pub mod elf;
pub mod error;
pub mod exec;
pub mod grant;
pub mod host;
pub mod inspect;
pub mod launch;
pub mod launch_host;
pub mod limits;
pub mod macho;
pub mod metadata;
pub mod ole;
pub mod paths;
pub mod pe;
pub mod plan;
pub mod prefix;
pub mod profile;
pub mod recommend;
pub mod report;
pub mod runtime_cmd;
pub mod sandbox;
pub mod script;
pub mod timefmt;

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
pub use runtime_cmd::PlanError;
