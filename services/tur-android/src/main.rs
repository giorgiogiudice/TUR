// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-android`: TUR's Android runtime, first slice (ADR-0006;
//! docs/architecture/android-runtime.md).
//!
//! Android applications run in Waydroid, a complete Android system in a
//! container on the host kernel, from the Fedora package in the system
//! image. Android is off until the person switches it on; switching on
//! explains what is downloaded and what it costs, asks for consent and for
//! administrator authentication, and fetches the pinned Android images.
//! Applications are installed from APK files the person has, start as
//! windows on the desktop and appear in the launcher. Switching off leaves
//! nothing of Android running, mounted or connected.
//!
//! ```text
//! tur-android status | explain | enable | disable
//! tur-android install APK | list | launch ID | remove ID | codes
//! ```
//!
//! Every command takes `--json`. Exit status: 0 done; 1 failed, with a
//! `TUR-ANDROID-*` code; 2 usage error; 3 refused before anything was
//! changed, with a code.

mod apk;
mod axml;
mod codes;
mod host;
mod outcome;
mod pins;
mod run;
mod system;
mod user;

use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::json;

use crate::codes::Code;
use crate::outcome::{Done, Failure};
use crate::pins::PINS_PATH;

/// The command line.
#[derive(Debug, Parser)]
#[command(
    name = "tur-android",
    version,
    about = "TUR's Android runtime: switch Android on and off, and install, list, start and \
             remove Android applications."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show whether Android is switched on and what of it runs, is mounted and is connected
    Status(JsonArgs),
    /// Explain what switching Android on downloads and costs, without doing it
    Explain(PinsArgs),
    /// Switch Android on: download the pinned Android images and set up the container
    Enable(EnableArgs),
    /// Switch Android off: stop everything of Android and leave nothing running or mounted
    Disable(DisableArgs),
    /// Install an Android application from an APK file
    Install(InstallArgs),
    /// List the Android applications in the launcher
    List(ListArgs),
    /// Start an Android application as a window on the desktop
    Launch(AppArgs),
    /// Remove an Android application and its data
    Remove(AppArgs),
    /// List the TUR-ANDROID error codes
    Codes(JsonArgs),
}

