// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Plans: what an operation will do, written down before anything is done.
//!
//! Every prefix operation and every launch is first a [`Plan`], a list of
//! [`Step`]s that can be printed, compared and tested without touching the
//! system. A plan is carried out by [`run_plan`] through an [`Executor`]:
//! the real one in [`crate::exec`], or a recording one in tests.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Serialize, Serializer};

use crate::casefold::CaseFoldRecord;

/// The operation a plan carries out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    /// Create a prefix.
    Create,
    /// Check a prefix without changing it.
    Verify,
    /// Restore what a prefix is missing, with the runtime it is on.
    Repair,
    /// Replace a prefix with a fresh one, keeping its settings.
    Reset,
    /// Archive a prefix.
    Backup,
    /// Replace a prefix with an archived one.
    Restore,
    /// Delete a prefix.
    Remove,
    /// Move a prefix to the installed runtime version, after a backup.
    Migrate,
    /// Change the recorded settings of a prefix.
    Configure,
    /// Start the application.
    Launch,
    /// Install an application into a fresh prefix.
    Install,
    /// Remove an installed application and everything kept for it.
    Uninstall,
}

impl Operation {
    /// The operation's machine-readable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Operation::Create => "create",
            Operation::Verify => "verify",
            Operation::Repair => "repair",
            Operation::Reset => "reset",
            Operation::Backup => "backup",
            Operation::Restore => "restore",
            Operation::Remove => "remove",
            Operation::Migrate => "migrate",
            Operation::Configure => "configure",
            Operation::Launch => "launch",
            Operation::Install => "install",
            Operation::Uninstall => "uninstall",
        }
    }
}

/// A command: program, arguments, the whole environment and the working
/// directory. Nothing is inherited from the caller's environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandSpec {
    /// The program to start, as an absolute path.
    pub program: String,
    /// Its arguments.
    pub arguments: Vec<String>,
    /// Every environment variable the program receives.
    pub environment: BTreeMap<String, String>,
    /// The directory to start it in, when that matters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
}

impl CommandSpec {
    /// The program and its arguments as one line, quoted for reading. The
    /// line is for people; it is never given to a shell.
    pub fn display_line(&self) -> String {
        let mut line = quote(&self.program);
        for argument in &self.arguments {
            line.push(' ');
            line.push_str(&quote(argument));
        }
        line
    }
}

/// Quotes `text` the way a POSIX shell would need it, for display only.
fn quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'/' | b'.' | b'_' | b'-' | b'=' | b':' | b',' | b'+' | b'@'
                )
        });
    if plain {
        text.to_owned()
    } else {
        let visible: String = text
            .chars()
            .map(|character| {
                if character.is_control() {
                    '\u{fffd}'
                } else {
                    character
                }
            })
            .collect();
        format!("'{}'", visible.replace('\'', "'\\''"))
    }
}

/// A read-only test of the prefix, used by verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Check {
    /// The path is a directory, and not a symbolic link to one.
    Directory {
        /// The path.
        path: PathBuf,
    },
    /// The path is a regular file, and not a symbolic link to one.
    File {
        /// The path.
        path: PathBuf,
    },
    /// The path is a regular file whose first bytes contain `text`.
    FileContains {
        /// The path.
        path: PathBuf,
        /// The text looked for.
        text: String,
    },
    /// The path belongs to the user running the check.
    OwnedByUser {
        /// The path.
        path: PathBuf,
    },
    /// Nothing is at the path: no file, no directory, no link.
    Absent {
        /// The path.
        path: PathBuf,
    },
}

impl Check {
    /// What the check expects, in plain language.
    pub fn describe(&self) -> String {
        match self {
            Check::Directory { path } => format!("{} is a directory", path.display()),
            Check::File { path } => format!("{} is a file", path.display()),
            Check::FileContains { path, text } => {
                format!("{} contains {text:?}", path.display())
            }
            Check::OwnedByUser { path } => {
                format!("{} belongs to the current user", path.display())
            }
            Check::Absent { path } => format!("{} is gone", path.display()),
        }
    }
}

