// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Tests of problem reports on the client side: the report's fields, the
//! bundle it travels in, boot health proposals, the configuration, the local
//! records and the answers of a receiver, given by a fake `curl`.

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;

use serde_json::{Value, json};

use super::*;
use crate::bundle::{build_bundle, pack, unpack};
use crate::host::{CommandOutput, FakeHost};

const EPOCH: u64 = 1_793_000_000;
const RECEIVER: &str = "https://reports.tur.example";
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const REQUEST_SCHEMA: &str = include_str!("../../../../schemas/report-request.schema.json");
const PREVIEW_SCHEMA: &str = include_str!("../../../../schemas/report-preview.schema.json");
const RECORD_SCHEMA: &str = include_str!("../../../../schemas/report-record.schema.json");
const RECEIVER_SCHEMA: &str = include_str!("../../../../schemas/report-receiver.schema.json");

fn request() -> ReportRequest {
    ReportRequest {
        schema_version: REPORT_SCHEMA_VERSION,
        kind: ReportKind::Request,
        title: "The sound stops after resume".to_owned(),
        description: "After the laptop wakes up, alice hears nothing.\nRestarting helps."
            .to_owned(),
        error_code: Some("TUR-AUDIO-RESUME-001".to_owned()),
        component: "audio".to_owned(),
        affected_version: None,
        prepared_by: PreparedBy::Person,
        boot_health: None,
    }
}

fn host() -> FakeHost {
    FakeHost::new()
        .with_command(
            "uname",
            "Linux workstation-7 6.17.1-300.fc45.x86_64 #1 SMP x86_64 GNU/Linux\n",
        )
        .with_file("/proc/sys/kernel/osrelease", "6.17.1-300.fc45.x86_64\n")
        .with_file(
            "/usr/lib/tur/release",
            "TUR_VERSION=0.1.0\nTUR_BUILD_ID=20261112.g0123456789ab\n",
        )
        .with_command(
            "journalctl --boot=0 --output=json --no-pager --lines=5000",
            "{\"MESSAGE\":\"pipewire: suspend for alice on workstation-7\",\"PRIORITY\":\"3\"}\n",
        )
}

fn known() -> KnownIdentifiers {
    KnownIdentifiers {
        usernames: vec!["alice".to_owned()],
        hostnames: vec!["workstation-7".to_owned()],
        machine_ids: Vec::new(),
        preserved_ids: Vec::new(),
    }
}

fn prepared() -> PreparedReport {
    prepare(&host(), &request(), false, EPOCH, known()).expect("the report is prepared")
}

fn reply(body: &str, status: u16) -> CommandOutput {
    CommandOutput {
        exit_code: Some(0),
        stdout: format!("{body}\n{status}").into_bytes(),
        stderr: Vec::new(),
        truncated: false,
    }
}

