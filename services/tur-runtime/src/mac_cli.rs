// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-runtime mac ...`: macOS applications in Darling
//! (docs/architecture/runtime-manager.md, "Running a macOS application").
//!
//! ```text
//! tur-runtime mac identify <path> [--json]          what a package is; whether Darling can start it
//! tur-runtime mac route <path> [--linux-edition] [--rehost V] [--darling-measured]
//! tur-runtime mac runtime status|fetch|remove       TUR's build of Darling on this computer
//! tur-runtime mac install <package> ...             install into a Darling prefix of its own
//! tur-runtime mac list | show <app-id>              what is installed
//! tur-runtime mac launch <app-id> [-- args...]      start it, in the sandbox
//! tur-runtime mac uninstall <app-id>                remove it, and everything kept for it
//! ```
//!
//! `identify`, `route`, `runtime status`, `list` and `show` read only.
//! Everything else takes `--dry-run`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use tur_runtime::exec::{Forward, SystemExecutor};
use tur_runtime::grant::GrantContext;
use tur_runtime::launch_host::{LaunchHost, PROGRAM_DIRECTORIES, find_program};
use tur_runtime::macos::MacError;
use tur_runtime::macos::analysis::{
    AnalysisOptions, AppAnalysis, Verdict, analyse_bundle, analyse_program,
};
use tur_runtime::macos::darling::{
    self, Artifact, DarlingMode, Manifest, PrefixLayout, Source, Store,
};
use tur_runtime::macos::package::{self, PackageKind};
use tur_runtime::macos::plan::{self as mac_plan, Setup};
use tur_runtime::macos::record::{
    self, DarlingRecord, LauncherRecord, MacAppRecord, RECORD_SCHEMA_VERSION, RecordedFinding,
    SourceRecord,
};
use tur_runtime::macos::route::{RehostVerdict, RouteFacts, decide};
use tur_runtime::paths::{AppPaths, RuntimeDirs};
use tur_runtime::plan::{Plan, PlanReport, run_plan};
use tur_runtime::prefix::default_prefix_id;
use tur_runtime::sha256::sha256_file;
use tur_runtime::timefmt::Timestamp;
use tur_runtime::{Arch, Code};

use crate::{
    EXIT_FINDINGS, EXIT_INPUT, EXIT_MISSING, EXIT_OK, EXIT_REFUSED, EXIT_STEP_FAILED,
    EXIT_UNSUPPORTED, EXIT_USAGE, fail, fail_after_report, failure_message, parse_environment,
    print_error_line, print_json, print_line,
};

