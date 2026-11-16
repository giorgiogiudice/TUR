// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-rehost`: run the Windows or macOS edition of an application on the
//! Linux build of its own runtime.
//!
//! ```text
//! tur-rehost detect <package> [--json]
//! tur-rehost plan <package> [--app-id ID] [--codecs upstream|open-only]
//!         [--allow-builds] [--allow-unresolved MODULE]... [--json]
//! tur-rehost install <package> [same options] [--name NAME] [--json]
//! tur-rehost list [--json]
//! tur-rehost show <app-id> [--json]
//! tur-rehost launch <app-id> [--accessibility] [-- ARGUMENTS]
//! tur-rehost uninstall <app-id> [--json]
//! tur-rehost index [--json]
//! ```
//!
//! Exit status: 0 success; 1 an input could not be read or is not valid;
//! 2 the command line is wrong; 3 the package is not re-hostable (native
//! code, a family not built yet, a runtime TUR has not pinned, a module
//! with no Linux build); 4 it can be re-hosted once something is allowed
//! or installed (source builds, bubblewrap, a tool); 6 refused before
//! anything changed; 7 a step failed and was rolled back; 8 a removal left
//! something behind. `launch` exits with the application's own status.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use tur_rehost::apps::{self, LaunchRequest};
use tur_rehost::codes::{Code, EXIT_OK, Failure};
use tur_rehost::detect;
use tur_rehost::index::Index;
use tur_rehost::install::{self, Codecs, Environment, Options};
use tur_rehost::paths::{AppId, Roots};
use tur_rehost::runner::{SystemRunner, find_program};
use tur_rehost::sandbox::{Session, running_as_root};
use tur_rehost::unpack::{self, Tools};

