// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Tool adapters: how a tool of a manifest is actually performed.
//!
//! A manifest says what a tool is: its name, its class and the shape of its
//! input. It names a D-Bus method, and the services that will answer those
//! methods do not exist yet. An adapter says how the tool is performed today.
//! There is one kind, `command`: run a TUR command-line program, in the
//! read-only sandbox, and return what it prints.
//!
//! An adapter is data, in a file described by
//! `schemas/pax-tool-adapter.schema.json`:
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "service": "org.tur.Catalogue1",
//!   "adapters": [
//!     {
//!       "tool": "catalogue_show",
//!       "kind": "command",
//!       "program": "/usr/bin/tur-catalogue",
//!       "arguments": [
//!         { "literal": "show" },
//!         { "literal": "--json" },
//!         { "literal": "--" },
//!         { "input": "id" }
//!       ],
//!       "success_exit_codes": [0, 6]
//!     }
//!   ]
//! }
//! ```
//!
//! The rules that make this safe to drive from a model's output:
//!
//! * An adapter never adds a tool and never states a class. Both come from
//!   the manifest.
//! * The program is a fixed absolute path. It is never looked up along a
//!   search path and never chosen by the model.
//! * The command line is an argument vector. There is no shell, and no value
//!   is ever joined into a string that something will split again.
//! * An argument is a literal of the file, or one value of the tool's input
//!   as one argument. The input was validated against the manifest's schema
//!   before the adapter sees it, and every input an adapter refers to is
//!   checked against that schema when the adapter is loaded.
//! * A value placed as an argument of its own may not begin with `-`, so it
//!   cannot be read as an option. A value attached to an option is written
//!   as `--option=value`, one argument.
//! * A value may not hold a control character.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::faults::{Fault, TaskCode};
use crate::manifest::ToolSpec;
use crate::sandbox::is_plain_absolute;

/// The adapter format version this build reads.
pub const ADAPTER_SCHEMA_VERSION: u64 = 1;

/// The longest value of a tool input that is placed on a command line, in
/// bytes.
pub const INPUT_VALUE_MAX: usize = 4096;

/// The default wall-clock limit of an adapter's program, in seconds.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 20;

/// The longest wall-clock limit an adapter may ask for, in seconds.
pub const MAX_TIMEOUT_SECONDS: u64 = 120;

/// The default bound on what is kept of the program's output, in bytes.
pub const DEFAULT_OUTPUT_BYTES: usize = 64 * 1024;

/// The largest bound an adapter may ask for, in bytes.
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;

/// The bound on what a feeding program may produce, in bytes.
pub const FEED_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    schema_version: u64,
    service: String,
    adapters: Vec<RawAdapter>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAdapter {
    tool: String,
    kind: String,
    program: String,
    #[serde(default)]
    arguments: Vec<RawArgument>,
    #[serde(default)]
    success_exit_codes: Option<Vec<i64>>,
    #[serde(default)]
    reads: Vec<String>,
    #[serde(default)]
    feed: Option<RawFeed>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    max_output_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArgument {
    #[serde(default)]
    literal: Option<String>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    option: Option<String>,
    #[serde(default)]
    flag: Option<String>,
    #[serde(default)]
    when: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFeed {
    program: String,
    #[serde(default)]
    arguments: Vec<String>,
}

/// One argument of an adapter's command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentTemplate {
    /// A fixed argument: `{ "literal": "--json" }`.
    Literal(String),
    /// The value of a text or whole-number input, as one argument:
    /// `{ "input": "id" }`. Left out when the input is absent. Refused when
    /// the value begins with `-`.
    Input(String),
    /// The value of a text or whole-number input attached to an option, as
    /// the one argument `--option=value`: `{ "option": "--access", "input":
    /// "access" }`. Left out when the input is absent.
    Option {
        /// The option, such as `--access`.
        option: String,
        /// The input whose value follows the `=`.
        input: String,
    },
    /// A fixed option that is present when a true-or-false input is true:
    /// `{ "flag": "--redact", "when": "redact" }`.
    Flag {
        /// The option, such as `--redact`.
        flag: String,
        /// The true-or-false input that switches it on.
        when: String,
    },
}

/// A fixed command whose standard output becomes the standard input of an
/// adapter's program. It takes nothing from the tool's input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    /// The program, an absolute path.
    pub program: String,
    /// Its arguments, all fixed.
    pub arguments: Vec<String>,
}