/// The `mac` commands.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub(crate) enum MacCommand {
    /// Say what a macOS package or program is and whether Darling can be
    /// expected to start it: architectures, Swift, SwiftUI, encryption.
    /// Nothing is run or unpacked.
    Identify {
        /// The application bundle, disk image, installer package, archive or
        /// program.
        path: PathBuf,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Assume this processor instead of this computer's.
        #[arg(long, value_name = "ARCH")]
        host_arch: Option<String>,
        /// Treat a Swift application that carries its own Swift runtime as
        /// one to try rather than one to refuse.
        #[arg(long)]
        allow_swift: bool,
    },
    /// Decide the route for a macOS package: its Linux edition, re-hosting
    /// its portable payload, Darling when it is measured to work there, or
    /// none. Nothing is run.
    Route {
        /// The package.
        path: PathBuf,
        /// The catalogue knows a native Linux edition of the application.
        #[arg(long)]
        linux_edition: bool,
        /// The re-hosting component's verdict on the payload: portable,
        /// not-portable or unknown.
        #[arg(long, value_name = "VERDICT", default_value = "unknown")]
        rehost: String,
        /// The catalogue records the application as measured to work in
        /// Darling.
        #[arg(long)]
        darling_measured: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// TUR's build of Darling on this computer.
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },
    /// Install a macOS application into a Darling prefix of its own, inside
    /// the sandbox. Darling is fetched first when it is not here yet.
    Install {
        /// An application bundle (`.app`), a disk image (`.dmg`), an
        /// installer package (`.pkg`) or a zip archive holding an
        /// application.
        package: PathBuf,
        /// The application identifier, such as org.example.app. By default
        /// the bundle's identifier, or one made from the file's name under
        /// "local.".
        #[arg(long, value_name = "ID")]
        app_id: Option<String>,
        /// The application's name, instead of the one its bundle states.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// The bundle to install when the package holds several, by name
        /// (`Example.app`) or by its path in the package.
        #[arg(long = "app", value_name = "BUNDLE")]
        bundle: Option<String>,
        /// Try a Swift application that carries its own Swift runtime.
        #[arg(long)]
        allow_swift: bool,
        /// Do not share the host's network with the application.
        #[arg(long)]
        no_network: bool,
        /// How Darling runs: nonroot (the default) or rootless.
        #[arg(long, value_name = "MODE", default_value = "nonroot")]
        darling_mode: String,
        /// How the prefix is made: layered (the application's own layer over
        /// Darling's read-only system; the default with nonroot) or darling
        /// (Darling makes it: a full copy in nonroot, its own overlay in
        /// rootless; the default with rootless).
        #[arg(long, value_name = "LAYOUT")]
        prefix_layout: Option<String>,
        /// How long an installer package may take, in seconds.
        #[arg(long, value_name = "SECONDS", default_value_t = 900)]
        timeout: u64,
        /// Refuse instead of fetching Darling when it is not here.
        #[arg(long)]
        no_fetch: bool,
        /// Keep what was made when the installation fails, for a look.
        #[arg(long)]
        keep_failed: bool,
        /// The tur-runtime program the launcher entry starts.
        #[arg(long, default_value = "tur-runtime")]
        launcher: String,
        #[command(flatten)]
        common: MacCommon,
    },
    /// List the installed macOS applications.
    List {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show the record of an installed macOS application.
    Show {
        /// The application identifier.
        app_id: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Start an installed macOS application in Darling, inside the sandbox.
    Launch {
        /// The application identifier.
        app_id: String,
        /// Set an environment variable for the application.
        #[arg(long = "env", value_name = "NAME=VALUE")]
        environment: Vec<String>,
        #[command(flatten)]
        common: MacCommon,
        /// Arguments passed on to the application.
        #[arg(last = true)]
        arguments: Vec<String>,
    },
    /// Remove an installed macOS application: its Darling prefix, its private
    /// home, its record, its launcher entry and its logs; then check that
    /// nothing is left.
    Uninstall {
        /// The application identifier.
        app_id: String,
        #[command(flatten)]
        common: MacCommon,
    },
}

/// The commands about TUR's build of Darling.
#[derive(Subcommand)]
pub(crate) enum RuntimeCommand {
    /// Say whether TUR's build of Darling is here, and which one.
    Status {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Fetch TUR's build of Darling from the package registry, check its
    /// size and SHA-256 against the manifest, and unpack it.
    Fetch {
        /// Import the archive from this file instead of downloading it.
        #[arg(long, value_name = "FILE")]
        from_file: Option<PathBuf>,
        /// The archive's SHA-256, for a build this program's manifest does
        /// not name as published yet.
        #[arg(long, value_name = "HEX")]
        sha256: Option<String>,
        #[command(flatten)]
        common: MacCommon,
    },
    /// Delete every build of Darling kept here. Installed applications keep
    /// their prefixes and need Darling fetched again to start.
    Remove {
        #[command(flatten)]
        common: MacCommon,
    },
}

/// Options shared by the `mac` commands that plan something.
#[derive(Args)]
pub(crate) struct MacCommon {
    /// Print the plan and carry none of it out.
    #[arg(long)]
    dry_run: bool,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
    /// Use this bubblewrap program instead of the one found in /usr/bin.
    #[arg(long, value_name = "PATH")]
    sandbox_program: Option<PathBuf>,
    /// Use Darling although this system is not the Fedora release it was
    /// built for. Its libraries may not match.
    #[arg(long)]
    any_system: bool,
}

/// Runs a `mac` command and returns the exit status.
pub(crate) fn run(command: MacCommand) -> u8 {
    match command {
        MacCommand::Identify {
            path,
            json,
            host_arch,
            allow_swift,
        } => run_identify(&path, json, host_arch.as_deref(), allow_swift),
        MacCommand::Route {
            path,
            linux_edition,
            rehost,
            darling_measured,
            json,
        } => run_route(&path, linux_edition, &rehost, darling_measured, json),
        MacCommand::Runtime { command } => match command {
            RuntimeCommand::Status { json } => run_status(json),
            RuntimeCommand::Fetch {
                from_file,
                sha256,
                common,
            } => run_fetch(from_file.as_deref(), sha256.as_deref(), &common),
            RuntimeCommand::Remove { common } => run_remove_runtime(&common),
        },
        MacCommand::Install {
            package,
            app_id,
            name,
            bundle,
            allow_swift,
            no_network,
            darling_mode,
            prefix_layout,
            timeout,
            no_fetch,
            keep_failed,
            launcher,
            common,
        } => run_install(&Install {
            package: &package,
            app_id: app_id.as_deref(),
            name: name.as_deref(),
            bundle: bundle.as_deref(),
            allow_swift,
            network: !no_network,
            mode: &darling_mode,
            layout: prefix_layout.as_deref(),
            timeout,
            fetch: !no_fetch,
            keep_failed,
            launcher: &launcher,
            common: &common,
        }),
        MacCommand::List { json } => run_list(json),
        MacCommand::Show { app_id, json } => run_show(&app_id, json),
        MacCommand::Launch {
            app_id,
            environment,
            common,
            arguments,
        } => run_launch(&app_id, &environment, &common, &arguments),
        MacCommand::Uninstall { app_id, common } => run_uninstall(&app_id, &common),
    }
}

/// An error with the exit status it ends the command with.
struct Failure {
    code: Code,
    message: String,
    status: u8,
}

impl Failure {
    fn new(code: Code, message: impl Into<String>, status: u8) -> Self {
        Failure {
            code,
            message: message.into(),
            status,
        }
    }

    fn from_mac(error: MacError, status: u8) -> Self {
        Failure::new(error.code, error.message, status)
    }

    fn report(&self, json: bool) -> u8 {
        fail(json, self.code, &self.message, self.status)
    }
}

fn this_arch() -> Arch {
    Arch::from_name(std::env::consts::ARCH).unwrap_or(Arch::Other)
}

fn arch_option(name: Option<&str>) -> Result<Arch, Failure> {
    match name {
        None => Ok(this_arch()),
        Some(name) => Arch::from_name(name).ok_or_else(|| {
            Failure::new(
                Code::MacosRequestInvalid,
                format!("unknown architecture \"{name}\" for --host-arch"),
                EXIT_USAGE,
            )
        }),
    }
}

fn verdict_exit(verdict: Verdict) -> u8 {
    match verdict {
        Verdict::Refused => EXIT_UNSUPPORTED,
        Verdict::Runnable | Verdict::RunnableWithWarnings => EXIT_OK,
    }
}

fn print_findings(findings: &[tur_runtime::macos::analysis::Finding]) {
    for finding in findings {
        let severity = serde_json::to_value(finding.severity)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        match finding.code {
            Some(code) => print_line(&format!("  {severity} [{code}]: {}", finding.message)),
            None => print_line(&format!("  {severity}: {}", finding.message)),
        }
    }
}

fn run_identify(path: &Path, json: bool, host_arch: Option<&str>, allow_swift: bool) -> u8 {
    let host = match arch_option(host_arch) {
        Ok(host) => host,
        Err(failure) => return failure.report(json),
    };
    let options = AnalysisOptions { host, allow_swift };
    let kind = match package::identify(path) {
        Ok(kind) => kind,
        Err(error) => return fail(json, error.code(), &error.to_string(), EXIT_INPUT),
    };
    let shown = path.display().to_string();
    match kind {
        PackageKind::AppBundle => match analyse_bundle(path, &options) {
            Ok(analysis) => {
                if json {
                    print_json(&json!({
                        "path": shown,
                        "kind": kind,
                        "verdict": analysis.verdict,
                        "application": analysis,
                    }));
                } else {
                    print_line(&format!("{shown}: {}", kind.label()));
                    print_line(&format!(
                        "  {} {}",
                        analysis.name.as_deref().unwrap_or(&analysis.bundle),
                        analysis.version.as_deref().unwrap_or("")
                    ));
                    print_findings(&analysis.findings);
                    print_line(&format!("  verdict: {}", verdict_name(analysis.verdict)));
                }
                verdict_exit(analysis.verdict)
            }
            Err(error) => fail(json, error.code(), &error.to_string(), EXIT_INPUT),
        },
        PackageKind::Program => match analyse_program(path, &options) {
            Ok(analysis) => {
                if json {
                    print_json(&json!({
                        "path": shown,
                        "kind": kind,
                        "verdict": analysis.verdict,
                        "program": analysis,
                    }));
                } else {
                    print_line(&format!("{shown}: {}", kind.label()));
                    print_findings(&analysis.findings);
                    print_line(&format!("  verdict: {}", verdict_name(analysis.verdict)));
                }
                verdict_exit(analysis.verdict)
            }
            Err(error) => fail(json, error.code(), &error.to_string(), EXIT_INPUT),
        },
        PackageKind::DiskImage | PackageKind::ZipArchive | PackageKind::InstallerPackage => {
            let note = "The application inside is analysed when the package is installed; \
                        nothing is unpacked or run here.";
            if json {
                print_json(&json!({
                    "path": shown,
                    "kind": kind,
                    "verdict": null,
                    "note": note,
                }));
            } else {
                print_line(&format!("{shown}: {}", kind.label()));
                print_line(&format!("  {note}"));
            }
            EXIT_OK
        }
        PackageKind::NotMacSoftware => fail(
            json,
            Code::MacosNotMacSoftware,
            &format!("{shown} is not a macOS application, disk image, installer package or archive"),
            EXIT_UNSUPPORTED,
        ),
    }
}

fn verdict_name(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Runnable => "nothing known stands in the way (not a promise that it works)",
        Verdict::RunnableWithWarnings => "it can be tried; see the warnings",
        Verdict::Refused => "refused",
    }
}

fn run_route(path: &Path, linux_edition: bool, rehost: &str, measured: bool, json: bool) -> u8 {
    let Some(rehost) = RehostVerdict::from_name(rehost) else {
        return fail(
            json,
            Code::MacosRequestInvalid,
            "--rehost takes portable, not-portable or unknown",
            EXIT_USAGE,
        );
    };
    let options = AnalysisOptions {
        host: this_arch(),
        allow_swift: false,
    };
    let refusal = match package::identify(path) {
        Ok(PackageKind::AppBundle) => match analyse_bundle(path, &options) {
            Ok(analysis) => analysis
                .refusal()
                .map(|finding| (finding.code.unwrap_or(Code::MacosNoRoute), finding.message.clone())),
            Err(error) => return fail(json, error.code(), &error.to_string(), EXIT_INPUT),
        },
        Ok(PackageKind::Program) => match analyse_program(path, &options) {
            Ok(analysis) => analysis
                .refusal()
                .map(|finding| (finding.code.unwrap_or(Code::MacosNoRoute), finding.message.clone())),
            Err(error) => return fail(json, error.code(), &error.to_string(), EXIT_INPUT),
        },
        Ok(PackageKind::NotMacSoftware) => Some((
            Code::MacosNotMacSoftware,
            "It is not a macOS package.".to_owned(),
        )),
        Ok(_) => None,
        Err(error) => return fail(json, error.code(), &error.to_string(), EXIT_INPUT),
    };
    let decision = decide(&RouteFacts {
        linux_edition,
        rehost,
        darling_measured: measured,
        darling_refusal: refusal,
    });
    if json {
        print_json(&json!(decision));
    } else {
        for considered in &decision.considered {
            print_line(&format!(
                "{} {}: {}",
                if considered.usable { "+" } else { "-" },
                considered.route.as_str(),
                considered.reason
            ));
        }
    }
    match (decision.chosen, decision.refusal) {
        (Some(route), _) => {
            if !json {
                print_line(&format!("route: {}", route.as_str()));
            }
            EXIT_OK
        }
        (None, refusal) => {
            if !json {
                print_error_line(&format!(
                    "error[{}]: {}",
                    refusal.unwrap_or(Code::MacosNoRoute),
                    decision.message
                ));
            }
            EXIT_UNSUPPORTED
        }
    }
}

/// Where this program keeps things, and the build of Darling it would use.
struct Context {
    dirs: RuntimeDirs,
    store: Store,
    artifact: Option<Artifact>,
}

impl Context {
    fn open() -> Result<Self, Failure> {
        let dirs = RuntimeDirs::from_env(|name| std::env::var(name).ok())
            .map_err(|error| Failure::new(Code::StorageUnusable, error.to_string(), EXIT_REFUSED))?;
        let manifest = Manifest::compiled().map_err(|error| {
            Failure::new(
                Code::StorageUnusable,
                format!("the Darling manifest compiled into this program is not valid: {error}"),
                EXIT_INPUT,
            )
        })?;
        let store = Store::new(&dirs);
        let artifact = manifest.for_arch(this_arch()).cloned();
        Ok(Context {
            dirs,
            store,
            artifact,
        })
    }

    fn artifact(&self) -> Result<&Artifact, Failure> {
        self.artifact.as_ref().ok_or_else(|| {
            Failure::new(
                Code::MacosRuntimeNotPublished,
                format!(
                    "TUR has no build of Darling for {} processors",
                    this_arch().as_str()
                ),
                EXIT_MISSING,
            )
        })
    }
}

fn check_release(artifact: &Artifact, any_system: bool) -> Result<(), Failure> {
    if any_system {
        return Ok(());
    }
    match darling::system_release() {
        Some(release) if release == artifact.fedora_release => Ok(()),
        found => Err(Failure::new(
            Code::MacosRuntimeIncompatible,
            format!(
                "TUR's build of Darling links against the libraries of Fedora {}, and this system \
                 is {}; --any-system uses it anyway",
                artifact.fedora_release,
                found.map_or_else(|| "not Fedora".to_owned(), |release| format!("Fedora {release}"))
            ),
            EXIT_REFUSED,
        )),
    }
}

fn program(names: &[&str], what: &str) -> Result<PathBuf, Failure> {
    find_program(&PROGRAM_DIRECTORIES, names).ok_or_else(|| {
        Failure::new(
            Code::MacosToolMissing,
            format!("{what} ({}) is not installed", names.join(" or ")),
            EXIT_MISSING,
        )
    })
}

fn executor(dirs: &RuntimeDirs, log: PathBuf, forward: Forward) -> SystemExecutor {
    if let Some(parent) = log.parent() {
        let _ = fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent);
    }
    SystemExecutor::new(dirs)
        .with_command_log(log)
        .forwarding(forward)
}

