// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-diag heal`: the command line of self-healing
//! (docs/architecture/self-healing.md). Only parsing, printing and the
//! socket's client are here; the logic is in `tur_diag::heal`.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use tur_diag::heal::engine::{
    DEFAULT_SOCKET, MAX_REQUEST_BYTES, REQUEST_SCHEMA_VERSION, uid_of_connection,
};
use tur_diag::heal::{
    Actor, ApplyRequest, Catalogue, Check, FaultState, Fixes, HealState, Lock, Machine, Outcome,
    Paths, RealMachine, Running, apply, handle_request, notices, scan, status_view, undo,
};
use tur_diag::report::{self, REPORT_SCHEMA_VERSION, ReportKind, ReportRequest};
use tur_diag::{DiagError, ErrorKind, SystemHost};

use crate::{Places, prepare_and_show, print_json};

/// Exit status of `apply` when the remedy ran and the check still finds
/// the fault.
const NOT_REPAIRED: u8 = 4;

/// Where self-healing keeps its files. For tests; the defaults are the
/// system's.
#[derive(Debug, Clone, Default, Args)]
pub(crate) struct HealPlaces {
    /// The state file instead of /var/log/tur/heal/state.json.
    #[arg(long = "heal-state", value_name = "FILE", hide = true)]
    state: Option<PathBuf>,
    /// The data directory instead of /var/lib/tur/heal.
    #[arg(long = "heal-data", value_name = "DIR", hide = true)]
    data: Option<PathBuf>,
    /// The list of fixed codes instead of /usr/share/tur/heal/fixes.json.
    #[arg(long, value_name = "FILE", hide = true)]
    fixes: Option<PathBuf>,
    /// Take every file a check or remedy reads or changes below this
    /// directory. Commands are still the computer's.
    #[arg(long, value_name = "DIR", hide = true)]
    root: Option<PathBuf>,
    /// The healing service's socket instead of /run/tur/heal.socket.
    #[arg(long, value_name = "PATH", hide = true)]
    socket: Option<PathBuf>,
}

impl HealPlaces {
    fn paths(&self) -> Paths {
        let defaults = Paths::default();
        Paths {
            state: self.state.clone().unwrap_or(defaults.state),
            data_dir: self.data.clone().unwrap_or(defaults.data_dir),
            fixes: self.fixes.clone().unwrap_or(defaults.fixes),
        }
    }

    fn machine(&self) -> RealMachine {
        match &self.root {
            Some(root) => RealMachine::below(root),
            None => RealMachine::new(),
        }
    }

    fn socket(&self) -> PathBuf {
        self.socket
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET))
    }
}

