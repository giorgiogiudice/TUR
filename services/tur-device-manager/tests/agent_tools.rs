// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The assistant tools this component ships: `agent-tools.json`, and beside
//! it the adapters that perform them with this component's own program,
//! `agent-tools.adapter.json`.
//!
//! The files are loaded with P@x's own loader (`tur-pax`, a development
//! dependency of this test only), and every command line an adapter builds
//! is held against the program this component has just built: the
//! subcommand has to be one the program lists, and every option on the line
//! one its help describes. The tools are also run for real, against a
//! recorded machine.
//!
//! No tool is run by P@x here and no model is involved. That the installed
//! program runs in P@x's sandbox is checked by the `pax` workflow.

use std::process::Command;

use serde_json::{Value, json};
use tur_pax::adapter::AdapterFile;
use tur_pax::registry::ToolRegistry;
use tur_pax::{Action, ActionClass};

const MANIFEST: &str = include_str!("../agent-tools.json");
const ADAPTERS: &str = include_str!("../agent-tools.adapter.json");

/// The program this component built.
const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-device");

/// Where the adapters expect it to be installed.
const INSTALLED: &str = "/usr/bin/tur-device";

/// The service the manifest declares.
const SERVICE: &str = "org.tur.Device1";

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
/// every option before a `--` is one the help of the subcommand or of the
/// program describes.
fn check_against_the_program(tool: &str, line: &[String], subcommand: &str) {
    assert_eq!(line[0], INSTALLED, "{tool}");
    assert!(
        line[1..].iter().any(|argument| argument == subcommand),
        "{tool}: {line:?} does not name {subcommand}"
    );
    let program_help = help(&[]);
    assert!(
        program_help
            .lines()
            .any(|text| text.split_whitespace().next() == Some(subcommand)),
        "{tool}: the program lists no subcommand {subcommand}:\n{program_help}"
    );
    let subcommand_help = help(&[subcommand]);
    for argument in line[1..].iter().take_while(|argument| *argument != "--") {
        let Some(option) = argument.strip_prefix("--") else {
            continue;
        };
        let name = option.split('=').next().unwrap_or(option);
        let flag = format!("--{name}");
        assert!(
            subcommand_help.contains(&flag) || program_help.contains(&flag),
            "{tool}: {flag} is not an option of `{subcommand}`:\n{subcommand_help}"
        );
    }
}

