// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Integration tests: the built `uos-installer` binary run on the fixtures,
//! a session written to a real directory, and the JSON Schemas checked
//! against what the crate actually writes.
//!
//! Nothing here touches a disk. Sessions are written under Cargo's
//! temporary directory for integration tests.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use uos_installer_core::codes::InstallCode;
use uos_installer_core::identity::{Identity, TargetSelector, WeakIdentityPolicy, select_target};
use uos_installer_core::inventory::{BlockKind, FileProbe, Inventory, Transport, read_json_file};
use uos_installer_core::phase::{Completion, PhaseId, PhaseState, Progress};
use uos_installer_core::plan::{
    ConfirmedPlan, ERASE_DISK_SCHEME, Filesystem, InstallMode, LayoutOptions, PartitionRole, Plan,
    PlanRequest, build_plan,
};
use uos_installer_core::preflight::{
    EnvironmentFacts, HostFacts, Requirements, TargetFacts, run_preflight,
};
use uos_installer_core::session::{
    INVENTORY_FILE, OsFileSystem, PHASES_FILE, PLAN_FILE, ResumeDecision, SESSION_FILE, Session,
    load,
};
use uos_installer_core::time::{SystemClock, Timestamp};

const NVME: &str = "/dev/disk/by-id/nvme-Samsung_SSD_980_PRO_1TB_S5GXNX0T123456A";
const SATA: &str = "serial=S4EWNX0M700001B";
const WEAK: &str = "model=Generic Mass Storage,size=64023257088";

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Runs the built binary and returns its exit code, stdout and stderr.
fn installer(arguments: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_uos-installer"))
        .args(arguments)
        .output()
        .expect("the uos-installer binary runs");
    (
        output.status.code().expect("the binary exits normally"),
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    )
}

fn workstation_plan_arguments<'a>(target: &'a str, lsblk: &'a str, links: &'a str) -> Vec<&'a str> {
    vec!["plan", "--from", lsblk, "--links", links, "--target", target]
}

/// A fresh, empty directory for one test.
fn scratch(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "uos-installer-{name}-{}",
        std::process::id()
    ));
    if directory.exists() {
        fs::remove_dir_all(&directory).expect("the old scratch directory can be removed");
    }
    directory
}

fn inventory(lsblk: &str, links: &str) -> Inventory {
    let probe = FileProbe::new(Path::new(&fixture(lsblk)), Some(Path::new(&fixture(links))));
    Inventory::probe(&probe).expect("the fixture parses")
}

fn good_facts() -> EnvironmentFacts {
    read_json_file(Path::new(&fixture("facts-uefi.json"))).expect("the facts fixture parses")
}

fn plan_for(inventory: &Inventory, target: &str, options: LayoutOptions) -> Plan {
    let selector: TargetSelector = target.parse().expect("the selector parses");
    let selected = select_target(inventory, &selector, WeakIdentityPolicy::Acknowledged)
        .expect("the target is selectable");
    let request = PlanRequest {
        mode: InstallMode::EraseDisk,
        options,
    };
    build_plan(&selected, &request, &ERASE_DISK_SCHEME).expect("the plan builds")
}

fn confirm(inventory: &Inventory, target: &str, plan: &Plan) -> ConfirmedPlan {
    let selector: TargetSelector = target.parse().expect("the selector parses");
    let selected = select_target(inventory, &selector, WeakIdentityPolicy::Acknowledged)
        .expect("the target is selectable");
    let facts = HostFacts {
        environment: good_facts(),
        target: Some(TargetFacts::of(selected.device())),
    };
    let requirements = Requirements::for_layout(&ERASE_DISK_SCHEME, LayoutOptions::default())
        .expect("the requirements compute");
    let preflight = run_preflight(&facts, &requirements);
    plan.confirm(
        &plan.confirmation.required_phrase,
        &preflight,
        Timestamp::from_unix_millis(1_794_464_220_000),
    )
    .expect("the plan confirms")
}

fn run_phase(session: &mut Session<'_>, phase: PhaseId) {
    session.begin(phase).expect("the phase begins");
    session
        .finish(phase, Completion::succeeded("Done"))
        .expect("the phase finishes");
}

