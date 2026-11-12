// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Registry of the installer's stable error codes (specification §29.3).
//!
//! Every condition the installer core can report has one [`InstallCode`]. The
//! meaning of a released code never changes. A preflight code names a check:
//! the result's status says whether that check passed, warned or blocked.
//!
//! Errors implement [`Fault`], which gives the two layers specification §6.3
//! requires: a user message (`Display`) and a technical detail.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tur_core::{ErrorCode, ErrorCodeParseError};

/// How serious a registered condition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational: the installation is not affected.
    Info,
    /// The installation can continue, but the user should know.
    Warning,
    /// The requested operation cannot continue.
    Error,
}

macro_rules! install_codes {
    ($( $variant:ident => ($text:literal, $severity:ident, $title:literal) ),+ $(,)?) => {
        /// A registered `TUR-INSTALL-*` code.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum InstallCode {
            $(
                #[doc = $title]
                $variant,
            )+
        }

        impl InstallCode {
            /// Every registered code, in registry order.
            pub const ALL: &'static [InstallCode] = &[ $( InstallCode::$variant ),+ ];

            /// The code as text, for example `TUR-INSTALL-TARGET-001`.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( InstallCode::$variant => $text, )+
                }
            }

            /// The registry title: one line saying what the code means.
            pub const fn title(self) -> &'static str {
                match self {
                    $( InstallCode::$variant => $title, )+
                }
            }

            /// The registry severity.
            pub const fn severity(self) -> Severity {
                match self {
                    $( InstallCode::$variant => Severity::$severity, )+
                }
            }
        }
    };
}

