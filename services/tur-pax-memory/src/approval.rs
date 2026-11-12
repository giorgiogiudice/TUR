// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Standing approvals: what the user approved in advance for one schedule.
//!
//! Addendum A section 5.6: a standing approval "names one tool and its exact
//! parameters. An action that differs in any parameter does not match."
//!
//! # Two types
//!
//! - [`ApprovalRequest`] is what a proposal carries: the assistant saying
//!   "this task will need to make this call". It is plain data anyone can
//!   build, and it authorises nothing.
//! - [`StandingApproval`] is what a schedule carries after the user created
//!   it. It has no public constructor and cannot be deserialised from outside
//!   this crate: it comes into being only when a schedule is built from a
//!   proposal and a user confirmation, or is read back from the store.
//!
//! # Mapping to the policy engine
//!
//! This crate does not depend on `tur-pax`. A [`StandingApproval`]
//! corresponds one-to-one to `tur_pax::context::StandingApproval`:
//!
//! | This crate | Policy engine | |
//! |---|---|---|
//! | `service` | `service` | the D-Bus name of the service that owns the tool |
//! | `tool` | `tool` | the tool name |
//! | `arguments` | `arguments` | the complete arguments, a JSON object |
//!
//! The JSON form is the same on both sides (three properties, no others), so a
//! stored approval can be handed to the engine by serialising it here and
//! deserialising it there.
//!
//! # Matching
//!
//! [`StandingApproval::matches`] is exact: the service and the tool are equal
//! byte for byte, the arguments have the same property names, and each value
//! is identical. Identical means the same JSON type and the same content;
//! strings are compared byte for byte with no normalisation of case, white
//! space or Unicode form; arrays are compared in order; the order of an
//! object's properties does not matter.
//!
//! Numbers are compared as written, as far as a parsed document allows: an
//! integer never matches a number with a fraction or an exponent, so `1`,
//! `1.0` and `1e0` are three different arguments, and `0.0` does not match
//! `-0.0`. Two spellings that parse to the same 64-bit floating-point value,
//! such as `1.0` and `1.00`, cannot be told apart after parsing and do match;
//! they are the same number of the same kind.
//!
//! This is at least as strict as the engine's comparison: whatever matches
//! here matches there. The one known difference is that the engine treats
//! `0.0` and `-0.0` as equal and this crate does not.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::codes::{Code, Problem};
use crate::entry::{is_service_name, is_tool_name};

/// The largest number of standing approvals on one schedule.
pub const MAX_STANDING_APPROVALS: usize = 16;

/// The largest size of an approval's arguments, as JSON, in bytes.
pub const MAX_APPROVAL_ARGUMENT_BYTES: usize = 16 * 1024;

/// A call the assistant says a scheduled task will need to make. Part of a
/// proposal; it authorises nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRequest {
    /// The D-Bus name of the service that owns the tool.
    pub service: String,
    /// The tool name.
    pub tool: String,
    /// The exact arguments.
    pub arguments: Map<String, Value>,
}

impl ApprovalRequest {
    /// Check the request: a service name, a tool name, and arguments within
    /// the size limit.
    pub fn validate(&self) -> Result<(), Problem> {
        let invalid = |detail: &str| Problem::new(Code::ScheduleInvalidApproval, detail);
        if !is_service_name(&self.service) {
            return Err(invalid("the approval does not name a service"));
        }
        if !is_tool_name(&self.tool) {
            return Err(invalid("the approval does not name a tool"));
        }
        let size = serde_json::to_vec(&self.arguments).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_APPROVAL_ARGUMENT_BYTES {
            return Err(invalid("the arguments of the approval are too large"));
        }
        Ok(())
    }
}

/// An approval the user gave when creating a schedule, for one exact tool
/// call. Stored with the schedule that owns it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StandingApproval {
    service: String,
    tool: String,
    arguments: Map<String, Value>,
}

impl StandingApproval {
    /// The approval for a request. Called only while a schedule is built from
    /// a confirmed proposal or read from the store.
    pub(crate) fn granted(request: ApprovalRequest) -> Result<StandingApproval, Problem> {
        request.validate()?;
        Ok(StandingApproval {
            service: request.service,
            tool: request.tool,
            arguments: request.arguments,
        })
    }

    /// The same content as a request, for writing to the store.
    pub(crate) fn to_request(&self) -> ApprovalRequest {
        ApprovalRequest {
            service: self.service.clone(),
            tool: self.tool.clone(),
            arguments: self.arguments.clone(),
        }
    }

    /// The D-Bus name of the service that owns the tool.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The tool name.
    pub fn tool(&self) -> &str {
        &self.tool
    }

    /// The exact arguments the approval covers.
    pub fn arguments(&self) -> &Map<String, Value> {
        &self.arguments
    }

