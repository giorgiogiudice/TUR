// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Tests that run the built `tur-update` binary on the fixtures and check
//! its JSON output against `schemas/update-report.schema.json`.
//!
//! The schema check uses a small validator written here, which understands
//! exactly the keywords the schema uses and fails on any other, so that the
//! schema cannot grow a rule these tests silently skip. It is the validator
//! of `tur-storage`'s tests, with this schema's one pattern.
//!
//! Every run here reads a file (`--from`). The program's one live command is
//! exercised with a stand-in for `bootc` in its unit tests, and with the real
//! `bootc` of the base image by the `update` workflow.

use std::fs;
use std::process::Command;

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_tur-update");
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
const SCHEMA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../schemas/update-report.schema.json"
);

struct Run {
    status: i32,
    stdout: String,
    stderr: String,
}

fn run_with_environment(args: &[&str], environment: &[(&str, &str)]) -> Run {
    let output = Command::new(BINARY)
        .args(args)
        .env_clear()
        .envs(environment.iter().copied())
        .output()
        .expect("the tur-update binary starts");
    Run {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    }
}

fn run(args: &[&str]) -> Run {
    run_with_environment(args, &[])
}

/// A fixture path that lives as long as the test process, so that it can sit
/// in an argument list beside string literals.
fn path(name: &str) -> &'static str {
    Box::leak(format!("{FIXTURES}/{name}").into_boxed_str())
}

/// Runs a command that must succeed and returns its standard output.
fn ok(args: &[&str]) -> String {
    let result = run(args);
    assert_eq!(result.status, 0, "{args:?}\n{}", result.stderr);
    assert!(result.stderr.is_empty(), "{args:?}\n{}", result.stderr);
    result.stdout
}

fn json(args: &[&str]) -> Value {
    serde_json::from_str(&ok(args)).unwrap_or_else(|error| panic!("{args:?}: {error}"))
}

// ---------------------------------------------------------------------------
// A validator for the subset of JSON Schema that the update schema uses.
// ---------------------------------------------------------------------------

const KNOWN_KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "title",
    "description",
    "$defs",
    "$ref",
    "oneOf",
    "type",
    "enum",
    "const",
    "required",
    "properties",
    "additionalProperties",
    "items",
    "minimum",
    "minLength",
    "pattern",
];

fn type_matches(name: &str, value: &Value) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        _ => false,
    }
}

fn is_update_code(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("TUR-UPDATE-") else {
        return false;
    };
    let Some((topic, number)) = rest.rsplit_once('-') else {
        return false;
    };
    !topic.is_empty()
        && topic
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && number.len() == 3
        && number.bytes().all(|b| b.is_ascii_digit())
}

/// The schema's patterns, implemented by hand. `None` for a pattern this
/// validator does not know.
fn pattern_matches(pattern: &str, text: &str) -> Option<bool> {
    match pattern {
        "^TUR-UPDATE-[A-Z0-9]+-[0-9]{3}$" => Some(is_update_code(text)),
        _ => None,
    }
}

fn validate(root: &Value, schema: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(schema) = schema.as_object() else {
        errors.push(format!("{path}: the schema is not an object"));
        return;
    };
    for keyword in schema.keys() {
        if !KNOWN_KEYWORDS.contains(&keyword.as_str()) {
            errors.push(format!("{path}: schema keyword {keyword} is not supported"));
        }
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let target = reference
            .strip_prefix("#/$defs/")
            .and_then(|name| root["$defs"].get(name));
        match target {
            Some(target) => validate(root, target, value, path, errors),
            None => errors.push(format!("{path}: reference {reference} does not resolve")),
        }
    }
    if let Some(alternatives) = schema.get("oneOf").and_then(Value::as_array) {
        let matching = alternatives
            .iter()
            .filter(|alternative| {
                let mut inner = Vec::new();
                validate(root, alternative, value, path, &mut inner);
                inner.is_empty()
            })
            .count();
        if matching != 1 {
            errors.push(format!(
                "{path}: matches {matching} oneOf alternatives, expected 1"
            ));
        }
    }
    if let Some(expected) = schema.get("const")
        && expected != value
    {
        errors.push(format!("{path}: expected {expected}, found {value}"));
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        errors.push(format!("{path}: {value} is not one of the allowed values"));
    }
    if let Some(kind) = schema.get("type") {
        let names: Vec<&str> = match kind {
            Value::String(name) => vec![name.as_str()],
            Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if !names.iter().any(|name| type_matches(name, value)) {
            errors.push(format!("{path}: {value} is not of type {names:?}"));
        }
    }
    if let Some(object) = value.as_object() {
        let required = schema.get("required").and_then(Value::as_array);
        for name in required.into_iter().flatten().filter_map(Value::as_str) {
            if !object.contains_key(name) {
                errors.push(format!("{path}: required property {name} is missing"));
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        let closed = schema.get("additionalProperties") == Some(&Value::Bool(false));
        for (name, member) in object {
            match properties.and_then(|properties| properties.get(name)) {
                Some(member_schema) => {
                    validate(
                        root,
                        member_schema,
                        member,
                        &format!("{path}/{name}"),
                        errors,
                    );
                }
                None if closed => errors.push(format!("{path}: unexpected property {name}")),
                None => {}
            }
        }
    }
    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, item) in array.iter().enumerate() {
            validate(root, items, item, &format!("{path}/{index}"), errors);
        }
    }
    if let Some(text) = value.as_str() {
        let length = u64::try_from(text.chars().count()).unwrap_or(u64::MAX);
        if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64)
            && length < minimum
        {
            errors.push(format!("{path}: shorter than {minimum}"));
        }
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
            match pattern_matches(pattern, text) {
                Some(true) => {}
                Some(false) => errors.push(format!("{path}: {text:?} does not match {pattern}")),
                None => errors.push(format!("{path}: pattern {pattern} is not supported")),
            }
        }
    }
    if let (Some(minimum), Some(number)) = (
        schema.get("minimum").and_then(Value::as_i64),
        value.as_i64(),
    ) && number < minimum
    {
        errors.push(format!("{path}: {number} is below {minimum}"));
    }
}

