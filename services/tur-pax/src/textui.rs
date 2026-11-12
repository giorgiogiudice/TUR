// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The text interface: `tur-pax run` and `tur-pax chat`.
//!
//! ```text
//! tur-pax run "<request>" [--provider-config <file>] [--policy <file>]
//!                         [--tools <dir>] [--manifests <dir>] [--adapters <dir>]
//!                         [--grant-read <path>]... [--audit-log <file>]
//!                         [--memory-store <dir>] [--key-env <VARIABLE>]
//!                         [--max-steps <n>] [--max-seconds <n>]
//!                         [--no-commands] [--task <id>] [--json]
//! tur-pax chat            the same options, without --json
//! ```
//!
//! `run` performs one task and prints what P@x did: every proposed action
//! with its class and its decision, what was run and how it ended, and the
//! final answer. `chat` reads one request per line from standard input and
//! keeps the conversation between them.
//!
//! This module is where the pieces are put together from the machine: the
//! files are read, the key is taken from its source, the provider adapter is
//! built and the sandbox executor is the real one. Two things it has no
//! option for, on purpose: the trusted confirmation path, and running
//! anything outside the sandbox.
//!
//! The audit trail is always written. Without `--audit-log` it goes to
//! `$XDG_STATE_HOME/tur/pax/audit.jsonl`; if no file can be opened, nothing
//! is started.
//!
//! Exit status of `run`: 0 when the task completed; 1 when an input could
//! not be read or was not valid, and nothing was started; 20 when the task
//! was stopped by a limit or cancelled; 21 when the provider refused or
//! failed; 23 when the audit trail could not be written.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Args;
use serde_json::{Value, json};
use tur_pax_memory::{FileStore, Memory, RecallBudget, RecalledNotes, StdFs};
use tur_pax_providers::{
    EnvKeySource, KeySource, Message, Provider, ProviderConfig, SpendMeter, connect,
};

use crate::codes::{Code, Problem};
use crate::exec::BwrapExecutor;
use crate::faults::{Fault, TaskCode};
use crate::orchestrator::{
    NoControl, NoObserver, Observer, Orchestrator, ReportItem, Situation, SystemTaskClock,
    TaskLimits, TaskOutcome, TaskReport, TaskRequest,
};
use crate::policy::UserPolicy;
use crate::registry::{ToolFiles, ToolRegistry, read_tool_sets};
use crate::sandbox::{Grant, GrantRules};
use crate::trail::FileTrail;

/// Exit status: the task completed.
pub const EXIT_COMPLETED: u8 = 0;
/// Exit status: an input was unreadable or invalid. Nothing was started.
pub const EXIT_NOT_STARTED: u8 = 1;
/// Exit status: the task was stopped by a limit, or cancelled.
pub const EXIT_STOPPED: u8 = 20;
/// Exit status: the provider refused or failed.
pub const EXIT_PROVIDER: u8 = 21;
/// Exit status: the audit trail could not be written.
pub const EXIT_AUDIT: u8 = 23;

/// The environment variable the API key is read from unless another is
/// named. For development only; the keyring source is not built yet.
pub const DEFAULT_KEY_VARIABLE: &str = "TUR_PAX_API_KEY";

/// The directory of tool sets that is read unless another is named: one
/// subdirectory for each component, holding its `agent-tools.json` and its
/// `agent-tools.adapter.json` ([`crate::registry::read_tool_sets`]).
pub const DEFAULT_TOOLS_DIR: &str = "/usr/share/tur/pax/tools";

