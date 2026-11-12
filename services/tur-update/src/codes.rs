// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `TUR-UPDATE-*` error-code registry (specification §29.3).
//!
//! Every code has one registry entry with a title, severity, detection
//! criteria, user message, technical explanation and recovery action.
//! [`UpdateCode::entry`] is an exhaustive `match`, so a code without an entry
//! does not compile. The meaning of a released code never changes.
//!
//! The user messages follow the tone rules and, where it gives one, the
//! wording of the desktop design (desktop-experience.md, sections 5.6 and
//! 11.1): what happened first, then what to do. A message for something that
//! stopped an update begins "The update was not applied."
//!
//! No text in this registry tells a user to bypass a check; a test scans the
//! registry for such wording.

use serde::Serialize;
use tur_core::{ErrorCode, Family};

string_enum! {
    /// How serious the condition behind a code is.
    pub enum Severity {
        /// Nothing is wrong; the code records a normal outcome.
        Info => "info",
        /// The user did not get what was asked for, by design, and nothing is wrong.
        Notice => "notice",
        /// Something needs the user's attention.
        Warning => "warning",
        /// The operation could not be carried out.
        Error => "error",
    }
}

string_enum! {
    /// A stable update error code.
    pub enum UpdateCode {
        /// The state of the system could not be read.
        StateUnreadable => "TUR-UPDATE-STATE-001",
        /// bootc reports no booted deployment.
        NotBootcHost => "TUR-UPDATE-STATE-002",
        /// `bootc status` printed a format this version does not know.
        FormatUnknown => "TUR-UPDATE-STATE-003",
        /// Reading the state of this machine needs root.
        NeedsAdministrator => "TUR-UPDATE-STATE-004",
        /// The booted deployment is not one bootc can update.
        BootedNotManaged => "TUR-UPDATE-STATE-005",
        /// The system runs from a read-only medium.
        ReadOnlyMedium => "TUR-UPDATE-STATE-006",
        /// The boot-health state given to the program could not be read.
        BootHealthUnreadable => "TUR-UPDATE-STATE-007",
        /// A check for updates may run.
        CheckAllowed => "TUR-UPDATE-CHECK-001",
        /// There is no network connection.
        Offline => "TUR-UPDATE-NETWORK-001",
        /// The connection is metered and downloads on metered connections are off.
        Metered => "TUR-UPDATE-NETWORK-002",
        /// An automatic download waits for mains power.
        OnBattery => "TUR-UPDATE-POWER-001",
        /// The battery is too low even for a download a person asked for.
        BatteryLow => "TUR-UPDATE-POWER-002",
        /// There is not enough free space to stage the update.
        NotEnoughSpace => "TUR-UPDATE-SPACE-001",
        /// The available image failed its start-up trial on this machine.
        ImageRejected => "TUR-UPDATE-HEALTH-001",
        /// A person asked for an image that failed its trial to be tried again.
        RetryRejected => "TUR-UPDATE-HEALTH-002",
        /// Boot attempts cannot be counted on this installation.
        CountingUnusable => "TUR-UPDATE-HEALTH-003",
        /// The boot-health state is not known.
        BootHealthUnknown => "TUR-UPDATE-HEALTH-004",
        /// The running version has not been marked good yet.
        TrialInProgress => "TUR-UPDATE-HEALTH-005",
        /// The running version failed its start-up checks outside a trial.
        BootUnhealthy => "TUR-UPDATE-HEALTH-006",
        /// Automatic downloads are switched off.
        AutomaticDownloadOff => "TUR-UPDATE-SETTINGS-001",
        /// A repair session is open.
        RepairSession => "TUR-UPDATE-SESSION-001",
        /// The update may be downloaded and staged.
        StageAllowed => "TUR-UPDATE-STAGE-001",
        /// The last check found nothing newer.
        UpToDate => "TUR-UPDATE-STAGE-002",
        /// No check has been made, so no update is known.
        NotChecked => "TUR-UPDATE-STAGE-003",
        /// The update is staged and will be used at the next restart.
        AlreadyStaged => "TUR-UPDATE-STAGE-004",
        /// The update is staged but held; it may be released.
        StagedHeld => "TUR-UPDATE-STAGE-005",
        /// A newer update than the staged one may be staged in its place.
        StagedSuperseded => "TUR-UPDATE-STAGE-006",
        /// Going back to the previous version may proceed.
        RollbackAllowed => "TUR-UPDATE-ROLLBACK-001",
        /// There is no previous version.
        NoPreviousVersion => "TUR-UPDATE-ROLLBACK-002",
        /// The previous version is one that failed its start-up trial.
        PreviousVersionRejected => "TUR-UPDATE-ROLLBACK-003",
        /// The previous version is not one bootc can make the default.
        PreviousNotManaged => "TUR-UPDATE-ROLLBACK-004",
        /// Going back is already queued for the next restart.
        RollbackQueued => "TUR-UPDATE-ROLLBACK-005",
        /// The version may be kept.
        PinAllowed => "TUR-UPDATE-PIN-001",
        /// The version is already kept.
        AlreadyKept => "TUR-UPDATE-PIN-002",
        /// The version named is not on this computer.
        NoSuchVersion => "TUR-UPDATE-PIN-003",
        /// The version may be released.
        UnpinAllowed => "TUR-UPDATE-PIN-004",
        /// The version is not kept, so there is nothing to release.
        NotKept => "TUR-UPDATE-PIN-005",
    }
}

