// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The mount decision.
//!
//! [`decide`] is a pure function from volume facts, the requested access and
//! the policy overrides to a [`Decision`]. It reads the filesystem policy
//! table and nothing else: no device, no file, no clock, no process state.
//!
//! Rules, in order:
//!
//! 1. The running system's own storage is never offered.
//! 2. A volume whose identity is claimed by another device is refused.
//! 3. A volume already mounted outside the UOS namespace is not mounted again.
//! 4. Unsupported filesystems and containers are never passed to a driver.
//! 5. Writing needs every write condition of the policy row to hold, a
//!    writable row and a writable device. Otherwise a read/write request is
//!    downgraded to read-only, or refused; it is never granted.
//!
//! No decision carries an option that overrides a driver's safety check, and
//! no message recommends one.

use serde::Serialize;

use crate::codes::{Severity, StorageCode};
use crate::filesystem::Filesystem;
use crate::namespace::canonical_path;
use crate::policy::{
    DefaultAccess, FsPolicy, ImplementationKind, PolicyOverrides, ReadCondition, UnsafeNtfsPolicy,
    WriteCondition, policy_for,
};
use crate::volume::{Encryption, HfsJournal, NtfsState, Volume, normalise_identifier};

string_enum! {
    /// The access a caller asks for.
    pub enum Access {
        /// Read-only.
        ReadOnly => "read-only",
        /// Read and write.
        ReadWrite => "read-write",
    }
}

string_enum! {
    /// What the policy decides to do with a volume.
    pub enum Outcome {
        /// Mount it read/write.
        MountReadWrite => "mount-read-write",
        /// Mount it read-only. The decision says why.
        MountReadOnly => "mount-read-only",
        /// Do not mount it.
        Refuse => "refuse",
        /// It is encrypted and locked; unlock it first.
        NeedsUnlock => "needs-unlock",
        /// UOS does not support it.
        Unsupported => "unsupported",
    }
}

/// A request to mount a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MountRequest {
    /// The access asked for.
    pub access: Access,
    /// The user who will own files on filesystems without POSIX ownership.
    pub uid: u32,
    /// The group for the same purpose.
    pub gid: u32,
}

/// The technical layer of a decision (specification §6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TechnicalDetail {
    /// The device, by filesystem UUID where there is a usable one.
    pub device: String,
    /// The driver the decision selects, if it mounts.
    pub driver: Option<&'static str>,
    /// The mount policy applied: `read/write`, `read-only` or `not mounted`.
    pub mount_policy: &'static str,
    /// The facts the decision rests on.
    pub evidence: String,
    /// The registry's technical explanation of the code.
    pub explanation: &'static str,
}

/// A mount decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    /// What to do.
    pub outcome: Outcome,
    /// The access that was asked for.
    pub requested_access: Access,
    /// The stable code for this outcome.
    pub code: StorageCode,
    /// The severity of the code.
    pub severity: Severity,
    /// The driver to use. Present exactly when the outcome is a mount.
    pub driver: Option<&'static str>,
    /// Where the driver runs. Present exactly when the outcome is a mount.
    pub implementation: Option<ImplementationKind>,
    /// The filesystem type for the mount call, for kernel drivers.
    pub mount_type: Option<&'static str>,
    /// The exact mount options. Empty unless the outcome is a mount.
    pub options: Vec<String>,
    /// What the user is told happened.
    pub user_message: String,
    /// Why.
    pub reason: String,
    /// What the user can safely do.
    pub recovery_action: String,
    /// The technical layer.
    pub technical_detail: TechnicalDetail,
}

impl Decision {
    /// Whether the decision is to mount the volume.
    pub fn is_mount(&self) -> bool {
        matches!(self.outcome, Outcome::MountReadWrite | Outcome::MountReadOnly)
    }

    /// The access the decision grants, if it mounts.
    pub fn granted_access(&self) -> Option<Access> {
        match self.outcome {
            Outcome::MountReadWrite => Some(Access::ReadWrite),
            Outcome::MountReadOnly => Some(Access::ReadOnly),
            Outcome::Refuse | Outcome::NeedsUnlock | Outcome::Unsupported => None,
        }
    }

    /// The decision as a two-layer message in the form of specification §6.3.
    pub fn render(&self) -> String {
        let options =
            if self.options.is_empty() { "none".to_owned() } else { self.options.join(",") };
        format!(
            "{code}\n{message}\n\nReason:\n{reason}\n\nAction:\n{action}\n\nTechnical detail:\n\
             Device: {device}\nDriver: {driver}\nMount policy: {policy}\n\
             Mount options: {options}\nEvidence: {evidence}\n",
            code = self.code,
            message = self.user_message,
            reason = self.reason,
            action = self.recovery_action,
            device = self.technical_detail.device,
            driver = self.technical_detail.driver.unwrap_or("none"),
            policy = self.technical_detail.mount_policy,
            evidence = self.technical_detail.evidence,
        )
    }
}

/// Makes untrusted text safe to print: control characters, quotes and
/// backslashes are escaped.
pub(crate) fn printable(text: &str) -> String {
    text.escape_debug().to_string()
}

struct Draft {
    outcome: Outcome,
    code: StorageCode,
    reason: String,
}

