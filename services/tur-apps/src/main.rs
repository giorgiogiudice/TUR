// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-apps`: install, list, update and remove applications by the route
//! the catalogue's router chooses.
//!
//! ```text
//! tur-apps plan <id> [--route R] [--installer FILE] [--offline] [--json]
//! tur-apps install <id> [--route R] [--installer FILE] [--yes] [--events]
//! tur-apps list [--check-updates] [--json]
//! tur-apps update <id> [--events]
//! tur-apps remove <id> [--keep-data | --delete-data] [--events]
//! tur-apps launch <id>
//! tur-apps sources [enable NAME | disable NAME] [--json]
//! ```
//!
//! Exit status: 0 success; 1 an input could not be read; 2 the command line
//! is wrong; 3 refused before anything was changed; 5 a file is not the one
//! expected, and nothing in it ran; 6 no such application, or not
//! installed; 7 a step failed; 8 a removal left something behind. `launch`
//! exits with the application's own status.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use clap::Parser;
use serde_json::Value;

use tur_apps::cli::{Cli, Command, SourceAction};
use tur_apps::codes::{Code, EXIT_OK, Failure};
use tur_apps::events::{Emitter, Event};
use tur_apps::exec::{Program, find};
use tur_apps::ops::{self, DataChoice};
use tur_apps::paths::Dirs;
use tur_apps::plan::{self, Context, Plan, Request};
use tur_catalogue::Route;

fn main() -> ExitCode {
    let cli = Cli::parse();
    ExitCode::from(run(cli))
}

fn print_line(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}

fn print_error_line(text: &str) {
    let _ = writeln!(std::io::stderr(), "{text}");
}

fn print_json(value: &Value) {
    print_line(&serde_json::to_string_pretty(value).unwrap_or_default());
}

/// Reports a failure outside an event stream: JSON on standard output with
/// `--json`, a line on standard error otherwise.
fn report(json: bool, failure: &Failure) -> u8 {
    if json {
        print_line(&serde_json::json!({ "error": failure }).to_string());
    } else {
        print_error_line(&failure.to_string());
    }
    failure.code.exit_status()
}

fn context(cli: &Cli) -> Result<Context, Failure> {
    let program = |name: &'static str, given: &Option<std::path::PathBuf>, defaults: &[&str]| {
        Program::new(
            name,
            given.clone().unwrap_or_else(|| find(defaults)),
        )
    };
    Ok(Context {
        catalogue: cli.catalogue.clone(),
        dirs: Dirs::current()?,
        flatpak: program("flatpak", &cli.flatpak_program, &["/usr/bin/flatpak"]),
        curl: program("curl", &cli.curl_program, &["/usr/bin/curl"]),
        tur_runtime: program(
            "tur-runtime",
            &cli.tur_runtime_program,
            &["/usr/bin/tur-runtime", "/usr/local/bin/tur-runtime"],
        ),
        wine: cli
            .wine_program
            .clone()
            .unwrap_or_else(|| find(&["/usr/bin/wine"])),
        host_file: cli.host.clone(),
        preferences_file: cli.prefs.clone(),
    })
}

fn parse_route(name: Option<&str>) -> Result<Option<Route>, Failure> {
    match name {
        None => Ok(None),
        Some(name) => Route::from_name(name)
            .filter(|route| *route != Route::NoRoute)
            .map(Some)
            .ok_or_else(|| {
                Failure::new(
                    Code::InputInvalid,
                    format!("\"{name}\" is not a route; the routes this program installs by are flatpak and wine"),
                )
            }),
    }
}

/// Asks a yes-or-no question on the terminal. `default` is the answer to
/// an empty line.
fn ask(question: &str, default: bool) -> bool {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    let _ = write!(std::io::stderr(), "{question} {hint} ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    match answer.trim().to_lowercase().as_str() {
        "" => default,
        "y" | "yes" => true,
        _ => false,
    }
}

/// Ends an operation: its result or its failure, as the last event.
fn finish(emitter: &mut Emitter, operation: &str, outcome: Result<Value, Failure>) -> u8 {
    match outcome {
        Ok(result) => {
            let text = !emitter.is_json();
            emitter.emit(Event::Result {
                operation: operation.to_owned(),
                result: result.clone(),
            });
            if text {
                print_json(&result);
            }
            EXIT_OK
        }
        Err(failure) => {
            let status = failure.code.exit_status();
            if emitter.is_json() {
                emitter.emit(Event::Error { error: failure });
            } else {
                print_error_line(&failure.to_string());
            }
            status
        }
    }
}

