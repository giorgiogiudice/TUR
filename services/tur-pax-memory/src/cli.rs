// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The command-line interface: the user's view of their memory and schedules.
//!
//! ```text
//! tur-pax-memory [--store <dir>] list [--kind K] [--state S] [--tag T]
//!                                     [--application A] [--contains TEXT] [--json]
//! tur-pax-memory [--store <dir>] show <id> [--json]
//! tur-pax-memory [--store <dir>] pending [--json]
//! tur-pax-memory [--store <dir>] confirm <id>
//! tur-pax-memory [--store <dir>] switch on|off
//! tur-pax-memory [--store <dir>] add --stated <text> [--tag T]...
//! tur-pax-memory [--store <dir>] forget <id>
//! tur-pax-memory [--store <dir>] export
//! tur-pax-memory [--store <dir>] schedules list [--json]
//! tur-pax-memory [--store <dir>] schedules next [<id>] [--now SECONDS] [--json]
//! tur-pax-memory [--store <dir>] schedules show <id> [--units] [--now SECONDS] [--json]
//! ```
//!
//! The program reads, and it does what is the user's to do with memory: it
//! can forget an entry, confirm a pending one, turn memory on or off, and
//! record something the user states. Those four are acts of the person who
//! runs the program, for the one thing named on its command line; in the
//! finished system the same decisions are taken in the settings interface.
//! `add` records a *stated* entry and nothing else: an observed entry, a
//! procedure and an entry taken from content are the assistant's to record,
//! and this program is not the assistant. The program cannot create, change
//! or approve a schedule.
//!
//! Exit status: 0 on success; 1 when the command failed, with the error code
//! and a sentence on standard error; 2 for a usage error; 3 when the command
//! completed but damaged files were found in the store, each reported on
//! standard error.
//!
//! Text from the store is printed with control characters replaced, so an
//! entry cannot send escape sequences to the terminal.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use crate::codes::{Code, Problem};
use crate::confirm::UserConfirmation;
use crate::entry::{EntryKind, EntryState, MemoryEntry, NewEntry};
use crate::fs::StdFs;
use crate::ids::{EntryId, ScheduleId};
use crate::memory::{DamagedFile, Filter, Memory, MemorySwitch};
use crate::schedule::{Schedule, ScheduleBook, ScheduledAction};
use crate::store::FileStore;
use crate::systemd::{UnitOptions, render_units};
use crate::time::{Clock, Timestamp};
use crate::trigger::NextRun;

/// Exit status: the command succeeded.
pub const EXIT_OK: u8 = 0;
/// Exit status: the command failed. Nothing was changed unless the message
/// says so.
pub const EXIT_ERROR: u8 = 1;
/// Exit status: the command completed, and damaged files were found.
pub const EXIT_DAMAGED: u8 = 3;

/// The origin recorded for an entry the user stated on the command line.
pub const STATED_ORIGIN: &str = "stated by the user with tur-pax-memory add";

/// P@x memory, reminders and scheduled tasks: read, review and forget.
#[derive(Debug, Parser)]
#[command(name = "tur-pax-memory", version)]
pub struct Cli {
    /// The store directory. Without it, $XDG_DATA_HOME/tur/pax is used, or
    /// $HOME/.local/share/tur/pax.
    #[arg(long, global = true, value_name = "DIR")]
    pub store: Option<PathBuf>,
    /// What to do.
    #[command(subcommand)]
    pub command: TopCommand,
}

impl Cli {
    /// The entry the command line asks to confirm, if it is a `confirm`
    /// command.
    pub fn confirm_target(&self) -> Option<&str> {
        match &self.command {
            TopCommand::Confirm { id } => Some(id.as_str()),
            _ => None,
        }
    }

    /// The state the command line asks the memory switch to be put in, if
    /// it is a `switch` command that names one.
    pub fn switch_target(&self) -> Option<bool> {
        match &self.command {
            TopCommand::Switch { state } => parse_switch(state).ok(),
            _ => None,
        }
    }
}

