// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The model, held against real output.
//!
//! The fixtures are not written by hand. `tests/fixtures/provenance.json`
//! says where each one comes from:
//!
//! * `bootc/tur-*.json`: what `bootc status` printed on the TUR test image,
//!   booted in a virtual machine by the `boot` and `rollback` workflows: one
//!   deployment; a staged one beside it; after an automatic fallback; after
//!   a good update.
//! * `bootc/container-base-image.json`: what it prints in the pinned base
//!   image run as a container.
//! * `bootc-upstream/*.json`: bootc's own samples, from its source tree at
//!   the version in the image, converted from YAML.
//! * `boot-health/*.json`: what `tur-boot-health status --json` printed on
//!   the same machines at the same moments.
//!
//! The `update` workflow checks that the files are what the provenance file
//! says (`ci/check_fixtures.py`). These tests check that the model reads
//! every one of them and reads it right.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use tur_update::bootc::{self, ParseError};
use tur_update::codes::UpdateCode;
use tur_update::conditions::Conditions;
use tur_update::health::{BootHealth, Verdict, parse_boot_health, parse_rejected};
use tur_update::policy::{
    Action, Outcome, Requester, decide_check, decide_pin, decide_rollback, decide_stage,
    decide_unpin, next_step,
};
use tur_update::state::{
    SignatureKind, Slot, StateError, SystemState, UpdateKnowledge, UpdateState,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read(name: &str) -> String {
    let path = fixtures().join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn state(name: &str) -> SystemState {
    let host = bootc::parse_status(&read(name)).unwrap_or_else(|error| panic!("{name}: {error}"));
    SystemState::from_host(&host).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn health(name: &str) -> BootHealth {
    parse_boot_health(&read(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// Favourable conditions, with the boot-health state of a fixture.
fn with_health(name: &str) -> Conditions {
    Conditions {
        boot_health: Some(health(name)),
        ..Conditions::favourable()
    }
}

fn provenance() -> Value {
    serde_json::from_str(&read("provenance.json")).expect("the provenance file is JSON")
}

fn listed(content: &str) -> Vec<String> {
    provenance()["fixtures"]
        .as_array()
        .expect("fixtures")
        .iter()
        .filter(|entry| entry["content"] == content)
        .map(|entry| entry["file"].as_str().expect("a file name").to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

#[test]
fn every_fixture_has_a_recorded_source() {
    let provenance = provenance();
    assert_eq!(provenance["schema_version"], 1);
    assert_eq!(provenance["bootc_version"], "1.17.1");
    let mut files = BTreeSet::new();
    for entry in provenance["fixtures"].as_array().expect("fixtures") {
        let file = entry["file"].as_str().expect("a file name");
        assert!(files.insert(file.to_owned()), "{file} is listed twice");
        assert!(fixtures().join(file).is_file(), "{file} is missing");
        let source = &entry["source"];
        match entry["kind"].as_str() {
            Some("ci-capture") => {
                // A run of this repository, the artifact and the file in it.
                assert_eq!(source["repository"], "giorgiogiudice/TUR", "{file}");
                assert!(source["run_id"].is_u64(), "{file}");
                assert_eq!(source["commit"].as_str().map(str::len), Some(40), "{file}");
                for member in ["workflow", "artifact", "path", "command"] {
                    assert!(source[member].is_string(), "{file}: {member}");
                }
                assert_eq!(entry["sha256"].as_str().map(str::len), Some(64), "{file}");
                assert!(entry["state"].is_string(), "{file}");
            }
            Some("upstream-fixture") => {
                // bootc's source tree, at the tag of the version in the image.
                assert_eq!(source["repository"], "bootc-dev/bootc", "{file}");
                assert_eq!(source["tag"], "v1.17.1", "{file}");
                let path = source["path"].as_str().expect("a path");
                assert!(path.starts_with("crates/lib/src/fixtures/spec-"), "{file}");
                assert_eq!(source["blob_sha"].as_str().map(str::len), Some(40), "{file}");
                assert_eq!(source["licence"], "MIT OR Apache-2.0", "{file}");
                assert!(file.starts_with("bootc-upstream/"), "{file}");
            }
            other => panic!("{file}: unknown kind {other:?}"),
        }
    }
    // Nothing on disk is unlisted.
    let mut on_disk = BTreeSet::new();
    for directory in ["bootc", "bootc-upstream", "boot-health"] {
        for entry in fs::read_dir(fixtures().join(directory)).expect("a directory") {
            let name = entry.expect("an entry").file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".json") {
                on_disk.insert(format!("{directory}/{name}"));
            }
        }
    }
    assert_eq!(on_disk, files);
    assert_eq!(listed("bootc-status").len(), 19);
    assert_eq!(listed("boot-health-status").len(), 4);
    // bootc's samples are bootc's: its licence text travels with them.
    let licence = read("bootc-upstream/LICENSE-MIT");
    assert!(licence.starts_with("Permission is hereby granted, free of charge"));
}

// ---------------------------------------------------------------------------
// Every document is read
// ---------------------------------------------------------------------------

#[test]
fn every_bootc_document_is_read_and_only_three_are_not_bootc_hosts() {
    let mut not_hosts = Vec::new();
    let files = listed("bootc-status");
    for file in &files {
        let text = read(file);
        let host = bootc::parse_status(&text).unwrap_or_else(|error| panic!("{file}: {error}"));
        match SystemState::from_host(&host) {
            Ok(state) => {
                // Whatever the machine, every decision can be made.
                for conditions in [Conditions::default(), Conditions::favourable()] {
                    let _ = decide_check(&state, &conditions);
                    let _ = decide_stage(&state, &conditions, Requester::Automatic);
                    let _ = decide_stage(&state, &conditions, Requester::Person);
                    let _ = decide_rollback(&state, &conditions);
                    let _ = next_step(&state, &conditions);
                }
                assert!(!state.update_message().is_empty(), "{file}");
                assert!(!state.deployments().is_empty(), "{file}");
            }
            Err(StateError::NotBootcHost) => not_hosts.push(file.as_str()),
        }
    }
    assert_eq!(
        not_hosts,
        [
            "bootc/container-base-image.json",
            "bootc-upstream/staged-rollback.json",
            "bootc-upstream/v1-null.json",
        ]
    );
}

#[test]
fn real_output_is_version_1_and_bootc_s_older_samples_are_read_too() {
    for file in listed("bootc-status") {
        let host = bootc::parse_status(&read(&file)).expect("a document");
        let captured = file.starts_with("bootc/");
        if captured || file == "bootc-upstream/v1-null.json" {
            assert_eq!(host.api_version, bootc::API_VERSION, "{file}");
        } else {
            assert_eq!(host.api_version, bootc::API_VERSION_ALPHA, "{file}");
        }
        assert_eq!(host.kind, bootc::KIND, "{file}");
    }
}

#[test]
fn real_output_with_one_member_spoiled_is_refused() {
    let text = read("bootc/tur-staged.json");
    // A format this code does not know.
    let other = text.replace("org.containers.bootc/v1", "org.containers.bootc/v2");
    assert!(matches!(
        bootc::parse_status(&other),
        Err(ParseError::Format { .. })
    ));
    // A member bootc always writes is gone.
    for (member, replacement) in [
        (r#""pinned":false,"#, ""),
        (r#""imageDigest":"#, r#""digest":"#),
        (r#""deploySerial":0"#, r#""deploySerial":"zero""#),
        (r#""incompatible":false"#, r#""incompatible":"no""#),
    ] {
        assert!(text.contains(member), "{member}");
        let spoiled = text.replace(member, replacement);
        assert!(
            matches!(bootc::parse_status(&spoiled), Err(ParseError::Shape(_))),
            "{member}"
        );
    }
    // Cut short.
    assert!(bootc::parse_status(&text[..text.len() / 2]).is_err());
    // A member this code does not know is passed over, as bootc does.
    let extended = text.replacen(
        r#""status":{"#,
        r#""status":{"somethingNew":{"a":[1,2,3]},"#,
        1,
    );
    assert_ne!(extended, text);
    assert_eq!(
        bootc::parse_status(&extended).expect("a document").status,
        bootc::parse_status(&text).expect("a document").status
    );
}

// ---------------------------------------------------------------------------
// The TUR test image, booted
// ---------------------------------------------------------------------------

#[test]
fn the_installed_test_image_has_one_version_and_has_never_checked() {
    let state = state("bootc/tur-booted.json");
    assert!(state.staged.is_none() && state.rollback.is_none() && state.others.is_empty());
    assert!(!state.rollback_queued && !state.read_only && state.usr_overlay.is_none());
    let booted = &state.booted;
    let image = booted.image.as_ref().expect("an image");
    assert_eq!(image.reference, "localhost/tur-boot-test:ci");
    assert_eq!(image.transport, "registry");
    assert_eq!(
        image.digest,
        "sha256:afe89b500bc99aa43f8e238746d592b93fdf208ac5f8b053848df7793cde4fef"
    );
    assert_eq!(image.version.as_deref(), Some("0.1.0-dev"));
    assert_eq!(
        image.timestamp.as_deref(),
        Some("2026-11-12T08:53:00.265217967Z")
    );
    assert_eq!(image.architecture, "amd64");
    assert!(booted.managed() && !booted.pinned && !booted.download_only);
    // bootc 1.17.1 reports these two; neither plays a part in any decision.
    assert!(booted.soft_reboot_capable);
    assert_eq!(booted.store.as_deref(), Some("ostreeContainer"));
    assert!(!booted.composefs);
    let ostree = booted.ostree.as_ref().expect("the OSTree backend");
    assert_eq!(
        ostree.checksum,
        "24d79b42bb184000a57439f528f9078dce8d9465352d84bf6bf5bef8a20e9a69"
    );
    assert_eq!((ostree.deploy_serial, ostree.stateroot.as_str()), (0, "default"));
    // No signature verification is configured for the test image.
    let follows = state.follows.as_ref().expect("an image to follow");
    assert_eq!(follows.reference, "localhost/tur-boot-test:ci");
    assert_eq!(follows.signature.kind, SignatureKind::None);
    assert_eq!(state.update, UpdateKnowledge::NotChecked);
    assert_eq!(state.update_state(), UpdateState::NotChecked);
    assert_eq!(
        state.update_message(),
        "TUR has not checked for updates yet. Version 0.1.0-dev."
    );
    // The update manager would check; there is nothing to go back to.
    let next = next_step(&state, &with_health("boot-health/normal.json"));
    assert_eq!(next.action, Action::Check);
    assert_eq!(
        decide_rollback(&state, &Conditions::favourable()).code,
        UpdateCode::NoPreviousVersion
    );
}

#[test]
fn a_staged_version_is_ready_and_boot_health_agrees() {
    let state = state("bootc/tur-staged.json");
    let staged = state.staged.as_ref().expect("a staged deployment");
    assert_eq!(staged.slot, Slot::Staged);
    assert!(!staged.download_only);
    let image = staged.image.as_ref().expect("an image");
    assert_eq!(image.transport, "oci");
    assert_eq!(image.reference, "/run/tur-test/images:check-fails");
    // What the system follows is the staged image's reference.
    assert_eq!(
        state.follows.as_ref().map(|follows| follows.reference.as_str()),
        Some("/run/tur-test/images:check-fails")
    );
    assert_eq!(state.update_state(), UpdateState::Ready);
    assert_eq!(
        state.update_message(),
        "TUR 0.1.0-dev is ready. Restart to use it."
    );
    // Boot health at the same moment: nothing rejected, counting usable.
    let health = health("boot-health/normal.json");
    assert!(health.counting_usable && !health.trial_in_progress);
    assert_eq!(health.verdict, Some(Verdict::Degraded));
    assert!(health.rejected.is_empty() && health.notice.is_none());
    let conditions = with_health("boot-health/normal.json");
    let decision = decide_stage(&state, &conditions, Requester::Automatic);
    assert_eq!(
        (decision.outcome, decision.code),
        (Outcome::Nothing, UpdateCode::AlreadyStaged)
    );
    assert_eq!(next_step(&state, &conditions).action, Action::Nothing);
}

#[test]
fn after_a_fallback_the_failed_version_is_never_gone_back_to() {
    let state = state("bootc/tur-after-fallback.json");
    let health = health("boot-health/after-fallback.json");
    // The previous version runs again; the failed one is the rollback entry.
    let failed = state.rollback.as_ref().expect("the failed version");
    assert_eq!(
        failed.digest(),
        Some("sha256:d726caf08260b02710b5c7d954b82465a40af81b5f79ccd0b69497906d7d4da4")
    );
    assert!(!state.rollback_queued);
    // tur-boot-health recorded it, with the event, and tells the user.
    assert_eq!(health.rejected.len(), 1);
    assert_eq!(health.rejected[0].event_id, "1794483294-ae5a5f67");
    assert!(health.is_rejected(failed.digest().expect("a digest")));
    assert!(!health.is_rejected(state.booted.digest().expect("a digest")));
    let notice = health.notice.as_ref().expect("a notice");
    assert_eq!(notice.kind, "fallback");
    assert_eq!(notice.error_code, "TUR-BOOT-TRIAL-003");
    assert!(
        notice
            .text
            .ends_with("The update will not be tried again automatically."),
        "{}",
        notice.text
    );
    // The state file on the same machine holds the same list.
    assert_eq!(
        parse_rejected(&read("boot-health/rejected.json")).expect("the list"),
        health.rejected
    );
    // Going back to it is refused: a rollback is not a counted trial.
    let conditions = with_health("boot-health/after-fallback.json");
    let decision = decide_rollback(&state, &conditions);
    assert_eq!(
        (decision.outcome, decision.code),
        (Outcome::Refuse, UpdateCode::PreviousVersionRejected)
    );
    assert!(decision.reason.contains("sha256:d726caf08260…"), "{}", decision.reason);
    // The update manager keeps checking for a corrected update.
    assert_eq!(next_step(&state, &conditions).action, Action::Check);
}

#[test]
fn after_a_good_update_the_previous_version_can_be_returned_to() {
    let state = state("bootc/tur-after-update.json");
    let health = health("boot-health/after-update.json");
    assert!(health.counting_usable && !health.trial_in_progress);
    assert!(health.rejected.is_empty());
    let previous = state.rollback.as_ref().expect("the previous version");
    assert_eq!(
        previous.image.as_ref().map(|image| image.reference.as_str()),
        Some("localhost/tur-boot-test:ci")
    );
    assert_eq!(state.update_state(), UpdateState::NotChecked);
    let conditions = with_health("boot-health/after-update.json");
    let decision = decide_rollback(&state, &conditions);
    assert_eq!(
        (decision.outcome, decision.code),
        (Outcome::Proceed, UpdateCode::RollbackAllowed)
    );
    // Either version can be kept.
    for slot in [Slot::Booted, Slot::Rollback] {
        assert_eq!(decide_pin(&state, slot).code, UpdateCode::PinAllowed);
        assert_eq!(decide_unpin(&state, slot).code, UpdateCode::NotKept);
    }
}

#[test]
fn an_update_that_was_not_applied_leaves_a_notice() {
    let health = health("boot-health/not-applied.json");
    let notice = health.notice.expect("a notice");
    assert_eq!(notice.kind, "not-applied");
    assert_eq!(notice.error_code, "TUR-BOOT-TRIAL-001");
    assert!(
        notice.text.starts_with("The update was not applied. TUR is still on "),
        "{}",
        notice.text
    );
    assert!(health.rejected.is_empty());
}

#[test]
fn every_boot_health_document_is_read() {
    let files = listed("boot-health-status");
    for file in &files {
        let health = health(file);
        assert!(health.counting_usable, "{file}");
        assert!(!health.trial_in_progress, "{file}");
        assert_eq!(health.verdict, Some(Verdict::Degraded), "{file}");
    }
    // A bootc document is not a boot-health document, and the reverse.
    assert!(parse_boot_health(&read("bootc/tur-booted.json")).is_err());
    assert!(bootc::parse_status(&read("boot-health/normal.json")).is_err());
}

// ---------------------------------------------------------------------------
// The base image as a container
// ---------------------------------------------------------------------------

#[test]
fn in_a_container_bootc_reports_no_booted_deployment() {
    let text = read("bootc/container-base-image.json");
    let host = bootc::parse_status(&text).expect("a document");
    assert_eq!(host.api_version, "org.containers.bootc/v1");
    assert!(host.spec.image.is_none());
    assert!(host.status.booted.is_none());
    assert!(host.status.staged.is_none() && host.status.rollback.is_none());
    assert!(host.status.host_type.is_none());
    assert!(!host.status.read_only && !host.status.rollback_queued);
    assert_eq!(
        SystemState::from_host(&host),
        Err(StateError::NotBootcHost)
    );
}

// ---------------------------------------------------------------------------
// bootc's own samples
// ---------------------------------------------------------------------------

#[test]
fn a_cached_update_is_an_available_update_unless_it_is_what_runs() {
    let conditions = Conditions::favourable();
    let newer = state("bootc-upstream/booted-with-update.json");
    let image = newer.update.image().expect("an update");
    assert_eq!(image.version.as_deref(), Some("stream9.20240901.0"));
    assert_eq!(image.timestamp.as_deref(), Some("2024-09-01T12:00:00Z"));
    assert_eq!(newer.update_state(), UpdateState::Available);
    assert_eq!(newer.update_message(), "TUR stream9.20240901.0 is available.");
    let decision = decide_stage(&newer, &conditions, Requester::Automatic);
    assert_eq!(
        (decision.outcome, decision.code),
        (Outcome::Proceed, UpdateCode::StageAllowed)
    );
    assert_eq!(next_step(&newer, &conditions).action, Action::Stage);

    let same = state("bootc-upstream/booted-update-same-digest.json");
    assert_eq!(same.update, UpdateKnowledge::UpToDate);
    assert_eq!(
        same.update_message(),
        "TUR is up to date. Version stream9.20240807.0."
    );
    assert_eq!(next_step(&same, &conditions).action, Action::Check);

    // An update whose image has no version label is named by its digest.
    let unnamed = state("bootc-upstream/booted-with-update-no-version.json");
    let image = unnamed.update.image().expect("an update");
    assert_eq!(image.version, None);
    assert_eq!(image.label(), "TUR (sha256:b1b1b1b1b1b1…)");
    assert_eq!(
        unnamed.update_message(),
        "TUR (sha256:b1b1b1b1b1b1…) is available."
    );

    let unchecked = state("bootc-upstream/only-booted.json");
    assert_eq!(unchecked.update, UpdateKnowledge::NotChecked);
}

#[test]
fn a_held_deployment_is_told_from_a_ready_one() {
    let conditions = Conditions::favourable();
    let ready = state("bootc-upstream/staged-booted.json");
    assert_eq!(ready.update_state(), UpdateState::Ready);
    assert_eq!(
        decide_stage(&ready, &conditions, Requester::Automatic).code,
        UpdateCode::AlreadyStaged
    );
    let held = state("bootc-upstream/staged-download-only.json");
    assert!(held.staged.as_ref().is_some_and(|staged| staged.download_only));
    assert_eq!(held.update_state(), UpdateState::Held);
    assert_eq!(
        held.update_message(),
        "TUR nightly has been downloaded and is waiting to be released."
    );
    let decision = decide_stage(&held, &conditions, Requester::Automatic);
    assert_eq!(
        (decision.outcome, decision.code),
        (Outcome::Proceed, UpdateCode::StagedHeld)
    );
    // This sample's image is fetched with verification switched off.
    let follows = held.follows.as_ref().expect("an image to follow");
    assert_eq!(follows.signature.kind, SignatureKind::Insecure);
    assert!(!follows.signature.verifies());
    assert!(
        decision
            .notes
            .iter()
            .any(|note| note.contains("signature: insecure")),
        "{:?}",
        decision.notes
    );
}

#[test]
fn kept_versions_are_listed_and_can_be_released() {
    let state = state("bootc-upstream/booted-pinned.json");
    assert!(state.booted.pinned);
    assert_eq!(state.others.len(), 1);
    assert!(state.others[0].pinned);
    assert_eq!(state.others[0].slot, Slot::Other);
    assert_eq!(state.kept(), 2);
    assert_eq!(state.deployments().len(), 2);
    assert_eq!(
        decide_pin(&state, Slot::Booted).code,
        UpdateCode::AlreadyKept
    );
    assert_eq!(
        decide_unpin(&state, Slot::Booted).code,
        UpdateCode::UnpinAllowed
    );
    // The other kept version cannot be named in this slice.
    assert_eq!(
        decide_unpin(&state, Slot::Other).code,
        UpdateCode::NoSuchVersion
    );
}

#[test]
fn a_system_that_is_not_a_plain_image_is_not_updated() {
    // Booted from an OSTree commit that is not a container image; a layered
    // deployment is staged.
    let layered = state("bootc-upstream/rfe-ostree-deployment.json");
    assert!(layered.booted.image.is_none() && !layered.booted.managed());
    assert!(layered.follows.is_none());
    assert!(layered.staged.as_ref().is_some_and(|staged| staged.incompatible));
    // Booted from such a commit, with an image staged to move to.
    let moving = state("bootc-upstream/ostree-to-bootc.json");
    assert!(moving.booted.image.is_none());
    assert!(moving.staged.as_ref().is_some_and(|staged| staged.managed()));
    for state in [&layered, &moving] {
        let conditions = Conditions::favourable();
        assert_eq!(
            decide_check(state, &conditions).code,
            UpdateCode::BootedNotManaged
        );
        for requester in [Requester::Automatic, Requester::Person] {
            let decision = decide_stage(state, &conditions, requester);
            assert_eq!(
                (decision.outcome, decision.code),
                (Outcome::Refuse, UpdateCode::BootedNotManaged)
            );
        }
        assert_eq!(next_step(state, &conditions).action, Action::Refuse);
    }
}

#[test]
fn signature_transport_and_overlay_are_read() {
    let remote = state("bootc-upstream/ostree-remote.json");
    let follows = remote.follows.as_ref().expect("an image to follow");
    assert_eq!(follows.signature.kind, SignatureKind::OstreeRemote);
    assert_eq!(follows.signature.remote.as_deref(), Some("fedora"));
    assert!(follows.signature.verifies());
    // A version label that looks like a number is still text.
    assert_eq!(
        remote
            .booted
            .image
            .as_ref()
            .and_then(|image| image.version.as_deref()),
        Some("20231230.1")
    );

    let local = state("bootc-upstream/via-local-oci.json");
    assert_eq!(
        local
            .booted
            .image
            .as_ref()
            .map(|image| image.transport.as_str()),
        Some("oci")
    );

    let overlay = state("bootc-upstream/booted-usroverlay.json");
    let mounted = overlay.usr_overlay.as_ref().expect("an overlay");
    assert_eq!(
        (mounted.access_mode.as_str(), mounted.persistence.as_str()),
        ("readWrite", "transient")
    );
}
