// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `tur-update` command line.
//!
//! Every command reads and prints. None changes the system:
//!
//! * `status`: the versions on this computer and the state of updates.
//! * `check`: a preview: may a check for updates run now, and what would the
//!   update manager do next by itself, and for a person who asked.
//! * `plan stage|rollback|pin|unpin`: the decision for the operation, the
//!   exact commands it would run, and their effects. The commands are
//!   printed, never run.
//! * `commands`: every command of another program this tool names.
//! * `codes`: the `TUR-UPDATE-*` registry.
//!
//! The deployments come from one of two places, and the command line says
//! which. `--from` names a file holding what `bootc status --format=json`
//! printed earlier; then the tool runs no program at all. `--live` asks
//! about this machine: the tool runs `/usr/bin/bootc status --format=json`,
//! which prints the deployments and changes nothing, and reads what it
//! prints ([`crate::probe`]). It runs nothing else. On an installed system
//! that needs root.
//!
//! Everything else a decision depends on is given as options: the
//! boot-health state as a file (`--boot-health`, what `tur-boot-health
//! status --json` printed), and power, network, free space, a repair session
//! and the two settings as plain options. What is not given is unknown, and
//! the policy says what it does with each unknown. The tool takes no setting
//! from the process surroundings.
//!
//! Exit status: 0 when the answer was printed, whatever the answer is: a
//! decision to wait or to refuse is an answer. 1 when there is no answer: the
//! state could not be read, or the output could not be written. 2 for a
//! usage error.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::SCHEMA_VERSION;
use crate::bootc::{self, ParseError};
use crate::codes::{CodeEntry, UpdateCode, registry};
use crate::conditions::{Conditions, Metered, Network, Power, Settings};
use crate::health::{BootHealth, parse_boot_health};
use crate::plan::{
    CHECK, COMMANDS, ExternalCommand, Plan, plan_pin, plan_rollback, plan_stage, plan_unpin,
};
use crate::policy::{Action, Decision, Outcome, Requester, decide_stage, next_step};
use crate::probe::{CommandRunner, ProbeError, SystemRunner, live_status};
use crate::state::{Slot, SystemState, UpdateKnowledge, UpdateState, printable, short_digest};

/// The parsed command line.
#[derive(Debug, Parser)]
#[command(
    name = "tur-update",
    version,
    about = "TUR update manager, first slice: shows the versions on this computer, previews \
             what an update would do and prints the plan. It never changes the system."
)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show the versions on this computer and the state of updates
    Status(StatusArgs),
    /// Preview what a check for updates, and the update manager, would do next
    Check(CheckArgs),
    /// Print the commands an operation would run, and their effects, without running them
    Plan(PlanArgs),
    /// List every command of another program this tool names
    Commands(ListArgs),
    /// List the TUR-UPDATE error codes
    Codes(ListArgs),
}

/// Where the deployments and the boot-health state come from.
#[derive(Debug, Args)]
pub struct SourceArgs {
    /// File holding the output of `bootc status --format=json`
    #[arg(
        long,
        value_name = "FILE",
        required_unless_present = "live",
        conflicts_with = "live"
    )]
    pub from: Option<PathBuf>,
    /// Ask about this machine: run `bootc status --format=json` and read
    /// what it prints. Nothing else is run and nothing is changed. Needs
    /// root on an installed system
    #[arg(long)]
    pub live: bool,
    /// File holding the output of `tur-boot-health status --json`. Without
    /// it the boot-health state is unknown
    #[arg(long, value_name = "FILE")]
    pub boot_health: Option<PathBuf>,
    /// Print JSON instead of text
    #[arg(long)]
    pub json: bool,
}

/// What the computer runs on, on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PowerArg {
    /// Not known
    Unknown,
    /// Mains power
    Mains,
    /// Battery
    Battery,
}

/// The network, on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum NetworkArg {
    /// Not known
    Unknown,
    /// Connected
    Online,
    /// Not connected
    Offline,
}

/// Whether the connection is metered, on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MeteredArg {
    /// Not known
    Unknown,
    /// Metered
    Yes,
    /// Not metered
    No,
}

