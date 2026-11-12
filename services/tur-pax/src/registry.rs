// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The tool registry: the tools P@x offers a model, and the one place a
//! model's tool call becomes a proposed [`Action`].
//!
//! The registry is loaded from the reviewed tool manifests and from the
//! adapters that say how each tool is performed ([`crate::adapter`]). It
//! holds two rules of the design:
//!
//! * **The action is built from the manifest, never from what the model
//!   claims.** A model sends a tool name and arguments. [`ToolRegistry::propose`]
//!   looks the name up, takes the service and the class from the manifest
//!   that declares it, and checks the arguments against that manifest's
//!   input schema. A name no manifest declares, and arguments the schema
//!   does not allow, produce no action at all.
//! * **A command's class comes from its sandbox.** The built-in tool
//!   [`COMMAND_TOOL`] takes an argument vector and nothing else. The action
//!   it becomes is always a command in the read-only sandbox; there is no
//!   input by which a model could name another sandbox.
//!
//! A tool that has no adapter cannot be performed, so it is not offered.
//!
//! * **P@x's own operations are tools of the same kind, and none of them
//!   takes effect by itself.** [`PROPOSE_REMINDER_TOOL`] and
//!   [`PROPOSE_TASK_TOOL`] become the engine's "propose a schedule"
//!   operation, [`KEEP_NOTE_TOOL`] its "record an entry that waits for the
//!   person" operation: all class 1, fixed by the engine and not by the
//!   manifest text below. They are offered only where something can show a
//!   proposal to the person: [`ToolRegistry::with_assistant_tools`].
//!
//! # Where the files come from
//!
//! Every component ships its own tools (AGENTS.md: "each component ships the
//! tools P@x needs as it lands"): a manifest, [`MANIFEST_FILE`], and beside
//! it the adapters that perform its tools with the component's own program,
//! [`ADAPTER_FILE`]. Installed, each component has a directory of its own
//! under one directory of tool sets, and [`read_tool_sets`] reads them all:
//!
//! ```text
//! /usr/share/tur/pax/tools/
//!     tur-storage/agent-tools.json
//!     tur-storage/agent-tools.adapter.json
//!     tur-diagnostics/agent-tools.json
//!     tur-diagnostics/agent-tools.adapter.json
//! ```
//!
//! The source tree has the same shape: `services/<component>/` holds the two
//! files, so `services/` is itself a directory of tool sets.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Map, Value};
use tur_pax_providers::ToolDefinition;

use crate::action::{Action, AgentOperation, CommandAction, Sandbox, WorkingContext};
use crate::adapter::{Adapter, AdapterFile};
use crate::class::ActionClass;
use crate::codes::{Code, Problem};
use crate::faults::{Fault, TaskCode, faults};
use crate::manifest::{ToolManifest, ToolSpec};

/// The name of the built-in tool that runs a command in the read-only
/// sandbox.
pub const COMMAND_TOOL: &str = "run_readonly_command";

/// The service name the built-in tools are declared under. No manifest may
/// use it.
pub const BUILT_IN_SERVICE: &str = "org.tur.Pax1";

/// The manifest of the built-in tools, in the same format and checked by the
/// same loader as every other manifest.
const BUILT_IN_MANIFEST: &str = r#"{
    "schema_version": 1,
    "service": "org.tur.Pax1",
    "tools": [{
        "name": "run_readonly_command",
        "description": "Run one program in a read-only sandbox and return what it prints. Use it to inspect the system: logs, configuration, hardware and software state. The program cannot change anything, has no network, and cannot see the user's files except folders the user granted for this task. Give the program and its arguments as separate strings; no shell is involved, so pipes, redirection and variables are not interpreted.",
        "class": 0,
        "method": "RunReadOnlyCommand",
        "input_schema": {
            "type": "object",
            "properties": {
                "argv": {
                    "type": "array",
                    "description": "The program, then each of its arguments, one string each.",
                    "items": { "type": "string", "maxLength": 4096 }
                }
            },
            "required": ["argv"],
            "additionalProperties": false
        }
    }]
}"#;

