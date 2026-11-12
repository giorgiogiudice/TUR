// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! When a reminder or a scheduled task fires, and what is due now.
//!
//! Everything here is a pure function of its arguments. Nothing reads a clock
//! or the host's time-zone settings: local-time triggers name their IANA zone,
//! and the zone rules are compiled in.
//!
//! # Triggers
//!
//! | Trigger | Fires |
//! |---|---|
//! | `once` | at one instant |
//! | `every` | at `anchor + k * interval` for k = 0, 1, 2, ...; the interval is a whole number of minutes, hours or days of elapsed time |
//! | `daily` | every day at a wall-clock time in a named zone |
//! | `weekly` | on the given weekdays at a wall-clock time in a named zone |
//! | `monthly` | on a day of the month at a wall-clock time in a named zone |
//! | `event` | when the named system event happens; it has no time |
//!
//! # No drift
//!
//! Occurrences are computed from the trigger alone: from the anchor for
//! `every`, from the calendar for the others. The time a run actually started
//! or finished never enters the calculation, so a late run does not move the
//! next one.
//!
//! # Daylight saving
//!
//! A wall-clock time on a given date is turned into exactly one instant:
//!
//! - **It exists once:** that instant.
//! - **It does not exist** (clocks go forward over it): the first instant
//!   after the gap, which is the moment the clocks change. A 02:30 trigger on
//!   a night when 02:00 jumps to 03:00 fires at 03:00. It fires once, as soon
//!   as the wall clock has passed the requested time.
//! - **It occurs twice** (clocks go back over it): the first occurrence only.
//!   A 02:30 trigger on a night when 03:00 falls back to 02:00 fires at the
//!   earlier 02:30 and not again an hour later.
//!
//! So a wall-clock trigger fires exactly once on each date it applies to, and
//! `every` triggers, being elapsed time, are not affected at all.
//!
//! # Months without the day
//!
//! A monthly trigger on day 29, 30 or 31 states what happens in a shorter
//! month: `last_day` (the default) fires on the month's last day, `skip` does
//! not fire that month.
//!
//! # Missed runs
//!
//! See [`next_run`].

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::codes::{Code, Problem};
use crate::time::Timestamp;

/// The shortest time between two firings of one schedule, in seconds.
///
/// An `every` trigger with a shorter interval is refused, and an event
/// trigger does not fire again within this time of its last run.
pub const MIN_INTERVAL_SECONDS: u64 = 300;

/// The longest interval of an `every` trigger, in seconds: ten years.
pub const MAX_INTERVAL_SECONDS: u64 = 3660 * 86_400;

/// How late a run may start and still count as on time, in seconds.
///
/// A timer does not fire at the exact second. A run that starts within this
/// time of its scheduled instant is on time whatever the missed-run policy.
/// It is shorter than [`MIN_INTERVAL_SECONDS`], so a run can never be on time
/// for two occurrences.
pub const ON_TIME_GRACE_SECONDS: u64 = 120;

// How many days the calendar search looks through. The longest gap between
// two occurrences of a calendar trigger is under two months (day 31 with
// `skip`, over February).
const SEARCH_DAYS: u32 = 400;

/// A wall-clock time of day, to the minute. Written `HH:MM` in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LocalTime {
    hour: u8,
    minute: u8,
}

impl LocalTime {
    /// A time of day, if the hour is below 24 and the minute below 60.
    pub fn new(hour: u8, minute: u8) -> Option<LocalTime> {
        (hour < 24 && minute < 60).then_some(LocalTime { hour, minute })
    }

    /// The hour, 0 to 23.
    pub fn hour(self) -> u8 {
        self.hour
    }

    /// The minute, 0 to 59.
    pub fn minute(self) -> u8 {
        self.minute
    }

    /// Parse `HH:MM`.
    pub fn parse(text: &str) -> Option<LocalTime> {
        let bytes = text.as_bytes();
        if bytes.len() != 5 || bytes[2] != b':' {
            return None;
        }
        let digit = |index: usize| -> Option<u8> {
            let byte = bytes[index];
            if byte.is_ascii_digit() {
                Some(byte - b'0')
            } else {
                None
            }
        };
        let hour = digit(0)? * 10 + digit(1)?;
        let minute = digit(3)? * 10 + digit(4)?;
        LocalTime::new(hour, minute)
    }
}

impl std::fmt::Display for LocalTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

impl TryFrom<String> for LocalTime {
    type Error = String;

    fn try_from(text: String) -> Result<LocalTime, String> {
        LocalTime::parse(&text)
            .ok_or_else(|| "a time of day is written HH:MM, from 00:00 to 23:59".to_owned())
    }
}

impl From<LocalTime> for String {
    fn from(time: LocalTime) -> String {
        time.to_string()
    }
}

/// A day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    /// Monday.
    Monday,
    /// Tuesday.
    Tuesday,
    /// Wednesday.
    Wednesday,
    /// Thursday.
    Thursday,
    /// Friday.
    Friday,
    /// Saturday.
    Saturday,
    /// Sunday.
    Sunday,
}

