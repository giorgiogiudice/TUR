// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! End-to-end test of the bundle path through the public API: collect from a
//! fake host into a file in a temporary directory, verify the file, then
//! damage it and check that verification fails with the right error code.
//!
//! Nothing here touches the real machine apart from the temporary directory.

use std::fs;
use std::path::PathBuf;

use uos_diag::bundle::{CHECKSUMS_PATH, MANIFEST_PATH, pack, unpack};
use uos_diag::manifest::{MissingReason, format_utc};
use uos_diag::{
    COLLECTORS, CollectRequest, ErrorKind, FakeHost, KnownIdentifiers, Profile, build_bundle,
    gather_known_identifiers, verify_file, write_bundle,
};

const EPOCH: u64 = 1_793_000_000;
const SERIAL: &str = "S4EWNX0M123456";
const MAC: &str = "3c:52:82:aa:0b:1f";
const DISK_UUID: &str = "6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10";
const PUBLIC_IP: &str = "93.184.216.34";
const BOOT_ID: &str = "0f0e0d0c-0b0a-4909-8807-060504030201";
const MACHINE_ID: &str = "0123456789abcdef0123456789abcdef";

/// A directory under the system temporary directory, removed when dropped.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("uos-diag-{name}-{}", std::process::id()));
        // Left over from an earlier run that was killed; absence is the normal case.
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the temporary directory can be created");
        TempDir { path }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best effort: a leftover temporary directory must not fail a test.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A machine with a user, a host name, hardware identifiers and a few tools
/// that are missing, broken or slow.
fn fake_machine() -> FakeHost {
    let journal = [
        format!(
            r#"{{"MESSAGE":"Accepted publickey for alice from {PUBLIC_IP} port 51234","_HOSTNAME":"workstation-7","_MACHINE_ID":"{MACHINE_ID}","_BOOT_ID":"0f0e0d0c0b0a49098807060504030201","PRIORITY":"6"}}"#
        ),
        format!(
            r#"{{"MESSAGE":"wlan0: associated with {MAC} (SSID='Casa Rossi 5G' freq=5180 MHz)","_HOSTNAME":"workstation-7","_CMDLINE":"/usr/bin/app --config /home/alice/.config/app.toml"}}"#
        ),
        format!(
            r#"{{"MESSAGE":"nvme nvme0: {SERIAL} ready; mounted /dev/disk/by-uuid/{DISK_UUID}","_HOSTNAME":"workstation-7"}}"#
        ),
    ]
    .join("\n");

    FakeHost::new()
        .with_file("/etc/passwd", "root:x:0:0:Super User:/root:/bin/bash\nalice:x:1000:1000:Alice Example:/home/alice:/bin/bash\n")
        .with_file("/proc/sys/kernel/hostname", "workstation-7\n")
        .with_file("/etc/machine-id", &format!("{MACHINE_ID}\n"))
        .with_file("/proc/sys/kernel/random/boot_id", &format!("{BOOT_ID}\n"))
        .with_file("/proc/sys/kernel/osrelease", "6.17.1-300.fc45.x86_64\n")
        .with_file(
            "/usr/lib/uos/release",
            "UOS_VERSION=0.1.0\nUOS_BUILD_ID=20261112.g0123456789ab\nUOS_SOURCE_REVISION=fedcba9876543210fedcba9876543210fedcba98\n",
        )
        .with_file("/etc/os-release", "NAME=\"Fedora Linux\"\nPRETTY_NAME=\"UOS 0.1.0\"\n")
        .with_file("/proc/cmdline", &format!("BOOT_IMAGE=/vmlinuz root=UUID={DISK_UUID} ro quiet\n"))
        .with_file("/proc/modules", "nvme 65536 2 - Live 0x0000000000000000\n")
        .with_file("/proc/mounts", &format!("/dev/nvme0n1p3 /var/home btrfs rw 0 0\n/dev/sdb1 /run/media/alice/{SERIAL} vfat rw 0 0\n"))
        .with_command("uname", "Linux workstation-7 6.17.1-300.fc45.x86_64 #1 SMP x86_64 GNU/Linux\n")
        .with_command("journalctl --boot=0 --output=json --no-pager --lines=50000", &format!("{journal}\n"))
        .with_command(
            "lsblk",
            &format!(
                "{{\n   \"blockdevices\": [\n      {{\"name\": \"nvme0n1\", \"serial\": \"{SERIAL}\", \"uuid\": \"{DISK_UUID}\", \"wwn\": \"eui.0025385b21404321\"}}\n   ]\n}}\n"
            ),
        )
        .with_command(
            "bluetoothctl",
            "Controller 3C:52:82:AA:0B:1F (public)\n\tName: workstation-7\n\tAlias: Alice's computer\n\tPowered: yes\n",
        )
        .with_command("lspci", "00:14.3 Network controller [0280]: Intel Corporation Wi-Fi 6 AX201 [8086:a0f0]\n\tKernel driver in use: iwlwifi\n")
        .with_failing_command("dmesg", 1, "dmesg: read kernel buffer failed: Operation not permitted")
        .with_command_result("fwupdmgr", Err(uos_diag::host::RunError::TimedOut))
}