/// The subcommands of `tur-diag heal`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub(crate) enum HealCommand {
    /// Show the problems TUR has found on this computer and what was done
    /// about each. Reads only.
    Status {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        heal: HealPlaces,
    },
    /// List every problem TUR looks for, with its repairs. Reads only.
    Catalogue {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Explain one problem: what it is, what TUR can do about it, what that
    /// changes and how it is undone. Reads only.
    Explain {
        /// The problem's code, for example TUR-NETWORK-TIME-001.
        #[arg(value_name = "CODE")]
        code: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        heal: HealPlaces,
    },
    /// Look for every problem now. With --repair, apply the repairs TUR
    /// makes by itself. Run by tur-heal.service, as root.
    Scan {
        /// Apply the automatic repairs of the problems found.
        #[arg(long)]
        repair: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        heal: HealPlaces,
    },
    /// Apply a repair to a problem TUR found, and check that it worked.
    /// Without administrator rights the request goes to TUR's healing
    /// service, which accepts only repairs that can be undone.
    Apply {
        /// The problem's code.
        #[arg(value_name = "CODE")]
        code: String,
        /// The repair's name, as 'tur-diag heal explain' shows it.
        #[arg(value_name = "REMEDY")]
        remedy: String,
        /// Confirm a repair that cannot be undone without being asked.
        #[arg(long)]
        yes: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        heal: HealPlaces,
    },
    /// Undo a repair that was applied.
    Undo {
        /// The problem's code.
        #[arg(value_name = "CODE")]
        code: String,
        /// The repair's name.
        #[arg(value_name = "REMEDY")]
        remedy: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        heal: HealPlaces,
    },
    /// Answer one request read from standard input. Started by
    /// tur-heal.socket for each connection.
    Handle {
        /// The connection's instance name, as systemd gives it.
        #[arg(long, value_name = "NAME")]
        connection: Option<String>,
        #[command(flatten)]
        heal: HealPlaces,
    },
    /// Show what TUR has to tell you that you have not seen yet.
    Notices {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Remember them as seen.
        #[arg(long)]
        mark_shown: bool,
        #[command(flatten)]
        heal: HealPlaces,
    },
    /// Show what you have not seen yet as desktop notifications. Started by
    /// tur-heal-notify.service in your session.
    Notify {
        #[command(flatten)]
        heal: HealPlaces,
    },
    /// Prepare a problem report for a problem TUR found and offers a report
    /// for, and show exactly what it would contain. Nothing is sent: send
    /// it yourself with 'tur-diag report send'.
    Report {
        /// The problem's code.
        #[arg(value_name = "CODE")]
        code: String,
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
        #[command(flatten)]
        heal: HealPlaces,
    },
}

fn catalogue() -> Result<Catalogue, DiagError> {
    Catalogue::built_in().map_err(|problem| DiagError::new(ErrorKind::Internal, problem))
}

/// Whether this process runs with administrator rights.
fn is_administrator() -> bool {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                let ids = line.strip_prefix("Uid:")?;
                ids.split_whitespace().nth(1).map(|uid| uid == "0")
            })
        })
        .unwrap_or(false)
}

