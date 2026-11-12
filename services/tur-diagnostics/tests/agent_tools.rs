// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The assistant tools this component ships: `agent-tools.json`, and beside
//! it the adapters that perform them with this component's own program,
//! `agent-tools.adapter.json`.
//!
//! Beside them are `agent-tools.next.json` and `agent-tools.next.adapter.json`:
//! the same files with the two read-only problem report tools added
//! (`diag_report_prepare`, preview only, and `diag_report_list`). They are
//! what the shipped files become once P@x's own test of the whole tool set
//! (`services/tur-pax/tests/tools.rs`) lists the two new names; until then
//! P@x does not load them. Every check below runs on both pairs.
//!
//! The files are loaded with P@x's own loader (`tur-pax`, a development
//! dependency of this test only), and every command line an adapter builds
//! is held against the program this component has just built: each
//! subcommand has to be one the program lists, and every option on the line
//! one its help describes.
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
const NEXT_MANIFEST: &str = include_str!("../agent-tools.next.json");
const NEXT_ADAPTERS: &str = include_str!("../agent-tools.next.adapter.json");

/// The program this component built.
const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-diag");

/// Where the adapters expect it to be installed.
const INSTALLED: &str = "/usr/bin/tur-diag";

/// The service the manifest declares.
const SERVICE: &str = "org.tur.Diagnostics1";

