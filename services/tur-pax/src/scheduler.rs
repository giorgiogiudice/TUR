// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The schedule runner: what is due now, what is done about it, and when to
//! look again.
//!
//! `tur-pax-memory` stores reminders and scheduled tasks and computes their
//! occurrences; it runs nothing. This module is the part that does. One
//! call of [`run_due`] is one look at every schedule of the person:
//!
//! | The schedule | What happens |
//! |---|---|
//! | A reminder that is due | It is shown ([`Runner::remind`]). Once the notification service has taken it, a run record says so. If it could not be shown, nothing is recorded and the reminder stays due. |
//! | A scheduled task that is due | It is run unattended ([`Runner::run_task`]) and a run record keeps what it did and what it had to leave for the person. If it cannot start now, because another task is running, it stays due. |
//! | Occurrences that were missed, and the schedule says to skip them | One run record says that they were passed over. Nothing is shown or run. |
//! | Anything else | Nothing. |
//!
//! Afterwards the report says when the next occurrence of any schedule is,
//! which is when the caller sets its timer ([`crate::wake`]).
//!
//! # Late
//!
//! An occurrence that is found more than two minutes after its time is
//! late: the computer was off, asleep, or nobody was signed in. What then
//! happens is the schedule's own choice, made when it was created
//! (`tur-pax-memory`, `MissedRunPolicy`):
//!
//! * `run_once`: it is shown or run once, however many occurrences were
//!   missed, and the reminder says when it was due;
//! * `skip`: the missed occurrences are passed over, and the next one is
//!   waited for.
//!
//! # The clock
//!
//! Time is read only through the clock that is handed in, so every rule
//! below is tested with a clock moved by hand.
//!
//! * **Daylight saving.** Occurrences are instants computed by
//!   `tur-pax-memory` from the zone stored with the schedule: a wall-clock
//!   time that does not exist fires when the gap ends, one that occurs
//!   twice fires the first time. The runner adds nothing to that.
//! * **A change of time zone.** A schedule keeps the zone it was created
//!   in. Moving the computer to another zone moves no occurrence.
//! * **A clock set backwards.** A schedule's last run is taken from its run
//!   records. If the clock now shows an earlier time than that, the
//!   schedule waits for the first occurrence after its last run: nothing
//!   fires twice. A run record is never dated after the moment it is
//!   written.
//!
//! # What an unattended task may do
//!
//! Nothing here decides that. The task goes through the same loop and the
//! same policy engine as any other, with nobody present
//! ([`crate::orchestrator::Situation`]): classes 0 and 1 run, a memory
//! record on the person's word is held back, and an action above class 1
//! never runs; what was held back is in the run record, for the person's
//! return (addendum A sections 5.6, 18 and 22).
//!
//! Event triggers (`on a device being connected`) are stored and not
//! delivered: nothing sends the runner events yet.

use tur_pax_memory::{
    ActionRecord, Clock, DeferredAction, NewRun, NextRun, RunOutcome, Schedule, ScheduleBook,
    ScheduledAction, Timestamp,
};

/// How long after a reminder could not be shown it is tried again, in
/// seconds.
pub const RETRY_SECONDS: u64 = 60;

/// What an unattended run of a scheduled task did.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRun {
    /// When the run started.
    pub started_at: Timestamp,
    /// How it ended. `Deferred` exactly when something was held back.
    pub outcome: RunOutcome,
    /// The calls that were made.
    pub performed: Vec<ActionRecord>,
    /// The calls that were held back for the person.
    pub deferred: Vec<DeferredAction>,
    /// A short account of the outcome, in the system's words.
    pub detail: String,
    /// The task identifier the run has in the audit trail.
    pub task: String,
}

/// What the runner needs from the outside: a way to show a reminder and a
/// way to run a task with nobody there.
pub trait Runner {
    /// Show a reminder. `late` says the occurrence is older than the
    /// on-time grace.
    ///
    /// # Errors
    ///
    /// A sentence saying why it could not be shown. The reminder then stays
    /// due.
    fn remind(
        &mut self,
        schedule: &Schedule,
        message: &str,
        scheduled_for: Timestamp,
        late: bool,
    ) -> Result<(), String>;

    /// Run a scheduled task, unattended. `None` if it cannot start now; it
    /// then stays due.
    fn run_task(
        &mut self,
        schedule: &Schedule,
        request: &str,
        scheduled_for: Timestamp,
        late: bool,
    ) -> Option<TaskRun>;
}

/// What became of one schedule that was looked at and was not simply
/// waiting.
#[derive(Debug, Clone, PartialEq)]
pub struct Fired {
    /// The schedule.
    pub schedule: String,
    /// Its name.
    pub name: String,
    /// Whether it is a reminder.
    pub reminder: bool,
    /// The occurrence concerned.
    pub scheduled_for: Option<Timestamp>,
    /// Whether the occurrence was found late.
    pub late: bool,
    /// How it ended, if a run record was written. `None` if the reminder
    /// could not be shown or the task could not start: it is still due.
    pub outcome: Option<RunOutcome>,
    /// The identifier of the run record, if one was written.
    pub run: Option<String>,
    /// The task identifier of an unattended run.
    pub task: Option<String>,
    /// How many actions were held back for the person.
    pub held: usize,
}

/// What one look at the schedules did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TickReport {
    /// The schedules something happened to, in the order of their
    /// identifiers.
    pub fired: Vec<Fired>,
    /// The earliest next occurrence of any schedule: when to look again.
    pub next_wake: Option<Timestamp>,
    /// What went wrong, each in a sentence. A problem with one schedule
    /// does not stop the others.
    pub problems: Vec<String>,
}

