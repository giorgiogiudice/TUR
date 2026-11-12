// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The built `tur-device` binary, run on fixture directories only.
//!
//! Every invocation here passes `--from`, so the binary reads a directory of
//! test data and runs no command. The binary is never run against the machine
//! the tests execute on.

mod support;

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tur_device_manager::{FsProbe, ProblemReport, collect};

const EXIT_OK: i32 = 0;
const EXIT_FAILURE: i32 = 1;
const EXIT_USAGE: i32 = 2;

struct Run {
    status: i32,
    stdout: Vec<u8>,
    stderr: String,
}

impl Run {
    fn text(&self) -> &str {
        std::str::from_utf8(&self.stdout).expect("stdout is UTF-8")
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.stdout).unwrap_or_else(|error| {
            panic!(
                "stdout is not JSON ({error}); status {}, stderr {:?}",
                self.status, self.stderr
            )
        })
    }
}

fn tur_device(args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_tur-device"))
        .args(args)
        .output()
        .expect("tur-device starts");
    Run {
        status: output
            .status
            .code()
            .expect("tur-device exits with a status"),
        stdout: output.stdout,
        stderr: String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    }
}

/// A fresh, empty directory under Cargo's directory for test files.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("tur-device-{name}-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("the old scratch directory can be removed");
    }
    fs::create_dir_all(&dir).expect("the scratch directory can be created");
    dir
}

fn write(root: &Path, path: &str, content: &[u8]) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().expect("the path has a parent")).expect("directories");
    fs::write(full, content).expect("the file can be written");
}

fn link(root: &Path, path: &str, target: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().expect("the path has a parent")).expect("directories");
    symlink(target, full).expect("the link can be created");
}

#[test]
fn inventory_json_is_the_library_inventory_and_matches_the_schema() {
    let run = tur_device(&["inventory", "--from", support::LAPTOP, "--json"]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    assert!(run.stderr.is_empty(), "{}", run.stderr);
    let document = run.json();
    support::validate(&document).expect("the output matches the schema");

    let expected = serde_json::to_value(collect(&FsProbe::fixture(support::LAPTOP))).unwrap();
    assert_eq!(document, expected);
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["redacted"], Value::Bool(false));
    assert_eq!(document["pci"].as_array().map(Vec::len), Some(18));
    assert_eq!(document["usb"].as_array().map(Vec::len), Some(8));
    assert_eq!(document["pci"][2]["driver"], "i915");
    assert_eq!(
        document["usb"][4]["serial"],
        serde_json::json!({ "sensitive": true, "redacted": false, "value": "4C530001230915112345" })
    );
    assert!(run.text().ends_with("}\n"));
}