/// The options `run` and `chat` share.
#[derive(Debug, Args)]
pub struct TaskArgs {
    /// The provider configuration file. Without it,
    /// $XDG_CONFIG_HOME/tur/pax/provider.json is read.
    #[arg(long, value_name = "FILE")]
    pub provider_config: Option<PathBuf>,
    /// The user policy file. Without it, the default policy applies. A file
    /// that cannot be read or is not valid leaves P@x able to observe only.
    #[arg(long, value_name = "FILE")]
    pub policy: Option<PathBuf>,
    /// The directory of tool sets: one subdirectory for each component that
    /// ships tools, holding its agent-tools.json and, beside it, its
    /// agent-tools.adapter.json. Without it, /usr/share/tur/pax/tools is
    /// read if it exists.
    #[arg(long, value_name = "DIR")]
    pub tools: Option<PathBuf>,
    /// A further directory of tool manifests, one JSON file per service.
    /// Read in addition to the tool sets.
    #[arg(long, value_name = "DIR")]
    pub manifests: Option<PathBuf>,
    /// A further directory of tool adapters, one JSON file per service.
    /// Read in addition to the tool sets.
    #[arg(long, value_name = "DIR")]
    pub adapters: Option<PathBuf>,
    /// The environment variable that holds the API key.
    #[arg(long, value_name = "VARIABLE", default_value = DEFAULT_KEY_VARIABLE)]
    pub key_env: String,
    /// A folder or a file the commands of this task may read. May be given
    /// more than once. Keys, keyrings, browser profiles and hidden folders
    /// are refused.
    #[arg(long = "grant-read", value_name = "PATH")]
    pub grants: Vec<String>,
    /// The file the audit trail is appended to, one JSON object per line.
    #[arg(long, value_name = "FILE")]
    pub audit_log: Option<PathBuf>,
    /// The directory of P@x's memory. Notes are recalled from it only if the
    /// user has turned memory on.
    #[arg(long, value_name = "DIR")]
    pub memory_store: Option<PathBuf>,
    /// The most responses the model may be asked for in one task.
    #[arg(long, value_name = "N", default_value_t = 12)]
    pub max_steps: u32,
    /// The wall-clock seconds one task may take.
    #[arg(long, value_name = "SECONDS", default_value_t = 300)]
    pub max_seconds: u64,
    /// Do not offer the model the read-only command tool.
    #[arg(long)]
    pub no_commands: bool,
    /// The task identifier written to the audit trail.
    #[arg(long, value_name = "ID")]
    pub task: Option<String>,
}

/// The arguments of `run`.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// What you want P@x to do.
    pub request: String,
    /// Print one JSON document instead of text.
    #[arg(long)]
    pub json: bool,
    /// The options shared with `chat`.
    #[command(flatten)]
    pub common: TaskArgs,
}

/// The arguments of `chat`.
#[derive(Debug, Args)]
pub struct ChatArgs {
    /// The options shared with `run`.
    #[command(flatten)]
    pub common: TaskArgs,
}

/// Everything a task needs, read from the machine.
struct Setup {
    config: ProviderConfig,
    provider: Box<dyn Provider>,
    registry: ToolRegistry,
    policy: UserPolicy,
    rules: GrantRules,
    grants: Vec<Grant>,
    trail: FileTrail,
    audit_path: PathBuf,
    limits: TaskLimits,
    memory_store: Option<PathBuf>,
}

/// `text` with every control character but the line break and the tab
/// replaced, so that nothing a model or a tool wrote can drive the terminal.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}

fn base_dir(variable: &str, fallback: &str) -> Option<PathBuf> {
    let stated = std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    stated.or_else(|| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|home| home.join(fallback))
    })
}

fn input_fault(code: Code, detail: String) -> Fault {
    Fault::from(Problem::new(code, detail))
}

fn read_file(path: &Path) -> Result<String, Fault> {
    fs::read_to_string(path).map_err(|error| {
        input_fault(
            Code::InputUnreadable,
            format!("{}: {error}", path.display()),
        )
    })
}

/// The tool sets of the components: what the directory of tool sets holds.
///
/// A directory that was not named and does not exist holds nothing.
fn read_tools(named: Option<&Path>, default: &str) -> Result<ToolFiles, Fault> {
    let directory = match named {
        Some(directory) => directory,
        None if Path::new(default).is_dir() => Path::new(default),
        None => return Ok(ToolFiles::default()),
    };
    read_tool_sets(directory)
        .map_err(|error| input_fault(Code::InputUnreadable, error.to_string()))
}

