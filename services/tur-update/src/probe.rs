// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Where the state of this machine comes from, and the one place in this
//! crate that starts a program.
//!
//! [`live_status`] runs exactly one program, `/usr/bin/bootc`, with exactly
//! two arguments, `status --format=json`: a command that prints the
//! deployments and changes nothing. It is run only when the command line
//! says `--live`. Nothing else is run by any code of this crate, and nothing
//! is written; `tests/interfaces.rs` reads the sources to hold that.
//!
//! On an installed system `bootc status` must run as root: bootc enters a
//! private mount namespace to read the deployments, and refuses otherwise
//! (bootc 1.17.1, `crates/lib/src/status.rs` and `cli.rs`). It starts
//! `unshare` for that, by name, so the program is given a fixed search path
//! as well as a fixed locale, and nothing else of this process's
//! surroundings.

use std::fmt;

/// What a program printed and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// The exit status; `None` if a signal ended the program.
    pub status: Option<i32>,
    /// What it wrote to standard output.
    pub stdout: Vec<u8>,
    /// What it wrote to standard error.
    pub stderr: Vec<u8>,
}

/// Runs a program and waits for it. The live read is handed one, so that a
/// test can stand in for the machine and can see exactly what would be run.
pub trait CommandRunner {
    /// Run `program` with `arguments`. No shell is involved.
    ///
    /// # Errors
    ///
    /// The program could not be started.
    fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput, String>;

    /// Whether this process runs as root.
    fn is_root(&self) -> bool;
}