fn registry_of(manifest: &str, adapters: &str) -> ToolRegistry {
    match ToolRegistry::load(&[manifest.to_owned()], &[adapters.to_owned()], false) {
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

fn registry() -> ToolRegistry {
    registry_of(MANIFEST, ADAPTERS)
}

fn next_registry() -> ToolRegistry {
    registry_of(NEXT_MANIFEST, NEXT_ADAPTERS)
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
/// program; `subcommands` follow it in order, and each is a subcommand that
/// the help of the one before lists; and every option after them is one the
/// help of the last subcommand describes.
fn check_against_the_program(tool: &str, line: &[String], subcommands: &[&str]) {
    assert_eq!(line[0], INSTALLED, "{tool}");
    let named: Vec<&str> = line[1..=subcommands.len()]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(named, subcommands, "{tool}: {line:?}");
    for depth in 0..subcommands.len() {
        let parent_help = help(&subcommands[..depth]);
        let subcommand = subcommands[depth];
        assert!(
            parent_help
                .lines()
                .any(|text| text.split_whitespace().next() == Some(subcommand)),
            "{tool}: {:?} lists no subcommand {subcommand}:\n{parent_help}",
            &subcommands[..depth]
        );
    }
    let subcommand_help = help(subcommands);
    for argument in line[subcommands.len() + 1..]
        .iter()
        .take_while(|argument| *argument != "--")
    {
        let Some(option) = argument.strip_prefix("--") else {
            continue;
        };
        let name = option.split('=').next().unwrap_or(option);
        let flag = format!("--{name}");
        assert!(
            subcommand_help.contains(&flag),
            "{tool}: {flag} is not an option of `{}`:\n{subcommand_help}",
            subcommands.join(" ")
        );
    }
}

/// The tools P@x is offered today, in the order of the manifest.
const OFFERED: [&str; 1] = ["diag_get_status"];

/// The tools of the next manifest, in its order.
const NEXT_OFFERED: [&str; 3] = ["diag_get_status", "diag_report_prepare", "diag_report_list"];

/// The tools the manifest declares that no program performs yet.
const NOT_OFFERED: [&str; 0] = [];

/// Every tool with the class the manifest declares.
const CLASSES: [(&str, u64); 1] = [("diag_get_status", 0)];

/// Every tool of the next manifest with its class. Preparing a report is a
/// preview and listing reports reads a local list: both observe. Sending is
/// class 2 (ADR-0024) and is not a tool at all.
const NEXT_CLASSES: [(&str, u64); 3] = [
    ("diag_get_status", 0),
    ("diag_report_prepare", 0),
    ("diag_report_list", 0),
];

/// A call of a tool: its arguments, the subcommands that perform it, and
/// the whole command line.
type Case = (&'static str, Value, Vec<&'static str>, Vec<&'static str>);

fn cases() -> Vec<Case> {
    vec![(
        "diag_get_status",
        json!({}),
        vec!["status"],
        vec!["/usr/bin/tur-diag", "status", "--json"],
    )]
}

fn next_cases() -> Vec<Case> {
    let mut cases = cases();
    cases.push((
        "diag_report_prepare",
        json!({
            "title": "No sound after resume",
            "description": "The speakers stay silent after the laptop wakes up.",
            "component": "audio",
            "error_code": "TUR-AUDIO-RESUME-001",
            "from_boot_health": false
        }),
        vec!["report", "prepare"],
        vec![
            "/usr/bin/tur-diag",
            "report",
            "prepare",
            "--preview",
            "--json",
            "--prepared-by=pax",
            "--title=No sound after resume",
            "--description=The speakers stay silent after the laptop wakes up.",
            "--component=audio",
            "--error-code=TUR-AUDIO-RESUME-001",
        ],
    ));
    cases.push((
        "diag_report_prepare",
        json!({ "from_boot_health": true }),
        vec!["report", "prepare"],
        vec![
            "/usr/bin/tur-diag",
            "report",
            "prepare",
            "--preview",
            "--json",
            "--prepared-by=pax",
            "--from-boot-health",
        ],
    ));
    cases.push((
        "diag_report_list",
        json!({}),
        vec!["report", "list"],
        vec!["/usr/bin/tur-diag", "report", "list", "--json"],
    ));
    cases
}

fn declared_classes(manifest: &str) -> Vec<(String, u64)> {
    let manifest: Value = serde_json::from_str(manifest).expect("the manifest is JSON");
    manifest["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| {
            (
                tool["name"].as_str().expect("a name").to_owned(),
                tool["class"].as_u64().expect("a class"),
            )
        })
        .collect()
}

fn check_loads(registry: &ToolRegistry, adapters: &str, offered: &[&str]) {
    let tools = registry.offered();
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, offered);
    for tool in &tools {
        assert_eq!(tool.service, SERVICE, "{}", tool.name);
    }
    // Declared, with no program to perform them yet: loaded, not offered.
    assert_eq!(registry.unavailable(), NOT_OFFERED);

    // The adapters are for this component's service, and every one names
    // this component's program.
    let file = AdapterFile::from_json(adapters).expect("a valid adapter file");
    assert_eq!(file.service(), SERVICE);
    for adapter in file.adapters() {
        assert_eq!(adapter.program(), INSTALLED);
    }
}

#[test]
fn the_manifest_and_the_adapters_load() {
    check_loads(&registry(), ADAPTERS, &OFFERED);
    check_loads(&next_registry(), NEXT_ADAPTERS, &NEXT_OFFERED);
}

#[test]
fn every_tool_has_the_class_the_design_gives_it() {
    // Addendum A section 5: reading state is class 0. A tool is class 0
    // unless the design says otherwise, and the ones it says otherwise for
    // are named here.
    let expected: Vec<(String, u64)> = CLASSES
        .iter()
        .map(|(name, class)| ((*name).to_owned(), *class))
        .collect();
    assert_eq!(declared_classes(MANIFEST), expected);
    let expected: Vec<(String, u64)> = NEXT_CLASSES
        .iter()
        .map(|(name, class)| ((*name).to_owned(), *class))
        .collect();
    assert_eq!(declared_classes(NEXT_MANIFEST), expected);
    // Whatever is offered only observes.
    for tool in registry().offered().iter().chain(next_registry().offered().iter()) {
        assert_eq!(tool.class, ActionClass::Observe, "{}", tool.name);
    }
}

#[test]
fn the_next_manifest_only_adds_to_the_shipped_one() {
    let shipped: Value = serde_json::from_str(MANIFEST).expect("JSON");
    let next: Value = serde_json::from_str(NEXT_MANIFEST).expect("JSON");
    assert_eq!(shipped["service"], next["service"]);
    for tool in shipped["tools"].as_array().expect("tools") {
        assert!(
            next["tools"].as_array().expect("tools").contains(tool),
            "{} changed",
            tool["name"]
        );
    }
    let shipped: Value = serde_json::from_str(ADAPTERS).expect("JSON");
    let next: Value = serde_json::from_str(NEXT_ADAPTERS).expect("JSON");
    for adapter in shipped["adapters"].as_array().expect("adapters") {
        assert!(
            next["adapters"].as_array().expect("adapters").contains(adapter),
            "{} changed",
            adapter["tool"]
        );
    }
}

#[test]
fn no_tool_can_send_a_report() {
    // ADR-0024: sending is class 2 when P@x proposes it, and P@x performs
    // nothing above class 1, so no adapter may lead to `report send` or to
    // anything but a preview.
    for adapters in [ADAPTERS, NEXT_ADAPTERS] {
        let file: Value = serde_json::from_str(adapters).expect("JSON");
        let text = file.to_string();
        assert!(!text.contains("\"send\""), "an adapter names send");
        assert!(!text.contains("--yes"), "an adapter confirms");
        assert!(!text.contains("--output"), "an adapter saves a file");
    }
    let registry = next_registry();
    for (tool, arguments, _, _) in next_cases() {
        let line = command_line(&registry, tool, arguments);
        assert!(!line.iter().any(|argument| argument == "send"), "{tool}");
        if line.iter().any(|argument| argument == "prepare") {
            assert!(line.iter().any(|argument| argument == "--preview"), "{tool}");
        }
    }
    // A model cannot slip an option in through an input.
    let refused = registry.propose(
        "diag_report_prepare",
        json!({ "title": "x", "component": "audio", "error_code": "--yes" })
            .as_object()
            .cloned()
            .expect("an object"),
    );
    if let Ok(proposal) = refused {
        let line = proposal.performer.command_line().expect("a line");
        assert!(!line.iter().any(|argument| argument == "--yes"), "{line:?}");
    }
}

#[test]
fn every_command_line_is_one_the_program_really_has() {
    for (registry, cases, offered) in [
        (registry(), cases(), OFFERED.to_vec()),
        (next_registry(), next_cases(), NEXT_OFFERED.to_vec()),
    ] {
        // Every offered tool is covered.
        let covered: Vec<&str> = cases.iter().map(|case| case.0).collect();
        for tool in offered {
            assert!(covered.contains(&tool), "{tool} has no case");
        }
        for (tool, arguments, subcommands, expected) in cases {
            let line = command_line(&registry, tool, arguments);
            assert_eq!(line, expected, "{tool}");
            check_against_the_program(tool, &line, &subcommands);
        }
    }
}
