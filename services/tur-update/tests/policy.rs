// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The update policy, rule by rule and then as a whole.
//!
//! The first tests take conditions under which nothing holds an update back
//! and change one thing at a time: each row is one rule of the policy table
//! in `docs/architecture/update-manager.md`. The last tests walk every
//! combination of the conditions (a little over a hundred thousand) and hold
//! each decision against the properties the policy promises, whatever the
//! combination.

use tur_update::codes::{Severity, UpdateCode};
use tur_update::conditions::{Conditions, GIB, Metered, Network, Power};
use tur_update::health::{BootHealth, RejectedImage, Verdict};
use tur_update::plan::{COMMANDS, Operation, plan_pin, plan_rollback, plan_stage, plan_unpin};
use tur_update::policy::{
    Action, Decision, Outcome, Requester, decide_check, decide_pin, decide_rollback, decide_stage,
    decide_unpin, next_step,
};
use tur_update::state::{
    Deployment, Image, ImageRef, Ostree, Signature, SignatureKind, Slot, SystemState,
    UpdateKnowledge,
};

// ---------------------------------------------------------------------------
// Machines
// ---------------------------------------------------------------------------

fn digest(letter: char) -> String {
    format!("sha256:{}", letter.to_string().repeat(64))
}

fn signature() -> Signature {
    Signature {
        kind: SignatureKind::ContainerPolicy,
        remote: None,
    }
}

fn image(letter: char, version: &str) -> Image {
    Image {
        reference: "example.org/tur:stable".to_owned(),
        transport: "registry".to_owned(),
        signature: signature(),
        digest: digest(letter),
        version: Some(version.to_owned()),
        timestamp: Some("2026-11-12T08:53:00Z".to_owned()),
        architecture: "amd64".to_owned(),
    }
}

fn deployment(slot: Slot, image: Image) -> Deployment {
    Deployment {
        slot,
        ostree: Some(Ostree {
            checksum: image.digest.replace("sha256:", ""),
            deploy_serial: 0,
            stateroot: "default".to_owned(),
        }),
        image: Some(image),
        incompatible: false,
        pinned: false,
        soft_reboot_capable: true,
        download_only: false,
        store: Some("ostreeContainer".to_owned()),
        composefs: false,
    }
}

/// TUR 0.3.2 is running; nothing else is on the machine; no check was made.
fn not_checked() -> SystemState {
    SystemState {
        follows: Some(ImageRef {
            reference: "example.org/tur:stable".to_owned(),
            transport: "registry".to_owned(),
            signature: signature(),
        }),
        booted: deployment(Slot::Booted, image('a', "0.3.2")),
        staged: None,
        rollback: None,
        others: Vec::new(),
        rollback_queued: false,
        read_only: false,
        usr_overlay: None,
        update: UpdateKnowledge::NotChecked,
    }
}

fn up_to_date() -> SystemState {
    SystemState {
        update: UpdateKnowledge::UpToDate,
        ..not_checked()
    }
}

/// TUR 0.4.0 was found by a check and is not staged.
fn available() -> SystemState {
    SystemState {
        update: UpdateKnowledge::Available(image('b', "0.4.0")),
        ..not_checked()
    }
}

/// TUR 0.4.0 is staged and will be used at the next restart.
fn staged_ready() -> SystemState {
    SystemState {
        staged: Some(deployment(Slot::Staged, image('b', "0.4.0"))),
        ..available()
    }
}

/// TUR 0.4.0 is staged and held.
fn staged_held() -> SystemState {
    let mut state = staged_ready();
    if let Some(staged) = state.staged.as_mut() {
        staged.download_only = true;
    }
    state
}

/// TUR 0.4.0 is staged; a check since then found TUR 0.4.1.
fn staged_superseded() -> SystemState {
    SystemState {
        update: UpdateKnowledge::Available(image('c', "0.4.1")),
        ..staged_ready()
    }
}

/// TUR 0.4.0 is running after an update; TUR 0.3.2 is the previous version.
fn updated() -> SystemState {
    SystemState {
        booted: deployment(Slot::Booted, image('b', "0.4.0")),
        rollback: Some(deployment(Slot::Rollback, image('a', "0.3.2"))),
        ..up_to_date()
    }
}

fn rejecting(letter: char) -> BootHealth {
    BootHealth {
        rejected: vec![RejectedImage {
            digest: digest(letter),
            image: "example.org/tur:stable".to_owned(),
            version: "0.4.0".to_owned(),
            event_id: "1794483294-ae5a5f67".to_owned(),
        }],
        ..BootHealth::healthy()
    }
}

fn health(change: impl FnOnce(&mut BootHealth)) -> Option<BootHealth> {
    let mut health = BootHealth::healthy();
    change(&mut health);
    Some(health)
}

fn favourable() -> Conditions {
    Conditions::favourable()
}

fn with(change: impl FnOnce(&mut Conditions)) -> Conditions {
    let mut conditions = favourable();
    change(&mut conditions);
    conditions
}

fn battery(percent: Option<u8>) -> Power {
    Power::Battery { percent }
}

const ONLINE: Network = Network::Online {
    metered: Metered::No,
};
const METERED: Network = Network::Online {
    metered: Metered::Yes,
};

fn assert_decision(decision: &Decision, outcome: Outcome, code: UpdateCode, context: &str) {
    assert_eq!(
        (decision.outcome, decision.code),
        (outcome, code),
        "{context}: {decision:?}"
    );
    // The text is the registry's, unchanged.
    let entry = code.entry();
    assert_eq!(decision.user_message, entry.user_message, "{context}");
    assert_eq!(decision.recovery_action, entry.recovery_action, "{context}");
    assert_eq!(decision.severity, entry.severity, "{context}");
    assert_eq!(decision.title, entry.title, "{context}");
}

// ---------------------------------------------------------------------------
// Staging: one rule at a time
// ---------------------------------------------------------------------------

#[test]
fn with_nothing_in_the_way_the_update_is_staged() {
    for requester in [Requester::Automatic, Requester::Person] {
        let decision = decide_stage(&available(), &favourable(), requester);
        assert_decision(
            &decision,
            Outcome::Proceed,
            UpdateCode::StageAllowed,
            requester.as_str(),
        );
        assert!(decision.also.is_empty());
        assert!(decision.notes.is_empty(), "{:?}", decision.notes);
        assert!(decision.reason.contains("TUR 0.4.0"), "{}", decision.reason);
        assert!(decision.reason.contains("TUR 0.3.2"), "{}", decision.reason);
    }
}

/// One condition changed from the favourable ones, and the decision for an
/// automatic request and for a person.
type Row = (
    &'static str,
    Conditions,
    (Outcome, UpdateCode),
    (Outcome, UpdateCode),
);