/// Run the built program with the arguments of a command line.
fn run(arguments: &[String]) -> (Option<i32>, String, String) {
    let output = Command::new(PROGRAM)
        .args(arguments)
        .output()
        .expect("the built program runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The tools P@x is offered, in the order of the manifest.
const OFFERED: [&str; 4] = [
    "device_list",
    "device_get",
    "device_get_inventory",
    "device_list_problems",
];

/// The tools the manifest declares that no program performs yet.
const NOT_OFFERED: [&str; 0] = [];

/// Every tool with the class the manifest declares. All of them read.
const CLASSES: [(&str, u64); 4] = [
    ("device_list", 0),
    ("device_get", 0),
    ("device_get_inventory", 0),
    ("device_list_problems", 0),
];

/// A call of each offered tool: its arguments, the subcommand that performs
/// it, and the whole command line.
fn cases() -> Vec<(&'static str, Value, &'static str, Vec<&'static str>)> {
    vec![
        (
            "device_list",
            json!({}),
            "list",
            vec!["/usr/bin/tur-device", "list", "--json"],
        ),
        (
            "device_list",
            json!({ "categories": ["usb", "storage_controller"] }),
            "list",
            vec![
                "/usr/bin/tur-device",
                "list",
                "--json",
                "--category=usb",
                "--category=storage_controller",
            ],
        ),
        (
            "device_get",
            json!({ "id": "pci-0000:00:1f.3" }),
            "get",
            vec![
                "/usr/bin/tur-device",
                "get",
                "--json",
                "--",
                "pci-0000:00:1f.3",
            ],
        ),
        (
            "device_get_inventory",
            json!({}),
            "inventory",
            vec!["/usr/bin/tur-device", "inventory", "--json", "--redact"],
        ),
        (
            "device_list_problems",
            json!({}),
            "problems",
            vec!["/usr/bin/tur-device", "problems", "--json"],
        ),
    ]
}

#[test]
fn every_category_the_manifest_allows_is_one_the_program_takes() {
    // The manifest and the command line have one spelling for a category.
    let manifest: Value = serde_json::from_str(MANIFEST).expect("the manifest is JSON");
    let allowed = manifest["tools"][0]["input_schema"]["properties"]["categories"]["items"]["enum"]
        .as_array()
        .expect("the categories of device_list")
        .clone();
    assert_eq!(manifest["tools"][0]["name"], "device_list");
    assert_eq!(allowed.len(), 10);
    let text = help(&["list"]);
    let registry = registry();
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/laptop");
    for category in &allowed {
        let name = category.as_str().expect("text");
        assert!(text.contains(name), "{name} is not in the help:\n{text}");
        // And the program accepts it, from the adapter's own command line.
        let line = command_line(&registry, "device_list", json!({ "categories": [name] }));
        assert_eq!(line[3], format!("--category={name}"));
        let mut arguments: Vec<String> = line[1..].to_vec();
        arguments.extend(["--from".to_owned(), fixture.to_owned()]);
        let (status, stdout, stderr) = run(&arguments);
        assert_eq!(status, Some(0), "{name}: {stderr}");
        let listed: Value = serde_json::from_str(&stdout).expect("JSON");
        for device in listed["devices"].as_array().expect("devices") {
            assert_eq!(device["category"], *category, "{name}");
        }
    }
    // A value the manifest does not allow never reaches the program.
    assert!(
        registry
            .propose(
                "device_list",
                json!({ "categories": ["--from=/etc"] })
                    .as_object()
                    .cloned()
                    .expect("an object"),
            )
            .is_err()
    );
}

#[test]
fn the_tools_run_against_a_recorded_machine() {
    // The adapters read this machine. The same command lines, reading a
    // recorded laptop instead (`--from`), show that they are ones the
    // program accepts and answers in JSON.
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/laptop");
    // `--from` goes right after the subcommand, before any `--`.
    let recorded = |line: Vec<String>| -> Vec<String> {
        let mut arguments = vec![line[1].clone(), "--from".to_owned(), fixture.to_owned()];
        arguments.extend(line.into_iter().skip(2));
        arguments
    };
    let registry = registry();

    let line = command_line(&registry, "device_list", json!({}));
    let (status, stdout, stderr) = run(&recorded(line));
    assert_eq!(status, Some(0), "{stderr}");
    let listed: Value = serde_json::from_str(&stdout).expect("the listing is JSON");
    let devices = listed["devices"].as_array().expect("devices");
    assert!(devices.len() >= 26, "{}", devices.len());

    // Every device it listed can be shown by the identifier it printed.
    for device in devices {
        let id = device["id"].as_str().expect("an identifier");
        let line = command_line(&registry, "device_get", json!({ "id": id }));
        let (status, stdout, stderr) = run(&recorded(line));
        assert_eq!(status, Some(0), "{id}: {stderr}");
        let shown: Value = serde_json::from_str(&stdout).expect("the device is JSON");
        assert_eq!(shown["device"]["id"], id);
        assert_eq!(shown["redacted"], true, "{id}");
    }

    // The other two tools, the same way.
    for tool in ["device_get_inventory", "device_list_problems"] {
        let line = command_line(&registry, tool, json!({}));
        let (status, stdout, stderr) = run(&recorded(line));
        assert_eq!(status, Some(0), "{tool}: {stderr}");
        let document: Value = serde_json::from_str(&stdout).expect("JSON");
        assert!(document.is_object(), "{tool}");
    }

    // An identifier no device has is the program's own failure, with its
    // code and its own exit status.
    let line = command_line(&registry, "device_get", json!({ "id": "pci-nowhere" }));
    let (status, stdout, stderr) = run(&recorded(line));
    assert_eq!(status, Some(3), "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.starts_with("error: TUR-DEVICE-INPUT-002"), "{stderr}");
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
    // Declared, with no program to perform them yet: loaded, not offered.
    assert_eq!(registry.unavailable(), NOT_OFFERED);

    // The adapters are for this component's service, and every one names
    // this component's program.
    let file = AdapterFile::from_json(ADAPTERS).expect("a valid adapter file");
    assert_eq!(file.service(), SERVICE);
    for adapter in file.adapters() {
        assert_eq!(adapter.program(), INSTALLED);
    }
}

#[test]
fn every_tool_has_the_class_the_design_gives_it() {
    // Addendum A section 5: reading state is class 0. A tool is class 0
    // unless the design says otherwise, and the ones it says otherwise for
    // are named here.
    let manifest: Value = serde_json::from_str(MANIFEST).expect("the manifest is JSON");
    let declared: Vec<(&str, u64)> = manifest["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| {
            (
                tool["name"].as_str().expect("a name"),
                tool["class"].as_u64().expect("a class"),
            )
        })
        .collect();
    assert_eq!(declared, CLASSES);
    // Whatever is offered today only observes.
    for tool in registry().offered() {
        assert_eq!(tool.class, ActionClass::Observe, "{}", tool.name);
    }
}

#[test]
fn every_command_line_is_one_the_program_really_has() {
    let registry = registry();
    let cases = cases();
    // Every offered tool is covered.
    let covered: Vec<&str> = cases.iter().map(|case| case.0).collect();
    for tool in OFFERED {
        assert!(covered.contains(&tool), "{tool} has no case");
    }
    for (tool, arguments, subcommand, expected) in cases {
        let line = command_line(&registry, tool, arguments);
        assert_eq!(line, expected, "{tool}");
        check_against_the_program(tool, &line, subcommand);
    }
}
