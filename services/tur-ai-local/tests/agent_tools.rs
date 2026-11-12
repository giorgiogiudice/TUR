// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The assistant tools this component ships: `pax/agent-tools.json`, and
//! beside it the adapters that perform them with this component's own
//! program, `pax/agent-tools.adapter.json`.
//!
//! The files are loaded with P@x's own loader (`tur-pax`, a development
//! dependency of this test only), and every command line an adapter builds
//! is held against the program this component has just built: the
//! subcommand has to be one the program lists, and every option on the line
//! one its help describes. The tools are also run for real, against a
//! recorded machine and with the environment cleared as P@x's sandbox does.
//!
//! All three tools read and nothing else. Downloading a model, starting or
//! stopping one, and allowing network access are not tools of this
//! manifest at all: they change the computer and use its resources, and
//! are the person's acts.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tur_pax::adapter::AdapterFile;
use tur_pax::registry::{ADAPTER_FILE, MANIFEST_FILE, ToolRegistry, read_tool_sets};
use tur_pax::{Action, ActionClass};

const MANIFEST: &str = include_str!("../pax/agent-tools.json");
const ADAPTERS: &str = include_str!("../pax/agent-tools.adapter.json");

/// The program this component built.
const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-ai-local");

/// Where the adapters expect it to be installed.
const INSTALLED: &str = "/usr/bin/tur-ai-local";

/// The service the manifest declares.
const SERVICE: &str = "org.tur.AiLocal1";

/// The tools P@x is offered, in the order of the manifest.
const OFFERED: [&str; 3] = ["ai_local_status", "ai_local_models", "ai_local_recommend"];

const CATALOGUE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/catalogue");
const SMALL_VM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hosts/small-vm");

fn registry() -> ToolRegistry {
    match ToolRegistry::load(&[MANIFEST.to_owned()], &[ADAPTERS.to_owned()], false) {
        Ok(registry) => registry,
        Err(problems) => {
            let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!(
                "the tools of this component do not load:\n{}",
                lines.join("\n")
            );
        }
    }
}

/// The command line a call of a tool would be performed with.
fn command_line(registry: &ToolRegistry, tool: &str, arguments: Value) -> Vec<String> {
    let arguments = arguments.as_object().cloned().expect("an object");
    let proposal = registry
        .propose(tool, arguments)
        .unwrap_or_else(|problems| panic!("{tool}: {problems:?}"));
    assert!(matches!(proposal.action, Action::Tool(_)), "{tool}");
    proposal
        .performer
        .command_line()
        .unwrap_or_else(|problem| panic!("{tool}: {problem}"))
}

