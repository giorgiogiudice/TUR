// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The output filter: what a tool or a command returned is scanned for
//! credential-like text before it can leave the machine (AI control layer
//! design, section 17, item 3).
//!
//! The detectors are not new. This module calls the secret detector of
//! `tur-pax-memory` ([`tur_pax_memory::secret::detect`] and
//! [`tur_pax_memory::secret::detect_in_json`]), which recognises private
//! keys, tokens of known forms, payment card numbers, one-time codes,
//! passwords in obvious forms and long random-looking strings. What is added
//! here is only the removal: the detector says whether a text holds a secret,
//! and this module cuts the text into pieces small enough to withhold one
//! without losing the rest.
//!
//! * A JSON result is filtered value by value: a string, a number or a
//!   property the detector flags is replaced by [`WITHHELD`].
//! * Any other result is filtered line by line. A private-key block is
//!   withheld from its first line to its last.
//! * If the filtered text as a whole still looks like it holds a secret (one
//!   split across lines, for example), all of it is withheld.
//!
//! The filter is a heuristic and errs in both directions, exactly as the
//! detector it is built on does; its limits are listed in the documentation
//! of that module. It reports what kind of thing was withheld and never the
//! text.

use serde_json::{Map, Value};
use tur_pax_memory::SecretKind;
use tur_pax_memory::secret::{detect, detect_in_json};

/// The text put in place of something withheld. It names no kind and holds
/// no word the detector reacts to.
pub const WITHHELD: &str = "[withheld by P@x: credential-like text]";

/// The kind of a withheld piece, as written to the audit trail.
pub const fn kind_name(kind: SecretKind) -> &'static str {
    match kind {
        SecretKind::Password => "password",
        SecretKind::KnownToken => "known_token",
        SecretKind::HighEntropy => "high_entropy",
        SecretKind::PrivateKey => "private_key",
        SecretKind::PaymentCard => "payment_card",
        SecretKind::OneTimeCode => "one_time_code",
    }
}

/// A filtered text and what was withheld from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredText {
    /// The text that may leave the machine.
    pub text: String,
    /// The kind of each withheld piece, in order.
    pub withheld: Vec<SecretKind>,
}

/// A filtered JSON value and what was withheld from it.
#[derive(Debug, Clone, PartialEq)]
pub struct FilteredJson {
    /// The value that may leave the machine.
    pub value: Value,
    /// The kind of each withheld piece, in order.
    pub withheld: Vec<SecretKind>,
}

/// Filter a text line by line.
///
/// ```
/// use tur_pax::redact::{WITHHELD, filter_text};
///
/// let filtered = filter_text("host = example.org\npassword = hunter2\nport = 22");
/// assert_eq!(filtered.text, format!("host = example.org\n{WITHHELD}\nport = 22"));
/// assert_eq!(filtered.withheld.len(), 1);
/// ```
pub fn filter_text(text: &str) -> FilteredText {
    let mut withheld = Vec::new();
    let mut lines: Vec<&str> = Vec::new();
    let mut inside_key = false;
    for line in text.split('\n') {
        if inside_key {
            // The block was marked once, at its first line.
            if line.contains("-----END") {
                inside_key = false;
            }
            continue;
        }
        match detect(line) {
            Some(kind) => {
                if kind == SecretKind::PrivateKey && !line.contains("-----END") {
                    inside_key = true;
                }
                withheld.push(kind);
                lines.push(WITHHELD);
            }
            None => lines.push(line),
        }
    }
    let filtered = lines.join("\n");
    // A secret can be spread over lines that are harmless one by one.
    if let Some(kind) = detect(&filtered) {
        withheld.push(kind);
        return FilteredText {
            text: WITHHELD.to_owned(),
            withheld,
        };
    }
    FilteredText {
        text: filtered,
        withheld,
    }
}

/// Filter a JSON value: every string, number and property the detector flags
/// is replaced by [`WITHHELD`]; a property whose own name is flagged is
/// removed.
///
/// ```
/// use serde_json::json;
/// use tur_pax::redact::{WITHHELD, filter_json};
///
/// let filtered = filter_json(&json!({ "user": "ada", "api_key": "k-123456", "port": 22 }));
/// assert_eq!(filtered.value, json!({ "user": "ada", "api_key": WITHHELD, "port": 22 }));
/// assert_eq!(filtered.withheld.len(), 1);
/// ```
pub fn filter_json(value: &Value) -> FilteredJson {
    let mut withheld = Vec::new();
    let filtered = filter_value(value, &mut withheld);
    // By construction nothing is left; if the detector disagrees, it wins.
    if let Some(kind) = detect_in_json(&filtered) {
        withheld.push(kind);
        return FilteredJson {
            value: Value::String(WITHHELD.to_owned()),
            withheld,
        };
    }
    FilteredJson {
        value: filtered,
        withheld,
    }
}

