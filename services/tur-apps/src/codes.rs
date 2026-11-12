// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The stable error codes of `tur-apps` (specification §29.3).
//!
//! The codes are in the `DESKTOP` family with an `APPS` topic, as the
//! catalogue's are in `DESKTOP` with `CATALOGUE`: installing applications is
//! the application centre's work (desktop-experience.md, section 8). A code
//! never changes its meaning once released.

use std::fmt;

use serde::{Serialize, Serializer};
use tur_core::ErrorCode;

/// A condition `tur-apps` reports with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    /// The catalogue, a file given on the command line or a record could
    /// not be read.
    Unreadable,
    /// No application in the catalogue has this identifier.
    UnknownApplication,
    /// The router says no route runs the application on this computer.
    NoRoute,
    /// The route is one this program cannot install by yet: a Windows
    /// guest, the Android runtime, Darling, Proton, a distribution package
    /// or a web application.
    RouteNotBuilt,
    /// TUR may not download this application's installer: the user must
    /// supply the file.
    InstallerFileNeeded,
    /// A file is not the one the catalogue pins: its SHA-256 or its size
    /// differs. Nothing in it was run.
    NotVerified,
    /// A download did not complete.
    DownloadFailed,
    /// The source the application comes from is not switched on.
    SourceOff,
    /// A program this needs (flatpak, curl, tur-runtime, Wine) is not
    /// installed.
    ProgramMissing,
    /// A step failed while the operation was carried out.
    StepFailed,
    /// The application is not installed by `tur-apps`.
    NotInstalled,
    /// The application is already installed.
    AlreadyInstalled,
    /// The operation needs the user's confirmation, and none was given.
    ConfirmationNeeded,
    /// The route asked for is not one the router offers for this
    /// application on this computer, or it is blocked.
    RouteNotOffered,
    /// After a removal, something of the application was still there.
    LeftBehind,
    /// A record of an installed application is not valid.
    RecordInvalid,
    /// A host-facts or preferences file, or an argument, is not valid.
    InputInvalid,
}

impl Code {
    /// Every code: the one table.
    pub const ALL: [Code; 17] = [
        Code::Unreadable,
        Code::UnknownApplication,
        Code::NoRoute,
        Code::RouteNotBuilt,
        Code::InstallerFileNeeded,
        Code::NotVerified,
        Code::DownloadFailed,
        Code::SourceOff,
        Code::ProgramMissing,
        Code::StepFailed,
        Code::NotInstalled,
        Code::AlreadyInstalled,
        Code::ConfirmationNeeded,
        Code::RouteNotOffered,
        Code::LeftBehind,
        Code::RecordInvalid,
        Code::InputInvalid,
    ];

    /// The code as text, for example `TUR-DESKTOP-APPS-003`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::Unreadable => "TUR-DESKTOP-APPS-001",
            Code::UnknownApplication => "TUR-DESKTOP-APPS-002",
            Code::NoRoute => "TUR-DESKTOP-APPS-003",
            Code::RouteNotBuilt => "TUR-DESKTOP-APPS-004",
            Code::InstallerFileNeeded => "TUR-DESKTOP-APPS-005",
            Code::NotVerified => "TUR-DESKTOP-APPS-006",
            Code::DownloadFailed => "TUR-DESKTOP-APPS-007",
            Code::SourceOff => "TUR-DESKTOP-APPS-008",
            Code::ProgramMissing => "TUR-DESKTOP-APPS-009",
            Code::StepFailed => "TUR-DESKTOP-APPS-010",
            Code::NotInstalled => "TUR-DESKTOP-APPS-011",
            Code::AlreadyInstalled => "TUR-DESKTOP-APPS-012",
            Code::ConfirmationNeeded => "TUR-DESKTOP-APPS-013",
            Code::RouteNotOffered => "TUR-DESKTOP-APPS-014",
            Code::LeftBehind => "TUR-DESKTOP-APPS-015",
            Code::RecordInvalid => "TUR-DESKTOP-APPS-016",
            Code::InputInvalid => "TUR-DESKTOP-APPS-017",
        }
    }

    /// A short title for the condition.
    pub const fn title(self) -> &'static str {
        match self {
            Code::Unreadable => "Something this needs could not be read",
            Code::UnknownApplication => "No application has this identifier",
            Code::NoRoute => "No route runs this application on this computer",
            Code::RouteNotBuilt => "Installing by this route is not built yet",
            Code::InstallerFileNeeded => "You need to supply the installer",
            Code::NotVerified => "The file is not the one expected",
            Code::DownloadFailed => "The download did not complete",
            Code::SourceOff => "The source is not switched on",
            Code::ProgramMissing => "A program this needs is not installed",
            Code::StepFailed => "A step failed",
            Code::NotInstalled => "The application is not installed",
            Code::AlreadyInstalled => "The application is already installed",
            Code::ConfirmationNeeded => "Nothing was done without your confirmation",
            Code::RouteNotOffered => "That way of running it is not offered here",
            Code::LeftBehind => "Something of the application is still there",
            Code::RecordInvalid => "A record of an installed application is not valid",
            Code::InputInvalid => "An input is not valid",
        }
    }

    /// The exit status the command line uses for this code.
    pub const fn exit_status(self) -> u8 {
        match self {
            Code::Unreadable | Code::RecordInvalid | Code::InputInvalid => EXIT_INPUT,
            Code::NoRoute
            | Code::RouteNotBuilt
            | Code::InstallerFileNeeded
            | Code::SourceOff
            | Code::ProgramMissing
            | Code::AlreadyInstalled
            | Code::ConfirmationNeeded
            | Code::RouteNotOffered => EXIT_REFUSED,
            Code::NotVerified => EXIT_NOT_VERIFIED,
            Code::UnknownApplication | Code::NotInstalled => EXIT_NOT_FOUND,
            Code::DownloadFailed | Code::StepFailed => EXIT_STEP_FAILED,
            Code::LeftBehind => EXIT_LEFT_BEHIND,
        }
    }

    /// The code as a validated [`ErrorCode`].
    ///
    /// # Panics
    ///
    /// Never for a released build: every entry of [`Code::ALL`] is checked
    /// to parse by this module's tests.
    pub fn error_code(self) -> ErrorCode {
        self.as_str()
            .parse()
            .expect("every registered code is a valid error code")
    }
}