/// Carries a plan out; a failed step is a [`Failure`] with `code`.
fn carry_out(
    plan: &Plan,
    executor: &mut SystemExecutor,
    code: Code,
    reports: &mut Vec<Value>,
) -> Result<PlanReport, Failure> {
    let report = run_plan(plan, executor);
    reports.push(json!({ "plan": plan, "report": report }));
    if let Some(failure) = &report.failure {
        return Err(Failure::new(
            code,
            failure_message(failure, executor.last_output()),
            EXIT_STEP_FAILED,
        ));
    }
    Ok(report)
}

fn run_status(json: bool) -> u8 {
    let context = match Context::open() {
        Ok(context) => context,
        Err(failure) => return failure.report(json),
    };
    let manifest = Manifest::compiled().unwrap_or_else(|_| Manifest {
        comment: None,
        schema_version: darling::MANIFEST_SCHEMA_VERSION,
        artifacts: Vec::new(),
    });
    let system = darling::system_build();
    let status = darling::status(&context.store, &manifest, this_arch(), system.as_ref());
    if json {
        print_json(&json!(status));
    } else {
        print_line(&status.message);
        if let Some(artifact) = &status.artifact {
            print_line(&format!("  build: {}", artifact.id));
            print_line(&format!("  Darling commit: {}", artifact.darling_commit));
        }
        if let Some(root) = &status.install_root {
            print_line(&format!("  at: {root}"));
        }
    }
    EXIT_OK
}