/// The texts of the `.json` files of a directory, in name order.
///
/// A directory that was not named holds nothing.
fn read_json_dir(named: Option<&Path>) -> Result<Vec<String>, Fault> {
    let Some(directory) = named else {
        return Ok(Vec::new());
    };
    let entries = fs::read_dir(directory).map_err(|error| {
        input_fault(
            Code::InputUnreadable,
            format!("{}: {error}", directory.display()),
        )
    })?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    paths.iter().map(|path| read_file(path)).collect()
}

fn prepare(args: &TaskArgs, err: &mut dyn Write) -> io::Result<Option<Setup>> {
    match build(args, err)? {
        Ok(setup) => Ok(Some(setup)),
        Err(problems) => {
            for problem in problems {
                writeln!(err, "error: {problem}")?;
            }
            Ok(None)
        }
    }
}

fn build(args: &TaskArgs, err: &mut dyn Write) -> io::Result<Result<Setup, Vec<Fault>>> {
    // The provider: configuration, key, adapter.
    let config_path = match &args.provider_config {
        Some(path) => Some(path.clone()),
        None => {
            base_dir("XDG_CONFIG_HOME", ".config").map(|base| base.join("tur/pax/provider.json"))
        }
    };
    let Some(config_path) = config_path else {
        return Ok(Err(vec![input_fault(
            Code::InputUnreadable,
            "no provider configuration was named and no home directory is known".to_owned(),
        )]));
    };
    let config_text = match read_file(&config_path) {
        Ok(text) => text,
        Err(fault) => return Ok(Err(vec![fault])),
    };
    let provider_fault = |error: tur_pax_providers::ProviderError| Fault {
        code: error.kind().error_code(),
        detail: error.to_string(),
    };
    let config = match ProviderConfig::from_json(&config_text) {
        Ok(config) => config,
        Err(error) => return Ok(Err(vec![provider_fault(error)])),
    };
    let key = match EnvKeySource::new(args.key_env.as_str()).api_key(config.provider) {
        Ok(key) => key,
        Err(error) => return Ok(Err(vec![provider_fault(error)])),
    };
    let provider = match connect(&config, key) {
        Ok(provider) => provider,
        Err(error) => return Ok(Err(vec![provider_fault(error)])),
    };

    // The tools: what each component ships, then any further directories.
    let tools = match read_tools(args.tools.as_deref(), DEFAULT_TOOLS_DIR) {
        Ok(files) => files,
        Err(fault) => return Ok(Err(vec![fault])),
    };
    let mut manifests = tools.manifests;
    let mut adapters = tools.adapters;
    match read_json_dir(args.manifests.as_deref()) {
        Ok(texts) => manifests.extend(texts),
        Err(fault) => return Ok(Err(vec![fault])),
    }
    match read_json_dir(args.adapters.as_deref()) {
        Ok(texts) => adapters.extend(texts),
        Err(fault) => return Ok(Err(vec![fault])),
    }
    let registry = match ToolRegistry::load(&manifests, &adapters, !args.no_commands) {
        Ok(registry) => registry,
        Err(problems) => return Ok(Err(problems)),
    };

    // The policy. A file that cannot be used leaves observation only.
    let policy = match &args.policy {
        None => UserPolicy::default(),
        Some(path) => {
            let loaded = read_file(path)
                .map_err(|fault| vec![fault])
                .and_then(|text| {
                    UserPolicy::from_json(&text)
                        .map_err(|problems| problems.into_iter().map(Fault::from).collect())
                });
            match loaded {
                Ok(policy) => policy,
                Err(problems) => {
                    for problem in problems {
                        writeln!(err, "warning: {problem}")?;
                    }
                    writeln!(
                        err,
                        "warning: the policy file cannot be used, so P@x only observes and \
                         changes nothing"
                    )?;
                    UserPolicy::fail_closed()
                }
            }
        }
    };

    // What the commands of the task may read.
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let rules = GrantRules::new(home.as_deref());
    let mut grants = Vec::new();
    let mut refused = Vec::new();
    for path in &args.grants {
        match rules.admit(path) {
            Ok(grant) => grants.push(grant),
            Err(refusal) => refused.push(Fault::new(
                TaskCode::SandboxGrantRefused,
                format!("{path:?} cannot be granted: {refusal}"),
            )),
        }
    }
    if !refused.is_empty() {
        return Ok(Err(refused));
    }

    // The audit trail. No trail, no task.
    let audit_path = match &args.audit_log {
        Some(path) => Some(path.clone()),
        None => {
            base_dir("XDG_STATE_HOME", ".local/state").map(|base| base.join("tur/pax/audit.jsonl"))
        }
    };
    let Some(audit_path) = audit_path else {
        return Ok(Err(vec![input_fault(
            Code::AuditUnavailable,
            "no audit log was named and no home directory is known".to_owned(),
        )]));
    };
    let trail = match FileTrail::open(&audit_path) {
        Ok(trail) => trail,
        Err(error) => {
            return Ok(Err(vec![input_fault(
                Code::AuditUnavailable,
                format!("{}: {error}", audit_path.display()),
            )]));
        }
    };

    Ok(Ok(Setup {
        config,
        provider,
        registry,
        policy,
        rules,
        grants,
        trail,
        audit_path,
        limits: TaskLimits {
            max_steps: args.max_steps,
            max_seconds: args.max_seconds,
            ..TaskLimits::default()
        },
        memory_store: args.memory_store.clone(),
    }))
}