fn rows() -> Vec<Row> {
    use Outcome::{Proceed, Refuse, Wait};
    use UpdateCode as Code;
    let staged = (Proceed, Code::StageAllowed);
    vec![
        (
            "boot health is not known",
            with(|c| c.boot_health = None),
            (Refuse, Code::BootHealthUnknown),
            (Refuse, Code::BootHealthUnknown),
        ),
        (
            "attempts cannot be counted",
            with(|c| c.boot_health = health(|h| h.counting_usable = false)),
            (Refuse, Code::CountingUnusable),
            (Refuse, Code::CountingUnusable),
        ),
        (
            "the update failed its trial here",
            with(|c| c.boot_health = Some(rejecting('b'))),
            (Refuse, Code::ImageRejected),
            (Proceed, Code::RetryRejected),
        ),
        (
            "another image failed its trial here",
            with(|c| c.boot_health = Some(rejecting('z'))),
            staged,
            staged,
        ),
        (
            "too little free space, size unknown",
            with(|c| c.free_bytes = Some(10 * GIB - 1)),
            (Refuse, Code::NotEnoughSpace),
            (Refuse, Code::NotEnoughSpace),
        ),
        (
            "just enough free space, size unknown",
            with(|c| c.free_bytes = Some(10 * GIB)),
            staged,
            staged,
        ),
        (
            "too little free space, size known",
            with(|c| {
                c.update_bytes = Some(3 * GIB);
                c.free_bytes = Some(5 * GIB - 1);
            }),
            (Refuse, Code::NotEnoughSpace),
            (Refuse, Code::NotEnoughSpace),
        ),
        (
            "just enough free space, size known",
            with(|c| {
                c.update_bytes = Some(3 * GIB);
                c.free_bytes = Some(5 * GIB);
            }),
            staged,
            staged,
        ),
        (
            "free space is not known",
            with(|c| c.free_bytes = None),
            staged,
            staged,
        ),
        (
            "a trial is in progress",
            with(|c| c.boot_health = health(|h| h.trial_in_progress = true)),
            (Wait, Code::TrialInProgress),
            (Wait, Code::TrialInProgress),
        ),
        (
            "the gate has not given its verdict",
            with(|c| c.boot_health = health(|h| h.verdict = None)),
            (Wait, Code::TrialInProgress),
            (Wait, Code::TrialInProgress),
        ),
        (
            "this start failed its checks",
            with(|c| c.boot_health = health(|h| h.verdict = Some(Verdict::Failed))),
            (Wait, Code::BootUnhealthy),
            staged,
        ),
        (
            "this start is degraded",
            with(|c| c.boot_health = health(|h| h.verdict = Some(Verdict::Degraded))),
            staged,
            staged,
        ),
        (
            "automatic downloads are off",
            with(|c| c.settings.automatic_download = false),
            (Wait, Code::AutomaticDownloadOff),
            staged,
        ),
        (
            "a repair session is open",
            with(|c| c.repair_session = true),
            (Wait, Code::RepairSession),
            staged,
        ),
        (
            "offline",
            with(|c| c.network = Network::Offline),
            (Wait, Code::Offline),
            (Wait, Code::Offline),
        ),
        (
            "metered",
            with(|c| c.network = METERED),
            (Wait, Code::Metered),
            staged,
        ),
        (
            "metered, and allowed in the settings",
            with(|c| {
                c.network = METERED;
                c.settings.download_on_metered = true;
            }),
            staged,
            staged,
        ),
        (
            "metered state unknown",
            with(|c| {
                c.network = Network::Online {
                    metered: Metered::Unknown,
                }
            }),
            staged,
            staged,
        ),
        (
            "network state unknown",
            with(|c| c.network = Network::Unknown),
            staged,
            staged,
        ),
        (
            "on battery at 49 %",
            with(|c| c.power = battery(Some(49))),
            (Wait, Code::OnBattery),
            staged,
        ),
        (
            "on battery at 50 %",
            with(|c| c.power = battery(Some(50))),
            staged,
            staged,
        ),
        (
            "on battery, charge unknown",
            with(|c| c.power = battery(None)),
            (Wait, Code::OnBattery),
            staged,
        ),
        (
            "on battery at 19 %",
            with(|c| c.power = battery(Some(19))),
            (Wait, Code::OnBattery),
            (Wait, Code::BatteryLow),
        ),
        (
            "on battery at 20 %",
            with(|c| c.power = battery(Some(20))),
            (Wait, Code::OnBattery),
            staged,
        ),
        (
            "power unknown",
            with(|c| c.power = Power::Unknown),
            staged,
            staged,
        ),
    ]
}

#[test]
fn each_rule_alone_gives_its_code() {
    let state = available();
    let rows = rows();
    assert_eq!(rows.len(), 26);
    for (name, conditions, automatic, person) in rows {
        let decision = decide_stage(&state, &conditions, Requester::Automatic);
        assert_decision(
            &decision,
            automatic.0,
            automatic.1,
            &format!("{name}, automatic"),
        );
        assert!(decision.also.is_empty(), "{name}: {:?}", decision.also);
        let decision = decide_stage(&state, &conditions, Requester::Person);
        assert_decision(&decision, person.0, person.1, &format!("{name}, person"));
        assert!(decision.also.is_empty(), "{name}: {:?}", decision.also);
    }
}

#[test]
fn the_reason_carries_the_numbers() {
    let state = available();
    let space = decide_stage(
        &state,
        &with(|c| c.free_bytes = Some(GIB)),
        Requester::Automatic,
    );
    assert_eq!(
        space.reason,
        "1073741824 bytes are free on the system disk; 10737418240 bytes are needed: \
         8589934592 bytes assumed for the update, whose size is not known, and 2147483648 \
         bytes kept free"
    );
    let known = decide_stage(
        &state,
        &with(|c| {
            c.free_bytes = Some(GIB);
            c.update_bytes = Some(3 * GIB);
        }),
        Requester::Automatic,
    );
    assert!(
        known.reason.contains("3221225472 bytes for the update"),
        "{}",
        known.reason
    );
    let power = decide_stage(
        &state,
        &with(|c| c.power = battery(Some(31))),
        Requester::Automatic,
    );
    assert!(power.reason.contains("at 31 %"), "{}", power.reason);
    assert!(power.reason.contains("50 % or more"), "{}", power.reason);
    let low = decide_stage(
        &state,
        &with(|c| c.power = battery(Some(7))),
        Requester::Person,
    );
    assert!(low.reason.contains("at 7 %"), "{}", low.reason);
    assert!(low.reason.contains("20 % or more"), "{}", low.reason);
}

#[test]
fn notes_say_what_was_not_checked_and_what_a_person_overrode() {
    let state = available();
    let unknown = decide_stage(
        &state,
        &with(|c| {
            c.free_bytes = None;
            c.network = Network::Unknown;
            c.power = Power::Unknown;
        }),
        Requester::Automatic,
    );
    assert_eq!(
        unknown.notes,
        ["Not known, and so not checked: free space, the network, power."]
    );
    let metered = decide_stage(&state, &with(|c| c.network = METERED), Requester::Person);
    assert_eq!(metered.notes, ["The connection is metered."]);
    let retry = decide_stage(
        &state,
        &with(|c| c.boot_health = Some(rejecting('b'))),
        Requester::Person,
    );
    assert_eq!(retry.severity, Severity::Warning);
    assert_eq!(retry.notes.len(), 1);
    assert!(
        retry.notes[0].contains("It stays there."),
        "{:?}",
        retry.notes
    );
    // A kept version is not removed to make room.
    let mut kept = available();
    kept.booted.pinned = true;
    let decision = decide_stage(&kept, &favourable(), Requester::Automatic);
    assert_eq!(
        decision.notes,
        ["1 kept version(s) on this computer are not removed to make room."]
    );
    // An image that is fetched without a signature check is said to be.
    let mut unsigned = available();
    if let Some(follows) = unsigned.follows.as_mut() {
        follows.signature.kind = SignatureKind::None;
    }
    for decision in [
        decide_stage(&unsigned, &favourable(), Requester::Automatic),
        decide_check(&unsigned, &favourable()),
    ] {
        assert_eq!(decision.notes.len(), 1, "{:?}", decision.notes);
        assert!(
            decision.notes[0].starts_with("No signature verification is configured"),
            "{:?}",
            decision.notes
        );
        assert!(decision.proceeds());
    }
}

