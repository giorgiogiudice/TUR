// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `tur-diag` command: a thin shell around the `tur_diag` library.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand, ValueEnum};

use tur_diag::manifest::format_utc;
use tur_diag::report::{
    self, InspectedReport, ListedReport, PREPARED_DIR, PreparedBy, REPORT_SCHEMA_VERSION,
    ReportKind, ReportRequest, SENT_DIR, SentRecord,
};
use tur_diag::{
    COLLECTORS, CollectRequest, DEFAULT_MAX_BYTES, DiagError, ErrorKind, Profile, SystemHost,
    build_bundle, gather_known_identifiers, gather_status, render_status, verify_file,
    write_bundle,
};

/// Collect and verify TUR diagnostic bundles, and prepare problem reports.
/// Everything stays on this computer, except a problem report you send
/// yourself with `tur-diag report send` after confirming it.
#[derive(Debug, Parser)]
#[command(name = "tur-diag", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// Parsed once per run; the size of the largest variant does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
enum Command {
    /// Show the TUR build, kernel, boot ID and number of failed units.
    Status {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Collect a support bundle into a new local file.
    Collect {
        /// The file to create, for example TUR-support.tar.zst. It must not exist.
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
    /// Prepare, send and list problem reports for TUR's developers. Nothing
    /// is sent unless you confirm it.
    Report {
        #[command(subcommand)]
        command: ReportCommand,
    },
}

/// Where reports are kept and where the receiver's address is read.
#[derive(Debug, Clone, Args)]
struct Places {
    /// The directory for prepared and sent reports. Default:
    /// $XDG_STATE_HOME/tur/reports, or ~/.local/state/tur/reports.
    #[arg(long, value_name = "DIR")]
    state_dir: Option<PathBuf>,
    /// Read the receiver's address from this file instead of
    /// /etc/tur/problem-reports.conf or /usr/lib/tur/problem-reports.conf.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
enum ReportCommand {
    /// Collect a bundle for a problem and show exactly what would be sent.
    /// Nothing is sent.
    Prepare {
        /// One line that names the problem.
        #[arg(long)]
        title: Option<String>,
        /// What happened, in words.
        #[arg(long)]
        description: Option<String>,
        /// Read the description from this file.
        #[arg(long, value_name = "FILE", conflicts_with = "description")]
        description_file: Option<PathBuf>,
        /// The component concerned, for example update, audio or network.
        #[arg(long)]
        component: Option<String>,
        /// The TUR error code involved, for example TUR-BOOT-GATE-003.
        #[arg(long, value_name = "CODE")]
        error_code: Option<String>,
        /// What kind of problem this is. Default: request, or what boot
        /// health recorded.
        #[arg(long, value_enum)]
        kind: Option<KindArg>,
        /// Make the report from boot health's notice of a failed update or a
        /// failed start.
        #[arg(long)]
        from_boot_health: bool,
        /// Read boot health's status from this file instead of running
        /// tur-boot-health status --json.
        #[arg(long, value_name = "FILE", requires = "from_boot_health")]
        boot_health_status: Option<PathBuf>,
        /// Leave out data from earlier boots.
        #[arg(long)]
        since_boot: bool,
        /// Who wrote the report's text.
        #[arg(long, value_enum, default_value_t = PreparedByArg::Person)]
        prepared_by: PreparedByArg,
        /// Show what would be sent without saving anything.
        #[arg(long)]
        preview: bool,
        /// Save the prepared report to this file instead of the reports directory.
        #[arg(long, value_name = "FILE", conflicts_with = "preview")]
        output: Option<PathBuf>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        places: Places,
    },
    /// Send a prepared report to TUR's configured receiver, once you confirm.
    Send {
        /// The receiver's address. It must be the one configured for TUR.
        #[arg(long, value_name = "URL")]
        to: String,
        /// Confirm sending without being asked. Read what will be sent first.
        #[arg(long)]
        yes: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// The prepared report, as written by `tur-diag report prepare`.
        #[arg(value_name = "FILE")]
        file: PathBuf,
        #[command(flatten)]
        places: Places,
    },
    /// List the reports sent from this account, and those prepared but not sent.
    List {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        places: Places,
    },
    /// Ask the receiver for the state of a sent report.
    Status {
        /// The report's identifier, as `tur-diag report list` shows it.
        #[arg(value_name = "REPORT_ID")]
        report_id: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        places: Places,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum KindArg {
    /// An update failed, was not applied or was refused.
    FailedUpdate,
    /// The system started with a failed health check.
    BootProblem,
    /// A program or service crashed.
    Crash,
    /// A repair did not finish.
    RepairFailed,
    /// Any other problem.
    Request,
}

impl From<KindArg> for ReportKind {
    fn from(kind: KindArg) -> Self {
        match kind {
            KindArg::FailedUpdate => ReportKind::FailedUpdate,
            KindArg::BootProblem => ReportKind::BootProblem,
            KindArg::Crash => ReportKind::Crash,
            KindArg::RepairFailed => ReportKind::RepairFailed,
            KindArg::Request => ReportKind::Request,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PreparedByArg {
    /// You.
    Person,
    /// P@x, for you to read and send.
    Pax,
}

impl From<PreparedByArg> for PreparedBy {
    fn from(by: PreparedByArg) -> Self {
        match by {
            PreparedByArg::Person => PreparedBy::Person,
            PreparedByArg::Pax => PreparedBy::Pax,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn internal(error: impl std::fmt::Display) -> DiagError {
    DiagError::new(ErrorKind::Internal, error.to_string())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn session_users() -> Vec<String> {
    ["USER", "LOGNAME", "SUDO_USER"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .collect()
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<(), DiagError> {
    let text = serde_json::to_string_pretty(value).map_err(internal)?;
    println!("{text}");
    Ok(())
}

fn run(command: Command) -> Result<(), DiagError> {
    let host = SystemHost::new();
    match command {
        Command::Status { json } => {
            let status = gather_status(&host);
            if json {
                print_json(&status)?;
            } else {
                print!("{}", render_status(&status));
            }
            Ok(())
        }
        Command::Collect {
            output,
            since_boot,
            profile,
        } => collect(&host, &output, since_boot, profile.into()),
        Command::VerifyBundle { file } => {
            let verified = verify_file(&file)?;
            let shown = file.display();
            let count = verified.files_verified;
            let created = &verified.manifest.created_utc;
            let profile_name = verified.manifest.redaction_profile.as_str();
            println!("{shown}: OK");
            println!("  {count} files match their SHA-256");
            println!("  created {created}, redaction profile {profile_name}");
            Ok(())
        }
        Command::Report { command } => run_report(&host, command),
    }
}

fn collect(
    host: &SystemHost,
    output: &Path,
    since_boot: bool,
    profile: Profile,
) -> Result<(), DiagError> {
    let request = CollectRequest {
        profile,
        since_boot,
        created_epoch_seconds: now(),
        collectors: COLLECTORS,
        known: gather_known_identifiers(host, &session_users()),
        max_bytes: DEFAULT_MAX_BYTES,
    };
    let built = build_bundle(host, &request)?;
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

/// The reports directory: the one named, or the default one.
fn state_dir(places: &Places) -> Result<PathBuf, DiagError> {
    places
        .state_dir
        .clone()
        .or_else(report::default_state_dir)
        .ok_or_else(|| {
            DiagError::new(
                ErrorKind::ReportRecordsUnavailable,
                "neither XDG_STATE_HOME nor HOME is set to an absolute path",
            )
        })
}

fn run_report(host: &SystemHost, command: ReportCommand) -> Result<(), DiagError> {
    match command {
        ReportCommand::Prepare {
            title,
            description,
            description_file,
            component,
            error_code,
            kind,
            from_boot_health,
            boot_health_status,
            since_boot,
            prepared_by,
            preview,
            output,
            json,
            places,
        } => {
            let prepared_by = PreparedBy::from(prepared_by);
            let mut request = if from_boot_health {
                let status = match &boot_health_status {
                    Some(path) => fs::read_to_string(path).map_err(|error| {
                        DiagError::new(
                            ErrorKind::ReportNothingFromBootHealth,
                            format!("{}: {error}", path.display()),
                        )
                    })?,
                    None => report::read_boot_health(host)?,
                };
                report::request_from_boot_health(&status, prepared_by)?
            } else {
                let missing = if title.is_none() {
                    Some("--title")
                } else if component.is_none() {
                    Some("--component")
                } else {
                    None
                };
                if let Some(option) = missing {
                    return Err(DiagError::new(
                        ErrorKind::ReportInvalid,
                        format!("give {option}, or --from-boot-health"),
                    ));
                }
                ReportRequest {
                    schema_version: REPORT_SCHEMA_VERSION,
                    kind: ReportKind::Request,
                    title: String::new(),
                    description: String::new(),
                    error_code: None,
                    component: String::new(),
                    affected_version: None,
                    prepared_by,
                    boot_health: None,
                }
            };
            if let Some(title) = title {
                request.title = title;
            }
            if let Some(path) = &description_file {
                request.description = fs::read_to_string(path).map_err(|error| {
                    DiagError::new(
                        ErrorKind::ReportInvalid,
                        format!("{}: {error}", path.display()),
                    )
                })?;
            } else if let Some(description) = description {
                request.description = description;
            }
            if let Some(component) = component {
                request.component = component;
            }
            if let Some(code) = error_code {
                request.error_code = Some(code);
            }
            if let Some(kind) = kind {
                request.kind = kind.into();
            }
            // A configuration that cannot be read does not stop a report from
            // being prepared; it is reported when sending.
            let receiver = report::load_config(places.config.as_deref())
                .ok()
                .and_then(|config| config.receiver_url);
            let known = gather_known_identifiers(host, &session_users());
            let prepared = report::prepare(host, &request, since_boot, now(), known)?;
            let saved = if preview {
                None
            } else {
                let path = match output {
                    Some(path) => path,
                    None => {
                        let directory = state_dir(&places)?.join(PREPARED_DIR);
                        report::ensure_private_dir(&directory)?;
                        directory.join(report::prepared_file_name(&prepared))
                    }
                };
                write_bundle(&path, &prepared.archive)?;
                Some(path)
            };
            let shown = report::preview(&prepared, receiver.as_deref(), saved.as_deref());
            if json {
                print_json(&shown)
            } else {
                print!("{}", report::render_preview(&shown));
                Ok(())
            }
        }
        ReportCommand::Send {
            to,
            yes,
            json,
            file,
            places,
        } => send(host, &to, yes, json, &file, &places),
        ReportCommand::List { json, places } => {
            let directory = places.state_dir.clone().or_else(report::default_state_dir);
            let list = report::list_reports(directory.as_deref())?;
            if json {
                print_json(&list)
            } else {
                print!("{}", report::render_list(&list));
                Ok(())
            }
        }
        ReportCommand::Status {
            report_id,
            json,
            places,
        } => status(host, &report_id, json, &places),
    }
}

/// Shows what is about to be sent, briefly; the full list is what
/// `report prepare` printed.
fn describe(inspected: &InspectedReport, receiver: &str) -> String {
    let report = &inspected.report;
    format!(
        "About to send this problem report to {receiver}:\n\
         \x20 title:      {}\n\
         \x20 kind:       {}\n\
         \x20 component:  {}\n\
         \x20 error code: {}\n\
         \x20 bundle:     {} files, {} bytes, SHA-256 {}\n\
         Every file is listed by 'tur-diag report prepare'; any file can be read with\n\
         \x20 tar --zstd -xOf <file> <path>\n\
         The receiver removes personal data again, files a short summary for TUR's\n\
         developers in a private tracker, keeps the bundle encrypted for {} days and\n\
         then deletes it.\n",
        report.title,
        report.kind.as_str(),
        report.component,
        report.error_code.as_deref().unwrap_or("none"),
        inspected.members.len(),
        inspected.bytes,
        inspected.sha256,
        report::RETENTION_DAYS,
    )
}

/// Asks the person to type `yes`, unless `--yes` was given. Without a
/// terminal to ask on, sending needs `--yes`.
fn confirm(yes: bool, receiver: &str) -> Result<(), DiagError> {
    if yes {
        return Ok(());
    }
    let refused = |detail: &str| DiagError::new(ErrorKind::ReportNotConfirmed, detail.to_owned());
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        return Err(refused(
            "there is no terminal to ask on; read what will be sent, then add --yes",
        ));
    }
    eprint!("Send this report to {receiver}? Type yes to send: ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    stdin
        .lock()
        .read_line(&mut answer)
        .map_err(|_| refused("the answer could not be read"))?;
    if answer.trim().eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        Err(refused("the answer was not yes"))
    }
}

fn send(
    host: &SystemHost,
    to: &str,
    yes: bool,
    json: bool,
    file: &Path,
    places: &Places,
) -> Result<(), DiagError> {
    let config = report::load_config(places.config.as_deref())?;
    let source = config
        .source
        .as_ref()
        .map_or_else(|| "no configuration file".to_owned(), |path| path.display().to_string());
    let Some(receiver) = config.receiver_url else {
        return Err(DiagError::new(
            ErrorKind::ReportNoReceiver,
            format!("RECEIVER_URL is empty ({source})"),
        ));
    };
    let to = report::normalise_receiver_url(to).map_err(|problem| {
        DiagError::new(ErrorKind::ReportWrongReceiver, format!("--to {problem}"))
    })?;
    if to != receiver {
        return Err(DiagError::new(
            ErrorKind::ReportWrongReceiver,
            format!("--to is not the RECEIVER_URL of {source}"),
        ));
    }
    let archive = fs::read(file).map_err(|error| {
        DiagError::new(
            ErrorKind::BundleRead,
            format!("{}: {error}", file.display()),
        )
    })?;
    let inspected = report::inspect_archive(&archive)?;
    // The record must be writable before anything is sent, so that a sent
    // report is never missing from the list.
    let directory = state_dir(places)?;
    report::ensure_private_dir(&directory.join(SENT_DIR))?;

    if json {
        eprint!("{}", describe(&inspected, &receiver));
    } else {
        print!("{}", describe(&inspected, &receiver));
    }
    confirm(yes, &receiver)?;

    let receipt = report::send_prepared(host, &receiver, file)?;
    let record = SentRecord {
        schema_version: 1,
        report_id: receipt.report_id.clone(),
        sent_utc: format_utc(now()),
        receiver: receiver.clone(),
        status_url: receipt.status_url.clone(),
        title: inspected.report.title.clone(),
        kind: inspected.report.kind,
        component: inspected.report.component.clone(),
        error_code: inspected.report.error_code.clone(),
        bundle_sha256: inspected.sha256.clone(),
        bundle_bytes: inspected.bytes,
        state: receipt.state.clone(),
        fixed_in: None,
        state_checked_utc: None,
    };
    report::write_record(&directory, &record)?;

    // A prepared file in the reports directory has done its job; a file
    // elsewhere belongs to the person and is left alone.
    let prepared_dir = directory.join(PREPARED_DIR);
    let in_prepared = match (file.canonicalize(), prepared_dir.canonicalize()) {
        (Ok(file), Ok(prepared)) => file.parent() == Some(prepared.as_path()),
        _ => false,
    };
    if in_prepared {
        let _ = fs::remove_file(file);
    }

    if json {
        print_json(&ListedReport::from(&record))
    } else {
        println!(
            "Sent. The receiver gave it the identifier {} and its state is {}.",
            record.report_id, record.state
        );
        println!(
            "It keeps the bundle encrypted for {} days, then deletes it.",
            receipt.retention_days
        );
        println!(
            "Check its state with: tur-diag report status {}",
            record.report_id
        );
        Ok(())
    }
}

fn status(host: &SystemHost, report_id: &str, json: bool, places: &Places) -> Result<(), DiagError> {
    let directory = state_dir(places)?;
    let (records, _) = report::read_records(&directory)?;
    let Some(mut record) = records
        .into_iter()
        .find(|record| record.report_id == report_id)
    else {
        return Err(DiagError::new(
            ErrorKind::ReportUnknown,
            format!("no record named {report_id}"),
        ));
    };
    let state = report::fetch_state(host, &record)?;
    record.state = state.state;
    record.fixed_in = state.fixed_in;
    record.state_checked_utc = Some(format_utc(now()));
    report::write_record(&directory, &record)?;
    let listed = ListedReport::from(&record);
    if json {
        print_json(&listed)
    } else {
        let state = match &listed.fixed_in {
            Some(version) => format!("fixed in {version}"),
            None => listed.state.clone(),
        };
        println!("{}: {state}", listed.report_id);
        Ok(())
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
        let cli = Cli::try_parse_from(["tur-diag", "status", "--json"]).expect("status parses");
        assert!(matches!(cli.command, Command::Status { json: true }));

        let cli = Cli::try_parse_from([
            "tur-diag",
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

        let cli = Cli::try_parse_from(["tur-diag", "collect", "--output", "b.tar.zst"])
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

        let cli = Cli::try_parse_from(["tur-diag", "verify-bundle", "b.tar.zst"])
            .expect("verify-bundle parses");
        assert!(matches!(cli.command, Command::VerifyBundle { .. }));
    }

    #[test]
    fn report_subcommands_parse_as_documented() {
        let cli = Cli::try_parse_from([
            "tur-diag",
            "report",
            "prepare",
            "--preview",
            "--json",
            "--prepared-by=pax",
            "--title=No sound",
            "--component=audio",
            "--error-code=TUR-AUDIO-RESUME-001",
        ])
        .expect("prepare parses");
        match cli.command {
            Command::Report {
                command:
                    ReportCommand::Prepare {
                        preview,
                        json,
                        prepared_by,
                        title,
                        component,
                        output,
                        ..
                    },
            } => {
                assert!(preview && json);
                assert_eq!(prepared_by, PreparedByArg::Pax);
                assert_eq!(title.as_deref(), Some("No sound"));
                assert_eq!(component.as_deref(), Some("audio"));
                assert_eq!(output, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let cli = Cli::try_parse_from([
            "tur-diag",
            "report",
            "send",
            "--to",
            "https://reports.tur.example",
            "--yes",
            "r.tar.zst",
        ])
        .expect("send parses");
        match cli.command {
            Command::Report {
                command: ReportCommand::Send { to, yes, file, .. },
            } => {
                assert_eq!(to, "https://reports.tur.example");
                assert!(yes);
                assert_eq!(file, PathBuf::from("r.tar.zst"));
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let cli = Cli::try_parse_from(["tur-diag", "report", "list", "--json"]).expect("list");
        assert!(matches!(
            cli.command,
            Command::Report {
                command: ReportCommand::List { json: true, .. }
            }
        ));
        let cli = Cli::try_parse_from(["tur-diag", "report", "status", "R-0123456789abcdef"])
            .expect("status");
        assert!(matches!(
            cli.command,
            Command::Report {
                command: ReportCommand::Status { .. }
            }
        ));
    }

    #[test]
    fn invalid_command_lines_are_rejected() {
        assert!(Cli::try_parse_from(["tur-diag"]).is_err());
        assert!(Cli::try_parse_from(["tur-diag", "collect"]).is_err());
        assert!(
            Cli::try_parse_from(["tur-diag", "collect", "--output", "x", "--profile", "all"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["tur-diag", "verify-bundle"]).is_err());
        assert!(Cli::try_parse_from(["tur-diag", "upload"]).is_err());
        // Sending names its receiver and its file.
        assert!(Cli::try_parse_from(["tur-diag", "report", "send", "r.tar.zst"]).is_err());
        assert!(
            Cli::try_parse_from(["tur-diag", "report", "send", "--to", "https://x.example"])
                .is_err()
        );
        // A preview saves nothing, so it has no output file.
        assert!(
            Cli::try_parse_from([
                "tur-diag", "report", "prepare", "--preview", "--output", "x", "--title", "t",
                "--component", "audio"
            ])
            .is_err()
        );
        // A file of boot health's status only makes sense with --from-boot-health.
        assert!(
            Cli::try_parse_from(["tur-diag", "report", "prepare", "--boot-health-status", "s"])
                .is_err()
        );
        // There is no way to send in the background or automatically.
        assert!(Cli::try_parse_from(["tur-diag", "report", "send", "--background"]).is_err());
        assert!(Cli::try_parse_from(["tur-diag", "report", "auto-send"]).is_err());
    }
}