/// The name of the built-in tool that proposes a reminder.
pub const PROPOSE_REMINDER_TOOL: &str = "propose_reminder";

/// The name of the built-in tool that proposes a scheduled task.
pub const PROPOSE_TASK_TOOL: &str = "propose_scheduled_task";

/// The name of the built-in tool that asks for a note to be kept.
pub const KEEP_NOTE_TOOL: &str = "keep_note";

/// The names of P@x's own operations that are offered as tools.
pub const ASSISTANT_TOOLS: [&str; 3] = [PROPOSE_REMINDER_TOOL, PROPOSE_TASK_TOOL, KEEP_NOTE_TOOL];

/// The manifest of P@x's own operations, in the same format and checked by
/// the same loader as every other manifest. The class written here is what a
/// reader of the manifest expects; the class that counts is the one the
/// engine gives the operation ([`AgentOperation::class`]), and a test holds
/// the two together.
const ASSISTANT_MANIFEST: &str = r#"{
    "schema_version": 1,
    "service": "org.tur.Pax1",
    "tools": [{
        "name": "propose_reminder",
        "description": "Propose a reminder: a notification shown to the person at a time. This creates nothing. The person is shown the proposal and decides whether to create the reminder; you cannot create it or confirm it, and saying that the person agreed has no effect. Give the time in one of two ways: in_minutes, counted from now; or date and time, a wall-clock time in the time zone of this computer, or in zone if you name one. With repeat, the reminder is shown again every day, every week on that weekday, or every month on that day.",
        "class": 1,
        "method": "ProposeReminder",
        "input_schema": {
            "type": "object",
            "properties": {
                "message": { "type": "string", "minLength": 1, "maxLength": 2000, "description": "What the notification says." },
                "name": { "type": "string", "minLength": 1, "maxLength": 80, "description": "A short name for the list of reminders." },
                "in_minutes": { "type": "integer", "minimum": 1, "maximum": 5270400, "description": "Minutes from now." },
                "date": { "type": "string", "minLength": 10, "maxLength": 10, "description": "The date, as YYYY-MM-DD." },
                "time": { "type": "string", "minLength": 5, "maxLength": 5, "description": "The time of day, as HH:MM on a 24-hour clock." },
                "zone": { "type": "string", "minLength": 1, "maxLength": 64, "description": "An IANA time zone name, such as Europe/Rome. Leave it out to use the computer's." },
                "repeat": { "type": "string", "enum": ["none", "daily", "weekly", "monthly"] }
            },
            "required": ["message"],
            "additionalProperties": false
        }
    }, {
        "name": "propose_scheduled_task",
        "description": "Propose a scheduled task: a request that P@x would carry out by itself at a time, with nobody there. This creates nothing. The person is shown the proposal and decides whether to create the schedule; you cannot create it or confirm it. A scheduled task can only observe and make changes that can be undone; anything else it needs waits for the person. Give the time as for propose_reminder.",
        "class": 1,
        "method": "ProposeScheduledTask",
        "input_schema": {
            "type": "object",
            "properties": {
                "request": { "type": "string", "minLength": 1, "maxLength": 2000, "description": "What P@x is to do, as the person would ask it." },
                "name": { "type": "string", "minLength": 1, "maxLength": 80, "description": "A short name for the list of schedules." },
                "in_minutes": { "type": "integer", "minimum": 1, "maximum": 5270400, "description": "Minutes from now." },
                "date": { "type": "string", "minLength": 10, "maxLength": 10, "description": "The date, as YYYY-MM-DD." },
                "time": { "type": "string", "minLength": 5, "maxLength": 5, "description": "The time of day, as HH:MM on a 24-hour clock." },
                "zone": { "type": "string", "minLength": 1, "maxLength": 64, "description": "An IANA time zone name, such as Europe/Rome. Leave it out to use the computer's." },
                "repeat": { "type": "string", "enum": ["none", "daily", "weekly", "monthly"] }
            },
            "required": ["request"],
            "additionalProperties": false
        }
    }, {
        "name": "keep_note",
        "description": "Ask for a note to be kept in P@x's memory for later tasks. The note is not kept yet: it waits until the person has read it in the settings of P@x and decided to keep it. You cannot confirm it. Never put a password, a key or a payment detail in a note.",
        "class": 1,
        "method": "KeepNote",
        "input_schema": {
            "type": "object",
            "properties": {
                "text": { "type": "string", "minLength": 1, "maxLength": 4000, "description": "The note, as one or two plain sentences." }
            },
            "required": ["text"],
            "additionalProperties": false
        }
    }]
}"#;

