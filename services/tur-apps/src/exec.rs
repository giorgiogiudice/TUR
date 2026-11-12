// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Running the programs this component drives: `flatpak`, `curl` and
//! `tur-runtime`.
//!
//! A program is always started by its absolute path with an argument list,
//! never through a shell, so no argument is ever interpreted. Every program
//! runs with `LC_ALL=C.UTF-8`, because `flatpak` translates what it prints
//! and formats sizes by the locale, and its output is read here.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;

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
        let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
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
        let mut command = self.command(arguments);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = spawn(&mut command).map_err(|error| {
            Failure::new(
                Code::ProgramMissing,
                format!("cannot start {} ({}): {error}", self.name, self.path.display()),
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
        for (stream, line) in receiver {
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
        for reader in readers {
            let _ = reader.join();
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
    fn a_missing_program_is_reported_with_its_name() {
        let program = Program::new("flatpak", "/nonexistent/flatpak");
        assert!(!program.exists());
        let failure = program.require("to install Linux applications").unwrap_err();
        assert_eq!(failure.code, Code::ProgramMissing);
        assert!(failure.message.contains("flatpak"));
        assert_eq!(
            program.run(&[]).unwrap_err().code,
            Code::ProgramMissing
        );
    }
}