/// One step of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "step", rename_all = "kebab-case")]
pub enum Step {
    /// Create a directory and its parents, readable by the user only.
    CreateDir {
        /// The directory.
        path: PathBuf,
    },
    /// Create an empty directory, as `create-dir` does, and ask its file
    /// system to fold the case of the names in it (`chattr +F`). A file
    /// system that cannot is an outcome, not a failure: the directory is
    /// then an ordinary one and the plan goes on. Only creating the
    /// directory can fail the step.
    CaseFold {
        /// The directory.
        path: PathBuf,
        /// The `chattr` command, or nothing when `chattr` is not installed.
        command: Option<CommandSpec>,
    },
    /// Write a file, replacing it atomically if it exists.
    WriteFile {
        /// The file.
        path: PathBuf,
        /// Its whole contents.
        contents: String,
    },
    /// Delete a directory tree. Only paths inside the runtime manager's own
    /// directories are ever deleted; a missing path is not an error.
    RemoveTree {
        /// The directory.
        path: PathBuf,
    },
    /// Write a file that is not text, replacing it atomically if it exists.
    /// A plan shows the size of the contents, not the contents.
    WriteBytes {
        /// The file.
        path: PathBuf,
        /// Its whole contents.
        #[serde(rename = "size", serialize_with = "byte_count")]
        bytes: Vec<u8>,
    },
    /// Rename a file or directory.
    Rename {
        /// The existing path.
        from: PathBuf,
        /// The new path.
        to: PathBuf,
    },
    /// Run a command to completion. Any exit status but zero fails the plan.
    Run {
        /// What the command is for.
        label: String,
        /// The command.
        command: CommandSpec,
    },
    /// Run a command to completion, for at most `limit_seconds`. Any exit
    /// status but zero fails the plan, and so does running out of time: the
    /// command is then stopped, with everything it started.
    RunBounded {
        /// What the command is for.
        label: String,
        /// The command.
        command: CommandSpec,
        /// How long it may take, in seconds.
        limit_seconds: u64,
    },
    /// Start the application and wait for it. Its output goes to `log`; its
    /// exit status is the result of the plan.
    Launch {
        /// The command.
        command: CommandSpec,
        /// The log file its output is captured to.
        log: PathBuf,
    },
    /// Test something without changing it. A failed check is recorded as a
    /// finding and does not stop the plan.
    Check {
        /// The test.
        check: Check,
    },
    /// Test something the rest of the plan depends on. A failed requirement
    /// stops the plan.
    Require {
        /// The test.
        check: Check,
    },
}

/// Serialises the contents of a binary file as their length.
#[allow(clippy::ptr_arg)]
fn byte_count<S: Serializer>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
}

impl Step {
    /// One line that says what the step does.
    pub fn describe(&self) -> String {
        match self {
            Step::CreateDir { path } => format!("create directory {}", path.display()),
            Step::CaseFold { path, command } => match command {
                Some(command) => format!(
                    "create directory {} and make it case-insensitive where the file system can: {}",
                    path.display(),
                    command.display_line()
                ),
                None => format!(
                    "create directory {}; it cannot be made case-insensitive: chattr is not installed",
                    path.display()
                ),
            },
            Step::WriteFile { path, contents } => {
                format!("write {} ({} bytes)", path.display(), contents.len())
            }
            Step::WriteBytes { path, bytes } => {
                format!("write {} ({} bytes)", path.display(), bytes.len())
            }
            Step::RemoveTree { path } => format!("delete {}", path.display()),
            Step::Rename { from, to } => {
                format!("rename {} to {}", from.display(), to.display())
            }
            Step::Run { label, command } => format!("{label}: {}", command.display_line()),
            Step::RunBounded {
                label,
                command,
                limit_seconds,
            } => format!(
                "{label}, within {limit_seconds} seconds: {}",
                command.display_line()
            ),
            Step::Launch { command, log } => format!(
                "launch, logging to {}: {}",
                log.display(),
                command.display_line()
            ),
            Step::Check { check } => format!("check that {}", check.describe()),
            Step::Require { check } => format!("require that {}", check.describe()),
        }
    }
}

