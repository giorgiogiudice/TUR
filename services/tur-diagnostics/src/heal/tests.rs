// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The healing loop on a fake machine: every check kind, every remedy kind
//! and its undo, the rules a request is held to, the link from a fix in an
//! update back to the fault, and the notices.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::catalogue::{BUILT_IN, Catalogue, Check, ReportPolicy};
use super::engine::{
    ApplyRequest, Fix, Fixes, Paths, Running, apply, handle_request, notices, scan, status_view,
    uid_of_connection, undo,
};
use super::machine::{FakeEntry, FakeMachine, exited, printed};
use super::state::{Actor, EventKind, FaultState, HealState};
use crate::error::ErrorKind;

const NOW: u64 = 1_800_000_000;
const BOOT: u64 = 1_799_990_000;
const SERVICE: &str = "TUR-DESKTOP-SERVICE-001";
const TIME: &str = "TUR-NETWORK-TIME-001";
const RESOLVER: &str = "TUR-NETWORK-RESOLVER-001";
const SPACE: &str = "TUR-STORAGE-SPACE-001";
const CRASH: &str = "TUR-DIAG-CRASH-001";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("tur-diag-heal-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a scratch directory");
        TempDir(path)
    }

    fn paths(&self) -> Paths {
        Paths {
            state: self.0.join("log/state.json"),
            data_dir: self.0.join("data"),
            fixes: self.0.join("fixes.json"),
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn catalogue() -> Catalogue {
    Catalogue::built_in().expect("the built-in catalogue is valid")
}

fn running(build: &str) -> Running {
    Running {
        build_id: Some(build.to_owned()),
        version: Some("0.9.0".to_owned()),
        boot_id: Some("boot-1".to_owned()),
    }
}

fn units_line() -> String {
    let catalogue = catalogue();
    let Check::UnitFailed { units } = &catalogue.fault(SERVICE).expect("service").check else {
        panic!("the service fault watches units");
    };
    format!("systemctl is-failed --no-pager {}", units.join(" "))
}

fn unit_states(failed: &str) -> String {
    let catalogue = catalogue();
    let Check::UnitFailed { units } = &catalogue.fault(SERVICE).expect("service").check else {
        panic!("the service fault watches units");
    };
    units
        .iter()
        .map(|unit| {
            if unit == failed {
                "failed\n"
            } else {
                "active\n"
            }
        })
        .collect()
}

const TIME_LINE: &str = "timedatectl show --property=NTP --property=CanNTP";
const RESOLVED_LINE: &str = "systemctl is-active systemd-resolved.service";
const DF_LINE: &str = "df --output=avail,size -B1 /var";

fn crash_line() -> String {
    format!("coredumpctl list --json=short --no-pager --quiet --since=@{BOOT}")
}

fn base() -> FakeMachine {
    FakeMachine::new(NOW).with_file("/proc/stat", &format!("cpu 1 2 3\nbtime {BOOT}\n"), 0o444)
}

fn request(code: &str, remedy: &str, actor: Actor, confirmed: bool) -> ApplyRequest {
    ApplyRequest {
        code: code.to_owned(),
        remedy: remedy.to_owned(),
        actor,
        uid: Some(1000),
        confirmed,
    }
}

fn empty_state() -> HealState {
    HealState {
        schema_version: 1,
        next_event: 1,
        ..HealState::default()
    }
}

// ---------------------------------------------------------------- catalogue

#[test]
fn the_built_in_catalogue_is_valid_and_covers_every_kind() {
    let catalogue = catalogue();
    let codes: Vec<&str> = catalogue
        .faults
        .iter()
        .map(|fault| fault.code.as_str())
        .collect();
    assert_eq!(codes, [SERVICE, TIME, RESOLVER, SPACE, CRASH]);
    let kinds: BTreeSet<&str> = catalogue
        .faults
        .iter()
        .map(|fault| fault.check.kind_name())
        .collect();
    assert_eq!(kinds.len(), 5);
    // At least one fault ends as an offered report with no remedy at all.
    assert!(
        catalogue
            .faults
            .iter()
            .any(|fault| fault.remedies.is_empty() && fault.report == ReportPolicy::Always)
    );
    // Every automatic remedy is reversible and at most class 1; every
    // irreversible one is class 2.
    for fault in &catalogue.faults {
        for remedy in &fault.remedies {
            if remedy.automatic {
                assert!(remedy.class <= 1 && remedy.reversible, "{}", remedy.id);
            }
            if !remedy.reversible {
                assert_eq!(remedy.class, 2, "{}", remedy.id);
            }
        }
    }
}

#[test]
fn the_validator_refuses_what_the_design_forbids() {
    let refused = |change: &dyn Fn(&mut Value), expected: &str| {
        let mut value: Value = serde_json::from_str(BUILT_IN).expect("JSON");
        change(&mut value);
        let catalogue: Catalogue = serde_json::from_value(value).expect("still a catalogue");
        let problems = catalogue.validate().join("\n");
        assert!(
            problems.contains(expected),
            "{expected:?} not in:\n{problems}"
        );
    };
    // An irreversible remedy may not be automatic, nor class 1.
    refused(
        &|value| value["faults"][3]["remedies"][0]["automatic"] = json!(true),
        "only a reversible remedy of class 0 or 1 may be automatic",
    );
    refused(
        &|value| value["faults"][3]["remedies"][0]["class"] = json!(1),
        "must be reversible",
    );
    // Nothing system-critical is a catalogue remedy.
    refused(
        &|value| value["faults"][1]["remedies"][0]["class"] = json!(3),
        "is never a catalogue remedy",
    );
    // A remedy answers the kind of check it was made for.
    refused(
        &|value| value["faults"][1]["remedies"][0]["kind"] = json!("vacuum-journal"),
        "does not answer",
    );
    // A fault without a remedy ends as an offered report.
    refused(
        &|value| value["faults"][4]["report"] = json!("never"),
        "must offer a report",
    );
    // Codes parse and are not tur-diag's own.
    refused(
        &|value| value["faults"][0]["code"] = json!("TUR-NOPE-1"),
        "does not parse",
    );
    refused(
        &|value| value["faults"][0]["code"] = json!("TUR-DIAG-HEAL-001"),
        "tur-diag's own errors",
    );
    refused(
        &|value| value["faults"][0]["component"] = json!("kitchen"),
        "not a report component",
    );
    refused(
        &|value| value["faults"][0]["sources"] = json!([]),
        "no source",
    );
    refused(
        &|value| value["faults"][2]["check"]["restore_target"] = json!("/tmp/x"),
        "restore_target",
    );
}

#[test]
fn the_catalogue_has_the_fields_its_schema_describes() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../../schemas/heal-catalogue.schema.json"
    ))
    .expect("the schema is JSON");
    let catalogue: Value = serde_json::from_str(BUILT_IN).expect("JSON");
    let top: BTreeSet<&String> = schema["properties"]
        .as_object()
        .expect("properties")
        .keys()
        .collect();
    assert_eq!(top, catalogue.as_object().expect("object").keys().collect());
    let fault_fields = schema["$defs"]["fault"]["properties"]
        .as_object()
        .expect("fault");
    let remedy_fields = schema["$defs"]["remedy"]["properties"]
        .as_object()
        .expect("remedy");
    for fault in catalogue["faults"].as_array().expect("faults") {
        for name in fault.as_object().expect("object").keys() {
            assert!(fault_fields.contains_key(name), "fault field {name}");
        }
        for remedy in fault["remedies"].as_array().expect("remedies") {
            for name in remedy.as_object().expect("object").keys() {
                assert!(remedy_fields.contains_key(name), "remedy field {name}");
            }
        }
    }
}

