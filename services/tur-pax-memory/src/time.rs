// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Time as this crate sees it: a number of seconds, and a clock behind a trait.
//!
//! No rule in this crate reads a clock. Operations take the time as an
//! argument; the command-line front end is the one place that asks a
//! [`Clock`], so tests fix the time and are deterministic.

use std::cell::Cell;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

/// A point in time, in whole seconds since the Unix epoch (UTC).
///
/// The same representation as the policy engine's `Timestamp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// The latest time this crate accepts: the last second of the year 9999.
    ///
    /// A stored time beyond this is treated as damage, and no schedule fires
    /// after it. The bound keeps every calculation far from overflow.
    pub const MAX: Timestamp = Timestamp(253_402_300_799);

    /// Seconds since the Unix epoch.
    pub const fn seconds(self) -> u64 {
        self.0
    }

    /// Whether the time is within the supported range.
    pub const fn is_supported(self) -> bool {
        self.0 <= Timestamp::MAX.0
    }

    /// This time plus a number of seconds, if the result is in range.
    pub fn checked_add(self, seconds: u64) -> Option<Timestamp> {
        self.0
            .checked_add(seconds)
            .map(Timestamp)
            .filter(|time| time.is_supported())
    }

    /// The time as a UTC date and time, if it is in range.
    pub(crate) fn to_utc(self) -> Option<DateTime<Utc>> {
        if !self.is_supported() {
            return None;
        }
        let seconds = i64::try_from(self.0).ok()?;
        DateTime::<Utc>::from_timestamp(seconds, 0)
    }

    /// A time from signed seconds, if it is not negative and is in range.
    pub(crate) fn from_signed(seconds: i64) -> Option<Timestamp> {
        u64::try_from(seconds)
            .ok()
            .map(Timestamp)
            .filter(|time| time.is_supported())
    }

    /// The time in RFC 3339 form, in UTC, for example `2026-11-12T10:00:00Z`.
    ///
    /// A time outside the supported range is shown as `@` and the number of
    /// seconds.
    pub fn to_rfc3339(self) -> String {
        match self.to_utc() {
            Some(utc) => utc.to_rfc3339_opts(SecondsFormat::Secs, true),
            None => format!("@{}", self.0),
        }
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

/// A source of the current time.
pub trait Clock {
    /// The current time.
    fn now(&self) -> Timestamp;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        Timestamp(seconds)
    }
}

/// A clock that shows the time it was last set to. For tests and scripts.
#[derive(Debug, Clone)]
pub struct ManualClock {
    now: Cell<u64>,
}

impl ManualClock {
    /// A clock showing `now`.
    pub fn new(now: Timestamp) -> Self {
        ManualClock {
            now: Cell::new(now.0),
        }
    }

    /// Set the clock.
    pub fn set(&self, now: Timestamp) {
        self.now.set(now.0);
    }

    /// Move the clock forward.
    pub fn advance(&self, seconds: u64) {
        self.now.set(self.now.get().saturating_add(seconds));
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.now.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_utc() {
        assert_eq!(Timestamp(0).to_rfc3339(), "1970-01-01T00:00:00Z");
        // 2026-11-12T10:00:00Z.
        assert_eq!(
            Timestamp(1_794_477_600).to_rfc3339(),
            "2026-11-12T10:00:00Z"
        );
        assert_eq!(Timestamp::MAX.to_rfc3339(), "9999-12-31T23:59:59Z");
        assert_eq!(Timestamp(u64::MAX).to_rfc3339(), format!("@{}", u64::MAX));
    }

    #[test]
    fn addition_stays_in_range() {
        assert_eq!(Timestamp(10).checked_add(5), Some(Timestamp(15)));
        assert_eq!(Timestamp::MAX.checked_add(1), None);
        assert_eq!(Timestamp(u64::MAX).checked_add(1), None);
        assert_eq!(Timestamp::from_signed(-1), None);
        assert_eq!(Timestamp::from_signed(7), Some(Timestamp(7)));
    }

    #[test]
    fn manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(Timestamp(100));
        assert_eq!(clock.now(), Timestamp(100));
        clock.advance(20);
        assert_eq!(clock.now(), Timestamp(120));
        clock.set(Timestamp(5));
        assert_eq!(clock.now(), Timestamp(5));
    }
}
