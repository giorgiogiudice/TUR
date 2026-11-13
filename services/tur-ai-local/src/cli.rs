// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `tur-ai-local` command line.
//!
//! Commands that only read:
//!
//! * `hardware`: memory, processor, graphics devices and the engine's
//!   backends.
//! * `models`: the catalogue, with what is downloaded and what fits.
//! * `recommend`: the model that fits this computer best.
//! * `check MODEL`: whether one model fits, and why.
//! * `status`: whether a model is being served, where, and its memory.
//! * `codes`: the `TUR-AI-LOCAL-*` registry.
//!
//! Commands that act, only when a person runs them:
//!
//! * `pull MODEL`: download from the publisher (or import a file) and check
//!   size and SHA-256 before the model can be used.
//! * `verify MODEL`: check a downloaded model's files again.
//! * `remove MODEL`: delete a downloaded model.
//! * `serve MODEL`: run the model with the engine on the loopback interface
//!   until it is idle or stopped.
//! * `stop`: stop the model being served.
//! * `ensure MODEL`: everything a person's choice of a model needs, in one
//!   step: the licence and the memory checked, the disk's free space
//!   checked, the files downloaded (going on from a partial download) and
//!   checked, and the engine started; its progress as JSON lines.
//! * `cancel`: ask a running `ensure` to stop.
//!
//! Every command prints text, or JSON with `--json`
//! (`schemas/ai-local-report.schema.json`). `serve --json` prints one JSON
//! object per line: when the model is ready, and when it stopped.
//!
//! Exit status: 0 when the command did what was asked or answered the
//! question (a model that does not fit is an answer to `check`); 1 when it
//! could not, with a code on standard error; 2 for a usage error.

use std::ffi::OsString;
use std::fmt;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::SCHEMA_VERSION;
use crate::catalogue::{Capability, Catalogue, INSTALLED_CATALOGUE, Model, Tested, Thinking};
use crate::codes::{AiLocalCode, CodeEntry, registry};
use crate::download::{self, DownloadError};
use crate::engine::{self, DEFAULT_ENGINE, DEFAULT_PORT, Launch, Listen, Reasoning};
use crate::ensure;
use crate::fit::{self, DEFAULT_CONTEXT, Fit, Recommendation, Verdict};
use crate::hardware::{self, Hardware, Source};
use crate::human_bytes;
use crate::serve::{
    self, DEFAULT_IDLE_STOP, DEFAULT_START_TIMEOUT, KEY_FILE, ServeOptions, State, StopOutcome,
};
use crate::store::{self, InstallState, Store};

/// The parsed command line.
#[derive(Debug, Parser)]
#[command(
    name = "tur-ai-local",
    version,
    about = "TUR local AI: run AI models on this computer and serve them on the loopback \
             interface only, through an OpenAI-compatible API."
)]
pub struct Cli {
    /// Where things are.
    #[command(flatten)]
    pub places: Places,
    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Where the catalogue, the models, the state and the hardware are.
#[derive(Debug, Args)]
pub struct Places {
    /// The model catalogue directory
    #[arg(long, global = true, value_name = "DIR")]
    pub catalogue: Option<PathBuf>,
    /// The models folder. Without it, $XDG_DATA_HOME/tur/models
    #[arg(long, global = true, value_name = "DIR")]
    pub models_dir: Option<PathBuf>,
    /// The session's runtime folder. Without it, $XDG_RUNTIME_DIR
    #[arg(long, global = true, value_name = "DIR")]
    pub runtime_dir: Option<PathBuf>,
    /// The engine program (llama.cpp's llama-server)
    #[arg(long, global = true, value_name = "FILE")]
    pub engine: Option<PathBuf>,
    /// Read /proc and /sys under this directory instead of this computer's:
    /// for recorded machines
    #[arg(long, global = true, value_name = "DIR")]
    pub root: Option<PathBuf>,
    /// A file holding what `vulkaninfo --summary` printed, instead of
    /// running it
    #[arg(long, global = true, value_name = "FILE")]
    pub vulkaninfo: Option<PathBuf>,
}

/// The commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show the memory, processor and graphics devices, and the engine's backends
    Hardware(JsonArgs),
    /// List the models TUR offers, what is downloaded and what fits
    Models(ModelsArgs),
    /// Recommend the model that fits this computer best
    Recommend(ContextArgs),
    /// Say whether one model fits this computer, and why
    Check(CheckArgs),
    /// Download a model from its publisher and check it
    Pull(PullArgs),
    /// Check a downloaded model's files again
    Verify(ModelArgs),
    /// Delete a downloaded model
    Remove(ModelArgs),
    /// Run a model on this computer until it is idle or stopped
    Serve(ServeArgs),
    /// Show whether a model is being served
    Status(JsonArgs),
    /// Stop the model being served
    Stop(JsonArgs),
    /// Make a model ready to use: download it if needed (going on from a
    /// partial download), check it, and start it
    Ensure(EnsureArgs),
    /// Ask a running ensure to stop
    Cancel(JsonArgs),
    /// List the TUR-AI-LOCAL error codes
    Codes(JsonArgs),
}

/// Only `--json`.
#[derive(Debug, Args)]
pub struct JsonArgs {
    /// Print JSON instead of text
    #[arg(long)]
    pub json: bool,
}

/// The context size.
#[derive(Debug, Args)]
pub struct ContextArgs {
    /// Print JSON instead of text
    #[arg(long)]
    pub json: bool,
    /// The context size, in tokens
    #[arg(long, value_name = "TOKENS", default_value_t = DEFAULT_CONTEXT,
          value_parser = clap::value_parser!(u32).range(512..=1_048_576))]
    pub context: u32,
}

/// Arguments of `models`.
#[derive(Debug, Args)]
pub struct ModelsArgs {
    /// The context size.
    #[command(flatten)]
    pub context: ContextArgs,
    /// Do not look at the models folder: say nothing about what is
    /// downloaded. For callers that cannot see it, such as the assistant's
    /// sandbox
    #[arg(long)]
    pub catalogue_only: bool,
}

/// Arguments of `check`.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// The model's id
    #[arg(value_name = "MODEL")]
    pub model: String,
    /// The context size.
    #[command(flatten)]
    pub context: ContextArgs,
}

/// A model and `--json`.
#[derive(Debug, Args)]
pub struct ModelArgs {
    /// The model's id
    #[arg(value_name = "MODEL")]
    pub model: String,
    /// Print JSON instead of text
    #[arg(long)]
    pub json: bool,
}

/// Arguments of `pull`.
#[derive(Debug, Args)]
pub struct PullArgs {
    /// The model's id
    #[arg(value_name = "MODEL")]
    pub model: String,
    /// Print JSON instead of text
    #[arg(long)]
    pub json: bool,
    /// Accept the use restrictions of the model's licence, which the
    /// catalogue shows
    #[arg(long)]
    pub accept_licence: bool,
    /// Import the model file from here instead of downloading it. It is
    /// checked like a download, and copied, never moved
    #[arg(long, value_name = "FILE")]
    pub from_file: Option<PathBuf>,
}

/// Whether the model may think first, on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReasoningArg {
    /// Let it think
    On,
    /// Ask it to answer directly
    Off,
    /// Leave it to the model
    Auto,
}