impl Setup {
    /// Notes recalled for a request. Nothing, unless a store was named and
    /// the user has turned memory on in it.
    fn recall(&self, request: &str, err: &mut dyn Write) -> io::Result<RecalledNotes> {
        let Some(directory) = &self.memory_store else {
            return Ok(RecalledNotes::none());
        };
        let memory = Memory::new(FileStore::new(directory.clone(), StdFs));
        match memory.recall(request, RecallBudget::default()) {
            Ok(notes) => Ok(notes),
            Err(problem) => {
                writeln!(err, "warning: nothing was recalled from memory: {problem}")?;
                Ok(RecalledNotes::none())
            }
        }
    }

    /// Run one task with the real sandbox executor and the machine's clock.
    fn run_one(
        &mut self,
        task_id: &str,
        request: &str,
        recalled: &RecalledNotes,
        history: &mut Vec<Message>,
        observer: &mut dyn Observer,
    ) -> TaskReport {
        let mut executor = BwrapExecutor::new();
        let clock = SystemTaskClock::new();
        let control = NoControl;
        let mut meter = SpendMeter::for_config(&self.config);
        let mut orchestrator = Orchestrator {
            provider: &*self.provider,
            config: &self.config,
            registry: &self.registry,
            policy: &self.policy,
            situation: Situation::attended(),
            limits: self.limits,
            rules: &self.rules,
            grants: &self.grants,
            executor: &mut executor,
            trail: &mut self.trail,
            clock: &clock,
            control: &control,
            observer,
        };
        let task = TaskRequest {
            task_id,
            request,
            recalled,
        };
        orchestrator.run(&task, history, &mut meter)
    }

    fn tools_json(&self) -> Value {
        let offered: Vec<Value> = self
            .registry
            .offered()
            .into_iter()
            .map(|tool| json!({ "name": tool.name, "service": tool.service, "class": tool.class }))
            .collect();
        json!(offered)
    }
}

/// Prints the items of a task as they happen.
struct Printer<'w> {
    out: &'w mut dyn Write,
    failed: Option<io::Error>,
}

