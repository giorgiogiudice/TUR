// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The stable error codes of `tur-rehost` (specification §29.3), in the
//! `REHOST` family. A code never changes its meaning once released; the
//! table is `docs/architecture/rehost.md`, section 9.

use std::fmt;

use serde::{Serialize, Serializer};
use tur_core::ErrorCode;

/// Exit status: success.
pub const EXIT_OK: u8 = 0;
/// Exit status: the input could not be read, or is not valid.
pub const EXIT_INPUT: u8 = 1;
/// Exit status: the command line is wrong.
pub const EXIT_USAGE: u8 = 2;
/// Exit status: the package is not something TUR re-hosts.
pub const EXIT_NOT_REHOSTABLE: u8 = 3;
/// Exit status: the package can be re-hosted once the person allows or
/// installs something first.
pub const EXIT_NEEDS_SOMETHING: u8 = 4;
/// Exit status: refused before anything was changed or fetched.
pub const EXIT_REFUSED: u8 = 6;
/// Exit status: a step failed while the operation was carried out; it was
/// rolled back.
pub const EXIT_STEP_FAILED: u8 = 7;
/// Exit status: something was still there after a removal.
pub const EXIT_LEFT_BEHIND: u8 = 8;

/// A condition `tur-rehost` reports with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Code {
    /// The package, or a file the operation needs, could not be read.
    Unreadable,
    /// The package's container could not be opened by any of the tools.
    ContainerNotOpened,
    /// Nothing in the package was recognised.
    NothingRecognised,
    /// The package is native code (or a Windows-only framework): it is
    /// not re-hostable. The finding names the route that may run it.
    NativeProgram,
    /// The runtime family was recognised, and re-hosting it is not built
    /// in this slice (only Electron is).
    FamilyNotBuilt,
    /// The runtime's version could not be read from the package.
    VersionUnknown,
    /// A vendor build of Electron (castlabs' with Widevine): the stock
    /// Linux build lacks what the vendor added.
    VendorBuild,
    /// The package is already built for Linux: use it as it is.
    AlreadyLinux,
    /// TUR's pinned index is unreadable or not valid.
    IndexInvalid,
    /// The runtime version the payload needs is not pinned in TUR's index,
    /// so it is not fetched.
    RuntimeNotPinned,
    /// A download did not complete.
    FetchFailed,
    /// A downloaded file is not the one the index pins (SHA-256 or size).
    /// It was deleted; nothing in it was used.
    FetchMismatch,
    /// A native module has no Linux build TUR can supply (a dead end, or
    /// not in the index), and the person did not accept running without it.
    ModuleUnresolved,
    /// A native module must be built from source, and the person has not
    /// allowed builds on this computer.
    BuildNotAllowed,
    /// Building a native module from source failed.
    BuildFailed,
    /// A program the operation needs is not installed (bsdtar, 7zz,
    /// innoextract, msiextract, xz, curl, podman).
    ProgramMissing,
    /// bubblewrap is not installed: nothing is run without the sandbox.
    SandboxMissing,
    /// The effective user is root: refused.
    RunningAsRoot,
    /// The application identifier or another value given is not allowed.
    InputInvalid,
    /// An application with this identifier is already re-hosted.
    AlreadyInstalled,
    /// A step failed while installing; the installation was rolled back.
    StepFailed,
    /// No application with this identifier is re-hosted.
    NotInstalled,
    /// The application's record is unreadable, not valid, or another
    /// application's.
    RecordInvalid,
    /// The payload changed since it was installed: its SHA-256 differs from
    /// the record. Not started.
    PayloadChanged,
    /// Something of the application was still there after it was removed.
    LeftBehind,
}

