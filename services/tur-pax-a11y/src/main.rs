// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-pax-a11y`: P@x's accessibility bridge.
//!
//! ```text
//! tur-pax-a11y serve     answer the P@x task loop on standard input and output
//! tur-pax-a11y survey    print what each application exposes, as JSON
//! tur-pax-a11y confirm-point WINDOW X Y NAME [ROLE]
//!                        the rule for a position proposed from a picture,
//!                        on a Windows application's window: what is there,
//!                        and whether acting there would be confirmed
//! ```

use std::io::{self, Write};
use std::process::ExitCode;

use serde_json::json;
use tur_pax_a11y::tree::{Candidate, choose_window};
use tur_pax_a11y::vision;
use tur_pax_a11y::wine::Wine;

const USAGE: &str = "usage: tur-pax-a11y serve | survey | --version\n\
\n\
  serve    answer the P@x task loop, one JSON request per line on standard\n\
           input and one answer per line on standard output\n\
  survey   print, as JSON, what each application on the accessibility bus\n\
           exposes: counts of roles, interfaces and actions, never content\n\
  confirm-point WINDOW X Y NAME [ROLE]\n\
           look up a point of a Windows application's window (in the\n\
           window's own coordinates) as a position proposed from a picture\n\
           would be, and say whether acting there would be confirmed; it\n\
           acts on nothing\n";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("serve") if arguments.len() == 1 => {
            let stdin = io::stdin();
            match tur_pax_a11y::serve::serve(stdin.lock(), io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    let _ = writeln!(io::stderr(), "tur-pax-a11y: {error}");
                    ExitCode::from(1)
                }
            }
        }
        Some("survey") if arguments.len() == 1 => match tur_pax_a11y::survey::survey() {
            Ok(value) => {
                let _ = writeln!(io::stdout(), "{value:#}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                let _ = writeln!(io::stderr(), "tur-pax-a11y: {error}");
                ExitCode::from(1)
            }
        },
        Some("confirm-point") if (5..=6).contains(&arguments.len()) => {
            confirm_point(&arguments[1..])
        }
        Some("--version") => {
            let _ = writeln!(io::stdout(), "tur-pax-a11y {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            let _ = write!(io::stdout(), "{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            let _ = write!(io::stderr(), "{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `confirm-point`: what the bridge has under a point of a Windows
/// application's window, and the rule's answer. Nothing is acted on.
fn confirm_point(arguments: &[String]) -> ExitCode {
    let (Ok(x), Ok(y)) = (arguments[1].parse::<i32>(), arguments[2].parse::<i32>()) else {
        let _ = write!(io::stderr(), "{USAGE}");
        return ExitCode::from(2);
    };
    let name = &arguments[3];
    let role = arguments.get(4).map(String::as_str);
    let mut wine = Wine::from_environment();
    let windows = wine.windows();
    let candidates: Vec<Candidate> = windows
        .iter()
        .map(|window| {
            let mut names = vec![window.application.clone()];
            names.extend(window.also_known_as());
            (window.id.clone(), window.title.clone(), names)
        })
        .collect();
    let window = match choose_window(&arguments[0], &candidates) {
        Ok(index) => &windows[index],
        Err(miss) => {
            let _ = writeln!(io::stderr(), "tur-pax-a11y: {}: {}", miss.code, miss.message);
            return ExitCode::from(1);
        }
    };
    let under = match wine.at_point(window, x, y) {
        Ok(under) => under,
        Err(error) => {
            let _ = writeln!(io::stderr(), "tur-pax-a11y: {error}");
            return ExitCode::from(1);
        }
    };
    let confirmation = vision::confirm(name, role, &under);
    let answer = json!({
        "window": window.id,
        "point": [x, y],
        "asked": { "name": name, "role": role },
        "covered": under.covered,
        "confirmation": confirmation.to_json(),
        "class": confirmation.class().as_u8(),
    });
    let _ = writeln!(io::stdout(), "{answer:#}");
    ExitCode::SUCCESS
}
