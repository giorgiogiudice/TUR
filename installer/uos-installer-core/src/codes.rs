// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

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
use uos_core::{ErrorCode, ErrorCodeParseError};

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
        /// A registered `UOS-INSTALL-*` code.
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

            /// The code as text, for example `UOS-INSTALL-TARGET-001`.
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
    InventoryUnreadable => ("UOS-INSTALL-INVENTORY-001", Error, "Installer input data could not be read"),
    InventoryMalformed => ("UOS-INSTALL-INVENTORY-002", Error, "Installer input data is not in the expected format"),

    TargetDevicePath => ("UOS-INSTALL-TARGET-001", Error, "A device path was given as the installation target"),
    TargetSelectorInvalid => ("UOS-INSTALL-TARGET-002", Error, "The target selector is not valid"),
    TargetNotFound => ("UOS-INSTALL-TARGET-003", Error, "No disk matches the target selector"),
    TargetAmbiguous => ("UOS-INSTALL-TARGET-004", Error, "More than one disk matches the target"),
    TargetNotWholeDisk => ("UOS-INSTALL-TARGET-005", Error, "The target is not a whole disk"),
    TargetNoMedium => ("UOS-INSTALL-TARGET-006", Error, "The target drive holds no medium"),
    TargetWeakIdentity => ("UOS-INSTALL-TARGET-007", Error, "The target has no stable identifier and this was not acknowledged"),

    IdentityMissing => ("UOS-INSTALL-IDENTITY-001", Error, "The target disk is no longer present"),
    IdentitySizeChanged => ("UOS-INSTALL-IDENTITY-002", Error, "The target disk changed size"),
    IdentitySerialChanged => ("UOS-INSTALL-IDENTITY-003", Error, "A different disk is now in the target's place"),
    IdentityAmbiguous => ("UOS-INSTALL-IDENTITY-004", Error, "Several disks now match the target's identity"),
    IdentityAttributesChanged => ("UOS-INSTALL-IDENTITY-005", Error, "The target disk's reported attributes changed"),

    PreflightBootMode => ("UOS-INSTALL-PREFLIGHT-001", Error, "Preflight check: firmware boot mode is UEFI"),
    PreflightSecureBoot => ("UOS-INSTALL-PREFLIGHT-002", Warning, "Preflight check: Secure Boot state"),
    PreflightCpuArchitecture => ("UOS-INSTALL-PREFLIGHT-003", Error, "Preflight check: CPU architecture"),
    PreflightCpuFeatures => ("UOS-INSTALL-PREFLIGHT-004", Error, "Preflight check: required CPU features"),
    PreflightMemory => ("UOS-INSTALL-PREFLIGHT-005", Error, "Preflight check: installed memory"),
    PreflightTargetCapacity => ("UOS-INSTALL-PREFLIGHT-006", Error, "Preflight check: target disk capacity"),
    PreflightTargetInUse => ("UOS-INSTALL-PREFLIGHT-007", Error, "Preflight check: target disk is not in use"),
    PreflightTargetLiveMedium => ("UOS-INSTALL-PREFLIGHT-008", Error, "Preflight check: target disk is not the live medium"),
    PreflightOtherOs => ("UOS-INSTALL-PREFLIGHT-009", Warning, "Preflight check: other operating systems on the target"),
    PreflightSafeGraphics => ("UOS-INSTALL-PREFLIGHT-010", Warning, "Preflight check: installer graphics mode"),
    PreflightNetwork => ("UOS-INSTALL-PREFLIGHT-011", Info, "Preflight check: network availability"),
    PreflightTargetReadOnly => ("UOS-INSTALL-PREFLIGHT-012", Error, "Preflight check: target disk is writable"),
    PreflightTargetIdentity => ("UOS-INSTALL-PREFLIGHT-013", Warning, "Preflight check: target disk has a stable identifier"),

    PlanDiskTooSmall => ("UOS-INSTALL-PLAN-001", Error, "The target disk is too small"),
    PlanModeNotImplemented => ("UOS-INSTALL-PLAN-002", Error, "The installation mode is not implemented yet"),
    PlanSectorSize => ("UOS-INSTALL-PLAN-003", Error, "The target disk's sector size is not supported"),
    PlanArithmetic => ("UOS-INSTALL-PLAN-004", Error, "A partition size calculation overflowed"),
    PlanTargetLiveMedium => ("UOS-INSTALL-PLAN-005", Error, "The target is the medium the installer runs from"),
    PlanTargetInUse => ("UOS-INSTALL-PLAN-006", Error, "The target disk is in use"),
    PlanTargetReadOnly => ("UOS-INSTALL-PLAN-007", Error, "The target disk is read-only"),
    PlanInvalid => ("UOS-INSTALL-PLAN-008", Error, "The partition plan is not consistent"),

    ConfirmPhraseMismatch => ("UOS-INSTALL-CONFIRM-001", Error, "The confirmation phrase was not typed exactly"),
    ConfirmPreflightBlocked => ("UOS-INSTALL-CONFIRM-002", Error, "A blocking preflight check prevents confirmation"),
    ConfirmPreflightMismatch => ("UOS-INSTALL-CONFIRM-003", Error, "The preflight report is not for the planned target"),
    ConfirmContentsChanged => ("UOS-INSTALL-CONFIRM-004", Error, "The target's contents changed after confirmation"),
    ConfirmWeakIdentity => ("UOS-INSTALL-CONFIRM-005", Error, "The plan's weaker identity was not acknowledged"),

    PhaseIllegalTransition => ("UOS-INSTALL-PHASE-001", Error, "Illegal phase transition"),
    PhaseOutOfOrder => ("UOS-INSTALL-PHASE-002", Error, "A phase was started before an earlier phase finished"),
    PhaseRetryNotAllowed => ("UOS-INSTALL-PHASE-003", Error, "A non-repeatable phase cannot be retried"),
    PhaseReopenRefused => ("UOS-INSTALL-PHASE-004", Error, "Phases cannot be reopened after the target was written"),
    PhaseNotOptional => ("UOS-INSTALL-PHASE-005", Error, "A mandatory phase cannot be skipped"),

    SessionIo => ("UOS-INSTALL-SESSION-001", Error, "The session directory could not be read or written"),
    SessionCorrupt => ("UOS-INSTALL-SESSION-002", Error, "A session file is corrupt"),
    SessionUnsupportedVersion => ("UOS-INSTALL-SESSION-003", Error, "The session was written by an incompatible installer"),
    SessionReconciliationRequired => ("UOS-INSTALL-SESSION-004", Error, "The session wrote to the target and must be reconciled"),
    SessionInterrupted => ("UOS-INSTALL-SESSION-005", Warning, "The installer stopped while a phase was running"),
    SessionAlreadyExists => ("UOS-INSTALL-SESSION-006", Error, "The session directory already holds a session"),
    SessionTargetNotValidated => ("UOS-INSTALL-SESSION-007", Error, "A phase that writes to the target needs a revalidated target"),
    SessionNoPlan => ("UOS-INSTALL-SESSION-008", Error, "The session has no partition plan"),
    SessionNotFound => ("UOS-INSTALL-SESSION-009", Error, "No session was found in the directory"),
    SessionNotResumable => ("UOS-INSTALL-SESSION-010", Error, "The session cannot be resumed"),
    SessionInvalidId => ("UOS-INSTALL-SESSION-011", Error, "The session identifier is not valid"),
    SessionUnusable => ("UOS-INSTALL-SESSION-012", Error, "The session failed to persist and must be reloaded"),
    SessionPlanLocked => ("UOS-INSTALL-SESSION-013", Error, "The plan cannot change after the target was written"),
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

/// Any valid UOS error code, as stored in a phase record.
///
/// A phase can fail with a code from another family (for example
/// `UOS-STORAGE-*`), so this type accepts every code [`ErrorCode`] accepts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Code(String);

impl Code {
    /// Validates `text` as a UOS error code.
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

    use uos_core::Family;

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
            assert!(code.as_str().starts_with("UOS-INSTALL-"), "{code}");
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
        assert!(serde_json::from_str::<InstallCode>("\"UOS-INSTALL-NOPE-999\"").is_err());
    }

    #[test]
    fn general_codes_are_validated() {
        let storage = Code::parse("UOS-STORAGE-NTFS-004").unwrap();
        assert_eq!(storage.as_str(), "UOS-STORAGE-NTFS-004");
        assert!(Code::parse("sda").is_err());
        assert!(serde_json::from_str::<Code>("\"UOS-NOPE-001\"").is_err());
        let from_install: Code = InstallCode::SessionInterrupted.into();
        assert_eq!(from_install.as_str(), "UOS-INSTALL-SESSION-005");
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
