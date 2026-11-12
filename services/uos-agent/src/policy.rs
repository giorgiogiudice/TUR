// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The user-adjustable policy (addendum section 5.3) and its file format.
//!
//! The user can make the assistant stricter at any time, and can relax the
//! handling of classes 1 and 2 for a named application. Nothing in this file
//! format can relax class 3 or class 4. The file is written by the user on the
//! trusted path; changing it is a class 4 operation for the assistant.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::codes::{Code, Problem};

/// The policy file schema version this build reads.
pub const POLICY_SCHEMA_VERSION: u64 = 1;

/// The global mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The rules of the addendum, unchanged.
    #[default]
    Normal,
    /// Ask before every action, of every class.
    AskEveryAction,
    /// Only class 0 actions are available.
    ObserveOnly,
}

impl Mode {
    /// Every mode.
    pub const ALL: [Mode; 3] = [Mode::Normal, Mode::AskEveryAction, Mode::ObserveOnly];

    /// The mode as written in the policy file.
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::AskEveryAction => "ask_every_action",
            Mode::ObserveOnly => "observe_only",
        }
    }
}

/// How far a relaxation goes. There is no level for class 3 or class 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RelaxationLevel {
    /// Class 1. Class 1 is already allowed in normal mode, and the stricter
    /// modes override every relaxation, so this level changes no decision.
    Reversible,
    /// Class 2: consequential accessibility and screen actions in the named
    /// application run without asking, in normal mode, while attended.
    Consequential,
}

impl RelaxationLevel {
    /// Both levels.
    pub const ALL: [RelaxationLevel; 2] =
        [RelaxationLevel::Reversible, RelaxationLevel::Consequential];

    /// The class number written in the policy file.
    pub const fn class_number(self) -> u8 {
        match self {
            RelaxationLevel::Reversible => 1,
            RelaxationLevel::Consequential => 2,
        }
    }
}

/// A relaxation for one named application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relaxation {
    /// The application identifier. Matched exactly, including case.
    pub application: String,
    /// The highest class the relaxation covers.
    pub level: RelaxationLevel,
}

/// The user's policy.
///
/// The default is normal mode with no exclusions, sensitive applications or
/// relaxations.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserPolicy {
    /// The global mode.
    pub mode: Mode,
    /// Applications the assistant must neither observe nor act in.
    pub excluded_applications: Vec<String>,
    /// Applications in which every action is treated as class 2 at least.
    pub sensitive_applications: Vec<String>,
    /// Per-application relaxations.
    pub relaxations: Vec<Relaxation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    schema_version: u64,
    mode: Mode,
    #[serde(default)]
    excluded_applications: Vec<String>,
    #[serde(default)]
    sensitive_applications: Vec<String>,
    #[serde(default)]
    relaxations: Vec<RawRelaxation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelaxation {
    application: String,
    class: i64,
}

impl UserPolicy {
    /// The policy to use when the policy file cannot be read or is invalid:
    /// observe-only.
    pub fn fail_closed() -> Self {
        UserPolicy {
            mode: Mode::ObserveOnly,
            ..UserPolicy::default()
        }
    }