/// Runs a subcommand of `tur-diag heal`.
pub(crate) fn run(command: HealCommand) -> Result<ExitCode, DiagError> {
    match command {
        HealCommand::Status { json, heal } => {
            let paths = heal.paths();
            let state = HealState::load(&paths.state)?;
            let view = status_view(&catalogue()?, &state, &paths.state);
            if json {
                print_json(&view)?;
            } else {
                print!("{}", render_status(&view));
            }
            Ok(ExitCode::SUCCESS)
        }
        HealCommand::Catalogue { json } => {
            let catalogue = catalogue()?;
            if json {
                print_json(&catalogue)?;
            } else {
                for fault in &catalogue.faults {
                    println!("{}  {}", fault.code, fault.title);
                    for remedy in &fault.remedies {
                        println!(
                            "    {}  class {}{}",
                            remedy.id,
                            remedy.class,
                            if remedy.automatic { ", automatic" } else { "" }
                        );
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        HealCommand::Explain { code, json, heal } => explain(&code, json, &heal),
        HealCommand::Scan { repair, json, heal } => {
            let paths = heal.paths();
            let machine = heal.machine();
            let catalogue = catalogue()?;
            let _lock = Lock::take(&paths.data_dir)?;
            let mut state = HealState::load(&paths.state)?;
            let running = Running::read(&machine);
            let fixes = Fixes::load(&paths.fixes);
            let lines = scan(&machine, &catalogue, &paths, &mut state, &running, &fixes, repair);
            state.save(&paths.state)?;
            for line in &lines {
                if line.present || line.state.is_some() || line.unavailable.is_some() {
                    eprintln!("tur-heal: {}: {}", line.code, line.action);
                }
            }
            if json {
                print_json(&json!({ "state_file": paths.state, "checks": lines }))?;
            } else {
                for line in &lines {
                    println!("{}  {}", line.code, line.action);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        HealCommand::Apply {
            code,
            remedy,
            yes,
            json,
            heal,
        } => apply_command(&code, &remedy, yes, json, &heal),
        HealCommand::Undo {
            code,
            remedy,
            json,
            heal,
        } => {
            if is_administrator() || heal.root.is_some() {
                let paths = heal.paths();
                let machine = heal.machine();
                let catalogue = catalogue()?;
                let _lock = Lock::take(&paths.data_dir)?;
                let mut state = HealState::load(&paths.state)?;
                let outcome = undo(
                    &machine,
                    &catalogue,
                    &paths,
                    &mut state,
                    &code,
                    &remedy,
                    Actor::Administrator,
                    None,
                )?;
                state.save(&paths.state)?;
                show_outcome(&outcome, json)
            } else {
                request(&heal.socket(), "heal_undo_remedy", &code, &remedy, json)
            }
        }
        HealCommand::Handle { connection, heal } => {
            let mut input = Vec::new();
            io::stdin()
                .take(u64::try_from(MAX_REQUEST_BYTES).unwrap_or(u64::MAX) + 1)
                .read_to_end(&mut input)
                .map_err(|error| DiagError::new(ErrorKind::HealRequestInvalid, error.to_string()))?;
            let answer = handle_request(
                &heal.machine(),
                &catalogue()?,
                &heal.paths(),
                &input,
                connection.as_deref(),
            );
            let uid = connection
                .as_deref()
                .and_then(uid_of_connection)
                .map_or_else(|| "unknown".to_owned(), |uid| uid.to_string());
            eprintln!(
                "tur-heal: request from UID {uid}: {}",
                summary_of_answer(&answer)
            );
            println!("{answer}");
            Ok(ExitCode::SUCCESS)
        }
        HealCommand::Notices {
            json,
            mark_shown,
            heal,
        } => {
            let paths = heal.paths();
            let state = HealState::load(&paths.state)?;
            let mut shown = read_shown();
            let found = notices(&catalogue()?, &state, &shown, &reported_codes());
            if json {
                print_json(&found)?;
            } else if found.is_empty() {
                println!("Nothing new.");
            } else {
                for notice in &found {
                    println!("{} ({})\n  {}", notice.title, notice.code, notice.body);
                    for action in &notice.actions {
                        println!("  - {action}");
                    }
                }
            }
            if mark_shown {
                shown.extend(found.iter().map(|notice| notice.id));
                write_shown(&shown)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        HealCommand::Notify { heal } => {
            let paths = heal.paths();
            let state = HealState::load(&paths.state)?;
            let mut shown = read_shown();
            let found = notices(&catalogue()?, &state, &shown, &reported_codes());
            let machine = RealMachine::new();
            for notice in &found {
                let mut body = notice.body.clone();
                for action in &notice.actions {
                    body.push('\n');
                    body.push_str(action);
                }
                let title = format!("TUR: {}", notice.title);
                let delivered = machine.run(
                    "gdbus",
                    &[
                        "call",
                        "--session",
                        "--dest",
                        "org.freedesktop.Notifications",
                        "--object-path",
                        "/org/freedesktop/Notifications",
                        "--method",
                        "org.freedesktop.Notifications.Notify",
                        &gvariant_string("TUR"),
                        "0",
                        &gvariant_string("dialog-information"),
                        &gvariant_string(&title),
                        &gvariant_string(&body),
                        "[]",
                        "{}",
                        "15000",
                    ],
                );
                match delivered {
                    Ok(output) if output.success() => {
                        shown.insert(notice.id);
                    }
                    _ => eprintln!("tur-heal: notice {} could not be shown", notice.id),
                }
            }
            write_shown(&shown)?;
            Ok(ExitCode::SUCCESS)
        }
        HealCommand::Report {
            code,
            preview,
            output,
            json,
            places,
            heal,
        } => {
            let request = report_request(&code, &heal)?;
            prepare_and_show(
                &SystemHost::new(),
                &request,
                false,
                preview,
                output,
                json,
                &places,
            )?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// A text as a GVariant string literal, for `gdbus call`.
fn gvariant_string(text: &str) -> String {
    let mut out = String::from("'");
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            character if character.is_control() => {}
            character => out.push(character),
        }
    }
    out.push('\'');
    out
}

/// The file that remembers which notices this person has seen.
fn shown_file() -> Option<PathBuf> {
    report::default_state_dir().and_then(|reports| {
        reports
            .parent()
            .map(|tur| tur.join("heal").join("shown.json"))
    })
}

fn read_shown() -> BTreeSet<u64> {
    shown_file()
        .and_then(|path| fs::read(path).ok())
        .and_then(|text| serde_json::from_slice::<Value>(&text).ok())
        .and_then(|value| {
            value.get("shown").and_then(Value::as_array).map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_u64)
                    .collect::<BTreeSet<u64>>()
            })
        })
        .unwrap_or_default()
}

fn write_shown(shown: &BTreeSet<u64>) -> Result<(), DiagError> {
    let Some(path) = shown_file() else {
        return Ok(());
    };
    let unusable =
        |error: io::Error| DiagError::new(ErrorKind::HealStateUnavailable, error.to_string());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(unusable)?;
    }
    let newest: Vec<u64> = shown.iter().rev().take(1000).copied().collect();
    fs::write(&path, json!({ "shown": newest }).to_string()).map_err(unusable)
}

/// The codes of the reports this person has sent.
fn reported_codes() -> BTreeSet<String> {
    report::default_state_dir()
        .and_then(|directory| report::read_records(&directory).ok())
        .map(|(records, _)| {
            records
                .into_iter()
                .filter_map(|record| record.error_code)
                .collect()
        })
        .unwrap_or_default()
}

fn summary_of_answer(answer: &Value) -> String {
    if answer["ok"] == Value::Bool(true) {
        format!(
            "{} {} {}: {}",
            answer["tool"].as_str().unwrap_or_default(),
            answer["result"]["code"].as_str().unwrap_or_default(),
            answer["result"]["remedy"].as_str().unwrap_or_default(),
            answer["result"]["outcome"].as_str().unwrap_or_default()
        )
    } else if let Some(code) = answer["error"]["code"].as_str() {
        format!("refused with {code}")
    } else {
        format!(
            "{} {}: {}",
            answer["tool"].as_str().unwrap_or_default(),
            answer["result"]["code"].as_str().unwrap_or_default(),
            answer["result"]["outcome"].as_str().unwrap_or_default()
        )
    }
}

fn render_status(view: &tur_diag::heal::StatusView) -> String {
    let mut out = String::new();
    if view.faults.is_empty() {
        out.push_str("TUR has found no problem that needs attention.\n");
    }
    for fault in &view.faults {
        let record = &fault.record;
        out.push_str(&format!(
            "{} ({}): {:?}\n  {}\n",
            fault.title, record.code, record.state, fault.explanation
        ));
        for line in &record.evidence {
            out.push_str(&format!("  seen: {line}\n"));
        }
        for remedy in &fault.remedies {
            out.push_str(&format!(
                "  repair {} (class {}): {}\n    undo: {}\n",
                remedy.id, remedy.class, remedy.changes, remedy.undo
            ));
        }
        if record.state == FaultState::ReportOffered {
            out.push_str(&format!(
                "  a report is offered: tur-diag heal report {}\n",
                record.code
            ));
        }
    }
    if !view.resolved.is_empty() {
        out.push_str("Resolved recently:\n");
        for record in &view.resolved {
            out.push_str(&format!(
                "  {} {} ({})\n",
                record.resolved_utc.as_deref().unwrap_or_default(),
                record.code,
                record.resolved_how.as_deref().unwrap_or_default()
            ));
        }
    }
    out
}

fn explain(code: &str, json: bool, heal: &HealPlaces) -> Result<ExitCode, DiagError> {
    let catalogue = catalogue()?;
    let fault = catalogue
        .fault(code)
        .ok_or_else(|| DiagError::new(ErrorKind::HealUnknown, format!("no fault {code}")))?;
    // The record is shown when the state can be read; P@x's sandbox can.
    let record = HealState::load(&heal.paths().state)
        .ok()
        .and_then(|state| {
            state
                .latest(code)
                .map(|index| state.faults[index].clone())
        });
    if json {
        print_json(&json!({ "fault": fault, "record": record }))?;
    } else {
        println!("{} ({})\n\n{}\n", fault.title, fault.code, fault.explanation);
        for remedy in &fault.remedies {
            println!(
                "Repair {}: {}\n  class {}{}; undo: {}",
                remedy.id,
                remedy.changes,
                remedy.class,
                if remedy.automatic {
                    ", applied by TUR by itself"
                } else {
                    ""
                },
                remedy.undo
            );
        }
        if fault.remedies.is_empty() {
            println!("No repair is safe to try; TUR offers a report instead.");
        }
        if let Some(record) = record {
            println!("\nOn this computer: {:?} since {}", record.state, record.first_seen_utc);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn show_outcome(outcome: &Outcome, json: bool) -> Result<ExitCode, DiagError> {
    if json {
        print_json(outcome)?;
    } else {
        println!(
            "{}: {}: {}\n  {}\n  undo: {}",
            outcome.code, outcome.remedy, outcome.outcome, outcome.detail, outcome.undo
        );
    }
    Ok(if outcome.outcome == "repaired" || outcome.outcome == "undone" {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(NOT_REPAIRED)
    })
}

/// Asks the person to type `yes` for a repair that cannot be undone.
fn confirm_repair(yes: bool, what: &str) -> bool {
    if yes {
        return true;
    }
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        return false;
    }
    eprint!("{what}\nThis cannot be undone. Type yes to go ahead: ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    stdin.lock().read_line(&mut answer).is_ok() && answer.trim().eq_ignore_ascii_case("yes")
}

fn apply_command(
    code: &str,
    remedy_id: &str,
    yes: bool,
    json: bool,
    heal: &HealPlaces,
) -> Result<ExitCode, DiagError> {
    let catalogue = catalogue()?;
    let remedy = catalogue
        .fault(code)
        .and_then(|fault| fault.remedy(remedy_id))
        .ok_or_else(|| {
            DiagError::new(
                ErrorKind::HealUnknown,
                format!("{code} has no remedy {remedy_id}"),
            )
        })?;
    if !is_administrator() && heal.root.is_none() {
        if remedy.class > 1 {
            return Err(DiagError::new(
                ErrorKind::HealNeedsApproval,
                format!("{remedy_id} is class {}", remedy.class),
            ));
        }
        return request(&heal.socket(), "heal_apply_remedy", code, remedy_id, json);
    }
    let confirmed = remedy.class <= 1 || confirm_repair(yes, &remedy.changes);
    let paths = heal.paths();
    let machine = heal.machine();
    let _lock = Lock::take(&paths.data_dir)?;
    let mut state = HealState::load(&paths.state)?;
    let running = Running::read(&machine);
    let outcome = apply(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running,
        &ApplyRequest {
            code: code.to_owned(),
            remedy: remedy_id.to_owned(),
            actor: Actor::Administrator,
            uid: None,
            confirmed,
        },
    )?;
    state.save(&paths.state)?;
    show_outcome(&outcome, json)
}

/// Sends one request to the healing service and shows its answer.
fn request(
    socket: &Path,
    tool: &str,
    code: &str,
    remedy: &str,
    json: bool,
) -> Result<ExitCode, DiagError> {
    let unreachable = |detail: String| DiagError::new(ErrorKind::HealServiceUnreachable, detail);
    let mut stream = UnixStream::connect(socket)
        .map_err(|error| unreachable(format!("{}: {error}", socket.display())))?;
    let timeout = Some(Duration::from_secs(180));
    stream
        .set_read_timeout(timeout)
        .and_then(|()| stream.set_write_timeout(timeout))
        .map_err(|error| unreachable(error.to_string()))?;
    let line = json!({
        "schema_version": REQUEST_SCHEMA_VERSION,
        "tool": tool,
        "arguments": { "code": code, "remedy": remedy },
    });
    stream
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|()| stream.shutdown(std::net::Shutdown::Write))
        .map_err(|error| unreachable(error.to_string()))?;
    let mut answer = Vec::new();
    stream
        .take(1024 * 1024)
        .read_to_end(&mut answer)
        .map_err(|error| unreachable(error.to_string()))?;
    let answer: Value = serde_json::from_slice(&answer)
        .map_err(|error| unreachable(format!("the answer is not JSON: {error}")))?;
    if let Some(code) = answer["error"]["code"].as_str() {
        let kind = ErrorKind::ALL
            .into_iter()
            .find(|kind| kind.code_str() == code)
            .unwrap_or(ErrorKind::HealRequestInvalid);
        let detail = answer["error"]["detail"].as_str().unwrap_or_default();
        return Err(DiagError::new(kind, detail.to_owned()));
    }
    let outcome = &answer["result"];
    if json {
        print_json(outcome)?;
    } else {
        println!(
            "{}: {}: {}\n  {}\n  undo: {}",
            outcome["code"].as_str().unwrap_or_default(),
            outcome["remedy"].as_str().unwrap_or_default(),
            outcome["outcome"].as_str().unwrap_or_default(),
            outcome["detail"].as_str().unwrap_or_default(),
            outcome["undo"].as_str().unwrap_or_default()
        );
    }
    Ok(if answer["ok"] == Value::Bool(true) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(NOT_REPAIRED)
    })
}

/// The problem report for a fault TUR offers one for.
fn report_request(code: &str, heal: &HealPlaces) -> Result<ReportRequest, DiagError> {
    let catalogue = catalogue()?;
    let fault = catalogue
        .fault(code)
        .ok_or_else(|| DiagError::new(ErrorKind::HealUnknown, format!("no fault {code}")))?;
    let state = HealState::load(&heal.paths().state)?;
    let record = state
        .active(code)
        .map(|index| &state.faults[index])
        .filter(|record| record.report_offered || record.state == FaultState::ReportOffered)
        .ok_or_else(|| {
            DiagError::new(
                ErrorKind::HealNoReport,
                format!("{code} has no report offered now"),
            )
        })?;
    let mut component = fault.component.clone();
    let mut title = fault.title.clone();
    if let Check::TurProgramCrashed { components, .. } = &fault.check
        && let Some(program) = record.subjects.first()
    {
        if let Some(found) = components.get(program) {
            component.clone_from(found);
        }
        title = format!("{} crashed", record.subjects.join(", "));
    }
    let mut description = format!("{}\n\nWhat TUR saw:\n", fault.explanation);
    for line in &record.evidence {
        description.push_str(&format!("- {line}\n"));
    }
    if !record.applied.is_empty() {
        description.push_str("\nRepairs tried:\n");
        for applied in &record.applied {
            description.push_str(&format!(
                "- {} ({:?}): {}\n",
                applied.remedy, applied.by, applied.outcome
            ));
        }
    }
    description.push_str(&format!(
        "\nFirst seen {}, last seen {}.\n",
        record.first_seen_utc, record.last_seen_utc
    ));
    Ok(ReportRequest {
        schema_version: REPORT_SCHEMA_VERSION,
        kind: fault.report_kind.unwrap_or(ReportKind::Request),
        title: title.chars().take(report::MAX_TITLE_CHARS).collect(),
        description: description
            .chars()
            .take(report::MAX_DESCRIPTION_CHARS)
            .collect(),
        error_code: Some(code.to_owned()),
        component,
        affected_version: None,
        prepared_by: report::PreparedBy::Person,
        boot_health: None,
    })
}
