// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Self-healing: notice a fault, explain it, repair it where that is safe,
//! check that the repair held, and offer a problem report when the cause is
//! in TUR (docs/architecture/self-healing.md, ADR-0027).
//!
//! * [`catalogue`] is the fault catalogue: data, compiled in, validated.
//! * [`machine`] is the only door to the computer.
//! * [`checks`] and [`remedies`] are the fixed kinds of check and remedy.
//! * [`state`] is what is kept between runs, and the lock.
//! * [`engine`] is the loop: scan, apply, undo, the socket's requests, the
//!   status P@x reads and the notices a person is shown.
//!
//! None of it depends on P@x. TUR's healing service runs [`engine::scan`]
//! on a timer; P@x can only read the state and ask the service, through its
//! socket, for what the service would do itself.

pub mod catalogue;
pub mod checks;
pub mod engine;
pub mod machine;
pub mod remedies;
pub mod state;

#[cfg(test)]
mod tests;

pub use catalogue::{Catalogue, Check, Fault, Remedy, RemedyKind, ReportPolicy};
pub use engine::{
    ApplyRequest, Fixes, Notice, Outcome, Paths, Running, ScanLine, StatusView, apply,
    handle_request, notices, scan, status_view, undo,
};
pub use machine::{FakeMachine, Machine, RealMachine};
pub use state::{Actor, EventKind, FaultState, HealState, Lock};
