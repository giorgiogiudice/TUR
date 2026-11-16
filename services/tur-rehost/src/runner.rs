// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Starting programs: the one place `tur-rehost` runs anything.
//!
//! Every program is described as an [`Invocation`] first, so that a plan
//! can print it and a test can record it; [`SystemRunner`] is the only code
//! that starts a process. A program gets exactly the environment its
//! invocation names, nothing inherited, and a time limit; at most 64 KiB
//! of its output is kept.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Output kept from one program: the last 64 KiB of standard output and
/// standard error together.
pub const OUTPUT_KEPT: usize = 64 * 1024;

/// The directories programs are looked for in. Not the caller's `PATH`:
/// what is found here is also what the sandbox shows.
pub const PROGRAM_DIRECTORIES: [&str; 3] = ["/usr/bin", "/usr/local/bin", "/bin"];

/// One program to start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Invocation {
    /// What it is for, in a few words, for the log and the plan.
    pub purpose: String,
    /// The program.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its whole environment.
    pub env: Vec<(String, String)>,
    /// Its working directory.
    pub cwd: Option<PathBuf>,
    /// How long it may run, in seconds.
    pub timeout_seconds: u64,
}

impl Invocation {
    /// The command line, for people: the program and its arguments, each
    /// quoted when it holds a space.
    pub fn display(&self) -> String {
        let mut words = vec![self.program.display().to_string()];
        words.extend(self.args.iter().cloned());
        words
            .iter()
            .map(|word| {
                if word.contains(' ') || word.is_empty() {
                    format!("'{word}'")
                } else {
                    word.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What came of running a program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    /// Its exit status; `None` when it was killed by a signal or by the
    /// time limit.
    pub status: Option<i32>,
    /// Whether the time limit stopped it.
    pub timed_out: bool,
    /// The last [`OUTPUT_KEPT`] bytes of its output, as text.
    pub output: String,
}

impl Outcome {
    /// Whether the program exited with status 0.
    pub fn succeeded(&self) -> bool {
        self.status == Some(0) && !self.timed_out
    }

    /// The last line of the output, for a message.
    pub fn last_line(&self) -> &str {
        self.output
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
    }
}

/// Something that starts programs.
pub trait Runner {
    /// Runs `invocation` to completion or to its time limit.
    fn run(&mut self, invocation: &Invocation) -> std::io::Result<Outcome>;
}

/// The runner that really starts processes.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&mut self, invocation: &Invocation) -> std::io::Result<Outcome> {
        let mut command = Command::new(&invocation.program);
        command
            .args(&invocation.args)
            .env_clear()
            .envs(invocation.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(directory) = &invocation.cwd {
            command.current_dir(directory);
        }
        let mut child = command.spawn()?;
        let kept = Arc::new(Mutex::new(Vec::<u8>::new()));
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(spawn_reader(stdout, Arc::clone(&kept)));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(spawn_reader(stderr, Arc::clone(&kept)));
        }
        let deadline = Instant::now() + Duration::from_secs(invocation.timeout_seconds.max(1));
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                timed_out = true;
                let _ = child.kill();
                break child.wait()?;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        for reader in readers {
            let _ = reader.join();
        }
        let bytes = kept
            .lock()
            .map(|buffer| buffer.to_vec())
            .unwrap_or_default();
        Ok(Outcome {
            status: if timed_out { None } else { status.code() },
            timed_out,
            output: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut source: R,
    kept: Arc<Mutex<Vec<u8>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match source.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut all) = kept.lock() {
                        all.extend_from_slice(&buffer[..read]);
                        if all.len() > OUTPUT_KEPT {
                            let excess = all.len() - OUTPUT_KEPT;
                            all.drain(..excess);
                        }
                    }
                }
            }
        }
    })
}

/// A runner for tests: records every invocation and answers with what
/// `answer` returns, starting nothing.
pub struct RecordingRunner {
    /// Every invocation, in order.
    pub calls: Vec<Invocation>,
    answer: Box<dyn FnMut(&Invocation) -> Outcome>,
}

impl RecordingRunner {
    /// A runner that answers every invocation with `answer`.
    pub fn new(answer: impl FnMut(&Invocation) -> Outcome + 'static) -> Self {
        RecordingRunner {
            calls: Vec::new(),
            answer: Box::new(answer),
        }
    }

    /// A runner whose every program succeeds with no output.
    pub fn succeeding() -> Self {
        RecordingRunner::new(|_| Outcome {
            status: Some(0),
            timed_out: false,
            output: String::new(),
        })
    }
}

impl Runner for RecordingRunner {
    fn run(&mut self, invocation: &Invocation) -> std::io::Result<Outcome> {
        self.calls.push(invocation.clone());
        Ok((self.answer)(invocation))
    }
}

/// The first of `names` found as a regular, executable file in
/// [`PROGRAM_DIRECTORIES`].
pub fn find_program(names: &[&str]) -> Option<PathBuf> {
    find_program_in(names, &PROGRAM_DIRECTORIES.map(Path::new))
}

/// [`find_program`] in other directories (tests).
pub fn find_program_in(names: &[&str], directories: &[&Path]) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    for name in names {
        for directory in directories {
            let path = directory.join(name);
            if let Ok(metadata) = std::fs::metadata(&path)
                && metadata.is_file()
                && metadata.permissions().mode() & 0o111 != 0
            {
                return Some(path);
            }
        }
    }
    None
}

/// Text from a path, refusing a path that is not valid UTF-8: every path
/// that reaches a command line or a record is text.
pub fn path_text(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_is_shown_with_its_spaces_quoted() {
        let invocation = Invocation {
            purpose: "test".to_owned(),
            program: PathBuf::from("/usr/bin/bsdtar"),
            args: vec!["-xf".to_owned(), "/tmp/a b.zip".to_owned()],
            env: Vec::new(),
            cwd: None,
            timeout_seconds: 5,
        };
        assert_eq!(invocation.display(), "/usr/bin/bsdtar -xf '/tmp/a b.zip'");
    }

    #[test]
    fn the_system_runner_passes_only_the_given_environment_and_keeps_the_output() {
        let Some(shell) = find_program(&["sh"]) else {
            return;
        };
        let mut runner = SystemRunner;
        let outcome = runner
            .run(&Invocation {
                purpose: "test".to_owned(),
                program: shell,
                args: vec![
                    "-c".to_owned(),
                    "echo \"[$TUR_TEST_VALUE][$HOME]\"; exit 3".to_owned(),
                ],
                env: vec![("TUR_TEST_VALUE".to_owned(), "given".to_owned())],
                cwd: None,
                timeout_seconds: 10,
            })
            .unwrap();
        assert_eq!(outcome.status, Some(3));
        assert!(!outcome.succeeded());
        assert_eq!(outcome.last_line(), "[given][]");
    }

    #[test]
    fn the_time_limit_stops_a_program() {
        let Some(shell) = find_program(&["sh"]) else {
            return;
        };
        let started = Instant::now();
        let outcome = SystemRunner
            .run(&Invocation {
                purpose: "test".to_owned(),
                program: shell,
                args: vec!["-c".to_owned(), "sleep 30".to_owned()],
                env: Vec::new(),
                cwd: None,
                timeout_seconds: 1,
            })
            .unwrap();
        assert!(outcome.timed_out);
        assert_eq!(outcome.status, None);
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn programs_are_looked_for_in_fixed_directories_only() {
        assert!(find_program_in(&["no-such-program-anywhere"], &[Path::new("/usr/bin")]).is_none());
    }
}
