// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! `uos-runtime`: the command-line front end of the Universal Runtime Manager.
//!
//! ```text
//! uos-runtime inspect <path> [--json]            identify a file and recommend a runtime
//! uos-runtime profile validate <file>            validate an application profile
//! uos-runtime prefix <operation> <app-id> ...    manage an application's prefix
//! uos-runtime launch <profile> [-- args...]      start a Windows application
//! uos-runtime desktop-entry <profile>            print a desktop entry for it
//! ```
//!
//! `inspect`, `profile validate` and `desktop-entry` never run, load or modify
//! the file they are given. `prefix` and `launch` accept `--dry-run`, which
//! prints the plan and carries none of it out.
//!
//! Exit status:
//!
//! | Status | Meaning |
//! |---|---|
//! | 0 | Success. For `inspect`: the file has a route and everything it needs is installed. |
//! | 1 | The input could not be read, or is truncated, malformed or over an inspection limit. |
//! | 2 | The command line is wrong. |
//! | 3 | The file was inspected and no supported runtime exists for it. |
//! | 4 | The file has a route, but something the route needs is not installed. |
//! | 5 | The profile is not valid. |
//! | 6 | The operation was refused before anything was changed or started. |
//! | 7 | A step of the operation failed while it was being carried out. |
//! | 8 | Verification found problems in the prefix. |
//!
//! `launch` exits with the application's own exit status once the application
//! has run. With `--json` the report says which it was.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};

use uos_runtime::desktop::desktop_entry;
use uos_runtime::exec::{Forward, SystemExecutor, supervise};
use uos_runtime::grant::GrantContext;
use uos_runtime::launch::{LaunchPlan, LaunchRequest, plan_launch};
use uos_runtime::launch_host::{LaunchHost, RuntimeVersion, WineInstall, probe_wine_version};
use uos_runtime::limits::MAX_PROFILE_BYTES;
use uos_runtime::metadata::{GrantAccess, PrefixMetadata, RuntimeKind};
use uos_runtime::paths::{AppPaths, PathsError, RuntimeDirs};
use uos_runtime::plan::{Plan, PlanReport, run_plan};
use uos_runtime::prefix::{
    Planner, PrefixSettings, default_prefix_id, load_metadata, with_grant, without_grant,
};
use uos_runtime::profile::{AppProfile, GraphicsBackend, ProfileRuntime, parse_profile};
use uos_runtime::report::render_text;
use uos_runtime::runtime_cmd::DEFAULT_WINE_DEBUG;
use uos_runtime::timefmt::Timestamp;
use uos_runtime::{
    Arch, Code, Component, HostFacts, Inspection, PlanError, Recommendation, Runtime, inspect,
    recommend,
};

const EXIT_OK: u8 = 0;
const EXIT_INPUT: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_UNSUPPORTED: u8 = 3;
const EXIT_MISSING: u8 = 4;
const EXIT_INVALID_PROFILE: u8 = 5;
const EXIT_REFUSED: u8 = 6;
const EXIT_STEP_FAILED: u8 = 7;
const EXIT_FINDINGS: u8 = 8;

