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
const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-storage");

/// Where the adapters expect it to be installed.
const INSTALLED: &str = "/usr/bin/tur-storage";

/// The service the manifest declares.
const SERVICE: &str = "org.tur.Storage1";

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
const OFFERED: [&str; 2] = ["storage_list_volumes", "storage_preview_mount"];

/// The tools the manifest declares that no program performs yet: every tool
/// of this component above class 0. They need the service that mounts, which
/// does not exist; this program never mounts, unlocks or writes.
const NOT_OFFERED: [&str; 3] = ["storage_mount", "storage_unmount", "storage_unlock"];

/// Every tool with the class the manifest declares. Listing and previewing
/// read state: class 0. Mounting, unmounting and unlocking change session
/// state in a way that can be undone: class 1 (addendum A section 5, and the
/// manifest example of section 11, where `storage_unmount` is class 1).
/// Formatting, partitioning and encrypting are class 3 and are not tools of
/// this manifest at all.
const CLASSES: [(&str, u64); 5] = [
    ("storage_list_volumes", 0),
    ("storage_preview_mount", 0),
    ("storage_mount", 1),
    ("storage_unmount", 1),
    ("storage_unlock", 1),
];

/// A call of each offered tool: its arguments, the subcommand that performs
/// it, and the whole command line.
fn cases() -> Vec<(&'static str, Value, &'static str, Vec<&'static str>)> {
    vec![
        (
            "storage_list_volumes",
            json!({}),
            "volumes",
            vec!["/usr/bin/tur-storage", "volumes", "--live", "--json"],
        ),
        (
            "storage_preview_mount",
            json!({ "volume_id": "fs-01d9a7c2e4f6b801", "access": "read-only" }),
            "decide",
            vec![
                "/usr/bin/tur-storage",
                "decide",
                "--live",
                "--json",
                "--access=read-only",
                "--volume=fs-01d9a7c2e4f6b801",
            ],
        ),
        (
            "storage_preview_mount",
            json!({ "volume_id": "fs-01d9a7c2e4f6b801", "access": "read-write" }),
            "decide",
            vec![
                "/usr/bin/tur-storage",
                "decide",
                "--live",
                "--json",
                "--access=read-write",
                "--volume=fs-01d9a7c2e4f6b801",
            ],
        ),
    ]
}

#[test]
fn access_is_spelled_the_same_in_the_manifest_and_on_the_command_line() {
    // Every value the manifest allows for `access` is a value the program's
    // help lists, and the program reports it back in the same spelling.
    let manifest: Value = serde_json::from_str(MANIFEST).expect("the manifest is JSON");
    let text = help(&["decide"]);
    let mut seen = 0;
    for tool in manifest["tools"].as_array().expect("tools") {
        let access = &tool["input_schema"]["properties"]["access"];
        if access.is_null() {
            continue;
        }
        seen += 1;
        assert_eq!(
            access["enum"],
            json!(["read-only", "read-write"]),
            "{}",
            tool["name"]
        );
        for value in access["enum"].as_array().expect("values") {
            let value = value.as_str().expect("text");
            assert!(text.contains(value), "{value} is not in the help:\n{text}");
        }
    }
    assert_eq!(seen, 2);
}

#[test]
fn the_tools_run_against_a_recorded_machine() {
    // The adapters ask about this machine (`--live`). The same command
    // lines, reading a recorded `lsblk` instead, show that they are ones
    // the program accepts and answers in JSON. The live path itself is
    // exercised with a stand-in for `lsblk` in the unit tests of the
    // program.
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dual-boot.json");
    let recorded = |line: Vec<String>| -> Vec<String> {
        line.into_iter()
            .skip(1)
            .flat_map(|argument| {
                if argument == "--live" {
                    vec!["--from".to_owned(), fixture.to_owned()]
                } else {
                    vec![argument]
                }
            })
            .collect()
    };
    let registry = registry();

    let line = command_line(&registry, "storage_list_volumes", json!({}));
    let (status, stdout, stderr) = run(&recorded(line));
    assert_eq!(status, Some(0), "{stderr}");
    let inventory: Value = serde_json::from_str(&stdout).expect("the inventory is JSON");
    assert_eq!(inventory["kind"], "inventory");
    let volumes = inventory["volumes"].as_array().expect("volumes");
    assert!(!volumes.is_empty());

    // Every volume it listed can be previewed, with either access, by the
    // identifier it printed.
    for volume in volumes {
        let id = volume["id"].as_str().expect("an identifier");
        for access in ["read-only", "read-write"] {
            let line = command_line(
                &registry,
                "storage_preview_mount",
                json!({ "volume_id": id, "access": access }),
            );
            let (status, stdout, stderr) = run(&recorded(line));
            assert_eq!(status, Some(0), "{id} {access}: {stderr}");
            let decision: Value = serde_json::from_str(&stdout).expect("the decision is JSON");
            assert_eq!(decision["kind"], "decision", "{id} {access}");
            assert_eq!(decision["volume"]["id"], id, "{id} {access}");
            assert_eq!(
                decision["volume"]["decision"]["requested_access"], access,
                "{id} {access}"
            );
        }
    }

    // A volume the machine does not have is the program's own failure, with
    // its code, and nothing on standard output.
    let line = command_line(
        &registry,
        "storage_preview_mount",
        json!({ "volume_id": "fs-nowhere", "access": "read-only" }),
    );
    let (status, stdout, stderr) = run(&recorded(line));
    assert_eq!(status, Some(1), "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.starts_with("TUR-STORAGE-VOLUME-001"), "{stderr}");
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
