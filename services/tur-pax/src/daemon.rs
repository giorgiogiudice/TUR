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
//! tur-pax models list|use|cancel|settings … [--bus <address>]
//! ```
//!
//! * `models` is a client of `org.tur.Agent1`: it lists the models of a
//!   provider (`local`: what local AI offers this computer, with how each
//!   suits it), chooses one (`local:<id>` makes it ready first, after the
//!   person was told its download size and accepted a licence that
//!   restricts use), and shows or changes a model's generation settings.
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
use std::sync::mpsc::Receiver;
use std::time::Duration;

use clap::{Args, Subcommand};
use tur_pax_memory::SystemClock;
use tur_pax_providers::Zeroizing;

use crate::bus::{Connection, Message, Value as BusValue};
use crate::dbus::{self, BusEvents};
use crate::exec::{Executor, StoppableExecutor};
use crate::faults::{Fault, TaskCode};
use crate::keyring::{KeyStore, SecretService};
use crate::notify::BusNotifier;
use crate::service::{Core, Parts};
use crate::settings::{ModelList, Paths, Settings, find_provider, host_zone, is_provider_id};
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
    /// The program of local AI. Without it, /usr/bin/tur-ai-local.
    #[arg(long, value_name = "FILE")]
    pub ai_local: Option<PathBuf>,
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
        /// The identifier of the provider, such as anthropic.
        provider: String,
        /// The bus.
        #[command(flatten)]
        bus: BusArgs,
    },
    /// Remove the key of a provider.
    Remove {
        /// The identifier of the provider, such as anthropic.
        provider: String,
        /// The bus.
        #[command(flatten)]
        bus: BusArgs,
    },
    /// Say whether a key is stored for a provider. The key is not read.
    Status {
        /// The identifier of the provider, such as anthropic.
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
    if let Some(program) = &args.ai_local {
        paths.ai_local.clone_from(program);
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
    // The item of the provider's entry, from the model list and the
    // person's own entries; for an identifier no entry has, the identifier.
    let entry = paths().ok().and_then(|paths| {
        let settings = Settings::load(&paths.settings).ok()?;
        let models = ModelList::load(&paths.models).ok()?;
        find_provider(&models, &settings, provider)
    });
    let item = match entry {
        Some((entry, _)) => entry.keyring_item().to_owned(),
        None if is_provider_id(provider) => provider.clone(),
        None => {
            let fault = Fault::new(
                TaskCode::ServiceInvalidRequest,
                "that is not the identifier of a provider",
            );
            return failed(&fault, err);
        }
    };
    let item = item.as_str();
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
                .set(item, line.as_str())
                .map(|()| "The key is stored.".to_owned())
        }
        KeyCommand::Remove { .. } => store.remove(item).map(|removed| {
            if removed {
                "The key is removed.".to_owned()
            } else {
                "No key was stored.".to_owned()
            }
        }),
        KeyCommand::Status { .. } => store.is_set(item).map(|set| {
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

/// The `models` subcommands.
#[derive(Debug, Subcommand)]
pub enum ModelsCommand {
    /// List the providers, or the models of one. `local`: what local AI
    /// offers this computer and how each model suits it
    List {
        /// A provider's identifier, such as anthropic or local
        provider: Option<String>,
        /// Ask the provider for its own list of models (sends it the key)
        #[arg(long)]
        refresh: bool,
        /// Print the service's JSON
        #[arg(long)]
        json: bool,
        /// The bus.
        #[command(flatten)]
        bus: BusArgs,
    },
    /// Choose the model P@x uses, as PROVIDER:MODEL. With local:<id>, local
    /// AI downloads the model if it is not there yet, checks it and starts
    /// it first
    Use {
        /// The provider and the model, as PROVIDER:MODEL
        choice: String,
        /// Agree to download a local model of the size shown
        #[arg(long)]
        yes: bool,
        /// Accept the use restrictions of a local model's licence
        #[arg(long)]
        accept_licence: bool,
        /// The bus.
        #[command(flatten)]
        bus: BusArgs,
    },
    /// Stop making a local model ready
    Cancel {
        /// The bus.
        #[command(flatten)]
        bus: BusArgs,
    },
    /// Show, or change, the generation settings of PROVIDER:MODEL
    Settings {
        /// The provider and the model, as PROVIDER:MODEL
        choice: String,
        /// A setting to change, as NAME=VALUE: temperature, top_p, top_k,
        /// stop (a JSON list), effort, thinking_budget, max_output_tokens,
        /// context_tokens
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
        /// A setting to take back to the model's default
        #[arg(long = "clear", value_name = "NAME")]
        clear: Vec<String>,
        /// Print the service's JSON
        #[arg(long)]
        json: bool,
        /// The bus.
        #[command(flatten)]
        bus: BusArgs,
    },
}

/// The longest a call of the service is waited for.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// How long `models use` follows a local model being made ready: a large
/// download over a slow connection takes hours.
const FOLLOW_TIMEOUT: Duration = Duration::from_secs(24 * 3600);

fn call_service(bus: &Connection, method: &str, body: Vec<BusValue>) -> Result<Message, String> {
    bus.call(
        Message::method_call(dbus::NAME, dbus::PATH, dbus::INTERFACE, method, body),
        CALL_TIMEOUT,
    )
    .map_err(|error| error.to_string())
}

fn json_reply(reply: &Message) -> Result<serde_json::Value, String> {
    crate::strict::value_from_str(reply.first_string().unwrap_or_default())
        .map_err(|_| "the service did not answer with JSON".to_owned())
}

fn split_choice(choice: &str) -> Result<(&str, &str), String> {
    choice
        .split_once(':')
        .filter(|(provider, model)| !provider.is_empty() && !model.is_empty())
        .ok_or_else(|| format!("{choice:?} is not PROVIDER:MODEL"))
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

fn items(value: &serde_json::Value) -> &[serde_json::Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// Run `tur-pax models`.
pub fn run_models(
    command: &ModelsCommand,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    let bus = match command {
        ModelsCommand::List { bus, .. }
        | ModelsCommand::Use { bus, .. }
        | ModelsCommand::Cancel { bus }
        | ModelsCommand::Settings { bus, .. } => bus,
    };
    let address = match bus_address(bus) {
        Ok(address) => address,
        Err(fault) => return failed(&fault, err),
    };
    let (connection, incoming) = match Connection::open(&address) {
        Ok(opened) => opened,
        Err(error) => {
            writeln!(err, "error: {error}")?;
            return Ok(EXIT_FAILED);
        }
    };
    let result = match command {
        ModelsCommand::List {
            provider,
            refresh,
            json,
            ..
        } => models_list(&connection, provider.as_deref(), *refresh, *json, out),
        ModelsCommand::Use {
            choice,
            yes,
            accept_licence,
            ..
        } => models_use(&connection, &incoming, choice, *yes, *accept_licence, out),
        ModelsCommand::Cancel { .. } => call_service(&connection, "CancelLocalModel", Vec::new())
            .and_then(|_| {
                writeln!(
                    out,
                    "Local AI was asked to stop. What it downloaded is kept."
                )
                .map_err(|error| error.to_string())
            }),
        ModelsCommand::Settings {
            choice,
            set,
            clear,
            json,
            ..
        } => models_settings(&connection, choice, set, clear, *json, out),
    };
    match result {
        Ok(()) => Ok(EXIT_DONE),
        Err(message) => {
            writeln!(err, "error: {message}")?;
            Ok(EXIT_FAILED)
        }
    }
}

fn models_list(
    bus: &Connection,
    provider: Option<&str>,
    refresh: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let local = provider == Some("local") && !refresh;
    let document = match provider {
        None => json_reply(&call_service(bus, "ListModels", Vec::new())?)?,
        Some(_) if local => json_reply(&call_service(bus, "ListLocalModels", Vec::new())?)?,
        Some(provider) => json_reply(&call_service(
            bus,
            "ListProviderModels",
            vec![BusValue::string(provider), BusValue::Bool(refresh)],
        )?)?,
    };
    let mut text = String::new();
    if json {
        text = format!("{document:#}\n");
    } else if let Some(provider) = provider.filter(|_| !local) {
        text.push_str(&format!(
            "{provider}: models from {}\n",
            match document["source"].as_str() {
                Some("provider") => "the provider's own list, asked just now",
                Some("cache") => "the provider's own list, asked within a day",
                _ => "TUR's list (--refresh asks the provider)",
            }
        ));
        for model in items(&document["models"]) {
            text.push_str(&format!(
                "{provider}:{}{}\n",
                model["model"].as_str().unwrap_or_default(),
                if model["in_list"] == true {
                    ""
                } else {
                    " (not in TUR's list: no settings or prices known)"
                }
            ));
        }
    } else if local {
        text.push_str(&format!(
            "{}\n",
            document["message"].as_str().unwrap_or_default()
        ));
        for model in items(&document["models"]) {
            let tier = &model["tier"];
            text.push_str(&format!(
                "local:{} ({}) [{}], download {}{}{}\n  {}\n",
                model["id"].as_str().unwrap_or_default(),
                model["name"].as_str().unwrap_or_default(),
                tier["tier"].as_str().unwrap_or("not judged"),
                gigabytes(model["download_bytes"].as_u64().unwrap_or(0)),
                if model["installed"] == "installed" {
                    ", downloaded"
                } else {
                    ""
                },
                if model["licence"]["restricts_use"] == true {
                    ", licence with restrictions"
                } else {
                    ""
                },
                tier["message"].as_str().unwrap_or_default()
            ));
        }
    } else {
        for entry in items(&document["providers"]) {
            text.push_str(&format!(
                "{} ({}, {}): {} models listed\n",
                entry["id"].as_str().unwrap_or_default(),
                entry["name"].as_str().unwrap_or_default(),
                entry["adapter"].as_str().unwrap_or_default(),
                items(&entry["models"]).len()
            ));
        }
    }
    out.write_all(text.as_bytes())
        .map_err(|error| error.to_string())
}

fn say(out: &mut dyn Write, text: &str) -> Result<(), String> {
    writeln!(out, "{text}").map_err(|error| error.to_string())
}

fn models_use(
    bus: &Connection,
    incoming: &Receiver<Message>,
    choice: &str,
    yes: bool,
    accept_licence: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (provider, model) = split_choice(choice)?;
    if provider != "local" {
        call_service(
            bus,
            "SetProvider",
            vec![BusValue::string(provider), BusValue::string(model)],
        )?;
        return say(out, &format!("P@x uses {model} of {provider} now."));
    }
    let offers = json_reply(&call_service(bus, "ListLocalModels", Vec::new())?)?;
    let Some(offer) = items(&offers["models"])
        .iter()
        .find(|offer| offer["id"] == model)
    else {
        return Err(format!("local AI offers no model {model:?}"));
    };
    let name = offer["name"].as_str().unwrap_or(model);
    let size = offer["download_bytes"].as_u64().unwrap_or(0);
    say(
        out,
        &format!(
            "{name}: {}",
            offer["tier"]["message"].as_str().unwrap_or("not judged")
        ),
    )?;
    if offer["installed"] != "installed" {
        say(
            out,
            &format!(
                "It is not on this computer yet: it downloads {} from {}.",
                gigabytes(size),
                offer["repository"].as_str().unwrap_or("its publisher")
            ),
        )?;
        if !yes {
            return Err("nothing was downloaded: run again with --yes to download it".to_owned());
        }
    }
    if offer["licence"]["restricts_use"] == true {
        say(
            out,
            &format!(
                "Its licence restricts use: {}",
                offer["licence"]["restrictions"]
                    .as_str()
                    .unwrap_or_default()
            ),
        )?;
        if !accept_licence {
            return Err(
                "nothing was downloaded: run again with --accept-licence to accept it".to_owned(),
            );
        }
    }
    bus.add_match(&format!(
        "type='signal',interface='{}',member='LocalModelProgress'",
        dbus::INTERFACE
    ))
    .map_err(|error| error.to_string())?;
    let options = serde_json::json!({
        "download_bytes": size,
        "accept_licence": accept_licence,
    });
    call_service(
        bus,
        "UseLocalModel",
        vec![
            BusValue::string(model),
            BusValue::string(options.to_string()),
        ],
    )?;
    let started = std::time::Instant::now();
    let mut last_tenth: Option<u64> = None;
    while started.elapsed() < FOLLOW_TIMEOUT {
        let Ok(message) = incoming.recv_timeout(Duration::from_secs(60)) else {
            continue;
        };
        if message.member.as_deref() != Some("LocalModelProgress")
            || message.body.first().and_then(BusValue::as_str) != Some(model)
        {
            continue;
        }
        let Some(event) = message
            .body
            .get(1)
            .and_then(BusValue::as_str)
            .and_then(|text| crate::strict::value_from_str(text).ok())
        else {
            continue;
        };
        let text = event["message"].as_str().unwrap_or_default();
        match event["event"].as_str() {
            Some("progress") => {
                let done = event["bytes_done"].as_u64().unwrap_or(0);
                let total = event["bytes_total"].as_u64().unwrap_or(1).max(1);
                let tenth = done.saturating_mul(10) / total;
                if last_tenth != Some(tenth) {
                    last_tenth = Some(tenth);
                    say(out, text)?;
                }
            }
            Some("in-use") => return say(out, text),
            Some("failed" | "cancelled") => {
                return Err(format!(
                    "{} {text}",
                    event["code"].as_str().unwrap_or_default()
                ));
            }
            _ => say(out, text)?,
        }
    }
    Err("local AI did not finish in time; it may still be working".to_owned())
}

fn models_settings(
    bus: &Connection,
    choice: &str,
    set: &[String],
    clear: &[String],
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (provider, model) = split_choice(choice)?;
    if !set.is_empty() || !clear.is_empty() {
        let mut change = serde_json::Map::new();
        for item in set {
            let (name, value) = item
                .split_once('=')
                .ok_or_else(|| format!("{item:?} is not NAME=VALUE"))?;
            let value = crate::strict::value_from_str(value)
                .unwrap_or_else(|_| serde_json::Value::String(value.to_owned()));
            change.insert(name.to_owned(), value);
        }
        for name in clear {
            change.insert(name.clone(), serde_json::Value::Null);
        }
        call_service(
            bus,
            "SetGenerationSettings",
            vec![
                BusValue::string(provider),
                BusValue::string(model),
                BusValue::string(serde_json::Value::Object(change).to_string()),
            ],
        )?;
    }
    let document = json_reply(&call_service(
        bus,
        "GetGenerationSettings",
        vec![BusValue::string(provider), BusValue::string(model)],
    )?)?;
    let mut text = if json {
        format!("{document:#}\n")
    } else {
        format!(
            "{provider}:{model}\n  accepts: {}\n  chosen: {}\n  in effect: {}\n",
            document["accepts"], document["chosen"], document["effective"]
        )
    };
    if !json {
        for notice in items(&document["notices"]) {
            text.push_str(&format!("  {}\n", notice.as_str().unwrap_or_default()));
        }
    }
    out.write_all(text.as_bytes())
        .map_err(|error| error.to_string())
}