/// Makes TUR's build of Darling ready: fetched from the registry, or
/// imported from a file. Returns the build's identifier.
fn ensure_runtime(
    context: &Context,
    from_file: Option<&Path>,
    given: Option<&str>,
    dry_run: bool,
    reports: &mut Vec<Value>,
) -> Result<String, Failure> {
    let artifact = context.artifact()?;
    let store = &context.store;
    if from_file.is_none() && darling::is_ready(store, &artifact.id) {
        return Ok(artifact.id.clone());
    }
    let tar = program(&["tar"], "tar")?;
    let log = context
        .dirs
        .state_root()
        .join("logs")
        .join("darling")
        .join("operations.log");
    if let Some(file) = from_file {
        let (digest, size) = darling::check_archive(file, artifact, given)
            .map_err(|error| Failure::from_mac(error, EXIT_INPUT))?;
        let record = darling::verified_record(artifact, digest, size, Source::File, Timestamp::now());
        let plan = darling::plan_unpack(store, artifact, file, &tar, &record)
            .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
        if dry_run {
            reports.push(json!({ "plan": plan }));
            return Ok(artifact.id.clone());
        }
        let mut executor = executor(&context.dirs, log, Forward::LogOnly);
        carry_out(&plan, &mut executor, Code::MacosInstallFailed, reports)?;
        return Ok(artifact.id.clone());
    }

    if !artifact.published() {
        return Err(Failure::new(
            Code::MacosRuntimeNotPublished,
            "TUR has not published this build of Darling yet; import one with \
             `tur-runtime mac runtime fetch --from-file FILE --sha256 HEX`",
            EXIT_MISSING,
        ));
    }
    let curl = program(&["curl"], "curl")?;
    let download = darling::plan_download(store, artifact, &curl)
        .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
    if dry_run {
        reports.push(json!({
            "plan": download,
            "then": "the archive's size and SHA-256 are checked against the manifest, and it is unpacked",
        }));
        return Ok(artifact.id.clone());
    }

    // The registry's anonymous pull token, into a file only this user can
    // read; it never appears on a command line or in a log.
    let token_command = darling::token_command(&curl, artifact)
        .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
    let output = Command::new(&token_command.program)
        .args(&token_command.arguments)
        .env_clear()
        .envs(&token_command.environment)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            Failure::new(
                Code::MacosToolMissing,
                format!("cannot start curl: {error}"),
                EXIT_MISSING,
            )
        })?;
    if !output.status.success() {
        return Err(Failure::new(
            Code::MacosRuntimeNotPublished,
            format!(
                "the package registry did not answer: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            EXIT_STEP_FAILED,
        ));
    }
    let token = darling::parse_token(&output.stdout)
        .map_err(|error| Failure::from_mac(error, EXIT_STEP_FAILED))?;
    let staging = store.staging(&artifact.id);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&staging)
        .map_err(|error| Failure::new(Code::StorageUnusable, error.to_string(), EXIT_REFUSED))?;
    let header = store.header_file(&artifact.id);
    let written = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&header)
        .and_then(|mut file| file.write_all(darling::header_contents(&token).as_bytes()));
    if let Err(error) = written {
        let _ = fs::remove_file(&header);
        return Err(Failure::new(Code::StorageUnusable, error.to_string(), EXIT_REFUSED));
    }
    let mut executor = executor(&context.dirs, log, Forward::LogOnly);
    let fetched = carry_out(&download, &mut executor, Code::MacosInstallFailed, reports);
    let _ = fs::remove_file(&header);
    fetched?;
    let archive = store.download(&artifact.id);
    let (digest, size) = match darling::check_archive(&archive, artifact, None) {
        Ok(checked) => checked,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(Failure::from_mac(error, EXIT_STEP_FAILED));
        }
    };
    let record = darling::verified_record(artifact, digest, size, Source::Registry, Timestamp::now());
    let unpack = darling::plan_unpack(store, artifact, &archive, &tar, &record)
        .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
    carry_out(&unpack, &mut executor, Code::MacosInstallFailed, reports)?;
    Ok(artifact.id.clone())
}

fn run_fetch(from_file: Option<&Path>, sha256: Option<&str>, common: &MacCommon) -> u8 {
    let json = common.json;
    // The fetch is the fallback for systems without Darling in their image.
    if let Some(system) = darling::system_build()
        && from_file.is_none()
    {
        if json {
            print_json(&json!({ "dry_run": common.dry_run, "system": system, "plans": [] }));
        } else {
            print_line(&format!(
                "Darling {} is part of this system ({}); nothing is fetched",
                system.id, system.root
            ));
        }
        return EXIT_OK;
    }
    let result = Context::open().and_then(|context| {
        let artifact = context.artifact()?.clone();
        check_release(&artifact, common.any_system)?;
        let mut reports = Vec::new();
        let id = ensure_runtime(&context, from_file, sha256, common.dry_run, &mut reports)?;
        Ok((context, id, reports))
    });
    match result {
        Ok((context, id, reports)) => {
            if json {
                print_json(&json!({
                    "dry_run": common.dry_run,
                    "build": id,
                    "plans": reports,
                    "install_root": context.store.install_root(&id).display().to_string(),
                }));
            } else if common.dry_run {
                for report in &reports {
                    print_line(&serde_json::to_string_pretty(report).unwrap_or_default());
                }
            } else {
                print_line(&format!(
                    "Darling {id} is ready at {}",
                    context.store.install_root(&id).display()
                ));
            }
            EXIT_OK
        }
        Err(failure) => failure.report(json),
    }
}

