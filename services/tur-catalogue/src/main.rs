// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-catalogue`: the command-line front end of the application catalogue
//! and the router.
//!
//! ```text
//! tur-catalogue validate [dir]            load and validate a catalogue
//! tur-catalogue search <query>            find applications by name or alias
//! tur-catalogue show <id>                 print one application's entry
//! tur-catalogue route <id> [--host <facts.json>] [--prefs <prefs.json>] [--json]
//!                                         order the routes that can run it
//! ```
//!
//! No command installs, launches, fetches or modifies anything.
//!
//! Exit status:
//!
//! | Status | Meaning |
//! |---|---|
//! | 0 | Success: the catalogue is valid; the search found something; a route exists and needs nothing first. |
//! | 1 | The catalogue, or a host-facts or preferences file, could not be read. |
//! | 2 | The command line is wrong. |
//! | 3 | `route`: no route runs the application on this machine. |
//! | 4 | `route`: the recommended route needs something installed or enabled first. |
//! | 5 | The catalogue is not valid. |
//! | 6 | No application has that ID, or nothing matches the search. |

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::Serialize;

use tur_catalogue::report::{render_entry, render_hits, render_routes, render_statistics};
use tur_catalogue::{
    Catalogue, Code, Date, HostFacts, Preferences, Problem, Route, RouteOption, RouteRequest,
    parse_host_facts, parse_preferences, recommended, route, search,
};

const EXIT_OK: u8 = 0;
const EXIT_UNREADABLE: u8 = 1;
const EXIT_NO_ROUTE: u8 = 3;
const EXIT_NEEDS_SETUP: u8 = 4;
const EXIT_INVALID: u8 = 5;
const EXIT_NOT_FOUND: u8 = 6;

/// The largest host-facts or preferences file, in bytes.
const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// The TUR application catalogue and router. Nothing is installed or run.
#[derive(Parser)]
#[command(name = "tur-catalogue", version)]
struct Cli {
    /// The catalogue directory: one .toml file per application.
    #[arg(long, global = true, value_name = "DIR", default_value = "catalogue")]
    catalogue: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Load every entry of a catalogue and check it against the rules.
    Validate {
        /// The catalogue directory. Defaults to --catalogue.
        dir: Option<PathBuf>,
        /// The repository root. When given, every record that claims a TUR
        /// test must point at a report file that exists under it.
        #[arg(long, value_name = "DIR")]
        repository: Option<PathBuf>,
    },
    /// Find applications by name or alias. Tolerant of case, punctuation,
    /// spacing and small typos. Uses the catalogue only, never the network.
    Search {
        /// The words to search for.
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print everything the catalogue holds about one application.
    Show {
        /// The application ID.
        id: String,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Order the routes by which an application can run on a machine.
    /// Nothing is installed or launched.
    Route {
        /// The application ID.
        id: String,
        /// A JSON file of facts about the machine. Without it, a plain
        /// x86-64 machine with Flatpak and nothing optional is assumed.
        #[arg(long, value_name = "FILE")]
        host: Option<PathBuf>,
        /// A JSON file of the user's preferences.
        #[arg(long, value_name = "FILE")]
        prefs: Option<PathBuf>,
        /// Offer the Windows guest as well, whatever Wine's rating.
        #[arg(long)]
        offer_guest: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

/// One search hit in the JSON output.
#[derive(Serialize)]
struct HitReport<'a> {
    id: &'a str,
    name: &'a str,
    category: &'a str,
    publisher: &'a str,
    matched: &'a str,
    kind: tur_catalogue::MatchKind,
    score: u32,
}

/// The JSON document printed by `search --json`.
#[derive(Serialize)]
struct SearchReport<'a> {
    query: &'a str,
    hits: Vec<HitReport<'a>>,
}

/// The application a route report is about.
#[derive(Serialize)]
struct ApplicationReport<'a> {
    id: &'a str,
    name: &'a str,
    category: &'a str,
}

/// The JSON document printed by `route --json`.
#[derive(Serialize)]
struct RouteReport<'a> {
    application: ApplicationReport<'a>,
    host: &'a HostFacts,
    host_assumed: bool,
    preferences: &'a Preferences,
    offer_windows_guest: bool,
    recommended: Route,
    options: &'a [RouteOption],
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let status = match cli.command {
        Command::Validate { dir, repository } => {
            let directory = dir.unwrap_or(cli.catalogue);
            run_validate(&directory, repository.as_deref())
        }
        Command::Search { query, json } => run_search(&cli.catalogue, &query.join(" "), json),
        Command::Show { id, json } => run_show(&cli.catalogue, &id, json),
        Command::Route {
            id,
            host,
            prefs,
            offer_guest,
            json,
        } => run_route(
            &cli.catalogue,
            &id,
            host.as_deref(),
            prefs.as_deref(),
            offer_guest,
            json,
        ),
    };
    ExitCode::from(status)
}

