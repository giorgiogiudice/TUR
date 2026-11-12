// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Tests that run the built `tur-storage` binary on the fixtures and check
//! its JSON output against `schemas/storage-volume.schema.json`.
//!
//! The schema check uses a small validator written here, which understands
//! exactly the keywords the schema uses and fails on any other, so that the
//! schema cannot grow a rule these tests silently skip.

use std::fs;
use std::process::Command;

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_tur-storage");
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
const SCHEMA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../schemas/storage-volume.schema.json"
);

const WINDOWS: &str = "fs-01d9a7c2e4f6b801";
const APFS: &str = "fs-0a0a0a0a-3333-4bbb-8ccc-000000000a03";

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
        .expect("the tur-storage binary starts");
    Run {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    }
}

fn run(args: &[&str]) -> Run {
    run_with_environment(args, &[])
}

fn fixture(name: &str) -> String {
    format!("{FIXTURES}/{name}")
}

/// A fixture path that lives as long as the test process, so that it can sit
/// in an argument list beside string literals.
fn path(name: &str) -> &'static str {
    Box::leak(fixture(name).into_boxed_str())
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

/// The command-line form of one point of the policy override space.
fn policy_flags(bits: u8) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if bits & 1 != 0 {
        flags.extend(["--ntfs-unsafe", "refuse"]);
    }
    if bits & 2 != 0 {
        flags.push("--hfsplus-write");
    }
    if bits & 4 != 0 {
        flags.push("--apfs-read-only");
    }
    if bits & 8 != 0 {
        flags.push("--noexec-removable");
    }
    flags
}

// ---------------------------------------------------------------------------
// A validator for the subset of JSON Schema that the storage schema uses.
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
    "maxLength",
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

fn is_volume_id(text: &str) -> bool {
    ["fs-", "part-", "dev-", "tmp-"].iter().any(|prefix| {
        text.strip_prefix(prefix).is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    })
}