// ----------------------------------------------------------- the service

#[test]
fn a_failed_service_is_restarted_once_by_the_service_and_the_repair_is_checked_to_hold() {
    let dir = TempDir::new("service");
    let paths = dir.paths();
    let machine = base()
        .with_command(&units_line(), exited(0, &unit_states("chronyd.service")))
        .with_command("systemctl reset-failed chronyd.service", printed(""))
        .with_command("systemctl start chronyd.service", printed(""))
        .with_effect(
            "systemctl start chronyd.service",
            &units_line(),
            exited(1, &unit_states("none")),
        );
    let catalogue = catalogue();
    let mut state = empty_state();
    let lines = scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &Fixes::default(),
        true,
    );
    let line = lines
        .iter()
        .find(|line| line.code == SERVICE)
        .expect("a line");
    assert!(line.present);
    assert_eq!(line.state, Some(FaultState::Repaired), "{}", line.action);
    let ran = machine.ran();
    let reset = ran
        .iter()
        .position(|line| line == "systemctl reset-failed chronyd.service");
    let start = ran
        .iter()
        .position(|line| line == "systemctl start chronyd.service");
    assert!(reset.is_some() && start > reset, "{ran:?}");
    let record = &state.faults[state.active(SERVICE).expect("a record")];
    assert_eq!(record.applied.len(), 1);
    assert_eq!(record.applied[0].by, Actor::Service);
    assert_eq!(record.applied[0].outcome, "repaired");
    // No "found" notice for a fault the service repairs by itself, a
    // "repaired" one instead.
    let kinds: Vec<EventKind> = state.events.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, [EventKind::Repaired]);

    // The next scan finds it still running: the repair held.
    scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &Fixes::default(),
        true,
    );
    assert!(state.active(SERVICE).is_none());
    let record = &state.faults[state.latest(SERVICE).expect("a record")];
    assert_eq!(record.resolved_how.as_deref(), Some("repair-held"));
    assert_eq!(
        state.events.last().map(|event| event.kind),
        Some(EventKind::Held)
    );
}

