// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The commands that put the session service together from the machine:
//! `tur-pax serve`, `tur-pax schedule wake` and `tur-pax key`.
//!
//! ```text
//! tur-pax serve [--bus <address>] [--tools <dir>] [--manifests <dir>]
//!               [--adapters <dir>] [--models <file>]
//! tur-pax schedule wake [--bus <address>]
//! tur-pax key set <provider> [--bus <address>]      the key is read from standard input
//! tur-pax key remove <provider> [--bus <address>]
//! tur-pax key status <provider> [--bus <address>]
//! ```
//!
//! * `serve` connects to the session bus, takes the name `org.tur.Agent1`
//!   and answers until the bus goes away. The user unit
//!   `tur-pax.service` starts it, and the bus starts that unit when the
//!   name is first called (`units/`).
//! * `schedule wake` is what the timers start. It reads the settings
//!   file: if P@x is switched off it does nothing at all, so that a login
//!   does not start the service of a person who never turned P@x on.
//!   Otherwise it calls `Wake`, which starts the service if it is not
//!   running.
//! * `key` talks to the password store directly, so that a key can be set
//!   or removed without the service. The key is read from standard input,
//!   never from the command line, where every program of the person could
//!   read it in the process list.
//!
//! The bus is the one `--bus` names, or `DBUS_SESSION_BUS_ADDRESS`, or the
//! socket `bus` in `XDG_RUNTIME_DIR`.
//!
//! Exit status: 0 done; 1 something could not be read, reached or done,
//! with the `TUR-AI-*` code on standard error; 2 a usage error (clap).

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Subcommand};
use tur_pax_memory::SystemClock;
use tur_pax_providers::Zeroizing;

use crate::bus::{Connection, Message};
use crate::dbus::{self, BusEvents};
use crate::exec::{Executor, StoppableExecutor};
use crate::faults::{Fault, TaskCode};
use crate::keyring::{KeyStore, SecretService};
use crate::notify::BusNotifier;
use crate::service::{Core, Parts};
use crate::settings::{Paths, Settings, host_zone, provider_from_name};
use crate::wake::SystemdWaker;

/// Exit status: done.
pub const EXIT_DONE: u8 = 0;
/// Exit status: something could not be read, reached or done.
pub const EXIT_FAILED: u8 = 1;

/// The bus option every command here has.
#[derive(Debug, Args)]
pub struct BusArgs {
    /// The address of the session bus. Without it, DBUS_SESSION_BUS_ADDRESS,
    /// or the socket "bus" in XDG_RUNTIME_DIR.
    #[arg(long, value_name = "ADDRESS")]
    pub bus: Option<String>,
}

/// The arguments of `serve`.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// The bus.
    #[command(flatten)]
    pub bus: BusArgs,
    /// The directory of tool sets. Without it, /usr/share/tur/pax/tools.
    #[arg(long, value_name = "DIR")]
    pub tools: Option<PathBuf>,
    /// A further directory of tool manifests, one JSON file per service.
    #[arg(long, value_name = "DIR")]
    pub manifests: Option<PathBuf>,
    /// A further directory of tool adapters, one JSON file per service.
    #[arg(long, value_name = "DIR")]
    pub adapters: Option<PathBuf>,
    /// The model list. Without it, /etc/tur/pax/models.json if it exists,
    /// then /usr/share/tur/pax/models.json.
    #[arg(long, value_name = "FILE")]
    pub models: Option<PathBuf>,
}

/// The `schedule` subcommands.
#[derive(Debug, Subcommand)]
pub enum ScheduleCommand {
    /// Ask the session service to look at the schedules that are due. Does
    /// nothing while P@x is switched off.
    Wake(BusArgs),
}

/// The `key` subcommands.
#[derive(Debug, Subcommand)]
pub enum KeyCommand {
    /// Store the key of a provider, read from standard input, in place of
    /// any key already stored.
    Set {
        /// anthropic, openai or google.
        provider: String,
        /// The bus.
        #[command(flatten)]
        bus: BusArgs,
    },
    /// Remove the key of a provider.
    Remove {
        /// anthropic, openai or google.
        provider: String,
        /// The bus.
        #[command(flatten)]
        bus: BusArgs,
    },
    /// Say whether a key is stored for a provider. The key is not read.
    Status {
        /// anthropic, openai or google.
        provider: String,
        /// The bus.
        #[command(flatten)]
        bus: BusArgs,
    },
}

/// The address of the session bus.
fn bus_address(args: &BusArgs) -> Result<String, Fault> {
    if let Some(address) = &args.bus {
        return Ok(address.clone());
    }
    if let Ok(address) = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        && !address.is_empty()
    {
        return Ok(address);
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|runtime| format!("unix:path={}", runtime.join("bus").display()))
        .ok_or_else(|| {
            Fault::new(
                TaskCode::KeyStoreUnavailable,
                "no session bus is known: neither DBUS_SESSION_BUS_ADDRESS nor XDG_RUNTIME_DIR is set",
            )
        })
}

