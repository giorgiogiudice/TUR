// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The assistant tools this component ships: `agent-tools.json`, and beside
//! it the adapters that perform them with this component's own program,
//! `agent-tools.adapter.json`.
//!
//! The files are loaded with P@x's own loader (`tur-pax`, a development
//! dependency of this test only), the classes are those of the Android
//! runtime design (android-runtime.md, section 9.3), and every command line
//! an adapter builds is held against the program this component has just
//! built. The tools that need no Android are also run.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tur_pax::adapter::AdapterFile;
use tur_pax::registry::{ADAPTER_FILE, MANIFEST_FILE, ToolRegistry, read_tool_sets};
use tur_pax::{Action, ActionClass};

const MANIFEST: &str = include_str!("../agent-tools.json");
const ADAPTERS: &str = include_str!("../agent-tools.adapter.json");
const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-android");
const INSTALLED: &str = "/usr/bin/tur-android";
const SERVICE: &str = "org.tur.Android1";

/// Every tool the manifest declares, with the class of the design
/// (android-runtime.md, section 9.3).
const DECLARED: [(&str, ActionClass); 8] = [
    ("android_status", ActionClass::Observe),
    ("android_explain", ActionClass::Observe),
    ("android_list_apps", ActionClass::Observe),
    ("android_launch_app", ActionClass::Reversible),
    ("android_install_app", ActionClass::Consequential),
    ("android_remove_app", ActionClass::Consequential),
    ("android_switch_on", ActionClass::SystemCritical),
    ("android_switch_off", ActionClass::SystemCritical),
];

/// The tools P@x is offered, the subcommand that performs each and the
/// arguments of a sample call. P@x offers only tools that observe; the
/// others are declared, with their classes, and have no adapter until P@x
/// can perform an action above class 0 through the trusted path
/// (services/tur-pax/tests/tools.rs holds every component to that).
fn cases() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("android_status", "status", json!({})),
        ("android_explain", "explain", json!({})),
        ("android_list_apps", "list", json!({})),
    ]
}

fn registry() -> ToolRegistry {
    match ToolRegistry::load(&[MANIFEST.to_owned()], &[ADAPTERS.to_owned()], false) {
        Ok(registry) => registry,
        Err(problems) => {
            let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("the tools do not load:\n{}", lines.join("\n"));
        }
    }
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

#[test]
fn the_tools_load_with_the_classes_of_the_design() {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("the manifest is JSON");
    let declared: Vec<(String, u64)> = manifest["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| {
            (
                tool["name"].as_str().expect("a name").to_owned(),
                tool["class"].as_u64().expect("a class"),
            )
        })
        .collect();
    let expected: Vec<(String, u64)> = DECLARED
        .iter()
        .map(|(name, class)| ((*name).to_owned(), u64::from(u8::from(*class))))
        .collect();
    assert_eq!(declared, expected);

    let registry = registry();
    let offered = registry.offered();
    let names: Vec<&str> = offered.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, ["android_status", "android_explain", "android_list_apps"]);
    for tool in &offered {
        assert_eq!(tool.service, SERVICE, "{}", tool.name);
        assert_eq!(tool.class, ActionClass::Observe, "{}", tool.name);
    }
    let mut unavailable = registry.unavailable();
    unavailable.sort();
    let mut above: Vec<String> = DECLARED
        .iter()
        .filter(|(_, class)| *class != ActionClass::Observe)
        .map(|(name, _)| (*name).to_owned())
        .collect();
    above.sort();
    assert_eq!(unavailable, above);
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
    for (tool, subcommand, arguments) in cases() {
        let arguments = arguments.as_object().cloned().expect("an object");
        let proposal = registry
            .propose(tool, arguments.clone())
            .unwrap_or_else(|problems| panic!("{tool}: {problems:?}"));
        assert!(matches!(proposal.action, Action::Tool(_)), "{tool}");
        let line = proposal
            .performer
            .command_line()
            .unwrap_or_else(|problem| panic!("{tool}: {problem}"));
        assert_eq!(line[0], INSTALLED, "{tool}");
        assert_eq!(line[1], subcommand, "{tool}");
        assert!(
            program_help
                .lines()
                .any(|text| text.split_whitespace().next() == Some(subcommand)),
            "{tool}: no subcommand {subcommand}"
        );
        let subcommand_help = help(&[subcommand]);
        let mut rest = line[2..].iter();
        for argument in rest.by_ref() {
            if argument == "--" {
                break;
            }
            let name = argument.split('=').next().unwrap_or(argument);
            assert!(name.starts_with("--"), "{tool}: {argument}");
            assert!(
                subcommand_help.contains(name),
                "{tool}: {name} is not an option of {subcommand}"
            );
        }
        // The only thing after `--` is the tool's one input, as given.
        let after: Vec<&String> = rest.collect();
        let inputs: Vec<String> = arguments
            .values()
            .map(|value| value.as_str().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(
            after.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            inputs.iter().map(String::as_str).collect::<Vec<_>>(),
            "{tool}"
        );
        // An argument the tool does not declare is refused by the loader.
        let mut extra = arguments.clone();
        extra.insert("mirror".to_owned(), json!("http://example.org/"));
        assert!(registry.propose(tool, extra).is_err(), "{tool}");
    }
}

#[test]
fn no_adapter_can_name_another_image_source() {
    assert!(!ADAPTERS.contains("--mirror"));
    assert!(!ADAPTERS.contains("--pins"));
}

#[test]
fn the_tools_that_need_no_android_answer_here() {
    let pins = concat!(env!("CARGO_MANIFEST_DIR"), "/data/images.json");
    let output = Command::new(PROGRAM)
        .args(["explain", "--json", "--pins", pins])
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(report["explanation"]["download_bytes"], 1_037_268_032u64);

    let output = Command::new(PROGRAM)
        .args(["status", "--json"])
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(report["command"], "status");
    assert!(report["enabled"].is_boolean());

    // On a machine without Waydroid (this one), switching on is refused
    // before anything is done, with a code, and so is starting an
    // application.
    let output = Command::new(PROGRAM)
        .args(["enable", "--json", "--pins", pins])
        .output()
        .expect("runs");
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    if report["code"] == "TUR-ANDROID-RUNTIME-004" {
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(report["refused"], true);
    } else {
        // A machine with Waydroid: without --yes, consent is asked first.
        assert_eq!(report["code"], "TUR-ANDROID-CONSENT-001");
        assert!(report["explanation"]["paragraphs"].is_array());
    }
}

#[test]
fn where_the_tool_set_is() {
    let component = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(
        (MANIFEST_FILE, ADAPTER_FILE),
        ("agent-tools.json", "agent-tools.adapter.json")
    );
    let services = component.parent().expect("the services directory");
    let files = read_tool_sets(services).expect("the services directory can be read");
    let at = files
        .components
        .iter()
        .position(|name| name == "tur-android")
        .expect("tur-android is read as a tool set");
    assert_eq!(files.manifests[at], MANIFEST);
}
