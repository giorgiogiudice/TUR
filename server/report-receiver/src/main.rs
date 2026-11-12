// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-report-receiver`: the service and the administrator's commands.

#![forbid(unsafe_code)]

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};

use tur_report_receiver::clock::{Clock, ManualClock, SystemClock};
use tur_report_receiver::config::{Config, DEFAULT_CONFIG};
use tur_report_receiver::crypto::generate_key_hex;
use tur_report_receiver::http;
use tur_report_receiver::log::Log;
use tur_report_receiver::receiver::Receiver;
use tur_report_receiver::tracker::GitHub;

/// TUR's problem report receiver (ADR-0024). See
/// docs/guides/deploy-report-receiver.md.
#[derive(Debug, Parser)]
#[command(name = "tur-report-receiver", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the receiver until SIGTERM.
    Serve {
        /// The configuration file.
        #[arg(long, value_name = "FILE", default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Check the configuration, the keys and the token file, then exit.
    CheckConfig {
        /// The configuration file.
        #[arg(long, value_name = "FILE", default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Delete what has passed its retention period now, and print counts.
    Sweep {
        /// The configuration file.
        #[arg(long, value_name = "FILE", default_value = DEFAULT_CONFIG)]
        config: PathBuf,
        /// Sweep as if it were this time (seconds since 1970), for tests.
        #[arg(long, value_name = "EPOCH_SECONDS")]
        now: Option<u64>,
    },
    /// Mark every report filed under an issue as fixed in a version.
    MarkFixed {
        /// The configuration file.
        #[arg(long, value_name = "FILE", default_value = DEFAULT_CONFIG)]
        config: PathBuf,
        /// The issue number.
        #[arg(long)]
        issue: u64,
        /// The TUR version that contains the fix.
        #[arg(long)]
        version: String,
    },
    /// Write the decrypted bundle of a report to a new file, for a maintainer.
    Decrypt {
        /// The configuration file.
        #[arg(long, value_name = "FILE", default_value = DEFAULT_CONFIG)]
        config: PathBuf,
        /// The report's identifier, R-...
        #[arg(long, value_name = "ID")]
        report: String,
        /// The file to create; it must not exist.
        #[arg(long, value_name = "FILE")]
        output: PathBuf,
    },
    /// Make a new encryption key file (mode 0400). It must not exist.
    Keygen {
        /// The key file to create, <keys_dir>/<id>.key.
        #[arg(long, value_name = "FILE")]
        output: PathBuf,
    },
}

fn create_private(path: &Path, mode: u32, data: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(data)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn open(config: &Path, clock: Arc<dyn Clock>) -> Result<Receiver, String> {
    let config = Config::load(config)?;
    Receiver::open(config, clock, Log::stderr())
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Serve { config } => {
            let receiver = open(&config, Arc::new(SystemClock))?;
            let tracker = GitHub::new(&receiver.config().github);
            tracker.token().map_err(|_| {
                "the GitHub token file is missing, empty or readable by others".to_owned()
            })?;
            http::serve(Arc::new(receiver), Arc::new(tracker))
        }
        Command::CheckConfig { config } => {
            let receiver = open(&config, Arc::new(SystemClock))?;
            GitHub::new(&receiver.config().github)
                .token()
                .map_err(|_| {
                    "the GitHub token file is missing, empty or readable by others".to_owned()
                })?;
            println!("configuration, keys and token file are usable");
            Ok(())
        }
        Command::Sweep { config, now } => {
            let clock: Arc<dyn Clock> = match now {
                Some(now) => Arc::new(ManualClock::new(now)),
                None => Arc::new(SystemClock),
            };
            let receiver = open(&config, clock)?;
            let round = receiver
                .sweep()
                .map_err(|error| format!("sweep: {error}"))?;
            println!(
                "{}",
                serde_json::to_string(&round).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        Command::MarkFixed {
            config,
            issue,
            version,
        } => {
            let receiver = open(&config, Arc::new(SystemClock))?;
            let marked = receiver.mark_fixed(issue, &version)?;
            println!("{marked} reports marked fixed in {version}");
            Ok(())
        }
        Command::Decrypt {
            config,
            report,
            output,
        } => {
            let receiver = open(&config, Arc::new(SystemClock))?;
            let bundle = receiver.decrypt(&report)?;
            create_private(&output, 0o600, &bundle)?;
            println!("{} bytes written to {}", bundle.len(), output.display());
            Ok(())
        }
        Command::Keygen { output } => {
            let key = generate_key_hex()?;
            create_private(&output, 0o400, format!("{key}\n").as_bytes())?;
            println!("key written to {}", output.display());
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("tur-report-receiver: {problem}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn command_line_definition_is_consistent() {
        Cli::command().debug_assert();
        assert!(Cli::try_parse_from(["tur-report-receiver", "sweep", "--now", "5"]).is_ok());
        assert!(
            Cli::try_parse_from(["tur-report-receiver", "decrypt", "--report", "R-1"]).is_err()
        );
    }
}