impl Weekday {
    /// Every weekday, from Monday.
    pub const ALL: [Weekday; 7] = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];

    /// The weekday as it appears in JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Weekday::Monday => "monday",
            Weekday::Tuesday => "tuesday",
            Weekday::Wednesday => "wednesday",
            Weekday::Thursday => "thursday",
            Weekday::Friday => "friday",
            Weekday::Saturday => "saturday",
            Weekday::Sunday => "sunday",
        }
    }

    /// 1 for Monday to 7 for Sunday.
    pub const fn number_from_monday(self) -> u32 {
        match self {
            Weekday::Monday => 1,
            Weekday::Tuesday => 2,
            Weekday::Wednesday => 3,
            Weekday::Thursday => 4,
            Weekday::Friday => 5,
            Weekday::Saturday => 6,
            Weekday::Sunday => 7,
        }
    }
}

/// The unit of an `every` trigger. All three are elapsed time: a day is
/// 86 400 seconds, not a calendar day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntervalUnit {
    /// 60 seconds.
    Minutes,
    /// 3 600 seconds.
    Hours,
    /// 86 400 seconds.
    Days,
}

impl IntervalUnit {
    /// The unit in seconds.
    pub const fn seconds(self) -> u64 {
        match self {
            IntervalUnit::Minutes => 60,
            IntervalUnit::Hours => 3_600,
            IntervalUnit::Days => 86_400,
        }
    }

    /// The unit as it appears in JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            IntervalUnit::Minutes => "minutes",
            IntervalUnit::Hours => "hours",
            IntervalUnit::Days => "days",
        }
    }
}

/// What a monthly trigger does in a month that lacks its day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortMonth {
    /// Fire on the last day of the month.
    #[default]
    LastDay,
    /// Do not fire that month.
    Skip,
}

/// A named system event a schedule can fire on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemEvent {
    /// An operating-system update is available.
    UpdateAvailable,
    /// A device was connected.
    DeviceConnected,
    /// A device was disconnected.
    DeviceDisconnected,
    /// The network came up.
    NetworkOnline,
    /// The user unlocked the session.
    SessionUnlocked,
    /// Free space on a volume fell below the system's threshold.
    StorageLow,
}

impl SystemEvent {
    /// Every event.
    pub const ALL: [SystemEvent; 6] = [
        SystemEvent::UpdateAvailable,
        SystemEvent::DeviceConnected,
        SystemEvent::DeviceDisconnected,
        SystemEvent::NetworkOnline,
        SystemEvent::SessionUnlocked,
        SystemEvent::StorageLow,
    ];

    /// The event as it appears in JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            SystemEvent::UpdateAvailable => "update_available",
            SystemEvent::DeviceConnected => "device_connected",
            SystemEvent::DeviceDisconnected => "device_disconnected",
            SystemEvent::NetworkOnline => "network_online",
            SystemEvent::SessionUnlocked => "session_unlocked",
            SystemEvent::StorageLow => "storage_low",
        }
    }
}

/// What happens to occurrences that passed while the machine was off or
/// asleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedRunPolicy {
    /// Run once on return, however many occurrences were missed.
    RunOnce,
    /// Do not run for missed occurrences; wait for the next one.
    Skip,
}

impl MissedRunPolicy {
    /// The policy as it appears in JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            MissedRunPolicy::RunOnce => "run_once",
            MissedRunPolicy::Skip => "skip",
        }
    }
}

/// When a schedule fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    /// Once, at an instant.
    Once {
        /// The instant.
        at: Timestamp,
    },
    /// Every `every` units of elapsed time, counted from `anchor`.
    Every {
        /// How many units.
        every: u32,
        /// The unit.
        unit: IntervalUnit,
        /// The first occurrence. Later ones are whole intervals after it.
        anchor: Timestamp,
    },
    /// Every day at a wall-clock time.
    Daily {
        /// The time of day.
        time: LocalTime,
        /// The IANA time zone the time is in, for example `Europe/Rome`.
        zone: String,
    },
    /// On given weekdays at a wall-clock time.
    Weekly {
        /// The weekdays: at least one, none twice.
        weekdays: Vec<Weekday>,
        /// The time of day.
        time: LocalTime,
        /// The IANA time zone the time is in.
        zone: String,
    },
    /// On a day of the month at a wall-clock time.
    Monthly {
        /// The day of the month, 1 to 31.
        day: u8,
        /// The time of day.
        time: LocalTime,
        /// The IANA time zone the time is in.
        zone: String,
        /// What to do in a month that lacks the day.
        #[serde(default)]
        if_day_missing: ShortMonth,
    },
    /// When a system event happens.
    Event {
        /// The event.
        event: SystemEvent,
    },
}

fn invalid(detail: impl Into<String>) -> Problem {
    Problem::new(Code::ScheduleInvalidTrigger, detail)
}

fn parse_zone(name: &str) -> Result<Tz, Problem> {
    name.parse::<Tz>()
        .map_err(|_| invalid("the time zone is not a name in the IANA time-zone database"))
}

// Which dates a calendar trigger applies to.
#[derive(Clone, Copy)]
enum DayRule<'a> {
    Daily,
    Weekly(&'a [Weekday]),
    Monthly { day: u8, if_day_missing: ShortMonth },
}

// A calendar trigger with its zone resolved.
#[derive(Clone, Copy)]
struct Calendar<'a> {
    zone: Tz,
    time: LocalTime,
    rule: DayRule<'a>,
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|first| first.pred_opt())
        .map_or(28, |last| last.day())
}

// The wall-clock reading in `zone` at an instant.
fn wall_clock(zone: Tz, instant: i64) -> Option<NaiveDateTime> {
    DateTime::<Utc>::from_timestamp(instant, 0).map(|utc| utc.with_timezone(&zone).naive_local())
}