/// What the built program prints for `--help` after these arguments.
fn help(arguments: &[&str]) -> String {
    let output = Command::new(PROGRAM)
        .args(arguments)
        .arg("--help")
        .output()
        .expect("the built program runs");
    assert!(
        output.status.success(),
        "{arguments:?} --help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Hold a command line against the built program.
fn check_against_the_program(tool: &str, line: &[String], subcommand: &str) {
    assert_eq!(line[0], INSTALLED, "{tool}");
    assert_eq!(line[1], subcommand, "{tool}: {line:?}");
    let program_help = help(&[]);
    assert!(
        program_help
            .lines()
            .any(|text| text.split_whitespace().next() == Some(subcommand)),
        "{tool}: the program lists no subcommand {subcommand}:\n{program_help}"
    );
    let subcommand_help = help(&[subcommand]);
    for argument in &line[2..] {
        let option = argument
            .strip_prefix("--")
            .unwrap_or_else(|| panic!("{tool}: {argument} is not an option"));
        let name = option.split('=').next().unwrap_or(option);
        let flag = format!("--{name}");
        assert!(
            subcommand_help.contains(&flag),
            "{tool}: {flag} is not an option of `{subcommand}`:\n{subcommand_help}"
        );
    }
}

/// Run the built program with the arguments of a command line, with the
/// environment cleared as in P@x's sandbox.
fn run(arguments: &[String]) -> (Option<i32>, String, String) {
    let output = Command::new(PROGRAM)
        .args(arguments)
        .env_clear()
        .output()
        .expect("the built program runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn cases() -> Vec<(&'static str, &'static str, Vec<&'static str>)> {
    vec![
        (
            "ai_local_status",
            "status",
            vec!["/usr/bin/tur-ai-local", "status", "--json"],
        ),
        (
            "ai_local_models",
            "models",
            vec![
                "/usr/bin/tur-ai-local",
                "models",
                "--json",
                "--catalogue-only",
            ],
        ),
        (
            "ai_local_recommend",
            "recommend",
            vec!["/usr/bin/tur-ai-local", "recommend", "--json"],
        ),
    ]
}

#[test]
fn the_manifest_and_the_adapters_load() {
    let registry = registry();
    let offered = registry.offered();
    let names: Vec<&str> = offered.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, OFFERED);
    for tool in &offered {
        assert_eq!(tool.service, SERVICE, "{}", tool.name);
    }
    assert!(registry.unavailable().is_empty());
    let file = AdapterFile::from_json(ADAPTERS).expect("a valid adapter file");
    assert_eq!(file.service(), SERVICE);
    assert_eq!(file.adapters().len(), 3);
    for adapter in file.adapters() {
        assert_eq!(adapter.program(), INSTALLED);
    }
}

#[test]
fn every_tool_only_observes() {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("the manifest is JSON");
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["service"], SERVICE);
    let tools = manifest["tools"].as_array().expect("tools");
    let declared: Vec<(&str, u64)> = tools
        .iter()
        .map(|tool| {
            (
                tool["name"].as_str().expect("a name"),
                tool["class"].as_u64().expect("a class"),
            )
        })
        .collect();
    assert_eq!(
        declared,
        [
            ("ai_local_status", 0),
            ("ai_local_models", 0),
            ("ai_local_recommend", 0)
        ]
    );
    for tool in registry().offered() {
        assert_eq!(tool.class, ActionClass::Observe, "{}", tool.name);
    }
    // No tool takes an argument, and none is named for something that
    // downloads, starts, stops or opens anything.
    let changing = [
        "pull", "download", "serve", "start", "stop", "run", "remove", "delete", "install",
        "network", "allow", "kill",
    ];
    for tool in tools {
        assert_eq!(tool["input_schema"]["properties"], json!({}), "{tool}");
        assert_eq!(
            tool["input_schema"]["additionalProperties"], false,
            "{tool}"
        );
        let name = tool["name"].as_str().expect("a name");
        assert!(name.starts_with("ai_local_"), "{name}");
        for field in ["name", "method"] {
            let text = tool[field].as_str().expect("text").to_lowercase();
            for word in changing {
                assert!(!text.contains(word), "tool {field} {text} offers {word}");
            }
        }
        let description = tool["description"].as_str().expect("a description");
        assert!(description.ends_with("Reads only."), "{name}");
    }
    // The adapters build fixed command lines: nothing from a model reaches
    // the program, and no adapter names a subcommand that acts.
    let adapters: Value = serde_json::from_str(ADAPTERS).expect("the adapters are JSON");
    for adapter in adapters["adapters"].as_array().expect("adapters") {
        let arguments = adapter["arguments"].as_array().expect("arguments");
        for argument in arguments {
            let keys: Vec<&str> = argument
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ["literal"], "{adapter}");
        }
        let subcommand = arguments[0]["literal"].as_str().expect("a subcommand");
        assert!(
            ["status", "models", "recommend"].contains(&subcommand),
            "{adapter}"
        );
        assert!(adapter.get("reads").is_none(), "{adapter}");
        assert!(adapter.get("feed").is_none(), "{adapter}");
    }
}