/// Writes a session to `directory` that got as far as starting to write the
/// partition table, with a varied history before that. Nothing is written
/// to any disk: only the bookkeeping runs.
fn write_interrupted_session(directory: &Path, stop_before_writing: bool) {
    let inventory = inventory("lsblk-workstation.json", "links-workstation.json");
    let plan = plan_for(&inventory, SATA, LayoutOptions::default());
    let confirmed = confirm(&inventory, SATA, &plan);
    let clock = SystemClock;
    let file_system = OsFileSystem;
    let mut session =
        Session::create(&file_system, &clock, directory, "integration-test").expect("created");
    session.save_inventory(&inventory).expect("inventory saved");

    session.begin(PhaseId::EnvironmentCheck).expect("begins");
    session
        .finish(
            PhaseId::EnvironmentCheck,
            Completion::warning("Secure Boot is switched off")
                .with_details("secure_boot: disabled")
                .with_log("logs/installer-ui.log"),
        )
        .expect("finishes");
    run_phase(&mut session, PhaseId::HardwareInventory);

    // Storage discovery fails once, with progress, and is retried.
    session.begin(PhaseId::StorageDiscovery).expect("begins");
    session
        .set_progress(
            PhaseId::StorageDiscovery,
            Progress::Determinate {
                completed: 2,
                total: 6,
                unit: "devices".to_owned(),
            },
        )
        .expect("progress");
    session
        .finish(
            PhaseId::StorageDiscovery,
            Completion::failed(
                InstallCode::InventoryMalformed.into(),
                "The device list could not be read",
            )
            .with_recovery("Retry"),
        )
        .expect("finishes");
    run_phase(&mut session, PhaseId::StorageDiscovery);

    // The user picks a disk, goes back, and picks again.
    run_phase(&mut session, PhaseId::TargetSelection);
    session.reopen(PhaseId::TargetSelection).expect("reopens");
    session.begin(PhaseId::TargetSelection).expect("begins");
    session
        .set_progress(
            PhaseId::TargetSelection,
            Progress::Indeterminate {
                activity: "Waiting for a choice".to_owned(),
            },
        )
        .expect("progress");
    session
        .finish(PhaseId::TargetSelection, Completion::succeeded("Chosen"))
        .expect("finishes");
    run_phase(&mut session, PhaseId::PartitionPlan);
    session.save_plan(confirmed.plan()).expect("plan saved");

    session.begin(PhaseId::UserConfirmation).expect("begins");
    session
        .finish(
            PhaseId::UserConfirmation,
            Completion::cancelled("The user went back"),
        )
        .expect("finishes");
    run_phase(&mut session, PhaseId::UserConfirmation);
    if stop_before_writing {
        return;
    }

    let fresh = self::inventory("lsblk-workstation.json", "links-workstation.json");
    let validated = confirmed.revalidate(&fresh).expect("the target revalidates");
    session
        .begin_destructive(PhaseId::Partitioning, &validated)
        .expect("the phase is recorded as started");
}

// --- inventory ---------------------------------------------------------------

#[test]
fn inventory_json_describes_every_device() {
    let (code, stdout, stderr) = installer(&[
        "inventory",
        "--from",
        &fixture("lsblk-workstation.json"),
        "--links",
        &fixture("links-workstation.json"),
        "--json",
    ]);
    assert_eq!(code, 0, "{stderr}");
    let value: Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(value["schema_version"], 1);
    let devices = value["devices"].as_array().expect("devices is an array");
    assert_eq!(devices.len(), 6);
    assert_eq!(devices[0]["model"], "Samsung SSD 980 PRO 1TB");
    assert_eq!(devices[0]["children"].as_array().map(Vec::len), Some(4));
    assert_eq!(devices[2]["children"][0]["mountpoints"][0], "/run/initramfs/live");
    // The binary's output is the library's inventory.
    let parsed: Inventory = serde_json::from_str(&stdout).expect("stdout is an inventory");
    assert_eq!(
        parsed,
        inventory("lsblk-workstation.json", "links-workstation.json")
    );
}

#[test]
fn inventory_text_shows_the_oddities() {
    let (code, stdout, stderr) = installer(&[
        "inventory",
        "--from",
        &fixture("lsblk-oddities.json"),
        "--links",
        &fixture("links-oddities.json"),
    ]);
    assert_eq!(code, 0, "{stderr}");
    for needle in [
        "Storage inventory: 6 whole disks, 0 other top-level devices",
        "Disk: Generic Mass Storage",
        "Serial:           not reported",
        "Stable path:      none",
        "can be chosen only with an explicit weaker-identity acknowledgement",
        "refused: the disk is in use",
        "Detected system:  Linux (likely)",
        "encrypted volume",
        "fedora-home",
    ] {
        assert!(stdout.contains(needle), "missing \"{needle}\" in:\n{stdout}");
    }
}

#[test]
fn inventory_works_without_a_links_file() {
    let (code, stdout, stderr) =
        installer(&["inventory", "--from", &fixture("lsblk-workstation.json")]);
    assert_eq!(code, 0, "{stderr}");
    // lsblk's own id-link column still gives the stable path.
    assert!(stdout.contains(NVME), "{stdout}");
    assert!(stdout.contains("Port:             unknown"), "{stdout}");
}

