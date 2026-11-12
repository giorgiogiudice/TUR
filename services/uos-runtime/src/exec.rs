// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Carrying a plan out for real.
//!
//! [`SystemExecutor`] is the [`Executor`] that touches the system: it creates
//! directories, writes files, deletes and renames inside the runtime
//! manager's own directories only, and starts the commands a plan names with
//! exactly the environment the plan gives them.
//!
//! [`supervise`] is the small job this program does inside the sandbox: start
//! Wine, wait for it, then wait for the Wine server to write the registry and
//! exit, so that the sandbox is not torn down under it.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::launch_host::current_effective_uid;
use crate::limits::{MAX_CHECK_BYTES, MAX_ERROR_OUTPUT_BYTES, MAX_LOG_BYTES};
use crate::paths::{RuntimeDirs, plain_text};
use crate::plan::{Check, CommandSpec, Executor, Step, StepError, StepOutcome};
use crate::timefmt::Timestamp;

/// Where a launched application's output goes besides its log file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forward {
    /// Only to the log file.
    LogOnly,
    /// Also to this process's standard output and standard error.
    Inherit,
}

/// The executor that carries plans out on the running system.
pub struct SystemExecutor {
    roots: Vec<PathBuf>,
    command_log: Option<PathBuf>,
    forward: Forward,
    last_output: String,
}

impl SystemExecutor {
    /// An executor that deletes and renames only inside `dirs`.
    pub fn new(dirs: &RuntimeDirs) -> Self {
        SystemExecutor {
            roots: vec![
                dirs.data_root().to_path_buf(),
                dirs.state_root().to_path_buf(),
            ],
            command_log: None,
            forward: Forward::LogOnly,
            last_output: String::new(),
        }
    }

    /// Appends the output of every command a plan runs to `path`.
    #[must_use]
    pub fn with_command_log(mut self, path: PathBuf) -> Self {
        self.command_log = Some(path);
        self
    }

    /// Chooses where a launched application's output goes besides its log.
    #[must_use]
    pub fn forwarding(mut self, forward: Forward) -> Self {
        self.forward = forward;
        self
    }

    /// The end of the output of the most recent command, for error messages.
    pub fn last_output(&self) -> &str {
        &self.last_output
    }

    /// Refuses a path that is not strictly inside one of the roots, at least
    /// two levels down: an application's directory, or something inside one.
    fn guard(&self, path: &Path) -> Result<(), StepError> {
        let inside = plain_text(path).is_some()
            && self.roots.iter().any(|root| {
                path.strip_prefix(root)
                    .is_ok_and(|rest| rest.components().count() >= 2)
            });
        if inside {
            Ok(())
        } else {
            Err(StepError::new(format!(
                "refusing to delete or move {}: it is not inside an application's directory",
                path.display()
            )))
        }
    }

