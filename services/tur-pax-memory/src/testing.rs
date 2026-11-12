// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Helpers for this crate's own unit tests. Compiled only for tests.
//!
//! This is the one file under `src/` that implements
//! [`crate::UserConfirmation`], and it is not part of the library that ships:
//! `lib.rs` includes it under `#[cfg(test)]`. `tests/authority.rs` checks
//! both facts.

use crate::confirm::UserConfirmation;
use crate::time::Timestamp;

/// A stand-in for the user in unit tests.
pub(crate) struct TestUser {
    /// Whether the user agrees to what is put in front of them.
    pub(crate) agrees: bool,
    /// When they answer.
    pub(crate) at: Timestamp,
}

impl TestUser {
    /// A user who agrees, at the given time.
    pub(crate) fn agreeing(at: u64) -> TestUser {
        TestUser {
            agrees: true,
            at: Timestamp(at),
        }
    }

    /// A user who does not agree.
    pub(crate) fn refusing(at: u64) -> TestUser {
        TestUser {
            agrees: false,
            at: Timestamp(at),
        }
    }
}

impl<T: ?Sized> UserConfirmation<T> for TestUser {
    fn covers(&self, _subject: &T) -> bool {
        self.agrees
    }

    fn confirmed_at(&self) -> Timestamp {
        self.at
    }
}