fn schema() -> Value {
    let text = fs::read_to_string(SCHEMA).expect("the schema file is readable");
    serde_json::from_str(&text).expect("the schema is valid JSON")
}

fn schema_errors(schema: &Value, document: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    validate(schema, schema, document, "", &mut errors);
    errors
}

fn assert_valid(schema: &Value, document: &Value, context: &str) {
    let errors = schema_errors(schema, document);
    assert!(errors.is_empty(), "{context}: {errors:#?}");
}

// ---------------------------------------------------------------------------
// The fixtures
// ---------------------------------------------------------------------------

/// Every fixture that is a bootc host, with the boot-health document taken
/// on the same machine at the same moment where there is one.
const MACHINES: &[(&str, Option<&str>)] = &[
    ("bootc/tur-booted.json", None),
    ("bootc/tur-staged.json", Some("boot-health/normal.json")),
    (
        "bootc/tur-after-fallback.json",
        Some("boot-health/after-fallback.json"),
    ),
    (
        "bootc/tur-after-update.json",
        Some("boot-health/after-update.json"),
    ),
    ("bootc-upstream/booted-pinned.json", None),
    ("bootc-upstream/booted-update-same-digest.json", None),
    ("bootc-upstream/booted-usroverlay.json", None),
    ("bootc-upstream/booted-with-update-no-version.json", None),
    (
        "bootc-upstream/booted-with-update.json",
        Some("boot-health/normal.json"),
    ),
    ("bootc-upstream/only-booted.json", None),
    ("bootc-upstream/ostree-remote.json", None),
    ("bootc-upstream/ostree-to-bootc.json", None),
    ("bootc-upstream/rfe-ostree-deployment.json", None),
    ("bootc-upstream/staged-booted.json", None),
    (
        "bootc-upstream/staged-download-only.json",
        Some("boot-health/normal.json"),
    ),
    ("bootc-upstream/via-local-oci.json", None),
];

const NOT_HOSTS: &[&str] = &[
    "bootc/container-base-image.json",
    "bootc-upstream/staged-rollback.json",
    "bootc-upstream/v1-null.json",
];

fn source(machine: &(&str, Option<&str>)) -> Vec<&'static str> {
    let mut arguments = vec!["--from", path(machine.0)];
    if let Some(health) = machine.1 {
        arguments.extend(["--boot-health", path(health)]);
    }
    arguments
}

fn with<'a>(head: &[&'a str], machine: &[&'a str], tail: &[&'a str]) -> Vec<&'a str> {
    let mut arguments = head.to_vec();
    arguments.extend_from_slice(machine);
    arguments.extend_from_slice(tail);
    arguments
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

