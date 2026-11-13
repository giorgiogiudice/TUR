// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-pax-a11y`: P@x's accessibility bridge.
//!
//! ```text
//! tur-pax-a11y serve     answer the P@x task loop on standard input and output
//! tur-pax-a11y survey    print what each application exposes, as JSON
//! ```

use std::io::{self, Write};
use std::process::ExitCode;

const USAGE: &str = "usage: tur-pax-a11y serve | survey | --version\n\
\n\
  serve    answer the P@x task loop, one JSON request per line on standard\n\
           input and one answer per line on standard output\n\
  survey   print, as JSON, what each application on the accessibility bus\n\
           exposes: counts of roles, interfaces and actions, never content\n";

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