#[test]
fn malformed_input_is_a_typed_error() {
    let directory = scratch("malformed");
    fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join("not-lsblk.json");
    fs::write(&path, "{\"devices\": []}").expect("file written");
    let path = path.to_string_lossy().into_owned();

    let (code, stdout, stderr) = installer(&["inventory", "--from", &path]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(stderr.starts_with("error: UOS-INSTALL-INVENTORY-002\n"), "{stderr}");

    let (code, _, stderr) = installer(&[
        "inventory",
        "--from",
        &fixture("lsblk-workstation.json"),
        "--links",
        &path,
    ]);
    assert_eq!(code, 1);
    assert!(stderr.starts_with("error: UOS-INSTALL-INVENTORY-002\n"), "{stderr}");
}

// --- plan --------------------------------------------------------------------

#[test]
fn plan_text_shows_plan_preflight_and_consequences() {
    let lsblk = fixture("lsblk-workstation.json");
    let links = fixture("links-workstation.json");
    let facts = fixture("facts-uefi.json");
    let mut arguments = workstation_plan_arguments(NVME, &lsblk, &links);
    arguments.extend(["--facts", facts.as_str()]);
    let (code, stdout, stderr) = installer(&arguments);
    assert_eq!(code, 0, "{stderr}\n{stdout}");
    assert!(stderr.is_empty(), "{stderr}");
    for needle in [
        "UOS installer dry run. Nothing is written to any disk.",
        "Preflight checks",
        "[pass]     UOS-INSTALL-PREFLIGHT-001",
        "[warning]  UOS-INSTALL-PREFLIGHT-009",
        "Result: 0 blocking, 1 warnings. No check blocks installation.",
        "Samsung SSD 980 PRO 1TB",
        "S5GXNX0T123456A",
        NVME,
        "/dev/nvme0n1 when planned (not stable; never used to find the disk)",
        "Plan: erase the target disk and install UOS",
        "uos-erase-disk-gpt-draft-1 (an unverified assumption",
        "1  EFI-SYSTEM  1.0 GiB  FAT32  mounted at /boot/efi",
        "3  root  929.5 GiB  Btrfs  mounted at /",
        "Everything on the disk will be permanently destroyed: 4 existing",
        "/dev/nvme0n1p3  930.7 GiB  Microsoft basic data, ntfs, label \"Windows\"",
        "Other operating system detected: Windows (likely)",
        "User data preserved: no",
        "Encryption: none.",
        "To confirm, type exactly: ERASE T123456A",
    ] {
        assert!(stdout.contains(needle), "missing \"{needle}\" in:\n{stdout}");
    }
}

#[test]
fn plan_json_carries_the_plan_the_preflight_report_and_no_refusal() {
    let lsblk = fixture("lsblk-workstation.json");
    let links = fixture("links-workstation.json");
    let facts = fixture("facts-uefi.json");
    let mut arguments = workstation_plan_arguments(NVME, &lsblk, &links);
    arguments.extend(["--facts", facts.as_str(), "--encrypt", "--data-partition", "--json"]);
    let (code, stdout, stderr) = installer(&arguments);
    assert_eq!(code, 0, "{stderr}");
    let value: Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["refusal"], Value::Null);
    assert_eq!(value["preflight"]["results"].as_array().map(Vec::len), Some(13));
    assert_eq!(value["plan"]["encryption"], "luks2");
    assert_eq!(value["plan"]["scheme_verified"], false);
    assert_eq!(value["plan"]["target"]["identity"]["serial"], "S5GXNX0T123456A");
    assert_eq!(value["plan"]["target"]["identity"]["by_id_path"], NVME);
    assert_eq!(value["plan"]["confirmation"]["required_phrase"], "ERASE T123456A");
    let roles: Vec<&str> = value["plan"]["partitions"]
        .as_array()
        .expect("partitions is an array")
        .iter()
        .map(|partition| partition["role"].as_str().expect("role is a string"))
        .collect();
    assert_eq!(roles, ["esp", "boot", "root", "data"]);

    // The binary's plan is the library's plan.
    let from_binary: Plan =
        serde_json::from_value(value["plan"].clone()).expect("the plan deserialises");
    let options = LayoutOptions {
        encrypt: true,
        data_partition: true,
    };
    let inventory = inventory("lsblk-workstation.json", "links-workstation.json");
    assert_eq!(from_binary, plan_for(&inventory, NVME, options));
}

#[test]
fn plan_without_host_facts_is_blocked_because_nothing_was_determined() {
    let lsblk = fixture("lsblk-workstation.json");
    let links = fixture("links-workstation.json");
    let (code, stdout, stderr) = installer(&workstation_plan_arguments(SATA, &lsblk, &links));
    assert_eq!(code, 3, "{stderr}");
    for needle in [
        "[BLOCKING] UOS-INSTALL-PREFLIGHT-001",
        "detail: boot_mode: not determined",
        "Installation would not be allowed to proceed.",
        "To confirm, type exactly: ERASE M700001B",
    ] {
        assert!(stdout.contains(needle), "missing \"{needle}\" in:\n{stdout}");
    }
}

#[test]
fn plan_on_a_legacy_bios_machine_is_blocked() {
    let lsblk = fixture("lsblk-workstation.json");
    let links = fixture("links-workstation.json");
    let facts = fixture("facts-bios.json");
    let mut arguments = workstation_plan_arguments(SATA, &lsblk, &links);
    arguments.extend(["--facts", facts.as_str(), "--json"]);
    let (code, stdout, _) = installer(&arguments);
    assert_eq!(code, 3);
    let value: Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    let blocking: Vec<&str> = value["preflight"]["results"]
        .as_array()
        .expect("results is an array")
        .iter()
        .filter(|result| result["status"] == "blocking")
        .map(|result| result["code"].as_str().expect("code is a string"))
        .collect();
    assert_eq!(blocking, ["UOS-INSTALL-PREFLIGHT-001"]);
}