fn run_remove_runtime(common: &MacCommon) -> u8 {
    let json = common.json;
    let context = match Context::open() {
        Ok(context) => context,
        Err(failure) => return failure.report(json),
    };
    let ids = darling::kept(&context.store);
    let plan = darling::plan_remove(&context.store, &ids);
    let (records, _) = record::installed(&context.dirs);
    let affected: Vec<&str> = records.iter().map(|record| record.app_id.as_str()).collect();
    if common.dry_run {
        if json {
            print_json(&json!({ "dry_run": true, "plan": plan, "affected": affected }));
        } else {
            print_line(plan.render_text().trim_end());
        }
        return EXIT_OK;
    }
    let log = context.dirs.state_root().join("logs").join("darling").join("operations.log");
    let mut executor = executor(&context.dirs, log, Forward::LogOnly);
    let report = run_plan(&plan, &mut executor);
    if json {
        print_json(&json!({ "dry_run": false, "plan": plan, "report": report, "affected": affected }));
    } else if !affected.is_empty() {
        print_line(&format!(
            "{} installed macOS applications need Darling fetched again to start: {}",
            affected.len(),
            affected.join(", ")
        ));
    }
    if let Some(failure) = &report.failure {
        return fail_after_report(
            json,
            Code::StepFailed,
            &failure_message(failure, executor.last_output()),
            EXIT_STEP_FAILED,
        );
    }
    if !report.findings.is_empty() {
        return fail_after_report(json, Code::StepFailed, "a build was still there after removal", EXIT_FINDINGS);
    }
    EXIT_OK
}

/// What `mac install` is asked to do.
struct Install<'a> {
    package: &'a Path,
    app_id: Option<&'a str>,
    name: Option<&'a str>,
    bundle: Option<&'a str>,
    allow_swift: bool,
    network: bool,
    mode: &'a str,
    layout: Option<&'a str>,
    timeout: u64,
    fetch: bool,
    keep_failed: bool,
    launcher: &'a str,
    common: &'a MacCommon,
}

/// The session of a command that runs something in an application's
/// sandbox.
struct Session {
    context: Context,
    host: LaunchHost,
    grants: GrantContext,
}

impl Session {
    fn open(common: &MacCommon) -> Result<Self, Failure> {
        let context = Context::open()?;
        let mut host = LaunchHost::detect().ok_or_else(|| {
            Failure::new(
                Code::StorageUnusable,
                "cannot determine the user's home directory",
                EXIT_REFUSED,
            )
        })?;
        if host.euid == 0 {
            return Err(Failure::new(
                Code::RunningAsRoot,
                "macOS software is never run as root",
                EXIT_REFUSED,
            ));
        }
        if let Some(program) = &common.sandbox_program {
            host.bwrap = Some(program.clone());
        }
        let grants = GrantContext::new(
            &host.home,
            &context.dirs,
            host.runtime_dir.as_deref(),
            host.agent_socket.as_deref(),
        );
        Ok(Session {
            context,
            host,
            grants,
        })
    }

    fn app(&self, app_id: &str) -> Result<AppPaths, Failure> {
        self.context.dirs.app(app_id).map_err(|error| {
            Failure::new(Code::MacosRequestInvalid, error.to_string(), EXIT_USAGE)
        })
    }
}

/// The identifier an application gets when none is given: its bundle
/// identifier in lower case when that is a valid one, otherwise `local.`
/// and the file's name.
fn default_app_id(dirs: &RuntimeDirs, identifier: Option<&str>, file: &Path) -> String {
    if let Some(identifier) = identifier {
        let lower = identifier.to_ascii_lowercase();
        if dirs.app(&lower).is_ok() {
            return lower;
        }
    }
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("application");
    format!("local.{}", default_prefix_id(stem))
}

fn recorded(analysis: &AppAnalysis) -> Vec<RecordedFinding> {
    analysis
        .findings
        .iter()
        .take(64)
        .map(|finding| RecordedFinding {
            reason: finding.reason.to_owned(),
            severity: serde_json::to_value(finding.severity)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
            code: finding.code.map(|code| code.as_str().to_owned()),
        })
        .collect()
}

fn refusal_of(analysis: &AppAnalysis) -> Option<Failure> {
    analysis.refusal().map(|finding| {
        Failure::new(
            finding.code.unwrap_or(Code::MacosNoRunnableSlice),
            format!("{}: {}", analysis.bundle, finding.message),
            EXIT_UNSUPPORTED,
        )
    })
}

fn run_install(request: &Install<'_>) -> u8 {
    let json = request.common.json;
    let mut reports = Vec::new();
    match install(request, &mut reports) {
        Ok(record) => {
            if json {
                print_json(&json!({
                    "dry_run": request.common.dry_run,
                    "installed": record,
                    "plans": reports,
                }));
            } else if let Some(record) = record {
                print_line(&format!(
                    "installed: {} ({}) in Darling, as {}",
                    record.name, record.bundle, record.app_id
                ));
                print_line(&format!("start it with: tur-runtime mac launch {}", record.app_id));
            } else {
                for report in &reports {
                    print_line(&serde_json::to_string_pretty(report).unwrap_or_default());
                }
            }
            EXIT_OK
        }
        Err(failure) => {
            if json && !reports.is_empty() {
                print_json(&json!({
                    "dry_run": request.common.dry_run,
                    "plans": reports,
                    "error": {
                        "code": failure.code.as_str(),
                        "title": failure.code.title(),
                        "message": failure.message,
                    }
                }));
                return failure.status;
            }
            failure.report(json)
        }
    }
}

