// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Entry point of the `tur-ai-local` command-line tool. All logic is in the
//! library; see [`tur_ai_local::cli`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    ExitCode::from(tur_ai_local::cli::run(
        std::env::args_os(),
        &mut stdout,
        &mut stderr,
    ))
}