#[test]
fn a_service_that_fails_again_is_not_restarted_twice_in_one_boot_and_a_report_is_offered() {
    let dir = TempDir::new("service-again");
    let paths = dir.paths();
    let machine = base()
        .with_command(&units_line(), exited(0, &unit_states("cups.service")))
        .with_command("systemctl reset-failed cups.service", printed(""))
        .with_command("systemctl start cups.service", exited(1, ""));
    let catalogue = catalogue();
    let mut state = empty_state();
    for _ in 0..3 {
        machine.advance(60);
        scan(
            &machine,
            &catalogue,
            &paths,
            &mut state,
            &running("b1"),
            &Fixes::default(),
            true,
        );
    }
    let starts = machine
        .ran()
        .iter()
        .filter(|line| *line == "systemctl start cups.service")
        .count();
    assert_eq!(starts, 1);
    let record = &state.faults[state.active(SERVICE).expect("a record")];
    assert_eq!(record.state, FaultState::ReportOffered);
    assert_eq!(record.applied[0].outcome, "failed");
    assert!(record.report_offered);
}

// ------------------------------------------------------------- network time

#[test]
fn network_time_waits_for_a_person_and_a_request_repairs_and_undoes_it() {
    let dir = TempDir::new("time");
    let paths = dir.paths();
    let machine = base()
        .with_command(TIME_LINE, printed("NTP=no\nCanNTP=yes\n"))
        .with_command("timedatectl set-ntp true", printed(""))
        .with_effect(
            "timedatectl set-ntp true",
            TIME_LINE,
            printed("NTP=yes\nCanNTP=yes\n"),
        )
        .with_command("timedatectl set-ntp false", printed(""))
        .with_effect(
            "timedatectl set-ntp false",
            TIME_LINE,
            printed("NTP=no\nCanNTP=yes\n"),
        );
    let catalogue = catalogue();
    let mut state = empty_state();
    scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &Fixes::default(),
        true,
    );
    assert!(!machine.ran().iter().any(|line| line.contains("set-ntp")));
    let index = state.active(TIME).expect("a record");
    assert_eq!(state.faults[index].state, FaultState::Present);
    assert_eq!(state.events[0].kind, EventKind::Detected);

    let outcome = apply(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &request(TIME, "enable-network-time", Actor::Request, false),
    )
    .expect("applied");
    assert_eq!(outcome.outcome, "repaired");
    assert_eq!(outcome.state, FaultState::Repaired);
    let record = &state.faults[state.active(TIME).expect("a record")];
    assert_eq!(record.applied[0].by, Actor::Request);
    assert_eq!(record.applied[0].uid, Some(1000));

    let undone = undo(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        TIME,
        "enable-network-time",
        Actor::Request,
        Some(1000),
    )
    .expect("undone");
    assert_eq!(undone.outcome, "undone");
    assert!(
        machine
            .ran()
            .contains(&"timedatectl set-ntp false".to_owned())
    );
    let record = &state.faults[state.active(TIME).expect("a record")];
    assert!(record.automatic_suppressed);
    assert!(record.applied[0].undone_utc.is_some());
    // Undone once; there is nothing left to undo.
    let again = undo(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        TIME,
        "enable-network-time",
        Actor::Request,
        None,
    )
    .expect_err("nothing to undo");
    assert_eq!(again.kind(), ErrorKind::HealNothingToUndo);
}