fn request(host: &FakeHost, profile: Profile) -> CollectRequest<'static> {
    CollectRequest {
        profile,
        since_boot: false,
        created_epoch_seconds: EPOCH,
        collectors: COLLECTORS,
        known: gather_known_identifiers(host, &["alice".to_owned()]),
        max_bytes: 1 << 20,
    }
}

#[test]
fn collect_write_verify_then_corrupt() {
    let directory = TempDir::new("roundtrip");
    let host = fake_machine();
    let built = build_bundle(&host, &request(&host, Profile::Default)).expect("the bundle builds");

    let path = directory.file("UOS-support.tar.zst");
    write_bundle(&path, &built.archive).expect("the bundle is written");

    // The file on disk verifies and carries the manifest that was built.
    let verified = verify_file(&path).expect("the bundle verifies");
    assert_eq!(verified.manifest, built.manifest);
    assert_eq!(verified.files_verified, built.manifest.files.len() + 1);

    let manifest = &verified.manifest;
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.created_utc, format_utc(EPOCH));
    assert!(manifest.created_utc.starts_with("2026-10-26T"), "{}", manifest.created_utc);
    assert_eq!(manifest.uos.build_id.as_deref(), Some("20261112.g0123456789ab"));
    assert_eq!(manifest.boot_id.as_deref(), Some(BOOT_ID));
    assert_eq!(manifest.kernel_release.as_deref(), Some("6.17.1-300.fc45.x86_64"));
    assert_eq!(manifest.redaction_profile, Profile::Default);
    assert!(!manifest.since_boot);

    // What could not be collected is recorded with its reason, and did not
    // stop the rest.
    let reason_of = |path: &str| {
        manifest
            .missing
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.reason)
    };
    assert_eq!(reason_of("boot/kernel-ring-buffer.txt"), Some(MissingReason::NonZeroExit));
    assert_eq!(reason_of("hardware/firmware.txt"), Some(MissingReason::TimedOut));
    assert_eq!(reason_of("hardware/usb.txt"), Some(MissingReason::CommandNotFound));
    assert_eq!(reason_of("boot/previous-journal.jsonl"), Some(MissingReason::CommandNotFound));
    assert_eq!(reason_of("system/kernel.txt"), None);
    assert_eq!(manifest.files.len() + manifest.missing.len(), COLLECTORS.len() + 2);

    // Nothing that identifies the machine or its user is in any member.
    let members = unpack(&built.archive).expect("the bundle unpacks");
    assert_eq!(members.first().map(|(path, _)| path.as_str()), Some(MANIFEST_PATH));
    assert_eq!(members.last().map(|(path, _)| path.as_str()), Some(CHECKSUMS_PATH));
    for (member_path, data) in &members {
        let text = String::from_utf8(data.clone()).expect("members are UTF-8");
        for original in [
            "alice",
            "Alice",
            "workstation-7",
            SERIAL,
            MAC,
            "3C:52:82",
            DISK_UUID,
            PUBLIC_IP,
            MACHINE_ID,
            "Casa Rossi",
            "eui.0025385b21404321",
        ] {
            assert!(!text.contains(original), "{original} is in {member_path}");
        }
    }

    // Correlations survive: the serial number has one pseudonym everywhere.
    let text_of = |wanted: &str| {
        let (_, data) = members
            .iter()
            .find(|(path, _)| path.as_str() == wanted)
            .unwrap_or_else(|| panic!("{wanted} is in the bundle"));
        String::from_utf8_lossy(data).into_owned()
    };
    let journal = text_of("boot/current-journal.jsonl");
    let block_devices = text_of("hardware/block-devices.json");
    let mounts = text_of("storage/mounts.txt");
    let serial_pseudonym = block_devices
        .split("\"serial\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("lsblk output has a serial field")
        .to_owned();
    assert!(serial_pseudonym.starts_with("<serial-"), "{serial_pseudonym}");
    assert!(journal.contains(&format!("nvme nvme0: {serial_pseudonym} ready")), "{journal}");
    assert!(mounts.contains(&format!("/run/media/<user-1>/{serial_pseudonym} vfat")), "{mounts}");
    assert!(journal.contains("/home/<user-1>/.config/app.toml"), "{journal}");
    assert!(text_of("system/kernel.txt").starts_with("Linux <host-1> 6.17.1-300.fc45.x86_64"));
    assert!(!text_of("hardware/bluetooth.txt").contains("Alias"));

    // The redaction report counts and never names.
    let uuids = &built.report.categories["uuid"];
    assert_eq!(uuids.distinct_values, 1);
    assert!(uuids.replacements >= 3, "{uuids:?}");
    assert_eq!(built.report.categories["mac_address"].distinct_values, 1);
    assert_eq!(built.report.categories["wifi_ssid"].distinct_values, 1);
    assert_eq!(built.report.categories["machine_id"].distinct_values, 1);

    // One flipped byte in the file on disk is caught, with the stable code
    // for a damaged archive.
    let mut bytes = fs::read(&path).expect("the bundle can be read back");
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x01;
    let damaged_path = directory.file("damaged.tar.zst");
    fs::write(&damaged_path, &bytes).expect("the damaged copy is written");
    let error = verify_file(&damaged_path).expect_err("a damaged bundle must not verify");
    assert_eq!(error.kind(), ErrorKind::ArchiveDamaged);
    assert_eq!(error.code().to_string(), "UOS-DIAG-BUNDLE-002");
    assert!(error.to_string().starts_with("UOS-DIAG-BUNDLE-002\n"));

    // One flipped byte inside a member, in an archive that is otherwise
    // well-formed, is caught by the recorded SHA-256.
    let mut tampered = members.clone();
    let (_, kernel) = tampered
        .iter_mut()
        .find(|(path, _)| path.as_str() == "system/kernel.txt")
        .expect("system/kernel.txt is in the bundle");
    kernel[0] ^= 0x01;
    let tampered_path = directory.file("tampered.tar.zst");
    fs::write(&tampered_path, pack(&tampered, EPOCH).expect("the members pack"))
        .expect("the tampered copy is written");
    let error = verify_file(&tampered_path).expect_err("a tampered bundle must not verify");
    assert_eq!(error.kind(), ErrorKind::ChecksumMismatch);
    assert_eq!(error.code().to_string(), "UOS-DIAG-BUNDLE-007");
    assert!(error.detail().contains("system/kernel.txt"), "{}", error.detail());

    // The original is untouched by all of this and still verifies.
    verify_file(&path).expect("the original still verifies");
}

