// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-runtime`: the command-line front end of the Universal Runtime Manager.
//!
//! ```text
//! tur-runtime inspect <path> [--json]            identify a file and recommend a runtime
//! tur-runtime profile validate <file>            validate an application profile
//! tur-runtime prefix <operation> <app-id> ...    manage an application's prefix
//! tur-runtime launch <profile> [-- args...]      start a Windows application
//! tur-runtime desktop-entry <profile>            print a desktop entry for it
//! tur-runtime installer identify <file>          say what kind of installer a file is
//! tur-runtime app install <installer> ...        install an application into a prefix of its own
//! tur-runtime app list | show <app-id>           what is installed
//! tur-runtime app launch <app-id> [-- args...]   start an installed application
//! tur-runtime app uninstall <app-id>             remove it, and everything kept for it
//! ```
//!
//! `inspect`, `profile validate`, `desktop-entry`, `installer identify`,
//! `app list` and `app show` never run, load or modify the file they are
//! given. `prefix`, `launch`, `app install` and `app uninstall` accept
//! `--dry-run`, which prints the plan and carries none of it out.
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
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};

use tur_runtime::app_record::{AppRecord, SourceRecord, load_record};
use tur_runtime::desktop::desktop_entry;
use tur_runtime::discover::{discover, snapshot};
use tur_runtime::exec::{Forward, SystemExecutor, supervise};
use tur_runtime::grant::GrantContext;
use tur_runtime::install::{
    InstallRequest, RecordOptions, application_paths, describe_installation,
    installed_applications, launcher_destinations, plan_installer_run, plan_record, plan_removal,
    plan_removal_check, plan_rollback, plan_uninstaller, survey_leftovers,
};
use tur_runtime::installer::{IdentifyError, InstallerInfo, identify};
use tur_runtime::launch::{LaunchPlan, LaunchRequest, plan_launch};
use tur_runtime::launch_host::{LaunchHost, RuntimeVersion, WineInstall, probe_wine_version};
use tur_runtime::limits::MAX_PROFILE_BYTES;
use tur_runtime::metadata::{GrantAccess, PrefixMetadata, RuntimeKind};
use tur_runtime::paths::{AppPaths, PathsError, RuntimeDirs};
use tur_runtime::plan::{Plan, PlanReport, run_plan};
use tur_runtime::prefix::{
    Planner, PrefixSettings, default_prefix_id, load_metadata, with_grant, without_grant,
};
use tur_runtime::profile::{AppProfile, GraphicsBackend, ProfileRuntime, parse_profile};
use tur_runtime::report::render_text;
use tur_runtime::runtime_cmd::DEFAULT_WINE_DEBUG;
use tur_runtime::sha256::sha256_file;
use tur_runtime::timefmt::Timestamp;
use tur_runtime::{
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
#[command(name = "tur-runtime", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// The command line is parsed once; the size of its largest variant does not
// matter.
#[allow(clippy::large_enum_variant)]
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
        /// The tur-runtime program the entry starts, by name or absolute path.
        #[arg(long, default_value = "tur-runtime")]
        launcher: String,
    },
    /// Look at installer files.
    Installer {
        #[command(subcommand)]
        command: InstallerCommand,
    },
    /// Install, list, start and remove Windows applications.
    App {
        #[command(subcommand)]
        command: AppCommand,
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

#[derive(Subcommand)]
enum InstallerCommand {
    /// Say what kind of installer a file is, from its content, and how it
    /// is run unattended. The file is never executed.
    Identify {
        /// The installer, program or archive.
        file: PathBuf,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

// The command line is parsed once; the size of its largest variant does not
// matter.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum AppCommand {
    /// Install an application into a fresh prefix of its own: run its
    /// installer unattended inside the sandbox, record what was installed
    /// and generate a launcher entry.
    Install {
        /// The installer: a Windows Installer package, an NSIS or Inno
        /// Setup installer, a zip archive of a portable application, or a
        /// program.
        installer: PathBuf,
        /// The application identifier, such as org.example.app. By default,
        /// one made from the file's name under "local.".
        #[arg(long, value_name = "ID")]
        app_id: Option<String>,
        /// The application's name, instead of the one its installer
        /// registers.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// The program the launcher entry starts, by file name or by the
        /// end of its path, when the installer installs several and none
        /// stands out.
        #[arg(long = "main-exe", value_name = "PROGRAM")]
        main_exe: Option<String>,
        /// The file is the application itself, not an installer: copy it
        /// into the prefix instead of running it.
        #[arg(long)]
        portable: bool,
        /// Do not share the host's network with the installer or the
        /// application.
        #[arg(long)]
        no_network: bool,
        /// How long the installer may take, in seconds.
        #[arg(long, value_name = "SECONDS", default_value_t = 900)]
        timeout: u64,
        /// Keep the prefix when the installation fails, for a look at what
        /// went wrong. By default a failed installation leaves only its log.
        #[arg(long)]
        keep_failed: bool,
        /// The tur-runtime program the launcher entry starts, by name or
        /// absolute path.
        #[arg(long, default_value = "tur-runtime")]
        launcher: String,
        #[command(flatten)]
        common: Common,
    },
    /// List the installed applications.
    List {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show the record of an installed application.
    Show {
        /// The application identifier.
        app_id: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Start an installed application, inside the sandbox.
    Launch {
        /// The application identifier.
        app_id: String,
        /// If the prefix is on another runtime version than the one
        /// installed, back it up and migrate it instead of refusing.
        #[arg(long)]
        migrate: bool,
        /// Set an environment variable for the application.
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
    /// Remove an installed application: run its own uninstaller, then
    /// delete its prefix, its private home, its record, its launcher entry
    /// and its logs, and check that nothing is left.
    Uninstall {
        /// The application identifier.
        app_id: String,
        /// Do not run the application's own uninstaller; only delete.
        #[arg(long)]
        skip_uninstaller: bool,
        /// Delete the backups of the prefix too. By default they are kept.
        #[arg(long)]
        remove_backups: bool,
        /// How long the application's uninstaller may take, in seconds.
        #[arg(long, value_name = "SECONDS", default_value_t = 900)]
        timeout: u64,
        #[command(flatten)]
        common: Common,
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
        Command::Installer {
            command: InstallerCommand::Identify { file, json },
        } => run_installer_identify(&file, json),
        Command::App { command } => run_app(command),
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

fn failure_message(failure: &tur_runtime::plan::Failure, output: &str) -> String {
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
                refusal: tur_runtime::grant::GrantRefusal::Missing,
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
            tur_runtime::report::wire_name(&launch.sync.selected),
            launch.sync.reason
        ),
    );
    text.push_str("  environment:\n");
    for (name, value) in &application.environment {
        text.push_str(&format!("    {name}={value}\n"));
    }
    text.push_str("  sandbox:\n");
    for mount in &launch.sandbox {
        let kind = tur_runtime::report::wire_name(&mount.kind);
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

/// The installer's identification as text.
fn render_installer(path: &Path, info: &InstallerInfo) -> String {
    let mut text = format!(
        "File: {}\nKind: {} ({})\n",
        path.display(),
        info.label,
        info.kind.as_str()
    );
    if let Some(architecture) = info.architecture {
        text.push_str(&format!("Installer program: {architecture}\n"));
    }
    text.push_str(&format!("Identified by: {}\n", info.evidence));
    match &info.unattended {
        Some(unattended) => text.push_str(&format!(
            "Unattended mode: {} (documented at {})\n",
            unattended.arguments.join(" "),
            unattended.documented_at
        )),
        None => text.push_str("Unattended mode: none; the file is not run as an installer\n"),
    }
    for note in &info.notes {
        text.push_str(&format!("Note: {note}\n"));
    }
    text
}

/// Reports why a file cannot be installed and returns the exit status.
fn fail_identify(json: bool, error: &IdentifyError) -> u8 {
    let status = match error {
        IdentifyError::Inspect(_) => EXIT_INPUT,
        IdentifyError::NotInstallable { .. } => EXIT_UNSUPPORTED,
    };
    fail(json, error.code(), &error.to_string(), status)
}

fn run_installer_identify(file: &Path, json: bool) -> u8 {
    match identify(file) {
        Ok(info) => {
            if json {
                print_json(&json!({ "path": file.display().to_string(), "installer": info }));
            } else {
                print_line(render_installer(file, &info).trim_end());
            }
            EXIT_OK
        }
        Err(error) => fail_identify(json, &error),
    }
}

fn run_app(command: AppCommand) -> u8 {
    match command {
        AppCommand::Install {
            installer,
            app_id,
            name,
            main_exe,
            portable,
            no_network,
            timeout,
            keep_failed,
            launcher,
            common,
        } => run_app_install(&InstallArguments {
            installer: &installer,
            app_id: app_id.as_deref(),
            name: name.as_deref(),
            main_exe: main_exe.as_deref(),
            portable,
            no_network,
            timeout,
            keep_failed,
            launcher: &launcher,
            common: &common,
        }),
        AppCommand::List { json } => run_app_list(json),
        AppCommand::Show { app_id, json } => run_app_show(&app_id, json),
        AppCommand::Launch {
            app_id,
            migrate,
            environment,
            wine_debug,
            common,
            arguments,
        } => match installed_record(&app_id) {
            Ok(record) => run_launch(
                Path::new(&record.profile),
                migrate,
                &environment,
                &wine_debug,
                &common,
                &arguments,
            ),
            Err(error) => refuse(common.json, &error),
        },
        AppCommand::Uninstall {
            app_id,
            skip_uninstaller,
            remove_backups,
            timeout,
            common,
        } => run_app_uninstall(&app_id, skip_uninstaller, remove_backups, timeout, &common),
    }
}

/// What `app install` was asked to do.
struct InstallArguments<'a> {
    installer: &'a Path,
    app_id: Option<&'a str>,
    name: Option<&'a str>,
    main_exe: Option<&'a str>,
    portable: bool,
    no_network: bool,
    timeout: u64,
    keep_failed: bool,
    launcher: &'a str,
    common: &'a Common,
}

/// The record of an installed application, or the reason there is none.
fn installed_record(app_id: &str) -> Result<AppRecord, PlanError> {
    let dirs = RuntimeDirs::from_env(|name| std::env::var(name).ok())?;
    let app = dirs.app(app_id)?;
    load_record(&app)?.ok_or_else(|| PlanError::AppNotInstalled {
        has_prefix: std::fs::symlink_metadata(&app.app_dir).is_ok(),
    })
}

fn run_app_install(arguments: &InstallArguments<'_>) -> u8 {
    let common = arguments.common;
    let json = common.json;
    let info = match identify(arguments.installer) {
        Ok(info) => info,
        Err(error) => return fail_identify(json, &error),
    };
    // The installer by its real, absolute path: that is the file shown to
    // the sandbox.
    let installer = match std::fs::canonicalize(arguments.installer) {
        Ok(path) => path,
        Err(error) => {
            let message = format!("cannot resolve {}: {error}", arguments.installer.display());
            return fail(json, Code::Unreadable, &message, EXIT_INPUT);
        }
    };
    let Some(installer_text) = installer.to_str() else {
        let message = "the installer's path is not valid UTF-8";
        return fail(json, Code::Unreadable, message, EXIT_INPUT);
    };
    let file_name = installer
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("installer")
        .to_owned();
    let app_id = match arguments.app_id {
        Some(app_id) => app_id.to_owned(),
        None => {
            let stem = installer
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("");
            format!("local.{}", default_prefix_id(stem))
        }
    };

    let planned = Session::open(&app_id, common).and_then(|mut session| {
        session.use_runtime_program(RuntimeKind::Wine, common);
        let version = session.runtime_version(RuntimeKind::Wine, common, None)?;
        let exists = std::fs::symlink_metadata(&session.app.app_dir).is_ok();
        let settings = PrefixSettings {
            prefix_id: default_prefix_id(&session.app.app_id),
            runtime: RuntimeKind::Wine,
            version,
            pinned: false,
            graphics: GraphicsBackend::Auto,
            network: !arguments.no_network,
            grants: Vec::new(),
        };
        let (create, metadata) = session.planner().create(&settings, exists)?;
        let run = plan_installer_run(
            &session.planner(),
            &metadata,
            &InstallRequest {
                installer: installer_text,
                info: &info,
                portable: arguments.portable,
                limit_seconds: arguments.timeout,
            },
        )?;
        Ok((session, create, metadata, run))
    });
    let (session, create, metadata, run) = match planned {
        Ok(planned) => planned,
        Err(error) => return refuse(json, &error),
    };

    if common.dry_run {
        if json {
            print_json(&json!({
                "dry_run": true,
                "app_id": app_id,
                "installer": info,
                "plans": [create, run.plan],
            }));
        } else {
            print_line(render_installer(&installer, &info).trim_end());
            print_line(create.render_text().trim_end());
            print_line(run.plan.render_text().trim_end());
        }
        return EXIT_OK;
    }

    let (sha256, size) = match sha256_file(&installer) {
        Ok(hashed) => hashed,
        Err(error) => {
            let message = format!("cannot read {}: {error}", installer.display());
            return fail(json, Code::Unreadable, &message, EXIT_INPUT);
        }
    };

    let planner = session.planner();
    let mut executor = session.executor(Forward::LogOnly);
    // A failed installation leaves its log and nothing else, unless the
    // caller asked to keep the prefix for a look.
    let roll_back = |executor: &mut SystemExecutor| {
        if !arguments.keep_failed {
            let _ = run_plan(&plan_rollback(&planner), executor);
        }
    };
    let kept = if arguments.keep_failed {
        "\nThe prefix was kept; `prefix remove` deletes it."
    } else {
        ""
    };

    // 1. The prefix.
    let started = Instant::now();
    let report = run_plan(&create, &mut executor);
    if let Some(failure) = &report.failure {
        let message = failure_message(failure, executor.last_output());
        roll_back(&mut executor);
        return fail(
            json,
            Code::StepFailed,
            &format!("{message}{kept}"),
            EXIT_STEP_FAILED,
        );
    }
    let prefix_seconds = started.elapsed().as_secs_f64();
    let before = snapshot(&session.app.prefix);

    // 2. The installer.
    let started = Instant::now();
    let report = run_plan(&run.plan, &mut executor);
    let installer_seconds = started.elapsed().as_secs_f64();
    if let Some(failure) = &report.failure {
        let code = if report.timed_out {
            Code::InstallerTimedOut
        } else if report.exit_status.is_some() {
            Code::InstallerFailed
        } else {
            Code::StepFailed
        };
        let message = failure_message(failure, executor.last_output());
        roll_back(&mut executor);
        return fail(json, code, &format!("{message}{kept}"), EXIT_STEP_FAILED);
    }

    // 3. What was installed, and the record of it.
    let discovery = discover(&session.app.prefix, &before);
    let described = describe_installation(
        &planner,
        &metadata,
        &info,
        &discovery,
        RecordOptions {
            name: arguments.name,
            main_executable: arguments.main_exe,
            launcher: arguments.launcher,
            source: SourceRecord {
                file_name,
                sha256,
                size,
            },
            method: run.method,
            arguments: run.arguments.clone(),
        },
    );
    let installed = match described {
        Ok(installed) => installed,
        Err(error) => {
            roll_back(&mut executor);
            let message = format!("{error}{kept}");
            return fail(json, error.code(), &message, EXIT_STEP_FAILED);
        }
    };
    let report = run_plan(&plan_record(&planner, &installed), &mut executor);
    if let Some(failure) = &report.failure {
        let message = failure_message(failure, executor.last_output());
        roll_back(&mut executor);
        return fail(
            json,
            Code::StepFailed,
            &format!("{message}{kept}"),
            EXIT_STEP_FAILED,
        );
    }

    let record = &installed.record;
    let (entry_destination, icon_destination) = launcher_destinations(&installed);
    let entry_destination = format!("$XDG_DATA_HOME/{entry_destination}");
    let icon_destination = icon_destination.map(|icon| format!("$XDG_DATA_HOME/{icon}"));
    if json {
        let icon = installed.icon.as_ref().map(|icon| {
            json!({
                "width": icon.width,
                "height": icon.height,
                "source_bits": icon.source_bits,
                "was_png": icon.was_png,
            })
        });
        print_json(&json!({
            "dry_run": false,
            "installer": info,
            "record": record,
            "discovery": discovery,
            "launcher": {
                "staged_in": session.app.launchers,
                "would_install_entry_to": entry_destination,
                "would_install_icon_to": icon_destination,
                "icon": icon,
            },
            "seconds": {
                "prefix": prefix_seconds,
                "installer": installer_seconds,
            },
            "notes": installed.notes,
        }));
    } else {
        print_line(&format!("installed: {} ({})", record.name, record.app_id));
        print_line(&format!(
            "  version: {}",
            record.version.as_deref().unwrap_or("not stated")
        ));
        print_line(&format!(
            "  publisher: {}",
            record.publisher.as_deref().unwrap_or("not stated")
        ));
        print_line(&format!("  installer: {}", info.label));
        print_line(&format!(
            "  main program: {} ({})",
            record.main_executable_windows, record.main_executable_reason
        ));
        print_line(&format!("  record: {}", session.app.record.display()));
        print_line(&format!(
            "  launcher entry: {} (staged; installed it would be {entry_destination})",
            record.launcher.desktop_entry
        ));
        print_line(&format!(
            "  start it with: tur-runtime app launch {}",
            record.app_id
        ));
        for note in &installed.notes {
            print_line(&format!("  note: {note}"));
        }
    }
    EXIT_OK
}

fn run_app_list(json: bool) -> u8 {
    let dirs = match RuntimeDirs::from_env(|name| std::env::var(name).ok()) {
        Ok(dirs) => dirs,
        Err(error) => return refuse(json, &PlanError::from(error)),
    };
    let (records, problems) = installed_applications(&dirs);
    if json {
        print_json(&json!({ "applications": records, "problems": problems }));
    } else {
        for record in &records {
            print_line(&format!(
                "{}\t{}\t{}",
                record.app_id,
                record.name,
                record.version.as_deref().unwrap_or("-")
            ));
        }
        for problem in &problems {
            print_error_line(&format!(
                "warning[{}]: {problem}",
                Code::AppRecordInvalid.error_code()
            ));
        }
    }
    EXIT_OK
}

fn run_app_show(app_id: &str, json: bool) -> u8 {
    let record = match installed_record(app_id) {
        Ok(record) => record,
        Err(error) => return refuse(json, &error),
    };
    if json {
        print_line(record.to_json().trim_end());
    } else {
        print_line(&format!("{} ({})", record.name, record.app_id));
        let line = |label: &str, value: &str| print_line(&format!("  {label}: {value}"));
        line("version", record.version.as_deref().unwrap_or("not stated"));
        line(
            "publisher",
            record.publisher.as_deref().unwrap_or("not stated"),
        );
        line("main program", &record.main_executable_windows);
        line("chosen because", &record.main_executable_reason);
        line("prefix", &record.prefix);
        line(
            "runtime",
            &format!("{} {}", record.runtime.as_str(), record.runtime_version),
        );
        line("installed", &record.installed_at.rfc3339());
        line(
            "installed from",
            &format!(
                "{} (SHA-256 {})",
                record.install.source.file_name, record.install.source.sha256
            ),
        );
        line("installer kind", record.install.kind.label());
        line("launcher entry", &record.launcher.desktop_entry);
    }
    EXIT_OK
}

fn run_app_uninstall(
    app_id: &str,
    skip_uninstaller: bool,
    remove_backups: bool,
    timeout: u64,
    common: &Common,
) -> u8 {
    let json = common.json;
    let planned = Session::open(app_id, common).and_then(|mut session| {
        let record = load_record(&session.app)?.ok_or_else(|| PlanError::AppNotInstalled {
            has_prefix: std::fs::symlink_metadata(&session.app.app_dir).is_ok(),
        })?;
        let metadata = load_metadata(&session.app)?.ok_or(PlanError::PrefixMissing)?;
        session.use_runtime_program(metadata.runtime, common);
        let planner = session.planner();
        let uninstaller = if skip_uninstaller {
            None
        } else {
            plan_uninstaller(&planner, &metadata, &record, timeout)?
        };
        let removal = plan_removal(&planner, remove_backups)?;
        let check = plan_removal_check(&planner, remove_backups);
        Ok((session, record, uninstaller, removal, check))
    });
    let (session, record, uninstaller, removal, check) = match planned {
        Ok(planned) => planned,
        Err(error) => return refuse(json, &error),
    };

    if common.dry_run {
        let plans: Vec<&Plan> = uninstaller.iter().chain([&removal, &check]).collect();
        if json {
            print_json(&json!({ "dry_run": true, "app_id": app_id, "plans": plans }));
        } else {
            for plan in plans {
                print_line(plan.render_text().trim_end());
            }
        }
        return EXIT_OK;
    }

    let mut executor = session.executor(Forward::LogOnly);

    // 1. The application's own uninstaller. What it does is recorded; it
    // does not decide whether the application is removed.
    let mut uninstaller_report = json!({ "ran": false });
    let mut uninstaller_problem: Option<String> = None;
    if let Some(plan) = &uninstaller {
        let started = Instant::now();
        let report = run_plan(plan, &mut executor);
        let seconds = started.elapsed().as_secs_f64();
        if let Some(failure) = &report.failure {
            uninstaller_problem = Some(failure_message(failure, executor.last_output()));
        }
        uninstaller_report = json!({
            "ran": true,
            "succeeded": report.failure.is_none(),
            "exit_status": report.exit_status,
            "timed_out": report.timed_out,
            "seconds": seconds,
            "left_in_prefix": survey_leftovers(&session.app, &record),
            "code": report.failure.as_ref().map(|_| Code::UninstallerFailed),
        });
    }

    // 2. Everything kept for the application.
    let report = run_plan(&removal, &mut executor);
    if let Some(failure) = &report.failure {
        let message = failure_message(failure, executor.last_output());
        return fail(json, Code::StepFailed, &message, EXIT_STEP_FAILED);
    }

    // 3. Nothing is left.
    let checked = run_plan(&check, &mut executor);
    let removed = application_paths(&session.app, remove_backups);
    let kept: Vec<&Path> = if !remove_backups && session.app.backups.exists() {
        vec![session.app.backups.as_path()]
    } else {
        Vec::new()
    };
    if json {
        print_json(&json!({
            "dry_run": false,
            "app_id": app_id,
            "uninstaller": uninstaller_report,
            "removed": removed,
            "kept": kept,
            "left_behind": checked.findings,
        }));
    } else {
        if let Some(problem) = &uninstaller_problem {
            print_error_line(&format!(
                "warning[{}]: the application's own uninstaller did not finish; the \
                 application was removed all the same. {problem}",
                Code::UninstallerFailed.error_code()
            ));
        }
        for finding in &checked.findings {
            print_line(&format!(
                "finding: expected that {}; {}",
                finding.expected, finding.found
            ));
        }
    }
    if !checked.findings.is_empty() {
        let message = format!(
            "{} of {} locations were not removed",
            checked.findings.len(),
            checked.total
        );
        return fail_after_report(json, Code::UninstallIncomplete, &message, EXIT_FINDINGS);
    }
    if !json {
        print_line(&format!("removed: {} ({})", record.name, record.app_id));
        for path in &kept {
            print_line(&format!("kept: {}", path.display()));
        }
    }
    EXIT_OK
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