impl From<ReasoningArg> for Reasoning {
    fn from(value: ReasoningArg) -> Reasoning {
        match value {
            ReasoningArg::On => Reasoning::On,
            ReasoningArg::Off => Reasoning::Off,
            ReasoningArg::Auto => Reasoning::Auto,
        }
    }
}

/// Arguments of `serve`.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// The model's id
    #[arg(value_name = "MODEL")]
    pub model: String,
    /// Print one JSON object per line instead of text
    #[arg(long)]
    pub json: bool,
    /// The address to listen on. Only loopback addresses without
    /// --allow-network
    #[arg(
        long,
        value_name = "ADDRESS",
        default_value = "127.0.0.1",
        conflicts_with = "socket"
    )]
    pub host: String,
    /// The port to listen on
    #[arg(long, value_name = "PORT", default_value_t = DEFAULT_PORT,
          value_parser = clap::value_parser!(u16).range(1..), conflicts_with = "socket")]
    pub port: u16,
    /// Listen on this Unix socket instead (an absolute path ending in .sock)
    #[arg(long, value_name = "PATH")]
    pub socket: Option<PathBuf>,
    /// Allow an address other devices on the network can reach. Settings
    /// sets this only while its network switch is on
    #[arg(long)]
    pub allow_network: bool,
    /// The context size, in tokens
    #[arg(long, value_name = "TOKENS", default_value_t = DEFAULT_CONTEXT,
          value_parser = clap::value_parser!(u32).range(512..=1_048_576))]
    pub context: u32,
    /// Stop after this many seconds without a request
    #[arg(long, value_name = "SECONDS", default_value_t = DEFAULT_IDLE_STOP.as_secs(),
          value_parser = clap::value_parser!(u64).range(5..))]
    pub idle_stop: u64,
    /// Give up if the model is not loaded after this many seconds
    #[arg(long, value_name = "SECONDS", default_value_t = DEFAULT_START_TIMEOUT.as_secs(),
          value_parser = clap::value_parser!(u64).range(5..))]
    pub start_timeout: u64,
    /// Whether the model may think before it answers
    #[arg(long, value_enum, value_name = "WHEN", default_value = "off")]
    pub reasoning: ReasoningArg,
}

/// Arguments of `ensure`.
#[derive(Debug, Args)]
pub struct EnsureArgs {
    /// The model's id
    #[arg(value_name = "MODEL")]
    pub model: String,
    /// Print one JSON object per line instead of text
    #[arg(long)]
    pub json: bool,
    /// Accept the use restrictions of the model's licence, which the
    /// catalogue shows
    #[arg(long)]
    pub accept_licence: bool,
    /// The context size, in tokens
    #[arg(long, value_name = "TOKENS", default_value_t = DEFAULT_CONTEXT,
          value_parser = clap::value_parser!(u32).range(512..=1_048_576))]
    pub context: u32,
    /// Download and check only; do not start the engine
    #[arg(long)]
    pub no_serve: bool,
    /// The port to listen on
    #[arg(long, value_name = "PORT", default_value_t = DEFAULT_PORT,
          value_parser = clap::value_parser!(u16).range(1..))]
    pub port: u16,
    /// Stop after this many seconds without a request
    #[arg(long, value_name = "SECONDS", default_value_t = DEFAULT_IDLE_STOP.as_secs(),
          value_parser = clap::value_parser!(u64).range(5..))]
    pub idle_stop: u64,
    /// Give up if the model is not loaded after this many seconds
    #[arg(long, value_name = "SECONDS", default_value_t = DEFAULT_START_TIMEOUT.as_secs(),
          value_parser = clap::value_parser!(u64).range(5..))]
    pub start_timeout: u64,
    /// Whether the model may think before it answers
    #[arg(long, value_enum, value_name = "WHEN", default_value = "off")]
    pub reasoning: ReasoningArg,
}

/// Why a command failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A condition with a registered error code.
    Coded {
        /// The code.
        code: AiLocalCode,
        /// What exactly went wrong.
        detail: String,
    },
    /// The command line asked for something that makes no sense.
    Usage(String),
    /// The result could not be written or serialised.
    Output(String),
}

impl Failure {
    fn coded(code: AiLocalCode, detail: impl Into<String>) -> Failure {
        Failure::Coded {
            code,
            detail: detail.into(),
        }
    }

    /// The process exit status for this failure.
    pub fn exit_status(&self) -> u8 {
        match self {
            Failure::Usage(_) => 2,
            Failure::Coded { .. } | Failure::Output(_) => 1,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Coded { code, detail } => {
                let entry = code.entry();
                write!(
                    f,
                    "{code}\n{}\n\nAction:\n{}\n\nTechnical detail:\n{detail}",
                    entry.user_message, entry.recovery_action
                )
            }
            Failure::Usage(message) => write!(f, "error: {message}"),
            Failure::Output(message) => write!(f, "error: cannot write output: {message}"),
        }
    }
}

/// Runs the command line and returns the process exit status.
///
/// `args` includes the program name. Results go to `stdout`; usage errors
/// and failures go to `stderr`.
pub fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            // Help and version requests arrive here too; they are not errors.
            let text = error.to_string();
            return if error.use_stderr() {
                let _ = stderr.write_all(text.as_bytes());
                2
            } else {
                let _ = stdout.write_all(text.as_bytes());
                0
            };
        }
    };
    let places = &cli.places;
    let result = match &cli.command {
        Command::Hardware(args) => hardware_command(places, args, stdout),
        Command::Models(args) => models_command(places, args, stdout),
        Command::Recommend(args) => recommend_command(places, args, stdout),
        Command::Check(args) => check_command(places, args, stdout),
        Command::Pull(args) => pull_command(places, args, stdout, stderr),
        Command::Verify(args) => verify_command(places, args, stdout),
        Command::Remove(args) => remove_command(places, args, stdout),
        Command::Serve(args) => serve_command(places, args, stdout),
        Command::Status(args) => status_command(places, args, stdout),
        Command::Stop(args) => stop_command(places, args, stdout),
        Command::Ensure(args) => ensure_command(places, args, stdout),
        Command::Cancel(args) => cancel_command(places, args, stdout),
        Command::Codes(args) => codes_command(args, stdout),
    };
    match result {
        Ok(()) => 0,
        Err(failure) => {
            let _ = writeln!(stderr, "{failure}");
            failure.exit_status()
        }
    }
}

// ------------------------------------------------------------------ places

fn catalogue_dir(places: &Places) -> PathBuf {
    places
        .catalogue
        .clone()
        .unwrap_or_else(|| PathBuf::from(INSTALLED_CATALOGUE))
}

fn load_catalogue(places: &Places) -> Result<Catalogue, Failure> {
    let directory = catalogue_dir(places);
    Catalogue::load(&directory).map_err(|problems| {
        Failure::coded(
            AiLocalCode::CatalogueInvalid,
            format!("{}:\n{}", directory.display(), problems.join("\n")),
        )
    })
}

