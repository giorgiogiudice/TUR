// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The command-line interface for scripted callers.
//!
//! ```text
//! uos-agent manifest validate <file>
//! uos-agent policy check --manifest <file> --tool <name> [--args <json>]
//!                        [--policy <file>] [context flags] [--json]
//! ```
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
use crate::context::{Context, Presence, RepairSession, StandingApproval, Timestamp};
use crate::decision::{Decision, Verdict};
use crate::engine::decide;
use crate::manifest::ToolManifest;
use crate::policy::UserPolicy;

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

/// UOS AI control layer: tool manifests and the policy engine.
#[derive(Debug, Parser)]
#[command(name = "uos-agent", version)]
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
    /// The trusted confirmation path is available. Without this flag only
    /// classes 0 and 1 can be allowed.
    #[arg(long)]
    pub trusted_path: bool,
    /// A repair session is open until this time, in seconds since the Unix epoch.
    #[arg(long, requires = "repair_session_purpose")]
    pub repair_session_until: Option<u64>,
    /// The stated purpose of the repair session.
    #[arg(long, requires = "repair_session_until")]
    pub repair_session_purpose: Option<String>,
    /// A standing approval, as JSON with "service", "tool" and "arguments".
    /// May be given more than once.
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

    let arguments = match serde_json::from_str::<Value>(&args.args) {
        Ok(Value::Object(arguments)) => arguments,
        Ok(_) => return Err(invalid_input("--args must be a JSON object".to_owned())),
        Err(error) => return Err(invalid_input(format!("--args is not valid JSON: {error}"))),
    };
    let call = manifest.tool_call(&args.tool, arguments)?;

    let policy = match &args.policy {
        Some(path) => UserPolicy::from_json(&read(path)?)?,
        None => UserPolicy::default(),
    };

    let mut standing_approvals = Vec::with_capacity(args.standing_approvals.len());
    for text in &args.standing_approvals {
        let approval: StandingApproval = serde_json::from_str(text)
            .map_err(|error| invalid_input(format!("--standing-approval is not valid: {error}")))?;
        standing_approvals.push(approval);
    }

    let repair_session = match (&args.repair_session_purpose, args.repair_session_until) {
        (Some(purpose), Some(until)) => Some(RepairSession {
            purpose: purpose.clone(),
            expires_at: Timestamp(until),
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
        trusted_path_available: args.trusted_path,
        repair_session,
        standing_approvals,
        model_flags_consequential: args.model_flags_consequential,
    };
    let now = Timestamp(args.now.unwrap_or_else(clock_seconds));

    let action = Action::Tool(call);
    let decision = decide(&action, &context, &policy, now);
    let meta = AuditMeta {
        task: &args.task,
        provider: "none",
    };
    let record = AuditRecord::new(&meta, &action, &decision);
    Ok((decision, record))
}

fn clock_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
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
            "uos-agent",
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
        assert!(!args.trusted_path);
        assert!(!args.unattended);
        assert!(!args.locked);
        assert!(!args.model_flags_consequential);
        assert_eq!(args.args, "{}");
        assert!(args.standing_approvals.is_empty());
    }

    #[test]
    fn a_repair_session_needs_both_a_purpose_and_an_expiry() {
        let base = [
            "uos-agent",
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
    fn an_unreadable_manifest_is_an_input_error() {
        let cli = Cli::try_parse_from([
            "uos-agent",
            "manifest",
            "validate",
            "/nonexistent/uos-agent-test/manifest.json",
        ])
        .expect("the command line is valid");
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let status = run(&cli, &mut out, &mut err).expect("writing to memory succeeds");
        assert_eq!(status, EXIT_INPUT_ERROR);
        assert!(out.is_empty());
        let message = String::from_utf8(err).expect("the message is UTF-8");
        assert!(
            message.starts_with("error: UOS-AI-INPUT-001: "),
            "{message}"
        );
    }
}
