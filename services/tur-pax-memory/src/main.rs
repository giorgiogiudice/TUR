// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Command-line entry point for `tur-pax-memory`. All logic lives in the
//! library.
//!
//! This file is the one place in the component that reads the clock and the
//! environment, and the one place that implements
//! `tur_pax_memory::UserConfirmation`: the library cannot confirm anything on
//! the user's behalf, so the program that the user runs does it, for the one
//! entry or the one state of the memory switch named on its command line.

use std::env;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use tur_pax_memory::cli::{Cli, EXIT_ERROR, Environment, run};
use tur_pax_memory::{
    Clock, MemoryEntry, MemorySwitch, SystemClock, Timestamp, UserConfirmation, default_store_dir,
};

/// The person who typed the command.
///
/// Running `tur-pax-memory confirm <id>` is the user's confirmation of that
/// entry and of no other. In the finished system the same decision is taken in
/// the settings interface; an assistant that ran this command itself would be
/// running a class 2 command, which the policy engine shows to the user first.
struct PersonAtTerminal {
    confirms: Option<String>,
    /// The state `tur-pax-memory switch` names, if that is the command.
    switches: Option<bool>,
    at: Timestamp,
}

/// Running `tur-pax-memory switch on` is the user turning memory on, and
/// `switch off` the user turning it off. It covers that state and not the
/// other, and no other command covers either.
impl UserConfirmation<MemorySwitch> for PersonAtTerminal {
    fn covers(&self, switch: &MemorySwitch) -> bool {
        self.switches == Some(switch.enabled)
    }

    fn confirmed_at(&self) -> Timestamp {
        self.at
    }
}

impl UserConfirmation<MemoryEntry> for PersonAtTerminal {
    fn covers(&self, entry: &MemoryEntry) -> bool {
        self.confirms.as_deref() == Some(entry.id().as_str())
    }

    fn confirmed_at(&self) -> Timestamp {
        self.at
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let clock = SystemClock;
    let user = PersonAtTerminal {
        confirms: cli.confirm_target().map(str::to_owned),
        switches: cli.switch_target(),
        at: clock.now(),
    };
    let xdg_data_home = env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = env::var_os("HOME").map(PathBuf::from);
    let environment = Environment {
        clock: &clock,
        user: &user,
        switch: &user,
        default_store: default_store_dir(xdg_data_home.as_deref(), home.as_deref()),
    };
    let stdout = io::stdout();
    let stderr = io::stderr();
    let status =
        run(&cli, &environment, &mut stdout.lock(), &mut stderr.lock()).unwrap_or(EXIT_ERROR);
    ExitCode::from(status)
}