/// Inspect executables and packages, recommend a runtime, and run Windows
/// programs in a per-application prefix and sandbox.
#[derive(Parser)]
#[command(name = "uos-runtime", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Identify a file or application bundle from its content and recommend a
    /// runtime for it. The file is never executed.
    Inspect {
        /// The file or directory to inspect.
        path: PathBuf,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Assume this host architecture instead of the one detected.
        #[arg(long, value_name = "ARCH")]
        host_arch: Option<String>,
        /// Assume exactly these components are installed instead of detecting
        /// them. A comma-separated list of: wine, darling, android,
        /// arm-translation, windows-vm. An empty list means none.
        #[arg(long, value_name = "LIST")]
        available: Option<String>,
    },
    /// Work with application profiles.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Manage the prefix of one Windows application.
    Prefix {
        #[command(subcommand)]
        command: PrefixCommand,
    },
    /// Start a Windows application from its profile, inside the sandbox.
    Launch {
        /// The application's profile, a JSON file.
        profile: PathBuf,
        /// If the prefix is on another runtime version than the one
        /// installed, back it up and migrate it instead of refusing.
        #[arg(long)]
        migrate: bool,
        /// Set an environment variable for the application. Nothing else of
        /// the caller's environment is passed on.
        #[arg(long = "env", value_name = "NAME=VALUE")]
        environment: Vec<String>,
        /// The value of WINEDEBUG.
        #[arg(long, value_name = "CHANNELS", default_value = DEFAULT_WINE_DEBUG)]
        wine_debug: String,
        #[command(flatten)]
        common: Common,
        /// Arguments passed on to the application.
        #[arg(last = true)]
        arguments: Vec<String>,
    },
    /// Print the text of a desktop entry for an application. Nothing is
    /// installed.
    DesktopEntry {
        /// The application's profile, a JSON file.
        profile: PathBuf,
        /// The uos-runtime program the entry starts, by name or absolute path.
        #[arg(long, default_value = "uos-runtime")]
        launcher: String,
    },
    /// Internal: run a program inside the sandbox and wait for the Wine
    /// server before exiting.
    #[command(hide = true)]
    Supervise {
        /// The wineserver program to wait for afterwards.
        #[arg(long, value_name = "PATH")]
        settle: Option<PathBuf>,
        /// The program and its arguments.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ProfileCommand {
    /// Check an application profile against the schema and its rules.
    Validate {
        /// The profile, a JSON file.
        file: PathBuf,
    },
}

/// Options shared by every command that plans something.
#[derive(Args)]
struct Common {
    /// Print the plan and carry none of it out.
    #[arg(long)]
    dry_run: bool,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
    /// Use this runtime launcher (wine, or umu-run for Proton) instead of
    /// the one found in /usr/bin.
    #[arg(long, value_name = "PATH")]
    runtime_program: Option<PathBuf>,
    /// Assume the runtime reports this version instead of asking it. For
    /// Proton, the build to use.
    #[arg(long, value_name = "VERSION")]
    runtime_version: Option<String>,
    /// Use this bubblewrap program instead of the one found in /usr/bin.
    #[arg(long, value_name = "PATH")]
    sandbox_program: Option<PathBuf>,
}