fn draft(outcome: Outcome, code: StorageCode, reason: &str) -> Draft {
    Draft { outcome, code, reason: reason.to_owned() }
}

/// Decides how a volume may be mounted.
///
/// The result depends only on the arguments.
pub fn decide(volume: &Volume, request: &MountRequest, policy: &PolicyOverrides) -> Decision {
    let row = policy_for(volume.filesystem);
    let draft = classify(volume, request, policy, row);
    finish(volume, request, policy, row, draft)
}

fn classify(
    volume: &Volume,
    request: &MountRequest,
    policy: &PolicyOverrides,
    row: &FsPolicy,
) -> Draft {
    if volume.system {
        return draft(
            Outcome::Refuse,
            StorageCode::SystemVolume,
            "The volume is part of the running system's own storage.",
        );
    }
    if volume.duplicate_identity {
        return draft(
            Outcome::Refuse,
            StorageCode::DuplicateIdentity,
            "Another connected device carries the same filesystem identifier, so UOS cannot \
             tell the two apart safely.",
        );
    }
    let canonical = canonical_path(&volume.id);
    let elsewhere: Vec<String> = volume
        .mountpoints
        .iter()
        .filter(|mountpoint| **mountpoint != canonical)
        .map(|mountpoint| printable(mountpoint))
        .collect();
    if !elsewhere.is_empty() {
        return Draft {
            outcome: Outcome::Refuse,
            code: StorageCode::MountedElsewhere,
            reason: format!(
                "The volume is already mounted at {} and UOS keeps one mount per filesystem.",
                elsewhere.join(", ")
            ),
        };
    }

    match row.default_access {
        DefaultAccess::Unsupported => return unsupported(volume, row),
        DefaultAccess::Refuse => return container(volume),
        DefaultAccess::ReadOnly | DefaultAccess::ReadWrite => {}
    }

    for &condition in row.read_conditions {
        match condition {
            ReadCondition::ApfsFeatureEnabled => {
                if !policy.apfs_read_only {
                    return draft(
                        Outcome::Unsupported,
                        StorageCode::ApfsNotEnabled,
                        "APFS support is not enabled in this release.",
                    );
                }
            }
        }
    }

    if let Some((code, reason)) = write_denial(volume, policy, row) {
        let ntfs_state_unsafe = matches!(code, StorageCode::NtfsUnsafe | StorageCode::NtfsDirty);
        if ntfs_state_unsafe && policy.unsafe_ntfs == UnsafeNtfsPolicy::Refuse {
            return Draft { outcome: Outcome::Refuse, code, reason };
        }
        if request.access == Access::ReadWrite {
            return Draft { outcome: Outcome::MountReadOnly, code, reason };
        }
    }

    match request.access {
        Access::ReadWrite => Draft {
            outcome: Outcome::MountReadWrite,
            code: StorageCode::MountReadWrite,
            reason: format!(
                "Policy allows writing to {} volumes and no unsafe state was reported.",
                row.spec_name
            ),
        },
        Access::ReadOnly => draft(
            Outcome::MountReadOnly,
            StorageCode::MountReadOnlyRequested,
            "A read-only mount was requested.",
        ),
    }
}

fn unsupported(volume: &Volume, row: &FsPolicy) -> Draft {
    match volume.filesystem {
        Filesystem::Unknown => {
            let reason = match volume.signature.as_deref() {
                Some(signature) => format!(
                    "The volume has the signature \"{}\", which is not a filesystem UOS mounts.",
                    printable(signature)
                ),
                None => "No filesystem signature was found on the volume.".to_owned(),
            };
            Draft { outcome: Outcome::Unsupported, code: StorageCode::FilesystemUnknown, reason }
        }
        Filesystem::BitLocker => draft(
            Outcome::Unsupported,
            StorageCode::BitLockerUnsupported,
            "The volume is encrypted with BitLocker, which UOS cannot unlock.",
        ),
        _ => Draft {
            outcome: Outcome::Unsupported,
            code: StorageCode::FilesystemUnsupported,
            reason: format!("{} is not supported by UOS.", row.spec_name),
        },
    }
}

fn container(volume: &Volume) -> Draft {
    if volume.filesystem == Filesystem::Luks && volume.encryption == Encryption::Locked {
        return draft(
            Outcome::NeedsUnlock,
            StorageCode::EncryptedLocked,
            "The volume is encrypted and locked.",
        );
    }
    draft(
        Outcome::Refuse,
        StorageCode::ContainerNotMountable,
        "The device is a container for other volumes, not a filesystem.",
    )
}