#[test]
fn an_existing_file_is_never_overwritten() {
    let directory = TempDir::new("no-overwrite");
    let path = directory.file("existing.tar.zst");
    fs::write(&path, b"precious").expect("the existing file is written");

    let error = write_bundle(&path, b"new content").expect_err("writing must be refused");
    assert_eq!(error.kind(), ErrorKind::OutputExists);
    assert_eq!(error.code().to_string(), "UOS-DIAG-COLLECT-002");
    assert_eq!(fs::read(&path).expect("the file is readable"), b"precious");

    let nowhere = directory.file("no-such-directory/bundle.tar.zst");
    let error = write_bundle(&nowhere, b"content").expect_err("the directory does not exist");
    assert_eq!(error.kind(), ErrorKind::OutputWrite);
}

#[test]
fn a_written_bundle_is_private_to_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDir::new("mode");
    let path = directory.file("bundle.tar.zst");
    write_bundle(&path, b"content").expect("the bundle is written");
    let mode = fs::metadata(&path).expect("the file exists").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn verifying_something_that_is_not_a_bundle_fails_cleanly() {
    let directory = TempDir::new("not-a-bundle");

    let absent = directory.file("absent.tar.zst");
    let error = verify_file(&absent).expect_err("the file does not exist");
    assert_eq!(error.kind(), ErrorKind::BundleRead);
    assert_eq!(error.code().to_string(), "UOS-DIAG-BUNDLE-001");

    let text = directory.file("notes.txt");
    fs::write(&text, b"just some text, not an archive").expect("the file is written");
    let error = verify_file(&text).expect_err("text is not a bundle");
    assert_eq!(error.kind(), ErrorKind::ArchiveDamaged);

    let empty = directory.file("empty.tar.zst");
    fs::write(&empty, b"").expect("the file is written");
    let error = verify_file(&empty).expect_err("an empty file is not a bundle");
    assert!(
        matches!(error.kind(), ErrorKind::ArchiveDamaged | ErrorKind::MemberMissing),
        "{error}"
    );
}