/// Success.
pub const EXIT_OK: u8 = 0;
/// An input could not be read or is not valid.
pub const EXIT_INPUT: u8 = 1;
/// The command line is wrong (clap's own status).
pub const EXIT_USAGE: u8 = 2;
/// Refused before anything was changed.
pub const EXIT_REFUSED: u8 = 3;
/// A file is not the one expected; nothing in it ran.
pub const EXIT_NOT_VERIFIED: u8 = 5;
/// No such application, or it is not installed.
pub const EXIT_NOT_FOUND: u8 = 6;
/// A step failed while the operation was carried out.
pub const EXIT_STEP_FAILED: u8 = 7;
/// A removal left something behind.
pub const EXIT_LEFT_BEHIND: u8 = 8;

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

/// A refusal or a failure: a stable code and a plain sentence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Failure {
    /// The stable code.
    pub code: Code,
    /// The code's title.
    pub title: &'static str,
    /// What happened, in plain language, and what to do.
    pub message: String,
    /// More detail, as JSON, when there is some: for a refusal by the
    /// router, the router's option for "no route".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl Failure {
    /// A failure with a code and a message.
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Failure {
            code,
            title: code.title(),
            message: message.into(),
            detail: None,
        }
    }

    /// The same failure with detail attached.
    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code, self.message)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tur_core::Family;

    use super::*;

    #[test]
    fn every_code_is_a_valid_unique_desktop_code() {
        let mut seen = BTreeSet::new();
        for code in Code::ALL {
            let parsed: ErrorCode = code.as_str().parse().unwrap();
            assert_eq!(parsed, code.error_code());
            assert_eq!(parsed.family(), Family::Desktop);
            assert!(code.as_str().starts_with("TUR-DESKTOP-APPS-"));
            assert!(seen.insert(parsed.number()), "duplicate number in {code}");
            assert!(!code.title().is_empty());
            assert_ne!(code.exit_status(), EXIT_OK);
            assert_ne!(code.exit_status(), EXIT_USAGE);
        }
        assert_eq!(seen.len(), Code::ALL.len());
    }

    #[test]
    fn the_table_lists_every_variant() {
        let count = |code: Code| match code {
            Code::Unreadable
            | Code::UnknownApplication
            | Code::NoRoute
            | Code::RouteNotBuilt
            | Code::InstallerFileNeeded
            | Code::NotVerified
            | Code::DownloadFailed
            | Code::SourceOff
            | Code::ProgramMissing
            | Code::StepFailed
            | Code::NotInstalled
            | Code::AlreadyInstalled
            | Code::ConfirmationNeeded
            | Code::RouteNotOffered
            | Code::LeftBehind
            | Code::RecordInvalid
            | Code::InputInvalid => 1usize,
        };
        assert_eq!(Code::ALL.into_iter().map(count).sum::<usize>(), 17);
    }

    #[test]
    fn a_failure_prints_and_serialises_its_code() {
        let failure = Failure::new(Code::NotVerified, "the digest differs");
        assert_eq!(
            failure.to_string(),
            "error[TUR-DESKTOP-APPS-006]: the digest differs"
        );
        let json = serde_json::to_value(&failure).unwrap();
        assert_eq!(json["code"], "TUR-DESKTOP-APPS-006");
        assert_eq!(json["title"], Code::NotVerified.title());
        assert!(json.get("detail").is_none());
    }
}