/// Why a read/write mount is not allowed, if it is not.
fn write_denial(
    volume: &Volume,
    policy: &PolicyOverrides,
    row: &FsPolicy,
) -> Option<(StorageCode, String)> {
    for &condition in row.write_conditions {
        match condition {
            WriteCondition::NtfsClean => match volume.ntfs_state.unwrap_or(NtfsState::Unknown) {
                NtfsState::Clean => {}
                NtfsState::Dirty => {
                    return Some((
                        StorageCode::NtfsDirty,
                        "The volume is marked as not cleanly shut down and needs to be checked \
                         by Windows."
                            .to_owned(),
                    ));
                }
                NtfsState::Hibernated | NtfsState::Unknown => {
                    return Some((
                        StorageCode::NtfsUnsafe,
                        "The volume appears to be hibernated or in an unsafe state.".to_owned(),
                    ));
                }
            },
            WriteCondition::HfsPlusNotJournaled => {
                if volume.hfs_journal != Some(HfsJournal::NotJournaled) {
                    return Some((
                        StorageCode::HfsPlusJournaled,
                        "The volume is journalled, or its journal state could not be \
                         determined, and the Linux driver cannot write to a journalled volume \
                         safely."
                            .to_owned(),
                    ));
                }
            }
            WriteCondition::HfsPlusWriteOptIn => {
                if !policy.hfsplus_write {
                    return Some((
                        StorageCode::HfsPlusWriteDisabled,
                        "Writing to HFS+ volumes is switched off by policy.".to_owned(),
                    ));
                }
            }
        }
    }
    if !row.writable {
        return Some(if volume.filesystem == Filesystem::Apfs {
            (StorageCode::ApfsReadOnly, "APFS volumes are always read-only in UOS.".to_owned())
        } else {
            (
                StorageCode::FilesystemReadOnlyByDesign,
                format!("{} is a read-only format.", row.spec_name),
            )
        });
    }
    if volume.device_read_only {
        return Some((
            StorageCode::DeviceReadOnly,
            "The device or medium is write-protected.".to_owned(),
        ));
    }
    None
}

/// The sentence added to the recovery action when UOS has protected a volume
/// from a write that was asked for.
fn protection_sentence(outcome: Outcome, code: StorageCode) -> Option<&'static str> {
    let protective = matches!(
        code,
        StorageCode::NtfsUnsafe
            | StorageCode::NtfsDirty
            | StorageCode::HfsPlusJournaled
            | StorageCode::HfsPlusWriteDisabled
    );
    match outcome {
        Outcome::MountReadOnly if protective => {
            Some("UOS mounts the volume read-only to protect your data.")
        }
        Outcome::Refuse if protective => Some("UOS does not mount the volume, to protect your data."),
        _ => None,
    }
}

fn finish(
    volume: &Volume,
    request: &MountRequest,
    policy: &PolicyOverrides,
    row: &FsPolicy,
    draft: Draft,
) -> Decision {
    let entry = draft.code.entry();
    let mounts = matches!(draft.outcome, Outcome::MountReadWrite | Outcome::MountReadOnly);
    let read_only = draft.outcome == Outcome::MountReadOnly;
    let driver = if mounts { row.driver } else { None };
    let recovery_action = match protection_sentence(draft.outcome, draft.code) {
        Some(sentence) => format!("{} {sentence}", entry.recovery_action),
        None => entry.recovery_action.to_owned(),
    };
    let mount_policy = match draft.outcome {
        Outcome::MountReadWrite => "read/write",
        Outcome::MountReadOnly => "read-only",
        Outcome::Refuse | Outcome::NeedsUnlock | Outcome::Unsupported => "not mounted",
    };
    Decision {
        outcome: draft.outcome,
        requested_access: request.access,
        code: draft.code,
        severity: entry.severity,
        driver,
        implementation: mounts.then_some(row.implementation),
        mount_type: if mounts { row.mount_type } else { None },
        options: if mounts { mount_options(volume, read_only, request, policy) } else { Vec::new() },
        user_message: entry.user_message.to_owned(),
        reason: draft.reason,
        recovery_action,
        technical_detail: TechnicalDetail {
            device: device_reference(volume),
            driver,
            mount_policy,
            evidence: evidence(volume),
            explanation: entry.technical_explanation,
        },
    }
}

/// The exact options for a mount. Every volume this function sees is outside
/// the running system's own storage, so `nosuid` and `nodev` always apply.
fn mount_options(
    volume: &Volume,
    read_only: bool,
    request: &MountRequest,
    policy: &PolicyOverrides,
) -> Vec<String> {
    let mut options: Vec<String> = Vec::new();
    let mut add = |option: &str| options.push(option.to_owned());
    add(if read_only { "ro" } else { "rw" });
    add("nosuid");
    add("nodev");
    if volume.removable && policy.noexec_removable {
        add("noexec");
    }
    let uid = format!("uid={}", request.uid);
    let gid = format!("gid={}", request.gid);
    match volume.filesystem {
        Filesystem::Ntfs => {
            add(&uid);
            add(&gid);
            add("dmask=0077");
            add("fmask=0177");
            add("iocharset=utf8");
            if !read_only {
                add("windows_names");
            }
        }
        Filesystem::Exfat => {
            add(&uid);
            add(&gid);
            add("dmask=0077");
            add("fmask=0177");
            add("iocharset=utf8");
            add("errors=remount-ro");
        }
        Filesystem::Vfat => {
            add(&uid);
            add(&gid);
            add("dmask=0077");
            add("fmask=0177");
            add("shortname=mixed");
            add("utf8=1");
            add("errors=remount-ro");
            if !read_only && volume.removable {
                add("flush");
            }
        }
        Filesystem::HfsPlus => {
            add(&uid);
            add(&gid);
        }
        _ => {}
    }
    options
}