fn find_model<'a>(catalogue: &'a Catalogue, id: &str) -> Result<&'a Model, Failure> {
    catalogue.get(id).ok_or_else(|| {
        Failure::coded(
            AiLocalCode::UnknownModel,
            format!(
                "{id:?} is not in {}; the catalogue has {}",
                catalogue.directory.display(),
                catalogue
                    .models
                    .iter()
                    .map(|model| model.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    })
}

fn models_store(places: &Places) -> Option<Store> {
    places
        .models_dir
        .clone()
        .or_else(store::default_root)
        .map(|root| Store { root })
}

fn require_store(places: &Places) -> Result<Store, Failure> {
    models_store(places).ok_or_else(|| {
        Failure::coded(
            AiLocalCode::StorageFailed,
            "neither --models-dir, XDG_DATA_HOME nor HOME is set",
        )
    })
}

fn hardware_source(places: &Places) -> Source {
    Source {
        root: places.root.clone().unwrap_or_else(|| PathBuf::from("/")),
        vulkaninfo: places.vulkaninfo.clone(),
    }
}

fn read_hardware(places: &Places) -> Result<Hardware, Failure> {
    hardware::read(&hardware_source(places))
        .map_err(|error| Failure::coded(AiLocalCode::HardwareUnreadable, error.0))
}

fn engine_program(places: &Places) -> PathBuf {
    places
        .engine
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ENGINE))
}

/// The engine as installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineInfo {
    /// The program.
    pub program: String,
    /// It exists.
    pub present: bool,
    /// Its backend libraries, for example `cpu`, `vulkan`.
    pub backends: Vec<String>,
    /// It has a Vulkan backend.
    pub vulkan_backend: bool,
}

fn engine_info(places: &Places) -> EngineInfo {
    let program = engine_program(places);
    let backends = engine::backends(&program);
    EngineInfo {
        program: program.to_string_lossy().into_owned(),
        present: program.is_file(),
        vulkan_backend: backends.iter().any(|backend| backend == "vulkan"),
        backends,
    }
}

fn write_json<T: Serialize>(out: &mut dyn Write, value: &T) -> Result<(), Failure> {
    let text =
        serde_json::to_string_pretty(value).map_err(|error| Failure::Output(error.to_string()))?;
    writeln!(out, "{text}").map_err(|error| Failure::Output(error.to_string()))
}

fn write_text(out: &mut dyn Write, text: &str) -> Result<(), Failure> {
    out.write_all(text.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|error| Failure::Output(error.to_string()))
}

// ---------------------------------------------------------------- hardware

#[derive(Serialize)]
struct HardwareReport<'a> {
    schema_version: u32,
    kind: &'static str,
    hardware: &'a Hardware,
    engine: EngineInfo,
}

fn hardware_command(places: &Places, args: &JsonArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let hardware = read_hardware(places)?;
    let engine = engine_info(places);
    if args.json {
        return write_json(
            out,
            &HardwareReport {
                schema_version: SCHEMA_VERSION,
                kind: "hardware",
                hardware: &hardware,
                engine,
            },
        );
    }
    let mut text = String::new();
    let memory = hardware.memory;
    text.push_str(&format!(
        "Memory: {} in all, {} free now.\n",
        human_bytes(memory.total_bytes),
        human_bytes(memory.available_bytes)
    ));
    let cpu = &hardware.cpu;
    text.push_str(&format!(
        "Processor: {} ({}), {} logical processors{}.\n",
        cpu.model.as_deref().unwrap_or("model not known"),
        cpu.architecture,
        cpu.logical_cpus,
        cpu.physical_cores
            .map(|cores| format!(", {cores} cores"))
            .unwrap_or_default()
    ));
    text.push_str(&format!(
        "Features for inference: {}.\n",
        if cpu.features.is_empty() {
            "none known".to_owned()
        } else {
            cpu.features.join(", ")
        }
    ));
    for gpu in &hardware.gpus {
        text.push_str(&format!(
            "Graphics device {}: {} {}, driver {}{}.\n",
            gpu.card,
            gpu.vendor,
            gpu.device_id.as_deref().unwrap_or("?"),
            gpu.driver.as_deref().unwrap_or("none"),
            gpu.vram_bytes
                .map(|bytes| format!(", {} of its own memory", human_bytes(bytes)))
                .unwrap_or_default()
        ));
    }
    text.push_str(&format!("Vulkan: {}.\n", hardware.vulkan.status));
    for device in &hardware.vulkan.devices {
        text.push_str(&format!(
            "  GPU{}: {} ({}, driver {}){}\n",
            device.index,
            device.name.as_deref().unwrap_or("?"),
            device.device_type,
            device.driver_name.as_deref().unwrap_or("?"),
            if device.usable { ", usable" } else { "" }
        ));
    }
    text.push_str(&format!(
        "Engine: {} {}; backends: {}.\n",
        engine.program,
        if engine.present {
            "installed"
        } else {
            "not installed"
        },
        if engine.backends.is_empty() {
            "none found".to_owned()
        } else {
            engine.backends.join(", ")
        }
    ));
    write_text(out, &text)
}

// ------------------------------------------------------------------ models

/// One file of a model, in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileView {
    /// Its name.
    pub name: String,
    /// The publisher's URL.
    pub url: String,
    /// Its size.
    pub size_bytes: u64,
    /// Its SHA-256 digest.
    pub sha256: String,
}

/// The licence, in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LicenceView {
    /// Its identifier.
    pub id: String,
    /// Where it is.
    pub url: String,
    /// What it lets a person do.
    pub summary: String,
    /// Its use restrictions, or "none".
    pub restrictions: String,
    /// It restricts use.
    pub restricts_use: bool,
}

/// A model, in a report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelView {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its publisher.
    pub publisher: String,
    /// The publisher's repository.
    pub repository: String,
    /// The pinned revision.
    pub revision: String,
    /// The quantisation.
    pub quantisation: String,
    /// The number of parameters.
    pub parameters: String,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// How it thinks.
    pub thinking: Thinking,
    /// The longest context it supports.
    pub context_length: u32,
    /// The bytes downloaded.
    pub download_bytes: u64,
    /// The memory it needs at the report's context.
    pub memory_bytes: u64,
    /// Its files.
    pub files: Vec<FileView>,
    /// Languages.
    pub languages: Vec<String>,
    /// Further language support claimed.
    pub languages_note: Option<String>,
    /// What it is good for.
    pub good_for: Vec<String>,
    /// The licence.
    pub licence: LicenceView,
    /// Whether it is downloaded; null when the models folder was not looked
    /// at.
    pub installed: Option<InstallState>,
    /// Why it is damaged, when it is.
    pub install_problem: Option<String>,
    /// Whether it fits; null when the memory could not be read.
    pub fit: Option<Fit>,
    /// How it suits this computer: runs well, slow, does not fit; null when
    /// the memory could not be read, and for an embeddings model.
    pub tier: Option<fit::ModelTier>,
    /// What TUR measured.
    pub tested: Vec<Tested>,
}