fn failed(fault: &Fault, err: &mut dyn Write) -> io::Result<u8> {
    writeln!(err, "error: {fault}")?;
    Ok(EXIT_FAILED)
}

fn paths() -> Result<Paths, Fault> {
    Paths::from_environment().ok_or_else(|| {
        Fault::new(
            TaskCode::ServiceStorage,
            "no home directory and no XDG base directories are known",
        )
    })
}

/// Run `tur-pax serve`.
pub fn run_serve(args: &ServeArgs, err: &mut dyn Write) -> io::Result<u8> {
    let address = match bus_address(&args.bus) {
        Ok(address) => address,
        Err(fault) => return failed(&fault, err),
    };
    let mut paths = match paths() {
        Ok(paths) => paths,
        Err(fault) => return failed(&fault, err),
    };
    if let Some(tools) = &args.tools {
        paths.tools.clone_from(tools);
    }
    paths.extra_manifests.clone_from(&args.manifests);
    paths.extra_adapters.clone_from(&args.adapters);
    if let Some(models) = &args.models {
        paths.models = vec![models.clone()];
    }
    let (bus, incoming) = match Connection::open(&address) {
        Ok(connection) => connection,
        Err(error) => {
            writeln!(err, "error: {error}")?;
            return Ok(EXIT_FAILED);
        }
    };
    let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/bin/tur-pax"));
    let events = Arc::new(BusEvents::new(bus.clone()));
    // The settings are <config home>/tur/pax/settings.json: the timer's
    // command is told the same configuration directory.
    let config_home = paths
        .settings
        .ancestors()
        .nth(3)
        .map(std::path::Path::to_path_buf);
    let parts = Parts {
        paths,
        clock: Arc::new(SystemClock),
        zone: Arc::new(host_zone),
        keys: Arc::new(SecretService::new(address.clone())),
        notifier: Arc::new(BusNotifier::new(bus.clone())),
        waker: Arc::new(SystemdWaker::new(program, address, config_home)),
        events: events.clone(),
        executors: Arc::new(
            |stop: Arc<std::sync::atomic::AtomicBool>| -> Box<dyn Executor + Send> {
                Box::new(StoppableExecutor::new(stop))
            },
        ),
    };
    let core = Core::new(parts);
    events.attach(&core);
    core.spawn_runner()?;
    match dbus::serve(&core, &bus, &incoming) {
        Ok(()) => Ok(EXIT_DONE),
        Err(error) => {
            writeln!(err, "error: {error}")?;
            Ok(EXIT_FAILED)
        }
    }
}

/// Run `tur-pax schedule`.
pub fn run_schedule(
    command: &ScheduleCommand,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    let ScheduleCommand::Wake(bus) = command;
    let settings = match paths().and_then(|paths| Settings::load(&paths.settings)) {
        Ok(settings) => settings,
        Err(fault) => return failed(&fault, err),
    };
    if !settings.enabled {
        writeln!(out, "P@x is switched off; nothing to do.")?;
        return Ok(EXIT_DONE);
    }
    let address = match bus_address(bus) {
        Ok(address) => address,
        Err(fault) => return failed(&fault, err),
    };
    let woken = Connection::open(&address).and_then(|(bus, _)| {
        bus.call(
            Message::method_call(dbus::NAME, dbus::PATH, dbus::INTERFACE, "Wake", Vec::new()),
            Duration::from_secs(60),
        )
    });
    match woken {
        Ok(_) => Ok(EXIT_DONE),
        Err(error) => {
            writeln!(err, "error: {}: {error}", TaskCode::RunnerWake.as_str())?;
            Ok(EXIT_FAILED)
        }
    }
}

/// Run `tur-pax key`.
pub fn run_key(command: &KeyCommand, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<u8> {
    let (provider, bus) = match command {
        KeyCommand::Set { provider, bus }
        | KeyCommand::Remove { provider, bus }
        | KeyCommand::Status { provider, bus } => (provider, bus),
    };
    let Some(provider) = provider_from_name(provider) else {
        let fault = Fault::new(
            TaskCode::ServiceInvalidRequest,
            "the provider is not one of anthropic, openai and google",
        );
        return failed(&fault, err);
    };
    let address = match bus_address(bus) {
        Ok(address) => address,
        Err(fault) => return failed(&fault, err),
    };
    let store = SecretService::new(address);
    let done = match command {
        KeyCommand::Set { .. } => {
            let mut line = Zeroizing::new(String::new());
            io::stdin().lock().read_line(&mut line)?;
            store
                .set(provider, line.as_str())
                .map(|()| "The key is stored.".to_owned())
        }
        KeyCommand::Remove { .. } => store.remove(provider).map(|removed| {
            if removed {
                "The key is removed.".to_owned()
            } else {
                "No key was stored.".to_owned()
            }
        }),
        KeyCommand::Status { .. } => store.is_set(provider).map(|set| {
            if set {
                "set".to_owned()
            } else {
                "not set".to_owned()
            }
        }),
    };
    match done {
        Ok(message) => {
            writeln!(out, "{message}")?;
            Ok(EXIT_DONE)
        }
        Err(fault) => failed(&fault, err),
    }
}
