// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The stable error codes this component reports (specification §29.3).
//!
//! Conditions that are not specific to one ecosystem use the runtime
//! manager's own family, `RUNTIME`, with a `MANAGER` or `PROFILE` topic.
//! Conditions specific to Windows, macOS or Android software use the
//! `WINDOWS`, `DARWIN` and `ANDROID` families, and refusals made to keep an
//! application inside its sandbox use `SECURITY`.
//!
//! Until `tur-core` had a `RUNTIME` family the first group was parked under
//! `DESKTOP`: `TUR-DESKTOP-RUNTIME-00N` is now `TUR-RUNTIME-MANAGER-00N` and
//! `TUR-DESKTOP-PROFILE-00N` is now `TUR-RUNTIME-PROFILE-00N`, with the
//! numbers unchanged. Nothing outside this component used those ten strings.
//! The `WINDOWS` and `SECURITY` codes are where they belong and did not
//! move; the Windows end-to-end test (`tests/windows-runtime`) reads them.

use std::fmt;

use serde::{Serialize, Serializer};
use tur_core::ErrorCode;

/// A condition the runtime manager reports with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Code {
    /// The path could not be opened or read.
    Unreadable,
    /// A file of a recognised, ecosystem-neutral format is truncated or malformed.
    Damaged,
    /// The file asks for more work than one inspection may do.
    LimitExceeded,
    /// The file was inspected and there is no supported way to run it.
    NoRoute,
    /// A Windows PE file is truncated or malformed.
    WindowsDamaged,
    /// Wine is recommended and is not installed.
    WineMissing,
    /// The file requires a Windows kernel; only a full Windows VM can host it.
    WindowsKernelRequired,
    /// A Windows VM is offered and its backend is not installed.
    WindowsVmMissing,
    /// A Windows file that no TUR runtime handles.
    WindowsUnsupported,
    /// A Mach-O file or application bundle is truncated or malformed.
    DarwinDamaged,
    /// Darling is recommended and is not installed.
    DarlingMissing,
    /// A macOS kernel extension, which is never loaded.
    DarwinKext,
    /// A macOS file that no TUR runtime handles.
    DarwinUnsupported,
    /// The Android runtime is recommended and is not installed.
    AndroidRuntimeMissing,
    /// The package needs ARM translation, which is not installed.
    AndroidTranslationMissing,
    /// An Android package that no TUR runtime handles.
    AndroidUnsupported,
    /// An application profile could not be read.
    ProfileUnreadable,
    /// An application profile is not valid JSON or has the wrong shape.
    ProfileSyntax,
    /// An application profile field holds a value that is not allowed.
    ProfileInvalid,
    /// An application profile holds something that looks like a credential.
    ProfileSecret,
    /// The runtime manager's data or state directory cannot be found or used.
    StorageUnusable,
    /// A program the runtime manager needs for the operation is not installed.
    ToolMissing,
    /// The profile pins a runtime version and a different one is installed.
    RuntimePinMismatch,
    /// The runtime did not report a version that can be recorded.
    RuntimeVersionUnknown,
    /// Proton is started through `umu-run`, which is not installed.
    ProtonLauncherMissing,
    /// A Proton profile does not pin a version, so the launcher would follow the latest build.
    ProtonUnpinned,
    /// The profile names a runtime or a format this build cannot launch.
    LaunchUnsupported,
    /// A step of a prefix operation or a launch failed while it was being carried out.
    StepFailed,
    /// The operation needs a prefix and the application has none.
    PrefixMissing,
    /// A prefix was to be created and the application already has one.
    PrefixExists,
    /// The metadata file beside a prefix cannot be read or is not valid.
    PrefixMetadataInvalid,
    /// Verification found something missing or wrong in a prefix.
    PrefixDamaged,
    /// The prefix is recorded as belonging to a different application, prefix name or runtime.
    PrefixOwner,
    /// The installed runtime is not the version the prefix is on; only an explicit migration changes that.
    PrefixRuntimeMismatch,
    /// A migration was asked for and the prefix is pinned.
    PrefixPinned,
    /// The named backup does not exist for this application.
    BackupMissing,
    /// Case-insensitive file names were asked for and the file system holding the prefix cannot provide them. Reported; the prefix is created case-sensitive.
    CaseFoldUnsupported,
    /// Case-insensitive file names were asked for and the prefix's drive C: could not be given them for another reason. Reported; the prefix is created case-sensitive.
    CaseFoldFailed,
    /// The effective user is root; Wine and Proton are never run as root.
    RunningAsRoot,
    /// The sandbox cannot be built because `bwrap` is not installed. Nothing is run without it.
    SandboxMissing,
    /// A directory was to be granted that is, contains or lies inside a protected location.
    GrantRefused,
    /// A recorded grant now resolves somewhere else, or no longer passes the grant rules.
    GrantChanged,
    /// An extra environment variable has a name or value that is not allowed.
    EnvironmentRefused,
    /// The profile's binary is not a regular file, or lies in a protected location.
    BinaryRefused,
    /// The file is not a Windows installer, program or archive of programs.
    InstallerUnrecognised,
    /// The file's kind has no known unattended mode, so it is not run.
    InstallerNoUnattendedMode,
    /// The installer ran and exited with a failure status.
    InstallerFailed,
    /// The installer did not finish in the time it was given and was stopped.
    InstallerTimedOut,
    /// The installer finished and no new program was found in the prefix.
    InstallNothingFound,
    /// Programs were installed and none could be chosen as the main one.
    InstallMainProgramUnclear,
    /// No application with this identifier is installed.
    AppNotInstalled,
    /// The application record is unreadable, not valid, or not this application's.
    AppRecordInvalid,
    /// Something of the application was still there after it was removed.
    UninstallIncomplete,
    /// A value given for an installation is not allowed.
    InstallRequestInvalid,
    /// The application's own uninstaller failed or ran out of time.
    UninstallerFailed,
}