/// The conditions a decision is made under.
#[derive(Debug, Args)]
pub struct ConditionArgs {
    /// What the computer runs on
    #[arg(long, value_enum, value_name = "SOURCE", default_value = "unknown")]
    pub power: PowerArg,
    /// The battery charge, with --power battery
    #[arg(long, value_name = "PERCENT", value_parser = clap::value_parser!(u8).range(0..=100))]
    pub battery_percent: Option<u8>,
    /// The network
    #[arg(long, value_enum, value_name = "STATE", default_value = "unknown")]
    pub network: NetworkArg,
    /// Whether the connection is metered
    #[arg(long, value_enum, value_name = "METERED", default_value = "unknown")]
    pub metered: MeteredArg,
    /// Free space on the system disk
    #[arg(long, value_name = "BYTES")]
    pub free_bytes: Option<u64>,
    /// The size of the update on disk, if it is known
    #[arg(long, value_name = "BYTES")]
    pub update_bytes: Option<u64>,
    /// A repair session is open
    #[arg(long)]
    pub repair_session: bool,
    /// The setting "Download updates automatically" is off
    #[arg(long)]
    pub no_automatic_download: bool,
    /// The setting "Download on metered connections" is on
    #[arg(long)]
    pub download_on_metered: bool,
}

/// Arguments of `status`.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Where the state comes from.
    #[command(flatten)]
    pub source: SourceArgs,
}

/// Arguments of `check`.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Where the state comes from.
    #[command(flatten)]
    pub source: SourceArgs,
    /// The conditions.
    #[command(flatten)]
    pub conditions: ConditionArgs,
}

/// Arguments of `plan`.
#[derive(Debug, Args)]
pub struct PlanArgs {
    /// The operation to plan.
    #[command(subcommand)]
    pub operation: PlanOperation,
}

/// The operations that can be planned.
#[derive(Debug, Subcommand)]
pub enum PlanOperation {
    /// Download the known update, stage it beside the running system and release it
    Stage(PlanStageArgs),
    /// Go back to the previous version
    Rollback(PlanRollbackArgs),
    /// Keep a version: it is never removed when updates are installed
    Pin(PlanPinArgs),
    /// Release a kept version
    Unpin(PlanPinArgs),
}

/// Arguments of `plan stage`.
#[derive(Debug, Args)]
pub struct PlanStageArgs {
    /// Where the state comes from.
    #[command(flatten)]
    pub source: SourceArgs,
    /// The conditions.
    #[command(flatten)]
    pub conditions: ConditionArgs,
    /// Decide as for a person who asked for the update, not as for the
    /// update manager acting by itself
    #[arg(long)]
    pub requested: bool,
}

/// Arguments of `plan rollback`.
#[derive(Debug, Args)]
pub struct PlanRollbackArgs {
    /// Where the state comes from.
    #[command(flatten)]
    pub source: SourceArgs,
    /// The conditions.
    #[command(flatten)]
    pub conditions: ConditionArgs,
}

/// The version a keep or release request names, on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DeploymentArg {
    /// The version in use
    Booted,
    /// The previous version
    Rollback,
}

/// Arguments of `plan pin` and `plan unpin`.
#[derive(Debug, Args)]
pub struct PlanPinArgs {
    /// Where the state comes from.
    #[command(flatten)]
    pub source: SourceArgs,
    /// The version to keep or release
    #[arg(long, value_enum, value_name = "WHICH", default_value = "booted")]
    pub deployment: DeploymentArg,
}

/// Arguments of `commands` and `codes`.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Print JSON instead of text
    #[arg(long)]
    pub json: bool,
}

/// Why a command failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A condition with a registered error code.
    Coded {
        /// The code.
        code: UpdateCode,
        /// What exactly went wrong.
        detail: String,
    },
    /// The command line asked for something that makes no sense.
    Usage(String),
    /// The result could not be written or serialised.
    Output(String),
}

impl Failure {
    /// The process exit status for this failure.
    pub fn exit_status(&self) -> u8 {
        match self {
            Failure::Usage(_) => 2,
            Failure::Coded { .. } | Failure::Output(_) => 1,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Coded { code, detail } => {
                let entry = code.entry();
                write!(
                    f,
                    "{code}\n{}\n\nAction:\n{}\n\nTechnical detail:\n{detail}",
                    entry.user_message, entry.recovery_action
                )
            }
            Failure::Usage(message) => write!(f, "error: {message}"),
            Failure::Output(message) => write!(f, "error: cannot write output: {message}"),
        }
    }
}

string_enum! {
    /// Where the deployments of a report came from.
    pub enum Source {
        /// A file given with `--from`.
        File => "file",
        /// This machine, with `--live`.
        Live => "live",
    }
}

/// One line of "Versions on this computer".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Version {
    /// Where the deployment stands.
    pub slot: Slot,
    /// Its name for a person.
    pub name: String,
    /// The manifest digest of its image, if it is an image.
    pub digest: Option<String>,
    /// When its image was built, if the image says.
    pub built: Option<String>,
    /// It is kept.
    pub kept: bool,
    /// It is staged but held.
    pub held: bool,
    /// It failed its start-up trial on this computer, and is not the version
    /// in use. `None` when the boot-health state is not known.
    pub failed_to_start: Option<bool>,
}