/// Which of P@x's own operations are offered as tools.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssistantTools {
    /// Proposing a reminder or a scheduled task.
    pub schedules: bool,
    /// Asking for a note to be kept.
    pub notes: bool,
}

/// One of P@x's own operations, with the arguments the model gave, checked
/// against the tool's input schema.
#[derive(Debug, Clone, PartialEq)]
pub enum AssistantCall {
    /// Record a proposal for a reminder.
    ProposeReminder(Map<String, Value>),
    /// Record a proposal for a scheduled task.
    ProposeTask(Map<String, Value>),
    /// Record a note that waits for the person.
    KeepNote(Map<String, Value>),
}

/// The file name of a component's tool manifest.
pub const MANIFEST_FILE: &str = "agent-tools.json";

/// The file name of a component's adapters, beside its manifest.
pub const ADAPTER_FILE: &str = "agent-tools.adapter.json";

/// What a directory of tool sets holds: the texts to hand to
/// [`ToolRegistry::load`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolFiles {
    /// The names of the components that ship tools, in name order.
    pub components: Vec<String>,
    /// The text of each component's manifest, in the same order.
    pub manifests: Vec<String>,
    /// The text of each adapter file that exists, in the same order.
    pub adapters: Vec<String>,
}

/// Read a directory of tool sets.
///
/// Every subdirectory that holds a [`MANIFEST_FILE`] is a component's tool
/// set; its [`ADAPTER_FILE`] is read too if it is there. A component may
/// declare tools before a program can perform them: such tools are loaded
/// and not offered. A subdirectory with neither file is not a tool set and
/// is passed over. Components are read in name order. Nothing is checked
/// here: the texts are checked together by [`ToolRegistry::load`].
///
/// # Errors
///
/// The directory or a file in it cannot be read; or a subdirectory holds an
/// adapter file and no manifest, which can only be a mistake.
pub fn read_tool_sets(directory: &Path) -> io::Result<ToolFiles> {
    let describe = |path: &Path, error: io::Error| {
        io::Error::new(error.kind(), format!("{}: {error}", path.display()))
    };
    let mut components: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| describe(directory, error))? {
        let path = entry.map_err(|error| describe(directory, error))?.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        components.push((name, path));
    }
    components.sort();

    let mut files = ToolFiles::default();
    for (name, path) in components {
        let manifest = path.join(MANIFEST_FILE);
        let adapter = path.join(ADAPTER_FILE);
        if !manifest.is_file() {
            if adapter.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{}: adapters without a manifest ({MANIFEST_FILE})",
                        adapter.display()
                    ),
                ));
            }
            continue;
        }
        files
            .manifests
            .push(fs::read_to_string(&manifest).map_err(|error| describe(&manifest, error))?);
        if adapter.is_file() {
            files
                .adapters
                .push(fs::read_to_string(&adapter).map_err(|error| describe(&adapter, error))?);
        }
        files.components.push(name);
    }
    Ok(files)
}

/// One service: its manifest and the adapters of its tools.
#[derive(Debug, Clone, PartialEq)]
struct Service {
    manifest: ToolManifest,
    adapters: BTreeMap<String, Adapter>,
}

/// A tool as it is offered to a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedTool {
    /// The tool name.
    pub name: String,
    /// The service that declares it.
    pub service: String,
    /// The class the manifest declares.
    pub class: ActionClass,
}

