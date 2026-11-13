// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Checks on what ships beside the code: the D-Bus introspection XML, the
//! design note, the schema and the source files themselves.
//!
//! These are consistency checks on text. Nothing here talks to D-Bus, and no
//! service implements the interface yet.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tur_update::codes::{UpdateCode, registry};
use tur_update::plan::COMMANDS;

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
    read(&component().join("dbus/org.tur.Update1.xml"))
}

fn design_note() -> String {
    read(&component().join("../../docs/architecture/update-manager.md"))
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
fn dbus_interface_has_the_documented_members() {
    let xml = dbus_xml();
    assert!(
        xml.starts_with("<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->\n")
    );
    assert!(xml.contains("Status: proposed, not frozen, and not implemented."));
    assert_eq!(xml_names(&xml, "node"), ["/org/tur/Update1"]);
    assert_eq!(xml_names(&xml, "interface"), ["org.tur.Update1"]);
    assert_eq!(
        xml_names(&xml, "method"),
        [
            "GetStatus",
            "PreviewCheck",
            "Plan",
            "Check",
            "Download",
            "Rollback",
            "Keep",
            "Release",
            "Cancel"
        ]
    );
    assert_eq!(xml_names(&xml, "signal"), ["StateChanged", "Progress"]);
    assert_eq!(xml_names(&xml, "property"), ["SchemaVersion", "State"]);
    for element in ["node", "interface", "method", "signal"] {
        let opened = xml.matches(&format!("<{element} ")).count();
        let closed = xml.matches(&format!("</{element}>")).count();
        assert_eq!(opened, closed, "{element}");
    }
    // No method restarts the machine, and none is named for bootc's
    // `--apply`.
    for method in xml_names(&xml, "method") {
        let lowered = method.to_lowercase();
        for word in ["reboot", "restart", "apply", "shutdown", "poweroff"] {
            assert!(
                !lowered.contains(word),
                "D-Bus method {method} offers {word}"
            );
        }
    }
}

#[test]
fn every_tool_maps_to_a_dbus_method_that_only_reads() {
    let methods = xml_names(&dbus_xml(), "method");
    let manifest = manifest();
    let mut offered = BTreeSet::new();
    for tool in manifest["tools"].as_array().expect("tools") {
        let method = tool["method"].as_str().expect("a method");
        assert!(
            methods.iter().any(|known| known == method),
            "{method} is not in the XML"
        );
        offered.insert(method.to_owned());
    }
    // What is offered to the assistant is the two methods that read. The
    // methods that change the system are not tools.
    assert_eq!(
        offered,
        BTreeSet::from(["GetStatus".to_owned(), "PreviewCheck".to_owned()])
    );
}

#[test]
fn the_design_note_covers_every_code_method_and_command() {
    let note = design_note();
    for entry in registry() {
        assert!(
            note.contains(entry.code.as_str()),
            "{} is not documented",
            entry.code
        );
    }
    assert_eq!(registry().len(), UpdateCode::all().len());
    for method in xml_names(&dbus_xml(), "method") {
        assert!(
            note.contains(&format!("`{method}`")),
            "{method} is not documented"
        );
    }
    for command in COMMANDS {
        let line = command.argv(None).join(" ");
        let line = line.strip_prefix("/usr/bin/").unwrap_or(&line);
        assert!(note.contains(line), "`{line}` is not documented");
    }
    // What was not tested is said to be not tested.
    assert!(note.contains("NOT RUN"));
    assert!(note.contains("org.tur.Update1"));
}

#[test]
fn the_schema_names_every_code_family_outcome_and_step() {
    let schema: Value = serde_json::from_str(&read(
        &component().join("../../schemas/update-report.schema.json"),
    ))
    .expect("the schema is JSON");
    let defs = &schema["$defs"];
    assert_eq!(
        defs["error_code"]["pattern"],
        "^TUR-UPDATE-[A-Z0-9]+-[0-9]{3}$"
    );
    let values = |name: &str| -> Vec<String> {
        defs[name]["enum"]
            .as_array()
            .unwrap_or_else(|| panic!("{name} has no enum"))
            .iter()
            .map(|value| value.as_str().expect("text").to_owned())
            .collect()
    };
    assert_eq!(values("outcome"), ["proceed", "nothing", "wait", "refuse"]);
    assert_eq!(values("severity"), ["info", "notice", "warning", "error"]);
    assert_eq!(values("slot"), ["staged", "booted", "rollback", "other"]);
    assert_eq!(values("changes"), ["nothing", "update-metadata", "system"]);
    // A step of a plan is one of the commands the program names, except the
    // check, which no plan contains.
    let steps: Vec<String> = defs["step"]["properties"]["id"]["enum"]
        .as_array()
        .expect("step ids")
        .iter()
        .map(|value| value.as_str().expect("text").to_owned())
        .collect();
    let named: Vec<&str> = COMMANDS
        .iter()
        .map(|command| command.id)
        .filter(|id| *id != "check")
        .collect();
    assert_eq!(steps, named);
    // A plan never says it was carried out.
    assert_eq!(defs["plan"]["properties"]["executed"]["const"], false);
}

#[test]
fn every_source_file_starts_with_the_licence_identifier() {
    let mut checked = 0;
    for directory in ["src", "tests"] {
        for file in rust_files(directory) {
            let text = read(&file);
            assert!(
                text.starts_with(
                    "// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\n"
                ),
                "{}",
                file.display()
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 16, "{checked}");
    for script in ["ci/bootc_capture.py", "ci/check_fixtures.py"] {
        let text = read(&component().join(script));
        assert!(
            text.starts_with(
                "#!/usr/bin/env python3\n# SPDX-License-Identifier: \
                 LicenseRef-TUR-Noncommercial-1.0.0\n"
            ),
            "{script}"
        );
    }
    let cargo = read(&component().join("Cargo.toml"));
    assert!(cargo.starts_with("# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\n"));
}

/// The library reads its arguments and the files they name. It reads nothing
/// from the process environment and writes no file. It starts a program in
/// one place only, the live read (`src/probe.rs`), and that program is
/// `bootc status --format=json`: asked for with `--live`, and never
/// otherwise. No source file names a command that changes the system except
/// as text in a plan.
#[test]
fn the_sources_read_no_environment_write_no_file_and_start_only_bootc_status() {
    let files = rust_files("src");
    assert_eq!(files.len(), 11);
    let mut probes = 0;
    for file in files {
        let text = read(&file);
        let name = file.display();
        let is_probe = file.file_name().is_some_and(|base| base == "probe.rs");
        // Nothing is written, by any file.
        for forbidden in [
            "fs::write",
            "File::create",
            "OpenOptions",
            "remove_file",
            "remove_dir",
            "create_dir",
            "set_permissions",
            "rename(",
        ] {
            assert!(!text.contains(forbidden), "{name} contains {forbidden}");
        }
        // Nothing is read from the environment, by any file.
        for forbidden in ["env::var", "env::vars", "env!(", "option_env!("] {
            assert!(!text.contains(forbidden), "{name} contains {forbidden}");
        }
        // No clock, no network, no thread.
        for forbidden in [
            "SystemTime",
            "Instant",
            "TcpStream",
            "UdpSocket",
            "thread::",
        ] {
            assert!(!text.contains(forbidden), "{name} contains {forbidden}");
        }
        if !is_probe {
            // The only use of the environment module is the argument list,
            // and no program is started.
            let environment = text.matches("env::").count();
            let arguments = text.matches("env::args_os").count();
            assert_eq!(environment, arguments, "{name} refers to the environment");
            for forbidden in ["process::Command", "Command::new", "Stdio", ".spawn("] {
                assert!(!text.contains(forbidden), "{name} contains {forbidden}");
            }
            continue;
        }
        probes += 1;
        // The live read: one place starts a program, and it hands the
        // program an emptied environment of two fixed variables and no
        // standard input.
        assert_eq!(text.matches("Command::new").count(), 1, "{name}");
        assert_eq!(text.matches(".env_clear()").count(), 1, "{name}");
        assert_eq!(text.matches(".env(").count(), 2, "{name}");
        assert!(text.contains(".env(\"LC_ALL\", \"C.UTF-8\")"), "{name}");
        assert!(
            text.contains(".env(\"PATH\", \"/usr/sbin:/usr/bin\")"),
            "{name}"
        );
        assert_eq!(
            text.matches(".stdin(std::process::Stdio::null())").count(),
            1
        );
        assert_eq!(text.matches(".spawn(").count(), 0, "{name}");
        // One caller hands that place a program: the live read, with the
        // fixed command line. The other calls of `run` in the file are the
        // stand-in of its unit tests.
        let shipped = text.split("#[cfg(test)]").next().unwrap_or_default();
        assert_eq!(shipped.matches(".run(").count(), 1, "{name}");
        assert_eq!(
            shipped
                .matches(".run(STATUS_PROGRAM, &STATUS_ARGUMENTS)")
                .count(),
            1,
            "{name}"
        );
        // And the command line is the one that prints the deployments.
        assert!(shipped.contains("pub const STATUS_PROGRAM: &str = \"/usr/bin/bootc\";"));
        assert!(
            shipped.contains(
                "pub const STATUS_ARGUMENTS: [&str; 2] = [\"status\", \"--format=json\"];"
            )
        );
    }
    assert_eq!(probes, 1);
}

/// Outside `src/plan.rs`, where the commands are data, no shipped source
/// holds the words of a command that changes the system.
#[test]
fn commands_that_change_the_system_are_named_in_one_file_only() {
    let changing = [
        "\"upgrade\"",
        "\"rollback\"",
        "\"switch\"",
        "\"--apply\"",
        "\"--download-only\"",
        "\"--from-downloaded\"",
        "\"--unpin\"",
        "\"admin\"",
        "/usr/bin/ostree",
    ];
    for file in rust_files("src") {
        let base = file
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        if base == "plan.rs" {
            continue;
        }
        let text = read(&file);
        let shipped = text.split("#[cfg(test)]").next().unwrap_or_default();
        for word in changing {
            // `rollback` is also the name of a deployment's place, in the
            // model and on the command line.
            if word == "\"rollback\"" && (base == "state.rs" || base == "cli.rs") {
                continue;
            }
            assert!(!shipped.contains(word), "{base} contains {word}");
        }
    }
}