fn versions(state: &SystemState, health: Option<&BootHealth>) -> Vec<Version> {
    state
        .deployments()
        .into_iter()
        .map(|deployment| Version {
            slot: deployment.slot,
            name: deployment.label(),
            digest: deployment.digest().map(str::to_owned),
            built: deployment
                .image
                .as_ref()
                .and_then(|image| image.timestamp.clone()),
            kept: deployment.pinned,
            held: deployment.download_only,
            // The version in use is running: an old entry for it in the
            // rejected list, left from before a person tried it again, is
            // not a reason to call it failed.
            failed_to_start: health.map(|health| {
                deployment.slot != Slot::Booted
                    && deployment
                        .digest()
                        .is_some_and(|digest| health.is_rejected(digest))
            }),
        })
        .collect()
}

#[derive(Serialize)]
struct StatusReport<'a> {
    schema_version: u32,
    kind: &'static str,
    source: Source,
    state: UpdateState,
    message: String,
    versions: Vec<Version>,
    system: &'a SystemState,
    boot_health: Option<&'a BootHealth>,
}

#[derive(Serialize)]
struct CheckReport<'a> {
    schema_version: u32,
    kind: &'static str,
    source: Source,
    state: UpdateState,
    message: String,
    update: &'a UpdateKnowledge,
    conditions: &'a Conditions,
    check: &'a Decision,
    check_command: Vec<String>,
    automatic_action: Action,
    automatic_stage: &'a Decision,
    requested_stage: &'a Decision,
}

#[derive(Serialize)]
struct PlanReport<'a> {
    schema_version: u32,
    kind: &'static str,
    source: Source,
    requester: Requester,
    conditions: &'a Conditions,
    plan: &'a Plan,
}

#[derive(Serialize)]
struct CommandReport {
    schema_version: u32,
    kind: &'static str,
    commands: &'static [ExternalCommand],
}

#[derive(Serialize)]
struct CodeReport {
    schema_version: u32,
    kind: &'static str,
    codes: Vec<CodeEntry>,
}

/// Runs the command line and returns the process exit status.
///
/// `args` includes the program name. Results go to `stdout`; usage errors
/// and failures go to `stderr`. With `--live`, `bootc status` is run on this
/// machine.
pub fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run_with_runner(args, &SystemRunner, stdout, stderr)
}

/// As [`run`], with the runner the live read uses given by the caller. A
/// test passes one that runs nothing.
pub fn run_with_runner<I, T>(
    args: I,
    runner: &dyn CommandRunner,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            // Help and version requests arrive here too; they are not errors.
            let text = error.to_string();
            return if error.use_stderr() {
                let _ = stderr.write_all(text.as_bytes());
                2
            } else {
                let _ = stdout.write_all(text.as_bytes());
                0
            };
        }
    };
    let result = match &cli.command {
        Command::Status(args) => status(args, runner, stdout),
        Command::Check(args) => check(args, runner, stdout),
        Command::Plan(args) => plan(args, runner, stdout),
        Command::Commands(args) => commands(args, stdout),
        Command::Codes(args) => codes(args, stdout),
    };
    match result {
        Ok(()) => 0,
        Err(failure) => {
            let _ = writeln!(stderr, "{failure}");
            failure.exit_status()
        }
    }
}

fn read(path: &Path, code: UpdateCode) -> Result<String, Failure> {
    fs::read_to_string(path).map_err(|error| Failure::Coded {
        code,
        detail: format!("cannot read {}: {error}", path.display()),
    })
}

/// The deployments a request asks about: a file captured earlier, or this
/// machine.
fn load_state(
    source: &SourceArgs,
    runner: &dyn CommandRunner,
) -> Result<(SystemState, Source), Failure> {
    let (text, from, name) = match &source.from {
        Some(path) => (
            read(path, UpdateCode::StateUnreadable)?,
            Source::File,
            path.display().to_string(),
        ),
        None => {
            // `--live`: the command line requires one of the two.
            let text = live_status(runner).map_err(|error| Failure::Coded {
                code: match &error {
                    ProbeError::Command(_) => UpdateCode::StateUnreadable,
                    ProbeError::NeedsAdministrator(_) => UpdateCode::NeedsAdministrator,
                },
                detail: error.to_string(),
            })?;
            (text, Source::Live, "this machine".to_owned())
        }
    };
    let host = bootc::parse_status(&text).map_err(|error| Failure::Coded {
        code: match &error {
            ParseError::Shape(_) => UpdateCode::StateUnreadable,
            ParseError::Format { .. } => UpdateCode::FormatUnknown,
        },
        detail: format!("{name}: {error}"),
    })?;
    let state = SystemState::from_host(&host).map_err(|error| Failure::Coded {
        code: UpdateCode::NotBootcHost,
        detail: format!("{name}: {error}"),
    })?;
    Ok((state, from))
}