/// Installs, or with `--dry-run` plans the first steps. `Ok(None)` for a dry
/// run.
fn install(request: &Install<'_>, reports: &mut Vec<Value>) -> Result<Option<MacAppRecord>, Failure> {
    let common = request.common;
    let Some(mode) = DarlingMode::from_name(request.mode) else {
        return Err(Failure::new(
            Code::MacosRequestInvalid,
            "--darling-mode takes nonroot or rootless",
            EXIT_USAGE,
        ));
    };
    let layout = match (request.layout, mode) {
        (None, DarlingMode::Nonroot) => PrefixLayout::Layered,
        (None, DarlingMode::Rootless) => PrefixLayout::Darling,
        (Some(name), _) => PrefixLayout::from_name(name).ok_or_else(|| {
            Failure::new(
                Code::MacosRequestInvalid,
                "--prefix-layout takes layered or darling",
                EXIT_USAGE,
            )
        })?,
    };
    if layout == PrefixLayout::Layered && mode == DarlingMode::Rootless {
        return Err(Failure::new(
            Code::MacosRequestInvalid,
            "a layered prefix is for the nonroot mode; the rootless mode layers the prefix itself",
            EXIT_USAGE,
        ));
    }
    if request.timeout == 0 || request.timeout > tur_runtime::limits::MAX_INSTALLER_SECONDS {
        return Err(Failure::new(
            Code::MacosRequestInvalid,
            "--timeout must be between 1 second and a day",
            EXIT_USAGE,
        ));
    }
    let package = fs::canonicalize(request.package).map_err(|error| {
        Failure::new(
            Code::Unreadable,
            format!("cannot resolve {}: {error}", request.package.display()),
            EXIT_INPUT,
        )
    })?;
    let kind = package::identify(&package)
        .map_err(|error| Failure::new(error.code(), error.to_string(), EXIT_INPUT))?;
    match kind {
        PackageKind::NotMacSoftware => {
            return Err(Failure::new(
                Code::MacosNotMacSoftware,
                format!(
                    "{} is not a macOS application, disk image, installer package or archive",
                    package.display()
                ),
                EXIT_UNSUPPORTED,
            ));
        }
        PackageKind::Program => {
            return Err(Failure::new(
                Code::MacosNotMacSoftware,
                "a program on its own is not an application; install the application bundle, \
                 disk image or package it came in",
                EXIT_UNSUPPORTED,
            ));
        }
        _ => {}
    }
    let options = AnalysisOptions {
        host: this_arch(),
        allow_swift: request.allow_swift,
    };
    // A bundle is analysed before anything is made.
    let early = if kind == PackageKind::AppBundle {
        let analysis = analyse_bundle(&package, &options)
            .map_err(|error| Failure::new(error.code(), error.to_string(), EXIT_INPUT))?;
        if let Some(failure) = refusal_of(&analysis) {
            return Err(failure);
        }
        Some(analysis)
    } else {
        None
    };

    let session = Session::open(common)?;
    let app_id = match request.app_id {
        Some(id) => id.to_owned(),
        None => default_app_id(
            &session.context.dirs,
            early.as_ref().and_then(|analysis| analysis.identifier.as_deref()),
            &package,
        ),
    };
    let app = session.app(&app_id)?;
    if fs::symlink_metadata(&app.app_dir).is_ok() {
        return Err(Failure::new(
            Code::MacosAppExists,
            format!("{app_id} is already installed; nothing is installed over it"),
            EXIT_REFUSED,
        ));
    }
    let seven_zip = if kind.is_unpacked_by_7zip() {
        Some(program(&["7zz", "7z", "7za"], "7-Zip")?)
    } else {
        None
    };
    let cp = program(&["cp"], "cp")?;
    let find = program(&["find"], "find")?;

    // Darling: the image's own, or on a system without it, TUR's build
    // fetched now (the fallback).
    let build = resolve_build(&session.context, common, request.fetch, reports)?;
    let gid = effective_gid().ok_or_else(|| {
        Failure::new(
            Code::StorageUnusable,
            "cannot read this process's group ID",
            EXIT_REFUSED,
        )
    })?;
    let setup = Setup {
        host: &session.host,
        app: &app,
        context: &session.grants,
        tree: &build.tree,
        mode,
        layout,
        gid,
        network: request.network,
    };

    let prepare = mac_plan::plan_prepare(&setup, &package, seven_zip.as_deref())
        .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
    if common.dry_run {
        let initialise = mac_plan::plan_initialise(&setup)
            .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
        reports.push(json!({ "plan": prepare }));
        reports.push(json!({
            "plan": initialise,
            "then": "the application is put into the prefix (copied, or installed by Darling's \
                     installer), analysed, and recorded with its launcher entry; what those \
                     steps are depends on what the package holds",
        }));
        return Ok(None);
    }

    let mut executor = executor(&session.context.dirs, app.operations_log(), Forward::LogOnly);
    let installed = put_in_place(&setup, request, kind, &package, &options, seven_zip.as_deref(), &cp, &find, &mut executor, reports)
        .and_then(|(analysis, bundle, method)| {
            let record = make_record(request, &app, &build, mode, layout, &package, kind, &analysis, &bundle, method)?;
            let entry = mac_plan::desktop_entry(&record, request.launcher)
                .map_err(|error| Failure::from_mac(error, EXIT_USAGE))?;
            let plan = mac_plan::plan_record(&setup, &record, &entry)
                .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
            carry_out(&plan, &mut executor, Code::MacosInstallFailed, reports)?;
            Ok(record)
        });
    match installed {
        Ok(record) => Ok(Some(record)),
        Err(failure) => {
            if !request.keep_failed {
                let rollback = mac_plan::plan_rollback(&app);
                let report = run_plan(&rollback, &mut executor);
                reports.push(json!({ "plan": rollback, "report": report }));
            }
            Err(failure)
        }
    }
}

/// Puts the application into the Darling prefix. Returns its analysis, the
/// bundle's name in `/Applications`, and how it got there.
#[allow(clippy::too_many_arguments)]
fn put_in_place(
    setup: &Setup<'_>,
    request: &Install<'_>,
    kind: PackageKind,
    package: &Path,
    options: &AnalysisOptions,
    seven_zip: Option<&Path>,
    cp: &Path,
    find: &Path,
    executor: &mut SystemExecutor,
    reports: &mut Vec<Value>,
) -> Result<(AppAnalysis, String, &'static str), Failure> {
    let prepare = mac_plan::plan_prepare(setup, package, seven_zip)
        .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
    carry_out(&prepare, executor, Code::MacosInstallFailed, reports)?;

    // What to put in: a bundle, or an installer package.
    let (bundle_source, pkg, shown): (Option<PathBuf>, Option<PathBuf>, bool) = match kind {
        PackageKind::AppBundle => (Some(package.to_path_buf()), None, true),
        PackageKind::InstallerPackage => (None, Some(package.to_path_buf()), true),
        _ => {
            let Some(seven_zip) = seven_zip else {
                return Err(Failure::new(Code::MacosToolMissing, "7-Zip is not installed", EXIT_MISSING));
            };
            let mut found = package::search(&setup.unpacked());
            // A disk image's volume may come out as an image of its own.
            for _ in 0..2 {
                if found.partitions.is_empty() {
                    break;
                }
                let plan = mac_plan::plan_unpack_partitions(setup, &found.partitions, seven_zip)
                    .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
                carry_out(&plan, executor, Code::MacosInstallFailed, reports)?;
                found = package::search(&setup.unpacked());
            }
            if found.bundles.is_empty() {
                match found.packages.first() {
                    Some(inner) => (None, Some(inner.clone()), false),
                    None => {
                        return Err(Failure::new(
                            Code::MacosNoApplicationFound,
                            "the package holds no application bundle and no installer package",
                            EXIT_UNSUPPORTED,
                        ));
                    }
                }
            } else {
                let chosen = package::choose(&found, request.bundle).map_err(|message| {
                    Failure::new(Code::MacosNoApplicationFound, message, EXIT_UNSUPPORTED)
                })?;
                (Some(chosen.path.clone()), None, false)
            }
        }
    };

    if let Some(source) = bundle_source {
        let analysis = analyse_bundle(&source, options)
            .map_err(|error| Failure::new(error.code(), error.to_string(), EXIT_INPUT))?;
        if let Some(failure) = refusal_of(&analysis) {
            return Err(failure);
        }
        let initialise = mac_plan::plan_initialise(setup)
            .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
        carry_out(&initialise, executor, Code::MacosInstallFailed, reports)?;
        let (copy, name) = mac_plan::plan_copy_bundle(setup, &source, shown, cp, find)
            .map_err(|error| Failure::from_mac(error, EXIT_UNSUPPORTED))?;
        carry_out(&copy, executor, Code::MacosInstallFailed, reports)?;
        let method = if kind == PackageKind::AppBundle {
            "copied-bundle"
        } else {
            "unpacked"
        };
        return Ok((analysis, name, method));
    }

    let Some(pkg) = pkg else {
        return Err(Failure::new(
            Code::MacosNoApplicationFound,
            "nothing to install was found",
            EXIT_UNSUPPORTED,
        ));
    };
    let initialise = mac_plan::plan_initialise(setup)
        .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
    carry_out(&initialise, executor, Code::MacosInstallFailed, reports)?;
    let before: Vec<PathBuf> = package::search(&setup.applications())
        .bundles
        .into_iter()
        .map(|bundle| bundle.path)
        .collect();
    let installer = mac_plan::plan_installer_package(setup, &pkg, shown, request.timeout)
        .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?;
    carry_out(&installer, executor, Code::MacosInstallFailed, reports)?;
    let mut after = package::search(&setup.applications());
    after.bundles.retain(|bundle| !before.contains(&bundle.path));
    let chosen = package::choose(&after, request.bundle)
        .map_err(|message| Failure::new(Code::MacosNoApplicationFound, message, EXIT_UNSUPPORTED))?;
    let analysis = analyse_bundle(&chosen.path, options)
        .map_err(|error| Failure::new(error.code(), error.to_string(), EXIT_INPUT))?;
    if let Some(failure) = refusal_of(&analysis) {
        return Err(failure);
    }
    let name = chosen
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();
    Ok((analysis, name, "installer-package"))
}