fn model_view(
    model: &Model,
    store: Option<&Store>,
    hardware: Option<&Hardware>,
    context: u32,
    tiers: &[fit::ModelTier],
) -> ModelView {
    let (installed, install_problem) = match store {
        Some(store) => {
            let (state, problem) = store.state(model);
            (Some(state), problem)
        }
        None => (None, None),
    };
    ModelView {
        id: model.id.clone(),
        name: model.name.clone(),
        publisher: model.publisher.clone(),
        repository: model.repository.clone(),
        revision: model.revision.clone(),
        quantisation: model.quantisation.clone(),
        parameters: model.parameters.clone(),
        capabilities: model.capabilities.clone(),
        thinking: model.thinking,
        context_length: model.context_length,
        download_bytes: model.download_bytes(),
        memory_bytes: model.memory_for_context(context),
        files: model
            .files()
            .into_iter()
            .map(|file| FileView {
                name: file.name.clone(),
                url: file.url.clone(),
                size_bytes: file.size_bytes,
                sha256: file.sha256.clone(),
            })
            .collect(),
        languages: model.languages.clone(),
        languages_note: model.languages_note.clone(),
        good_for: model.good_for.clone(),
        licence: LicenceView {
            id: model.licence.id.clone(),
            url: model.licence.url.clone(),
            summary: model.licence.summary.clone(),
            restrictions: model.licence.restrictions.clone(),
            restricts_use: model.licence.restricts_use(),
        },
        installed,
        install_problem,
        fit: hardware.map(|hardware| fit::judge(model, hardware, context)),
        tier: tiers.iter().find(|tier| tier.model == model.id).cloned(),
        tested: model.tested.clone(),
    }
}

#[derive(Serialize)]
struct ModelsReport {
    schema_version: u32,
    kind: &'static str,
    catalogue: String,
    models_dir: Option<String>,
    context: u32,
    models: Vec<ModelView>,
    other_folders: Vec<String>,
}

fn capability_words(capabilities: &[Capability]) -> String {
    capabilities
        .iter()
        .map(|capability| match capability {
            Capability::Chat => "conversation",
            Capability::Tools => "tool calling",
            Capability::Vision => "image input",
            Capability::Embeddings => "embeddings",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn models_command(places: &Places, args: &ModelsArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let catalogue = load_catalogue(places)?;
    let store = if args.catalogue_only {
        None
    } else {
        models_store(places)
    };
    let hardware = hardware::read(&hardware_source(places)).ok();
    let context = args.context.context;
    let build = fit::EngineBuild::from_backends(&engine::backends(&engine_program(places)));
    let tiers = hardware
        .as_ref()
        .map(|hardware| fit::tiers(&catalogue, hardware, context, build))
        .unwrap_or_default();
    let models: Vec<ModelView> = catalogue
        .models
        .iter()
        .map(|model| model_view(model, store.as_ref(), hardware.as_ref(), context, &tiers))
        .collect();
    let other_folders: Vec<String> = store
        .as_ref()
        .map(|store| {
            store
                .folders()
                .into_iter()
                .filter(|name| catalogue.get(name).is_none())
                .collect()
        })
        .unwrap_or_default();
    if args.context.json {
        return write_json(
            out,
            &ModelsReport {
                schema_version: SCHEMA_VERSION,
                kind: "models",
                catalogue: catalogue.directory.to_string_lossy().into_owned(),
                models_dir: store.map(|store| store.root.to_string_lossy().into_owned()),
                context,
                models,
                other_folders,
            },
        );
    }
    let mut text = String::new();
    for view in &models {
        let installed = match view.installed {
            Some(InstallState::Installed) => " [downloaded]",
            Some(InstallState::Damaged) => " [damaged]",
            _ => "",
        };
        text.push_str(&format!("{} — {}{}\n", view.id, view.name, installed));
        text.push_str(&format!(
            "  {} by {}, {}; download {}, needs about {} of memory.\n",
            view.parameters,
            view.publisher,
            view.quantisation,
            human_bytes(view.download_bytes),
            human_bytes(view.memory_bytes)
        ));
        text.push_str(&format!(
            "  Can do: {}.\n",
            capability_words(&view.capabilities)
        ));
        text.push_str(&format!("  Good for: {}\n", view.good_for.join(" ")));
        text.push_str(&format!(
            "  Licence: {}. {} Restrictions: {}\n",
            view.licence.id, view.licence.summary, view.licence.restrictions
        ));
        if let Some(fit) = &view.fit {
            text.push_str(&format!("  {}\n", fit.message));
        }
        if let Some(tier) = &view.tier {
            text.push_str(&format!(
                "  {} {}\n",
                tier.message,
                tier.reasons.last().map_or("", String::as_str)
            ));
        }
    }
    for name in &other_folders {
        text.push_str(&format!(
            "{name} — not in the catalogue, not reviewed by TUR.\n"
        ));
    }
    write_text(out, &text)
}

// --------------------------------------------------------------- recommend

#[derive(Serialize)]
struct RecommendReport<'a> {
    schema_version: u32,
    kind: &'static str,
    context: u32,
    recommendation: &'a Recommendation,
}

fn recommend_command(
    places: &Places,
    args: &ContextArgs,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let catalogue = load_catalogue(places)?;
    let hardware = read_hardware(places)?;
    let build = fit::EngineBuild::from_backends(&engine::backends(&engine_program(places)));
    let recommendation = fit::recommend(&catalogue, &hardware, args.context, build);
    if args.json {
        return write_json(
            out,
            &RecommendReport {
                schema_version: SCHEMA_VERSION,
                kind: "recommend",
                context: args.context,
                recommendation: &recommendation,
            },
        );
    }
    let mut text = format!("{}\n{}\n", recommendation.code, recommendation.message);
    for reason in &recommendation.reasons {
        text.push_str(&format!("  {reason}\n"));
    }
    if let Some(id) = &recommendation.vision_model {
        text.push_str(&format!("For questions about images: {id}.\n"));
    }
    if let Some(id) = &recommendation.embeddings_model {
        text.push_str(&format!("For search by meaning (embeddings): {id}.\n"));
    }
    text.push_str("\nEvery model, for this computer:\n");
    for tier in &recommendation.tiers {
        text.push_str(&format!(
            "{} [{}] {}\n",
            tier.model, tier.tier, tier.message
        ));
        for reason in &tier.reasons {
            text.push_str(&format!("  {reason}\n"));
        }
    }
    write_text(out, &text)
}

// ------------------------------------------------------------------- check

#[derive(Serialize)]
struct CheckReport<'a> {
    schema_version: u32,
    kind: &'static str,
    fit: &'a Fit,
}

fn check_command(places: &Places, args: &CheckArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let catalogue = load_catalogue(places)?;
    let model = find_model(&catalogue, &args.model)?;
    let hardware = read_hardware(places)?;
    let fit = fit::judge(model, &hardware, args.context.context);
    if args.context.json {
        return write_json(
            out,
            &CheckReport {
                schema_version: SCHEMA_VERSION,
                kind: "check",
                fit: &fit,
            },
        );
    }
    write_text(out, &format!("{}\n{}\n", fit.code, fit.message))
}

// -------------------------------------------------------------------- pull

/// What happened to one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileResult {
    /// Its name.
    pub name: String,
    /// Where it is now.
    pub path: String,
    /// Its size.
    pub size_bytes: u64,
    /// Its SHA-256 digest, as checked.
    pub sha256: String,
    /// `downloaded`, `imported`, `already-present` or `verified`.
    pub action: &'static str,
}

#[derive(Serialize)]
struct PullReport {
    schema_version: u32,
    kind: &'static str,
    model: String,
    code: AiLocalCode,
    message: String,
    licence: LicenceView,
    files: Vec<FileResult>,
}