fn load_health(source: &SourceArgs) -> Result<Option<BootHealth>, Failure> {
    let Some(path) = &source.boot_health else {
        return Ok(None);
    };
    let text = read(path, UpdateCode::BootHealthUnreadable)?;
    parse_boot_health(&text)
        .map(Some)
        .map_err(|error| Failure::Coded {
            code: UpdateCode::BootHealthUnreadable,
            detail: format!("{}: {error}", path.display()),
        })
}

fn conditions_from(
    arguments: &ConditionArgs,
    boot_health: Option<BootHealth>,
) -> Result<Conditions, Failure> {
    let power = match (arguments.power, arguments.battery_percent) {
        (PowerArg::Battery, percent) => Power::Battery { percent },
        (_, Some(_)) => {
            return Err(Failure::Usage(
                "--battery-percent needs --power battery".to_owned(),
            ));
        }
        (PowerArg::Mains, None) => Power::Mains,
        (PowerArg::Unknown, None) => Power::Unknown,
    };
    let metered = match arguments.metered {
        MeteredArg::Unknown => Metered::Unknown,
        MeteredArg::Yes => Metered::Yes,
        MeteredArg::No => Metered::No,
    };
    let network = match (arguments.network, metered) {
        (NetworkArg::Offline, Metered::Unknown) => Network::Offline,
        (NetworkArg::Offline, _) => {
            return Err(Failure::Usage(
                "--metered says nothing about a network that is offline".to_owned(),
            ));
        }
        (NetworkArg::Online, metered) => Network::Online { metered },
        (NetworkArg::Unknown, Metered::Unknown) => Network::Unknown,
        // A connection that is known to be metered, or not, is a connection.
        (NetworkArg::Unknown, metered) => Network::Online { metered },
    };
    Ok(Conditions {
        power,
        network,
        free_bytes: arguments.free_bytes,
        update_bytes: arguments.update_bytes,
        boot_health,
        repair_session: arguments.repair_session,
        settings: Settings {
            automatic_download: !arguments.no_automatic_download,
            download_on_metered: arguments.download_on_metered,
        },
        ..Conditions::default()
    })
}

fn emit(out: &mut dyn Write, text: &str) -> Result<(), Failure> {
    out.write_all(text.as_bytes())
        .map_err(|error| Failure::Output(error.to_string()))
}

fn emit_json<T: Serialize>(out: &mut dyn Write, report: &T) -> Result<(), Failure> {
    let mut json = serde_json::to_string_pretty(report)
        .map_err(|error| Failure::Output(format!("cannot serialise the report: {error}")))?;
    json.push('\n');
    emit(out, &json)
}

fn role(slot: Slot) -> &'static str {
    match slot {
        Slot::Staged => "waiting",
        Slot::Booted => "in use",
        Slot::Rollback => "previous",
        Slot::Other => "other",
    }
}

fn describe_versions(versions: &[Version]) -> String {
    let mut text = String::from("Versions on this computer\n");
    for version in versions {
        let mut marks: Vec<&str> = Vec::new();
        if version.kept {
            marks.push("kept");
        }
        if version.held {
            marks.push("held");
        }
        if version.failed_to_start == Some(true) {
            marks.push("failed to start here");
        }
        let digest = version
            .digest
            .as_deref()
            .map_or_else(|| "not an image".to_owned(), short_digest);
        let built = version
            .built
            .as_deref()
            .map_or_else(String::new, |stamp| format!("  built {}", printable(stamp)));
        let marks = if marks.is_empty() {
            String::new()
        } else {
            format!("  [{}]", marks.join(", "))
        };
        text.push_str(&format!(
            "  {:<9} {}  {digest}{built}{marks}\n",
            role(version.slot),
            version.name
        ));
    }
    text
}

fn describe_health(health: Option<&BootHealth>) -> String {
    let Some(health) = health else {
        return "Boot health: not known (give --boot-health)\n".to_owned();
    };
    let mut text = String::from("Boot health\n");
    text.push_str(&format!(
        "  Attempt counting:   {}\n",
        if health.counting_usable {
            "usable"
        } else {
            "NOT usable"
        }
    ));
    text.push_str(&format!(
        "  Trial in progress:  {}\n",
        if health.trial_in_progress {
            "yes"
        } else {
            "no"
        }
    ));
    text.push_str(&format!(
        "  This start:         {}\n",
        health
            .verdict
            .map_or("not judged yet", |verdict| verdict.as_str())
    ));
    text.push_str(&format!(
        "  Failed to start:    {} version(s)\n",
        health.rejected.len()
    ));
    if let Some(notice) = &health.notice {
        text.push_str(&format!(
            "  Notice:             {} ({})\n",
            printable(&notice.text),
            printable(&notice.error_code)
        ));
    }
    text
}

