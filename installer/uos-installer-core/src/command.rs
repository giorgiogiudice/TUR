// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The record of one executed command (specification §14.4).
//!
//! Every command the installer runs must go through a wrapper that records
//! the program, its sanitised arguments, start and end time, exit code, a
//! reference to the captured output and the phase it ran in. This module
//! defines that record and the wrapper's interface, [`CommandRunner`].
//!
//! **There is no implementation of [`CommandRunner`] in this crate.** Nothing
//! here starts a process. The crate's only process is the read-only inventory
//! probe in [`crate::system_probe`].
//!
//! # Secrets
//!
//! A secret enters a [`CommandSpec`] as a [`Secret`], which has no
//! `Serialize` implementation and prints as `<redacted>`. A
//! [`CommandRecord`] is built from the spec by sanitising it, so the record
//! holds no secret to leak. Sanitising also covers mistakes by the caller:
//!
//! * a plain argument `--name=value` or `--name value` whose name suggests a
//!   secret (it contains `pass`, `secret`, `token`, `key` or `credential`)
//!   has its value redacted;
//! * an environment variable with such a name has its value redacted;
//! * any occurrence of a declared secret's text inside another argument or
//!   environment value is redacted.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::phase::PhaseId;
use crate::time::Timestamp;

/// What a secret is replaced with in a record.
pub const REDACTED: &str = "<redacted>";

/// Fragments of an option or variable name that mark its value as secret.
const SENSITIVE_NAME_FRAGMENTS: [&str; 5] = ["pass", "secret", "token", "key", "credential"];

/// A value that must never be logged: a passphrase, key or token.
///
/// It cannot be serialised, and `Debug` prints `<redacted>`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret value.
    pub fn new(value: &str) -> Secret {
        Secret(value.to_owned())
    }

    /// The secret itself, for handing to the process that needs it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// One argument or environment value of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Argument {
    /// A value that may be recorded.
    Plain(String),
    /// A value that must not be recorded.
    Secret(Secret),
}

/// An environment variable set for a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariable {
    /// The variable's name.
    pub name: String,
    /// The variable's value.
    pub value: Argument,
}

/// A command to run. It may hold secrets, so it cannot be serialised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// The phase the command belongs to.
    pub phase_id: PhaseId,
    /// The program to run.
    pub program: String,
    /// The arguments, in order.
    pub arguments: Vec<Argument>,
    /// Environment variables set for the command.
    pub environment: Vec<EnvironmentVariable>,
}

impl CommandSpec {
    /// A command with no arguments.
    pub fn new(phase_id: PhaseId, program: &str) -> CommandSpec {
        CommandSpec {
            phase_id,
            program: program.to_owned(),
            arguments: Vec::new(),
            environment: Vec::new(),
        }
    }

    /// Adds an argument that may be recorded.
    pub fn arg(mut self, value: &str) -> CommandSpec {
        self.arguments.push(Argument::Plain(value.to_owned()));
        self
    }

    /// Adds an argument that must not be recorded.
    pub fn secret_arg(mut self, value: &str) -> CommandSpec {
        self.arguments.push(Argument::Secret(Secret::new(value)));
        self
    }

    /// Sets an environment variable whose value may be recorded.
    pub fn env(mut self, name: &str, value: &str) -> CommandSpec {
        self.environment.push(EnvironmentVariable {
            name: name.to_owned(),
            value: Argument::Plain(value.to_owned()),
        });
        self
    }

    /// Sets an environment variable whose value must not be recorded.
    pub fn secret_env(mut self, name: &str, value: &str) -> CommandSpec {
        self.environment.push(EnvironmentVariable {
            name: name.to_owned(),
            value: Argument::Secret(Secret::new(value)),
        });
        self
    }

    /// Every declared secret value that is not empty.
    fn secrets(&self) -> Vec<&str> {
        self.arguments
            .iter()
            .chain(self.environment.iter().map(|variable| &variable.value))
            .filter_map(|argument| match argument {
                Argument::Secret(secret) if !secret.expose().is_empty() => Some(secret.expose()),
                _ => None,
            })
            .collect()
    }
}

fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SENSITIVE_NAME_FRAGMENTS
        .iter()
        .any(|fragment| lower.contains(*fragment))
}

fn scrub(text: &str, secrets: &[&str]) -> String {
    let mut scrubbed = text.to_owned();
    for secret in secrets {
        scrubbed = scrubbed.replace(*secret, REDACTED);
    }
    scrubbed
}

fn sanitise_arguments(arguments: &[Argument], secrets: &[&str]) -> Vec<String> {
    let mut sanitised = Vec::with_capacity(arguments.len());
    let mut redact_next = false;
    for argument in arguments {
        let redact_this = redact_next;
        redact_next = false;
        let value = match argument {
            Argument::Secret(_) => REDACTED.to_owned(),
            Argument::Plain(_) if redact_this => REDACTED.to_owned(),
            Argument::Plain(text) => {
                if let Some((name, _)) = text.split_once('=')
                    && is_sensitive_name(name)
                {
                    format!("{}={REDACTED}", scrub(name, secrets))
                } else {
                    redact_next = text.starts_with('-') && is_sensitive_name(text);
                    scrub(text, secrets)
                }
            }
        };
        sanitised.push(value);
    }
    sanitised
}

fn sanitise_environment(
    environment: &[EnvironmentVariable],
    secrets: &[&str],
) -> BTreeMap<String, String> {
    environment
        .iter()
        .map(|variable| {
            let value = match &variable.value {
                Argument::Secret(_) => REDACTED.to_owned(),
                Argument::Plain(_) if is_sensitive_name(&variable.name) => REDACTED.to_owned(),
                Argument::Plain(text) => scrub(text, secrets),
            };
            (scrub(&variable.name, secrets), value)
        })
        .collect()
}

/// The record of one command, safe to write to the session directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRecord {
    /// The command's position in the session, starting at 1.
    pub sequence: u32,
    /// The phase the command ran in.
    pub phase_id: PhaseId,
    /// The program that was run.
    pub program: String,
    /// The arguments, with secrets replaced by `<redacted>`.
    pub arguments: Vec<String>,
    /// The environment variables set for the command, with secrets replaced
    /// by `<redacted>`.
    pub environment: BTreeMap<String, String>,
    /// When the command started.
    pub started_at: Timestamp,
    /// When the command ended.
    pub finished_at: Option<Timestamp>,
    /// How long the command took.
    pub duration_ms: Option<u64>,
    /// The exit code. `None` while running, or when a signal ended the process.
    pub exit_code: Option<i32>,
    /// Where the captured standard output and error are, relative to the
    /// session directory.
    pub output_reference: Option<String>,
}

impl CommandRecord {
    /// The record of a command that is about to start.
    pub fn started(sequence: u32, spec: &CommandSpec, now: Timestamp) -> CommandRecord {
        let secrets = spec.secrets();
        CommandRecord {
            sequence,
            phase_id: spec.phase_id,
            program: scrub(&spec.program, &secrets),
            arguments: sanitise_arguments(&spec.arguments, &secrets),
            environment: sanitise_environment(&spec.environment, &secrets),
            started_at: now,
            finished_at: None,
            duration_ms: None,
            exit_code: None,
            output_reference: None,
        }
    }

    /// Records the end of the command.
    pub fn finish(&mut self, now: Timestamp, exit_code: Option<i32>, output_reference: Option<&str>) {
        self.finished_at = Some(now);
        self.duration_ms = Some(now.millis_since(self.started_at));
        self.exit_code = exit_code;
        self.output_reference = output_reference.map(str::to_owned);
    }

    /// The record's file name under `commands/` in the session directory,
    /// for example `0003-example-tool.json`.
    pub fn file_name(&self) -> String {
        let program = self.program.rsplit('/').next().unwrap_or("");
        let safe: String = program
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                    character
                } else {
                    '-'
                }
            })
            .collect();
        format!("{:04}-{safe}.json", self.sequence)
    }
}