impl Feed {
    /// The feeding command as an argument vector.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = vec![self.program.clone()];
        argv.extend(self.arguments.iter().cloned());
        argv
    }
}

/// How one tool is performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    tool: String,
    program: String,
    arguments: Vec<ArgumentTemplate>,
    success_exit_codes: Vec<i32>,
    reads: Vec<String>,
    feed: Option<Feed>,
    timeout_seconds: u64,
    max_output_bytes: usize,
}

/// The adapters of one service, as loaded from one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterFile {
    service: String,
    adapters: Vec<Adapter>,
}

fn fault(code: TaskCode, detail: String) -> Fault {
    Fault::new(code, detail)
}

fn is_option_name(text: &str) -> bool {
    text.strip_prefix("--").is_some_and(|name| {
        let mut bytes = name.bytes();
        name.len() <= 64
            && matches!(bytes.next(), Some(b'a'..=b'z'))
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

fn is_literal(text: &str) -> bool {
    text.len() <= INPUT_VALUE_MAX && !text.chars().any(char::is_control)
}

impl AdapterFile {
    /// Load and validate an adapter file from its JSON text.
    ///
    /// Returns every problem found, not only the first. What can only be
    /// checked against a manifest is checked by [`Adapter::check_against`].
    pub fn from_json(text: &str) -> Result<Self, Vec<Fault>> {
        let raw: RawFile = serde_json::from_str(text)
            .map_err(|error| vec![fault(TaskCode::AdapterParse, error.to_string())])?;
        let mut problems = Vec::new();
        if raw.schema_version != ADAPTER_SCHEMA_VERSION {
            problems.push(fault(
                TaskCode::AdapterVersion,
                format!(
                    "schema_version is {}, this build reads {ADAPTER_SCHEMA_VERSION}",
                    raw.schema_version
                ),
            ));
        }
        if raw.adapters.is_empty() {
            problems.push(fault(
                TaskCode::AdapterParse,
                "adapters must list at least one adapter".to_owned(),
            ));
        }
        let mut adapters = Vec::with_capacity(raw.adapters.len());
        for (index, adapter) in raw.adapters.into_iter().enumerate() {
            let at = format!("adapters[{index}]");
            match convert(adapter, &at) {
                Ok(adapter) => adapters.push(adapter),
                Err(found) => problems.extend(found),
            }
        }
        if problems.is_empty() {
            Ok(AdapterFile {
                service: raw.service,
                adapters,
            })
        } else {
            Err(problems)
        }
    }

    /// The D-Bus name of the service whose tools these adapters perform.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The adapters, in file order.
    pub fn adapters(&self) -> &[Adapter] {
        &self.adapters
    }

    /// The adapters, taken out of the file.
    pub fn into_adapters(self) -> Vec<Adapter> {
        self.adapters
    }
}

fn convert(raw: RawAdapter, at: &str) -> Result<Adapter, Vec<Fault>> {
    let mut problems = Vec::new();
    if raw.kind != "command" {
        problems.push(fault(
            TaskCode::AdapterParse,
            format!("{at}.kind must be \"command\", the one kind this build performs"),
        ));
    }
    if !is_plain_absolute(&raw.program) || raw.program == "/" {
        problems.push(fault(
            TaskCode::AdapterProgram,
            format!("{at}.program must be the full path of a program"),
        ));
    }
    for path in &raw.reads {
        if !is_plain_absolute(path) || path == "/" {
            problems.push(fault(
                TaskCode::AdapterProgram,
                format!("{at}.reads must hold plain absolute paths"),
            ));
        }
    }

    let mut arguments = Vec::with_capacity(raw.arguments.len());
    for (index, argument) in raw.arguments.into_iter().enumerate() {
        let place = format!("{at}.arguments[{index}]");
        let template = match (
            argument.literal,
            argument.input,
            argument.option,
            argument.flag,
            argument.when,
        ) {
            (Some(literal), None, None, None, None) if is_literal(&literal) => {
                Some(ArgumentTemplate::Literal(literal))
            }
            (None, Some(input), None, None, None) => Some(ArgumentTemplate::Input(input)),
            (None, Some(input), Some(option), None, None) if is_option_name(&option) => {
                Some(ArgumentTemplate::Option { option, input })
            }
            (None, None, None, Some(flag), Some(when)) if is_option_name(&flag) => {
                Some(ArgumentTemplate::Flag { flag, when })
            }
            _ => None,
        };
        match template {
            Some(template) => arguments.push(template),
            None => problems.push(fault(
                TaskCode::AdapterArgument,
                format!(
                    "{place} must be one of {{\"literal\"}}, {{\"input\"}}, \
                     {{\"option\", \"input\"}} or {{\"flag\", \"when\"}}, with an option of the \
                     form --name"
                ),
            )),
        }
    }

    let mut success_exit_codes = Vec::new();
    for code in raw.success_exit_codes.unwrap_or_else(|| vec![0]) {
        match i32::try_from(code) {
            Ok(code) if (0..=255).contains(&code) => success_exit_codes.push(code),
            _ => problems.push(fault(
                TaskCode::AdapterParse,
                format!("{at}.success_exit_codes must hold exit statuses from 0 to 255"),
            )),
        }
    }
    if success_exit_codes.is_empty() {
        problems.push(fault(
            TaskCode::AdapterParse,
            format!("{at}.success_exit_codes must not be empty"),
        ));
    }

    let timeout_seconds = raw.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    if timeout_seconds == 0 || timeout_seconds > MAX_TIMEOUT_SECONDS {
        problems.push(fault(
            TaskCode::AdapterParse,
            format!("{at}.timeout_seconds must be from 1 to {MAX_TIMEOUT_SECONDS}"),
        ));
    }
    let max_output_bytes = match raw.max_output_bytes {
        None => DEFAULT_OUTPUT_BYTES,
        Some(bytes) => usize::try_from(bytes).unwrap_or(usize::MAX),
    };
    if !(1024..=MAX_OUTPUT_BYTES).contains(&max_output_bytes) {
        problems.push(fault(
            TaskCode::AdapterParse,
            format!("{at}.max_output_bytes must be from 1024 to {MAX_OUTPUT_BYTES}"),
        ));
    }

    let feed = raw.feed.map(|feed| Feed {
        program: feed.program,
        arguments: feed.arguments,
    });
    if let Some(feed) = &feed {
        if !is_plain_absolute(&feed.program) || feed.program == "/" {
            problems.push(fault(
                TaskCode::AdapterProgram,
                format!("{at}.feed.program must be the full path of a program"),
            ));
        }
        if !feed.arguments.iter().all(|argument| is_literal(argument)) {
            problems.push(fault(
                TaskCode::AdapterArgument,
                format!("{at}.feed.arguments must be plain text"),
            ));
        }
    }

    if problems.is_empty() {
        Ok(Adapter {
            tool: raw.tool,
            program: raw.program,
            arguments,
            success_exit_codes,
            reads: raw.reads,
            feed,
            timeout_seconds,
            max_output_bytes,
        })
    } else {
        Err(problems)
    }
}

impl Adapter {
    /// The name of the tool this adapter performs.
    pub fn tool(&self) -> &str {
        &self.tool
    }

    /// The program that is run.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The argument templates, in order.
    pub fn arguments(&self) -> &[ArgumentTemplate] {
        &self.arguments
    }

    /// The exit statuses that mean the program succeeded.
    pub fn success_exit_codes(&self) -> &[i32] {
        &self.success_exit_codes
    }

    /// Paths outside the system view that the program has to read.
    pub fn reads(&self) -> &[String] {
        &self.reads
    }

    /// The command whose output is given to the program on standard input.
    pub fn feed(&self) -> Option<&Feed> {
        self.feed.as_ref()
    }

    /// The wall-clock limit of the program, in seconds.
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }

    /// The bound on what is kept of the program's output, in bytes.
    pub fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }

    /// Check the adapter against the tool it is for: every input it refers
    /// to must be declared by the tool's input schema, with a kind that fits
    /// the form of the argument.
    pub fn check_against(&self, tool: &ToolSpec) -> Vec<Fault> {
        let properties = tool
            .input_schema()
            .get("properties")
            .and_then(Value::as_object);
        let mut problems = Vec::new();
        for (index, argument) in self.arguments.iter().enumerate() {
            let fits = match argument {
                ArgumentTemplate::Literal(_) => true,
                ArgumentTemplate::Input(input) | ArgumentTemplate::Option { input, .. } => {
                    matches!(kind_of(properties, input), Some("string" | "integer"))
                }
                ArgumentTemplate::Flag { when, .. } => {
                    kind_of(properties, when) == Some("boolean")
                }
            };
            if !fits {
                problems.push(fault(
                    TaskCode::AdapterArgument,
                    format!(
                        "the adapter of {} refers, in arguments[{index}], to an input the tool \
                         does not declare or to one of a kind that cannot be placed there",
                        self.tool
                    ),
                ));
            }
        }
        problems
    }

    /// Build the command line for one call: the program, then one argument
    /// for each template.
    ///
    /// `input` is the tool input after validation against the manifest. A
    /// value that cannot be passed safely is refused here: the call is not
    /// performed.
    pub fn argv(&self, input: &Map<String, Value>) -> Result<Vec<String>, Fault> {
        let mut argv = vec![self.program.clone()];
        for argument in &self.arguments {
            match argument {
                ArgumentTemplate::Literal(literal) => argv.push(literal.clone()),
                ArgumentTemplate::Input(name) => {
                    if let Some(value) = text_of(input.get(name))? {
                        if value.starts_with('-') {
                            return Err(refused(
                                "a value that would be read as an option was refused",
                            ));
                        }
                        argv.push(value);
                    }
                }
                ArgumentTemplate::Option { option, input: name } => {
                    if let Some(value) = text_of(input.get(name))? {
                        argv.push(format!("{option}={value}"));
                    }
                }
                ArgumentTemplate::Flag { flag, when } => {
                    if input.get(when) == Some(&Value::Bool(true)) {
                        argv.push(flag.clone());
                    }
                }
            }
        }
        Ok(argv)
    }
}

/// The declared type of an input property, if the schema declares it.
fn kind_of<'a>(properties: Option<&'a Map<String, Value>>, name: &str) -> Option<&'a str> {
    properties
        .and_then(|known| known.get(name))
        .and_then(|schema| schema.get("type"))
        .and_then(Value::as_str)
}

