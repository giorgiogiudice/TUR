// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The published JSON schemas agree with the loaders.
//!
//! This crate has no JSON Schema validator, so these tests do not validate
//! documents against the schemas. They check the points where a schema and the
//! Rust loader state the same fact, so that one cannot change without the other.

use std::collections::BTreeSet;

use serde_json::Value;
use uos_agent::manifest::{MANIFEST_SCHEMA_VERSION, SCHEMA_TYPES, ToolManifest};
use uos_agent::policy::POLICY_SCHEMA_VERSION;
use uos_agent::{Mode, RelaxationLevel, UserPolicy};

const MANIFEST_SCHEMA: &str = include_str!("../../../schemas/agent-tool-manifest.schema.json");
const POLICY_SCHEMA: &str = include_str!("../../../schemas/agent-policy.schema.json");

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("the schema is valid JSON")
}

#[test]
fn the_manifest_schema_agrees_with_the_loader() {
    let schema = parse(MANIFEST_SCHEMA);
    assert_eq!(
        schema["$id"],
        "https://github.com/giorgiogiudice/UOS/schemas/agent-tool-manifest.schema.json"
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        MANIFEST_SCHEMA_VERSION
    );

    let tool = &schema["$defs"]["tool"];
    assert_eq!(tool["additionalProperties"], false);
    assert_eq!(tool["properties"]["class"]["minimum"], 0);
    // Class 4 cannot be declared.
    assert_eq!(tool["properties"]["class"]["maximum"], 3);
    assert_eq!(
        tool["properties"]["name"]["pattern"],
        "^[a-z][a-z0-9_]{0,63}$"
    );
    assert_eq!(
        tool["properties"]["input_schema"]["$ref"],
        "#/$defs/object_schema"
    );

    // The input schema must be closed.
    let object_schema = &schema["$defs"]["object_schema"];
    assert_eq!(
        object_schema["properties"]["additionalProperties"]["const"],
        false
    );
    assert!(
        object_schema["required"]
            .as_array()
            .expect("required is a list")
            .contains(&Value::from("additionalProperties"))
    );

    // The value types of the schema subset are the ones the loader accepts.
    let mut types = BTreeSet::new();
    for name in [
        "object_schema",
        "array_schema",
        "string_schema",
        "number_schema",
        "plain_schema",
    ] {
        let definition = &schema["$defs"][name];
        // No keyword outside the subset.
        assert_eq!(definition["additionalProperties"], false, "{name}");
        let kind = &definition["properties"]["type"];
        if let Some(single) = kind["const"].as_str() {
            types.insert(single.to_owned());
        }
        if let Some(options) = kind["enum"].as_array() {
            for option in options {
                types.insert(option.as_str().expect("a type name").to_owned());
            }
        }
    }
    let accepted: BTreeSet<String> = SCHEMA_TYPES.iter().map(|kind| (*kind).to_owned()).collect();
    assert_eq!(types, accepted);
}

#[test]
fn the_policy_schema_agrees_with_the_loader() {
    let schema = parse(POLICY_SCHEMA);
    assert_eq!(
        schema["$id"],
        "https://github.com/giorgiogiudice/UOS/schemas/agent-policy.schema.json"
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        POLICY_SCHEMA_VERSION
    );

    let modes: Vec<&str> = schema["properties"]["mode"]["enum"]
        .as_array()
        .expect("mode is an enum")
        .iter()
        .map(|mode| mode.as_str().expect("a mode name"))
        .collect();
    let known: Vec<&str> = Mode::ALL.iter().map(|mode| mode.as_str()).collect();
    assert_eq!(modes, known);

    // Only classes 1 and 2 can be relaxed.
    let relaxation = &schema["properties"]["relaxations"]["items"];
    assert_eq!(relaxation["additionalProperties"], false);
    let classes: Vec<u64> = relaxation["properties"]["class"]["enum"]
        .as_array()
        .expect("class is an enum")
        .iter()
        .map(|class| class.as_u64().expect("a class number"))
        .collect();
    let levels: Vec<u64> = RelaxationLevel::ALL
        .iter()
        .map(|level| u64::from(level.class_number()))
        .collect();
    assert_eq!(classes, levels);
    assert_eq!(classes, [1, 2]);
}

#[test]
fn the_test_fixtures_load() {
    let manifest = ToolManifest::from_json(include_str!("data/test-service.manifest.json"))
        .expect("the test service manifest is valid");
    let classes: Vec<u8> = manifest
        .tools()
        .iter()
        .map(|tool| tool.class().as_u8())
        .collect();
    assert_eq!(classes, [0, 1, 2, 3]);

    assert!(ToolManifest::from_json(include_str!("data/invalid-class-4.manifest.json")).is_err());

    let observe_only = UserPolicy::from_json(include_str!("data/policy-observe-only.json"))
        .expect("the observe-only policy is valid");
    assert_eq!(observe_only.mode, Mode::ObserveOnly);
    let ask_every = UserPolicy::from_json(include_str!("data/policy-ask-every-action.json"))
        .expect("the ask-every-action policy is valid");
    assert_eq!(ask_every.mode, Mode::AskEveryAction);
    assert!(UserPolicy::from_json(include_str!("data/policy-invalid-relaxation.json")).is_err());
}