/// The Darling an application is installed or started with.
struct Build {
    /// The build's identifier.
    id: String,
    /// Darling's commit.
    commit: String,
    /// Where it is on this system: what is shown at /opt/tur-darling.
    tree: PathBuf,
}

/// The image's Darling, or on a system without Darling in its image, TUR's
/// build fetched on demand (the fallback; `fetch` false refuses instead).
fn resolve_build(
    context: &Context,
    common: &MacCommon,
    fetch: bool,
    reports: &mut Vec<Value>,
) -> Result<Build, Failure> {
    if let Some(system) = darling::system_build() {
        return Ok(Build {
            id: system.id,
            commit: system.darling_commit,
            tree: PathBuf::from(system.root),
        });
    }
    let artifact = context.artifact()?.clone();
    check_release(&artifact, common.any_system)?;
    if !darling::is_ready(&context.store, &artifact.id) && !fetch {
        return Err(Failure::new(
            Code::MacosRuntimeNotInstalled,
            "Darling is not part of this system and has not been fetched; fetch it with \
             `tur-runtime mac runtime fetch`",
            EXIT_MISSING,
        ));
    }
    let id = ensure_runtime(context, None, None, common.dry_run, reports)?;
    Ok(Build {
        tree: context.store.install_root(&id),
        id,
        commit: artifact.darling_commit,
    })
}

