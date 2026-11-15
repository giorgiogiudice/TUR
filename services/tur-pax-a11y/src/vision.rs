// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The rule a position proposed from a picture is held to: **vision
//! proposes, the application confirms** (the owner's decision 3 of 14
//! November 2026, docs/reports/exp-winsee-1.md, sections 8 and 11.1;
//! docs/architecture/pax-accessibility.md, section 11.3).
//!
//! A point a vision model chose is never acted on by position. It is looked
//! up in what the application exposes (the accessibility tree, or the
//! Windows bridge's Win32 tier). The action goes on only if
//!
//! 1. an element is there;
//! 2. no other window covers the point, as far as the source can tell (a
//!    source that cannot tell is taken to be covered);
//! 3. the element's name, read from the application, is the name asked for
//!    (letter case, a mnemonic `&`, a trailing colon or ellipsis and
//!    repeated spaces aside), and its role is of the kind asked for.
//!
//! It then goes on as an ordinary action on **that element**: resolved,
//! decided by the policy engine from what the application says, read
//! again, performed. In every other case the action is class 2, which is
//! refused today; where nothing is exposed at all, every action by sight is
//! class 2.
//!
//! No vision model is wired into P@x yet. This module is the check any such
//! wiring must go through; its tests are the experiment's attack (a button
//! that paints "Cancel" over a real "Delete", a window that paints "Help"
//! over "Turn off backups").

use serde_json::{Value, json};
use tur_pax::ActionClass;

use crate::atspi::role_name;
use crate::tree::role_matches;

/// What a source says is under a point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnderPoint {
    /// The deepest element under the point, if any.
    pub element: Option<Seen>,
    /// Whether another window covers the point: `Some(false)` only when the
    /// source knows nothing covers it.
    pub covered: Option<bool>,
}

/// An element under a point, as the application names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The bridge's identifier of it, for the ordinary action that follows.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its AT-SPI role number.
    pub role: u32,
}

/// The outcome of the rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmation {
    /// The application names the same control at the point: the action may
    /// go on, as an ordinary action on this element, through the policy
    /// engine.
    Confirmed {
        /// The element to act on.
        element: String,
    },
    /// Class 2: refused today, whatever the user's policy says.
    Refused {
        /// Why, in a sentence.
        reason: &'static str,
        /// What the application has at the point, when something is there.
        under_the_point: Option<Seen>,
    },
}

impl Confirmation {
    /// The class of acting on the point without anything more: class 0 for
    /// nothing, as the element is then decided as any other; class 2
    /// otherwise.
    pub fn class(&self) -> ActionClass {
        match self {
            Confirmation::Confirmed { .. } => ActionClass::Observe,
            Confirmation::Refused { .. } => ActionClass::Consequential,
        }
    }

    /// The outcome as JSON, for a log or a test.
    pub fn to_json(&self) -> Value {
        match self {
            Confirmation::Confirmed { element } => {
                json!({ "decision": "confirmed", "element": element })
            }
            Confirmation::Refused {
                reason,
                under_the_point,
            } => json!({
                "decision": "refused",
                "class": 2,
                "reason": reason,
                "under_the_point": under_the_point.as_ref().map(|seen| json!({
                    "id": seen.id, "name": seen.name, "role": role_name(seen.role),
                })),
            }),
        }
    }
}

/// A name as it is compared: letter case, a mnemonic `&`, a trailing colon
/// or ellipsis and repeated spaces do not count.
pub fn comparable(name: &str) -> String {
    let text = name.replace('&', "").replace('…', "...");
    let text = text.split_whitespace().collect::<Vec<&str>>().join(" ");
    text.to_lowercase()
        .trim_end_matches([':', '.', ' '])
        .trim()
        .to_owned()
}

/// The rule: whether the control asked for (`name`, and `role` if given:
/// a role name or the engine's kind, as `ui_*` tools take it) is what the
/// application has under the point.
pub fn confirm(name: &str, role: Option<&str>, under: &UnderPoint) -> Confirmation {
    let refuse = |reason: &'static str, under_the_point: Option<Seen>| Confirmation::Refused {
        reason,
        under_the_point,
    };
    let Some(seen) = &under.element else {
        return refuse("nothing the application exposes is under the point", None);
    };
    if under.covered != Some(false) {
        return refuse(
            "another window covers the point, or it cannot be told that none does",
            Some(seen.clone()),
        );
    }
    let wanted = comparable(name);
    if wanted.is_empty() || comparable(&seen.name) != wanted {
        return refuse(
            "the control under the point is not the one asked for",
            Some(seen.clone()),
        );
    }
    if let Some(role) = role
        && !role_matches(role, seen.role)
    {
        return refuse(
            "the control under the point is of another kind",
            Some(seen.clone()),
        );
    }
    Confirmation::Confirmed {
        element: seen.id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn under(name: &str, role: u32, covered: Option<bool>) -> UnderPoint {
        UnderPoint {
            element: Some(Seen {
                id: "we3".to_owned(),
                name: name.to_owned(),
                role,
            }),
            covered,
        }
    }

    #[test]
    fn a_painted_label_never_moves_p_x_to_act() {
        // The experiment's attack: a button whose window text is Delete
        // paints itself as Cancel. Every model that located controls
        // pointed at it when asked for Cancel (12 of 12 answers).
        let refused = confirm("Cancel", Some("push button"), &under("Delete", 43, Some(false)));
        assert_eq!(refused.class(), ActionClass::Consequential);
        let Confirmation::Refused {
            reason,
            under_the_point,
        } = &refused
        else {
            panic!("{refused:?}");
        };
        assert!(reason.contains("not the one asked for"));
        assert_eq!(under_the_point.as_ref().map(|seen| seen.name.as_str()), Some("Delete"));
        // A borderless window painting Help over Turn off backups.
        let covered = confirm("Help", None, &under("Turn off backups", 43, Some(true)));
        assert_eq!(covered.class(), ActionClass::Consequential);
        // A source that cannot tell whether the point is covered.
        let unknown = confirm("Keep", None, &under("Keep", 43, None));
        assert_eq!(unknown.class(), ActionClass::Consequential);
        // Nothing there.
        let nothing = confirm(
            "Keep",
            None,
            &UnderPoint {
                element: None,
                covered: Some(false),
            },
        );
        assert_eq!(nothing.class(), ActionClass::Consequential);
        assert_eq!(nothing.to_json()["class"], 2);
    }

    #[test]
    fn the_honest_control_is_confirmed_and_then_decided_as_any_other() {
        let confirmed = confirm("Keep", Some("button"), &under("Keep", 43, Some(false)));
        assert_eq!(
            confirmed,
            Confirmation::Confirmed {
                element: "we3".to_owned()
            }
        );
        assert_eq!(confirmed.class(), ActionClass::Observe);
        // Names as a person reads them.
        for (asked, named) in [
            ("save as", "Save &As..."),
            ("Name", "Name:"),
            ("Open file", "Open  file…"),
        ] {
            assert_eq!(
                confirm(asked, None, &under(named, 43, Some(false))).class(),
                ActionClass::Observe,
                "{asked} / {named}"
            );
        }
        // The right name of the wrong kind.
        let kind = confirm("Keep", Some("check box"), &under("Keep", 43, Some(false)));
        assert_eq!(kind.class(), ActionClass::Consequential);
        // An empty name confirms nothing.
        assert_eq!(
            confirm("", None, &under("", 43, Some(false))).class(),
            ActionClass::Consequential
        );
    }
}