    fn command(spec: &CommandSpec) -> Command {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.arguments)
            .env_clear()
            .envs(&spec.environment)
            .stdin(Stdio::null());
        if let Some(directory) = &spec.working_directory {
            command.current_dir(directory);
        }
        command
    }

    fn append_to_command_log(&self, heading: &str, output: &[u8]) {
        let Some(path) = &self.command_log else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = DirBuilder::new().recursive(true).mode(0o700).create(parent);
        }
        let opened = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path);
        // The log is a convenience; failing to write it does not fail the step.
        if let Ok(mut file) = opened {
            let _ = writeln!(file, "# {} {heading}", Timestamp::now());
            let _ = file.write_all(output);
            if !output.ends_with(b"\n") && !output.is_empty() {
                let _ = file.write_all(b"\n");
            }
        }
    }

    fn run(&mut self, label: &str, spec: &CommandSpec) -> Result<StepOutcome, StepError> {
        let output = Self::command(spec)
            .output()
            .map_err(|error| StepError::new(format!("cannot start {}: {error}", spec.program)))?;
        let mut combined = output.stdout;
        combined.extend_from_slice(&output.stderr);
        let status = exit_code(output.status);
        self.append_to_command_log(
            &format!("{label}: {} (exit status {status})", spec.display_line()),
            &combined,
        );
        let tail_from = combined.len().saturating_sub(MAX_ERROR_OUTPUT_BYTES);
        self.last_output = printable(combined.get(tail_from..).unwrap_or_default());
        Ok(StepOutcome::Exited { status })
    }

    fn launch(&mut self, spec: &CommandSpec, log: &Path) -> Result<StepOutcome, StepError> {
        let failed =
            |what: &str, error: io::Error| StepError::new(format!("cannot {what}: {error}"));
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(log)
            .map_err(|error| failed("open the log file", error))?;
        let names: Vec<&str> = spec.environment.keys().map(String::as_str).collect();
        let header = format!(
            "# uos-runtime launch, {}\n# command: {}\n# environment: {}\n",
            Timestamp::now(),
            spec.display_line(),
            names.join(" ")
        );
        file.write_all(header.as_bytes())
            .map_err(|error| failed("write the log file", error))?;

        let mut command = Self::command(spec);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| failed("start the sandbox", error))?;

        let sink = Arc::new(Mutex::new(LogSink {
            file,
            remaining: MAX_LOG_BYTES,
        }));
        let forward = self.forward;
        let copiers = [
            child.stdout.take().map(|stream| {
                let sink = Arc::clone(&sink);
                thread::spawn(move || copy_output(stream, &sink, forward, false))
            }),
            child.stderr.take().map(|stream| {
                let sink = Arc::clone(&sink);
                thread::spawn(move || copy_output(stream, &sink, forward, true))
            }),
        ];
        let status = child
            .wait()
            .map_err(|error| failed("wait for the application", error))?;
        for copier in copiers.into_iter().flatten() {
            let _ = copier.join();
        }
        let status = exit_code(status);
        if let Ok(mut sink) = sink.lock() {
            let _ = writeln!(sink.file, "# exit status: {status}, {}", Timestamp::now());
        }
        Ok(StepOutcome::Exited { status })
    }
}

/// A log file with a budget: what an application prints is untrusted and
/// unbounded.
struct LogSink {
    file: File,
    remaining: usize,
}

impl LogSink {
    fn write(&mut self, bytes: &[u8]) {
        if self.remaining == 0 {
            return;
        }
        let kept = bytes.get(..self.remaining).unwrap_or(bytes);
        self.remaining -= kept.len();
        let _ = self.file.write_all(kept);
        if self.remaining == 0 {
            let _ = self
                .file
                .write_all(b"\n# the log is full; later output was not recorded\n");
        }
    }
}

fn copy_output(mut stream: impl Read, sink: &Mutex<LogSink>, forward: Forward, is_error: bool) {
    let mut buffer = [0u8; 8192];
    loop {
        let read = match stream.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let bytes = buffer.get(..read).unwrap_or_default();
        if let Ok(mut sink) = sink.lock() {
            sink.write(bytes);
        }
        if forward == Forward::Inherit {
            // A closed pipe on our side must not stop the application's
            // output from being drained and logged.
            if is_error {
                let mut out = io::stderr();
                let _ = out.write_all(bytes);
                let _ = out.flush();
            } else {
                let mut out = io::stdout();
                let _ = out.write_all(bytes);
                let _ = out.flush();
            }
        }
    }
}

/// The exit status as one number: the exit code, or 128 plus the signal
/// number when a signal ended the process.
fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

