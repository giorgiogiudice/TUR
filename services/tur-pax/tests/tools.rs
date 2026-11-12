// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The tool set that ships: the manifests of the read-only tools that exist
//! today, and the adapters that perform them with the components' own
//! command-line programs.
//!
//! The manifests of the device manager and of storage are the ones those
//! components ship (`agent-tools.json`). Diagnostics, the catalogue and the
//! runtime manager do not ship one yet; theirs are kept here, in
//! `tools/manifests`, until they do.
//!
//! These tests load the files and check the command lines. That the programs
//! really run in the sandbox and return what the adapters expect is checked
//! by the `pax` workflow.

use serde_json::{Map, Value, json};
use tur_pax::registry::{Performer, ToolRegistry};
use tur_pax::sandbox::is_system_visible;
use tur_pax::{Action, ActionClass};

const MANIFESTS: [&str; 5] = [
    include_str!("../../tur-device-manager/agent-tools.json"),
    include_str!("../../tur-storage/agent-tools.json"),
    include_str!("../tools/manifests/catalogue.json"),
    include_str!("../tools/manifests/diagnostics.json"),
    include_str!("../tools/manifests/runtime.json"),
];

const ADAPTERS: [&str; 5] = [
    include_str!("../tools/adapters/catalogue.json"),
    include_str!("../tools/adapters/device.json"),
    include_str!("../tools/adapters/diagnostics.json"),
    include_str!("../tools/adapters/runtime.json"),
    include_str!("../tools/adapters/storage.json"),
];

/// The adapter file format, as published.
const ADAPTER_SCHEMA: &str = include_str!("../../../schemas/pax-tool-adapter.schema.json");

fn registry() -> ToolRegistry {
    let manifests: Vec<String> = MANIFESTS.iter().map(|text| (*text).to_owned()).collect();
    let adapters: Vec<String> = ADAPTERS.iter().map(|text| (*text).to_owned()).collect();
    match ToolRegistry::load(&manifests, &adapters, true) {
        Ok(registry) => registry,
        Err(problems) => {
            let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("the shipped tool set does not load:\n{}", lines.join("\n"));
        }
    }
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

/// The command line a tool call is performed with.
fn command_line(registry: &ToolRegistry, tool: &str, arguments: Value) -> Vec<String> {
    let proposal = registry
        .propose(tool, object(arguments))
        .unwrap_or_else(|problems| panic!("{tool}: {problems:?}"));
    assert!(matches!(proposal.action, Action::Tool(_)), "{tool}");
    match proposal.performer {
        Performer::Adapter { adapter, arguments } => {
            adapter.argv(&arguments).expect("a command line")
        }
        Performer::Command { .. } => panic!("{tool} is not a tool of a manifest"),
    }
}

#[test]
fn the_shipped_tool_set_loads_and_every_offered_tool_only_observes() {
    let registry = registry();
    let offered = registry.offered();
    let names: Vec<&str> = offered.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "device_get_inventory",
            "device_list_problems",
            "storage_list_volumes",
            "catalogue_search",
            "catalogue_show",
            "catalogue_route",
            "diag_get_status",
            "runtime_inspect_file",
            "run_readonly_command"
        ]
    );
    for tool in &offered {
        assert_eq!(tool.class, ActionClass::Observe, "{}", tool.name);
    }
    // Declared by a component, with no program to perform them yet: not
    // offered. Among them every tool above class 0.
    assert_eq!(
        registry.unavailable(),
        [
            "device_list",
            "device_get",
            "storage_preview_mount",
            "storage_mount",
            "storage_unmount",
            "storage_unlock"
        ]
    );
    let definitions = registry.definitions().expect("definitions");
    assert_eq!(definitions.len(), offered.len());
}