#[test]
fn inventory_redact_removes_identifying_values() {
    let run = tur_device(&["inventory", "--from", support::LAPTOP, "--json", "--redact"]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    let document = run.json();
    support::validate(&document).expect("the output matches the schema");
    assert_eq!(document["redacted"], Value::Bool(true));
    assert_eq!(
        document["usb"][4]["serial"],
        serde_json::json!({ "sensitive": true, "redacted": true, "value": null })
    );
    for (path, field) in support::sensitive_fields(&document) {
        assert_eq!(field["value"], Value::Null, "{path}");
    }
    let text = run.text().to_lowercase();
    for secret in [
        "pf2abcde",
        "4c530001230915112345",
        "8c:16:45:aa:bb:01",
        "00e04c680001",
        "s5gxnx0r123456",
        "eui.0025385b21404321",
        "d0:ab:cd:44:55:66",
        "00a1b2c3-0092-8f18-ffff-ffffffffffff",
    ] {
        assert!(!text.contains(secret), "{secret} survived --redact");
    }
    // The identifiers are the same as without --redact.
    let plain = tur_device(&["inventory", "--from", support::LAPTOP, "--json"]).json();
    assert_eq!(document["usb"][4]["id"], plain["usb"][4]["id"]);
    assert_eq!(document["network"][1]["id"], plain["network"][1]["id"]);
}

#[test]
fn inventory_text_lists_every_category() {
    let run = tur_device(&["inventory", "--from", support::LAPTOP]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    assert!(run.stderr.is_empty());
    let text = run.text();
    assert!(
        text.starts_with("TUR hardware inventory\n\nPlatform\n"),
        "{text}"
    );
    for expected in [
        "  System:      Example Computer Notebook 14 G2 1.0\n",
        "  Firmware:    Example Firmware N3HET74W (1.46) (03/15/2026)\n",
        "  Serial:      PF2ABCDE\n",
        "  Boot mode:   uefi\n",
        "  Secure Boot: enabled\n",
        "\nPCI devices (18)\n",
        "  0000:00:02.0  [8086:9a49]  class 030000  TigerLake-LP GT2 [Iris Xe Graphics]  driver: i915\n",
        "    id: pci-0000:00:02.0-8086:9a49\n",
        "    card1 eDP-1: connected\n",
        "  0000:04:00.0  [10de:25a2]  class 030200  TU117M [GeForce MX550]  driver: none\n",
        "\nUSB devices (8)\n",
        "  2-1  [0781:5581]  SanDisk Ultra  5000 Mbit/s  removable\n",
        "    serial: 4C530001230915112345\n",
        "    2-1:1.0  class 08  Mass storage  driver: usb-storage\n",
        "    1-3:1.0  class ff  Vendor specific  driver: none\n",
        "\nNetwork interfaces (5)\n",
        "  wlp0s20f3  wireless  state: up  driver: iwlwifi  address: 4c:79:6e:aa:bb:02\n",
        "\nSound cards (1)\n",
        "  card0  PCH  HDA Intel PCH  driver: snd_hda_intel\n",
        "\nStorage controllers (3)\n",
        "  nvme  driver: nvme\n",
        "\nBlock devices (3)\n",
        "  sdb  usb  Ultra  removable: yes  serial: 4C530001230915112345\n",
        "\nInput devices (5)\n",
        "  AT Translated Set 2 keyboard  bus: i8042  [0001:0001]  keyboard, key\n",
        "\nBluetooth adapters (1)\n",
        "  hci0  driver: btusb  blocked: no\n",
        "\nVideo devices (2)\n",
        "  video0  Integrated Camera: Integrated C  capture: yes  driver: uvcvideo\n",
        "\nThunderbolt devices (2)\n",
        "  0-1  CalDigit, Inc. TS3 Plus  not_authorised\n",
        "  0-0  Example Computer Notebook 14 G2  authorised  (host)\n",
        "\nFirmware (3)\n",
        "  fwupd: available\n",
        "  System Firmware  version 1.46  updatable: yes\n",
        "    update available: 1.52 (urgency high)\n",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
    }
    // Every source of the fixture was usable.
    assert!(!text.contains("Sources not used"));

    let redacted = tur_device(&["inventory", "--from", support::LAPTOP, "--redact"]);
    assert_eq!(redacted.status, EXIT_OK);
    let text = redacted.text();
    assert!(text.starts_with("TUR hardware inventory (redacted)\n"));
    assert!(text.contains("  Serial:      <redacted>\n"));
    assert!(text.contains("    serial: <redacted>\n"));
    assert!(
        text.contains("  enx<redacted>  ethernet  state: up  driver: r8152  address: <redacted>\n")
    );
    assert!(!text.contains("4C530001230915112345"));
    assert!(!text.contains("PF2ABCDE"));
}

#[test]
fn problems_json_is_the_library_report_and_matches_the_schema() {
    let run = tur_device(&["problems", "--from", support::LAPTOP, "--json"]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    assert!(run.stderr.is_empty());
    let document = run.json();
    support::validate_as("problem_report", &document).expect("the output matches the schema");

    let inventory = collect(&FsProbe::fixture(support::LAPTOP));
    let expected = serde_json::to_value(ProblemReport::of(&inventory)).unwrap();
    assert_eq!(document, expected);

    let codes: Vec<&str> = document["findings"]
        .as_array()
        .expect("findings is an array")
        .iter()
        .map(|finding| finding["code"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        codes,
        [
            "TUR-DEVICE-DRIVER-003",
            "TUR-DEVICE-DRIVER-001",
            "TUR-DEVICE-FIRMWARE-001",
            "TUR-DEVICE-INACTIVE-001",
            "TUR-DEVICE-DRIVER-001",
            "TUR-DEVICE-DRIVER-001",
            "TUR-DEVICE-DRIVER-001",
            "TUR-DEVICE-DRIVER-002",
            "TUR-DEVICE-FIRMWARE-001",
            "TUR-DEVICE-THUNDERBOLT-001",
        ]
    );
    assert_eq!(document["findings"][0]["severity"], "error");
    assert_eq!(
        document["findings"][0]["device_id"],
        "pci-0000:04:00.0-10de:25a2"
    );
}

#[test]
fn problems_text_has_a_message_an_action_and_detail_for_each_finding() {
    let run = tur_device(&["problems", "--from", support::LAPTOP]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    let text = run.text();
    assert!(
        text.starts_with(
            "TUR-DEVICE-DRIVER-003  [error]  Driver did not attach to the device\n\
             The driver nouveau did not accept the Display controller"
        ),
        "{text}"
    );
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("TUR-DEVICE-"))
            .count(),
        10
    );
    assert_eq!(text.matches("\nAction:\n").count(), 10);
    assert_eq!(text.matches("\nTechnical detail:\n").count(), 10);
    assert!(text.contains("  firmware_file: rtw88/rtw8822c_fw.bin\n"));
    assert!(text.contains("  device_id: pci-0000:04:00.0-10de:25a2\n"));
    assert!(text.contains(
        "TUR-DEVICE-THUNDERBOLT-001  [info]  Thunderbolt device not authorised\n\
         TS3 Plus is connected through Thunderbolt but has not been authorised"
    ));
}

#[test]
fn an_unreadable_fixture_directory_is_reported_with_a_code() {
    let file = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    for path in ["/nonexistent/tur-device/fixture", file] {
        for command in ["inventory", "problems", "list"] {
            let run = tur_device(&[command, "--from", path, "--json"]);
            assert_eq!(run.status, EXIT_FAILURE, "{command} {path}");
            assert!(run.stdout.is_empty(), "{command} {path}");
            assert!(
                run.stderr.starts_with("error: TUR-DEVICE-INPUT-001: "),
                "{command} {path}: {}",
                run.stderr
            );
        }
    }
}

/// The serial number of the USB stick in the laptop fixture.
const STICK_SERIAL: &str = "4C530001230915112345";

#[test]
fn list_prints_a_summary_of_every_device_and_no_sensitive_value() {
    let run = tur_device(&["list", "--from", support::LAPTOP, "--json"]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    assert!(run.stderr.is_empty(), "{}", run.stderr);
    let document = run.json();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["redacted"], Value::Bool(true));

    // The listing is the library's summaries of the redacted inventory.
    let inventory = collect(&FsProbe::fixture(support::LAPTOP)).redacted();
    let expected = serde_json::to_value(inventory.summaries()).unwrap();
    assert_eq!(document["devices"], expected);
    let devices = document["devices"].as_array().expect("devices");
    // At least the 18 PCI functions and the 8 USB devices of the fixture.
    assert!(devices.len() >= 26, "{}", devices.len());
    for device in devices {
        let keys: Vec<&str> = device
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["category", "driver", "id", "name"]);
        assert!(!device["id"].as_str().expect("an identifier").is_empty());
    }
    // The fixture's inventory holds a serial number; the listing does not.
    let full = tur_device(&["inventory", "--from", support::LAPTOP, "--json"]);
    assert!(full.text().contains(STICK_SERIAL));
    assert!(!run.text().contains(STICK_SERIAL));

    // Only the categories asked for, and all of those.
    let filtered = tur_device(&[
        "list",
        "--from",
        support::LAPTOP,
        "--json",
        "--category=usb",
        "--category=network",
    ]);
    assert_eq!(filtered.status, EXIT_OK, "{}", filtered.stderr);
    let wanted: Vec<&Value> = devices
        .iter()
        .filter(|device| device["category"] == "usb" || device["category"] == "network")
        .collect();
    assert!(wanted.len() >= 8);
    let listed = filtered.json();
    let listed: Vec<&Value> = listed["devices"]
        .as_array()
        .expect("devices")
        .iter()
        .collect();
    assert_eq!(listed, wanted);

    // A category is spelled as the inventory spells it, and nothing else is
    // one.
    let run = tur_device(&[
        "list",
        "--from",
        support::LAPTOP,
        "--json",
        "--category=storage_controller",
    ]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    for category in ["printer", "storage-controller", "USB", ""] {
        let option = format!("--category={category}");
        let run = tur_device(&["list", "--from", support::LAPTOP, &option]);
        assert_eq!(run.status, EXIT_USAGE, "{category}");
        assert!(run.stdout.is_empty(), "{category}");
    }

    // As text: one line for each device, beginning with its identifier.
    let text = tur_device(&["list", "--from", support::LAPTOP]);
    assert_eq!(text.status, EXIT_OK, "{}", text.stderr);
    let lines: Vec<&str> = text.text().lines().collect();
    assert_eq!(lines.len(), devices.len());
    for (line, device) in lines.iter().zip(devices) {
        let id = device["id"].as_str().expect("an identifier");
        assert!(line.starts_with(id), "{line}");
    }
    assert!(!text.text().contains(STICK_SERIAL));
}

#[test]
fn get_prints_one_device_redacted_by_the_identifier_list_printed() {
    let listed = tur_device(&["list", "--from", support::LAPTOP, "--json"]).json();
    let devices = listed["devices"].as_array().expect("devices");
    let mut redacted_values = 0;
    for summary in devices {
        let id = summary["id"].as_str().expect("an identifier");
        let run = tur_device(&["get", "--from", support::LAPTOP, "--json", "--", id]);
        assert_eq!(run.status, EXIT_OK, "{id}: {}", run.stderr);
        let document = run.json();
        assert_eq!(document["schema_version"], 1, "{id}");
        assert_eq!(document["redacted"], Value::Bool(true), "{id}");
        // The device `list` named, in the category it named.
        assert_eq!(document["category"], summary["category"], "{id}");
        assert_eq!(document["device"]["id"], summary["id"], "{id}");
        assert_eq!(document["device"]["driver"], summary["driver"], "{id}");
        // Never a serial number.
        assert!(!run.text().contains(STICK_SERIAL), "{id}");
        redacted_values += run.text().matches("\"redacted\": true").count() - 1;
        // Without --json the same document is printed.
        let plain = tur_device(&["get", "--from", support::LAPTOP, "--", id]);
        assert_eq!(plain.stdout, run.stdout, "{id}");
    }
    // Some devices of the fixture do have sensitive values, and they were
    // marked as removed rather than left out.
    assert!(redacted_values > 0);

    // An identifier no device has: its own exit status and code, nothing on
    // standard output.
    for id in ["pci-does-not-exist", "--from", "", "x\u{1b}[31m\n"] {
        let run = tur_device(&["get", "--from", support::LAPTOP, "--json", "--", id]);
        assert_eq!(run.status, 3, "{id:?}: {}", run.stderr);
        assert!(run.stdout.is_empty(), "{id:?}");
        assert!(
            run.stderr.starts_with("error: TUR-DEVICE-INPUT-002: "),
            "{id:?}: {}",
            run.stderr
        );
        // What was asked for is repeated in printable form only.
        assert!(!run.stderr.contains('\u{1b}'), "{id:?}");
        assert_eq!(run.stderr.lines().count(), 1, "{id:?}");
    }

    // No identifier at all, or two, is a usage error.
    for args in [
        &["get", "--from", support::LAPTOP][..],
        &["get", "--from", support::LAPTOP, "--", "a", "b"][..],
    ] {
        let run = tur_device(args);
        assert_eq!(run.status, EXIT_USAGE, "{args:?}");
        assert!(run.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn a_usage_error_prints_nothing_on_standard_output() {
    let command_lines: [&[&str]; 6] = [
        &[],
        &["frobnicate"],
        &["inventory", "--nope"],
        &["inventory", "--from"],
        // `problems` has no --redact: findings hold no sensitive value.
        &["problems", "--from", support::LAPTOP, "--redact"],
        &["inventory", "extra"],
    ];
    for args in command_lines {
        let run = tur_device(args);
        assert_eq!(run.status, EXIT_USAGE, "{args:?}");
        assert!(run.stdout.is_empty(), "{args:?}");
        assert!(!run.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn help_and_version_are_printed() {
    let run = tur_device(&["--help"]);
    assert_eq!(run.status, EXIT_OK);
    assert!(run.text().contains("inventory"));
    assert!(run.text().contains("problems"));
    assert!(run.text().contains("list"));
    assert!(run.text().contains("get"));

    let run = tur_device(&["list", "--help"]);
    assert_eq!(run.status, EXIT_OK);
    for option in ["--from", "--json", "--category", "storage_controller"] {
        assert!(run.text().contains(option), "{option}");
    }

    let run = tur_device(&["inventory", "--help"]);
    assert_eq!(run.status, EXIT_OK);
    for option in ["--from", "--json", "--redact"] {
        assert!(run.text().contains(option), "{option}");
    }

    let run = tur_device(&["--version"]);
    assert_eq!(run.status, EXIT_OK);
    assert_eq!(run.text(), "tur-device 0.1.0\n");
}

#[test]
fn an_empty_directory_gives_an_empty_inventory() {
    let dir = scratch("empty");
    let from = dir.to_str().expect("the scratch path is UTF-8");

    let run = tur_device(&["inventory", "--from", from, "--json"]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    let document = run.json();
    support::validate(&document).expect("the output matches the schema");
    assert_eq!(document["pci"], serde_json::json!([]));
    assert_eq!(document["platform"]["boot_mode"], "unknown");
    assert_eq!(document["firmware"]["fwupd"], "unavailable");
    assert_eq!(document["kernel"]["available"], Value::Bool(false));
    // With --from no command is run, so every command-backed source is
    // reported as not used rather than answered from the host.
    let sources: Vec<&str> = document["source_notes"]
        .as_array()
        .expect("source_notes is an array")
        .iter()
        .map(|note| note["source"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        sources,
        [
            "machine-id",
            "udev",
            "kernel-module-aliases",
            "fwupd",
            "kernel-log"
        ]
    );

    let run = tur_device(&["problems", "--from", from]);
    assert_eq!(run.status, EXIT_OK);
    assert_eq!(run.text(), "No hardware problems found.\n");

    let run = tur_device(&["problems", "--from", from, "--json"]);
    assert_eq!(
        run.json(),
        serde_json::json!({ "schema_version": 1, "findings": [] })
    );
    fs::remove_dir_all(dir).expect("the scratch directory can be removed");
}

#[test]
fn hostile_descriptor_strings_on_disk_do_not_reach_the_output() {
    let dir = scratch("hostile");
    let from = dir.to_str().expect("the scratch path is UTF-8");

    let device = "sys/devices/pci0000:00/0000:00:14.0/usb1/1-2";
    link(
        &dir,
        "sys/bus/usb/devices/1-2",
        "../../../devices/pci0000:00/0000:00:14.0/usb1/1-2",
    );
    write(&dir, &format!("{device}/idVendor"), b"dead\n");
    write(&dir, &format!("{device}/idProduct"), b"beef\n");
    write(&dir, &format!("{device}/bDeviceClass"), b"00\n");
    // An escape sequence that would retitle a terminal and clear the screen.
    write(
        &dir,
        &format!("{device}/manufacturer"),
        b"\x1b]0;pwned\x07\x1b[2JAcme\x00\n",
    );
    // Invalid UTF-8, including an overlong encoding and a lone surrogate.
    write(
        &dir,
        &format!("{device}/product"),
        b"Disk \xff\xfe\xc0\xaf\xed\xa0\x80 \xe2\x80\xae drive\n",
    );
    // Far longer than a descriptor string can be.
    write(&dir, &format!("{device}/serial"), &b"S".repeat(1_000_000));
    write(&dir, &format!("{device}/1-2:1.0/bInterfaceClass"), b"08\n");
    write(&dir, &format!("{device}/1-2:1.0/modalias"), &[0xff; 512]);
    // A link that points out of the tree, and a link to itself.
    link(&dir, &format!("{device}/1-2:1.0/driver"), "/etc/passwd");
    link(&dir, "sys/bus/usb/devices/1-9", "1-9");
    // A directory entry whose name is not UTF-8.
    fs::create_dir_all(
        dir.join("sys/class/net")
            .join(OsStr::from_bytes(b"eth\xff\xfe")),
    )
    .expect("a directory with a non-UTF-8 name can be created");
    write(
        &dir,
        "etc/machine-id",
        b"0123456789abcdef0123456789abcdef\n",
    );
    write(
        &dir,
        "commands/fwupdmgr-get-devices.json",
        b"{\"Devices\": [{\"Name\": 7}]",
    );
    write(&dir, "commands/udevadm-info-export-db.txt", &[0x00; 4096]);
    write(&dir, "commands/journalctl-kernel.txt", b"\x1b[31m\xff\n");

    let run = tur_device(&["inventory", "--from", from, "--json"]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    assert!(run.stderr.is_empty(), "{}", run.stderr);
    let document = run.json();
    support::validate(&document).expect("the output matches the schema");
    assert_eq!(document["usb"].as_array().map(Vec::len), Some(1));
    let usb = &document["usb"][0];
    assert_eq!(usb["manufacturer"], "]0;pwned [2JAcme");
    assert_eq!(
        usb["product"],
        "Disk \u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD} drive"
    );
    assert_eq!(
        usb["serial"]["value"]
            .as_str()
            .map(|text| text.chars().count()),
        Some(256)
    );
    assert_eq!(usb["interfaces"][0]["modalias"], Value::Null);
    assert_eq!(usb["interfaces"][0]["driver"], "passwd");
    assert_eq!(document["network"], serde_json::json!([]));
    assert_eq!(document["firmware"]["fwupd"], "invalid");
    for (path, text) in support::strings(&document) {
        assert!(!text.chars().any(char::is_control), "{path}: {text:?}");
    }
    // The raw output holds no escape byte either: only printable text and
    // line feeds.
    assert!(
        run.stdout
            .iter()
            .all(|byte| *byte == b'\n' || *byte >= 0x20),
        "control bytes in the JSON output"
    );

    let command_lines: [&[&str]; 3] = [
        &["inventory", "--from", from],
        &["inventory", "--from", from, "--redact"],
        &["problems", "--from", from],
    ];
    for args in command_lines {
        let run = tur_device(args);
        assert_eq!(run.status, EXIT_OK, "{args:?}: {}", run.stderr);
        let text = run.text();
        assert!(
            !text.chars().any(|ch| ch.is_control() && ch != '\n'),
            "{args:?} printed a control character"
        );
        assert!(!text.contains('\u{202e}'), "{args:?}");
    }
    let redacted = tur_device(&["inventory", "--from", from, "--redact", "--json"]);
    assert!(!redacted.text().contains("SSSSSSSS"));

    let run = tur_device(&["problems", "--from", from, "--json"]);
    assert_eq!(run.status, EXIT_OK, "{}", run.stderr);
    support::validate_as("problem_report", &run.json()).expect("the findings match the schema");
    fs::remove_dir_all(dir).expect("the scratch directory can be removed");
}