impl Printer<'_> {
    fn print(&mut self, item: &ReportItem) -> io::Result<()> {
        match item {
            ReportItem::Said { text } => writeln!(self.out, "P@x: {}", printable(text)),
            ReportItem::Notice { code, message } => match code {
                Some(code) => writeln!(self.out, "note: {message} [{code}]"),
                None => writeln!(self.out, "note: {message}"),
            },
            ReportItem::Proposal(proposal) => {
                let class = proposal
                    .class
                    .map_or_else(|| "-".to_owned(), |class| class.to_string());
                write!(
                    self.out,
                    "proposed: {}  class {class}  {} ({}",
                    proposal.tool, proposal.decision, proposal.reason
                )?;
                if let Some(code) = &proposal.error_code {
                    write!(self.out, ", {code}")?;
                }
                writeln!(self.out, ")")?;
                if !proposal.description.is_empty() {
                    writeln!(self.out, "          {}", proposal.description)?;
                }
                if let Some(result) = proposal.result {
                    let exit = proposal
                        .exit_code
                        .map_or_else(|| "none".to_owned(), |code| code.to_string());
                    writeln!(
                        self.out,
                        "          ran: {result}, exit status {exit}, {} bytes of output{}, {} \
                         withheld",
                        proposal.output_bytes,
                        if proposal.truncated { " (cut)" } else { "" },
                        proposal.withheld
                    )?;
                }
                if let Some(message) = &proposal.message {
                    writeln!(self.out, "          {message}")?;
                }
                Ok(())
            }
        }
    }
}

impl Observer for Printer<'_> {
    fn item(&mut self, item: &ReportItem) {
        if self.failed.is_none()
            && let Err(error) = self.print(item)
        {
            self.failed = Some(error);
        }
    }
}

fn task_id(args: &TaskArgs) -> String {
    args.task.clone().unwrap_or_else(|| {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        format!("task-{seconds}-{}", std::process::id())
    })
}

/// The exit status for an outcome.
pub const fn exit_status(outcome: TaskOutcome) -> u8 {
    match outcome {
        TaskOutcome::Completed => EXIT_COMPLETED,
        TaskOutcome::StepLimit
        | TaskOutcome::TimeLimit
        | TaskOutcome::SpendLimit
        | TaskOutcome::Cancelled
        | TaskOutcome::OutputLimit => EXIT_STOPPED,
        TaskOutcome::ProviderRefusal | TaskOutcome::ProviderError => EXIT_PROVIDER,
        TaskOutcome::AuditUnavailable => EXIT_AUDIT,
    }
}

fn print_ending(report: &TaskReport, out: &mut dyn Write) -> io::Result<()> {
    if !report.answer.is_empty() {
        writeln!(out, "\nanswer:\n{}", printable(&report.answer))?;
    }
    write!(out, "\noutcome: {}", report.outcome.as_str())?;
    if let Some(code) = &report.error_code {
        write!(out, " ({code})")?;
    }
    write!(
        out,
        "; {} steps, {} requests, {} input and {} output tokens",
        report.steps, report.requests, report.usage.input_tokens, report.usage.output_tokens
    )?;
    if let (Some(cost), Some(currency)) = (report.cost, &report.currency) {
        write!(out, ", about {cost:.4} {currency}")?;
    }
    writeln!(out)
}

