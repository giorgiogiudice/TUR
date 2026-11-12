// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The `uos-diag` command: a thin shell around the `uos_diag` library.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand, ValueEnum};

use uos_diag::{
    COLLECTORS, CollectRequest, DEFAULT_MAX_BYTES, DiagError, ErrorKind, Profile, SystemHost,
    build_bundle, gather_known_identifiers, gather_status, render_status, verify_file,
    write_bundle,
};

/// Collect and verify UOS diagnostic bundles. Everything stays on this
/// computer: uos-diag never uploads anything.
#[derive(Debug, Parser)]
#[command(name = "uos-diag", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the UOS build, kernel, boot ID and number of failed units.
    Status {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Collect a support bundle into a new local file.
    Collect {
        /// The file to create, for example UOS-support.tar.zst. It must not exist.
        #[arg(long, value_name = "FILE")]
        output: PathBuf,
        /// Leave out data from earlier boots.
        #[arg(long)]
        since_boot: bool,
        /// How much to redact. "default" replaces every identifier it
        /// recognises. "technical" keeps hardware serial numbers, MAC
        /// addresses and disk UUIDs, which identify this computer.
        #[arg(long, value_enum, default_value_t = ProfileArg::Default)]
        profile: ProfileArg,
    },
    /// Check a support bundle's integrity and every recorded checksum.
    VerifyBundle {
        /// The bundle to check.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ProfileArg {
    /// Redact every identifier that is recognised.
    Default,
    /// Keep serial numbers, MAC addresses and disk UUIDs.
    Technical,
}

impl From<ProfileArg> for Profile {
    fn from(profile: ProfileArg) -> Self {
        match profile {
            ProfileArg::Default => Profile::Default,
            ProfileArg::Technical => Profile::Technical,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: &Command) -> Result<(), DiagError> {
    let host = SystemHost::new();
    match command {
        Command::Status { json } => {
            let status = gather_status(&host);
            if *json {
                let text = serde_json::to_string_pretty(&status)
                    .map_err(|error| DiagError::new(ErrorKind::Internal, error.to_string()))?;
                println!("{text}");
            } else {
                print!("{}", render_status(&status));
            }
            Ok(())
        }
        Command::Collect {
            output,
            since_boot,
            profile,
        } => {
            let profile = Profile::from(*profile);
            let session_users: Vec<String> = ["USER", "LOGNAME", "SUDO_USER"]
                .into_iter()
                .filter_map(|name| std::env::var(name).ok())
                .collect();
            let created_epoch_seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            let request = CollectRequest {
                profile,
                since_boot: *since_boot,
                created_epoch_seconds,
                collectors: COLLECTORS,
                known: gather_known_identifiers(&host, &session_users),
                max_bytes: DEFAULT_MAX_BYTES,
            };
            let built = build_bundle(&host, &request)?;
            write_bundle(output, &built.archive)?;

            let shown = output.display();
            let files = built.manifest.files.len();
            let missing = built.manifest.missing.len();
            let profile_name = profile.as_str();
            let replacements: u64 = built
                .report
                .categories
                .values()
                .map(|category| category.replacements)
                .sum();
            println!("Support bundle written to {shown}");
            println!("  files:             {files}");
            println!("  not collected:     {missing} (reasons are in manifest.json)");
            println!("  redaction profile: {profile_name}");
            println!("  identifiers replaced: {replacements}");
            if profile == Profile::Technical {
                println!(
                    "The technical profile keeps serial numbers, MAC addresses and disk UUIDs. \
                     Share this file only with someone you trust with them."
                );
            }
            println!(
                "Nothing was uploaded. Redaction cannot recognise every personal detail: \
                 read the files before you share the bundle."
            );
            Ok(())
        }
        Command::VerifyBundle { file } => {
            let verified = verify_file(file)?;
            let shown = file.display();
            let count = verified.files_verified;
            let created = &verified.manifest.created_utc;
            let profile_name = verified.manifest.redaction_profile.as_str();
            println!("{shown}: OK");
            println!("  {count} files match their SHA-256");
            println!("  created {created}, redaction profile {profile_name}");
            Ok(())
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
    }

    #[test]
    fn subcommands_parse_as_documented() {
        let cli = Cli::try_parse_from(["uos-diag", "status", "--json"]).expect("status parses");
        assert!(matches!(cli.command, Command::Status { json: true }));

        let cli = Cli::try_parse_from([
            "uos-diag",
            "collect",
            "--output",
            "/tmp/b.tar.zst",
            "--since-boot",
            "--profile",
            "technical",
        ])
        .expect("collect parses");
        match cli.command {
            Command::Collect {
                output,
                since_boot,
                profile,
            } => {
                assert_eq!(output, PathBuf::from("/tmp/b.tar.zst"));
                assert!(since_boot);
                assert_eq!(Profile::from(profile), Profile::Technical);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let cli = Cli::try_parse_from(["uos-diag", "collect", "--output", "b.tar.zst"])
            .expect("collect parses");
        match cli.command {
            Command::Collect {
                since_boot,
                profile,
                ..
            } => {
                assert!(!since_boot);
                assert_eq!(Profile::from(profile), Profile::Default);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let cli = Cli::try_parse_from(["uos-diag", "verify-bundle", "b.tar.zst"])
            .expect("verify-bundle parses");
        assert!(matches!(cli.command, Command::VerifyBundle { .. }));
    }

    #[test]
    fn invalid_command_lines_are_rejected() {
        assert!(Cli::try_parse_from(["uos-diag"]).is_err());
        assert!(Cli::try_parse_from(["uos-diag", "collect"]).is_err());
        assert!(
            Cli::try_parse_from(["uos-diag", "collect", "--output", "x", "--profile", "all"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["uos-diag", "verify-bundle"]).is_err());
        assert!(Cli::try_parse_from(["uos-diag", "upload"]).is_err());
    }
}