#[test]
fn everything_that_holds_an_update_back_is_listed_refusals_first() {
    let conditions = with(|c| {
        c.boot_health = Some(BootHealth {
            counting_usable: false,
            trial_in_progress: true,
            ..rejecting('b')
        });
        c.free_bytes = Some(0);
        c.settings.automatic_download = false;
        c.repair_session = true;
        c.network = Network::Offline;
        c.power = battery(Some(3));
    });
    let decision = decide_stage(&available(), &conditions, Requester::Automatic);
    assert_decision(
        &decision,
        Outcome::Refuse,
        UpdateCode::CountingUnusable,
        "everything at once",
    );
    assert_eq!(
        decision.also,
        [
            UpdateCode::ImageRejected,
            UpdateCode::NotEnoughSpace,
            UpdateCode::TrialInProgress,
            UpdateCode::AutomaticDownloadOff,
            UpdateCode::RepairSession,
            UpdateCode::Offline,
            UpdateCode::OnBattery,
        ]
    );
    // For a person the settings, the session and the rejection do not hold
    // it back; the rest does.
    let decision = decide_stage(&available(), &conditions, Requester::Person);
    assert_decision(
        &decision,
        Outcome::Refuse,
        UpdateCode::CountingUnusable,
        "everything at once, person",
    );
    assert_eq!(
        decision.also,
        [
            UpdateCode::NotEnoughSpace,
            UpdateCode::TrialInProgress,
            UpdateCode::Offline,
            UpdateCode::BatteryLow,
        ]
    );
}

// ---------------------------------------------------------------------------
// Staging: what there is to stage
// ---------------------------------------------------------------------------

#[test]
fn with_nothing_to_stage_that_is_the_answer() {
    // Whatever the conditions: nothing is refused that was not going to
    // happen.
    let worst = with(|c| {
        c.boot_health = None;
        c.free_bytes = Some(0);
        c.network = Network::Offline;
        c.power = battery(Some(1));
        c.repair_session = true;
    });
    for conditions in [favourable(), worst] {
        for requester in [Requester::Automatic, Requester::Person] {
            let decision = decide_stage(&not_checked(), &conditions, requester);
            assert_decision(
                &decision,
                Outcome::Nothing,
                UpdateCode::NotChecked,
                "unchecked",
            );
            let decision = decide_stage(&up_to_date(), &conditions, requester);
            assert_decision(
                &decision,
                Outcome::Nothing,
                UpdateCode::UpToDate,
                "up to date",
            );
            assert!(decision.reason.contains("TUR 0.3.2"), "{}", decision.reason);
            let decision = decide_stage(&staged_ready(), &conditions, requester);
            assert_decision(
                &decision,
                Outcome::Nothing,
                UpdateCode::AlreadyStaged,
                "staged",
            );
            assert!(decision.reason.contains("TUR 0.4.0"), "{}", decision.reason);
            // Staged by someone, with no check on record.
            let mut by_hand = staged_ready();
            by_hand.update = UpdateKnowledge::NotChecked;
            let decision = decide_stage(&by_hand, &conditions, requester);
            assert_decision(
                &decision,
                Outcome::Nothing,
                UpdateCode::AlreadyStaged,
                "by hand",
            );
        }
    }
}

#[test]
fn a_held_update_is_released_without_a_download() {
    // Nothing is downloaded, so space, the network, power and the download
    // setting do not matter.
    let nothing_to_download_with = with(|c| {
        c.free_bytes = Some(0);
        c.network = Network::Offline;
        c.power = battery(Some(1));
        c.settings.automatic_download = false;
    });
    for state in [staged_held(), {
        let mut unknown = staged_held();
        unknown.update = UpdateKnowledge::NotChecked;
        unknown
    }] {
        for conditions in [favourable(), nothing_to_download_with.clone()] {
            for requester in [Requester::Automatic, Requester::Person] {
                let decision = decide_stage(&state, &conditions, requester);
                assert_decision(&decision, Outcome::Proceed, UpdateCode::StagedHeld, "held");
                assert!(decision.notes.is_empty(), "{:?}", decision.notes);
            }
        }
    }
    // What protects the machine still applies.
    let held = staged_held();
    for (conditions, automatic, person) in [
        (
            with(|c| c.boot_health = None),
            (Outcome::Refuse, UpdateCode::BootHealthUnknown),
            (Outcome::Refuse, UpdateCode::BootHealthUnknown),
        ),
        (
            with(|c| c.boot_health = Some(rejecting('b'))),
            (Outcome::Refuse, UpdateCode::ImageRejected),
            (Outcome::Proceed, UpdateCode::RetryRejected),
        ),
        (
            with(|c| c.boot_health = health(|h| h.trial_in_progress = true)),
            (Outcome::Wait, UpdateCode::TrialInProgress),
            (Outcome::Wait, UpdateCode::TrialInProgress),
        ),
        (
            with(|c| c.repair_session = true),
            (Outcome::Wait, UpdateCode::RepairSession),
            (Outcome::Proceed, UpdateCode::StagedHeld),
        ),
    ] {
        let decision = decide_stage(&held, &conditions, Requester::Automatic);
        assert_decision(&decision, automatic.0, automatic.1, "held, automatic");
        let decision = decide_stage(&held, &conditions, Requester::Person);
        assert_decision(&decision, person.0, person.1, "held, person");
    }
}

#[test]
fn a_newer_update_replaces_the_staged_one() {
    for requester in [Requester::Automatic, Requester::Person] {
        let decision = decide_stage(&staged_superseded(), &favourable(), requester);
        assert_decision(
            &decision,
            Outcome::Proceed,
            UpdateCode::StagedSuperseded,
            "superseded",
        );
        assert_eq!(
            decision.reason,
            format!(
                "TUR 0.4.1 (sha256:{}…) replaces the staged TUR 0.4.0",
                "c".repeat(12)
            )
        );
    }
    // A held one that is not the update found since is replaced too.
    let mut held = staged_superseded();
    if let Some(staged) = held.staged.as_mut() {
        staged.download_only = true;
    }
    let decision = decide_stage(&held, &favourable(), Requester::Automatic);
    assert_decision(
        &decision,
        Outcome::Proceed,
        UpdateCode::StagedSuperseded,
        "held and superseded",
    );
    // It is a download: the download rules apply.
    let decision = decide_stage(
        &held,
        &with(|c| c.network = Network::Offline),
        Requester::Automatic,
    );
    assert_decision(
        &decision,
        Outcome::Wait,
        UpdateCode::Offline,
        "superseded, offline",
    );
}

