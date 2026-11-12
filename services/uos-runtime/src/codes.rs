// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The stable error codes this component reports (specification §29.3).
//!
//! `uos-core` has no family for the runtime manager itself, so conditions that
//! are not specific to one ecosystem use the `DESKTOP` family with a `RUNTIME`
//! or `PROFILE` topic. Conditions specific to Windows, macOS or Android
//! software use the `WINDOWS`, `DARWIN` and `ANDROID` families.

use std::fmt;

use serde::{Serialize, Serializer};
use uos_core::ErrorCode;

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
    /// A Windows file that no UOS runtime handles.
    WindowsUnsupported,
    /// A Mach-O file or application bundle is truncated or malformed.
    DarwinDamaged,
    /// Darling is recommended and is not installed.
    DarlingMissing,
    /// A macOS kernel extension, which is never loaded.
    DarwinKext,
    /// A macOS file that no UOS runtime handles.
    DarwinUnsupported,
    /// The Android runtime is recommended and is not installed.
    AndroidRuntimeMissing,
    /// The package needs ARM translation, which is not installed.
    AndroidTranslationMissing,
    /// An Android package that no UOS runtime handles.
    AndroidUnsupported,
    /// An application profile could not be read.
    ProfileUnreadable,
    /// An application profile is not valid JSON or has the wrong shape.
    ProfileSyntax,
    /// An application profile field holds a value that is not allowed.
    ProfileInvalid,
    /// An application profile holds something that looks like a credential.
    ProfileSecret,
}

impl Code {
    /// Every code this component can report.
    pub const ALL: [Code; 20] = [
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
    ];

    /// The code as text, for example `UOS-WINDOWS-RUNTIME-001`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::Unreadable => "UOS-DESKTOP-RUNTIME-001",
            Code::Damaged => "UOS-DESKTOP-RUNTIME-002",
            Code::LimitExceeded => "UOS-DESKTOP-RUNTIME-003",
            Code::NoRoute => "UOS-DESKTOP-RUNTIME-004",
            Code::WindowsDamaged => "UOS-WINDOWS-INSPECT-001",
            Code::WineMissing => "UOS-WINDOWS-RUNTIME-001",
            Code::WindowsKernelRequired => "UOS-WINDOWS-RUNTIME-002",
            Code::WindowsVmMissing => "UOS-WINDOWS-RUNTIME-003",
            Code::WindowsUnsupported => "UOS-WINDOWS-RUNTIME-004",
            Code::DarwinDamaged => "UOS-DARWIN-INSPECT-001",
            Code::DarlingMissing => "UOS-DARWIN-RUNTIME-001",
            Code::DarwinKext => "UOS-DARWIN-RUNTIME-002",
            Code::DarwinUnsupported => "UOS-DARWIN-RUNTIME-003",
            Code::AndroidRuntimeMissing => "UOS-ANDROID-RUNTIME-001",
            Code::AndroidTranslationMissing => "UOS-ANDROID-RUNTIME-002",
            Code::AndroidUnsupported => "UOS-ANDROID-RUNTIME-003",
            Code::ProfileUnreadable => "UOS-DESKTOP-PROFILE-001",
            Code::ProfileSyntax => "UOS-DESKTOP-PROFILE-002",
            Code::ProfileInvalid => "UOS-DESKTOP-PROFILE-003",
            Code::ProfileSecret => "UOS-DESKTOP-PROFILE-004",
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
            .expect("every registered code is a valid UOS error code")
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

    use uos_core::Family;

    use super::*;

    #[test]
    fn every_code_is_a_valid_unique_uos_error_code() {
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
        assert_eq!(Code::NoRoute.error_code().family(), Family::Desktop);
        assert_eq!(Code::ProfileSecret.error_code().family(), Family::Desktop);
    }

    #[test]
    fn serialises_as_the_code_text() {
        let json = serde_json::to_string(&Code::DarlingMissing).unwrap();
        assert_eq!(json, "\"UOS-DARWIN-RUNTIME-001\"");
    }
}