/// Output as text without control characters other than newlines and tabs,
/// so that it can be shown on a terminal.
fn printable(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|character| {
            if character.is_control() && character != '\n' && character != '\t' {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

fn check(check: &Check) -> StepOutcome {
    let found = |detail: String| StepOutcome::Checked {
        passed: false,
        detail,
    };
    let passed = StepOutcome::Checked {
        passed: true,
        detail: String::new(),
    };
    let (path, wanted) = match check {
        Check::Directory { path } => (path, "a directory"),
        Check::File { path } | Check::FileContains { path, .. } => (path, "a regular file"),
        Check::OwnedByUser { path } => (path, "there"),
    };
    // Links are not followed: a link where a file or directory should be is
    // a finding.
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => return found(format!("{} cannot be examined: {error}", path.display())),
    };
    let right_kind = match check {
        Check::Directory { .. } => metadata.is_dir(),
        Check::File { .. } | Check::FileContains { .. } => metadata.is_file(),
        Check::OwnedByUser { .. } => true,
    };
    if !right_kind {
        return found(format!("{} is not {wanted}", path.display()));
    }
    match check {
        Check::Directory { .. } | Check::File { .. } => passed,
        Check::OwnedByUser { .. } => {
            if Some(metadata.uid()) == current_effective_uid() {
                passed
            } else {
                found(format!(
                    "{} belongs to user {}",
                    path.display(),
                    metadata.uid()
                ))
            }
        }
        Check::FileContains { text, .. } => {
            let limit = u64::try_from(MAX_CHECK_BYTES).unwrap_or(u64::MAX);
            let mut head = Vec::new();
            let read = File::open(path).and_then(|file| file.take(limit).read_to_end(&mut head));
            match read {
                Err(error) => found(format!("{} cannot be read: {error}", path.display())),
                Ok(_) if String::from_utf8_lossy(&head).contains(text.as_str()) => passed,
                Ok(_) => found(format!("{} does not contain {text:?}", path.display())),
            }
        }
    }
}

fn write_file(path: &Path, contents: &str) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("the path has no file name"))?;
    let mut temporary_name = std::ffi::OsString::from(".");
    temporary_name.push(name);
    temporary_name.push(format!(".tmp-{}", std::process::id()));
    let temporary = path.with_file_name(temporary_name);
    // A stale temporary file from an interrupted write is replaced.
    let _ = fs::remove_file(&temporary);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let written = file
        .write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

fn remove_tree(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        // A link is removed, never followed.
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
    }
}

impl Executor for SystemExecutor {
    fn execute(&mut self, step: &Step) -> Result<StepOutcome, StepError> {
        let io_error = |action: &str, path: &Path, error: io::Error| {
            StepError::new(format!("cannot {action} {}: {error}", path.display()))
        };
        match step {
            Step::CreateDir { path } => DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)
                .map(|()| StepOutcome::Done)
                .map_err(|error| io_error("create", path, error)),
            Step::WriteFile { path, contents } => write_file(path, contents)
                .map(|()| StepOutcome::Done)
                .map_err(|error| io_error("write", path, error)),
            Step::RemoveTree { path } => {
                self.guard(path)?;
                remove_tree(path)
                    .map(|()| StepOutcome::Done)
                    .map_err(|error| io_error("delete", path, error))
            }
            Step::Rename { from, to } => {
                self.guard(from)?;
                self.guard(to)?;
                fs::rename(from, to)
                    .map(|()| StepOutcome::Done)
                    .map_err(|error| io_error("rename", from, error))
            }
            Step::Run { label, command } => self.run(label, command),
            Step::Launch { command, log } => self.launch(command, log),
            Step::Check { check: wanted } | Step::Require { check: wanted } => Ok(check(wanted)),
        }
    }
}