fn device_reference(volume: &Volume) -> String {
    match volume.uuid.as_deref() {
        Some(uuid) if normalise_identifier(uuid).is_some() => {
            format!("/dev/disk/by-uuid/{}", uuid.trim())
        }
        _ => volume.devices.first().map(|device| printable(device)).unwrap_or_default(),
    }
}

fn evidence(volume: &Volume) -> String {
    let mut facts: Vec<String> = vec![format!("filesystem={}", volume.filesystem)];
    if let Some(state) = volume.ntfs_state {
        facts.push(format!("ntfs_state={state}"));
    }
    if let Some(journal) = volume.hfs_journal {
        facts.push(format!("hfs_journal={journal}"));
    }
    facts.push(format!("encryption={}", volume.encryption));
    facts.push(format!("removable={}", volume.removable));
    facts.push(format!("device_read_only={}", volume.device_read_only));
    facts.push(format!("system={}", volume.system));
    facts.push(format!("duplicate_identity={}", volume.duplicate_identity));
    if !volume.mountpoints.is_empty() {
        let mounted: Vec<String> = volume.mountpoints.iter().map(|m| printable(m)).collect();
        facts.push(format!("mounted_at={}", mounted.join(",")));
    }
    format!("{} (facts from the probe; no journal reference at this layer)", facts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::{CodeOrigin, FORBIDDEN_WORDING, registry};
    use crate::policy::EXCLUDED_IMPLEMENTATIONS;
    use crate::volume::testing;
    use std::collections::BTreeSet;

    const RW: MountRequest = MountRequest { access: Access::ReadWrite, uid: 1000, gid: 1000 };
    const RO: MountRequest = MountRequest { access: Access::ReadOnly, uid: 1000, gid: 1000 };

    struct Case {
        volume: Volume,
        request: MountRequest,
        policy: PolicyOverrides,
        decision: Decision,
    }

    /// Every combination of the facts the decision reads.
    fn all_volumes() -> Vec<Volume> {
        let mut volumes = Vec::new();
        for &filesystem in Filesystem::all() {
            let ntfs_states: Vec<Option<NtfsState>> = if filesystem == Filesystem::Ntfs {
                NtfsState::all().iter().copied().map(Some).collect()
            } else {
                vec![None]
            };
            let journals: Vec<Option<HfsJournal>> = if filesystem == Filesystem::HfsPlus {
                HfsJournal::all().iter().copied().map(Some).collect()
            } else {
                vec![None]
            };
            let encryptions: Vec<Encryption> = if filesystem == Filesystem::Luks {
                vec![Encryption::Locked, Encryption::Unlocked]
            } else {
                vec![Encryption::None, Encryption::Mapping]
            };
            for &ntfs_state in &ntfs_states {
                for &hfs_journal in &journals {
                    for &encryption in &encryptions {
                        for flags in 0_u8..16 {
                            for mounted in 0_u8..3 {
                                let mut volume = testing::volume(filesystem);
                                volume.ntfs_state = ntfs_state;
                                volume.hfs_journal = hfs_journal;
                                volume.encryption = encryption;
                                volume.removable = flags & 1 != 0;
                                volume.device_read_only = flags & 2 != 0;
                                volume.system = flags & 4 != 0;
                                volume.duplicate_identity = flags & 8 != 0;
                                volume.mountpoints = match mounted {
                                    0 => Vec::new(),
                                    1 => vec!["/mnt/elsewhere".to_owned()],
                                    _ => vec![canonical_path(&volume.id)],
                                };
                                volumes.push(volume);
                            }
                        }
                    }
                }
            }
        }
        volumes
    }

    /// Every decision the function can return: all facts, both requests and
    /// the whole override space.
    fn all_cases() -> Vec<Case> {
        let mut cases = Vec::new();
        for volume in all_volumes() {
            for request in [RO, RW] {
                for policy in PolicyOverrides::all() {
                    let decision = decide(&volume, &request, &policy);
                    cases.push(Case { volume: volume.clone(), request, policy, decision });
                }
            }
        }
        cases
    }

    fn numeric_option(option: &str, prefix: &str) -> bool {
        option
            .strip_prefix(prefix)
            .is_some_and(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
    }

    fn option_is_allowed(option: &str) -> bool {
        const FIXED: &[&str] = &[
            "ro",
            "rw",
            "nosuid",
            "nodev",
            "noexec",
            "dmask=0077",
            "fmask=0177",
            "iocharset=utf8",
            "windows_names",
            "errors=remount-ro",
            "shortname=mixed",
            "utf8=1",
            "flush",
        ];
        FIXED.contains(&option) || numeric_option(option, "uid=") || numeric_option(option, "gid=")
    }

    fn texts(decision: &Decision) -> Vec<String> {
        vec![
            decision.user_message.clone(),
            decision.reason.clone(),
            decision.recovery_action.clone(),
            decision.technical_detail.device.clone(),
            decision.technical_detail.mount_policy.to_owned(),
            decision.technical_detail.evidence.clone(),
            decision.technical_detail.explanation.to_owned(),
            decision.render(),
        ]
    }

    fn has(decision: &Decision, option: &str) -> bool {
        decision.options.iter().any(|candidate| candidate == option)
    }

    #[test]
    fn the_enumeration_is_large_and_covers_every_outcome() {
        let cases = all_cases();
        assert!(cases.len() > 50_000, "{}", cases.len());
        let outcomes: BTreeSet<Outcome> = cases.iter().map(|case| case.decision.outcome).collect();
        assert_eq!(outcomes.len(), Outcome::all().len());
    }

    #[test]
    fn no_decision_produces_or_recommends_an_unsafe_mount() {
        for case in all_cases() {
            let decision = &case.decision;
            for option in &decision.options {
                assert!(option_is_allowed(option), "unexpected option {option:?}");
                for word in FORBIDDEN_WORDING {
                    assert!(!option.to_lowercase().contains(word), "{option:?}");
                }
            }
            for text in texts(decision) {
                let lowered = text.to_lowercase();
                for word in FORBIDDEN_WORDING {
                    assert!(!lowered.contains(word), "{word:?} in {text:?}");
                }
            }
            for excluded in EXCLUDED_IMPLEMENTATIONS {
                assert_ne!(decision.driver, Some(excluded.name));
                assert_ne!(decision.mount_type, Some(excluded.name));
            }
        }
    }

    #[test]
    fn a_request_is_never_exceeded() {
        for case in all_cases() {
            if case.request.access == Access::ReadOnly {
                assert_ne!(case.decision.outcome, Outcome::MountReadWrite);
                assert!(!has(&case.decision, "rw"));
            }
            assert_eq!(case.decision.requested_access, case.request.access);
        }
    }

    #[test]
    fn writing_is_granted_only_when_every_rule_allows_it() {
        for case in all_cases() {
            let volume = &case.volume;
            let row = policy_for(volume.filesystem);
            let conditions_hold = row.write_conditions.iter().all(|condition| match condition {
                WriteCondition::NtfsClean => volume.ntfs_state == Some(NtfsState::Clean),
                WriteCondition::HfsPlusNotJournaled => {
                    volume.hfs_journal == Some(HfsJournal::NotJournaled)
                }
                WriteCondition::HfsPlusWriteOptIn => case.policy.hfsplus_write,
            });
            let mounted_elsewhere =
                volume.mountpoints.iter().any(|m| *m != canonical_path(&volume.id));
            let expected = case.request.access == Access::ReadWrite
                && row.writable
                && conditions_hold
                && !volume.device_read_only
                && !volume.system
                && !volume.duplicate_identity
                && !mounted_elsewhere;
            assert_eq!(
                case.decision.outcome == Outcome::MountReadWrite,
                expected,
                "{} {:?} {:?}",
                volume.filesystem,
                case.policy,
                case.decision.code
            );
        }
    }

    #[test]
    fn only_a_clean_ntfs_volume_is_writable() {
        let mut seen_read_only = false;
        let mut seen_refused = false;
        for case in all_cases() {
            if case.volume.filesystem != Filesystem::Ntfs {
                continue;
            }
            let state = case.volume.ntfs_state.unwrap();
            if state != NtfsState::Clean {
                assert!(
                    matches!(case.decision.outcome, Outcome::MountReadOnly | Outcome::Refuse),
                    "{state}: {:?}",
                    case.decision.outcome
                );
                assert!(!has(&case.decision, "rw"));
                seen_read_only |= case.decision.outcome == Outcome::MountReadOnly;
                seen_refused |= case.decision.outcome == Outcome::Refuse;
            }
            if case.decision.outcome == Outcome::MountReadWrite {
                assert_eq!(state, NtfsState::Clean);
                assert_eq!(case.decision.driver, Some("ntfs3"));
            }
        }
        assert!(seen_read_only && seen_refused);
    }

    #[test]
    fn missing_ntfs_state_is_treated_as_unknown() {
        let mut volume = testing::volume(Filesystem::Ntfs);
        volume.ntfs_state = None;
        let decision = decide(&volume, &RW, &PolicyOverrides::default());
        assert_eq!(decision.outcome, Outcome::MountReadOnly);
        assert_eq!(decision.code, StorageCode::NtfsUnsafe);
    }

    #[test]
    fn apfs_is_never_writable_over_the_whole_option_space() {
        let mut mounted = 0;
        for case in all_cases() {
            if case.volume.filesystem != Filesystem::Apfs {
                continue;
            }
            assert_ne!(case.decision.outcome, Outcome::MountReadWrite);
            assert!(!has(&case.decision, "rw"));
            assert_ne!(case.decision.driver, Some("linux-apfs-rw"));
            if case.decision.is_mount() {
                mounted += 1;
                assert!(case.policy.apfs_read_only);
                assert_eq!(case.decision.driver, Some("apfs-fuse"));
                assert_eq!(case.decision.implementation, Some(ImplementationKind::Userspace));
                assert_eq!(case.decision.options[0], "ro");
            }
            if !case.policy.apfs_read_only {
                assert!(!case.decision.is_mount());
            }
        }
        assert!(mounted > 0);
    }

    #[test]
    fn system_storage_is_never_offered() {
        for case in all_cases() {
            if case.volume.system {
                assert_eq!(case.decision.outcome, Outcome::Refuse);
                assert_eq!(case.decision.code, StorageCode::SystemVolume);
            }
        }
    }

    #[test]
    fn duplicates_and_foreign_mounts_are_refused() {
        for case in all_cases() {
            if case.volume.system {
                continue;
            }
            let elsewhere = case.volume.mountpoints.iter().any(|m| m == "/mnt/elsewhere");
            if case.volume.duplicate_identity {
                assert_eq!(case.decision.code, StorageCode::DuplicateIdentity);
                assert_eq!(case.decision.outcome, Outcome::Refuse);
            } else if elsewhere {
                assert_eq!(case.decision.code, StorageCode::MountedElsewhere);
                assert_eq!(case.decision.outcome, Outcome::Refuse);
                assert!(case.decision.reason.contains("/mnt/elsewhere"));
            } else {
                assert_ne!(case.decision.code, StorageCode::MountedElsewhere);
            }
        }
    }

    #[test]
    fn every_mount_has_safe_defaults_and_a_driver() {
        for case in all_cases() {
            let decision = &case.decision;
            if decision.is_mount() {
                let first = decision.options[0].as_str();
                let expected = if decision.outcome == Outcome::MountReadWrite { "rw" } else { "ro" };
                assert_eq!(first, expected);
                assert_eq!(has(decision, "ro"), decision.outcome == Outcome::MountReadOnly);
                assert_eq!(has(decision, "rw"), decision.outcome == Outcome::MountReadWrite);
                assert!(has(decision, "nosuid") && has(decision, "nodev"));
                assert!(decision.driver.is_some() && decision.implementation.is_some());
                assert_eq!(decision.technical_detail.driver, decision.driver);
                let noexec = case.volume.removable && case.policy.noexec_removable;
                assert_eq!(has(decision, "noexec"), noexec);
                let unique: BTreeSet<&String> = decision.options.iter().collect();
                assert_eq!(unique.len(), decision.options.len());
            } else {
                assert!(decision.options.is_empty());
                assert!(decision.driver.is_none());
                assert!(decision.implementation.is_none());
                assert!(decision.mount_type.is_none());
                assert_eq!(decision.technical_detail.mount_policy, "not mounted");
                assert!(decision.granted_access().is_none());
            }
        }
    }

    #[test]
    fn every_decision_code_is_registered_and_every_registered_code_is_exercised() {
        let produced: BTreeSet<StorageCode> =
            all_cases().iter().map(|case| case.decision.code).collect();
        let registered: BTreeSet<StorageCode> = registry()
            .into_iter()
            .filter(|entry| entry.origin == CodeOrigin::Decision)
            .map(|entry| entry.code)
            .collect();
        assert_eq!(produced, registered);
        for case in all_cases() {
            let entry = case.decision.code.entry();
            assert_eq!(case.decision.severity, entry.severity);
            assert_eq!(case.decision.user_message, entry.user_message);
            assert!(case.decision.recovery_action.starts_with(entry.recovery_action));
            assert!(!case.decision.reason.is_empty());
        }
    }

    #[test]
    fn the_decision_is_deterministic() {
        for case in all_cases().into_iter().step_by(97) {
            assert_eq!(decide(&case.volume, &case.request, &case.policy), case.decision);
        }
    }

    #[test]
    fn hibernated_ntfs_follows_the_specification_example() {
        let mut volume = testing::volume(Filesystem::Ntfs);
        volume.ntfs_state = Some(NtfsState::Hibernated);
        volume.uuid = Some("A1B2C3D4E5F60718".to_owned());
        let decision = decide(&volume, &RW, &PolicyOverrides::default());
        assert_eq!(decision.outcome, Outcome::MountReadOnly);
        assert_eq!(decision.code.as_str(), "UOS-STORAGE-NTFS-004");
        assert_eq!(decision.user_message, "The Windows volume was not mounted for writing.");
        assert_eq!(decision.reason, "The volume appears to be hibernated or in an unsafe state.");
        assert_eq!(
            decision.recovery_action,
            "Boot Windows, disable Fast Startup/hibernation, shut down fully, then retry. UOS \
             mounts the volume read-only to protect your data."
        );
        let rendered = decision.render();
        let expected_start = "UOS-STORAGE-NTFS-004\nThe Windows volume was not mounted for \
                              writing.\n\nReason:\nThe volume appears to be hibernated or in an \
                              unsafe state.\n\nAction:\nBoot Windows, disable Fast \
                              Startup/hibernation, shut down fully, then retry.";
        assert!(rendered.starts_with(expected_start), "{rendered}");
        assert!(rendered.contains("\nTechnical detail:\nDevice: /dev/disk/by-uuid/A1B2C3D4E5F60718\n"));
        assert!(rendered.contains("\nDriver: ntfs3\nMount policy: read-only\n"));
        assert!(rendered.contains("ntfs_state=hibernated"));
    }

    #[test]
    fn unsafe_ntfs_can_be_refused_but_never_written() {
        let refuse =
            PolicyOverrides { unsafe_ntfs: UnsafeNtfsPolicy::Refuse, ..PolicyOverrides::default() };
        for (state, code) in [
            (NtfsState::Hibernated, StorageCode::NtfsUnsafe),
            (NtfsState::Unknown, StorageCode::NtfsUnsafe),
            (NtfsState::Dirty, StorageCode::NtfsDirty),
        ] {
            let mut volume = testing::volume(Filesystem::Ntfs);
            volume.ntfs_state = Some(state);
            for request in [RO, RW] {
                let decision = decide(&volume, &request, &refuse);
                assert_eq!(decision.outcome, Outcome::Refuse, "{state}");
                assert_eq!(decision.code, code);
                assert!(decision.recovery_action.ends_with("to protect your data."));
            }
            let downgraded = decide(&volume, &RW, &PolicyOverrides::default());
            assert_eq!(downgraded.outcome, Outcome::MountReadOnly);
            assert_eq!(downgraded.code, code);
            let requested = decide(&volume, &RO, &PolicyOverrides::default());
            assert_eq!(requested.outcome, Outcome::MountReadOnly);
            assert_eq!(requested.code, StorageCode::MountReadOnlyRequested);
        }
    }

    #[test]
    fn dirty_ntfs_tells_the_user_to_let_windows_check_the_disk() {
        let mut volume = testing::volume(Filesystem::Ntfs);
        volume.ntfs_state = Some(NtfsState::Dirty);
        let decision = decide(&volume, &RW, &PolicyOverrides::default());
        assert_eq!(decision.code.as_str(), "UOS-STORAGE-NTFS-005");
        assert!(decision.recovery_action.starts_with("Boot Windows, let it check the disk"));
    }

    #[test]
    fn clean_ntfs_mounts_read_write_with_exact_options() {
        let volume = testing::volume(Filesystem::Ntfs);
        let request = MountRequest { access: Access::ReadWrite, uid: 1001, gid: 1002 };
        let decision = decide(&volume, &request, &PolicyOverrides::default());
        assert_eq!(decision.outcome, Outcome::MountReadWrite);
        assert_eq!(decision.code, StorageCode::MountReadWrite);
        assert_eq!(decision.mount_type, Some("ntfs3"));
        assert_eq!(
            decision.options,
            [
                "rw",
                "nosuid",
                "nodev",
                "uid=1001",
                "gid=1002",
                "dmask=0077",
                "fmask=0177",
                "iocharset=utf8",
                "windows_names"
            ]
        );
    }

    #[test]
    fn removable_fat_and_exfat_get_ownership_and_error_handling_options() {
        let mut stick = testing::volume(Filesystem::Vfat);
        stick.removable = true;
        let decision = decide(&stick, &RW, &PolicyOverrides::default());
        assert_eq!(
            decision.options,
            [
                "rw",
                "nosuid",
                "nodev",
                "uid=1000",
                "gid=1000",
                "dmask=0077",
                "fmask=0177",
                "shortname=mixed",
                "utf8=1",
                "errors=remount-ro",
                "flush"
            ]
        );
        let strict =
            PolicyOverrides { noexec_removable: true, ..PolicyOverrides::default() };
        let mut card = testing::volume(Filesystem::Exfat);
        card.removable = true;
        let decision = decide(&card, &RO, &strict);
        assert_eq!(
            decision.options,
            [
                "ro",
                "nosuid",
                "nodev",
                "noexec",
                "uid=1000",
                "gid=1000",
                "dmask=0077",
                "fmask=0177",
                "iocharset=utf8",
                "errors=remount-ro"
            ]
        );
        // The override applies to removable media only.
        let internal = decide(&testing::volume(Filesystem::Exfat), &RW, &strict);
        assert!(!has(&internal, "noexec"));
    }

    #[test]
    fn linux_filesystems_mount_read_write_with_generic_options_only() {
        for filesystem in [Filesystem::Ext4, Filesystem::Btrfs, Filesystem::Xfs] {
            let decision = decide(&testing::volume(filesystem), &RW, &PolicyOverrides::default());
            assert_eq!(decision.outcome, Outcome::MountReadWrite, "{filesystem}");
            assert_eq!(decision.options, ["rw", "nosuid", "nodev"]);
            assert_eq!(decision.implementation, Some(ImplementationKind::Kernel));
        }
    }

    #[test]
    fn hfsplus_is_read_only_unless_opted_in_and_not_journalled() {
        let allow = PolicyOverrides { hfsplus_write: true, ..PolicyOverrides::default() };
        let cases = [
            (HfsJournal::Journaled, false, StorageCode::HfsPlusJournaled),
            (HfsJournal::Journaled, true, StorageCode::HfsPlusJournaled),
            (HfsJournal::Unknown, true, StorageCode::HfsPlusJournaled),
            (HfsJournal::NotJournaled, false, StorageCode::HfsPlusWriteDisabled),
        ];
        for (journal, opted_in, code) in cases {
            let mut volume = testing::volume(Filesystem::HfsPlus);
            volume.hfs_journal = Some(journal);
            let policy = if opted_in { allow } else { PolicyOverrides::default() };
            let decision = decide(&volume, &RW, &policy);
            assert_eq!(decision.outcome, Outcome::MountReadOnly, "{journal}");
            assert_eq!(decision.code, code, "{journal}");
            assert_eq!(decision.options, ["ro", "nosuid", "nodev", "uid=1000", "gid=1000"]);
        }
        let volume = testing::volume(Filesystem::HfsPlus);
        let decision = decide(&volume, &RW, &allow);
        assert_eq!(decision.outcome, Outcome::MountReadWrite);
        assert_eq!(decision.driver, Some("hfsplus"));
    }

    #[test]
    fn apfs_is_unsupported_by_default_and_read_only_when_enabled() {
        let volume = testing::volume(Filesystem::Apfs);
        let default = decide(&volume, &RW, &PolicyOverrides::default());
        assert_eq!(default.outcome, Outcome::Unsupported);
        assert_eq!(default.code, StorageCode::ApfsNotEnabled);
        let enabled = PolicyOverrides { apfs_read_only: true, ..PolicyOverrides::default() };
        let decision = decide(&volume, &RW, &enabled);
        assert_eq!(decision.outcome, Outcome::MountReadOnly);
        assert_eq!(decision.code, StorageCode::ApfsReadOnly);
        assert_eq!(decision.options, ["ro", "nosuid", "nodev"]);
        assert_eq!(decision.mount_type, None);
    }

    #[test]
    fn unsupported_and_unknown_filesystems_are_never_mounted() {
        let cases = [
            (Filesystem::Refs, StorageCode::FilesystemUnsupported),
            (Filesystem::Zfs, StorageCode::FilesystemUnsupported),
            (Filesystem::BitLocker, StorageCode::BitLockerUnsupported),
            (Filesystem::Unknown, StorageCode::FilesystemUnknown),
        ];
        for (filesystem, code) in cases {
            for request in [RO, RW] {
                let decision =
                    decide(&testing::volume(filesystem), &request, &PolicyOverrides::default());
                assert_eq!(decision.outcome, Outcome::Unsupported, "{filesystem}");
                assert_eq!(decision.code, code, "{filesystem}");
            }
        }
        let mut odd = testing::volume(Filesystem::Unknown);
        odd.signature = Some("bcache\u{7}fs".to_owned());
        let decision = decide(&odd, &RW, &PolicyOverrides::default());
        assert!(decision.reason.contains("bcache\\u{7}fs"), "{}", decision.reason);
        odd.signature = None;
        let decision = decide(&odd, &RW, &PolicyOverrides::default());
        assert_eq!(decision.reason, "No filesystem signature was found on the volume.");
    }

    #[test]
    fn containers_need_unlocking_or_are_refused() {
        let locked = testing::volume(Filesystem::Luks);
        let decision = decide(&locked, &RW, &PolicyOverrides::default());
        assert_eq!(decision.outcome, Outcome::NeedsUnlock);
        assert_eq!(decision.code, StorageCode::EncryptedLocked);

        let mut unlocked = testing::volume(Filesystem::Luks);
        unlocked.encryption = Encryption::Unlocked;
        let decision = decide(&unlocked, &RW, &PolicyOverrides::default());
        assert_eq!(decision.outcome, Outcome::Refuse);
        assert_eq!(decision.code, StorageCode::ContainerNotMountable);

        for filesystem in [Filesystem::LvmMember, Filesystem::RaidMember] {
            let decision = decide(&testing::volume(filesystem), &RO, &PolicyOverrides::default());
            assert_eq!(decision.outcome, Outcome::Refuse);
            assert_eq!(decision.code, StorageCode::ContainerNotMountable);
        }
    }

    #[test]
    fn read_only_devices_and_formats_downgrade_a_write_request() {
        let mut protected = testing::volume(Filesystem::Ext4);
        protected.device_read_only = true;
        let decision = decide(&protected, &RW, &PolicyOverrides::default());
        assert_eq!(decision.outcome, Outcome::MountReadOnly);
        assert_eq!(decision.code, StorageCode::DeviceReadOnly);

        let mut disc = testing::volume(Filesystem::Iso9660);
        disc.device_read_only = true;
        disc.optical = true;
        let decision = decide(&disc, &RW, &PolicyOverrides::default());
        assert_eq!(decision.outcome, Outcome::MountReadOnly);
        assert_eq!(decision.code, StorageCode::FilesystemReadOnlyByDesign);
        assert_eq!(decision.options, ["ro", "nosuid", "nodev"]);
        assert_eq!(decision.reason, "ISO 9660 is a read-only format.");
    }

    #[test]
    fn a_volume_mounted_at_its_canonical_path_is_decided_normally() {
        let mut volume = testing::volume(Filesystem::Ext4);
        volume.mountpoints = vec![canonical_path(&volume.id)];
        let decision = decide(&volume, &RW, &PolicyOverrides::default());
        assert_eq!(decision.outcome, Outcome::MountReadWrite);
    }

    #[test]
    fn technical_detail_falls_back_to_the_device_path() {
        let mut volume = testing::volume(Filesystem::Unknown);
        volume.uuid = None;
        volume.devices = vec!["/dev/sd\u{1b}[31mx".to_owned()];
        let decision = decide(&volume, &RW, &PolicyOverrides::default());
        assert_eq!(decision.technical_detail.device, "/dev/sd\\u{1b}[31mx");
        assert!(!decision.render().contains('\u{1b}'));
    }
}