#[derive(Debug, Args)]
struct JsonArgs {
    /// Print JSON instead of text
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct PinsArgs {
    /// Print JSON instead of text
    #[arg(long)]
    json: bool,
    /// The image pins to use (for tests)
    #[arg(long, hide = true, default_value = PINS_PATH)]
    pins: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum GraphicsArg {
    /// Decide from this computer's graphics device
    Auto,
    /// Use the graphics device
    Hardware,
    /// Draw in software
    Software,
}

#[derive(Debug, Args)]
struct EnableArgs {
    /// Consent given: switch on without asking
    #[arg(long)]
    yes: bool,
    /// Return at once and switch on in the background; `status` shows the progress
    #[arg(long)]
    detach: bool,
    /// How Android draws
    #[arg(long, value_enum, default_value = "auto")]
    graphics: GraphicsArg,
    /// Fetch the pinned files from this address instead of their publisher; their digests are
    /// checked all the same
    #[arg(long, value_name = "URL")]
    mirror: Option<String>,
    #[command(flatten)]
    pins: PinsArgs,
}

#[derive(Debug, Args)]
struct DisableArgs {
    /// Consent given: switch off without asking
    #[arg(long)]
    yes: bool,
    /// Also delete the Android images (they are downloaded again when Android is switched on)
    #[arg(long)]
    remove_images: bool,
    /// Print JSON instead of text
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct InstallArgs {
    /// The APK file
    apk: PathBuf,
    /// Seconds to wait for Android to start, if it is not running
    #[arg(long, value_name = "SECONDS", default_value_t = 300)]
    wait: u64,
    #[command(flatten)]
    pins: PinsArgs,
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Include Android's own applications, which the launcher hides
    #[arg(long)]
    all: bool,
    /// Print JSON instead of text
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AppArgs {
    /// The application's identifier, its package name, such as org.example.app
    id: String,
    /// Seconds to wait for Android to start, if it is not running
    #[arg(long, value_name = "SECONDS", default_value_t = 300)]
    wait: u64,
    /// Print JSON instead of text
    #[arg(long)]
    json: bool,
}

/// Asks the person at the terminal; `false` when there is no terminal.
fn ask(paragraphs: &[String], question: &str) -> bool {
    if !io::stdin().is_terminal() {
        return false;
    }
    let mut out = io::stderr();
    for paragraph in paragraphs {
        let _ = writeln!(out, "{paragraph}\n");
    }
    let _ = write!(out, "{question} Type yes to continue: ");
    let _ = out.flush();
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer).is_ok() && answer.trim() == "yes"
}

/// Runs this program again through pkexec, which asks for administrator
/// authentication on the desktop's trusted path, and returns its exit
/// status.
fn elevate(arguments: &[String]) -> Result<u8, Failure> {
    let program = std::env::current_exe()
        .ok()
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/usr/bin/tur-android"));
    let status = Process::new("pkexec").arg(program).args(arguments).status();
    match status.map(|s| s.code()) {
        Ok(Some(code)) if code != 126 && code != 127 => Ok(u8::try_from(code).unwrap_or(1)),
        Ok(_) => Err(Failure::refused(
            Code::AdminRequired,
            "Administrator authentication was not given, so nothing was changed.",
        )),
        Err(error) => Err(Failure::refused(
            Code::AdminRequired,
            format!(
                "This needs administrator rights and pkexec could not be started ({error}). Run \
                 it as root."
            ),
        )),
    }
}

fn is_root() -> bool {
    host::effective_uid() == Some(0)
}

fn status() -> Done {
    let version = host::waydroid_version();
    let enabled = user::enabled();
    let processes = host::android_processes();
    let mounts = host::android_mounts();
    let interfaces = host::android_interfaces();
    let state: serde_json::Value = std::fs::read_to_string(host::STATE_FILE)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::Value::Null);
    let switching_on = host::unit_state("tur-android-enable.service").active == "active";
    let message = match (&version, enabled, switching_on) {
        (None, _, _) => "Android cannot be used here: Waydroid is not in this system image.".to_owned(),
        (Some(_), _, true) => "Android is being switched on.".to_owned(),
        (Some(_), true, false) => format!(
            "Android is switched on: {} process(es) running, {} mount(s), {} network interface(s).",
            processes.len(),
            mounts.len(),
            interfaces.len()
        ),
        (Some(_), false, false) => format!(
            "Android is switched off: {} process(es), {} mount(s), {} network interface(s) of \
             Android.",
            processes.len(),
            mounts.len(),
            interfaces.len()
        ),
    };
    Done::new(message)
        .with("waydroid", json!(version))
        .with("binder_filesystem", json!(host::binder_filesystem()))
        .with("enabled", json!(enabled))
        .with("switching_on", json!(switching_on))
        .with("state", state)
        .with("service", json!(host::unit_state(host::SERVICE)))
        .with("binderfs_unit", json!(host::unit_state(host::BINDERFS_UNIT)))
        .with("processes", json!(processes))
        .with("mounts", json!(mounts))
        .with("interfaces", json!(interfaces))
        .with("container_memory_bytes", json!(host::container_memory()))
        .with("graphics_device", json!(host::graphics()))
        .with("selinux_enforcing", json!(host::selinux_enforcing()))
        .with("free_bytes", json!(host::free_bytes("/var/lib")))
        .with("session", user::session_facts())
}

fn explain(pins_path: &Path) -> Result<Done, Failure> {
    let pins = system::load_pins(pins_path)?;
    let explanation = pins::explain(&pins);
    Ok(Done::new(explanation.paragraphs.join("\n\n")).with("explanation", json!(explanation)))
}

fn enable(arguments: &EnableArgs) -> Result<Result<Done, Failure>, u8> {
    let (_, pins) = match system::preflight(&arguments.pins.pins) {
        Ok(found) => found,
        Err(failure) => return Ok(Err(failure)),
    };
    let explanation = pins::explain(&pins);
    if !arguments.yes
        && (arguments.pins.json
            || !ask(&explanation.paragraphs, "Switch Android on now?"))
    {
        return Ok(Err(Failure::refused(
            Code::ConsentRequired,
            "Android was not switched on: it needs your consent first. Read what it downloads \
             and costs (`tur-android explain`), then run `tur-android enable --yes`.",
        )
        .with("explanation", json!(explanation))));
    }
    let mut again = vec!["enable".to_owned(), "--yes".to_owned()];
    if arguments.pins.json {
        again.push("--json".to_owned());
    }
    again.push(format!(
        "--graphics={}",
        match arguments.graphics {
            GraphicsArg::Auto => "auto",
            GraphicsArg::Hardware => "hardware",
            GraphicsArg::Software => "software",
        }
    ));
    if let Some(mirror) = &arguments.mirror {
        again.push(format!("--mirror={mirror}"));
    }
    if arguments.pins.pins != Path::new(PINS_PATH) {
        again.push(format!("--pins={}", arguments.pins.pins.display()));
    }
    if !is_root() {
        if arguments.detach {
            again.push("--detach".to_owned());
        }
        return match elevate(&again) {
            Ok(status) => Err(status),
            Err(failure) => Ok(Err(failure)),
        };
    }
    if arguments.detach {
        let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/bin/tur-android"));
        let mut command = Process::new("systemd-run");
        command
            .args(["--unit=tur-android-enable", "--collect", "--quiet", "--"])
            .arg(program)
            .args(&again);
        let started = run::run_command(&mut command, Duration::from_secs(60));
        return Ok(if started.ok() {
            Ok(Done::new(
                "Switching Android on has started. It downloads about 1 GB; `tur-android status` \
                 shows when it is done.",
            )
            .with("unit", json!("tur-android-enable.service")))
        } else {
            Err(Failure::failed(
                Code::StepFailed,
                format!("Switching on could not be started: {}", started.account()),
            ))
        });
    }
    let options = system::EnableOptions {
        graphics: match arguments.graphics {
            GraphicsArg::Auto => system::Graphics::Auto,
            GraphicsArg::Hardware => system::Graphics::Hardware,
            GraphicsArg::Software => system::Graphics::Software,
        },
        mirror: arguments.mirror.clone(),
        pins: arguments.pins.pins.clone(),
    };
    Ok(system::enable(&options).map(|done| done.with("explanation", json!(explanation))))
}

fn disable(arguments: &DisableArgs) -> Result<Result<Done, Failure>, u8> {
    let paragraphs = vec![
        "Switching Android off stops every Android application and the Android system, and \
         unmounts everything of Android. Your Android applications and their data are kept."
            .to_owned(),
    ];
    if !arguments.yes
        && (arguments.json || !ask(&paragraphs, "Switch Android off now?"))
    {
        return Ok(Err(Failure::refused(
            Code::ConsentRequired,
            "Android was not switched off: run `tur-android disable --yes` to confirm.",
        )));
    }
    if !is_root() {
        let mut again = vec!["disable".to_owned(), "--yes".to_owned()];
        if arguments.json {
            again.push("--json".to_owned());
        }
        if arguments.remove_images {
            again.push("--remove-images".to_owned());
        }
        return match elevate(&again) {
            Ok(status) => Err(status),
            Err(failure) => Ok(Err(failure)),
        };
    }
    Ok(system::disable(arguments.remove_images))
}

fn codes() -> Done {
    let list: Vec<_> = Code::ALL
        .iter()
        .map(|code| {
            json!({
                "code": code,
                "number": code.error_code().number(),
                "title": code.title(),
            })
        })
        .collect();
    let text: Vec<String> = Code::ALL
        .iter()
        .map(|code| format!("{} {}", code.as_str(), code.title()))
        .collect();
    Done::new(text.join("\n")).with("codes", json!(list))
}

fn run_cli(arguments: Vec<OsString>) -> u8 {
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return if error.use_stderr() { 2 } else { 0 };
        }
    };
    let (name, json_output, outcome) = match &cli.command {
        Command::Status(a) => ("status", a.json, Ok(status())),
        Command::Explain(a) => ("explain", a.json, explain(&a.pins)),
        Command::Enable(a) => match enable(a) {
            Ok(outcome) => ("enable", a.pins.json, outcome),
            Err(status) => return status,
        },
        Command::Disable(a) => match disable(a) {
            Ok(outcome) => ("disable", a.json, outcome),
            Err(status) => return status,
        },
        Command::Install(a) => (
            "install",
            a.pins.json,
            system::load_pins(&a.pins.pins).and_then(|pins| {
                user::install(&a.apk, &pins, Duration::from_secs(a.wait))
            }),
        ),
        Command::List(a) => ("list", a.json, Ok(user::list(a.all))),
        Command::Launch(a) => ("launch", a.json, user::launch(&a.id, Duration::from_secs(a.wait))),
        Command::Remove(a) => ("remove", a.json, user::remove(&a.id, Duration::from_secs(a.wait))),
        Command::Codes(a) => ("codes", a.json, Ok(codes())),
    };
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    outcome::print(name, &outcome, json_output, &mut stdout, &mut stderr)
}

fn main() -> ExitCode {
    ExitCode::from(run_cli(std::env::args_os().collect()))
}