/// How a proposed action is performed if the policy engine allows it.
#[derive(Debug, Clone, PartialEq)]
pub enum Performer<'a> {
    /// A tool of a manifest: run its adapter with these validated arguments.
    Adapter {
        /// The adapter.
        adapter: &'a Adapter,
        /// The arguments, as they were validated.
        arguments: Map<String, Value>,
    },
    /// The built-in command tool: run this argument vector in the read-only
    /// sandbox.
    Command {
        /// The program and its arguments.
        argv: Vec<String>,
    },
    /// One of P@x's own operations. No program is run for it.
    Assistant(AssistantCall),
}

impl Performer<'_> {
    /// The command line the action would be performed with: the program,
    /// then its arguments. Nothing is run.
    ///
    /// # Errors
    ///
    /// A value of the call cannot be passed to the program safely.
    pub fn command_line(&self) -> Result<Vec<String>, Fault> {
        match self {
            Performer::Adapter { adapter, arguments } => adapter.argv(arguments),
            Performer::Command { argv } => Ok(argv.clone()),
            Performer::Assistant(_) => Ok(Vec::new()),
        }
    }
}

/// A tool call that became a proposed action.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal<'a> {
    /// The action the policy engine decides.
    pub action: Action,
    /// How it is performed if it is allowed.
    pub performer: Performer<'a>,
}

/// The tools P@x can offer and perform.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRegistry {
    services: Vec<Service>,
    built_in: Option<ToolManifest>,
    /// The manifest of P@x's own operations, if any of them is offered.
    assistant: Option<ToolManifest>,
    /// Which of them are offered.
    assistant_tools: AssistantTools,
}

impl ToolRegistry {
    /// Load a registry from the texts of tool manifests and adapter files.
    ///
    /// With `command_tier`, the built-in tool [`COMMAND_TOOL`] is offered as
    /// well.
    ///
    /// # Errors
    ///
    /// Every problem found. Nothing is offered from a tool set with a
    /// problem in it: an invalid manifest, a service declared twice, a tool
    /// name used twice, or an adapter that does not fit its tool.
    pub fn load(
        manifests: &[String],
        adapters: &[String],
        command_tier: bool,
    ) -> Result<Self, Vec<Fault>> {
        let mut problems: Vec<Fault> = Vec::new();
        let mut services: Vec<Service> = Vec::new();
        let mut tool_names: BTreeSet<String> = BTreeSet::new();
        tool_names.insert(COMMAND_TOOL.to_owned());
        // The names of P@x's own operations are taken, offered or not.
        tool_names.extend(ASSISTANT_TOOLS.iter().map(|name| (*name).to_owned()));

        for text in manifests {
            let manifest = match ToolManifest::from_json(text) {
                Ok(manifest) => manifest,
                Err(found) => {
                    problems.extend(faults(found));
                    continue;
                }
            };
            let taken = manifest.service() == BUILT_IN_SERVICE
                || services
                    .iter()
                    .any(|service| service.manifest.service() == manifest.service());
            if taken {
                problems.push(Fault::new(
                    TaskCode::RegistryDuplicateService,
                    format!("{} is declared twice or is reserved", manifest.service()),
                ));
                continue;
            }
            for tool in manifest.tools() {
                if !tool_names.insert(tool.name().to_owned()) {
                    problems.push(Fault::new(
                        TaskCode::RegistryDuplicateTool,
                        format!(
                            "{} of {} has the name of another tool",
                            tool.name(),
                            manifest.service()
                        ),
                    ));
                }
            }
            services.push(Service {
                manifest,
                adapters: BTreeMap::new(),
            });
        }

        for text in adapters {
            let file = match AdapterFile::from_json(text) {
                Ok(file) => file,
                Err(found) => {
                    problems.extend(found);
                    continue;
                }
            };
            let Some(service) = services
                .iter_mut()
                .find(|service| service.manifest.service() == file.service())
            else {
                problems.push(Fault::new(
                    TaskCode::AdapterUnknownTool,
                    format!("no manifest declares the service {}", file.service()),
                ));
                continue;
            };
            for adapter in file.into_adapters() {
                let Some(tool) = service.manifest.tool(adapter.tool()) else {
                    problems.push(Fault::new(
                        TaskCode::AdapterUnknownTool,
                        format!(
                            "{} declares no tool named {}",
                            service.manifest.service(),
                            adapter.tool()
                        ),
                    ));
                    continue;
                };
                problems.extend(adapter.check_against(tool));
                if service.adapters.contains_key(adapter.tool()) {
                    problems.push(Fault::new(
                        TaskCode::AdapterUnknownTool,
                        format!("{} has two adapters", adapter.tool()),
                    ));
                    continue;
                }
                service.adapters.insert(adapter.tool().to_owned(), adapter);
            }
        }

        let built_in = if command_tier {
            match ToolManifest::from_json(BUILT_IN_MANIFEST) {
                Ok(manifest) => Some(manifest),
                Err(found) => {
                    problems.extend(faults(found));
                    None
                }
            }
        } else {
            None
        };

        if problems.is_empty() {
            Ok(ToolRegistry {
                services,
                built_in,
                assistant: None,
                assistant_tools: AssistantTools::default(),
            })
        } else {
            Err(problems)
        }
    }

