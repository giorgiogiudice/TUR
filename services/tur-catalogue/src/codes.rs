// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The stable error codes this component reports (specification §29.3).
//!
//! `uos-core` has no family for the catalogue yet, so every code uses the
//! `DESKTOP` family with a `CATALOGUE` topic. A dedicated family is planned;
//! when it exists these codes move in one change, here.

use std::fmt;

use serde::{Serialize, Serializer};
use uos_core::ErrorCode;

/// A condition the catalogue or the router reports with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    /// The catalogue directory, or a file in it, could not be read.
    Unreadable,
    /// An entry is not valid TOML, has an unknown field or a value of the
    /// wrong type.
    EntrySyntax,
    /// An entry parses, and a field holds a value that is not allowed.
    EntryInvalid,
    /// Two entries share an application ID.
    DuplicateId,
    /// A name or alias is used twice, in one entry or across entries.
    AliasCollision,
    /// A compatibility rating has no admissible source: it is not `unknown`,
    /// not attributed to a named external source with a URL, and not backed
    /// by a TUR test report.
    UnsourcedRating,
    /// A compatibility record names a route its edition cannot use.
    RouteMismatch,
    /// The catalogue, or one entry, exceeds a size limit.
    LimitExceeded,
    /// No application in the catalogue has the requested ID.
    UnknownApplication,
    /// No application matches the search query.
    NoMatch,
    /// The router found no route that runs the application on TUR.
    NoRoute,
    /// A host-facts or preferences file could not be read or is not valid.
    InputInvalid,
}

impl Code {
    /// Every code this component can report: the one table.
    pub const ALL: [Code; 12] = [
        Code::Unreadable,
        Code::EntrySyntax,
        Code::EntryInvalid,
        Code::DuplicateId,
        Code::AliasCollision,
        Code::UnsourcedRating,
        Code::RouteMismatch,
        Code::LimitExceeded,
        Code::UnknownApplication,
        Code::NoMatch,
        Code::NoRoute,
        Code::InputInvalid,
    ];

    /// The code as text, for example `UOS-DESKTOP-CATALOGUE-001`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::Unreadable => "UOS-DESKTOP-CATALOGUE-001",
            Code::EntrySyntax => "UOS-DESKTOP-CATALOGUE-002",
            Code::EntryInvalid => "UOS-DESKTOP-CATALOGUE-003",
            Code::DuplicateId => "UOS-DESKTOP-CATALOGUE-004",
            Code::AliasCollision => "UOS-DESKTOP-CATALOGUE-005",
            Code::UnsourcedRating => "UOS-DESKTOP-CATALOGUE-006",
            Code::RouteMismatch => "UOS-DESKTOP-CATALOGUE-007",
            Code::LimitExceeded => "UOS-DESKTOP-CATALOGUE-008",
            Code::UnknownApplication => "UOS-DESKTOP-CATALOGUE-009",
            Code::NoMatch => "UOS-DESKTOP-CATALOGUE-010",
            Code::NoRoute => "UOS-DESKTOP-CATALOGUE-011",
            Code::InputInvalid => "UOS-DESKTOP-CATALOGUE-012",
        }
    }

    /// A short title for the condition.
    pub const fn title(self) -> &'static str {
        match self {
            Code::Unreadable => "The catalogue could not be read",
            Code::EntrySyntax => "A catalogue entry is not valid TOML or has the wrong shape",
            Code::EntryInvalid => "A catalogue entry has a field that is not allowed",
            Code::DuplicateId => "Two catalogue entries share an application ID",
            Code::AliasCollision => "A name or alias is used more than once",
            Code::UnsourcedRating => "A compatibility rating has no admissible source",
            Code::RouteMismatch => "A compatibility record names a route its edition cannot use",
            Code::LimitExceeded => "The catalogue exceeds a size limit",
            Code::UnknownApplication => "No application has this ID",
            Code::NoMatch => "No application matches the search",
            Code::NoRoute => "No route runs this application on TUR",
            Code::InputInvalid => "A host-facts or preferences file is not valid",
        }
    }

    /// The code as a validated [`ErrorCode`].
    ///
    /// # Panics
    ///
    /// Never for a released build: every entry of [`Code::ALL`] is checked to
    /// parse by this crate's tests.
    pub fn error_code(self) -> ErrorCode {
        self.as_str()
            .parse()
            .expect("every registered code is a valid error code")
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Code {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// One problem found while loading or validating the catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// The stable code of the condition.
    pub code: Code,
    /// The file the problem is in, when it belongs to one file.
    pub file: Option<String>,
    /// What is wrong, in plain language.
    pub message: String,
}

impl Problem {
    /// A problem that belongs to one file.
    pub fn in_file(code: Code, file: &str, message: impl Into<String>) -> Self {
        Problem {
            code,
            file: Some(file.to_owned()),
            message: message.into(),
        }
    }

    /// A problem that does not belong to one file.
    pub fn general(code: Code, message: impl Into<String>) -> Self {
        Problem {
            code,
            file: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "error[{}]: {}: {}", self.code, file, self.message),
            None => write!(f, "error[{}]: {}", self.code, self.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use uos_core::Family;

    use super::*;

    #[test]
    fn every_code_is_a_valid_unique_error_code() {
        let mut seen = BTreeSet::new();
        let mut numbers = BTreeSet::new();
        for code in Code::ALL {
            let parsed: ErrorCode = code.as_str().parse().unwrap();
            assert_eq!(parsed, code.error_code());
            assert!(seen.insert(code.as_str()), "duplicate code {code}");
            assert!(numbers.insert(parsed.number()), "duplicate number in {code}");
            assert!(!code.title().is_empty());
        }
        assert_eq!(seen.len(), Code::ALL.len());
    }

    #[test]
    fn every_code_is_in_the_desktop_family_with_the_catalogue_topic() {
        for code in Code::ALL {
            assert_eq!(code.error_code().family(), Family::Desktop);
            assert!(code.as_str().starts_with("UOS-DESKTOP-CATALOGUE-"));
        }
    }

    #[test]
    fn the_table_lists_every_variant() {
        // A new variant must be added to `ALL`: this match fails to compile
        // when a variant is missing here, and the count below fails when it
        // is missing from the table.
        let count = |code: Code| match code {
            Code::Unreadable
            | Code::EntrySyntax
            | Code::EntryInvalid
            | Code::DuplicateId
            | Code::AliasCollision
            | Code::UnsourcedRating
            | Code::RouteMismatch
            | Code::LimitExceeded
            | Code::UnknownApplication
            | Code::NoMatch
            | Code::NoRoute
            | Code::InputInvalid => 1usize,
        };
        let total: usize = Code::ALL.into_iter().map(count).sum();
        assert_eq!(total, 12);
    }

    #[test]
    fn serialises_as_the_code_text() {
        let json = serde_json::to_string(&Code::NoRoute).unwrap();
        assert_eq!(json, "\"UOS-DESKTOP-CATALOGUE-011\"");
    }

    #[test]
    fn a_problem_prints_its_code_file_and_message() {
        let problem = Problem::in_file(Code::EntryInvalid, "a.toml", "bad");
        assert_eq!(
            problem.to_string(),
            "error[UOS-DESKTOP-CATALOGUE-003]: a.toml: bad"
        );
        let general = Problem::general(Code::Unreadable, "gone");
        assert_eq!(general.to_string(), "error[UOS-DESKTOP-CATALOGUE-001]: gone");
    }
}
