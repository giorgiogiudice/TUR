// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What ships beside the code, held against it: the proposed D-Bus
//! interface and the command line, the JSON schemas and the Rust types,
//! the design note and the error codes, and the one place that downloads.
//!
//! These are checks on text. Nothing here talks to D-Bus; no service
//! implements the interface yet.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use clap::CommandFactory;
use serde_json::Value;
use tur_apps::cli::{Cli, DBUS_METHODS};
use tur_apps::codes::Code;
use tur_apps::events::Event;
use tur_apps::records::{FlatpakPart, RECORD_VERSION, Record, Source, WindowsPart};

fn component() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn schema(name: &str) -> Value {
    serde_json::from_str(&read(&component().join("../../schemas").join(name))).expect("JSON")
}

fn xml() -> String {
    read(&component().join("dbus/org.tur.Apps1.xml"))
}

fn xml_names(xml: &str, element: &str) -> Vec<String> {
    let opening = format!("<{element} name=\"");
    xml.match_indices(&opening)
        .filter_map(|(start, _)| {
            xml[start + opening.len()..]
                .split_once('"')
                .map(|(name, _)| name.to_owned())
        })
        .collect()
}

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

#[test]
fn the_dbus_interface_and_the_command_line_are_one() {
    let xml = xml();
    assert!(xml.starts_with("<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->\n"));
    assert!(xml.contains("Status: proposed, not frozen, and not implemented."));
    assert_eq!(xml_names(&xml, "node"), ["/org/tur/Apps1"]);
    assert_eq!(xml_names(&xml, "interface"), ["org.tur.Apps1"]);
    let methods: Vec<String> = DBUS_METHODS.iter().map(|(name, _, _)| (*name).to_owned()).collect();
    assert_eq!(xml_names(&xml, "method"), methods);
    assert_eq!(xml_names(&xml, "signal"), ["Event"]);
    assert_eq!(xml_names(&xml, "property"), ["SchemaVersion"]);

    // Every method that a subcommand does the work of names one the
    // program has, and every subcommand has a method.
    let command = Cli::command();
    let subcommands: BTreeSet<String> = command
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_owned())
        .filter(|name| name != "help")
        .collect();
    let mapped: BTreeSet<String> = DBUS_METHODS
        .iter()
        .filter_map(|(_, subcommand, _)| subcommand.map(str::to_owned))
        .collect();
    assert_eq!(mapped, subcommands);
    // Only Cancel belongs to the service alone.
    let service_only: Vec<&str> = DBUS_METHODS
        .iter()
        .filter(|(_, subcommand, _)| subcommand.is_none())
        .map(|(name, _, _)| *name)
        .collect();
    assert_eq!(service_only, ["Cancel"]);
    // Remove requires the user's answer about the data.
    assert!(xml.contains("\"data\" (s): \"keep\" or \"delete\". Required"));
}

#[test]
fn the_assistant_s_tools_are_methods_that_only_read() {
    let manifest: Value =
        serde_json::from_str(&read(&component().join("agent-tools.json"))).expect("JSON");
    let xml_methods = xml_names(&xml(), "method");
    for tool in manifest["tools"].as_array().expect("tools") {
        let method = tool["method"].as_str().expect("a method");
        assert!(xml_methods.iter().any(|known| known == method), "{method}");
        let (_, _, changes) = DBUS_METHODS
            .iter()
            .find(|(name, _, _)| *name == method)
            .expect("a method of the table");
        assert!(!changes, "the tool for {method} would change something");
        assert_eq!(tool["class"], 0);
    }
}

#[test]
fn the_record_schema_names_the_record_s_fields() {
    let schema = schema("app-installed.schema.json");
    let record = Record {
        schema_version: RECORD_VERSION,
        id: "a.b".to_owned(),
        name: "B".to_owned(),
        route: "wine".to_owned(),
        source: Source {
            kind: "user-file".to_owned(),
            remote: None,
            url: None,
            file_name: None,
            sha256: None,
            size: None,
            pinned: false,
        },
        version: None,
        installed_at: "2026-11-12T00:00:00Z".to_owned(),
        flatpak: Some(FlatpakPart {
            app_id: "a.b.C".to_owned(),
            reference: "app/a.b.C/x86_64/stable".to_owned(),
            remote: "flathub".to_owned(),
            commit: None,
            added_refs: Vec::new(),
        }),
        windows: Some(WindowsPart {
            runtime_app_id: "a.b".to_owned(),
            main_executable: None,
        }),
    };
    let written = serde_json::to_value(&record).unwrap();
    assert_eq!(keys(&written), keys(&schema["properties"]));
    assert_eq!(keys(&written["source"]), keys(&schema["$defs"]["source"]["properties"]));
    assert_eq!(keys(&written["flatpak"]), keys(&schema["$defs"]["flatpak"]["properties"]));
    assert_eq!(keys(&written["windows"]), keys(&schema["$defs"]["windows"]["properties"]));
    assert_eq!(schema["properties"]["schema_version"]["const"], RECORD_VERSION);
    for object in [&schema, &schema["$defs"]["source"], &schema["$defs"]["flatpak"], &schema["$defs"]["windows"]] {
        assert_eq!(object["additionalProperties"], Value::Bool(false));
    }
}