install_codes! {
    InventoryUnreadable => ("TUR-INSTALL-INVENTORY-001", Error, "Installer input data could not be read"),
    InventoryMalformed => ("TUR-INSTALL-INVENTORY-002", Error, "Installer input data is not in the expected format"),

    TargetDevicePath => ("TUR-INSTALL-TARGET-001", Error, "A device path was given as the installation target"),
    TargetSelectorInvalid => ("TUR-INSTALL-TARGET-002", Error, "The target selector is not valid"),
    TargetNotFound => ("TUR-INSTALL-TARGET-003", Error, "No disk matches the target selector"),
    TargetAmbiguous => ("TUR-INSTALL-TARGET-004", Error, "More than one disk matches the target"),
    TargetNotWholeDisk => ("TUR-INSTALL-TARGET-005", Error, "The target is not a whole disk"),
    TargetNoMedium => ("TUR-INSTALL-TARGET-006", Error, "The target drive holds no medium"),
    TargetWeakIdentity => ("TUR-INSTALL-TARGET-007", Error, "The target has no stable identifier and this was not acknowledged"),

    IdentityMissing => ("TUR-INSTALL-IDENTITY-001", Error, "The target disk is no longer present"),
    IdentitySizeChanged => ("TUR-INSTALL-IDENTITY-002", Error, "The target disk changed size"),
    IdentitySerialChanged => ("TUR-INSTALL-IDENTITY-003", Error, "A different disk is now in the target's place"),
    IdentityAmbiguous => ("TUR-INSTALL-IDENTITY-004", Error, "Several disks now match the target's identity"),
    IdentityAttributesChanged => ("TUR-INSTALL-IDENTITY-005", Error, "The target disk's reported attributes changed"),

    PreflightBootMode => ("TUR-INSTALL-PREFLIGHT-001", Error, "Preflight check: firmware boot mode is UEFI"),
    PreflightSecureBoot => ("TUR-INSTALL-PREFLIGHT-002", Warning, "Preflight check: Secure Boot state"),
    PreflightCpuArchitecture => ("TUR-INSTALL-PREFLIGHT-003", Error, "Preflight check: CPU architecture"),
    PreflightCpuFeatures => ("TUR-INSTALL-PREFLIGHT-004", Error, "Preflight check: required CPU features"),
    PreflightMemory => ("TUR-INSTALL-PREFLIGHT-005", Error, "Preflight check: installed memory"),
    PreflightTargetCapacity => ("TUR-INSTALL-PREFLIGHT-006", Error, "Preflight check: target disk capacity"),
    PreflightTargetInUse => ("TUR-INSTALL-PREFLIGHT-007", Error, "Preflight check: target disk is not in use"),
    PreflightTargetLiveMedium => ("TUR-INSTALL-PREFLIGHT-008", Error, "Preflight check: target disk is not the live medium"),
    PreflightOtherOs => ("TUR-INSTALL-PREFLIGHT-009", Warning, "Preflight check: other operating systems on the target"),
    PreflightSafeGraphics => ("TUR-INSTALL-PREFLIGHT-010", Warning, "Preflight check: installer graphics mode"),
    PreflightNetwork => ("TUR-INSTALL-PREFLIGHT-011", Info, "Preflight check: network availability"),
    PreflightTargetReadOnly => ("TUR-INSTALL-PREFLIGHT-012", Error, "Preflight check: target disk is writable"),
    PreflightTargetIdentity => ("TUR-INSTALL-PREFLIGHT-013", Warning, "Preflight check: target disk has a stable identifier"),

    PlanDiskTooSmall => ("TUR-INSTALL-PLAN-001", Error, "The target disk is too small"),
    PlanModeNotImplemented => ("TUR-INSTALL-PLAN-002", Error, "The installation mode is not implemented yet"),
    PlanSectorSize => ("TUR-INSTALL-PLAN-003", Error, "The target disk's sector size is not supported"),
    PlanArithmetic => ("TUR-INSTALL-PLAN-004", Error, "A partition size calculation overflowed"),
    PlanTargetLiveMedium => ("TUR-INSTALL-PLAN-005", Error, "The target is the medium the installer runs from"),
    PlanTargetInUse => ("TUR-INSTALL-PLAN-006", Error, "The target disk is in use"),
    PlanTargetReadOnly => ("TUR-INSTALL-PLAN-007", Error, "The target disk is read-only"),
    PlanInvalid => ("TUR-INSTALL-PLAN-008", Error, "The partition plan is not consistent"),

    ConfirmPhraseMismatch => ("TUR-INSTALL-CONFIRM-001", Error, "The confirmation phrase was not typed exactly"),
    ConfirmPreflightBlocked => ("TUR-INSTALL-CONFIRM-002", Error, "A blocking preflight check prevents confirmation"),
    ConfirmPreflightMismatch => ("TUR-INSTALL-CONFIRM-003", Error, "The preflight report is not for the planned target"),
    ConfirmContentsChanged => ("TUR-INSTALL-CONFIRM-004", Error, "The target's contents changed after confirmation"),
    ConfirmWeakIdentity => ("TUR-INSTALL-CONFIRM-005", Error, "The plan's weaker identity was not acknowledged"),

    PhaseIllegalTransition => ("TUR-INSTALL-PHASE-001", Error, "Illegal phase transition"),
    PhaseOutOfOrder => ("TUR-INSTALL-PHASE-002", Error, "A phase was started before an earlier phase finished"),
    PhaseRetryNotAllowed => ("TUR-INSTALL-PHASE-003", Error, "A non-repeatable phase cannot be retried"),
    PhaseReopenRefused => ("TUR-INSTALL-PHASE-004", Error, "Phases cannot be reopened after the target was written"),
    PhaseNotOptional => ("TUR-INSTALL-PHASE-005", Error, "A mandatory phase cannot be skipped"),

    SessionIo => ("TUR-INSTALL-SESSION-001", Error, "The session directory could not be read or written"),
    SessionCorrupt => ("TUR-INSTALL-SESSION-002", Error, "A session file is corrupt"),
    SessionUnsupportedVersion => ("TUR-INSTALL-SESSION-003", Error, "The session was written by an incompatible installer"),
    SessionReconciliationRequired => ("TUR-INSTALL-SESSION-004", Error, "The session wrote to the target and must be reconciled"),
    SessionInterrupted => ("TUR-INSTALL-SESSION-005", Warning, "The installer stopped while a phase was running"),
    SessionAlreadyExists => ("TUR-INSTALL-SESSION-006", Error, "The session directory already holds a session"),
    SessionTargetNotValidated => ("TUR-INSTALL-SESSION-007", Error, "A phase that writes to the target needs a revalidated target"),
    SessionNoPlan => ("TUR-INSTALL-SESSION-008", Error, "The session has no partition plan"),
    SessionNotFound => ("TUR-INSTALL-SESSION-009", Error, "No session was found in the directory"),
    SessionNotResumable => ("TUR-INSTALL-SESSION-010", Error, "The session cannot be resumed"),
    SessionInvalidId => ("TUR-INSTALL-SESSION-011", Error, "The session identifier is not valid"),
    SessionUnusable => ("TUR-INSTALL-SESSION-012", Error, "The session failed to persist and must be reloaded"),
    SessionPlanLocked => ("TUR-INSTALL-SESSION-013", Error, "The plan cannot change after the target was written"),
}

