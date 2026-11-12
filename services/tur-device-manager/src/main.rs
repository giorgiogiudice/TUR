// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-device`: print the hardware inventory and the problems found in it.
//!
//! The tool only reads. With `--from` it reads a fixture directory laid out
//! like the root of a small system and runs no command at all; without it, it
//! reads `/sys`, `/proc` and `/etc/machine-id` and runs `udevadm`, `fwupdmgr`
//! and `journalctl` with fixed, read-only arguments.
//!
//! Exit status: 0 on success (whether or not problems were found), 1 if the
//! input or output failed, 2 for a usage error.

#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::Serialize;
use tur_device_manager::{FsProbe, ProblemReport, collect, render};

const EXIT_OK: u8 = 0;
const EXIT_FAILURE: u8 = 1;

/// The directory given with `--from` cannot be read.
const CODE_INPUT: &str = "TUR-DEVICE-INPUT-001";
/// The result could not be encoded or written.
const CODE_OUTPUT: &str = "TUR-DEVICE-OUTPUT-001";

/// Read-only hardware inventory for TUR.
#[derive(Debug, Parser)]
#[command(name = "tur-device", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the hardware inventory.
    Inventory {
        /// Read this directory, laid out like a small /sys and /proc tree,
        /// instead of the live system. No command is run.
        #[arg(long, value_name = "DIR")]
        from: Option<PathBuf>,
        /// Print JSON (schemas/device-inventory.schema.json).
        #[arg(long)]
        json: bool,
        /// Remove serial numbers, addresses and other identifying values.
        #[arg(long)]
        redact: bool,
    },
    /// Print the problems found: devices without a driver, drivers that did
    /// not bind, firmware that could not be loaded.
    Problems {
        /// Read this directory, laid out like a small /sys and /proc tree,
        /// instead of the live system. No command is run.
        #[arg(long, value_name = "DIR")]
        from: Option<PathBuf>,
        /// Print JSON (the `problem_report` definition of
        /// schemas/device-inventory.schema.json).
        #[arg(long)]
        json: bool,
    },
}

fn load(from: Option<&Path>) -> Result<tur_device_manager::Inventory, String> {
    let probe = match from {
        Some(dir) => {
            if !dir.is_dir() {
                return Err(format!(
                    "{CODE_INPUT}: {} is not a readable directory",
                    dir.display()
                ));
            }
            FsProbe::fixture(dir)
        }
        None => FsProbe::host(),
    };
    Ok(collect(&probe))
}

fn to_json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|mut text| {
            text.push('\n');
            text
        })
        .map_err(|error| format!("{CODE_OUTPUT}: cannot encode the output: {error}"))
}

fn run(command: Command) -> Result<String, String> {
    match command {
        Command::Inventory { from, json, redact } => {
            let mut inventory = load(from.as_deref())?;
            if redact {
                inventory = inventory.redacted();
            }
            if json {
                to_json(&inventory)
            } else {
                Ok(render::inventory_text(&inventory))
            }
        }
        Command::Problems { from, json } => {
            let report = ProblemReport::of(&load(from.as_deref())?);
            if json {
                to_json(&report)
            } else {
                Ok(render::problems_text(&report.findings))
            }
        }
    }
}

fn fail(message: &str) -> ExitCode {
    // If standard error is closed there is nowhere left to report to.
    let _ = writeln!(io::stderr(), "error: {message}");
    ExitCode::from(EXIT_FAILURE)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let text = match run(cli.command) {
        Ok(text) => text,
        Err(message) => return fail(&message),
    };
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::from(EXIT_OK),
        // A reader that stops early (`tur-device inventory | head`) is not a
        // failure of this program.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::from(EXIT_OK),
        Err(error) => fail(&format!("{CODE_OUTPUT}: cannot write the output: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tur_core::{ErrorCode, Family};

    #[test]
    fn the_command_line_codes_are_valid_device_codes() {
        for text in [CODE_INPUT, CODE_OUTPUT] {
            let code: ErrorCode = text.parse().unwrap();
            assert_eq!(code.family(), Family::Device);
        }
    }

    #[test]
    fn the_command_line_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
