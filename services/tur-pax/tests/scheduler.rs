// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The schedule runner against a clock moved by hand.
//!
//! Each test keeps its schedules in an in-memory store, creates them the
//! way the person does (with a confirmation for exactly the proposal), and
//! then moves the clock the way a timer would: to the next wake the runner
//! asked for, or somewhere else on purpose. Nothing here sleeps, and no
//! real clock is read.

use std::cell::Cell;

use tur_pax::scheduler::{RETRY_SECONDS, Runner, TaskRun, TickReport, run_due};
use tur_pax_memory::{
    Clock, DeferReason, DeferredAction, LocalTime, MemoryStore, MissedRunPolicy, Proposal,
    ProposedAction, RunOutcome, Schedule, ScheduleBook, ScheduleId, ShortMonth, Timestamp, Trigger,
    UserConfirmation,
};

/// The clock of a test.
struct TestClock(Cell<u64>);

impl TestClock {
    fn at(seconds: u64) -> Self {
        TestClock(Cell::new(seconds))
    }

    fn set(&self, seconds: u64) {
        self.0.set(seconds);
    }
}

impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.get())
    }
}

/// The person, confirming exactly what they were shown.
struct Person {
    shown: Proposal,
    at: Timestamp,
}

impl UserConfirmation<Proposal> for Person {
    fn covers(&self, subject: &Proposal) -> bool {
        self.shown == *subject
    }

    fn confirmed_at(&self) -> Timestamp {
        self.at
    }
}

/// What the runner was asked to do, and what it answers.
#[derive(Default)]
struct TestRunner {
    shown: Vec<(String, u64, bool)>,
    ran: Vec<(String, u64, bool)>,
    /// Refuse to show reminders: the notification service is down.
    notifications_down: bool,
    /// Refuse to start tasks: another task holds the slot.
    busy: bool,
    /// Hold back one action of every task.
    hold_back: bool,
    now: u64,
}

impl Runner for TestRunner {
    fn remind(
        &mut self,
        schedule: &Schedule,
        _message: &str,
        scheduled_for: Timestamp,
        late: bool,
    ) -> Result<(), String> {
        if self.notifications_down {
            return Err("the notification service is not there".to_owned());
        }
        self.shown
            .push((schedule.name().to_owned(), scheduled_for.0, late));
        Ok(())
    }

    fn run_task(
        &mut self,
        schedule: &Schedule,
        _request: &str,
        scheduled_for: Timestamp,
        late: bool,
    ) -> Option<TaskRun> {
        if self.busy {
            return None;
        }
        self.ran
            .push((schedule.name().to_owned(), scheduled_for.0, late));
        let deferred = if self.hold_back {
            vec![DeferredAction {
                action: tur_pax_memory::ActionRecord {
                    service: "org.tur.Test1".to_owned(),
                    tool: "test_consequential".to_owned(),
                    arguments: serde_json::Map::new(),
                    arguments_withheld: false,
                },
                reason: DeferReason::NoStandingApproval,
                detail: "Nobody was there to approve this.".to_owned(),
            }]
        } else {
            Vec::new()
        };
        Some(TaskRun {
            started_at: Timestamp(self.now),
            outcome: if deferred.is_empty() {
                RunOutcome::Completed
            } else {
                RunOutcome::Deferred
            },
            performed: Vec::new(),
            deferred,
            detail: String::new(),
            task: format!("t-{}", self.ran.len()),
        })
    }
}

fn create(
    book: &mut ScheduleBook<MemoryStore>,
    name: &str,
    action: ProposedAction,
    trigger: Trigger,
    missed_runs: MissedRunPolicy,
    at: u64,
) -> ScheduleId {
    let proposal = Proposal {
        name: name.to_owned(),
        action,
        trigger,
        missed_runs,
        proposed_by_task: None,
    };
    let person = Person {
        shown: proposal.clone(),
        at: Timestamp(at),
    };
    book.create(proposal, &person)
        .expect("the schedule is created")
        .id()
        .clone()
}

fn reminder(message: &str) -> ProposedAction {
    ProposedAction::Reminder {
        message: message.to_owned(),
    }
}

