// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Properties of the schedule arithmetic, checked over many generated cases.
//!
//! The cases come from a small fixed-seed generator, so every run tests the
//! same ones and a failure can be reproduced. The two properties the design
//! asks for are here: the next run is always after the reference time, and a
//! recurrence never drifts, however late each run starts.

use chrono::{DateTime, Datelike, NaiveDateTime, Timelike, Utc};
use chrono_tz::Tz;
use tur_pax_memory::trigger::{MIN_INTERVAL_SECONDS, ON_TIME_GRACE_SECONDS};
use tur_pax_memory::{
    IntervalUnit, LocalTime, MissedRunPolicy, NextRun, ShortMonth, SystemEvent, Timestamp, Trigger,
    Weekday, next_run,
};

// xorshift64*: enough to spread test cases; nothing here needs more.
struct Random(u64);

impl Random {
    fn draw(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.draw() % bound
    }

    fn between(&mut self, low: u64, high: u64) -> u64 {
        low + self.below(high - low + 1)
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let count = u64::try_from(items.len()).expect("fits");
        items[usize::try_from(self.below(count)).expect("fits")]
    }
}

// 2001-09-09 to 2033-05-18.
const START: u64 = 1_000_000_000;
const END: u64 = 2_000_000_000;

const ZONES: [&str; 8] = [
    "UTC",
    "Europe/Rome",
    "America/New_York",
    "Australia/Lord_Howe",
    "Asia/Kolkata",
    "Pacific/Auckland",
    "America/Sao_Paulo",
    "Asia/Tokyo",
];

const POLICIES: [MissedRunPolicy; 2] = [MissedRunPolicy::RunOnce, MissedRunPolicy::Skip];

fn random_time(random: &mut Random) -> LocalTime {
    let hour = u8::try_from(random.below(24)).expect("fits");
    let minute = u8::try_from(random.below(60)).expect("fits");
    LocalTime::new(hour, minute).expect("a valid time")
}

fn random_interval(random: &mut Random) -> (u32, IntervalUnit) {
    let unit = random.pick(&[
        IntervalUnit::Minutes,
        IntervalUnit::Hours,
        IntervalUnit::Days,
    ]);
    let every = match unit {
        IntervalUnit::Minutes => random.between(5, 10_000),
        IntervalUnit::Hours => random.between(1, 500),
        IntervalUnit::Days => random.between(1, 30),
    };
    (u32::try_from(every).expect("fits"), unit)
}

fn random_trigger(random: &mut Random) -> Trigger {
    let zone = random.pick(&ZONES).to_owned();
    let time = random_time(random);
    match random.below(6) {
        0 => Trigger::Once {
            at: Timestamp(random.between(START, END)),
        },
        1 => {
            let (every, unit) = random_interval(random);
            Trigger::Every {
                every,
                unit,
                anchor: Timestamp(random.between(START - 100_000_000, END)),
            }
        }
        2 => Trigger::Daily { time, zone },
        3 => {
            let mut weekdays: Vec<Weekday> = Weekday::ALL
                .into_iter()
                .filter(|_| random.below(2) == 0)
                .collect();
            if weekdays.is_empty() {
                weekdays.push(random.pick(&Weekday::ALL));
            }
            Trigger::Weekly {
                weekdays,
                time,
                zone,
            }
        }
        4 => Trigger::Monthly {
            day: u8::try_from(random.between(1, 31)).expect("fits"),
            time,
            zone,
            if_day_missing: random.pick(&[ShortMonth::LastDay, ShortMonth::Skip]),
        },
        _ => Trigger::Event {
            event: random.pick(&SystemEvent::ALL),
        },
    }
}

fn after(trigger: &Trigger, reference: Timestamp) -> Option<Timestamp> {
    trigger
        .occurrence_after(reference)
        .unwrap_or_else(|problem| panic!("{trigger:?}: {problem}"))
}

fn at_or_before(trigger: &Trigger, reference: Timestamp) -> Option<Timestamp> {
    trigger
        .occurrence_at_or_before(reference)
        .unwrap_or_else(|problem| panic!("{trigger:?}: {problem}"))
}

fn wall_clock(zone: Tz, at: Timestamp) -> NaiveDateTime {
    let seconds = i64::try_from(at.0).expect("fits");
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .expect("in range")
        .with_timezone(&zone)
        .naive_local()
}

