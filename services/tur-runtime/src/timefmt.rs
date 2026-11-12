// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Timestamps for prefix metadata, backup names and log names.
//!
//! A timestamp is whole seconds since the Unix epoch, written in UTC as
//! RFC 3339 (`2026-11-12T10:15:00Z`). The conversion is done here so that the
//! crate needs no date library.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const SECONDS_PER_DAY: u64 = 86_400;
/// 9999-12-31T23:59:59Z, the last second a four-digit year can express.
const LAST_SECOND: u64 = 253_402_300_799;

/// A point in time in whole seconds, UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    /// The time `seconds` after the Unix epoch. Values past the year 9999 are
    /// clamped to its last second.
    pub const fn from_unix(seconds: u64) -> Self {
        if seconds > LAST_SECOND {
            Timestamp(LAST_SECOND)
        } else {
            Timestamp(seconds)
        }
    }

    /// The current time. A clock set before 1970 reads as the epoch.
    pub fn now() -> Self {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        Timestamp::from_unix(seconds)
    }

    /// Seconds since the Unix epoch.
    pub const fn unix(self) -> u64 {
        self.0
    }

    /// Year, month, day, hour, minute and second in UTC.
    fn fields(self) -> [u64; 6] {
        let days = self.0 / SECONDS_PER_DAY;
        let rest = self.0 % SECONDS_PER_DAY;
        // Days to a civil date, after Howard Hinnant's `civil_from_days`.
        let shifted = days + 719_468;
        let era = shifted / 146_097;
        let day_of_era = shifted % 146_097;
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
        [year, month, day, rest / 3_600, (rest / 60) % 60, rest % 60]
    }

    /// The time as RFC 3339 in UTC, for example `2026-11-12T10:15:00Z`.
    pub fn rfc3339(self) -> String {
        let [year, month, day, hour, minute, second] = self.fields();
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    }

    /// The time without punctuation, safe in a file name: `20261112T101500Z`.
    pub fn compact(self) -> String {
        let [year, month, day, hour, minute, second] = self.fields();
        format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
    }

    /// Parses exactly the form [`Timestamp::rfc3339`] writes. Offsets other
    /// than `Z`, fractions of a second and years before 1970 are refused.
    pub fn parse_rfc3339(text: &str) -> Option<Timestamp> {
        let bytes = text.as_bytes();
        if bytes.len() != 20 {
            return None;
        }
        let separators = [
            (4, b'-'),
            (7, b'-'),
            (10, b'T'),
            (13, b':'),
            (16, b':'),
            (19, b'Z'),
        ];
        if separators
            .iter()
            .any(|&(at, byte)| bytes.get(at) != Some(&byte))
        {
            return None;
        }
        let number = |from: usize, to: usize| -> Option<u64> {
            let digits = text.get(from..to)?;
            if digits.bytes().all(|byte| byte.is_ascii_digit()) {
                digits.parse().ok()
            } else {
                None
            }
        };
        let year = number(0, 4)?;
        let month = number(5, 7)?;
        let day = number(8, 10)?;
        let hour = number(11, 13)?;
        let minute = number(14, 16)?;
        let second = number(17, 19)?;
        if year < 1970 || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        if day < 1 || day > days_in_month(year, month) {
            return None;
        }
        // A civil date to days, after Howard Hinnant's `days_from_civil`.
        let shifted_year = if month <= 2 { year - 1 } else { year };
        let era = shifted_year / 400;
        let year_of_era = shifted_year % 400;
        let month_index = if month > 2 { month - 3 } else { month + 9 };
        let day_of_year = (153 * month_index + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        let days = (era * 146_097 + day_of_era).checked_sub(719_468)?;
        Some(Timestamp(
            days * SECONDS_PER_DAY + hour * 3_600 + minute * 60 + second,
        ))
    }
}

fn days_in_month(year: u64, month: u64) -> u64 {
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 31,
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.rfc3339())
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.rfc3339())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Timestamp::parse_rfc3339(&text).ok_or_else(|| {
            serde::de::Error::custom("a timestamp must have the form 2026-11-12T10:15:00Z")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: [(u64, &str); 7] = [
        (0, "1970-01-01T00:00:00Z"),
        (946_684_799, "1999-12-31T23:59:59Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (1_709_208_000, "2024-02-29T12:00:00Z"),
        (1_794_478_500, "2026-11-12T10:15:00Z"),
        (4_107_542_400, "2100-03-01T00:00:00Z"),
        (253_402_300_799, "9999-12-31T23:59:59Z"),
    ];

    #[test]
    fn known_instants_format_and_parse() {
        for (seconds, text) in KNOWN {
            let timestamp = Timestamp::from_unix(seconds);
            assert_eq!(timestamp.rfc3339(), text);
            assert_eq!(timestamp.to_string(), text);
            assert_eq!(Timestamp::parse_rfc3339(text), Some(timestamp));
            assert_eq!(timestamp.unix(), seconds);
        }
        assert_eq!(
            Timestamp::from_unix(1_794_478_500).compact(),
            "20261112T101500Z"
        );
    }

    #[test]
    fn every_day_boundary_round_trips_for_two_centuries() {
        let mut seconds = 0;
        while seconds < 6_400_000_000 {
            for offset in [0, 1, SECONDS_PER_DAY - 1] {
                let timestamp = Timestamp::from_unix(seconds + offset);
                assert_eq!(
                    Timestamp::parse_rfc3339(&timestamp.rfc3339()),
                    Some(timestamp)
                );
            }
            seconds += SECONDS_PER_DAY * 13;
        }
    }

    #[test]
    fn values_past_the_last_year_are_clamped() {
        assert_eq!(
            Timestamp::from_unix(u64::MAX).rfc3339(),
            "9999-12-31T23:59:59Z"
        );
    }

    #[test]
    fn malformed_text_is_refused() {
        for text in [
            "",
            "2026-11-12",
            "2026-11-12T10:15:00",
            "2026-11-12T10:15:00+00:00",
            "2026-11-12 10:15:00Z",
            "2026-13-12T10:15:00Z",
            "2026-00-12T10:15:00Z",
            "2026-02-30T10:15:00Z",
            "2025-02-29T10:15:00Z",
            "2026-11-31T10:15:00Z",
            "2026-11-12T24:00:00Z",
            "2026-11-12T10:60:00Z",
            "2026-11-12T10:15:60Z",
            "1969-12-31T23:59:59Z",
            "2026-11-12T10:15:0xZ",
            "+026-11-12T10:15:00Z",
            "２０２６-11-12T10:15:00Z",
        ] {
            assert_eq!(Timestamp::parse_rfc3339(text), None, "{text}");
        }
    }

    #[test]
    fn serialises_as_text_and_refuses_bad_text() {
        let timestamp = Timestamp::from_unix(1_794_478_500);
        let json = serde_json::to_string(&timestamp).unwrap();
        assert_eq!(json, "\"2026-11-12T10:15:00Z\"");
        assert_eq!(serde_json::from_str::<Timestamp>(&json).unwrap(), timestamp);
        assert!(serde_json::from_str::<Timestamp>("\"yesterday\"").is_err());
        assert!(serde_json::from_str::<Timestamp>("1794478500").is_err());
    }

    #[test]
    fn now_is_after_this_code_was_written() {
        assert!(Timestamp::now().unix() > 1_700_000_000);
    }
}