// The first instant at which the wall clock in `zone` reads later than
// `local`, for a `local` that the wall clock never shows: the end of the gap.
fn first_instant_after_gap(zone: Tz, local: NaiveDateTime) -> Option<i64> {
    // Read the wall-clock value as if it were UTC. The true instant is within
    // two days of that for any zone offset.
    let centre = local.and_utc().timestamp();
    let mut before = centre - 2 * 86_400;
    let mut after = centre + 2 * 86_400;
    if wall_clock(zone, before)? >= local || wall_clock(zone, after)? <= local {
        return None;
    }
    // Invariant: the wall clock reads earlier than `local` at `before` and
    // later at `after`.
    while after - before > 1 {
        let middle = before + (after - before) / 2;
        if wall_clock(zone, middle)? > local {
            after = middle;
        } else {
            before = middle;
        }
    }
    Some(after)
}

// The one instant a wall-clock date and time in a zone stands for, by the
// daylight-saving rule of this module: the instant if it exists once, the
// first of the two if it occurs twice, the end of the gap if it does not
// exist.
fn resolve_local(zone: Tz, local: NaiveDateTime) -> Option<Timestamp> {
    // `earliest` is the only instant when there is one, and the first of the
    // two when the wall-clock time occurs twice.
    let instant = match zone.from_local_datetime(&local).earliest() {
        Some(first) => first.timestamp(),
        None => first_instant_after_gap(zone, local)?,
    };
    Timestamp::from_signed(instant)
}

impl Calendar<'_> {
    fn applies_on(&self, date: NaiveDate) -> bool {
        match self.rule {
            DayRule::Daily => true,
            DayRule::Weekly(weekdays) => {
                let today = date.weekday().number_from_monday();
                weekdays
                    .iter()
                    .any(|weekday| weekday.number_from_monday() == today)
            }
            DayRule::Monthly {
                day,
                if_day_missing,
            } => {
                let day = u32::from(day);
                let last = last_day_of_month(date.year(), date.month());
                if day <= last {
                    date.day() == day
                } else {
                    if_day_missing == ShortMonth::LastDay && date.day() == last
                }
            }
        }
    }

    // The instant of the occurrence on `date`, if the trigger applies then.
    fn occurrence_on(&self, date: NaiveDate) -> Option<Timestamp> {
        if !self.applies_on(date) {
            return None;
        }
        let local = date.and_hms_opt(u32::from(self.time.hour), u32::from(self.time.minute), 0)?;
        resolve_local(self.zone, local)
    }

    fn date_at(&self, instant: Timestamp) -> Option<NaiveDate> {
        let seconds = i64::try_from(instant.0).ok()?;
        wall_clock(self.zone, seconds).map(|local| local.date())
    }

    fn after(&self, reference: Timestamp) -> Option<Timestamp> {
        let mut date = self.date_at(reference)?.pred_opt()?;
        for _ in 0..SEARCH_DAYS {
            if let Some(instant) = self.occurrence_on(date)
                && instant > reference
            {
                return Some(instant);
            }
            date = date.succ_opt()?;
        }
        None
    }

    fn at_or_before(&self, reference: Timestamp) -> Option<Timestamp> {
        let mut date = self.date_at(reference)?.succ_opt()?;
        for _ in 0..SEARCH_DAYS {
            if let Some(instant) = self.occurrence_on(date)
                && instant <= reference
            {
                return Some(instant);
            }
            date = date.pred_opt()?;
        }
        None
    }
}