/// What an operation will do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// The operation.
    pub operation: Operation,
    /// The application it is for.
    pub app_id: String,
    /// The steps, in order.
    pub steps: Vec<Step>,
    /// Things a reader of the plan should know: decisions taken, fallbacks,
    /// what is not covered.
    pub notes: Vec<String>,
}

impl Plan {
    /// An empty plan.
    pub fn new(operation: Operation, app_id: &str) -> Self {
        Plan {
            operation,
            app_id: app_id.to_owned(),
            steps: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// The plan as text: a heading, one numbered line per step, then notes.
    pub fn render_text(&self) -> String {
        let mut text = format!(
            "Plan: {} {} ({} steps)\n",
            self.operation.as_str(),
            self.app_id,
            self.steps.len()
        );
        for (index, step) in self.steps.iter().enumerate() {
            text.push_str(&format!("  {}. {}\n", index + 1, step.describe()));
        }
        for note in &self.notes {
            text.push_str(&format!("  note: {note}\n"));
        }
        text
    }
}

/// What carrying out one step produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum StepOutcome {
    /// The step was carried out.
    Done,
    /// A command ran and exited with this status.
    Exited {
        /// The exit status; 128 plus the signal number when a signal ended
        /// the process.
        status: i32,
    },
    /// A check was made.
    Checked {
        /// Whether what the check expects holds.
        passed: bool,
        /// What was found when it does not.
        detail: String,
    },
    /// A command was stopped because it ran out of time.
    TimedOut {
        /// How long it was given, in seconds.
        seconds: u64,
    },
    /// A directory was created and the file system asked to fold the case
    /// of names in it.
    CaseFolded {
        /// Whether it does, and why or why not.
        record: CaseFoldRecord,
    },
}

/// A step that could not be carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepError {
    /// What went wrong.
    pub message: String,
}

impl StepError {
    /// An error with this message.
    pub fn new(message: impl Into<String>) -> Self {
        StepError {
            message: message.into(),
        }
    }
}

impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StepError {}

/// Carries out the steps of a plan.
pub trait Executor {
    /// Carries out one step.
    ///
    /// # Errors
    ///
    /// A [`StepError`] when the step could not be carried out. A command that
    /// ran and exited with a failure status is not an error here: it is
    /// reported as [`StepOutcome::Exited`].
    fn execute(&mut self, step: &Step) -> Result<StepOutcome, StepError>;
}

/// A check that did not hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// What was expected.
    pub expected: String,
    /// What was found.
    pub found: String,
}

/// The step at which a plan stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    /// The index of the step in the plan, from zero.
    pub step: usize,
    /// What the step was.
    pub description: String,
    /// What went wrong.
    pub message: String,
}

/// What carrying out a plan produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanReport {
    /// How many steps were carried out.
    pub completed: usize,
    /// How many steps the plan has.
    pub total: usize,
    /// The step at which the plan stopped, if it did not finish.
    pub failure: Option<Failure>,
    /// The checks that did not hold.
    pub findings: Vec<Finding>,
    /// The exit status of the application, when the plan launched one, or
    /// of the last time-limited command the plan ran.
    pub exit_status: Option<i32>,
    /// Whether the plan stopped because a command ran out of time.
    pub timed_out: bool,
    /// What became of making a directory case-insensitive, when the plan
    /// asked for that.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub case_folding: Option<CaseFoldRecord>,
}

impl PlanReport {
    /// Whether every step was carried out and every check held.
    pub fn succeeded(&self) -> bool {
        self.failure.is_none() && self.findings.is_empty()
    }
}