#[test]
fn every_report_of_every_machine_matches_the_schema() {
    let schema = schema();
    let favourable = [
        "--power",
        "mains",
        "--network",
        "online",
        "--metered",
        "no",
        "--free-bytes",
        "214748364800",
    ];
    let harsh = [
        "--power",
        "battery",
        "--battery-percent",
        "9",
        "--network",
        "offline",
        "--free-bytes",
        "0",
        "--update-bytes",
        "3000000000",
        "--repair-session",
        "--no-automatic-download",
        "--download-on-metered",
    ];
    let mut documents = 0;
    for machine in MACHINES {
        let from = source(machine);
        let mut commands: Vec<(Vec<&str>, &str)> = vec![
            (with(&["status"], &from, &["--json"]), "status"),
            (with(&["check"], &from, &["--json"]), "check"),
            (with(&["plan", "rollback"], &from, &["--json"]), "plan"),
            (with(&["plan", "pin"], &from, &["--json"]), "plan"),
            (
                with(
                    &["plan", "pin"],
                    &from,
                    &["--json", "--deployment", "rollback"],
                ),
                "plan",
            ),
            (with(&["plan", "unpin"], &from, &["--json"]), "plan"),
            (
                with(
                    &["plan", "unpin"],
                    &from,
                    &["--json", "--deployment", "rollback"],
                ),
                "plan",
            ),
        ];
        for conditions in [&favourable[..], &harsh[..]] {
            let mut tail = vec!["--json"];
            tail.extend_from_slice(conditions);
            commands.push((with(&["check"], &from, &tail), "check"));
            commands.push((with(&["plan", "stage"], &from, &tail), "plan"));
            commands.push((with(&["plan", "rollback"], &from, &tail), "plan"));
            tail.push("--requested");
            commands.push((with(&["plan", "stage"], &from, &tail), "plan"));
        }
        for (arguments, kind) in commands {
            let document = json(&arguments);
            assert_valid(&schema, &document, &format!("{arguments:?}"));
            assert_eq!(document["kind"], kind, "{arguments:?}");
            assert_eq!(document["schema_version"], 1, "{arguments:?}");
            assert_eq!(document["source"], "file", "{arguments:?}");
            if kind == "plan" {
                // Nothing was run, and steps exist only for a decision to proceed.
                let plan = &document["plan"];
                assert_eq!(plan["executed"], false, "{arguments:?}");
                let proceeds = plan["decision"]["outcome"] == "proceed";
                let steps = plan["steps"].as_array().expect("steps");
                assert_eq!(steps.is_empty(), !proceeds, "{arguments:?}");
            }
            documents += 1;
        }
    }
    assert_eq!(documents, 16 * 15);
    for arguments in [["commands", "--json"], ["codes", "--json"]] {
        let document = json(&arguments);
        assert_valid(&schema, &document, &format!("{arguments:?}"));
    }
}

#[test]
fn the_validator_rejects_what_the_schema_forbids() {
    let schema = schema();
    let good = json(&with(
        &["plan", "stage"],
        &source(&(
            "bootc-upstream/booted-with-update.json",
            Some("boot-health/normal.json"),
        )),
        &["--json", "--power", "mains", "--network", "online"],
    ));
    assert_valid(&schema, &good, "the unmodified document");
    assert_eq!(good["plan"]["decision"]["outcome"], "proceed");
    let spoil = |change: &dyn Fn(&mut Value)| {
        let mut document = good.clone();
        change(&mut document);
        schema_errors(&schema, &document)
    };
    // A plan that claims to have been carried out.
    assert!(!spoil(&|d| d["plan"]["executed"] = Value::Bool(true)).is_empty());
    // A code of another family, an outcome and a step that do not exist.
    assert!(!spoil(&|d| d["plan"]["decision"]["code"] = "TUR-STORAGE-NTFS-004".into()).is_empty());
    assert!(!spoil(&|d| d["plan"]["decision"]["outcome"] = "force".into()).is_empty());
    assert!(!spoil(&|d| d["plan"]["steps"][0]["id"] = "reboot".into()).is_empty());
    // A member too many, and a member missing.
    assert!(!spoil(&|d| d["plan"]["extra"] = Value::Null).is_empty());
    assert!(
        !spoil(&|d| {
            d["plan"]
                .as_object_mut()
                .expect("an object")
                .remove("effects");
        })
        .is_empty()
    );
    // Another kind, another version.
    assert!(!spoil(&|d| d["kind"] = "status".into()).is_empty());
    assert!(!spoil(&|d| d["schema_version"] = 2.into()).is_empty());
    // A schema that uses a keyword the validator does not know is an error,
    // not a rule silently skipped.
    let mut unknown = schema.clone();
    unknown["maxProperties"] = 3.into();
    assert!(
        schema_errors(&unknown, &good)
            .iter()
            .any(|error| error.contains("maxProperties"))
    );
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

#[test]
fn status_lists_the_versions_and_marks_the_one_that_failed() {
    let machine = (
        "bootc/tur-after-fallback.json",
        Some("boot-health/after-fallback.json"),
    );
    let report = json(&with(&["status"], &source(&machine), &["--json"]));
    assert_eq!(report["state"], "not-checked");
    assert_eq!(
        report["message"],
        "TUR has not checked for updates yet. Version 0.1.0-dev."
    );
    let versions = report["versions"].as_array().expect("versions");
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["slot"], "booted");
    assert_eq!(versions[0]["name"], "TUR 0.1.0-dev");
    assert_eq!(versions[0]["failed_to_start"], false);
    assert_eq!(versions[0]["built"], "2026-11-12T11:23:57.270584439Z");
    assert_eq!(versions[1]["slot"], "rollback");
    assert_eq!(versions[1]["failed_to_start"], true);
    assert_eq!(
        versions[1]["digest"],
        "sha256:d726caf08260b02710b5c7d954b82465a40af81b5f79ccd0b69497906d7d4da4"
    );
    assert_eq!(report["boot_health"]["notice"]["kind"], "fallback");
    assert_eq!(report["boot_health"]["rejected"][0]["version"], "0.1.0-dev");
    assert_eq!(report["system"]["rollback"]["image"]["transport"], "oci");

    // The same, for a person.
    let text = ok(&with(&["status"], &source(&machine), &[]));
    let expected = "\
TUR has not checked for updates yet. Version 0.1.0-dev.
State:    not-checked
Follows:  localhost/tur-boot-test:ci (registry), signature: none

Versions on this computer
  in use    TUR 0.1.0-dev  sha256:864f59815ad6…  built 2026-11-12T11:23:57.270584439Z
  previous  TUR 0.1.0-dev  sha256:d726caf08260…  built 2026-11-12T11:27:09.488245657Z  [failed to start here]

Boot health
  Attempt counting:   usable
  Trial in progress:  no
  This start:         degraded
  Failed to start:    1 version(s)
";
    assert!(text.starts_with(expected), "{text}");
    assert!(
        text.contains("  Notice:             The update to TUR 0.1.0-dev"),
        "{text}"
    );
    assert!(text.trim_end().ends_with("(TUR-BOOT-TRIAL-003)"), "{text}");

    // Without the boot-health state, nothing is claimed about it.
    let report = json(&["status", "--from", path(machine.0), "--json"]);
    assert!(report["boot_health"].is_null());
    assert!(report["versions"][1]["failed_to_start"].is_null());
    let text = ok(&["status", "--from", path(machine.0)]);
    assert!(
        text.ends_with("Boot health: not known (give --boot-health)\n"),
        "{text}"
    );
    assert!(!text.contains("failed to start here"), "{text}");
}

