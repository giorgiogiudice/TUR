// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Waking the schedule runner at the time of the next reminder or
//! scheduled task.
//!
//! Nothing in P@x sits in a loop waiting for a time. The service asks the
//! person's own service manager for one timer, set to the instant of the
//! next occurrence, and goes back to waiting for messages. When the timer
//! elapses, systemd starts `tur-pax schedule wake`, which tells the service
//! to look at what is due.
//!
//! The timer is a transient user timer, made with `systemd-run --user`:
//!
//! ```text
//! systemctl --user stop tur-pax-wake-*.timer
//! systemd-run --user --quiet --collect
//!     --unit=tur-pax-wake-<instant>-<n>
//!     --description=P@x: look at the schedules that are due
//!     --timer-property=AccuracySec=1s
//!     --setenv=DBUS_SESSION_BUS_ADDRESS=<the bus the service is on>
//!     --setenv=XDG_CONFIG_HOME=<where the service reads its settings>
//!     --on-calendar=<YYYY-MM-DD HH:MM:SS UTC>
//!     -- <tur-pax> schedule wake
//! ```
//!
//! * The instant is absolute and in UTC. The recurrence is not written in
//!   systemd's calendar syntax: the rules of `tur-pax-memory` for daylight
//!   saving, short months and the frequency cap stay the only ones, and
//!   the timer is set again after every run.
//! * A calendar timer counts wall-clock time, so it elapses when the
//!   computer wakes from sleep after the instant has passed. The runner
//!   then finds the occurrence late and says so.
//! * A transient timer does not outlive the person's session. What came
//!   due while the computer was off, or while nobody was signed in, is
//!   found when the session starts: the unit `tur-pax-catchup.timer`
//!   (`services/tur-pax/units/`) starts the same command then, and again
//!   when the clock or the time zone is changed.
//! * Nothing of a schedule is on the command line: not its name, not its
//!   text. The command is fixed.
//!
//! The program paths are fixed and are never looked up along a search path.
//! How systemd reads these options is taken from its documentation
//! (`systemd-run(1)`, `systemd.timer(5)`); the `pax` workflow runs the real
//! thing where a user service manager is available.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use tur_pax_memory::Timestamp;

/// The program that makes the transient timer.
pub const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";

/// The program that stops an earlier timer.
pub const SYSTEMCTL: &str = "/usr/bin/systemctl";

/// The prefix of the names of the transient units.
pub const UNIT_PREFIX: &str = "tur-pax-wake-";

/// Sets, moves or removes the one timer that wakes the schedule runner.
pub trait Waker: Send + Sync {
    /// Wake the runner at `at`; with `None`, do not wake it at all.
    ///
    /// # Errors
    ///
    /// A sentence saying why the timer could not be set.
    fn arm(&self, at: Option<Timestamp>) -> Result<(), String>;
}

/// A waker that sets no timer. For a caller that is woken some other way.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWaker;

impl Waker for NoWaker {
    fn arm(&self, _at: Option<Timestamp>) -> Result<(), String> {
        Ok(())
    }
}

/// An instant in the form systemd's `OnCalendar=` takes for one moment, in
/// UTC: `YYYY-MM-DD HH:MM:SS UTC`.
pub fn calendar_time(at: Timestamp) -> Option<String> {
    let text = at.to_rfc3339();
    // `2026-11-13T06:30:00Z`
    let (date, rest) = text.split_once('T')?;
    let time = rest.strip_suffix('Z')?;
    (date.len() == 10 && time.len() == 8).then(|| format!("{date} {time} UTC"))
}

/// The arguments of `systemctl` that stop every timer a waker set before.
pub fn stop_arguments() -> Vec<String> {
    vec![
        "--user".to_owned(),
        "--quiet".to_owned(),
        "stop".to_owned(),
        format!("{UNIT_PREFIX}*.timer"),
    ]
}

/// The arguments of `systemd-run` that set the timer.
///
/// `program` is the path of `tur-pax`; `bus_address` is the session bus the
/// service is on, and `config_home` the directory its settings are under,
/// so that the command the timer starts reaches the same service and reads
/// the same switch; `serial` makes the unit name one of its own.
pub fn run_arguments(
    program: &Path,
    bus_address: &str,
    config_home: Option<&Path>,
    at: Timestamp,
    serial: u64,
) -> Option<Vec<String>> {
    let when = calendar_time(at)?;
    // What goes into an environment assignment of a unit: nothing in it may
    // break out of that.
    let plain = |text: &str| {
        !text.is_empty()
            && text.len() <= 1024
            && text
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\'' | b'\\' | b'$'))
    };
    let config_home = match config_home {
        Some(directory) => {
            let text = directory
                .to_str()
                .filter(|text| text.starts_with('/') && plain(text))?;
            Some(format!("--setenv=XDG_CONFIG_HOME={text}"))
        }
        None => None,
    };
    let program = program.to_str().filter(|path| {
        path.starts_with('/')
            && path
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\'' | b'\\'))
    })?;
    if !plain(bus_address) {
        return None;
    }
    let mut arguments = vec![
        "--user".to_owned(),
        "--quiet".to_owned(),
        "--collect".to_owned(),
        format!("--unit={UNIT_PREFIX}{}-{serial}", at.seconds()),
        "--description=P@x: look at the schedules that are due".to_owned(),
        "--timer-property=AccuracySec=1s".to_owned(),
        format!("--setenv=DBUS_SESSION_BUS_ADDRESS={bus_address}"),
    ];
    arguments.extend(config_home);
    arguments.extend([
        format!("--on-calendar={when}"),
        "--".to_owned(),
        program.to_owned(),
        "schedule".to_owned(),
        "wake".to_owned(),
    ]);
    Some(arguments)
}

