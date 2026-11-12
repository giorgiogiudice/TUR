// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! JSON parsing that refuses an object with a repeated key.
//!
//! JSON does not say what `{"to": "a", "to": "b"}` means. `serde_json` keeps
//! the last value; other parsers keep the first, and a person reading the text
//! may see either. Where the text decides something, that difference is a way
//! to show one thing and do another. So the arguments of a tool call, the
//! arguments of a standing approval and the input schema of a manifest are all
//! read strictly, and a repeated key at any depth is an error.
//!
//! The parser itself lives in the provider layer
//! ([`tur_pax_providers::strict`]), which reads everything a provider sends
//! through it. It is re-exported here unchanged, so that the provider layer
//! and the engine cannot read the same text in two ways.
//!
//! The typed parts of the manifest and of the policy file are structs with
//! `deny_unknown_fields`, where `serde` already rejects a repeated field.
//!
//! ```
//! use tur_pax::strict;
//!
//! assert!(strict::value_from_str(r#"{"to": "a", "cc": ["b"]}"#).is_ok());
//! assert!(strict::value_from_str(r#"{"to": "a", "to": "b"}"#).is_err());
//! assert!(strict::value_from_str(r#"[{"to": {"x": 1, "x": 2}}]"#).is_err());
//!
//! let arguments = strict::object_from_str(r#"{"volume_id": "vol1"}"#).unwrap();
//! assert_eq!(arguments["volume_id"], "vol1");
//! assert!(strict::object_from_str("[]").is_err());
//! assert!(strict::object_from_str(r#"{"a": 1, "a": 1}"#).is_err());
//! ```

pub use tur_pax_providers::strict::{object, object_from_str, value, value_from_str};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_reads_with_the_parser_of_the_provider_layer() {
        // One implementation: what the provider layer refuses, the engine
        // refuses, and what it accepts is read the same way.
        for text in [
            r#"{"a": 1, "a": 2}"#,
            r#"{"a": {"b": 1, "b": 2}}"#,
            r#"[{"a": 1, "a": 1}]"#,
        ] {
            assert!(value_from_str(text).is_err(), "{text}");
            assert!(
                tur_pax_providers::strict::value_from_str(text).is_err(),
                "{text}"
            );
        }
        let text = r#"{"a": [1, {"b": null}]}"#;
        assert_eq!(
            object_from_str(text).ok(),
            tur_pax_providers::strict::object_from_str(text).ok()
        );
        assert!(object_from_str(text).is_ok());
    }
}