#[test]
fn the_technical_profile_keeps_what_it_says_and_nothing_else() {
    let host = fake_machine();
    let built = build_bundle(&host, &request(&host, Profile::Technical)).expect("the bundle builds");
    assert_eq!(built.manifest.redaction_profile, Profile::Technical);

    let members = unpack(&built.archive).expect("the bundle unpacks");
    let all_text: String = members
        .iter()
        .map(|(_, data)| String::from_utf8_lossy(data).into_owned())
        .collect();
    for kept in [SERIAL, MAC, DISK_UUID] {
        assert!(all_text.contains(kept), "{kept} should be kept by the technical profile");
    }
    for redacted in ["alice", "workstation-7", PUBLIC_IP, MACHINE_ID, "Casa Rossi"] {
        assert!(!all_text.contains(redacted), "{redacted} must be redacted in every profile");
    }
    let report = &built.report.categories;
    assert_eq!(report["mac_address"].replacements, 0);
    assert_eq!(report["serial_number"].replacements, 0);
    assert_eq!(report["uuid"].replacements, 0);
    assert!(report["username"].replacements > 0);
}

#[test]
fn known_identifiers_are_optional() {
    // With no advance knowledge at all, identifiers recognisable by shape or
    // context are still redacted.
    let host = fake_machine();
    let mut request = request(&host, Profile::Default);
    request.known = KnownIdentifiers::default();
    let built = build_bundle(&host, &request).expect("the bundle builds");
    let members = unpack(&built.archive).expect("the bundle unpacks");
    for (path, data) in &members {
        let text = String::from_utf8_lossy(data);
        for original in [SERIAL, MAC, DISK_UUID, PUBLIC_IP, "Casa Rossi", "/home/alice"] {
            assert!(!text.contains(original), "{original} is in {path}");
        }
    }
}
