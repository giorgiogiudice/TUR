// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Time as this crate sees it: a number of seconds, and a clock behind a trait.
//!
//! No rule in this crate reads a clock. Operations take the time as an
//! argument; the command-line front end is the one place that asks a
//! [`Clock`], so tests fix the time and are deterministic.

use std::cell::Cell;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, NaiveDate, SecondsFormat, Timelike, Utc};
use chrono_tz::Tz;
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

/// Whether `name` is a zone of the IANA time-zone database compiled into this
/// crate, for example `Europe/Rome`.
pub fn is_known_zone(name: &str) -> bool {
    name.parse::<Tz>().is_ok()
}

// A calendar date written `YYYY-MM-DD`, and nothing else.
fn parse_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
    if !shaped {
        return None;
    }
    let year = text[0..4].parse::<i32>().ok()?;
    let month = text[5..7].parse::<u32>().ok()?;
    let day = text[8..10].parse::<u32>().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)
}

/// The weekday a calendar date (`YYYY-MM-DD`) falls on, and its day of the
/// month. `None` if the text is not a date.
pub fn weekday_and_day_of(date: &str) -> Option<(crate::trigger::Weekday, u8)> {
    let date = parse_date(date)?;
    let weekday = crate::trigger::Weekday::ALL
        .get(usize::try_from(date.weekday().num_days_from_monday()).ok()?)
        .copied()?;
    Some((weekday, u8::try_from(date.day()).ok()?))
}

impl Timestamp {
    /// What a wall clock in the named zone shows at this time: the date as
    /// `YYYY-MM-DD` and the time of day as `HH:MM`. `None` if the zone is not
    /// known or the time is out of range.
    pub fn wall_clock_in(self, zone: &str) -> Option<(String, String)> {
        let zone: Tz = zone.parse().ok()?;
        let local = self.to_utc()?.with_timezone(&zone).naive_local();
        Some((
            format!(
                "{:04}-{:02}-{:02}",
                local.year(),
                local.month(),
                local.day()
            ),
            format!("{:02}:{:02}", local.hour(), local.minute()),
        ))
    }

    /// The one instant a wall-clock date (`YYYY-MM-DD`) and time of day in a
    /// named zone stand for, by the daylight-saving rule of
    /// [`crate::trigger`]: the instant if the time exists once on that date,
    /// the first of the two if it occurs twice, and the end of the gap if the
    /// clocks go forward over it. `None` if the date, the time or the zone is
    /// not valid.
    pub fn from_wall_clock(date: &str, hour: u8, minute: u8, zone: &str) -> Option<Timestamp> {
        let zone: Tz = zone.parse().ok()?;
        let local = parse_date(date)?.and_hms_opt(u32::from(hour), u32::from(minute), 0)?;
        crate::trigger::resolve_local(zone, local)
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
    fn a_wall_clock_time_in_a_zone_is_one_instant_and_back() {
        // 2026-11-12T10:00:00Z is 11:00 in Rome and 05:00 in New York.
        let instant = Timestamp(1_794_477_600);
        assert_eq!(
            instant.wall_clock_in("Europe/Rome"),
            Some(("2026-11-12".to_owned(), "11:00".to_owned()))
        );
        assert_eq!(
            instant.wall_clock_in("America/New_York"),
            Some(("2026-11-12".to_owned(), "05:00".to_owned()))
        );
        assert_eq!(
            Timestamp::from_wall_clock("2026-11-12", 11, 0, "Europe/Rome"),
            Some(instant)
        );
        assert_eq!(instant.wall_clock_in("Mars/Olympus"), None);
        assert_eq!(Timestamp(u64::MAX).wall_clock_in("UTC"), None);
        // 12 November 2026 is a Thursday.
        assert_eq!(
            weekday_and_day_of("2026-11-12"),
            Some((crate::trigger::Weekday::Thursday, 12))
        );
        assert_eq!(
            weekday_and_day_of("2024-02-29"),
            Some((crate::trigger::Weekday::Thursday, 29))
        );
        assert_eq!(weekday_and_day_of("2026-02-29"), None);
        assert!(is_known_zone("Europe/Rome"));
        assert!(is_known_zone("UTC"));
        assert!(!is_known_zone("Europe/Atlantis"));
        assert!(!is_known_zone(""));

        // Dates that are not dates, and times that are not times.
        for date in [
            "2026-02-30",
            "2026-13-01",
            "2026-1-01",
            "26-11-12",
            "2026/11/12",
            "2026-11-12 ",
            "",
        ] {
            assert_eq!(
                Timestamp::from_wall_clock(date, 9, 0, "UTC"),
                None,
                "{date:?}"
            );
        }
        assert_eq!(Timestamp::from_wall_clock("2026-11-12", 24, 0, "UTC"), None);
        assert_eq!(Timestamp::from_wall_clock("2026-11-12", 9, 60, "UTC"), None);
        assert_eq!(
            Timestamp::from_wall_clock("2026-11-12", 9, 0, "Nowhere"),
            None
        );
    }

    #[test]
    fn a_wall_clock_time_follows_the_daylight_saving_rule_of_the_triggers() {
        // Rome, 29 March 2026: 02:00 jumps to 03:00. 02:30 does not exist and
        // stands for the end of the gap, 03:00 local, which is 01:00 UTC.
        let gap = Timestamp::from_wall_clock("2026-03-29", 2, 30, "Europe/Rome").expect("resolved");
        assert_eq!(gap.to_rfc3339(), "2026-03-29T01:00:00Z");
        // Rome, 25 October 2026: 03:00 falls back to 02:00. 02:30 occurs
        // twice and stands for the first, 00:30 UTC.
        let twice =
            Timestamp::from_wall_clock("2026-10-25", 2, 30, "Europe/Rome").expect("resolved");
        assert_eq!(twice.to_rfc3339(), "2026-10-25T00:30:00Z");
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