    /// Load a policy from the text of a policy file.
    ///
    /// Returns every problem found, not only the first.
    pub fn from_json(text: &str) -> Result<Self, Vec<Problem>> {
        let raw: RawPolicy = serde_json::from_str(text)
            .map_err(|err| vec![Problem::new(Code::PolicyFileParse, err.to_string())])?;

        let mut problems = Vec::new();
        if raw.schema_version != POLICY_SCHEMA_VERSION {
            problems.push(Problem::new(
                Code::PolicyFileVersion,
                format!(
                    "schema_version is {}, this build reads {POLICY_SCHEMA_VERSION}",
                    raw.schema_version
                ),
            ));
        }

        let lists = [
            ("excluded_applications", &raw.excluded_applications),
            ("sensitive_applications", &raw.sensitive_applications),
        ];
        for (list, applications) in lists {
            for (index, application) in applications.iter().enumerate() {
                if !is_application_id(application) {
                    problems.push(Problem::new(
                        Code::PolicyFileApplication,
                        format!("{list}[{index}]: {APPLICATION_RULE}"),
                    ));
                }
            }
        }

        let mut relaxed = BTreeSet::new();
        let mut relaxations = Vec::with_capacity(raw.relaxations.len());
        for (index, relaxation) in raw.relaxations.into_iter().enumerate() {
            if !is_application_id(&relaxation.application) {
                problems.push(Problem::new(
                    Code::PolicyFileApplication,
                    format!("relaxations[{index}].application: {APPLICATION_RULE}"),
                ));
                continue;
            }
            if !relaxed.insert(relaxation.application.clone()) {
                problems.push(Problem::new(
                    Code::PolicyFileRelaxation,
                    format!("relaxations[{index}]: the application already has a relaxation"),
                ));
                continue;
            }
            let level = match relaxation.class {
                1 => RelaxationLevel::Reversible,
                2 => RelaxationLevel::Consequential,
                other => {
                    problems.push(Problem::new(
                        Code::PolicyFileRelaxation,
                        format!(
                            "relaxations[{index}].class is {other}; only classes 1 and 2 can be relaxed"
                        ),
                    ));
                    continue;
                }
            };
            relaxations.push(Relaxation {
                application: relaxation.application,
                level,
            });
        }

        if problems.is_empty() {
            Ok(UserPolicy {
                mode: raw.mode,
                excluded_applications: raw.excluded_applications,
                sensitive_applications: raw.sensitive_applications,
                relaxations,
            })
        } else {
            Err(problems)
        }
    }

    /// Whether the application is excluded.
    ///
    /// Compared without regard to ASCII case or surrounding whitespace, so that
    /// a near-identical identifier is still excluded.
    pub fn is_excluded(&self, application: &str) -> bool {
        self.excluded_applications
            .iter()
            .any(|entry| same_application(entry, application))
    }

    /// Whether the application is marked sensitive. Compared like
    /// [`UserPolicy::is_excluded`].
    pub fn is_sensitive(&self, application: &str) -> bool {
        self.sensitive_applications
            .iter()
            .any(|entry| same_application(entry, application))
    }

    /// The relaxation for the application, if any.
    ///
    /// Compared exactly, so that a near-identical identifier is not relaxed. If
    /// a policy built in code names the application twice, the lower level
    /// applies.
    pub fn relaxation_for(&self, application: &str) -> Option<RelaxationLevel> {
        self.relaxations
            .iter()
            .filter(|relaxation| relaxation.application == application)
            .map(|relaxation| relaxation.level)
            .min()
    }
}

const APPLICATION_RULE: &str = "an application identifier must be 1 to 255 characters with no control characters and no surrounding whitespace";

fn is_application_id(application: &str) -> bool {
    !application.is_empty()
        && application.chars().count() <= 255
        && application.trim() == application
        && !application.chars().any(char::is_control)
}

