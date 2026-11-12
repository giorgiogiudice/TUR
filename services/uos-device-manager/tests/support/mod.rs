// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Shared by the integration tests: the fixture location and a validator for
//! the subset of JSON Schema that `schemas/device-inventory.schema.json` uses.
//!
//! The validator is deliberately strict about itself: a keyword it does not
//! implement is an error, not something silently ignored. So a constraint
//! written in the schema is a constraint the tests enforce.

#![allow(dead_code)]

use serde_json::Value;

/// The `laptop` fixture: a small `/sys`, `/proc`, `/etc` and `/usr` tree and
/// recorded command output. Hand-written test data, not a capture of a real
/// machine.
pub const LAPTOP: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/laptop");

/// The schema of the inventory and of the other documents (under `$defs`).
pub const SCHEMA: &str = include_str!("../../../../schemas/device-inventory.schema.json");

/// Keywords that describe and do not constrain.
const ANNOTATIONS: [&str; 5] = ["$schema", "$id", "$defs", "title", "description"];

/// The parsed schema.
pub fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the schema is JSON")
}

/// Validate `value` against the schema's root.
pub fn validate(value: &Value) -> Result<(), String> {
    let root = schema();
    check(&root, &root, value, "$")
}

/// Validate `value` against one of the schema's `$defs`.
pub fn validate_as(definition: &str, value: &Value) -> Result<(), String> {
    let root = schema();
    let node = root["$defs"]
        .get(definition)
        .unwrap_or_else(|| panic!("the schema has no definition {definition}"));
    check(&root, node, value, "$")
}

fn type_matches(name: &str, value: &Value) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        other => panic!("unknown type {other} in the schema"),
    }
}

fn check(root: &Value, node: &Value, value: &Value, path: &str) -> Result<(), String> {
    let schema = node
        .as_object()
        .unwrap_or_else(|| panic!("schema node at {path} is not an object"));
    for (keyword, rule) in schema {
        match keyword.as_str() {
            keyword if ANNOTATIONS.contains(&keyword) => {}
            "$ref" => {
                let pointer = rule.as_str().expect("$ref is a string");
                let name = pointer
                    .strip_prefix("#/$defs/")
                    .unwrap_or_else(|| panic!("unsupported $ref {pointer}"));
                let target = root["$defs"]
                    .get(name)
                    .unwrap_or_else(|| panic!("the schema has no definition {name}"));
                check(root, target, value, path)?;
            }
            "type" => {
                let allowed: Vec<&str> = match rule {
                    Value::String(name) => vec![name.as_str()],
                    Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
                    _ => panic!("bad type keyword at {path}"),
                };
                if !allowed.iter().any(|name| type_matches(name, value)) {
                    return Err(format!("{path}: {value} is not of type {allowed:?}"));
                }
            }
            "const" => {
                if value != rule {
                    return Err(format!("{path}: {value} is not {rule}"));
                }
            }
            "enum" => {
                let allowed = rule.as_array().expect("enum is an array");
                if !allowed.contains(value) {
                    return Err(format!("{path}: {value} is not one of {rule}"));
                }
            }
            "required" => {
                if let Some(object) = value.as_object() {
                    for name in rule.as_array().expect("required is an array") {
                        let name = name.as_str().expect("required names are strings");
                        if !object.contains_key(name) {
                            return Err(format!("{path}: missing field {name}"));
                        }
                    }
                }
            }
            "properties" => {
                if let Some(object) = value.as_object() {
                    for (name, property) in rule.as_object().expect("properties is an object") {
                        if let Some(inner) = object.get(name) {
                            check(root, property, inner, &format!("{path}.{name}"))?;
                        }
                    }
                }
            }
            "additionalProperties" => {
                if let Some(object) = value.as_object() {
                    let known = schema.get("properties").and_then(Value::as_object);
                    for (name, inner) in object {
                        if known.is_some_and(|known| known.contains_key(name)) {
                            continue;
                        }
                        match rule {
                            Value::Bool(false) => {
                                return Err(format!("{path}: unexpected field {name}"));
                            }
                            Value::Bool(true) => {}
                            rule => check(root, rule, inner, &format!("{path}.{name}"))?,
                        }
                    }
                }
            }
            "items" => {
                if let Some(items) = value.as_array() {
                    for (index, item) in items.iter().enumerate() {
                        check(root, rule, item, &format!("{path}[{index}]"))?;
                    }
                }
            }
            "maxItems" => {
                let limit = rule.as_u64().expect("maxItems is a number");
                if value.as_array().is_some_and(|items| items.len() as u64 > limit) {
                    return Err(format!("{path}: more than {limit} items"));
                }
            }
            "minLength" | "maxLength" => {
                let limit = rule.as_u64().expect("a length is a number");
                if let Some(text) = value.as_str() {
                    let length = text.chars().count() as u64;
                    let ok = if keyword == "minLength" {
                        length >= limit
                    } else {
                        length <= limit
                    };
                    if !ok {
                        return Err(format!("{path}: length {length} violates {keyword} {limit}"));
                    }
                }
            }
            "minimum" => {
                let limit = rule.as_f64().expect("minimum is a number");
                if value.as_f64().is_some_and(|number| number < limit) {
                    return Err(format!("{path}: {value} is below {limit}"));
                }
            }
            "pattern" => {
                let pattern = rule.as_str().expect("pattern is a string");
                if let Some(text) = value.as_str()
                    && !pattern_matches(pattern, text)
                {
                    return Err(format!("{path}: {text:?} does not match {pattern}"));
                }
            }
            "oneOf" => {
                let branches = rule.as_array().expect("oneOf is an array");
                let matching = branches
                    .iter()
                    .filter(|branch| check(root, branch, value, path).is_ok())
                    .count();
                if matching != 1 {
                    return Err(format!(
                        "{path}: {matching} of {} oneOf branches match",
                        branches.len()
                    ));
                }
            }
            other => panic!("the validator does not implement the keyword {other}"),
        }
    }
    Ok(())
}