/// The top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum TopCommand {
    /// List memory entries.
    List {
        /// Only this kind: stated, observed, procedure or from_content.
        #[arg(long)]
        kind: Option<String>,
        /// Only this state: active or pending.
        #[arg(long)]
        state: Option<String>,
        /// Only entries with this tag.
        #[arg(long)]
        tag: Option<String>,
        /// Only entries about this application.
        #[arg(long)]
        application: Option<String>,
        /// Only entries whose text contains this, without regard to case.
        #[arg(long)]
        contains: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show one memory entry.
    Show {
        /// The entry identifier.
        id: String,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Forget one memory entry: remove it from the store.
    Forget {
        /// The entry identifier.
        id: String,
    },
    /// Print everything in the store as one JSON document.
    Export,
    /// List the entries taken from content that wait for confirmation.
    Pending {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Confirm a pending entry, so that it is kept and can be recalled.
    Confirm {
        /// The entry identifier.
        id: String,
    },
    /// Turn memory on or off. It is off until the user turns it on. Turning
    /// it off deletes nothing.
    Switch {
        /// `on` or `off`.
        state: String,
    },
    /// Remember something the user states. Refused while memory is off, and
    /// for a text that looks like a secret.
    Add {
        /// The text to remember, as the user states it.
        #[arg(long, value_name = "TEXT")]
        stated: String,
        /// A tag for the entry. May be given more than once.
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<String>,
    },
    /// Reminders and scheduled tasks.
    Schedules {
        /// What to do with schedules.
        #[command(subcommand)]
        command: ScheduleCommand,
    },
}

/// The `schedules` subcommands.
#[derive(Debug, Subcommand)]
pub enum ScheduleCommand {
    /// List the schedules with their next run.
    List {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Say what is due now and when each schedule runs next.
    Next {
        /// Only this schedule.
        id: Option<String>,
        /// Use this time, in seconds since the Unix epoch, instead of the
        /// clock.
        #[arg(long, value_name = "SECONDS")]
        now: Option<u64>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show one schedule with its standing approvals and recent runs.
    Show {
        /// The schedule identifier.
        id: String,
        /// Also print the systemd units that would be installed. Nothing is
        /// installed.
        #[arg(long)]
        units: bool,
        /// Use this time, in seconds since the Unix epoch, instead of the
        /// clock.
        #[arg(long, value_name = "SECONDS")]
        now: Option<u64>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
}

/// What the program is given by its `main`: the clock, the user and the
/// default store directory. The library reads none of them itself.
pub struct Environment<'a> {
    /// The clock.
    pub clock: &'a dyn Clock,
    /// The confirmation of the person running the command, used by `confirm`.
    /// The library cannot make one; `main` does.
    pub user: &'a dyn UserConfirmation<MemoryEntry>,
    /// The same person's confirmation of the memory switch, used by
    /// `switch`. Also made by `main`, for the state named on the command
    /// line and no other.
    pub switch: &'a dyn UserConfirmation<MemorySwitch>,
    /// The store directory to use when `--store` is not given.
    pub default_store: Option<PathBuf>,
}

// Either a refusal to report to the user, or a failure to write the output.
enum Failure {
    Problem(Problem),
    Io(io::Error),
}

impl From<Problem> for Failure {
    fn from(problem: Problem) -> Failure {
        Failure::Problem(problem)
    }
}

impl From<io::Error> for Failure {
    fn from(err: io::Error) -> Failure {
        Failure::Io(err)
    }
}

/// Run a command. Returns the exit status; an `Err` means the output could
/// not be written.
pub fn run(
    cli: &Cli,
    environment: &Environment<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    let root = cli
        .store
        .clone()
        .or_else(|| environment.default_store.clone());
    let Some(root) = root else {
        let problem = Problem::new(
            Code::InputInvalid,
            "no store directory: pass --store, or set XDG_DATA_HOME or HOME",
        );
        writeln!(err, "{problem}")?;
        return Ok(EXIT_ERROR);
    };
    match execute(cli, environment, &root, out, err) {
        Ok(status) => Ok(status),
        Err(Failure::Problem(problem)) => {
            writeln!(err, "{problem}")?;
            Ok(EXIT_ERROR)
        }
        Err(Failure::Io(failure)) => Err(failure),
    }
}

fn execute(
    cli: &Cli,
    environment: &Environment<'_>,
    root: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Failure> {
    let mut memory = Memory::new(FileStore::new(root, StdFs));
    let book = ScheduleBook::new(FileStore::new(root, StdFs));
    match &cli.command {
        TopCommand::List {
            kind,
            state,
            tag,
            application,
            contains,
            json,
        } => {
            let filter = Filter {
                kind: kind.as_deref().map(parse_kind).transpose()?,
                state: state.as_deref().map(parse_state).transpose()?,
                tag: tag.clone(),
                application: application.clone(),
                contains: contains.clone(),
            };
            list_entries(&memory, &filter, *json, out, err)
        }
        TopCommand::Pending { json } => {
            let filter = Filter {
                state: Some(EntryState::Pending),
                ..Filter::default()
            };
            list_entries(&memory, &filter, *json, out, err)
        }
        TopCommand::Show { id, json } => {
            let entry = memory.get(&entry_id(id)?)?;
            if *json {
                writeln!(out, "{}", pretty(&entry))?;
            } else {
                show_entry(&entry, out)?;
            }
            Ok(EXIT_OK)
        }
        TopCommand::Forget { id } => {
            let id = entry_id(id)?;
            memory.forget(&id)?;
            writeln!(out, "forgot {id}")?;
            Ok(EXIT_OK)
        }
        TopCommand::Confirm { id } => {
            let id = entry_id(id)?;
            memory.confirm(&id, environment.user)?;
            writeln!(out, "confirmed {id}")?;
            Ok(EXIT_OK)
        }
        TopCommand::Switch { state } => {
            let enabled = parse_switch(state)?;
            memory.set_enabled(MemorySwitch { enabled }, environment.switch)?;
            writeln!(out, "memory is {}", if enabled { "on" } else { "off" })?;
            Ok(EXIT_OK)
        }
        TopCommand::Add { stated, tags } => {
            // The person running the program states it: that is what a
            // stated entry is. The library refuses it while memory is off
            // and when the text looks like a secret.
            let mut new = NewEntry::stated(stated, STATED_ORIGIN);
            new.tags.clone_from(tags);
            let entry = memory.add(new, environment.clock.now())?;
            writeln!(out, "added {}", entry.id())?;
            Ok(EXIT_OK)
        }
        TopCommand::Export => {
            let memory_part = memory.export()?;
            let schedule_part = book.export()?;
            let damaged = count(&memory_part["damaged"]) + count(&schedule_part["damaged"]);
            let document = json!({
                "schema_version": 1,
                "memory": memory_part,
                "schedules": schedule_part,
            });
            writeln!(out, "{}", pretty(&document))?;
            if damaged > 0 {
                writeln!(
                    err,
                    "warning: {damaged} damaged file(s) are listed in the export under \"damaged\""
                )?;
                return Ok(EXIT_DAMAGED);
            }
            Ok(EXIT_OK)
        }
        TopCommand::Schedules { command } => match command {
            ScheduleCommand::List { json } => {
                let now = environment.clock.now();
                list_schedules(&book, None, now, *json, false, out, err)
            }
            ScheduleCommand::Next { id, now, json } => {
                let only = id.as_deref().map(schedule_id).transpose()?;
                let now = now.map_or_else(|| environment.clock.now(), Timestamp);
                list_schedules(&book, only.as_ref(), now, *json, true, out, err)
            }
            ScheduleCommand::Show {
                id,
                units,
                now,
                json,
            } => {
                let id = schedule_id(id)?;
                let now = now.map_or_else(|| environment.clock.now(), Timestamp);
                show_schedule(&book, &id, now, *units, *json, out)
            }
        },
    }
}

fn parse_kind(text: &str) -> Result<EntryKind, Problem> {
    EntryKind::parse(text).ok_or_else(|| {
        Problem::new(
            Code::InputInvalid,
            "--kind is one of stated, observed, procedure, from_content",
        )
    })
}

fn parse_state(text: &str) -> Result<EntryState, Problem> {
    EntryState::parse(text)
        .ok_or_else(|| Problem::new(Code::InputInvalid, "--state is active or pending"))
}

fn entry_id(text: &str) -> Result<EntryId, Problem> {
    EntryId::parse(text).ok_or_else(|| {
        Problem::new(
            Code::MemoryInvalidId,
            "a memory identifier looks like m-0000002a",
        )
    })
}

fn schedule_id(text: &str) -> Result<ScheduleId, Problem> {
    ScheduleId::parse(text).ok_or_else(|| {
        Problem::new(
            Code::ScheduleInvalidId,
            "a schedule identifier looks like s-00000003",
        )
    })
}

fn count(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn pretty<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_owned())
}

// Characters a terminal acts on rather than shows: control characters, and the
// bidirectional overrides and isolates, which reorder what is displayed.
fn acts_on_terminal(c: char) -> bool {
    c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Text made safe for a terminal: control characters other than a line break,
/// and bidirectional overrides, become spaces.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            if acts_on_terminal(c) && c != '\n' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Text on one line, at most `max_chars` characters, safe for a terminal.
pub fn one_line(text: &str, max_chars: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if acts_on_terminal(c) { ' ' } else { c })
        .collect();
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let mut cut: String = flat.chars().take(max_chars.saturating_sub(3)).collect();
    cut.push_str("...");
    cut
}

fn warn_damaged(code: Code, damaged: &[DamagedFile], err: &mut dyn Write) -> io::Result<()> {
    for file in damaged {
        writeln!(
            err,
            "warning: {code}: {}: {}",
            one_line(&file.id, 80),
            one_line(&file.reason, 200)
        )?;
    }
    Ok(())
}

fn list_entries(
    memory: &Memory<FileStore<StdFs>>,
    filter: &Filter,
    as_json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Failure> {
    let listing = memory.list(filter)?;
    if as_json {
        let document = json!({
            "entries": serde_json::to_value(&listing.entries).unwrap_or(Value::Null),
            "damaged": serde_json::to_value(&listing.damaged).unwrap_or(Value::Null),
        });
        writeln!(out, "{}", pretty(&document))?;
    } else {
        for entry in &listing.entries {
            writeln!(
                out,
                "{}  {:<7}  {:<12}  {}  {}",
                entry.id(),
                entry.state().as_str(),
                entry.kind().as_str(),
                entry.updated_at(),
                one_line(entry.text(), 80)
            )?;
        }
    }
    warn_damaged(Code::MemoryDamaged, &listing.damaged, err)?;
    Ok(if listing.damaged.is_empty() {
        EXIT_OK
    } else {
        EXIT_DAMAGED
    })
}

// The state a `switch` command names.
fn parse_switch(state: &str) -> Result<bool, Problem> {
    match state {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(Problem::new(
            Code::InputInvalid,
            "the memory switch is \"on\" or \"off\"",
        )),
    }
}

fn show_entry(entry: &MemoryEntry, out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "id: {}", entry.id())?;
    writeln!(out, "kind: {}", entry.kind().as_str())?;
    writeln!(out, "trust: {}", entry.trust().as_str())?;
    writeln!(out, "state: {}", entry.state().as_str())?;
    writeln!(out, "created: {}", entry.created_at())?;
    writeln!(out, "updated: {}", entry.updated_at())?;
    if let Some(confirmed) = entry.confirmed_at() {
        writeln!(out, "confirmed: {confirmed}")?;
    }
    let provenance = entry.provenance();
    writeln!(out, "source: {}", one_line(&provenance.origin, 600))?;
    if let Some(task) = &provenance.task {
        writeln!(out, "task: {}", one_line(task, 200))?;
    }
    if let Some(application) = entry.application() {
        writeln!(out, "application: {}", one_line(application, 300))?;
    }
    if !entry.tags().is_empty() {
        let tags: Vec<String> = entry.tags().iter().map(|tag| one_line(tag, 80)).collect();
        writeln!(out, "tags: {}", tags.join(", "))?;
    }
    writeln!(out, "text:")?;
    writeln!(out, "{}", printable(entry.text()))?;
    if let Some(procedure) = entry.procedure() {
        writeln!(out, "procedure: {}", one_line(&procedure.name, 200))?;
        for (index, step) in procedure.steps.iter().enumerate() {
            writeln!(
                out,
                "  {}. {} {} {}",
                index + 1,
                one_line(&step.service, 300),
                one_line(&step.tool, 80),
                Value::Object(step.arguments.clone())
            )?;
        }
    }
    Ok(())
}

fn describe_next(next: &NextRun) -> String {
    match next {
        NextRun::Due {
            scheduled_for,
            late,
            next,
        } => {
            let timing = if *late { "late" } else { "on time" };
            let after = next.map_or_else(
                || "no further run".to_owned(),
                |next| format!("then {next}"),
            );
            format!("due now (scheduled for {scheduled_for}, {timing}); {after}")
        }
        NextRun::Wait {
            next,
            skipped_missed,
        } => {
            let note = if *skipped_missed {
                " (missed runs were skipped)"
            } else {
                ""
            };
            format!("next run {next}{note}")
        }
        NextRun::OnEvent => "runs on its event".to_owned(),
        NextRun::Finished { skipped_missed } => {
            if *skipped_missed {
                "finished (its run was missed and skipped)".to_owned()
            } else {
                "finished".to_owned()
            }
        }
    }
}

fn kind_of(schedule: &Schedule) -> &'static str {
    if schedule.is_reminder() {
        "reminder"
    } else {
        "task"
    }
}

fn list_schedules(
    book: &ScheduleBook<FileStore<StdFs>>,
    only: Option<&ScheduleId>,
    now: Timestamp,
    as_json: bool,
    next_only: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Failure> {
    let listing = book.list()?;
    if let Some(wanted) = only
        && !listing
            .schedules
            .iter()
            .any(|schedule| schedule.id() == wanted)
    {
        // Reports a damaged file as such, and a missing one as not found.
        book.get(wanted)?;
    }
    let mut rows = Vec::new();
    for schedule in &listing.schedules {
        if only.is_some_and(|wanted| wanted != schedule.id()) {
            continue;
        }
        let last_run = book.last_run(schedule.id())?;
        let next = schedule.next_run(last_run, now)?;
        if as_json {
            rows.push(if next_only {
                json!({ "id": schedule.id(), "next_run": next })
            } else {
                json!({ "schedule": schedule, "last_run": last_run, "next_run": next })
            });
        } else if next_only {
            writeln!(out, "{}  {}", schedule.id(), describe_next(&next))?;
        } else {
            writeln!(
                out,
                "{}  {:<8}  {}  approvals {}  {}  {}",
                schedule.id(),
                kind_of(schedule),
                describe_next(&next),
                schedule.standing_approvals().len(),
                schedule.trigger().describe(),
                one_line(schedule.name(), 80)
            )?;
        }
    }
    if as_json {
        writeln!(out, "{}", pretty(&rows))?;
    }
    warn_damaged(Code::ScheduleDamaged, &listing.damaged, err)?;
    Ok(if listing.damaged.is_empty() {
        EXIT_OK
    } else {
        EXIT_DAMAGED
    })
}

fn show_schedule(
    book: &ScheduleBook<FileStore<StdFs>>,
    id: &ScheduleId,
    now: Timestamp,
    with_units: bool,
    as_json: bool,
    out: &mut dyn Write,
) -> Result<u8, Failure> {
    let schedule = book.get(id)?;
    let last_run = book.last_run(id)?;
    let next = schedule.next_run(last_run, now)?;
    let runs = book.runs(Some(id))?.runs;
    let following = match next {
        NextRun::Due { next, .. } => next,
        NextRun::Wait { next, .. } => Some(next),
        NextRun::OnEvent | NextRun::Finished { .. } => None,
    };
    let units = if with_units {
        Some(render_units(&schedule, following, &UnitOptions::default())?)
    } else {
        None
    };

    if as_json {
        let mut document = json!({
            "schedule": schedule,
            "last_run": last_run,
            "next_run": next,
            "runs": serde_json::to_value(&runs).unwrap_or(Value::Null),
        });
        if let Some(units) = &units {
            document["units"] = json!({
                "service": { "name": units.service.name, "contents": units.service.contents },
                "timer": units.timer.as_ref().map(|timer| {
                    json!({ "name": timer.name, "contents": timer.contents })
                }),
            });
        }
        writeln!(out, "{}", pretty(&document))?;
        return Ok(EXIT_OK);
    }

    writeln!(out, "id: {}", schedule.id())?;
    writeln!(out, "name: {}", one_line(schedule.name(), 100))?;
    writeln!(out, "kind: {}", kind_of(&schedule))?;
    match schedule.action() {
        ScheduledAction::Reminder { message } => {
            writeln!(out, "message: {}", one_line(message, 2100))?;
        }
        ScheduledAction::Task { request, .. } => {
            writeln!(out, "request: {}", one_line(request, 2100))?;
        }
    }
    writeln!(out, "trigger: {}", schedule.trigger().describe())?;
    writeln!(out, "missed runs: {}", schedule.missed_runs().as_str())?;
    writeln!(out, "created: {}", schedule.created_at())?;
    writeln!(out, "confirmed by the user: {}", schedule.confirmed_at())?;
    if let Some(task) = schedule.proposed_by_task() {
        writeln!(out, "proposed during task: {}", one_line(task, 200))?;
    }
    writeln!(
        out,
        "standing approvals: {}",
        schedule.standing_approvals().len()
    )?;
    for approval in schedule.standing_approvals() {
        writeln!(
            out,
            "  {} {} {}",
            one_line(approval.service(), 300),
            one_line(approval.tool(), 80),
            Value::Object(approval.arguments().clone())
        )?;
    }
    match last_run {
        Some(last) => writeln!(out, "last run: {last}")?,
        None => writeln!(out, "last run: never")?,
    }
    writeln!(out, "now: {}", describe_next(&next))?;
    writeln!(out, "run records: {}", runs.len())?;
    for run in &runs {
        let outcome = serde_json::to_value(run.outcome).unwrap_or(Value::Null);
        writeln!(
            out,
            "  {}  {}  {}  performed {}  deferred {}{}",
            run.id,
            run.finished_at,
            outcome.as_str().unwrap_or("?"),
            run.performed.len(),
            run.deferred.len(),
            if run.deferred.is_empty() || run.presented_at.is_some() {
                ""
            } else {
                "  (not yet shown to the user)"
            }
        )?;
    }
    if let Some(units) = &units {
        writeln!(out, "--- {} (not installed)", units.service.name)?;
        write!(out, "{}", units.service.contents)?;
        if let Some(timer) = &units.timer {
            writeln!(out, "--- {} (not installed)", timer.name)?;
            write!(out, "{}", timer.contents)?;
        }
    }
    Ok(EXIT_OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_text_has_no_control_characters() {
        assert_eq!(printable("a\u{1b}[31mb\nc\td"), "a [31mb\nc d");
        assert_eq!(one_line("a\u{1b}[31mb\nc", 80), "a [31mb c");
        assert_eq!(one_line("abcdefghij", 10), "abcdefghij");
        assert_eq!(one_line("abcdefghijk", 10), "abcdefg...");
        assert_eq!(one_line("abc", 0), "...");
        // A right-to-left override would show the rest of the line reversed.
        assert_eq!(one_line("a\u{202e}b\u{2066}c", 80), "a b c");
        assert_eq!(printable("a\u{202e}b"), "a b");
    }

    #[test]
    fn the_command_line_parses() {
        let cli = Cli::try_parse_from(["tur-pax-memory", "--store", "/s", "confirm", "m-00000001"])
            .expect("parses");
        assert_eq!(cli.store, Some(PathBuf::from("/s")));
        assert_eq!(cli.confirm_target(), Some("m-00000001"));
        // --store is accepted after the subcommand too.
        let cli = Cli::try_parse_from([
            "tur-pax-memory",
            "schedules",
            "next",
            "s-00000001",
            "--now",
            "5",
            "--store",
            "/s",
        ])
        .expect("parses");
        assert_eq!(cli.store, Some(PathBuf::from("/s")));
        assert_eq!(cli.confirm_target(), None);
        assert!(Cli::try_parse_from(["tur-pax-memory", "remember", "x"]).is_err());
        assert!(Cli::try_parse_from(["tur-pax-memory", "schedules", "create"]).is_err());
    }

    #[test]
    fn next_runs_are_described_in_words() {
        assert_eq!(
            describe_next(&NextRun::Wait {
                next: Timestamp(0),
                skipped_missed: false
            }),
            "next run 1970-01-01T00:00:00Z"
        );
        assert_eq!(
            describe_next(&NextRun::Due {
                scheduled_for: Timestamp(0),
                late: true,
                next: None
            }),
            "due now (scheduled for 1970-01-01T00:00:00Z, late); no further run"
        );
        assert_eq!(describe_next(&NextRun::OnEvent), "runs on its event");
        assert_eq!(
            describe_next(&NextRun::Finished {
                skipped_missed: false
            }),
            "finished"
        );
    }
}