#[test]
fn an_occurrence_is_after_its_reference_and_occurrences_are_consistent() {
    let mut random = Random(0x5EED_0001_ABCD_1234);
    for case in 0..3_000 {
        let trigger = random_trigger(&mut random);
        let reference = Timestamp(random.between(START, END));
        let next = after(&trigger, reference);
        let previous = at_or_before(&trigger, reference);
        let place = format!("case {case}: {trigger:?} at {reference}");

        if let Some(previous) = previous {
            assert!(previous <= reference, "{place}");
            // Nothing lies between the previous occurrence and the next.
            assert_eq!(after(&trigger, previous), next, "{place}");
        }
        if let Some(next) = next {
            assert!(next > reference, "{place}");
            // What `occurrence_after` returns is itself an occurrence, and
            // the answer does not depend on where in the gap one asks from.
            assert_eq!(at_or_before(&trigger, next), Some(next), "{place}");
            let just_before = Timestamp(next.0 - 1);
            assert_eq!(after(&trigger, just_before), Some(next), "{place}");
            assert_eq!(at_or_before(&trigger, just_before), previous, "{place}");
        }
    }
}

#[test]
fn the_next_run_is_always_after_now() {
    let mut random = Random(0x5EED_0002_ABCD_1234);
    for case in 0..3_000 {
        let trigger = random_trigger(&mut random);
        let policy = random.pick(&POLICIES);
        let since = Timestamp(random.between(START, END));
        // Sometimes close to `since`, sometimes years away, sometimes before
        // it, as after a clock was set back.
        let now = match random.below(3) {
            0 => Timestamp(since.0 + random.below(200_000)),
            1 => Timestamp(since.0 - random.below(200_000)),
            _ => Timestamp(random.between(START, END)),
        };
        let place = format!("case {case}: {trigger:?} since {since} now {now}");
        let outcome = next_run(&trigger, policy, since, now)
            .unwrap_or_else(|problem| panic!("{place}: {problem}"));
        match outcome {
            NextRun::Due {
                scheduled_for,
                late,
                next,
            } => {
                assert!(scheduled_for <= now, "{place}");
                assert!(scheduled_for > since, "{place}");
                assert_eq!(
                    late,
                    now.0 - scheduled_for.0 > ON_TIME_GRACE_SECONDS,
                    "{place}"
                );
                if late {
                    assert_eq!(policy, MissedRunPolicy::RunOnce, "{place}");
                }
                if let Some(next) = next {
                    assert!(next > now, "{place}");
                }
                assert_eq!(next, after(&trigger, now), "{place}");
            }
            NextRun::Wait {
                next,
                skipped_missed,
            } => {
                assert!(next > now, "{place}");
                assert!(next > since, "{place}");
                if skipped_missed {
                    assert_eq!(policy, MissedRunPolicy::Skip, "{place}");
                }
            }
            NextRun::OnEvent => {
                assert!(matches!(trigger, Trigger::Event { .. }), "{place}");
            }
            NextRun::Finished { .. } => {
                assert_eq!(after(&trigger, now.max(since)), None, "{place}");
            }
        }
    }
}

#[test]
fn an_interval_schedule_never_drifts() {
    let mut random = Random(0x5EED_0003_ABCD_1234);
    for case in 0..60 {
        let (every, unit) = random_interval(&mut random);
        let interval = u64::from(every) * unit.seconds();
        assert!(interval >= MIN_INTERVAL_SECONDS);
        let anchor = random.between(START, END);
        let trigger = Trigger::Every {
            every,
            unit,
            anchor: Timestamp(anchor),
        };
        let policy = random.pick(&POLICIES);
        let created = Timestamp(anchor + random.below(interval * 3));
        let mut since = created;
        let mut expected = after(&trigger, created).expect("an interval schedule has no end");
        assert_eq!((expected.0 - anchor) % interval, 0, "case {case}");

        for run in 0..300 {
            let place = format!("case {case}, run {run}: {trigger:?}");
            // Most runs start a little late. Now and then the machine is off
            // for a few intervals.
            let outage = random.below(10) == 0;
            let now = if outage {
                Timestamp(expected.0 + ON_TIME_GRACE_SECONDS + 1 + random.below(interval * 4))
            } else {
                Timestamp(expected.0 + random.below(ON_TIME_GRACE_SECONDS + 1))
            };
            let outcome = next_run(&trigger, policy, since, now)
                .unwrap_or_else(|problem| panic!("{place}: {problem}"));
            let next = match outcome {
                NextRun::Due {
                    scheduled_for,
                    late,
                    next,
                } => {
                    assert_eq!((scheduled_for.0 - anchor) % interval, 0, "{place}");
                    assert!(scheduled_for >= expected, "{place}");
                    if !outage {
                        assert_eq!(scheduled_for, expected, "{place}");
                        assert!(!late, "{place}");
                    }
                    next.expect("an interval schedule has no end")
                }
                NextRun::Wait {
                    next,
                    skipped_missed,
                } => {
                    // Only an outage under the skip policy ends without a run.
                    assert!(outage && skipped_missed, "{place}");
                    assert_eq!(policy, MissedRunPolicy::Skip, "{place}");
                    next
                }
                other => panic!("{place}: unexpected {other:?}"),
            };
            // The next occurrence is on the grid set by the anchor, whatever
            // the latency of this run was.
            assert_eq!((next.0 - anchor) % interval, 0, "{place}");
            assert!(next > now, "{place}");
            assert!(next.0 - now.0 <= interval, "{place}");
            if !outage {
                assert_eq!(next.0, expected.0 + interval, "{place}");
            }
            // The run, or the skipped occurrence, is recorded at `now`.
            since = now;
            expected = next;
        }
    }
}

