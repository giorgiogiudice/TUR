// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The command-line interface for scripted callers.
//!
//! ```text
//! tur-pax manifest validate <file>
//! tur-pax policy check --manifest <file> --tool <name> [--args <json>]
//!                        [--policy <file>] [context flags] [--json]
//! tur-pax run "<request>" [options] [--json]
//! tur-pax chat [options]
//! tur-pax serve [--bus <address>] [--tools <dir>] [--manifests <dir>]
//!               [--adapters <dir>] [--models <file>]
//! tur-pax schedule wake [--bus <address>]
//! tur-pax key set|remove|status <provider> [--bus <address>]
//! tur-pax models list [<provider>] [--refresh] [--json] [--bus <address>]
//! tur-pax models use <provider>:<model> [--yes] [--accept-licence] [--bus <address>]
//! tur-pax models cancel [--bus <address>]
//! tur-pax models settings <provider>:<model> [--set NAME=VALUE]... [--clear NAME]...
//!                         [--json] [--bus <address>]
//! ```
//!
//! `serve`, `schedule` and `key` are described in [`crate::daemon`].
//!
//! `run` and `chat` are the text interface of the orchestrator; their options
//! and exit statuses are described in [`crate::textui`].
//!
//! `policy check` exits with 0 only when the decision is `allow`, so a script
//! that tests the exit status fails closed. The other decisions have their own
//! exit statuses, listed with the constants below.
//!
//! This module is the only place in the crate that reads a file or the clock.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};

use crate::action::Action;
use crate::audit::{AuditMeta, AuditRecord};
use crate::codes::{Code, Problem};
use crate::context::{
    Context, DEFAULT_REPAIR_SESSION_SECONDS, Presence, RepairSession, StandingApproval, Timestamp,
    WindowOwner,
};
use crate::decision::{Decision, Verdict};
use crate::engine::decide;
use crate::manifest::ToolManifest;
use crate::policy::UserPolicy;
use crate::store::HeldApprovals;
use crate::strict;

/// Exit status: the decision is `allow`, or the manifest is valid.
pub const EXIT_ALLOW: u8 = 0;
/// Exit status: an input was unreadable or invalid. Nothing was decided.
pub const EXIT_INPUT_ERROR: u8 = 1;
/// Exit status: the decision is `ask`.
pub const EXIT_ASK: u8 = 10;
/// Exit status: the decision is `authenticate`.
pub const EXIT_AUTHENTICATE: u8 = 11;
/// Exit status: the decision is `defer`.
pub const EXIT_DEFER: u8 = 12;
/// Exit status: the decision is `refuse`.
pub const EXIT_REFUSE: u8 = 13;

/// TUR AI control layer: tool manifests and the policy engine.
#[derive(Debug, Parser)]
#[command(name = "tur-pax", version)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: TopCommand,
}

/// The top-level subcommands.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum TopCommand {
    /// Work with tool manifests.
    Manifest {
        /// What to do with a manifest.
        #[command(subcommand)]
        command: ManifestCommand,
    },
    /// Evaluate the policy engine.
    Policy {
        /// What to ask the policy engine.
        #[command(subcommand)]
        command: PolicyCommand,
    },
    /// Run one task and print what P@x did: every proposed action with its
    /// class and decision, and the final answer.
    Run(crate::textui::RunArgs),
    /// Talk to P@x on a terminal, one request per line.
    Chat(crate::textui::ChatArgs),
    /// Serve org.tur.Agent1 on the session bus: the session service the
    /// desktop talks to. Started by the user unit tur-pax.service.
    Serve(crate::daemon::ServeArgs),
    /// Reminders and scheduled tasks.
    Schedule {
        /// What to do.
        #[command(subcommand)]
        command: crate::daemon::ScheduleCommand,
    },
    /// The provider key, in the password store of the session.
    Key {
        /// What to do with the key.
        #[command(subcommand)]
        command: crate::daemon::KeyCommand,
    },
    /// The models P@x can use: list them, choose one (a local model is
    /// downloaded and started first), and their generation settings.
    Models {
        /// What to do.
        #[command(subcommand)]
        command: crate::daemon::ModelsCommand,
    },
}