fn licence_view(model: &Model) -> LicenceView {
    LicenceView {
        id: model.licence.id.clone(),
        url: model.licence.url.clone(),
        summary: model.licence.summary.clone(),
        restrictions: model.licence.restrictions.clone(),
        restricts_use: model.licence.restricts_use(),
    }
}

fn storage_failure(path: &Path, error: &std::io::Error) -> Failure {
    Failure::coded(
        AiLocalCode::StorageFailed,
        format!("{}: {error}", path.display()),
    )
}

fn download_failure(name: &str, error: &DownloadError) -> Failure {
    Failure::coded(error.code(), format!("{name}: {error}"))
}

fn pull_command(
    places: &Places,
    args: &PullArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let catalogue = load_catalogue(places)?;
    let model = find_model(&catalogue, &args.model)?;
    if model.licence.restricts_use() && !args.accept_licence {
        return Err(Failure::coded(
            AiLocalCode::LicenceNotAccepted,
            format!(
                "{} ({}): {}",
                model.licence.id, model.licence.url, model.licence.restrictions
            ),
        ));
    }
    if args.from_file.is_some() && model.projector.is_some() {
        return Err(Failure::Usage(format!(
            "{} has a projector file as well; --from-file imports a single-file model only",
            model.id
        )));
    }
    // A model that can never run here is not downloaded.
    if let Ok(hardware) = hardware::read(&hardware_source(places)) {
        let fit = fit::judge(model, &hardware, DEFAULT_CONTEXT.min(model.context_length));
        if fit.verdict == Verdict::TooLarge {
            return Err(Failure::coded(AiLocalCode::TooLarge, fit.message));
        }
    }
    let store = require_store(places)?;
    let dir = store
        .model_dir(&model.id)
        .ok_or_else(|| Failure::Usage(format!("{:?} is not a model id", model.id)))?;
    let (state, _) = store.state(model);
    let mut files = Vec::new();
    if state == InstallState::Installed {
        for file in model.files() {
            files.push(FileResult {
                name: file.name.clone(),
                path: dir.join(&file.name).to_string_lossy().into_owned(),
                size_bytes: file.size_bytes,
                sha256: file.sha256.clone(),
                action: "already-present",
            });
        }
    } else {
        if state == InstallState::Damaged {
            store
                .remove(&model.id)
                .map_err(|error| storage_failure(&dir, &error))?;
        }
        store::create_private_dir(&dir).map_err(|error| storage_failure(&dir, &error))?;
        let agent = download::agent();
        for file in model.files() {
            let destination = dir.join(&file.name);
            let total = file.size_bytes;
            let mut next_report = 0u64;
            // Progress is for a person at a terminal: a program reading
            // standard error finds the code of a failure on its first line.
            let quiet = args.json || !std::io::stderr().is_terminal();
            let mut progress = |written: u64| {
                if !quiet && written >= next_report {
                    let _ = writeln!(
                        err,
                        "{}: {} of {} ({}%)",
                        file.name,
                        human_bytes(written),
                        human_bytes(total),
                        written.saturating_mul(100) / total.max(1)
                    );
                    next_report = written.saturating_add(total / 10).max(1);
                }
            };
            let (result, action) = match &args.from_file {
                Some(source) => (
                    download::import(source, file, &destination, &mut progress),
                    "imported",
                ),
                None => (
                    download::download(&agent, file, &destination, &mut progress),
                    "downloaded",
                ),
            };
            if let Err(error) = result {
                return Err(download_failure(&file.name, &error));
            }
            files.push(FileResult {
                name: file.name.clone(),
                path: destination.to_string_lossy().into_owned(),
                size_bytes: file.size_bytes,
                sha256: file.sha256.clone(),
                action,
            });
        }
        store
            .write_verified(model, store::now_seconds())
            .map_err(|error| storage_failure(&dir, &error))?;
    }
    let message = AiLocalCode::Installed.entry().user_message.to_owned();
    if args.json {
        return write_json(
            out,
            &PullReport {
                schema_version: SCHEMA_VERSION,
                kind: "pull",
                model: model.id.clone(),
                code: AiLocalCode::Installed,
                message,
                licence: licence_view(model),
                files,
            },
        );
    }
    let mut text = format!("{}\n{} {message}\n", AiLocalCode::Installed, model.name);
    for file in &files {
        text.push_str(&format!(
            "  {} ({}, {}): {}\n",
            file.name,
            human_bytes(file.size_bytes),
            file.action,
            file.path
        ));
    }
    text.push_str(&format!(
        "Licence: {}. {}\n",
        model.licence.id, model.licence.summary
    ));
    write_text(out, &text)
}

// ------------------------------------------------------------------ verify

#[derive(Serialize)]
struct VerifyReport {
    schema_version: u32,
    kind: &'static str,
    model: String,
    code: AiLocalCode,
    message: String,
    files: Vec<FileResult>,
}

fn verify_command(places: &Places, args: &ModelArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let catalogue = load_catalogue(places)?;
    let model = find_model(&catalogue, &args.model)?;
    let store = require_store(places)?;
    let (state, problem) = store.state(model);
    match state {
        InstallState::Absent => {
            return Err(Failure::coded(
                AiLocalCode::NotDownloaded,
                format!(
                    "{} has no verified files in {}",
                    model.id,
                    store.root.display()
                ),
            ));
        }
        InstallState::Damaged => {
            return Err(Failure::coded(
                AiLocalCode::FileChanged,
                problem.unwrap_or_default(),
            ));
        }
        InstallState::Installed => {}
    }
    let mut files = Vec::new();
    for file in model.files() {
        let path = store
            .file_path(model, &file.name)
            .ok_or_else(|| Failure::Usage(format!("{:?} is not a model id", model.id)))?;
        download::check_existing(&path, file).map_err(|error| {
            Failure::coded(AiLocalCode::FileChanged, format!("{}: {error}", file.name))
        })?;
        files.push(FileResult {
            name: file.name.clone(),
            path: path.to_string_lossy().into_owned(),
            size_bytes: file.size_bytes,
            sha256: file.sha256.clone(),
            action: "verified",
        });
    }
    let message = AiLocalCode::Installed.entry().user_message.to_owned();
    if args.json {
        return write_json(
            out,
            &VerifyReport {
                schema_version: SCHEMA_VERSION,
                kind: "verify",
                model: model.id.clone(),
                code: AiLocalCode::Installed,
                message,
                files,
            },
        );
    }
    write_text(
        out,
        &format!(
            "{}\n{}: every file has the size and SHA-256 digest the catalogue gives.\n",
            AiLocalCode::Installed,
            model.name
        ),
    )
}

// ------------------------------------------------------------------ remove

#[derive(Serialize)]
struct RemoveReport {
    schema_version: u32,
    kind: &'static str,
    model: String,
    code: AiLocalCode,
    message: String,
    freed_bytes: u64,
}