/// One element of a pattern: the characters it accepts and how many times.
struct Atom {
    accepts: Vec<(char, char)>,
    min: usize,
    max: usize,
}

/// Match the small regular-expression subset the schema uses: `^...$` around
/// a sequence of literal characters, `\`-escaped characters and `[...]`
/// classes with ranges, each optionally followed by `+`, `*`, `?`, `{n}` or
/// `{n,m}`. Anything else is a panic, so an unsupported pattern cannot pass
/// unnoticed.
pub fn pattern_matches(pattern: &str, text: &str) -> bool {
    let body = pattern
        .strip_prefix('^')
        .and_then(|rest| rest.strip_suffix('$'))
        .unwrap_or_else(|| panic!("pattern {pattern} is not anchored"));
    let chars: Vec<char> = body.chars().collect();
    let mut atoms: Vec<Atom> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let accepts = match chars[index] {
            '[' => {
                let close = chars[index..]
                    .iter()
                    .position(|ch| *ch == ']')
                    .unwrap_or_else(|| panic!("unclosed class in {pattern}"))
                    + index;
                let class = &chars[index + 1..close];
                let mut ranges = Vec::new();
                let mut at = 0;
                while at < class.len() {
                    if at + 2 < class.len() && class[at + 1] == '-' {
                        ranges.push((class[at], class[at + 2]));
                        at += 3;
                    } else {
                        ranges.push((class[at], class[at]));
                        at += 1;
                    }
                }
                index = close + 1;
                ranges
            }
            '\\' => {
                let escaped = chars[index + 1];
                index += 2;
                vec![(escaped, escaped)]
            }
            ch if "(){}|.*+?".contains(ch) => panic!("unsupported {ch} in pattern {pattern}"),
            ch => {
                index += 1;
                vec![(ch, ch)]
            }
        };
        let (min, max) = match chars.get(index) {
            Some('+') => {
                index += 1;
                (1, usize::MAX)
            }
            Some('*') => {
                index += 1;
                (0, usize::MAX)
            }
            Some('?') => {
                index += 1;
                (0, 1)
            }
            Some('{') => {
                let close = chars[index..]
                    .iter()
                    .position(|ch| *ch == '}')
                    .unwrap_or_else(|| panic!("unclosed quantifier in {pattern}"))
                    + index;
                let spec: String = chars[index + 1..close].iter().collect();
                index = close + 1;
                match spec.split_once(',') {
                    Some((low, high)) => (low.parse().unwrap(), high.parse().unwrap()),
                    None => {
                        let count = spec.parse().unwrap();
                        (count, count)
                    }
                }
            }
            _ => (1, 1),
        };
        atoms.push(Atom { accepts, min, max });
    }
    let text: Vec<char> = text.chars().collect();
    matches_from(&atoms, &text)
}

fn matches_from(atoms: &[Atom], text: &[char]) -> bool {
    let Some((atom, rest)) = atoms.split_first() else {
        return text.is_empty();
    };
    let available = text
        .iter()
        .take_while(|ch| {
            atom.accepts
                .iter()
                .any(|(low, high)| (*low..=*high).contains(*ch))
        })
        .count()
        .min(atom.max);
    if available < atom.min {
        return false;
    }
    // Greedy, backing off one character at a time.
    (atom.min..=available)
        .rev()
        .any(|taken| matches_from(rest, &text[taken..]))
}

/// Every string in a JSON document, with its path.
pub fn strings(value: &Value) -> Vec<(String, &str)> {
    fn walk<'a>(value: &'a Value, path: String, out: &mut Vec<(String, &'a str)>) {
        match value {
            Value::String(text) => out.push((path, text.as_str())),
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    walk(item, format!("{path}[{index}]"), out);
                }
            }
            Value::Object(fields) => {
                for (name, item) in fields {
                    walk(item, format!("{path}.{name}"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, "$".to_owned(), &mut out);
    out
}

/// Every object in a JSON document that is marked `"sensitive": true`.
pub fn sensitive_fields(value: &Value) -> Vec<(String, &Value)> {
    fn walk<'a>(value: &'a Value, path: String, out: &mut Vec<(String, &'a Value)>) {
        match value {
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    walk(item, format!("{path}[{index}]"), out);
                }
            }
            Value::Object(fields) => {
                if fields.get("sensitive") == Some(&Value::Bool(true)) {
                    out.push((path.clone(), value));
                }
                for (name, item) in fields {
                    walk(item, format!("{path}.{name}"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, "$".to_owned(), &mut out);
    out
}