impl Trigger {
    /// The trigger's kind as it appears in JSON.
    pub const fn kind(&self) -> &'static str {
        match self {
            Trigger::Once { .. } => "once",
            Trigger::Every { .. } => "every",
            Trigger::Daily { .. } => "daily",
            Trigger::Weekly { .. } => "weekly",
            Trigger::Monthly { .. } => "monthly",
            Trigger::Event { .. } => "event",
        }
    }

    /// The interval of an `every` trigger in seconds, if it is one and the
    /// product does not overflow.
    fn interval_seconds(&self) -> Option<u64> {
        match self {
            Trigger::Every { every, unit, .. } => u64::from(*every).checked_mul(unit.seconds()),
            _ => None,
        }
    }

    // The calendar form of a wall-clock trigger.
    fn calendar(&self) -> Result<Option<Calendar<'_>>, Problem> {
        let (time, zone, rule) = match self {
            Trigger::Daily { time, zone } => (*time, zone, DayRule::Daily),
            Trigger::Weekly {
                weekdays,
                time,
                zone,
            } => (*time, zone, DayRule::Weekly(weekdays)),
            Trigger::Monthly {
                day,
                time,
                zone,
                if_day_missing,
            } => (
                *time,
                zone,
                DayRule::Monthly {
                    day: *day,
                    if_day_missing: *if_day_missing,
                },
            ),
            Trigger::Once { .. } | Trigger::Every { .. } | Trigger::Event { .. } => {
                return Ok(None);
            }
        };
        Ok(Some(Calendar {
            zone: parse_zone(zone)?,
            time,
            rule,
        }))
    }

    /// Check the trigger: times in range, a known zone, at least one weekday
    /// and none twice, a day from 1 to 31, and an interval within the limits.
    ///
    /// An interval shorter than [`MIN_INTERVAL_SECONDS`] is refused with
    /// `UOS-AI-SCHEDULE-004`; everything else with `UOS-AI-SCHEDULE-003`.
    pub fn validate(&self) -> Result<(), Problem> {
        match self {
            Trigger::Once { at } => {
                if !at.is_supported() {
                    return Err(invalid("the time is outside the supported range"));
                }
            }
            Trigger::Every { every, anchor, .. } => {
                if !anchor.is_supported() {
                    return Err(invalid("the anchor is outside the supported range"));
                }
                if *every == 0 {
                    return Err(invalid("the interval is zero"));
                }
                let seconds = self.interval_seconds().unwrap_or(u64::MAX);
                if seconds < MIN_INTERVAL_SECONDS {
                    return Err(Problem::new(
                        Code::ScheduleTooFrequent,
                        format!(
                            "the interval is {seconds} seconds; the shortest allowed is {MIN_INTERVAL_SECONDS}"
                        ),
                    ));
                }
                if seconds > MAX_INTERVAL_SECONDS {
                    return Err(invalid("the interval is longer than ten years"));
                }
            }
            Trigger::Daily { .. } => {}
            Trigger::Weekly { weekdays, .. } => {
                if weekdays.is_empty() {
                    return Err(invalid("a weekly trigger names no weekday"));
                }
                let distinct: std::collections::BTreeSet<Weekday> =
                    weekdays.iter().copied().collect();
                if distinct.len() != weekdays.len() {
                    return Err(invalid("a weekday is named twice"));
                }
            }
            Trigger::Monthly { day, .. } => {
                if !(1..=31).contains(day) {
                    return Err(invalid("the day of the month is not from 1 to 31"));
                }
            }
            Trigger::Event { .. } => {}
        }
        // Resolves the zone of a wall-clock trigger.
        self.calendar().map(|_| ())
    }

    /// The first occurrence strictly after `reference`, or `None` if there is
    /// none: a `once` trigger whose time has passed, an event trigger, or a
    /// time beyond the supported range.
    ///
    /// Always later than `reference`. Depends on nothing but the trigger and
    /// `reference`.
    pub fn occurrence_after(&self, reference: Timestamp) -> Result<Option<Timestamp>, Problem> {
        self.validate()?;
        let found = match self {
            Trigger::Once { at } => (*at > reference).then_some(*at),
            Trigger::Every { anchor, .. } => {
                let interval = self.interval_seconds().unwrap_or(u64::MAX);
                if reference < *anchor {
                    Some(*anchor)
                } else {
                    let elapsed = reference.0 - anchor.0;
                    (elapsed / interval)
                        .checked_add(1)
                        .and_then(|steps| steps.checked_mul(interval))
                        .and_then(|offset| anchor.checked_add(offset))
                }
            }
            Trigger::Event { .. } => None,
            Trigger::Daily { .. } | Trigger::Weekly { .. } | Trigger::Monthly { .. } => self
                .calendar()?
                .and_then(|calendar| calendar.after(reference)),
        };
        Ok(found.filter(|instant| instant.is_supported() && *instant > reference))
    }

    /// The last occurrence at or before `reference`, or `None` if there is
    /// none.
    pub fn occurrence_at_or_before(
        &self,
        reference: Timestamp,
    ) -> Result<Option<Timestamp>, Problem> {
        self.validate()?;
        let found = match self {
            Trigger::Once { at } => (*at <= reference).then_some(*at),
            Trigger::Every { anchor, .. } => {
                let interval = self.interval_seconds().unwrap_or(u64::MAX);
                if reference < *anchor {
                    None
                } else {
                    let elapsed = reference.0 - anchor.0;
                    anchor.checked_add((elapsed / interval) * interval)
                }
            }
            Trigger::Event { .. } => None,
            Trigger::Daily { .. } | Trigger::Weekly { .. } | Trigger::Monthly { .. } => self
                .calendar()?
                .and_then(|calendar| calendar.at_or_before(reference)),
        };
        Ok(found.filter(|instant| *instant <= reference))
    }

    /// The trigger in words, for a listing.
    pub fn describe(&self) -> String {
        match self {
            Trigger::Once { at } => format!("once at {at}"),
            Trigger::Every {
                every,
                unit,
                anchor,
            } => format!("every {every} {} from {anchor}", unit.as_str()),
            Trigger::Daily { time, zone } => format!("daily at {time} {zone}"),
            Trigger::Weekly {
                weekdays,
                time,
                zone,
            } => {
                let days: Vec<&str> = weekdays.iter().map(|weekday| weekday.as_str()).collect();
                format!("weekly on {} at {time} {zone}", days.join(", "))
            }
            Trigger::Monthly {
                day,
                time,
                zone,
                if_day_missing,
            } => {
                let rule = match if_day_missing {
                    ShortMonth::LastDay => "the last day in a shorter month",
                    ShortMonth::Skip => "skipping a shorter month",
                };
                format!("monthly on day {day} at {time} {zone}, {rule}")
            }
            Trigger::Event { event } => format!("on event {}", event.as_str()),
        }
    }
}