fn describe_follows(state: &SystemState) -> String {
    match &state.follows {
        Some(follows) => format!(
            "Follows:  {} ({}), signature: {}\n",
            printable(&follows.reference),
            printable(&follows.transport),
            follows.signature.kind
        ),
        None => "Follows:  no image\n".to_owned(),
    }
}

fn describe_decision(title: &str, decision: &Decision) -> String {
    let mut text = format!(
        "{title}: {} ({})\n  {}\n",
        decision.outcome, decision.code, decision.user_message
    );
    if !decision.reason.is_empty() {
        text.push_str(&format!("  Reason: {}\n", decision.reason));
    }
    if decision.outcome != Outcome::Proceed {
        text.push_str(&format!("  What to do: {}\n", decision.recovery_action));
    }
    if !decision.also.is_empty() {
        let also: Vec<&str> = decision.also.iter().map(|code| code.as_str()).collect();
        text.push_str(&format!("  Also: {}\n", also.join(", ")));
    }
    for note in &decision.notes {
        text.push_str(&format!("  Note: {note}\n"));
    }
    text
}

fn status(
    args: &StatusArgs,
    runner: &dyn CommandRunner,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let (state, source) = load_state(&args.source, runner)?;
    let health = load_health(&args.source)?;
    let versions = versions(&state, health.as_ref());
    if args.source.json {
        return emit_json(
            out,
            &StatusReport {
                schema_version: SCHEMA_VERSION,
                kind: "status",
                source,
                state: state.update_state(),
                message: state.update_message(),
                versions,
                system: &state,
                boot_health: health.as_ref(),
            },
        );
    }
    let mut text = format!("{}\n", state.update_message());
    text.push_str(&format!("State:    {}\n", state.update_state()));
    text.push_str(&describe_follows(&state));
    if state.rollback_queued {
        text.push_str("Next restart: the previous version\n");
    }
    if state.read_only {
        text.push_str("The system runs from read-only media.\n");
    }
    if let Some(overlay) = &state.usr_overlay {
        text.push_str(&format!(
            "An overlay is mounted on /usr ({}, {}).\n",
            printable(&overlay.persistence),
            printable(&overlay.access_mode)
        ));
    }
    text.push('\n');
    text.push_str(&describe_versions(&versions));
    text.push('\n');
    text.push_str(&describe_health(health.as_ref()));
    emit(out, &text)
}

fn check(args: &CheckArgs, runner: &dyn CommandRunner, out: &mut dyn Write) -> Result<(), Failure> {
    let health = load_health(&args.source)?;
    let conditions = conditions_from(&args.conditions, health)?;
    let (state, source) = load_state(&args.source, runner)?;
    let next = next_step(&state, &conditions);
    let requested = decide_stage(&state, &conditions, Requester::Person);
    let check_command = CHECK.argv(None);
    if args.source.json {
        return emit_json(
            out,
            &CheckReport {
                schema_version: SCHEMA_VERSION,
                kind: "check",
                source,
                state: state.update_state(),
                message: state.update_message(),
                update: &state.update,
                conditions: &conditions,
                check: &next.check,
                check_command,
                automatic_action: next.action,
                automatic_stage: &next.stage,
                requested_stage: &requested,
            },
        );
    }
    let mut text = format!("{}\n", state.update_message());
    text.push_str(&describe_follows(&state));
    text.push('\n');
    text.push_str(&describe_decision("Check for updates", &next.check));
    text.push_str(&format!(
        "  A check would run: {}\n  This command does not run it.\n\n",
        check_command.join(" ")
    ));
    text.push_str(&format!(
        "By itself, the update manager would now: {}\n",
        next.action
    ));
    text.push_str(&describe_decision("Download, by itself", &next.stage));
    text.push('\n');
    text.push_str(&describe_decision("Download, if you ask", &requested));
    emit(out, &text)
}