#[test]
fn a_system_bootc_cannot_update_is_refused_before_anything_else() {
    let mut no_image = available();
    no_image.booted.image = None;
    let mut layered = available();
    layered.booted.incompatible = true;
    let mut live_media = available();
    live_media.read_only = true;
    let mut both = available();
    both.booted.incompatible = true;
    both.read_only = true;
    for conditions in [favourable(), with(|c| c.boot_health = None)] {
        for (state, code, also) in [
            (&no_image, UpdateCode::BootedNotManaged, vec![]),
            (&layered, UpdateCode::BootedNotManaged, vec![]),
            (&live_media, UpdateCode::ReadOnlyMedium, vec![]),
            (
                &both,
                UpdateCode::BootedNotManaged,
                vec![UpdateCode::ReadOnlyMedium],
            ),
        ] {
            for requester in [Requester::Automatic, Requester::Person] {
                let decision = decide_stage(state, &conditions, requester);
                assert_decision(&decision, Outcome::Refuse, code, "stage");
                assert_eq!(decision.also, also);
            }
            let decision = decide_check(state, &conditions);
            assert_decision(&decision, Outcome::Refuse, code, "check");
            assert_eq!(decision.also, also);
            assert_eq!(next_step(state, &conditions).action, Action::Refuse);
        }
    }
    assert!(
        decide_stage(&no_image, &favourable(), Requester::Person)
            .reason
            .contains("no container image")
    );
    assert!(
        decide_stage(&layered, &favourable(), Requester::Person)
            .reason
            .contains("incompatible")
    );
}

// ---------------------------------------------------------------------------
// Checking
// ---------------------------------------------------------------------------

#[test]
fn a_check_needs_only_a_network() {
    let state = not_checked();
    let decision = decide_check(&state, &favourable());
    assert_decision(
        &decision,
        Outcome::Proceed,
        UpdateCode::CheckAllowed,
        "favourable",
    );
    assert_eq!(
        decision.reason,
        "the system follows example.org/tur:stable (registry)"
    );
    assert!(decision.notes.is_empty());
    // Nothing known about the conditions at all: a check may run.
    let decision = decide_check(&state, &Conditions::default());
    assert_decision(
        &decision,
        Outcome::Proceed,
        UpdateCode::CheckAllowed,
        "unknown",
    );
    // On a dying battery, in a repair session, with a full disk and a
    // metered connection, with boot health unknown: a check may run.
    let harsh = with(|c| {
        c.power = battery(Some(1));
        c.repair_session = true;
        c.free_bytes = Some(0);
        c.network = METERED;
        c.boot_health = None;
        c.settings.automatic_download = false;
    });
    let decision = decide_check(&state, &harsh);
    assert_decision(
        &decision,
        Outcome::Proceed,
        UpdateCode::CheckAllowed,
        "harsh",
    );
    assert_eq!(decision.notes.len(), 1);
    assert!(
        decision.notes[0].contains("a few kilobytes"),
        "{:?}",
        decision.notes
    );
    // Offline: the desktop's sentence, word for word.
    let decision = decide_check(&state, &with(|c| c.network = Network::Offline));
    assert_decision(&decision, Outcome::Wait, UpdateCode::Offline, "offline");
    assert_eq!(
        decision.user_message,
        "TUR cannot check for updates without a network connection. Nothing is wrong."
    );
    // A system that follows no image.
    let mut adrift = not_checked();
    adrift.follows = None;
    assert!(
        decide_check(&adrift, &favourable())
            .reason
            .contains("names no image")
    );
}

// ---------------------------------------------------------------------------
// The next automatic step
// ---------------------------------------------------------------------------

#[test]
fn the_update_manager_checks_stages_or_waits() {
    // Nothing known, or up to date: check.
    for state in [not_checked(), up_to_date()] {
        let next = next_step(&state, &favourable());
        assert_eq!(next.action, Action::Check);
        assert_eq!(next.check.code, UpdateCode::CheckAllowed);
        // ... unless offline.
        let next = next_step(&state, &with(|c| c.network = Network::Offline));
        assert_eq!(next.action, Action::Wait);
        assert_eq!(next.check.code, UpdateCode::Offline);
    }
    // An update is known and nothing is in the way: stage.
    let next = next_step(&available(), &favourable());
    assert_eq!(next.action, Action::Stage);
    assert_eq!(next.stage.code, UpdateCode::StageAllowed);
    // It is staged: nothing, until a restart.
    let next = next_step(&staged_ready(), &favourable());
    assert_eq!(next.action, Action::Nothing);
    assert_eq!(next.stage.code, UpdateCode::AlreadyStaged);
    // Held: release it.
    assert_eq!(
        next_step(&staged_held(), &favourable()).action,
        Action::Stage
    );
    // The known update failed here: keep checking for a corrected one, and
    // do not stage this one.
    let rejected = with(|c| c.boot_health = Some(rejecting('b')));
    let next = next_step(&available(), &rejected);
    assert_eq!(next.action, Action::Check);
    assert_eq!(next.stage.code, UpdateCode::ImageRejected);
    assert_eq!(next.stage.outcome, Outcome::Refuse);
    // Waiting for something: wait.
    for conditions in [
        with(|c| c.power = battery(Some(10))),
        with(|c| c.network = METERED),
        with(|c| c.repair_session = true),
        with(|c| c.settings.automatic_download = false),
        with(|c| c.boot_health = health(|h| h.trial_in_progress = true)),
    ] {
        assert_eq!(next_step(&available(), &conditions).action, Action::Wait);
    }
    // Something has to be put right: refuse.
    for conditions in [
        with(|c| c.boot_health = None),
        with(|c| c.boot_health = health(|h| h.counting_usable = false)),
        with(|c| c.free_bytes = Some(0)),
    ] {
        assert_eq!(next_step(&available(), &conditions).action, Action::Refuse);
    }
}

// ---------------------------------------------------------------------------
// Going back
// ---------------------------------------------------------------------------