/// What a schedule is to do now, and when it fires next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum NextRun {
    /// A run is due now.
    Due {
        /// The occurrence the run is for: the latest one at or before now.
        scheduled_for: Timestamp,
        /// Whether the occurrence is older than the on-time grace, so the run
        /// makes up for time the machine was off.
        late: bool,
        /// The first occurrence after now, if there is one.
        next: Option<Timestamp>,
    },
    /// Nothing is due. The next firing is at `next`, which is after now.
    Wait {
        /// The next occurrence.
        next: Timestamp,
        /// Whether occurrences were missed and, by the schedule's policy,
        /// dropped.
        skipped_missed: bool,
    },
    /// The schedule fires on an event and has no time.
    OnEvent,
    /// The schedule will not fire again.
    Finished {
        /// Whether its last occurrence was missed and, by the schedule's
        /// policy, dropped.
        skipped_missed: bool,
    },
}

/// What a schedule is to do at `now`.
///
/// `since` is the time of the schedule's last run, or, if it has never run,
/// the time the user created or last changed it. Occurrences at or before
/// `since` are done with; the function looks at those after it.
///
/// - No occurrence after `since`: [`NextRun::Finished`].
/// - The first one is still in the future: [`NextRun::Wait`] for it.
/// - Otherwise at least one occurrence has passed. Take the latest one at or
///   before `now`.
///   - If it is at most [`ON_TIME_GRACE_SECONDS`] old, the run is on time:
///     [`NextRun::Due`]. Any older occurrences that passed are not run
///     separately, whatever the policy.
///   - If it is older, occurrences were missed while the machine was off.
///     With [`MissedRunPolicy::RunOnce`] the result is one [`NextRun::Due`]
///     with `late` set; with [`MissedRunPolicy::Skip`] it is
///     [`NextRun::Wait`] for the first occurrence after `now`.
///
/// After a run, the caller records `now` as the last run. However many
/// occurrences were missed, at most one run results, and the next occurrence
/// is still computed from the trigger, so nothing drifts.
///
/// Every time this function returns in `next` is strictly after `now`. If the
/// clock was set back so that `since` is after `now`, the result is a wait for
/// the first occurrence after `since`: nothing runs twice.
pub fn next_run(
    trigger: &Trigger,
    missed: MissedRunPolicy,
    since: Timestamp,
    now: Timestamp,
) -> Result<NextRun, Problem> {
    if matches!(trigger, Trigger::Event { .. }) {
        trigger.validate()?;
        return Ok(NextRun::OnEvent);
    }
    let Some(first) = trigger.occurrence_after(since)? else {
        return Ok(NextRun::Finished {
            skipped_missed: false,
        });
    };
    if first > now {
        return Ok(NextRun::Wait {
            next: first,
            skipped_missed: false,
        });
    }
    let latest = trigger
        .occurrence_at_or_before(now)?
        .map_or(first, |latest| latest.max(first));
    let next = trigger.occurrence_after(now)?;
    let on_time = now.0 - latest.0 <= ON_TIME_GRACE_SECONDS;
    if on_time || missed == MissedRunPolicy::RunOnce {
        return Ok(NextRun::Due {
            scheduled_for: latest,
            late: !on_time,
            next,
        });
    }
    Ok(match next {
        Some(next) => NextRun::Wait {
            next,
            skipped_missed: true,
        },
        None => NextRun::Finished {
            skipped_missed: true,
        },
    })
}

/// Whether an event trigger may fire at `now`, given its last run: not within
/// [`MIN_INTERVAL_SECONDS`] of it. This bounds how often a burst of events,
/// such as a device that keeps reconnecting, can start a task.
pub fn event_may_fire(last_run: Option<Timestamp>, now: Timestamp) -> bool {
    last_run.is_none_or(|last| now.0.saturating_sub(last.0) >= MIN_INTERVAL_SECONDS)
}

#[cfg(test)]
mod tests {
    use chrono::Timelike;

    use super::*;

    fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Timestamp {
        let instant = Utc
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .expect("a valid UTC time");
        Timestamp::from_signed(instant.timestamp()).expect("in range")
    }

    fn time(text: &str) -> LocalTime {
        LocalTime::parse(text).expect("a valid time of day")
    }

    fn daily(at: &str, zone: &str) -> Trigger {
        Trigger::Daily {
            time: time(at),
            zone: zone.to_owned(),
        }
    }

    fn after(trigger: &Trigger, reference: Timestamp) -> Timestamp {
        trigger
            .occurrence_after(reference)
            .expect("a valid trigger")
            .expect("an occurrence")
    }

    #[test]
    fn local_time_parses_strictly() {
        assert_eq!(time("07:30").to_string(), "07:30");
        assert_eq!(time("00:00").hour(), 0);
        assert_eq!(time("23:59").minute(), 59);
        for text in ["24:00", "7:30", "07:60", "0730", "07:30:00", " 07:30", "ab:cd", "", "07-30"] {
            assert_eq!(LocalTime::parse(text), None, "{text:?}");
        }
        assert!(serde_json::from_str::<LocalTime>("\"25:00\"").is_err());
        assert_eq!(
            serde_json::to_string(&time("07:05")).expect("serialises"),
            "\"07:05\""
        );
    }

    #[test]
    fn once_fires_once() {
        let trigger = Trigger::Once { at: Timestamp(1_000) };
        assert_eq!(trigger.occurrence_after(Timestamp(999)), Ok(Some(Timestamp(1_000))));
        assert_eq!(trigger.occurrence_after(Timestamp(1_000)), Ok(None));
        assert_eq!(trigger.occurrence_at_or_before(Timestamp(999)), Ok(None));
        assert_eq!(
            trigger.occurrence_at_or_before(Timestamp(1_000)),
            Ok(Some(Timestamp(1_000)))
        );
    }