fn daily(hour: u8, minute: u8, zone: &str) -> Trigger {
    Trigger::Daily {
        time: LocalTime::new(hour, minute).expect("a time"),
        zone: zone.to_owned(),
    }
}

fn tick(
    book: &mut ScheduleBook<MemoryStore>,
    clock: &TestClock,
    runner: &mut TestRunner,
) -> TickReport {
    runner.now = clock.now().0;
    run_due(book, clock, runner)
}

/// Move the clock the way the timer would: to each wake the runner asks
/// for, a few seconds late, until `until`.
fn follow_the_timer(
    book: &mut ScheduleBook<MemoryStore>,
    clock: &TestClock,
    runner: &mut TestRunner,
    until: u64,
) {
    let mut wakes = 0;
    loop {
        let report = tick(book, clock, runner);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        let Some(next) = report.next_wake else {
            return;
        };
        assert!(next.0 > clock.now().0, "a wake is always after now");
        if next.0 > until {
            return;
        }
        clock.set(next.0 + 3);
        wakes += 1;
        assert!(wakes < 10_000, "the timer never stops");
    }
}

// 2026-03-27T00:00:00Z.
const BEFORE_SPRING: u64 = 1_774_569_600;
// 2026-10-23T00:00:00Z.
const BEFORE_AUTUMN: u64 = 1_792_713_600;
// 2026-11-12T08:00:00Z: 09:00 in Rome, 03:00 in New York.
const NINE_IN_ROME: u64 = 1_794_470_400;
const DAY: u64 = 86_400;

#[test]
fn a_daily_reminder_fires_once_a_day_across_the_spring_daylight_saving_change() {
    let clock = TestClock::at(BEFORE_SPRING);
    let mut book = ScheduleBook::new(MemoryStore::new());
    create(
        &mut book,
        "Pills",
        reminder("Take the pills"),
        daily(2, 30, "Europe/Rome"),
        MissedRunPolicy::RunOnce,
        BEFORE_SPRING,
    );
    let mut runner = TestRunner::default();
    follow_the_timer(&mut book, &clock, &mut runner, BEFORE_SPRING + 4 * DAY);
    let fired: Vec<u64> = runner.shown.iter().map(|(_, at, _)| *at).collect();
    assert_eq!(
        fired,
        [
            // 27 March, 02:30 CET.
            1_774_575_000,
            // 28 March, 02:30 CET.
            1_774_661_400,
            // 29 March: 02:30 does not exist; the end of the gap, 03:00 CEST.
            1_774_746_000,
            // 30 March, 02:30 CEST.
            1_774_830_600,
        ]
    );
    // Shown on time each day: the timer woke the runner a few seconds after.
    assert!(runner.shown.iter().all(|(_, _, late)| !late));
    assert_eq!(book.runs(None).expect("the runs").runs.len(), 4);
}

#[test]
fn a_daily_reminder_fires_once_on_the_night_the_clocks_go_back() {
    let clock = TestClock::at(BEFORE_AUTUMN);
    let mut book = ScheduleBook::new(MemoryStore::new());
    create(
        &mut book,
        "Pills",
        reminder("Take the pills"),
        daily(2, 30, "Europe/Rome"),
        MissedRunPolicy::RunOnce,
        BEFORE_AUTUMN,
    );
    let mut runner = TestRunner::default();
    follow_the_timer(&mut book, &clock, &mut runner, BEFORE_AUTUMN + 4 * DAY);
    let fired: Vec<u64> = runner.shown.iter().map(|(_, at, _)| *at).collect();
    assert_eq!(
        fired,
        [
            // 23 October, 02:30 CEST.
            1_792_715_400,
            // 24 October, 02:30 CEST.
            1_792_801_800,
            // 25 October: 02:30 occurs twice; only the first, CEST.
            1_792_888_200,
            // 26 October, 02:30 CET.
            1_792_978_200,
        ]
    );
}

