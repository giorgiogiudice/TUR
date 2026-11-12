// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! JSON parsing that refuses an object with a repeated key.
//!
//! JSON does not say what `{"to": "a", "to": "b"}` means. `serde_json` keeps
//! the last value; other parsers keep the first, and a person reading the text
//! may see either. Where the text decides something, that difference is a way
//! to show one thing and do another. So the arguments of a tool call, the
//! arguments of a standing approval and the input schema of a manifest are all
//! read through this module, and a repeated key at any depth is an error.
//!
//! The typed parts of the manifest and of the policy file are structs with
//! `deny_unknown_fields`, where `serde` already rejects a repeated field.

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

/// A JSON value with no repeated key in any of its objects.
struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictVisitor).map(Strict)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Number::from_f64(value).map_or(Value::Null, Value::Number))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut access: A) -> Result<Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut items = Vec::new();
        while let Some(Strict(item)) = access.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A>(self, mut access: A) -> Result<Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            let Strict(value) = access.next_value()?;
            // The key is not repeated in the message: it may be anything.
            if object.insert(key, value).is_some() {
                return Err(de::Error::custom("an object repeats a key"));
            }
        }
        Ok(Value::Object(object))
    }
}

/// Deserialize a JSON value, refusing a repeated key at any depth.
///
/// For use as `#[serde(deserialize_with = "...")]`.
pub fn value<'de, D>(deserializer: D) -> Result<Value, D::Error>
where
    D: Deserializer<'de>,
{
    Strict::deserialize(deserializer).map(|Strict(value)| value)
}

/// Deserialize a JSON object, refusing a repeated key at any depth.
///
/// For use as `#[serde(deserialize_with = "...")]`.
pub fn object<'de, D>(deserializer: D) -> Result<Map<String, Value>, D::Error>
where
    D: Deserializer<'de>,
{
    match value(deserializer)? {
        Value::Object(object) => Ok(object),
        _ => Err(de::Error::custom("expected a JSON object")),
    }
}

/// Parse a JSON value, refusing a repeated key at any depth.
///
/// ```
/// use uos_agent::strict;
///
/// assert!(strict::value_from_str(r#"{"to": "a", "cc": ["b"]}"#).is_ok());
/// assert!(strict::value_from_str(r#"{"to": "a", "to": "b"}"#).is_err());
/// assert!(strict::value_from_str(r#"[{"to": {"x": 1, "x": 2}}]"#).is_err());
/// ```
pub fn value_from_str(text: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str::<Strict>(text).map(|Strict(value)| value)
}

/// Parse a JSON object, refusing a repeated key at any depth.
///
/// This is how the arguments of a tool call must be parsed. What is sent to
/// the service must then be this value, written out again, and never the text
/// the model produced: the user is shown, and a standing approval is compared
/// with, the parsed value.
///
/// ```
/// use uos_agent::strict;
///
/// let arguments = strict::object_from_str(r#"{"volume_id": "vol1"}"#).unwrap();
/// assert_eq!(arguments["volume_id"], "vol1");
/// assert!(strict::object_from_str("[]").is_err());
/// assert!(strict::object_from_str(r#"{"a": 1, "a": 1}"#).is_err());
/// ```
pub fn object_from_str(text: &str) -> Result<Map<String, Value>, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let parsed = object(&mut deserializer)?;
    deserializer.end()?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn agrees_with_serde_json_where_no_key_is_repeated() {
        for text in [
            "null",
            "true",
            "0",
            "-1",
            "18446744073709551615",
            "-9223372036854775808",
            "1.5",
            "1e2",
            "18446744073709551616",
            "\"text\"",
            "\"\\u00e9\\n\"",
            "[]",
            "{}",
            r#"[1, [2, [3, {"a": null}]]]"#,
            r#"{"a": {"a": {"a": 1}}, "b": [{"a": 1}, {"a": 2}]}"#,
            r#"{"": 1, " ": 2}"#,
        ] {
            let expected: Value = serde_json::from_str(text).expect("valid JSON");
            let parsed = value_from_str(text).unwrap_or_else(|err| panic!("{text}: {err}"));
            assert_eq!(parsed, expected, "{text}");
            // Numbers keep their kind: 1 and 1.0 stay different.
            assert_eq!(parsed.to_string(), expected.to_string(), "{text}");
        }
    }

    #[test]
    fn rejects_a_repeated_key_at_any_depth() {
        for text in [
            r#"{"a": 1, "a": 1}"#,
            r#"{"a": 1, "a": 2}"#,
            r#"{"a": 1, "b": 2, "a": 3}"#,
            r#"{"a": {"b": 1, "b": 2}}"#,
            r#"[{"a": 1, "a": 2}]"#,
            r#"{"a": [[{"b": {"c": 1, "c": 1}}]]}"#,
            // The same key written with an escape.
            r#"{"a": 1, "a": 2}"#,
            r#"{"": 1, "": 2}"#,
        ] {
            assert!(value_from_str(text).is_err(), "{text}");
        }
    }

    #[test]
    fn rejects_what_is_not_json() {
        for text in ["", "{", "{\"a\": 1,}", "{\"a\": 1} x", "nul", "1e999"] {
            assert!(value_from_str(text).is_err(), "{text:?}");
            assert!(object_from_str(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn an_object_is_required_where_one_is_asked_for() {
        assert_eq!(
            object_from_str(r#"{"a": 1}"#).expect("an object"),
            json!({ "a": 1 }).as_object().cloned().unwrap_or_default()
        );
        for text in ["[]", "null", "1", "\"{}\""] {
            assert!(object_from_str(text).is_err(), "{text}");
        }
    }

    #[test]
    fn nesting_is_bounded_by_the_parser() {
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert!(value_from_str(&deep).is_err());
    }
}
