// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The tool set that ships: every component's manifest of the tools P@x may
//! offer, and the adapters that perform them with the component's own
//! command-line program.
//!
//! Every component ships its own two files, and there is one rule for where
//! they are: `services/<component>/agent-tools.json` and, beside it,
//! `agent-tools.adapter.json`. This crate holds none of them and lists none
//! of them: the tests here **discover** the tool sets in the source tree, the
//! way `tur-pax` reads an installed directory of tool sets, load them with
//! the program's own loader, and hold invariants over the whole discovered
//! set. A component that adds a tool, or a component that ships tools for
//! the first time, needs no change here.
//!
//! What is checked of every discovered tool set:
//!
//! * the whole set loads as one registry, with no service or name used twice;
//! * every tool declares a class; every tool that is offered (it has an
//!   adapter) only observes, class 0, and every tool above class 0 has no
//!   adapter and so is not offered;
//! * no adapter puts on a command line a word or an option that sends,
//!   installs, removes, stages, applies or otherwise changes something,
//!   unless its tool's class is 2 or more;
//! * every program and path an adapter names is in the sandbox's view of the
//!   system, and every field it uses is one the published format describes;
//! * every input schema stays within what llama.cpp's tool-call grammar
//!   accepts (see [`LLAMA_REPETITION_THRESHOLD`]).
//!
//! Each component also has a test of its own (`tests/agent_tools.rs`) that
//! holds every command line against the program it has built. A few tests
//! below look at particular tools by name: what command line a call becomes,
//! and that a value from the model cannot become an option. They are kept
//! for the tools they name and say nothing about any other.
//!
//! That the programs really run in the sandbox and return what the adapters
//! expect is checked by the `pax` workflow (`tests/pax/e2e.py`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use tur_pax::registry::{
    ADAPTER_FILE, AssistantTools, MANIFEST_FILE, Performer, ToolFiles, ToolRegistry, read_tool_sets,
};
use tur_pax::sandbox::is_system_visible;
use tur_pax::{Action, ActionClass};

/// The adapter file format, as published.
const ADAPTER_SCHEMA: &str = include_str!("../../../schemas/pax-tool-adapter.schema.json");

/// Tool files that are not yet where the rule puts them, with the reason.
///
/// `tur-update` keeps its tools one directory down, in `pax/`, because
/// `tur-update` is a program of the image (`image/programs.tsv`): at the
/// component's root its tools would be installed into the image at once, and
/// the image's smoke test and the boot test still count five installed tool
/// sets (`tests/image/smoke.sh`, `tests/boot/report.py`), which this change
/// may not edit. Moving them is a rename and the deletion of these two lines.
const NOT_YET_AT_THE_ROOT: [&str; 2] = [
    "tur-update/pax/agent-tools.json",
    "tur-update/pax/agent-tools.adapter.json",
];

/// The `services` directory of the source tree.
fn services() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The components that ship tools by the rule, found by looking: every
/// directory of `services/` with an `agent-tools.json` of its own, in name
/// order.
fn discovered_components() -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(services())
        .expect("the services directory can be read")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join(MANIFEST_FILE).is_file())
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    found.sort();
    found
}

/// The discovered tool sets, as the program's own reader reads them.
fn shipped() -> ToolFiles {
    read_tool_sets(&services()).expect("the services directory can be read")
}

/// The whole discovered set, loaded as one registry with the command tool.
fn registry() -> ToolRegistry {
    let files = shipped();
    match ToolRegistry::load(&files.manifests, &files.adapters, true) {
        Ok(registry) => registry,
        Err(problems) => {
            let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("the shipped tool set does not load:\n{}", lines.join("\n"));
        }
    }
}

/// The text of one component's file, by the rule.
fn component_file(component: &str, file: &str) -> String {
    let path = services().join(component).join(file);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Every tool of the discovered manifests: its name and the class it
/// declares, read from the JSON as it is written.
fn declared_classes() -> BTreeMap<String, u64> {
    let mut classes = BTreeMap::new();
    for text in shipped().manifests {
        let manifest: Value = serde_json::from_str(&text).expect("a manifest is JSON");
        for tool in manifest["tools"].as_array().expect("tools") {
            let name = tool["name"].as_str().expect("a tool has a name").to_owned();
            let class = tool["class"]
                .as_u64()
                .unwrap_or_else(|| panic!("{name} declares no class"));
            classes.insert(name, class);
        }
    }
    classes
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
        Performer::Command { .. } | Performer::Assistant(_) | Performer::Interface(_) => {
            panic!("{tool} is not a tool of a manifest")
        }
    }
}