impl Code {
    /// Every code: the one table.
    pub const ALL: [Code; 25] = [
        Code::Unreadable,
        Code::ContainerNotOpened,
        Code::NothingRecognised,
        Code::NativeProgram,
        Code::FamilyNotBuilt,
        Code::VersionUnknown,
        Code::VendorBuild,
        Code::AlreadyLinux,
        Code::IndexInvalid,
        Code::RuntimeNotPinned,
        Code::FetchFailed,
        Code::FetchMismatch,
        Code::ModuleUnresolved,
        Code::BuildNotAllowed,
        Code::BuildFailed,
        Code::ProgramMissing,
        Code::SandboxMissing,
        Code::RunningAsRoot,
        Code::InputInvalid,
        Code::AlreadyInstalled,
        Code::StepFailed,
        Code::NotInstalled,
        Code::RecordInvalid,
        Code::PayloadChanged,
        Code::LeftBehind,
    ];

    /// The code as text, for example `TUR-REHOST-DETECT-001`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::Unreadable => "TUR-REHOST-DETECT-001",
            Code::ContainerNotOpened => "TUR-REHOST-DETECT-002",
            Code::NothingRecognised => "TUR-REHOST-DETECT-003",
            Code::NativeProgram => "TUR-REHOST-DETECT-004",
            Code::FamilyNotBuilt => "TUR-REHOST-DETECT-005",
            Code::VersionUnknown => "TUR-REHOST-DETECT-006",
            Code::VendorBuild => "TUR-REHOST-DETECT-007",
            Code::AlreadyLinux => "TUR-REHOST-DETECT-008",
            Code::IndexInvalid => "TUR-REHOST-INDEX-001",
            Code::RuntimeNotPinned => "TUR-REHOST-INDEX-002",
            Code::FetchFailed => "TUR-REHOST-FETCH-001",
            Code::FetchMismatch => "TUR-REHOST-FETCH-002",
            Code::ModuleUnresolved => "TUR-REHOST-NATIVE-001",
            Code::BuildNotAllowed => "TUR-REHOST-NATIVE-002",
            Code::BuildFailed => "TUR-REHOST-NATIVE-003",
            Code::ProgramMissing => "TUR-REHOST-HOST-001",
            Code::SandboxMissing => "TUR-REHOST-HOST-002",
            Code::RunningAsRoot => "TUR-REHOST-HOST-003",
            Code::InputInvalid => "TUR-REHOST-APP-001",
            Code::AlreadyInstalled => "TUR-REHOST-APP-002",
            Code::StepFailed => "TUR-REHOST-APP-003",
            Code::NotInstalled => "TUR-REHOST-APP-004",
            Code::RecordInvalid => "TUR-REHOST-APP-005",
            Code::PayloadChanged => "TUR-REHOST-APP-006",
            Code::LeftBehind => "TUR-REHOST-APP-007",
        }
    }

    /// A short title for the condition, for people.
    pub const fn title(self) -> &'static str {
        match self {
            Code::Unreadable => "The file could not be read",
            Code::ContainerNotOpened => "The package could not be opened",
            Code::NothingRecognised => "Nothing in the package was recognised",
            Code::NativeProgram => "This is native code: it cannot run on another runtime",
            Code::FamilyNotBuilt => "Re-hosting this kind of program is not built yet",
            Code::VersionUnknown => "The runtime's version could not be read",
            Code::VendorBuild => "A vendor's own build of Electron",
            Code::AlreadyLinux => "This package is already for Linux",
            Code::IndexInvalid => "TUR's list of pinned downloads is not valid",
            Code::RuntimeNotPinned => "TUR has not pinned this runtime version",
            Code::FetchFailed => "A download did not complete",
            Code::FetchMismatch => "A download was not the file TUR pinned",
            Code::ModuleUnresolved => "A part of the application has no Linux build",
            Code::BuildNotAllowed => "Some parts must be built on this computer first",
            Code::BuildFailed => "Building a part of the application failed",
            Code::ProgramMissing => "A program this needs is not installed",
            Code::SandboxMissing => "The sandbox (bubblewrap) is not installed",
            Code::RunningAsRoot => "Not run as the administrator",
            Code::InputInvalid => "A value given is not allowed",
            Code::AlreadyInstalled => "This application is already set up",
            Code::StepFailed => "A step failed; nothing was left behind",
            Code::NotInstalled => "This application is not set up",
            Code::RecordInvalid => "The application's record is not valid",
            Code::PayloadChanged => "The application's files changed since they were set up",
            Code::LeftBehind => "Something of the application is still there",
        }
    }

    /// The exit status the command line uses for this code.
    pub const fn exit_status(self) -> u8 {
        match self {
            Code::Unreadable | Code::IndexInvalid | Code::RecordInvalid | Code::InputInvalid => {
                EXIT_INPUT
            }
            Code::ContainerNotOpened
            | Code::NothingRecognised
            | Code::NativeProgram
            | Code::FamilyNotBuilt
            | Code::VersionUnknown
            | Code::VendorBuild
            | Code::AlreadyLinux
            | Code::RuntimeNotPinned
            | Code::ModuleUnresolved => EXIT_NOT_REHOSTABLE,
            Code::BuildNotAllowed | Code::ProgramMissing | Code::SandboxMissing => {
                EXIT_NEEDS_SOMETHING
            }
            Code::RunningAsRoot
            | Code::AlreadyInstalled
            | Code::NotInstalled
            | Code::PayloadChanged => EXIT_REFUSED,
            Code::FetchFailed | Code::FetchMismatch | Code::BuildFailed | Code::StepFailed => {
                EXIT_STEP_FAILED
            }
            Code::LeftBehind => EXIT_LEFT_BEHIND,
        }
    }

    /// The code as a `tur-core` error code.
    pub fn error_code(self) -> ErrorCode {
        ErrorCode::from_static(self.as_str())
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

/// A condition with its code and a message for people. Messages never
/// carry a secret: they name files and programs, never environment values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    /// The stable code.
    pub code: Code,
    /// The code's title.
    pub title: &'static str,
    /// What happened, in plain language.
    pub message: String,
}