    #[test]
    fn every_stays_on_its_grid() {
        let trigger = Trigger::Every {
            every: 15,
            unit: IntervalUnit::Minutes,
            anchor: Timestamp(10_000),
        };
        assert_eq!(after(&trigger, Timestamp(0)), Timestamp(10_000));
        assert_eq!(after(&trigger, Timestamp(9_999)), Timestamp(10_000));
        assert_eq!(after(&trigger, Timestamp(10_000)), Timestamp(10_900));
        assert_eq!(after(&trigger, Timestamp(10_001)), Timestamp(10_900));
        assert_eq!(after(&trigger, Timestamp(10_899)), Timestamp(10_900));
        assert_eq!(after(&trigger, Timestamp(10_900)), Timestamp(11_800));
        assert_eq!(trigger.occurrence_at_or_before(Timestamp(9_999)), Ok(None));
        assert_eq!(
            trigger.occurrence_at_or_before(Timestamp(10_899)),
            Ok(Some(Timestamp(10_000)))
        );
        assert_eq!(
            trigger.occurrence_at_or_before(Timestamp(10_900)),
            Ok(Some(Timestamp(10_900)))
        );
    }

    #[test]
    fn frequency_and_shape_limits() {
        let every = |every: u32, unit: IntervalUnit| Trigger::Every {
            every,
            unit,
            anchor: Timestamp(0),
        };
        assert_eq!(
            every(4, IntervalUnit::Minutes).validate().expect_err("too frequent").code,
            Code::ScheduleTooFrequent
        );
        assert_eq!(every(5, IntervalUnit::Minutes).validate(), Ok(()));
        assert_eq!(
            every(0, IntervalUnit::Days).validate().expect_err("zero").code,
            Code::ScheduleInvalidTrigger
        );
        assert_eq!(every(3660, IntervalUnit::Days).validate(), Ok(()));
        assert_eq!(
            every(3661, IntervalUnit::Days).validate().expect_err("too long").code,
            Code::ScheduleInvalidTrigger
        );
        assert_eq!(
            every(u32::MAX, IntervalUnit::Days).validate().expect_err("too long").code,
            Code::ScheduleInvalidTrigger
        );

        assert!(daily("07:30", "Europe/Rome").validate().is_ok());
        for zone in ["Europe/Atlantis", "", "CEST+2", "../../etc/localtime"] {
            assert_eq!(
                daily("07:30", zone).validate().expect_err("unknown zone").code,
                Code::ScheduleInvalidTrigger,
                "{zone:?}"
            );
        }
        let weekly = |weekdays: Vec<Weekday>| Trigger::Weekly {
            weekdays,
            time: time("07:30"),
            zone: "UTC".to_owned(),
        };
        assert!(weekly(Vec::new()).validate().is_err());
        assert!(weekly(vec![Weekday::Monday, Weekday::Monday]).validate().is_err());
        assert!(weekly(Weekday::ALL.to_vec()).validate().is_ok());
        let monthly = |day: u8| Trigger::Monthly {
            day,
            time: time("07:30"),
            zone: "UTC".to_owned(),
            if_day_missing: ShortMonth::LastDay,
        };
        assert!(monthly(0).validate().is_err());
        assert!(monthly(32).validate().is_err());
        assert!(monthly(31).validate().is_ok());
        assert!(Trigger::Once { at: Timestamp(u64::MAX) }.validate().is_err());
    }

    #[test]
    fn daily_keeps_its_wall_clock_time_across_daylight_saving() {
        // Rome is UTC+1 in winter and UTC+2 in summer.
        let trigger = daily("07:30", "Europe/Rome");
        assert_eq!(after(&trigger, utc(2026, 1, 10, 0, 0)), utc(2026, 1, 10, 6, 30));
        assert_eq!(after(&trigger, utc(2026, 1, 10, 6, 30)), utc(2026, 1, 11, 6, 30));
        // Summer time starts on 29 March 2026.
        assert_eq!(after(&trigger, utc(2026, 3, 28, 6, 30)), utc(2026, 3, 29, 5, 30));
        assert_eq!(after(&trigger, utc(2026, 3, 29, 5, 30)), utc(2026, 3, 30, 5, 30));
        // Summer time ends on 25 October 2026.
        assert_eq!(after(&trigger, utc(2026, 10, 24, 5, 30)), utc(2026, 10, 25, 6, 30));
    }

    #[test]
    fn a_time_that_does_not_exist_fires_when_the_gap_ends() {
        // Rome, 29 March 2026: 02:00 jumps to 03:00 at 01:00 UTC.
        let trigger = daily("02:30", "Europe/Rome");
        assert_eq!(after(&trigger, utc(2026, 3, 28, 12, 0)), utc(2026, 3, 29, 1, 0));
        // The day before and the day after are ordinary.
        assert_eq!(after(&trigger, utc(2026, 3, 27, 12, 0)), utc(2026, 3, 28, 1, 30));
        assert_eq!(after(&trigger, utc(2026, 3, 29, 1, 0)), utc(2026, 3, 30, 0, 30));
        // It fires once that day, not also at 03:30.
        assert_eq!(
            trigger.occurrence_at_or_before(utc(2026, 3, 29, 23, 0)),
            Ok(Some(utc(2026, 3, 29, 1, 0)))
        );

        // New York, 8 March 2026: 02:00 jumps to 03:00 at 07:00 UTC.
        let trigger = daily("02:30", "America/New_York");
        assert_eq!(after(&trigger, utc(2026, 3, 7, 12, 0)), utc(2026, 3, 8, 7, 0));

        // Lord Howe Island moves by half an hour: on 4 October 2026, 02:00
        // (UTC+10:30) jumps to 02:30 (UTC+11) at 15:30 UTC the day before.
        let trigger = daily("02:15", "Australia/Lord_Howe");
        assert_eq!(after(&trigger, utc(2026, 10, 3, 0, 0)), utc(2026, 10, 3, 15, 30));
    }

