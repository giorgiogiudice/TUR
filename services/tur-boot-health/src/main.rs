// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `tur-boot-health` command: a thin shell around the `tur_boot_health`
//! library.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match tur_boot_health::run(&arguments) {
        0 => ExitCode::SUCCESS,
        status => ExitCode::from(u8::try_from(status).unwrap_or(1)),
    }
}