#[test]
fn status_shows_what_is_waiting_kept_and_held() {
    // A staged version that failed here before is marked, wherever it stands.
    let text = ok(&[
        "status",
        "--from",
        path("bootc/tur-staged.json"),
        "--boot-health",
        path("boot-health/after-fallback.json"),
    ]);
    assert!(
        text.contains(
            "\n  waiting   TUR 0.1.0-dev  sha256:d726caf08260…  built \
             2026-11-12T11:27:09.488245657Z  [failed to start here]\n"
        ),
        "{text}"
    );
    assert_eq!(text.matches("failed to start here").count(), 1, "{text}");

    let text = ok(&["status", "--from", path("bootc/tur-staged.json")]);
    assert!(text.starts_with("TUR 0.1.0-dev is ready. Restart to use it.\nState:    ready\n"));
    assert!(text.contains("Follows:  /run/tur-test/images:check-fails (oci), signature: none\n"));
    assert!(
        text.contains("\n  waiting   TUR 0.1.0-dev  sha256:d726caf08260…"),
        "{text}"
    );
    assert!(
        text.contains("\n  in use    TUR 0.1.0-dev  sha256:864f59815ad6…"),
        "{text}"
    );

    let text = ok(&[
        "status",
        "--from",
        path("bootc-upstream/booted-pinned.json"),
    ]);
    assert!(text.contains("\n  in use    TUR stream9.20240807.0  sha256:47e5ed613a97…  [kept]\n"));
    assert!(text.contains("\n  other     TUR stream9.20240807.0  sha256:47e5ed613a97…  [kept]\n"));

    let text = ok(&[
        "status",
        "--from",
        path("bootc-upstream/staged-download-only.json"),
    ]);
    assert!(text.starts_with("TUR nightly has been downloaded and is waiting to be released.\n"));
    assert!(text.contains("State:    held\n"), "{text}");
    assert!(text.contains("signature: insecure\n"), "{text}");
    assert!(text.contains("[held]\n"), "{text}");

    let text = ok(&[
        "status",
        "--from",
        path("bootc-upstream/booted-usroverlay.json"),
    ]);
    assert!(
        text.contains("An overlay is mounted on /usr (transient, readWrite).\n"),
        "{text}"
    );

    let text = ok(&[
        "status",
        "--from",
        path("bootc-upstream/rfe-ostree-deployment.json"),
    ]);
    assert!(text.contains("Follows:  no image\n"), "{text}");
    assert!(
        text.contains("  in use    a version that is not an image (f9fa3a553cea)  not an image\n")
    );
}

// ---------------------------------------------------------------------------
// check
// ---------------------------------------------------------------------------