#[derive(Subcommand)]
enum PrefixCommand {
    /// Create the application's prefix.
    Create {
        /// The application identifier, such as com.example.app.
        app_id: String,
        /// The runtime: wine or proton.
        #[arg(long, default_value = "wine")]
        runtime: String,
        /// The prefix name used by the application's profile. By default,
        /// the application identifier with dots turned into hyphens.
        #[arg(long, value_name = "NAME")]
        prefix_id: Option<String>,
        /// The Direct3D translation setting: auto, dxvk, vkd3d-proton,
        /// wined3d or native.
        #[arg(long, default_value = "auto")]
        graphics: String,
        /// Pin the prefix to the runtime version it is created with.
        #[arg(long)]
        pin: bool,
        /// Do not share the host's network with the application.
        #[arg(long)]
        no_network: bool,
        /// Show this directory to the application, read-only.
        #[arg(long, value_name = "DIR")]
        grant: Vec<String>,
        /// Show this directory to the application, read-write.
        #[arg(long, value_name = "DIR")]
        grant_rw: Vec<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Check the prefix without changing it.
    Verify {
        /// The application identifier.
        app_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Restore the prefix's system files with the runtime it is on.
    Repair {
        /// The application identifier.
        app_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Replace the prefix with a fresh one, keeping its settings and grants.
    Reset {
        /// The application identifier.
        app_id: String,
        /// Delete the old prefix without archiving it first.
        #[arg(long)]
        no_backup: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Archive the prefix, its private home and its metadata.
    Backup {
        /// The application identifier.
        app_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Replace the prefix with a backup.
    Restore {
        /// The application identifier.
        app_id: String,
        /// The backup's name, as printed by backup.
        name: String,
        #[command(flatten)]
        common: Common,
    },
    /// Delete the prefix, its private home, its metadata and its logs.
    /// Backups are kept.
    Remove {
        /// The application identifier.
        app_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Back the prefix up and move it to the installed runtime version.
    Migrate {
        /// The application identifier.
        app_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Show a host directory to the application.
    Grant {
        /// The application identifier.
        app_id: String,
        /// The directory.
        directory: String,
        /// Let the application change the directory, not only read it.
        #[arg(long)]
        read_write: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Stop showing a host directory to the application.
    Revoke {
        /// The application identifier.
        app_id: String,
        /// The directory.
        directory: String,
        #[command(flatten)]
        common: Common,
    },
    /// Pin the prefix to the runtime version it is on: it is never migrated.
    Pin {
        /// The application identifier.
        app_id: String,
        #[command(flatten)]
        common: Common,
    },
    /// Allow the prefix to be migrated again.
    Unpin {
        /// The application identifier.
        app_id: String,
        #[command(flatten)]
        common: Common,
    },
}

/// The JSON document printed by `inspect --json`.
#[derive(Serialize)]
struct InspectReport<'a> {
    path: String,
    host: &'a HostFacts,
    inspection: &'a Inspection,
    recommendation: &'a Recommendation,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let status = match cli.command {
        Command::Inspect {
            path,
            json,
            host_arch,
            available,
        } => run_inspect(&path, json, host_arch.as_deref(), available.as_deref()),
        Command::Profile {
            command: ProfileCommand::Validate { file },
        } => run_validate(&file),
        Command::Prefix { command } => run_prefix(command),
        Command::Launch {
            profile,
            migrate,
            environment,
            wine_debug,
            common,
            arguments,
        } => run_launch(
            &profile,
            migrate,
            &environment,
            &wine_debug,
            &common,
            &arguments,
        ),
        Command::DesktopEntry { profile, launcher } => run_desktop_entry(&profile, &launcher),
        Command::Supervise { settle, command } => run_supervise(settle.as_deref(), &command),
    };
    ExitCode::from(status)
}

/// Writes a line to standard output. A closed pipe is not an error worth a
/// panic, so write failures are ignored.
fn print_line(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}

/// Writes a line to standard error, ignoring write failures.
fn print_error_line(text: &str) {
    let _ = writeln!(std::io::stderr(), "{text}");
}

/// Reports a failure with its stable code and returns `status`.
fn fail(json: bool, code: Code, message: &str, status: u8) -> u8 {
    let error_code = code.error_code();
    if json {
        let body = serde_json::json!({
            "error": {
                "code": error_code.as_str(),
                "title": code.title(),
                "message": message,
            }
        });
        print_line(&body.to_string());
    } else {
        print_error_line(&format!("error[{error_code}]: {message}"));
    }
    status
}

/// Builds the host facts from detection and the two override options.
fn host_facts(host_arch: Option<&str>, available: Option<&str>) -> Result<HostFacts, String> {
    let detected = HostFacts::detect();
    let arch = match host_arch {
        Some(name) => Arch::from_name(name)
            .ok_or_else(|| format!("unknown architecture \"{name}\" for --host-arch"))?,
        None => detected.arch,
    };
    let components = match available {
        Some(list) => {
            let mut components = Vec::new();
            for name in list
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                let component = Component::from_name(name)
                    .ok_or_else(|| format!("unknown component \"{name}\" for --available"))?;
                components.push(component);
            }
            components
        }
        None => detected.available,
    };
    Ok(HostFacts::new(arch, &components))
}

fn run_inspect(path: &Path, json: bool, host_arch: Option<&str>, available: Option<&str>) -> u8 {
    let host = match host_facts(host_arch, available) {
        Ok(host) => host,
        Err(message) => {
            print_error_line(&format!("error: {message}"));
            return EXIT_USAGE;
        }
    };
    let inspection = match inspect(path) {
        Ok(inspection) => inspection,
        Err(error) => return fail(json, error.code(), &error.to_string(), EXIT_INPUT),
    };
    let recommendation = recommend(&inspection, &host);
    let shown_path = path.display().to_string();

    if json {
        let report = InspectReport {
            path: shown_path,
            host: &host,
            inspection: &inspection,
            recommendation: &recommendation,
        };
        match serde_json::to_string_pretty(&report) {
            Ok(text) => print_line(&text),
            Err(error) => {
                print_error_line(&format!("error: cannot write JSON: {error}"));
                return EXIT_INPUT;
            }
        }
    } else {
        let text = render_text(&shown_path, &inspection, &recommendation);
        print_line(text.trim_end());
    }

    if recommendation.runtime == Runtime::Unsupported {
        EXIT_UNSUPPORTED
    } else if recommendation.missing.is_empty() {
        EXIT_OK
    } else {
        EXIT_MISSING
    }
}

fn run_validate(file: &Path) -> u8 {
    match load_profile(file, false) {
        Ok(profile) => {
            print_line(&format!(
                "valid: {} ({}), schema version {}",
                profile.app_id, profile.display_name, profile.schema_version
            ));
            EXIT_OK
        }
        Err(status) => status,
    }
}

/// Reads and validates a profile. On failure the error has been reported and
/// the exit status is returned.
fn load_profile(file: &Path, json: bool) -> Result<AppProfile, u8> {
    let text = read_profile(file).map_err(|message| {
        let message = format!("cannot read {}: {message}", file.display());
        fail(json, Code::ProfileUnreadable, &message, EXIT_INPUT)
    })?;
    parse_profile(&text)
        .map_err(|error| fail(json, error.code(), &error.to_string(), EXIT_INVALID_PROFILE))
}

/// Reads a profile file, refusing anything larger than a profile may be
/// before reading all of it.
fn read_profile(file: &Path) -> Result<String, String> {
    let limit = u64::try_from(MAX_PROFILE_BYTES).unwrap_or(u64::MAX);
    let handle = File::open(file).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    handle
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(format!(
            "it is larger than the {MAX_PROFILE_BYTES} bytes a profile may be"
        ));
    }
    String::from_utf8(bytes).map_err(|_| "it is not valid UTF-8".to_owned())
}

/// Reports a refusal and returns the exit status for one.
fn refuse(json: bool, error: &PlanError) -> u8 {
    fail(json, error.code(), &error.to_string(), EXIT_REFUSED)
}

/// Everything a planning command needs to know about where it runs.
struct Session {
    dirs: RuntimeDirs,
    app: AppPaths,
    host: LaunchHost,
    context: GrantContext,
    now: Timestamp,
}

impl Session {
    /// Detects the host, applies the override options and resolves the
    /// application's paths.
    fn open(app_id: &str, common: &Common) -> Result<Self, PlanError> {
        let dirs = RuntimeDirs::from_env(|name| std::env::var(name).ok())?;
        let app = dirs.app(app_id)?;
        let mut host = LaunchHost::detect().ok_or(PlanError::Paths(PathsError::NoHome))?;
        if host.euid == 0 {
            return Err(PlanError::RunningAsRoot);
        }
        if let Some(program) = &common.sandbox_program {
            host.bwrap = Some(program.clone());
        }
        let context = GrantContext::new(
            &host.home,
            &dirs,
            host.runtime_dir.as_deref(),
            host.agent_socket.as_deref(),
        );
        Ok(Session {
            dirs,
            app,
            host,
            context,
            now: Timestamp::now(),
        })
    }

    /// Applies `--runtime-program` once the runtime is known.
    fn use_runtime_program(&mut self, runtime: RuntimeKind, common: &Common) {
        let Some(program) = &common.runtime_program else {
            return;
        };
        match runtime {
            RuntimeKind::Wine => {
                let server = program
                    .parent()
                    .map(|directory| directory.join("wineserver"));
                self.host.wine = Some(WineInstall {
                    program: program.clone(),
                    server,
                });
            }
            RuntimeKind::Proton => self.host.umu_run = Some(program.clone()),
        }
    }

    fn planner(&self) -> Planner<'_> {
        Planner {
            host: &self.host,
            app: &self.app,
            context: &self.context,
            now: self.now,
        }
    }

    /// The runtime version in effect: the one given on the command line, the
    /// one Wine reports, or for Proton the one the prefix is on.
    fn runtime_version(
        &self,
        runtime: RuntimeKind,
        common: &Common,
        recorded: Option<&PrefixMetadata>,
    ) -> Result<RuntimeVersion, PlanError> {
        if let Some(version) = &common.runtime_version {
            return Ok(RuntimeVersion {
                version: version.clone(),
                build: version.clone(),
            });
        }
        match runtime {
            RuntimeKind::Wine => {
                let wine = self.host.wine.as_ref().ok_or(PlanError::RuntimeMissing {
                    runtime,
                    program: "wine",
                })?;
                probe_wine_version(&wine.program).ok_or(PlanError::VersionUnknown)
            }
            RuntimeKind::Proton => recorded
                .map(|metadata| RuntimeVersion {
                    version: metadata.runtime_version.clone(),
                    build: metadata.runtime_build.clone(),
                })
                .ok_or(PlanError::VersionUnknown),
        }
    }

    fn executor(&self, forward: Forward) -> SystemExecutor {
        SystemExecutor::new(&self.dirs)
            .with_command_log(self.app.operations_log())
            .forwarding(forward)
    }
}

/// The result of planning a prefix operation.
struct Planned {
    plan: Plan,
    /// Extra members for the JSON report, such as the name of a backup.
    extra: Vec<(&'static str, Value)>,
}

impl Planned {
    fn of(plan: Plan) -> Self {
        Planned {
            plan,
            extra: Vec::new(),
        }
    }
}

fn run_prefix(command: PrefixCommand) -> u8 {
    let (common, planned) = match plan_prefix(&command) {
        (common, Ok(planned)) => (common, planned),
        (common, Err(error)) => return refuse(common.json, &error),
    };
    let (session, planned) = planned;
    let json = common.json;

    if common.dry_run {
        if json {
            print_json(&document(true, &planned, None));
        } else {
            print_line(planned.plan.render_text().trim_end());
        }
        return EXIT_OK;
    }

    let mut executor = session.executor(Forward::LogOnly);
    let report = run_plan(&planned.plan, &mut executor);
    if json {
        print_json(&document(false, &planned, Some(&report)));
    }
    if let Some(failure) = &report.failure {
        let message = failure_message(failure, executor.last_output());
        return fail_after_report(json, Code::StepFailed, &message, EXIT_STEP_FAILED);
    }
    if !report.findings.is_empty() {
        if !json {
            for finding in &report.findings {
                print_line(&format!(
                    "finding: expected that {}; {}",
                    finding.expected, finding.found
                ));
            }
        }
        let message = format!(
            "{} of {} checks did not hold",
            report.findings.len(),
            report.total
        );
        return fail_after_report(json, Code::PrefixDamaged, &message, EXIT_FINDINGS);
    }
    if !json {
        print_line(&format!(
            "done: {} {} ({} steps)",
            planned.plan.operation.as_str(),
            planned.plan.app_id,
            report.completed
        ));
        for (name, value) in &planned.extra {
            if let Some(text) = value.as_str() {
                print_line(&format!("{name}: {text}"));
            }
        }
    }
    EXIT_OK
}

/// Reports a failure that follows a JSON report already printed: the report
/// carries the detail, so only the text form adds an error line.
fn fail_after_report(json: bool, code: Code, message: &str, status: u8) -> u8 {
    if !json {
        print_error_line(&format!("error[{}]: {message}", code.error_code()));
    }
    status
}

fn failure_message(failure: &uos_runtime::plan::Failure, output: &str) -> String {
    let mut message = format!(
        "step {} failed ({}): {}",
        failure.step + 1,
        failure.description,
        failure.message
    );
    let output = output.trim_end();
    if !output.is_empty() {
        message.push_str("\n--- end of the command's output ---\n");
        message.push_str(output);
    }
    message
}

fn print_json(value: &Value) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => print_line(&text),
        Err(error) => print_error_line(&format!("error: cannot write JSON: {error}")),
    }
}

fn document(dry_run: bool, planned: &Planned, report: Option<&PlanReport>) -> Value {
    let mut body = json!({
        "dry_run": dry_run,
        "plan": planned.plan,
    });
    if let Some(report) = report {
        body["report"] = json!(report);
    }
    for (name, value) in &planned.extra {
        body[*name] = value.clone();
    }
    body
}

/// Plans a prefix operation. The command's shared options are returned in
/// either case, so that a refusal can be reported in the right form.
fn plan_prefix(command: &PrefixCommand) -> (&Common, Result<(Session, Planned), PlanError>) {
    let (app_id, common) = match command {
        PrefixCommand::Create { app_id, common, .. }
        | PrefixCommand::Verify { app_id, common }
        | PrefixCommand::Repair { app_id, common }
        | PrefixCommand::Reset { app_id, common, .. }
        | PrefixCommand::Backup { app_id, common }
        | PrefixCommand::Restore { app_id, common, .. }
        | PrefixCommand::Remove { app_id, common }
        | PrefixCommand::Migrate { app_id, common }
        | PrefixCommand::Grant { app_id, common, .. }
        | PrefixCommand::Revoke { app_id, common, .. }
        | PrefixCommand::Pin { app_id, common }
        | PrefixCommand::Unpin { app_id, common } => (app_id, common),
    };
    let planned = Session::open(app_id, common).and_then(|mut session| {
        let planned = plan_prefix_in(&mut session, command, common)?;
        Ok((session, planned))
    });
    (common, planned)
}

fn plan_prefix_in(
    session: &mut Session,
    command: &PrefixCommand,
    common: &Common,
) -> Result<Planned, PlanError> {
    let exists = std::fs::symlink_metadata(&session.app.app_dir).is_ok();

    // The operations that need no existing, valid prefix.
    match command {
        PrefixCommand::Create {
            runtime,
            prefix_id,
            graphics,
            pin,
            no_network,
            grant,
            grant_rw,
            ..
        } => {
            let runtime =
                RuntimeKind::from_name(runtime).ok_or_else(|| PlanError::Unsupported {
                    reason: format!("\"{runtime}\" is not a runtime; use wine or proton"),
                })?;
            let graphics = serde_json::from_value::<GraphicsBackend>(Value::from(
                graphics.as_str(),
            ))
            .map_err(|_| PlanError::Unsupported {
                reason: format!("\"{graphics}\" is not a graphics setting"),
            })?;
            session.use_runtime_program(runtime, common);
            let version = session.runtime_version(runtime, common, None)?;
            let planner = session.planner();
            let mut grants = Vec::new();
            let requested = grant
                .iter()
                .map(|path| (path, GrantAccess::ReadOnly))
                .chain(grant_rw.iter().map(|path| (path, GrantAccess::ReadWrite)));
            for (path, access) in requested {
                grants.push(planner.admit_grant(path, access)?);
            }
            let settings = PrefixSettings {
                prefix_id: prefix_id
                    .clone()
                    .unwrap_or_else(|| default_prefix_id(&session.app.app_id)),
                runtime,
                version,
                pinned: *pin,
                graphics,
                network: !no_network,
                grants,
            };
            let (plan, _) = planner.create(&settings, exists)?;
            return Ok(Planned::of(plan));
        }
        PrefixCommand::Remove { .. } => {
            return session.planner().remove(exists).map(Planned::of);
        }
        PrefixCommand::Restore { name, .. } => {
            if !session.app.backup_archive(name).is_file() {
                return Err(PlanError::BackupMissing { name: name.clone() });
            }
            return session.planner().restore(name, exists).map(Planned::of);
        }
        _ => {}
    }

    let metadata = load_metadata(&session.app)?.ok_or(PlanError::PrefixMissing)?;
    session.use_runtime_program(metadata.runtime, common);
    let session = &*session;
    let installed = || session.runtime_version(metadata.runtime, common, Some(&metadata));
    let planner = session.planner();
    let planned = match command {
        PrefixCommand::Verify { .. } => Planned::of(planner.verify(&metadata)?),
        PrefixCommand::Repair { .. } => Planned::of(planner.repair(&metadata, &installed()?)?),
        PrefixCommand::Reset { no_backup, .. } => {
            Planned::of(planner.reset(&metadata, &installed()?, !no_backup)?)
        }
        PrefixCommand::Migrate { .. } => Planned::of(planner.migrate(&metadata, &installed()?)?),
        PrefixCommand::Backup { .. } => {
            let (plan, name) = planner.backup("manual")?;
            Planned {
                plan,
                extra: vec![("backup", Value::from(name))],
            }
        }
        PrefixCommand::Grant {
            directory,
            read_write,
            ..
        } => {
            let access = if *read_write {
                GrantAccess::ReadWrite
            } else {
                GrantAccess::ReadOnly
            };
            let grant = planner.admit_grant(directory, access)?;
            let granted = Value::from(grant.path.clone());
            Planned {
                plan: planner.configure(&with_grant(&metadata, grant))?,
                extra: vec![("granted", granted)],
            }
        }
        PrefixCommand::Revoke { directory, .. } => {
            let changed = without_grant(&metadata, directory).ok_or_else(|| PlanError::Grant {
                path: directory.clone(),
                refusal: uos_runtime::grant::GrantRefusal::Missing,
            })?;
            Planned::of(planner.configure(&changed)?)
        }
        PrefixCommand::Pin { .. } | PrefixCommand::Unpin { .. } => {
            let pinned = matches!(command, PrefixCommand::Pin { .. });
            Planned::of(planner.configure(&PrefixMetadata {
                pinned,
                ..metadata.clone()
            })?)
        }
        PrefixCommand::Create { .. }
        | PrefixCommand::Remove { .. }
        | PrefixCommand::Restore { .. } => unreachable!("handled above"),
    };
    Ok(planned)
}

/// Splits `NAME=VALUE`.
fn parse_environment(pairs: &[String]) -> Result<Vec<(String, String)>, String> {
    pairs
        .iter()
        .map(|pair| {
            pair.split_once('=')
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .ok_or_else(|| "--env takes NAME=VALUE".to_owned())
        })
        .collect()
}

fn run_launch(
    profile_path: &Path,
    migrate: bool,
    environment: &[String],
    wine_debug: &str,
    common: &Common,
    arguments: &[String],
) -> u8 {
    let json = common.json;
    let extra_environment = match parse_environment(environment) {
        Ok(pairs) => pairs,
        Err(message) => {
            print_error_line(&format!("error: {message}"));
            return EXIT_USAGE;
        }
    };
    let profile = match load_profile(profile_path, json) {
        Ok(profile) => profile,
        Err(status) => return status,
    };

    let planned = Session::open(&profile.app_id, common).and_then(|mut session| {
        let existing = load_metadata(&session.app)?;
        let installed = match profile.runtime {
            ProfileRuntime::Wine => {
                session.use_runtime_program(RuntimeKind::Wine, common);
                Some(session.runtime_version(RuntimeKind::Wine, common, existing.as_ref())?)
            }
            ProfileRuntime::Proton => {
                session.use_runtime_program(RuntimeKind::Proton, common);
                None
            }
            _ => None,
        };
        let plan = plan_launch(
            &session.planner(),
            &LaunchRequest {
                profile: &profile,
                installed: installed.as_ref(),
                existing: existing.as_ref(),
                arguments,
                extra_environment: &extra_environment,
                wine_debug,
                migrate,
                process_id: std::process::id(),
            },
        )?;
        Ok((session, plan))
    });
    let (session, launch) = match planned {
        Ok(planned) => planned,
        Err(error) => return refuse(json, &error),
    };

    if common.dry_run {
        if json {
            print_json(&json!({ "dry_run": true, "launch": launch }));
        } else {
            print_line(render_launch(&profile, &launch).trim_end());
        }
        return EXIT_OK;
    }

    // In text form the application's output is this command's output. In
    // JSON form standard output carries the report, so the application's
    // output goes to the log only.
    let forward = if json {
        Forward::LogOnly
    } else {
        Forward::Inherit
    };
    let mut executor = session.executor(forward);
    let report = run_plan(&launch.plan, &mut executor);
    if json {
        print_json(&json!({ "dry_run": false, "launch": launch, "report": report }));
    }
    if let Some(failure) = &report.failure {
        let message = failure_message(failure, executor.last_output());
        return fail_after_report(json, Code::StepFailed, &message, EXIT_STEP_FAILED);
    }
    // The application ran: its exit status is this command's.
    match report.exit_status {
        Some(status) => u8::try_from(status).unwrap_or(EXIT_INPUT),
        None => EXIT_OK,
    }
}

/// The launch plan as text.
fn render_launch(profile: &AppProfile, launch: &LaunchPlan) -> String {
    let application = &launch.application;
    let mut text = format!(
        "Launch: {} ({}) with {} {}\n",
        profile.app_id,
        profile.display_name,
        launch.runtime.as_str(),
        launch.runtime_version
    );
    let mut line = |label: &str, value: &str| {
        text.push_str(&format!("  {label}: {value}\n"));
    };
    line("program", &application.program);
    line("arguments", &format!("{:?}", application.arguments));
    line(
        "working directory",
        application.working_directory.as_deref().unwrap_or("-"),
    );
    line("prefix", &launch.prefix.display().to_string());
    line("log", &launch.log.display().to_string());
    if let Some(overrides) = &launch.dll_overrides {
        line("DLL overrides", overrides);
    }
    line(
        "synchronisation",
        &format!(
            "{} ({})",
            uos_runtime::report::wire_name(&launch.sync.selected),
            launch.sync.reason
        ),
    );
    text.push_str("  environment:\n");
    for (name, value) in &application.environment {
        text.push_str(&format!("    {name}={value}\n"));
    }
    text.push_str("  sandbox:\n");
    for mount in &launch.sandbox {
        let kind = uos_runtime::report::wire_name(&mount.kind);
        let source = match &mount.source {
            Some(source) if *source != mount.target => format!(" (from {source})"),
            _ => String::new(),
        };
        let optional = if mount.optional { ", if present" } else { "" };
        text.push_str(&format!(
            "    {kind} {}{source}: {}{optional}\n",
            mount.target, mount.purpose
        ));
    }
    text.push_str(&launch.plan.render_text());
    text
}

fn run_desktop_entry(profile_path: &Path, launcher: &str) -> u8 {
    let profile = match load_profile(profile_path, false) {
        Ok(profile) => profile,
        Err(status) => return status,
    };
    // The entry names the profile by its real, absolute path.
    let absolute = match std::fs::canonicalize(profile_path) {
        Ok(absolute) => absolute,
        Err(error) => {
            let message = format!("cannot resolve {}: {error}", profile_path.display());
            return fail(false, Code::ProfileUnreadable, &message, EXIT_INPUT);
        }
    };
    match desktop_entry(&profile, &absolute.to_string_lossy(), launcher) {
        Ok(entry) => {
            print_line(entry.contents.trim_end());
            EXIT_OK
        }
        Err(error) => refuse(false, &error),
    }
}

fn run_supervise(settle: Option<&Path>, command: &[String]) -> u8 {
    let Some((program, arguments)) = command.split_first() else {
        return EXIT_USAGE;
    };
    match supervise(settle, program, arguments) {
        Ok(status) => u8::try_from(status).unwrap_or(EXIT_INPUT),
        Err(error) => {
            print_error_line(&format!(
                "error[{}]: {error}",
                Code::StepFailed.error_code()
            ));
            EXIT_STEP_FAILED
        }
    }
}
