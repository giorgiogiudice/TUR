// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The update policy.
//!
//! Every function here is pure: it takes the state of the system and the
//! conditions and returns a [`Decision`]. It reads no file, no clock and no
//! process state, and it runs nothing.
//!
//! A decision has one of four outcomes. **Proceed**: the operation may run.
//! **Nothing**: there is nothing for it to do, and that is not a problem.
//! **Wait**: it may not run now, and will be able to without anyone repairing
//! anything. **Refuse**: it may not run until something is put right. Every
//! decision carries a `TUR-UPDATE-*` code, whatever its outcome.
//!
//! When several rules hold an operation back, the decision is the first of
//! them in this order: refusals before waits, and within each in the order
//! the rules are written below. The others are listed in
//! [`Decision::also`], so that a person can see everything that has to change
//! and not one thing at a time.
//!
//! # Checking for updates ([`decide_check`])
//!
//! 1. A system bootc cannot update is refused (`STATE-005`, `STATE-006`).
//! 2. Offline: wait (`NETWORK-001`).
//! 3. Otherwise a check may run (`CHECK-001`), on battery and on a metered
//!    connection too: it fetches a few kilobytes.
//!
//! # Downloading and staging ([`decide_stage`])
//!
//! 1. A system bootc cannot update is refused.
//! 2. If there is nothing to stage, that is the answer: up to date
//!    (`STAGE-002`), never checked (`STAGE-003`), already staged
//!    (`STAGE-004`).
//! 3. Boot health unknown (`HEALTH-004`) or unable to count attempts
//!    (`HEALTH-003`): refused. An update TUR could not undo is not started.
//! 4. The image failed its trial here: refused for an automatic request
//!    (`HEALTH-001`); for a person, allowed, with a warning (`HEALTH-002`).
//! 5. Not enough free space: refused (`SPACE-001`).
//! 6. The running version is still on trial: wait (`HEALTH-005`). Staging
//!    now would remove the only version known to work.
//! 7. The running version failed its checks: an automatic request waits
//!    (`HEALTH-006`).
//! 8. Automatic downloads are off: an automatic request waits
//!    (`SETTINGS-001`).
//! 9. A repair session is open: an automatic request waits (`SESSION-001`).
//! 10. Offline: wait (`NETWORK-001`). Metered, and not allowed in the
//!     settings: an automatic request waits (`NETWORK-002`).
//! 11. On battery below the threshold: wait (`POWER-001` for an automatic
//!     request, `POWER-002` for a person, with a lower threshold).
//! 12. Otherwise the update may be staged (`STAGE-001`; `STAGE-006` when it
//!     replaces a staged one; `STAGE-005` when a held one only needs
//!     releasing, for which rules 5, 8, 10 and 11 do not apply, because
//!     nothing is downloaded).
//!
//! # Going back ([`decide_rollback`])
//!
//! Going back is always a person's request (class 3: it is confirmed on the
//! trusted path, which is not this component's business).
//!
//! 1. Read-only media: refused. No previous version: refused
//!    (`ROLLBACK-002`). Already queued: nothing (`ROLLBACK-005`).
//! 2. The previous version is not a plain image: refused (`ROLLBACK-004`).
//! 3. The previous version failed its trial here: refused (`ROLLBACK-003`).
//!    A rollback starts a version without counting its attempts.
//! 4. The running version is still on trial: wait (`HEALTH-005`).
//! 5. Otherwise it may proceed (`ROLLBACK-001`).
//!
//! # Keeping a version ([`decide_pin`], [`decide_unpin`])
//!
//! "Keep this version" pins a deployment: it is never removed when updates
//! are installed. It does not stop updates. Only the version in use and the
//! previous version can be named.

use serde::Serialize;

use crate::codes::{Severity, UpdateCode};
use crate::conditions::{Conditions, Network, Power};
use crate::health::Verdict;
use crate::state::{Deployment, Image, Slot, SystemState, UpdateKnowledge, short_digest};