    /// The same registry, offering P@x's own operations as well.
    ///
    /// Only a caller that can put a proposal in front of the person offers
    /// them: the session service does, `tur-pax run` does not.
    #[must_use]
    pub fn with_assistant_tools(mut self, tools: AssistantTools) -> Self {
        self.assistant_tools = tools;
        self.assistant = if tools.schedules || tools.notes {
            ToolManifest::from_json(ASSISTANT_MANIFEST).ok()
        } else {
            None
        };
        self
    }

    /// Whether the named tool is one of P@x's own operations and is offered.
    fn offers_assistant_tool(&self, name: &str) -> bool {
        match name {
            PROPOSE_REMINDER_TOOL | PROPOSE_TASK_TOOL => self.assistant_tools.schedules,
            KEEP_NOTE_TOOL => self.assistant_tools.notes,
            _ => false,
        }
    }

    /// The tools that are offered: every tool with an adapter, in manifest
    /// order, then the built-in command tool if it is enabled.
    pub fn offered(&self) -> Vec<OfferedTool> {
        let mut offered = Vec::new();
        for (service, tool) in self.performable() {
            offered.push(OfferedTool {
                name: tool.name().to_owned(),
                service: service.to_owned(),
                class: tool.class(),
            });
        }
        offered
    }

    /// The names of the tools a manifest declares that have no adapter, and
    /// so are not offered.
    pub fn unavailable(&self) -> Vec<String> {
        let mut names = Vec::new();
        for service in &self.services {
            for tool in service.manifest.tools() {
                if !service.adapters.contains_key(tool.name()) {
                    names.push(tool.name().to_owned());
                }
            }
        }
        names
    }

    /// Whether the built-in command tool is offered.
    pub fn has_command_tier(&self) -> bool {
        self.built_in.is_some()
    }

    /// The description of an offered tool, as its manifest states it.
    pub fn description_of(&self, name: &str) -> Option<&str> {
        self.performable()
            .into_iter()
            .find(|(_, tool)| tool.name() == name)
            .map(|(_, tool)| tool.description())
    }

    /// The tool definitions to give a provider: the name, the description
    /// and the input schema of every offered tool, exactly as its manifest
    /// states them. Nothing else about a tool is sent.
    ///
    /// # Errors
    ///
    /// A fault if a tool cannot be described to a provider.
    pub fn definitions(&self) -> Result<Vec<ToolDefinition>, Fault> {
        let mut definitions = Vec::new();
        for (_, tool) in self.performable() {
            let definition =
                ToolDefinition::new(tool.name(), tool.description(), tool.input_schema().clone())
                    .map_err(|error| {
                    Fault::from(Problem::new(
                        Code::ManifestToolField,
                        format!("{} cannot be offered to a provider: {error}", tool.name()),
                    ))
                })?;
            definitions.push(definition);
        }
        Ok(definitions)
    }