fn same_application(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(result: Result<UserPolicy, Vec<Problem>>) -> Vec<Code> {
        result
            .expect_err("the policy must be rejected")
            .into_iter()
            .map(|problem| problem.code)
            .collect()
    }

    #[test]
    fn loads_a_complete_policy() {
        let policy = UserPolicy::from_json(
            r#"{
                "schema_version": 1,
                "mode": "ask_every_action",
                "excluded_applications": ["org.example.Vault"],
                "sensitive_applications": ["org.example.Bank"],
                "relaxations": [
                    { "application": "org.example.Notes", "class": 1 },
                    { "application": "org.example.Mail", "class": 2 }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(policy.mode, Mode::AskEveryAction);
        assert!(policy.is_excluded("org.example.Vault"));
        assert!(policy.is_sensitive("org.example.Bank"));
        assert_eq!(
            policy.relaxation_for("org.example.Notes"),
            Some(RelaxationLevel::Reversible)
        );
        assert_eq!(
            policy.relaxation_for("org.example.Mail"),
            Some(RelaxationLevel::Consequential)
        );
        assert_eq!(policy.relaxation_for("org.example.Other"), None);
    }

    #[test]
    fn lists_are_optional_but_mode_and_version_are_not() {
        let policy = UserPolicy::from_json(r#"{ "schema_version": 1, "mode": "normal" }"#).unwrap();
        assert_eq!(policy, UserPolicy::default());

        assert_eq!(
            codes(UserPolicy::from_json(r#"{ "schema_version": 1 }"#)),
            [Code::PolicyFileParse]
        );
        assert_eq!(
            codes(UserPolicy::from_json(r#"{ "mode": "normal" }"#)),
            [Code::PolicyFileParse]
        );
    }

    #[test]
    fn every_mode_round_trips_through_its_file_name() {
        for mode in Mode::ALL {
            let text = format!(r#"{{ "schema_version": 1, "mode": "{}" }}"#, mode.as_str());
            assert_eq!(UserPolicy::from_json(&text).unwrap().mode, mode);
        }
    }

    #[test]
    fn rejects_unknown_fields_modes_and_versions() {
        assert_eq!(
            codes(UserPolicy::from_json(
                r#"{ "schema_version": 1, "mode": "normal", "allow_everything": true }"#
            )),
            [Code::PolicyFileParse]
        );
        assert_eq!(
            codes(UserPolicy::from_json(
                r#"{ "schema_version": 1, "mode": "relaxed" }"#
            )),
            [Code::PolicyFileParse]
        );
        assert_eq!(
            codes(UserPolicy::from_json(
                r#"{ "schema_version": 2, "mode": "normal" }"#
            )),
            [Code::PolicyFileVersion]
        );
        assert_eq!(
            codes(UserPolicy::from_json("not json")),
            [Code::PolicyFileParse]
        );
    }

    #[test]
    fn classes_other_than_1_and_2_cannot_be_relaxed() {
        for class in [-1, 0, 3, 4, 5] {
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal",
                     "relaxations": [{{ "application": "org.example.App", "class": {class} }}] }}"#
            );
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileRelaxation],
                "class {class}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_relaxations_and_bad_application_names() {
        assert_eq!(
            codes(UserPolicy::from_json(
                r#"{ "schema_version": 1, "mode": "normal", "relaxations": [
                    { "application": "org.example.App", "class": 1 },
                    { "application": "org.example.App", "class": 2 }
                ] }"#
            )),
            [Code::PolicyFileRelaxation]
        );
        assert_eq!(
            codes(UserPolicy::from_json(
                r#"{ "schema_version": 1, "mode": "normal",
                     "excluded_applications": ["", " padded "],
                     "sensitive_applications": ["line\nbreak"],
                     "relaxations": [{ "application": "", "class": 2 }] }"#
            )),
            [
                Code::PolicyFileApplication,
                Code::PolicyFileApplication,
                Code::PolicyFileApplication,
                Code::PolicyFileApplication
            ]
        );
    }

    #[test]
    fn exclusion_matches_loosely_and_relaxation_exactly() {
        let policy = UserPolicy {
            mode: Mode::Normal,
            excluded_applications: vec!["org.example.Vault".to_owned()],
            sensitive_applications: vec!["org.example.Bank".to_owned()],
            relaxations: vec![Relaxation {
                application: "org.example.Mail".to_owned(),
                level: RelaxationLevel::Consequential,
            }],
        };
        assert!(policy.is_excluded("ORG.EXAMPLE.VAULT"));
        assert!(policy.is_excluded(" org.example.vault "));
        assert!(policy.is_sensitive("org.example.bank"));
        assert!(!policy.is_excluded("org.example.Vault2"));
        assert_eq!(policy.relaxation_for("org.example.mail"), None);
        assert_eq!(policy.relaxation_for(" org.example.Mail"), None);
    }

    #[test]
    fn the_lower_level_wins_when_an_application_is_named_twice() {
        let policy = UserPolicy {
            relaxations: vec![
                Relaxation {
                    application: "org.example.Mail".to_owned(),
                    level: RelaxationLevel::Consequential,
                },
                Relaxation {
                    application: "org.example.Mail".to_owned(),
                    level: RelaxationLevel::Reversible,
                },
            ],
            ..UserPolicy::default()
        };
        assert_eq!(
            policy.relaxation_for("org.example.Mail"),
            Some(RelaxationLevel::Reversible)
        );
    }

    #[test]
    fn fail_closed_is_observe_only() {
        assert_eq!(UserPolicy::fail_closed().mode, Mode::ObserveOnly);
    }
}