/// Every file under `directory` whose name starts with `agent-tools` and
/// ends with `.json`, as a path relative to `services/`. Build output is not
/// looked into, and links are not followed: the device fixtures are trees of
/// `/sys` with links that lead in circles.
fn tool_files_under(directory: &Path, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if name != "target" && !name.starts_with('.') {
                tool_files_under(&path, found);
            }
        } else if name.starts_with("agent-tools") && name.ends_with(".json") {
            let relative = path
                .strip_prefix(services())
                .expect("below services")
                .to_string_lossy()
                .into_owned();
            found.push(relative);
        }
    }
}

#[test]
fn the_source_tree_is_read_as_a_directory_of_tool_sets() {
    // `services/` has the shape of an installed directory of tool sets: one
    // subdirectory for each component, with its two files. The loader the
    // program uses reads it, and finds exactly the components that ship
    // tools by the rule, each with its own two files.
    let components = discovered_components();
    let files = shipped();
    assert_eq!(files.components, components);
    for (index, component) in components.iter().enumerate() {
        assert_eq!(
            files.manifests[index],
            component_file(component, MANIFEST_FILE),
            "{component}"
        );
        assert!(
            services().join(component).join(ADAPTER_FILE).is_file(),
            "{component} ships a manifest without adapters"
        );
    }
    assert_eq!(files.adapters.len(), components.len());
    assert_eq!(
        (MANIFEST_FILE, ADAPTER_FILE),
        ("agent-tools.json", "agent-tools.adapter.json")
    );
    // The tests below that look at tools by name rely on these components.
    for component in [
        "tur-catalogue",
        "tur-device-manager",
        "tur-diagnostics",
        "tur-runtime",
        "tur-storage",
    ] {
        assert!(
            components.iter().any(|found| found == component),
            "{component}"
        );
    }
    // The assistant holds no tool set of its own.
    for component in ["tur-pax", "tur-pax-providers", "tur-pax-memory"] {
        assert!(!services().join(component).join(MANIFEST_FILE).exists());
        assert!(!services().join(component).join(ADAPTER_FILE).exists());
    }
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR")).join("tools").exists());

    // What it read loads as one registry.
    let loaded = ToolRegistry::load(&files.manifests, &files.adapters, true)
        .expect("the tool sets of the source tree load");
    assert_eq!(loaded, registry());
}

