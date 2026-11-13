// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Running the programs this component drives: `flatpak`, `curl` and
//! `tur-runtime`.
//!
//! A program is always started by its absolute path with an argument list,
//! never through a shell, so no argument is ever interpreted. Every program
//! runs with `LC_ALL=C.UTF-8`, because `flatpak` translates what it prints
//! and formats sizes by the locale, and its output is read here.

use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::codes::{Code, Failure};

/// The most output kept from one stream of a program, in bytes. What is
/// beyond it is dropped, and the record of the output says so.
pub const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

/// What a finished program left.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The exit status, or `None` when the program was ended by a signal.
    pub status: Option<i32>,
    /// What it printed on standard output.
    pub stdout: String,
    /// What it printed on standard error.
    pub stderr: String,
    /// Whether it was stopped because a cancel was asked for
    /// ([`Program::run_streaming_until`]).
    pub stopped: bool,
}

impl Outcome {
    /// Whether the program exited with status 0.
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }

    /// The last lines of standard error, or of standard output when
    /// standard error is empty, for a message.
    pub fn tail(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let start = lines.len().saturating_sub(6);
        lines[start..].join(" | ")
    }
}

/// Which stream a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// A program and how to start it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// Its absolute path.
    pub path: PathBuf,
    /// What it is, for messages: `flatpak`, `curl`, `tur-runtime`.
    pub name: &'static str,
}

impl Program {
    /// A program at `path`.
    pub fn new(name: &'static str, path: impl Into<PathBuf>) -> Self {
        Program {
            path: path.into(),
            name,
        }
    }

    /// Whether the program is there to be run.
    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    /// Refuses when the program is not there.
    pub fn require(&self, why: &str) -> Result<(), Failure> {
        if self.exists() {
            Ok(())
        } else {
            Err(Failure::new(
                Code::ProgramMissing,
                format!(
                    "{} is needed {why}, and it is not installed at {}",
                    self.name,
                    self.path.display()
                ),
            ))
        }
    }

    fn command(&self, arguments: &[String]) -> Command {
        let mut command = Command::new(&self.path);
        command
            .args(arguments)
            .env("LC_ALL", "C.UTF-8")
            .env("LANG", "C.UTF-8")
            .stdin(Stdio::null());
        command
    }

    /// Runs the program to the end and returns what it printed.
    pub fn run(&self, arguments: &[String]) -> Result<Outcome, Failure> {
        self.run_streaming(arguments, &mut |_, _| {})
    }

    /// Runs the program to the end, handing each line it prints to
    /// `on_line` as it comes, and returns everything it printed.
    pub fn run_streaming(
        &self,
        arguments: &[String],
        on_line: &mut dyn FnMut(Stream, &str),
    ) -> Result<Outcome, Failure> {
        self.run_with(arguments, on_line, None)
    }

    /// Like [`Program::run_streaming`], but the program is stopped, with
    /// everything it started, as soon as `stop` says so. It then runs in a
    /// process group of its own, which is ended with SIGTERM and, five
    /// seconds later, SIGKILL. The outcome says whether it was stopped.
    pub fn run_streaming_until(
        &self,
        arguments: &[String],
        on_line: &mut dyn FnMut(Stream, &str),
        stop: &dyn Fn() -> bool,
    ) -> Result<Outcome, Failure> {
        self.run_with(arguments, on_line, Some(stop))
    }

    fn run_with(
        &self,
        arguments: &[String],
        on_line: &mut dyn FnMut(Stream, &str),
        stop: Option<&dyn Fn() -> bool>,
    ) -> Result<Outcome, Failure> {
        let mut command = self.command(arguments);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        if stop.is_some() {
            command.process_group(0);
        }
        let mut child = spawn(&mut command).map_err(|error| {
            Failure::new(
                Code::ProgramMissing,
                format!(
                    "cannot start {} ({}): {error}",
                    self.name,
                    self.path.display()
                ),
            )
        })?;
        let (sender, receiver) = mpsc::channel::<(Stream, String)>();
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(forward(stdout, Stream::Stdout, sender.clone()));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(forward(stderr, Stream::Stderr, sender.clone()));
        }
        drop(sender);

        let mut outcome = Outcome::default();
        // Once the program is stopped, what its group still prints is read
        // for a while, and then no longer waited for.
        let mut give_up: Option<Instant> = None;
        loop {
            if !outcome.stopped && stop.is_some_and(|stop| stop()) {
                outcome.stopped = true;
                terminate(&mut child);
                give_up = Some(Instant::now() + Duration::from_secs(10));
            }
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok((stream, line)) => {
                    on_line(stream, &line);
                    let target = match stream {
                        Stream::Stdout => &mut outcome.stdout,
                        Stream::Stderr => &mut outcome.stderr,
                    };
                    if target.len() + line.len() < MAX_OUTPUT_BYTES {
                        target.push_str(&line);
                        target.push('\n');
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if give_up.is_some_and(|deadline| Instant::now() > deadline) {
                        break;
                    }
                }
            }
        }
        if !outcome.stopped {
            for reader in readers {
                let _ = reader.join();
            }
        }
        let status = child.wait().map_err(|error| {
            Failure::new(
                Code::StepFailed,
                format!("lost track of {}: {error}", self.name),
            )
        })?;
        outcome.status = status.code();
        Ok(outcome)
    }
}