#[test]
fn check_previews_the_check_and_what_follows_it() {
    let machine = (
        "bootc-upstream/booted-with-update.json",
        Some("boot-health/normal.json"),
    );
    let from = source(&machine);
    let favourable = [
        "--power",
        "mains",
        "--network",
        "online",
        "--free-bytes",
        "99999999999",
    ];
    let report = json(&with(
        &["check"],
        &from,
        &with(&["--json"], &favourable, &[]),
    ));
    assert_eq!(report["state"], "available");
    assert_eq!(report["update"]["status"], "available");
    assert_eq!(report["update"]["image"]["version"], "stream9.20240901.0");
    assert_eq!(report["check"]["outcome"], "proceed");
    assert_eq!(report["check"]["code"], "TUR-UPDATE-CHECK-001");
    assert_eq!(
        report["check_command"],
        serde_json::json!(["/usr/bin/bootc", "upgrade", "--check"])
    );
    assert_eq!(report["automatic_action"], "stage");
    assert_eq!(report["automatic_stage"]["code"], "TUR-UPDATE-STAGE-001");
    assert_eq!(report["requested_stage"]["code"], "TUR-UPDATE-STAGE-001");
    // The conditions are the ones given, and nothing else was measured.
    assert_eq!(report["conditions"]["power"]["source"], "mains");
    assert_eq!(report["conditions"]["network"]["state"], "online");
    assert_eq!(report["conditions"]["free_bytes"], 99_999_999_999_u64);
    assert_eq!(report["conditions"]["boot_health"]["counting_usable"], true);

    // On battery the update manager waits; a person does not have to.
    let report = json(&with(
        &["check"],
        &from,
        &[
            "--json",
            "--power",
            "battery",
            "--battery-percent",
            "30",
            "--network",
            "online",
        ],
    ));
    assert_eq!(report["automatic_action"], "wait");
    assert_eq!(report["automatic_stage"]["code"], "TUR-UPDATE-POWER-001");
    assert_eq!(report["requested_stage"]["outcome"], "proceed");

    // Offline: the desktop's sentence.
    let text = ok(&with(&["check"], &from, &["--network", "offline"]));
    assert!(
        text.contains(
            "Check for updates: wait (TUR-UPDATE-NETWORK-001)\n  TUR cannot check for updates \
             without a network connection. Nothing is wrong.\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("By itself, the update manager would now: wait\n"),
        "{text}"
    );
    assert!(
        text.contains("  A check would run: /usr/bin/bootc upgrade --check\n"),
        "{text}"
    );
    assert!(text.contains("  This command does not run it.\n"), "{text}");

    // Without boot health an update is not staged, by anyone.
    let report = json(&["check", "--from", path(machine.0), "--json"]);
    assert_eq!(report["automatic_action"], "refuse");
    assert_eq!(report["automatic_stage"]["code"], "TUR-UPDATE-HEALTH-004");
    assert_eq!(report["requested_stage"]["code"], "TUR-UPDATE-HEALTH-004");
    assert!(report["conditions"]["boot_health"].is_null());
    // ... and a check may still run.
    assert_eq!(report["check"]["outcome"], "proceed");
}

// ---------------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------------

#[test]
fn plan_stage_prints_the_commands_and_runs_none() {
    let machine = (
        "bootc-upstream/booted-with-update.json",
        Some("boot-health/normal.json"),
    );
    let from = source(&machine);
    let conditions = [
        "--power",
        "mains",
        "--network",
        "online",
        "--free-bytes",
        "99999999999",
    ];
    let text = ok(&with(&["plan", "stage"], &from, &conditions));
    let expected_head = "\
Plan: stage (request: automatic)
Decision: proceed (TUR-UPDATE-STAGE-001)
  The update can be downloaded and prepared.
  Reason: TUR stream9.20240901.0 (sha256:a0a0a0a0a0a0…) can be staged beside TUR stream9.20240807.0
  Note: No signature verification is configured for the image this system follows (quay.io/centos-bootc/centos-bootc:stream9; signature: none).

This command runs nothing. These commands would be run, as administrator, in this order:
  1. /usr/bin/bootc upgrade --download-only --quiet
";
    assert!(text.starts_with(expected_head), "{text}");
    assert!(
        text.contains("\n  2. /usr/bin/bootc status --format=json\n"),
        "{text}"
    );
    assert!(
        text.contains("\n  3. /usr/bin/bootc upgrade --from-downloaded\n"),
        "{text}"
    );
    assert!(
        text.contains("\nWhat would happen\n  - TUR stream9.20240901.0 is downloaded"),
        "{text}"
    );
    assert!(
        text.contains("It gets 3 attempts to start properly"),
        "{text}"
    );
    assert!(
        text.ends_with(
            "\nThe change takes effect at a restart. No command here restarts the computer.\n"
        ),
        "{text}"
    );

    let report = json(&with(
        &["plan", "stage"],
        &from,
        &with(&["--json"], &conditions, &[]),
    ));
    assert_eq!(report["requester"], "automatic");
    assert_eq!(report["plan"]["operation"], "stage");
    assert_eq!(report["plan"]["restart_required"], true);
    let steps = report["plan"]["steps"].as_array().expect("steps");
    let ids: Vec<&str> = steps
        .iter()
        .filter_map(|step| step["id"].as_str())
        .collect();
    assert_eq!(ids, ["stage-held", "read-status", "release"]);
    assert_eq!(steps[0]["changes"], "system");
    assert_eq!(steps[1]["changes"], "nothing");

    // A person asking on a metered connection is told, and may go ahead.
    let report = json(&with(
        &["plan", "stage"],
        &from,
        &[
            "--json",
            "--requested",
            "--network",
            "online",
            "--metered",
            "yes",
        ],
    ));
    assert_eq!(report["requester"], "person");
    assert_eq!(report["plan"]["decision"]["outcome"], "proceed");
    assert_eq!(
        report["plan"]["decision"]["notes"][0],
        "The connection is metered."
    );

    // Held back: the reason, what to do, everything else in the way, and no
    // command.
    let text = ok(&with(
        &["plan", "stage"],
        &from,
        &[
            "--network",
            "offline",
            "--power",
            "battery",
            "--battery-percent",
            "12",
            "--free-bytes",
            "1000",
            "--repair-session",
        ],
    ));
    assert!(
        text.contains("Decision: refuse (TUR-UPDATE-SPACE-001)\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "  The update was not applied. There is not enough free space on this computer.\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("  Reason: 1000 bytes are free on the system disk; "),
        "{text}"
    );
    assert!(
        text.contains("  What to do: Free some space, or release a version"),
        "{text}"
    );
    assert!(
        text.contains(
            "  Also: TUR-UPDATE-SESSION-001, TUR-UPDATE-NETWORK-001, TUR-UPDATE-POWER-001\n"
        ),
        "{text}"
    );
    assert!(text.ends_with("\nNo command would be run.\n"), "{text}");
    assert!(!text.contains("/usr/bin/"), "{text}");
}

#[test]
fn plan_rollback_refuses_the_version_that_failed_and_allows_the_one_that_worked() {
    let failed = (
        "bootc/tur-after-fallback.json",
        Some("boot-health/after-fallback.json"),
    );
    let text = ok(&with(&["plan", "rollback"], &source(&failed), &[]));
    assert!(text.starts_with("Plan: rollback\nDecision: refuse (TUR-UPDATE-ROLLBACK-003)\n"));
    assert!(
        text.contains(
            "  TUR will not go back to that version: it did not start properly on this computer.\n"
        ),
        "{text}"
    );
    assert!(text.ends_with("\nNo command would be run.\n"), "{text}");

    let worked = (
        "bootc/tur-after-update.json",
        Some("boot-health/after-update.json"),
    );
    let text = ok(&with(&["plan", "rollback"], &source(&worked), &[]));
    assert!(
        text.contains("Decision: proceed (TUR-UPDATE-ROLLBACK-001)\n"),
        "{text}"
    );
    assert!(text.contains("\n  1. /usr/bin/bootc rollback\n"), "{text}");
    assert!(text.contains("each version has its own /etc"), "{text}");
    assert!(!text.contains("\n  2. "), "{text}");
    let report = json(&with(&["plan", "rollback"], &source(&worked), &["--json"]));
    assert_eq!(report["requester"], "person");
    assert_eq!(
        report["plan"]["steps"][0]["command"],
        serde_json::json!(["/usr/bin/bootc", "rollback"])
    );

    // No previous version at all.
    let report = json(&[
        "plan",
        "rollback",
        "--from",
        path("bootc/tur-booted.json"),
        "--json",
    ]);
    assert_eq!(
        report["plan"]["decision"]["code"],
        "TUR-UPDATE-ROLLBACK-002"
    );
    assert_eq!(report["plan"]["steps"], serde_json::json!([]));
}

#[test]
fn plan_pin_and_unpin_name_the_deployment_by_its_role() {
    let updated = path("bootc/tur-after-update.json");
    let text = ok(&["plan", "pin", "--from", updated]);
    assert!(
        text.starts_with("Plan: pin\nDecision: proceed (TUR-UPDATE-PIN-001)\n"),
        "{text}"
    );
    assert!(
        text.contains("\n  1. /usr/bin/ostree admin pin booted\n"),
        "{text}"
    );
    assert!(!text.contains("restart"), "{text}");
    let text = ok(&["plan", "pin", "--from", updated, "--deployment", "rollback"]);
    assert!(
        text.contains("\n  1. /usr/bin/ostree admin pin rollback\n"),
        "{text}"
    );
    let report = json(&["plan", "unpin", "--from", updated, "--json"]);
    assert_eq!(report["plan"]["decision"]["code"], "TUR-UPDATE-PIN-005");
    assert_eq!(report["plan"]["decision"]["outcome"], "nothing");

    let kept = path("bootc-upstream/booted-pinned.json");
    let report = json(&["plan", "unpin", "--from", kept, "--json"]);
    assert_eq!(
        report["plan"]["steps"][0]["command"],
        serde_json::json!(["/usr/bin/ostree", "admin", "pin", "--unpin", "booted"])
    );
    let report = json(&["plan", "pin", "--from", kept, "--json"]);
    assert_eq!(report["plan"]["decision"]["code"], "TUR-UPDATE-PIN-002");
    let report = json(&[
        "plan",
        "pin",
        "--from",
        kept,
        "--json",
        "--deployment",
        "rollback",
    ]);
    assert_eq!(report["plan"]["decision"]["code"], "TUR-UPDATE-PIN-003");
}

// ---------------------------------------------------------------------------
// commands and codes
// ---------------------------------------------------------------------------

#[test]
fn commands_lists_everything_the_tool_names_and_what_it_runs() {
    let report = json(&["commands", "--json"]);
    let commands = report["commands"].as_array().expect("commands");
    assert_eq!(commands.len(), 7);
    let run: Vec<&Value> = commands
        .iter()
        .filter(|command| command["run_by_this_program"] == true)
        .collect();
    assert_eq!(run.len(), 1);
    assert_eq!(run[0]["id"], "read-status");
    assert_eq!(run[0]["changes"], "nothing");
    assert_eq!(run[0]["program"], "/usr/bin/bootc");
    assert_eq!(run[0]["subcommand"], serde_json::json!(["status"]));
    assert_eq!(run[0]["options"], serde_json::json!(["--format=json"]));
    let text = ok(&["commands"]);
    assert!(text.contains(
        "/usr/bin/bootc status --format=json\n  read-status: changes nothing; run by tur-update\n"
    ));
    assert!(text.contains("/usr/bin/ostree admin pin --unpin <booted|rollback>\n  unpin: changes system; never run by tur-update\n"));
    assert_eq!(text.matches("never run by tur-update").count(), 6);
}

#[test]
fn codes_prints_the_registry() {
    let report = json(&["codes", "--json"]);
    let codes = report["codes"].as_array().expect("codes");
    assert_eq!(codes.len(), 38);
    assert_eq!(codes[0]["code"], "TUR-UPDATE-STATE-001");
    let offline = codes
        .iter()
        .find(|entry| entry["code"] == "TUR-UPDATE-NETWORK-001")
        .expect("the offline code");
    assert_eq!(
        offline["user_message"],
        "TUR cannot check for updates without a network connection. Nothing is wrong."
    );
    assert_eq!(offline["severity"], "notice");
    let text = ok(&["codes"]);
    assert!(text.starts_with("TUR-UPDATE-STATE-001 [error] System state could not be read\n"));
    assert_eq!(text.matches("\n  What to do: ").count(), 38);
}

// ---------------------------------------------------------------------------
// Failures and the command line
// ---------------------------------------------------------------------------

#[test]
fn a_machine_that_is_not_a_bootc_host_is_a_coded_failure() {
    for name in NOT_HOSTS {
        for command in [
            vec!["status"],
            vec!["check"],
            vec!["plan", "stage"],
            vec!["plan", "rollback"],
            vec!["plan", "pin"],
        ] {
            let result = run(&with(&command, &["--from", path(name)], &["--json"]));
            assert_eq!(result.status, 1, "{name} {command:?}");
            assert!(result.stdout.is_empty(), "{name} {command:?}");
            assert!(
                result.stderr.starts_with(
                    "TUR-UPDATE-STATE-002\nThis system does not receive TUR updates.\n\nAction:\n"
                ),
                "{}",
                result.stderr
            );
            assert!(result.stderr.contains(name), "{}", result.stderr);
        }
    }
}

#[test]
fn input_that_cannot_be_read_is_a_coded_failure_and_nothing_is_printed() {
    let good = path("bootc/tur-booted.json");
    let cases: Vec<(Vec<&str>, &str)> = vec![
        // No such file.
        (
            vec!["status", "--from", "/nonexistent/bootc.json"],
            "TUR-UPDATE-STATE-001",
        ),
        // Not a bootc document: a boot-health document, the provenance file.
        (
            vec!["status", "--from", path("boot-health/normal.json")],
            "TUR-UPDATE-STATE-001",
        ),
        (
            vec!["check", "--from", path("provenance.json")],
            "TUR-UPDATE-STATE-001",
        ),
        // A directory.
        (vec!["status", "--from", FIXTURES], "TUR-UPDATE-STATE-001"),
        // The boot-health file is missing, or is not a boot-health document.
        (
            vec![
                "status",
                "--from",
                good,
                "--boot-health",
                "/nonexistent/h.json",
            ],
            "TUR-UPDATE-STATE-007",
        ),
        (
            vec!["check", "--from", good, "--boot-health", good],
            "TUR-UPDATE-STATE-007",
        ),
        (
            vec![
                "plan",
                "stage",
                "--from",
                good,
                "--boot-health",
                path("boot-health/rejected.json"),
            ],
            "TUR-UPDATE-STATE-007",
        ),
        (
            vec!["plan", "pin", "--from", good, "--boot-health", good],
            "TUR-UPDATE-STATE-007",
        ),
    ];
    for (arguments, code) in cases {
        let result = run(&arguments);
        assert_eq!(result.status, 1, "{arguments:?}");
        assert!(result.stdout.is_empty(), "{arguments:?}");
        assert!(
            result.stderr.starts_with(&format!("{code}\n")),
            "{arguments:?}: {}",
            result.stderr
        );
        assert!(
            result.stderr.contains("\nTechnical detail:\n"),
            "{arguments:?}"
        );
    }
}

#[test]
fn usage_errors_exit_with_two() {
    let good = path("bootc/tur-booted.json");
    let cases: Vec<Vec<&str>> = vec![
        vec![],
        vec!["status"],
        vec!["status", "--from", good, "--live"],
        vec!["check", "--from", good, "--power", "solar"],
        vec!["check", "--from", good, "--battery-percent", "101"],
        vec!["check", "--from", good, "--battery-percent", "50"],
        vec![
            "check",
            "--from",
            good,
            "--network",
            "offline",
            "--metered",
            "yes",
        ],
        vec!["check", "--from", good, "--free-bytes", "-1"],
        vec!["plan"],
        vec!["plan", "stage"],
        vec!["plan", "reboot", "--from", good],
        vec!["plan", "pin", "--from", good, "--deployment", "staged"],
        vec!["plan", "pin", "--from", good, "--deployment", "0"],
        vec!["plan", "rollback", "--from", good, "--requested"],
        vec!["stage", "--from", good],
        vec!["rollback"],
        vec!["status", "--from", good, "--apply"],
    ];
    for arguments in cases {
        let result = run(&arguments);
        assert_eq!(result.status, 2, "{arguments:?}: {}", result.stderr);
        assert!(result.stdout.is_empty(), "{arguments:?}");
        assert!(!result.stderr.is_empty(), "{arguments:?}");
    }
}

#[test]
fn help_and_version_say_that_nothing_is_changed() {
    // Whatever width the help is wrapped to.
    let flat = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let help = ok(&["--help"]);
    assert!(
        flat(&help).contains("It never changes the system."),
        "{help}"
    );
    for subcommand in ["status", "check", "plan", "commands", "codes"] {
        assert!(
            help.lines()
                .any(|line| line.split_whitespace().next() == Some(subcommand)),
            "{subcommand}:\n{help}"
        );
    }
    let plan = ok(&["plan", "--help"]);
    for operation in ["stage", "rollback", "pin", "unpin"] {
        assert!(
            plan.lines()
                .any(|line| line.split_whitespace().next() == Some(operation)),
            "{operation}:\n{plan}"
        );
    }
    assert!(flat(&plan).contains("without running them"), "{plan}");
    let status = ok(&["status", "--help"]);
    assert!(status.contains("--live"), "{status}");
    assert!(
        flat(&status).contains("Nothing else is run and nothing is changed"),
        "{status}"
    );
    assert_eq!(ok(&["--version"]), "tur-update 0.1.0\n");
}

#[test]
fn the_surroundings_of_the_process_change_nothing() {
    let arguments = [
        "check",
        "--from",
        path("bootc-upstream/booted-with-update.json"),
        "--boot-health",
        path("boot-health/normal.json"),
        "--json",
    ];
    let plain = run(&arguments);
    assert_eq!(plain.status, 0, "{}", plain.stderr);
    let noisy = run_with_environment(
        &arguments,
        &[
            ("TUR_UPDATE_POWER", "battery"),
            ("TUR_UPDATE_NETWORK", "offline"),
            ("POWER", "battery"),
            ("NETWORK", "offline"),
            ("FREE_BYTES", "0"),
            ("TUR_UPDATE_LIVE", "1"),
            ("PATH", "/nonexistent"),
            ("HOME", "/nonexistent"),
            ("LANG", "tr_TR.UTF-8"),
            ("LC_ALL", "tr_TR.UTF-8"),
            ("TZ", "Pacific/Kiritimati"),
        ],
    );
    assert_eq!(noisy.status, 0, "{}", noisy.stderr);
    assert_eq!(noisy.stdout, plain.stdout);
    assert_eq!(noisy.stderr, plain.stderr);
}