/// The `manifest` subcommands.
#[derive(Debug, Subcommand)]
pub enum ManifestCommand {
    /// Validate a tool manifest and list its tools.
    Validate {
        /// The manifest file.
        file: PathBuf,
    },
}

/// The `policy` subcommands.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum PolicyCommand {
    /// Decide what happens to one tool call and print the decision.
    Check(CheckArgs),
}

/// The arguments of `policy check`.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// The tool manifest that declares the tool.
    #[arg(long)]
    pub manifest: PathBuf,
    /// The name of the tool to call.
    #[arg(long)]
    pub tool: String,
    /// The tool arguments, as a JSON object.
    #[arg(long, default_value = "{}")]
    pub args: String,
    /// The user policy file. Without it, the default policy applies.
    #[arg(long)]
    pub policy: Option<PathBuf>,
    /// The task is unattended: a scheduled task or a reminder firing.
    #[arg(long)]
    pub unattended: bool,
    /// The session is locked.
    #[arg(long)]
    pub locked: bool,
    /// Suppose a separate trusted session existed and had been proven
    /// (addendum A section 19, item 2). It does not exist; this asks what
    /// the engine would decide if it did, and performs nothing. Without the
    /// flag only classes 0 and 1 can be allowed. `--trusted-path` is the
    /// former name of the flag.
    #[arg(long = "trusted-session-proven", alias = "trusted-path")]
    pub trusted_session_proven: bool,
    /// A repair session expires at this time, in seconds since the Unix epoch.
    #[arg(long, requires = "repair_session_purpose")]
    pub repair_session_until: Option<u64>,
    /// The time the repair session was opened. Without it, the session is
    /// taken to have the default length of 15 minutes.
    #[arg(long, requires = "repair_session_until")]
    pub repair_session_opened: Option<u64>,
    /// The stated purpose of the repair session.
    #[arg(long, requires = "repair_session_until")]
    pub repair_session_purpose: Option<String>,
    /// A standing approval, as JSON with "service", "tool" and "arguments".
    /// May be given more than once. It is checked and then NOT honoured:
    /// standing approvals reach the engine only from the policy store, which
    /// does not exist yet (addendum A section 18, item 1). The output says
    /// how many were ignored.
    #[arg(long = "standing-approval")]
    pub standing_approvals: Vec<String>,
    /// The model flagged the action as consequential.
    #[arg(long)]
    pub model_flags_consequential: bool,
    /// The current time, in seconds since the Unix epoch. Defaults to the clock.
    #[arg(long)]
    pub now: Option<u64>,
    /// The task identifier written to the audit record.
    #[arg(long, default_value = "cli")]
    pub task: String,
    /// Print the decision and its audit record as JSON.
    #[arg(long)]
    pub json: bool,
}