#[test]
fn a_change_of_the_computers_time_zone_moves_no_occurrence() {
    // The schedule names its zone. The runner is not told the zone of the
    // computer at all, so moving the computer to New York changes nothing:
    // 09:00 in Rome stays 08:00 UTC.
    let created = NINE_IN_ROME - DAY;
    let clock = TestClock::at(created);
    let mut book = ScheduleBook::new(MemoryStore::new());
    create(
        &mut book,
        "Standup",
        reminder("Standup"),
        daily(9, 0, "Europe/Rome"),
        MissedRunPolicy::RunOnce,
        created,
    );
    let mut runner = TestRunner::default();
    let report = tick(&mut book, &clock, &mut runner);
    assert_eq!(report.next_wake, Some(Timestamp(NINE_IN_ROME - DAY + DAY)));
    clock.set(NINE_IN_ROME + 1);
    tick(&mut book, &clock, &mut runner);
    assert_eq!(runner.shown, [("Standup".to_owned(), NINE_IN_ROME, false)]);
}

#[test]
fn a_clock_set_backwards_fires_nothing_twice() {
    let created = NINE_IN_ROME - 3600;
    let clock = TestClock::at(created);
    let mut book = ScheduleBook::new(MemoryStore::new());
    let id = create(
        &mut book,
        "Standup",
        reminder("Standup"),
        daily(9, 0, "Europe/Rome"),
        MissedRunPolicy::RunOnce,
        created,
    );
    let mut runner = TestRunner::default();
    clock.set(NINE_IN_ROME + 30);
    tick(&mut book, &clock, &mut runner);
    assert_eq!(runner.shown.len(), 1);

    // The clock is set back two days and runs forward again. The occurrences
    // it passes over were before the last run: none of them fires.
    clock.set(NINE_IN_ROME - 2 * DAY);
    let report = tick(&mut book, &clock, &mut runner);
    assert!(report.fired.is_empty());
    // The runner waits for the first occurrence after its last run.
    assert_eq!(report.next_wake, Some(Timestamp(NINE_IN_ROME + DAY)));
    for step in 0..96 {
        clock.set(NINE_IN_ROME - 2 * DAY + step * 1800);
        tick(&mut book, &clock, &mut runner);
    }
    assert_eq!(runner.shown.len(), 1, "nothing fired twice");

    // The next day, it fires again.
    clock.set(NINE_IN_ROME + DAY + 5);
    tick(&mut book, &clock, &mut runner);
    assert_eq!(runner.shown.len(), 2);
    assert_eq!(runner.shown[1].1, NINE_IN_ROME + DAY);
    // No run is recorded as starting after it was recorded.
    for run in book.runs(Some(&id)).expect("the runs").runs {
        assert!(run.started_at <= run.finished_at);
    }
}

#[test]
fn what_came_due_while_the_computer_was_off_is_shown_once_and_late() {
    let created = NINE_IN_ROME - 3600;
    let clock = TestClock::at(created);
    let mut book = ScheduleBook::new(MemoryStore::new());
    create(
        &mut book,
        "Standup",
        reminder("Standup"),
        daily(9, 0, "Europe/Rome"),
        MissedRunPolicy::RunOnce,
        created,
    );
    // The computer was off for three days and a half.
    clock.set(NINE_IN_ROME + 3 * DAY + DAY / 2);
    let mut runner = TestRunner::default();
    let report = tick(&mut book, &clock, &mut runner);
    // Four occurrences passed; one reminder, for the latest, marked late.
    assert_eq!(
        runner.shown,
        [("Standup".to_owned(), NINE_IN_ROME + 3 * DAY, true)]
    );
    assert_eq!(report.fired.len(), 1);
    assert!(report.fired[0].late);
    assert_eq!(report.fired[0].outcome, Some(RunOutcome::Completed));
    assert_eq!(report.next_wake, Some(Timestamp(NINE_IN_ROME + 4 * DAY)));
    let run = &book.runs(None).expect("the runs").runs[0];
    assert!(run.detail.starts_with("shown late"), "{}", run.detail);
    // Looking again finds nothing more to do.
    let report = tick(&mut book, &clock, &mut runner);
    assert!(report.fired.is_empty());
    assert_eq!(runner.shown.len(), 1);
}

