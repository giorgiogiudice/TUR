// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The assistant tools this component ships: `pax/agent-tools.json`, and
//! beside it the adapters that perform them with this component's own
//! program, `pax/agent-tools.adapter.json`.
//!
//! The files are loaded with P@x's own loader (`tur-pax`, a development
//! dependency of this test only), and every command line an adapter builds
//! is held against the program this component has just built: the
//! subcommand has to be one the program lists, and every option on the line
//! one its help describes. The tools are also run for real, against recorded
//! machines.
//!
//! No tool is run by P@x here and no model is involved. That the installed
//! program runs in P@x's sandbox is not checked anywhere yet; see
//! `where_the_tool_set_is_and_is_not` for why.
//!
//! Both tools read and nothing else. Staging, going back and keeping a
//! version are class 3 (the AI design, section 5: OS update and rollback):
//! they are confirmed by a person on the trusted path, and this slice
//! declares no tool for them at all.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tur_pax::adapter::AdapterFile;
use tur_pax::registry::{ADAPTER_FILE, MANIFEST_FILE, ToolRegistry, read_tool_sets};
use tur_pax::{Action, ActionClass};

const MANIFEST: &str = include_str!("../pax/agent-tools.json");
const ADAPTERS: &str = include_str!("../pax/agent-tools.adapter.json");

/// The program this component built.
const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-update");

/// Where the adapters expect it to be installed.
const INSTALLED: &str = "/usr/bin/tur-update";

/// The service the manifest declares.
const SERVICE: &str = "org.tur.Update1";

/// The tools P@x is offered, in the order of the manifest.
const OFFERED: [&str; 2] = ["update_status", "update_preview_check"];

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

/// Hold a command line against the built program: it names the installed
/// program, `subcommand` is on it and is a subcommand the program lists, and
/// every option is one the help of the subcommand describes.
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

/// Run the built program with the arguments of a command line.
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

/// A call of each offered tool: the subcommand that performs it, and the
/// whole command line.
fn cases() -> Vec<(&'static str, &'static str, Vec<&'static str>)> {
    vec![
        (
            "update_status",
            "status",
            vec!["/usr/bin/tur-update", "status", "--live", "--json"],
        ),
        (
            "update_preview_check",
            "check",
            vec!["/usr/bin/tur-update", "check", "--live", "--json"],
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
    // Every declared tool has a program to perform it.
    assert!(registry.unavailable().is_empty());

    // The adapters are for this component's service, and every one names
    // this component's program.
    let file = AdapterFile::from_json(ADAPTERS).expect("a valid adapter file");
    assert_eq!(file.service(), SERVICE);
    assert_eq!(file.adapters().len(), 2);
    for adapter in file.adapters() {
        assert_eq!(adapter.program(), INSTALLED);
    }
}

#[test]
fn every_tool_only_observes() {
    // The AI design, section 5: reading state is class 0. OS update and
    // rollback are class 3 and are not tools of this manifest at all.
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
        [("update_status", 0), ("update_preview_check", 0)]
    );
    for tool in registry().offered() {
        assert_eq!(tool.class, ActionClass::Observe, "{}", tool.name);
    }
    // No tool takes an argument, and none is named for something that
    // changes the system.
    let changing = [
        "stage", "apply", "upgrade", "install", "rollback", "revert", "pin", "keep", "release",
        "switch", "reboot", "restart", "download",
    ];
    for tool in tools {
        assert_eq!(tool["input_schema"]["properties"], json!({}), "{tool}");
        assert_eq!(
            tool["input_schema"]["additionalProperties"], false,
            "{tool}"
        );
        let name = tool["name"].as_str().expect("a name");
        assert!(name.starts_with("update_"), "{name}");
        for field in ["name", "method"] {
            let text = tool[field].as_str().expect("text").to_lowercase();
            for word in changing {
                assert!(!text.contains(word), "tool {field} {text} offers {word}");
            }
        }
        let description = tool["description"].as_str().expect("a description");
        assert!(description.ends_with('.'), "{name}");
    }
    // The adapters build fixed command lines: nothing from a model reaches
    // the program.
    let adapters: Value = serde_json::from_str(ADAPTERS).expect("the adapters are JSON");
    for adapter in adapters["adapters"].as_array().expect("adapters") {
        for argument in adapter["arguments"].as_array().expect("arguments") {
            let keys: Vec<&str> = argument
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ["literal"], "{adapter}");
        }
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
        // An argument the tool does not declare is refused by the loader,
        // not passed on.
        let arguments = json!({ "deployment": "rollback" });
        let arguments = arguments.as_object().cloned().expect("an object");
        assert!(registry.propose(tool, arguments).is_err(), "{tool}");
    }
}