// ------------------------------------------------------------- the resolver

#[test]
fn the_resolver_link_is_restored_and_the_kept_copy_puts_back_exactly_what_was_there() {
    let dir = TempDir::new("resolver");
    let paths = dir.paths();
    let original = "# written by a VPN\nnameserver 192.0.2.53\nsearch example.org\n";
    let machine = base()
        .with_command(RESOLVED_LINE, printed("active\n"))
        .with_file("/etc/resolv.conf", original, 0o640);
    let catalogue = catalogue();
    let mut state = empty_state();
    scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &Fixes::default(),
        true,
    );
    let index = state.active(RESOLVER).expect("found");
    // The evidence says how many servers, never which.
    let evidence = state.faults[index].evidence.join(" ");
    assert!(evidence.contains("1 name server"), "{evidence}");
    assert!(!evidence.contains("192.0.2.53") && !evidence.contains("example.org"));
    // Not automatic: nothing was changed.
    assert!(matches!(
        machine.entry("/etc/resolv.conf"),
        Some(FakeEntry::File(..))
    ));

    let outcome = apply(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &request(RESOLVER, "restore-resolver-link", Actor::Request, false),
    )
    .expect("applied");
    assert_eq!(outcome.outcome, "repaired");
    assert_eq!(
        machine.entry("/etc/resolv.conf"),
        Some(FakeEntry::Link(
            "../run/systemd/resolve/stub-resolv.conf".to_owned()
        ))
    );
    assert_eq!(machine.relabelled(), ["/etc/resolv.conf"]);
    // The copy is the root's alone, and it is not in the state anyone reads.
    let backups: Vec<PathBuf> = fs::read_dir(paths.data_dir.join("backups"))
        .expect("backups")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(backups.len(), 1);
    let mode = fs::metadata(&backups[0])
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    state.save(&paths.state).expect("saved");
    let shared = fs::read_to_string(&paths.state).expect("state");
    assert!(!shared.contains("192.0.2.53") && !shared.contains("example.org"));

    undo(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        RESOLVER,
        "restore-resolver-link",
        Actor::Request,
        None,
    )
    .expect("undone");
    assert_eq!(
        machine.entry("/etc/resolv.conf"),
        Some(FakeEntry::File(original.as_bytes().to_vec(), 0o640))
    );
}

#[test]
fn a_resolver_configuration_that_is_the_resolvers_own_is_not_a_fault() {
    let catalogue = catalogue();
    let check = &catalogue.fault(RESOLVER).expect("resolver").check;
    let linked = base()
        .with_command(RESOLVED_LINE, printed("active\n"))
        .with_link(
            "/etc/resolv.conf",
            "../run/systemd/resolve/stub-resolv.conf",
        );
    assert!(!super::checks::run_check(&linked, check).present);
    let stub_file = base()
        .with_command(RESOLVED_LINE, printed("active\n"))
        .with_file(
            "/etc/resolv.conf",
            "nameserver 127.0.0.53\noptions edns0\n",
            0o644,
        );
    assert!(!super::checks::run_check(&stub_file, check).present);
    let not_running = base()
        .with_command(RESOLVED_LINE, exited(3, "inactive\n"))
        .with_file("/etc/resolv.conf", "nameserver 192.0.2.1\n", 0o644);
    assert!(!super::checks::run_check(&not_running, check).present);
    let elsewhere = base()
        .with_command(RESOLVED_LINE, printed("active\n"))
        .with_link("/etc/resolv.conf", "/run/NetworkManager/resolv.conf");
    assert!(super::checks::run_check(&elsewhere, check).present);
}