/// Why a command could not be run at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    /// The program that was to run.
    pub program: String,
    /// What went wrong.
    pub reason: String,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} could not be run: {}", self.program, self.reason)
    }
}

impl std::error::Error for CommandError {}

/// The wrapper every installer command must go through.
///
/// An implementation runs the command described by a [`CommandSpec`],
/// captures its output to a file in the session directory, and returns the
/// finished [`CommandRecord`]. This crate provides no implementation.
pub trait CommandRunner {
    /// Runs one command and returns its record.
    fn run(&mut self, spec: &CommandSpec) -> Result<CommandRecord, CommandError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSPHRASE: &str = "correct horse battery staple";

    fn at(millis: u64) -> Timestamp {
        Timestamp::from_unix_millis(millis)
    }

    fn assert_no_leak(record: &CommandRecord) {
        let json = serde_json::to_string(record).unwrap();
        assert!(!json.contains(PASSPHRASE), "leaked in JSON: {json}");
        assert!(!json.contains("staple"), "leaked in part: {json}");
        let debug = format!("{record:?}");
        assert!(!debug.contains(PASSPHRASE), "leaked in Debug: {debug}");
    }

    #[test]
    fn a_secret_argument_is_redacted() {
        let spec = CommandSpec::new(PhaseId::EncryptionSetup, "/usr/bin/example-tool")
            .arg("open")
            .arg("--label")
            .arg("root")
            .secret_arg(PASSPHRASE);
        let record = CommandRecord::started(1, &spec, at(10));
        assert_eq!(record.arguments, ["open", "--label", "root", REDACTED]);
        assert_eq!(record.program, "/usr/bin/example-tool");
        assert_eq!(record.phase_id, PhaseId::EncryptionSetup);
        assert_no_leak(&record);
        assert!(!format!("{spec:?}").contains(PASSPHRASE));
    }

    #[test]
    fn a_secret_environment_variable_is_redacted() {
        let spec = CommandSpec::new(PhaseId::EncryptionSetup, "example-tool")
            .env("LANG", "C.UTF-8")
            .secret_env("UNLOCK_INPUT", PASSPHRASE);
        let record = CommandRecord::started(2, &spec, at(10));
        assert_eq!(record.environment["LANG"], "C.UTF-8");
        assert_eq!(record.environment["UNLOCK_INPUT"], REDACTED);
        assert_no_leak(&record);
        assert!(!format!("{spec:?}").contains(PASSPHRASE));
    }

    #[test]
    fn a_passphrase_passed_as_a_plain_value_by_mistake_is_still_redacted() {
        let attached = format!("--passphrase={PASSPHRASE}");
        let spec = CommandSpec::new(PhaseId::EncryptionSetup, "example-tool")
            .arg(&attached)
            .arg("--key-file")
            .arg("/run/keys/root.key")
            .arg("--Recovery-Token")
            .arg(PASSPHRASE)
            .arg("--verbose")
            .env("EXAMPLE_PASSWORD", PASSPHRASE)
            .env("api_token", PASSPHRASE);
        let record = CommandRecord::started(3, &spec, at(10));
        assert_eq!(
            record.arguments,
            [
                "--passphrase=<redacted>",
                "--key-file",
                REDACTED,
                "--Recovery-Token",
                REDACTED,
                "--verbose"
            ]
        );
        assert_eq!(record.environment["EXAMPLE_PASSWORD"], REDACTED);
        assert_eq!(record.environment["api_token"], REDACTED);
        assert_no_leak(&record);
    }

    #[test]
    fn a_declared_secret_is_scrubbed_wherever_else_it_appears() {
        let embedded = format!("--options=cipher=aes,phrase={PASSPHRASE},discard");
        let positional = format!("prefix-{PASSPHRASE}-suffix");
        let spec = CommandSpec::new(PhaseId::EncryptionSetup, "example-tool")
            .secret_arg(PASSPHRASE)
            .arg(&embedded)
            .arg(&positional)
            .env("EXTRA_OPTIONS", &positional);
        let record = CommandRecord::started(4, &spec, at(10));
        assert_eq!(
            record.arguments,
            [
                REDACTED,
                "--options=cipher=aes,phrase=<redacted>,discard",
                "prefix-<redacted>-suffix"
            ]
        );
        assert_eq!(record.environment["EXTRA_OPTIONS"], "prefix-<redacted>-suffix");
        assert_no_leak(&record);
    }