/// Runs the Windows or macOS edition of an application on the Linux build
/// of the runtime it is built on (Electron in this version), from your own
/// package, without Wine and without a virtual machine. The installer is
/// never run.
#[derive(Parser, Debug)]
#[command(name = "tur-rehost", version)]
struct Cli {
    /// TUR's pinned index of what may be fetched, instead of the built-in
    /// one.
    #[arg(long, global = true, value_name = "FILE")]
    index: Option<PathBuf>,
    /// Keep everything under this directory instead of the XDG
    /// directories.
    #[arg(long, global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,
    /// The bubblewrap program.
    #[arg(long, global = true, value_name = "PATH")]
    sandbox_program: Option<PathBuf>,
    /// The curl program.
    #[arg(long, global = true, value_name = "PATH")]
    curl_program: Option<PathBuf>,
    /// The podman program, for source builds.
    #[arg(long, global = true, value_name = "PATH")]
    podman_program: Option<PathBuf>,
    /// The program launcher entries start.
    #[arg(long, global = true, value_name = "PATH", default_value = "/usr/bin/tur-rehost")]
    launcher: String,
    /// What to do.
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CodecChoice {
    /// Electron's Linux build unchanged.
    Upstream,
    /// Electron's open-codecs media library instead (no H.264, no AAC).
    OpenOnly,
}

#[derive(clap::Args, Debug)]
struct PlanArgs {
    /// The package: an installer, archive or disk image you have, or an
    /// unpacked application directory or `.app` bundle.
    package: PathBuf,
    /// The application identifier (lower case, dot-separated).
    #[arg(long, value_name = "ID")]
    app_id: Option<String>,
    /// Which media library Electron keeps.
    #[arg(long, value_enum, default_value = "upstream")]
    codecs: CodecChoice,
    /// Allow native modules to be built from source on this computer,
    /// without network, with Fedora's compiler.
    #[arg(long)]
    allow_builds: bool,
    /// A native module to run without (`name@version`, or `file:<name>`
    /// for one that cannot be identified): the application may fail when it
    /// loads it.
    #[arg(long, value_name = "MODULE")]
    allow_unresolved: Vec<String>,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Say which runtime a package carries; nothing is fetched or run.
    Detect {
        /// The package.
        package: PathBuf,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show what re-hosting a package would fetch and do, and do nothing.
    Plan(PlanArgs),
    /// Re-host a package: fetch the runtime, lay the payload under it,
    /// record it.
    Install {
        /// The plan's options.
        #[command(flatten)]
        plan: PlanArgs,
        /// The name shown for the application.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },
    /// List re-hosted applications.
    List {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show a re-hosted application's record.
    Show {
        /// The application identifier.
        app_id: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Start a re-hosted application in its sandbox.
    Launch {
        /// The application identifier.
        app_id: String,
        /// Start Chromium with its accessibility tree on (for P@x and
        /// screen readers).
        #[arg(long)]
        accessibility: bool,
        /// Arguments for the application.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Remove a re-hosted application and everything it kept.
    Uninstall {
        /// The application identifier.
        app_id: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show TUR's pinned index: the runtimes and modules that can be
    /// fetched.
    Index {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    ExitCode::from(run(cli))
}

fn print_line(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}

fn print_json<T: Serialize>(value: &T) {
    print_line(&serde_json::to_string_pretty(value).unwrap_or_default());
}

fn report(json: bool, failure: &Failure) -> u8 {
    if json {
        print_line(&serde_json::json!({ "error": failure }).to_string());
    } else {
        let _ = writeln!(std::io::stderr(), "{failure}");
    }
    failure.code.exit_status()
}

fn roots(cli: &Cli) -> Result<Roots, Failure> {
    match &cli.data_dir {
        Some(directory) if directory.is_absolute() => Ok(Roots::under(directory)),
        Some(_) => Err(Failure::new(Code::InputInvalid, "--data-dir must be an absolute path")),
        None => Roots::from_environment(),
    }
}

fn index(cli: &Cli) -> Result<Index, Failure> {
    match &cli.index {
        Some(path) => Index::read(path),
        None => Index::built_in(),
    }
}

fn options(args: &PlanArgs, name: Option<String>, launcher: &str) -> Result<Options, Failure> {
    let app_id = args.app_id.as_deref().map(AppId::parse).transpose()?;
    if let Some(name) = &name
        && (name.is_empty() || name.len() > 200 || name.chars().any(char::is_control))
    {
        return Err(Failure::new(Code::InputInvalid, "the name must be 1 to 200 characters without control characters"));
    }
    if args.allow_unresolved.len() > 64 {
        return Err(Failure::new(Code::InputInvalid, "at most 64 modules can be allowed unresolved"));
    }
    Ok(Options {
        app_id,
        name,
        codecs: match args.codecs {
            CodecChoice::Upstream => Codecs::Upstream,
            CodecChoice::OpenOnly => Codecs::OpenOnly,
        },
        allow_builds: args.allow_builds,
        allow_unresolved: args.allow_unresolved.clone(),
        launcher: launcher.to_owned(),
    })
}

fn run(cli: Cli) -> u8 {
    match &cli.command {
        Command::Detect { package, json } => detect_command(&cli, package, *json),
        Command::Plan(args) => plan_command(&cli, args, None, false),
        Command::Install { plan, name } => plan_command(&cli, plan, name.clone(), true),
        Command::List { json } => {
            let roots = match roots(&cli) {
                Ok(roots) => roots,
                Err(failure) => return report(*json, &failure),
            };
            let listed = apps::list(&roots);
            if *json {
                print_json(&serde_json::json!({ "applications": listed }));
            } else if listed.is_empty() {
                print_line("No application is re-hosted.");
            } else {
                for item in &listed {
                    print_line(&format!(
                        "{}  {} {}  ({} package, {})",
                        item.app_id,
                        item.name,
                        item.app_version.clone().unwrap_or_default(),
                        item.platform,
                        item.runtime
                    ));
                }
            }
            EXIT_OK
        }
        Command::Show { app_id, json } => {
            let result = AppId::parse(app_id).and_then(|id| apps::show(&roots(&cli)?, &id));
            match result {
                Ok(record) => {
                    if *json {
                        print_json(&record);
                    } else {
                        print_line(&format!("{} ({})", record.name, record.app_id));
                        print_line(&format!(
                            "  {} package {}, running on Linux's own build of {} {}",
                            record.package.platform, record.package.file_name, record.runtime.family, record.runtime.version
                        ));
                        print_line("  Not the publisher's Linux version; the publisher does not support this.");
                        for module in &record.modules {
                            print_line(&format!("  module {}: {}", module.module, module.resolution));
                        }
                    }
                    EXIT_OK
                }
                Err(failure) => report(*json, &failure),
            }
        }
        Command::Launch { app_id, accessibility, args } => {
            match launch_command(&cli, app_id, *accessibility, args) {
                Ok(launched) => u8::try_from(launched.exit_status.clamp(0, 255)).unwrap_or(1),
                Err(failure) => report(false, &failure),
            }
        }
        Command::Uninstall { app_id, json } => {
            let result = AppId::parse(app_id).and_then(|id| apps::uninstall(&roots(&cli)?, &id));
            match result {
                Ok(removed) => {
                    if *json {
                        print_json(&removed);
                    } else {
                        print_line(&format!("{} removed: {}", removed.app_id, removed.deleted.join(", ")));
                    }
                    EXIT_OK
                }
                Err(failure) => report(*json, &failure),
            }
        }
        Command::Index { json } => match index(&cli) {
            Ok(index) => {
                if *json {
                    print_json(&index);
                } else {
                    print_line(&format!("TUR's pinned index, checked {}", index.checked));
                    for (version, release) in &index.electron {
                        print_line(&format!(
                            "  Electron {version} (ABI {}): {} bytes from {}",
                            release.abi, release.linux_x64.size, release.linux_x64.url
                        ));
                    }
                    print_line(&format!("  {} native modules and helper packages", index.modules.len()));
                }
                EXIT_OK
            }
            Err(failure) => report(*json, &failure),
        },
    }
}

fn launch_command(
    cli: &Cli,
    app_id: &str,
    accessibility: bool,
    args: &[String],
) -> Result<apps::Launched, Failure> {
    let roots = roots(cli)?;
    let id = AppId::parse(app_id)?;
    let bwrap = cli
        .sandbox_program
        .clone()
        .or_else(|| find_program(&["bwrap"]))
        .ok_or_else(|| {
            Failure::new(
                Code::SandboxMissing,
                "bubblewrap (bwrap) is not installed: nothing is started without the sandbox",
            )
        })?;
    let session = Session::from_environment();
    apps::launch(&LaunchRequest {
        roots: &roots,
        id: &id,
        bwrap: &bwrap,
        accessibility,
        args,
        session: &session,
    })
}

fn open_and_detect(
    cli: &Cli,
    package: &std::path::Path,
) -> Result<(unpack::Opened, Vec<tur_rehost::Finding>, PathBuf), Failure> {
    if running_as_root() {
        return Err(Failure::new(Code::RunningAsRoot, "tur-rehost does not open packages as root"));
    }
    let roots = roots(cli)?;
    let tools = if package.is_dir() {
        Tools::find_lenient(cli.sandbox_program.clone())
    } else {
        Tools::find(cli.sandbox_program.clone())?
    };
    let work = roots.work("detect");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)
        .map_err(|error| Failure::new(Code::StepFailed, format!("cannot create {}: {error}", work.display())))?;
    let mut runner = SystemRunner;
    let opened = match unpack::open(package, &work, &tools, &mut runner) {
        Ok(opened) => opened,
        Err(failure) => {
            let _ = std::fs::remove_dir_all(&work);
            return Err(failure);
        }
    };
    let findings = detect::detect(&opened.root);
    Ok((opened, findings, work))
}

fn detect_command(cli: &Cli, package: &std::path::Path, json: bool) -> u8 {
    let (opened, findings, work) = match open_and_detect(cli, package) {
        Ok(found) => found,
        Err(failure) => return report(json, &failure),
    };
    let status = match findings.first() {
        None => Code::NothingRecognised.exit_status(),
        Some(finding) if finding.verdict == tur_rehost::Verdict::NotRehostable => Code::NativeProgram.exit_status(),
        Some(_) => EXIT_OK,
    };
    if json {
        print_json(&serde_json::json!({
            "package": package.display().to_string(),
            "container": opened.container,
            "layers": opened.layers,
            "problems": opened.problems,
            "findings": findings,
        }));
    } else {
        print_line(&format!(
            "{}: {}",
            package.display(),
            if opened.layers.is_empty() { opened.container.clone() } else { opened.layers.join(" > ") }
        ));
        match findings.first() {
            Some(finding) => {
                print_line(&format!("  {}", finding.summary()));
                for note in &finding.notes {
                    print_line(&format!("  - {note}"));
                }
            }
            None => print_line("  nothing recognised"),
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    status
}

fn setup(
    cli: &Cli,
    args: &PlanArgs,
    name: Option<String>,
) -> Result<(Roots, Index, Tools, Options), Failure> {
    if running_as_root() {
        return Err(Failure::new(Code::RunningAsRoot, "tur-rehost does not install applications as root"));
    }
    let tools = if args.package.is_dir() {
        Tools::find_lenient(cli.sandbox_program.clone())
    } else {
        Tools::find(cli.sandbox_program.clone())?
    };
    Ok((roots(cli)?, index(cli)?, tools, options(args, name, &cli.launcher)?))
}

fn plan_command(cli: &Cli, args: &PlanArgs, name: Option<String>, install_it: bool) -> u8 {
    let json = args.json;
    let (roots, index, tools, options) = match setup(cli, args, name) {
        Ok(setup) => setup,
        Err(failure) => return report(json, &failure),
    };
    let curl = cli
        .curl_program
        .clone()
        .or_else(|| find_program(&["curl"]))
        .unwrap_or_else(|| PathBuf::from("/usr/bin/curl"));
    let podman = cli.podman_program.clone().or_else(|| find_program(&["podman"]));
    let mut runner = SystemRunner;
    let mut environment = Environment {
        roots: &roots,
        index: &index,
        tools: &tools,
        curl,
        podman,
        runner: &mut runner,
    };
    let prepared = match install::prepare(&args.package, &options, &mut environment) {
        Ok(prepared) => prepared,
        Err(failure) => return report(json, &failure),
    };
    if !install_it || !prepared.plan.ready {
        let status = prepared.plan.exit_status();
        print_plan(&prepared.plan, json);
        prepared.discard();
        return status;
    }
    match install::install(prepared, &options, &mut environment) {
        Ok(report) => {
            if json {
                print_json(&report);
            } else {
                print_plan(&report.plan, false);
                print_line(&format!("Set up {} in {}", report.record.app_id, report.directory));
                print_line(&format!("Launcher entry (staged): {}", report.record.desktop_entry));
            }
            EXIT_OK
        }
        Err(failure) => report(json, &failure),
    }
}

fn print_plan(plan: &install::Plan, json: bool) {
    if json {
        print_json(plan);
        return;
    }
    print_line(&format!(
        "{} ({}): {}",
        plan.package,
        plan.app_id,
        if plan.layers.is_empty() { plan.container.clone() } else { plan.layers.join(" > ") }
    ));
    if let Some(finding) = &plan.finding {
        print_line(&format!("  {}", finding.summary()));
    }
    if let Some(runtime) = &plan.runtime {
        print_line(&format!("  Runs on Linux's own build of Electron {} (not the publisher's Linux version).", runtime.version));
        print_line(&format!("  {}", runtime.notice));
    }
    for module in &plan.modules {
        let name = module.identity.clone().or_else(|| module.files.first().cloned()).unwrap_or_default();
        let mark = if module.accepted { " (accepted: runs without it)" } else { "" };
        print_line(&format!("  module {name}: {}{mark}", module.detail));
    }
    if !plan.downloads.is_empty() {
        print_line(&format!("  To download ({} bytes in all), each checked against TUR's pinned SHA-256:", plan.download_bytes));
        for download in &plan.downloads {
            print_line(&format!("    {} ({} bytes) from {}", download.what, download.size, download.url));
        }
    }
    if plan.needs_build_environment {
        print_line("  Native modules will be built on this computer, without network; Fedora's build tools are downloaded once from Fedora.");
    }
    for blocker in &plan.blockers {
        print_line(&format!("  ! {blocker}"));
    }
    if plan.ready {
        print_line("  Nothing of the package leaves this computer.");
    }
}
