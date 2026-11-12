// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The circumstances in which an action is proposed.
//!
//! A [`Context`] is assembled by the orchestrator from trusted sources: the
//! session manager, the compositor, the repair-session record and the schedule
//! store. The policy file, the standing approvals and the repair-session
//! record are owned by a separate system component; the assistant reads them
//! through it and cannot write them (addendum section 17, item 5). Nothing in
//! a context may come from the model or from the assistant's memory, with one
//! exception: the model's "consequential" hint, which can only make a decision
//! stricter.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::action::ToolCall;

/// A point in time, in whole seconds.
///
/// The policy engine never reads a clock. The caller passes the time in, and
/// must take it from the same clock as the times in the [`RepairSession`]. That
/// clock should not be one the user or the network can set back: see
/// [`RepairSession::is_open_at`].
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

/// Whose process the window targeted by an accessibility or screen action
/// belongs to, as reported by the compositor (addendum section 17, item 2).
///
/// A click in a window of a process with administrator rights can have a
/// class 3 effect with no prompt at all, so such a window is out of reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WindowOwner {
    /// The compositor did not say. Acting in the window is then treated as
    /// class 2: when in doubt, ask.
    #[default]
    Unknown,
    /// A process running as the user, with no administrator rights.
    User,
    /// A process running with administrator rights. Accessibility and screen
    /// actions on the window are refused.
    Administrator,
}

impl WindowOwner {
    /// Every value.
    pub const ALL: [WindowOwner; 3] = [
        WindowOwner::Unknown,
        WindowOwner::User,
        WindowOwner::Administrator,
    ];
}

/// The length of a repair session when the user does not choose one, in
/// seconds: 15 minutes (addendum section 17, item 10).
pub const DEFAULT_REPAIR_SESSION_SECONDS: u64 = 15 * 60;

/// The longest a repair session may last, in seconds: 60 minutes (addendum
/// section 17, item 10). A record that claims more is not a repair session,
/// and is treated as closed.
pub const MAX_REPAIR_SESSION_SECONDS: u64 = 60 * 60;

/// A repair session: the user authenticated once on the trusted path, for a
/// stated purpose and a limited time (addendum section 5.5).
///
/// The record is written by the system component that owns it, never by the
/// assistant. That component sets [`RepairSession::ended`] when the user ends
/// the session, when the task finishes and when the session locks: a session
/// that was locked does not reopen when it is unlocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairSession {
    /// The purpose the user authenticated for.
    pub purpose: String,
    /// The moment the user authenticated. The session is not open before it.
    pub opened_at: Timestamp,
    /// The moment the session ends. The session is closed at this moment.
    pub expires_at: Timestamp,
    /// The session was ended before its time: by the user, by the end of the
    /// task, or by a lock.
    pub ended: bool,
}

impl RepairSession {
    /// A session of the default length, opened at `opened_at`.
    ///
    /// Returns `None` if the purpose is blank or the expiry cannot be
    /// represented.
    pub fn open(purpose: impl Into<String>, opened_at: Timestamp) -> Option<RepairSession> {
        RepairSession::with_length(purpose, opened_at, DEFAULT_REPAIR_SESSION_SECONDS)
    }

    /// A session of `seconds`, opened at `opened_at`.
    ///
    /// Returns `None` if the purpose is blank, if `seconds` is zero or more
    /// than [`MAX_REPAIR_SESSION_SECONDS`], or if the expiry cannot be
    /// represented.
    pub fn with_length(
        purpose: impl Into<String>,
        opened_at: Timestamp,
        seconds: u64,
    ) -> Option<RepairSession> {
        let purpose = purpose.into();
        if purpose.trim().is_empty() || seconds == 0 || seconds > MAX_REPAIR_SESSION_SECONDS {
            return None;
        }
        let expires_at = opened_at.0.checked_add(seconds)?;
        Some(RepairSession {
            purpose,
            opened_at,
            expires_at: Timestamp(expires_at),
            ended: false,
        })
    }

    /// Whether the session is open at `now`.
    ///
    /// A session is open only if all of these hold: it was not ended; it has
    /// a stated purpose; `now` is not before the moment it was opened; `now`
    /// is before its expiry; and from opening to expiry it lasts no more than
    /// [`MAX_REPAIR_SESSION_SECONDS`]. The engine also treats every session as
    /// closed while the session is locked or the task is unattended.
    ///
    /// The check is on the times it is given. If the clock is set back to a
    /// moment inside the session, an expired session reads as open again. The
    /// owner of the record must therefore mark a session as ended when it
    /// expires, and should take its times from a clock that cannot be set
    /// back.
    pub fn is_open_at(&self, now: Timestamp) -> bool {
        !self.ended
            && !self.purpose.trim().is_empty()
            && self.opened_at <= now
            && now < self.expires_at
            && self.expires_at.0 - self.opened_at.0 <= MAX_REPAIR_SESSION_SECONDS
    }
}

