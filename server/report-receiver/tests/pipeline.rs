// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The receiver end to end, in process: a clock the test sets, a tracker
//! that records what it is given and can be switched off, a log kept in
//! memory, and real bundles (built without a client's redaction, so the
//! receiver's own pass is what is tested).
//!
//! The same behaviour over HTTP, in the container, with `tur-diag` as the
//! client and a stand-in GitHub API, is the `reports` workflow.

use std::fs;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tur_diag::Profile;
use tur_report_receiver::clock::ManualClock;
use tur_report_receiver::config::Config;
use tur_report_receiver::crypto::Keyring;
use tur_report_receiver::log::Log;
use tur_report_receiver::receiver::{Answer, Receiver};
use tur_report_receiver::testing::{self, PLANTED, USEFUL};
use tur_report_receiver::tracker::{Call, RecordingTracker};

const CONTENT_TYPE: &str = "application/vnd.tur.diag-bundle+zstd";
const START: u64 = 1_793_000_000;
const DAY: u64 = 86_400;

struct Fixture {
    receiver: Receiver,
    clock: Arc<ManualClock>,
    tracker: RecordingTracker,
    log: Arc<Mutex<Vec<u8>>>,
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture(name: &str, limits: Value) -> Fixture {
    let root = std::env::temp_dir().join(format!("tur-receiver-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("a temporary directory");
    let config = json!({
        "listen": "127.0.0.1:0",
        "public_url": "https://reports.tur.example",
        "data_dir": root.join("data"),
        "keys_dir": root.join("keys"),
        "current_key": "k1",
        "github": {
            "repository": "tur-test/reports",
            "token_file": root.join("token"),
        },
        "limits": limits,
        "timing": {
            "file_interval_seconds": 1,
            "retry_initial_seconds": 60,
            "retry_max_seconds": 600,
            "sweep_interval_seconds": 1,
            "upload_timeout_seconds": 5
        }
    });
    let config = Config::parse(&config.to_string()).expect("a valid configuration");
    let clock = Arc::new(ManualClock::new(START));
    let (log, memory) = Log::memory();
    let keyring = Keyring::from_keys("k1", &[("k1", [7u8; 32])]);
    let receiver = Receiver::new(config, keyring, clock.clone(), log).expect("a receiver");
    Fixture {
        receiver,
        clock,
        tracker: RecordingTracker::new(),
        log: memory,
        root,
    }
}

fn default_fixture(name: &str) -> Fixture {
    fixture(name, json!({}))
}

fn client(last: u8) -> IpAddr {
    IpAddr::from([203, 0, 113, last])
}

fn body(answer: &Answer) -> Value {
    serde_json::from_str(&answer.body).expect("a JSON answer")
}

fn report(title: &str, code: &str) -> String {
    json!({
        "schema_version": 1,
        "kind": "request",
        "title": title,
        "description": "It stopped.",
        "error_code": code,
        "component": "audio",
        "affected_version": null,
        "prepared_by": "person",
        "boot_health": null
    })
    .to_string()
}

fn bundle(title: &str, code: &str) -> Vec<u8> {
    testing::bundle(
        Some(&report(title, code)),
        &[("system/kernel.txt", title)],
        Profile::Default,
    )
}

fn submit(fixture: &Fixture, archive: &[u8]) -> Answer {
    assert_eq!(fixture.receiver.admit_upload(client(1)), None);
    fixture.receiver.submit(Some(CONTENT_TYPE), archive)
}

fn token_of(answer: &Answer) -> String {
    body(answer)["status_token"]
        .as_str()
        .expect("a token")
        .to_owned()
}

fn state(fixture: &Fixture, token: &str) -> Value {
    let answer = fixture.receiver.status(client(2), token);
    assert_eq!(answer.status, 200, "{}", answer.body);
    body(&answer)
}

fn log_text(fixture: &Fixture) -> String {
    String::from_utf8(fixture.log.lock().expect("lock").clone()).expect("text")
}

#[test]
fn a_good_report_is_filed_once_and_a_duplicate_becomes_a_comment() {
    let fixture = default_fixture("duplicate");
    let first = submit(
        &fixture,
        &bundle("No sound after resume", "TUR-AUDIO-RESUME-001"),
    );
    assert_eq!(first.status, 202, "{}", first.body);
    let receipt = body(&first);
    let id = receipt["report_id"].as_str().expect("an id").to_owned();
    assert!(tur_diag::report::is_report_id(&id));
    let token = token_of(&first);
    assert_eq!(
        receipt["status_url"],
        format!("https://reports.tur.example/v1/reports/{token}")
    );
    assert_eq!(receipt["retention_days"], 90);
    assert_eq!(
        state(&fixture, &token),
        json!({"schema_version": 1, "state": "received", "fixed_in": null})
    );

    let round = fixture.receiver.file_due(&fixture.tracker);
    assert_eq!((round.filed, round.failed), (1, 0));
    assert_eq!(state(&fixture, &token)["state"], "filed");

    // The same error code, component and version: a comment, not an issue.
    fixture.clock.advance(1);
    let second = submit(&fixture, &bundle("Still no sound", "TUR-AUDIO-RESUME-001"));
    assert_eq!(second.status, 202);
    // Another code: a new issue.
    fixture.clock.advance(1);
    let third = submit(&fixture, &bundle("Crackling", "TUR-AUDIO-RESUME-002"));
    assert_eq!(third.status, 202);
    let round = fixture.receiver.file_due(&fixture.tracker);
    assert_eq!((round.filed, round.failed), (2, 0));
    // Nothing is filed twice.
    assert_eq!(fixture.receiver.file_due(&fixture.tracker).filed, 0);

    let calls = fixture.tracker.calls();
    assert_eq!(calls.len(), 3, "{calls:?}");
    match &calls[0] {
        Call::Issue {
            number,
            title,
            body,
            labels,
        } => {
            assert_eq!(*number, 1);
            assert!(title.starts_with("[TUR-AUDIO-RESUME-001] audio 0.2.0: No sound after resume"));
            assert!(body.contains(&id));
            assert_eq!(labels, &["problem-report".to_owned()]);
        }
        other => panic!("not an issue: {other:?}"),
    }
    assert!(
        matches!(&calls[1], Call::Comment { issue: 1, body } if body.contains("Still no sound"))
    );
    assert!(matches!(&calls[2], Call::Issue { number: 2, .. }));
    // The raw bundle never goes to the tracker.
    for call in &calls {
        let text = format!("{call:?}");
        assert!(!text.contains("manifest.json\\\":"), "a manifest was filed");
        assert!(!text.contains("checksums.sha256  "), "checksums were filed");
    }
}

#[test]
fn planted_personal_data_never_leaves_the_receiver() {
    let fixture = default_fixture("planted");
    let archive = testing::planted_bundle();
    let answer = submit(&fixture, &archive);
    assert_eq!(answer.status, 202, "{}", answer.body);
    let token = token_of(&answer);
    let duplicate = submit(&fixture, &archive);
    assert_eq!(duplicate.status, 202);
    let round = fixture.receiver.file_due(&fixture.tracker);
    assert_eq!(round.filed, 2);

    let calls = fixture.tracker.calls();
    assert!(matches!(calls[0], Call::Issue { .. }));
    assert!(matches!(calls[1], Call::Comment { .. }));
    let mut filed = String::new();
    for call in &calls {
        match call {
            Call::Issue {
                title,
                body,
                labels,
                ..
            } => {
                filed.push_str(title);
                filed.push('\n');
                filed.push_str(body);
                filed.push_str(&labels.join(","));
            }
            Call::Comment { body, .. } => filed.push_str(body),
        }
    }
    let leaked = testing::leaks(&filed);
    assert!(
        leaked.is_empty(),
        "planted data reached the tracker: {leaked:?}\n\n{filed}"
    );
    // The scrubber does not just delete everything.
    for useful in USEFUL {
        assert!(
            filed.contains(useful),
            "{useful} is missing from the summary:\n{filed}"
        );
    }
    assert!(filed.contains("[TUR-AUDIO-RESUME-001] audio 0.2.0:"));
    // Every category of the corpus was planted.
    let categories: std::collections::BTreeSet<&str> = PLANTED.iter().map(|p| p.category).collect();
    assert!(categories.len() >= 15);

    let mut answers = answer.body.clone();
    answers.push_str(&duplicate.body);
    answers.push_str(&fixture.receiver.status(client(3), &token).body);
    assert!(testing::leaks(&answers).is_empty(), "{answers}");

    let log = log_text(&fixture);
    assert!(testing::leaks(&log).is_empty(), "{log}");
    assert!(!log.contains(&token), "the status token is in the log");
    assert!(
        !log.contains("203.0.113."),
        "a client address is in the log"
    );
}

#[test]
fn bundles_are_encrypted_at_rest_and_decrypt_for_a_maintainer() {
    let fixture = default_fixture("at-rest");
    let archive = testing::planted_bundle();
    let answer = submit(&fixture, &archive);
    let id = body(&answer)["report_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let stored =
        fs::read(fixture.root.join("data/bundles").join(format!("{id}.bin"))).expect("stored");
    assert_ne!(stored, archive);
    assert!(stored.starts_with(b"TURR1"));
    // Nothing in the data directory reads as plain text.
    for directory in ["bundles", "summaries", "reports", "tokens"] {
        for entry in fs::read_dir(fixture.root.join("data").join(directory)).expect("listed") {
            let data = fs::read(entry.expect("an entry").path()).expect("read");
            let text = String::from_utf8_lossy(&data);
            assert!(
                testing::leaks(&text).is_empty(),
                "{directory} holds planted data"
            );
            assert!(
                !text.contains(&token_of(&answer)),
                "{directory} holds the token"
            );
        }
    }
    assert_eq!(fixture.receiver.decrypt(&id).expect("decrypts"), archive);
    assert!(fixture.receiver.decrypt("R-0000000000000000").is_err());
}

#[test]
fn what_is_not_a_good_report_is_refused_with_a_stable_code() {
    let fixture = fixture(
        "refusals",
        json!({ "max_upload_bytes": 4096, "max_unpacked_bytes": 65536 }),
    );
    let good = bundle("t", "TUR-AUDIO-RESUME-001");
    let refused = |content_type: Option<&str>, data: &[u8], status: u16, code: &str| {
        let answer = fixture.receiver.submit(content_type, data);
        assert_eq!(answer.status, status, "{}", answer.body);
        assert_eq!(body(&answer)["code"], code, "{}", answer.body);
        answer
    };
    refused(
        Some("application/octet-stream"),
        &good,
        415,
        "TUR-DIAG-RECEIVER-004",
    );
    refused(None, &good, 415, "TUR-DIAG-RECEIVER-004");
    let answer = refused(
        Some(CONTENT_TYPE),
        b"not a bundle at all",
        422,
        "TUR-DIAG-RECEIVER-005",
    );
    assert_eq!(body(&answer)["detail_code"], "TUR-DIAG-BUNDLE-002");
    // A bundle whose file was changed after it was made.
    let mut members = testing::members(
        Some(&report("t", "TUR-AUDIO-RESUME-001")),
        &[("system/kernel.txt", "x")],
        Profile::Default,
    );
    for (path, data) in &mut members {
        if path == "system/kernel.txt" {
            *data = b"changed".to_vec();
        }
    }
    let answer = refused(
        Some(CONTENT_TYPE),
        &testing::pack_members(&members),
        422,
        "TUR-DIAG-RECEIVER-005",
    );
    assert_eq!(body(&answer)["detail_code"], "TUR-DIAG-BUNDLE-007");
    refused(
        Some(CONTENT_TYPE),
        &testing::bundle(None, &[], Profile::Default),
        422,
        "TUR-DIAG-RECEIVER-006",
    );
    refused(
        Some(CONTENT_TYPE),
        &testing::bundle(Some("{\"schema_version\":1}"), &[], Profile::Default),
        422,
        "TUR-DIAG-RECEIVER-006",
    );
    refused(
        Some(CONTENT_TYPE),
        &testing::bundle(
            Some(&report("t", "TUR-AUDIO-RESUME-001")),
            &[],
            Profile::Technical,
        ),
        422,
        "TUR-DIAG-RECEIVER-007",
    );
    refused(
        Some(CONTENT_TYPE),
        &[0u8; 4097],
        413,
        "TUR-DIAG-RECEIVER-003",
    );
    // Small when sent, too large when unpacked.
    let filler = "a".repeat(70_000);
    let bomb = testing::bundle(
        Some(&report("t", "TUR-AUDIO-RESUME-001")),
        &[("system/filler.txt", filler.as_str())],
        Profile::Default,
    );
    assert!(bomb.len() < 4096);
    let answer = refused(Some(CONTENT_TYPE), &bomb, 413, "TUR-DIAG-RECEIVER-003");
    assert_eq!(body(&answer)["detail_code"], "TUR-DIAG-BUNDLE-009");
    // Nothing was stored.
    assert!(
        fixture
            .receiver
            .store()
            .list("bundles")
            .expect("listed")
            .is_empty()
    );
    // A status token that is not one.
    let upper = "A".repeat(64);
    let zeros = "0".repeat(64);
    for token in ["", "../../etc/passwd", upper.as_str(), zeros.as_str()] {
        let answer = fixture.receiver.status(client(9), token);
        assert_eq!(answer.status, 404);
        assert_eq!(body(&answer)["code"], "TUR-DIAG-RECEIVER-011");
    }
}

#[test]
fn rate_limits_hold_per_address_and_for_everyone() {
    let fixture = fixture(
        "limits",
        json!({ "uploads_per_address_per_hour": 2, "uploads_per_hour": 3, "lookups_per_address_per_hour": 2 }),
    );
    let receiver = &fixture.receiver;
    assert_eq!(receiver.admit_upload(client(1)), None);
    assert_eq!(receiver.admit_upload(client(1)), None);
    let limited = receiver.admit_upload(client(1)).expect("limited");
    assert_eq!(limited.status, 429);
    assert_eq!(body(&limited)["code"], "TUR-DIAG-RECEIVER-008");
    assert!(
        limited
            .retry_after
            .is_some_and(|after| after > 0 && after <= 3600)
    );
    assert_eq!(receiver.admit_upload(client(2)), None);
    let global = receiver.admit_upload(client(3)).expect("limited");
    assert_eq!(body(&global)["code"], "TUR-DIAG-RECEIVER-009");
    // An hour later the allowance is back.
    fixture.clock.advance(3600);
    assert_eq!(receiver.admit_upload(client(1)), None);
    // Status lookups have their own limit.
    let token = "0".repeat(64);
    assert_eq!(receiver.status(client(4), &token).status, 404);
    assert_eq!(receiver.status(client(4), &token).status, 404);
    let answer = receiver.status(client(4), &token);
    assert_eq!(body(&answer)["code"], "TUR-DIAG-RECEIVER-014");
}

#[test]
fn reports_wait_while_the_tracker_is_down_and_are_filed_later() {
    let fixture = default_fixture("down");
    fixture.tracker.set_down(true);
    let answer = submit(&fixture, &bundle("Queued", "TUR-NETWORK-DOWN-001"));
    assert_eq!(answer.status, 202);
    let token = token_of(&answer);
    let second = submit(&fixture, &bundle("Also queued", "TUR-NETWORK-DOWN-002"));
    assert_eq!(second.status, 202);

    let round = fixture.receiver.file_due(&fixture.tracker);
    assert_eq!(
        (round.filed, round.failed),
        (0, 1),
        "the round stops at the first failure"
    );
    assert_eq!(state(&fixture, &token)["state"], "received");
    let log = log_text(&fixture);
    assert!(log.contains("event=filing-failed"), "{log}");
    assert!(log.contains("reason=network"), "{log}");

    // Back up, but the retry is not due yet.
    fixture.tracker.set_down(false);
    fixture.clock.advance(30);
    let round = fixture.receiver.file_due(&fixture.tracker);
    assert_eq!(round.filed, 1, "only the report that never failed is due");
    // After the back-off, the rest.
    fixture.clock.advance(60);
    let round = fixture.receiver.file_due(&fixture.tracker);
    assert_eq!(round.filed, 1);
    assert_eq!(state(&fixture, &token)["state"], "filed");
    assert_eq!(fixture.tracker.calls().len(), 2);
}

#[test]
fn a_deleted_issue_gets_a_new_one() {
    let fixture = default_fixture("gone");
    submit(&fixture, &bundle("First", "TUR-AUDIO-RESUME-001"));
    fixture.receiver.file_due(&fixture.tracker);
    fixture.tracker.delete_issue(1);
    submit(&fixture, &bundle("Second", "TUR-AUDIO-RESUME-001"));
    let round = fixture.receiver.file_due(&fixture.tracker);
    assert_eq!(round.filed, 1);
    let calls = fixture.tracker.calls();
    assert!(
        matches!(calls.last(), Some(Call::Issue { number: 2, .. })),
        "{calls:?}"
    );
    // And later duplicates go to the new issue.
    submit(&fixture, &bundle("Third", "TUR-AUDIO-RESUME-001"));
    fixture.receiver.file_due(&fixture.tracker);
    assert!(matches!(
        fixture.tracker.calls().last(),
        Some(Call::Comment { issue: 2, .. })
    ));
}

#[test]
fn bundles_are_deleted_after_ninety_days_and_records_after_a_year() {
    let fixture = default_fixture("sweep");
    let answer = submit(&fixture, &bundle("Old", "TUR-AUDIO-RESUME-001"));
    let id = body(&answer)["report_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let token = token_of(&answer);
    fixture.receiver.file_due(&fixture.tracker);

    fixture.clock.set(START + 89 * DAY);
    let round = fixture.receiver.sweep().expect("swept");
    assert_eq!((round.bundles_deleted, round.records_deleted), (0, 0));
    assert!(fixture.receiver.decrypt(&id).is_ok());

    fixture.clock.set(START + 90 * DAY);
    let round = fixture.receiver.sweep().expect("swept");
    assert_eq!((round.bundles_deleted, round.records_deleted), (1, 0));
    assert!(
        fixture
            .receiver
            .store()
            .list("bundles")
            .expect("listed")
            .is_empty()
    );
    let error = fixture.receiver.decrypt(&id).expect_err("deleted");
    assert!(error.contains("retention"), "{error}");
    // The state can still be read.
    assert_eq!(state(&fixture, &token)["state"], "filed");
    // A second sweep finds nothing more.
    assert_eq!(fixture.receiver.sweep().expect("swept").bundles_deleted, 0);

    fixture.clock.set(START + 365 * DAY);
    let round = fixture.receiver.sweep().expect("swept");
    assert_eq!(round.records_deleted, 1);
    assert_eq!(fixture.receiver.status(client(5), &token).status, 404);
    for directory in ["reports", "tokens", "bundles", "summaries"] {
        assert!(
            fixture
                .receiver
                .store()
                .list(directory)
                .expect("listed")
                .is_empty(),
            "{directory}"
        );
    }
}

#[test]
fn a_maintainer_marks_an_issue_fixed_and_the_person_sees_it() {
    let fixture = default_fixture("fixed");
    let first = submit(&fixture, &bundle("A", "TUR-AUDIO-RESUME-001"));
    let second = submit(&fixture, &bundle("B", "TUR-AUDIO-RESUME-001"));
    fixture.receiver.file_due(&fixture.tracker);
    assert_eq!(fixture.receiver.mark_fixed(1, "0.2.1").expect("marked"), 2);
    for answer in [&first, &second] {
        assert_eq!(
            state(&fixture, &token_of(answer)),
            json!({"schema_version": 1, "state": "fixed", "fixed_in": "0.2.1"})
        );
    }
    assert!(fixture.receiver.mark_fixed(1, "0.2.1; rm -rf /").is_err());
}

#[test]
fn the_receipt_and_the_state_match_the_published_schema() {
    let fixture = default_fixture("schema");
    let schema: Value =
        serde_json::from_str(include_str!("../../../schemas/report-receiver.schema.json"))
            .expect("JSON");
    let answer = submit(&fixture, &bundle("Schema", "TUR-AUDIO-RESUME-001"));
    let keys = |value: &Value| -> Vec<String> {
        let mut keys: Vec<String> = value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys(&body(&answer)),
        keys(&schema["$defs"]["receipt"]["properties"])
    );
    let state = state(&fixture, &token_of(&answer));
    assert_eq!(keys(&state), keys(&schema["$defs"]["state"]["properties"]));
    let refusal = fixture.receiver.status(client(1), "x");
    assert_eq!(
        keys(&body(&refusal)),
        keys(&schema["$defs"]["error"]["properties"])
    );
    // The client's parser accepts the receipt.
    let receipt: tur_diag::report::Receipt = serde_json::from_str(&answer.body).expect("a receipt");
    assert_eq!(receipt.state, "received");
}