/// This process's effective group ID, from `/proc/self/status`.
fn effective_gid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find_map(|line| line.strip_prefix("Gid:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

#[allow(clippy::too_many_arguments)]
fn make_record(
    request: &Install<'_>,
    app: &AppPaths,
    build: &Build,
    mode: DarlingMode,
    layout: PrefixLayout,
    package: &Path,
    kind: PackageKind,
    analysis: &AppAnalysis,
    bundle: &str,
    method: &str,
) -> Result<MacAppRecord, Failure> {
    let executable = analysis.executable.clone().ok_or_else(|| {
        Failure::new(
            Code::MacosNoRunnableSlice,
            "the bundle has no main program",
            EXIT_UNSUPPORTED,
        )
    })?;
    let slice = analysis
        .program
        .as_ref()
        .and_then(|program| program.slice)
        .map_or_else(|| this_arch().as_str().to_owned(), |arch| arch.as_str().to_owned());
    let (size, sha256) = if kind == PackageKind::AppBundle {
        (0, None)
    } else {
        let (digest, size) = sha256_file(package).map_err(|error| {
            Failure::new(Code::Unreadable, error.to_string(), EXIT_INPUT)
        })?;
        (size, Some(digest))
    };
    let kind_name = serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let name = request
        .name
        .map(str::to_owned)
        .or_else(|| analysis.name.clone())
        .unwrap_or_else(|| bundle.trim_end_matches(".app").to_owned());
    Ok(MacAppRecord {
        schema_version: RECORD_SCHEMA_VERSION,
        app_id: app.app_id.clone(),
        name,
        version: analysis.version.clone(),
        bundle_identifier: analysis.identifier.clone(),
        bundle: bundle.to_owned(),
        executable,
        slice,
        runtime: "darling".to_owned(),
        darling: DarlingRecord {
            artifact: build.id.clone(),
            commit: build.commit.clone(),
            mode,
            layout,
        },
        network: request.network,
        allowed_swift: request.allow_swift,
        installed_at: Timestamp::now().rfc3339(),
        source: SourceRecord {
            kind: kind_name,
            name: package
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("package")
                .to_owned(),
            size,
            sha256,
        },
        method: method.to_owned(),
        findings: recorded(analysis),
        launcher: LauncherRecord {
            desktop_entry: mac_plan::desktop_entry_path(app).display().to_string(),
            installed: false,
        },
    })
}

fn run_list(json: bool) -> u8 {
    let dirs = match RuntimeDirs::from_env(|name| std::env::var(name).ok()) {
        Ok(dirs) => dirs,
        Err(error) => return fail(json, Code::StorageUnusable, &error.to_string(), EXIT_REFUSED),
    };
    let (records, problems) = record::installed(&dirs);
    if json {
        print_json(&json!({ "applications": records, "problems": problems }));
    } else {
        for record in &records {
            print_line(&format!(
                "{}  {} {}  ({})",
                record.app_id,
                record.name,
                record.version.as_deref().unwrap_or(""),
                record.bundle
            ));
        }
        for problem in &problems {
            print_error_line(&format!("warning[{}]: {problem}", Code::MacosRecordInvalid));
        }
    }
    EXIT_OK
}

fn load_installed(dirs: &RuntimeDirs, app_id: &str) -> Result<(AppPaths, MacAppRecord), Failure> {
    let app = dirs
        .app(app_id)
        .map_err(|error| Failure::new(Code::MacosRequestInvalid, error.to_string(), EXIT_USAGE))?;
    match record::load(&app) {
        Ok(Some(record)) => Ok((app, record)),
        Ok(None) => Err(Failure::new(
            Code::MacosAppNotInstalled,
            format!("no macOS application {app_id} is installed"),
            EXIT_REFUSED,
        )),
        Err(error) => Err(Failure::from_mac(error, EXIT_INPUT)),
    }
}

fn run_show(app_id: &str, json: bool) -> u8 {
    let dirs = match RuntimeDirs::from_env(|name| std::env::var(name).ok()) {
        Ok(dirs) => dirs,
        Err(error) => return fail(json, Code::StorageUnusable, &error.to_string(), EXIT_REFUSED),
    };
    match load_installed(&dirs, app_id) {
        Ok((_, record)) => {
            if json {
                print_json(&json!(record));
            } else {
                print_line(&serde_json::to_string_pretty(&record).unwrap_or_default());
            }
            EXIT_OK
        }
        Err(failure) => failure.report(json),
    }
}

fn run_launch(app_id: &str, environment: &[String], common: &MacCommon, arguments: &[String]) -> u8 {
    let json = common.json;
    let extra = match parse_environment(environment) {
        Ok(extra) => extra,
        Err(message) => return fail(json, Code::EnvironmentRefused, &message, EXIT_USAGE),
    };
    let planned = Session::open(common).and_then(|session| {
        let (app, record) = load_installed(&session.context.dirs, app_id)?;
        // The image's Darling, whichever build the application was installed
        // with: it is kept updated with the image, and a layered prefix
        // holds only the application's own layer over it. Without it, the
        // fetched build the application was installed with (the fallback).
        let tree = match darling::system_build() {
            Some(system) => PathBuf::from(system.root),
            None => {
                let artifact = session.context.artifact()?.clone();
                check_release(&artifact, common.any_system)?;
                if !darling::is_ready(&session.context.store, &record.darling.artifact) {
                    return Err(Failure::new(
                        Code::MacosRuntimeNotInstalled,
                        format!(
                            "Darling is not part of this system, and the build {app_id} was \
                             installed with ({}) is not here; fetch it with \
                             `tur-runtime mac runtime fetch`",
                            record.darling.artifact
                        ),
                        EXIT_MISSING,
                    ));
                }
                session.context.store.install_root(&record.darling.artifact)
            }
        };
        let gid = effective_gid().ok_or_else(|| {
            Failure::new(
                Code::StorageUnusable,
                "cannot read this process's group ID",
                EXIT_REFUSED,
            )
        })?;
        let log = app.logs.join(format!(
            "{}-{}.log",
            Timestamp::now().compact(),
            std::process::id()
        ));
        let plan = {
            let setup = Setup {
                host: &session.host,
                app: &app,
                context: &session.grants,
                tree: &tree,
                mode: record.darling.mode,
                layout: record.darling.layout,
                gid,
                network: record.network,
            };
            mac_plan::plan_launch(&setup, &record, arguments, &extra, log)
                .map_err(|error| Failure::from_mac(error, EXIT_REFUSED))?
        };
        Ok((session, app, plan))
    });
    let (session, app, plan) = match planned {
        Ok(planned) => planned,
        Err(failure) => return failure.report(json),
    };
    if common.dry_run {
        if json {
            print_json(&json!({ "dry_run": true, "plan": plan }));
        } else {
            print_line(plan.render_text().trim_end());
        }
        return EXIT_OK;
    }
    let forward = if json { Forward::LogOnly } else { Forward::Inherit };
    let mut executor = executor(&session.context.dirs, app.operations_log(), forward);
    let report = run_plan(&plan, &mut executor);
    if json {
        print_json(&json!({ "dry_run": false, "plan": plan, "report": report }));
    }
    if let Some(failure) = &report.failure {
        return fail_after_report(
            json,
            Code::StepFailed,
            &failure_message(failure, executor.last_output()),
            EXIT_STEP_FAILED,
        );
    }
    match report.exit_status {
        Some(status) => u8::try_from(status).unwrap_or(EXIT_INPUT),
        None => EXIT_OK,
    }
}

fn run_uninstall(app_id: &str, common: &MacCommon) -> u8 {
    let json = common.json;
    let context = match Context::open() {
        Ok(context) => context,
        Err(failure) => return failure.report(json),
    };
    // Only a macOS application's directory is removed here: a Windows
    // application's has no macOS record and is refused.
    let (app, _) = match load_installed(&context.dirs, app_id) {
        Ok(found) => found,
        Err(failure) => return failure.report(json),
    };
    let plan = mac_plan::plan_uninstall(&app);
    if common.dry_run {
        if json {
            print_json(&json!({ "dry_run": true, "plan": plan }));
        } else {
            print_line(plan.render_text().trim_end());
        }
        return EXIT_OK;
    }
    // The removal deletes the application's logs too, so its own record of
    // what it did goes to the Darling operations log.
    let log = context.dirs.state_root().join("logs").join("darling").join("operations.log");
    let mut executor = executor(&context.dirs, log, Forward::LogOnly);
    let report = run_plan(&plan, &mut executor);
    if json {
        print_json(&json!({ "dry_run": false, "plan": plan, "report": report }));
    }
    if let Some(failure) = &report.failure {
        return fail_after_report(
            json,
            Code::StepFailed,
            &failure_message(failure, executor.last_output()),
            EXIT_STEP_FAILED,
        );
    }
    if !report.findings.is_empty() {
        let message = format!(
            "{} paths were still there after the removal",
            report.findings.len()
        );
        return fail_after_report(json, Code::MacosUninstallIncomplete, &message, EXIT_FINDINGS);
    }
    if !json {
        print_line(&format!("removed: {app_id}"));
    }
    EXIT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_identifiers() {
        let dirs = RuntimeDirs::new(Path::new("/d"), Path::new("/s"));
        assert_eq!(
            default_app_id(&dirs, Some("ch.jakob.TableTool"), Path::new("/x/Table Tool.app")),
            "ch.jakob.tabletool"
        );
        assert!(
            default_app_id(&dirs, Some("not an id"), Path::new("/x/TableTool.dmg"))
                .starts_with("local.")
        );
        assert!(default_app_id(&dirs, None, Path::new("/x/TableTool.dmg")).starts_with("local."));
    }
}
