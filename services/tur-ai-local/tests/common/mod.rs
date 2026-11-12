// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What the integration tests share: a validator for the subset of JSON
//! Schema the two schemas of this component use, scratch directories, and
//! a small catalogue whose one model is a few bytes long.
//!
//! The validator understands exactly the keywords the schemas use and fails
//! on any other, so that a schema cannot grow a rule these tests silently
//! skip. Patterns are implemented by hand, one by one.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};

pub const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-ai-local");
pub const COMPONENT: &str = env!("CARGO_MANIFEST_DIR");

pub fn repository() -> PathBuf {
    Path::new(COMPONENT).join("../..")
}

pub fn fixture_host(name: &str) -> PathBuf {
    Path::new(COMPONENT).join("tests/fixtures/hosts").join(name)
}

pub fn seed_catalogue() -> PathBuf {
    Path::new(COMPONENT).join("catalogue")
}

pub fn fake_engine() -> PathBuf {
    Path::new(COMPONENT).join("tests/fixtures/fake-llama-server")
}

pub fn schema(name: &str) -> Value {
    let path = repository().join("schemas").join(name);
    let text =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).expect("the schema is JSON")
}

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
    "minItems",
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

fn all_of(text: &str, allowed: impl Fn(u8) -> bool) -> bool {
    text.bytes().all(allowed)
}

fn lower_hex(text: &str, length: usize) -> bool {
    text.len() == length && all_of(text, |b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The schemas' patterns, implemented by hand. `None` for a pattern this
/// validator does not know.
fn pattern_matches(pattern: &str, text: &str) -> Option<bool> {
    match pattern {
        "^TUR-AI-LOCAL-[0-9]{3}$" => Some(
            text.strip_prefix("TUR-AI-LOCAL-")
                .is_some_and(|number| number.len() == 3 && all_of(number, |b| b.is_ascii_digit())),
        ),
        "^[a-z0-9][a-z0-9.-]{0,63}$" => Some(tur_ai_local::catalogue::is_model_id(text)),
        "^[0-9a-f]{40}$" => Some(lower_hex(text, 40)),
        "^[0-9a-f]{64}$" => Some(lower_hex(text, 64)),
        "^https://" => Some(text.starts_with("https://")),
        "^[0-9]{4}-[0-9]{2}-[0-9]{2}$" => Some(
            text.len() == 10
                && text.bytes().enumerate().all(|(index, b)| {
                    if index == 4 || index == 7 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                }),
        ),
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
    if let Some(array) = value.as_array() {
        if let Some(items) = schema.get("items") {
            for (index, item) in array.iter().enumerate() {
                validate(root, items, item, &format!("{path}/{index}"), errors);
            }
        }
        if let Some(minimum) = schema.get("minItems").and_then(Value::as_u64)
            && (array.len() as u64) < minimum
        {
            errors.push(format!("{path}: fewer than {minimum} items"));
        }
    }
    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
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

pub fn schema_errors(schema: &Value, document: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    validate(schema, schema, document, "", &mut errors);
    errors
}

pub fn assert_valid(schema: &Value, document: &Value, context: &str) {
    let errors = schema_errors(schema, document);
    assert!(errors.is_empty(), "{context}: {errors:#?}\n{document:#}");
}

/// A fresh scratch directory for one test.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tur-ai-local-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The bytes of the tiny model.
pub const TINY: &[u8] = b"GGUF\x03\x00\x00\x00 a tiny stand-in for a model file, for tests only";

/// A catalogue directory in `dir` with one model, `tiny-test`, whose file
/// is [`TINY`].
pub fn tiny_catalogue(dir: &Path) -> PathBuf {
    let catalogue = dir.join("catalogue");
    fs::create_dir_all(&catalogue).expect("catalogue directory");
    let digest = hex(&Sha256::digest(TINY));
    let entry = format!(
        r#"# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
schema_version = 1
id = "tiny-test"
name = "Tiny Test"
publisher = "TUR tests"
repository = "https://example.org/tiny"
revision = "0123456789abcdef0123456789abcdef01234567"
format = "gguf"
quantisation = "none"
parameters = "0"
context_length = 4096
memory_bytes = 100000000
memory_context = 512
kv_bytes_per_token = 1000
capabilities = ["chat", "tools"]
thinking = "none"
languages = ["en"]
good_for = ["Tests."]
checked = "2026-11-12"

[file]
name = "tiny.gguf"
url = "https://example.org/tiny/0123456789abcdef0123456789abcdef01234567/tiny.gguf"
size_bytes = {size}
sha256 = "{digest}"

[licence]
id = "Apache-2.0"
url = "https://www.apache.org/licenses/LICENSE-2.0"
summary = "Use for any purpose."
restrictions = "none"
evidence = "A test entry."
checked = "2026-11-12"
"#,
        size = TINY.len()
    );
    fs::write(catalogue.join("tiny-test.toml"), entry).expect("entry");
    catalogue
}

/// Run the program with an empty environment and these arguments.
pub fn run(arguments: &[&str]) -> Output {
    Command::new(PROGRAM)
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("the built program runs")
}

pub fn stdout_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)))
}

pub fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