impl InstallCode {
    /// The code as the shared [`ErrorCode`] type.
    ///
    /// Every registered code parses; the registry tests check this.
    pub fn error_code(self) -> Result<ErrorCode, ErrorCodeParseError> {
        self.as_str().parse()
    }

    /// Looks a registered code up by its text.
    pub fn from_text(text: &str) -> Option<InstallCode> {
        Self::ALL.iter().copied().find(|code| code.as_str() == text)
    }
}

impl fmt::Display for InstallCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for InstallCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InstallCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        InstallCode::from_text(&text).ok_or_else(|| {
            serde::de::Error::custom(format!("\"{text}\" is not a registered installer code"))
        })
    }
}

/// Any valid TUR error code, as stored in a phase record.
///
/// A phase can fail with a code from another family (for example
/// `TUR-STORAGE-*`), so this type accepts every code [`ErrorCode`] accepts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Code(String);

impl Code {
    /// Validates `text` as a TUR error code.
    pub fn parse(text: &str) -> Result<Code, ErrorCodeParseError> {
        let code: ErrorCode = text.parse()?;
        Ok(Code(code.as_str().to_owned()))
    }

    /// The code as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<InstallCode> for Code {
    fn from(code: InstallCode) -> Self {
        Code(code.as_str().to_owned())
    }
}

impl From<ErrorCode> for Code {
    fn from(code: ErrorCode) -> Self {
        Code(code.as_str().to_owned())
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Code {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Code {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Code::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A typed installer failure with the two layers specification §6.3 requires.
///
/// `Display` is the user message: what happened and what to do. It never
/// suggests forcing an unsafe operation.
pub trait Fault: std::error::Error {
    /// The stable code for this failure.
    fn code(&self) -> InstallCode;

    /// The technical detail: exact identifiers and values, for a developer.
    fn technical_detail(&self) -> String;
}

/// A [`Fault`] flattened for display or serialisation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultReport {
    /// The stable code.
    pub code: InstallCode,
    /// The user message.
    pub message: String,
    /// The technical detail.
    pub technical_detail: String,
}

impl FaultReport {
    /// Captures a fault's code, user message and technical detail.
    pub fn of(fault: &dyn Fault) -> FaultReport {
        FaultReport {
            code: fault.code(),
            message: fault.to_string(),
            technical_detail: fault.technical_detail(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tur_core::Family;

    use super::*;

    #[test]
    fn every_registered_code_is_a_valid_install_code() {
        for &code in InstallCode::ALL {
            let parsed = code
                .error_code()
                .unwrap_or_else(|error| panic!("{code} is malformed: {error}"));
            assert_eq!(parsed.family(), Family::Install, "{code}");
            assert_eq!(parsed.as_str(), code.as_str());
            assert!(!code.title().is_empty(), "{code}");
            assert!(code.as_str().starts_with("TUR-INSTALL-"), "{code}");
        }
    }

    #[test]
    fn registered_codes_are_unique() {
        let texts: BTreeSet<&str> = InstallCode::ALL.iter().map(|code| code.as_str()).collect();
        assert_eq!(texts.len(), InstallCode::ALL.len());
    }

    #[test]
    fn install_codes_round_trip_through_json() {
        for &code in InstallCode::ALL {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.as_str()));
            assert_eq!(serde_json::from_str::<InstallCode>(&json).unwrap(), code);
        }
        assert!(serde_json::from_str::<InstallCode>("\"TUR-INSTALL-NOPE-999\"").is_err());
    }

    #[test]
    fn general_codes_are_validated() {
        let storage = Code::parse("TUR-STORAGE-NTFS-004").unwrap();
        assert_eq!(storage.as_str(), "TUR-STORAGE-NTFS-004");
        assert!(Code::parse("sda").is_err());
        assert!(serde_json::from_str::<Code>("\"TUR-NOPE-001\"").is_err());
        let from_install: Code = InstallCode::SessionInterrupted.into();
        assert_eq!(from_install.as_str(), "TUR-INSTALL-SESSION-005");
        let json = serde_json::to_string(&from_install).unwrap();
        assert_eq!(serde_json::from_str::<Code>(&json).unwrap(), from_install);
    }

    #[test]
    fn severities_follow_the_registry() {
        assert_eq!(InstallCode::TargetDevicePath.severity(), Severity::Error);
        assert_eq!(InstallCode::PreflightOtherOs.severity(), Severity::Warning);
        assert_eq!(InstallCode::PreflightNetwork.severity(), Severity::Info);
    }
}