fn filter_value(value: &Value, withheld: &mut Vec<SecretKind>) -> Value {
    match value {
        Value::Null | Value::Bool(_) => value.clone(),
        Value::Number(number) => match detect(&number.to_string()) {
            Some(kind) => {
                withheld.push(kind);
                Value::String(WITHHELD.to_owned())
            }
            None => value.clone(),
        },
        Value::String(text) => match detect(text) {
            Some(kind) => {
                withheld.push(kind);
                Value::String(WITHHELD.to_owned())
            }
            None => value.clone(),
        },
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| filter_value(item, withheld))
                .collect(),
        ),
        Value::Object(map) => {
            let mut filtered = Map::new();
            for (name, item) in map {
                if let Some(kind) = detect(name) {
                    // The name itself is the secret: the property goes.
                    withheld.push(kind);
                    continue;
                }
                // A name that says the property holds a secret is enough,
                // whatever the value looks like. The detector decides that
                // on the pair.
                if item.is_string() {
                    let mut pair = Map::new();
                    pair.insert(name.clone(), item.clone());
                    if let Some(kind) = detect_in_json(&Value::Object(pair)) {
                        withheld.push(kind);
                        filtered.insert(name.clone(), Value::String(WITHHELD.to_owned()));
                        continue;
                    }
                }
                filtered.insert(name.clone(), filter_value(item, withheld));
            }
            Value::Object(filtered)
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // Values with the form of a credential. None of them is a real one.
    const TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
    const AWS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
    const CARD: &str = "4111 1111 1111 1111";

    #[test]
    fn the_marker_is_not_itself_taken_for_a_secret() {
        assert_eq!(detect(WITHHELD), None);
        assert_eq!(detect(&format!("{WITHHELD}\n{WITHHELD}\n2026 0001 123456")), None);
        assert_eq!(detect_in_json(&json!({ "password": WITHHELD })), None);
    }

    #[test]
    fn ordinary_output_passes_unchanged() {
        let text = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nHOME_URL=\"https://www.ubuntu.com/\"\n";
        let filtered = filter_text(text);
        assert_eq!(filtered.text, text);
        assert!(filtered.withheld.is_empty());

        let value = json!({ "findings": [{ "code": "TUR-DEVICE-DRIVER-001", "count": 2 }] });
        let filtered = filter_json(&value);
        assert_eq!(filtered.value, value);
        assert!(filtered.withheld.is_empty());
    }

    #[test]
    fn each_kind_of_secret_is_withheld_from_text_and_the_rest_is_kept() {
        let cases = [
            (format!("token {TOKEN}"), SecretKind::KnownToken),
            (format!("aws_access_key_id = {AWS_KEY}"), SecretKind::KnownToken),
            (format!("card {CARD}"), SecretKind::PaymentCard),
            ("password: hunter2".to_owned(), SecretKind::Password),
            (
                "your verification code is 482913".to_owned(),
                SecretKind::OneTimeCode,
            ),
            (
                "postgres://ada:s3cr3tpw@db.example.org/app".to_owned(),
                SecretKind::Password,
            ),
        ];
        for (line, kind) in cases {
            let text = format!("first line\n{line}\nlast line");
            let filtered = filter_text(&text);
            assert_eq!(
                filtered.text,
                format!("first line\n{WITHHELD}\nlast line"),
                "{kind:?}"
            );
            assert_eq!(filtered.withheld, vec![kind], "{line}");
        }
    }

    #[test]
    fn a_private_key_block_is_withheld_from_its_first_line_to_its_last() {
        let text = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\nAAAAB3NzaC1yc2E/AAAA\n-----END OPENSSH PRIVATE KEY-----\nafter";
        let filtered = filter_text(text);
        assert_eq!(filtered.text, format!("before\n{WITHHELD}\nafter"));
        assert_eq!(filtered.withheld, vec![SecretKind::PrivateKey]);

        // A block that never ends takes the rest of the output with it.
        let open = "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nIBAAKC";
        let filtered = filter_text(open);
        assert_eq!(filtered.text, format!("before\n{WITHHELD}"));
        assert!(!filtered.text.contains("MIIEow"));
    }

    #[test]
    fn a_secret_spread_over_lines_takes_the_whole_output() {
        // Harmless line by line; together, a password assignment.
        let text = "password\n= hunter2hunter2";
        assert_eq!(detect("password"), None);
        assert_eq!(detect("= hunter2hunter2"), None);
        let filtered = filter_text(text);
        assert_eq!(filtered.text, WITHHELD);
        assert_eq!(filtered.withheld.len(), 1);
    }

    #[test]
    fn json_is_filtered_value_by_value() {
        let value = json!({
            "name": "router",
            "settings": {
                "wifi_password": "correct horse",
                "note": format!("the token is {TOKEN}"),
                "ports": [22, 443]
            },
            "cards": [4111111111111111_u64, 7],
            "list": ["plain", AWS_KEY]
        });
        let filtered = filter_json(&value);
        assert_eq!(
            filtered.value,
            json!({
                "name": "router",
                "settings": {
                    "wifi_password": WITHHELD,
                    "note": WITHHELD,
                    "ports": [22, 443]
                },
                "cards": [WITHHELD, 7],
                "list": ["plain", WITHHELD]
            })
        );
        assert_eq!(filtered.withheld.len(), 4);
        let text = filtered.value.to_string();
        for secret in [TOKEN, AWS_KEY, "correct horse", "4111111111111111"] {
            assert!(!text.contains(secret), "{secret} was kept");
        }
    }

    #[test]
    fn a_property_whose_name_is_a_secret_is_removed() {
        let mut map = Map::new();
        map.insert(TOKEN.to_owned(), json!(1));
        map.insert("kept".to_owned(), json!(2));
        let filtered = filter_json(&Value::Object(map));
        assert_eq!(filtered.value, json!({ "kept": 2 }));
        assert_eq!(filtered.withheld, vec![SecretKind::KnownToken]);
    }

    #[test]
    fn every_kind_has_its_own_name() {
        let names: std::collections::BTreeSet<&str> =
            SecretKind::ALL.iter().map(|kind| kind_name(*kind)).collect();
        assert_eq!(names.len(), SecretKind::ALL.len());
    }
}
