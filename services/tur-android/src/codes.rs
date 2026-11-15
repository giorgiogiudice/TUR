// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The stable error codes this component reports (specification §29.3).
//!
//! All are in the `ANDROID` family. Where the Android runtime design
//! (android-runtime.md, section 8.2) proposed a code with the same meaning it
//! is used as proposed. Two of its proposals could not be used: the runtime
//! manager had already released `TUR-ANDROID-RUNTIME-002` and `-003` with
//! other meanings (ARM translation missing; a package no runtime handles), and
//! a released code never changes its meaning. The container's failures are
//! therefore under the `CONTAINER` topic.

use serde::{Serialize, Serializer};
use tur_core::ErrorCode;

/// A condition this component reports with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Code {
    /// The kernel provides no binder driver.
    KernelNoBinder,
    /// Binder devices could not be created or mounted.
    KernelBinderDevices,
    /// Android support is switched off.
    NotEnabled,
    /// Waydroid is not part of this system image.
    WaydroidMissing,
    /// A step of switching Android on or off failed.
    StepFailed,
    /// The Android container did not start.
    ContainerFailed,
    /// Android did not finish starting in time.
    BootTimeout,
    /// The desktop session has no sound server socket, which the container needs.
    NoSoundServer,
    /// An Android image is missing, or its digest does not match the pin.
    ImageMismatch,
    /// An Android image could not be downloaded.
    ImageDownload,
    /// There is not enough free space for the Android images.
    ImageSpace,
    /// The image pins shipped with the system cannot be read.
    ImagePins,
    /// No usable graphics device: Android draws in software.
    SoftwareRendering,
    /// Not a valid Android package.
    PkgInvalid,
    /// The package needs a newer Android than the image provides.
    PkgNewerAndroid,
    /// The file is an App Bundle, which cannot be installed directly.
    PkgBundle,
    /// The file is a split-APK set, which this slice does not install.
    PkgSplitSet,
    /// The package contains ARM code only.
    ArmOnly,
    /// Switching Android on or off needs the person's consent, which was not given.
    ConsentRequired,
    /// Administrator authentication was needed and not given.
    AdminRequired,
    /// The command needs the person's desktop session.
    NoSession,
    /// Android did not install the package.
    InstallFailed,
    /// No Android application with that identifier is installed.
    AppUnknown,
    /// Android did not start the application.
    LaunchFailed,
    /// Android did not remove the application.
    RemoveFailed,
    /// Something of Android was still running, mounted or connected after switching off.
    Leftover,
}

impl Code {
    /// Every code, in registry order.
    pub(crate) const ALL: [Code; 26] = [
        Code::KernelNoBinder,
        Code::KernelBinderDevices,
        Code::NotEnabled,
        Code::WaydroidMissing,
        Code::StepFailed,
        Code::ContainerFailed,
        Code::BootTimeout,
        Code::NoSoundServer,
        Code::ImageMismatch,
        Code::ImageDownload,
        Code::ImageSpace,
        Code::ImagePins,
        Code::SoftwareRendering,
        Code::PkgInvalid,
        Code::PkgNewerAndroid,
        Code::PkgBundle,
        Code::PkgSplitSet,
        Code::ArmOnly,
        Code::ConsentRequired,
        Code::AdminRequired,
        Code::NoSession,
        Code::InstallFailed,
        Code::AppUnknown,
        Code::LaunchFailed,
        Code::RemoveFailed,
        Code::Leftover,
    ];