#[test]
fn going_back_needs_a_previous_version_that_worked() {
    // No previous version.
    let decision = decide_rollback(&available(), &favourable());
    assert_decision(
        &decision,
        Outcome::Refuse,
        UpdateCode::NoPreviousVersion,
        "none",
    );
    assert!(decision.also.is_empty());
    // A previous version, and nothing in the way.
    let decision = decide_rollback(&updated(), &favourable());
    assert_decision(
        &decision,
        Outcome::Proceed,
        UpdateCode::RollbackAllowed,
        "updated",
    );
    assert_eq!(
        decision.reason,
        "TUR 0.3.2 becomes the version started first; TUR 0.4.0 becomes the previous version"
    );
    assert!(decision.notes.is_empty());
    // Already queued: running the command again would undo it.
    let mut queued = updated();
    queued.rollback_queued = true;
    let decision = decide_rollback(&queued, &favourable());
    assert_decision(
        &decision,
        Outcome::Nothing,
        UpdateCode::RollbackQueued,
        "queued",
    );
    // The previous version is the one that failed here.
    let decision = decide_rollback(&updated(), &with(|c| c.boot_health = Some(rejecting('a'))));
    assert_decision(
        &decision,
        Outcome::Refuse,
        UpdateCode::PreviousVersionRejected,
        "rejected",
    );
    // The previous version is not a plain image.
    let mut layered = updated();
    if let Some(previous) = layered.rollback.as_mut() {
        previous.incompatible = true;
    }
    let decision = decide_rollback(&layered, &favourable());
    assert_decision(
        &decision,
        Outcome::Refuse,
        UpdateCode::PreviousNotManaged,
        "layered",
    );
    let mut no_image = updated();
    if let Some(previous) = no_image.rollback.as_mut() {
        previous.image = None;
    }
    let decision = decide_rollback(&no_image, &favourable());
    assert_decision(
        &decision,
        Outcome::Refuse,
        UpdateCode::PreviousNotManaged,
        "no image",
    );
    // The running version is still on trial, or not judged yet.
    for change in [
        health(|h| h.trial_in_progress = true),
        health(|h| h.verdict = None),
    ] {
        let decision = decide_rollback(&updated(), &with(|c| c.boot_health = change));
        assert_decision(
            &decision,
            Outcome::Wait,
            UpdateCode::TrialInProgress,
            "trial",
        );
    }
    // The running version failed its checks: that is a reason to go back.
    let failed = with(|c| c.boot_health = health(|h| h.verdict = Some(Verdict::Failed)));
    assert!(decide_rollback(&updated(), &failed).proceeds());
    // Read-only media.
    let mut live_media = updated();
    live_media.read_only = true;
    let decision = decide_rollback(&live_media, &favourable());
    assert_decision(
        &decision,
        Outcome::Refuse,
        UpdateCode::ReadOnlyMedium,
        "read-only",
    );
    live_media.rollback = None;
    let decision = decide_rollback(&live_media, &favourable());
    assert_decision(
        &decision,
        Outcome::Refuse,
        UpdateCode::ReadOnlyMedium,
        "read-only",
    );
    assert_eq!(decision.also, [UpdateCode::NoPreviousVersion]);
    // Rejected and on trial at once: the refusal comes first.
    let both = with(|c| {
        c.boot_health = Some(BootHealth {
            trial_in_progress: true,
            ..rejecting('a')
        })
    });
    let decision = decide_rollback(&updated(), &both);
    assert_decision(
        &decision,
        Outcome::Refuse,
        UpdateCode::PreviousVersionRejected,
        "both",
    );
    assert_eq!(decision.also, [UpdateCode::TrialInProgress]);
}

#[test]
fn going_back_does_not_depend_on_power_network_space_or_settings() {
    let harsh = with(|c| {
        c.power = battery(Some(1));
        c.network = Network::Offline;
        c.free_bytes = Some(0);
        c.settings.automatic_download = false;
    });
    let decision = decide_rollback(&updated(), &harsh);
    assert_decision(
        &decision,
        Outcome::Proceed,
        UpdateCode::RollbackAllowed,
        "harsh",
    );
    assert!(decision.notes.is_empty());
    // What it says besides: boot health unknown, a staged update, a session.
    let mut state = updated();
    state.staged = Some(deployment(Slot::Staged, image('c', "0.4.1")));
    let decision = decide_rollback(
        &state,
        &with(|c| {
            c.boot_health = None;
            c.repair_session = true;
        }),
    );
    assert!(decision.proceeds());
    assert_eq!(decision.notes.len(), 3, "{:?}", decision.notes);
    assert!(decision.notes[0].starts_with("Boot-health state is not known"));
    assert_eq!(
        decision.notes[1],
        "The update waiting to be used, TUR 0.4.1, is discarded."
    );
    assert_eq!(
        decision.notes[2],
        "A repair session is open. The restart ends it."
    );
}

// ---------------------------------------------------------------------------
// Keeping and releasing
// ---------------------------------------------------------------------------

#[test]
fn keeping_and_releasing_name_the_version_in_use_or_the_previous_one() {
    let state = updated();
    for slot in [Slot::Booted, Slot::Rollback] {
        let decision = decide_pin(&state, slot);
        assert_decision(&decision, Outcome::Proceed, UpdateCode::PinAllowed, "pin");
        assert_eq!(decision.notes, ["Keeping a version does not stop updates."]);
        let decision = decide_unpin(&state, slot);
        assert_decision(&decision, Outcome::Nothing, UpdateCode::NotKept, "unpin");
    }
    let mut kept = updated();
    kept.booted.pinned = true;
    if let Some(previous) = kept.rollback.as_mut() {
        previous.pinned = true;
    }
    for slot in [Slot::Booted, Slot::Rollback] {
        let decision = decide_pin(&kept, slot);
        assert_decision(&decision, Outcome::Nothing, UpdateCode::AlreadyKept, "pin");
        let decision = decide_unpin(&kept, slot);
        assert_decision(
            &decision,
            Outcome::Proceed,
            UpdateCode::UnpinAllowed,
            "unpin",
        );
    }
    // There is no previous version.
    for decision in [
        decide_pin(&available(), Slot::Rollback),
        decide_unpin(&available(), Slot::Rollback),
    ] {
        assert_decision(
            &decision,
            Outcome::Refuse,
            UpdateCode::NoSuchVersion,
            "none",
        );
        assert_eq!(decision.reason, "bootc reports no rollback deployment");
    }
    // A staged version, or any other, cannot be named.
    for slot in [Slot::Staged, Slot::Other] {
        for decision in [
            decide_pin(&staged_ready(), slot),
            decide_unpin(&staged_ready(), slot),
        ] {
            assert_decision(
                &decision,
                Outcome::Refuse,
                UpdateCode::NoSuchVersion,
                "slot",
            );
        }
    }
    assert!(
        decide_pin(&staged_ready(), Slot::Staged)
            .reason
            .contains("OSTree refuses to pin a staged deployment")
    );
    // Read-only media: nothing can be changed, but what is already so is said.
    let mut live_media = kept.clone();
    live_media.read_only = true;
    assert_eq!(
        decide_unpin(&live_media, Slot::Booted).code,
        UpdateCode::ReadOnlyMedium
    );
    assert_eq!(
        decide_pin(&live_media, Slot::Booted).code,
        UpdateCode::AlreadyKept
    );
    live_media.booted.pinned = false;
    assert_eq!(
        decide_pin(&live_media, Slot::Booted).code,
        UpdateCode::ReadOnlyMedium
    );
    assert_eq!(
        decide_unpin(&live_media, Slot::Booted).code,
        UpdateCode::NotKept
    );
}

// ---------------------------------------------------------------------------
// Plans
// ---------------------------------------------------------------------------

fn lines(plan: &tur_update::Plan) -> Vec<String> {
    plan.steps
        .iter()
        .map(|step| step.command.join(" "))
        .collect()
}