/// Ends a program started in a process group of its own, and everything it
/// started: SIGTERM to the group, then SIGKILL five seconds later if the
/// program has not ended. The signals are sent by the system's `kill`
/// program: this crate does not call the C library to send them itself.
fn terminate(child: &mut Child) {
    let group = format!("-{}", child.id());
    let kill = find(&["/usr/bin/kill", "/bin/kill"]);
    let signal = |name: &str| {
        let _ = Command::new(&kill)
            .args([name, "--", group.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    };
    signal("-TERM");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    signal("-KILL");
    let _ = child.kill();
}

/// Starts a command, trying again for a moment when the program file is
/// still open for writing somewhere (`ETXTBSY`): a file another thread has
/// just written can be held open briefly by a child that thread forked.
pub fn spawn(command: &mut Command) -> std::io::Result<Child> {
    const ETXTBSY: i32 = 26;
    let mut attempts = 0;
    loop {
        match command.spawn() {
            Err(error) if error.raw_os_error() == Some(ETXTBSY) && attempts < 50 => {
                attempts += 1;
                thread::sleep(std::time::Duration::from_millis(20));
            }
            result => return result,
        }
    }
}

/// Reads lines from a stream on a thread of its own and sends them on.
fn forward(
    stream: impl Read + Send + 'static,
    which: Stream,
    sender: mpsc::Sender<(Stream, String)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buffer);
                    let line = line.trim_end_matches(['\n', '\r']).to_owned();
                    if sender.send((which, line)).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

/// The first of `candidates` that is a file, or the first candidate when
/// none is: the program a command line names by default.
pub fn find(candidates: &[&str]) -> PathBuf {
    candidates
        .iter()
        .map(Path::new)
        .find(|path| path.is_file())
        .unwrap_or_else(|| Path::new(candidates[0]))
        .to_path_buf()
}

/// Turns a list of string slices into owned arguments.
pub fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|argument| (*argument).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_s_lines_are_streamed_and_kept() {
        let program = Program::new("sh", "/bin/sh");
        let mut seen = Vec::new();
        let outcome = program
            .run_streaming(
                &args(&["-c", "echo one; echo two >&2; echo \"$LC_ALL\"; exit 3"]),
                &mut |stream, line| seen.push((stream, line.to_owned())),
            )
            .unwrap();
        assert_eq!(outcome.status, Some(3));
        assert!(!outcome.success());
        assert_eq!(outcome.stdout, "one\nC.UTF-8\n");
        assert_eq!(outcome.stderr, "two\n");
        assert!(seen.contains(&(Stream::Stdout, "one".to_owned())));
        assert!(seen.contains(&(Stream::Stderr, "two".to_owned())));
        assert_eq!(outcome.tail(), "two");
    }

    #[test]
    fn a_program_is_stopped_with_what_it_started_when_asked() {
        let program = Program::new("sh", "/bin/sh");
        let seen_one = std::cell::Cell::new(false);
        let started = Instant::now();
        let outcome = program
            .run_streaming_until(
                &args(&["-c", "echo one; sleep 60; echo two"]),
                &mut |_, line| {
                    if line == "one" {
                        seen_one.set(true);
                    }
                },
                &|| seen_one.get(),
            )
            .unwrap();
        assert!(outcome.stopped);
        assert!(!outcome.success());
        assert_eq!(outcome.stdout, "one\n");
        // The sleep the shell started was ended too, or the pipe would have
        // stayed open for a minute.
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{:?}",
            started.elapsed()
        );

        let outcome = program
            .run_streaming_until(&args(&["-c", "echo done"]), &mut |_, _| {}, &|| false)
            .unwrap();
        assert!(!outcome.stopped);
        assert!(outcome.success());
    }

    #[test]
    fn a_missing_program_is_reported_with_its_name() {
        let program = Program::new("flatpak", "/nonexistent/flatpak");
        assert!(!program.exists());
        let failure = program
            .require("to install Linux applications")
            .unwrap_err();
        assert_eq!(failure.code, Code::ProgramMissing);
        assert!(failure.message.contains("flatpak"));
        assert_eq!(program.run(&[]).unwrap_err().code, Code::ProgramMissing);
    }
}
