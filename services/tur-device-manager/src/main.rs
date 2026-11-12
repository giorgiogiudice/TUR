// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-device`: print the hardware inventory and the problems found in it.
//!
//! The tool only reads. With `--from` it reads a fixture directory laid out
//! like the root of a small system and runs no command at all; without it, it
//! reads `/sys`, `/proc` and `/etc/machine-id` and runs `udevadm`, `fwupdmgr`
//! and `journalctl` with fixed, read-only arguments.
//!
//! `list` and `get` are the command-line forms of the assistant tools
//! `device_list` and `device_get`: a line for each device, and everything
//! known about one. Both always give the redacted view.
//!
//! Exit status: 0 on success (whether or not problems were found), 1 if the
//! input or output failed, 2 for a usage error, 3 if `get` was asked for an
//! identifier no device has.

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

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use tur_device_manager::model::{Category, DeviceRef, DeviceSummary};
use tur_device_manager::{FsProbe, ProblemReport, SCHEMA_VERSION, collect, render};

const EXIT_OK: u8 = 0;
const EXIT_FAILURE: u8 = 1;
/// `get` was asked for an identifier no device has.
const EXIT_NO_DEVICE: u8 = 3;

/// The directory given with `--from` cannot be read.
const CODE_INPUT: &str = "TUR-DEVICE-INPUT-001";
/// No device has the identifier given to `get`.
const CODE_NO_DEVICE: &str = "TUR-DEVICE-INPUT-002";
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
    /// List the devices: an identifier, a category, a name and the bound
    /// kernel driver for each. Serial numbers and addresses are never
    /// included.
    List {
        /// Read this directory, laid out like a small /sys and /proc tree,
        /// instead of the live system. No command is run.
        #[arg(long, value_name = "DIR")]
        from: Option<PathBuf>,
        /// Print JSON: the devices as a list of summaries.
        #[arg(long)]
        json: bool,
        /// Only list devices of this category. May be given more than once.
        #[arg(long = "category", value_enum, value_name = "CATEGORY")]
        categories: Vec<CategoryArg>,
    },
    /// Show everything known about one device, by the identifier `list`
    /// printed. Serial numbers and addresses are redacted.
    Get {
        /// Read this directory, laid out like a small /sys and /proc tree,
        /// instead of the live system. No command is run.
        #[arg(long, value_name = "DIR")]
        from: Option<PathBuf>,
        /// Print JSON: the category and the device.
        #[arg(long)]
        json: bool,
        /// A device identifier, as `list` prints it.
        #[arg(value_name = "ID")]
        id: String,
    },
}

/// A device category on the command line, spelled as the inventory and the
/// assistant tool manifest spell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CategoryArg {
    /// PCI and PCI Express functions
    #[value(name = "pci")]
    Pci,
    /// USB devices
    #[value(name = "usb")]
    Usb,
    /// Network interfaces
    #[value(name = "network")]
    Network,
    /// Sound cards
    #[value(name = "audio")]
    Audio,
    /// Storage controllers
    #[value(name = "storage_controller")]
    StorageController,
    /// Whole block devices
    #[value(name = "block")]
    Block,
    /// Input devices
    #[value(name = "input")]
    Input,
    /// Bluetooth adapters
    #[value(name = "bluetooth")]
    Bluetooth,
    /// Video devices
    #[value(name = "camera")]
    Camera,
    /// Thunderbolt and USB4 devices
    #[value(name = "thunderbolt")]
    Thunderbolt,
}

impl CategoryArg {
    fn category(self) -> Category {
        match self {
            CategoryArg::Pci => Category::Pci,
            CategoryArg::Usb => Category::Usb,
            CategoryArg::Network => Category::Network,
            CategoryArg::Audio => Category::Audio,
            CategoryArg::StorageController => Category::StorageController,
            CategoryArg::Block => Category::Block,
            CategoryArg::Input => Category::Input,
            CategoryArg::Bluetooth => Category::Bluetooth,
            CategoryArg::Camera => Category::Camera,
            CategoryArg::Thunderbolt => Category::Thunderbolt,
        }
    }
}

/// What `list --json` prints.
#[derive(Serialize)]
struct DeviceList {
    schema_version: u32,
    /// Always true: a listing never holds a sensitive value.
    redacted: bool,
    devices: Vec<DeviceSummary>,
}

/// What `get --json` prints: the device, with its category beside it.
#[derive(Serialize)]
struct DeviceDetail<'a> {
    schema_version: u32,
    /// Always true: `get` gives the redacted view.
    redacted: bool,
    #[serde(flatten)]
    found: DeviceRef<'a>,
}

/// What went wrong, and the exit status that says so.
struct Failure {
    status: u8,
    message: String,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure {
            status: EXIT_FAILURE,
            message,
        }
    }
}