/// The outcome of a run as the audit trail and the interface write it.
pub const fn outcome_name(outcome: RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Completed => "completed",
        RunOutcome::Deferred => "deferred",
        RunOutcome::Failed => "failed",
        RunOutcome::SkippedMissed => "skipped_missed",
    }
}

/// Look at every schedule once: show the reminders that are due, run the
/// scheduled tasks that are due, pass over what the schedules say to skip,
/// and say when to look again.
pub fn run_due<S: tur_pax_memory::Store>(
    book: &mut ScheduleBook<S>,
    clock: &dyn Clock,
    runner: &mut dyn Runner,
) -> TickReport {
    let mut report = TickReport::default();
    let schedules = match book.list() {
        Ok(listing) => {
            for damaged in listing.damaged {
                report.problems.push(format!(
                    "the schedule file {} is damaged and was left alone: {}",
                    damaged.id, damaged.reason
                ));
            }
            listing.schedules
        }
        Err(problem) => {
            report.problems.push(format!(
                "the schedules could not be read: {}",
                problem.detail
            ));
            return report;
        }
    };

    // A reminder that could not be shown is tried again this long after.
    let mut retry = false;
    for schedule in &schedules {
        let id = schedule.id().clone();
        let now = clock.now();
        let due = match book.next_run(&id, now) {
            Ok(due) => due,
            Err(problem) => {
                report.problems.push(format!("{id}: {}", problem.detail));
                continue;
            }
        };
        let mut fired = Fired {
            schedule: id.to_string(),
            name: schedule.name().to_owned(),
            reminder: schedule.is_reminder(),
            scheduled_for: None,
            late: false,
            outcome: None,
            run: None,
            task: None,
            held: 0,
        };
        let new_run = match due {
            NextRun::Due {
                scheduled_for,
                late,
                ..
            } => {
                fired.scheduled_for = Some(scheduled_for);
                fired.late = late;
                match schedule.action() {
                    ScheduledAction::Reminder { message } => {
                        match runner.remind(schedule, message, scheduled_for, late) {
                            Ok(()) => Some(NewRun {
                                schedule: id.clone(),
                                scheduled_for: Some(scheduled_for),
                                event: None,
                                started_at: now,
                                outcome: RunOutcome::Completed,
                                performed: Vec::new(),
                                deferred: Vec::new(),
                                detail: if late {
                                    format!("shown late: it was due at {scheduled_for}")
                                } else {
                                    String::new()
                                },
                            }),
                            Err(reason) => {
                                report
                                    .problems
                                    .push(format!("{id}: the reminder was not shown: {reason}"));
                                retry = true;
                                None
                            }
                        }
                    }
                    ScheduledAction::Task { request, .. } => {
                        match runner.run_task(schedule, request, scheduled_for, late) {
                            Some(run) => {
                                fired.task = Some(run.task.clone());
                                fired.held = run.deferred.len();
                                Some(NewRun {
                                    schedule: id.clone(),
                                    scheduled_for: Some(scheduled_for),
                                    event: None,
                                    started_at: run.started_at,
                                    outcome: run.outcome,
                                    performed: run.performed,
                                    deferred: run.deferred,
                                    detail: run.detail,
                                })
                            }
                            None => None,
                        }
                    }
                }
            }
            NextRun::Wait {
                skipped_missed: true,
                ..
            }
            | NextRun::Finished {
                skipped_missed: true,
            } => {
                let missed = schedule
                    .trigger()
                    .occurrence_at_or_before(now)
                    .ok()
                    .flatten();
                fired.scheduled_for = missed;
                fired.late = true;
                Some(NewRun {
                    schedule: id.clone(),
                    scheduled_for: missed,
                    event: None,
                    started_at: now,
                    outcome: RunOutcome::SkippedMissed,
                    performed: Vec::new(),
                    deferred: Vec::new(),
                    detail: "missed while the computer was off or asleep, and passed over as \
                             this schedule asks"
                        .to_owned(),
                })
            }
            NextRun::Wait { .. } | NextRun::Finished { .. } | NextRun::OnEvent => continue,
        };
        if let Some(mut new_run) = new_run {
            // The clock may have been set back while the run went on: a run
            // is never recorded as starting after it was recorded.
            let recorded_at = clock.now();
            new_run.started_at = new_run.started_at.min(recorded_at);
            let outcome = new_run.outcome;
            match book.record_run(new_run, recorded_at) {
                Ok(record) => {
                    fired.outcome = Some(outcome);
                    fired.run = Some(record.id.to_string());
                }
                Err(problem) => report.problems.push(format!(
                    "{id}: the run could not be recorded: {}",
                    problem.detail
                )),
            }
        }
        report.fired.push(fired);
    }

    // When to look again: the earliest next occurrence of any schedule.
    let now = clock.now();
    for schedule in &schedules {
        let next = match book.next_run(schedule.id(), now) {
            Ok(NextRun::Wait { next, .. }) => Some(next),
            Ok(NextRun::Due { next, .. }) => next,
            Ok(NextRun::Finished { .. } | NextRun::OnEvent) | Err(_) => None,
        };
        if let Some(next) = next
            && report.next_wake.is_none_or(|earliest| next < earliest)
        {
            report.next_wake = Some(next);
        }
    }
    if retry && let Some(soon) = now.checked_add(RETRY_SECONDS) {
        report.next_wake = Some(report.next_wake.map_or(soon, |next| next.min(soon)));
    }
    report
}