#[test]
fn the_plan_for_staging_downloads_held_verifies_and_releases() {
    let plan = plan_stage(&available(), &favourable(), Requester::Automatic);
    assert_eq!(plan.operation, Operation::Stage);
    assert!(plan.decision.proceeds());
    assert!(!plan.executed);
    assert!(plan.restart_required);
    assert_eq!(
        lines(&plan),
        [
            "/usr/bin/bootc upgrade --download-only --quiet",
            "/usr/bin/bootc status --format=json",
            "/usr/bin/bootc upgrade --from-downloaded",
        ]
    );
    assert!(
        plan.steps[1].effect.contains(&digest('b')),
        "{}",
        plan.steps[1].effect
    );
    assert_eq!(
        plan.effects,
        [
            "TUR 0.4.0 is downloaded and stored beside the version in use. TUR 0.3.2 keeps \
             running and is not changed.",
            "At the next restart the computer starts TUR 0.4.0. It gets 3 attempts to start \
             properly; if it does not, TUR returns to TUR 0.3.2 by itself.",
            "TUR 0.3.2 stays on this computer as the previous version. Files, applications and \
             settings carry over to TUR 0.4.0.",
        ]
    );
    // With a previous version on the machine: it goes, unless it is kept.
    let mut with_previous = available();
    with_previous.rollback = Some(deployment(Slot::Rollback, image('9', "0.3.1")));
    let plan = plan_stage(&with_previous, &favourable(), Requester::Automatic);
    assert_eq!(
        plan.effects[3],
        "TUR 0.3.1, the previous version now, is removed when TUR 0.4.0 is applied."
    );
    if let Some(previous) = with_previous.rollback.as_mut() {
        previous.pinned = true;
    }
    let plan = plan_stage(&with_previous, &favourable(), Requester::Automatic);
    assert_eq!(
        plan.effects[3],
        "TUR 0.3.1, the previous version now, stays: TUR was asked to keep it."
    );
    // A newer update than the staged one.
    let plan = plan_stage(&staged_superseded(), &favourable(), Requester::Automatic);
    assert_eq!(lines(&plan).len(), 3);
    assert_eq!(
        plan.effects.last().map(String::as_str),
        Some("The update that was waiting, TUR 0.4.0, is replaced.")
    );
}

#[test]
fn the_plan_for_a_held_update_only_verifies_and_releases() {
    let plan = plan_stage(&staged_held(), &favourable(), Requester::Automatic);
    assert!(plan.decision.proceeds());
    assert_eq!(
        lines(&plan),
        [
            "/usr/bin/bootc status --format=json",
            "/usr/bin/bootc upgrade --from-downloaded",
        ]
    );
    assert_eq!(
        plan.effects[0],
        "TUR 0.4.0 is already on this computer. Nothing is downloaded."
    );
    assert_eq!(plan.effects.len(), 3);
}

#[test]
fn the_plan_for_going_back_says_what_happens_to_settings() {
    let mut state = updated();
    state.staged = Some(deployment(Slot::Staged, image('c', "0.4.1")));
    let plan = plan_rollback(&state, &favourable());
    assert_eq!(plan.operation, Operation::Rollback);
    assert_eq!(lines(&plan), ["/usr/bin/bootc rollback"]);
    assert!(plan.restart_required && !plan.executed);
    assert_eq!(
        plan.effects,
        [
            "At the next restart the computer starts TUR 0.3.2. Until then nothing changes.",
            "TUR 0.4.0 stays on this computer as the previous version, and you can return to it.",
            "Files in home folders and installed applications are not changed.",
            "System settings changed since TUR 0.3.2 was last in use go back to what they were \
             then: each version has its own /etc, and going back does not merge it.",
            "The update waiting to be used, TUR 0.4.1, is discarded.",
            "TUR 0.3.2 is started without counted attempts: it has run on this computer before.",
        ]
    );
}

#[test]
fn the_plans_for_keeping_and_releasing() {
    let state = updated();
    let plan = plan_pin(&state, Slot::Rollback);
    assert_eq!(plan.operation, Operation::Pin);
    assert_eq!(lines(&plan), ["/usr/bin/ostree admin pin rollback"]);
    assert!(!plan.restart_required && !plan.executed);
    assert_eq!(
        plan.effects[0],
        "TUR 0.3.2 is never removed when updates are installed, until it is released."
    );
    let plan = plan_pin(&state, Slot::Booted);
    assert_eq!(lines(&plan), ["/usr/bin/ostree admin pin booted"]);

    let mut kept = updated();
    kept.booted.pinned = true;
    if let Some(previous) = kept.rollback.as_mut() {
        previous.pinned = true;
    }
    let plan = plan_unpin(&kept, Slot::Rollback);
    assert_eq!(plan.operation, Operation::Unpin);
    assert_eq!(lines(&plan), ["/usr/bin/ostree admin pin --unpin rollback"]);
    assert_eq!(
        plan.effects,
        [
            "TUR 0.3.2 is treated like any other version again.",
            "It is removed when the next update is applied.",
        ]
    );
    let plan = plan_unpin(&kept, Slot::Booted);
    assert_eq!(lines(&plan), ["/usr/bin/ostree admin pin --unpin booted"]);
    assert!(plan.effects[1].starts_with("It becomes the previous version"));
}

#[test]
fn a_plan_has_steps_only_when_the_decision_is_to_proceed() {
    let state = updated();
    let empty = [
        plan_stage(
            &available(),
            &with(|c| c.boot_health = None),
            Requester::Person,
        ),
        plan_stage(
            &available(),
            &with(|c| c.network = Network::Offline),
            Requester::Person,
        ),
        plan_stage(&up_to_date(), &favourable(), Requester::Automatic),
        plan_stage(&staged_ready(), &favourable(), Requester::Automatic),
        plan_rollback(&available(), &favourable()),
        plan_rollback(&state, &with(|c| c.boot_health = Some(rejecting('a')))),
        plan_pin(&available(), Slot::Rollback),
        plan_pin(&staged_ready(), Slot::Staged),
        plan_unpin(&state, Slot::Booted),
    ];
    for plan in empty {
        assert!(!plan.decision.proceeds(), "{plan:?}");
        assert!(plan.steps.is_empty(), "{plan:?}");
        assert!(plan.effects.is_empty(), "{plan:?}");
        assert!(!plan.restart_required && !plan.executed, "{plan:?}");
    }
}

// ---------------------------------------------------------------------------
// Every combination
// ---------------------------------------------------------------------------

fn every_state() -> Vec<(&'static str, SystemState)> {
    let mut no_image = available();
    no_image.booted.image = None;
    let mut live_media = available();
    live_media.read_only = true;
    let mut held_superseded = staged_superseded();
    if let Some(staged) = held_superseded.staged.as_mut() {
        staged.download_only = true;
    }
    let mut updated_with_update = updated();
    updated_with_update.update = UpdateKnowledge::Available(image('c', "0.4.1"));
    let mut queued = updated();
    queued.rollback_queued = true;
    vec![
        ("not checked", not_checked()),
        ("up to date", up_to_date()),
        ("available", available()),
        ("staged", staged_ready()),
        ("held", staged_held()),
        ("superseded", staged_superseded()),
        ("held and superseded", held_superseded),
        ("updated", updated()),
        ("updated, update available", updated_with_update),
        ("rollback queued", queued),
        ("no image", no_image),
        ("read-only", live_media),
    ]
}