#[test]
fn every_shipped_adapter_builds_the_command_line_of_its_program() {
    let registry = registry();
    let cases = [
        (
            "diag_get_status",
            json!({}),
            vec!["/usr/bin/tur-diag", "status", "--json"],
        ),
        (
            "device_get_inventory",
            json!({}),
            vec!["/usr/bin/tur-device", "inventory", "--json", "--redact"],
        ),
        (
            "device_list_problems",
            json!({}),
            vec!["/usr/bin/tur-device", "problems", "--json"],
        ),
        (
            "storage_list_volumes",
            json!({}),
            vec!["/usr/bin/tur-storage", "volumes", "--from", "/dev/stdin", "--json"],
        ),
        (
            "catalogue_search",
            json!({ "query": "video editor" }),
            vec![
                "/usr/bin/tur-catalogue",
                "--catalogue",
                "/usr/share/tur/catalogue",
                "search",
                "--json",
                "--",
                "video editor",
            ],
        ),
        (
            "catalogue_show",
            json!({ "id": "com.discord.discord" }),
            vec![
                "/usr/bin/tur-catalogue",
                "--catalogue",
                "/usr/share/tur/catalogue",
                "show",
                "--json",
                "--",
                "com.discord.discord",
            ],
        ),
        (
            "catalogue_route",
            json!({ "id": "com.discord.discord" }),
            vec![
                "/usr/bin/tur-catalogue",
                "--catalogue",
                "/usr/share/tur/catalogue",
                "route",
                "--json",
                "--",
                "com.discord.discord",
            ],
        ),
        (
            "runtime_inspect_file",
            json!({ "path": "/usr/bin/true" }),
            vec!["/usr/bin/tur-runtime", "inspect", "--json", "--", "/usr/bin/true"],
        ),
    ];
    for (tool, arguments, expected) in cases {
        assert_eq!(command_line(&registry, tool, arguments), expected, "{tool}");
    }
}

#[test]
fn a_value_from_the_model_cannot_become_an_option_of_a_shipped_program() {
    let registry = registry();
    for (tool, arguments) in [
        ("catalogue_search", json!({ "query": "--catalogue=/etc" })),
        ("catalogue_show", json!({ "id": "-h" })),
        ("catalogue_route", json!({ "id": "--host=/etc/shadow" })),
        ("runtime_inspect_file", json!({ "path": "--available=wine" })),
        ("runtime_inspect_file", json!({ "path": "/tmp/a\nb" })),
    ] {
        let proposal = registry
            .propose(tool, object(arguments.clone()))
            .expect("the arguments fit the schema");
        let Performer::Adapter { adapter, arguments } = proposal.performer else {
            panic!("{tool} is not a tool of a manifest");
        };
        let refused = adapter.argv(&arguments).expect_err("the value is refused");
        assert_eq!(refused.code.as_str(), "TUR-AI-TOOL-002", "{tool}");
    }
}

#[test]
fn every_shipped_program_and_path_is_in_the_system_view_of_the_sandbox() {
    for text in ADAPTERS {
        let file = tur_pax::adapter::AdapterFile::from_json(text).expect("a valid adapter file");
        for adapter in file.adapters() {
            assert!(is_system_visible(adapter.program()), "{}", adapter.program());
            for path in adapter.reads() {
                assert!(is_system_visible(path), "{path}");
            }
            if let Some(feed) = adapter.feed() {
                assert!(is_system_visible(&feed.program), "{}", feed.program);
            }
            assert!(!adapter.success_exit_codes().is_empty());
        }
    }
}

#[test]
fn the_shipped_adapters_use_only_what_the_published_format_describes() {
    // The schema is a description for people and for other tools; the loader
    // decides. This holds the two together on the shipped files only: every
    // field a shipped adapter uses is a field the schema declares.
    let schema: Value = serde_json::from_str(ADAPTER_SCHEMA).expect("the schema is JSON");
    assert_eq!(schema["properties"]["schema_version"]["const"], 1);
    let adapter_fields = schema["$defs"]["adapter"]["properties"]
        .as_object()
        .expect("the adapter definition");
    let argument_fields = schema["$defs"]["argument"]["properties"]
        .as_object()
        .expect("the argument definition");
    let feed_fields = schema["$defs"]["feed"]["properties"]
        .as_object()
        .expect("the feed definition");
    for text in ADAPTERS {
        let file: Value = serde_json::from_str(text).expect("JSON");
        for name in file.as_object().expect("an object").keys() {
            assert!(schema["properties"].get(name).is_some(), "{name}");
        }
        for adapter in file["adapters"].as_array().expect("adapters") {
            for (name, value) in adapter.as_object().expect("an object") {
                assert!(adapter_fields.contains_key(name), "adapter field {name}");
                if name == "arguments" {
                    for argument in value.as_array().expect("arguments") {
                        for field in argument.as_object().expect("an object").keys() {
                            assert!(argument_fields.contains_key(field), "argument field {field}");
                        }
                    }
                }
                if name == "feed" {
                    for field in value.as_object().expect("an object").keys() {
                        assert!(feed_fields.contains_key(field), "feed field {field}");
                    }
                }
            }
            assert_eq!(adapter["kind"], "command");
        }
    }
}