    /// Whether the approval covers exactly this call. See the module
    /// documentation for what "exactly" means.
    pub fn matches(&self, service: &str, tool: &str, arguments: &Map<String, Value>) -> bool {
        self.service == service && self.tool == tool && maps_identical(&self.arguments, arguments)
    }

    /// Whether the approval is for this tool, whatever the arguments. Used to
    /// say why a call was deferred, never to allow one.
    pub fn names(&self, service: &str, tool: &str) -> bool {
        self.service == service && self.tool == tool
    }
}

fn maps_identical(a: &Map<String, Value>, b: &Map<String, Value>) -> bool {
    a.len() == b.len()
        && a.iter().all(|(name, value)| {
            b.get(name)
                .is_some_and(|other| values_identical(value, other))
        })
}

/// Whether two JSON values are identical in the sense of this module.
pub fn values_identical(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => numbers_identical(x, y),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_identical(p, q))
        }
        (Value::Object(x), Value::Object(y)) => maps_identical(x, y),
        _ => false,
    }
}

// The same kind of number (unsigned integer, signed integer, floating point)
// and the same canonical text.
fn numbers_identical(a: &Number, b: &Number) -> bool {
    a.is_u64() == b.is_u64()
        && a.is_i64() == b.is_i64()
        && a.is_f64() == b.is_f64()
        && a.to_string() == b.to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn approval(arguments: Value) -> StandingApproval {
        StandingApproval::granted(ApprovalRequest {
            service: "org.tur.Mail1".to_owned(),
            tool: "mail_send".to_owned(),
            arguments: object(arguments),
        })
        .expect("a valid approval")
    }

    // Parse the arguments from text, so that numbers arrive as they were
    // written.
    fn parsed(text: &str) -> Map<String, Value> {
        object(serde_json::from_str(text).expect("valid JSON"))
    }

    #[test]
    fn an_approval_matches_the_exact_call() {
        let approved = approval(json!({
            "to": "a@example.org",
            "count": 1,
            "ratio": 0.5,
            "tags": ["x", "y"],
            "options": { "urgent": false, "note": null }
        }));
        let same = object(json!({
            "to": "a@example.org",
            "count": 1,
            "ratio": 0.5,
            "tags": ["x", "y"],
            "options": { "urgent": false, "note": null }
        }));
        assert!(approved.matches("org.tur.Mail1", "mail_send", &same));
        // The order of properties does not matter, at any depth.
        let reordered = parsed(
            r#"{"options":{"note":null,"urgent":false},"tags":["x","y"],"ratio":0.5,"count":1,"to":"a@example.org"}"#,
        );
        assert!(approved.matches("org.tur.Mail1", "mail_send", &reordered));
    }

    #[test]
    fn near_misses_do_not_match() {
        let approved = approval(json!({ "to": "a@example.org", "count": 1, "tags": ["x"] }));
        let near_misses = [
            // One argument different.
            r#"{"to":"b@example.org","count":1,"tags":["x"]}"#,
            r#"{"to":"A@example.org","count":1,"tags":["x"]}"#,
            r#"{"to":"a@example.org ","count":1,"tags":["x"]}"#,
            r#"{"to":"a@example.org","count":2,"tags":["x"]}"#,
            r#"{"to":"a@example.org","count":1,"tags":["y"]}"#,
            // An extra argument, at the top level and inside an array.
            r#"{"to":"a@example.org","count":1,"tags":["x"],"cc":"c@example.org"}"#,
            r#"{"to":"a@example.org","count":1,"tags":["x","y"]}"#,
            r#"{"to":"a@example.org","count":1,"tags":["x"],"bcc":null}"#,
            // A missing argument.
            r#"{"to":"a@example.org","count":1}"#,
            r#"{"to":"a@example.org","count":1,"tags":[]}"#,
            r#"{}"#,
            // The same number written differently.
            r#"{"to":"a@example.org","count":1.0,"tags":["x"]}"#,
            r#"{"to":"a@example.org","count":1e0,"tags":["x"]}"#,
            r#"{"to":"a@example.org","count":"1","tags":["x"]}"#,
            r#"{"to":"a@example.org","count":true,"tags":["x"]}"#,
            r#"{"to":"a@example.org","count":[1],"tags":["x"]}"#,
            // A different type in place of a value.
            r#"{"to":["a@example.org"],"count":1,"tags":["x"]}"#,
            r#"{"to":"a@example.org","count":1,"tags":"x"}"#,
            r#"{"to":"a@example.org","count":1,"tags":{"0":"x"}}"#,
            // A property name that differs only in case.
            r#"{"To":"a@example.org","count":1,"tags":["x"]}"#,
        ];
        for text in near_misses {
            assert!(
                !approved.matches("org.tur.Mail1", "mail_send", &parsed(text)),
                "{text}"
            );
        }
        // The exact call, parsed from text, does match.
        assert!(approved.matches(
            "org.tur.Mail1",
            "mail_send",
            &parsed(r#"{"to":"a@example.org","count":1,"tags":["x"]}"#)
        ));
    }

    #[test]
    fn number_formatting_is_part_of_the_approval() {
        let pairs = [
            ("1", "1.0"),
            ("1", "1e0"),
            ("10", "1e1"),
            ("0", "0.0"),
            ("0", "-0"),
            ("0.0", "-0.0"),
            ("-1", "-1.0"),
            ("100", "1.0e2"),
            ("1", "\"1\""),
            ("18446744073709551615", "1.8446744073709552e19"),
        ];
        for (approved_text, called_text) in pairs {
            let approved = approval(json!({}));
            let approved = StandingApproval {
                arguments: parsed(&format!("{{\"n\":{approved_text}}}")),
                ..approved
            };
            let called = parsed(&format!("{{\"n\":{called_text}}}"));
            assert!(
                !approved.matches("org.tur.Mail1", "mail_send", &called),
                "{approved_text} matched {called_text}"
            );
            // And the other way round.
            let approved = StandingApproval {
                arguments: called,
                ..approved
            };
            let called = parsed(&format!("{{\"n\":{approved_text}}}"));
            assert!(
                !approved.matches("org.tur.Mail1", "mail_send", &called),
                "{called_text} matched {approved_text}"
            );
        }
        // Each spelling matches itself.
        for text in ["1", "1.0", "-0.0", "1e300", "-7", "18446744073709551615"] {
            let arguments = parsed(&format!("{{\"n\":{text}}}"));
            let approved = StandingApproval {
                arguments: arguments.clone(),
                ..approval(json!({}))
            };
            assert!(
                approved.matches("org.tur.Mail1", "mail_send", &arguments),
                "{text}"
            );
        }
    }

    #[test]
    fn an_approval_is_bound_to_its_service_and_tool() {
        let arguments = object(json!({ "to": "a@example.org" }));
        let approved = approval(json!({ "to": "a@example.org" }));
        assert!(approved.matches("org.tur.Mail1", "mail_send", &arguments));
        assert!(!approved.matches("org.tur.Mail1", "mail_send_all", &arguments));
        assert!(!approved.matches("org.tur.Mail1", "mail_sen", &arguments));
        assert!(!approved.matches("org.tur.Mail2", "mail_send", &arguments));
        assert!(!approved.matches("org.tur.mail1", "mail_send", &arguments));
        assert!(!approved.matches("", "", &arguments));
        assert!(approved.names("org.tur.Mail1", "mail_send"));
        assert!(!approved.names("org.tur.Mail1", "mail_delete"));
    }

    #[test]
    fn an_approval_has_the_shape_the_policy_engine_reads() {
        let approved = approval(json!({ "to": "a@example.org" }));
        let value = serde_json::to_value(&approved).expect("serialises");
        assert_eq!(
            value,
            json!({
                "service": "org.tur.Mail1",
                "tool": "mail_send",
                "arguments": { "to": "a@example.org" }
            })
        );
        assert_eq!(approved.service(), "org.tur.Mail1");
        assert_eq!(approved.tool(), "mail_send");
        assert_eq!(
            approved.arguments(),
            &object(json!({ "to": "a@example.org" }))
        );
        assert_eq!(approved.to_request().arguments, *approved.arguments());
    }

    #[test]
    fn a_request_must_name_a_service_and_a_tool() {
        let request = |service: &str, tool: &str| ApprovalRequest {
            service: service.to_owned(),
            tool: tool.to_owned(),
            arguments: Map::new(),
        };
        assert!(request("org.tur.Mail1", "mail_send").validate().is_ok());
        for (service, tool) in [
            ("", "mail_send"),
            ("mail", "mail_send"),
            ("org.tur.Mail1", ""),
            ("org.tur.Mail1", "*"),
            ("org.tur.Mail1", "Mail_Send"),
            ("org.tur.*", "mail_send"),
            ("org.tur.Mail1", "mail send"),
        ] {
            let problem = request(service, tool).validate().expect_err("invalid");
            assert_eq!(
                problem.code,
                Code::ScheduleInvalidApproval,
                "{service} {tool}"
            );
        }
        let mut large = request("org.tur.Mail1", "mail_send");
        large.arguments.insert(
            "body".to_owned(),
            Value::String("x".repeat(MAX_APPROVAL_ARGUMENT_BYTES)),
        );
        assert!(large.validate().is_err());
        // Unknown fields are not accepted in a request.
        let text = r#"{"service":"s.s","tool":"t","arguments":{},"class":0}"#;
        assert!(serde_json::from_str::<ApprovalRequest>(text).is_err());
        let text = r#"{"service":"s.s","tool":"t"}"#;
        assert!(serde_json::from_str::<ApprovalRequest>(text).is_err());
    }
}
