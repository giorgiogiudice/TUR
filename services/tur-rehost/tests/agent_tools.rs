// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The assistant tools this component ships: `agent-tools.json`, and the
//! adapters that perform them with this component's own program,
//! `agent-tools.adapter.json`.
//!
//! Loaded with P@x's own loader (`tur-pax`, a development dependency of
//! this test only); every command line an adapter builds is held against
//! the program just built. Every tool only reads: re-hosting an
//! application downloads and installs, which is class 2 and is not a tool.

use std::process::Command;

use serde_json::{Value, json};
use tur_pax::adapter::AdapterFile;
use tur_pax::registry::ToolRegistry;
use tur_pax::{Action, ActionClass};

const MANIFEST: &str = include_str!("../agent-tools.json");
const ADAPTERS: &str = include_str!("../agent-tools.adapter.json");
const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-rehost");
const INSTALLED: &str = "/usr/bin/tur-rehost";
const SERVICE: &str = "org.tur.Rehost1";

/// The tools P@x is offered, in the order of the manifest.
const OFFERED: [&str; 4] = ["rehost_detect", "rehost_plan", "rehost_list", "rehost_show"];

fn registry() -> ToolRegistry {
    match ToolRegistry::load(&[MANIFEST.to_owned()], &[ADAPTERS.to_owned()], false) {
        Ok(registry) => registry,
        Err(problems) => {
            let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("the tools of this component do not load:\n{}", lines.join("\n"));
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
            "rehost_detect",
            json!({ "path": "/home/person/Downloads/Example-Setup.exe" }),
            "detect",
            vec![INSTALLED, "detect", "--json", "--", "/home/person/Downloads/Example-Setup.exe"],
        ),
        (
            "rehost_plan",
            json!({ "path": "/home/person/Downloads/Example.dmg" }),
            "plan",
            vec![INSTALLED, "plan", "--json", "--", "/home/person/Downloads/Example.dmg"],
        ),
        ("rehost_list", json!({}), "list", vec![INSTALLED, "list", "--json"]),
        (
            "rehost_show",
            json!({ "app_id": "com.vscodium.vscodium" }),
            "show",
            vec![INSTALLED, "show", "--json", "--", "com.vscodium.vscodium"],
        ),
    ]
}

#[test]
fn the_manifest_and_the_adapters_load_and_only_read() {
    let registry = registry();
    let offered = registry.offered();
    let names: Vec<&str> = offered.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, OFFERED);
    for tool in &offered {
        assert_eq!(tool.service, SERVICE, "{}", tool.name);
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
        for forbidden in ["install", "uninstall", "launch", "--allow-builds"] {
            assert!(!line.iter().any(|argument| argument == forbidden), "{tool}");
        }
    }
}

#[test]
fn the_reading_tools_run_and_return_one_json_document() {
    let file = AdapterFile::from_json(ADAPTERS).expect("a valid adapter file");
    let registry = registry();
    let home = std::env::temp_dir().join(format!("tur-rehost-tools-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    for (tool, arguments, _, _) in cases() {
        if !matches!(tool, "rehost_list" | "rehost_show") {
            continue; // detect and plan need bubblewrap and a package
        }
        let line: Vec<String> = command_line(&registry, tool, arguments).into_iter().skip(1).collect();
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
        let document: Value =
            serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{tool}: not JSON ({error}): {stdout}"));
        assert!(document.is_object(), "{tool}");
    }
    let _ = std::fs::remove_dir_all(&home);
}
