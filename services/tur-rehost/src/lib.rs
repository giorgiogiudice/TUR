// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! TUR re-hosting: the Windows or macOS edition of an application, run on
//! the Linux build of the runtime it is built on, from the person's own
//! package, without Wine and without a virtual machine.
//!
//! The design is `docs/architecture/rehost.md`; the measurements it rests
//! on are `docs/reports/exp-rehost-1.md`. In order:
//!
//! 1. [`unpack`] opens the package in a sandbox, never running it;
//! 2. [`detect`] says which runtime it carries, at which version, and its
//!    platform-specific native code ([`finding`]);
//! 3. [`install::prepare`] plans what will be fetched, from TUR's pinned
//!    [`index`], and refuses before fetching anything when it cannot work;
//! 4. [`install::install`] fetches ([`fetch`]), lays the payload under the
//!    runtime, swaps native modules ([`natives`], [`build`]) and records it
//!    ([`record`]);
//! 5. [`apps`] lists, launches (in [`sandbox`]) and removes.
//!
//! Only Electron is re-hosted in this slice; the other families are
//! detected and refused with `TUR-REHOST-DETECT-005`.

pub mod apps;
pub mod asar;
pub mod build;
pub mod codes;
pub mod detect;
pub mod fetch;
pub mod finding;
pub mod index;
pub mod install;
pub mod natives;
pub mod paths;
pub mod record;
pub mod runner;
pub mod sandbox;
pub mod scan;
pub mod sniff;
pub mod unpack;

pub use codes::{Code, Failure};
pub use finding::{Family, Finding, Platform, Verdict};
pub use paths::{AppId, Roots};
