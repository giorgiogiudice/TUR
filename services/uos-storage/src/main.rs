// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Entry point of the `uos-storage` command-line tool. All logic is in the
//! library; see [`uos_storage::cli`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    ExitCode::from(uos_storage::cli::run(std::env::args_os(), &mut stdout, &mut stderr))
}