#[test]
fn a_device_path_is_refused_as_a_target() {
    let lsblk = fixture("lsblk-workstation.json");
    let links = fixture("links-workstation.json");
    for target in ["/dev/nvme0n1", "/dev/sda", "/dev/disk/by-path/pci-0000:01:00.0-nvme-1"] {
        let (code, stdout, stderr) =
            installer(&workstation_plan_arguments(target, &lsblk, &links));
        assert_eq!(code, 1, "{target}");
        assert!(stderr.starts_with("error: UOS-INSTALL-TARGET-001\n"), "{stderr}");
        assert!(stderr.contains(target), "{stderr}");
        assert!(stdout.contains("No plan was made."), "{stdout}");
        assert!(!stdout.contains("ERASE"), "{stdout}");
    }

    let mut arguments = workstation_plan_arguments("/dev/sda", &lsblk, &links);
    arguments.push("--json");
    let (code, stdout, _) = installer(&arguments);
    assert_eq!(code, 1);
    let value: Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(value["plan"], Value::Null);
    assert_eq!(value["preflight"], Value::Null);
    assert_eq!(value["refusal"]["code"], "UOS-INSTALL-TARGET-001");
}

#[test]
fn unsafe_and_unusable_targets_are_refused_with_their_codes() {
    let workstation = (
        fixture("lsblk-workstation.json"),
        fixture("links-workstation.json"),
    );
    let oddities = (
        fixture("lsblk-oddities.json"),
        fixture("links-oddities.json"),
    );
    for (files, target, code) in [
        // The live medium.
        (
            &workstation,
            "serial=4C530001230506112345",
            "UOS-INSTALL-PLAN-005",
        ),
        // A card reader with no medium.
        (&workstation, "model=Card Reader", "UOS-INSTALL-TARGET-006"),
        // A partition.
        (
            &workstation,
            "/dev/disk/by-id/ata-Samsung_SSD_860_EVO_500GB_S4EWNX0M700001B-part3",
            "UOS-INSTALL-TARGET-005",
        ),
        // Nothing matches.
        (&workstation, "serial=NOSUCHDISK", "UOS-INSTALL-TARGET-003"),
        // A kernel name.
        (&workstation, "sda", "UOS-INSTALL-TARGET-002"),
        // Two enclosures with one serial number.
        (
            &oddities,
            "serial=0123456789ABCDEF",
            "UOS-INSTALL-TARGET-004",
        ),
        // Two paths to one multipath disk.
        (
            &oddities,
            "wwn=0x5000c500a1b2c3d4",
            "UOS-INSTALL-TARGET-004",
        ),
        // No serial number, not acknowledged.
        (&oddities, WEAK, "UOS-INSTALL-TARGET-007"),
        // Open LUKS and LVM volumes.
        (&oddities, "serial=LVMLUKS0001", "UOS-INSTALL-PLAN-006"),
    ] {
        let (exit, stdout, stderr) =
            installer(&workstation_plan_arguments(target, &files.0, &files.1));
        assert_eq!(exit, 1, "{target}: {stderr}");
        assert!(
            stderr.starts_with(&format!("error: {code}\n")),
            "{target}: {stderr}"
        );
        assert!(stdout.contains("No plan was made."), "{target}: {stdout}");
    }

    // The live medium's refusal still shows the preflight results.
    let (_, stdout, _) = installer(&workstation_plan_arguments(
        "serial=4C530001230506112345",
        &workstation.0,
        &workstation.1,
    ));
    assert!(stdout.contains("[BLOCKING] UOS-INSTALL-PREFLIGHT-008"), "{stdout}");
    assert!(stdout.contains("[BLOCKING] UOS-INSTALL-PREFLIGHT-007"), "{stdout}");
}

#[test]
fn a_weak_identity_needs_the_explicit_flag_and_is_recorded() {
    let lsblk = fixture("lsblk-oddities.json");
    let links = fixture("links-oddities.json");
    let facts = fixture("facts-uefi.json");
    let mut arguments = workstation_plan_arguments(WEAK, &lsblk, &links);
    arguments.extend(["--facts", facts.as_str(), "--acknowledge-weak-identity", "--json"]);
    let (code, stdout, stderr) = installer(&arguments);
    assert_eq!(code, 0, "{stderr}");
    let value: Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(value["plan"]["target"]["weaker_identity_acknowledged"], true);
    assert_eq!(value["plan"]["target"]["identity_strength"], "weak");
    assert_eq!(value["plan"]["target"]["identity"]["serial"], Value::Null);
    assert_eq!(value["plan"]["confirmation"]["required_phrase"], "ERASE 64023257088");
    let identity_check = value["preflight"]["results"]
        .as_array()
        .expect("results is an array")
        .iter()
        .find(|result| result["code"] == "UOS-INSTALL-PREFLIGHT-013")
        .expect("the identity check ran");
    assert_eq!(identity_check["status"], "warning");
}

