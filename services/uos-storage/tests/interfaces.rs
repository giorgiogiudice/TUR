// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Checks on what ships beside the code: the assistant tool manifest, the
//! D-Bus introspection XML, the design note and the source files themselves.
//!
//! These are consistency checks on text. Nothing here talks to D-Bus, and no
//! service implements the interface yet.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use uos_storage::codes::registry;
use uos_storage::{EXCLUDED_IMPLEMENTATIONS, POLICY_TABLE};

fn component() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn manifest() -> Value {
    serde_json::from_str(&read(&component().join("agent-tools.json"))).expect("valid JSON")
}

fn dbus_xml() -> String {
    read(&component().join("dbus/org.uos.Storage1.xml"))
}

fn design_note() -> String {
    read(&component().join("../../docs/architecture/storage.md"))
}

/// The names of the elements of one kind in the introspection XML.
fn xml_names(xml: &str, element: &str) -> Vec<String> {
    let opening = format!("<{element} name=\"");
    xml.match_indices(&opening)
        .filter_map(|(start, _)| {
            let rest = &xml[start + opening.len()..];
            rest.split_once('"').map(|(name, _)| name.to_owned())
        })
        .collect()
}

fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn rust_files(directory: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(component().join(directory))
        .expect("the directory is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    files.sort();
    files
}

#[test]
fn manifest_follows_the_addendum_format() {
    let manifest = manifest();
    assert_eq!(
        keys(&manifest),
        BTreeSet::from(["schema_version", "service", "tools"])
    );
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["service"], "org.uos.Storage1");
    let mut names = BTreeSet::new();
    for tool in manifest["tools"].as_array().expect("tools is an array") {
        assert_eq!(
            keys(tool),
            BTreeSet::from(["class", "description", "input_schema", "method", "name"]),
            "{tool}"
        );
        let name = tool["name"].as_str().expect("name is a string");
        assert!(names.insert(name.to_owned()), "{name} appears twice");
        let suffix = name
            .strip_prefix("storage_")
            .expect("tool names start with storage_");
        assert!(!suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'));
        // The rules below are those of schemas/agent-tool-manifest.schema.json.
        assert!(name.len() <= 64, "{name}");
        let description = tool["description"].as_str().expect("a description");
        assert!(description.ends_with('.'), "{name}");
        assert!(description.chars().count() <= 1024, "{name}");
        assert!(!description.chars().any(char::is_control), "{name}");
        // Class 4 cannot be declared; this manifest stays within 0 and 1.
        let class = tool["class"].as_u64().expect("class is a whole number");
        assert!(class <= 1, "{name}: class {class}");
        let method = tool["method"].as_str().expect("method is a string");
        let first_ok = method.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
        let rest_ok = method
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
        assert!(first_ok && rest_ok, "{name}: {method}");

        let input = &tool["input_schema"];
        assert!(keys(input).is_subset(&BTreeSet::from([
            "type",
            "properties",
            "required",
            "additionalProperties"
        ])));
        assert_eq!(input["type"], "object", "{name}");
        assert_eq!(input["additionalProperties"], false, "{name}");
        let properties = input["properties"]
            .as_object()
            .expect("properties is an object");
        for (property, schema) in properties {
            assert!(
                keys(schema).is_subset(&BTreeSet::from(["type", "enum"])),
                "{name}"
            );
            assert_eq!(schema["type"], "string", "{name}: {property}");
            if let Some(allowed) = schema.get("enum") {
                let allowed = allowed.as_array().expect("enum is an array");
                assert!(
                    !allowed.is_empty() && allowed.iter().all(Value::is_string),
                    "{name}"
                );
            }
        }
        for required in input["required"].as_array().into_iter().flatten() {
            let required = required.as_str().expect("required names are strings");
            assert!(properties.contains_key(required), "{name}: {required}");
        }
    }
}

#[test]
fn manifest_offers_only_observation_and_reversible_tools() {
    let manifest = manifest();
    let classes: BTreeMap<&str, u64> = manifest["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            (
                tool["name"].as_str().unwrap(),
                tool["class"].as_u64().unwrap(),
            )
        })
        .collect();
    let expected = BTreeMap::from([
        ("storage_list_volumes", 0),
        ("storage_preview_mount", 0),
        ("storage_mount", 1),
        ("storage_unmount", 1),
        ("storage_unlock", 1),
    ]);
    assert_eq!(classes, expected);
}

