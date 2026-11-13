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
//! tur-runtime app autopilot <app-id>             try remedies in throwaway copies of its prefix
//! tur-runtime app autopilot --apply <result-id>   apply what a result found, after a backup
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
//! | 10 | `app autopilot`: no remedy made the application pass, or it needs something TUR cannot supply. |
//!
//! `launch` exits with the application's own exit status once the application
//! has run. With `--json` the report says which it was.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::{Value, json};

use tur_runtime::app_record::{AppRecord, SourceRecord, load_record};
use tur_runtime::autopilot::{
    APPLIED_SCHEMA_VERSION, Abilities, AppliedRemedies, AutopilotResult, CandidatePlan, CheckSpec,
    DEFAULT_MAX_ATTEMPTS, FactsRecord, Limits, MAX_ATTEMPTS_LIMIT, MAX_RESULT_BYTES,
    MAX_SMALL_FILE_BYTES, Outcome, PrefixFingerprints, RESULT_SCHEMA_VERSION, SearchContext,
    Unsuppliable, candidates, catalogue_record, evidence_digest, parse_result_id, search,
};
use tur_runtime::autopilot_run::{
    CopyTrials, WindowProbe, applied_file, copy_paths, create_private_dir, fingerprint, font_files,
    import_registry, make_base, merge_copy, probe_case_folding as probe_folding_ability,
    reg_import_command, remove_tree, results_dir, static_facts, trial_settings_text,
    wine_builtin_dlls,
};
use tur_runtime::casefold::{
    CaseFoldRecord, CaseFolding, chattr_command, probe as probe_case_folding,
};
use tur_runtime::failure::{FailureClass, StaticFacts};
use tur_runtime::launch_host::{PROGRAM_DIRECTORIES, find_program};

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
use tur_runtime::metadata::{GrantAccess, NtsyncDevice, PrefixMetadata, RuntimeKind};
use tur_runtime::paths::{AppPaths, PathsError, RuntimeDirs};
use tur_runtime::plan::{Operation, Plan, PlanReport, Step, run_plan};
use tur_runtime::prefix::{
    Planner, PrefixSettings, default_prefix_id, load_metadata, with_grant, without_grant,
};
use tur_runtime::profile::{AppProfile, GraphicsBackend, ProfileRuntime, parse_profile};
use tur_runtime::remedy::{Catalogue, EnvironmentValue, RemedyKind, Resolved, reg_file};
use tur_runtime::report::render_text;
use tur_runtime::runtime_cmd::DEFAULT_WINE_DEBUG;
use tur_runtime::runtime_cmd::wine_has_ntsync;
use tur_runtime::sandbox::SandboxError;
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
/// The autopilot found no remedy, or the failure needs something TUR cannot
/// supply.
const EXIT_NOT_FIXED: u8 = 10;

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
        /// Make the prefix's drive C: case-insensitive where the file
        /// system can (ext4 with the casefold feature); see --no-casefold.
        #[arg(long, conflicts_with = "no_casefold")]
        casefold: bool,
        /// Leave the prefix's drive C: an ordinary directory, in which Wine
        /// emulates case-insensitivity.
        #[arg(long)]
        no_casefold: bool,
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
    /// The compatibility autopilot: when an application fails, try known
    /// remedies in throwaway copies of its prefix and keep the first that
    /// makes its check pass. The real prefix is never changed, except by
    /// --apply, after a backup; --revert restores that backup.
    Autopilot {
        /// The application identifier.
        #[arg(required_unless_present_any = ["apply", "plan"])]
        app_id: Option<String>,
        /// Say what would be tried, in which order and why, and what would
        /// not be; run nothing and change nothing.
        #[arg(long, conflicts_with_all = ["results", "apply", "revert"])]
        plan: bool,
        /// With --plan: the failure class to plan for, such as dll-missing
        /// or crash. By default the class of the latest result, or every
        /// class.
        #[arg(long, value_name = "CLASS", requires = "plan")]
        failure: Option<String>,
        /// List the results kept for the application, and the remedies
        /// applied to it.
        #[arg(long, conflicts_with_all = ["apply", "revert"])]
        results: bool,
        /// Apply the remedies a result found to the application's real
        /// prefix, after a backup. This is the person's decision.
        #[arg(long, value_name = "RESULT", conflicts_with = "revert")]
        apply: Option<String>,
        /// Undo the applied remedies: restore the backup taken when they
        /// were applied.
        #[arg(long)]
        revert: bool,
        /// A check file (schemas/remedy-check.schema.json) that says what
        /// "works" means for the application. By default: a window of its
        /// own within 60 seconds, then 3 seconds without a crash.
        #[arg(long, value_name = "FILE")]
        check: Option<PathBuf>,
        /// The most remedy attempts, the unchanged run not counted.
        #[arg(long, value_name = "N", default_value_t = DEFAULT_MAX_ATTEMPTS)]
        max_attempts: usize,
        /// Try single remedies only, no combinations.
        #[arg(long)]
        no_combinations: bool,
        /// Share the host's network with the throwaway copies. By default
        /// they have none.
        #[arg(long)]
        network: bool,
        /// Keep the copy of the prefix the attempts started from, for a
        /// look. It is deleted by default.
        #[arg(long)]
        keep_copies: bool,
        #[command(flatten)]
        common: Common,
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
        /// Make the prefix's drive C: case-insensitive where the file
        /// system can (ext4 with the casefold feature); see --no-casefold.
        #[arg(long, conflicts_with = "no_casefold")]
        casefold: bool,
        /// Leave the prefix's drive C: an ordinary directory, in which Wine
        /// emulates case-insensitivity.
        #[arg(long)]
        no_casefold: bool,
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
            None,
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

    /// Whether the application's drive C: is case-insensitive now, as a
    /// choice for a prefix that replaces it: a reset or a restore keeps it
    /// as it was. Unknown counts as not.
    fn live_case_folding(&self) -> CaseFolding {
        let drive_c = self.app.prefix.join("drive_c");
        let folded = self
            .host
            .lsattr
            .as_deref()
            .and_then(|lsattr| probe_case_folding(lsattr, &drive_c));
        if folded == Some(true) {
            CaseFolding::Try
        } else {
            CaseFolding::Off
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
        let mut body = document(false, &planned, Some(&report));
        if let Some(record) = &report.case_folding {
            body["warnings"] = case_folding_warnings(record);
        }
        print_json(&body);
    }
    if let Some(record) = &report.case_folding {
        report_case_folding(json, record);
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

/// Says what became of making drive C: case-insensitive. A file system that
/// cannot is not a failure: the line goes to standard error with its code,
/// and with `--json` the report already carries the record.
fn report_case_folding(json: bool, record: &CaseFoldRecord) {
    if json {
        return;
    }
    print_line(&format!("drive C: {}", record.summary()));
    if let Some(code) = record.reason.code() {
        print_error_line(&format!(
            "warning[{}]: {}; the prefix was created case-sensitive",
            code.error_code(),
            record.reason.describe()
        ));
    }
}

/// The warnings a case-folding record gives rise to, for a JSON report.
fn case_folding_warnings(record: &CaseFoldRecord) -> Value {
    match record.reason.code() {
        Some(code) => json!([{
            "code": code.as_str(),
            "message": record.reason.describe(),
        }]),
        None => json!([]),
    }
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
            casefold,
            no_casefold,
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
                case_folding: CaseFolding::from_options(*casefold, *no_casefold),
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
            let case_folding = session.live_case_folding();
            return session
                .planner()
                .restore(name, exists, case_folding)
                .map(Planned::of);
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
            let case_folding = session.live_case_folding();
            Planned::of(planner.reset(&metadata, &installed()?, !no_backup, case_folding)?)
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
    applied: Option<&AppliedRemedies>,
) -> u8 {
    let json = common.json;
    let mut extra_environment = match parse_environment(environment) {
        Ok(pairs) => pairs,
        Err(message) => {
            print_error_line(&format!("error: {message}"));
            return EXIT_USAGE;
        }
    };
    // Remedies the person applied with the autopilot: their variables, unless
    // the same variable was given on the command line.
    if let Some(applied) = applied {
        for (name, value) in applied.environment_pairs() {
            if !extra_environment.iter().any(|(given, _)| *given == name) {
                extra_environment.push((name, value));
            }
        }
    }
    let hide_ntsync = applied.is_some_and(|applied| applied.hide_ntsync);
    let profile = match load_profile(profile_path, json) {
        Ok(profile) => profile,
        Err(status) => return status,
    };

    let planned = Session::open(&profile.app_id, common).and_then(|mut session| {
        if hide_ntsync {
            session.host.ntsync = NtsyncDevice::Absent;
        }
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
            casefold,
            no_casefold,
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
            case_folding: CaseFolding::from_options(casefold, no_casefold),
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
            Ok(record) => {
                let applied = match load_applied(&app_id) {
                    Ok(applied) => applied,
                    Err(message) => {
                        return fail(common.json, Code::AppRecordInvalid, &message, EXIT_REFUSED);
                    }
                };
                run_launch(
                    Path::new(&record.profile),
                    migrate,
                    &environment,
                    &wine_debug,
                    &common,
                    &arguments,
                    applied.as_ref(),
                )
            }
            Err(error) => refuse(common.json, &error),
        },
        AppCommand::Autopilot {
            app_id,
            plan,
            failure,
            results,
            apply,
            revert,
            check,
            max_attempts,
            no_combinations,
            network,
            keep_copies,
            common,
        } => {
            if let Some(result_id) = apply {
                return run_autopilot_apply(&result_id, &common);
            }
            if plan {
                return run_autopilot_plan(app_id.as_deref(), failure.as_deref(), common.json);
            }
            let Some(app_id) = app_id else {
                print_error_line("error: the application identifier is required");
                return EXIT_USAGE;
            };
            if results {
                run_autopilot_results(&app_id, common.json)
            } else if revert {
                run_autopilot_revert(&app_id, &common)
            } else if common.dry_run {
                run_autopilot_plan(Some(&app_id), None, common.json)
            } else {
                run_autopilot(
                    &app_id,
                    &AutopilotOptions {
                        check: check.as_deref(),
                        max_attempts,
                        combinations: !no_combinations,
                        network,
                        keep_copies,
                        common: &common,
                    },
                )
            }
        }
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
    case_folding: CaseFolding,
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
            case_folding: arguments.case_folding,
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
    let case_folding = report
        .case_folding
        .clone()
        .unwrap_or_else(CaseFoldRecord::not_requested);
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
            case_folding: Some(case_folding.clone()),
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
            "case_folding": case_folding,
            "warnings": case_folding_warnings(&case_folding),
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
        print_line(&format!("  drive C: {}", case_folding.summary()));
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
        if let Some(code) = case_folding.reason.code() {
            print_error_line(&format!(
                "warning[{}]: {}; the application was installed in a case-sensitive prefix",
                code.error_code(),
                case_folding.reason.describe()
            ));
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
        line(
            "drive C",
            &record.case_folding.as_ref().map_or_else(
                || "not recorded (installed before case folding was recorded)".to_owned(),
                CaseFoldRecord::summary,
            ),
        );
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

// --------------------------------------------------------------------------
// The compatibility autopilot (docs/architecture/compat-autopilot.md).
// --------------------------------------------------------------------------

/// What `app autopilot <app-id>` was asked to do.
struct AutopilotOptions<'a> {
    check: Option<&'a Path>,
    max_attempts: usize,
    combinations: bool,
    network: bool,
    keep_copies: bool,
    common: &'a Common,
}

/// Reads a small text file, refusing anything larger than `limit` before
/// reading all of it.
fn read_text(file: &Path, limit: usize) -> Result<String, String> {
    let cap = u64::try_from(limit).unwrap_or(u64::MAX);
    let handle =
        File::open(file).map_err(|error| format!("cannot read {}: {error}", file.display()))?;
    let mut bytes = Vec::new();
    handle
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", file.display()))?;
    if bytes.len() > limit {
        return Err(format!("{} is larger than {limit} bytes", file.display()));
    }
    String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", file.display()))
}

/// Writes a file readable by the user only, replacing it.
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// The remedies applied to an installed application, when there are any.
fn load_applied(app_id: &str) -> Result<Option<AppliedRemedies>, String> {
    let dirs = RuntimeDirs::from_env(|name| std::env::var(name).ok()).map_err(|e| e.to_string())?;
    let app = dirs.app(app_id).map_err(|error| error.to_string())?;
    let path = applied_file(&app);
    if std::fs::symlink_metadata(&path).is_err() {
        return Ok(None);
    }
    let text = read_text(&path, MAX_SMALL_FILE_BYTES)?;
    AppliedRemedies::parse(&text, app_id)
        .map(Some)
        .map_err(|problem| {
            format!(
                "the applied remedies in {} are not valid: {problem}",
                path.display()
            )
        })
}

/// The record, metadata and session of an installed Wine application.
fn autopilot_session(
    app_id: &str,
    common: &Common,
) -> Result<(Session, AppRecord, PrefixMetadata), PlanError> {
    let mut session = Session::open(app_id, common)?;
    let record = load_record(&session.app)?.ok_or_else(|| PlanError::AppNotInstalled {
        has_prefix: std::fs::symlink_metadata(&session.app.app_dir).is_ok(),
    })?;
    let metadata = load_metadata(&session.app)?.ok_or(PlanError::PrefixMissing)?;
    session.use_runtime_program(metadata.runtime, common);
    Ok((session, record, metadata))
}

/// The plans as text.
fn render_plans(plans: &[CandidatePlan]) -> String {
    let mut text = String::new();
    for plan in plans {
        text.push_str(&format!(
            "If the failure is {} ({}): {}\n",
            plan.class,
            plan.class.code(),
            plan.class.describe()
        ));
        if plan.candidates.is_empty() {
            text.push_str("  nothing is tried\n");
        }
        for (index, candidate) in plan.candidates.iter().enumerate() {
            text.push_str(&format!(
                "  {}. {}: {}\n",
                index + 1,
                candidate.instance.label,
                candidate.reason
            ));
        }
        for passed in &plan.passed_over {
            text.push_str(&format!(
                "  not tried: {}: {}\n",
                passed.remedy, passed.reason
            ));
        }
    }
    text
}

/// `app autopilot --plan`: what would be tried and why. Reads only.
fn run_autopilot_plan(app_id: Option<&str>, failure: Option<&str>, json: bool) -> u8 {
    let catalogue = Catalogue::shipped();
    let given = match failure.map(|name| (name, FailureClass::from_name(name))) {
        None => None,
        Some((_, Some(class))) => Some(class),
        Some((name, None)) => {
            let names: Vec<&str> = FailureClass::ALL
                .iter()
                .map(|class| class.as_str())
                .collect();
            print_error_line(&format!(
                "error: \"{name}\" is not a failure class; the classes are {}",
                names.join(", ")
            ));
            return EXIT_USAGE;
        }
    };
    let mut notes = Vec::new();
    let mut facts = StaticFacts {
        exe_name: "program.exe".to_owned(),
        ..StaticFacts::default()
    };
    let mut known: Option<(
        FailureClass,
        Option<String>,
        Vec<tur_runtime::failure::Hint>,
    )> = given.map(|class| (class, None, Vec::new()));
    let mut source = if given.is_some() { "given" } else { "none" };
    if let Some(app_id) = app_id {
        let dirs = match RuntimeDirs::from_env(|name| std::env::var(name).ok()) {
            Ok(dirs) => dirs,
            Err(error) => return refuse(json, &PlanError::from(error)),
        };
        let app = match dirs.app(app_id) {
            Ok(app) => app,
            Err(error) => return refuse(json, &PlanError::from(error)),
        };
        match load_record(&app) {
            Ok(Some(record)) => {
                let program = PathBuf::from(&record.main_executable);
                facts = static_facts(&program, &app.prefix.join("drive_c"));
            }
            Ok(None) => notes.push(format!(
                "{app_id} is not installed here, or its directory is not visible to this \
                 program; the plan is made without what its files would tell."
            )),
            Err(error) => return refuse(json, &PlanError::from(error)),
        }
        if known.is_none()
            && let Some(latest) = latest_result(&dirs, app_id)
            && let Some(failure) = latest.failure
        {
            known = Some((failure.class, failure.dll, failure.hints));
            source = "latest-result";
            notes.push(format!(
                "The failure is the one result {} recorded.",
                latest.result_id
            ));
        }
    }
    let host = LaunchHost::detect();
    let wanted: BTreeSet<String> = catalogue
        .remedies
        .iter()
        .filter_map(|remedy| remedy.font_file.clone())
        .collect();
    let abilities = Abilities {
        x11_display: host.as_ref().is_some_and(|host| host.sockets.x11.is_some()),
        // Whether the file system can fold case is found out by trying it,
        // which a plan does not do.
        case_folding: true,
        ntsync: host
            .as_ref()
            .is_some_and(|host| host.ntsync == NtsyncDevice::Present),
        builtin_dlls: wine_builtin_dlls(),
        font_files: font_files(&wanted),
    };
    notes.push(
        "drive-c-case-insensitive is listed as if the file system could fold case; the autopilot \
         finds out by trying the attribute on an empty directory when it runs. ntsync-off is \
         listed when /dev/ntsync is there; it only matters with Wine 11.0 or later."
            .to_owned(),
    );
    let exe = facts.exe_name.clone();
    let plans: Vec<CandidatePlan> = match &known {
        Some((class, dll, hints)) => {
            let mut hints = hints.clone();
            for hint in tur_runtime::failure::hints("", &facts, dll.as_deref()) {
                if !hints.contains(&hint) {
                    hints.push(hint);
                }
            }
            vec![candidates(
                &catalogue,
                *class,
                dll.as_deref(),
                &hints,
                &exe,
                RuntimeKind::Wine,
                &facts,
                &abilities,
            )]
        }
        None => {
            notes.push(
                "The failure is not known yet: the autopilot runs the application unchanged \
                 first and plans for the class it sees. Every class is listed."
                    .to_owned(),
            );
            let hints = tur_runtime::failure::hints("", &facts, None);
            FailureClass::ALL
                .iter()
                .filter(|class| !class.is_unsuppliable())
                .map(|class| {
                    candidates(
                        &catalogue,
                        *class,
                        None,
                        &hints,
                        &exe,
                        RuntimeKind::Wine,
                        &facts,
                        &abilities,
                    )
                })
                .collect()
        }
    };
    let unsuppliable: Vec<&str> = catalogue
        .microsoft_components
        .iter()
        .map(|component| component.name.as_str())
        .collect();
    if json {
        print_json(&json!({
            "app_id": app_id,
            "failure_source": source,
            "facts": FactsRecord::from(&facts),
            "plans": plans,
            "never_supplied": unsuppliable,
            "catalogue_checked": catalogue.checked,
            "notes": notes,
        }));
    } else {
        print_line(render_plans(&plans).trim_end());
        print_line(&format!(
            "Never supplied, the Windows guest is suggested instead: {}",
            unsuppliable.join("; ")
        ));
        for note in &notes {
            print_line(&format!("note: {note}"));
        }
    }
    EXIT_OK
}

/// The results kept for an application, oldest first, with the problems of
/// those that could not be read.
fn kept_results(dirs: &RuntimeDirs, app_id: &str) -> (Vec<AutopilotResult>, Vec<String>) {
    let directory = results_dir(dirs, app_id);
    let mut stamps: Vec<String> = std::fs::read_dir(&directory)
        .map(|reader| {
            reader
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| parse_result_id(&format!("{app_id}@{name}")).is_some())
                .collect()
        })
        .unwrap_or_default();
    stamps.sort();
    let mut results = Vec::new();
    let mut problems = Vec::new();
    for stamp in stamps {
        let result_id = format!("{app_id}@{stamp}");
        let path = directory.join(&stamp).join("result.json");
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        match read_text(&path, MAX_RESULT_BYTES)
            .and_then(|text| AutopilotResult::parse(&text, &result_id))
        {
            Ok(result) => results.push(result),
            Err(problem) => problems.push(format!("{result_id}: {problem}")),
        }
    }
    (results, problems)
}

fn latest_result(dirs: &RuntimeDirs, app_id: &str) -> Option<AutopilotResult> {
    kept_results(dirs, app_id).0.pop()
}

/// `app autopilot <app-id> --results`. Reads only.
fn run_autopilot_results(app_id: &str, json: bool) -> u8 {
    let dirs = match RuntimeDirs::from_env(|name| std::env::var(name).ok()) {
        Ok(dirs) => dirs,
        Err(error) => return refuse(json, &PlanError::from(error)),
    };
    if let Err(error) = dirs.app(app_id) {
        return refuse(json, &PlanError::from(error));
    }
    let directory = results_dir(&dirs, app_id);
    let (results, mut problems) = kept_results(&dirs, app_id);
    let applied = match load_applied(app_id) {
        Ok(applied) => applied,
        Err(problem) => {
            problems.push(problem);
            None
        }
    };
    let summaries: Vec<Value> = results
        .iter()
        .map(|result| {
            let stamp = result
                .result_id
                .split_once('@')
                .map_or("", |(_, stamp)| stamp);
            let record = directory.join(stamp).join("record.toml");
            json!({
                "result_id": result.result_id,
                "finished_at": result.finished_at,
                "outcome": result.outcome,
                "failure": result.failure.as_ref().map(|failure| json!({
                    "class": failure.class,
                    "code": failure.code,
                    "summary": failure.summary,
                })),
                "found": result.found.iter().map(|found| found.label.clone()).collect::<Vec<_>>(),
                "attempts": result.attempts.len().saturating_sub(1),
                "seconds": result.seconds,
                "runtime_version": result.runtime_version,
                "real_prefix_unchanged": result.real_prefix.unchanged,
                "result_file": directory.join(stamp).join("result.json"),
                "record_file": record.is_file().then_some(record),
            })
        })
        .collect();
    if json {
        print_json(&json!({
            "app_id": app_id,
            "looked_in": directory,
            "results": summaries,
            "applied": applied,
            "problems": problems,
        }));
    } else {
        if results.is_empty() {
            print_line(&format!("no results in {}", directory.display()));
        }
        for result in &results {
            let found: Vec<&str> = result
                .found
                .iter()
                .map(|found| found.label.as_str())
                .collect();
            print_line(&format!(
                "{}\t{}\t{}\t{}",
                result.result_id,
                result.outcome.as_str(),
                result
                    .failure
                    .as_ref()
                    .map_or("-", |failure| failure.class.as_str()),
                if found.is_empty() {
                    "-".to_owned()
                } else {
                    found.join(" + ")
                }
            ));
        }
        if let Some(applied) = &applied {
            let labels: Vec<&str> = applied.remedies.iter().map(|r| r.label.as_str()).collect();
            print_line(&format!(
                "applied: {} from {} (backup {})",
                labels.join(" + "),
                applied.result_id,
                applied.backup
            ));
        }
        for problem in &problems {
            print_error_line(&format!(
                "warning[{}]: {problem}",
                Code::AutopilotResultInvalid.error_code()
            ));
        }
    }
    EXIT_OK
}

/// `app autopilot <app-id>`: the search, in throwaway copies.
#[allow(clippy::too_many_lines)]
fn run_autopilot(app_id: &str, options: &AutopilotOptions<'_>) -> u8 {
    let common = options.common;
    let json = common.json;
    if options.max_attempts == 0 || options.max_attempts > MAX_ATTEMPTS_LIMIT {
        print_error_line(&format!(
            "error: --max-attempts must be 1 to {MAX_ATTEMPTS_LIMIT}"
        ));
        return EXIT_USAGE;
    }
    let check = match options.check {
        None => CheckSpec::default(),
        Some(path) => {
            match read_text(path, MAX_SMALL_FILE_BYTES).and_then(|text| CheckSpec::parse(&text)) {
                Ok(check) => check,
                Err(problem) => {
                    let message = format!("the check {} cannot be used: {problem}", path.display());
                    return fail(json, Code::AutopilotCheckInvalid, &message, EXIT_INPUT);
                }
            }
        }
    };
    let (session, record, metadata) = match autopilot_session(app_id, common) {
        Ok(prepared) => prepared,
        Err(error) => return refuse(json, &error),
    };
    if metadata.runtime != RuntimeKind::Wine {
        let message = "the autopilot runs Wine applications; this one runs with Proton";
        return fail(json, Code::AutopilotUnavailable, message, EXIT_REFUSED);
    }
    let installed = match session.runtime_version(RuntimeKind::Wine, common, Some(&metadata)) {
        Ok(installed) => installed,
        Err(error) => return refuse(json, &error),
    };
    if installed.version != metadata.runtime_version {
        return refuse(
            json,
            &PlanError::RuntimeMismatch {
                recorded: metadata.runtime_version.clone(),
                installed: installed.version.clone(),
            },
        );
    }
    if session.host.bwrap.is_none() {
        return refuse(json, &PlanError::Sandbox(SandboxError::Missing));
    }
    let Some(cp) = find_program(&PROGRAM_DIRECTORIES, &["cp"]) else {
        return refuse(json, &PlanError::ToolMissing { program: "cp" });
    };
    let probe = WindowProbe::for_host(
        &session.host,
        find_program(&PROGRAM_DIRECTORIES, &["xwininfo"]),
    );
    if check.window && probe.is_none() {
        let message = "the check looks for a window of the program, and there is no X11 display \
                       or xwininfo is not installed; give a check without a window (--check)";
        return fail(json, Code::AutopilotUnavailable, message, EXIT_REFUSED);
    }
    let program = PathBuf::from(&record.main_executable);
    let Ok(relative) = program.strip_prefix(&session.app.prefix) else {
        let message = "the application's main program is not inside its prefix";
        return fail(json, Code::AppRecordInvalid, message, EXIT_REFUSED);
    };
    let relative = relative.to_path_buf();
    // Remedies already applied are part of how the application runs.
    let applied = match load_applied(app_id) {
        Ok(applied) => applied,
        Err(problem) => return fail(json, Code::AppRecordInvalid, &problem, EXIT_REFUSED),
    };
    let catalogue = Catalogue::shipped();
    let facts = static_facts(&program, &session.app.prefix.join("drive_c"));

    let stamp = session.now.compact();
    let result_id = format!("{app_id}@{stamp}");
    let directory = results_dir(&session.dirs, app_id).join(&stamp);
    if std::fs::symlink_metadata(&directory).is_ok() {
        let message = format!(
            "{} exists already; start the autopilot again",
            directory.display()
        );
        return fail(json, Code::StepFailed, &message, EXIT_REFUSED);
    }
    if let Err(problem) = create_private_dir(&directory) {
        return fail(json, Code::StorageUnusable, &problem, EXIT_STEP_FAILED);
    }
    let wanted: BTreeSet<String> = catalogue
        .remedies
        .iter()
        .filter_map(|remedy| remedy.font_file.clone())
        .collect();
    let abilities = Abilities {
        x11_display: session.host.sockets.x11.is_some(),
        case_folding: probe_folding_ability(
            &session.dirs,
            session.host.chattr.as_deref(),
            &directory,
        ),
        ntsync: session.host.ntsync == NtsyncDevice::Present && wine_has_ntsync(&installed.version),
        builtin_dlls: wine_builtin_dlls(),
        font_files: font_files(&wanted),
    };

    let real_roots = [session.app.prefix.as_path(), session.app.home.as_path()];
    let before = fingerprint(&real_roots);
    let started = Instant::now();
    let case_folded = session.live_case_folding() == CaseFolding::Try;
    if !json {
        print_line(&format!(
            "autopilot: {} ({app_id}): copying the prefix",
            record.name
        ));
    }
    let base = directory.join("base");
    let (copier, snapshot) = match make_base(
        &session.dirs,
        &session.app,
        &base,
        &cp,
        session.host.chattr.as_deref(),
        case_folded,
    ) {
        Ok(made) => made,
        Err(problem) => {
            let _ = remove_tree(&session.dirs, &directory);
            let message = format!("the prefix could not be copied: {problem}");
            return fail(json, Code::StepFailed, &message, EXIT_STEP_FAILED);
        }
    };
    let mut notes = Vec::new();
    if let Err(problem) = import_registry(
        &session.host,
        &session.context,
        &session.dirs,
        &copy_paths(&base, app_id),
        &metadata,
        &trial_settings_text(),
        &directory.join("trial-settings.reg"),
    ) {
        notes.push(format!(
            "Wine's crash dialog could not be switched off in the copies ({problem}); a crash \
             then lasts until the time limit."
        ));
    }
    let mut runner = CopyTrials {
        host: &session.host,
        context: &session.context,
        dirs: &session.dirs,
        metadata: &metadata,
        app_id,
        base: base.clone(),
        base_case_folded: case_folded,
        result_dir: directory.clone(),
        program: relative,
        check: &check,
        class: check.class_for(&facts.exe_name),
        copier,
        probe,
        network: options.network,
        base_environment: applied
            .as_ref()
            .map(AppliedRemedies::environment_pairs)
            .unwrap_or_default(),
        base_hide_ntsync: applied.as_ref().is_some_and(|applied| applied.hide_ntsync),
        now: session.now,
    };
    let context = SearchContext {
        check: &check,
        facts: &facts,
        abilities: &abilities,
        runtime: RuntimeKind::Wine,
    };
    let found = search(
        &catalogue,
        &context,
        Limits {
            max_attempts: options.max_attempts,
            combinations: options.combinations,
        },
        &mut runner,
    );
    if options.keep_copies {
        notes.push(format!(
            "The copy the attempts started from was kept: {}.",
            base.display()
        ));
    } else {
        let _ = remove_tree(&session.dirs, &base);
    }
    let after = fingerprint(&real_roots);
    notes.extend(found.notes.iter().cloned());
    notes.push(format!(
        "Every attempt ran in a fresh copy of the prefix, in the sandbox, with no granted \
         directory and {}; the real prefix was only read.",
        if options.network {
            "the host's network"
        } else {
            "no network"
        }
    ));
    let unsuppliable = (found.outcome == Outcome::Unsuppliable).then(|| {
        let failure = found.failure.as_ref();
        let component = failure
            .and_then(|failure| failure.component.clone())
            .unwrap_or_else(|| "a .NET runtime".to_owned());
        Unsuppliable {
            explanation: format!(
                "The application needs {component}. TUR never downloads or installs Microsoft \
                 components, so no remedy can supply it. The Windows guest route runs the \
                 application on real Windows."
            ),
            component,
            dll: failure.and_then(|failure| failure.dll.clone()),
            route: "windows-guest".to_owned(),
        }
    });
    let result = AutopilotResult {
        schema_version: RESULT_SCHEMA_VERSION,
        result_id: result_id.clone(),
        app_id: app_id.to_owned(),
        app_name: record.name.clone(),
        app_version: record.version.clone(),
        installer_sha256: Some(record.install.source.sha256.clone()),
        main_executable_windows: record.main_executable_windows.clone(),
        runtime: RuntimeKind::Wine,
        runtime_version: installed.version.clone(),
        runtime_build: installed.build.clone(),
        started_at: session.now,
        finished_at: Timestamp::now(),
        outcome: found.outcome,
        check: check.clone(),
        trials_network: options.network,
        snapshot,
        facts: FactsRecord::from(&facts),
        failure: found.failure.clone(),
        plan: found.plan.clone(),
        evidence_sha256: evidence_digest(&found.attempts),
        attempts: found.attempts.clone(),
        found: found.found.clone(),
        unsuppliable,
        real_prefix: PrefixFingerprints {
            unchanged: before == after,
            before,
            after,
        },
        seconds: started.elapsed().as_secs_f64(),
        notes,
    };
    let result_file = directory.join("result.json");
    if let Err(problem) = write_private(&result_file, &result.to_json()) {
        return fail(json, Code::StorageUnusable, &problem, EXIT_STEP_FAILED);
    }
    let record_file = directory.join("record.toml");
    let record_written = match catalogue_record(&result) {
        Some(text) => match write_private(&record_file, &text) {
            Ok(()) => true,
            Err(problem) => return fail(json, Code::StorageUnusable, &problem, EXIT_STEP_FAILED),
        },
        None => false,
    };

    if json {
        print_json(&json!({
            "result": result,
            "result_file": result_file,
            "record_file": record_written.then_some(&record_file),
        }));
    } else {
        print_autopilot_text(
            &result,
            &result_file,
            record_written.then_some(record_file.as_path()),
        );
    }
    if !result.real_prefix.unchanged {
        let message = "the real prefix changed while the autopilot ran: the autopilot does not \
                       write to it, so another program did (was the application running?)";
        return fail_after_report(json, Code::StepFailed, message, EXIT_STEP_FAILED);
    }
    match result.outcome {
        Outcome::Fixed | Outcome::PassedWithoutRemedy => EXIT_OK,
        Outcome::NotFixed => fail_after_report(
            json,
            Code::AutopilotNoRemedy,
            "no remedy that was tried made the application pass its check",
            EXIT_NOT_FIXED,
        ),
        Outcome::Unsuppliable => fail_after_report(
            json,
            Code::AutopilotUnsuppliable,
            "the application needs a Microsoft component TUR cannot supply; the Windows guest \
             is the route",
            EXIT_NOT_FIXED,
        ),
    }
}

fn print_autopilot_text(result: &AutopilotResult, result_file: &Path, record_file: Option<&Path>) {
    print_line(&format!(
        "result {}: {}",
        result.result_id,
        result.outcome.as_str()
    ));
    if let Some(failure) = &result.failure {
        print_line(&format!(
            "  unchanged: {} ({}): {}",
            failure.class, failure.code, failure.summary
        ));
    }
    for attempt in result.attempts.iter().skip(1) {
        let labels: Vec<&str> = attempt.remedies.iter().map(|r| r.label.as_str()).collect();
        let what = match (&attempt.failure, &attempt.note) {
            (_, Some(note)) => format!("not applicable: {note}"),
            (Some(failure), None) => format!("failed: {}", failure.class),
            (None, None) => "passed".to_owned(),
        };
        print_line(&format!(
            "  attempt {}: {} -> {what} ({:.1} s)",
            attempt.number,
            labels.join(" + "),
            attempt.timings.snapshot_seconds
                + attempt.timings.remedy_seconds
                + attempt.timings.run_seconds
        ));
    }
    if !result.found.is_empty() {
        let labels: Vec<&str> = result.found.iter().map(|r| r.label.as_str()).collect();
        print_line(&format!("  found: {}", labels.join(" + ")));
        print_line(&format!(
            "  apply it with: tur-runtime app autopilot --apply {}",
            result.result_id
        ));
    }
    if let Some(unsuppliable) = &result.unsuppliable {
        print_line(&format!("  {}", unsuppliable.explanation));
    }
    print_line(&format!(
        "  copies: {}, the first took {:.1} s ({} files, {} bytes)",
        result.snapshot.method,
        result.snapshot.seconds,
        result.snapshot.files,
        result.snapshot.bytes
    ));
    print_line(&format!(
        "  real prefix: {}",
        if result.real_prefix.unchanged {
            "unchanged"
        } else {
            "CHANGED"
        }
    ));
    print_line(&format!("  {:.1} s in all", result.seconds));
    print_line(&format!("  result: {}", result_file.display()));
    if let Some(record) = record_file {
        print_line(&format!("  compatibility record: {}", record.display()));
    }
}

/// The steps that give the prefix's drive C: the case-folding attribute: a
/// new, empty, case-folded directory first; then, once the attribute is
/// known to be there, the contents are merged in (`merge_copy`, not a plan
/// step) and a second plan swaps the two directories. Returns both plans,
/// drive C: and the new directory.
fn case_fold_swap_plans(session: &Session) -> (Plan, Plan, PathBuf, PathBuf) {
    let drive_c = session.app.prefix.join("drive_c");
    let fresh = session.app.prefix.join("drive_c.tur-autopilot");
    let old = session.app.prefix.join("drive_c.tur-before");
    let mut first = Plan::new(Operation::Configure, &session.app.app_id);
    first.steps.push(Step::RemoveTree {
        path: fresh.clone(),
    });
    first.steps.push(Step::CaseFold {
        path: fresh.clone(),
        command: session
            .host
            .chattr
            .as_deref()
            .and_then(|chattr| chattr_command(chattr, &fresh)),
    });
    let mut second = Plan::new(Operation::Configure, &session.app.app_id);
    second.steps.push(Step::Rename {
        from: drive_c.clone(),
        to: old.clone(),
    });
    second.steps.push(Step::Rename {
        from: fresh.clone(),
        to: drive_c.clone(),
    });
    second.steps.push(Step::RemoveTree { path: old });
    (first, second, drive_c, fresh)
}

/// `app autopilot --apply <result-id>`: the person's decision to apply what
/// a result found to the real prefix. A backup is taken first; any failure
/// after it restores the backup.
#[allow(clippy::too_many_lines)]
fn run_autopilot_apply(result_id: &str, common: &Common) -> u8 {
    let json = common.json;
    let Some((app_id, stamp)) = parse_result_id(result_id) else {
        let message = format!("\"{result_id}\" is not a result identifier (<app-id>@<time>)");
        return fail(json, Code::AutopilotResultMissing, &message, EXIT_INPUT);
    };
    let (session, record, metadata) = match autopilot_session(app_id, common) {
        Ok(prepared) => prepared,
        Err(error) => return refuse(json, &error),
    };
    let directory = results_dir(&session.dirs, app_id).join(stamp);
    let text = match read_text(&directory.join("result.json"), MAX_RESULT_BYTES) {
        Ok(text) => text,
        Err(problem) => {
            let message = format!("there is no result {result_id}: {problem}");
            return fail(json, Code::AutopilotResultMissing, &message, EXIT_INPUT);
        }
    };
    let result = match AutopilotResult::parse(&text, result_id) {
        Ok(result) => result,
        Err(problem) => {
            let message = format!("the result {result_id} cannot be used: {problem}");
            return fail(json, Code::AutopilotResultInvalid, &message, EXIT_INPUT);
        }
    };
    if result.outcome != Outcome::Fixed || result.found.is_empty() {
        let message = format!(
            "the result {result_id} found no remedy ({}); there is nothing to apply",
            result.outcome.as_str()
        );
        return fail(json, Code::AutopilotNothingToApply, &message, EXIT_REFUSED);
    }
    let installed = match session.runtime_version(RuntimeKind::Wine, common, Some(&metadata)) {
        Ok(installed) => installed,
        Err(error) => return refuse(json, &error),
    };
    if result.runtime_version != installed.version || metadata.runtime_version != installed.version
    {
        let message = format!(
            "the result was measured with {}, the prefix is on {} and {} is installed; run the \
             autopilot again",
            result.runtime_version, metadata.runtime_version, installed.version
        );
        return fail(json, Code::AutopilotUnavailable, &message, EXIT_REFUSED);
    }
    let exe = Path::new(&record.main_executable)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let catalogue = Catalogue::shipped();
    let mut resolved: Vec<Resolved> = Vec::new();
    for found in &result.found {
        if found
            .parameters
            .get("exe")
            .is_some_and(|named| *named != exe)
        {
            let message = format!(
                "the result was made for the program {:?}, and the application's program is {exe:?}",
                found.parameters.get("exe")
            );
            return fail(json, Code::AutopilotResultInvalid, &message, EXIT_REFUSED);
        }
        match catalogue.resolve(found) {
            Ok(remedy) => resolved.push(remedy),
            Err(problem) => {
                return fail(json, Code::AutopilotResultInvalid, &problem, EXIT_REFUSED);
            }
        }
    }
    if std::fs::symlink_metadata(applied_file(&session.app)).is_ok() {
        let message = format!(
            "remedies are already applied to {app_id}; undo them first (app autopilot {app_id} \
             --revert)"
        );
        return fail(json, Code::AutopilotAlreadyApplied, &message, EXIT_REFUSED);
    }
    let case_before = session.live_case_folding() == CaseFolding::Try;
    let planner = session.planner();
    let (backup, backup_name) = match planner.backup("autopilot") {
        Ok(planned) => planned,
        Err(error) => return refuse(json, &error),
    };
    let mut changes = Plan::new(Operation::Configure, app_id);
    let registry: Vec<&Resolved> = resolved
        .iter()
        .filter(|remedy| !remedy.registry.is_empty())
        .collect();
    if !registry.is_empty() {
        let reg_path = directory.join("apply.reg");
        changes.steps.push(Step::WriteFile {
            path: reg_path.clone(),
            contents: reg_file(&[], &registry),
        });
        let command = match reg_import_command(
            &session.host,
            &session.context,
            &session.app,
            &metadata,
            &reg_path,
        ) {
            Ok(command) => command,
            Err(error) => return refuse(json, &error),
        };
        changes.steps.push(Step::RunBounded {
            label: "set the remedies in the prefix's registry".to_owned(),
            command,
            limit_seconds: 180,
        });
    }
    let fold = !case_before
        && resolved
            .iter()
            .any(|remedy| remedy.kind == RemedyKind::DriveCCaseFolding);
    let swap = fold.then(|| case_fold_swap_plans(&session));
    let environment: Vec<EnvironmentValue> = resolved
        .iter()
        .flat_map(|remedy| remedy.environment.iter())
        .map(|(name, value)| EnvironmentValue {
            name: name.clone(),
            value: value.clone(),
        })
        .collect();
    let applied = AppliedRemedies {
        schema_version: APPLIED_SCHEMA_VERSION,
        app_id: app_id.to_owned(),
        result_id: result_id.to_owned(),
        applied_at: session.now,
        backup: backup_name.clone(),
        remedies: result.found.clone(),
        environment,
        hide_ntsync: resolved
            .iter()
            .any(|remedy| remedy.kind == RemedyKind::HideNtsync),
        case_folding_before: case_before,
    };
    let mut record_plan = Plan::new(Operation::Configure, app_id);
    record_plan.steps.push(Step::WriteFile {
        path: applied_file(&session.app),
        contents: applied.to_json(),
    });
    let undo = format!("tur-runtime app autopilot {app_id} --revert");

    let mut plans: Vec<&Plan> = vec![&backup, &changes];
    if let Some((first, second, _, _)) = &swap {
        plans.push(first);
        plans.push(second);
    }
    plans.push(&record_plan);
    if common.dry_run {
        if json {
            print_json(
                &json!({ "dry_run": true, "plans": plans, "applied": applied, "undo": undo }),
            );
        } else {
            for plan in &plans {
                print_line(plan.render_text().trim_end());
            }
        }
        return EXIT_OK;
    }

    let mut executor = session.executor(Forward::LogOnly);
    let report = run_plan(&backup, &mut executor);
    if let Some(failure) = &report.failure {
        let message = failure_message(failure, executor.last_output());
        return fail(json, Code::StepFailed, &message, EXIT_STEP_FAILED);
    }
    let restore_case = if case_before {
        CaseFolding::Try
    } else {
        CaseFolding::Off
    };
    let roll_back = |executor: &mut SystemExecutor, why: String| -> u8 {
        let restored = planner
            .restore(&backup_name, true, restore_case)
            .map(|plan| run_plan(&plan, executor).failure.is_none())
            .unwrap_or(false);
        let message = format!(
            "{why}. {}",
            if restored {
                format!("The prefix was restored from the backup {backup_name}.")
            } else {
                format!(
                    "Restoring the backup {backup_name} failed too; restore it with `tur-runtime \
                     prefix restore {app_id} {backup_name}`."
                )
            }
        );
        fail(json, Code::StepFailed, &message, EXIT_STEP_FAILED)
    };
    let report = run_plan(&changes, &mut executor);
    if let Some(failure) = &report.failure {
        let why = failure_message(failure, executor.last_output());
        return roll_back(&mut executor, why);
    }
    if let Some((first, second, drive_c, fresh)) = &swap {
        let report = run_plan(first, &mut executor);
        let folded = report
            .case_folding
            .as_ref()
            .is_some_and(|record| record.casefolded);
        if report.failure.is_some() || !folded {
            let why = report.case_folding.as_ref().map_or_else(
                || "drive C: could not be rebuilt".to_owned(),
                |record| {
                    format!(
                        "drive C: could not be made case-insensitive: {}",
                        record.summary()
                    )
                },
            );
            return roll_back(&mut executor, why);
        }
        if let Err(problem) = merge_copy(drive_c, fresh) {
            let _ = remove_tree(&session.dirs, fresh);
            let why = format!("drive C: could not be copied: {problem}");
            return roll_back(&mut executor, why);
        }
        let report = run_plan(second, &mut executor);
        if let Some(failure) = &report.failure {
            let why = failure_message(failure, executor.last_output());
            return roll_back(&mut executor, why);
        }
    }
    let report = run_plan(&record_plan, &mut executor);
    if let Some(failure) = &report.failure {
        let why = failure_message(failure, executor.last_output());
        return roll_back(&mut executor, why);
    }
    if json {
        print_json(&json!({ "dry_run": false, "applied": applied, "undo": undo }));
    } else {
        let labels: Vec<&str> = applied.remedies.iter().map(|r| r.label.as_str()).collect();
        print_line(&format!("applied to {app_id}: {}", labels.join(" + ")));
        print_line(&format!("  backup taken first: {backup_name}"));
        print_line(&format!("  undo with: {undo}"));
    }
    EXIT_OK
}

/// `app autopilot <app-id> --revert`: restore the backup taken when the
/// remedies were applied, and forget them.
fn run_autopilot_revert(app_id: &str, common: &Common) -> u8 {
    let json = common.json;
    let session = match Session::open(app_id, common) {
        Ok(session) => session,
        Err(error) => return refuse(json, &error),
    };
    let applied = match load_applied(app_id) {
        Ok(Some(applied)) => applied,
        Ok(None) => {
            let message = format!("no remedies are applied to {app_id}");
            return fail(json, Code::AutopilotNotApplied, &message, EXIT_REFUSED);
        }
        Err(problem) => return fail(json, Code::AppRecordInvalid, &problem, EXIT_REFUSED),
    };
    if !session.app.backup_archive(&applied.backup).is_file() {
        return refuse(
            json,
            &PlanError::BackupMissing {
                name: applied.backup.clone(),
            },
        );
    }
    let case_folding = if applied.case_folding_before {
        CaseFolding::Try
    } else {
        CaseFolding::Off
    };
    let exists = std::fs::symlink_metadata(&session.app.prefix).is_ok();
    let mut plan = match session
        .planner()
        .restore(&applied.backup, exists, case_folding)
    {
        Ok(plan) => plan,
        Err(error) => return refuse(json, &error),
    };
    plan.steps.push(Step::RemoveTree {
        path: applied_file(&session.app),
    });
    if common.dry_run {
        if json {
            print_json(&json!({ "dry_run": true, "plan": plan }));
        } else {
            print_line(plan.render_text().trim_end());
        }
        return EXIT_OK;
    }
    let mut executor = session.executor(Forward::LogOnly);
    let report = run_plan(&plan, &mut executor);
    if let Some(failure) = &report.failure {
        let message = failure_message(failure, executor.last_output());
        return fail(json, Code::StepFailed, &message, EXIT_STEP_FAILED);
    }
    if json {
        print_json(&json!({
            "dry_run": false,
            "reverted": applied.result_id,
            "restored_backup": applied.backup,
        }));
    } else {
        print_line(&format!(
            "reverted {app_id}: restored the backup {} taken before {} was applied",
            applied.backup, applied.result_id
        ));
    }
    EXIT_OK
}
