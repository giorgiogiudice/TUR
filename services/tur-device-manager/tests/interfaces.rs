// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The interface definitions shipped beside the code: the D-Bus introspection
//! XML, the assistant tool manifest, the schema and the design note. These
//! tests keep them consistent with each other and with the code. They do not
//! start a D-Bus service: none exists yet.

mod support;

use std::collections::BTreeSet;

use serde_json::Value;
use tur_device_manager::ProblemKind;

const INTERFACE_XML: &str = include_str!("../dbus/org.tur.Device1.xml");
const TOOL_MANIFEST: &str = include_str!("../agent-tools.json");
const DESIGN_NOTE: &str = include_str!("../../../docs/architecture/device-manager.md");

/// The values of the `name` attribute of every `<tag name="...">` element.
fn names_of(tag: &str) -> Vec<&'static str> {
    let open = format!("<{tag} name=\"");
    INTERFACE_XML
        .split(open.as_str())
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect()
}

/// A minimal well-formedness check: comments and declarations aside, every
/// opening tag is closed in order.
fn check_xml_nesting(xml: &str) {
    let mut open: Vec<&str> = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        rest = &rest[start..];
        if let Some(after) = rest.strip_prefix("<!--") {
            let end = after.find("-->").expect("a comment is closed");
            assert!(!after[..end].contains("--"), "\"--\" inside a comment");
            rest = &after[end + 3..];
            continue;
        }
        let end = rest.find('>').expect("a tag is closed");
        let tag = &rest[1..end];
        rest = &rest[end + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            assert_eq!(open.pop(), Some(name), "closing tag does not match");
        } else if !tag.ends_with('/') {
            open.push(tag.split_whitespace().next().expect("a tag has a name"));
        }
    }
    assert!(open.is_empty(), "unclosed tags: {open:?}");
}

#[test]
fn the_dbus_interface_is_read_only_and_well_formed() {
    check_xml_nesting(INTERFACE_XML);
    assert_eq!(names_of("node"), ["/org/tur/Device1"]);
    assert_eq!(names_of("interface"), ["org.tur.Device1"]);
    assert_eq!(
        names_of("method"),
        ["ListDevices", "GetDevice", "GetInventory", "ListProblems"]
    );
    assert_eq!(names_of("signal"), ["DeviceAdded", "DeviceRemoved"]);
    assert_eq!(names_of("property"), ["SchemaVersion"]);

    // Nothing in the interface can change the system: every method lists or
    // gets, and the one property cannot be written.
    for method in names_of("method") {
        assert!(
            method.starts_with("List") || method.starts_with("Get"),
            "{method} is not a read"
        );
    }
    assert!(!INTERFACE_XML.contains("access=\"write\""));
    assert!(!INTERFACE_XML.contains("access=\"readwrite\""));
    assert!(INTERFACE_XML.contains("SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0"));
}

#[test]
fn the_tool_manifest_follows_the_documented_format() {
    let manifest: Value = serde_json::from_str(TOOL_MANIFEST).expect("the manifest is JSON");
    let top: BTreeSet<&str> = manifest
        .as_object()
        .expect("the manifest is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(top, BTreeSet::from(["schema_version", "service", "tools"]));
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["service"], "org.tur.Device1");

    let methods = names_of("method");
    let tools = manifest["tools"].as_array().expect("tools is an array");
    assert_eq!(tools.len(), 4);
    let mut names = BTreeSet::new();
    let mut used_methods = BTreeSet::new();
    for tool in tools {
        let fields: BTreeSet<&str> = tool
            .as_object()
            .expect("a tool is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            fields,
            BTreeSet::from(["class", "description", "input_schema", "method", "name"])
        );

        let name = tool["name"].as_str().expect("a tool has a name");
        assert!(
            support::pattern_matches("^[a-z][a-z0-9_]{0,63}$", name),
            "{name}"
        );
        assert!(name.starts_with("device_"), "{name}");
        assert!(names.insert(name), "{name} is declared twice");

        // Read-only tools only: class 0, observe.
        assert_eq!(tool["class"], 0, "{name}");

        let description = tool["description"].as_str().expect("a description");
        assert!(!description.is_empty() && description.chars().count() <= 1024);
        assert!(!description.chars().any(char::is_control), "{name}");

        // The tool calls a method the interface really has.
        let method = tool["method"].as_str().expect("a method");
        assert!(methods.contains(&method), "{name} calls unknown {method}");
        used_methods.insert(method);

        let input = &tool["input_schema"];
        assert_eq!(input["type"], "object", "{name}");
        assert_eq!(input["additionalProperties"], Value::Bool(false), "{name}");
        let properties = input["properties"].as_object().expect("properties");
        for required in input["required"].as_array().into_iter().flatten() {
            let required = required.as_str().expect("required names are strings");
            assert!(properties.contains_key(required), "{name}: {required}");
        }
        // No tool can ask for unredacted values.
        for property in properties.keys() {
            assert!(!property.contains("sensitive"), "{name}: {property}");
        }
    }
    // Every method of the interface is offered as a tool.
    assert_eq!(used_methods.len(), methods.len());
}

#[test]
fn the_manifest_and_the_schema_name_the_same_categories() {
    let manifest: Value = serde_json::from_str(TOOL_MANIFEST).expect("the manifest is JSON");
    let offered =
        &manifest["tools"][0]["input_schema"]["properties"]["categories"]["items"]["enum"];
    let schema = support::schema();
    assert_eq!(offered, &schema["$defs"]["category"]["enum"]);
    assert_eq!(offered.as_array().map(Vec::len), Some(10));
}

#[test]
fn the_design_note_records_every_code_and_every_interface_member() {
    for kind in ProblemKind::ALL {
        assert!(
            DESIGN_NOTE.contains(kind.code_text()),
            "{} is not in the design note",
            kind.code_text()
        );
        assert!(DESIGN_NOTE.contains(kind.title()), "{}", kind.title());
    }
    for code in [
        "TUR-DEVICE-INPUT-001",
        "TUR-DEVICE-INPUT-002",
        "TUR-DEVICE-OUTPUT-001",
    ] {
        assert!(DESIGN_NOTE.contains(code), "{code}");
    }
    for member in names_of("method")
        .into_iter()
        .chain(names_of("signal"))
        .chain(names_of("property"))
    {
        assert!(DESIGN_NOTE.contains(member), "{member} is not documented");
    }
    // The note must not claim more than was done.
    assert!(DESIGN_NOTE.contains("NOT RUN"));
    assert!(DESIGN_NOTE.contains("not implemented"));
}