// ------------------------------------------------------------ free space

#[test]
fn deleting_logs_needs_an_administrator_who_confirms_and_is_never_done_for_a_request() {
    let dir = TempDir::new("space");
    let paths = dir.paths();
    let low = "     Avail       Size\n 600000000 20000000000\n";
    let machine = base()
        .with_command(DF_LINE, printed(low))
        .with_command("journalctl --rotate --vacuum-size=268435456", printed(""));
    let catalogue = catalogue();
    let mut state = empty_state();
    scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &Fixes::default(),
        true,
    );
    assert_eq!(
        state.faults[state.active(SPACE).expect("found")].state,
        FaultState::Present
    );

    let refused = apply(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &request(SPACE, "vacuum-journal", Actor::Request, true),
    )
    .expect_err("a request cannot");
    assert_eq!(refused.kind(), ErrorKind::HealNeedsApproval);
    let unconfirmed = apply(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &request(SPACE, "vacuum-journal", Actor::Administrator, false),
    )
    .expect_err("not confirmed");
    assert_eq!(unconfirmed.kind(), ErrorKind::HealNotConfirmed);
    assert!(
        !machine
            .ran()
            .iter()
            .any(|line| line.starts_with("journalctl"))
    );

    let outcome = apply(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &request(SPACE, "vacuum-journal", Actor::Administrator, true),
    )
    .expect("applied");
    // The logs were not what filled the disk: the check still finds it.
    assert_eq!(outcome.outcome, "not-repaired");
    let record = &state.faults[state.active(SPACE).expect("found")];
    assert_eq!(record.state, FaultState::NotRepaired);
    assert!(!record.report_offered, "a full disk is not TUR's defect");
    let undone = undo(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        SPACE,
        "vacuum-journal",
        Actor::Administrator,
        None,
    )
    .expect_err("irreversible");
    assert_eq!(undone.kind(), ErrorKind::HealNothingToUndo);

    // Space freed by other means: cleared.
    machine.set_command(
        DF_LINE,
        printed("     Avail       Size\n 9000000000 20000000000\n"),
    );
    scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &Fixes::default(),
        true,
    );
    assert!(state.active(SPACE).is_none());
}

// ------------------------------------------------- crashes and the fix link

fn crashed() -> String {
    json!([
        { "time": 1_799_995_000_000_000_u64, "pid": 4242, "uid": 0, "gid": 0, "sig": 11,
          "corefile": "present", "exe": "/usr/bin/tur-diag", "size": 1234 },
        { "time": 1_799_995_100_000_000_u64, "pid": 4343, "uid": 1000, "gid": 1000, "sig": 6,
          "corefile": "present", "exe": "/usr/bin/kwrite", "size": 99 }
    ])
    .to_string()
}

#[test]
fn a_crash_of_a_tur_program_has_no_remedy_and_ends_as_an_offered_report() {
    let dir = TempDir::new("crash");
    let paths = dir.paths();
    let machine = base().with_command(&crash_line(), printed(&crashed()));
    let catalogue = catalogue();
    let mut state = empty_state();
    scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &Fixes::default(),
        true,
    );
    let record = &state.faults[state.active(CRASH).expect("found")];
    assert_eq!(record.state, FaultState::ReportOffered);
    assert_eq!(
        record.subjects,
        ["tur-diag"],
        "another program's crash is not TUR's"
    );
    let kinds: Vec<EventKind> = state.events.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, [EventKind::ReportOffered]);
    let refused = apply(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &request(CRASH, "anything", Actor::Request, false),
    )
    .expect_err("no remedy");
    assert_eq!(refused.kind(), ErrorKind::HealUnknown);
    let shown = notices(&catalogue, &state, &BTreeSet::new(), &BTreeSet::new());
    assert_eq!(shown.len(), 1);
    assert!(
        shown[0].body.starts_with("tur-diag crashed."),
        "{}",
        shown[0].body
    );
    assert_eq!(shown[0].actions, [format!("tur-diag heal report {CRASH}")]);
    // Nothing is ever sent from here: no command but the checks ran.
    let checks = [
        "systemctl is-",
        "systemctl list-units --state=failed ",
        "timedatectl show ",
        "df ",
        "coredumpctl list ",
    ];
    for line in machine.ran() {
        assert!(checks.iter().any(|check| line.starts_with(check)), "{line}");
    }
}