/// Runs `program` and waits for it, then runs `settle --wait` so that the
/// Wine server has written the registry and gone before this process exits.
/// Returns the exit status of `program`.
///
/// The environment and the standard streams are inherited: this runs inside
/// the sandbox, where the environment is already the one the plan built.
///
/// # Errors
///
/// A [`StepError`] when `program` cannot be started.
pub fn supervise(
    settle: Option<&Path>,
    program: &str,
    arguments: &[String],
) -> Result<i32, StepError> {
    let status = Command::new(program)
        .args(arguments)
        .status()
        .map_err(|error| StepError::new(format!("cannot start {program}: {error}")))?;
    if let Some(settle) = settle {
        // Whether the server was still there or not does not change the
        // application's result.
        let _ = Command::new(settle)
            .arg("--wait")
            .stdin(Stdio::null())
            .status();
    }
    Ok(exit_code(status))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;
    use crate::plan::{Operation, Plan, run_plan};
    use crate::testdata::TempDir;

    struct Scratch {
        _temp: TempDir,
        dirs: RuntimeDirs,
        app: PathBuf,
    }

    fn scratch(label: &str) -> Scratch {
        let temp = TempDir::new(label);
        let dirs = RuntimeDirs::new(&temp.path().join("data"), &temp.path().join("state"));
        let app = dirs.prefixes().join("com.example.app");
        Scratch {
            _temp: temp,
            dirs,
            app,
        }
    }

    fn shell(script: &str) -> CommandSpec {
        let mut environment = BTreeMap::new();
        environment.insert("PATH".to_owned(), "/usr/bin:/bin".to_owned());
        environment.insert("UOS_TEST".to_owned(), "from the plan".to_owned());
        CommandSpec {
            program: "/bin/sh".to_owned(),
            arguments: vec!["-c".to_owned(), script.to_owned()],
            environment,
            working_directory: None,
        }
    }

    fn done(executor: &mut SystemExecutor, step: Step) {
        assert_eq!(executor.execute(&step), Ok(StepOutcome::Done), "{step:?}");
    }

    #[test]
    fn directories_and_files_are_private_to_the_user() {
        let scratch = scratch("exec-files");
        let mut executor = SystemExecutor::new(&scratch.dirs);
        let prefix = scratch.app.join("prefix");
        done(&mut executor, Step::CreateDir { path: prefix.clone() });
        // Creating it again is not an error.
        done(&mut executor, Step::CreateDir { path: prefix.clone() });
        let mode = fs::metadata(&prefix).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        let metadata = scratch.app.join("metadata.json");
        for contents in ["first\n", "second, longer\n", "3\n"] {
            done(
                &mut executor,
                Step::WriteFile {
                    path: metadata.clone(),
                    contents: contents.to_owned(),
                },
            );
            assert_eq!(fs::read_to_string(&metadata).unwrap(), contents);
        }
        let mode = fs::metadata(&metadata).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        // No temporary file is left behind.
        let names: Vec<String> = fs::read_dir(&scratch.app)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");

        // Writing into a directory that does not exist is an error.
        let error = executor
            .execute(&Step::WriteFile {
                path: scratch.app.join("missing/file"),
                contents: String::new(),
            })
            .unwrap_err();
        assert!(error.message.starts_with("cannot write "));
    }

    #[test]
    fn deleting_and_renaming_stay_inside_application_directories() {
        let scratch = scratch("exec-guard");
        let mut executor = SystemExecutor::new(&scratch.dirs);
        let prefix = scratch.app.join("prefix");
        fs::create_dir_all(prefix.join("drive_c")).unwrap();
        fs::write(prefix.join("system.reg"), b"x").unwrap();

        // A victim outside the roots, and a link to it inside the prefix.
        let outside = scratch.dirs.data_root().parent().unwrap().join("victim");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious"), b"keep").unwrap();
        symlink(&outside, prefix.join("link")).unwrap();

        for refused in [
            outside.clone(),
            PathBuf::from("/"),
            PathBuf::from("/tmp"),
            scratch.dirs.data_root().to_path_buf(),
            scratch.dirs.prefixes(),
            scratch.dirs.state_root().to_path_buf(),
            scratch.app.join("../../../victim"),
            PathBuf::from("relative/path"),
        ] {
            let error = executor
                .execute(&Step::RemoveTree {
                    path: refused.clone(),
                })
                .unwrap_err();
            assert!(error.message.starts_with("refusing to delete"), "{refused:?}");
            for (from, to) in [(&refused, &prefix), (&prefix, &refused)] {
                let error = executor
                    .execute(&Step::Rename {
                        from: from.clone(),
                        to: to.clone(),
                    })
                    .unwrap_err();
                assert!(error.message.starts_with("refusing to delete or move"));
            }
        }
        assert!(prefix.join("system.reg").exists());

        // Deleting the prefix removes the link, not what it points to.
        done(&mut executor, Step::RemoveTree { path: prefix.clone() });
        assert!(!prefix.exists());
        assert_eq!(fs::read(outside.join("precious")).unwrap(), b"keep");
        // Deleting something that is not there is not an error.
        done(&mut executor, Step::RemoveTree { path: prefix.clone() });

        // A link in place of a directory is removed as a link.
        symlink(&outside, &prefix).unwrap();
        done(&mut executor, Step::RemoveTree { path: prefix.clone() });
        assert!(fs::symlink_metadata(&prefix).is_err());
        assert!(outside.join("precious").exists());

        let moved = scratch.app.join("moved");
        fs::create_dir_all(&prefix).unwrap();
        done(
            &mut executor,
            Step::Rename {
                from: prefix.clone(),
                to: moved.clone(),
            },
        );
        assert!(moved.is_dir());
    }

    #[test]
    fn commands_get_exactly_the_planned_environment() {
        let scratch = scratch("exec-run");
        let log = scratch.dirs.state_root().join("operations.log");
        fs::create_dir_all(scratch.dirs.state_root()).unwrap();
        let mut executor = SystemExecutor::new(&scratch.dirs).with_command_log(log.clone());

        let outcome = executor
            .execute(&Step::Run {
                label: "probe".to_owned(),
                command: shell("echo \"[$UOS_TEST]\"; echo \"home=[$HOME]\"; env | wc -l"),
            })
            .unwrap();
        assert_eq!(outcome, StepOutcome::Exited { status: 0 });
        let output = executor.last_output().to_owned();
        assert!(output.contains("[from the plan]"), "{output}");
        // HOME is set for the test process and is not in the plan.
        assert!(output.contains("home=[]"), "{output}");

        let outcome = executor
            .execute(&Step::Run {
                label: "failing".to_owned(),
                command: shell("echo problem >&2; exit 3"),
            })
            .unwrap();
        assert_eq!(outcome, StepOutcome::Exited { status: 3 });
        assert!(executor.last_output().contains("problem"));

        let logged = fs::read_to_string(&log).unwrap();
        assert!(logged.contains("probe: /bin/sh -c "));
        assert!(logged.contains("(exit status 0)"));
        assert!(logged.contains("failing: /bin/sh -c "));
        assert!(logged.contains("(exit status 3)"));
        assert!(logged.contains("problem\n"));

        let error = executor
            .execute(&Step::Run {
                label: "missing".to_owned(),
                command: CommandSpec {
                    program: "/nonexistent/program".to_owned(),
                    ..shell("")
                },
            })
            .unwrap_err();
        assert!(error.message.starts_with("cannot start /nonexistent/program"));
    }

    #[test]
    fn a_launch_is_logged_and_its_exit_status_returned() {
        let scratch = scratch("exec-launch");
        let logs = scratch.dirs.state_root().join("logs/com.example.app");
        fs::create_dir_all(&logs).unwrap();
        let log = logs.join("launch.log");
        let mut executor = SystemExecutor::new(&scratch.dirs);
        let mut plan = Plan::new(Operation::Launch, "com.example.app");
        plan.steps.push(Step::Launch {
            command: shell("echo to-stdout; echo to-stderr >&2; echo \"[$UOS_TEST]\"; exit 42"),
            log: log.clone(),
        });
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.failure, None);
        assert_eq!(report.exit_status, Some(42));

        let logged = fs::read_to_string(&log).unwrap();
        assert!(logged.starts_with("# uos-runtime launch, "));
        assert!(logged.contains("# command: /bin/sh -c "));
        // Variable names are logged; values are not.
        assert!(logged.contains("# environment: PATH UOS_TEST\n"));
        assert!(logged.contains("to-stdout\n"));
        assert!(logged.contains("to-stderr\n"));
        assert!(logged.contains("[from the plan]\n"));
        assert!(logged.contains("# exit status: 42, "));
        let mode = fs::metadata(&log).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        // A process ended by a signal reports 128 plus the signal number.
        let outcome = executor
            .execute(&Step::Launch {
                command: shell("kill -TERM $$"),
                log: log.clone(),
            })
            .unwrap();
        assert_eq!(outcome, StepOutcome::Exited { status: 143 });
    }

    #[test]
    fn checks_examine_without_following_links() {
        let scratch = scratch("exec-checks");
        let mut executor = SystemExecutor::new(&scratch.dirs);
        let prefix = scratch.app.join("prefix");
        fs::create_dir_all(prefix.join("drive_c")).unwrap();
        fs::write(
            prefix.join("system.reg"),
            b"WINE REGISTRY Version 2\n\n#arch=win64\n",
        )
        .unwrap();
        symlink(prefix.join("system.reg"), prefix.join("user.reg")).unwrap();
        symlink(prefix.join("drive_c"), prefix.join("dosdevices")).unwrap();

        let mut outcome = |check: Check| match executor.execute(&Step::Check { check }).unwrap() {
            StepOutcome::Checked { passed, detail } => (passed, detail),
            other => panic!("{other:?}"),
        };
        let file = |name: &str| Check::File {
            path: prefix.join(name),
        };
        let directory = |name: &str| Check::Directory {
            path: prefix.join(name),
        };
        let contains = |name: &str, text: &str| Check::FileContains {
            path: prefix.join(name),
            text: text.to_owned(),
        };

        assert_eq!(outcome(file("system.reg")), (true, String::new()));
        assert_eq!(outcome(directory("drive_c")), (true, String::new()));
        assert!(outcome(contains("system.reg", "#arch=win64")).0);
        assert!(
            outcome(Check::OwnedByUser {
                path: prefix.clone()
            })
            .0
        );

        let (passed, detail) = outcome(file("missing.reg"));
        assert!(!passed);
        assert!(detail.contains("cannot be examined"));
        let (passed, detail) = outcome(file("drive_c"));
        assert!(!passed);
        assert!(detail.contains("is not a regular file"));
        let (passed, detail) = outcome(directory("system.reg"));
        assert!(!passed);
        assert!(detail.contains("is not a directory"));
        let (passed, detail) = outcome(contains("system.reg", "#arch=win32"));
        assert!(!passed);
        assert!(detail.contains("does not contain"));
        // A link where a file or directory should be is a finding.
        assert!(!outcome(file("user.reg")).0);
        assert!(!outcome(directory("dosdevices")).0);
        assert!(!outcome(contains("user.reg", "#arch=win64")).0);
        // Somebody else's file.
        let (passed, detail) = outcome(Check::OwnedByUser {
            path: PathBuf::from("/usr"),
        });
        if current_effective_uid() != Some(0) {
            assert!(!passed);
            assert!(detail.contains("belongs to user 0"));
        }
    }

    #[test]
    fn the_log_has_a_budget() {
        let scratch = scratch("exec-log-budget");
        fs::create_dir_all(scratch.dirs.state_root()).unwrap();
        let path = scratch.dirs.state_root().join("budget.log");
        let mut sink = LogSink {
            file: File::create(&path).unwrap(),
            remaining: 10,
        };
        sink.write(b"12345");
        sink.write(b"67890abc");
        sink.write(b"more");
        drop(sink);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "1234567890\n# the log is full; later output was not recorded\n"
        );
    }

    #[test]
    fn output_shown_in_errors_has_no_control_characters() {
        assert_eq!(printable(b"line one\n\ttabbed\n"), "line one\n\ttabbed\n");
        assert_eq!(printable(b"a\x1b[31mb\x07"), "a\u{fffd}[31mb\u{fffd}");
        assert_eq!(printable(b"\xff\xfe"), "\u{fffd}\u{fffd}");
    }

    #[test]
    fn the_supervisor_returns_the_programs_status_and_then_settles() {
        let scratch = scratch("exec-supervise");
        fs::create_dir_all(scratch.dirs.state_root()).unwrap();
        let marker = scratch.dirs.state_root().join("settled");
        // A stand-in for wineserver that records being asked to wait.
        let settle = scratch.dirs.state_root().join("settle.sh");
        fs::write(
            &settle,
            format!("#!/bin/sh\necho \"$1\" > '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&settle, fs::Permissions::from_mode(0o700)).unwrap();

        let status = supervise(
            Some(&settle),
            "/bin/sh",
            &["-c".to_owned(), "exit 7".to_owned()],
        )
        .unwrap();
        assert_eq!(status, 7);
        assert_eq!(fs::read_to_string(&marker).unwrap(), "--wait\n");

        assert_eq!(supervise(None, "/bin/sh", &["-c".to_owned(), "exit 0".to_owned()]), Ok(0));
        // A missing settle program does not change the result.
        let missing = scratch.dirs.state_root().join("no-such-program");
        assert_eq!(
            supervise(Some(&missing), "/bin/sh", &["-c".to_owned(), "exit 5".to_owned()]),
            Ok(5)
        );
        let error = supervise(None, "/nonexistent/program", &[]).unwrap_err();
        assert!(error.message.starts_with("cannot start /nonexistent/program"));
    }
}