fn is_storage_code(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("TUR-STORAGE-") else {
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
        "^(fs|part|dev|tmp)-[a-z0-9-]+$" => Some(is_volume_id(text)),
        "^TUR-STORAGE-[A-Z0-9]+-[0-9]{3}$" => Some(is_storage_code(text)),
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
        if let Some(maximum) = schema.get("maxLength").and_then(Value::as_u64)
            && length > maximum
        {
            errors.push(format!("{path}: longer than {maximum}"));
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

/// Every fixture with each of its property files (or none).
fn inputs() -> Vec<Vec<String>> {
    let sets: [(&str, &[&str]); 4] = [
        (
            "dual-boot.json",
            &[
                "dual-boot.clean.props",
                "dual-boot.hibernated.props",
                "dual-boot.dirty.props",
            ],
        ),
        ("removable.json", &["removable.props"]),
        ("linux-data.json", &["linux-data.props"]),
        ("foreign.json", &["foreign.props"]),
    ];
    let mut all = Vec::new();
    for (lsblk, properties) in sets {
        all.push(vec!["--from".to_owned(), fixture(lsblk)]);
        for file in properties {
            all.push(vec![
                "--from".to_owned(),
                fixture(lsblk),
                "--props".to_owned(),
                fixture(file),
            ]);
        }
    }
    all
}

fn with<'a>(command: &'a str, input: &'a [String], extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![command];
    args.extend(input.iter().map(String::as_str));
    args.extend_from_slice(extra);
    args
}

// ---------------------------------------------------------------------------
// Schema conformance
// ---------------------------------------------------------------------------

#[test]
fn inventory_json_conforms_to_the_schema_under_every_override() {
    let schema = schema();
    let mut documents = 0;
    for input in inputs() {
        for bits in 0..16 {
            let mut extra = policy_flags(bits);
            extra.extend(["--json", "--user", "jay"]);
            let document = json(&with("volumes", &input, &extra));
            assert_valid(&schema, &document, &format!("{input:?} {extra:?}"));
            assert_eq!(document["kind"], "inventory");
            assert_eq!(document["schema_version"], 1);
            documents += 1;
        }
    }
    assert_eq!(documents, 10 * 16);
}

#[test]
fn decision_json_conforms_to_the_schema_for_every_volume() {
    let schema = schema();
    let mut documents = 0;
    for input in inputs() {
        let inventory = json(&with("volumes", &input, &["--json"]));
        for volume in inventory["volumes"].as_array().unwrap() {
            let id = volume["id"].as_str().unwrap();
            for access in ["ro", "rw"] {
                let extra = ["--volume", id, "--access", access, "--json"];
                let document = json(&with("decide", &input, &extra));
                assert_valid(&schema, &document, &format!("{input:?} {id} {access}"));
                assert_eq!(document["kind"], "decision");
                assert_eq!(document["volume"]["id"], id);
                documents += 1;
            }
        }
    }
    assert!(documents > 150, "{documents}");
}

#[test]
fn capability_json_conforms_to_the_schema() {
    let schema = schema();
    let names = [
        "ext4", "btrfs", "xfs", "ntfs", "exfat", "vfat", "hfsplus", "apfs", "iso9660", "refs",
    ];
    for source in names {
        for destination in names {
            let args = [
                "capabilities",
                "--from-fs",
                source,
                "--to-fs",
                destination,
                "--json",
            ];
            let document = json(&args);
            assert_valid(&schema, &document, &format!("{source} -> {destination}"));
            assert_eq!(document["kind"], "capabilities");
        }
    }
}

#[test]
fn the_validator_rejects_documents_that_break_the_schema() {
    let schema = schema();
    let input = vec!["--from".to_owned(), fixture("removable.json")];
    let good = json(&with("volumes", &input, &["--json"]));
    assert_valid(&schema, &good, "baseline");

    let mut missing = good.clone();
    missing["volumes"][0]
        .as_object_mut()
        .unwrap()
        .remove("decision");
    assert!(!schema_errors(&schema, &missing).is_empty());

    let mut extra = good.clone();
    extra["volumes"][0]["force"] = Value::Bool(true);
    assert!(!schema_errors(&schema, &extra).is_empty());

    let mut wrong_enum = good.clone();
    wrong_enum["volumes"][0]["decision"]["outcome"] = Value::from("mount-anyway");
    assert!(!schema_errors(&schema, &wrong_enum).is_empty());

    let mut wrong_id = good.clone();
    wrong_id["volumes"][0]["id"] = Value::from("../../etc");
    assert!(!schema_errors(&schema, &wrong_id).is_empty());

    let mut wrong_code = good.clone();
    wrong_code["volumes"][0]["decision"]["code"] = Value::from("TUR-BOOT-001");
    assert!(!schema_errors(&schema, &wrong_code).is_empty());

    let mut wrong_type = good.clone();
    wrong_type["volumes"][0]["size_bytes"] = Value::from("large");
    assert!(!schema_errors(&schema, &wrong_type).is_empty());

    let mut wrong_version = good;
    wrong_version["schema_version"] = Value::from(2);
    assert!(!schema_errors(&schema, &wrong_version).is_empty());

    assert!(!schema_errors(&schema, &Value::from("not a document")).is_empty());
}

// ---------------------------------------------------------------------------
// Behaviour
// ---------------------------------------------------------------------------

#[test]
fn volumes_text_lists_each_volume_with_its_decision() {
    let stdout = ok(&[
        "volumes",
        "--from",
        path("dual-boot.json"),
        "--props",
        path("dual-boot.hibernated.props"),
        "--user",
        "jay",
    ]);
    assert!(
        stdout.contains("fs-01d9a7c2e4f6b801\n  Name:        Windows\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("  Decision:    mount-read-only (TUR-STORAGE-NTFS-004)\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("  Mount path:  /run/tur/volumes/fs-01d9a7c2e4f6b801\n"),
        "{stdout}"
    );
    assert!(stdout.contains("  View:        linux /run/media/jay/Windows (bind, read-only)\n"));
    assert!(stdout.contains("  View:        windows dosdevices/d: (symlink, read-only)\n"));
    assert!(stdout.contains("  View:        macos /Volumes/Windows (bind, read-only)\n"));
    assert!(stdout.contains(
        "  View:        android /storage/emulated/0/Volumes/Windows (grant, read-only, needs a \
         grant)\n"
    ));
    assert!(
        stdout.contains("  Decision:    refuse (TUR-STORAGE-SYSTEM-001)\n"),
        "{stdout}"
    );
    assert_eq!(stdout.matches("  Decision:    ").count(), 7);
    assert!(!stdout.contains("zram"));
}

#[test]
fn decide_prints_the_two_layer_message() {
    let stdout = ok(&[
        "decide",
        "--from",
        path("dual-boot.json"),
        "--props",
        path("dual-boot.hibernated.props"),
        "--volume",
        WINDOWS,
    ]);
    assert!(
        stdout.starts_with(
            "TUR-STORAGE-NTFS-004\nThe Windows volume was not mounted for writing.\n\nReason:\n\
             The volume appears to be hibernated or in an unsafe state.\n\nAction:\nBoot \
             Windows, disable Fast Startup/hibernation, shut down fully, then retry. TUR mounts \
             the volume read-only to protect your data.\n\nTechnical detail:\nDevice: \
             /dev/disk/by-uuid/01D9A7C2E4F6B801\nDriver: ntfs3\nMount policy: read-only\nMount \
             options: ro,nosuid,nodev,uid=1000,gid=1000,dmask=0077,fmask=0177,iocharset=utf8\n"
        ),
        "{stdout}"
    );
    assert!(stdout.ends_with(
        "\nVolume: fs-01d9a7c2e4f6b801\nMount path: /run/tur/volumes/fs-01d9a7c2e4f6b801\n"
    ));
}

#[test]
fn decide_defaults_to_read_write_and_honours_a_read_only_request() {
    let base = [
        "decide",
        "--from",
        path("dual-boot.json"),
        "--props",
        path("dual-boot.clean.props"),
        "--volume",
        WINDOWS,
        "--json",
        "--uid",
        "1234",
        "--gid",
        "5678",
    ];
    let default = json(&base);
    assert_eq!(default["volume"]["decision"]["outcome"], "mount-read-write");
    assert_eq!(
        default["volume"]["decision"]["code"],
        "TUR-STORAGE-MOUNT-001"
    );
    assert_eq!(
        default["volume"]["decision"]["requested_access"],
        "read-write"
    );
    let options: Vec<&str> = default["volume"]["decision"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        options,
        [
            "rw",
            "nosuid",
            "nodev",
            "uid=1234",
            "gid=5678",
            "dmask=0077",
            "fmask=0177",
            "iocharset=utf8",
            "windows_names"
        ]
    );

    let mut read_only = base.to_vec();
    read_only.extend(["--access", "ro"]);
    let requested = json(&read_only);
    assert_eq!(
        requested["volume"]["decision"]["outcome"],
        "mount-read-only"
    );
    assert_eq!(
        requested["volume"]["decision"]["code"],
        "TUR-STORAGE-MOUNT-002"
    );
    assert_eq!(requested["volume"]["decision"]["options"][0], "ro");
}

#[test]
fn an_unknown_volume_is_an_error_with_a_code() {
    let result = run(&[
        "decide",
        "--from",
        path("removable.json"),
        "--volume",
        "fs-does-not-exist",
    ]);
    assert_eq!(result.status, 1);
    assert!(result.stdout.is_empty());
    assert!(
        result.stderr.starts_with("TUR-STORAGE-VOLUME-001\n"),
        "{}",
        result.stderr
    );
    assert!(result.stderr.contains("fs-does-not-exist"));
}

#[test]
fn unusable_input_is_an_error_with_a_code() {
    let not_lsblk = path("not-lsblk.json");
    let human_sizes = path("human-sizes.json");
    let bad_props = path("malformed.props");
    let good = path("removable.json");
    let cases: [&[&str]; 4] = [
        &["volumes", "--from", not_lsblk],
        &["volumes", "--from", human_sizes],
        &["volumes", "--from", good, "--props", bad_props],
        &["volumes", "--from", "/nonexistent/lsblk.json", "--json"],
    ];
    for args in cases {
        let result = run(args);
        assert_eq!(result.status, 1, "{args:?}");
        assert!(result.stdout.is_empty(), "{args:?}");
        assert!(
            result.stderr.starts_with("TUR-STORAGE-PROBE-001\n"),
            "{args:?}: {}",
            result.stderr
        );
    }
    let result = run(&["volumes", "--from", human_sizes]);
    assert!(result.stderr.contains("465.8G"), "{}", result.stderr);
    let result = run(&["volumes", "--from", good, "--props", bad_props]);
    assert!(result.stderr.contains("line 3"), "{}", result.stderr);
}

#[test]
fn usage_errors_exit_with_status_two() {
    let from = path("removable.json");
    let cases: [&[&str]; 6] = [
        &[],
        &["volumes"],
        &["decide", "--from", from],
        &["volumes", "--from", from, "--user", "../root"],
        &["volumes", "--from", from, "--uid", "minus-one"],
        &["mount", "--from", from],
    ];
    for args in cases {
        let result = run(args);
        assert_eq!(result.status, 2, "{args:?}: {}", result.stderr);
        assert!(result.stdout.is_empty(), "{args:?}");
        assert!(!result.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn apfs_is_never_writable_through_the_command_line() {
    let from = path("foreign.json");
    let props = path("foreign.props");
    for bits in 0..16 {
        for access in ["ro", "rw"] {
            let mut args = vec!["decide", "--from", from, "--props", props, "--volume", APFS];
            args.extend(["--access", access, "--json"]);
            args.extend(policy_flags(bits));
            let document = json(&args);
            let decision = &document["volume"]["decision"];
            assert_ne!(decision["outcome"], "mount-read-write", "{args:?}");
            assert_ne!(decision["driver"], "linux-apfs-rw", "{args:?}");
            let options = decision["options"].as_array().unwrap();
            assert!(!options.contains(&Value::from("rw")), "{args:?}");
            if bits & 4 == 0 {
                assert_eq!(decision["outcome"], "unsupported", "{args:?}");
                assert_eq!(decision["code"], "TUR-STORAGE-APFS-002", "{args:?}");
                assert!(document["volume"]["mount"].is_null());
            } else {
                assert_eq!(decision["outcome"], "mount-read-only", "{args:?}");
                assert_eq!(decision["driver"], "apfs-fuse", "{args:?}");
                assert_eq!(options[0], "ro", "{args:?}");
                assert_eq!(document["volume"]["mount"]["read_only"], true);
            }
        }
    }
}

#[test]
fn flags_that_would_override_a_safety_check_do_not_exist() {
    let from = path("foreign.json");
    for flag in [
        "--force",
        "--apfs-write",
        "--apfs-read-write",
        "--apfs-experimental",
        "--experimental",
        "--remove-hiberfile",
        "--recover",
        "--ntfs-unsafe=read-write",
        "--ntfs-unsafe=force",
        "--access=force",
    ] {
        let result = run(&["decide", "--from", from, "--volume", APFS, flag]);
        assert_eq!(result.status, 2, "{flag} was accepted");
        assert!(result.stdout.is_empty(), "{flag}");
    }
    let help = ok(&["decide", "--help"]);
    for word in ["force", "experimental", "hiberfile", "recover"] {
        assert!(!help.to_lowercase().contains(word), "help mentions {word}");
    }
}

#[test]
fn a_hibernated_volume_is_never_writable_through_the_command_line() {
    let from = path("dual-boot.json");
    let props = path("dual-boot.hibernated.props");
    for bits in 0..16 {
        for access in ["ro", "rw"] {
            let mut args = vec![
                "decide", "--from", from, "--props", props, "--volume", WINDOWS,
            ];
            args.extend(["--access", access]);
            args.extend(policy_flags(bits));
            let text = ok(&args);
            assert!(!text.to_lowercase().contains("force"), "{args:?}\n{text}");
            assert!(!text.contains("remove_hiberfile"), "{args:?}");
            assert!(
                !text.contains("Mount policy: read/write"),
                "{args:?}\n{text}"
            );
            assert!(!text.contains("Mount options: rw"), "{args:?}\n{text}");

            args.push("--json");
            let document = json(&args);
            let outcome = document["volume"]["decision"]["outcome"]
                .as_str()
                .unwrap()
                .to_owned();
            let expected = if bits & 1 != 0 {
                "refuse"
            } else {
                "mount-read-only"
            };
            assert_eq!(outcome, expected, "{args:?}");
        }
    }
}

#[test]
fn the_environment_does_not_change_any_decision() {
    let hostile = [
        ("TUR_STORAGE_FORCE", "1"),
        ("TUR_STORAGE_APFS_WRITE", "1"),
        ("TUR_APFS_EXPERIMENTAL", "1"),
        ("TUR_STORAGE_NTFS_UNSAFE", "read-write"),
        ("APFS_WRITE", "1"),
        ("FORCE", "1"),
        ("HOME", "/nonexistent"),
        ("LANG", "tr_TR.UTF-8"),
    ];
    for input in inputs() {
        for bits in [0, 15] {
            let mut extra = policy_flags(bits);
            extra.extend(["--json", "--user", "jay"]);
            let args = with("volumes", &input, &extra);
            let plain = run(&args);
            let influenced = run_with_environment(&args, &hostile);
            assert_eq!(plain.status, 0);
            assert_eq!(influenced.status, 0);
            assert_eq!(plain.stdout, influenced.stdout, "{args:?}");
        }
    }
}

#[test]
fn a_hostile_label_does_not_reach_the_terminal_or_a_path() {
    let from = path("removable.json");
    let text = ok(&["volumes", "--from", from, "--user", "jay"]);
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains('\u{7}'));
    assert!(
        text.contains("  Name:        _.._etc_pass wd_x_y[31m\n"),
        "{text}"
    );
    assert!(
        text.contains("linux /run/media/jay/_.._etc_pass wd_x_y[31m ("),
        "{text}"
    );
    for line in text.lines().filter(|line| line.starts_with("  View:")) {
        assert!(!line.split('/').any(|part| part == ".."), "{line}");
    }

    // JSON carries the raw label, escaped by the JSON encoding.
    let document = json(&["volumes", "--from", from, "--json"]);
    let labels: Vec<&str> = document["volumes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|volume| volume["label"].as_str())
        .collect();
    assert!(labels.contains(&"../../etc/\u{7}pass\nwd:x\\y\u{1b}[31m"));
}

#[test]
fn equal_labels_are_distinct_in_the_output() {
    let from = path("removable.json");
    let props = path("removable.props");
    let document = json(&["volumes", "--from", from, "--props", props, "--json"]);
    let names: Vec<&str> = document["volumes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|volume| volume["label"] == "BACKUP")
        .filter_map(|volume| volume["display_name"].as_str())
        .collect();
    assert_eq!(names, ["BACKUP", "BACKUP (2)"]);
}

#[test]
fn capabilities_reports_losses() {
    let text = ok(&["capabilities", "--from-fs", "ntfs", "--to-fs", "exfat"]);
    assert!(
        text.starts_with("Copy from NTFS to exFAT\nAssessment: complete\n"),
        "{text}"
    );
    assert!(text.contains("  alternate-data-streams: lost (source yes, destination no)\n"));
    assert!(text.contains("  hard-links: lost (source yes, destination no)\n"));
}