/// Carries out `plan` through `executor`, stopping at the first step that
/// fails. Checks never stop the plan; the ones that do not hold are returned
/// as findings.
pub fn run_plan(plan: &Plan, executor: &mut dyn Executor) -> PlanReport {
    let mut report = PlanReport {
        completed: 0,
        total: plan.steps.len(),
        failure: None,
        findings: Vec::new(),
        exit_status: None,
        timed_out: false,
        case_folding: None,
    };
    for (index, step) in plan.steps.iter().enumerate() {
        let problem = match executor.execute(step) {
            Err(error) => Some(error.message),
            Ok(StepOutcome::Exited { status }) => match step {
                Step::Launch { .. } => {
                    report.exit_status = Some(status);
                    None
                }
                Step::RunBounded { .. } => {
                    report.exit_status = Some(status);
                    (status != 0).then(|| format!("the command exited with status {status}"))
                }
                _ if status == 0 => None,
                _ => Some(format!("the command exited with status {status}")),
            },
            Ok(StepOutcome::TimedOut { seconds }) => {
                report.timed_out = true;
                Some(format!(
                    "the command did not finish within {seconds} seconds and was stopped"
                ))
            }
            Ok(StepOutcome::Checked { passed, detail }) => match step {
                _ if passed => None,
                Step::Check { check } => {
                    report.findings.push(Finding {
                        expected: check.describe(),
                        found: detail,
                    });
                    None
                }
                _ => Some(detail),
            },
            Ok(StepOutcome::CaseFolded { record }) => {
                report.case_folding = Some(record);
                None
            }
            Ok(StepOutcome::Done) => None,
        };
        if let Some(message) = problem {
            report.failure = Some(Failure {
                step: index,
                description: step.describe(),
                message,
            });
            break;
        }
        report.completed += 1;
    }
    report
}

