// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The tool manifest a UOS service ships (addendum section 11).
//!
//! A manifest is loaded with [`ToolManifest::from_json`], which validates it
//! completely. A [`ToolManifest`] value therefore always satisfies the rules:
//!
//! * tool names are unique and use a safe character set;
//! * the class is 0 to 3; class 4 cannot be declared, because a prohibited
//!   operation is simply not offered as a tool;
//! * the input schema is an object schema with `additionalProperties: false`,
//!   written in the subset of JSON Schema that [`ToolSpec::check_arguments`]
//!   enforces. A keyword outside that subset is rejected, so a manifest cannot
//!   state a constraint the agent would silently ignore.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::action::ToolCall;
use crate::class::ActionClass;
use crate::codes::{Code, Problem};

/// The manifest schema version this build reads.
pub const MANIFEST_SCHEMA_VERSION: u64 = 1;

/// The longest tool name, in bytes.
pub const TOOL_NAME_MAX: usize = 64;

/// The longest tool description, in characters.
pub const DESCRIPTION_MAX: usize = 1024;

/// The value types an input schema may use.
pub const SCHEMA_TYPES: &[&str] = &[
    "object", "array", "string", "integer", "number", "boolean", "null",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema_version: u64,
    service: String,
    tools: Vec<RawTool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTool {
    name: String,
    description: String,
    class: i64,
    method: String,
    input_schema: Value,
}

/// A validated tool manifest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolManifest {
    schema_version: u64,
    service: String,
    tools: Vec<ToolSpec>,
}

/// One validated tool of a manifest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolSpec {
    name: String,
    description: String,
    class: ActionClass,
    method: String,
    input_schema: Value,
}

impl ToolManifest {
    /// Load and validate a manifest from its JSON text.
    ///
    /// Returns every problem found, not only the first.
    pub fn from_json(text: &str) -> Result<Self, Vec<Problem>> {
        let raw: RawManifest = serde_json::from_str(text)
            .map_err(|err| vec![Problem::new(Code::ManifestParse, err.to_string())])?;

        let mut problems = Vec::new();
        if raw.schema_version != MANIFEST_SCHEMA_VERSION {
            problems.push(Problem::new(
                Code::ManifestVersion,
                format!(
                    "schema_version is {}, this build reads {MANIFEST_SCHEMA_VERSION}",
                    raw.schema_version
                ),
            ));
        }
        if !is_bus_name(&raw.service) {
            problems.push(Problem::new(
                Code::ManifestService,
                "service must be a D-Bus well-known name such as org.uos.Storage1",
            ));
        }
        if raw.tools.is_empty() {
            problems.push(Problem::new(
                Code::ManifestNoTools,
                "tools must list at least one tool",
            ));
        }

        let mut seen = BTreeSet::new();
        let mut tools = Vec::with_capacity(raw.tools.len());
        for (index, tool) in raw.tools.into_iter().enumerate() {
            let before = problems.len();
            let at = format!("tools[{index}]");

            if !is_tool_name(&tool.name) {
                problems.push(Problem::new(
                    Code::ManifestToolName,
                    format!(
                        "{at}.name must match ^[a-z][a-z0-9_]*$ and be at most {TOOL_NAME_MAX} bytes"
                    ),
                ));
            } else if !seen.insert(tool.name.clone()) {
                problems.push(Problem::new(
                    Code::ManifestDuplicateTool,
                    format!("{at}.name {} is already declared", tool.name),
                ));
            }

            if !is_description(&tool.description) {
                problems.push(Problem::new(
                    Code::ManifestToolField,
                    format!(
                        "{at}.description must be 1 to {DESCRIPTION_MAX} characters with no control characters"
                    ),
                ));
            }
            if !is_member_name(&tool.method) {
                problems.push(Problem::new(
                    Code::ManifestToolField,
                    format!("{at}.method must be a D-Bus member name such as ListVolumes"),
                ));
            }

            let class = u8::try_from(tool.class)
                .ok()
                .and_then(ActionClass::from_u8);
            match class {
                Some(ActionClass::Prohibited) => problems.push(Problem::new(
                    Code::ManifestClassProhibited,
                    format!("{at}.class is 4; a prohibited operation is not offered as a tool"),
                )),
                Some(_) => {}
                None => problems.push(Problem::new(
                    Code::ManifestClassRange,
                    format!("{at}.class is {}; it must be 0 to 3", tool.class),
                )),
            }

            check_schema(
                &tool.input_schema,
                &format!("{at}.input_schema"),
                true,
                &mut problems,
            );

            if let Some(class) = class
                && problems.len() == before
            {
                tools.push(ToolSpec {
                    name: tool.name,
                    description: tool.description,
                    class,
                    method: tool.method,
                    input_schema: tool.input_schema,
                });
            }
        }

        if problems.is_empty() {
            Ok(ToolManifest {
                schema_version: raw.schema_version,
                service: raw.service,
                tools,
            })
        } else {
            Err(problems)
        }
    }