#[test]
fn destructive_operations_are_not_offered_anywhere() {
    let destructive = [
        "format",
        "partition",
        "encrypt",
        "wipe",
        "erase",
        "mkfs",
        "resize",
        "label",
        "repair",
    ];
    let manifest = manifest();
    for tool in manifest["tools"].as_array().unwrap() {
        for field in ["name", "method"] {
            let text = tool[field].as_str().unwrap().to_lowercase();
            for word in destructive {
                assert!(!text.contains(word), "tool {field} {text} offers {word}");
            }
        }
        // The assistant never passes a secret or a device path.
        let properties = keys(&tool["input_schema"]["properties"]);
        assert!(
            properties.is_subset(&BTreeSet::from(["volume_id", "access"])),
            "{tool}"
        );
    }
    let xml = dbus_xml();
    for method in xml_names(&xml, "method") {
        for word in destructive {
            assert!(
                !method.to_lowercase().contains(word),
                "D-Bus method {method} offers {word}"
            );
        }
    }
    for argument in xml_names(&xml, "arg") {
        assert!(
            !argument.contains("pass"),
            "argument {argument} looks like a secret"
        );
        assert!(
            !argument.contains("key"),
            "argument {argument} looks like a secret"
        );
    }
}

#[test]
fn dbus_interface_has_the_documented_members() {
    let xml = dbus_xml();
    assert!(xml.starts_with("<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->\n"));
    assert_eq!(xml_names(&xml, "node"), ["/org/uos/Storage1"]);
    assert_eq!(xml_names(&xml, "interface"), ["org.uos.Storage1"]);
    assert_eq!(
        xml_names(&xml, "method"),
        [
            "ListVolumes",
            "PreviewMount",
            "Mount",
            "Unmount",
            "Unlock",
            "Cancel"
        ]
    );
    assert_eq!(
        xml_names(&xml, "signal"),
        ["VolumeAdded", "VolumeRemoved", "VolumeChanged"]
    );
    assert_eq!(xml_names(&xml, "property"), ["SchemaVersion"]);
    for element in ["node", "interface", "method", "signal"] {
        let opened = xml.matches(&format!("<{element} ")).count();
        let closed = xml.matches(&format!("</{element}>")).count();
        assert_eq!(opened, closed, "{element}");
    }
}

#[test]
fn every_tool_maps_to_a_dbus_method() {
    let methods = xml_names(&dbus_xml(), "method");
    let manifest = manifest();
    let mut offered = BTreeSet::new();
    for tool in manifest["tools"].as_array().unwrap() {
        let method = tool["method"].as_str().unwrap();
        assert!(
            methods.iter().any(|known| known == method),
            "{method} is not in the XML"
        );
        offered.insert(method.to_owned());
    }
    // Cancel is for callers that started an operation; it is not a tool.
    let not_offered: Vec<&String> = methods
        .iter()
        .filter(|method| !offered.contains(*method))
        .collect();
    assert_eq!(not_offered, ["Cancel"]);
}

#[test]
fn design_note_covers_every_code_method_and_policy_row() {
    let note = design_note();
    for entry in registry() {
        assert!(
            note.contains(entry.code.as_str()),
            "{} is not documented",
            entry.code
        );
    }
    for method in xml_names(&dbus_xml(), "method") {
        assert!(
            note.contains(&format!("`{method}`")),
            "{method} is not documented"
        );
    }
    for row in POLICY_TABLE {
        assert!(
            note.contains(row.spec_name),
            "{} is not documented",
            row.spec_name
        );
        if let Some(driver) = row.driver {
            assert!(note.contains(driver), "{driver} is not documented");
        }
    }
    for excluded in EXCLUDED_IMPLEMENTATIONS {
        assert!(
            note.contains(excluded.name),
            "{} is not documented",
            excluded.name
        );
    }
    assert!(note.contains("NOT RUN"));
}

#[test]
fn every_rust_file_starts_with_the_licence_identifier() {
    let mut checked = 0;
    for directory in ["src", "tests"] {
        for file in rust_files(directory) {
            let text = read(&file);
            assert!(
                text.starts_with("// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0\n"),
                "{}",
                file.display()
            );
            checked += 1;
        }
    }
    assert!(checked >= 13, "{checked}");
}

/// The library reads its arguments and the files they name. It reads nothing
/// from the process environment, starts no program and writes no file.
#[test]
fn the_sources_read_no_environment_start_no_program_and_write_no_file() {
    let files = rust_files("src");
    assert!(files.len() >= 10);
    for file in files {
        let text = read(&file);
        let name = file.display();
        // The only use of the environment module is the argument list.
        let environment = text.matches("env").count();
        let arguments = text.matches("env::args_os").count();
        assert_eq!(environment, arguments, "{name} refers to the environment");
        for forbidden in [
            "process::Command",
            "Command::new",
            "fs::write",
            "File::create",
            "OpenOptions",
            "remove_file",
            "remove_dir",
            "create_dir",
            "set_permissions",
        ] {
            assert!(!text.contains(forbidden), "{name} contains {forbidden}");
        }
    }
}