fn remove_command(places: &Places, args: &ModelArgs, out: &mut dyn Write) -> Result<(), Failure> {
    if !crate::catalogue::is_model_id(&args.model) {
        return Err(Failure::Usage(format!(
            "{:?} is not a model id",
            args.model
        )));
    }
    if let Some(runtime) = serve::runtime_dir(places.runtime_dir.as_deref())
        && let Some(state) = serve::read_state(&runtime)
        && serve::supervisor_running(&state)
        && state.model == args.model
    {
        return Err(Failure::coded(
            AiLocalCode::AlreadyRunning,
            format!("{} is being served; stop it before removing it", args.model),
        ));
    }
    let store = require_store(places)?;
    let freed = store
        .remove(&args.model)
        .map_err(|error| storage_failure(&store.root.join(&args.model), &error))?;
    let Some(freed) = freed else {
        return Err(Failure::coded(
            AiLocalCode::NotDownloaded,
            format!("{} has no folder in {}", args.model, store.root.display()),
        ));
    };
    let message = AiLocalCode::Removed.entry().user_message.to_owned();
    if args.json {
        return write_json(
            out,
            &RemoveReport {
                schema_version: SCHEMA_VERSION,
                kind: "remove",
                model: args.model.clone(),
                code: AiLocalCode::Removed,
                message,
                freed_bytes: freed,
            },
        );
    }
    write_text(
        out,
        &format!(
            "{}\n{} {} freed.\n",
            AiLocalCode::Removed,
            message,
            human_bytes(freed)
        ),
    )
}

// ------------------------------------------------------------------- serve

#[derive(Serialize)]
struct ServeEvent<'a> {
    schema_version: u32,
    kind: &'static str,
    event: &'static str,
    code: AiLocalCode,
    message: String,
    model: &'a str,
    state: Option<&'a State>,
    peak_memory_bytes: Option<u64>,
}

fn write_line_json<T: Serialize>(out: &mut dyn Write, value: &T) -> Result<(), Failure> {
    let text = serde_json::to_string(value).map_err(|error| Failure::Output(error.to_string()))?;
    writeln!(out, "{text}")
        .and_then(|()| out.flush())
        .map_err(|error| Failure::Output(error.to_string()))
}

fn serve_command(places: &Places, args: &ServeArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let catalogue = load_catalogue(places)?;
    let model = find_model(&catalogue, &args.model)?;
    if args.context > model.context_length {
        return Err(Failure::Usage(format!(
            "{} supports a context of at most {} tokens",
            model.id, model.context_length
        )));
    }
    // Where it may listen is decided before anything else.
    let listen = match &args.socket {
        Some(path) => Listen::Socket(engine::check_socket(path).map_err(Failure::Usage)?),
        None => Listen::Tcp {
            address: engine::check_host(&args.host, args.allow_network).map_err(|detail| {
                if detail.contains("not a loopback address") {
                    Failure::coded(AiLocalCode::NetworkBindRefused, detail)
                } else {
                    Failure::Usage(detail)
                }
            })?,
            port: args.port,
        },
    };
    let store = require_store(places)?;
    let (state, problem) = store.state(model);
    match state {
        InstallState::Installed => {}
        InstallState::Absent => {
            return Err(Failure::coded(
                AiLocalCode::NotDownloaded,
                format!(
                    "{} has no verified files in {}",
                    model.id,
                    store.root.display()
                ),
            ));
        }
        InstallState::Damaged => {
            return Err(Failure::coded(
                AiLocalCode::FileChanged,
                problem.unwrap_or_default(),
            ));
        }
    }
    let hardware = read_hardware(places)?;
    let fit = fit::judge(model, &hardware, args.context);
    if fit.verdict != Verdict::Fits {
        return Err(Failure::coded(fit.code, fit.message));
    }
    let runtime = serve::runtime_dir(places.runtime_dir.as_deref()).ok_or_else(|| {
        Failure::coded(
            AiLocalCode::RuntimeDirMissing,
            "XDG_RUNTIME_DIR is not set and no --runtime-dir was given",
        )
    })?;
    let model_path = store
        .file_path(model, &model.file.name)
        .ok_or_else(|| Failure::Usage(format!("{:?} is not a model id", model.id)))?;
    let projector_path = model
        .projector
        .as_ref()
        .and_then(|projector| store.file_path(model, &projector.name));
    let (reasoning, pooling) = engine::model_options(model, args.reasoning.into());
    let options = ServeOptions {
        launch: Launch {
            program: engine_program(places),
            model_id: model.id.clone(),
            model_path,
            projector_path,
            listen,
            context: args.context,
            api_key_file: runtime.join(KEY_FILE),
            reasoning,
            pooling,
        },
        network_allowed: args.allow_network,
        idle_stop: Duration::from_secs(args.idle_stop),
        start_timeout: Duration::from_secs(args.start_timeout),
        runtime,
    };
    let json = args.json;
    let mut write_error: Option<Failure> = None;
    let mut on_ready = |state: &State| {
        let message = format!(
            "{} is running on {}. Programs on this computer can use it with the key in {}. It \
             stops after {} seconds without a request.",
            state.model,
            state
                .base_url
                .clone()
                .or_else(|| state.socket.clone())
                .unwrap_or_default(),
            state.api_key_file,
            state.idle_stop_seconds
        );
        let result = if json {
            write_line_json(
                out,
                &ServeEvent {
                    schema_version: SCHEMA_VERSION,
                    kind: "serve-event",
                    event: "ready",
                    code: AiLocalCode::Serving,
                    message,
                    model: &state.model,
                    state: Some(state),
                    peak_memory_bytes: None,
                },
            )
        } else {
            write_text(out, &format!("{}\n{message}\n", AiLocalCode::Serving))
        };
        if let Err(failure) = result {
            write_error = Some(failure);
        }
    };
    let stopped = serve::serve(&options, &mut on_ready)
        .map_err(|error| Failure::coded(error.code, error.detail))?;
    if let Some(failure) = write_error {
        return Err(failure);
    }
    let message = stopped.code.entry().user_message.to_owned();
    if json {
        return write_line_json(
            out,
            &ServeEvent {
                schema_version: SCHEMA_VERSION,
                kind: "serve-event",
                event: "stopped",
                code: stopped.code,
                message,
                model: &stopped.state.model,
                state: None,
                peak_memory_bytes: stopped.peak_memory_bytes,
            },
        );
    }
    write_text(
        out,
        &format!(
            "{}\n{message}{}\n",
            stopped.code,
            stopped
                .peak_memory_bytes
                .map(|bytes| format!(" The engine used at most {}.", human_bytes(bytes)))
                .unwrap_or_default()
        ),
    )
}

// ------------------------------------------------------------------ status

string_enum! {
    /// What the engine's health check said.
    pub enum Health {
        /// It answered and is ready.
        Ok => "ok",
        /// It answered and is still loading the model.
        Loading => "loading",
        /// It did not answer.
        Unreachable => "unreachable",
        /// Not asked: nothing is running.
        NotChecked => "not-checked",
    }
}

/// Memory of the engine process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProcessMemory {
    /// Resident now.
    pub rss_bytes: Option<u64>,
    /// The most it was resident.
    pub peak_rss_bytes: Option<u64>,
}

#[derive(Serialize)]
struct StatusReport {
    schema_version: u32,
    kind: &'static str,
    running: Option<bool>,
    code: AiLocalCode,
    message: String,
    stale: bool,
    state: Option<State>,
    idle_seconds: Option<u64>,
    memory: Option<ProcessMemory>,
    health: Health,
    engine: EngineInfo,
}

