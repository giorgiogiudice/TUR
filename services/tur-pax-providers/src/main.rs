// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Command-line entry point for `tur-pax-providers`. All logic lives in the library.

use std::io::Write;
use std::process::ExitCode;

use tur_pax_providers::cli::run;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let outcome = run(&arguments);
    // A closed pipe is not worth a panic: the exit status still says what happened.
    let _ = std::io::stdout().write_all(outcome.stdout.as_bytes());
    let _ = std::io::stderr().write_all(outcome.stderr.as_bytes());
    ExitCode::from(outcome.status)
}