    #[test]
    fn a_time_that_occurs_twice_fires_the_first_time_only() {
        // Rome, 25 October 2026: 03:00 falls back to 02:00 at 01:00 UTC, so
        // 02:30 happens at 00:30 UTC and again at 01:30 UTC.
        let trigger = daily("02:30", "Europe/Rome");
        let first = utc(2026, 10, 25, 0, 30);
        assert_eq!(after(&trigger, utc(2026, 10, 24, 12, 0)), first);
        // After the first occurrence the next is the following day, not the
        // repeat an hour later.
        assert_eq!(after(&trigger, first), utc(2026, 10, 26, 1, 30));
        assert_eq!(
            trigger.occurrence_at_or_before(utc(2026, 10, 25, 1, 45)),
            Ok(Some(first))
        );

        // New York, 1 November 2026: 01:30 happens at 05:30 and 06:30 UTC.
        let trigger = daily("01:30", "America/New_York");
        assert_eq!(after(&trigger, utc(2026, 10, 31, 12, 0)), utc(2026, 11, 1, 5, 30));
        assert_eq!(after(&trigger, utc(2026, 11, 1, 5, 30)), utc(2026, 11, 2, 6, 30));
    }

    #[test]
    fn weekly_fires_on_its_weekdays() {
        // 12 November 2026 is a Thursday.
        let trigger = Trigger::Weekly {
            weekdays: vec![Weekday::Monday, Weekday::Friday],
            time: time("09:00"),
            zone: "UTC".to_owned(),
        };
        let friday = utc(2026, 11, 13, 9, 0);
        let monday = utc(2026, 11, 16, 9, 0);
        assert_eq!(after(&trigger, utc(2026, 11, 12, 10, 0)), friday);
        assert_eq!(after(&trigger, friday), monday);
        assert_eq!(after(&trigger, monday), utc(2026, 11, 20, 9, 0));
        assert_eq!(
            trigger.occurrence_at_or_before(utc(2026, 11, 15, 0, 0)),
            Ok(Some(friday))
        );
    }

    #[test]
    fn monthly_handles_months_without_the_day() {
        let monthly = |day: u8, if_day_missing: ShortMonth| Trigger::Monthly {
            day,
            time: time("08:00"),
            zone: "UTC".to_owned(),
            if_day_missing,
        };
        // Day 31, falling back to the last day: 31 January, 28 February (2026
        // is not a leap year), 31 March, 30 April.
        let last = monthly(31, ShortMonth::LastDay);
        let january = utc(2026, 1, 31, 8, 0);
        let february = utc(2026, 2, 28, 8, 0);
        let march = utc(2026, 3, 31, 8, 0);
        assert_eq!(after(&last, utc(2026, 1, 1, 0, 0)), january);
        assert_eq!(after(&last, january), february);
        assert_eq!(after(&last, february), march);
        assert_eq!(after(&last, march), utc(2026, 4, 30, 8, 0));
        // A leap year has 29 February.
        assert_eq!(after(&last, utc(2028, 2, 1, 0, 0)), utc(2028, 2, 29, 8, 0));

        // Day 31, skipping: January, March, May.
        let skip = monthly(31, ShortMonth::Skip);
        assert_eq!(after(&skip, january), march);
        assert_eq!(after(&skip, march), utc(2026, 5, 31, 8, 0));
        // Day 29, skipping, fires in February only in a leap year.
        let skip = monthly(29, ShortMonth::Skip);
        assert_eq!(after(&skip, utc(2026, 1, 29, 8, 0)), utc(2026, 3, 29, 8, 0));
        assert_eq!(after(&skip, utc(2028, 1, 29, 8, 0)), utc(2028, 2, 29, 8, 0));
        // An ordinary day is the same under both rules.
        assert_eq!(
            after(&monthly(15, ShortMonth::Skip), january),
            after(&monthly(15, ShortMonth::LastDay), january)
        );
    }

    #[test]
    fn nothing_is_due_before_the_first_occurrence() {
        let trigger = daily("07:30", "UTC");
        let created = utc(2026, 11, 12, 10, 0);
        let first = utc(2026, 11, 13, 7, 30);
        for policy in [MissedRunPolicy::RunOnce, MissedRunPolicy::Skip] {
            assert_eq!(
                next_run(&trigger, policy, created, created),
                Ok(NextRun::Wait {
                    next: first,
                    skipped_missed: false
                })
            );
            assert_eq!(
                next_run(&trigger, policy, created, Timestamp(first.0 - 1)),
                Ok(NextRun::Wait {
                    next: first,
                    skipped_missed: false
                })
            );
        }
    }