#[test]
fn the_event_schema_names_every_event() {
    let schema = schema("app-install-event.schema.json");
    let samples = [
        Event::Plan { plan: Value::Null },
        Event::StepStarted { step: String::new(), text: String::new() },
        Event::StepFinished { step: String::new(), ok: true },
        Event::Activity { step: String::new(), text: String::new() },
        Event::Progress { step: String::new(), done: 0, total: Some(1) },
        Event::Result { operation: String::new(), result: Value::Null },
        Event::Error { error: tur_apps::Failure::new(Code::StepFailed, "x") },
    ];
    let count = |event: &Event| match event {
        Event::Plan { .. }
        | Event::StepStarted { .. }
        | Event::StepFinished { .. }
        | Event::Activity { .. }
        | Event::Progress { .. }
        | Event::Result { .. }
        | Event::Error { .. } => 1usize,
    };
    assert_eq!(samples.iter().map(count).sum::<usize>(), 7);
    let names: BTreeSet<String> = samples
        .iter()
        .map(|event| serde_json::to_value(event).unwrap()["event"].as_str().unwrap().to_owned())
        .collect();
    let listed: BTreeSet<String> = schema["properties"]["event"]["enum"]
        .as_array()
        .expect("the event kinds")
        .iter()
        .map(|name| name.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, listed);
    for sample in &samples {
        let written = serde_json::to_value(sample).unwrap();
        let kind = written["event"].as_str().unwrap();
        let mut fields = keys(&written);
        fields.insert("schema_version".to_owned());
        let documented = keys(&schema["$defs"][kind]["properties"]);
        assert_eq!(fields, documented, "{kind}");
    }
    let codes: Vec<String> = Code::ALL.iter().map(|code| code.as_str().to_owned()).collect();
    let pattern = schema["$defs"]["failure"]["properties"]["code"]["pattern"]
        .as_str()
        .unwrap();
    assert_eq!(pattern, "^TUR-DESKTOP-APPS-[0-9]{3}$");
    assert!(codes.iter().all(|code| code.starts_with("TUR-DESKTOP-APPS-")));
}

#[test]
fn the_plan_schema_names_the_plan_s_fields() {
    let schema = schema("app-install-plan.schema.json");
    let required: BTreeSet<String> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap().to_owned())
        .collect();
    // The fields of tur_apps::plan::Plan that are written.
    let fields: BTreeSet<String> = [
        "schema_version",
        "kind",
        "application",
        "route",
        "chosen_by",
        "router",
        "host",
        "source",
        "downloads",
        "installs",
        "access",
        "access_known",
        "removal",
        "steps",
        "notes",
    ]
    .iter()
    .map(|name| (*name).to_owned())
    .collect();
    assert_eq!(keys(&schema["properties"]), fields);
    assert_eq!(required, fields);
    assert_eq!(schema["properties"]["route"]["enum"], serde_json::json!(["flatpak", "wine"]));
    assert_eq!(
        schema["properties"]["removal"]["properties"]["data_kept_by_default"]["const"],
        true
    );
    // The source file declares the same fields.
    let source = read(&component().join("src/plan.rs"));
    let declaration = source
        .split("pub struct Plan {")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .expect("the declaration of Plan");
    for field in &fields {
        assert!(declaration.contains(&format!("pub {field}:")), "{field}");
    }
}

#[test]
fn the_design_note_names_every_code_and_command() {
    let note = read(&component().join("../../docs/architecture/app-install-backend.md"));
    for code in Code::ALL {
        assert!(note.contains(code.as_str()), "{code} is not documented");
    }
    for subcommand in Cli::command()
        .get_subcommands()
        .filter(|subcommand| subcommand.get_name() != "help")
    {
        assert!(
            note.contains(&format!("tur-apps {}", subcommand.get_name())),
            "{} is not documented",
            subcommand.get_name()
        );
    }
    assert!(note.contains("org.tur.Apps1"));
}

#[test]
fn only_one_module_downloads_and_none_uses_the_network_itself() {
    let mut files: Vec<PathBuf> = fs::read_dir(component().join("src"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    files.sort();
    for file in files {
        let text = read(&file);
        let production = text.split("#[cfg(test)]").next().unwrap_or_default();
        let name = file.file_name().unwrap().to_str().unwrap().to_owned();
        assert!(!production.contains(concat!("std::", "net")), "{name}");
        assert!(!production.contains(concat!("un", "safe ")), "{name}");
        if name != "download.rs" {
            // The options that make curl safe are written in one place.
            assert!(!production.contains("--proto"), "{name} builds a download");
            assert!(!production.contains("fn fetch_"), "{name} fetches");
        }
        assert!(
            text.starts_with("// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\n"),
            "{name}"
        );
    }
}