fn fixes() -> Fixes {
    Fixes {
        schema_version: 1,
        about: String::new(),
        fixes: vec![Fix {
            code: CRASH.to_owned(),
            summary: "tur-diag no longer crashes on an empty request".to_owned(),
            change: None,
        }],
    }
}

#[test]
fn after_an_update_that_names_the_code_a_fault_that_stays_away_is_reported_fixed() {
    let dir = TempDir::new("fixed");
    let paths = dir.paths();
    let catalogue = catalogue();
    let mut state = empty_state();
    let before = base().with_command(&crash_line(), printed(&crashed()));
    scan(
        &before,
        &catalogue,
        &paths,
        &mut state,
        &running("old"),
        &Fixes::default(),
        true,
    );
    // The new version, after its restart: no crash in this boot.
    let after = base().with_command(&crash_line(), exited(1, ""));
    // The same version without the fix in its list: only "cleared".
    let mut same = state.clone();
    scan(
        &after,
        &catalogue,
        &paths,
        &mut same,
        &running("old"),
        &fixes(),
        true,
    );
    assert_eq!(
        same.events.last().map(|event| event.kind),
        Some(EventKind::Cleared)
    );
    // The new one, whose list names the code.
    scan(
        &after,
        &catalogue,
        &paths,
        &mut state,
        &running("new"),
        &fixes(),
        true,
    );
    let record = &state.faults[state.latest(CRASH).expect("a record")];
    assert_eq!(record.resolved_how.as_deref(), Some("fixed-by-update"));
    let reported: BTreeSet<String> = [CRASH.to_owned()].into();
    let shown = notices(&catalogue, &state, &BTreeSet::new(), &reported);
    let fixed = shown
        .iter()
        .find(|notice| notice.kind == EventKind::FixedByUpdate)
        .expect("a notice");
    assert!(
        fixed.body.starts_with("The problem you reported is fixed"),
        "{}",
        fixed.body
    );
}

#[test]
fn after_an_update_that_names_the_code_a_fault_that_comes_back_is_offered_again() {
    let dir = TempDir::new("recurred");
    let paths = dir.paths();
    let catalogue = catalogue();
    let mut state = empty_state();
    let machine = base().with_command(&crash_line(), printed(&crashed()));
    scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("old"),
        &Fixes::default(),
        true,
    );
    scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("new"),
        &fixes(),
        true,
    );
    let kinds: Vec<EventKind> = state.events.iter().map(|event| event.kind).collect();
    assert_eq!(
        kinds,
        [
            EventKind::ReportOffered,
            EventKind::RecurredAfterUpdate,
            EventKind::ReportOffered
        ]
    );
    let record = &state.faults[state.active(CRASH).expect("a record")];
    assert_eq!(record.state, FaultState::ReportOffered);
    assert_eq!(record.build_id.as_deref(), Some("new"));
}

// ------------------------------------------------------------ the socket