/// Run `tur-pax run`. Returns the exit status.
///
/// Results go to `out` and problems to `err`. An error is returned only if
/// writing to either fails.
pub fn run_task(args: &RunArgs, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<u8> {
    let Some(mut setup) = prepare(&args.common, err)? else {
        return Ok(EXIT_NOT_STARTED);
    };
    let task = task_id(&args.common);
    let recalled = setup.recall(&args.request, err)?;
    let mut history = Vec::new();

    if args.json {
        let report = setup.run_one(
            &task,
            &args.request,
            &recalled,
            &mut history,
            &mut NoObserver,
        );
        let mut document = report.to_json();
        document["task"] = json!(task);
        document["provider"] = json!(setup.config.provider.as_str());
        document["model"] = json!(setup.config.model);
        document["tools"] = setup.tools_json();
        document["unavailable_tools"] = json!(setup.registry.unavailable());
        document["audit_log"] = json!(setup.audit_path.display().to_string());
        writeln!(out, "{document}")?;
        return Ok(exit_status(report.outcome));
    }

    let names: Vec<String> = setup
        .registry
        .offered()
        .into_iter()
        .map(|tool| format!("{} (class {})", tool.name, tool.class))
        .collect();
    writeln!(
        out,
        "task {task}: provider {}, model {}",
        setup.config.provider,
        printable(&setup.config.model)
    )?;
    writeln!(out, "tools offered: {}", names.join(", "))?;
    writeln!(out, "audit log: {}\n", setup.audit_path.display())?;
    let mut printer = Printer { out, failed: None };
    let report = setup.run_one(&task, &args.request, &recalled, &mut history, &mut printer);
    if let Some(error) = printer.failed {
        return Err(error);
    }
    print_ending(&report, printer.out)?;
    Ok(exit_status(report.outcome))
}

/// Run `tur-pax chat` on standard input. Returns the exit status.
pub fn run_chat(args: &ChatArgs, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<u8> {
    let stdin = io::stdin();
    chat(args, &mut stdin.lock(), out, err)
}

/// A conversation: one request per line of `input`, until its end or a line
/// that says `/quit`. Each line is a task of its own, with its own limits;
/// the conversation is kept between them.
pub fn chat(
    args: &ChatArgs,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    let Some(mut setup) = prepare(&args.common, err)? else {
        return Ok(EXIT_NOT_STARTED);
    };
    let base = task_id(&args.common);
    let mut history = Vec::new();
    let mut turn = 0u32;
    writeln!(
        out,
        "P@x, provider {}. One request per line; /quit to end.",
        setup.config.provider
    )?;
    loop {
        write!(out, "you> ")?;
        out.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        let request = line.trim();
        if request.is_empty() {
            continue;
        }
        if request == "/quit" || request == "/exit" {
            break;
        }
        turn += 1;
        let task = format!("{base}-{turn}");
        let recalled = setup.recall(request, err)?;
        let mut printer = Printer {
            out: &mut *out,
            failed: None,
        };
        let report = setup.run_one(&task, request, &recalled, &mut history, &mut printer);
        if let Some(error) = printer.failed {
            return Err(error);
        }
        print_ending(&report, out)?;
        if report.outcome == TaskOutcome::AuditUnavailable {
            return Ok(EXIT_AUDIT);
        }
    }
    writeln!(out)?;
    Ok(EXIT_COMPLETED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_do_not_reach_the_terminal() {
        assert_eq!(
            printable("a\u{1b}[2Jb\rc\u{7}d\ne\tf"),
            "a\u{fffd}[2Jb\u{fffd}c\u{fffd}d\ne\tf"
        );
        assert_eq!(printable("plain text, wörld"), "plain text, wörld");
    }

    #[test]
    fn only_a_completed_task_exits_with_zero() {
        for outcome in TaskOutcome::ALL {
            let status = exit_status(outcome);
            assert_eq!(
                status == 0,
                outcome == TaskOutcome::Completed,
                "{outcome:?}"
            );
            // None of the statuses of `policy check` is reused.
            assert!(!(10..=13).contains(&status), "{outcome:?}");
        }
    }

    #[test]
    fn a_directory_that_was_not_named_and_does_not_exist_holds_nothing() {
        let tools = read_tools(None, "/nonexistent/tur-pax-test/tools").expect("no error");
        assert_eq!(tools, ToolFiles::default());
        assert!(read_json_dir(None).expect("no error").is_empty());
        let missing = Path::new("/nonexistent/tur-pax-test/tools");
        assert_eq!(
            read_tools(Some(missing), DEFAULT_TOOLS_DIR)
                .expect_err("a named directory must exist")
                .code
                .as_str(),
            "TUR-AI-INPUT-001"
        );
        assert_eq!(
            read_json_dir(Some(missing))
                .expect_err("a named directory must exist")
                .code
                .as_str(),
            "TUR-AI-INPUT-001"
        );
        assert_eq!(DEFAULT_TOOLS_DIR, "/usr/share/tur/pax/tools");
    }
}