impl Code {
    /// Every code this component can report.
    pub const ALL: [Code; 55] = [
        Code::Unreadable,
        Code::Damaged,
        Code::LimitExceeded,
        Code::NoRoute,
        Code::WindowsDamaged,
        Code::WineMissing,
        Code::WindowsKernelRequired,
        Code::WindowsVmMissing,
        Code::WindowsUnsupported,
        Code::DarwinDamaged,
        Code::DarlingMissing,
        Code::DarwinKext,
        Code::DarwinUnsupported,
        Code::AndroidRuntimeMissing,
        Code::AndroidTranslationMissing,
        Code::AndroidUnsupported,
        Code::ProfileUnreadable,
        Code::ProfileSyntax,
        Code::ProfileInvalid,
        Code::ProfileSecret,
        Code::StorageUnusable,
        Code::ToolMissing,
        Code::RuntimePinMismatch,
        Code::RuntimeVersionUnknown,
        Code::ProtonLauncherMissing,
        Code::ProtonUnpinned,
        Code::LaunchUnsupported,
        Code::StepFailed,
        Code::PrefixMissing,
        Code::PrefixExists,
        Code::PrefixMetadataInvalid,
        Code::PrefixDamaged,
        Code::PrefixOwner,
        Code::PrefixRuntimeMismatch,
        Code::PrefixPinned,
        Code::BackupMissing,
        Code::CaseFoldUnsupported,
        Code::CaseFoldFailed,
        Code::RunningAsRoot,
        Code::SandboxMissing,
        Code::GrantRefused,
        Code::GrantChanged,
        Code::EnvironmentRefused,
        Code::BinaryRefused,
        Code::InstallerUnrecognised,
        Code::InstallerNoUnattendedMode,
        Code::InstallerFailed,
        Code::InstallerTimedOut,
        Code::InstallNothingFound,
        Code::InstallMainProgramUnclear,
        Code::AppNotInstalled,
        Code::AppRecordInvalid,
        Code::UninstallIncomplete,
        Code::InstallRequestInvalid,
        Code::UninstallerFailed,
    ];