fn every_health() -> Vec<Option<BootHealth>> {
    vec![
        None,
        Some(BootHealth::healthy()),
        health(|h| h.counting_usable = false),
        health(|h| h.trial_in_progress = true),
        health(|h| h.verdict = None),
        health(|h| h.verdict = Some(Verdict::Failed)),
        health(|h| h.verdict = Some(Verdict::Degraded)),
        // The update, the staged version and the previous version failed here.
        Some(rejecting('b')),
        Some(rejecting('c')),
        Some(rejecting('a')),
    ]
}

fn every_conditions() -> Vec<Conditions> {
    let powers = [
        Power::Unknown,
        Power::Mains,
        battery(None),
        battery(Some(5)),
        battery(Some(35)),
        battery(Some(80)),
    ];
    let networks = [
        Network::Unknown,
        Network::Offline,
        ONLINE,
        METERED,
        Network::Online {
            metered: Metered::Unknown,
        },
    ];
    let spaces = [None, Some(0), Some(200 * GIB)];
    let mut all = Vec::new();
    for power in powers {
        for network in networks {
            for free_bytes in spaces {
                for boot_health in every_health() {
                    for repair_session in [false, true] {
                        for automatic_download in [false, true] {
                            for download_on_metered in [false, true] {
                                let mut conditions = Conditions {
                                    power,
                                    network,
                                    free_bytes,
                                    boot_health: boot_health.clone(),
                                    repair_session,
                                    ..Conditions::default()
                                };
                                conditions.settings.automatic_download = automatic_download;
                                conditions.settings.download_on_metered = download_on_metered;
                                all.push(conditions);
                            }
                        }
                    }
                }
            }
        }
    }
    all
}

/// The one outcome each code is ever given with.
fn outcome_of(code: UpdateCode) -> Option<Outcome> {
    use UpdateCode as Code;
    Some(match code {
        Code::CheckAllowed
        | Code::StageAllowed
        | Code::StagedHeld
        | Code::StagedSuperseded
        | Code::RetryRejected
        | Code::RollbackAllowed
        | Code::PinAllowed
        | Code::UnpinAllowed => Outcome::Proceed,
        Code::UpToDate
        | Code::NotChecked
        | Code::AlreadyStaged
        | Code::RollbackQueued
        | Code::AlreadyKept
        | Code::NotKept => Outcome::Nothing,
        Code::Offline
        | Code::Metered
        | Code::OnBattery
        | Code::BatteryLow
        | Code::TrialInProgress
        | Code::BootUnhealthy
        | Code::AutomaticDownloadOff
        | Code::RepairSession => Outcome::Wait,
        Code::BootedNotManaged
        | Code::ReadOnlyMedium
        | Code::NotEnoughSpace
        | Code::ImageRejected
        | Code::CountingUnusable
        | Code::BootHealthUnknown
        | Code::NoPreviousVersion
        | Code::PreviousVersionRejected
        | Code::PreviousNotManaged
        | Code::NoSuchVersion => Outcome::Refuse,
        // Never the code of a decision: they are failures of the command
        // line, before any decision is made.
        Code::StateUnreadable
        | Code::NotBootcHost
        | Code::FormatUnknown
        | Code::NeedsAdministrator
        | Code::BootHealthUnreadable => return None,
    })
}

fn assert_coherent(decision: &Decision, context: &str) {
    assert_eq!(
        Some(decision.outcome),
        outcome_of(decision.code),
        "{context}: {decision:?}"
    );
    let entry = decision.code.entry();
    assert_eq!(decision.user_message, entry.user_message, "{context}");
    assert_eq!(decision.severity, entry.severity, "{context}");
    // What else holds it back: never the code itself, never twice, refusals
    // before waits, and only when it is held back.
    if matches!(decision.outcome, Outcome::Proceed | Outcome::Nothing) {
        assert!(decision.also.is_empty(), "{context}: {decision:?}");
    }
    let mut seen = vec![decision.code];
    let mut waiting = decision.outcome == Outcome::Wait;
    for code in &decision.also {
        assert!(!seen.contains(code), "{context}: {decision:?}");
        seen.push(*code);
        match outcome_of(*code) {
            Some(Outcome::Refuse) => assert!(!waiting, "{context}: {decision:?}"),
            Some(Outcome::Wait) => waiting = true,
            other => panic!("{context}: {code} listed with {other:?}"),
        }
    }
    // An error is never a mere wait, and a refusal is never mere information.
    match decision.outcome {
        Outcome::Refuse => assert_ne!(decision.severity, Severity::Info, "{context}"),
        Outcome::Wait => assert_ne!(decision.severity, Severity::Error, "{context}"),
        Outcome::Proceed | Outcome::Nothing => {
            assert_ne!(decision.severity, Severity::Error, "{context}");
        }
    }
}

fn staging_target(state: &SystemState) -> Option<(String, bool)> {
    // The digest staging would act on, and whether it is a download.
    let staged = state.staged.as_ref();
    match (&state.update, staged) {
        (UpdateKnowledge::Available(image), Some(staged))
            if staged.digest() == Some(image.digest.as_str()) =>
        {
            staged.download_only.then(|| (image.digest.clone(), false))
        }
        (UpdateKnowledge::Available(image), _) => Some((image.digest.clone(), true)),
        (_, Some(staged)) if staged.download_only => {
            staged.digest().map(|digest| (digest.to_owned(), false))
        }
        _ => None,
    }
}