fn run(cli: Cli) -> u8 {
    let json_errors = match &cli.command {
        Command::Plan { json, .. } | Command::List { json, .. } | Command::Sources { json, .. } => {
            *json
        }
        Command::Install { events, .. }
        | Command::Update { events, .. }
        | Command::Remove { events, .. } => *events,
        Command::Launch { .. } => false,
    };
    let context = match context(&cli) {
        Ok(context) => context,
        Err(failure) => return report(json_errors, &failure),
    };
    match cli.command {
        Command::Plan {
            id,
            route,
            installer,
            offline,
            json,
        } => {
            let request = match parse_route(route.as_deref()) {
                Ok(route) => Request {
                    id,
                    route,
                    installer,
                    offline,
                },
                Err(failure) => return report(json, &failure),
            };
            match plan::make(&context, &request) {
                Ok(plan) => {
                    if json {
                        print_json(&serde_json::to_value(&plan).unwrap_or_default());
                    } else {
                        print_line(plan::render(&plan).trim_end());
                    }
                    EXIT_OK
                }
                Err(failure) => report(json, &failure),
            }
        }
        Command::Install {
            id,
            route,
            installer,
            yes,
            events,
        } => {
            let mut emitter = Emitter::stdout(events);
            let request = match parse_route(route.as_deref()) {
                Ok(route) => Request {
                    id,
                    route,
                    installer,
                    offline: false,
                },
                Err(failure) => return finish(&mut emitter, "install", Err(failure)),
            };
            let interactive = !events && std::io::stdin().is_terminal();
            let mut confirm = |plan: &Plan| {
                yes || (interactive && ask(&format!("Install {}?", plan.application.name), false))
            };
            let outcome = ops::install(&context, &request, &mut emitter, &mut confirm);
            finish(&mut emitter, "install", outcome)
        }
        Command::List {
            check_updates,
            json,
        } => match ops::list(&context, check_updates) {
            Ok(document) => {
                if json {
                    print_json(&document);
                } else {
                    print_list(&document);
                }
                EXIT_OK
            }
            Err(failure) => report(json, &failure),
        },
        Command::Update { id, events } => {
            let mut emitter = Emitter::stdout(events);
            let outcome = ops::update(&context, &id, &mut emitter);
            finish(&mut emitter, "update", outcome)
        }
        Command::Remove {
            id,
            keep_data,
            delete_data,
            events,
        } => {
            let mut emitter = Emitter::stdout(events);
            let choice = if delete_data {
                DataChoice::Delete
            } else if keep_data {
                DataChoice::Keep
            } else if !events && std::io::stdin().is_terminal() {
                if ask("Keep its settings and documents?", true) {
                    DataChoice::Keep
                } else {
                    DataChoice::Delete
                }
            } else {
                emitter.text("Its settings and documents are kept (the default; --delete-data removes them).");
                DataChoice::Keep
            };
            let outcome = ops::remove(&context, &id, choice, &mut emitter);
            finish(&mut emitter, "remove", outcome)
        }
        Command::Launch { id } => match ops::launch(&context, &id) {
            Ok(status) => u8::try_from(status.clamp(0, 255)).unwrap_or(1),
            Err(failure) => report(false, &failure),
        },
        Command::Sources { action, json } => {
            // With --json, standard output carries the one document only.
            let mut emitter = if json {
                Emitter::into_buffer(false, Box::new(std::io::sink()))
            } else {
                Emitter::stdout(false)
            };
            let outcome = match action {
                None => ops::sources(&context),
                Some(SourceAction::Enable { name }) => {
                    ops::set_source(&context, &name, true, &mut emitter)
                }
                Some(SourceAction::Disable { name }) => {
                    ops::set_source(&context, &name, false, &mut emitter)
                }
            };
            match outcome {
                Ok(document) => {
                    if json {
                        print_json(&document);
                    } else {
                        print_sources(&document);
                    }
                    EXIT_OK
                }
                Err(failure) => report(json, &failure),
            }
        }
    }
}

fn print_list(document: &Value) {
    let applications = document["applications"].as_array().cloned().unwrap_or_default();
    if applications.is_empty() {
        print_line("Nothing is installed for you.");
    }
    for application in applications {
        let size = application["size"]
            .as_u64()
            .map_or_else(|| "size not known".to_owned(), tur_apps::events::human_size);
        let update = match application["update"]["available"].as_bool() {
            Some(true) => format!(
                "update available{}",
                application["update"]["version"]
                    .as_str()
                    .map(|version| format!(" ({version})"))
                    .unwrap_or_default()
            ),
            Some(false) => "up to date".to_owned(),
            None => "updates not checked".to_owned(),
        };
        print_line(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            application["id"].as_str().unwrap_or("?"),
            application["name"].as_str().unwrap_or("?"),
            application["route"].as_str().unwrap_or("?"),
            application["version"].as_str().unwrap_or("-"),
            size,
            application["from"].as_str().unwrap_or("?"),
            update
        ));
    }
    for problem in document["problems"].as_array().cloned().unwrap_or_default() {
        print_error_line(&format!("warning: {}", problem.as_str().unwrap_or("?")));
    }
}

fn print_sources(document: &Value) {
    if let Some(sources) = document["sources"].as_array() {
        for source in sources {
            print_line(&format!(
                "{}\t{}\t{}",
                source["state"].as_str().unwrap_or("?"),
                source["title"].as_str().unwrap_or("?"),
                source["location"].as_str().unwrap_or("")
            ));
        }
    } else {
        print_line(&format!(
            "{}: {}",
            document["name"].as_str().unwrap_or("?"),
            document["state"].as_str().unwrap_or("?")
        ));
    }
}