#[test]
fn a_daily_schedule_keeps_its_wall_clock_time_for_years() {
    // 2024-01-01T00:00:00Z.
    let start = Timestamp(1_704_067_200);
    for zone_name in ZONES {
        let zone: Tz = zone_name.parse().expect("a known zone");
        // Times of day that no daylight-saving change in these zones touches.
        for (hour, minute) in [(5u8, 0u8), (7, 30), (12, 15), (23, 59)] {
            let trigger = Trigger::Daily {
                time: LocalTime::new(hour, minute).expect("a valid time"),
                zone: zone_name.to_owned(),
            };
            let mut since = start;
            let mut expected = after(&trigger, start).expect("a daily schedule has no end");
            let mut previous_date = None;
            // Three years, with every daylight-saving change in them.
            for day in 0..1_100u64 {
                let place = format!("{zone_name} {hour:02}:{minute:02} day {day}");
                let latency = (day * 37) % (ON_TIME_GRACE_SECONDS + 1);
                let now = Timestamp(expected.0 + latency);
                let outcome = next_run(&trigger, MissedRunPolicy::Skip, since, now)
                    .unwrap_or_else(|problem| panic!("{place}: {problem}"));
                let NextRun::Due {
                    scheduled_for,
                    late,
                    next,
                } = outcome
                else {
                    panic!("{place}: not due: {outcome:?}");
                };
                assert_eq!(scheduled_for, expected, "{place}");
                assert!(!late, "{place}");
                let local = wall_clock(zone, scheduled_for);
                assert_eq!(
                    (local.hour(), local.minute(), local.second()),
                    (u32::from(hour), u32::from(minute), 0),
                    "{place}"
                );
                if let Some(previous) = previous_date {
                    assert_eq!(local.date().pred_opt(), Some(previous), "{place}");
                }
                previous_date = Some(local.date());
                since = now;
                expected = next.expect("a daily schedule has no end");
            }
        }
    }
}