#[test]
fn every_combination_keeps_the_promises_of_the_policy() {
    let states = every_state();
    let all = every_conditions();
    assert_eq!(all.len(), 6 * 5 * 3 * 10 * 2 * 2 * 2);
    let mut decisions = 0_u64;
    let mut proceeded = 0_u64;
    let mut codes_seen: Vec<UpdateCode> = Vec::new();
    for (name, state) in &states {
        let managed = state.booted.managed() && !state.read_only;
        let target = staging_target(state);
        for conditions in &all {
            let context = format!("{name} under {conditions:?}");
            let check = decide_check(state, conditions);
            assert_coherent(&check, &context);
            // A check runs exactly when the system can be updated and is not
            // known to be offline.
            assert_eq!(
                check.proceeds(),
                managed && conditions.network != Network::Offline,
                "{context}"
            );

            let automatic = decide_stage(state, conditions, Requester::Automatic);
            let person = decide_stage(state, conditions, Requester::Person);
            for (requester, decision) in [
                (Requester::Automatic, &automatic),
                (Requester::Person, &person),
            ] {
                let context = format!("{context}, {requester}");
                assert_coherent(decision, &context);
                decisions += 1;
                if !codes_seen.contains(&decision.code) {
                    codes_seen.push(decision.code);
                }
                let plan = plan_stage(state, conditions, requester);
                assert_eq!(&plan.decision, decision, "{context}");
                assert_eq!(plan.steps.is_empty(), !decision.proceeds(), "{context}");
                assert!(!plan.executed, "{context}");
                for step in &plan.steps {
                    let known = COMMANDS.iter().find(|command| command.id == step.id);
                    let known = known.unwrap_or_else(|| panic!("{context}: {}", step.id));
                    assert_eq!(step.command, known.argv(None), "{context}");
                    assert!(
                        !step.command.iter().any(|word| word == "--apply"),
                        "{context}"
                    );
                }
                if !decision.proceeds() {
                    continue;
                }
                proceeded += 1;
                // Whoever asks, an update is staged only on a system bootc
                // can update, with something to stage, with boot health
                // known and able to count, the running version marked good,
                // and, for a download, with a network that is not known to
                // be down and space that is not known to be short.
                assert!(managed, "{context}");
                let (digest, downloads) = target.clone().expect("something to stage");
                let health = conditions.boot_health.as_ref().expect("boot health known");
                assert!(health.counting_usable, "{context}");
                assert!(!health.trial_in_progress, "{context}");
                assert!(health.verdict.is_some(), "{context}");
                if downloads {
                    assert_ne!(conditions.network, Network::Offline, "{context}");
                    assert!(
                        conditions
                            .free_bytes
                            .is_none_or(|free| free >= conditions.required_bytes()),
                        "{context}"
                    );
                }
                // The first step of a download is the held download; a
                // release downloads nothing.
                assert_eq!(
                    plan.steps[0].id,
                    if downloads {
                        "stage-held"
                    } else {
                        "read-status"
                    },
                    "{context}"
                );
                assert_eq!(plan.steps.last().map(|step| step.id), Some("release"));
                match requester {
                    Requester::Automatic => {
                        // By itself the update manager never stages an image
                        // that failed here, never acts in a repair session or
                        // on a start that failed its checks, and never
                        // downloads against the settings, on a metered
                        // connection that was not allowed, or on a low or
                        // unknown battery.
                        assert!(!health.is_rejected(&digest), "{context}");
                        assert!(!conditions.repair_session, "{context}");
                        assert_ne!(health.verdict, Some(Verdict::Failed), "{context}");
                        if downloads {
                            assert!(conditions.settings.automatic_download, "{context}");
                            assert!(
                                !conditions.network.is_metered()
                                    || conditions.settings.download_on_metered,
                                "{context}"
                            );
                            assert!(
                                match conditions.power {
                                    Power::Battery { percent } =>
                                        percent.is_some_and(|charge| charge >= 50),
                                    Power::Mains | Power::Unknown => true,
                                },
                                "{context}"
                            );
                        }
                    }
                    Requester::Person => {
                        if downloads {
                            assert!(
                                match conditions.power {
                                    Power::Battery { percent } =>
                                        percent.is_none_or(|charge| charge >= 20),
                                    Power::Mains | Power::Unknown => true,
                                },
                                "{context}"
                            );
                        }
                        assert_eq!(
                            decision.code == UpdateCode::RetryRejected,
                            health.is_rejected(&digest),
                            "{context}"
                        );
                    }
                }
            }
            // A person is never held back where the update manager is not.
            if automatic.proceeds() {
                assert!(person.proceeds(), "{context}");
                assert_eq!(person.code, automatic.code, "{context}");
            }
            // With nothing to stage, both get the same answer.
            if automatic.outcome == Outcome::Nothing {
                assert_eq!(person, automatic, "{context}");
            }

            let next = next_step(state, conditions);
            assert_eq!(next.stage, automatic, "{context}");
            assert_eq!(next.check, check, "{context}");
            match next.action {
                Action::Stage => assert!(automatic.proceeds(), "{context}"),
                Action::Check => {
                    assert!(check.proceeds() && !automatic.proceeds(), "{context}");
                }
                Action::Nothing => {
                    assert_eq!(automatic.code, UpdateCode::AlreadyStaged, "{context}");
                }
                Action::Wait | Action::Refuse => assert!(!automatic.proceeds(), "{context}"),
            }

            let rollback = decide_rollback(state, conditions);
            assert_coherent(&rollback, &context);
            let plan = plan_rollback(state, conditions);
            assert_eq!(plan.steps.is_empty(), !rollback.proceeds(), "{context}");
            if rollback.proceeds() {
                let previous = state.rollback.as_ref().expect("a previous version");
                assert!(previous.managed() && !state.rollback_queued && !state.read_only);
                if let Some(health) = &conditions.boot_health {
                    assert!(!health.trial_in_progress && health.verdict.is_some());
                    assert!(
                        !previous
                            .digest()
                            .is_some_and(|digest| health.is_rejected(digest)),
                        "{context}"
                    );
                }
                assert_eq!(plan.steps.len(), 1, "{context}");
                assert_eq!(plan.steps[0].command, ["/usr/bin/bootc", "rollback"]);
            }
        }
        for slot in [Slot::Staged, Slot::Booted, Slot::Rollback, Slot::Other] {
            for decision in [decide_pin(state, slot), decide_unpin(state, slot)] {
                assert_coherent(&decision, name);
                if decision.proceeds() {
                    assert!(matches!(slot, Slot::Booted | Slot::Rollback), "{name}");
                    assert!(!state.read_only, "{name}");
                }
            }
            assert_eq!(
                plan_pin(state, slot).steps.is_empty(),
                !decide_pin(state, slot).proceeds()
            );
            assert_eq!(
                plan_unpin(state, slot).steps.is_empty(),
                !decide_unpin(state, slot).proceeds()
            );
        }
    }
    assert_eq!(decisions, 12 * 7200 * 2);
    // The walk is not vacuous: updates were staged, and every code a staging
    // decision can carry was met.
    assert!(proceeded > 1000, "{proceeded}");
    for code in [
        UpdateCode::StageAllowed,
        UpdateCode::StagedHeld,
        UpdateCode::StagedSuperseded,
        UpdateCode::RetryRejected,
        UpdateCode::UpToDate,
        UpdateCode::NotChecked,
        UpdateCode::AlreadyStaged,
        UpdateCode::BootedNotManaged,
        UpdateCode::ReadOnlyMedium,
        UpdateCode::BootHealthUnknown,
        UpdateCode::CountingUnusable,
        UpdateCode::ImageRejected,
        UpdateCode::NotEnoughSpace,
        UpdateCode::TrialInProgress,
        UpdateCode::BootUnhealthy,
        UpdateCode::AutomaticDownloadOff,
        UpdateCode::RepairSession,
        UpdateCode::Offline,
        UpdateCode::Metered,
        UpdateCode::OnBattery,
        UpdateCode::BatteryLow,
    ] {
        assert!(codes_seen.contains(&code), "{code} was never the decision");
    }
    assert_eq!(codes_seen.len(), 21, "{codes_seen:?}");
}

#[test]
fn every_code_has_one_outcome_and_the_table_covers_the_registry() {
    for &code in UpdateCode::all() {
        // The match in `outcome_of` is exhaustive, so a new code does not
        // compile until it is given its outcome here.
        let _ = outcome_of(code);
    }
    let never_decisions: Vec<UpdateCode> = UpdateCode::all()
        .iter()
        .copied()
        .filter(|code| outcome_of(*code).is_none())
        .collect();
    assert_eq!(
        never_decisions,
        [
            UpdateCode::StateUnreadable,
            UpdateCode::NotBootcHost,
            UpdateCode::FormatUnknown,
            UpdateCode::NeedsAdministrator,
            UpdateCode::BootHealthUnreadable,
        ]
    );
}
