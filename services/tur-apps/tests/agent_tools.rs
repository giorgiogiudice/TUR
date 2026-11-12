// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The assistant tools this component ships: `agent-tools.json`, and beside
//! it the adapters that perform them with this component's own program,
//! `agent-tools.adapter.json`.
//!
//! The files are loaded with P@x's own loader (`tur-pax`, a development
//! dependency of this test only), and every command line an adapter builds
//! is held against the program this component has just built. The two
//! tools are run for real, against the catalogue in the repository and an
//! empty home. Both only read: installing is class 2 and is not a tool
//! (ai-control-layer.md).

use std::process::Command;

use serde_json::{Value, json};
use tur_pax::adapter::AdapterFile;
use tur_pax::registry::ToolRegistry;
use tur_pax::{Action, ActionClass};

const MANIFEST: &str = include_str!("../pax/agent-tools.json");
const ADAPTERS: &str = include_str!("../pax/agent-tools.adapter.json");
const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-apps");
const INSTALLED: &str = "/usr/bin/tur-apps";
const SERVICE: &str = "org.tur.Apps1";
const INSTALLED_CATALOGUE: &str = "/usr/share/tur/catalogue";
const REPOSITORY_CATALOGUE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalogue");

/// The tools P@x is offered, in the order of the manifest.
const OFFERED: [&str; 2] = ["apps_list_installed", "apps_plan"];

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

fn help(arguments: &[&str]) -> String {
    let output = Command::new(PROGRAM)
        .args(arguments)
        .arg("--help")
        .output()
        .expect("the built program runs");
    assert!(output.status.success(), "{arguments:?} --help failed");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn cases() -> Vec<(&'static str, Value, &'static str, Vec<&'static str>)> {
    vec![
        (
            "apps_list_installed",
            json!({}),
            "list",
            vec![INSTALLED, "list", "--json"],
        ),
        (
            "apps_plan",
            json!({ "id": "org.notepad-plus-plus.notepad-plus-plus" }),
            "plan",
            vec![
                INSTALLED,
                "--catalogue",
                INSTALLED_CATALOGUE,
                "plan",
                "--offline",
                "--json",
                "--",
                "org.notepad-plus-plus.notepad-plus-plus",
            ],
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
        // Only reading is offered: installing is class 2, not a tool.
        assert_eq!(tool.class, ActionClass::Observe, "{}", tool.name);
    }
    assert!(registry.unavailable().is_empty());
    let file = AdapterFile::from_json(ADAPTERS).expect("a valid adapter file");
    assert_eq!(file.service(), SERVICE);
    for adapter in file.adapters() {
        assert_eq!(adapter.program(), INSTALLED);
    }
}

#[test]
fn every_command_line_is_one_the_program_really_has() {
    let registry = registry();
    let program_help = help(&[]);
    for (tool, arguments, subcommand, expected) in cases() {
        let line = command_line(&registry, tool, arguments);
        assert_eq!(line, expected, "{tool}");
        assert!(
            program_help
                .lines()
                .any(|text| text.split_whitespace().next() == Some(subcommand)),
            "{tool}: no subcommand {subcommand}"
        );
        let subcommand_help = help(&[subcommand]);
        for argument in line[1..].iter().take_while(|argument| *argument != "--") {
            if let Some(option) = argument.strip_prefix("--") {
                let flag = format!("--{option}");
                assert!(
                    subcommand_help.contains(&flag) || program_help.contains(&flag),
                    "{tool}: {flag} is not an option of {subcommand}"
                );
            }
        }
        // Nothing an assistant tool runs may install, update or remove.
        for forbidden in ["install", "update", "remove", "launch", "enable", "disable"] {
            assert!(!line.iter().any(|argument| argument == forbidden), "{tool}");
        }
    }
}

#[test]
fn the_tools_run_and_return_one_json_document() {
    let registry = registry();
    let file = AdapterFile::from_json(ADAPTERS).expect("a valid adapter file");
    let home = std::env::temp_dir().join(format!("tur-apps-tools-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    for (tool, arguments, _, _) in cases() {
        let line: Vec<String> = command_line(&registry, tool, arguments)
            .into_iter()
            .skip(1)
            .map(|argument| {
                if argument == INSTALLED_CATALOGUE {
                    REPOSITORY_CATALOGUE.to_owned()
                } else {
                    argument
                }
            })
            .collect();
        let output = Command::new(PROGRAM)
            .args(&line)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .output()
            .expect("the built program runs");
        let status = output.status.code().expect("an exit status");
        let adapter = file
            .adapters()
            .iter()
            .find(|adapter| adapter.tool() == tool)
            .expect("an adapter");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            adapter.success_exit_codes().contains(&status),
            "{tool}: exit status {status}: {stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let document: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|error| panic!("{tool}: not JSON ({error}): {stdout}"));
        assert!(document.is_object(), "{tool}");
    }
    let _ = std::fs::remove_dir_all(&home);
}
