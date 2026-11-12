// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Running a [`SandboxPlan`].
//!
//! [`Executor`] is the one door through which the orchestrator runs anything.
//! The only implementation in this crate is [`BwrapExecutor`], which starts
//! bubblewrap with the plan's command line: there is no implementation that
//! runs a command outside the sandbox. Tests of the task loop supply their
//! own executor that records the plans it is given and runs nothing.
//!
//! What the executor adds to the plan:
//!
//! * the sandbox program itself is started with an empty environment and
//!   with standard input closed, unless the plan supplies input;
//! * a granted path is checked once more, immediately before the start: it
//!   must still be what it was when it was granted, not a link to somewhere
//!   else;
//! * the wall-clock limit: when it passes, the sandbox is killed, and with it
//!   every process inside, because they share one process namespace that
//!   dies with its first process;
//! * the output bound: at most `output_bytes` of each stream are kept, and
//!   the rest is read and dropped so that the command does not block.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::sandbox::{MountKind, SandboxPlan};

/// How a sandboxed process ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecStatus {
    /// It ran and exited with this status.
    Exited(i32),
    /// It was killed by a signal.
    Killed,
    /// It was still running at its wall-clock limit and was killed.
    TimedOut,
    /// It was not started, or the sandbox could not be set up. The text says
    /// why. Nothing of the command ran.
    NotStarted(String),
}

impl ExecStatus {
    /// A short name for the audit trail: `exited`, `killed`, `timed_out` or
    /// `not_started`.
    pub const fn name(&self) -> &'static str {
        match self {
            ExecStatus::Exited(_) => "exited",
            ExecStatus::Killed => "killed",
            ExecStatus::TimedOut => "timed_out",
            ExecStatus::NotStarted(_) => "not_started",
        }
    }
}

/// What a sandboxed process produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    /// How it ended.
    pub status: ExecStatus,
    /// The kept part of standard output.
    pub stdout: Vec<u8>,
    /// The kept part of standard error.
    pub stderr: Vec<u8>,
    /// Whether standard output was longer than the bound.
    pub stdout_truncated: bool,
    /// Whether standard error was longer than the bound.
    pub stderr_truncated: bool,
}

impl ExecOutput {
    /// The output of something that did not start.
    pub fn not_started(reason: impl Into<String>) -> Self {
        ExecOutput {
            status: ExecStatus::NotStarted(reason.into()),
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }
}

/// Runs sandbox plans.
pub trait Executor {
    /// Run one plan to its end and return what it produced.
    fn run(&mut self, plan: &SandboxPlan) -> ExecOutput;
}

/// Runs a plan under bubblewrap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BwrapExecutor;

impl BwrapExecutor {
    /// An executor.
    pub fn new() -> Self {
        BwrapExecutor
    }
}

/// How often a running sandbox is looked at.
const POLL: Duration = Duration::from_millis(5);

impl Executor for BwrapExecutor {
    fn run(&mut self, plan: &SandboxPlan) -> ExecOutput {
        if !Path::new(&plan.program).is_file() {
            return ExecOutput::not_started("bubblewrap is not installed");
        }
        // A granted path must still be itself.
        for mount in &plan.mounts {
            if mount.kind == MountKind::Granted {
                let unchanged = fs::canonicalize(&mount.path)
                    .is_ok_and(|resolved| resolved == Path::new(&mount.path));
                if !unchanged {
                    return ExecOutput::not_started(
                        "a granted folder is no longer where it was when it was granted",
                    );
                }
            }
        }

        let mut command = Command::new(&plan.program);
        command
            .args(&plan.arguments)
            .args(&plan.command)
            .env_clear()
            .current_dir("/")
            .stdin(if plan.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return ExecOutput::not_started(format!(
                    "the sandbox could not be started: {error}"
                ));
            }
        };