/// Writes a line to standard output. A closed pipe is not worth a panic, so
/// write failures are ignored.
fn print_line(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}

/// Writes a line to standard error, ignoring write failures.
fn print_error_line(text: &str) {
    let _ = writeln!(std::io::stderr(), "{text}");
}

/// Prints a value as pretty JSON on standard output.
fn print_json<T: Serialize>(value: &T) -> u8 {
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            print_line(&text);
            EXIT_OK
        }
        Err(error) => {
            print_error_line(&format!("error: cannot write JSON: {error}"));
            EXIT_UNREADABLE
        }
    }
}

/// Reports a failure with its stable code and returns `status`.
fn fail(json: bool, code: Code, message: &str, status: u8) -> u8 {
    if json {
        let body = serde_json::json!({
            "error": {
                "code": code.as_str(),
                "title": code.title(),
                "message": message,
            }
        });
        print_line(&body.to_string());
    } else {
        print_error_line(&format!("error[{code}]: {message}"));
    }
    status
}

/// Prints the problems of a catalogue and returns the exit status for them.
fn report_problems(problems: &[Problem]) -> u8 {
    for problem in problems {
        print_error_line(&problem.to_string());
    }
    let unreadable = problems
        .iter()
        .all(|problem| problem.code == Code::Unreadable);
    if unreadable {
        EXIT_UNREADABLE
    } else {
        print_error_line(&format!("{} problems found", problems.len()));
        EXIT_INVALID
    }
}

/// Loads the catalogue, or prints why it cannot be used.
fn load(directory: &Path) -> Result<Catalogue, u8> {
    Catalogue::load(directory, Date::today_utc()).map_err(|problems| report_problems(&problems))
}

fn run_validate(directory: &Path, repository: Option<&Path>) -> u8 {
    let catalogue = match load(directory) {
        Ok(catalogue) => catalogue,
        Err(status) => return status,
    };
    let statistics = catalogue.statistics();
    if let Some(root) = repository {
        let missing = catalogue.missing_test_reports(root);
        if !missing.is_empty() {
            return report_problems(&missing);
        }
    }
    print_line(&render_statistics(&statistics));
    if repository.is_none() && statistics.rated_by_tur_tests > 0 {
        print_line("note: test-report files were not checked; pass --repository to check them");
    }
    EXIT_OK
}

fn run_search(directory: &Path, query: &str, json: bool) -> u8 {
    let catalogue = match load(directory) {
        Ok(catalogue) => catalogue,
        Err(status) => return status,
    };
    let hits = search(catalogue.entries(), query);
    if json {
        let report = SearchReport {
            query,
            hits: hits
                .iter()
                .map(|hit| HitReport {
                    id: &hit.entry.id,
                    name: &hit.entry.name,
                    category: hit.entry.category.as_str(),
                    publisher: &hit.entry.publisher,
                    matched: hit.matched,
                    kind: hit.kind,
                    score: hit.score,
                })
                .collect(),
        };
        let status = print_json(&report);
        if status != EXIT_OK {
            return status;
        }
    } else if hits.is_empty() {
        let message = format!("nothing in the catalogue matches \"{query}\"");
        return fail(false, Code::NoMatch, &message, EXIT_NOT_FOUND);
    } else {
        print_line(render_hits(&hits).trim_end());
    }
    if hits.is_empty() {
        EXIT_NOT_FOUND
    } else {
        EXIT_OK
    }
}