/// Run a parsed command line. Returns the exit status.
///
/// Results go to `out` and problems to `err`. An error is returned only if
/// writing to either fails.
pub fn run(cli: &Cli, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<u8> {
    match &cli.command {
        TopCommand::Manifest {
            command: ManifestCommand::Validate { file },
        } => manifest_validate(file, out, err),
        TopCommand::Policy {
            command: PolicyCommand::Check(args),
        } => policy_check(args, out, err),
        TopCommand::Run(args) => crate::textui::run_task(args, out, err),
        TopCommand::Chat(args) => crate::textui::run_chat(args, out, err),
        TopCommand::Serve(args) => crate::daemon::run_serve(args, err),
        TopCommand::Schedule { command } => crate::daemon::run_schedule(command, out, err),
        TopCommand::Key { command } => crate::daemon::run_key(command, out, err),
        TopCommand::Models { command } => crate::daemon::run_models(command, out, err),
    }
}

fn manifest_validate(file: &Path, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<u8> {
    match load_manifest(file) {
        Ok(manifest) => {
            writeln!(
                out,
                "valid: {} declares {} tools",
                manifest.service(),
                manifest.tools().len()
            )?;
            for tool in manifest.tools() {
                writeln!(out, "  class {}  {}", tool.class(), tool.name())?;
            }
            Ok(EXIT_ALLOW)
        }
        Err(problems) => {
            report(&problems, err)?;
            Ok(EXIT_INPUT_ERROR)
        }
    }
}

fn policy_check(args: &CheckArgs, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<u8> {
    let (decision, record) = match evaluate(args) {
        Ok(result) => result,
        Err(problems) => {
            report(&problems, err)?;
            return Ok(EXIT_INPUT_ERROR);
        }
    };
    // Checked, and not passed to the engine: see `evaluate`.
    let ignored = args.standing_approvals.len();

    if args.json {
        let audit: serde_json::Map<String, Value> = record
            .journal_fields()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), Value::String(value)))
            .collect();
        let document = json!({
            "decision": decision.verdict().as_str(),
            "class": decision.class(),
            "reason": decision.reason().as_str(),
            "error_code": decision.code().map(Code::as_str),
            "prompt": decision.prompt(),
            "standing_approvals_ignored": ignored,
            "audit": audit,
        });
        writeln!(out, "{document}")?;
    } else {
        writeln!(out, "decision: {}", decision.verdict().as_str())?;
        writeln!(out, "class: {}", decision.class())?;
        writeln!(out, "reason: {}", decision.reason().as_str())?;
        if let Some(code) = decision.code() {
            writeln!(out, "error_code: {code}")?;
        }
        if let Some(prompt) = decision.prompt() {
            writeln!(out, "prompt: {prompt}")?;
        }
        if ignored > 0 {
            writeln!(
                out,
                "standing approvals: {ignored} given, not honoured (no policy store exists yet)"
            )?;
        }
    }
    Ok(exit_status(&decision))
}

/// The exit status for a decision. Only `allow` is 0.
pub fn exit_status(decision: &Decision) -> u8 {
    match decision.verdict() {
        Verdict::Allow => EXIT_ALLOW,
        Verdict::Ask { .. } => EXIT_ASK,
        Verdict::Authenticate { .. } => EXIT_AUTHENTICATE,
        Verdict::Defer => EXIT_DEFER,
        Verdict::Refuse => EXIT_REFUSE,
    }
}

fn evaluate(args: &CheckArgs) -> Result<(Decision, AuditRecord), Vec<Problem>> {
    let manifest = load_manifest(&args.manifest)?;

    // A repeated key is an error: what is decided, shown and recorded must be
    // one reading of the text.
    let arguments = strict::object_from_str(&args.args).map_err(|error| {
        invalid_input(format!(
            "--args must be a JSON object with no repeated key: {error}"
        ))
    })?;
    let call = manifest.tool_call(&args.tool, arguments)?;

    let policy = match &args.policy {
        Some(path) => UserPolicy::from_json(&read(path)?)?,
        None => UserPolicy::default(),
    };

    // A standing approval given here is checked for its form and goes no
    // further. Only a policy store can hand the engine an approval
    // (`HeldApprovals`), and this program is not one: a text typed on a
    // command line is exactly what addendum A section 18, item 1 says must
    // not be honoured.
    for text in &args.standing_approvals {
        serde_json::from_str::<StandingApproval>(text)
            .map_err(|error| invalid_input(format!("--standing-approval is not valid: {error}")))?;
    }

    let repair_session = match (&args.repair_session_purpose, args.repair_session_until) {
        (Some(purpose), Some(until)) => Some(RepairSession {
            purpose: purpose.clone(),
            opened_at: Timestamp(
                args.repair_session_opened
                    .unwrap_or(until.saturating_sub(DEFAULT_REPAIR_SESSION_SECONDS)),
            ),
            expires_at: Timestamp(until),
            ended: false,
        }),
        _ => None,
    };

    let context = Context {
        presence: if args.unattended {
            Presence::Unattended
        } else {
            Presence::Attended
        },
        session_locked: args.locked,
        trusted_session_proven: args.trusted_session_proven,
        repair_session,
        standing_approvals: HeldApprovals::none(),
        model_flags_consequential: args.model_flags_consequential,
        // The command decides tool calls only, which have no target window.
        target_window_owner: WindowOwner::Unknown,
    };
    let now = Timestamp(args.now.unwrap_or_else(clock_seconds));

    let action = Action::Tool(call);
    let decision = decide(&action, &context, &policy, now);
    let meta = AuditMeta {
        task: &args.task,
        provider: "none",
    };
    let record = AuditRecord::new(&meta, &action, &context, &decision);
    Ok((decision, record))
}

