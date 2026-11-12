// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Calendar dates as the catalogue writes them: `YYYY-MM-DD`.
//!
//! The catalogue needs three things from a date: that it is a real day, that
//! two dates can be ordered, and that a record is not dated in the future.
//! That is little enough to do without a date library.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A day in the proleptic Gregorian calendar.
///
/// Ordering is chronological.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Date {
    year: i32,
    month: u8,
    day: u8,
}

/// The earliest year a catalogue date may have.
pub const MIN_YEAR: i32 = 2000;

impl Date {
    /// A date from its parts, or `None` when that day does not exist.
    pub fn new(year: i32, month: u8, day: u8) -> Option<Date> {
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        if day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Date { year, month, day })
    }

    /// Parses `YYYY-MM-DD`: exactly ten characters, ASCII digits and hyphens.
    pub fn parse(text: &str) -> Option<Date> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let year = digits(&bytes[0..4])?;
        let month = digits(&bytes[5..7])?;
        let day = digits(&bytes[8..10])?;
        Date::new(
            i32::try_from(year).ok()?,
            u8::try_from(month).ok()?,
            u8::try_from(day).ok()?,
        )
    }

    /// Today's date in UTC, from the system clock.
    ///
    /// A clock set before 1970 gives 1 January 1970.
    pub fn today_utc() -> Date {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let days = i64::try_from(seconds / 86_400).unwrap_or(0);
        Date::from_days_since_epoch(days)
    }

    /// The date `days` days after 1 January 1970.
    ///
    /// Values outside the years 1 to 9999 are clamped to that range.
    pub fn from_days_since_epoch(days: i64) -> Date {
        // Civil-from-days, after Howard Hinnant's public-domain algorithm.
        let days = days.clamp(-719_162, 2_932_896);
        let shifted = days + 719_468;
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
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
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Date {
            year: i32::try_from(year).unwrap_or(1970),
            month: u8::try_from(month).unwrap_or(1),
            day: u8::try_from(day).unwrap_or(1),
        }
    }

    /// The number of days from 1 January 1970 to this date.
    pub fn days_since_epoch(self) -> i64 {
        let month = i64::from(self.month);
        let year = i64::from(self.year) - i64::from(month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year.rem_euclid(400);
        let month_index = if month > 2 { month - 3 } else { month + 9 };
        let day_of_year = (153 * month_index + 2) / 5 + i64::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The year.
    pub fn year(self) -> i32 {
        self.year
    }

    /// The month, 1 to 12.
    pub fn month(self) -> u8 {
        self.month
    }

    /// The day of the month, 1 to 31.
    pub fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn digits(bytes: &[u8]) -> Option<u32> {
    let mut value = 0u32;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(byte - b'0');
    }
    Some(value)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_days_and_prints_them_back() {
        for text in ["2026-11-12", "2024-02-29", "2000-01-01", "1999-12-31"] {
            let date = Date::parse(text).unwrap();
            assert_eq!(date.to_string(), text);
        }
        let date = Date::parse("2026-11-12").unwrap();
        assert_eq!((date.year(), date.month(), date.day()), (2026, 11, 12));
    }

    #[test]
    fn rejects_days_that_do_not_exist_and_other_shapes() {
        for text in [
            "2026-02-29",
            "2026-02-30",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "2026-11-00",
            "1900-02-29",
            "2026-1-12",
            "26-11-12",
            "2026/11/12",
            "2026-11-12T00:00",
            "12-11-2026",
            "",
            "2026-11-1２",
            "0000-01-01",
        ] {
            assert_eq!(Date::parse(text), None, "{text}");
        }
        assert!(Date::parse("2000-02-29").is_some());
    }

    #[test]
    fn orders_chronologically() {
        let earlier = Date::parse("2025-12-31").unwrap();
        let later = Date::parse("2026-01-01").unwrap();
        assert!(earlier < later);
        assert!(Date::parse("2026-02-01").unwrap() > Date::parse("2026-01-31").unwrap());
    }

    #[test]
    fn converts_to_and_from_days_since_1970() {
        assert_eq!(Date::from_days_since_epoch(0).to_string(), "1970-01-01");
        assert_eq!(Date::from_days_since_epoch(19_358).to_string(), "2023-01-01");
        assert_eq!(Date::from_days_since_epoch(20_769).to_string(), "2026-11-12");
        assert_eq!(Date::from_days_since_epoch(-1).to_string(), "1969-12-31");
        assert_eq!(Date::parse("2026-11-12").unwrap().days_since_epoch(), 20_769);
        // Every day over more than a century round-trips, and consecutive
        // days are consecutive dates.
        let mut previous = Date::from_days_since_epoch(-1);
        for days in 0..40_000 {
            let date = Date::from_days_since_epoch(days);
            assert_eq!(date.days_since_epoch(), days);
            assert!(date > previous);
            assert_eq!(Date::parse(&date.to_string()), Some(date));
            previous = date;
        }
    }

    #[test]
    fn today_is_a_plausible_date() {
        let today = Date::today_utc();
        assert!(today >= Date::parse("2024-01-01").unwrap());
        assert_eq!(Date::from_days_since_epoch(today.days_since_epoch()), today);
    }
}