/// An approval the user gave in advance, when creating a schedule, for one
/// exact tool call.
///
/// It names the tool and fixes every argument. A call matches only if the
/// service, the tool and the complete arguments are equal. An integer and a
/// number with a fraction or an exponent are different: `1` does not match
/// `1.0`. Two numbers with a fraction or an exponent are equal if they denote
/// the same 64-bit floating-point value.
///
/// Read from JSON, the arguments must not repeat a key at any depth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingApproval {
    /// The D-Bus name of the service that owns the tool.
    pub service: String,
    /// The tool name.
    pub tool: String,
    /// The exact arguments the approval covers.
    #[serde(deserialize_with = "crate::strict::object")]
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
    /// to 2 and add a confirmation; it can never lower anything.
    pub model_flags_consequential: bool,
    /// Whose process the window targeted by an accessibility or screen action
    /// belongs to. It has no effect on any other kind of action.
    pub target_window_owner: WindowOwner,
}

impl Context {
    /// An attended, unlocked context with nothing granted: no trusted path, no
    /// repair session, no standing approvals, no model hint, and a target
    /// window whose owner is not known.
    pub fn attended() -> Self {
        Context {
            presence: Presence::Attended,
            session_locked: false,
            trusted_path_available: false,
            repair_session: None,
            standing_approvals: Vec::new(),
            model_flags_consequential: false,
            target_window_owner: WindowOwner::Unknown,
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

    fn session(opened_at: u64, expires_at: u64) -> RepairSession {
        RepairSession {
            purpose: "fix printing".to_owned(),
            opened_at: Timestamp(opened_at),
            expires_at: Timestamp(expires_at),
            ended: false,
        }
    }

    #[test]
    fn repair_session_is_open_from_its_opening_until_just_before_its_expiry() {
        let session = session(100, 200);
        assert!(!session.is_open_at(Timestamp(0)));
        assert!(!session.is_open_at(Timestamp(99)));
        assert!(session.is_open_at(Timestamp(100)));
        assert!(session.is_open_at(Timestamp(199)));
        assert!(!session.is_open_at(Timestamp(200)));
        assert!(!session.is_open_at(Timestamp(201)));
        assert!(!session.is_open_at(Timestamp(u64::MAX)));
    }

    #[test]
    fn repair_session_without_a_purpose_is_never_open() {
        for purpose in ["", "   ", "\t\n"] {
            let session = RepairSession {
                purpose: purpose.to_owned(),
                ..session(0, 100)
            };
            assert!(!session.is_open_at(Timestamp(0)), "{purpose:?}");
            assert!(!session.is_open_at(Timestamp(50)), "{purpose:?}");
        }
    }

    #[test]
    fn repair_session_that_was_ended_is_never_open() {
        let ended = RepairSession {
            ended: true,
            ..session(0, 100)
        };
        for now in [0, 1, 50, 99, 100] {
            assert!(!ended.is_open_at(Timestamp(now)), "{now}");
        }
        assert!(session(0, 100).is_open_at(Timestamp(50)));
    }

    #[test]
    fn repair_session_lasts_at_most_an_hour() {
        let opened = 1_000;
        let longest = session(opened, opened + MAX_REPAIR_SESSION_SECONDS);
        assert!(longest.is_open_at(Timestamp(opened)));
        assert!(longest.is_open_at(Timestamp(opened + MAX_REPAIR_SESSION_SECONDS - 1)));
        assert!(!longest.is_open_at(Timestamp(opened + MAX_REPAIR_SESSION_SECONDS)));

        // One second too long: closed for its whole length, not only at the end.
        let too_long = session(opened, opened + MAX_REPAIR_SESSION_SECONDS + 1);
        for now in [
            opened,
            opened + 1,
            opened + MAX_REPAIR_SESSION_SECONDS,
            opened + MAX_REPAIR_SESSION_SECONDS + 1,
        ] {
            assert!(!too_long.is_open_at(Timestamp(now)), "{now}");
        }

        // The author's former bound of 24 hours, and no bound at all.
        assert!(!session(opened, opened + 86_400).is_open_at(Timestamp(opened + 10)));
        assert!(!session(0, u64::MAX).is_open_at(Timestamp(10)));
        assert!(!session(0, u64::MAX).is_open_at(Timestamp(u64::MAX - 1)));
    }

    #[test]
    fn repair_session_with_impossible_times_is_never_open() {
        // Expiry before or at the opening.
        for (opened_at, expires_at) in [(100, 100), (100, 99), (100, 0), (u64::MAX, 0)] {
            let session = session(opened_at, expires_at);
            for now in [0, 99, 100, 101, u64::MAX] {
                assert!(
                    !session.is_open_at(Timestamp(now)),
                    "{opened_at}..{expires_at} at {now}"
                );
            }
        }
        // The latest possible times do not overflow.
        let last = session(u64::MAX - 10, u64::MAX);
        assert!(last.is_open_at(Timestamp(u64::MAX - 1)));
        assert!(!last.is_open_at(Timestamp(u64::MAX)));
        assert!(!last.is_open_at(Timestamp(0)));
    }

    #[test]
    fn a_clock_set_back_before_the_opening_closes_the_session() {
        let session = session(1_000, 1_900);
        assert!(session.is_open_at(Timestamp(1_500)));
        // Back to before the session was opened: closed.
        assert!(!session.is_open_at(Timestamp(999)));
        // Back to a moment inside the session, after it had expired: the
        // engine cannot tell. The owner of the record has to mark it ended.
        assert!(!session.is_open_at(Timestamp(1_900)));
        assert!(session.is_open_at(Timestamp(1_001)));
        let ended = RepairSession {
            ended: true,
            ..session
        };
        assert!(!ended.is_open_at(Timestamp(1_001)));
    }

    #[test]
    fn the_constructors_give_the_default_length_and_refuse_what_is_not_a_session() {
        let opened = Timestamp(5_000);
        let default = RepairSession::open("fix printing", opened).expect("a session");
        assert_eq!(default.opened_at, opened);
        assert_eq!(
            default.expires_at,
            Timestamp(5_000 + DEFAULT_REPAIR_SESSION_SECONDS)
        );
        assert_eq!(DEFAULT_REPAIR_SESSION_SECONDS, 900);
        assert_eq!(MAX_REPAIR_SESSION_SECONDS, 3_600);
        assert!(!default.ended);
        assert!(default.is_open_at(opened));

        let longest = RepairSession::with_length("fix printing", opened, 3_600).expect("a session");
        assert!(longest.is_open_at(Timestamp(5_000 + 3_599)));

        assert_eq!(RepairSession::with_length("fix printing", opened, 0), None);
        assert_eq!(
            RepairSession::with_length("fix printing", opened, 3_601),
            None
        );
        assert_eq!(
            RepairSession::with_length("fix printing", opened, u64::MAX),
            None
        );
        assert_eq!(RepairSession::open("", opened), None);
        assert_eq!(RepairSession::open("  \n", opened), None);
        assert_eq!(
            RepairSession::open("fix printing", Timestamp(u64::MAX)),
            None
        );
        assert_eq!(
            RepairSession::open("fix printing", Timestamp(u64::MAX - 899)),
            None
        );
        assert!(RepairSession::open("fix printing", Timestamp(u64::MAX - 900)).is_some());
    }

    #[test]
    fn standing_approval_matches_only_the_exact_call() {
        let approved = approval(json!({ "to": "a@example.org", "count": 1, "tags": ["x"] }));

        assert!(approved.matches(&call(
            json!({ "to": "a@example.org", "count": 1, "tags": ["x"] })
        )));
        // Key order does not matter; everything else does.
        assert!(approved.matches(&call(
            json!({ "tags": ["x"], "count": 1, "to": "a@example.org" })
        )));

        let near_misses = [
            json!({ "to": "b@example.org", "count": 1, "tags": ["x"] }),
            json!({ "to": "A@example.org", "count": 1, "tags": ["x"] }),
            json!({ "to": "a@example.org ", "count": 1, "tags": ["x"] }),
            json!({ "to": "a@example.org\u{200b}", "count": 1, "tags": ["x"] }),
            json!({ "to": "\u{430}@example.org", "count": 1, "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": 2, "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": 1.0, "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": -1, "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": "1", "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": true, "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": null, "tags": ["x"] }),
            json!({ "to": "a@example.org", "count": 1, "tags": ["x", "y"] }),
            json!({ "to": "a@example.org", "count": 1, "tags": ["X"] }),
            json!({ "to": "a@example.org", "count": 1, "tags": [["x"]] }),
            json!({ "to": "a@example.org", "count": 1, "tags": [] }),
            json!({ "to": "a@example.org", "count": 1, "tags": "x" }),
            json!({ "to": "a@example.org", "count": 1 }),
            json!({ "to": "a@example.org", "count": 1, "tags": ["x"], "cc": "c@example.org" }),
            json!({ "To": "a@example.org", "count": 1, "tags": ["x"] }),
            json!({}),
        ];
        for arguments in near_misses {
            assert!(!approved.matches(&call(arguments.clone())), "{arguments}");
        }
    }

    #[test]
    fn standing_approval_compares_numbers_by_kind_and_value() {
        let parse = |text: &str| -> Value { serde_json::from_str(text).expect("valid JSON") };
        let matches = |approved: &str, proposed: &str| {
            approval(parse(approved)).matches(&call(parse(proposed)))
        };
        // An integer is not a number with a fraction.
        assert!(!matches(r#"{"n": 1}"#, r#"{"n": 1.0}"#));
        assert!(!matches(r#"{"n": 0}"#, r#"{"n": 0.0}"#));
        assert!(!matches(r#"{"n": 100}"#, r#"{"n": 1e2}"#));
        // Integers are exact over their whole range.
        assert!(!matches(
            r#"{"n": 9007199254740992}"#,
            r#"{"n": 9007199254740993}"#
        ));
        assert!(!matches(
            r#"{"n": 18446744073709551614}"#,
            r#"{"n": 18446744073709551615}"#
        ));
        assert!(matches(
            r#"{"n": 18446744073709551615}"#,
            r#"{"n": 18446744073709551615}"#
        ));
        // Two spellings of one floating-point value are the same value.
        assert!(matches(r#"{"n": 1.0}"#, r#"{"n": 1.00}"#));
        assert!(matches(r#"{"n": 2.5}"#, r#"{"n": 25e-1}"#));
        assert!(!matches(r#"{"n": 0.1}"#, r#"{"n": 0.2}"#));
    }

    #[test]
    fn standing_approval_is_bound_to_service_and_tool() {
        let arguments = json!({ "to": "a@example.org" });
        let approved = approval(arguments.clone());

        let mut other_tool = call(arguments.clone());
        other_tool.tool = "mail_send_all".to_owned();
        assert!(!approved.matches(&other_tool));

        let mut other_case = call(arguments.clone());
        other_case.tool = "Mail_send".to_owned();
        assert!(!approved.matches(&other_case));

        let mut other_service = call(arguments.clone());
        other_service.service = "org.uos.Other1".to_owned();
        assert!(!approved.matches(&other_service));

        assert!(approved.matches(&call(arguments)));
    }

    #[test]
    fn standing_approval_rejects_unknown_missing_and_repeated_fields() {
        let good = r#"{"service":"s.s","tool":"t","arguments":{"a":1}}"#;
        assert!(serde_json::from_str::<StandingApproval>(good).is_ok());
        for text in [
            r#"{"service":"s.s","tool":"t","arguments":{},"class":0}"#,
            r#"{"service":"s.s","tool":"t"}"#,
            r#"{"service":"s.s","tool":"t","arguments":[]}"#,
            r#"{"service":"s.s","tool":"t","arguments":"{}"}"#,
            r#"{"service":"s.s","service":"x.x","tool":"t","arguments":{}}"#,
            r#"{"service":"s.s","tool":"t","tool":"u","arguments":{}}"#,
            r#"{"service":"s.s","tool":"t","arguments":{},"arguments":{}}"#,
            r#"{"service":"s.s","tool":"t","arguments":{"a":1,"a":2}}"#,
            r#"{"service":"s.s","tool":"t","arguments":{"a":{"b":1,"b":1}}}"#,
        ] {
            assert!(
                serde_json::from_str::<StandingApproval>(text).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn a_context_grants_nothing_unless_stated() {
        for context in [Context::attended(), Context::unattended()] {
            assert!(!context.session_locked);
            assert!(!context.trusted_path_available);
            assert_eq!(context.repair_session, None);
            assert!(context.standing_approvals.is_empty());
            assert!(!context.model_flags_consequential);
            assert_eq!(context.target_window_owner, WindowOwner::Unknown);
        }
        assert_eq!(WindowOwner::default(), WindowOwner::Unknown);
    }
}