    /// Every tool that can be performed, with the service that declares it.
    fn performable(&self) -> Vec<(&str, &ToolSpec)> {
        let mut tools = Vec::new();
        for service in &self.services {
            for tool in service.manifest.tools() {
                if service.adapters.contains_key(tool.name()) {
                    tools.push((service.manifest.service(), tool));
                }
            }
        }
        if let Some(built_in) = &self.built_in {
            for tool in built_in.tools() {
                tools.push((built_in.service(), tool));
            }
        }
        if let Some(assistant) = &self.assistant {
            for tool in assistant.tools() {
                if self.offers_assistant_tool(tool.name()) {
                    tools.push((assistant.service(), tool));
                }
            }
        }
        tools
    }

    /// Turn a model's tool call into a proposed action.
    ///
    /// `name` and `arguments` are what the model sent. The service, the
    /// class and the input rules come from the manifest that declares the
    /// tool.
    ///
    /// # Errors
    ///
    /// `TUR-AI-TOOL-001` if no offered tool has that name; `TUR-AI-TOOL-002`
    /// if the arguments do not match the tool's input schema. No action
    /// exists then, so nothing can be decided or run.
    pub fn propose(
        &self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> Result<Proposal<'_>, Vec<Fault>> {
        if name == COMMAND_TOOL
            && let Some(built_in) = &self.built_in
        {
            // The same check as for any tool; the action is a command.
            built_in
                .tool_call(COMMAND_TOOL, arguments.clone())
                .map_err(faults)?;
            let argv: Vec<String> = arguments
                .get("argv")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let action = Action::Command(CommandAction {
                sandbox: Sandbox::ReadOnly,
                argv: argv.clone(),
                working_context: WorkingContext::Session,
            });
            return Ok(Proposal {
                action,
                performer: Performer::Command { argv },
            });
        }
        if self.offers_assistant_tool(name)
            && let Some(assistant) = &self.assistant
        {
            // The same check as for any tool. The action is one of the
            // engine's own operations, whose class the engine fixes.
            assistant
                .tool_call(name, arguments.clone())
                .map_err(faults)?;
            let (operation, call) = if name == KEEP_NOTE_TOOL {
                (
                    AgentOperation::MemoryRecordStated,
                    AssistantCall::KeepNote(arguments),
                )
            } else if name == PROPOSE_TASK_TOOL {
                (
                    AgentOperation::ScheduleProposal,
                    AssistantCall::ProposeTask(arguments),
                )
            } else {
                (
                    AgentOperation::ScheduleProposal,
                    AssistantCall::ProposeReminder(arguments),
                )
            };
            return Ok(Proposal {
                action: Action::Agent(operation),
                performer: Performer::Assistant(call),
            });
        }
        for service in &self.services {
            if let Some(adapter) = service.adapters.get(name) {
                let call = service
                    .manifest
                    .tool_call(name, arguments.clone())
                    .map_err(faults)?;
                return Ok(Proposal {
                    action: Action::Tool(call),
                    performer: Performer::Adapter { adapter, arguments },
                });
            }
        }
        Err(vec![Fault::from(Problem::new(
            Code::ToolUnknown,
            "no offered tool has that name",
        ))])
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const TEST_MANIFEST: &str = include_str!("../tests/data/test-service.manifest.json");
    const TEST_ADAPTERS: &str = include_str!("../tests/data/test-service.adapters.json");

    fn registry() -> ToolRegistry {
        ToolRegistry::load(
            &[TEST_MANIFEST.to_owned()],
            &[TEST_ADAPTERS.to_owned()],
            true,
        )
        .expect("a valid tool set")
    }

    fn arguments(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("an object")
    }

    fn codes(result: Result<ToolRegistry, Vec<Fault>>) -> Vec<String> {
        result
            .expect_err("the tool set is refused")
            .iter()
            .map(|fault| fault.code.to_string())
            .collect()
    }

    #[test]
    fn the_offered_tools_are_the_ones_with_an_adapter_and_the_command_tool() {
        let registry = registry();
        let offered: Vec<(String, u8)> = registry
            .offered()
            .into_iter()
            .map(|tool| (tool.name, tool.class.as_u8()))
            .collect();
        assert_eq!(
            offered,
            [
                ("test_observe".to_owned(), 0),
                ("test_reversible".to_owned(), 1),
                ("test_consequential".to_owned(), 2),
                ("test_system_critical".to_owned(), 3),
                (COMMAND_TOOL.to_owned(), 0),
            ]
        );
        assert!(registry.unavailable().is_empty());
        assert!(registry.has_command_tier());

        let definitions = registry.definitions().expect("definitions");
        assert_eq!(definitions.len(), 5);
        // What a provider is told about a tool is its name, description and
        // input schema: the class is not among them.
        let manifest = ToolManifest::from_json(TEST_MANIFEST).expect("a valid manifest");
        for definition in &definitions[..4] {
            let tool = manifest.tool(definition.name()).expect("a declared tool");
            assert_eq!(definition.description(), tool.description());
            assert_eq!(definition.input_schema(), tool.input_schema());
        }

        let without =
            ToolRegistry::load(&[TEST_MANIFEST.to_owned()], &[], false).expect("a valid tool set");
        assert!(without.offered().is_empty());
        assert_eq!(without.unavailable().len(), 4);
        assert!(!without.has_command_tier());
        assert!(without.propose(COMMAND_TOOL, Map::new()).is_err());
    }

    #[test]
    fn the_action_takes_its_service_and_class_from_the_manifest() {
        let registry = registry();
        let proposal = registry
            .propose(
                "test_system_critical",
                arguments(json!({ "volume_id": "vol-1" })),
            )
            .expect("a proposal");
        let Action::Tool(call) = &proposal.action else {
            panic!("expected a tool call");
        };
        assert_eq!(call.service(), "org.tur.Test1");
        assert_eq!(call.tool(), "test_system_critical");
        assert_eq!(call.declared_class(), ActionClass::SystemCritical);
        assert!(matches!(proposal.performer, Performer::Adapter { .. }));
    }

    #[test]
    fn a_claim_about_the_class_or_an_invented_tool_produces_no_action() {
        let registry = registry();
        let refused = [
            // The model states a class, a service or a sandbox of its own.
            (
                "test_system_critical",
                json!({ "volume_id": "vol-1", "class": 0 }),
                "TUR-AI-TOOL-002",
            ),
            (
                "test_consequential",
                json!({ "recipient": "a@example.org", "declared_class": 1 }),
                "TUR-AI-TOOL-002",
            ),
            (
                "test_observe",
                json!({ "service": "org.tur.Other1" }),
                "TUR-AI-TOOL-002",
            ),
            (
                COMMAND_TOOL,
                json!({ "argv": ["/usr/bin/id"], "sandbox": "user" }),
                "TUR-AI-TOOL-002",
            ),
            (
                COMMAND_TOOL,
                json!({ "argv": ["/usr/bin/id"], "privileged": true }),
                "TUR-AI-TOOL-002",
            ),
            // Arguments outside the schema.
            ("test_system_critical", json!({}), "TUR-AI-TOOL-002"),
            (
                "test_system_critical",
                json!({ "volume_id": 7 }),
                "TUR-AI-TOOL-002",
            ),
            (
                "test_system_critical",
                json!({ "volume_id": "" }),
                "TUR-AI-TOOL-002",
            ),
            (COMMAND_TOOL, json!({}), "TUR-AI-TOOL-002"),
            (COMMAND_TOOL, json!({ "argv": "id" }), "TUR-AI-TOOL-002"),
            (COMMAND_TOOL, json!({ "argv": [1, 2] }), "TUR-AI-TOOL-002"),
            // Names no manifest declares, however close.
            ("test_format", json!({}), "TUR-AI-TOOL-001"),
            ("Test_Observe", json!({}), "TUR-AI-TOOL-001"),
            ("test_observe ", json!({}), "TUR-AI-TOOL-001"),
            ("org.tur.Test1:test_observe", json!({}), "TUR-AI-TOOL-001"),
            ("run_command", json!({ "argv": ["id"] }), "TUR-AI-TOOL-001"),
            ("", json!({}), "TUR-AI-TOOL-001"),
        ];
        for (name, value, code) in refused {
            let found = registry
                .propose(name, arguments(value.clone()))
                .expect_err(&format!("{name} {value} became an action"));
            assert_eq!(found[0].code.as_str(), code, "{name} {value}");
        }
    }

    #[test]
    fn the_command_tool_is_always_a_read_only_command() {
        let registry = registry();
        let proposal = registry
            .propose(
                COMMAND_TOOL,
                arguments(json!({ "argv": ["/usr/bin/sudo", "rm", "-rf", "/"] })),
            )
            .expect("a proposal");
        let expected: Vec<String> = ["/usr/bin/sudo", "rm", "-rf", "/"]
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect();
        assert_eq!(
            proposal.action,
            Action::Command(CommandAction {
                sandbox: Sandbox::ReadOnly,
                argv: expected.clone(),
                working_context: WorkingContext::Session,
            })
        );
        assert_eq!(proposal.performer, Performer::Command { argv: expected });
    }

    #[test]
    fn a_tool_set_with_a_problem_offers_nothing() {
        let other_service = TEST_MANIFEST.replace("org.tur.Test1", "org.tur.Other1");
        let reserved = TEST_MANIFEST.replace("org.tur.Test1", BUILT_IN_SERVICE);
        let command_name = TEST_MANIFEST.replace("test_observe", COMMAND_TOOL);
        let cases: Vec<(Vec<String>, Vec<String>, &str)> = vec![
            // The same service twice.
            (
                vec![TEST_MANIFEST.to_owned(), TEST_MANIFEST.to_owned()],
                vec![],
                "TUR-AI-REGISTRY-001",
            ),
            // The reserved service.
            (vec![reserved], vec![], "TUR-AI-REGISTRY-001"),
            // The same tool names under another service.
            (
                vec![TEST_MANIFEST.to_owned(), other_service],
                vec![],
                "TUR-AI-REGISTRY-002",
            ),
            // A tool with the name of the built-in tool.
            (vec![command_name], vec![], "TUR-AI-REGISTRY-002"),
            // An invalid manifest.
            (vec!["{}".to_owned()], vec![], "TUR-AI-MANIFEST-001"),
            // Adapters for a service or a tool that does not exist, and two
            // adapters for one tool.
            (vec![], vec![TEST_ADAPTERS.to_owned()], "TUR-AI-ADAPTER-003"),
            (
                vec![TEST_MANIFEST.to_owned()],
                vec![TEST_ADAPTERS.replace("test_observe", "test_missing")],
                "TUR-AI-ADAPTER-003",
            ),
            (
                vec![TEST_MANIFEST.to_owned()],
                vec![TEST_ADAPTERS.to_owned(), TEST_ADAPTERS.to_owned()],
                "TUR-AI-ADAPTER-003",
            ),
            // An adapter that refers to an input its tool does not declare.
            (
                vec![TEST_MANIFEST.to_owned()],
                vec![TEST_ADAPTERS.replace("\"input\": \"value\"", "\"input\": \"other\"")],
                "TUR-AI-ADAPTER-004",
            ),
            // An adapter file that does not parse.
            (
                vec![TEST_MANIFEST.to_owned()],
                vec!["[]".to_owned()],
                "TUR-AI-ADAPTER-001",
            ),
        ];
        for (manifests, adapters, code) in cases {
            let found = codes(ToolRegistry::load(&manifests, &adapters, true));
            assert!(
                found.iter().any(|text| text == code),
                "expected {code}, found {found:?}"
            );
        }
    }

    #[test]
    fn the_built_in_manifest_is_valid() {
        let manifest = ToolManifest::from_json(BUILT_IN_MANIFEST).expect("a valid manifest");
        assert_eq!(manifest.service(), BUILT_IN_SERVICE);
        assert_eq!(manifest.tools().len(), 1);
        assert_eq!(manifest.tools()[0].name(), COMMAND_TOOL);
        assert_eq!(manifest.tools()[0].class(), ActionClass::Observe);
    }
}
