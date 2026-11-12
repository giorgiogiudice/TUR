// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-boot-health`: decides whether a boot was good, and makes a fallback
//! to the previous OS version permanent.
//!
//! TUR deploys a new OS version beside the running one. GRUB counts the
//! attempts to start it in its environment block and starts the previous
//! version when they run out (ADR-0014; the design is
//! `docs/architecture/boot-health-and-rollback.md`). This program is the
//! userspace half:
//!
//! * `arm` sets the counter at shutdown, immediately before OSTree makes a
//!   staged deployment the default.
//! * `classify` decides early in every boot what kind of boot this is, and
//!   acts on it. In the previous version, after GRUB fell back to it, it
//!   makes that choice permanent with `bootc rollback`, records the failed
//!   image as rejected and leaves a notice for the user.
//! * `gate` runs the required health checks, each with a time limit.
//! * `mark-good` removes the counter once the gate has passed. That is the
//!   moment a boot is marked good.
//! * `record-failure` records a failed gate. The restart that follows in a
//!   trial is a plain systemd unit, not this program.
//! * `export-evidence` keeps the journals of a failed version's attempts.
//! * `status` and `acknowledge` are for people, the updater and `tur-diag`.
//!
//! The version under trial is never trusted to repair anything: its part is
//! to say "good", or to restart. Everything that touches the machine goes
//! through the `Host` trait in `host.rs`, so the logic is tested against a
//! machine in memory that can lose power at any write.
//!
//! The program has no dependencies outside this repository and the Rust
//! standard library.

#![forbid(unsafe_code)]

mod codes;
mod config;
mod gate;
mod host;
mod json;
mod model;
mod real;
mod state;
mod status;
mod trial;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

use config::GateConfig;
use host::Host;

const USAGE: &str = "\
Usage: tur-boot-health COMMAND

Commands run by systemd units (as root):
  classify          decide what kind of boot this is and act on it
  gate              run the boot health gate; exit 1 if a required check fails
  mark-good         mark this boot good: remove GRUB's boot counter
  record-failure    record that the gate failed
  arm               at shutdown: arm attempt counting for a staged update
  export-evidence   keep the journals of a failed update's attempts

Commands for people and other programs:
  status [--json]   this boot, the trial, rejected images, the pending notice
  status --codes    the error codes this program uses
  acknowledge       dismiss the pending notice
  --version         print the version
";

/// Reads `gate.toml` from the image. A missing file means the design's
/// values; a file that cannot be understood is an error.
fn load_config(host: &mut dyn Host) -> Result<GateConfig, String> {
    match host.read_file(&format!("{}/gate.toml", config::CONFIG_DIR)) {
        Some(text) => GateConfig::parse(&text),
        None => Ok(GateConfig::default()),
    }
}

/// The configuration for the commands that must act whatever the image
/// says: they fall back to the design's values and say so.
fn config_or_default(host: &mut dyn Host) -> GateConfig {
    match load_config(host) {
        Ok(config) => config,
        Err(error) => {
            host.console(&format!(
                "gate.toml is not usable ({error}); using the built-in values"
            ));
            GateConfig::default()
        }
    }
}

fn dispatch(host: &mut dyn Host, arguments: &[String]) -> i32 {
    let command = arguments.first().map(String::as_str).unwrap_or("");
    let flag = |name: &str| arguments.iter().skip(1).any(|argument| argument == name);
    match command {
        "classify" => {
            let config = config_or_default(host);
            trial::classify(host, &config);
            0
        }
        "gate" => {
            let config = load_config(host);
            if gate::gate(host, config) { 0 } else { 1 }
        }
        "mark-good" => match trial::mark_good(host) {
            Ok(()) => 0,
            Err(error) => {
                host.console(&format!("mark-good: refused: {error}"));
                1
            }
        },
        "record-failure" => {
            trial::record_failure(host);
            0
        }
        "arm" => {
            let config = config_or_default(host);
            trial::arm(host, &config);
            0
        }
        "export-evidence" => {
            match status::export_evidence(host) {
                Ok(done) => host.console(&format!("export-evidence: {done}")),
                Err(error) => host.console(&format!("export-evidence: failed: {error}")),
            }
            0
        }
        "status" => {
            if flag("--codes") {
                print!("{}", status::codes_text());
            } else {
                let status = status::status_json(host);
                if flag("--json") {
                    print!("{}", status.to_pretty());
                } else {
                    print!("{}", status::status_text(&status));
                }
            }
            0
        }
        "acknowledge" => match status::acknowledge(host) {
            Ok(true) => {
                println!("The notice was dismissed.");
                0
            }
            Ok(false) => {
                println!("There is no pending notice.");
                0
            }
            Err(error) => {
                eprintln!("tur-boot-health: {error}");
                1
            }
        },
        "--version" | "version" => {
            println!("tur-boot-health {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "--help" | "help" | "-h" => {
            print!("{USAGE}");
            0
        }
        _ => {
            eprint!("{USAGE}");
            2
        }
    }
}

/// Runs the command named by `arguments` (without the program name) on this
/// machine and returns the process's exit status.
pub fn run(arguments: &[String]) -> i32 {
    let mut host = real::SystemHost::new();
    dispatch(&mut host, arguments)
}