fn run_show(directory: &Path, id: &str, json: bool) -> u8 {
    let catalogue = match load(directory) {
        Ok(catalogue) => catalogue,
        Err(status) => return status,
    };
    let Some(entry) = catalogue.get(id) else {
        return unknown_application(json, id);
    };
    if json {
        print_json(entry)
    } else {
        print_line(render_entry(entry).trim_end());
        EXIT_OK
    }
}

fn unknown_application(json: bool, id: &str) -> u8 {
    let message = format!(
        "no application in the catalogue has the ID \"{id}\"; use `tur-catalogue search` to find it"
    );
    fail(json, Code::UnknownApplication, &message, EXIT_NOT_FOUND)
}

/// Reads a small JSON input file, refusing one larger than the limit before
/// reading all of it.
fn read_input(path: &Path) -> Result<String, String> {
    let limit = u64::try_from(MAX_INPUT_BYTES).unwrap_or(u64::MAX);
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(format!("it is larger than {MAX_INPUT_BYTES} bytes"));
    }
    String::from_utf8(bytes).map_err(|_| "it is not valid UTF-8".to_owned())
}

/// Reads and parses an optional input file; `None` gives the default value.
fn read_parsed<T: Default>(
    path: Option<&Path>,
    what: &str,
    parse: fn(&str) -> Result<T, String>,
) -> Result<T, String> {
    let Some(path) = path else {
        return Ok(T::default());
    };
    let text = read_input(path)
        .map_err(|reason| format!("cannot read the {what} file {}: {reason}", path.display()))?;
    parse(&text)
        .map_err(|reason| format!("the {what} file {} is not valid: {reason}", path.display()))
}

fn run_route(
    directory: &Path,
    id: &str,
    host_file: Option<&Path>,
    preferences_file: Option<&Path>,
    offer_guest: bool,
    json: bool,
) -> u8 {
    let host = match read_parsed(host_file, "host-facts", parse_host_facts) {
        Ok(host) => host,
        Err(message) => return fail(json, Code::InputInvalid, &message, EXIT_UNREADABLE),
    };
    let preferences = match read_parsed(preferences_file, "preferences", parse_preferences) {
        Ok(preferences) => preferences,
        Err(message) => return fail(json, Code::InputInvalid, &message, EXIT_UNREADABLE),
    };
    let catalogue = match load(directory) {
        Ok(catalogue) => catalogue,
        Err(status) => return status,
    };
    let Some(entry) = catalogue.get(id) else {
        return unknown_application(json, id);
    };

    let request = RouteRequest {
        entry,
        offer_windows_guest: offer_guest,
    };
    let options = route(&request, &host, &preferences);
    let chosen = recommended(&options);

    if json {
        let report = RouteReport {
            application: ApplicationReport {
                id: &entry.id,
                name: &entry.name,
                category: entry.category.as_str(),
            },
            host: &host,
            host_assumed: host_file.is_none(),
            preferences: &preferences,
            offer_windows_guest: offer_guest,
            recommended: chosen.route,
            options: &options,
        };
        let status = print_json(&report);
        if status != EXIT_OK {
            return status;
        }
    } else {
        print_line(render_routes(entry, &options).trim_end());
        if host_file.is_none() {
            print_line("");
            print_line(
                "Host: assumed, not detected: an x86-64 machine with Flatpak and nothing optional. Pass --host to state the facts.",
            );
        }
    }

    if chosen.route == Route::NoRoute {
        if !json {
            print_error_line(&format!(
                "error[{}]: {}",
                Code::NoRoute,
                Code::NoRoute.title()
            ));
        }
        EXIT_NO_ROUTE
    } else if chosen.prerequisites.is_empty() {
        EXIT_OK
    } else {
        EXIT_NEEDS_SETUP
    }
}
