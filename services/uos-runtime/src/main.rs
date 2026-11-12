// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! `uos-runtime`: the command-line front end of the Universal Runtime Manager.
//!
//! ```text
//! uos-runtime inspect <path> [--json]     identify a file and recommend a runtime
//! uos-runtime profile validate <file>     validate an application profile
//! ```
//!
//! Neither command runs, loads or modifies the file it is given.
//!
//! Exit status:
//!
//! | Status | Meaning |
//! |---|---|
//! | 0 | The file has a route and everything the route needs is installed; or the profile is valid. |
//! | 1 | The input could not be read, or is truncated, malformed or over an inspection limit. |
//! | 2 | The command line is wrong. |
//! | 3 | The file was inspected and no supported runtime exists for it. |
//! | 4 | The file has a route, but something the route needs is not installed. |
//! | 5 | The profile is not valid. |

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::Serialize;

use uos_runtime::limits::MAX_PROFILE_BYTES;
use uos_runtime::profile::parse_profile;
use uos_runtime::report::render_text;
use uos_runtime::{
    Arch, Code, Component, HostFacts, Inspection, Recommendation, Runtime, inspect, recommend,
};

const EXIT_OK: u8 = 0;
const EXIT_INPUT: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_UNSUPPORTED: u8 = 3;
const EXIT_MISSING: u8 = 4;
const EXIT_INVALID_PROFILE: u8 = 5;

/// Inspect executables and packages and recommend a runtime. Never runs them.
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
}

#[derive(Subcommand)]
enum ProfileCommand {
    /// Check an application profile against the schema and its rules.
    Validate {
        /// The profile, a JSON file.
        file: PathBuf,
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
    let text = match read_profile(file) {
        Ok(text) => text,
        Err(message) => {
            let message = format!("cannot read {}: {message}", file.display());
            return fail(false, Code::ProfileUnreadable, &message, EXIT_INPUT);
        }
    };
    match parse_profile(&text) {
        Ok(profile) => {
            print_line(&format!(
                "valid: {} ({}), schema version {}",
                profile.app_id, profile.display_name, profile.schema_version
            ));
            EXIT_OK
        }
        Err(error) => fail(
            false,
            error.code(),
            &error.to_string(),
            EXIT_INVALID_PROFILE,
        ),
    }
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