/// The runner for the real machine. It passes nothing of its own
/// surroundings on: the program gets no standard input and an environment
/// of two fixed variables.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput, String> {
        let output = std::process::Command::new(program)
            .args(arguments)
            .stdin(std::process::Stdio::null())
            .env_clear()
            .env("LC_ALL", "C.UTF-8")
            .env("PATH", "/usr/sbin:/usr/bin")
            .output()
            .map_err(|error| error.to_string())?;
        Ok(CommandOutput {
            status: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn is_root(&self) -> bool {
        // The owner of /proc/self is the effective user of this process.
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self").is_ok_and(|metadata| metadata.uid() == 0)
    }
}

/// The one program the live read runs: a full path, never looked up.
pub const STATUS_PROGRAM: &str = "/usr/bin/bootc";

/// Its arguments. `bootc status` prints the deployments and changes nothing.
pub const STATUS_ARGUMENTS: [&str; 2] = ["status", "--format=json"];

/// The most output the live read accepts, in bytes.
pub const STATUS_OUTPUT_MAX: usize = 4 * 1024 * 1024;

/// Why the state of this machine could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// The program could not be run, failed, or printed something unusable.
    Command(String),
    /// The program failed and this process is not root, which `bootc status`
    /// needs on an installed system.
    NeedsAdministrator(String),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeError::Command(message) | ProbeError::NeedsAdministrator(message) => {
                write!(f, "live read: {message}")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

/// The text `bootc status --format=json` prints on this machine.
///
/// Runs [`STATUS_PROGRAM`] with [`STATUS_ARGUMENTS`] through `runner`, once.
/// Nothing else is run and nothing is written.
///
/// # Errors
///
/// `bootc` could not be run, did not exit with 0, printed more than
/// [`STATUS_OUTPUT_MAX`] bytes, or printed text that is not UTF-8. Output
/// from a failed run is never used.
pub fn live_status(runner: &dyn CommandRunner) -> Result<String, ProbeError> {
    let failed = |message: String| {
        if runner.is_root() {
            ProbeError::Command(message)
        } else {
            ProbeError::NeedsAdministrator(message)
        }
    };
    let output = runner
        .run(STATUS_PROGRAM, &STATUS_ARGUMENTS)
        .map_err(|error| {
            ProbeError::Command(format!(
                "{STATUS_PROGRAM} could not be run: {}",
                one_line(&error)
            ))
        })?;
    if output.status != Some(0) {
        let status = output
            .status
            .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"));
        let said = one_line(&String::from_utf8_lossy(&output.stderr));
        return Err(failed(format!(
            "{STATUS_PROGRAM} ended with {status}: {said}"
        )));
    }
    if output.stdout.len() > STATUS_OUTPUT_MAX {
        return Err(ProbeError::Command(format!(
            "{STATUS_PROGRAM} printed more than {STATUS_OUTPUT_MAX} bytes"
        )));
    }
    String::from_utf8(output.stdout).map_err(|_| {
        ProbeError::Command(format!("{STATUS_PROGRAM} printed text that is not UTF-8"))
    })
}

/// The first line of a message, without control characters and at most 200
/// characters long: what a program said, made safe to repeat.
fn one_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .filter(|character| !character.is_control())
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A runner that runs nothing: it records what it was asked to run and
    /// answers with a prepared result.
    struct FakeRunner {
        asked: RefCell<Vec<(String, Vec<String>)>>,
        answer: Result<CommandOutput, String>,
        root: bool,
    }

    impl FakeRunner {
        fn ending(status: Option<i32>, stdout: &[u8], stderr: &[u8], root: bool) -> FakeRunner {
            FakeRunner {
                asked: RefCell::new(Vec::new()),
                answer: Ok(CommandOutput {
                    status,
                    stdout: stdout.to_vec(),
                    stderr: stderr.to_vec(),
                }),
                root,
            }
        }

        fn failing(message: &str) -> FakeRunner {
            FakeRunner {
                asked: RefCell::new(Vec::new()),
                answer: Err(message.to_owned()),
                root: true,
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput, String> {
            self.asked.borrow_mut().push((
                program.to_owned(),
                arguments
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
            ));
            self.answer.clone()
        }

        fn is_root(&self) -> bool {
            self.root
        }
    }

    #[test]
    fn the_live_read_runs_bootc_status_and_nothing_else() {
        let runner = FakeRunner::ending(Some(0), b"{\"kind\":\"BootcHost\"}", b"", true);
        let text = live_status(&runner).expect("the text");
        assert_eq!(text, "{\"kind\":\"BootcHost\"}");
        // One program was run, once: the one that prints the deployments, by
        // its full path, with the arguments that make it print JSON and
        // nothing that makes it change anything.
        let asked = runner.asked.borrow();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].0, "/usr/bin/bootc");
        assert_eq!(asked[0].1, ["status", "--format=json"]);
        // The constants are the whole command line.
        assert_eq!(STATUS_PROGRAM, "/usr/bin/bootc");
        assert_eq!(STATUS_ARGUMENTS, ["status", "--format=json"]);
    }

    #[test]
    fn a_failure_as_root_is_a_command_error() {
        let command = |runner: &FakeRunner| match live_status(runner) {
            Err(ProbeError::Command(message)) => message,
            other => panic!("not a command error: {other:?}"),
        };
        // The program is not there.
        let message = command(&FakeRunner::failing("No such file or directory"));
        assert!(
            message.contains("/usr/bin/bootc could not be run"),
            "{message}"
        );
        // It failed, and what it said is repeated on one line without
        // control characters.
        let message = command(&FakeRunner::ending(
            Some(1),
            b"",
            b"error: \x1b[31mStatus\x07: something\nsecond line\n",
            true,
        ));
        assert!(message.contains("status 1"), "{message}");
        assert!(message.contains("[31mStatus: something"), "{message}");
        assert!(!message.contains('\u{1b}') && !message.contains("second line"));
        // A signal ended it.
        let message = command(&FakeRunner::ending(None, b"", b"", true));
        assert!(message.contains("a signal"), "{message}");
        // It printed something that is not text.
        let message = command(&FakeRunner::ending(Some(0), b"\xff\xfe", b"", true));
        assert!(message.contains("not UTF-8"), "{message}");
        // It printed far too much.
        let flood = vec![b' '; STATUS_OUTPUT_MAX + 1];
        let message = command(&FakeRunner::ending(Some(0), &flood, b"", true));
        assert!(message.contains("printed more than"), "{message}");
        // Output from a failed run is not used, however good it looks.
        let message = command(&FakeRunner::ending(Some(1), b"{}", b"", true));
        assert!(message.contains("status 1"), "{message}");
        assert_eq!(
            ProbeError::Command("x".to_owned()).to_string(),
            "live read: x"
        );
    }

    #[test]
    fn a_failure_without_root_says_what_is_needed() {
        // What the source of bootc 1.17.1 makes it say to a user who is not
        // root (read from the source; not observed).
        let runner = FakeRunner::ending(
            Some(1),
            b"",
            b"error: Status: Preparing for write: Querying root privilege: This command must be executed as the root user\n",
            false,
        );
        match live_status(&runner) {
            Err(ProbeError::NeedsAdministrator(message)) => {
                assert!(message.contains("root user"), "{message}");
            }
            other => panic!("not a needs-administrator error: {other:?}"),
        }
        // A success without root is a success: in a container bootc answers
        // any user.
        let runner = FakeRunner::ending(Some(0), b"{}", b"", false);
        assert_eq!(live_status(&runner).as_deref(), Ok("{}"));
    }
}