#[test]
fn the_socket_answers_only_well_formed_requests_for_reversible_remedies() {
    let dir = TempDir::new("socket");
    let paths = dir.paths();
    let catalogue = catalogue();
    let machine = base()
        .with_command(TIME_LINE, printed("NTP=yes\nCanNTP=yes\n"))
        .with_command(DF_LINE, printed("Avail Size\n10 20000000000\n"));
    let ask = |text: &str| {
        handle_request(
            &machine,
            &catalogue,
            &paths,
            text.as_bytes(),
            Some("4-12345-678-1000"),
        )
    };
    let code = |answer: &Value| {
        answer["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(code(&ask("not json")), "TUR-DIAG-HEAL-010");
    assert_eq!(
        code(&ask(
            r#"{"schema_version":1,"tool":"heal_apply_remedy","arguments":{"code":"x","remedy":"y","extra":1}}"#
        )),
        "TUR-DIAG-HEAL-010"
    );
    assert_eq!(
        code(&ask(
            r#"{"schema_version":1,"tool":"heal_scan","arguments":{"code":"x","remedy":"y"}}"#
        )),
        "TUR-DIAG-HEAL-010"
    );
    assert_eq!(
        code(&ask(&format!(
            r#"{{"schema_version":1,"tool":"heal_apply_remedy","arguments":{{"code":"{SPACE}","remedy":"vacuum-journal"}}}}"#
        ))),
        "TUR-DIAG-HEAL-003"
    );
    // The clock is right: nothing to repair, nothing is changed.
    let answer = ask(&format!(
        r#"{{"schema_version":1,"tool":"heal_apply_remedy","arguments":{{"code":"{TIME}","remedy":"enable-network-time"}}}}"#
    ));
    assert_eq!(code(&answer), "TUR-DIAG-HEAL-002");
    assert_eq!(answer["ok"], json!(false));
    assert!(!machine.ran().iter().any(|line| line.contains("set-ntp")));
    assert!(
        !machine
            .ran()
            .iter()
            .any(|line| line.starts_with("journalctl"))
    );
}

#[test]
fn the_uid_is_read_from_the_connections_name() {
    assert_eq!(uid_of_connection("3-1234-1000"), Some(1000));
    assert_eq!(uid_of_connection("5-981273-4242_77-1001"), Some(1001));
    assert_eq!(uid_of_connection("nonsense"), None);
}

#[test]
fn the_state_round_trips_and_the_status_lists_only_what_is_not_resolved() {
    let dir = TempDir::new("status");
    let paths = dir.paths();
    let catalogue = catalogue();
    let machine = base()
        .with_command(TIME_LINE, printed("NTP=no\nCanNTP=yes\n"))
        .with_command(&crash_line(), printed(&crashed()));
    let mut state = empty_state();
    scan(
        &machine,
        &catalogue,
        &paths,
        &mut state,
        &running("b1"),
        &Fixes::default(),
        true,
    );
    state.save(&paths.state).expect("saved");
    let mode = fs::metadata(&paths.state)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o644, "readable by everyone, P@x's sandbox included");
    let loaded = HealState::load(&paths.state).expect("loaded");
    assert_eq!(loaded, state);
    let view = status_view(&catalogue, &loaded, &paths.state);
    let codes: Vec<&str> = view
        .faults
        .iter()
        .map(|fault| fault.record.code.as_str())
        .collect();
    assert_eq!(codes, [TIME, CRASH]);
    assert_eq!(view.faults[0].remedies[0].id, "enable-network-time");
    // A check that cannot run changes nothing and is not a fault.
    assert!(state.active(SERVICE).is_none() && state.active(SPACE).is_none());
    // systemctl could not count the failed units here: no count is kept.
    assert_eq!(loaded.failed_units, None);
}

#[test]
fn the_scan_keeps_the_count_of_failed_units_for_status_in_the_sandbox() {
    let dir = TempDir::new("failed-units");
    let paths = dir.paths();
    let machine = base().with_command(
        "systemctl list-units --state=failed --all --no-legend --plain --no-pager",
        printed("foo.service loaded failed failed Foo\nbar.mount loaded failed failed Bar\n"),
    );
    let mut state = empty_state();
    scan(
        &machine,
        &catalogue(),
        &paths,
        &mut state,
        &running("b1"),
        &Fixes::default(),
        false,
    );
    state.save(&paths.state).expect("saved");
    // What tur-diag status reads where systemctl cannot be asked.
    let host = crate::FakeHost::new()
        .with_failing_command("systemctl", 1, "Failed to connect to bus")
        .with_file(
            crate::identity::HEAL_STATE_FILE,
            &fs::read_to_string(&paths.state).expect("the state"),
        );
    let status = crate::gather_status(&host);
    assert_eq!(status.failed_units, Some(2));
    assert!(status.failed_units_as_of.is_some());
}