#[test]
fn a_schedule_that_skips_missed_occurrences_records_that_and_shows_nothing() {
    let created = NINE_IN_ROME - 3600;
    let clock = TestClock::at(created);
    let mut book = ScheduleBook::new(MemoryStore::new());
    create(
        &mut book,
        "Standup",
        reminder("Standup"),
        daily(9, 0, "Europe/Rome"),
        MissedRunPolicy::Skip,
        created,
    );
    clock.set(NINE_IN_ROME + 2 * DAY + 3600);
    let mut runner = TestRunner::default();
    let report = tick(&mut book, &clock, &mut runner);
    assert!(runner.shown.is_empty());
    assert_eq!(report.fired.len(), 1);
    assert_eq!(report.fired[0].outcome, Some(RunOutcome::SkippedMissed));
    assert_eq!(report.next_wake, Some(Timestamp(NINE_IN_ROME + 3 * DAY)));
    // It is passed over once, not again at every look.
    let report = tick(&mut book, &clock, &mut runner);
    assert!(report.fired.is_empty());
}

#[test]
fn a_reminder_that_could_not_be_shown_stays_due_and_is_tried_again_soon() {
    let created = NINE_IN_ROME - 3600;
    let clock = TestClock::at(created);
    let mut book = ScheduleBook::new(MemoryStore::new());
    create(
        &mut book,
        "Once",
        reminder("Once"),
        Trigger::Once {
            at: Timestamp(NINE_IN_ROME),
        },
        MissedRunPolicy::RunOnce,
        created,
    );
    clock.set(NINE_IN_ROME + 10);
    let mut runner = TestRunner {
        notifications_down: true,
        ..TestRunner::default()
    };
    let report = tick(&mut book, &clock, &mut runner);
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.fired[0].outcome, None);
    assert!(book.runs(None).expect("the runs").runs.is_empty());
    // A one-off reminder has no next occurrence; the runner asks to be woken
    // again soon all the same.
    assert_eq!(
        report.next_wake,
        Some(Timestamp(NINE_IN_ROME + 10 + RETRY_SECONDS))
    );
    // The notification service is back.
    runner.notifications_down = false;
    clock.set(NINE_IN_ROME + 10 + RETRY_SECONDS);
    let report = tick(&mut book, &clock, &mut runner);
    assert_eq!(runner.shown, [("Once".to_owned(), NINE_IN_ROME, false)]);
    assert_eq!(report.next_wake, None);
    // It is finished: nothing fires again, ever.
    clock.set(NINE_IN_ROME + 400 * DAY);
    assert!(tick(&mut book, &clock, &mut runner).fired.is_empty());
}

#[test]
fn a_scheduled_task_that_cannot_start_stays_due() {
    let created = NINE_IN_ROME - 3600;
    let clock = TestClock::at(created);
    let mut book = ScheduleBook::new(MemoryStore::new());
    create(
        &mut book,
        "Report",
        ProposedAction::Task {
            request: "Look at the disks".to_owned(),
            approvals: Vec::new(),
        },
        Trigger::Monthly {
            day: 31,
            time: LocalTime::new(9, 0).expect("a time"),
            zone: "Europe/Rome".to_owned(),
            if_day_missing: ShortMonth::LastDay,
        },
        MissedRunPolicy::RunOnce,
        created,
    );
    // 30 November is the last day of the month: the schedule fires then.
    let due = NINE_IN_ROME + 18 * DAY;
    clock.set(due + 5);
    let mut runner = TestRunner {
        busy: true,
        hold_back: true,
        ..TestRunner::default()
    };
    let report = tick(&mut book, &clock, &mut runner);
    assert_eq!(report.fired[0].outcome, None);
    assert!(runner.ran.is_empty());
    // The task that held the slot ended.
    runner.busy = false;
    let report = tick(&mut book, &clock, &mut runner);
    assert_eq!(runner.ran, [("Report".to_owned(), due, false)]);
    assert_eq!(report.fired[0].outcome, Some(RunOutcome::Deferred));
    assert_eq!(report.fired[0].held, 1);
    assert_eq!(report.fired[0].task.as_deref(), Some("t-1"));
    // What it held back waits for the person.
    let waiting = book.unpresented_deferrals().expect("the runs");
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].deferred[0].action.tool, "test_consequential");
}