    /// The manifest schema version.
    pub fn schema_version(&self) -> u64 {
        self.schema_version
    }

    /// The D-Bus name of the service.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The tools, in manifest order.
    pub fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// The tool with the given name.
    pub fn tool(&self, name: &str) -> Option<&ToolSpec> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    /// Build the action for a call to one of this manifest's tools.
    ///
    /// The class is taken from the manifest. The arguments are checked against
    /// the tool's input schema.
    pub fn tool_call(
        &self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> Result<ToolCall, Vec<Problem>> {
        let Some(tool) = self.tool(name) else {
            return Err(vec![Problem::new(
                Code::ToolUnknown,
                format!("{} declares no tool with that name", self.service),
            )]);
        };
        tool.check_arguments(&arguments)?;
        Ok(ToolCall {
            service: self.service.clone(),
            tool: tool.name.clone(),
            arguments,
            declared_class: tool.class,
        })
    }
}

impl ToolSpec {
    /// The tool name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The description given to the model.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The declared class, 0 to 3.
    pub fn class(&self) -> ActionClass {
        self.class
    }

    /// The D-Bus method the tool calls.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The input schema.
    pub fn input_schema(&self) -> &Value {
        &self.input_schema
    }

    /// Check proposed arguments against the input schema.
    ///
    /// The problems name the position and the expectation. They never repeat an
    /// argument value, and repeat a property name only if the schema declares it.
    pub fn check_arguments(&self, arguments: &Map<String, Value>) -> Result<(), Vec<Problem>> {
        let mut problems = Vec::new();
        check_object(&self.input_schema, arguments, "arguments", &mut problems);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

fn is_tool_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= TOOL_NAME_MAX
        && matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A D-Bus member name, also used for each element of a bus name and for
/// property names in an input schema.
fn is_member_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 255
        && matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn is_bus_name(name: &str) -> bool {
    name.len() <= 255 && name.split('.').count() >= 2 && name.split('.').all(is_member_name)
}

fn is_description(description: &str) -> bool {
    !description.trim().is_empty()
        && description.chars().count() <= DESCRIPTION_MAX
        && !description.chars().any(char::is_control)
}

/// Whether `keyword` may appear in a schema of type `kind`. This is the whole
/// supported subset: a keyword that is not listed is not supported.
fn keyword_applies(keyword: &str, kind: &str) -> bool {
    match keyword {
        "type" | "title" | "description" | "enum" => true,
        "properties" | "required" | "additionalProperties" => kind == "object",
        "items" => kind == "array",
        "minLength" | "maxLength" => kind == "string",
        "minimum" | "maximum" => kind == "integer" || kind == "number",
        _ => false,
    }
}

fn schema_problem(path: &str, message: &str) -> Problem {
    Problem::new(Code::ManifestInputSchema, format!("{path}: {message}"))
}

fn check_schema(schema: &Value, path: &str, root: bool, problems: &mut Vec<Problem>) {
    let Some(object) = schema.as_object() else {
        problems.push(schema_problem(path, "a schema must be a JSON object"));
        return;
    };
    let Some(kind) = object.get("type").and_then(Value::as_str) else {
        problems.push(schema_problem(path, "\"type\" must be present and be one string"));
        return;
    };
    if !SCHEMA_TYPES.contains(&kind) {
        problems.push(schema_problem(path, "\"type\" is not a supported type"));
        return;
    }
    if root && kind != "object" {
        problems.push(schema_problem(path, "the input schema must have type \"object\""));
    }

    for (keyword, value) in object {
        if !keyword_applies(keyword, kind) {
            problems.push(schema_problem(
                path,
                &format!("keyword {keyword:?} is not supported for type {kind:?}"),
            ));
            continue;
        }
        let well_formed = match keyword.as_str() {
            "title" | "description" => value.is_string(),
            "enum" => value.as_array().is_some_and(|options| !options.is_empty()),
            "minLength" | "maxLength" => value.is_u64(),
            "minimum" | "maximum" => value.is_number(),
            _ => true,
        };
        if !well_formed {
            problems.push(schema_problem(
                path,
                &format!("keyword {keyword:?} has a value of the wrong kind"),
            ));
        }
    }

    if kind == "object" {
        check_object_schema(object, path, problems);
    }
    if kind == "array" {
        match object.get("items") {
            Some(items) => check_schema(items, &format!("{path}.items"), false, problems),
            None => problems.push(schema_problem(path, "an array schema must declare \"items\"")),
        }
    }
}

fn check_object_schema(object: &Map<String, Value>, path: &str, problems: &mut Vec<Problem>) {
    if object.get("additionalProperties") != Some(&Value::Bool(false)) {
        problems.push(schema_problem(
            path,
            "an object schema must set \"additionalProperties\": false",
        ));
    }

    let properties = match object.get("properties") {
        None => None,
        Some(Value::Object(properties)) => Some(properties),
        Some(_) => {
            problems.push(schema_problem(path, "\"properties\" must be an object"));
            None
        }
    };
    if let Some(properties) = properties {
        for (name, schema) in properties {
            if is_member_name(name) && name.len() <= TOOL_NAME_MAX {
                check_schema(schema, &format!("{path}.properties.{name}"), false, problems);
            } else {
                problems.push(schema_problem(
                    path,
                    "a property name must match ^[A-Za-z_][A-Za-z0-9_]*$ and be at most 64 bytes",
                ));
            }
        }
    }

    match object.get("required") {
        None => {}
        Some(Value::Array(required)) => {
            for entry in required {
                let declared = entry
                    .as_str()
                    .is_some_and(|name| properties.is_some_and(|known| known.contains_key(name)));
                if !declared {
                    problems.push(schema_problem(
                        path,
                        "every \"required\" entry must name a declared property",
                    ));
                }
            }
        }
        Some(_) => problems.push(schema_problem(path, "\"required\" must be an array")),
    }
}

fn argument_problem(path: &str, message: &str) -> Problem {
    Problem::new(Code::ToolArguments, format!("{path}: {message}"))
}

fn check_object(
    schema: &Value,
    arguments: &Map<String, Value>,
    path: &str,
    problems: &mut Vec<Problem>,
) {
    let properties = schema.get("properties").and_then(Value::as_object);
    for (name, value) in arguments {
        match properties.and_then(|known| known.get(name)) {
            Some(property) => check_value(property, value, &format!("{path}.{name}"), problems),
            None => problems.push(argument_problem(
                path,
                "has a property the input schema does not declare",
            )),
        }
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if !arguments.contains_key(name) {
                problems.push(argument_problem(
                    path,
                    &format!("required property {name} is missing"),
                ));
            }
        }
    }
}

fn check_value(schema: &Value, value: &Value, path: &str, problems: &mut Vec<Problem>) {
    let kind = schema.get("type").and_then(Value::as_str).unwrap_or("");
    let type_matches = match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    if !type_matches {
        problems.push(argument_problem(path, &format!("must be of type {kind}")));
        return;
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array)
        && !options.contains(value)
    {
        problems.push(argument_problem(path, "is not one of the permitted values"));
    }

    match value {
        Value::Object(map) => check_object(schema, map, path, problems),
        Value::Array(items) => {
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    check_value(item_schema, item, &format!("{path}[{index}]"), problems);
                }
            }
        }
        Value::String(text) => {
            let length = u64::try_from(text.chars().count()).unwrap_or(u64::MAX);
            if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64)
                && length < minimum
            {
                problems.push(argument_problem(path, "is shorter than minLength"));
            }
            if let Some(maximum) = schema.get("maxLength").and_then(Value::as_u64)
                && length > maximum
            {
                problems.push(argument_problem(path, "is longer than maxLength"));
            }
        }
        Value::Number(number) => {
            // Compared as 64-bit floats: exact up to 2^53, approximate beyond.
            let number = number.as_f64();
            let minimum = schema.get("minimum").and_then(Value::as_f64);
            let maximum = schema.get("maximum").and_then(Value::as_f64);
            if let (Some(number), Some(minimum)) = (number, minimum)
                && number < minimum
            {
                problems.push(argument_problem(path, "is below minimum"));
            }
            if let (Some(number), Some(maximum)) = (number, maximum)
                && number > maximum
            {
                problems.push(argument_problem(path, "is above maximum"));
            }
        }
        Value::Bool(_) | Value::Null => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The example manifest from the addendum, section 11.
    const ADDENDUM_EXAMPLE: &str = r#"{
      "schema_version": 1,
      "service": "org.uos.Storage1",
      "tools": [
        {
          "name": "storage_list_volumes",
          "description": "List detected volumes with filesystem, state and mount point.",
          "class": 0,
          "method": "ListVolumes",
          "input_schema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
          "name": "storage_unmount",
          "description": "Unmount a volume by its UOS volume identifier.",
          "class": 1,
          "method": "Unmount",
          "input_schema": {
            "type": "object",
            "properties": { "volume_id": { "type": "string" } },
            "required": ["volume_id"],
            "additionalProperties": false
          }
        }
      ]
    }"#;

    fn manifest_with(tool: Value) -> String {
        json!({ "schema_version": 1, "service": "org.uos.Test1", "tools": [tool] }).to_string()
    }

    fn tool_with(change: impl FnOnce(&mut Map<String, Value>)) -> Value {
        let mut tool = json!({
            "name": "test_tool",
            "description": "A tool for tests.",
            "class": 1,
            "method": "Test",
            "input_schema": { "type": "object", "additionalProperties": false }
        });
        change(tool.as_object_mut().expect("the tool is an object"));
        tool
    }

    fn codes(text: &str) -> Vec<Code> {
        ToolManifest::from_json(text)
            .expect_err("the manifest must be rejected")
            .into_iter()
            .map(|problem| problem.code)
            .collect()
    }

    fn schema_codes(schema: Value) -> Vec<Code> {
        let text = manifest_with(tool_with(|tool| {
            tool.insert("input_schema".to_owned(), schema);
        }));
        match ToolManifest::from_json(&text) {
            Ok(_) => Vec::new(),
            Err(problems) => problems.into_iter().map(|problem| problem.code).collect(),
        }
    }

    #[test]
    fn loads_the_example_from_the_addendum() {
        let manifest = ToolManifest::from_json(ADDENDUM_EXAMPLE).unwrap();
        assert_eq!(manifest.schema_version(), 1);
        assert_eq!(manifest.service(), "org.uos.Storage1");
        assert_eq!(manifest.tools().len(), 2);
        let unmount = manifest.tool("storage_unmount").unwrap();
        assert_eq!(unmount.class(), ActionClass::Reversible);
        assert_eq!(unmount.method(), "Unmount");
        assert!(manifest.tool("storage_format").is_none());
    }

    #[test]
    fn accepts_classes_0_to_3_only() {
        for class in 0..=3 {
            let text = manifest_with(tool_with(|tool| {
                tool.insert("class".to_owned(), json!(class));
            }));
            let manifest = ToolManifest::from_json(&text).unwrap();
            assert_eq!(manifest.tools()[0].class().as_u8(), class);
        }
        for class in [-1, 5, 255, 256, 1_000_000] {
            let text = manifest_with(tool_with(|tool| {
                tool.insert("class".to_owned(), json!(class));
            }));
            assert_eq!(codes(&text), [Code::ManifestClassRange], "class {class}");
        }
    }

    #[test]
    fn class_4_cannot_be_declared() {
        let text = manifest_with(tool_with(|tool| {
            tool.insert("class".to_owned(), json!(4));
        }));
        assert_eq!(codes(&text), [Code::ManifestClassProhibited]);
    }

    #[test]
    fn a_class_that_is_not_an_integer_is_a_parse_error() {
        for class in [json!("1"), json!(1.5), json!(null), json!(true)] {
            let text = manifest_with(tool_with(|tool| {
                tool.insert("class".to_owned(), class);
            }));
            assert_eq!(codes(&text), [Code::ManifestParse]);
        }
    }

    #[test]
    fn tool_names_use_a_safe_character_set() {
        let longest = "a".repeat(TOOL_NAME_MAX);
        let too_long = "a".repeat(TOOL_NAME_MAX + 1);
        for name in ["a", "storage_list_volumes", "x9_", longest.as_str()] {
            let text = manifest_with(tool_with(|tool| {
                tool.insert("name".to_owned(), json!(name));
            }));
            assert!(ToolManifest::from_json(&text).is_ok(), "{name}");
        }
        for name in [
            "",
            "Storage",
            "9lives",
            "_hidden",
            "has space",
            "has-dash",
            "dot.name",
            "semi;colon",
            "new\nline",
            "naïve",
            "../etc",
            too_long.as_str(),
        ] {
            let text = manifest_with(tool_with(|tool| {
                tool.insert("name".to_owned(), json!(name));
            }));
            assert_eq!(codes(&text), [Code::ManifestToolName], "{name:?}");
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let text = json!({
            "schema_version": 1,
            "service": "org.uos.Test1",
            "tools": [tool_with(|_| {}), tool_with(|_| {})]
        })
        .to_string();
        assert_eq!(codes(&text), [Code::ManifestDuplicateTool]);
    }

    #[test]
    fn rejects_bad_versions_services_fields_and_empty_manifests() {
        let text = json!({ "schema_version": 2, "service": "org.uos.Test1", "tools": [tool_with(|_| {})] });
        assert_eq!(codes(&text.to_string()), [Code::ManifestVersion]);

        for service in ["", "nodots", "org..uos", "org.uos.", ".org.uos", "org.9uos", "org.uos/x", "org.uos x"] {
            let text = json!({ "schema_version": 1, "service": service, "tools": [tool_with(|_| {})] });
            assert_eq!(codes(&text.to_string()), [Code::ManifestService], "{service:?}");
        }

        let text = json!({ "schema_version": 1, "service": "org.uos.Test1", "tools": [] });
        assert_eq!(codes(&text.to_string()), [Code::ManifestNoTools]);

        for (field, value) in [
            ("description", ""),
            ("description", "   "),
            ("description", "line\nbreak"),
            ("method", ""),
            ("method", "9Method"),
            ("method", "Has.Dot"),
            ("method", "has space"),
        ] {
            let text = manifest_with(tool_with(|tool| {
                tool.insert(field.to_owned(), json!(value));
            }));
            assert_eq!(codes(&text), [Code::ManifestToolField], "{field}={value:?}");
        }
    }

    #[test]
    fn rejects_unknown_and_missing_fields() {
        let text = manifest_with(tool_with(|tool| {
            tool.insert("confirm".to_owned(), json!(false));
        }));
        assert_eq!(codes(&text), [Code::ManifestParse]);

        let text = manifest_with(tool_with(|tool| {
            tool.remove("class");
        }));
        assert_eq!(codes(&text), [Code::ManifestParse]);

        assert_eq!(codes("[]"), [Code::ManifestParse]);
        assert_eq!(codes("{"), [Code::ManifestParse]);
    }

    #[test]
    fn input_schema_must_be_a_closed_object_schema() {
        let bad = [
            json!(true),
            json!([]),
            json!({}),
            json!({ "type": "object" }),
            json!({ "type": "object", "additionalProperties": true }),
            json!({ "type": "object", "additionalProperties": {} }),
            json!({ "type": "object", "additionalProperties": { "type": "string" } }),
            json!({ "type": "string" }),
            json!({ "type": ["object", "null"], "additionalProperties": false }),
            json!({ "type": "object", "additionalProperties": false, "properties": [] }),
            json!({ "type": "object", "additionalProperties": false, "required": "x" }),
            json!({ "type": "object", "additionalProperties": false, "required": ["missing"] }),
            json!({ "type": "object", "additionalProperties": false, "required": [1] }),
        ];
        for schema in bad {
            let found = schema_codes(schema.clone());
            assert!(!found.is_empty(), "{schema} must be rejected");
            assert!(found.iter().all(|code| *code == Code::ManifestInputSchema), "{schema}");
        }
    }

    #[test]
    fn nested_object_schemas_must_be_closed_too() {
        let open_nested = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": { "options": { "type": "object" } }
        });
        assert_eq!(schema_codes(open_nested), [Code::ManifestInputSchema]);

        let open_in_array = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": { "list": { "type": "array", "items": { "type": "object" } } }
        });
        assert_eq!(schema_codes(open_in_array), [Code::ManifestInputSchema]);

        let closed = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "options": { "type": "object", "additionalProperties": false },
                "list": {
                    "type": "array",
                    "items": { "type": "object", "additionalProperties": false }
                }
            }
        });
        assert!(schema_codes(closed).is_empty());
    }

    #[test]
    fn unsupported_schema_keywords_are_rejected() {
        for (keyword, value) in [
            ("anyOf", json!([{ "type": "string" }])),
            ("oneOf", json!([{ "type": "string" }])),
            ("$ref", json!("#/somewhere")),
            ("patternProperties", json!({})),
            ("pattern", json!("^a$")),
            ("format", json!("uri")),
            ("default", json!(1)),
            ("const", json!(1)),
        ] {
            let mut property = json!({ "type": "string" });
            property
                .as_object_mut()
                .expect("an object")
                .insert(keyword.to_owned(), value);
            let schema = json!({
                "type": "object",
                "additionalProperties": false,
                "properties": { "value": property }
            });
            assert_eq!(schema_codes(schema), [Code::ManifestInputSchema], "{keyword}");
        }

        // A supported keyword on the wrong type, or with the wrong kind of value.
        for property in [
            json!({ "type": "string", "minimum": 1 }),
            json!({ "type": "integer", "maxLength": 1 }),
            json!({ "type": "string", "items": { "type": "string" } }),
            json!({ "type": "array" }),
            json!({ "type": "string", "minLength": -1 }),
            json!({ "type": "string", "enum": [] }),
            json!({ "type": "string", "description": 1 }),
            json!({ "type": "text" }),
            json!({}),
        ] {
            let schema = json!({
                "type": "object",
                "additionalProperties": false,
                "properties": { "value": property }
            });
            assert_eq!(schema_codes(schema.clone()), [Code::ManifestInputSchema], "{schema}");
        }
    }

    #[test]
    fn property_names_use_a_safe_character_set() {
        for name in ["", "has space", "9x", "pass-word", "naïve"] {
            let mut properties = Map::new();
            properties.insert(name.to_owned(), json!({ "type": "string" }));
            let schema = json!({
                "type": "object",
                "additionalProperties": false,
                "properties": properties
            });
            assert_eq!(schema_codes(schema), [Code::ManifestInputSchema], "{name:?}");
        }
    }

    fn checked_manifest() -> ToolManifest {
        let text = manifest_with(tool_with(|tool| {
            tool.insert(
                "input_schema".to_owned(),
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["volume_id"],
                    "properties": {
                        "volume_id": { "type": "string", "minLength": 1, "maxLength": 8 },
                        "mode": { "type": "string", "enum": ["fast", "safe"] },
                        "count": { "type": "integer", "minimum": 1, "maximum": 10 },
                        "ratio": { "type": "number" },
                        "force": { "type": "boolean" },
                        "labels": { "type": "array", "items": { "type": "string" } },
                        "options": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": { "quiet": { "type": "boolean" } }
                        }
                    }
                }),
            );
        }));
        ToolManifest::from_json(&text).unwrap()
    }

    fn argument_codes(manifest: &ToolManifest, arguments: Value) -> Vec<Code> {
        let arguments = arguments.as_object().cloned().unwrap_or_default();
        match manifest.tool_call("test_tool", arguments) {
            Ok(_) => Vec::new(),
            Err(problems) => problems.into_iter().map(|problem| problem.code).collect(),
        }
    }

    #[test]
    fn tool_call_takes_the_class_from_the_manifest() {
        let manifest = checked_manifest();
        let arguments = json!({ "volume_id": "vol1" }).as_object().cloned().unwrap_or_default();
        let call = manifest.tool_call("test_tool", arguments.clone()).unwrap();
        assert_eq!(call.service, "org.uos.Test1");
        assert_eq!(call.tool, "test_tool");
        assert_eq!(call.arguments, arguments);
        assert_eq!(call.declared_class, ActionClass::Reversible);

        assert_eq!(
            manifest
                .tool_call("no_such_tool", Map::new())
                .expect_err("unknown tool")
                .into_iter()
                .map(|problem| problem.code)
                .collect::<Vec<_>>(),
            [Code::ToolUnknown]
        );
    }

    #[test]
    fn arguments_are_checked_against_the_input_schema() {
        let manifest = checked_manifest();
        let good = [
            json!({ "volume_id": "vol1" }),
            json!({ "volume_id": "vol1", "mode": "safe", "count": 10, "ratio": 0.5, "force": true }),
            json!({ "volume_id": "vol1", "labels": [], "options": {} }),
            json!({ "volume_id": "vol1", "labels": ["a", "b"], "options": { "quiet": true } }),
            json!({ "volume_id": "vol1", "ratio": 3 }),
        ];
        for arguments in good {
            assert!(argument_codes(&manifest, arguments.clone()).is_empty(), "{arguments}");
        }

        let bad = [
            json!({}),
            json!({ "volume_id": "" }),
            json!({ "volume_id": "much-too-long" }),
            json!({ "volume_id": 7 }),
            json!({ "volume_id": "vol1", "extra": 1 }),
            json!({ "volume_id": "vol1", "mode": "reckless" }),
            json!({ "volume_id": "vol1", "count": 0 }),
            json!({ "volume_id": "vol1", "count": 11 }),
            json!({ "volume_id": "vol1", "count": 1.5 }),
            json!({ "volume_id": "vol1", "count": "1" }),
            json!({ "volume_id": "vol1", "force": "yes" }),
            json!({ "volume_id": "vol1", "labels": "a" }),
            json!({ "volume_id": "vol1", "labels": ["a", 1] }),
            json!({ "volume_id": "vol1", "options": { "loud": true } }),
            json!({ "volume_id": "vol1", "options": { "quiet": "no" } }),
            json!({ "volume_id": null }),
        ];
        for arguments in bad {
            assert_eq!(
                argument_codes(&manifest, arguments.clone()),
                [Code::ToolArguments],
                "{arguments}"
            );
        }
    }

    #[test]
    fn argument_problems_never_repeat_values_or_undeclared_names() {
        let manifest = checked_manifest();
        let arguments = json!({ "volume_id": 12345678, "hunter2secret": "p4ssw0rd-value" });
        let problems = manifest
            .tool_call("test_tool", arguments.as_object().cloned().unwrap_or_default())
            .expect_err("the arguments are invalid");
        assert_eq!(problems.len(), 2);
        for problem in problems {
            let text = problem.to_string();
            assert!(!text.contains("12345678"), "{text}");
            assert!(!text.contains("hunter2secret"), "{text}");
            assert!(!text.contains("p4ssw0rd-value"), "{text}");
        }
    }
}