#[test]
fn every_command_line_is_one_the_program_really_has() {
    let registry = registry();
    let cases = cases();
    let covered: Vec<&str> = cases.iter().map(|case| case.0).collect();
    assert_eq!(covered, OFFERED);
    for (tool, subcommand, expected) in cases {
        let line = command_line(&registry, tool, json!({}));
        assert_eq!(line, expected, "{tool}");
        check_against_the_program(tool, &line, subcommand);
        let arguments = json!({ "model": "qwen3-0.6b" });
        let arguments = arguments.as_object().cloned().expect("an object");
        assert!(registry.propose(tool, arguments).is_err(), "{tool}");
    }
}

#[test]
fn the_tools_run_as_in_the_sandbox() {
    // The adapters read the installed catalogue and this machine. The same
    // command lines, given the repository's catalogue and a recorded
    // machine, show what the assistant is told. The environment is empty,
    // as in P@x's sandbox: no XDG_RUNTIME_DIR, no HOME.
    let registry = registry();
    let recorded = |line: Vec<String>| -> Vec<String> {
        let mut arguments: Vec<String> = line.into_iter().skip(1).collect();
        arguments.extend([
            "--catalogue".to_owned(),
            CATALOGUE.to_owned(),
            "--root".to_owned(),
            SMALL_VM.to_owned(),
            "--engine".to_owned(),
            "/nonexistent/llama-server".to_owned(),
        ]);
        arguments
    };

    let (status, stdout, stderr) = run(&recorded(command_line(
        &registry,
        "ai_local_status",
        json!({}),
    )));
    assert_eq!(status, Some(0), "{stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("the status is JSON");
    assert_eq!(report["kind"], "status");
    // In the sandbox the runtime folder is not visible, and the answer says
    // so instead of claiming that nothing runs.
    assert_eq!(report["running"], Value::Null);
    assert_eq!(report["code"], "TUR-AI-LOCAL-027");
    assert_eq!(report["engine"]["present"], false);

    let (status, stdout, stderr) = run(&recorded(command_line(
        &registry,
        "ai_local_models",
        json!({}),
    )));
    assert_eq!(status, Some(0), "{stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("the list is JSON");
    assert_eq!(report["kind"], "models");
    assert_eq!(report["models_dir"], Value::Null);
    let models = report["models"].as_array().expect("models");
    assert!(models.len() >= 3);
    for model in models {
        assert_eq!(model["installed"], Value::Null, "{model}");
        assert!(model["licence"]["summary"].as_str().is_some(), "{model}");
        assert!(model["fit"]["verdict"].as_str().is_some(), "{model}");
    }

    let (status, stdout, stderr) = run(&recorded(command_line(
        &registry,
        "ai_local_recommend",
        json!({}),
    )));
    assert_eq!(status, Some(0), "{stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("the recommendation is JSON");
    assert_eq!(report["kind"], "recommend");
    // The recorded machine has 4 GB of memory, less than 1 GB of it free.
    assert_eq!(report["recommendation"]["code"], "TUR-AI-LOCAL-007");
    assert_eq!(report["recommendation"]["model"], Value::Null);
}

#[test]
fn where_the_tool_set_is_and_is_not() {
    // The two files have the names tur-pax looks for, and the directory that
    // holds them is a tool set as tur-pax reads one.
    let component = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = read_tool_sets(component).expect("the component directory can be read");
    assert_eq!(files.components, ["pax"]);
    assert_eq!(files.manifests, [MANIFEST]);
    assert_eq!(files.adapters, [ADAPTERS]);
    assert_eq!(
        (MANIFEST_FILE, ADAPTER_FILE),
        ("agent-tools.json", "agent-tools.adapter.json")
    );
    // They are one directory below where the first components keep theirs,
    // like tur-update's: `tests/pax/e2e.py` and tur-pax's own tests list the
    // components whose tools sit directly in their directory, and adding one
    // is a change to the assistant's tests. Until that change P@x does not
    // load these tools by default; the local-ai workflow gives them to
    // tur-pax explicitly.
    assert!(!component.join(MANIFEST_FILE).exists());
    assert!(!component.join(ADAPTER_FILE).exists());
}
