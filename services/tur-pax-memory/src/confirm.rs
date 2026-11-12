// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Evidence that the user, and not the assistant, decided something.
//!
//! Some operations in this crate are the user's to make: confirming a memory
//! entry taken from content, correcting an entry, turning memory on or off,
//! and creating, changing or removing a schedule and its standing approvals.
//! Each of them takes a [`UserConfirmation`] for the exact thing being decided.
//!
//! **This library never implements [`UserConfirmation`].** It defines the
//! trait and has no type that satisfies it, so no code path inside the library
//! can produce a confirmation: one has to be handed in from outside. The
//! component expected to implement it is the one that shows the decision to
//! the person and takes their answer through a path the assistant cannot
//! operate: the settings interface on the trusted confirmation path (addendum
//! A, sections 6 and 17 item 5). In this change the only implementation is in
//! the command-line program's `main.rs`, where the person typing the command
//! is the user. A test in `tests/authority.rs` reads the library sources and
//! fails if one appears.
//!
//! The trait cannot stop a caller from writing a dishonest implementation. It
//! makes the dishonesty a visible, reviewable act in the caller's code rather
//! than something this library can do by accident.

use crate::time::Timestamp;

/// A confirmation given by the user for one subject of type `T`.
///
/// `T` is what was shown to the user: a proposed schedule, a memory entry as
/// it will be stored, a change to a schedule, the memory switch.
pub trait UserConfirmation<T: ?Sized> {
    /// Whether the confirmation was given for exactly this subject.
    ///
    /// An implementation compares `subject` with what it showed the user and
    /// returns `false` on any difference. A confirmation given for one
    /// proposal must not cover another.
    fn covers(&self, subject: &T) -> bool;

    /// When the user confirmed.
    fn confirmed_at(&self) -> Timestamp;
}