#[test]
fn a_daily_schedule_fires_exactly_once_on_every_date_of_a_year_with_two_changes() {
    // 02:30 in Rome does not exist on 29 March 2026 and occurs twice on
    // 25 October 2026.
    let zone: Tz = "Europe/Rome".parse().expect("a known zone");
    let trigger = Trigger::Daily {
        time: LocalTime::new(2, 30).expect("a valid time"),
        zone: "Europe/Rome".to_owned(),
    };
    // 2026-01-01T00:00:00Z and 2027-01-01T00:00:00Z.
    let start = Timestamp(1_767_225_600);
    let end = Timestamp(1_798_761_600);
    let mut occurrences = Vec::new();
    let mut reference = start;
    while let Some(next) = after(&trigger, reference) {
        if next >= end {
            break;
        }
        occurrences.push(next);
        reference = next;
    }
    assert_eq!(occurrences.len(), 365);

    // One on each date, in order.
    let dates: Vec<_> = occurrences
        .iter()
        .map(|at| wall_clock(zone, *at).date())
        .collect();
    for pair in dates.windows(2) {
        assert_eq!(pair[0].succ_opt(), Some(pair[1]));
    }

    // Every step is 24 hours except around the two changes: 23.5 hours into
    // and out of the day whose 02:30 is replaced by 03:00, and 25 hours after
    // the day whose 02:30 is taken at its first occurrence.
    let unusual: Vec<(u32, u32, u64)> = occurrences
        .windows(2)
        .filter(|pair| pair[1].0 - pair[0].0 != 86_400)
        .map(|pair| {
            let date = wall_clock(zone, pair[1]).date();
            (date.month(), date.day(), pair[1].0 - pair[0].0)
        })
        .collect();
    assert_eq!(
        unusual,
        [(3, 29, 84_600), (3, 30, 84_600), (10, 26, 90_000)]
    );
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

#[test]
fn a_monthly_schedule_follows_its_rule_for_short_months() {
    let zone: Tz = "Europe/Rome".parse().expect("a known zone");
    // 2024-01-01T00:00:00Z: ten years from a leap year.
    let start = Timestamp(1_704_067_200);
    for day in [28u8, 29, 30, 31] {
        for rule in [ShortMonth::LastDay, ShortMonth::Skip] {
            let trigger = Trigger::Monthly {
                day,
                time: LocalTime::new(8, 0).expect("a valid time"),
                zone: "Europe/Rome".to_owned(),
                if_day_missing: rule,
            };
            let mut reference = start;
            let mut months_seen = 0u32;
            let mut previous: Option<(i32, u32)> = None;
            while months_seen < 120 {
                let occurrence = after(&trigger, reference).expect("a monthly schedule has no end");
                let local = wall_clock(zone, occurrence);
                let place = format!("day {day} {rule:?}: {local}");
                assert_eq!((local.hour(), local.minute()), (8, 0), "{place}");
                let length = days_in_month(local.year(), local.month());
                let wanted = u32::from(day);
                match rule {
                    ShortMonth::LastDay => assert_eq!(local.day(), wanted.min(length), "{place}"),
                    ShortMonth::Skip => assert_eq!(local.day(), wanted, "{place}"),
                }
                // Months come in order. Under `last_day` none is missed;
                // under `skip` exactly the short ones are.
                if let Some((year, month)) = previous {
                    let mut expected = if month == 12 {
                        (year + 1, 1)
                    } else {
                        (year, month + 1)
                    };
                    while rule == ShortMonth::Skip && days_in_month(expected.0, expected.1) < wanted
                    {
                        expected = if expected.1 == 12 {
                            (expected.0 + 1, 1)
                        } else {
                            (expected.0, expected.1 + 1)
                        };
                    }
                    assert_eq!((local.year(), local.month()), expected, "{place}");
                }
                previous = Some((local.year(), local.month()));
                months_seen += 1;
                reference = occurrence;
            }
        }
    }
}

#[test]
fn a_weekly_schedule_fires_only_on_its_weekdays() {
    let mut random = Random(0x5EED_0004_ABCD_1234);
    for case in 0..40 {
        let zone_name = random.pick(&ZONES);
        let zone: Tz = zone_name.parse().expect("a known zone");
        let mut weekdays: Vec<Weekday> = Weekday::ALL
            .into_iter()
            .filter(|_| random.below(3) == 0)
            .collect();
        if weekdays.is_empty() {
            weekdays.push(random.pick(&Weekday::ALL));
        }
        // From 05:00 on, clear of every daylight-saving change in these zones.
        let hour = u8::try_from(random.between(5, 23)).expect("fits");
        let minute = u8::try_from(random.below(60)).expect("fits");
        let trigger = Trigger::Weekly {
            weekdays: weekdays.clone(),
            time: LocalTime::new(hour, minute).expect("a valid time"),
            zone: zone_name.to_owned(),
        };
        let allowed: Vec<u32> = weekdays
            .iter()
            .map(|weekday| weekday.number_from_monday())
            .collect();
        let mut reference = Timestamp(random.between(START, END));
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..80 {
            let occurrence = after(&trigger, reference).expect("a weekly schedule has no end");
            let local = wall_clock(zone, occurrence);
            let place = format!("case {case}: {trigger:?}: {local}");
            let weekday = local.weekday().number_from_monday();
            assert!(allowed.contains(&weekday), "{place}");
            assert_eq!(
                (local.hour(), local.minute()),
                (u32::from(hour), u32::from(minute)),
                "{place}"
            );
            assert!(occurrence.0 - reference.0 <= 7 * 86_400 + 3_600, "{place}");
            seen.insert(weekday);
            reference = occurrence;
        }
        // Every chosen weekday came up.
        assert_eq!(seen.len(), allowed.len(), "case {case}");
    }
}
