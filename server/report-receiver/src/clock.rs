// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Where the receiver's time comes from. Retention and retries are tested
//! with a [`ManualClock`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A source of the current time, in seconds since the Unix epoch.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> u64;
}

impl std::fmt::Debug for dyn Clock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Clock({})", self.now())
    }
}

/// The system's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }
}

/// A clock that stands still until it is moved. For tests, and for
/// `tur-report-receiver sweep --now`.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: AtomicU64,
}

impl ManualClock {
    /// A clock that reads `now`.
    pub fn new(now: u64) -> Self {
        ManualClock {
            now: AtomicU64::new(now),
        }
    }

    /// Sets the time.
    pub fn set(&self, now: u64) {
        self.now.store(now, Ordering::SeqCst);
    }

    /// Moves the time forward.
    pub fn advance(&self, seconds: u64) {
        self.now.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(100);
        assert_eq!(clock.now(), 100);
        clock.advance(50);
        assert_eq!(clock.now(), 150);
        clock.set(7);
        assert_eq!(clock.now(), 7);
        assert!(SystemClock.now() > 1_700_000_000);
    }
}