fn status_command(places: &Places, args: &JsonArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let engine = engine_info(places);
    let mut report = StatusReport {
        schema_version: SCHEMA_VERSION,
        kind: "status",
        running: None,
        code: AiLocalCode::StateNotVisible,
        message: AiLocalCode::StateNotVisible.entry().user_message.to_owned(),
        stale: false,
        state: None,
        idle_seconds: None,
        memory: None,
        health: Health::NotChecked,
        engine,
    };
    if let Some(runtime) = serve::runtime_dir(places.runtime_dir.as_deref()) {
        let state_path = runtime.join(serve::STATE_FILE);
        match serve::read_state(&runtime) {
            Some(state) if serve::supervisor_running(&state) => {
                let (rss, peak) = serve::process_memory(state.engine_pid);
                let listen = match (&state.address, state.port, &state.socket) {
                    (Some(address), Some(port), _) => address
                        .parse()
                        .ok()
                        .map(|address| Listen::Tcp { address, port }),
                    (_, _, Some(socket)) => Some(Listen::Socket(PathBuf::from(socket))),
                    _ => None,
                };
                report.health = match listen.map(|listen| {
                    serve::local_get(&listen, "/health", None, Duration::from_secs(2))
                }) {
                    Some(Ok((200, _))) => Health::Ok,
                    Some(Ok((503, _))) => Health::Loading,
                    _ => Health::Unreachable,
                };
                report.running = Some(true);
                report.code = AiLocalCode::Serving;
                report.message = format!(
                    "Local AI is running {} on {}.",
                    state.model,
                    state
                        .base_url
                        .clone()
                        .or_else(|| state.socket.clone())
                        .unwrap_or_default()
                );
                report.idle_seconds =
                    Some(store::now_seconds().saturating_sub(state.last_activity));
                report.memory = Some(ProcessMemory {
                    rss_bytes: rss,
                    peak_rss_bytes: peak,
                });
                report.state = Some(state);
            }
            Some(state) => {
                report.running = Some(false);
                report.stale = true;
                report.code = AiLocalCode::NotRunning;
                report.message = AiLocalCode::NotRunning.entry().user_message.to_owned();
                report.state = Some(state);
            }
            None if state_path.exists() => {}
            None => {
                report.running = Some(false);
                report.code = AiLocalCode::NotRunning;
                report.message = AiLocalCode::NotRunning.entry().user_message.to_owned();
            }
        }
    }
    if args.json {
        return write_json(out, &report);
    }
    let mut text = format!("{}\n{}\n", report.code, report.message);
    if let (Some(state), Some(true)) = (&report.state, report.running) {
        if let Some(memory) = report.memory {
            text.push_str(&format!(
                "Memory: {} now, at most {}.\n",
                memory
                    .rss_bytes
                    .map(human_bytes)
                    .unwrap_or_else(|| "?".to_owned()),
                memory
                    .peak_rss_bytes
                    .map(human_bytes)
                    .unwrap_or_else(|| "?".to_owned())
            ));
        }
        text.push_str(&format!(
            "Engine process {}, health {}; access key in {}.\n",
            state.engine_pid, report.health, state.api_key_file
        ));
        text.push_str(&format!(
            "Idle for {} seconds; it stops after {} seconds without a request.\n",
            report.idle_seconds.unwrap_or(0),
            state.idle_stop_seconds
        ));
    }
    write_text(out, &text)
}

// -------------------------------------------------------------------- stop

#[derive(Serialize)]
struct StopReport {
    schema_version: u32,
    kind: &'static str,
    code: AiLocalCode,
    message: String,
    model: Option<String>,
}

fn stop_command(places: &Places, args: &JsonArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let runtime = serve::runtime_dir(places.runtime_dir.as_deref()).ok_or_else(|| {
        Failure::coded(
            AiLocalCode::RuntimeDirMissing,
            "XDG_RUNTIME_DIR is not set and no --runtime-dir was given",
        )
    })?;
    let outcome = serve::request_stop(&runtime, Duration::from_secs(30))
        .map_err(|error| Failure::coded(error.code, error.detail))?;
    let (code, model) = match outcome {
        StopOutcome::NotRunning => (AiLocalCode::NotRunning, None),
        StopOutcome::Stopped(state) => (AiLocalCode::Stopped, Some(state.model)),
    };
    let message = code.entry().user_message.to_owned();
    if args.json {
        return write_json(
            out,
            &StopReport {
                schema_version: SCHEMA_VERSION,
                kind: "stop",
                code,
                message,
                model,
            },
        );
    }
    write_text(out, &format!("{code}\n{message}\n"))
}

// ------------------------------------------------------------------ ensure

fn reasoning_text(reasoning: ReasoningArg) -> &'static str {
    match reasoning {
        ReasoningArg::On => "on",
        ReasoningArg::Off => "off",
        ReasoningArg::Auto => "auto",
    }
}

/// The first code a failed `serve` printed on its standard error.
fn code_in(text: &str) -> Option<AiLocalCode> {
    text.lines()
        .find_map(|line| AiLocalCode::from_text(line.split_whitespace().next().unwrap_or("")))
}

fn ensure_command(places: &Places, args: &EnsureArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let json = args.json;
    let id = args.model.clone();
    let mut write_error: Option<Failure> = None;
    let mut emit = |event: ensure::EnsureEvent| {
        let result = if json {
            write_line_json(out, &event)
        } else {
            write_text(out, &format!("{}\n", event.message))
        };
        if let Err(failure) = result {
            write_error = Some(failure);
        }
    };
    let outcome = ensure_steps(places, args, &mut emit);
    if let Err(Failure::Coded { code, detail }) = &outcome {
        let event = if *code == AiLocalCode::Cancelled {
            ensure::EnsureEvent::new("cancelled", &id, code.entry().user_message)
        } else {
            ensure::EnsureEvent::new(
                "failed",
                &id,
                format!("{} {detail}", code.entry().user_message),
            )
        };
        emit(event.with_code(*code));
    }
    drop(emit);
    if let Some(failure) = write_error {
        return Err(failure);
    }
    outcome
}