/// An executor that records the steps it is given and does nothing.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;

    /// Records steps. Commands exit with the scripted statuses, in order, and
    /// with zero once the script runs out.
    #[derive(Default)]
    pub(crate) struct FakeExecutor {
        /// Every step given to the executor.
        pub(crate) steps: Vec<Step>,
        /// Exit statuses for `Run` and `Launch` steps, first one first.
        pub(crate) statuses: Vec<i32>,
        /// The index of a step that fails with an error.
        pub(crate) fail_at: Option<usize>,
        /// Text which, when a check's description contains it, fails the
        /// check.
        pub(crate) failing_checks: Vec<String>,
        /// The index of a time-limited command that runs out of time.
        pub(crate) time_out_at: Option<usize>,
        /// The exit status and message of `chattr` for a case-folding step;
        /// by default it succeeds.
        pub(crate) case_fold: Option<(i32, &'static str)>,
    }

    impl Executor for FakeExecutor {
        fn execute(&mut self, step: &Step) -> Result<StepOutcome, StepError> {
            let index = self.steps.len();
            self.steps.push(step.clone());
            if self.fail_at == Some(index) {
                return Err(StepError::new("scripted failure"));
            }
            Ok(match step {
                Step::RunBounded { .. } if self.time_out_at == Some(index) => {
                    StepOutcome::TimedOut { seconds: 1 }
                }
                Step::Run { .. } | Step::RunBounded { .. } | Step::Launch { .. } => {
                    StepOutcome::Exited {
                        status: if self.statuses.is_empty() {
                            0
                        } else {
                            self.statuses.remove(0)
                        },
                    }
                }
                Step::CaseFold { command: None, .. } => StepOutcome::CaseFolded {
                    record: CaseFoldRecord::attempted(
                        crate::casefold::CaseFoldReason::ToolMissing,
                        None,
                    ),
                },
                Step::CaseFold { .. } => {
                    let (status, output) = self.case_fold.unwrap_or((0, ""));
                    StepOutcome::CaseFolded {
                        record: crate::casefold::classify(status, output),
                    }
                }
                Step::Check { check } | Step::Require { check } => {
                    let description = check.describe();
                    let failed = self
                        .failing_checks
                        .iter()
                        .any(|text| description.contains(text.as_str()));
                    StepOutcome::Checked {
                        passed: !failed,
                        detail: if failed {
                            "scripted finding".to_owned()
                        } else {
                            String::new()
                        },
                    }
                }
                _ => StepOutcome::Done,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeExecutor;
    use super::*;

    fn command(program: &str, arguments: &[&str]) -> CommandSpec {
        CommandSpec {
            program: program.to_owned(),
            arguments: arguments.iter().map(|text| (*text).to_owned()).collect(),
            environment: BTreeMap::new(),
            working_directory: None,
        }
    }

    fn sample() -> Plan {
        let mut plan = Plan::new(Operation::Create, "com.example.app");
        plan.steps = vec![
            Step::CreateDir {
                path: PathBuf::from("/d/prefix"),
            },
            Step::Run {
                label: "initialise".to_owned(),
                command: command("/usr/bin/wine", &["wineboot", "--init"]),
            },
            Step::Check {
                check: Check::File {
                    path: PathBuf::from("/d/prefix/system.reg"),
                },
            },
            Step::WriteFile {
                path: PathBuf::from("/d/metadata.json"),
                contents: "{}".to_owned(),
            },
            Step::Launch {
                command: command("/usr/bin/bwrap", &["--", "wine", "app.exe"]),
                log: PathBuf::from("/s/log"),
            },
        ];
        plan.notes.push("a note".to_owned());
        plan
    }

    #[test]
    fn a_plan_runs_every_step_in_order() {
        let plan = sample();
        let mut executor = FakeExecutor {
            statuses: vec![0, 42],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(executor.steps, plan.steps);
        assert_eq!(report.completed, 5);
        assert_eq!(report.total, 5);
        assert_eq!(report.failure, None);
        assert!(report.findings.is_empty());
        // The application's exit status is a result, not a failure.
        assert_eq!(report.exit_status, Some(42));
        assert!(report.succeeded());
    }

    #[test]
    fn a_failing_command_stops_the_plan() {
        let plan = sample();
        let mut executor = FakeExecutor {
            statuses: vec![3],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(executor.steps.len(), 2);
        assert_eq!(report.completed, 1);
        let failure = report.failure.clone().unwrap();
        assert_eq!(failure.step, 1);
        assert!(failure.message.contains("status 3"));
        assert!(failure.description.starts_with("initialise: "));
        assert_eq!(report.exit_status, None);
        assert!(!report.succeeded());
    }

    #[test]
    fn an_executor_error_stops_the_plan() {
        let plan = sample();
        let mut executor = FakeExecutor {
            fail_at: Some(0),
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(executor.steps.len(), 1);
        assert_eq!(report.completed, 0);
        assert_eq!(report.failure.unwrap().message, "scripted failure");
    }

    #[test]
    fn a_failed_check_is_a_finding_and_the_plan_continues() {
        let plan = sample();
        let mut executor = FakeExecutor {
            failing_checks: vec!["system.reg".to_owned()],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.completed, 5);
        assert_eq!(report.failure, None);
        assert_eq!(
            report.findings,
            [Finding {
                expected: "/d/prefix/system.reg is a file".to_owned(),
                found: "scripted finding".to_owned(),
            }]
        );
        assert!(!report.succeeded());
    }

    #[test]
    fn a_failed_requirement_stops_the_plan() {
        let mut plan = sample();
        plan.steps.insert(
            1,
            Step::Require {
                check: Check::File {
                    path: PathBuf::from("/d/archive.tar.gz"),
                },
            },
        );
        let mut executor = FakeExecutor {
            failing_checks: vec!["archive.tar.gz".to_owned()],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.completed, 1);
        assert_eq!(executor.steps.len(), 2);
        let failure = report.failure.unwrap();
        assert_eq!(failure.step, 1);
        assert_eq!(
            failure.description,
            "require that /d/archive.tar.gz is a file"
        );
        assert_eq!(failure.message, "scripted finding");
        assert!(report.findings.is_empty());

        // A requirement that holds changes nothing.
        let report = run_plan(&plan, &mut FakeExecutor::default());
        assert_eq!(report.completed, 6);
        assert!(report.succeeded());
    }

    #[test]
    fn plans_render_as_text_and_json() {
        let plan = sample();
        let text = plan.render_text();
        assert!(text.starts_with("Plan: create com.example.app (5 steps)\n"));
        assert!(text.contains("  1. create directory /d/prefix\n"));
        assert!(text.contains("  2. initialise: /usr/bin/wine wineboot --init\n"));
        assert!(text.contains("  3. check that /d/prefix/system.reg is a file\n"));
        assert!(text.contains("  4. write /d/metadata.json (2 bytes)\n"));
        assert!(text.contains("  5. launch, logging to /s/log: /usr/bin/bwrap -- wine app.exe\n"));
        assert!(text.ends_with("  note: a note\n"));

        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(json["operation"], "create");
        assert_eq!(json["app_id"], "com.example.app");
        assert_eq!(json["steps"][0]["step"], "create-dir");
        assert_eq!(json["steps"][0]["path"], "/d/prefix");
        assert_eq!(json["steps"][1]["step"], "run");
        assert_eq!(json["steps"][1]["command"]["program"], "/usr/bin/wine");
        assert_eq!(json["steps"][1]["command"]["arguments"][1], "--init");
        assert!(
            json["steps"][1]["command"]
                .get("working_directory")
                .is_none()
        );
        assert_eq!(json["steps"][2]["step"], "check");
        assert_eq!(json["steps"][2]["check"]["kind"], "file");
        assert_eq!(json["steps"][4]["step"], "launch");
        assert_eq!(json["steps"][4]["log"], "/s/log");
    }

    #[test]
    fn display_lines_are_quoted_and_carry_no_control_characters() {
        let spec = command(
            "/usr/bin/wine",
            &[
                "C:\\Program Files\\App\\app.exe",
                "",
                "it's",
                "a\u{1b}[31mb",
                "--flag=1",
            ],
        );
        assert_eq!(
            spec.display_line(),
            "/usr/bin/wine 'C:\\Program Files\\App\\app.exe' '' 'it'\\''s' 'a\u{fffd}[31mb' --flag=1"
        );
    }

    #[test]
    fn a_time_limited_command_reports_its_status_and_running_out_of_time() {
        let step = |label: &str| Step::RunBounded {
            label: label.to_owned(),
            command: command("/usr/bin/bwrap", &["--", "wine", "setup.exe", "/S"]),
            limit_seconds: 900,
        };
        let mut plan = Plan::new(Operation::Install, "com.example.app");
        plan.steps = vec![
            step("run the installer"),
            Step::WriteBytes {
                path: PathBuf::from("/d/icon.png"),
                bytes: vec![1, 2, 3],
            },
        ];
        // It succeeds: the plan goes on, and the status is reported.
        let report = run_plan(&plan, &mut FakeExecutor::default());
        assert!(report.succeeded());
        assert_eq!(report.exit_status, Some(0));
        assert!(!report.timed_out);
        // It fails: the plan stops, and the status is still reported.
        let mut executor = FakeExecutor {
            statuses: vec![2],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.completed, 0);
        assert_eq!(report.exit_status, Some(2));
        assert!(!report.timed_out);
        assert!(report.failure.unwrap().message.contains("status 2"));
        // It runs out of time: the plan stops and says so.
        let mut executor = FakeExecutor {
            time_out_at: Some(0),
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.completed, 0);
        assert!(report.timed_out);
        assert_eq!(report.exit_status, None);
        assert!(report.failure.unwrap().message.contains("did not finish"));

        let text = plan.render_text();
        assert!(text.contains(
            "  1. run the installer, within 900 seconds: /usr/bin/bwrap -- wine setup.exe /S\n"
        ));
        assert!(text.contains("  2. write /d/icon.png (3 bytes)\n"));
        // A plan shows the size of a binary file, never its bytes.
        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(json["steps"][0]["step"], "run-bounded");
        assert_eq!(json["steps"][0]["limit_seconds"], 900);
        assert_eq!(json["steps"][1]["step"], "write-bytes");
        assert_eq!(json["steps"][1]["size"], 3);
        assert!(json["steps"][1].get("bytes").is_none());
        assert_eq!(
            Check::Absent {
                path: PathBuf::from("/d/prefix")
            }
            .describe(),
            "/d/prefix is gone"
        );
    }

    #[test]
    fn operation_names() {
        for (operation, name) in [
            (Operation::Create, "create"),
            (Operation::Verify, "verify"),
            (Operation::Repair, "repair"),
            (Operation::Reset, "reset"),
            (Operation::Backup, "backup"),
            (Operation::Restore, "restore"),
            (Operation::Remove, "remove"),
            (Operation::Migrate, "migrate"),
            (Operation::Configure, "configure"),
            (Operation::Launch, "launch"),
            (Operation::Install, "install"),
            (Operation::Uninstall, "uninstall"),
        ] {
            assert_eq!(operation.as_str(), name);
            assert_eq!(serde_json::to_value(operation).unwrap(), name);
        }
    }
}