/// The clock, in seconds since the Unix epoch.
///
/// If the clock cannot be read, the result is the latest possible time, at
/// which every repair session has expired.
fn clock_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(u64::MAX, |elapsed| elapsed.as_secs())
}

fn invalid_input(detail: String) -> Vec<Problem> {
    vec![Problem::new(Code::InputInvalid, detail)]
}

fn read(path: &Path) -> Result<String, Vec<Problem>> {
    fs::read_to_string(path).map_err(|error| {
        vec![Problem::new(
            Code::InputUnreadable,
            format!("{}: {error}", path.display()),
        )]
    })
}

fn load_manifest(path: &Path) -> Result<ToolManifest, Vec<Problem>> {
    ToolManifest::from_json(&read(path)?)
}

fn report(problems: &[Problem], err: &mut dyn Write) -> io::Result<()> {
    for problem in problems {
        writeln!(err, "error: {problem}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_command_line_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn the_trusted_path_is_off_unless_stated() {
        let cli = Cli::try_parse_from([
            "tur-pax",
            "policy",
            "check",
            "--manifest",
            "m.json",
            "--tool",
            "t",
        ])
        .expect("the command line is valid");
        let TopCommand::Policy {
            command: PolicyCommand::Check(args),
        } = cli.command
        else {
            panic!("expected policy check");
        };
        assert!(!args.trusted_session_proven);
        assert!(!args.unattended);
        assert!(!args.locked);
        assert!(!args.model_flags_consequential);
        assert_eq!(args.args, "{}");
        assert!(args.standing_approvals.is_empty());
    }

    #[test]
    fn a_repair_session_needs_both_a_purpose_and_an_expiry() {
        let base = [
            "tur-pax",
            "policy",
            "check",
            "--manifest",
            "m.json",
            "--tool",
            "t",
        ];
        let mut only_until = base.to_vec();
        only_until.extend(["--repair-session-until", "10"]);
        assert!(Cli::try_parse_from(only_until).is_err());

        let mut only_purpose = base.to_vec();
        only_purpose.extend(["--repair-session-purpose", "fix"]);
        assert!(Cli::try_parse_from(only_purpose).is_err());

        let mut both = base.to_vec();
        both.extend([
            "--repair-session-until",
            "10",
            "--repair-session-purpose",
            "fix",
        ]);
        assert!(Cli::try_parse_from(both).is_ok());
    }

    #[test]
    fn a_repair_session_without_an_opening_time_has_the_default_length() {
        let cli = Cli::try_parse_from([
            "tur-pax",
            "policy",
            "check",
            "--manifest",
            "m.json",
            "--tool",
            "t",
            "--repair-session-until",
            "5000",
            "--repair-session-purpose",
            "fix",
        ])
        .expect("the command line is valid");
        let TopCommand::Policy {
            command: PolicyCommand::Check(args),
        } = cli.command
        else {
            panic!("expected policy check");
        };
        assert_eq!(args.repair_session_until, Some(5000));
        assert_eq!(args.repair_session_opened, None);

        // The opening time needs an expiry.
        assert!(
            Cli::try_parse_from([
                "tur-pax",
                "policy",
                "check",
                "--manifest",
                "m.json",
                "--tool",
                "t",
                "--repair-session-opened",
                "4000",
            ])
            .is_err()
        );
    }

    #[test]
    fn an_unreadable_manifest_is_an_input_error() {
        let cli = Cli::try_parse_from([
            "tur-pax",
            "manifest",
            "validate",
            "/nonexistent/tur-pax-test/manifest.json",
        ])
        .expect("the command line is valid");
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let status = run(&cli, &mut out, &mut err).expect("writing to memory succeeds");
        assert_eq!(status, EXIT_INPUT_ERROR);
        assert!(out.is_empty());
        let message = String::from_utf8(err).expect("the message is UTF-8");
        assert!(
            message.starts_with("error: TUR-AI-INPUT-001: "),
            "{message}"
        );
    }
}