string_enum! {
    /// What a decision comes to.
    pub enum Outcome {
        /// The operation may run.
        Proceed => "proceed",
        /// There is nothing for the operation to do.
        Nothing => "nothing",
        /// Not now; it will be possible without anything being repaired.
        Wait => "wait",
        /// Not until something is put right.
        Refuse => "refuse",
    }
}

string_enum! {
    /// Who asks for a download.
    pub enum Requester {
        /// The update manager itself, on its timer.
        Automatic => "automatic",
        /// A person, in Settings or at a terminal.
        Person => "person",
    }
}

/// A decision of the policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    /// What it comes to.
    pub outcome: Outcome,
    /// The code that says why.
    pub code: UpdateCode,
    /// The severity of the code.
    pub severity: Severity,
    /// The title of the code.
    pub title: &'static str,
    /// What the user is told. It is the registry's text, unchanged.
    pub user_message: &'static str,
    /// The facts of this case: versions, digests, numbers.
    pub reason: String,
    /// What the user can do. It is the registry's text, unchanged.
    pub recovery_action: &'static str,
    /// The other rules that hold the operation back, in order.
    pub also: Vec<UpdateCode>,
    /// Facts worth knowing that changed nothing about the outcome.
    pub notes: Vec<String>,
}

impl Decision {
    fn new(outcome: Outcome, code: UpdateCode, reason: String) -> Decision {
        let entry = code.entry();
        Decision {
            outcome,
            code,
            severity: entry.severity,
            title: entry.title,
            user_message: entry.user_message,
            reason,
            recovery_action: entry.recovery_action,
            also: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Whether the operation may run.
    pub fn proceeds(&self) -> bool {
        self.outcome == Outcome::Proceed
    }
}

/// One rule that holds an operation back.
struct Finding {
    outcome: Outcome,
    code: UpdateCode,
    reason: String,
}

fn refuse(code: UpdateCode, reason: String) -> Finding {
    Finding {
        outcome: Outcome::Refuse,
        code,
        reason,
    }
}

fn wait(code: UpdateCode, reason: String) -> Finding {
    Finding {
        outcome: Outcome::Wait,
        code,
        reason,
    }
}

/// The decision for a list of findings: the first refusal, or else the first
/// wait, with the others listed; or, with no finding, `otherwise`.
fn resolve(findings: Vec<Finding>, otherwise: Decision, notes: Vec<String>) -> Decision {
    let (refusals, waits): (Vec<Finding>, Vec<Finding>) = findings
        .into_iter()
        .partition(|finding| finding.outcome == Outcome::Refuse);
    let mut ordered = refusals.into_iter().chain(waits);
    let mut decision = match ordered.next() {
        Some(first) => Decision::new(first.outcome, first.code, first.reason),
        None => otherwise,
    };
    for finding in ordered {
        if finding.code != decision.code && !decision.also.contains(&finding.code) {
            decision.also.push(finding.code);
        }
    }
    decision.notes = notes;
    decision
}

/// The rules about the system itself: is it one bootc can change at all.
fn system_findings(state: &SystemState) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !state.booted.managed() {
        let what = if state.booted.image.is_none() {
            "the running deployment has no container image"
        } else {
            "the running deployment is marked incompatible (layered packages)"
        };
        findings.push(refuse(UpdateCode::BootedNotManaged, what.to_owned()));
    }
    if state.read_only {
        findings.push(refuse(
            UpdateCode::ReadOnlyMedium,
            "bootc reports the physical root as read-only".to_owned(),
        ));
    }
    findings
}

fn signature_note(state: &SystemState) -> Option<String> {
    let follows = state.follows.as_ref()?;
    if follows.signature.verifies() {
        return None;
    }
    Some(format!(
        "No signature verification is configured for the image this system follows \
         ({}; signature: {}).",
        crate::state::printable(&follows.reference),
        follows.signature.kind
    ))
}

/// May a check for updates run now?
pub fn decide_check(state: &SystemState, conditions: &Conditions) -> Decision {
    let mut findings = system_findings(state);
    if conditions.network == Network::Offline {
        findings.push(wait(
            UpdateCode::Offline,
            "the network is offline".to_owned(),
        ));
    }
    let mut notes = Vec::new();
    if conditions.network.is_metered() {
        notes.push(
            "The connection is metered. A check fetches only the manifest and configuration \
             of the image, a few kilobytes."
                .to_owned(),
        );
    }
    notes.extend(signature_note(state));
    let follows = match &state.follows {
        Some(follows) => format!(
            "the system follows {} ({})",
            crate::state::printable(&follows.reference),
            crate::state::printable(&follows.transport)
        ),
        None => "bootc names no image for this system to follow".to_owned(),
    };
    resolve(
        findings,
        Decision::new(Outcome::Proceed, UpdateCode::CheckAllowed, follows),
        notes,
    )
}

/// What staging would act on.
enum Target<'a> {
    /// An image to download and stage.
    Download(&'a Image),
    /// A held deployment to release.
    Release(&'a Deployment),
}

fn describe_image(image: &Image) -> String {
    format!("{} ({})", image.label(), short_digest(&image.digest))
}

fn bytes(count: u64) -> String {
    format!("{count} bytes")
}

/// May the known update be downloaded and staged now, for this requester?
pub fn decide_stage(
    state: &SystemState,
    conditions: &Conditions,
    requester: Requester,
) -> Decision {
    let automatic = requester == Requester::Automatic;
    let system = system_findings(state);
    if !system.is_empty() {
        // The fallback is never used: the list is not empty.
        let fallback = Decision::new(Outcome::Refuse, UpdateCode::BootedNotManaged, String::new());
        return resolve(system, fallback, Vec::new());
    }

    // What is there to stage?
    let staged = state.staged.as_ref();
    let nothing = |code: UpdateCode, reason: String| -> Decision {
        let mut decision = Decision::new(Outcome::Nothing, code, reason);
        decision.notes.extend(signature_note(state));
        decision
    };
    let staged_ready = |staged: &Deployment| -> Decision {
        nothing(
            UpdateCode::AlreadyStaged,
            format!(
                "{} is staged and will be used at the next restart",
                staged.label()
            ),
        )
    };
    let target = match (&state.update, staged) {
        (UpdateKnowledge::Available(image), Some(staged))
            if staged.digest() == Some(image.digest.as_str()) =>
        {
            if staged.download_only {
                Target::Release(staged)
            } else {
                return staged_ready(staged);
            }
        }
        (UpdateKnowledge::Available(image), _) => Target::Download(image),
        (_, Some(staged)) => {
            if staged.download_only {
                Target::Release(staged)
            } else {
                return staged_ready(staged);
            }
        }
        (UpdateKnowledge::UpToDate, None) => {
            return nothing(
                UpdateCode::UpToDate,
                format!(
                    "the last check found the image that is running, {}",
                    state.booted.label()
                ),
            );
        }
        (UpdateKnowledge::NotChecked, None) => {
            return nothing(
                UpdateCode::NotChecked,
                "bootc holds no result of a check for the running deployment".to_owned(),
            );
        }
    };
    let mut findings = Vec::new();
    let (digest, label, downloads) = match &target {
        Target::Download(image) => (Some(image.digest.as_str()), describe_image(image), true),
        Target::Release(staged) => (staged.digest(), staged.label(), false),
    };

    // Boot health: can a failed update be undone, and is this the moment?
    let mut retry = false;
    match &conditions.boot_health {
        None => findings.push(refuse(
            UpdateCode::BootHealthUnknown,
            "no boot-health state was given".to_owned(),
        )),
        Some(health) => {
            if !health.counting_usable {
                findings.push(refuse(
                    UpdateCode::CountingUnusable,
                    "tur-boot-health reports that boot attempts cannot be counted".to_owned(),
                ));
            }
            if let Some(digest) = digest
                && health.is_rejected(digest)
            {
                if automatic {
                    findings.push(refuse(
                        UpdateCode::ImageRejected,
                        format!("{label} is in the rejected list of tur-boot-health"),
                    ));
                } else {
                    retry = true;
                }
            }
            if health.trial_in_progress {
                findings.push(wait(
                    UpdateCode::TrialInProgress,
                    format!(
                        "a trial of {} is in progress: it has not been marked good",
                        state.booted.label()
                    ),
                ));
            } else if health.verdict.is_none() {
                findings.push(wait(
                    UpdateCode::TrialInProgress,
                    "the health gate has not given its verdict for this boot".to_owned(),
                ));
            } else if health.verdict == Some(Verdict::Failed) && automatic {
                findings.push(wait(
                    UpdateCode::BootUnhealthy,
                    "the health gate's verdict for this boot is failed".to_owned(),
                ));
            }
        }
    }

    // Space, for a download.
    let mut unknown: Vec<&str> = Vec::new();
    if downloads {
        let required = conditions.required_bytes();
        match conditions.free_bytes {
            Some(free) if free < required => {
                let size = match conditions.update_bytes {
                    Some(size) => format!("{} for the update", bytes(size)),
                    None => format!(
                        "{} assumed for the update, whose size is not known",
                        bytes(conditions.limits.assumed_update_bytes)
                    ),
                };
                findings.push(refuse(
                    UpdateCode::NotEnoughSpace,
                    format!(
                        "{} are free on the system disk; {} are needed: {size}, and {} kept free",
                        bytes(free),
                        bytes(required),
                        bytes(conditions.limits.reserve_bytes)
                    ),
                ));
            }
            Some(_) => {}
            None => unknown.push("free space"),
        }
    }

    // The user's settings and a repair session hold back an automatic request.
    if automatic && downloads && !conditions.settings.automatic_download {
        findings.push(wait(
            UpdateCode::AutomaticDownloadOff,
            "the setting Download updates automatically is off".to_owned(),
        ));
    }
    if automatic && conditions.repair_session {
        findings.push(wait(
            UpdateCode::RepairSession,
            "a repair session is open".to_owned(),
        ));
    }

    // Network and power, for a download.
    if downloads {
        match conditions.network {
            Network::Offline => findings.push(wait(
                UpdateCode::Offline,
                "the network is offline".to_owned(),
            )),
            Network::Online { .. } => {
                if automatic
                    && conditions.network.is_metered()
                    && !conditions.settings.download_on_metered
                {
                    findings.push(wait(
                        UpdateCode::Metered,
                        "the connection is metered and the setting Download on metered \
                         connections is off"
                            .to_owned(),
                    ));
                }
            }
            Network::Unknown => unknown.push("the network"),
        }
        match conditions.power {
            Power::Mains => {}
            Power::Unknown => unknown.push("power"),
            Power::Battery { percent } => {
                let limits = &conditions.limits;
                if automatic {
                    let enough =
                        percent.is_some_and(|charge| charge >= limits.automatic_battery_percent);
                    if !enough {
                        let charge = match percent {
                            Some(charge) => format!("at {charge} %"),
                            None => "at an unknown charge".to_owned(),
                        };
                        findings.push(wait(
                            UpdateCode::OnBattery,
                            format!(
                                "the computer runs on battery {charge}; an automatic download \
                                 starts on mains power, or on battery at {} % or more",
                                limits.automatic_battery_percent
                            ),
                        ));
                    }
                } else if let Some(charge) = percent
                    && charge < limits.requested_battery_percent
                {
                    findings.push(wait(
                        UpdateCode::BatteryLow,
                        format!(
                            "the computer runs on battery at {charge} %; a download starts at \
                             {} % or more",
                            limits.requested_battery_percent
                        ),
                    ));
                }
            }
        }
    }

    let mut notes = Vec::new();
    if retry {
        notes.push(format!(
            "{label} is in the rejected list of tur-boot-health. It stays there."
        ));
    }
    if downloads && !automatic && conditions.network.is_metered() {
        notes.push("The connection is metered.".to_owned());
    }
    let kept = state.kept();
    if downloads && kept > 0 {
        notes.push(format!(
            "{kept} kept version(s) on this computer are not removed to make room."
        ));
    }
    if !unknown.is_empty() {
        notes.push(format!(
            "Not known, and so not checked: {}.",
            unknown.join(", ")
        ));
    }
    notes.extend(signature_note(state));

    let proceed = if retry {
        Decision::new(
            Outcome::Proceed,
            UpdateCode::RetryRejected,
            format!("{label} failed its start-up trial before; a person asked for it again"),
        )
    } else {
        match (&target, staged) {
            (Target::Release(_), _) => Decision::new(
                Outcome::Proceed,
                UpdateCode::StagedHeld,
                format!("{label} is staged and held; releasing it downloads nothing"),
            ),
            (Target::Download(_), Some(old)) => Decision::new(
                Outcome::Proceed,
                UpdateCode::StagedSuperseded,
                format!("{label} replaces the staged {}", old.label()),
            ),
            (Target::Download(_), None) => Decision::new(
                Outcome::Proceed,
                UpdateCode::StageAllowed,
                format!("{label} can be staged beside {}", state.booted.label()),
            ),
        }
    };
    resolve(findings, proceed, notes)
}

/// May the system go back to the previous version?
pub fn decide_rollback(state: &SystemState, conditions: &Conditions) -> Decision {
    let mut findings = Vec::new();
    if state.read_only {
        findings.push(refuse(
            UpdateCode::ReadOnlyMedium,
            "bootc reports the physical root as read-only".to_owned(),
        ));
    }
    let Some(previous) = state.rollback.as_ref() else {
        let none = Decision::new(
            Outcome::Refuse,
            UpdateCode::NoPreviousVersion,
            "bootc reports no rollback deployment".to_owned(),
        );
        findings.push(refuse(none.code, none.reason.clone()));
        return resolve(findings, none, Vec::new());
    };
    if state.rollback_queued && findings.is_empty() {
        return Decision::new(
            Outcome::Nothing,
            UpdateCode::RollbackQueued,
            format!(
                "{} is already first in the start-up order",
                previous.label()
            ),
        );
    }
    if !previous.managed() {
        findings.push(refuse(
            UpdateCode::PreviousNotManaged,
            format!("{} cannot be made the default by bootc", previous.label()),
        ));
    }
    let mut notes = Vec::new();
    match &conditions.boot_health {
        Some(health) => {
            if let Some(digest) = previous.digest()
                && health.is_rejected(digest)
            {
                findings.push(refuse(
                    UpdateCode::PreviousVersionRejected,
                    format!(
                        "{} ({}) is in the rejected list of tur-boot-health",
                        previous.label(),
                        short_digest(digest)
                    ),
                ));
            }
            if health.trial_in_progress {
                findings.push(wait(
                    UpdateCode::TrialInProgress,
                    format!(
                        "a trial of {} is in progress: it has not been marked good",
                        state.booted.label()
                    ),
                ));
            } else if health.verdict.is_none() {
                findings.push(wait(
                    UpdateCode::TrialInProgress,
                    "the health gate has not given its verdict for this boot".to_owned(),
                ));
            }
        }
        None => notes.push(
            "Boot-health state is not known, so it was not checked whether the previous \
             version failed to start before."
                .to_owned(),
        ),
    }
    if let Some(staged) = &state.staged {
        notes.push(format!(
            "The update waiting to be used, {}, is discarded.",
            staged.label()
        ));
    }
    if conditions.repair_session {
        notes.push("A repair session is open. The restart ends it.".to_owned());
    }
    resolve(
        findings,
        Decision::new(
            Outcome::Proceed,
            UpdateCode::RollbackAllowed,
            format!(
                "{} becomes the version started first; {} becomes the previous version",
                previous.label(),
                state.booted.label()
            ),
        ),
        notes,
    )
}

/// The deployment a keep or release request names, or why there is none.
fn named(state: &SystemState, slot: Slot) -> Result<&Deployment, &'static str> {
    match slot {
        Slot::Booted => Ok(&state.booted),
        Slot::Rollback => state
            .rollback
            .as_ref()
            .ok_or("bootc reports no rollback deployment"),
        Slot::Staged => Err(
            "a staged version cannot be kept before it has been started: OSTree refuses to \
             pin a staged deployment",
        ),
        Slot::Other => Err("only the version in use and the previous version can be named"),
    }
}

fn no_such_version(reason: &str) -> Decision {
    Decision::new(
        Outcome::Refuse,
        UpdateCode::NoSuchVersion,
        reason.to_owned(),
    )
}

fn read_only_medium() -> Decision {
    Decision::new(
        Outcome::Refuse,
        UpdateCode::ReadOnlyMedium,
        "bootc reports the physical root as read-only".to_owned(),
    )
}

/// May this version be kept (pinned)?
pub fn decide_pin(state: &SystemState, slot: Slot) -> Decision {
    let deployment = match named(state, slot) {
        Ok(deployment) => deployment,
        Err(reason) => return no_such_version(reason),
    };
    if deployment.pinned {
        return Decision::new(
            Outcome::Nothing,
            UpdateCode::AlreadyKept,
            format!("{} is already pinned", deployment.label()),
        );
    }
    if state.read_only {
        return read_only_medium();
    }
    let mut decision = Decision::new(
        Outcome::Proceed,
        UpdateCode::PinAllowed,
        format!("{} ({slot}) is not pinned", deployment.label()),
    );
    decision
        .notes
        .push("Keeping a version does not stop updates.".to_owned());
    decision
}

/// May this version be released (unpinned)?
pub fn decide_unpin(state: &SystemState, slot: Slot) -> Decision {
    let deployment = match named(state, slot) {
        Ok(deployment) => deployment,
        Err(reason) => return no_such_version(reason),
    };
    if !deployment.pinned {
        return Decision::new(
            Outcome::Nothing,
            UpdateCode::NotKept,
            format!("{} is not pinned", deployment.label()),
        );
    }
    if state.read_only {
        return read_only_medium();
    }
    Decision::new(
        Outcome::Proceed,
        UpdateCode::UnpinAllowed,
        format!("{} ({slot}) is pinned", deployment.label()),
    )
}

string_enum! {
    /// What the update manager would do next by itself.
    pub enum Action {
        /// Check for updates.
        Check => "check",
        /// Download and stage the known update.
        Stage => "stage",
        /// Nothing: an update is staged and waits for a restart.
        Nothing => "nothing",
        /// Wait.
        Wait => "wait",
        /// Nothing can be done until something is put right.
        Refuse => "refuse",
    }
}

/// The next automatic step, with the two decisions it follows from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NextStep {
    /// What the update manager would do now.
    pub action: Action,
    /// May a check run now.
    pub check: Decision,
    /// May the known update be staged now, automatically.
    pub stage: Decision,
}

/// What the update manager would do next by itself.
///
/// It stages when it may. Otherwise it checks, when the reason it does not
/// stage is one a check could change: nothing is known, the system is up to
/// date, or the known update is one that failed here, which a corrected
/// update would replace. Otherwise it does what the staging decision says.
pub fn next_step(state: &SystemState, conditions: &Conditions) -> NextStep {
    let check = decide_check(state, conditions);
    let stage = decide_stage(state, conditions, Requester::Automatic);
    let a_check_could_help = matches!(
        stage.code,
        UpdateCode::UpToDate | UpdateCode::NotChecked | UpdateCode::ImageRejected
    );
    let action = if stage.proceeds() {
        Action::Stage
    } else if a_check_could_help {
        match check.outcome {
            Outcome::Proceed => Action::Check,
            Outcome::Nothing => Action::Nothing,
            Outcome::Wait => Action::Wait,
            Outcome::Refuse => Action::Refuse,
        }
    } else {
        match stage.outcome {
            Outcome::Proceed => Action::Stage,
            Outcome::Nothing => Action::Nothing,
            Outcome::Wait => Action::Wait,
            Outcome::Refuse => Action::Refuse,
        }
    };
    NextStep {
        action,
        check,
        stage,
    }
}
