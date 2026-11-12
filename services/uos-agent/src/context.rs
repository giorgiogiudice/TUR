// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The circumstances in which an action is proposed.
//!
//! A [`Context`] is assembled by the orchestrator from trusted sources: the
//! session manager, the compositor, the repair-session record and the schedule
//! store. Nothing in it may come from the model or from the assistant's memory,
//! with one exception: the model's "consequential" hint, which can only make a
//! decision stricter.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::action::ToolCall;

/// A point in time, in whole seconds since the Unix epoch (UTC).
///
/// The policy engine never reads a clock. The caller passes the time in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp(pub u64);

/// Whether a person is present for the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Presence {
    /// The user is present and asked for the task.
    Attended,
    /// A scheduled task or a reminder is firing; nobody can be asked.
    Unattended,
}

impl Presence {
    /// Both values.
    pub const ALL: [Presence; 2] = [Presence::Attended, Presence::Unattended];
}

/// A repair session: the user authenticated once on the trusted path, for a
/// stated purpose and a limited time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairSession {
    /// The purpose the user authenticated for.
    pub purpose: String,
    /// The moment the session ends. The session is closed at this moment.
    pub expires_at: Timestamp,
}

impl RepairSession {
    /// Whether the session is open at `now`.
    ///
    /// A session with no stated purpose is never open. The engine also treats
    /// every session as closed while the session is locked or unattended.
    pub fn is_open_at(&self, now: Timestamp) -> bool {
        !self.purpose.trim().is_empty() && now < self.expires_at
    }
}

/// An approval the user gave in advance, when creating a schedule, for one
/// exact tool call.
///
/// It names the tool and fixes every argument. A call matches only if the
/// service, the tool and the complete arguments are equal. JSON numbers compare
/// as written: `1` and `1.0` are different.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingApproval {
    /// The D-Bus name of the service that owns the tool.
    pub service: String,
    /// The tool name.
    pub tool: String,
    /// The exact arguments the approval covers.
    pub arguments: Map<String, Value>,
}

impl StandingApproval {
    /// Whether the approval covers exactly this call.
    pub fn matches(&self, call: &ToolCall) -> bool {
        self.service == call.service && self.tool == call.tool && self.arguments == call.arguments
    }
}

/// The circumstances in which an action is proposed.
#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    /// Whether a person is present for the task.
    pub presence: Presence,
    /// Whether the session is locked.
    pub session_locked: bool,
    /// Whether all five compositor guarantees of the trusted confirmation path
    /// hold (addendum section 6). Until they do, only classes 0 and 1 are
    /// available.
    pub trusted_path_available: bool,
    /// The repair session, if the user opened one.
    pub repair_session: Option<RepairSession>,
    /// The standing approvals attached to the schedule that started the task.
    pub standing_approvals: Vec<StandingApproval>,
    /// The model flagged the action as consequential. This can raise the class
    /// to 2; it can never lower anything.
    pub model_flags_consequential: bool,
}

impl Context {
    /// An attended, unlocked context with nothing granted: no trusted path, no
    /// repair session, no standing approvals, no model hint.
    pub fn attended() -> Self {
        Context {
            presence: Presence::Attended,
            session_locked: false,
            trusted_path_available: false,
            repair_session: None,
            standing_approvals: Vec::new(),
            model_flags_consequential: false,
        }
    }

    /// An unattended, unlocked context with nothing granted.
    pub fn unattended() -> Self {
        Context {
            presence: Presence::Unattended,
            ..Context::attended()
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::class::ActionClass;

    fn call(arguments: Value) -> ToolCall {
        ToolCall {
            service: "org.uos.Test1".to_owned(),
            tool: "mail_send".to_owned(),
            arguments: arguments.as_object().cloned().unwrap_or_default(),
            declared_class: ActionClass::Consequential,
        }
    }

    fn approval(arguments: Value) -> StandingApproval {
        StandingApproval {
            service: "org.uos.Test1".to_owned(),
            tool: "mail_send".to_owned(),
            arguments: arguments.as_object().cloned().unwrap_or_default(),
        }
    }

    #[test]
    fn repair_session_closes_exactly_at_expiry() {
        let session = RepairSession {
            purpose: "fix printing".to_owned(),
            expires_at: Timestamp(100),
        };
        assert!(session.is_open_at(Timestamp(0)));
        assert!(session.is_open_at(Timestamp(99)));
        assert!(!session.is_open_at(Timestamp(100)));
        assert!(!session.is_open_at(Timestamp(101)));
    }

    #[test]
    fn repair_session_without_a_purpose_is_never_open() {
        for purpose in ["", "   ", "\t\n"] {
            let session = RepairSession {
                purpose: purpose.to_owned(),
                expires_at: Timestamp(100),
            };
            assert!(!session.is_open_at(Timestamp(0)));
        }
    }

    #[test]
    fn standing_approval_matches_only_the_exact_call() {
        let approved = approval(json!({ "to": "a@example.org", "count": 1, "tags": ["x"] }));

        assert!(approved.matches(&call(json!({ "to": "a@example.org", "count": 1, "tags": ["x"] }))));
        // Key order does not matter; everything else does.
        assert!(approved.matches(&call(json!({ "tags": ["x"], "count": 1, "to": "a@example.org" }))));

        let near_misses = [
            json!({ "to": "b@example.org", "count": 1, "tags": ["x"] }),
            json!({ "to": "A@example.org", "count": 1, "tags": ["x"] }),
            json!({ "to": "a@example.org ", "count": 1, "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": 2, "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": 1.0, "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": "1", "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": 1, "tags": ["x", "y"] }),
            json!({ "to": "a@example.org", "count": 1, "tags": [] }),
            json!({ "to": "a@example.org", "count": 1 }),
            json!({ "to": "a@example.org", "count": 1, "tags": ["x"], "cc": "c@example.org" }),
            json!({}),
        ];
        for arguments in near_misses {
            assert!(!approved.matches(&call(arguments.clone())), "{arguments}");
        }
    }

    #[test]
    fn standing_approval_is_bound_to_service_and_tool() {
        let arguments = json!({ "to": "a@example.org" });
        let approved = approval(arguments.clone());

        let mut other_tool = call(arguments.clone());
        other_tool.tool = "mail_send_all".to_owned();
        assert!(!approved.matches(&other_tool));

        let mut other_service = call(arguments.clone());
        other_service.service = "org.uos.Other1".to_owned();
        assert!(!approved.matches(&other_service));

        assert!(approved.matches(&call(arguments)));
    }

    #[test]
    fn standing_approval_rejects_unknown_fields() {
        let text = r#"{"service":"s.s","tool":"t","arguments":{},"class":0}"#;
        assert!(serde_json::from_str::<StandingApproval>(text).is_err());
        let text = r#"{"service":"s.s","tool":"t"}"#;
        assert!(serde_json::from_str::<StandingApproval>(text).is_err());
    }
}