/// A registry entry: everything specification §29.3 requires for a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CodeEntry {
    /// The code.
    pub code: UpdateCode,
    /// A short title.
    pub title: &'static str,
    /// How serious the condition is.
    pub severity: Severity,
    /// The facts that cause this code to be reported.
    pub detection: &'static str,
    /// What the user is told happened.
    pub user_message: &'static str,
    /// Why, in technical terms.
    pub technical_explanation: &'static str,
    /// What the user can safely do about it.
    pub recovery_action: &'static str,
}

impl UpdateCode {
    /// The registry entry for this code.
    pub const fn entry(self) -> CodeEntry {
        match self {
            UpdateCode::StateUnreadable => CodeEntry {
                code: self,
                title: "System state could not be read",
                severity: Severity::Error,
                detection: "bootc status could not be run, did not end with status 0, or printed \
                            something that is not the document it is asked for; or the file \
                            given with --from could not be read or parsed.",
                user_message: "TUR could not read which versions are on this computer.",
                technical_explanation: "Every decision of the update manager starts from bootc \
                                        status --format=json. Without it nothing is known about \
                                        the deployments, so nothing is decided and nothing is \
                                        planned.",
                recovery_action: "Try again. If it keeps happening, create a support bundle.",
            },
            UpdateCode::NotBootcHost => CodeEntry {
                code: self,
                title: "Not an image-based installation",
                severity: Severity::Error,
                detection: "bootc status reports no booted deployment: status.booted is null. \
                            bootc reports this in a container and on a system that was not \
                            installed from an image.",
                user_message: "This system does not receive TUR updates.",
                technical_explanation: "TUR updates replace the booted deployment of a bootc \
                                        system. Where bootc finds no booted deployment there is \
                                        nothing to update and nothing to go back to.",
                recovery_action: "No action is needed. Updates apply to an installed TUR system.",
            },
            UpdateCode::FormatUnknown => CodeEntry {
                code: self,
                title: "Unknown bootc status format",
                severity: Severity::Error,
                detection: "The apiVersion of the document is neither org.containers.bootc/v1 \
                            nor org.containers.bootc/v1alpha1, or its kind is not BootcHost.",
                user_message: "TUR could not read which versions are on this computer.",
                technical_explanation: "The document is read as bootc 1.17.1 defines it. A \
                                        different format version may give the same members a \
                                        different meaning, so it is not interpreted at all.",
                recovery_action: "Create a support bundle. Nothing was changed.",
            },
            UpdateCode::NeedsAdministrator => CodeEntry {
                code: self,
                title: "Reading the system state needs administrator rights",
                severity: Severity::Notice,
                detection: "The state of this machine was asked for, bootc status failed, and \
                            the program does not run as root.",
                user_message: "Reading the versions on this computer needs administrator \
                               rights.",
                technical_explanation: "On an installed system bootc status must run as root: \
                                        it enters a private mount namespace to read the \
                                        deployments. A program in the user's session, or in the \
                                        assistant's read-only sandbox, cannot do that. The \
                                        update service is what will publish the state to them.",
                recovery_action: "Open Updates in Settings, or run the command as administrator.",
            },
            UpdateCode::BootedNotManaged => CodeEntry {
                code: self,
                title: "The running system is not a plain image",
                severity: Severity::Error,
                detection: "The booted deployment has no container image (status.booted.image \
                            is null), or is marked incompatible because packages were layered \
                            onto it.",
                user_message: "This installation was changed in a way that TUR updates cannot \
                               manage.",
                technical_explanation: "bootc only updates a deployment that is exactly a \
                                        container image. A deployment with local package \
                                        changes has to be changed with the tool that made them.",
                recovery_action: "Undo the local package changes to receive updates again. If \
                                  you did not make any, create a support bundle.",
            },
            UpdateCode::ReadOnlyMedium => CodeEntry {
                code: self,
                title: "The system runs from read-only media",
                severity: Severity::Notice,
                detection: "status.readOnly is true: the physical root is on a medium that \
                            cannot be written, such as live media.",
                user_message: "This system runs from read-only media and cannot be updated.",
                technical_explanation: "bootc reports that commands which change the system are \
                                        not available here.",
                recovery_action: "Install TUR to a disk to receive updates.",
            },
            UpdateCode::BootHealthUnreadable => CodeEntry {
                code: self,
                title: "Boot-health state could not be read",
                severity: Severity::Error,
                detection: "The file given with --boot-health could not be read, is not JSON, \
                            or is not the document tur-boot-health status --json prints.",
                user_message: "TUR could not read the record of earlier start-up checks.",
                technical_explanation: "The list of versions that failed to start, and whether \
                                        a failed update can be undone, come from tur-boot-health. \
                                        A document that cannot be read is not guessed at.",
                recovery_action: "Try again. If it keeps happening, create a support bundle.",
            },
            UpdateCode::CheckAllowed => CodeEntry {
                code: self,
                title: "A check for updates may run",
                severity: Severity::Info,
                detection: "The running system is one bootc can update and the network is not \
                            known to be offline.",
                user_message: "TUR can check for updates.",
                technical_explanation: "A check fetches the manifest and configuration of the \
                                        image the system follows, a few kilobytes, and stores \
                                        them as the cached update. It changes no deployment.",
                recovery_action: "No action is needed.",
            },
            UpdateCode::Offline => CodeEntry {
                code: self,
                title: "No network connection",
                severity: Severity::Notice,
                detection: "The network state given to the decision is offline.",
                user_message: "TUR cannot check for updates without a network connection. \
                               Nothing is wrong.",
                technical_explanation: "Checking and downloading both need the image registry. \
                                        Nothing is fetched and nothing is changed while the \
                                        computer is offline.",
                recovery_action: "Connect to a network. TUR checks again by itself.",
            },
            UpdateCode::Metered => CodeEntry {
                code: self,
                title: "Waiting for a connection that is not metered",
                severity: Severity::Notice,
                detection: "An automatic download would start, the connection is reported as \
                            metered, and the setting Download on metered connections is off.",
                user_message: "The update is waiting for a connection that is not metered.",
                technical_explanation: "A download can be several gigabytes. On a connection \
                                        that is charged by volume it starts only when a person \
                                        asks for it or has allowed it in the settings.",
                recovery_action: "Connect to a network that is not metered, or turn on Download \
                                  on metered connections.",
            },
            UpdateCode::OnBattery => CodeEntry {
                code: self,
                title: "Waiting for mains power",
                severity: Severity::Notice,
                detection: "An automatic download would start while the computer runs on \
                            battery, and the charge is below the automatic threshold or is not \
                            known.",
                user_message: "The update is waiting until the computer is plugged in.",
                technical_explanation: "Downloading and storing a new version takes minutes of \
                                        processor and disk work. Losing power in the middle \
                                        leaves the running version intact, but the work is lost.",
                recovery_action: "Plug the computer in. The download starts by itself.",
            },
            UpdateCode::BatteryLow => CodeEntry {
                code: self,
                title: "Battery too low for a download",
                severity: Severity::Warning,
                detection: "A person asked for the download while the computer runs on battery \
                            and the charge is below the threshold for requested downloads.",
                user_message: "The battery is too low to download the update now.",
                technical_explanation: "The download would probably be cut off by the computer \
                                        switching itself off. The running version would stay \
                                        intact; the work would be lost.",
                recovery_action: "Plug the computer in, then try again.",
            },
            UpdateCode::NotEnoughSpace => CodeEntry {
                code: self,
                title: "Not enough free space for the update",
                severity: Severity::Error,
                detection: "The free space on the system disk is known and is less than the \
                            size of the update, or the assumed size when it is not known, plus \
                            the reserve.",
                user_message: "The update was not applied. There is not enough free space on \
                               this computer.",
                technical_explanation: "A new version is stored beside the running one before \
                                        it is used, so the download needs free space on the \
                                        system disk. Versions the user asked TUR to keep are \
                                        never removed to make room.",
                recovery_action: "Free some space, or release a version you asked TUR to keep, \
                                  then try again.",
            },
            UpdateCode::ImageRejected => CodeEntry {
                code: self,
                title: "This update failed to start on this computer",
                severity: Severity::Notice,
                detection: "The digest of the available image is in the rejected list kept by \
                            tur-boot-health, and the request is automatic.",
                user_message: "TUR will not install this update again by itself.",
                technical_explanation: "This version was started on this computer, did not pass \
                                        the start-up checks within three attempts, and TUR \
                                        returned to the previous version. Staging it again \
                                        automatically would repeat that.",
                recovery_action: "Nothing is needed. When a corrected update is published, it \
                                  will be offered as usual.",
            },
            UpdateCode::RetryRejected => CodeEntry {
                code: self,
                title: "Trying an update again that failed before",
                severity: Severity::Warning,
                detection: "The digest of the available image is in the rejected list kept by \
                            tur-boot-health, and a person asked for it.",
                user_message: "This update did not start properly on this computer before. TUR \
                               will try it once more because you asked.",
                technical_explanation: "The image is staged again and gets a new trial of three \
                                        counted attempts. Its entry in the rejected list stays, \
                                        so it is still not installed automatically.",
                recovery_action: "No action is needed. If it fails again, TUR returns to the \
                                  version in use now.",
            },
            UpdateCode::CountingUnusable => CodeEntry {
                code: self,
                title: "Start-up attempts cannot be counted here",
                severity: Severity::Error,
                detection: "tur-boot-health reports counting.usable as false: the installed \
                            GRUB configuration does not implement the counting contract, or \
                            GRUB cannot write its environment block on this layout.",
                user_message: "The update was not applied. This installation cannot count \
                               start-up attempts, so a failed update could not be undone \
                               automatically.",
                technical_explanation: "A new version is tried for a bounded number of starts \
                                        under a counter in GRUB's environment block. Without \
                                        the counter a version that does not start would be \
                                        started for ever. tur-boot-health would refuse to apply \
                                        the update at shutdown; the update manager does not \
                                        download it in the first place.",
                recovery_action: "Create a support bundle. Updates stay off until the start-up \
                                  counter works.",
            },
            UpdateCode::BootHealthUnknown => CodeEntry {
                code: self,
                title: "Boot-health state is not known",
                severity: Severity::Error,
                detection: "No boot-health state was given to the decision.",
                user_message: "The update was not applied. TUR could not confirm that a failed \
                               update would be undone automatically.",
                technical_explanation: "Whether attempts can be counted, whether a trial is in \
                                        progress and which images failed before are all facts \
                                        of tur-boot-health. Without them an update would be an \
                                        unprotected trial, which TUR refuses.",
                recovery_action: "Try again after the next restart. If it keeps happening, \
                                  create a support bundle.",
            },
            UpdateCode::TrialInProgress => CodeEntry {
                code: self,
                title: "The running version is still on trial",
                severity: Severity::Notice,
                detection: "tur-boot-health reports a trial record or a boot counter, or the \
                            health gate has not given its verdict for this boot.",
                user_message: "TUR is still checking that this version started properly.",
                technical_explanation: "Until the running version is marked good, the previous \
                                        version is the only one known to work. Staging another \
                                        update would remove it, and changing the start-up order \
                                        would bypass the counter.",
                recovery_action: "Wait a few minutes. Nothing is needed.",
            },
            UpdateCode::BootUnhealthy => CodeEntry {
                code: self,
                title: "The running version failed its start-up checks",
                severity: Severity::Warning,
                detection: "The health gate's verdict for this boot is failed, outside a trial, \
                            and the request is automatic.",
                user_message: "TUR did not pass its start-up checks, so updates are not \
                               installed automatically.",
                technical_explanation: "An update keeps the running version as the one to fall \
                                        back to. When the running version is itself unhealthy, \
                                        replacing the older one with it is a choice for a \
                                        person, not for a timer.",
                recovery_action: "Open Recovery in Settings to see what failed. You can still \
                                  install the update yourself.",
            },
            UpdateCode::AutomaticDownloadOff => CodeEntry {
                code: self,
                title: "Automatic downloads are off",
                severity: Severity::Info,
                detection: "An update is known, the request is automatic, and the setting \
                            Download updates automatically is off.",
                user_message: "An update is available. It is not downloaded because automatic \
                               downloads are switched off.",
                technical_explanation: "With the setting off, TUR checks for updates and \
                                        reports them, and downloads only when a person asks.",
                recovery_action: "Download it from Updates in Settings, or turn on Download \
                                  updates automatically.",
            },
            UpdateCode::RepairSession => CodeEntry {
                code: self,
                title: "Waiting for a repair session to end",
                severity: Severity::Notice,
                detection: "A repair session is open and the request is automatic.",
                user_message: "The update is waiting until the repair session has ended.",
                technical_explanation: "A repair session never covers system updates, and TUR \
                                        does not change the system underneath one: what is \
                                        being diagnosed should stay as it is.",
                recovery_action: "End the repair session, or wait for it to end.",
            },
            UpdateCode::StageAllowed => CodeEntry {
                code: self,
                title: "The update may be downloaded and staged",
                severity: Severity::Info,
                detection: "An update is known, nothing is staged, and no rule holds it back.",
                user_message: "The update can be downloaded and prepared.",
                technical_explanation: "The image is downloaded and staged beside the running \
                                        system, held; the staged digest is compared with the \
                                        one that was checked; then it is released, to be used \
                                        at the next restart.",
                recovery_action: "No action is needed.",
            },
            UpdateCode::UpToDate => CodeEntry {
                code: self,
                title: "Up to date",
                severity: Severity::Info,
                detection: "The cached update of the booted deployment has the digest of the \
                            booted image, and nothing is staged.",
                user_message: "TUR is up to date.",
                technical_explanation: "The last check found the image the system already runs.",
                recovery_action: "No action is needed.",
            },
            UpdateCode::NotChecked => CodeEntry {
                code: self,
                title: "No check has been made",
                severity: Severity::Info,
                detection: "The booted deployment has no cached update, and nothing is staged.",
                user_message: "TUR has not checked for updates yet.",
                technical_explanation: "bootc keeps the result of the last check with the \
                                        booted deployment. There is none, so it is not known \
                                        whether an update exists.",
                recovery_action: "Check for updates.",
            },
            UpdateCode::AlreadyStaged => CodeEntry {
                code: self,
                title: "The update is staged",
                severity: Severity::Info,
                detection: "A deployment is staged and is not held, and no newer update is \
                            known.",
                user_message: "The update is ready. Restart to use it.",
                technical_explanation: "The staged deployment is finalised during a normal \
                                        shutdown and started at the next boot, for at most \
                                        three counted attempts.",
                recovery_action: "Restart when it suits you.",
            },
            UpdateCode::StagedHeld => CodeEntry {
                code: self,
                title: "The update is staged and held",
                severity: Severity::Info,
                detection: "A deployment is staged in download-only mode, and no different \
                            update is known.",
                user_message: "The update has been downloaded and is waiting to be released.",
                technical_explanation: "A held deployment is not used at the next restart and \
                                        is discarded by it. Releasing it fetches nothing.",
                recovery_action: "No action is needed.",
            },
            UpdateCode::StagedSuperseded => CodeEntry {
                code: self,
                title: "A newer update replaces the staged one",
                severity: Severity::Info,
                detection: "A deployment is staged and the known update has a different digest.",
                user_message: "A newer update replaces the one that was waiting.",
                technical_explanation: "bootc keeps one staged deployment. Staging the newer \
                                        image discards the one staged before.",
                recovery_action: "No action is needed.",
            },
            UpdateCode::RollbackAllowed => CodeEntry {
                code: self,
                title: "Going back may proceed",
                severity: Severity::Info,
                detection: "A previous version exists, is a container image, is not in the \
                            rejected list, and no trial is in progress.",
                user_message: "TUR can go back to the previous version.",
                technical_explanation: "bootc rollback makes the previous deployment the first \
                                        in the start-up order. The version in use becomes the \
                                        previous one. Nothing is downloaded.",
                recovery_action: "No action is needed.",
            },
            UpdateCode::NoPreviousVersion => CodeEntry {
                code: self,
                title: "No previous version",
                severity: Severity::Notice,
                detection: "bootc status reports no rollback deployment.",
                user_message: "There is no previous version on this computer to go back to.",
                technical_explanation: "A previous version exists from the first update \
                                        onwards.",
                recovery_action: "No action is possible until an update has been installed.",
            },
            UpdateCode::PreviousVersionRejected => CodeEntry {
                code: self,
                title: "The previous version failed to start",
                severity: Severity::Warning,
                detection: "The digest of the rollback deployment is in the rejected list kept \
                            by tur-boot-health.",
                user_message: "TUR will not go back to that version: it did not start properly \
                               on this computer.",
                technical_explanation: "Changing the start-up order starts a version without \
                                        counting its attempts. That is right for a version \
                                        that has worked here, and wrong for one that is known \
                                        not to start.",
                recovery_action: "To try it again, install it as an update, which gives it \
                                  three counted attempts.",
            },
            UpdateCode::PreviousNotManaged => CodeEntry {
                code: self,
                title: "The previous version is not a plain image",
                severity: Severity::Error,
                detection: "The rollback deployment has no container image, or is marked \
                            incompatible.",
                user_message: "The previous version cannot be used by TUR updates.",
                technical_explanation: "bootc rollback refuses a deployment that is not based \
                                        on a container image.",
                recovery_action: "Create a support bundle. Nothing was changed.",
            },
            UpdateCode::RollbackQueued => CodeEntry {
                code: self,
                title: "Going back is already queued",
                severity: Severity::Info,
                detection: "bootc status reports rollbackQueued as true.",
                user_message: "TUR will go back to the previous version at the next restart.",
                technical_explanation: "The previous deployment is already first in the \
                                        start-up order. Running bootc rollback again would \
                                        swap the order back, so it is not planned.",
                recovery_action: "Restart to use it.",
            },
            UpdateCode::PinAllowed => CodeEntry {
                code: self,
                title: "The version may be kept",
                severity: Severity::Info,
                detection: "The named deployment exists and is not pinned.",
                user_message: "TUR can keep this version.",
                technical_explanation: "A pinned deployment is never removed when updates are \
                                        installed. It uses disk space until it is released. \
                                        Keeping a version does not stop updates.",
                recovery_action: "No action is needed.",
            },
            UpdateCode::AlreadyKept => CodeEntry {
                code: self,
                title: "The version is already kept",
                severity: Severity::Info,
                detection: "The named deployment is already pinned.",
                user_message: "TUR is already keeping this version.",
                technical_explanation: "Pinning it again would change nothing.",
                recovery_action: "No action is needed.",
            },
            UpdateCode::NoSuchVersion => CodeEntry {
                code: self,
                title: "No such version",
                severity: Severity::Notice,
                detection: "The named deployment does not exist: there is no rollback \
                            deployment.",
                user_message: "There is no such version on this computer.",
                technical_explanation: "Only the version in use and the previous version can \
                                        be kept or released in this version of the update \
                                        manager.",
                recovery_action: "List the versions on this computer and choose one of them.",
            },
            UpdateCode::UnpinAllowed => CodeEntry {
                code: self,
                title: "The version may be released",
                severity: Severity::Info,
                detection: "The named deployment exists and is pinned.",
                user_message: "TUR can stop keeping this version.",
                technical_explanation: "A released deployment is treated like any other: the \
                                        previous version is removed when the next update is \
                                        applied.",
                recovery_action: "No action is needed.",
            },
            UpdateCode::NotKept => CodeEntry {
                code: self,
                title: "The version is not kept",
                severity: Severity::Info,
                detection: "The named deployment is not pinned.",
                user_message: "TUR is not keeping this version.",
                technical_explanation: "There is nothing to release.",
                recovery_action: "No action is needed.",
            },
        }
    }