fn ensure_steps(
    places: &Places,
    args: &EnsureArgs,
    emit: &mut dyn FnMut(ensure::EnsureEvent),
) -> Result<(), Failure> {
    let catalogue = load_catalogue(places)?;
    let model = find_model(&catalogue, &args.model)?;
    if args.context > model.context_length {
        return Err(Failure::Usage(format!(
            "{} supports a context of at most {} tokens",
            model.id, model.context_length
        )));
    }
    if model.licence.restricts_use() && !args.accept_licence {
        return Err(Failure::coded(
            AiLocalCode::LicenceNotAccepted,
            format!(
                "{} ({}): {}",
                model.licence.id, model.licence.url, model.licence.restrictions
            ),
        ));
    }
    // A model that can never run here is not downloaded.
    let hardware = read_hardware(places)?;
    let fit = fit::judge(model, &hardware, args.context);
    if fit.verdict == Verdict::TooLarge {
        return Err(Failure::coded(AiLocalCode::TooLarge, fit.message));
    }
    let runtime = serve::runtime_dir(places.runtime_dir.as_deref());
    if let Some(runtime) = &runtime {
        store::create_private_dir(runtime).map_err(|error| storage_failure(runtime, &error))?;
        // A stop asked of an earlier ensure is not this one's.
        let _ = std::fs::remove_file(runtime.join(ensure::CANCEL_FILE));
    }
    let cancel_file = runtime
        .as_ref()
        .map(|runtime| runtime.join(ensure::CANCEL_FILE));
    let cancelled = || cancel_file.as_ref().is_some_and(|file| file.exists());

    let store = require_store(places)?;
    let agent = download::agent_for(download::Scheme::HttpsOnly);
    let options = ensure::FetchOptions {
        agent: &agent,
        scheme: download::Scheme::HttpsOnly,
        free_space: &ensure::free_space,
        cancelled: &cancelled,
    };
    ensure::fetch_model(&store, model, &options, &mut *emit)
        .map_err(|error| Failure::coded(error.code, error.detail))?;
    if args.no_serve {
        return Ok(());
    }

    let runtime = runtime.ok_or_else(|| {
        Failure::coded(
            AiLocalCode::RuntimeDirMissing,
            "XDG_RUNTIME_DIR is not set and no --runtime-dir was given",
        )
    })?;
    // Already served as asked: nothing to start.
    if let Some(state) = serve::read_state(&runtime)
        && serve::supervisor_running(&state)
    {
        if state.model == model.id && state.context == args.context {
            let mut ready = ensure::EnsureEvent::new(
                "ready",
                &model.id,
                format!("{} is already running on this computer.", model.name),
            )
            .with_code(AiLocalCode::Serving);
            ready.state = Some(state);
            emit(ready);
            return Ok(());
        }
        emit(ensure::EnsureEvent::new(
            "stopping",
            &model.id,
            format!(
                "Stopping {}, which is running now, to start {}.",
                state.model, model.name
            ),
        ));
        serve::request_stop(&runtime, Duration::from_secs(30))
            .map_err(|error| Failure::coded(error.code, error.detail))?;
    }
    let fit = fit::judge(model, &read_hardware(places)?, args.context);
    if fit.verdict != Verdict::Fits {
        return Err(Failure::coded(fit.code, fit.message));
    }
    if cancelled() {
        return Err(Failure::coded(
            AiLocalCode::Cancelled,
            "stopped before the engine was started",
        ));
    }
    emit(ensure::EnsureEvent::new(
        "starting",
        &model.id,
        format!("Starting {}.", model.name),
    ));
    let program = std::env::current_exe()
        .map_err(|error| Failure::coded(AiLocalCode::EngineFailed, error.to_string()))?;
    let mut command = std::process::Command::new(program);
    command
        .arg("serve")
        .arg(&model.id)
        .arg("--json")
        .args(["--context", &args.context.to_string()])
        .args(["--port", &args.port.to_string()])
        .args(["--idle-stop", &args.idle_stop.to_string()])
        .args(["--start-timeout", &args.start_timeout.to_string()])
        .args(["--reasoning", reasoning_text(args.reasoning)])
        .arg("--catalogue")
        .arg(&catalogue.directory)
        .arg("--models-dir")
        .arg(&store.root)
        .arg("--runtime-dir")
        .arg(runtime.parent().unwrap_or(runtime.as_path()));
    if let Some(engine) = &places.engine {
        command.arg("--engine").arg(engine);
    }
    if let Some(root) = &places.root {
        command.arg("--root").arg(root);
    }
    if let Some(vulkaninfo) = &places.vulkaninfo {
        command.arg("--vulkaninfo").arg(vulkaninfo);
    }
    let output = runtime.join("serve.out");
    let errors = runtime.join("serve.err");
    let output_file =
        std::fs::File::create(&output).map_err(|error| storage_failure(&output, &error))?;
    let errors_file =
        std::fs::File::create(&errors).map_err(|error| storage_failure(&errors, &error))?;
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: a signal to the caller's terminal, or the
        // end of this command, does not reach the model it started.
        command.process_group(0);
    }
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(output_file)
        .stderr(errors_file)
        .spawn()
        .map_err(|error| Failure::coded(AiLocalCode::EngineFailed, error.to_string()))?;
    let started = std::time::Instant::now();
    let limit = Duration::from_secs(args.start_timeout.saturating_add(30));
    loop {
        if let Some(state) = serve::read_state(&runtime)
            && state.supervisor_pid == child.id()
            && state.model == model.id
        {
            let mut ready = ensure::EnsureEvent::new(
                "ready",
                &model.id,
                format!(
                    "{} is running on {}. It stops after {} seconds without a request.",
                    model.name,
                    state
                        .base_url
                        .clone()
                        .or_else(|| state.socket.clone())
                        .unwrap_or_default(),
                    state.idle_stop_seconds
                ),
            )
            .with_code(AiLocalCode::Serving);
            ready.state = Some(state);
            emit(ready);
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            let text = std::fs::read_to_string(&errors).unwrap_or_default();
            return Err(Failure::coded(
                code_in(&text).unwrap_or(AiLocalCode::EngineFailed),
                format!(
                    "the model could not be started ({status}): {}",
                    text.lines().take(12).collect::<Vec<_>>().join(" ")
                ),
            ));
        }
        if cancelled() {
            let _ = serve::request_stop(&runtime, Duration::from_secs(30));
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failure::coded(
                AiLocalCode::Cancelled,
                "stopped while the engine was starting",
            ));
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failure::coded(
                AiLocalCode::EngineFailed,
                format!("the model was not ready after {} seconds", limit.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[derive(Serialize)]
struct CancelReport {
    schema_version: u32,
    kind: &'static str,
    code: AiLocalCode,
    message: String,
}

fn cancel_command(places: &Places, args: &JsonArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let runtime = serve::runtime_dir(places.runtime_dir.as_deref()).ok_or_else(|| {
        Failure::coded(
            AiLocalCode::RuntimeDirMissing,
            "XDG_RUNTIME_DIR is not set and no --runtime-dir was given",
        )
    })?;
    store::create_private_dir(&runtime).map_err(|error| storage_failure(&runtime, &error))?;
    let file = runtime.join(ensure::CANCEL_FILE);
    std::fs::write(&file, b"cancel\n").map_err(|error| storage_failure(&file, &error))?;
    let message = "A running ensure was asked to stop. What it downloaded is kept.".to_owned();
    if args.json {
        return write_json(
            out,
            &CancelReport {
                schema_version: SCHEMA_VERSION,
                kind: "cancel",
                code: AiLocalCode::Cancelled,
                message,
            },
        );
    }
    write_text(out, &format!("{}\n{message}\n", AiLocalCode::Cancelled))
}

// ------------------------------------------------------------------- codes

#[derive(Serialize)]
struct CodeReport {
    schema_version: u32,
    kind: &'static str,
    codes: Vec<CodeEntry>,
}

fn codes_command(args: &JsonArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let codes = registry();
    if args.json {
        return write_json(
            out,
            &CodeReport {
                schema_version: SCHEMA_VERSION,
                kind: "codes",
                codes,
            },
        );
    }
    let mut text = String::new();
    for entry in &codes {
        text.push_str(&format!(
            "{} [{}] {}\n  {}\n",
            entry.code, entry.severity, entry.title, entry.user_message
        ));
    }
    write_text(out, &text)
}
