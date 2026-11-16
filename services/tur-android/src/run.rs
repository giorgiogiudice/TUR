// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Running other programs with a time limit.
//!
//! What a program prints goes to files, not pipes, so that a program that
//! leaves a child behind holding its output can never block this one. The
//! files are removed when the program has finished.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// What a finished program left.
#[derive(Debug, Clone, Default)]
pub(crate) struct Output {
    /// The exit status, if the program exited by itself.
    pub(crate) status: Option<i32>,
    /// What it printed on standard output.
    pub(crate) stdout: Vec<u8>,
    /// What it printed on standard error.
    pub(crate) stderr: String,
    /// The time limit was reached and the program was killed.
    pub(crate) timed_out: bool,
    /// The program could not be started at all.
    pub(crate) not_started: Option<String>,
}

impl Output {
    /// The program ran and exited with status 0.
    pub(crate) fn ok(&self) -> bool {
        self.status == Some(0) && !self.timed_out && self.not_started.is_none()
    }

    /// Standard output as text.
    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// A one-paragraph account of a failure: how it ended and the last lines
    /// it printed.
    pub(crate) fn account(&self) -> String {
        let ending = if let Some(reason) = &self.not_started {
            format!("could not be started: {reason}")
        } else if self.timed_out {
            "did not finish in time and was stopped".to_owned()
        } else {
            match self.status {
                Some(status) => format!("exited with status {status}"),
                None => "was ended by a signal".to_owned(),
            }
        };
        let mut lines: Vec<String> = self
            .stderr
            .lines()
            .chain(String::from_utf8_lossy(&self.stdout).lines())
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        let keep = lines.len().saturating_sub(6);
        lines.drain(..keep);
        if lines.is_empty() {
            ending
        } else {
            format!("{ending}: {}", lines.join(" | "))
        }
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn scratch(kind: &str) -> PathBuf {
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "tur-android-{}-{number}.{kind}",
        std::process::id()
    ))
}

fn take(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap_or_default();
    let _ = fs::remove_file(path);
    bytes
}

/// Runs `program` with `arguments` and waits at most `limit`.
pub(crate) fn run(program: &str, arguments: &[&str], limit: Duration) -> Output {
    let mut command = Command::new(program);
    command.args(arguments);
    run_command(&mut command, limit)
}

/// Runs a prepared command and waits at most `limit`. Standard input is
/// empty.
pub(crate) fn run_command(command: &mut Command, limit: Duration) -> Output {
    let out_path = scratch("out");
    let err_path = scratch("err");
    let (out_file, err_file) = match (File::create(&out_path), File::create(&err_path)) {
        (Ok(out_file), Ok(err_file)) => (out_file, err_file),
        (out_result, err_result) => {
            let reason = match (out_result, err_result) {
                (Err(error), _) | (_, Err(error)) => error.to_string(),
                _ => String::new(),
            };
            let _ = fs::remove_file(&out_path);
            let _ = fs::remove_file(&err_path);
            return Output {
                not_started: Some(format!("no file for its output: {reason}")),
                ..Output::default()
            };
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file));
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(&out_path);
            let _ = fs::remove_file(&err_path);
            return Output {
                not_started: Some(error.to_string()),
                ..Output::default()
            };
        }
    };
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {}
            Err(_) => break None,
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break None;
        }
        thread::sleep(Duration::from_millis(50));
    };
    Output {
        status,
        stdout: take(&out_path),
        stderr: String::from_utf8_lossy(&take(&err_path)).into_owned(),
        timed_out,
        not_started: None,
    }
}

/// Seconds as a number with one decimal, for reports.
pub(crate) fn seconds(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_succeeds() {
        let output = run(
            "sh",
            &["-c", "echo hello; echo there >&2"],
            Duration::from_secs(10),
        );
        assert!(output.ok(), "{output:?}");
        assert_eq!(output.text(), "hello\n");
        assert_eq!(output.stderr, "there\n");
    }

    #[test]
    fn a_program_that_fails_is_described() {
        let output = run(
            "sh",
            &["-c", "echo why >&2; exit 3"],
            Duration::from_secs(10),
        );
        assert!(!output.ok());
        assert_eq!(output.status, Some(3));
        assert_eq!(output.account(), "exited with status 3: why");
    }

    #[test]
    fn a_program_that_runs_too_long_is_stopped() {
        let started = Instant::now();
        let output = run("sleep", &["30"], Duration::from_millis(300));
        assert!(output.timed_out);
        assert!(!output.ok());
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(output.account().starts_with("did not finish in time"));
    }

    #[test]
    fn a_missing_program_is_not_started() {
        let output = run("/nonexistent/tur-android-test", &[], Duration::from_secs(1));
        assert!(output.not_started.is_some());
        assert!(output.account().starts_with("could not be started"));
    }

    #[test]
    fn a_grandchild_holding_the_output_does_not_block() {
        // The shell leaves a sleeping child that inherited its output.
        let started = Instant::now();
        let output = run(
            "sh",
            &["-c", "sleep 20 & echo done"],
            Duration::from_secs(10),
        );
        assert!(output.ok());
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(output.text(), "done\n");
    }

    #[test]
    fn seconds_have_one_decimal() {
        assert_eq!(seconds(Duration::from_millis(1234)), 1.2);
    }
}