#[test]
fn tool_files_live_only_at_the_root_of_a_component() {
    // One rule, so that nothing is parked where P@x does not look: a file
    // named like a tool file anywhere else in a component, or under another
    // name at its root (`agent-tools.next.json`), is a mistake.
    let mut found = Vec::new();
    tool_files_under(&services(), &mut found);
    found.sort();
    let misplaced: Vec<&String> = found
        .iter()
        .filter(|path| {
            let parts: Vec<&str> = path.split('/').collect();
            let by_the_rule =
                parts.len() == 2 && (parts[1] == MANIFEST_FILE || parts[1] == ADAPTER_FILE);
            !by_the_rule && !NOT_YET_AT_THE_ROOT.contains(&path.as_str())
        })
        .collect();
    assert!(
        misplaced.is_empty(),
        "tool files outside a component's root: {misplaced:?}"
    );
    // The exceptions are real files, not stale entries.
    for path in NOT_YET_AT_THE_ROOT {
        assert!(found.iter().any(|known| known == path), "{path} is gone");
    }
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
    let storage_manifest = component_file("tur-storage", MANIFEST_FILE);
    let storage_adapters = component_file("tur-storage", ADAPTER_FILE);
    let device_manifest = component_file("tur-device-manager", MANIFEST_FILE);
    let catalogue_adapters = component_file("tur-catalogue", ADAPTER_FILE);
    // Two components, the second without adapters yet; a directory that is
    // not a tool set; and files that are not in a component's directory.
    write("b-storage/agent-tools.json", &storage_manifest);
    write("b-storage/agent-tools.adapter.json", &storage_adapters);
    write("a-device/agent-tools.json", &device_manifest);
    write("c-other/README", "not a tool set");
    write("c-other/tools.json", "{}");
    write(
        "agent-tools.json",
        "{ not read: it is in no component's directory",
    );
    write("a-device/notes.json", "{ not read: it has another name");

    let files = read_tool_sets(&root).expect("the directory can be read");
    assert_eq!(files.components, ["a-device", "b-storage"]);
    assert_eq!(files.manifests, [device_manifest, storage_manifest]);
    assert_eq!(files.adapters, [storage_adapters]);
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
    write("d-broken/agent-tools.adapter.json", &catalogue_adapters);
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
fn every_tool_has_a_class_and_every_offered_tool_only_observes() {
    let registry = registry();
    let classes = declared_classes();
    assert!(!classes.is_empty());
    for (name, class) in &classes {
        assert!(
            ActionClass::from_u8(u8::try_from(*class).unwrap_or(u8::MAX)).is_some(),
            "{name} declares class {class}"
        );
    }

    // Offered: every tool with an adapter, and the command tool. Each only
    // observes, so nothing above class 1 (nor above class 0) is offered.
    let offered = registry.offered();
    for tool in &offered {
        assert_eq!(tool.class, ActionClass::Observe, "{}", tool.name);
    }
    assert!(
        offered
            .iter()
            .any(|tool| tool.name == "run_readonly_command")
    );
    for tool in &offered {
        if tool.name != "run_readonly_command" {
            assert_eq!(classes.get(&tool.name), Some(&0), "{}", tool.name);
        }
    }

    // Declared by a component, with no program to perform them: not
    // offered. They are every discovered tool above class 0, and nothing
    // else: a class 0 tool a component declares is one P@x can perform.
    let mut unavailable = registry.unavailable();
    unavailable.sort();
    let above_observe: Vec<String> = classes
        .iter()
        .filter(|(_, class)| **class > 0)
        .map(|(name, _)| name.clone())
        .collect();
    assert_eq!(unavailable, above_observe);
    let definitions = registry.definitions().expect("definitions");
    assert_eq!(definitions.len(), offered.len());
}

/// Words and options that send, install, remove, stage, apply or otherwise
/// change something. An adapter whose tool is below class 2 may not put any
/// of them on its command line, as a subcommand, a flag or an option. They
/// are compared without leading dashes, in lower case, and an option is
/// compared by its name before `=`.
const CHANGING_WORDS: [&str; 46] = [
    "send",
    "yes",
    "y",
    "force",
    "install",
    "uninstall",
    "reinstall",
    "remove",
    "delete",
    "rm",
    "erase",
    "wipe",
    "format",
    "stage",
    "apply",
    "commit",
    "execute",
    "upgrade",
    "update",
    "rollback",
    "switch",
    "deploy",
    "pull",
    "download",
    "download-only",
    "from-downloaded",
    "serve",
    "stop",
    "kill",
    "start",
    "restart",
    "enable",
    "disable",
    "mount",
    "unmount",
    "unlock",
    "write",
    "output",
    "accept-licence",
    "accept-license",
    "allow-network",
    "mark-good",
    "reboot",
    "poweroff",
    "shutdown",
    "upload",
];

/// The words of [`CHANGING_WORDS`] an adapter puts on its command lines.
fn changing_words_of(adapter: &Value) -> Vec<String> {
    let mut words = Vec::new();
    for argument in adapter["arguments"].as_array().into_iter().flatten() {
        for key in ["literal", "flag", "option"] {
            let Some(text) = argument[key].as_str() else {
                continue;
            };
            let name = text
                .trim_start_matches('-')
                .split('=')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if CHANGING_WORDS.contains(&name.as_str()) {
                words.push(text.to_owned());
            }
        }
    }
    words
}

/// Every adapter of `adapters` that changes something below class 2, with
/// the words it uses. `classes` holds the class each tool declares.
fn changing_adapters(
    adapters: &str,
    classes: &BTreeMap<String, u64>,
) -> Vec<(String, Vec<String>)> {
    let file: Value = serde_json::from_str(adapters).expect("an adapter file is JSON");
    let mut found = Vec::new();
    for adapter in file["adapters"].as_array().expect("adapters") {
        let tool = adapter["tool"].as_str().expect("a tool").to_owned();
        let class = classes.get(&tool).copied().unwrap_or(0);
        let words = changing_words_of(adapter);
        if class < 2 && !words.is_empty() {
            found.push((tool, words));
        }
    }
    found
}

#[test]
fn no_adapter_sends_or_changes_anything_unless_its_class_says_so() {
    let classes = declared_classes();
    for adapters in shipped().adapters {
        let found = changing_adapters(&adapters, &classes);
        assert!(found.is_empty(), "{found:?}");
    }
    // The rule is not empty: P@x's test service, whose tools are one of each
    // class, is held to it, and passes only because its class 2 and 3
    // tools say so. Declared as class 0, the same adapters are refused.
    let test_manifest: Value =
        serde_json::from_str(include_str!("data/test-service.manifest.json")).expect("JSON");
    let test_adapters = include_str!("data/test-service.adapters.json");
    let mut test_classes: BTreeMap<String, u64> = test_manifest["tools"]
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
    let found = changing_adapters(test_adapters, &test_classes);
    assert!(found.is_empty(), "{found:?}");
    for class in test_classes.values_mut() {
        *class = 0;
    }
    let refused: Vec<String> = changing_adapters(test_adapters, &test_classes)
        .into_iter()
        .map(|(tool, _)| tool)
        .collect();
    assert!(
        refused.contains(&"test_consequential".to_owned()),
        "{refused:?}"
    );
    assert!(
        refused.contains(&"test_system_critical".to_owned()),
        "{refused:?}"
    );
}

#[test]
fn every_shipped_adapter_builds_the_command_line_of_its_program() {
    // These tools, by name. Any other tool's command line is held against
    // its program by its own component's test.
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
    for text in shipped().adapters {
        let file = tur_pax::adapter::AdapterFile::from_json(&text).expect("a valid adapter file");
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
    for text in shipped().adapters {
        let file: Value = serde_json::from_str(&text).expect("JSON");
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

// ------------------------------------------- what llama.cpp's grammar takes

/// llama.cpp's limit on one repetition in a grammar, at the release TUR pins
/// (b9840): `#define MAX_REPETITION_THRESHOLD 2000` in
/// `src/llama-grammar.cpp`. The grammar parser refuses `x{m,n}` when `m` or
/// `n` is above it ("number of repetitions exceeds sane defaults"), and
/// refuses any repetition whose count, multiplied by the number of rules the
/// repeated item made, reaches it ("number of rules that are going to be
/// repeated multiplied by the new repetition exceeds sane defaults").
///
/// `common/json-schema-to-grammar.cpp` (same release) turns a tool's input
/// schema into such repetitions, and llama-server builds the grammar of a
/// request from every tool it is offered, so one schema over the limit
/// fails every request that offers it (`TUR-AI-PROVIDER-004`):
///
/// * a string with `minLength` or `maxLength` becomes `char{min,max}`, one
///   rule repeated: each bound must be below 2000, at most 1999;
/// * an array with `minItems` or `maxItems` becomes
///   `item ("," space item){min-1,max-1}`, a group that can count up to
///   three new rules: `(maxItems - 1) * 3` must be below 2000, so
///   `maxItems` at most 667;
/// * a `pattern`'s own `{m,n}` quantifiers are repeated the same way.
const LLAMA_REPETITION_THRESHOLD: u64 = 2000;

/// The most new rules the group of a separated repetition can count:
/// itself, `space` and the item's rule when either is first seen there.
const SEPARATED_GROUP_RULES: u64 = 3;

/// The counts of a pattern's `{m}`, `{m,}` and `{m,n}` quantifiers.
fn quantifier_counts(pattern: &str) -> Vec<u64> {
    let mut counts = Vec::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        let end = rest.find('}').unwrap_or(rest.len());
        let inside = &rest[..end];
        if !inside.is_empty() && inside.chars().all(|c| c.is_ascii_digit() || c == ',') {
            counts.extend(
                inside
                    .split(',')
                    .filter_map(|count| count.parse::<u64>().ok()),
            );
        }
    }
    counts
}

/// What in `schema` llama.cpp's grammar would refuse, each with where it is.
fn grammar_problems(path: &str, schema: &Value, problems: &mut Vec<String>) {
    let Some(object) = schema.as_object() else {
        return;
    };
    for key in ["minLength", "maxLength"] {
        if let Some(count) = object.get(key).and_then(Value::as_u64)
            && count >= LLAMA_REPETITION_THRESHOLD
        {
            problems.push(format!("{path}: {key} {count}"));
        }
    }
    for key in ["minItems", "maxItems"] {
        if let Some(count) = object.get(key).and_then(Value::as_u64)
            && count
                .saturating_sub(1)
                .saturating_mul(SEPARATED_GROUP_RULES)
                >= LLAMA_REPETITION_THRESHOLD
        {
            problems.push(format!("{path}: {key} {count}"));
        }
    }
    if let Some(pattern) = object.get("pattern").and_then(Value::as_str) {
        for count in quantifier_counts(pattern) {
            if count >= LLAMA_REPETITION_THRESHOLD {
                problems.push(format!("{path}: pattern repeats {count} times"));
            }
        }
    }
    for (key, value) in object {
        match key.as_str() {
            "properties" | "patternProperties" | "$defs" | "definitions" => {
                for (name, inner) in value.as_object().into_iter().flatten() {
                    grammar_problems(&format!("{path}/{key}/{name}"), inner, problems);
                }
            }
            "items"
            | "prefixItems"
            | "additionalProperties"
            | "contains"
            | "not"
            | "anyOf"
            | "oneOf"
            | "allOf" => {
                if let Some(list) = value.as_array() {
                    for (index, inner) in list.iter().enumerate() {
                        grammar_problems(&format!("{path}/{key}/{index}"), inner, problems);
                    }
                } else {
                    grammar_problems(&format!("{path}/{key}"), value, problems);
                }
            }
            _ => {}
        }
    }
}

/// The problems of every tool of a manifest's text.
fn manifest_grammar_problems(source: &str, text: &str, problems: &mut Vec<String>) {
    let manifest: Value = serde_json::from_str(text).expect("a manifest is JSON");
    for tool in manifest["tools"].as_array().expect("tools") {
        let name = tool["name"].as_str().unwrap_or("?");
        grammar_problems(
            &format!("{source}: {name}"),
            &tool["input_schema"],
            problems,
        );
    }
}

#[test]
fn the_grammar_check_knows_llama_cpp_s_limits() {
    let problems = |schema: Value| {
        let mut found = Vec::new();
        grammar_problems("t", &schema, &mut found);
        found
    };
    assert!(problems(json!({ "type": "string", "maxLength": 1999 })).is_empty());
    assert_eq!(
        problems(json!({ "type": "string", "maxLength": 2000 })).len(),
        1
    );
    assert_eq!(
        problems(json!({ "type": "string", "minLength": 2000 })).len(),
        1
    );
    assert!(problems(json!({ "type": "array", "maxItems": 667 })).is_empty());
    assert_eq!(
        problems(json!({ "type": "array", "maxItems": 668 })).len(),
        1
    );
    assert!(problems(json!({ "type": "string", "pattern": "^[a-z]{1,1999}$" })).is_empty());
    assert_eq!(
        problems(json!({ "type": "string", "pattern": "^[a-z]{2,2500}$" })).len(),
        1
    );
    // Found at any depth: the command tool's schema as it was.
    assert_eq!(
        problems(json!({
            "type": "object",
            "properties": {
                "argv": { "type": "array", "items": { "type": "string", "maxLength": 4096 } }
            }
        })),
        ["t/properties/argv/items: maxLength 4096"]
    );
    assert_eq!(
        problems(
            json!({ "anyOf": [{ "type": "string" }, { "type": "string", "maxLength": 4000 }] })
        ),
        ["t/anyOf/1: maxLength 4000"]
    );
}

#[test]
fn every_shipped_tool_schema_fits_llama_cpp_s_tool_grammar() {
    let mut problems = Vec::new();
    // Every tool every component ships, offered or not, wherever its files
    // are: also those not yet at the root.
    let mut files = Vec::new();
    tool_files_under(&services(), &mut files);
    let manifests: Vec<&String> = files
        .iter()
        .filter(|path| !path.ends_with(ADAPTER_FILE))
        .collect();
    assert!(manifests.len() >= discovered_components().len());
    for path in manifests {
        let text = fs::read_to_string(services().join(path)).expect("a manifest");
        manifest_grammar_problems(path, &text, &mut problems);
    }
    // P@x's own tools: the read-only command and its operations.
    let built_in = ToolRegistry::load(&[], &[], true)
        .expect("the built-in tools load")
        .with_assistant_tools(AssistantTools {
            schedules: true,
            notes: true,
        });
    let definitions = built_in.definitions().expect("definitions");
    let names: Vec<&str> = definitions
        .iter()
        .map(|definition| definition.name())
        .collect();
    assert_eq!(
        names,
        [
            "run_readonly_command",
            "propose_reminder",
            "propose_scheduled_task",
            "keep_note"
        ]
    );
    for definition in &definitions {
        grammar_problems(definition.name(), definition.input_schema(), &mut problems);
    }
    // The test fixture the local-ai workflow offers a real model.
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/pax/manifests");
    for entry in fs::read_dir(&fixtures)
        .expect("the fixture manifests")
        .filter_map(Result::ok)
    {
        let path = entry.path();
        let text = fs::read_to_string(&path).expect("a manifest");
        manifest_grammar_problems(&path.display().to_string(), &text, &mut problems);
    }
    assert!(problems.is_empty(), "over llama.cpp's limit: {problems:?}");
}