#[test]
fn the_tools_run_against_recorded_machines() {
    // The adapters ask about this machine (`--live`). The same command
    // lines, reading a recorded `bootc status` instead, show that they are
    // ones the program accepts and answers in JSON. The live path itself is
    // exercised with a stand-in for `bootc` in the unit tests of the
    // program, and with the real `bootc` of the base image by the `update`
    // workflow.
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let recorded = |line: Vec<String>, machine: &str| -> Vec<String> {
        line.into_iter()
            .skip(1)
            .flat_map(|argument| {
                if argument == "--live" {
                    vec!["--from".to_owned(), format!("{fixtures}/{machine}")]
                } else {
                    vec![argument]
                }
            })
            .collect()
    };
    let registry = registry();

    let line = command_line(&registry, "update_status", json!({}));
    let (status, stdout, stderr) = run(&recorded(line.clone(), "bootc/tur-after-update.json"));
    assert_eq!(status, Some(0), "{stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("the status is JSON");
    assert_eq!(report["kind"], "status");
    assert_eq!(report["versions"].as_array().map(Vec::len), Some(2));
    assert_eq!(report["versions"][1]["slot"], "rollback");

    let line_check = command_line(&registry, "update_preview_check", json!({}));
    let (status, stdout, stderr) = run(&recorded(
        line_check.clone(),
        "bootc-upstream/booted-with-update.json",
    ));
    assert_eq!(status, Some(0), "{stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("the preview is JSON");
    assert_eq!(report["kind"], "check");
    assert_eq!(report["update"]["status"], "available");
    // What the assistant is told when nothing is known about boot health,
    // which is the case for it today: a check may run, an update is not
    // staged, and why.
    assert_eq!(report["check"]["outcome"], "proceed");
    assert_eq!(report["automatic_action"], "refuse");
    assert_eq!(report["automatic_stage"]["code"], "TUR-UPDATE-HEALTH-004");

    // A machine that is not a bootc host is the program's own failure, with
    // its code, and nothing on standard output.
    for line in [line, line_check] {
        let (status, stdout, stderr) = run(&recorded(line, "bootc/container-base-image.json"));
        assert_eq!(status, Some(1), "{stderr}");
        assert!(stdout.is_empty(), "{stdout}");
        assert!(stderr.starts_with("TUR-UPDATE-STATE-002"), "{stderr}");
    }
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
    // They are one directory below where the other components keep theirs,
    // on purpose. `services/` is itself read as a directory of tool sets, by
    // tur-pax's own tests (`services/tur-pax/tests/tools.rs`) and by the
    // `pax` workflow (`tests/pax/e2e.py`), and both list the components that
    // ship tools. Putting the files in this component's directory adds a
    // sixth to those lists and needs `/usr/bin/tur-update` built and
    // installed in that workflow: a change to the assistant's own tests,
    // made together with the move (docs/architecture/update-manager.md,
    // section 9). Until then P@x does not load these tools.
    assert!(!component.join(MANIFEST_FILE).exists());
    assert!(!component.join(ADAPTER_FILE).exists());
}