impl Failure {
    /// A failure with `code` and `message`.
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Failure {
            code,
            title: code.title(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code, self.message)
    }
}

impl std::error::Error for Failure {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use tur_core::Family;

    #[test]
    fn every_code_is_valid_unique_and_in_the_rehost_family() {
        let mut seen = BTreeSet::new();
        for code in Code::ALL {
            let parsed: ErrorCode = code.as_str().parse().expect("a valid code");
            assert_eq!(parsed.family(), Family::Rehost, "{code}");
            assert!(seen.insert(code.as_str()), "{code} is used twice");
            assert!(!code.title().is_empty());
        }
    }

    #[test]
    fn released_codes_keep_their_meaning() {
        // A code never changes once released: these are written out a
        // second time by hand.
        assert_eq!(Code::Unreadable.as_str(), "TUR-REHOST-DETECT-001");
        assert_eq!(Code::NativeProgram.as_str(), "TUR-REHOST-DETECT-004");
        assert_eq!(Code::RuntimeNotPinned.as_str(), "TUR-REHOST-INDEX-002");
        assert_eq!(Code::FetchMismatch.as_str(), "TUR-REHOST-FETCH-002");
        assert_eq!(Code::BuildNotAllowed.as_str(), "TUR-REHOST-NATIVE-002");
        assert_eq!(Code::LeftBehind.as_str(), "TUR-REHOST-APP-007");
    }

    #[test]
    fn a_failure_prints_its_code_first() {
        let failure = Failure::new(Code::NotInstalled, "no application com.example.app");
        assert_eq!(
            failure.to_string(),
            "error[TUR-REHOST-APP-004]: no application com.example.app"
        );
        assert_eq!(Code::LeftBehind.exit_status(), EXIT_LEFT_BEHIND);
        assert_eq!(Code::BuildNotAllowed.exit_status(), EXIT_NEEDS_SOMETHING);
    }
}