    /// The code as a validated [`ErrorCode`].
    ///
    /// Returns `None` only if the text form is not a valid TUR update code; a
    /// unit test checks that this never happens for a registered code.
    pub fn error_code(self) -> Option<ErrorCode> {
        self.as_str()
            .parse::<ErrorCode>()
            .ok()
            .filter(|code| code.family() == Family::Update)
    }
}

/// The whole registry, in code order.
pub fn registry() -> Vec<CodeEntry> {
    UpdateCode::all().iter().map(|code| code.entry()).collect()
}

/// Wording that must never appear in a registry entry: it would steer a user
/// towards bypassing a check instead of fixing its cause.
pub const FORBIDDEN_WORDING: &[&str] =
    &["force", "override", "ignore", "skip", "disable", "anyway"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_code_parses_as_an_update_code() {
        for &code in UpdateCode::all() {
            let parsed = code
                .error_code()
                .unwrap_or_else(|| panic!("{code} does not parse"));
            assert_eq!(parsed.family(), Family::Update);
            assert_eq!(parsed.as_str(), code.as_str());
            assert_eq!(UpdateCode::from_text(code.as_str()), Some(code));
        }
    }

    #[test]
    fn codes_are_unique() {
        let texts: BTreeSet<&str> = UpdateCode::all().iter().map(|code| code.as_str()).collect();
        assert_eq!(texts.len(), UpdateCode::all().len());
        assert_eq!(UpdateCode::all().len(), 37);
    }

    #[test]
    fn every_entry_is_complete() {
        let entries = registry();
        assert_eq!(entries.len(), UpdateCode::all().len());
        for entry in entries {
            assert_eq!(entry.code.entry(), entry);
            for text in [
                entry.title,
                entry.detection,
                entry.user_message,
                entry.technical_explanation,
                entry.recovery_action,
            ] {
                assert!(!text.trim().is_empty(), "{}", entry.code);
                assert!(
                    !text.contains("  "),
                    "{}: double space in {text:?}",
                    entry.code
                );
                assert!(!text.chars().any(char::is_control), "{}", entry.code);
            }
            assert!(entry.user_message.ends_with('.'), "{}", entry.code);
            assert!(entry.recovery_action.ends_with('.'), "{}", entry.code);
            assert!(entry.detection.ends_with('.'), "{}", entry.code);
            assert!(!entry.title.ends_with('.'), "{}", entry.code);
        }
    }

    #[test]
    fn the_desktop_wording_is_kept() {
        // desktop-experience.md, section 11.1: the Offline state, word for word.
        assert_eq!(
            UpdateCode::Offline.entry().user_message,
            "TUR cannot check for updates without a network connection. Nothing is wrong."
        );
        // The Ready state, without the version, which the status report adds.
        assert_eq!(
            UpdateCode::AlreadyStaged.entry().user_message,
            "The update is ready. Restart to use it."
        );
        // After a rollback: "TUR will not install this update again by
        // itself. When a corrected update is published, it will be offered
        // as usual."
        let rejected = UpdateCode::ImageRejected.entry();
        assert_eq!(
            rejected.user_message,
            "TUR will not install this update again by itself."
        );
        assert_eq!(
            rejected.recovery_action,
            "Nothing is needed. When a corrected update is published, it will be offered as usual."
        );
        // "Not applied": every code that stops an update the user would
        // otherwise have got says so first (sections 5.6 and 11.1).
        for code in [
            UpdateCode::NotEnoughSpace,
            UpdateCode::CountingUnusable,
            UpdateCode::BootHealthUnknown,
        ] {
            let entry = code.entry();
            assert!(
                entry
                    .user_message
                    .starts_with("The update was not applied. "),
                "{code}"
            );
            assert_eq!(entry.severity, Severity::Error, "{code}");
        }
    }

    #[test]
    fn the_registry_never_suggests_bypassing_a_check() {
        for entry in registry() {
            for text in [
                entry.title,
                entry.user_message,
                entry.technical_explanation,
                entry.recovery_action,
            ] {
                let lowered = text.to_lowercase();
                for word in FORBIDDEN_WORDING {
                    assert!(
                        !lowered.contains(word),
                        "{}: {word:?} in {text:?}",
                        entry.code
                    );
                }
            }
        }
    }

    #[test]
    fn severities_and_text_forms_are_stable() {
        assert_eq!(Severity::Info.as_str(), "info");
        assert_eq!(Severity::from_text("error"), Some(Severity::Error));
        assert_eq!(UpdateCode::Offline.to_string(), "TUR-UPDATE-NETWORK-001");
        assert_eq!(UpdateCode::from_text("TUR-UPDATE-NOPE-001"), None);
    }
}