    #[test]
    fn ordinary_arguments_are_recorded_unchanged() {
        let spec = CommandSpec::new(PhaseId::StorageDiscovery, "lsblk")
            .arg("--json")
            .arg("--bytes")
            .arg("--output-all")
            .env("LC_ALL", "C");
        let record = CommandRecord::started(5, &spec, at(10));
        assert_eq!(record.arguments, ["--json", "--bytes", "--output-all"]);
        assert_eq!(record.environment.len(), 1);
        assert_eq!(record.file_name(), "0005-lsblk.json");
    }

    #[test]
    fn an_empty_secret_does_not_redact_everything() {
        let spec = CommandSpec::new(PhaseId::EncryptionSetup, "example-tool")
            .secret_arg("")
            .arg("status");
        let record = CommandRecord::started(6, &spec, at(10));
        assert_eq!(record.arguments, [REDACTED, "status"]);
    }

    #[test]
    fn finishing_records_time_exit_code_and_output() {
        let spec = CommandSpec::new(PhaseId::StorageDiscovery, "/usr/bin/lsblk").arg("--json");
        let mut record = CommandRecord::started(12, &spec, at(1_000));
        assert_eq!(record.finished_at, None);
        assert_eq!(record.exit_code, None);
        record.finish(at(1_250), Some(0), Some("commands/0012-lsblk.out"));
        assert_eq!(record.finished_at, Some(at(1_250)));
        assert_eq!(record.duration_ms, Some(250));
        assert_eq!(record.exit_code, Some(0));
        assert_eq!(
            record.output_reference.as_deref(),
            Some("commands/0012-lsblk.out")
        );
        assert_eq!(record.file_name(), "0012-lsblk.json");

        let json = serde_json::to_string(&record).unwrap();
        let again: CommandRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(again, record);
    }

    #[test]
    fn file_names_are_safe() {
        let spec = CommandSpec::new(PhaseId::Start, "../odd name;rm");
        let record = CommandRecord::started(7, &spec, at(0));
        assert_eq!(record.file_name(), "0007-odd-name-rm.json");
    }

    /// A runner that runs nothing, to show the trait's shape.
    struct RecordingRunner {
        clock: u64,
        records: Vec<CommandRecord>,
    }

    impl CommandRunner for RecordingRunner {
        fn run(&mut self, spec: &CommandSpec) -> Result<CommandRecord, CommandError> {
            if spec.program.is_empty() {
                return Err(CommandError {
                    program: spec.program.clone(),
                    reason: "no program was given".to_owned(),
                });
            }
            let sequence = u32::try_from(self.records.len() + 1).unwrap_or(u32::MAX);
            let mut record = CommandRecord::started(sequence, spec, at(self.clock));
            self.clock += 5;
            record.finish(at(self.clock), Some(0), None);
            self.records.push(record.clone());
            Ok(record)
        }
    }

    #[test]
    fn a_runner_returns_sanitised_records() {
        let mut runner = RecordingRunner {
            clock: 100,
            records: Vec::new(),
        };
        let spec = CommandSpec::new(PhaseId::EncryptionSetup, "example-tool").secret_arg(PASSPHRASE);
        let record = runner.run(&spec).unwrap();
        assert_eq!(record.sequence, 1);
        assert_eq!(record.duration_ms, Some(5));
        assert_no_leak(&record);
        assert_eq!(runner.records.len(), 1);

        let error = runner
            .run(&CommandSpec::new(PhaseId::Start, ""))
            .unwrap_err();
        assert_eq!(error.to_string(), " could not be run: no program was given");
    }
}
