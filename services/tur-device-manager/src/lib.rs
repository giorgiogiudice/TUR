// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The TUR device manager's core: a read-only hardware inventory.
//!
//! Linux owns the hardware (specification §3.1, §10.1). This crate only
//! describes it. It reads sysfs, procfs, the udev database, fwupd's device
//! list and, where permitted, the kernel log, and builds:
//!
//! * an [`Inventory`] of PCI and USB devices, network interfaces, sound cards,
//!   storage controllers, block devices, input devices, Bluetooth adapters,
//!   V4L2 devices, Thunderbolt devices, the platform and its firmware;
//! * [`Finding`]s for devices that have no driver, whose driver did not bind,
//!   or whose firmware could not be loaded;
//! * a [redacted view](Inventory::redacted) without serial numbers and
//!   addresses.
//!
//! It also defines, as types only, the [hand-off state machine](handoff) of
//! specification §10.4.
//!
//! **Read-only by design.** Everything is read through the [`Probe`] trait,
//! which has no operation that writes. Nothing in this crate changes device
//! ownership, binds or unbinds a driver, or writes to sysfs.
//!
//! **Untrusted input.** Much of what is read is supplied by devices. It is
//! bounded, cleaned and never trusted to be well formed; see [`text`]. In
//! code that is not a test, the crate denies `unwrap`, `expect`, `panic!` and
//! unchecked indexing.
//!
//! ```
//! use tur_device_manager::{collect, detect, probe::FakeProbe};
//!
//! // Tests use an in-memory tree; `FsProbe::host()` reads the real system.
//! let probe = FakeProbe::new()
//!     .file("/sys/devices/pci0000:00/0000:00:1f.6/vendor", "0x8086\n")
//!     .file("/sys/devices/pci0000:00/0000:00:1f.6/device", "0x15fc\n")
//!     .file("/sys/devices/pci0000:00/0000:00:1f.6/class", "0x020000\n")
//!     .link(
//!         "/sys/bus/pci/devices/0000:00:1f.6",
//!         "../../../devices/pci0000:00/0000:00:1f.6",
//!     );
//!
//! let inventory = collect(&probe);
//! assert_eq!(inventory.pci[0].address, "0000:00:1f.6");
//! assert_eq!(inventory.pci[0].driver, None);
//!
//! let findings = detect(&inventory);
//! assert_eq!(findings[0].code.as_str(), "TUR-DEVICE-DRIVER-001");
//! ```

#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::unimplemented,
        clippy::indexing_slicing
    )
)]

pub mod fwupd;
pub mod handoff;
pub mod ids;
pub mod kernel_log;
pub mod modalias;
pub mod model;
pub mod probe;
pub mod problems;
pub mod redact;
pub mod render;
pub mod scan;
pub mod text;
pub mod udev;

pub use model::{Inventory, SCHEMA_VERSION, Sensitive};
pub use probe::{FakeProbe, FsProbe, Probe, ProbeCommand, ProbeError};
pub use problems::{Finding, ProblemKind, ProblemReport, Severity, detect};
pub use scan::collect;
