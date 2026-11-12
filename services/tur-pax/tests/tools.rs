// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The tool set that ships: the manifests of the read-only tools that exist
//! today, and the adapters that perform them with the components' own
//! command-line programs.
//!
//! Every component ships its own two files: `agent-tools.json` and, beside
//! it, `agent-tools.adapter.json`. This crate holds none of them. Each
//! component also has a test of its own (`tests/agent_tools.rs`) that loads
//! its files with this crate's loader and holds every command line against
//! the program it has built. The tests here look at all of them together:
//! that the whole set loads as one registry, with no name used twice, and
//! that the source tree is read the way an installed directory of tool sets
//! is.
//!
//! That the programs really run in the sandbox and return what the adapters
//! expect is checked by the `pax` workflow.

use std::path::Path;

use serde_json::{Map, Value, json};
use tur_pax::registry::{ADAPTER_FILE, MANIFEST_FILE, Performer, ToolRegistry, read_tool_sets};
use tur_pax::sandbox::is_system_visible;
use tur_pax::{Action, ActionClass};

/// The components that ship tools, in name order.
const COMPONENTS: [&str; 5] = [
    "tur-catalogue",
    "tur-device-manager",
    "tur-diagnostics",
    "tur-runtime",
    "tur-storage",
];

const MANIFESTS: [&str; 5] = [
    include_str!("../../tur-catalogue/agent-tools.json"),
    include_str!("../../tur-device-manager/agent-tools.json"),
    include_str!("../../tur-diagnostics/agent-tools.json"),
    include_str!("../../tur-runtime/agent-tools.json"),
    include_str!("../../tur-storage/agent-tools.json"),
];

const ADAPTERS: [&str; 5] = [
    include_str!("../../tur-catalogue/agent-tools.adapter.json"),
    include_str!("../../tur-device-manager/agent-tools.adapter.json"),
    include_str!("../../tur-diagnostics/agent-tools.adapter.json"),
    include_str!("../../tur-runtime/agent-tools.adapter.json"),
    include_str!("../../tur-storage/agent-tools.adapter.json"),
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
fn the_source_tree_is_read_as_a_directory_of_tool_sets() {
    // `services/` has the shape of an installed directory of tool sets: one
    // subdirectory for each component, with its two files. The loader the
    // program uses reads it, and finds exactly the components that ship
    // tools. This crate's own directory holds neither file and is passed
    // over.
    let services = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let files = read_tool_sets(&services).expect("the services directory can be read");
    assert_eq!(files.components, COMPONENTS);
    assert_eq!(files.manifests, MANIFESTS);
    assert_eq!(files.adapters, ADAPTERS);
    assert_eq!(
        (MANIFEST_FILE, ADAPTER_FILE),
        ("agent-tools.json", "agent-tools.adapter.json")
    );
    for component in ["tur-pax", "tur-pax-providers", "tur-pax-memory"] {
        assert!(!services.join(component).join(MANIFEST_FILE).exists());
        assert!(!services.join(component).join(ADAPTER_FILE).exists());
    }
    // Nothing is left where the files used to be.
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR")).join("tools").exists());

    // What it read loads as the registry the other tests use.
    let loaded = ToolRegistry::load(&files.manifests, &files.adapters, true)
        .expect("the tool sets of the source tree load");
    assert_eq!(loaded, registry());
}

#[test]
fn a_directory_of_tool_sets_is_read_component_by_component() {
    let root = std::env::temp_dir().join(format!("tur-pax-tool-sets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let write = |relative: &str, text: &str| {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(path, text).expect("a file");
    };
    // Two components, the second without adapters yet; a directory that is
    // not a tool set; and files that are not in a component's directory.
    write("b-storage/agent-tools.json", MANIFESTS[4]);
    write("b-storage/agent-tools.adapter.json", ADAPTERS[4]);
    write("a-device/agent-tools.json", MANIFESTS[1]);
    write("c-other/README", "not a tool set");
    write("c-other/tools.json", "{}");
    write(
        "agent-tools.json",
        "{ not read: it is in no component's directory",
    );
    write("a-device/notes.json", "{ not read: it has another name");

    let files = read_tool_sets(&root).expect("the directory can be read");
    assert_eq!(files.components, ["a-device", "b-storage"]);
    assert_eq!(files.manifests, [MANIFESTS[1], MANIFESTS[4]]);
    assert_eq!(files.adapters, [ADAPTERS[4]]);
    let registry = ToolRegistry::load(&files.manifests, &files.adapters, false)
        .expect("the two tool sets load");
    // The component without adapters declares tools; none is offered.
    let offered: Vec<String> = registry
        .offered()
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(offered, ["storage_list_volumes", "storage_preview_mount"]);
    assert!(
        registry
            .unavailable()
            .contains(&"device_get_inventory".to_owned())
    );

    // Adapters without a manifest can only be a mistake.
    write("d-broken/agent-tools.adapter.json", ADAPTERS[0]);
    let error = read_tool_sets(&root).expect_err("adapters without a manifest");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("d-broken"), "{error}");

    // A directory that does not exist is an error, with its name.
    let missing = root.join("missing");
    let error = read_tool_sets(&missing).expect_err("no such directory");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(error.to_string().contains("missing"), "{error}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_shipped_tool_set_loads_and_every_offered_tool_only_observes() {
    let registry = registry();
    let offered = registry.offered();
    let names: Vec<&str> = offered.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "catalogue_search",
            "catalogue_show",
            "catalogue_route",
            "device_list",
            "device_get",
            "device_get_inventory",
            "device_list_problems",
            "diag_get_status",
            "runtime_inspect_file",
            "storage_list_volumes",
            "storage_preview_mount",
            "run_readonly_command"
        ]
    );
    for tool in &offered {
        assert_eq!(tool.class, ActionClass::Observe, "{}", tool.name);
    }
    // Declared by a component, with no program to perform them yet: not
    // offered. They are every shipped tool above class 0, and nothing else.
    assert_eq!(
        registry.unavailable(),
        ["storage_mount", "storage_unmount", "storage_unlock"]
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
            "device_list",
            json!({ "categories": ["usb", "network"] }),
            vec![
                "/usr/bin/tur-device",
                "list",
                "--json",
                "--category=usb",
                "--category=network",
            ],
        ),
        (
            "device_get",
            json!({ "id": "usb-1-4" }),
            vec!["/usr/bin/tur-device", "get", "--json", "--", "usb-1-4"],
        ),
        (
            "storage_list_volumes",
            json!({}),
            vec!["/usr/bin/tur-storage", "volumes", "--live", "--json"],
        ),
        (
            "storage_preview_mount",
            json!({ "volume_id": "fs-1", "access": "read-only" }),
            vec![
                "/usr/bin/tur-storage",
                "decide",
                "--live",
                "--json",
                "--access=read-only",
                "--volume=fs-1",
            ],
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
            vec![
                "/usr/bin/tur-runtime",
                "inspect",
                "--json",
                "--",
                "/usr/bin/true",
            ],
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
        (
            "runtime_inspect_file",
            json!({ "path": "--available=wine" }),
        ),
        ("runtime_inspect_file", json!({ "path": "/tmp/a\nb" })),
        ("device_get", json!({ "id": "--from=/etc" })),
        (
            "storage_preview_mount",
            json!({ "volume_id": "fs-1\n--live", "access": "read-only" }),
        ),
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
            assert!(
                is_system_visible(adapter.program()),
                "{}",
                adapter.program()
            );
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
                            assert!(
                                argument_fields.contains_key(field),
                                "argument field {field}"
                            );
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
