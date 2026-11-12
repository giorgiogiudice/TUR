// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Command-line entry point for `tur-pax`. All logic lives in the library.

use std::io;
use std::process::ExitCode;

use clap::Parser;
use tur_pax::cli::{Cli, EXIT_INPUT_ERROR, run};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let status = run(&cli, &mut stdout.lock(), &mut stderr.lock()).unwrap_or(EXIT_INPUT_ERROR);
    ExitCode::from(status)
}