#[test]
fn a_disk_that_is_too_small_is_refused() {
    let directory = scratch("small-disk");
    fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join("lsblk.json");
    fs::write(
        &path,
        r#"{"blockdevices": [{"name": "sda", "type": "disk", "size": 16013942784,
            "model": "Small Stick", "serial": "SMALL0001", "tran": "usb",
            "log-sec": 512, "phy-sec": 512, "rm": true}]}"#,
    )
    .expect("file written");
    let path = path.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        installer(&["plan", "--from", &path, "--target", "serial=SMALL0001"]);
    assert_eq!(code, 1);
    assert!(stderr.starts_with("error: UOS-INSTALL-PLAN-001\n"), "{stderr}");
    assert!(stderr.contains("required_bytes: 23624417280"), "{stderr}");
    assert!(stdout.contains("[BLOCKING] UOS-INSTALL-PREFLIGHT-006"), "{stdout}");
}

// --- session -----------------------------------------------------------------

#[test]
fn a_session_on_disk_is_durable_private_and_shown_as_needing_reconciliation() {
    let directory = scratch("interrupted");
    write_interrupted_session(&directory, false);

    // Exactly the four files, no temporary leftovers, owner-only modes.
    let mut names: Vec<String> = fs::read_dir(&directory)
        .expect("the directory lists")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [PLAN_FILE, PHASES_FILE, SESSION_FILE, INVENTORY_FILE]);
    for name in &names {
        let mode = fs::metadata(directory.join(name)).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{name}");
    }
    let directory_mode = fs::metadata(&directory).expect("metadata").permissions().mode();
    assert_eq!(directory_mode & 0o777, 0o700);

    // The library refuses to resume it.
    let loaded = load(&OsFileSystem, &directory).expect("the session loads");
    assert!(loaded.anomalies().is_empty(), "{:?}", loaded.anomalies());
    assert!(loaded.meta().target_writes_started);
    assert!(matches!(
        loaded.decision(),
        ResumeDecision::NeedsReconciliation {
            phase: PhaseId::Partitioning,
            state: PhaseState::Running,
            ..
        }
    ));
    let fresh = inventory("lsblk-workstation.json", "links-workstation.json");
    let reconciliation = loaded.reconcile(&fresh).expect("the target is still there");
    assert_eq!(reconciliation.device.serial.as_deref(), Some("S4EWNX0M700001B"));
    assert!(reconciliation.fresh_confirmation_required());
    assert!(loaded.resume(&OsFileSystem, &SystemClock).is_err());

    // The binary says the same.
    let path = directory.to_string_lossy().into_owned();
    let (code, stdout, stderr) = installer(&["session", "show", &path]);
    assert_eq!(code, 0, "{stderr}");
    for needle in [
        "Session integration-test",
        "Target written:     possibly",
        "Samsung SSD 860 EVO 500GB",
        "PARTITIONING              running    non_repeatable  writes to target",
        "ENVIRONMENT_CHECK         warning    idempotent      -",
        "ENCRYPTION_SETUP          pending",
        "Needs reconciliation: phase PARTITIONING (non_repeatable) is recorded as running.",
        "Do not resume",
    ] {
        assert!(stdout.contains(needle), "missing \"{needle}\" in:\n{stdout}");
    }
    let (code, stdout, _) = installer(&["session", "show", &path, "--json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert_eq!(value["decision"]["decision"], "needs_reconciliation");
    assert_eq!(value["decision"]["phase"], "PARTITIONING");
    assert_eq!(value["session"]["target_writes_started"], true);
    assert_eq!(value["plan_readable"], true);
    assert_eq!(value["anomalies"].as_array().map(Vec::len), Some(0));
}

#[test]
fn a_session_stopped_before_writing_can_be_resumed_from_disk() {
    let directory = scratch("resumable");
    write_interrupted_session(&directory, true);
    let path = directory.to_string_lossy().into_owned();
    let (code, stdout, stderr) = installer(&["session", "show", &path]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Target written:     no"), "{stdout}");
    assert!(
        stdout.contains("The session can continue from STORAGE_DISCOVERY"),
        "{stdout}"
    );

    // Tear the end of the log, as a crash during an append would.
    let log_path = directory.join(PHASES_FILE);
    let mut log = fs::read(&log_path).expect("the log reads");
    log.extend_from_slice(b"{\"seq\":99,\"recorded_at\":");
    fs::write(&log_path, &log).expect("the log is rewritten");
    let (code, stdout, _) = installer(&["session", "show", &path]);
    assert_eq!(code, 0);
    assert!(stdout.contains("is incomplete: the installer stopped while writing it"), "{stdout}");

    let loaded = load(&OsFileSystem, &directory).expect("the session loads");
    let clock = SystemClock;
    let file_system = OsFileSystem;
    let mut session = loaded.resume(&file_system, &clock).expect("the session resumes");
    assert_eq!(session.meta().target, None);
    run_phase(&mut session, PhaseId::StorageDiscovery);
    let again = load(&OsFileSystem, &directory).expect("the session loads again");
    assert_eq!(
        again.decision(),
        &ResumeDecision::Resume {
            from_phase: PhaseId::StorageDiscovery
        }
    );
    assert_eq!(
        again.phases().get(PhaseId::StorageDiscovery).state,
        PhaseState::Succeeded
    );
    // A second session cannot be created over it.
    assert!(Session::create(&file_system, &clock, &directory, "another").is_err());
}

#[test]
fn showing_a_directory_without_a_session_is_a_typed_error() {
    let directory = scratch("empty");
    fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.to_string_lossy().into_owned();
    let (code, stdout, stderr) = installer(&["session", "show", &path]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(stderr.starts_with("error: UOS-INSTALL-SESSION-009\n"), "{stderr}");
}

// --- schemas -----------------------------------------------------------------

fn schema(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas")
        .join(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} cannot be read: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{name} is not JSON: {error}"))
}

/// Keywords that describe a schema without constraining an instance.
const ANNOTATIONS: [&str; 5] = ["$schema", "$id", "title", "description", "$defs"];

fn type_matches(name: &str, instance: &Value) -> bool {
    match name {
        "null" => instance.is_null(),
        "boolean" => instance.is_boolean(),
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "integer" => instance.is_u64() || instance.is_i64(),
        "number" => instance.is_number(),
        _ => false,
    }
}

fn is_valid(schema: &Value, root: &Value, instance: &Value) -> bool {
    let mut errors = Vec::new();
    validate(schema, root, instance, "$", &mut errors);
    errors.is_empty()
}

/// A validator for the subset of JSON Schema 2020-12 the UOS schemas use.
/// A keyword it does not know is reported as an error, so a schema cannot
/// carry a constraint that these tests silently ignore.
fn validate(schema: &Value, root: &Value, instance: &Value, at: &str, errors: &mut Vec<String>) {
    let Some(keywords) = schema.as_object() else {
        errors.push(format!("{at}: the schema is not an object"));
        return;
    };
    for (keyword, value) in keywords {
        match keyword.as_str() {
            "$ref" => {
                let target = value
                    .as_str()
                    .and_then(|reference| reference.strip_prefix("#/$defs/"))
                    .and_then(|name| root["$defs"].get(name));
                match target {
                    Some(target) => validate(target, root, instance, at, errors),
                    None => errors.push(format!("{at}: unresolved reference {value}")),
                }
            }
            "type" => {
                let names: Vec<&str> = match value {
                    Value::String(name) => vec![name.as_str()],
                    Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
                    _ => Vec::new(),
                };
                if !names.iter().any(|name| type_matches(name, instance)) {
                    errors.push(format!("{at}: expected type {value}, found {instance}"));
                }
            }
            "enum" => {
                let allowed = value.as_array().map(Vec::as_slice).unwrap_or(&[]);
                if !allowed.contains(instance) {
                    errors.push(format!("{at}: {instance} is not one of {value}"));
                }
            }
            "const" => {
                if instance != value {
                    errors.push(format!("{at}: expected {value}, found {instance}"));
                }
            }
            "required" => {
                for name in value.as_array().into_iter().flatten() {
                    let present = name
                        .as_str()
                        .is_some_and(|name| instance.get(name).is_some());
                    if instance.is_object() && !present {
                        errors.push(format!("{at}: missing required property {name}"));
                    }
                }
            }
            "properties" => {
                for (name, subschema) in value.as_object().into_iter().flatten() {
                    if let Some(child) = instance.get(name) {
                        validate(subschema, root, child, &format!("{at}.{name}"), errors);
                    }
                }
            }
            "additionalProperties" => {
                let declared = keywords.get("properties").and_then(Value::as_object);
                if value != &Value::Bool(false) {
                    errors.push(format!("{at}: only additionalProperties: false is supported"));
                } else if let Some(object) = instance.as_object() {
                    for name in object.keys() {
                        if !declared.is_some_and(|properties| properties.contains_key(name)) {
                            errors.push(format!("{at}: unexpected property \"{name}\""));
                        }
                    }
                }
            }
            "items" => {
                for (index, child) in instance.as_array().into_iter().flatten().enumerate() {
                    validate(value, root, child, &format!("{at}[{index}]"), errors);
                }
            }
            "minItems" | "maxItems" => {
                if let (Some(limit), Some(items)) = (value.as_u64(), instance.as_array()) {
                    let count = items.len() as u64;
                    let ok = if keyword == "minItems" {
                        count >= limit
                    } else {
                        count <= limit
                    };
                    if !ok {
                        errors.push(format!("{at}: {count} items violates {keyword} {limit}"));
                    }
                }
            }
            "minimum" => {
                if let (Some(limit), Some(number)) = (value.as_i64(), instance.as_i64())
                    && number < limit
                {
                    errors.push(format!("{at}: {number} is below the minimum {limit}"));
                }
            }
            "minLength" => {
                if let (Some(limit), Some(text)) = (value.as_u64(), instance.as_str())
                    && (text.chars().count() as u64) < limit
                {
                    errors.push(format!("{at}: \"{text}\" is shorter than {limit}"));
                }
            }
            "oneOf" => {
                let matching = value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|subschema| is_valid(subschema, root, instance))
                    .count();
                if matching != 1 {
                    errors.push(format!(
                        "{at}: {instance} matches {matching} of the oneOf alternatives"
                    ));
                }
            }
            annotation if ANNOTATIONS.contains(&annotation) => {}
            unsupported => errors.push(format!(
                "{at}: the test validator does not support the keyword \"{unsupported}\""
            )),
        }
    }
}

fn assert_valid(schema: &Value, root: &Value, instance: &Value, what: &str) {
    let mut errors = Vec::new();
    validate(schema, root, instance, "$", &mut errors);
    assert!(errors.is_empty(), "{what} does not match its schema:\n{}", errors.join("\n"));
}

fn assert_invalid(schema: &Value, instance: &Value, what: &str) {
    assert!(!is_valid(schema, schema, instance), "{what} was accepted by the schema");
}

/// Every key the schema declares for an object must be required, because
/// the serde types always write every field.
fn assert_every_property_is_required(schema: &Value, at: &str) {
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        let mut required: Vec<&str> = schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let mut declared: Vec<&str> = properties.keys().map(String::as_str).collect();
        declared.sort_unstable();
        required.sort_unstable();
        assert_eq!(declared, required, "{at}: properties and required differ");
        assert_eq!(schema["additionalProperties"], false, "{at}");
    }
    match schema {
        Value::Object(map) => {
            for (key, value) in map {
                assert_every_property_is_required(value, &format!("{at}/{key}"));
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                assert_every_property_is_required(value, &format!("{at}/{index}"));
            }
        }
        _ => {}
    }
}

#[test]
fn the_plan_schema_accepts_every_plan_the_crate_writes() {
    let schema = schema("installer-plan.schema.json");
    assert_every_property_is_required(&schema, "plan");
    let workstation = inventory("lsblk-workstation.json", "links-workstation.json");
    let oddities = inventory("lsblk-oddities.json", "links-oddities.json");
    let all = LayoutOptions {
        encrypt: true,
        data_partition: true,
    };
    let plans = [
        ("NVMe, encrypted, with data", plan_for(&workstation, NVME, all)),
        ("SATA, defaults", plan_for(&workstation, SATA, LayoutOptions::default())),
        ("weak identity", plan_for(&oddities, WEAK, LayoutOptions::default())),
    ];
    for (name, plan) in &plans {
        let value = serde_json::to_value(plan).expect("the plan serialises");
        assert_valid(&schema, &schema, &value, name);
        // And what the schema accepts, the crate reads back unchanged.
        let again: Plan = serde_json::from_value(value).expect("the plan deserialises");
        assert_eq!(&again, plan, "{name}");
    }

    // The validator is not vacuous: damaged plans are rejected.
    let good = serde_json::to_value(&plans[0].1).expect("the plan serialises");
    let mut missing = good.clone();
    missing.as_object_mut().expect("object").remove("consequences");
    assert_invalid(&schema, &missing, "a plan without consequences");
    let mut extra = good.clone();
    extra["commands"] = Value::Array(Vec::new());
    assert_invalid(&schema, &extra, "a plan with an unknown key");
    let mut device_path = good.clone();
    device_path["target"]["identity"]["device_path"] = Value::from("/dev/sda");
    assert_invalid(&schema, &device_path, "an identity with a device path");
    let mut bad_mode = good.clone();
    bad_mode["mode"] = Value::from("wipe_everything");
    assert_invalid(&schema, &bad_mode, "a plan with an unknown mode");
    let mut no_partitions = good.clone();
    no_partitions["partitions"] = Value::Array(Vec::new());
    assert_invalid(&schema, &no_partitions, "a plan with no partitions");
    let mut wrong_type = good;
    wrong_type["partitions"][0]["size_bytes"] = Value::from("1 GiB");
    assert_invalid(&schema, &wrong_type, "a partition with a textual size");
}

#[test]
fn the_session_schema_accepts_the_files_a_session_writes() {
    let schema = schema("installer-session.schema.json");
    assert_every_property_is_required(&schema, "session");
    let directory = scratch("schema");
    write_interrupted_session(&directory, false);

    let read = |name: &str| -> String {
        fs::read_to_string(directory.join(name)).expect("the session file reads")
    };
    let session: Value = serde_json::from_str(&read(SESSION_FILE)).expect("session.json is JSON");
    assert_valid(&schema, &schema, &session, SESSION_FILE);
    assert_eq!(session["phases"].as_array().map(Vec::len), Some(16));

    // Every line of the log matches the transition record definition, and
    // between them the lines exercise every state and both kinds of progress.
    let record_schema = &schema["$defs"]["transition_record"];
    let log = read(PHASES_FILE);
    let mut states: Vec<String> = Vec::new();
    let mut progress_kinds: Vec<String> = Vec::new();
    for (index, line) in log.lines().enumerate() {
        let record: Value = serde_json::from_str(line).expect("a log line is JSON");
        assert_valid(record_schema, &schema, &record, &format!("log line {}", index + 1));
        assert_eq!(record["seq"], index as u64 + 1);
        states.extend(record["phase"]["state"].as_str().map(str::to_owned));
        progress_kinds.extend(record["phase"]["progress"]["kind"].as_str().map(str::to_owned));
    }
    states.sort();
    states.dedup();
    assert_eq!(
        states,
        ["cancelled", "failed", "pending", "running", "succeeded", "warning"]
    );
    progress_kinds.sort();
    progress_kinds.dedup();
    assert_eq!(progress_kinds, ["determinate", "indeterminate"]);

    // The plan file in the session matches the plan schema.
    let plan_schema = self::schema("installer-plan.schema.json");
    let plan: Value = serde_json::from_str(&read(PLAN_FILE)).expect("the plan file is JSON");
    assert_valid(&plan_schema, &plan_schema, &plan, PLAN_FILE);

    // Damaged session files are rejected.
    let mut missing = session.clone();
    missing.as_object_mut().expect("object").remove("target_writes_started");
    assert_invalid(&schema, &missing, "a session without the write flag");
    let mut short = session.clone();
    short["phases"].as_array_mut().expect("array").pop();
    assert_invalid(&schema, &short, "a session with fifteen phases");
    let mut bad_state = session.clone();
    bad_state["phases"][0]["state"] = Value::from("exploded");
    assert_invalid(&schema, &bad_state, "a phase in an unknown state");
    let mut bad_progress = session;
    bad_progress["phases"][3]["progress"] = serde_json::json!({ "kind": "percent", "value": 50 });
    assert_invalid(&schema, &bad_progress, "a percentage as progress");
}

/// The values of an `enum` keyword, without a trailing `null`.
fn enum_values(schema: &Value) -> Vec<Value> {
    schema["enum"]
        .as_array()
        .expect("the schema has an enum")
        .iter()
        .filter(|value| !value.is_null())
        .cloned()
        .collect()
}

#[test]
fn schema_enumerations_are_exactly_the_rust_enumerations() {
    let session = schema("installer-session.schema.json");
    let plan = schema("installer-plan.schema.json");

    let phases = enum_values(&session["$defs"]["phase_id"]);
    let expected: Vec<Value> = PhaseId::ALL
        .iter()
        .map(|phase| Value::from(phase.as_str()))
        .collect();
    assert_eq!(phases, expected);

    let states = enum_values(&session["$defs"]["phase_state"]);
    assert_eq!(states.len(), 7);
    for state in states {
        serde_json::from_value::<PhaseState>(state).expect("a schema state is a PhaseState");
    }

    for modes in [&session["properties"]["mode"], &plan["properties"]["mode"]] {
        let modes = enum_values(modes);
        assert_eq!(modes.len(), 4);
        for mode in modes {
            serde_json::from_value::<InstallMode>(mode).expect("a schema mode is an InstallMode");
        }
    }

    for root in [&session, &plan] {
        let identity = &root["$defs"]["identity"];
        let transports = enum_values(&identity["properties"]["transport"]);
        assert_eq!(transports.len(), 11);
        for transport in transports {
            serde_json::from_value::<Transport>(transport).expect("a schema transport parses");
        }
        // The identity has exactly the fields of the Identity type.
        let sample = serde_json::json!({
            "model": null, "serial": "S1", "wwn": null, "size_bytes": 1, "transport": "usb",
            "logical_sector_size": 512, "physical_sector_size": 512, "by_id_path": null,
            "by_path": null
        });
        assert!(is_valid(identity, root, &sample));
        serde_json::from_value::<Identity>(sample).expect("the sample is an Identity");
    }

    let kinds = enum_values(&plan["$defs"]["destroyed_item"]["properties"]["kind"]);
    assert_eq!(kinds.len(), 10);
    for kind in kinds {
        serde_json::from_value::<BlockKind>(kind).expect("a schema kind is a BlockKind");
    }
    let roles = enum_values(&plan["$defs"]["partition"]["properties"]["role"]);
    assert_eq!(roles.len(), 4);
    for role in roles {
        serde_json::from_value::<PartitionRole>(role).expect("a schema role is a PartitionRole");
    }
    let filesystems = enum_values(&plan["$defs"]["partition"]["properties"]["filesystem"]);
    assert_eq!(filesystems.len(), 4);
    for filesystem in filesystems {
        serde_json::from_value::<Filesystem>(filesystem).expect("a schema filesystem parses");
    }
}