/// `text` with every character that is not printable ASCII written as an
/// escape, so that an identifier given on the command line cannot disturb a
/// terminal when it is repeated.
fn printable(text: &str) -> String {
    text.chars()
        .take(256)
        .flat_map(|character| match character {
            ' '..='~' => character.to_string().chars().collect::<Vec<char>>(),
            other => other.escape_unicode().collect(),
        })
        .collect()
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

fn run(command: Command) -> Result<String, Failure> {
    match command {
        Command::Inventory { from, json, redact } => {
            let mut inventory = load(from.as_deref())?;
            if redact {
                inventory = inventory.redacted();
            }
            if json {
                Ok(to_json(&inventory)?)
            } else {
                Ok(render::inventory_text(&inventory))
            }
        }
        Command::Problems { from, json } => {
            let report = ProblemReport::of(&load(from.as_deref())?);
            if json {
                Ok(to_json(&report)?)
            } else {
                Ok(render::problems_text(&report.findings))
            }
        }
        Command::List {
            from,
            json,
            categories,
        } => {
            // The redacted view, like every answer to the assistant. A
            // summary has no sensitive field; redacting first also removes
            // a sensitive value that a device repeats in its name.
            let inventory = load(from.as_deref())?.redacted();
            let wanted: Vec<Category> = categories.iter().map(|arg| arg.category()).collect();
            let devices: Vec<DeviceSummary> = inventory
                .summaries()
                .into_iter()
                .filter(|device| wanted.is_empty() || wanted.contains(&device.category))
                .collect();
            if json {
                Ok(to_json(&DeviceList {
                    schema_version: SCHEMA_VERSION,
                    redacted: true,
                    devices,
                })?)
            } else {
                Ok(list_text(&devices))
            }
        }
        Command::Get { from, json, id } => {
            let inventory = load(from.as_deref())?.redacted();
            let Some(found) = inventory.find(&id) else {
                return Err(Failure {
                    status: EXIT_NO_DEVICE,
                    message: format!(
                        "{CODE_NO_DEVICE}: no device has the identifier \"{}\"; `tur-device \
                         list` prints the identifiers",
                        printable(&id)
                    ),
                });
            };
            let detail = DeviceDetail {
                schema_version: SCHEMA_VERSION,
                redacted: true,
                found,
            };
            // One device has no text layout of its own: it is printed as the
            // same document either way.
            let _ = json;
            Ok(to_json(&detail)?)
        }
    }
}

/// The listing as text: one line for each device.
fn list_text(devices: &[DeviceSummary]) -> String {
    if devices.is_empty() {
        return "No devices found.\n".to_owned();
    }
    let mut text = String::new();
    for device in devices {
        let category = serde_json::to_value(device.category)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        text.push_str(&format!(
            "{}  {}  {}  [{}]\n",
            device.id,
            category,
            printable(&device.name),
            device.driver.as_deref().map_or_else(|| "no driver".to_owned(), printable),
        ));
    }
    text
}

fn fail(status: u8, message: &str) -> ExitCode {
    // If standard error is closed there is nowhere left to report to.
    let _ = writeln!(io::stderr(), "error: {message}");
    ExitCode::from(status)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let text = match run(cli.command) {
        Ok(text) => text,
        Err(failure) => return fail(failure.status, &failure.message),
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
        Err(error) => fail(
            EXIT_FAILURE,
            &format!("{CODE_OUTPUT}: cannot write the output: {error}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tur_core::{ErrorCode, Family};

    #[test]
    fn the_command_line_codes_are_valid_device_codes() {
        for text in [CODE_INPUT, CODE_NO_DEVICE, CODE_OUTPUT] {
            let code: ErrorCode = text.parse().unwrap();
            assert_eq!(code.family(), Family::Device);
        }
    }

    #[test]
    fn the_command_line_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn a_category_is_spelled_as_the_inventory_spells_it() {
        // The command line, the JSON output and the assistant tool manifest
        // have one spelling for a category.
        for argument in CategoryArg::value_variants() {
            let on_the_command_line = argument
                .to_possible_value()
                .map(|value| value.get_name().to_owned());
            let in_the_inventory = serde_json::to_value(argument.category())
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned));
            assert!(on_the_command_line.is_some());
            assert_eq!(on_the_command_line, in_the_inventory, "{argument:?}");
        }
        assert_eq!(CategoryArg::value_variants().len(), 10);
    }

    #[test]
    fn an_identifier_is_repeated_only_in_printable_form() {
        assert_eq!(printable("pci-0000:00:1f.3"), "pci-0000:00:1f.3");
        assert_eq!(printable("a\u{1b}[31mb\n"), "a\\u{1b}[31mb\\u{a}");
        assert_eq!(printable(&"x".repeat(1000)).len(), 256);
    }
}