fn receipt_body() -> String {
    json!({
        "schema_version": 1,
        "report_id": "R-0123456789abcdef",
        "status_token": TOKEN,
        "status_url": format!("{RECEIVER}/v1/reports/{TOKEN}"),
        "state": "received",
        "retention_days": 90
    })
    .to_string()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("tur-diag-report-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a temporary directory");
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn record() -> SentRecord {
    SentRecord {
        schema_version: 1,
        report_id: "R-0123456789abcdef".to_owned(),
        sent_utc: "2026-11-12T10:00:00Z".to_owned(),
        receiver: RECEIVER.to_owned(),
        status_url: format!("{RECEIVER}/v1/reports/{TOKEN}"),
        title: "The sound stops after resume".to_owned(),
        kind: ReportKind::Request,
        component: "audio".to_owned(),
        error_code: None,
        bundle_sha256: "0".repeat(64),
        bundle_bytes: 1234,
        state: "received".to_owned(),
        fixed_in: None,
        state_checked_utc: None,
    }
}

// ------------------------------------------------------------- the fields

/// A change to a request that validation must refuse.
type Change = Box<dyn Fn(&mut ReportRequest)>;

#[test]
fn a_good_request_validates() {
    assert_eq!(request().validate(Strictness::Input), Ok(()));
}

#[test]
fn every_field_rule_is_enforced_and_names_the_field_not_the_value() {
    let secret_title = "x".repeat(MAX_TITLE_CHARS + 1);
    let cases: Vec<(Change, &str)> = vec![
        (Box::new(|r| r.title = "   ".to_owned()), "title is empty"),
        (
            Box::new(move |r| r.title = secret_title.clone()),
            "title is longer",
        ),
        (
            Box::new(|r| r.title = "two\nlines".to_owned()),
            "title has a control",
        ),
        (
            Box::new(|r| r.description = "y".repeat(MAX_DESCRIPTION_CHARS + 1)),
            "description is longer",
        ),
        (
            Box::new(|r| r.description = "bell\u{7}".to_owned()),
            "description has a control",
        ),
        (
            Box::new(|r| r.error_code = Some("OOPS-1".to_owned())),
            "error_code",
        ),
        (
            Box::new(|r| r.error_code = Some("TUR-NOSUCHFAMILY-001".to_owned())),
            "error_code",
        ),
        (Box::new(|r| r.component = "alice".to_owned()), "component"),
        (
            Box::new(|r| r.affected_version = Some("../etc".to_owned())),
            "affected_version",
        ),
        (Box::new(|r| r.schema_version = 2), "schema_version"),
        (
            Box::new(|r| {
                r.boot_health = Some(BootHealthEvidence {
                    notice_kind: "exploded".to_owned(),
                    error_code: None,
                    text: String::new(),
                    failed_check: None,
                    rejected_version: None,
                });
            }),
            "notice_kind",
        ),
    ];
    for (change, expected) in cases {
        let mut report = request();
        change(&mut report);
        let problem = report
            .validate(Strictness::Input)
            .expect_err("the change is refused");
        assert!(problem.contains(expected), "{problem} lacks {expected}");
        assert!(!problem.contains("alice"), "{problem} quotes a value");
        assert!(!problem.contains("xxxx"), "{problem} quotes a value");
    }
}

#[test]
fn received_text_may_be_longer_than_input_by_redaction() {
    let mut report = request();
    report.title = "t".repeat(MAX_TITLE_CHARS + 10);
    assert!(report.validate(Strictness::Input).is_err());
    assert_eq!(report.validate(Strictness::Received), Ok(()));
    report.title = "t".repeat(2 * MAX_TITLE_CHARS + 1);
    assert!(report.validate(Strictness::Received).is_err());
}

#[test]
fn the_signature_groups_by_code_component_and_version_only() {
    let a = signature(Some("TUR-BOOT-GATE-003"), "update", "0.2.0");
    assert_eq!(a.len(), 16);
    assert!(a.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(a, signature(Some("TUR-BOOT-GATE-003"), "update", "0.2.0"));
    assert_ne!(a, signature(Some("TUR-BOOT-GATE-004"), "update", "0.2.0"));
    assert_ne!(a, signature(Some("TUR-BOOT-GATE-003"), "boot", "0.2.0"));
    assert_ne!(a, signature(Some("TUR-BOOT-GATE-003"), "update", "0.2.1"));
    assert_ne!(a, signature(None, "update", "0.2.0"));
}

#[test]
fn the_report_collectors_differ_from_collect_only_in_the_journal_size() {
    let collectors = report_collectors();
    assert_eq!(collectors.len(), COLLECTORS.len());
    for (ours, theirs) in collectors.iter().zip(COLLECTORS) {
        assert_eq!(ours.bundle_path(), theirs.bundle_path());
        assert_eq!(ours.format, theirs.format);
        assert_eq!(ours.previous_boot, theirs.previous_boot);
        assert_eq!(ours.forbidden_path(), None);
        if ours.bundle_path().ends_with("journal.jsonl") {
            assert!(ours.describe_source().ends_with("--lines=5000"), "{ours:?}");
        } else {
            assert_eq!(ours, theirs);
        }
    }
}

// ------------------------------------------------------------- the bundle

#[test]
fn a_prepared_report_carries_its_redacted_report_and_verifies() {
    let prepared = prepared();
    let report = &prepared.inspected.report;
    // The person's text went through the same redaction as the files.
    assert!(
        !report.description.contains("alice"),
        "{}",
        report.description
    );
    assert!(
        report.description.contains("<user-"),
        "{}",
        report.description
    );
    assert_eq!(report.title, request().title);
    assert_eq!(report.error_code, request().error_code);
    assert_eq!(report.component, "audio");

    let members = unpack(&prepared.archive).expect("the bundle unpacks");
    let paths: Vec<&str> = members.iter().map(|(path, _)| path.as_str()).collect();
    assert!(paths.contains(&REPORT_PATH));
    assert_eq!(paths.first(), Some(&MANIFEST_PATH));
    let readme = &members
        .iter()
        .find(|(path, _)| path == "README.txt")
        .expect("a read-me")
        .1;
    assert!(String::from_utf8_lossy(readme).contains("report.json"));
    for (path, data) in &members {
        let text = String::from_utf8_lossy(data);
        assert!(!text.contains("alice"), "{path} names the user");
        assert!(!text.contains("workstation-7"), "{path} names the host");
    }
    assert_eq!(
        prepared.inspected.manifest.redaction_profile,
        Profile::Default
    );
    assert!(prepared.inspected.bytes <= MAX_UPLOAD_BYTES);
    // The same check passes on the archive alone.
    let again = inspect_archive(&prepared.archive).expect("the archive is a report");
    assert_eq!(again.report, *report);
    assert_eq!(again.sha256, prepared.inspected.sha256);
}

#[test]
fn an_invalid_request_is_refused_before_anything_is_collected() {
    let mut bad = request();
    bad.component = "nonsense".to_owned();
    let error = prepare(&FakeHost::new(), &bad, false, EPOCH, known()).expect_err("refused");
    assert_eq!(error.kind(), ErrorKind::ReportInvalid);
    assert_eq!(error.code().to_string(), "TUR-DIAG-REPORT-001");
}

#[test]
fn what_is_not_a_prepared_report_is_refused() {
    // An ordinary bundle: no report.json.
    let plain = build_bundle(
        &host(),
        &CollectRequest {
            profile: Profile::Default,
            since_boot: false,
            created_epoch_seconds: EPOCH,
            collectors: COLLECTORS,
            known: known(),
            max_bytes: 1 << 20,
        },
    )
    .expect("a bundle");
    let error = inspect_archive(&plain.archive).expect_err("no report.json");
    assert_eq!(error.kind(), ErrorKind::ReportNotPrepared);
    assert!(error.detail().contains("report.json"), "{}", error.detail());

    // A report made with the technical profile.
    let line = serde_json::to_string(&request()).expect("JSON");
    let technical = build_bundle_with(
        &host(),
        &CollectRequest {
            profile: Profile::Technical,
            since_boot: false,
            created_epoch_seconds: EPOCH,
            collectors: COLLECTORS,
            known: known(),
            max_bytes: 1 << 20,
        },
        &[ExtraDocument {
            path: REPORT_PATH.to_owned(),
            format: Format::JsonLines,
            text: line,
        }],
    )
    .expect("a bundle");
    let error = inspect_archive(&technical.archive).expect_err("technical profile");
    assert_eq!(error.kind(), ErrorKind::ReportNotPrepared);
    assert!(error.detail().contains("technical"), "{}", error.detail());

    // A damaged bundle carries the bundle's own code in the detail.
    let mut damaged = prepared().archive;
    let middle = damaged.len() / 2;
    damaged[middle] ^= 0xff;
    let error = inspect_archive(&damaged).expect_err("damaged");
    assert_eq!(error.kind(), ErrorKind::ReportNotPrepared);
    assert!(
        error.detail().starts_with("TUR-DIAG-BUNDLE-"),
        "{}",
        error.detail()
    );

    // A report.json that is not a report.
    let mut members = unpack(&prepared().archive).expect("unpacks");
    for (path, data) in &mut members {
        if path == REPORT_PATH {
            *data = b"{\"schema_version\":1}\n".to_vec();
        }
    }
    let error = verify_then_read(&members);
    assert!(error.contains("report.json"), "{error}");
}

fn verify_then_read(members: &[Member]) -> String {
    read_report(members).expect_err("not a report")
}

#[test]
fn a_bundle_over_the_size_limits_is_refused() {
    let big = vec![0u8; usize::try_from(MAX_UPLOAD_BYTES).unwrap() + 1];
    let error = inspect_archive(&big).expect_err("too big");
    assert_eq!(error.kind(), ErrorKind::ReportTooLarge);
    assert_eq!(error.code().to_string(), "TUR-DIAG-REPORT-012");

    // Small when compressed, larger than the limit when expanded.
    let filler = vec![b'a'; usize::try_from(MAX_UNPACKED_BYTES).unwrap() + 1024];
    let archive = pack(&[("filler.txt".to_owned(), filler)], EPOCH).expect("packs");
    assert!(u64::try_from(archive.len()).unwrap() < MAX_UPLOAD_BYTES);
    let error = inspect_archive(&archive).expect_err("expands too far");
    assert_eq!(error.kind(), ErrorKind::ReportTooLarge);
}

#[test]
fn an_extra_document_cannot_replace_a_bundle_member() {
    for path in [
        "manifest.json",
        "checksums.sha256",
        "system/kernel.txt",
        "../x",
        "/abs",
    ] {
        let result = build_bundle_with(
            &host(),
            &CollectRequest {
                profile: Profile::Default,
                since_boot: false,
                created_epoch_seconds: EPOCH,
                collectors: COLLECTORS,
                known: known(),
                max_bytes: 1 << 20,
            },
            &[ExtraDocument {
                path: path.to_owned(),
                format: Format::Text,
                text: "x".to_owned(),
            }],
        );
        let error = result.expect_err(path);
        assert_eq!(error.kind(), ErrorKind::Internal, "{path}");
    }
}

#[test]
fn the_preview_shows_every_file_and_the_report() {
    let prepared = prepared();
    let preview = preview(&prepared, None, None);
    assert!(!preview.sent);
    assert_eq!(preview.prepared_file, None);
    assert_eq!(preview.bundle.files.len(), prepared.inspected.members.len());
    assert!(
        preview
            .bundle
            .files
            .iter()
            .any(|file| file.path == REPORT_PATH)
    );
    let text = render_preview(&preview);
    assert!(text.contains("NOT sent"));
    assert!(text.contains("none configured"));
    assert!(text.contains("Preview only: nothing was saved."));
    assert!(text.contains(&prepared.inspected.report.title));
    for file in &preview.bundle.files {
        assert!(text.contains(&file.path), "{} is not shown", file.path);
    }
    let with_file = render_preview(&super::preview(
        &prepared,
        Some(RECEIVER),
        Some(Path::new("/tmp/r.tar.zst")),
    ));
    assert!(
        with_file.contains("tur-diag report send --to https://reports.tur.example /tmp/r.tar.zst")
    );
}

// ------------------------------------------------------------ boot health

fn boot_health_status(notice: Value) -> String {
    json!({
        "schema_version": 1,
        "boot_id": "0f0e0d0c-0b0a-4909-8807-060504030201",
        "build_id": "20261112.g0123456789ab",
        "boot": null,
        "verdict": null,
        "counting": {},
        "watchdog": null,
        "trial": null,
        "rejected": {
            "schema_version": 1,
            "images": [
                {"digest": "sha256:aa", "image": "ghcr.io/x", "version": "0.2.0", "event_id": "ev-1"},
                {"digest": "sha256:bb", "image": "ghcr.io/x", "version": "0.1.9", "event_id": "ev-0"}
            ]
        },
        "notice": notice,
        "recent_events": [],
        "something_new": true
    })
    .to_string()
}

#[test]
fn a_restored_version_becomes_a_failed_update_report() {
    let status = boot_health_status(json!({
        "schema_version": 1,
        "event_id": "ev-1",
        "kind": "fallback",
        "text": "The update did not start correctly. Your previous version was restored.",
        "error_code": "TUR-BOOT-GATE-003",
        "boot_id": "x",
        "acknowledged": false,
        "details": {"failed_check": "R4"}
    }));
    let report = request_from_boot_health(&status, PreparedBy::Pax).expect("a proposal");
    assert_eq!(report.kind, ReportKind::FailedUpdate);
    assert_eq!(report.component, "update");
    assert_eq!(report.error_code.as_deref(), Some("TUR-BOOT-GATE-003"));
    assert_eq!(report.affected_version.as_deref(), Some("0.2.0"));
    assert_eq!(report.prepared_by, PreparedBy::Pax);
    let evidence = report.boot_health.as_ref().expect("evidence");
    assert_eq!(evidence.notice_kind, "fallback");
    assert_eq!(evidence.failed_check.as_deref(), Some("R4"));
    assert_eq!(evidence.rejected_version.as_deref(), Some("0.2.0"));
    assert!(report.description.contains("R4"));
    assert_eq!(report.validate(Strictness::Input), Ok(()));

    // The proposal can be prepared as it stands.
    let prepared = prepare(&host(), &report, false, EPOCH, known()).expect("prepared");
    assert_eq!(prepared.inspected.report.kind, ReportKind::FailedUpdate);
}

#[test]
fn boot_health_without_a_notice_has_nothing_to_report() {
    let status = boot_health_status(Value::Null);
    let error = request_from_boot_health(&status, PreparedBy::Person).expect_err("nothing");
    assert_eq!(error.kind(), ErrorKind::ReportNothingFromBootHealth);
    let error = request_from_boot_health("not json", PreparedBy::Person).expect_err("nothing");
    assert_eq!(error.kind(), ErrorKind::ReportNothingFromBootHealth);
    let status = boot_health_status(json!({"kind": "party", "text": ""}));
    assert!(request_from_boot_health(&status, PreparedBy::Person).is_err());
}

#[test]
fn an_unhealthy_start_is_a_boot_problem_and_bad_values_are_dropped() {
    let status = boot_health_status(json!({
        "kind": "unhealthy",
        "event_id": "ev-9",
        "text": "Some services failed.\u{7}",
        "error_code": "not a code",
        "details": {"failed_check": "R4; rm -rf /"}
    }));
    let report = request_from_boot_health(&status, PreparedBy::Person).expect("a proposal");
    assert_eq!(report.kind, ReportKind::BootProblem);
    assert_eq!(report.component, "boot");
    assert_eq!(report.error_code, None);
    assert_eq!(report.affected_version, None);
    let evidence = report.boot_health.as_ref().expect("evidence");
    assert_eq!(evidence.failed_check, None);
    assert!(!evidence.text.contains('\u{7}'));
    assert_eq!(report.validate(Strictness::Input), Ok(()));
}

#[test]
fn boot_health_is_read_with_tur_boot_health() {
    let host = FakeHost::new().with_command("tur-boot-health status --json", "{}");
    assert_eq!(read_boot_health(&host).expect("read"), "{}");
    let error = read_boot_health(&FakeHost::new()).expect_err("not installed");
    assert_eq!(error.kind(), ErrorKind::ReportNothingFromBootHealth);
}

// ---------------------------------------------------------- configuration

#[test]
fn the_receiver_is_empty_by_default_and_must_be_https() {
    assert_eq!(parse_config(""), Ok(None));
    assert_eq!(parse_config("# no receiver yet\nRECEIVER_URL=\n"), Ok(None));
    assert_eq!(
        parse_config("RECEIVER_URL=https://reports.tur.example/\n"),
        Ok(Some(RECEIVER.to_owned()))
    );
    assert_eq!(
        parse_config("RECEIVER_URL=\"https://reports.tur.example:8443/tur\"\n"),
        Ok(Some("https://reports.tur.example:8443/tur".to_owned()))
    );
    assert!(parse_config("RECEIVER_URL=http://reports.tur.example\n").is_err());
    assert!(parse_config("SEND_AUTOMATICALLY=yes\n").is_err());
    assert!(parse_config("just text\n").is_err());

    for good in [
        "https://reports.tur.example",
        "http://127.0.0.1:8080",
        "http://localhost:8080",
        "http://[::1]:8080",
        "https://[2001:db8::1]:443/x",
    ] {
        assert!(normalise_receiver_url(good).is_ok(), "{good}");
    }
    for bad in [
        "reports.tur.example",
        "ftp://reports.tur.example",
        "http://192.0.2.1",
        "https://user:pw@reports.tur.example",
        "https://reports.tur.example/?q=1",
        "https://reports.tur.example/#x",
        "https://reports tur.example",
        "https://:443",
        "https://reports.tur.example:port",
        "--config=/etc/passwd",
    ] {
        assert!(normalise_receiver_url(bad).is_err(), "{bad}");
    }
}

#[test]
fn an_explicit_configuration_file_must_exist() {
    let dir = TempDir::new("config");
    let missing = dir.0.join("missing.conf");
    let error = load_config(Some(&missing)).expect_err("missing");
    assert_eq!(error.kind(), ErrorKind::ReportConfigInvalid);
    let file = dir.0.join("reports.conf");
    fs::write(&file, "RECEIVER_URL=http://127.0.0.1:8080/\n").expect("written");
    let config = load_config(Some(&file)).expect("read");
    assert_eq!(
        config.receiver_url.as_deref(),
        Some("http://127.0.0.1:8080")
    );
    assert_eq!(config.source.as_deref(), Some(file.as_path()));
}

// ---------------------------------------------------------------- records

#[test]
fn records_are_private_and_the_list_never_shows_the_status_token() {
    let dir = TempDir::new("records");
    let path = write_record(&dir.0, &record()).expect("written");
    let mode = fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let directory_mode = fs::metadata(dir.0.join(SENT_DIR))
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(directory_mode, 0o700);

    let mut later = record();
    later.report_id = "R-fedcba9876543210".to_owned();
    later.sent_utc = "2026-11-12T11:00:00Z".to_owned();
    write_record(&dir.0, &later).expect("written");
    fs::write(dir.0.join(SENT_DIR).join("broken.json"), "{").expect("written");
    fs::create_dir_all(dir.0.join(PREPARED_DIR)).expect("created");
    fs::write(dir.0.join(PREPARED_DIR).join("report-1-abc.tar.zst"), "zz").expect("written");

    let list = list_reports(Some(&dir.0)).expect("listed");
    assert!(list.directory_found);
    assert_eq!(list.unreadable_records, 1);
    let ids: Vec<&str> = list.sent.iter().map(|r| r.report_id.as_str()).collect();
    assert_eq!(ids, ["R-fedcba9876543210", "R-0123456789abcdef"]);
    assert_eq!(list.prepared.len(), 1);
    assert_eq!(list.prepared[0].name, "report-1-abc.tar.zst");
    let shown = serde_json::to_string(&list).expect("JSON") + &render_list(&list);
    assert!(!shown.contains(TOKEN), "the status token is listed");
    assert!(!shown.contains("status_url"));
    assert!(
        !shown.contains(&dir.0.display().to_string()),
        "the directory is listed"
    );

    let nowhere = list_reports(Some(&dir.0.join("absent"))).expect("listed");
    assert!(!nowhere.directory_found);
    assert!(render_list(&nowhere).contains("No problem reports"));
    assert!(!list_reports(None).expect("listed").directory_found);
}

#[test]
fn a_record_with_a_malformed_identifier_is_not_written() {
    let dir = TempDir::new("bad-record");
    let mut bad = record();
    bad.report_id = "../../escape".to_owned();
    assert!(write_record(&dir.0, &bad).is_err());
    assert!(!dir.0.join("escape.json").exists());
}

// ---------------------------------------------------------------- sending

#[test]
fn a_receipt_is_accepted_only_from_the_receiver_it_was_sent_to() {
    let dir = TempDir::new("send");
    let file = dir.0.join("r.tar.zst");
    fs::write(&file, b"bundle").expect("written");

    let host = FakeHost::new().with_command_result("curl", Ok(reply(&receipt_body(), 202)));
    let receipt = send_prepared(&host, RECEIVER, &file).expect("a receipt");
    assert_eq!(receipt.report_id, "R-0123456789abcdef");
    assert_eq!(receipt.state, "received");

    // The same answer, from a receiver configured at another address.
    let error = send_prepared(&host, "https://elsewhere.example", &file).expect_err("refused");
    assert_eq!(error.kind(), ErrorKind::ReportUnreachable);

    let mut body: Value = serde_json::from_str(&receipt_body()).expect("JSON");
    body["status_token"] = json!("short");
    let host = FakeHost::new().with_command_result("curl", Ok(reply(&body.to_string(), 202)));
    assert!(send_prepared(&host, RECEIVER, &file).is_err());
}

#[test]
fn the_upload_is_one_curl_call_with_the_bundle_as_its_body() {
    let file = Path::new("/tmp/r.tar.zst");
    let line = format!(
        "curl --silent --show-error --proto =https --max-redirs 0 --connect-timeout 20 \
         --max-time 300 --user-agent tur-diag/{} --header Accept: application/json \
         --write-out \\n%{{http_code}} --request POST --header Content-Type: {CONTENT_TYPE} \
         --data-binary @/tmp/r.tar.zst {RECEIVER}/v1/reports",
        crate::manifest::COLLECTOR_VERSION
    );
    let host = FakeHost::new().with_command_result(&line, Ok(reply(&receipt_body(), 202)));
    // Only the exact command line answers: anything else is "not installed".
    send_prepared(&host, RECEIVER, file).expect("the exact command line was used");
}

#[test]
fn refusals_and_failures_carry_their_codes() {
    let file = Path::new("/tmp/r.tar.zst");
    let refusal = json!({
        "schema_version": 1,
        "code": "TUR-DIAG-RECEIVER-008",
        "message": "Too many reports from this address; try again later.",
        "detail_code": null
    })
    .to_string();
    let host = FakeHost::new().with_command_result("curl", Ok(reply(&refusal, 429)));
    let error = send_prepared(&host, RECEIVER, file).expect_err("refused");
    assert_eq!(error.kind(), ErrorKind::ReportRefused);
    assert!(
        error.detail().contains("TUR-DIAG-RECEIVER-008"),
        "{}",
        error.detail()
    );
    assert!(error.detail().contains("429"));

    let host =
        FakeHost::new().with_command_result("curl", Ok(reply("<html>bad gateway</html>", 502)));
    let error = send_prepared(&host, RECEIVER, file).expect_err("not a receiver");
    assert_eq!(error.kind(), ErrorKind::ReportUnreachable);

    let host = FakeHost::new().with_failing_command("curl", 7, "curl: (7) Failed to connect");
    let error = send_prepared(&host, RECEIVER, file).expect_err("unreachable");
    assert_eq!(error.kind(), ErrorKind::ReportUnreachable);
    assert!(error.detail().contains("Failed to connect"));

    let error = send_prepared(&FakeHost::new(), RECEIVER, file).expect_err("no curl");
    assert_eq!(error.kind(), ErrorKind::ReportUnreachable);
    assert!(error.detail().contains("curl is not installed"));
}

#[test]
fn the_state_is_read_from_the_recorded_address_only() {
    let state = json!({"schema_version": 1, "state": "fixed", "fixed_in": "0.2.1"}).to_string();
    let host = FakeHost::new().with_command_result("curl", Ok(reply(&state, 200)));
    let read = fetch_state(&host, &record()).expect("a state");
    assert_eq!(read.state, "fixed");
    assert_eq!(read.fixed_in.as_deref(), Some("0.2.1"));

    let mut tampered = record();
    tampered.status_url = "--output=/home/alice/.bashrc".to_owned();
    let error = fetch_state(&host, &tampered).expect_err("refused");
    assert_eq!(error.kind(), ErrorKind::ReportRecordsUnavailable);

    let odd = json!({"schema_version": 1, "state": "lost", "fixed_in": null}).to_string();
    let host = FakeHost::new().with_command_result("curl", Ok(reply(&odd, 200)));
    assert!(fetch_state(&host, &record()).is_err());
}

// ---------------------------------------------------------------- schemas

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect()
}

fn required(node: &Value) -> BTreeSet<String> {
    node["required"]
        .as_array()
        .expect("required")
        .iter()
        .map(|item| item.as_str().expect("a string").to_owned())
        .collect()
}

/// The schema node and the Rust value have exactly the same keys, every key
/// is required, and nothing else is allowed.
fn same_shape(node: &Value, instance: &Value, what: &str) {
    assert_eq!(node["additionalProperties"], false, "{what}");
    assert_eq!(
        keys(&node["properties"]),
        keys(instance),
        "{what}: properties"
    );
    assert_eq!(required(node), keys(instance), "{what}: required");
}

fn enum_strings(node: &Value) -> BTreeSet<String> {
    node["enum"]
        .as_array()
        .expect("an enum")
        .iter()
        .map(|item| item.as_str().expect("a string").to_owned())
        .collect()
}

#[test]
fn the_report_schema_and_the_rust_type_agree() {
    let schema: Value = serde_json::from_str(REQUEST_SCHEMA).expect("JSON");
    let mut report = request();
    report.boot_health = Some(BootHealthEvidence {
        notice_kind: "fallback".to_owned(),
        error_code: None,
        text: String::new(),
        failed_check: None,
        rejected_version: None,
    });
    let instance = serde_json::to_value(&report).expect("JSON");
    same_shape(&schema, &instance, "report");
    same_shape(
        &schema["properties"]["boot_health"],
        &instance["boot_health"],
        "boot_health",
    );
    let kinds: BTreeSet<String> = ReportKind::ALL
        .iter()
        .map(|kind| kind.as_str().to_owned())
        .collect();
    assert_eq!(enum_strings(&schema["properties"]["kind"]), kinds);
    for kind in ReportKind::ALL {
        assert_eq!(serde_json::to_value(kind).expect("JSON"), kind.as_str());
    }
    let components: BTreeSet<String> = COMPONENTS.iter().map(|c| (*c).to_owned()).collect();
    assert_eq!(enum_strings(&schema["properties"]["component"]), components);
    let notices: BTreeSet<String> = NOTICE_KINDS.iter().map(|c| (*c).to_owned()).collect();
    assert_eq!(
        enum_strings(&schema["properties"]["boot_health"]["properties"]["notice_kind"]),
        notices
    );
    assert_eq!(
        schema["properties"]["title"]["maxLength"],
        json!(MAX_TITLE_CHARS)
    );
    assert_eq!(
        schema["properties"]["description"]["maxLength"],
        json!(MAX_DESCRIPTION_CHARS)
    );
}

#[test]
fn the_preview_schema_and_the_rust_type_agree() {
    let schema: Value = serde_json::from_str(PREVIEW_SCHEMA).expect("JSON");
    let instance = serde_json::to_value(preview(&prepared(), Some(RECEIVER), None)).expect("JSON");
    same_shape(&schema, &instance, "preview");
    same_shape(
        &schema["properties"]["bundle"],
        &instance["bundle"],
        "bundle",
    );
    same_shape(
        &schema["properties"]["bundle"]["properties"]["files"]["items"],
        &instance["bundle"]["files"][0],
        "files[]",
    );
}

#[test]
fn the_record_schema_and_the_rust_types_agree() {
    let schema: Value = serde_json::from_str(RECORD_SCHEMA).expect("JSON");
    let instance = serde_json::to_value(record()).expect("JSON");
    same_shape(&schema, &instance, "record");
    let list = ReportList {
        schema_version: 1,
        directory_found: true,
        sent: vec![ListedReport::from(&record())],
        prepared: vec![ListedPrepared {
            name: "x.tar.zst".to_owned(),
            bytes: 1,
        }],
        unreadable_records: 0,
    };
    let list_value = serde_json::to_value(&list).expect("JSON");
    let list_schema = &schema["$defs"]["list"];
    same_shape(list_schema, &list_value, "list");
    same_shape(
        &list_schema["properties"]["sent"]["items"],
        &list_value["sent"][0],
        "list.sent[]",
    );
    same_shape(
        &list_schema["properties"]["prepared"]["items"],
        &list_value["prepared"][0],
        "list.prepared[]",
    );
    assert!(!keys(&list_value["sent"][0]).contains("status_url"));
}

#[test]
fn the_receiver_schema_and_the_client_types_agree() {
    let schema: Value = serde_json::from_str(RECEIVER_SCHEMA).expect("JSON");
    let receipt: Receipt = serde_json::from_str(&receipt_body()).expect("a receipt");
    same_shape(
        &schema["$defs"]["receipt"],
        &serde_json::to_value(&receipt).expect("JSON"),
        "receipt",
    );
    let state = ReportState {
        schema_version: 1,
        state: "filed".to_owned(),
        fixed_in: None,
    };
    same_shape(
        &schema["$defs"]["state"],
        &serde_json::to_value(&state).expect("JSON"),
        "state",
    );
}