/// Sets the timer with the person's service manager.
#[derive(Debug)]
pub struct SystemdWaker {
    program: PathBuf,
    bus_address: String,
    config_home: Option<PathBuf>,
    serial: AtomicU64,
}

impl SystemdWaker {
    /// A waker whose timer starts `program schedule wake` with the session
    /// bus at `bus_address` and, if given, the settings under `config_home`.
    pub fn new(program: PathBuf, bus_address: String, config_home: Option<PathBuf>) -> Self {
        SystemdWaker {
            program,
            bus_address,
            config_home,
            serial: AtomicU64::new(1),
        }
    }
}

fn run(program: &str, arguments: &[String]) -> Result<(), String> {
    if !Path::new(program).is_file() {
        return Err(format!("{program} is not installed"));
    }
    let status = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("{program} could not be started: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} ended with {status}"))
    }
}

impl Waker for SystemdWaker {
    fn arm(&self, at: Option<Timestamp>) -> Result<(), String> {
        // Whatever was set before goes first. Nothing being there to stop
        // is not a failure.
        let _ = run(SYSTEMCTL, &stop_arguments());
        let Some(at) = at else {
            return Ok(());
        };
        let serial = self.serial.fetch_add(1, Ordering::SeqCst);
        let arguments = run_arguments(
            &self.program,
            &self.bus_address,
            self.config_home.as_deref(),
            at,
            serial,
        )
        .ok_or_else(|| "the timer could not be described".to_owned())?;
        run(SYSTEMD_RUN, &arguments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-13T06:30:00Z.
    const AT: Timestamp = Timestamp(1_794_551_400);

    #[test]
    fn the_instant_is_written_as_one_moment_in_utc() {
        assert_eq!(
            calendar_time(AT).as_deref(),
            Some("2026-11-13 06:30:00 UTC")
        );
        assert_eq!(
            calendar_time(Timestamp(0)).as_deref(),
            Some("1970-01-01 00:00:00 UTC")
        );
        // A time beyond the supported range is not a moment systemd is told.
        assert_eq!(calendar_time(Timestamp(u64::MAX)), None);
    }

    #[test]
    fn the_timer_starts_one_fixed_command() {
        let arguments = run_arguments(
            Path::new("/usr/bin/tur-pax"),
            "unix:path=/run/user/1000/bus",
            Some(Path::new("/home/ada/.config")),
            AT,
            7,
        )
        .expect("arguments");
        assert_eq!(
            arguments,
            [
                "--user",
                "--quiet",
                "--collect",
                "--unit=tur-pax-wake-1794551400-7",
                "--description=P@x: look at the schedules that are due",
                "--timer-property=AccuracySec=1s",
                "--setenv=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus",
                "--setenv=XDG_CONFIG_HOME=/home/ada/.config",
                "--on-calendar=2026-11-13 06:30:00 UTC",
                "--",
                "/usr/bin/tur-pax",
                "schedule",
                "wake",
            ]
        );
        assert_eq!(
            stop_arguments(),
            ["--user", "--quiet", "stop", "tur-pax-wake-*.timer"]
        );
    }

    #[test]
    fn nothing_odd_reaches_the_command_line_of_the_timer() {
        let program = Path::new("/usr/bin/tur-pax");
        for address in [
            "",
            "unix:path=/tmp/a b",
            "unix:path=/tmp/a\"b",
            "unix:path=/tmp/$HOME",
            "unix:path=/tmp/a\nb",
            "unix:path=/tmp/a\\b",
        ] {
            assert_eq!(
                run_arguments(program, address, None, AT, 1),
                None,
                "{address:?}"
            );
        }
        for path in ["tur-pax", "/usr/bin/tur pax", "/usr/bin/tur\"pax"] {
            assert_eq!(
                run_arguments(Path::new(path), "unix:path=/run/user/1/bus", None, AT, 1),
                None,
                "{path:?}"
            );
        }
        assert_eq!(
            run_arguments(
                program,
                "unix:path=/run/user/1/bus",
                None,
                Timestamp(u64::MAX),
                1
            ),
            None
        );
        for home in ["relative", "/home/a b", "/home/$USER"] {
            assert_eq!(
                run_arguments(
                    program,
                    "unix:path=/run/user/1/bus",
                    Some(Path::new(home)),
                    AT,
                    1
                ),
                None,
                "{home:?}"
            );
        }
        // Without a configuration directory, none is set.
        let plain =
            run_arguments(program, "unix:path=/run/user/1/bus", None, AT, 1).expect("arguments");
        assert!(
            plain
                .iter()
                .all(|argument| !argument.contains("XDG_CONFIG_HOME"))
        );
        // No timer at all is nothing to do, and nothing to fail.
        assert_eq!(NoWaker.arm(Some(AT)), Ok(()));
        assert_eq!(NoWaker.arm(None), Ok(()));
        // A program that is not installed is said to be missing.
        assert_eq!(
            run("/nonexistent/tur-pax-test/systemd-run", &[]),
            Err("/nonexistent/tur-pax-test/systemd-run is not installed".to_owned())
        );
    }
}