fn refused(detail: &str) -> Fault {
    Fault::from(crate::codes::Problem::new(
        crate::codes::Code::ToolArguments,
        detail,
    ))
}

/// The text of an input value that may be placed on a command line: a string
/// or a whole number. `None` if the input is absent.
fn text_of(value: Option<&Value>) -> Result<Option<String>, Fault> {
    let text = match value {
        None => return Ok(None),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) if number.is_i64() || number.is_u64() => number.to_string(),
        Some(_) => {
            return Err(refused(
                "a value of a kind that cannot be passed to a program was refused",
            ));
        }
    };
    if text.len() > INPUT_VALUE_MAX {
        return Err(refused("a value that is too long was refused"));
    }
    if text.chars().any(char::is_control) {
        return Err(refused(
            "a value with a control character was refused",
        ));
    }
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::manifest::ToolManifest;

    const FILE: &str = r#"{
        "schema_version": 1,
        "service": "org.tur.Example1",
        "adapters": [
            {
                "tool": "example_show",
                "kind": "command",
                "program": "/usr/bin/tur-example",
                "arguments": [
                    { "literal": "show" },
                    { "literal": "--json" },
                    { "option": "--access", "input": "access" },
                    { "flag": "--redact", "when": "redact" },
                    { "literal": "--" },
                    { "input": "id" },
                    { "input": "count" }
                ],
                "success_exit_codes": [0, 6],
                "reads": ["/usr/share/tur/example"],
                "feed": { "program": "/usr/bin/lsblk", "arguments": ["--json"] },
                "timeout_seconds": 5,
                "max_output_bytes": 2048
            }
        ]
    }"#;

    const MANIFEST: &str = r#"{
        "schema_version": 1,
        "service": "org.tur.Example1",
        "tools": [{
            "name": "example_show",
            "description": "Show one example.",
            "class": 0,
            "method": "Show",
            "input_schema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "count": { "type": "integer" },
                    "access": { "type": "string", "enum": ["ro", "rw"] },
                    "redact": { "type": "boolean" },
                    "tags": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id"],
                "additionalProperties": false
            }
        }]
    }"#;

    fn adapter() -> Adapter {
        AdapterFile::from_json(FILE)
            .expect("a valid adapter file")
            .into_adapters()
            .remove(0)
    }

    fn input(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("an object")
    }

    #[test]
    fn a_file_loads_with_its_defaults_and_its_stated_values() {
        let file = AdapterFile::from_json(FILE).expect("a valid adapter file");
        assert_eq!(file.service(), "org.tur.Example1");
        let adapter = &file.adapters()[0];
        assert_eq!(adapter.tool(), "example_show");
        assert_eq!(adapter.program(), "/usr/bin/tur-example");
        assert_eq!(adapter.success_exit_codes(), [0, 6]);
        assert_eq!(adapter.reads(), ["/usr/share/tur/example"]);
        assert_eq!(adapter.timeout_seconds(), 5);
        assert_eq!(adapter.max_output_bytes(), 2048);
        assert_eq!(
            adapter.feed().map(Feed::argv),
            Some(vec!["/usr/bin/lsblk".to_owned(), "--json".to_owned()])
        );

        let minimal = AdapterFile::from_json(
            r#"{"schema_version":1,"service":"org.tur.Example1","adapters":[
                {"tool":"example_show","kind":"command","program":"/usr/bin/tur-example"}]}"#,
        )
        .expect("a valid adapter file");
        let adapter = &minimal.adapters()[0];
        assert!(adapter.arguments().is_empty());
        assert_eq!(adapter.success_exit_codes(), [0]);
        assert_eq!(adapter.timeout_seconds(), DEFAULT_TIMEOUT_SECONDS);
        assert_eq!(adapter.max_output_bytes(), DEFAULT_OUTPUT_BYTES);
        assert!(adapter.feed().is_none());
        assert!(adapter.reads().is_empty());
    }

    #[test]
    fn the_command_line_is_built_one_argument_per_value() {
        let adapter = adapter();
        let argv = adapter
            .argv(&input(json!({
                "id": "com.example.app; rm -rf ~ $(reboot) `x` | y > z",
                "count": 3,
                "access": "ro",
                "redact": true
            })))
            .expect("a command line");
        assert_eq!(
            argv,
            [
                "/usr/bin/tur-example",
                "show",
                "--json",
                "--access=ro",
                "--redact",
                "--",
                // Shell syntax is only text: one argument, passed as it is.
                "com.example.app; rm -rf ~ $(reboot) `x` | y > z",
                "3"
            ]
        );

        // Absent inputs and a false flag leave nothing behind.
        let argv = adapter
            .argv(&input(json!({ "id": "a b", "redact": false })))
            .expect("a command line");
        assert_eq!(
            argv,
            ["/usr/bin/tur-example", "show", "--json", "--", "a b"]
        );
    }

    #[test]
    fn a_value_that_could_be_read_as_something_else_is_refused() {
        let adapter = adapter();
        let refused = [
            json!({ "id": "--catalogue=/etc" }),
            json!({ "id": "-x" }),
            json!({ "id": "-" }),
            json!({ "id": "a\nb" }),
            json!({ "id": "a\u{0}b" }),
            json!({ "id": "a\u{1b}[2Jb" }),
            json!({ "id": "x".repeat(INPUT_VALUE_MAX + 1) }),
            json!({ "id": "ok", "access": "ro\n--other" }),
            json!({ "id": 1.5 }),
            json!({ "id": ["a"] }),
            json!({ "id": { "a": 1 } }),
            json!({ "id": true }),
            json!({ "id": null }),
        ];
        for value in refused {
            let result = adapter.argv(&input(value.clone()));
            let fault = result.expect_err(&format!("{value} was accepted"));
            assert_eq!(fault.code.as_str(), "TUR-AI-TOOL-002", "{value}");
            // The refusal never repeats the value.
            assert!(!fault.detail.contains("catalogue"), "{fault}");
        }
        // A value attached to an option may begin with a hyphen: it cannot
        // be read as an option of its own.
        let argv = adapter
            .argv(&input(json!({ "id": "ok", "access": "-rw" })))
            .expect("a command line");
        assert!(argv.contains(&"--access=-rw".to_owned()));
    }

    #[test]
    fn an_adapter_is_checked_against_the_inputs_its_tool_declares() {
        let manifest = ToolManifest::from_json(MANIFEST).expect("a valid manifest");
        let tool = manifest.tool("example_show").expect("the tool");
        assert!(adapter().check_against(tool).is_empty());

        let wrong = [
            r#"{ "input": "missing" }"#,
            r#"{ "input": "redact" }"#,
            r#"{ "input": "tags" }"#,
            r#"{ "option": "--id", "input": "tags" }"#,
            r#"{ "flag": "--id", "when": "id" }"#,
            r#"{ "flag": "--id", "when": "missing" }"#,
        ];
        for argument in wrong {
            let text = format!(
                r#"{{"schema_version":1,"service":"org.tur.Example1","adapters":[
                    {{"tool":"example_show","kind":"command","program":"/usr/bin/tur-example",
                      "arguments":[{argument}]}}]}}"#
            );
            let file = AdapterFile::from_json(&text).expect("a well-formed adapter file");
            let problems = file.adapters()[0].check_against(tool);
            assert_eq!(problems.len(), 1, "{argument}");
            assert_eq!(problems[0].code.as_str(), "TUR-AI-ADAPTER-004", "{argument}");
        }
    }

    #[test]
    fn a_malformed_file_is_rejected_with_a_code() {
        let with = |change: &str| {
            format!(
                r#"{{"schema_version":1,"service":"org.tur.Example1","adapters":[
                    {{"tool":"example_show","kind":"command","program":"/usr/bin/tur-example"{change}}}]}}"#
            )
        };
        let cases: &[(String, &str)] = &[
            ("not json".to_owned(), "TUR-AI-ADAPTER-001"),
            (
                r#"{"schema_version":2,"service":"org.tur.Example1","adapters":[
                    {"tool":"t","kind":"command","program":"/usr/bin/x"}]}"#
                    .to_owned(),
                "TUR-AI-ADAPTER-002",
            ),
            (
                r#"{"schema_version":1,"service":"org.tur.Example1","adapters":[]}"#.to_owned(),
                "TUR-AI-ADAPTER-001",
            ),
            (with(r#","unknown":1"#), "TUR-AI-ADAPTER-001"),
            (with(r#","class":0"#), "TUR-AI-ADAPTER-001"),
            (with(r#","program":"/usr/bin/other""#), "TUR-AI-ADAPTER-001"),
            (with(r#","success_exit_codes":[]"#), "TUR-AI-ADAPTER-001"),
            (with(r#","success_exit_codes":[256]"#), "TUR-AI-ADAPTER-001"),
            (with(r#","success_exit_codes":[-1]"#), "TUR-AI-ADAPTER-001"),
            (with(r#","timeout_seconds":0"#), "TUR-AI-ADAPTER-001"),
            (with(r#","timeout_seconds":121"#), "TUR-AI-ADAPTER-001"),
            (with(r#","max_output_bytes":10"#), "TUR-AI-ADAPTER-001"),
            (with(r#","max_output_bytes":99999999"#), "TUR-AI-ADAPTER-001"),
            (with(r#","reads":["relative"]"#), "TUR-AI-ADAPTER-005"),
            (with(r#","reads":["/a/../b"]"#), "TUR-AI-ADAPTER-005"),
            (
                with(r#","feed":{"program":"lsblk"}"#),
                "TUR-AI-ADAPTER-005",
            ),
            (
                with(r#","arguments":[{"literal":"a","input":"b"}]"#),
                "TUR-AI-ADAPTER-004",
            ),
            (with(r#","arguments":[{}]"#), "TUR-AI-ADAPTER-004"),
            (
                with(r#","arguments":[{"option":"access","input":"access"}]"#),
                "TUR-AI-ADAPTER-004",
            ),
            (
                with(r#","arguments":[{"option":"--Bad Name","input":"access"}]"#),
                "TUR-AI-ADAPTER-004",
            ),
            (
                with(r#","arguments":[{"flag":"--x"}]"#),
                "TUR-AI-ADAPTER-004",
            ),
            (
                with(r#","arguments":[{"literal":"a\nb"}]"#),
                "TUR-AI-ADAPTER-004",
            ),
        ];
        for (text, code) in cases {
            let problems = AdapterFile::from_json(text).expect_err(text);
            assert!(
                problems.iter().any(|problem| problem.code.as_str() == *code),
                "{text}: {problems:?}"
            );
        }

        for (text, code) in [
            (
                r#"{"schema_version":1,"service":"org.tur.Example1","adapters":[
                    {"tool":"t","kind":"dbus","program":"/usr/bin/x"}]}"#,
                "TUR-AI-ADAPTER-001",
            ),
            (
                r#"{"schema_version":1,"service":"org.tur.Example1","adapters":[
                    {"tool":"t","kind":"command","program":"tur-example"}]}"#,
                "TUR-AI-ADAPTER-005",
            ),
            (
                r#"{"schema_version":1,"service":"org.tur.Example1","adapters":[
                    {"tool":"t","kind":"command","program":"/usr/bin/../bin/sh"}]}"#,
                "TUR-AI-ADAPTER-005",
            ),
        ] {
            let problems = AdapterFile::from_json(text).expect_err(text);
            assert_eq!(problems[0].code.as_str(), code, "{text}");
        }
    }
}
