// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The only door between `uos-diag` and the machine it runs on.
//!
//! Everything the collector learns about the host comes through [`Host`]:
//! running a command with a time and size limit, or reading a file with a
//! size limit. [`SystemHost`] does that for real; [`FakeHost`] answers from a
//! table, so tests never depend on the machine that runs them.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// How much of a command's standard error is kept for the missing-data reason.
const STDERR_LIMIT: usize = 4096;
/// How often a running command is checked for completion.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// How long to wait for a finished command's output pipes to close.
const PIPE_GRACE: Duration = Duration::from_secs(2);

/// Bounds on one command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The command is killed when it runs longer than this.
    pub timeout: Duration,
    /// Standard output beyond this many bytes is dropped.
    pub max_bytes: usize,
}

/// What a command that ran to completion produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandOutput {
    /// The exit code, or `None` when the command was ended by a signal.
    pub exit_code: Option<i32>,
    /// Standard output, at most [`Limits::max_bytes`] long.
    pub stdout: Vec<u8>,
    /// The start of standard error.
    pub stderr: Vec<u8>,
    /// Whether standard output was longer than the limit.
    pub truncated: bool,
}

impl CommandOutput {
    /// Whether the command exited with code zero.
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// Why a command produced no result at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The program is not installed or not on the search path.
    NotFound,
    /// The program exists but may not be executed.
    PermissionDenied,
    /// The command ran past its time limit and was killed.
    TimedOut,
    /// Any other failure to start or wait for the command.
    Io(String),
}

/// The content of a file that was read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileContent {
    /// The bytes read, at most the requested limit.
    pub data: Vec<u8>,
    /// Whether the file was longer than the limit.
    pub truncated: bool,
}

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The file does not exist.
    NotFound,
    /// The file exists but may not be read.
    PermissionDenied,
    /// Any other read failure.
    Io(String),
}

/// Access to the machine being diagnosed.
pub trait Host {
    /// Runs `program` with `args`, without a shell, within `limits`.
    fn run(&self, program: &str, args: &[&str], limits: Limits) -> Result<CommandOutput, RunError>;

    /// Reads at most `max_bytes` bytes of the file at `path`.
    fn read_file(&self, path: &str, max_bytes: usize) -> Result<FileContent, ReadError>;
}

/// The real machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemHost;

impl SystemHost {
    /// Creates a handle to the real machine.
    pub fn new() -> Self {
        SystemHost
    }
}

/// Reads `reader` to its end on a separate thread, keeping at most `limit` bytes.
///
/// The reader is always drained, so a command that writes more than the limit
/// is not blocked on a full pipe.
fn drain<R: Read + Send + 'static>(mut reader: R, limit: usize) -> mpsc::Receiver<(Vec<u8>, bool)> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut truncated = false;
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let room = limit.saturating_sub(kept.len());
                    if count > room {
                        truncated = true;
                    }
                    kept.extend_from_slice(&buffer[..count.min(room)]);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        // The receiver may have given up waiting; that is not an error here.
        let _ = sender.send((kept, truncated));
    });
    receiver
}

/// Waits briefly for a drained pipe's content.
fn drained(receiver: Option<mpsc::Receiver<(Vec<u8>, bool)>>) -> (Vec<u8>, bool) {
    receiver
        .and_then(|receiver| receiver.recv_timeout(PIPE_GRACE).ok())
        .unwrap_or_default()
}

impl Host for SystemHost {
    fn run(&self, program: &str, args: &[&str], limits: Limits) -> Result<CommandOutput, RunError> {
        let mut child = Command::new(program)
            .args(args)
            .env("LC_ALL", "C.UTF-8")
            .env("LANG", "C.UTF-8")
            .env("SYSTEMD_COLORS", "0")
            .env("SYSTEMD_PAGER", "")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => RunError::NotFound,
                io::ErrorKind::PermissionDenied => RunError::PermissionDenied,
                _ => RunError::Io(error.to_string()),
            })?;

        let stdout = child.stdout.take().map(|pipe| drain(pipe, limits.max_bytes));
        let stderr = child.stderr.take().map(|pipe| drain(pipe, STDERR_LIMIT));

        let deadline = Instant::now() + limits.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        // Both calls can only fail if the child is already gone.
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(RunError::TimedOut);
                    }
                    thread::sleep(POLL_INTERVAL);
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::Io(error.to_string()));
                }
            }
        };

        let (stdout, truncated) = drained(stdout);
        let (stderr, _) = drained(stderr);
        Ok(CommandOutput {
            exit_code: status.code(),
            stdout,
            stderr,
            truncated,
        })
    }

    fn read_file(&self, path: &str, max_bytes: usize) -> Result<FileContent, ReadError> {
        let convert = |error: io::Error| match error.kind() {
            io::ErrorKind::NotFound => ReadError::NotFound,
            io::ErrorKind::PermissionDenied => ReadError::PermissionDenied,
            _ => ReadError::Io(error.to_string()),
        };
        let file = File::open(path).map_err(convert)?;
        let limit = u64::try_from(max_bytes).unwrap_or(u64::MAX).saturating_add(1);
        let mut data = Vec::new();
        file.take(limit).read_to_end(&mut data).map_err(convert)?;
        let truncated = data.len() > max_bytes;
        data.truncate(max_bytes);
        Ok(FileContent { data, truncated })
    }
}