    #[test]
    fn a_run_within_the_grace_is_on_time_under_either_policy() {
        let trigger = daily("07:30", "UTC");
        let created = utc(2026, 11, 12, 10, 0);
        let first = utc(2026, 11, 13, 7, 30);
        let second = utc(2026, 11, 14, 7, 30);
        for policy in [MissedRunPolicy::RunOnce, MissedRunPolicy::Skip] {
            for latency in [0, 1, ON_TIME_GRACE_SECONDS] {
                assert_eq!(
                    next_run(&trigger, policy, created, Timestamp(first.0 + latency)),
                    Ok(NextRun::Due {
                        scheduled_for: first,
                        late: false,
                        next: Some(second)
                    })
                );
            }
        }
    }

    #[test]
    fn missed_runs_collapse_into_one_or_are_skipped() {
        let trigger = daily("07:30", "UTC");
        let created = utc(2026, 11, 12, 10, 0);
        // The machine was off for the 13th, 14th and 15th and comes back at
        // noon on the 15th.
        let back = utc(2026, 11, 15, 12, 0);
        let latest = utc(2026, 11, 15, 7, 30);
        let next = utc(2026, 11, 16, 7, 30);
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::RunOnce, created, back),
            Ok(NextRun::Due {
                scheduled_for: latest,
                late: true,
                next: Some(next)
            })
        );
        // Having run once, nothing more is due: the two older occurrences are
        // not made up one by one.
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::RunOnce, back, Timestamp(back.0 + 5)),
            Ok(NextRun::Wait {
                next,
                skipped_missed: false
            })
        );
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::Skip, created, back),
            Ok(NextRun::Wait {
                next,
                skipped_missed: true
            })
        );
        // Coming back just after an occurrence: that one is on time, and the
        // older ones are dropped under either policy.
        let just_after = Timestamp(latest.0 + 30);
        for policy in [MissedRunPolicy::RunOnce, MissedRunPolicy::Skip] {
            assert_eq!(
                next_run(&trigger, policy, created, just_after),
                Ok(NextRun::Due {
                    scheduled_for: latest,
                    late: false,
                    next: Some(next)
                })
            );
        }
    }

    #[test]
    fn a_once_trigger_finishes() {
        let trigger = Trigger::Once { at: Timestamp(10_000) };
        let created = Timestamp(5_000);
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::Skip, created, Timestamp(9_000)),
            Ok(NextRun::Wait {
                next: Timestamp(10_000),
                skipped_missed: false
            })
        );
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::Skip, created, Timestamp(10_010)),
            Ok(NextRun::Due {
                scheduled_for: Timestamp(10_000),
                late: false,
                next: None
            })
        );
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::RunOnce, created, Timestamp(99_000)),
            Ok(NextRun::Due {
                scheduled_for: Timestamp(10_000),
                late: true,
                next: None
            })
        );
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::Skip, created, Timestamp(99_000)),
            Ok(NextRun::Finished {
                skipped_missed: true
            })
        );
        // After it has run.
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::RunOnce, Timestamp(10_010), Timestamp(10_020)),
            Ok(NextRun::Finished {
                skipped_missed: false
            })
        );
    }

    #[test]
    fn a_clock_set_back_does_not_run_anything_twice() {
        let trigger = daily("07:30", "UTC");
        let ran = utc(2026, 11, 13, 7, 30);
        let earlier = utc(2026, 11, 13, 6, 0);
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::RunOnce, ran, earlier),
            Ok(NextRun::Wait {
                next: utc(2026, 11, 14, 7, 30),
                skipped_missed: false
            })
        );
    }

    #[test]
    fn events_have_no_time_and_are_rate_limited() {
        let trigger = Trigger::Event {
            event: SystemEvent::DeviceConnected,
        };
        assert_eq!(
            next_run(&trigger, MissedRunPolicy::Skip, Timestamp(0), Timestamp(10)),
            Ok(NextRun::OnEvent)
        );
        assert_eq!(trigger.occurrence_after(Timestamp(0)), Ok(None));
        assert!(event_may_fire(None, Timestamp(10)));
        assert!(!event_may_fire(Some(Timestamp(1_000)), Timestamp(1_000)));
        assert!(!event_may_fire(
            Some(Timestamp(1_000)),
            Timestamp(1_000 + MIN_INTERVAL_SECONDS - 1)
        ));
        assert!(event_may_fire(
            Some(Timestamp(1_000)),
            Timestamp(1_000 + MIN_INTERVAL_SECONDS)
        ));
        // A clock set back does not open the gate.
        assert!(!event_may_fire(Some(Timestamp(1_000)), Timestamp(10)));
    }

    #[test]
    fn the_wall_clock_reading_of_a_daily_trigger_is_what_was_asked_for() {
        // Every day of 2026 in a zone with daylight saving: the occurrence
        // reads 07:30 on the wall clock, and consecutive dates.
        let zone: Tz = "Europe/Rome".parse().expect("a known zone");
        let trigger = daily("07:30", "Europe/Rome");
        let mut reference = utc(2026, 1, 1, 0, 0);
        let mut previous_date = None;
        for _ in 0..365 {
            let occurrence = after(&trigger, reference);
            let seconds = i64::try_from(occurrence.0).expect("fits");
            let local = wall_clock(zone, seconds).expect("in range");
            assert_eq!((local.hour(), local.minute(), local.second()), (7, 30, 0));
            if let Some(previous) = previous_date {
                assert_eq!(Some(local.date()), NaiveDate::succ_opt(&previous));
            }
            previous_date = Some(local.date());
            reference = occurrence;
        }
    }
}
