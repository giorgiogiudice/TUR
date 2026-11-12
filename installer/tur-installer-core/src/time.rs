// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Timestamps and the clock abstraction.
//!
//! Time reaches the installer through the [`Clock`] trait so that tests are
//! deterministic. A [`Timestamp`] is a count of milliseconds since the Unix
//! epoch in UTC and is written to files as an RFC 3339 string such as
//! `2026-11-12T06:17:00.000Z`.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MILLIS_PER_SECOND: u64 = 1_000;
const MILLIS_PER_DAY: u64 = 86_400_000;
const RFC3339_LENGTH: usize = 24;
const SEPARATORS: [(usize, u8); 7] = [
    (4, b'-'),
    (7, b'-'),
    (10, b'T'),
    (13, b':'),
    (16, b':'),
    (19, b'.'),
    (23, b'Z'),
];

/// A point in time: milliseconds since 1970-01-01T00:00:00Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    /// Builds a timestamp from milliseconds since the Unix epoch.
    pub const fn from_unix_millis(millis: u64) -> Self {
        Timestamp(millis)
    }

    /// Milliseconds since the Unix epoch.
    pub const fn unix_millis(self) -> u64 {
        self.0
    }

    /// Milliseconds elapsed since `earlier`, or zero if `earlier` is later.
    pub const fn millis_since(self, earlier: Timestamp) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    /// Formats the timestamp as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub fn to_rfc3339(self) -> String {
        let (year, month, day) = civil_from_days(self.0 / MILLIS_PER_DAY);
        let in_day = self.0 % MILLIS_PER_DAY;
        let millis = in_day % MILLIS_PER_SECOND;
        let seconds = in_day / MILLIS_PER_SECOND;
        let hour = seconds / 3_600;
        let minute = seconds / 60 % 60;
        let second = seconds % 60;
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
    }

    /// Parses the exact form [`Timestamp::to_rfc3339`] produces.
    ///
    /// Only UTC (`Z`), millisecond precision and years 1970 to 9999 are accepted.
    pub fn parse_rfc3339(text: &str) -> Result<Timestamp, TimestampParseError> {
        let invalid = || TimestampParseError {
            text: text.to_owned(),
        };
        let bytes = text.as_bytes();
        if bytes.len() != RFC3339_LENGTH {
            return Err(invalid());
        }
        if SEPARATORS
            .iter()
            .any(|&(index, separator)| bytes[index] != separator)
        {
            return Err(invalid());
        }
        let field = |start: usize, end: usize| digits(&bytes[start..end]).ok_or_else(invalid);
        let year = field(0, 4)?;
        let month = field(5, 7)?;
        let day = field(8, 10)?;
        let hour = field(11, 13)?;
        let minute = field(14, 16)?;
        let second = field(17, 19)?;
        let millis = field(20, 23)?;
        if year < 1970
            || !(1..=12).contains(&month)
            || !(1..=days_in_month(year, month)).contains(&day)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return Err(invalid());
        }
        let days = days_from_civil(year, month, day);
        let in_day = (hour * 3_600 + minute * 60 + second) * MILLIS_PER_SECOND + millis;
        Ok(Timestamp(days * MILLIS_PER_DAY + in_day))
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_rfc3339())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Timestamp::parse_rfc3339(&text).map_err(serde::de::Error::custom)
    }
}

/// A string that is not a timestamp in the accepted form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampParseError {
    text: String,
}

impl fmt::Display for TimestampParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\"{}\" is not a UTC timestamp of the form YYYY-MM-DDTHH:MM:SS.mmmZ",
            self.text
        )
    }
}

impl std::error::Error for TimestampParseError {}

/// A source of the current time.
pub trait Clock {
    /// The current time.
    fn now(&self) -> Timestamp;
}

/// The system's wall clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        Timestamp(u64::try_from(millis).unwrap_or(u64::MAX))
    }
}

fn digits(bytes: &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u64::from(byte - b'0');
    }
    Some(value)
}

fn is_leap_year(year: u64) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u64, month: u64) -> u64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap_year(year) => 29,
        _ => 28,
    }
}

/// Converts days since 1970-01-01 to a (year, month, day) civil date.
///
/// This is the well-known era-based algorithm for the proleptic Gregorian
/// calendar, restricted to dates on or after the epoch.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// Converts a civil date on or after 1970-01-01 to days since the epoch.
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        assert_eq!(
            Timestamp::from_unix_millis(0).to_rfc3339(),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            Timestamp::from_unix_millis(951_782_400_000).to_rfc3339(),
            "2000-02-29T00:00:00.000Z"
        );
        assert_eq!(
            Timestamp::from_unix_millis(1_794_464_220_123).to_rfc3339(),
            "2026-11-12T06:17:00.123Z"
        );
        assert_eq!(
            Timestamp::from_unix_millis(1_709_251_199_999).to_rfc3339(),
            "2024-02-29T23:59:59.999Z"
        );
    }

    #[test]
    fn round_trips_across_many_days() {
        // Every 13 hours and 7 milliseconds across more than a century.
        let step = 13 * 3_600_000 + 7;
        let mut millis = 0u64;
        while millis < 4_200_000_000_000 {
            let stamp = Timestamp::from_unix_millis(millis);
            let text = stamp.to_rfc3339();
            assert_eq!(Timestamp::parse_rfc3339(&text), Ok(stamp), "{text}");
            millis += step;
        }
    }

    #[test]
    fn rejects_malformed_text() {
        for text in [
            "",
            "2026-11-12",
            "2026-11-12T06:17:00Z",
            "2026-11-12T06:17:00.000+00:00",
            "2026-13-12T06:17:00.000Z",
            "2026-02-29T06:17:00.000Z",
            "2026-11-31T06:17:00.000Z",
            "2026-11-12T24:00:00.000Z",
            "2026-11-12T06:60:00.000Z",
            "2026-11-12T06:17:60.000Z",
            "1969-12-31T23:59:59.999Z",
            "2026-11-12 06:17:00.000Z",
            "2026-11-1ZT06:17:00.000Z",
            "２０２６-11-12T06:17:00.0Z",
        ] {
            assert!(Timestamp::parse_rfc3339(text).is_err(), "{text}");
        }
    }

    #[test]
    fn serialises_as_a_string() {
        let stamp = Timestamp::from_unix_millis(1_794_464_220_123);
        let json = serde_json::to_string(&stamp).unwrap();
        assert_eq!(json, "\"2026-11-12T06:17:00.123Z\"");
        assert_eq!(serde_json::from_str::<Timestamp>(&json).unwrap(), stamp);
        assert!(serde_json::from_str::<Timestamp>("\"yesterday\"").is_err());
        assert!(serde_json::from_str::<Timestamp>("17").is_err());
    }

    #[test]
    fn elapsed_time_saturates() {
        let early = Timestamp::from_unix_millis(1_000);
        let late = Timestamp::from_unix_millis(1_250);
        assert_eq!(late.millis_since(early), 250);
        assert_eq!(early.millis_since(late), 0);
    }

    #[test]
    fn system_clock_is_after_the_epoch() {
        assert!(SystemClock.now().unix_millis() > 0);
    }
}
