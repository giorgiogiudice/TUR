// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-installer`: dry-run front end of the TUR installer core.
//!
//! Every subcommand reads its input from files and writes text or JSON to
//! standard output. Nothing here inspects or changes a real disk.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let code = tur_installer_core::cli::run(std::env::args_os(), &mut stdout, &mut stderr);
    ExitCode::from(code)
}