    /// The code as text, for example `TUR-WINDOWS-RUNTIME-001`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::Unreadable => "TUR-RUNTIME-MANAGER-001",
            Code::Damaged => "TUR-RUNTIME-MANAGER-002",
            Code::LimitExceeded => "TUR-RUNTIME-MANAGER-003",
            Code::NoRoute => "TUR-RUNTIME-MANAGER-004",
            Code::WindowsDamaged => "TUR-WINDOWS-INSPECT-001",
            Code::WineMissing => "TUR-WINDOWS-RUNTIME-001",
            Code::WindowsKernelRequired => "TUR-WINDOWS-RUNTIME-002",
            Code::WindowsVmMissing => "TUR-WINDOWS-RUNTIME-003",
            Code::WindowsUnsupported => "TUR-WINDOWS-RUNTIME-004",
            Code::DarwinDamaged => "TUR-DARWIN-INSPECT-001",
            Code::DarlingMissing => "TUR-DARWIN-RUNTIME-001",
            Code::DarwinKext => "TUR-DARWIN-RUNTIME-002",
            Code::DarwinUnsupported => "TUR-DARWIN-RUNTIME-003",
            Code::AndroidRuntimeMissing => "TUR-ANDROID-RUNTIME-001",
            Code::AndroidTranslationMissing => "TUR-ANDROID-RUNTIME-002",
            Code::AndroidUnsupported => "TUR-ANDROID-RUNTIME-003",
            Code::ProfileUnreadable => "TUR-RUNTIME-PROFILE-001",
            Code::ProfileSyntax => "TUR-RUNTIME-PROFILE-002",
            Code::ProfileInvalid => "TUR-RUNTIME-PROFILE-003",
            Code::ProfileSecret => "TUR-RUNTIME-PROFILE-004",
            Code::StorageUnusable => "TUR-RUNTIME-MANAGER-005",
            Code::ToolMissing => "TUR-RUNTIME-MANAGER-006",
            Code::RuntimePinMismatch => "TUR-WINDOWS-RUNTIME-005",
            Code::RuntimeVersionUnknown => "TUR-WINDOWS-RUNTIME-006",
            Code::ProtonLauncherMissing => "TUR-WINDOWS-RUNTIME-007",
            Code::ProtonUnpinned => "TUR-WINDOWS-RUNTIME-008",
            Code::LaunchUnsupported => "TUR-WINDOWS-RUNTIME-009",
            Code::StepFailed => "TUR-WINDOWS-RUNTIME-010",
            Code::PrefixMissing => "TUR-WINDOWS-PREFIX-001",
            Code::PrefixExists => "TUR-WINDOWS-PREFIX-002",
            Code::PrefixMetadataInvalid => "TUR-WINDOWS-PREFIX-003",
            Code::PrefixDamaged => "TUR-WINDOWS-PREFIX-004",
            Code::PrefixOwner => "TUR-WINDOWS-PREFIX-005",
            Code::PrefixRuntimeMismatch => "TUR-WINDOWS-PREFIX-006",
            Code::PrefixPinned => "TUR-WINDOWS-PREFIX-007",
            Code::BackupMissing => "TUR-WINDOWS-PREFIX-008",
            Code::CaseFoldUnsupported => "TUR-WINDOWS-PREFIX-009",
            Code::CaseFoldFailed => "TUR-WINDOWS-PREFIX-010",
            Code::RunningAsRoot => "TUR-SECURITY-SANDBOX-001",
            Code::SandboxMissing => "TUR-SECURITY-SANDBOX-002",
            Code::GrantRefused => "TUR-SECURITY-SANDBOX-003",
            Code::GrantChanged => "TUR-SECURITY-SANDBOX-004",
            Code::EnvironmentRefused => "TUR-SECURITY-SANDBOX-005",
            Code::BinaryRefused => "TUR-SECURITY-SANDBOX-006",
            Code::InstallerUnrecognised => "TUR-WINDOWS-APP-001",
            Code::InstallerNoUnattendedMode => "TUR-WINDOWS-APP-002",
            Code::InstallerFailed => "TUR-WINDOWS-APP-003",
            Code::InstallerTimedOut => "TUR-WINDOWS-APP-004",
            Code::InstallNothingFound => "TUR-WINDOWS-APP-005",
            Code::InstallMainProgramUnclear => "TUR-WINDOWS-APP-006",
            Code::AppNotInstalled => "TUR-WINDOWS-APP-007",
            Code::AppRecordInvalid => "TUR-WINDOWS-APP-008",
            Code::UninstallIncomplete => "TUR-WINDOWS-APP-009",
            Code::InstallRequestInvalid => "TUR-WINDOWS-APP-010",
            Code::UninstallerFailed => "TUR-WINDOWS-APP-011",
        }
    }

    /// A short title for the condition.
    pub const fn title(self) -> &'static str {
        match self {
            Code::Unreadable => "The file could not be read",
            Code::Damaged => "The file is truncated or malformed",
            Code::LimitExceeded => "The file exceeds an inspection limit",
            Code::NoRoute => "There is no supported way to run this file",
            Code::WindowsDamaged => "The Windows program file is truncated or malformed",
            Code::WineMissing => "Wine is not installed",
            Code::WindowsKernelRequired => "The file requires a Windows kernel",
            Code::WindowsVmMissing => "The Windows virtual machine backend is not installed",
            Code::WindowsUnsupported => "This kind of Windows file is not supported",
            Code::DarwinDamaged => "The macOS program file or bundle is truncated or malformed",
            Code::DarlingMissing => "Darling is not installed",
            Code::DarwinKext => "macOS kernel extensions cannot be loaded",
            Code::DarwinUnsupported => "This kind of macOS file is not supported",
            Code::AndroidRuntimeMissing => "The Android runtime is not installed",
            Code::AndroidTranslationMissing => "ARM translation for Android is not installed",
            Code::AndroidUnsupported => "This kind of Android package is not supported",
            Code::ProfileUnreadable => "The application profile could not be read",
            Code::ProfileSyntax => "The application profile is not valid",
            Code::ProfileInvalid => "The application profile has a field that is not allowed",
            Code::ProfileSecret => "The application profile appears to contain a credential",
            Code::StorageUnusable => "The runtime manager's own directories cannot be used",
            Code::ToolMissing => "A program the runtime manager needs is not installed",
            Code::RuntimePinMismatch => "The pinned runtime version is not the one installed",
            Code::RuntimeVersionUnknown => "The runtime version could not be determined",
            Code::ProtonLauncherMissing => "umu-run is not installed",
            Code::ProtonUnpinned => "A Proton launch needs a pinned Proton version",
            Code::LaunchUnsupported => "This profile cannot be launched",
            Code::StepFailed => "A step of the operation failed",
            Code::PrefixMissing => "No prefix exists for this application",
            Code::PrefixExists => "A prefix already exists for this application",
            Code::PrefixMetadataInvalid => "The prefix metadata is unreadable or not valid",
            Code::PrefixDamaged => "The prefix failed verification",
            Code::PrefixOwner => "The prefix belongs to another application or profile",
            Code::PrefixRuntimeMismatch => "The prefix is on another runtime version",
            Code::PrefixPinned => "The prefix is pinned to its runtime version",
            Code::BackupMissing => "The backup does not exist",
            Code::CaseFoldUnsupported => {
                "The file system holding the prefix cannot make file names case-insensitive"
            }
            Code::CaseFoldFailed => "The prefix's drive C: could not be made case-insensitive",
            Code::RunningAsRoot => "Windows software is never run as root",
            Code::SandboxMissing => "bubblewrap is not installed",
            Code::GrantRefused => "The directory cannot be granted to an application",
            Code::GrantChanged => "A granted directory is no longer what was granted",
            Code::EnvironmentRefused => "The environment variable cannot be set for an application",
            Code::BinaryRefused => "The application's program file cannot be used",
            Code::InstallerUnrecognised => "The file is not something that can be installed",
            Code::InstallerNoUnattendedMode => "The installer has no known unattended mode",
            Code::InstallerFailed => "The installer failed",
            Code::InstallerTimedOut => "The installer did not finish in time",
            Code::InstallNothingFound => "The installer installed no program",
            Code::InstallMainProgramUnclear => "The application's main program could not be chosen",
            Code::AppNotInstalled => "The application is not installed",
            Code::AppRecordInvalid => "The application record is unreadable or not valid",
            Code::UninstallIncomplete => "Something was left behind by the removal",
            Code::InstallRequestInvalid => "A value given for the installation is not allowed",
            Code::UninstallerFailed => "The application's own uninstaller failed",
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
            .expect("every registered code is a valid TUR error code")
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tur_core::Family;

    use super::*;

    #[test]
    fn every_code_is_a_valid_unique_tur_error_code() {
        let mut seen = BTreeSet::new();
        for code in Code::ALL {
            let parsed: ErrorCode = code.as_str().parse().unwrap();
            assert_eq!(parsed, code.error_code());
            assert!(seen.insert(code.as_str()), "duplicate code {code}");
            assert!(!code.title().is_empty());
        }
        assert_eq!(seen.len(), Code::ALL.len());
    }

    #[test]
    fn codes_use_the_expected_families() {
        assert_eq!(Code::WineMissing.error_code().family(), Family::Windows);
        assert_eq!(Code::WindowsDamaged.error_code().family(), Family::Windows);
        assert_eq!(Code::DarwinKext.error_code().family(), Family::Darwin);
        assert_eq!(
            Code::AndroidTranslationMissing.error_code().family(),
            Family::Android
        );
        assert_eq!(Code::NoRoute.error_code().family(), Family::Runtime);
        assert_eq!(Code::ProfileSecret.error_code().family(), Family::Runtime);
        assert_eq!(Code::PrefixPinned.error_code().family(), Family::Windows);
        assert_eq!(Code::StepFailed.error_code().family(), Family::Windows);
        assert_eq!(Code::RunningAsRoot.error_code().family(), Family::Security);
        assert_eq!(Code::GrantRefused.error_code().family(), Family::Security);
        assert_eq!(Code::StorageUnusable.error_code().family(), Family::Runtime);
        assert_eq!(
            Code::InstallerUnrecognised.error_code().family(),
            Family::Windows
        );
        assert_eq!(
            Code::UninstallerFailed.error_code().family(),
            Family::Windows
        );
    }

    #[test]
    fn the_application_codes_are_numbered_in_order() {
        // Installing, recording and removing an application: one topic, so
        // that the conditions are not mistaken for the operating system
        // installer's `TUR-INSTALL-*`.
        let application: Vec<&str> = Code::ALL
            .iter()
            .map(|code| code.as_str())
            .filter(|code| code.starts_with("TUR-WINDOWS-APP-"))
            .collect();
        let expected: Vec<String> = (1..=11)
            .map(|number| format!("TUR-WINDOWS-APP-{number:03}"))
            .collect();
        assert_eq!(application, expected);
    }

    #[test]
    fn no_code_is_parked_under_the_desktop_family() {
        // The ten conditions that are the runtime manager's own are in its
        // family, under two topics, with the numbers they always had.
        let own: Vec<&str> = Code::ALL
            .iter()
            .filter(|code| code.error_code().family() == Family::Runtime)
            .map(|code| code.as_str())
            .collect();
        let mut sorted = own.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            [
                "TUR-RUNTIME-MANAGER-001",
                "TUR-RUNTIME-MANAGER-002",
                "TUR-RUNTIME-MANAGER-003",
                "TUR-RUNTIME-MANAGER-004",
                "TUR-RUNTIME-MANAGER-005",
                "TUR-RUNTIME-MANAGER-006",
                "TUR-RUNTIME-PROFILE-001",
                "TUR-RUNTIME-PROFILE-002",
                "TUR-RUNTIME-PROFILE-003",
                "TUR-RUNTIME-PROFILE-004",
            ]
        );
        assert_eq!(Code::Unreadable.as_str(), "TUR-RUNTIME-MANAGER-001");
        assert_eq!(Code::NoRoute.as_str(), "TUR-RUNTIME-MANAGER-004");
        assert_eq!(Code::ToolMissing.as_str(), "TUR-RUNTIME-MANAGER-006");
        assert_eq!(Code::ProfileUnreadable.as_str(), "TUR-RUNTIME-PROFILE-001");
        assert_eq!(Code::ProfileSecret.as_str(), "TUR-RUNTIME-PROFILE-004");
        // Every other code is in the family of its ecosystem or in
        // `SECURITY`, and none is left in `DESKTOP`.
        for code in Code::ALL {
            let family = code.error_code().family();
            assert!(
                matches!(
                    family,
                    Family::Runtime
                        | Family::Windows
                        | Family::Darwin
                        | Family::Android
                        | Family::Security
                ),
                "{code} is in {family}"
            );
            assert!(!code.as_str().contains("-DESKTOP-"), "{code}");
        }
        // The codes other programs read did not move.
        assert_eq!(Code::RunningAsRoot.as_str(), "TUR-SECURITY-SANDBOX-001");
        assert_eq!(Code::PrefixPinned.as_str(), "TUR-WINDOWS-PREFIX-007");
        assert_eq!(Code::RuntimePinMismatch.as_str(), "TUR-WINDOWS-RUNTIME-005");
    }

    #[test]
    fn serialises_as_the_code_text() {
        let json = serde_json::to_string(&Code::DarlingMissing).unwrap();
        assert_eq!(json, "\"TUR-DARWIN-RUNTIME-001\"");
    }
}