/// A host that answers from a table. For tests.
///
/// A command is looked up first by its whole command line (program and
/// arguments joined by single spaces), then by program name alone. Anything
/// not in the table behaves as not installed or not present.
#[derive(Debug, Clone, Default)]
pub struct FakeHost {
    commands: BTreeMap<String, Result<CommandOutput, RunError>>,
    files: BTreeMap<String, Result<Vec<u8>, ReadError>>,
}

impl FakeHost {
    /// Creates a host on which no command exists and no file is present.
    pub fn new() -> Self {
        FakeHost::default()
    }

    /// Makes `command` succeed with `stdout`.
    ///
    /// `command` is either a whole command line or a program name.
    pub fn with_command(self, command: &str, stdout: &str) -> Self {
        self.with_command_result(
            command,
            Ok(CommandOutput {
                exit_code: Some(0),
                stdout: stdout.as_bytes().to_vec(),
                stderr: Vec::new(),
                truncated: false,
            }),
        )
    }

    /// Makes `command` exit with `exit_code` and the given standard error.
    pub fn with_failing_command(self, command: &str, exit_code: i32, stderr: &str) -> Self {
        self.with_command_result(
            command,
            Ok(CommandOutput {
                exit_code: Some(exit_code),
                stdout: Vec::new(),
                stderr: stderr.as_bytes().to_vec(),
                truncated: false,
            }),
        )
    }

    /// Makes `command` produce exactly `result`.
    pub fn with_command_result(
        mut self,
        command: &str,
        result: Result<CommandOutput, RunError>,
    ) -> Self {
        self.commands.insert(command.to_owned(), result);
        self
    }

    /// Makes the file at `path` exist with `content`.
    pub fn with_file(mut self, path: &str, content: &str) -> Self {
        self.files
            .insert(path.to_owned(), Ok(content.as_bytes().to_vec()));
        self
    }

    /// Makes reading the file at `path` fail with `error`.
    pub fn with_file_error(mut self, path: &str, error: ReadError) -> Self {
        self.files.insert(path.to_owned(), Err(error));
        self
    }
}

impl Host for FakeHost {
    fn run(&self, program: &str, args: &[&str], limits: Limits) -> Result<CommandOutput, RunError> {
        let mut line = program.to_owned();
        for arg in args {
            line.push(' ');
            line.push_str(arg);
        }
        let found = self
            .commands
            .get(&line)
            .or_else(|| self.commands.get(program));
        let mut output = match found {
            Some(result) => result.clone()?,
            None => return Err(RunError::NotFound),
        };
        if output.stdout.len() > limits.max_bytes {
            output.stdout.truncate(limits.max_bytes);
            output.truncated = true;
        }
        Ok(output)
    }

    fn read_file(&self, path: &str, max_bytes: usize) -> Result<FileContent, ReadError> {
        let mut data = match self.files.get(path) {
            Some(result) => result.clone()?,
            None => return Err(ReadError::NotFound),
        };
        let truncated = data.len() > max_bytes;
        data.truncate(max_bytes);
        Ok(FileContent { data, truncated })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENEROUS: Limits = Limits {
        timeout: Duration::from_secs(60),
        max_bytes: 1 << 20,
    };

    /// The test binary itself: the one program certain to exist wherever the
    /// tests run, so these tests need no particular host tool.
    fn this_test_binary() -> String {
        std::env::current_exe()
            .expect("the test binary has a path")
            .to_string_lossy()
            .into_owned()
    }

    /// Not a test of anything: a process that outlives a short time limit.
    /// Run by `system_host_kills_a_command_that_runs_too_long`.
    #[test]
    #[ignore = "helper process for the timeout test"]
    fn helper_that_sleeps() {
        thread::sleep(Duration::from_secs(120));
    }

    #[test]
    fn system_host_reports_a_missing_program() {
        let result = SystemHost::new().run("uos-diag-no-such-program-4f1c", &[], GENEROUS);
        assert_eq!(result, Err(RunError::NotFound));
    }

    #[test]
    fn system_host_captures_output_and_exit_code() {
        let output = SystemHost::new()
            .run(&this_test_binary(), &["--list"], GENEROUS)
            .expect("the test binary runs");
        assert!(output.success());
        assert!(!output.truncated);
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("system_host_captures_output_and_exit_code"), "{text}");
    }