fn describe_plan(plan: &Plan, requester: Option<Requester>) -> String {
    let mut text = match requester {
        Some(requester) => format!("Plan: {} (request: {requester})\n", plan.operation),
        None => format!("Plan: {}\n", plan.operation),
    };
    text.push_str(&describe_decision("Decision", &plan.decision));
    text.push('\n');
    if plan.steps.is_empty() {
        text.push_str("No command would be run.\n");
        return text;
    }
    text.push_str(
        "This command runs nothing. These commands would be run, as administrator, in this \
         order:\n",
    );
    for (index, step) in plan.steps.iter().enumerate() {
        text.push_str(&format!(
            "  {}. {}\n     {} (changes: {})\n",
            index + 1,
            step.command.join(" "),
            step.effect,
            step.changes
        ));
    }
    text.push_str("\nWhat would happen\n");
    for effect in &plan.effects {
        text.push_str(&format!("  - {effect}\n"));
    }
    if plan.restart_required {
        text.push_str(
            "\nThe change takes effect at a restart. No command here restarts the computer.\n",
        );
    }
    text
}

fn plan(args: &PlanArgs, runner: &dyn CommandRunner, out: &mut dyn Write) -> Result<(), Failure> {
    let slot = |deployment: DeploymentArg| match deployment {
        DeploymentArg::Booted => Slot::Booted,
        DeploymentArg::Rollback => Slot::Rollback,
    };
    // Going back, keeping and releasing are always a person's request.
    let (source_args, conditions, requester) = match &args.operation {
        PlanOperation::Stage(stage) => {
            let health = load_health(&stage.source)?;
            let requester = if stage.requested {
                Requester::Person
            } else {
                Requester::Automatic
            };
            (
                &stage.source,
                conditions_from(&stage.conditions, health)?,
                requester,
            )
        }
        PlanOperation::Rollback(rollback) => {
            let health = load_health(&rollback.source)?;
            (
                &rollback.source,
                conditions_from(&rollback.conditions, health)?,
                Requester::Person,
            )
        }
        PlanOperation::Pin(pin) | PlanOperation::Unpin(pin) => (
            &pin.source,
            Conditions {
                boot_health: load_health(&pin.source)?,
                ..Conditions::default()
            },
            Requester::Person,
        ),
    };
    let (state, source) = load_state(source_args, runner)?;
    let plan = match &args.operation {
        PlanOperation::Stage(_) => plan_stage(&state, &conditions, requester),
        PlanOperation::Rollback(_) => plan_rollback(&state, &conditions),
        PlanOperation::Pin(pin) => plan_pin(&state, slot(pin.deployment)),
        PlanOperation::Unpin(pin) => plan_unpin(&state, slot(pin.deployment)),
    };
    if source_args.json {
        return emit_json(
            out,
            &PlanReport {
                schema_version: SCHEMA_VERSION,
                kind: "plan",
                source,
                requester,
                conditions: &conditions,
                plan: &plan,
            },
        );
    }
    let shown = matches!(args.operation, PlanOperation::Stage(_)).then_some(requester);
    emit(out, &describe_plan(&plan, shown))
}

fn commands(args: &ListArgs, out: &mut dyn Write) -> Result<(), Failure> {
    if args.json {
        return emit_json(
            out,
            &CommandReport {
                schema_version: SCHEMA_VERSION,
                kind: "commands",
                commands: &COMMANDS,
            },
        );
    }
    let mut text = String::from(
        "Commands of other programs that tur-update names. It runs the first, and only with \
         --live; the others are printed in plans and never run.\n\n",
    );
    for command in &COMMANDS {
        let operand = if command.operands.is_empty() {
            String::new()
        } else {
            format!(" <{}>", command.operands.join("|"))
        };
        text.push_str(&format!(
            "{}{operand}\n  {}: changes {}; {}\n  {}\n\n",
            command.argv(None).join(" "),
            command.id,
            command.changes,
            if command.run_by_this_program {
                "run by tur-update"
            } else {
                "never run by tur-update"
            },
            command.purpose
        ));
    }
    emit(out, &text)
}

