// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The stable `TUR-BOOT-*` error codes of this program (design section 12).
//! The meaning of a released code never changes (specification §29.3).

use tur_core::ErrorCode;

/// Required check R1 failed: the root filesystem is not the booted deployment.
pub(crate) const HEALTH_ROOT: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-001");
/// Required check R2 failed: a critical path is not readable or writable.
pub(crate) const HEALTH_PATHS: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-002");
/// Required check R3 failed: the default target is not active.
pub(crate) const HEALTH_DEFAULT_TARGET: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-003");
/// Required check R4 failed: a core service is not active.
pub(crate) const HEALTH_CORE_UNITS: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-004");
/// Required check R5 failed: a required TUR unit is not active.
pub(crate) const HEALTH_TUR_UNITS: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-005");
/// Required check R6 failed: the display manager is not active.
pub(crate) const HEALTH_DISPLAY_MANAGER: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-006");
/// Required check R7 failed: the seat has no graphical session.
pub(crate) const HEALTH_SEAT_SESSION: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-007");
/// Required check R8 failed: the seat's session did not stay up.
pub(crate) const HEALTH_STABILITY: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-008");
/// A required check exceeded its time limit.
pub(crate) const HEALTH_TIMEOUT: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-020");
/// A required check from `checks.d` failed.
pub(crate) const HEALTH_EXTRA_CHECK: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-021");
/// The gate itself did not finish: no verdict was written.
pub(crate) const HEALTH_GATE_DIED: ErrorCode = ErrorCode::from_static("TUR-BOOT-HEALTH-030");
/// The update was not applied: finalisation did not happen.
pub(crate) const TRIAL_NOT_APPLIED: ErrorCode = ErrorCode::from_static("TUR-BOOT-TRIAL-001");
/// The update was refused: boot attempts cannot be counted on this installation.
pub(crate) const TRIAL_REFUSED: ErrorCode = ErrorCode::from_static("TUR-BOOT-TRIAL-002");
/// Fallback: the new version failed all attempts.
pub(crate) const TRIAL_FALLBACK: ErrorCode = ErrorCode::from_static("TUR-BOOT-TRIAL-003");
/// Fallback: the loader could not start the new version.
pub(crate) const TRIAL_FALLBACK_LOADER: ErrorCode = ErrorCode::from_static("TUR-BOOT-TRIAL-004");
/// GRUB could not save the counter; attempts are counted in userspace.
pub(crate) const TRIAL_COUNTED_IN_USERSPACE: ErrorCode =
    ErrorCode::from_static("TUR-BOOT-TRIAL-005");
/// The boot order does not match the trial record; nothing was changed.
pub(crate) const STATE_MISMATCH: ErrorCode = ErrorCode::from_static("TUR-BOOT-STATE-001");
/// A counter existed without a trial; removed.
pub(crate) const STATE_STRAY_COUNTER: ErrorCode = ErrorCode::from_static("TUR-BOOT-STATE-002");
/// The environment block was invalid and was recreated.
pub(crate) const STATE_BLOCK_RECREATED: ErrorCode = ErrorCode::from_static("TUR-BOOT-STATE-003");
/// `bootc rollback` failed during a fallback.
pub(crate) const STATE_ROLLBACK_FAILED: ErrorCode = ErrorCode::from_static("TUR-BOOT-STATE-004");

/// Every code with its meaning, for `status --codes` and for the tests.
pub(crate) fn registry() -> Vec<(ErrorCode, &'static str)> {
    vec![
        (
            HEALTH_ROOT,
            "Required check R1 failed: the root filesystem is not the booted deployment",
        ),
        (
            HEALTH_PATHS,
            "Required check R2 failed: a critical path is not readable or writable",
        ),
        (
            HEALTH_DEFAULT_TARGET,
            "Required check R3 failed: the default target is not active",
        ),
        (
            HEALTH_CORE_UNITS,
            "Required check R4 failed: a core service is not active",
        ),
        (
            HEALTH_TUR_UNITS,
            "Required check R5 failed: a required TUR unit is not active",
        ),
        (
            HEALTH_DISPLAY_MANAGER,
            "Required check R6 failed: the display manager is not active",
        ),
        (
            HEALTH_SEAT_SESSION,
            "Required check R7 failed: the seat has no graphical session",
        ),
        (
            HEALTH_STABILITY,
            "Required check R8 failed: the seat's session did not stay up",
        ),
        (HEALTH_TIMEOUT, "A required check exceeded its time limit"),
        (HEALTH_EXTRA_CHECK, "A required check from checks.d failed"),
        (HEALTH_GATE_DIED, "The gate itself did not finish"),
        (
            TRIAL_NOT_APPLIED,
            "The update was not applied: finalisation did not happen",
        ),
        (
            TRIAL_REFUSED,
            "The update was refused: boot attempts cannot be counted on this installation",
        ),
        (
            TRIAL_FALLBACK,
            "Fallback: the new version failed all attempts",
        ),
        (
            TRIAL_FALLBACK_LOADER,
            "Fallback: the loader could not start the new version",
        ),
        (
            TRIAL_COUNTED_IN_USERSPACE,
            "GRUB could not save the counter; attempts counted in userspace",
        ),
        (
            STATE_MISMATCH,
            "The boot order does not match the trial record; nothing was changed",
        ),
        (
            STATE_STRAY_COUNTER,
            "A counter existed without a trial; removed",
        ),
        (
            STATE_BLOCK_RECREATED,
            "The environment block was invalid and was recreated",
        ),
        (
            STATE_ROLLBACK_FAILED,
            "bootc rollback failed during a fallback",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use tur_core::Family;

    #[test]
    fn every_code_is_a_boot_code_and_is_listed_once() {
        let registry = registry();
        let unique: BTreeSet<String> = registry.iter().map(|(code, _)| code.to_string()).collect();
        assert_eq!(unique.len(), registry.len());
        assert_eq!(registry.len(), 20);
        for (code, meaning) in &registry {
            assert_eq!(code.family(), Family::Boot, "{code}");
            assert!(!meaning.is_empty());
            assert!(
                code.as_str().starts_with("TUR-BOOT-HEALTH-")
                    || code.as_str().starts_with("TUR-BOOT-TRIAL-")
                    || code.as_str().starts_with("TUR-BOOT-STATE-"),
                "{code}"
            );
        }
    }

    #[test]
    fn the_required_checks_have_the_codes_of_the_design() {
        assert_eq!(HEALTH_ROOT.number(), 1);
        assert_eq!(HEALTH_STABILITY.number(), 8);
        assert_eq!(HEALTH_TIMEOUT.as_str(), "TUR-BOOT-HEALTH-020");
        assert_eq!(TRIAL_FALLBACK.as_str(), "TUR-BOOT-TRIAL-003");
        assert_eq!(STATE_ROLLBACK_FAILED.as_str(), "TUR-BOOT-STATE-004");
    }
}