        let writer = match (child.stdin.take(), plan.stdin.clone()) {
            (Some(mut pipe), Some(input)) => Some(thread::spawn(move || {
                // A command that does not read its input closes the pipe.
                let _ = pipe.write_all(&input);
            })),
            _ => None,
        };
        let bound = plan.limits.output_bytes;
        let stdout = child
            .stdout
            .take()
            .map(|pipe| thread::spawn(move || read_bounded(pipe, bound)));
        let stderr = child
            .stderr
            .take()
            .map(|pipe| thread::spawn(move || read_bounded(pipe, bound)));

        let deadline = Instant::now() + Duration::from_secs(plan.limits.wall_seconds);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    break match status.code() {
                        Some(code) => ExecStatus::Exited(code),
                        None => ExecStatus::Killed,
                    };
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break ExecStatus::TimedOut;
                    }
                    thread::sleep(POLL);
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break ExecStatus::NotStarted(format!(
                        "the sandbox could not be waited for: {error}"
                    ));
                }
            }
        };

        if let Some(writer) = writer {
            let _ = writer.join();
        }
        let (stdout, stdout_truncated) = collect(stdout);
        let (stderr, stderr_truncated) = collect(stderr);

        // bubblewrap reports its own failures on standard error and exits
        // with 1; nothing of the command has run then.
        if status == ExecStatus::Exited(1) && stdout.is_empty() && stderr.starts_with(b"bwrap: ") {
            let reason = String::from_utf8_lossy(&stderr)
                .lines()
                .next()
                .unwrap_or("")
                .to_owned();
            return ExecOutput::not_started(format!("the sandbox could not be set up: {reason}"));
        }

        ExecOutput {
            status,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        }
    }
}

fn collect(reader: Option<thread::JoinHandle<(Vec<u8>, bool)>>) -> (Vec<u8>, bool) {
    reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

/// Read a stream to its end, keeping at most `bound` bytes. Returns what was
/// kept and whether there was more.
fn read_bounded(mut stream: impl Read, bound: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let room = bound.saturating_sub(kept.len());
                if count > room {
                    truncated = true;
                }
                kept.extend_from_slice(&buffer[..count.min(room)]);
            }
        }
    }
    (kept, truncated)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::sandbox::{GrantRules, Limits};

    #[test]
    fn a_stream_is_read_to_its_end_and_kept_up_to_the_bound() {
        let data: Vec<u8> = (0..20_000u32).map(|n| (n % 251) as u8).collect();
        let (kept, truncated) = read_bounded(Cursor::new(data.clone()), 1000);
        assert_eq!(kept, data[..1000]);
        assert!(truncated);

        let (kept, truncated) = read_bounded(Cursor::new(data.clone()), 20_000);
        assert_eq!(kept, data);
        assert!(!truncated);

        let (kept, truncated) = read_bounded(Cursor::new(Vec::new()), 10);
        assert!(kept.is_empty());
        assert!(!truncated);

        let (kept, truncated) = read_bounded(Cursor::new(data), 0);
        assert!(kept.is_empty());
        assert!(truncated);
    }

    #[test]
    fn nothing_runs_when_the_sandbox_program_is_missing() {
        let argv = vec!["/usr/bin/true".to_owned()];
        let mut plan = SandboxPlan::read_only(
            &GrantRules::for_home("/home/ada"),
            &[],
            &argv,
            Limits::default(),
        )
        .expect("a plan");
        plan.program = "/nonexistent/tur-pax-test/bwrap".to_owned();
        let output = BwrapExecutor::new().run(&plan);
        assert_eq!(
            output.status,
            ExecStatus::NotStarted("bubblewrap is not installed".to_owned())
        );
        assert_eq!(output.status.name(), "not_started");
        assert!(output.stdout.is_empty());
    }

    #[test]
    fn every_status_has_its_own_name() {
        let names = [
            ExecStatus::Exited(0).name(),
            ExecStatus::Killed.name(),
            ExecStatus::TimedOut.name(),
            ExecStatus::NotStarted(String::new()).name(),
        ];
        let unique: std::collections::BTreeSet<&str> = names.into_iter().collect();
        assert_eq!(unique.len(), 4);
    }
}