fn codes(args: &ListArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let entries = registry();
    if args.json {
        return emit_json(
            out,
            &CodeReport {
                schema_version: SCHEMA_VERSION,
                kind: "codes",
                codes: entries,
            },
        );
    }
    let mut text = String::new();
    for entry in entries {
        text.push_str(&format!(
            "{} [{}] {}\n  {}\n  What to do: {}\n\n",
            entry.code, entry.severity, entry.title, entry.user_message, entry.recovery_action
        ));
    }
    emit(out, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::CommandOutput;
    use std::cell::RefCell;

    /// Stands in for the machine: answers `bootc status` with a prepared
    /// text and records what was asked.
    struct FakeRunner {
        asked: RefCell<Vec<Vec<String>>>,
        status: Option<i32>,
        stdout: String,
        stderr: String,
        root: bool,
    }

    impl FakeRunner {
        fn printing(stdout: &str) -> FakeRunner {
            FakeRunner {
                asked: RefCell::new(Vec::new()),
                status: Some(0),
                stdout: stdout.to_owned(),
                stderr: String::new(),
                root: true,
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput, String> {
            let mut line = vec![program.to_owned()];
            line.extend(arguments.iter().map(|argument| (*argument).to_owned()));
            self.asked.borrow_mut().push(line);
            Ok(CommandOutput {
                status: self.status,
                stdout: self.stdout.clone().into_bytes(),
                stderr: self.stderr.clone().into_bytes(),
            })
        }

        fn is_root(&self) -> bool {
            self.root
        }
    }

    const BOOTED: &str = r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
        "spec":{"image":{"image":"example.org/tur:stable","transport":"registry"}},
        "status":{"booted":{"image":{"image":{"image":"example.org/tur:stable",
          "transport":"registry"},"imageDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaa",
          "architecture":"amd64","version":"0.3.2"},
          "cachedUpdate":{"image":{"image":"example.org/tur:stable","transport":"registry"},
          "imageDigest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbb","architecture":"amd64",
          "version":"0.4.0"},"incompatible":false,"pinned":false}}}"#;

    const NOT_A_HOST: &str = r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
        "status":{"booted":null}}"#;

    fn run_live(runner: &FakeRunner, arguments: &[&str]) -> (u8, String, String) {
        let mut line = vec!["tur-update"];
        line.extend(arguments);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let status = run_with_runner(line, runner, &mut stdout, &mut stderr);
        (
            status,
            String::from_utf8(stdout).expect("text"),
            String::from_utf8(stderr).expect("text"),
        )
    }

    #[test]
    fn live_reads_bootc_status_once_and_runs_nothing_else() {
        for arguments in [
            vec!["status", "--live"],
            vec!["status", "--live", "--json"],
            vec!["check", "--live", "--network", "online"],
            vec!["plan", "stage", "--live", "--requested"],
            vec!["plan", "rollback", "--live"],
            vec!["plan", "pin", "--live"],
            vec!["plan", "unpin", "--live", "--deployment", "rollback"],
        ] {
            let runner = FakeRunner::printing(BOOTED);
            let (status, stdout, stderr) = run_live(&runner, &arguments);
            assert_eq!(status, 0, "{arguments:?}: {stderr}");
            assert!(!stdout.is_empty(), "{arguments:?}");
            assert_eq!(
                *runner.asked.borrow(),
                [["/usr/bin/bootc", "status", "--format=json"]],
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn commands_that_need_no_state_run_no_program() {
        for arguments in [
            vec!["commands"],
            vec!["commands", "--json"],
            vec!["codes"],
            vec!["codes", "--json"],
            vec!["--help"],
            vec!["plan", "--help"],
            vec!["--version"],
        ] {
            let runner = FakeRunner::printing(BOOTED);
            let (status, stdout, _) = run_live(&runner, &arguments);
            assert_eq!(status, 0, "{arguments:?}");
            assert!(!stdout.is_empty(), "{arguments:?}");
            assert!(runner.asked.borrow().is_empty(), "{arguments:?}");
        }
    }

    #[test]
    fn a_live_status_report_says_it_is_live() {
        let runner = FakeRunner::printing(BOOTED);
        let (status, stdout, _) = run_live(&runner, &["status", "--live", "--json"]);
        assert_eq!(status, 0);
        let report: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
        assert_eq!(report["kind"], "status");
        assert_eq!(report["source"], "live");
        assert_eq!(report["state"], "available");
        assert_eq!(report["message"], "TUR 0.4.0 is available.");
        assert_eq!(report["versions"][0]["name"], "TUR 0.3.2");
        assert!(report["versions"][0]["failed_to_start"].is_null());
        assert!(report["boot_health"].is_null());
    }

    #[test]
    fn the_version_in_use_is_never_marked_as_failed() {
        let host = bootc::parse_status(BOOTED).expect("a document");
        let state = SystemState::from_host(&host).expect("a state");
        let mut health = BootHealth::healthy();
        health.rejected.push(crate::health::RejectedImage {
            digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            image: "example.org/tur:stable".to_owned(),
            version: "0.3.2".to_owned(),
            event_id: "e1".to_owned(),
        });
        assert!(health.is_rejected(state.booted.digest().expect("a digest")));
        // It runs: an entry left from before a person tried it again does
        // not make it a version that failed.
        let listed = versions(&state, Some(&health));
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].failed_to_start, Some(false));
        // Without boot health nothing is claimed either way.
        assert_eq!(versions(&state, None)[0].failed_to_start, None);
    }

    #[test]
    fn a_machine_that_is_not_a_bootc_host_has_its_code() {
        let runner = FakeRunner::printing(NOT_A_HOST);
        let (status, stdout, stderr) = run_live(&runner, &["status", "--live"]);
        assert_eq!(status, 1);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("TUR-UPDATE-STATE-002\n"), "{stderr}");
        assert!(stderr.contains("this machine"), "{stderr}");
    }

    #[test]
    fn a_failed_live_read_has_its_code_and_nothing_is_guessed() {
        // As root: the state could not be read.
        let mut runner = FakeRunner::printing(BOOTED);
        runner.status = Some(1);
        runner.stderr = "error: Status: Computing status: boom\n".to_owned();
        let (status, stdout, stderr) = run_live(&runner, &["check", "--live"]);
        assert_eq!(status, 1);
        assert!(stdout.is_empty(), "{stdout}");
        assert!(stderr.starts_with("TUR-UPDATE-STATE-001\n"), "{stderr}");
        assert!(stderr.contains("status 1"), "{stderr}");
        // Not as root: it says what is needed.
        runner.root = false;
        let (status, stdout, stderr) = run_live(&runner, &["status", "--live"]);
        assert_eq!(status, 1);
        assert!(stdout.is_empty(), "{stdout}");
        assert!(stderr.starts_with("TUR-UPDATE-STATE-004\n"), "{stderr}");
        // Another format.
        let other =
            FakeRunner::printing(r#"{"apiVersion":"org.containers.bootc/v2","kind":"BootcHost"}"#);
        let (status, _, stderr) = run_live(&other, &["status", "--live"]);
        assert_eq!(status, 1);
        assert!(stderr.starts_with("TUR-UPDATE-STATE-003\n"), "{stderr}");
        // Not the document at all.
        let garbage = FakeRunner::printing("hello");
        let (status, _, stderr) = run_live(&garbage, &["status", "--live"]);
        assert_eq!(status, 1);
        assert!(stderr.starts_with("TUR-UPDATE-STATE-001\n"), "{stderr}");
    }

    #[test]
    fn conditions_come_from_the_options_and_nowhere_else() {
        let parse = |arguments: &[&str]| -> Result<Conditions, Failure> {
            let mut line = vec!["tur-update", "check", "--live"];
            line.extend(arguments);
            let cli = Cli::try_parse_from(line).expect("a command line");
            let Command::Check(check) = cli.command else {
                panic!("not check");
            };
            conditions_from(&check.conditions, None)
        };
        // Nothing given: everything unknown, the settings at their defaults.
        assert_eq!(parse(&[]), Ok(Conditions::default()));
        let given = parse(&[
            "--power",
            "battery",
            "--battery-percent",
            "41",
            "--network",
            "online",
            "--metered",
            "yes",
            "--free-bytes",
            "1000",
            "--update-bytes",
            "10",
            "--repair-session",
            "--no-automatic-download",
            "--download-on-metered",
        ])
        .expect("conditions");
        assert_eq!(given.power, Power::Battery { percent: Some(41) });
        assert_eq!(
            given.network,
            Network::Online {
                metered: Metered::Yes
            }
        );
        assert_eq!(given.free_bytes, Some(1000));
        assert_eq!(given.update_bytes, Some(10));
        assert!(given.repair_session);
        assert!(!given.settings.automatic_download);
        assert!(given.settings.download_on_metered);
        // A metered connection is a connection.
        assert_eq!(
            parse(&["--metered", "no"]).expect("conditions").network,
            Network::Online {
                metered: Metered::No
            }
        );
        assert_eq!(
            parse(&["--power", "battery"]).expect("conditions").power,
            Power::Battery { percent: None }
        );
        // Combinations that say nothing are usage errors.
        for arguments in [
            vec!["--battery-percent", "50"],
            vec!["--power", "mains", "--battery-percent", "50"],
            vec!["--network", "offline", "--metered", "yes"],
        ] {
            let failure = parse(&arguments).expect_err("a usage error");
            assert_eq!(failure.exit_status(), 2, "{arguments:?}");
        }
    }

    #[test]
    fn failures_print_the_registry_text() {
        let failure = Failure::Coded {
            code: UpdateCode::NotBootcHost,
            detail: "x".to_owned(),
        };
        assert_eq!(failure.exit_status(), 1);
        assert_eq!(
            failure.to_string(),
            "TUR-UPDATE-STATE-002\nThis system does not receive TUR updates.\n\nAction:\nNo \
             action is needed. Updates apply to an installed TUR system.\n\nTechnical detail:\nx"
        );
        assert_eq!(Failure::Usage("u".to_owned()).to_string(), "error: u");
        assert_eq!(Failure::Output("o".to_owned()).exit_status(), 1);
    }
}