    #[test]
    fn system_host_truncates_long_output() {
        let limits = Limits {
            timeout: Duration::from_secs(60),
            max_bytes: 16,
        };
        let output = SystemHost::new()
            .run(&this_test_binary(), &["--list"], limits)
            .expect("the test binary runs");
        assert!(output.truncated);
        assert_eq!(output.stdout.len(), 16);
    }

    #[test]
    fn system_host_kills_a_command_that_runs_too_long() {
        let limits = Limits {
            timeout: Duration::from_millis(300),
            max_bytes: 1 << 20,
        };
        let started = Instant::now();
        let result = SystemHost::new().run(
            &this_test_binary(),
            &["--ignored", "--exact", "host::tests::helper_that_sleeps"],
            limits,
        );
        assert_eq!(result, Err(RunError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(60));
    }

    #[test]
    fn system_host_reads_files_with_a_limit() {
        let path = std::env::temp_dir().join(format!("uos-diag-host-{}.txt", std::process::id()));
        std::fs::write(&path, b"0123456789").expect("temporary file is writable");
        let name = path.to_string_lossy().into_owned();
        let host = SystemHost::new();

        let whole = host.read_file(&name, 100).expect("file is readable");
        assert_eq!(whole.data, b"0123456789");
        assert!(!whole.truncated);

        let exact = host.read_file(&name, 10).expect("file is readable");
        assert!(!exact.truncated);

        let part = host.read_file(&name, 4).expect("file is readable");
        assert_eq!(part.data, b"0123");
        assert!(part.truncated);

        std::fs::remove_file(&path).expect("temporary file is removable");
        assert_eq!(host.read_file(&name, 100), Err(ReadError::NotFound));
    }

    #[test]
    fn fake_host_prefers_the_whole_command_line() {
        let host = FakeHost::new()
            .with_command("journalctl", "any boot\n")
            .with_command("journalctl -b -1", "previous boot\n");
        let any = host.run("journalctl", &["-b", "0"], GENEROUS).unwrap();
        assert_eq!(any.stdout, b"any boot\n");
        let previous = host.run("journalctl", &["-b", "-1"], GENEROUS).unwrap();
        assert_eq!(previous.stdout, b"previous boot\n");
    }

    #[test]
    fn fake_host_reports_unknown_commands_and_files_as_absent() {
        let host = FakeHost::new();
        assert_eq!(host.run("lspci", &[], GENEROUS), Err(RunError::NotFound));
        assert_eq!(host.read_file("/proc/cmdline", 10), Err(ReadError::NotFound));
    }

    #[test]
    fn fake_host_applies_limits_and_errors() {
        let host = FakeHost::new()
            .with_command("dmesg", "0123456789")
            .with_failing_command("blkid", 2, "no devices")
            .with_command_result("lsusb", Err(RunError::TimedOut))
            .with_file("/proc/cmdline", "quiet splash")
            .with_file_error("/proc/modules", ReadError::PermissionDenied);
        let limits = Limits {
            timeout: Duration::from_secs(1),
            max_bytes: 4,
        };
        let dmesg = host.run("dmesg", &[], limits).unwrap();
        assert_eq!(dmesg.stdout, b"0123");
        assert!(dmesg.truncated);
        let blkid = host.run("blkid", &[], limits).unwrap();
        assert!(!blkid.success());
        assert_eq!(blkid.exit_code, Some(2));
        assert_eq!(host.run("lsusb", &[], limits), Err(RunError::TimedOut));
        let cmdline = host.read_file("/proc/cmdline", 5).unwrap();
        assert_eq!(cmdline.data, b"quiet");
        assert!(cmdline.truncated);
        assert_eq!(
            host.read_file("/proc/modules", 5),
            Err(ReadError::PermissionDenied)
        );
    }
}