    /// The code as text.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Code::KernelNoBinder => "TUR-ANDROID-KERNEL-001",
            Code::KernelBinderDevices => "TUR-ANDROID-KERNEL-002",
            Code::NotEnabled => "TUR-ANDROID-RUNTIME-001",
            Code::WaydroidMissing => "TUR-ANDROID-RUNTIME-004",
            Code::StepFailed => "TUR-ANDROID-RUNTIME-005",
            Code::ContainerFailed => "TUR-ANDROID-CONTAINER-001",
            Code::BootTimeout => "TUR-ANDROID-CONTAINER-002",
            Code::NoSoundServer => "TUR-ANDROID-CONTAINER-003",
            Code::ImageMismatch => "TUR-ANDROID-IMAGE-001",
            Code::ImageDownload => "TUR-ANDROID-IMAGE-003",
            Code::ImageSpace => "TUR-ANDROID-IMAGE-004",
            Code::ImagePins => "TUR-ANDROID-IMAGE-005",
            Code::SoftwareRendering => "TUR-ANDROID-GPU-001",
            Code::PkgInvalid => "TUR-ANDROID-PKG-001",
            Code::PkgNewerAndroid => "TUR-ANDROID-PKG-003",
            Code::PkgBundle => "TUR-ANDROID-PKG-006",
            Code::PkgSplitSet => "TUR-ANDROID-PKG-007",
            Code::ArmOnly => "TUR-ANDROID-ABI-001",
            Code::ConsentRequired => "TUR-ANDROID-CONSENT-001",
            Code::AdminRequired => "TUR-ANDROID-CONSENT-002",
            Code::NoSession => "TUR-ANDROID-SESSION-001",
            Code::InstallFailed => "TUR-ANDROID-APP-001",
            Code::AppUnknown => "TUR-ANDROID-APP-002",
            Code::LaunchFailed => "TUR-ANDROID-APP-003",
            Code::RemoveFailed => "TUR-ANDROID-APP-004",
            Code::Leftover => "TUR-ANDROID-DISABLE-001",
        }
    }

    /// A short title for the condition.
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Code::KernelNoBinder => "The kernel provides no binder driver",
            Code::KernelBinderDevices => "The binder devices could not be created or mounted",
            Code::NotEnabled => "Android support is switched off",
            Code::WaydroidMissing => "Waydroid is not part of this system",
            Code::StepFailed => "A step of switching Android on or off failed",
            Code::ContainerFailed => "The Android container did not start",
            Code::BootTimeout => "Android did not finish starting in time",
            Code::NoSoundServer => "The desktop session has no sound server",
            Code::ImageMismatch => "An Android image is missing or does not match its pin",
            Code::ImageDownload => "An Android image could not be downloaded",
            Code::ImageSpace => "There is not enough free space for Android",
            Code::ImagePins => "The Android image pins of this system cannot be read",
            Code::SoftwareRendering => "Android draws in software",
            Code::PkgInvalid => "This is not a valid Android package",
            Code::PkgNewerAndroid => "The package needs a newer Android than this one",
            Code::PkgBundle => "An App Bundle cannot be installed directly",
            Code::PkgSplitSet => "Split-APK sets cannot be installed yet",
            Code::ArmOnly => "The package contains ARM code only",
            Code::ConsentRequired => "Your consent is needed first",
            Code::AdminRequired => "Administrator authentication is needed",
            Code::NoSession => "This needs your desktop session",
            Code::InstallFailed => "Android did not install the package",
            Code::AppUnknown => "No such Android application is installed",
            Code::LaunchFailed => "Android did not start the application",
            Code::RemoveFailed => "Android did not remove the application",
            Code::Leftover => "Something of Android is still running after switching off",
        }
    }

    /// The code as a validated [`ErrorCode`]: the family and number of a
    /// code, as `tur-core` reads them.
    pub(crate) fn error_code(self) -> ErrorCode {
        ErrorCode::from_static(self.as_str())
    }
}

impl Serialize for Code {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use tur_core::Family;

    #[test]
    fn every_code_is_valid_unique_and_android() {
        let mut seen = BTreeSet::new();
        for code in Code::ALL {
            assert_eq!(code.error_code().family(), Family::Android, "{code:?}");
            assert_eq!(code.error_code().as_str(), code.as_str());
            assert!(seen.insert(code.as_str()), "{} twice", code.as_str());
            assert!(!code.title().is_empty());
            assert!(!code.title().ends_with('.'), "{code:?}");
        }
    }

    #[test]
    fn the_runtime_managers_released_codes_keep_their_meaning() {
        // tur-runtime released TUR-ANDROID-RUNTIME-002 (ARM translation is
        // not installed) and -003 (an Android package no runtime handles).
        // Neither number is reused here with another meaning.
        for code in Code::ALL {
            assert_ne!(code.as_str(), "TUR-ANDROID-RUNTIME-002");
            assert_ne!(code.as_str(), "TUR-ANDROID-RUNTIME-003");
        }
        // RUNTIME-001 means the same in both: Android is not available
        // until it is switched on.
        assert_eq!(Code::NotEnabled.as_str(), "TUR-ANDROID-RUNTIME-001");
    }

    #[test]
    fn serialises_as_the_code_text() {
        let json = serde_json::to_string(&Code::ArmOnly).unwrap();
        assert_eq!(json, "\"TUR-ANDROID-ABI-001\"");
    }
}
