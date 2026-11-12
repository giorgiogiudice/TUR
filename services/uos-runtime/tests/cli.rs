// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! End-to-end tests of the `uos-runtime` binary: its output and exit status.
//!
//! The inputs are written byte by byte into a scratch directory. The binary
//! under test only reads them; nothing here is ever executed except the
//! `uos-runtime` binary itself.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

/// A scratch directory removed on drop.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let process = std::process::id();
        let path = std::env::temp_dir().join(format!("uos-runtime-cli-{process}-{unique}-{label}"));
        std::fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }

    /// Writes a file and returns its path as text.
    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.path.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct Outcome {
    status: i32,
    stdout: String,
    stderr: String,
}

fn run(arguments: &[&str]) -> Outcome {
    let output = Command::new(env!("CARGO_BIN_EXE_uos-runtime"))
        .args(arguments)
        .output()
        .unwrap();
    Outcome {
        status: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// Runs `inspect` on an x86-64 host with exactly `available` installed.
fn inspect(path: &str, available: &str, json: bool) -> Outcome {
    let mut arguments = vec![
        "inspect",
        path,
        "--host-arch",
        "x86_64",
        "--available",
        available,
    ];
    if json {
        arguments.push("--json");
    }
    run(&arguments)
}

/// A 64-byte ELF header: a fixed-address x86-64 executable with no program
/// headers.
fn elf_executable() -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2; // ELFCLASS64
    bytes[5] = 1; // little-endian
    bytes[6] = 1; // version
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
    bytes
}

/// A PE32+ image for `machine` with the given subsystem.
fn pe(machine: u16, subsystem: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 0x80 + 24 + 240];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&240u16.to_le_bytes()); // SizeOfOptionalHeader
    bytes[0x96..0x98].copy_from_slice(&0x0022u16.to_le_bytes()); // Characteristics
    bytes[0x98..0x9a].copy_from_slice(&0x020bu16.to_le_bytes()); // PE32+ magic
    bytes[0x98 + 68..0x98 + 70].copy_from_slice(&subsystem.to_le_bytes());
    bytes
}

/// A thin 64-bit little-endian Mach-O executable header for ARM64.
fn macho_arm64() -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in [0xfeed_facf_u32, 0x0100_000c, 0, 2, 0, 0, 0, 0] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes
}

const VALID_PROFILE: &str = r#"{
    "schema_version": 1,
    "app_id": "com.example.application",
    "display_name": "Example Application",
    "binary": "/path/to/application.exe",
    "format": "pe",
    "architecture": "x86_64",
    "runtime": "wine",
    "prefix_id": "example-application",
    "isolation_profile": "desktop-standard",
    "fallback": "windows-vm-offer",
    "test_status": "untested"
}"#;

#[test]
fn native_program_is_reported_whatever_its_name() {
    let scratch = Scratch::new("native");
    let path = scratch.file("setup.exe", &elf_executable());
    let outcome = inspect(&path, "", false);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(
        outcome
            .stdout
            .contains("ELF 64-bit little-endian for x86_64, executable")
    );
    assert!(outcome.stdout.contains("native Linux"));
    assert!(outcome.stdout.contains("elf-matches-host"));
    assert!(outcome.stderr.is_empty());
}

#[test]
fn windows_program_needs_wine() {
    let scratch = Scratch::new("wine");
    let path = scratch.file("app.bin", &pe(0x8664, 2));

    let with_wine = inspect(&path, "wine", false);
    assert_eq!(with_wine.status, 0, "{}", with_wine.stderr);
    assert!(with_wine.stdout.contains("Wine"));
    assert!(!with_wine.stdout.contains("Not installed"));

    let without_wine = inspect(&path, "darling,android", false);
    assert_eq!(without_wine.status, 4);
    assert!(without_wine.stdout.contains("Not installed"));
    assert!(without_wine.stdout.contains("UOS-WINDOWS-RUNTIME-001"));
}

#[test]
fn json_report_has_inspection_recommendation_and_host() {
    let scratch = Scratch::new("json");
    let path = scratch.file("driver.sys", &pe(0x8664, 1));
    let outcome = inspect(&path, "windows-vm, wine", true);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(report["path"], path.as_str());
    assert_eq!(report["host"]["arch"], "x86_64");
    assert_eq!(report["host"]["available"][0], "wine");
    assert_eq!(report["host"]["available"][1], "windows-vm");
    assert_eq!(report["inspection"]["detected"]["format"], "pe");
    assert_eq!(report["inspection"]["detected"]["subsystem"], "native");
    assert_eq!(
        report["inspection"]["detected"]["requires_windows_kernel"],
        true
    );
    assert_eq!(report["recommendation"]["runtime"], "windows-vm-offer");
    assert_eq!(
        report["recommendation"]["reasons"][0],
        "pe-requires-windows-kernel"
    );
    assert_eq!(report["recommendation"]["code"], "UOS-WINDOWS-RUNTIME-002");
    let explanation = report["recommendation"]["explanation"].as_str().unwrap();
    assert!(explanation.contains("requires a Windows kernel"));

    // Without the virtual machine backend the offer stands, and the status
    // says that something is missing.
    let outcome = inspect(&path, "wine", true);
    assert_eq!(outcome.status, 4);
    let report: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(report["recommendation"]["missing"][0], "windows-vm");
    assert_eq!(report["recommendation"]["code"], "UOS-WINDOWS-RUNTIME-003");
}

#[test]
fn unsupported_files_exit_with_status_three() {
    let scratch = Scratch::new("unsupported");

    let arm_mac = scratch.file("tool", &macho_arm64());
    let outcome = inspect(&arm_mac, "wine,darling,android,arm-translation,windows-vm", false);
    assert_eq!(outcome.status, 3);
    assert!(outcome.stdout.contains("unsupported"));
    assert!(outcome.stdout.contains("mach-o-apple-silicon-only"));
    assert!(outcome.stdout.contains("UOS-DARWIN-RUNTIME-003"));

    let arm_windows = scratch.file("arm.exe", &pe(0xaa64, 2));
    let outcome = inspect(&arm_windows, "wine", false);
    assert_eq!(outcome.status, 3);
    assert!(outcome.stdout.contains("pe-foreign-architecture"));

    let unknown = scratch.file("notes.apk", b"this is not a package");
    let outcome = inspect(&unknown, "android", true);
    assert_eq!(outcome.status, 3);
    let report: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(report["inspection"]["detected"]["format"], "unknown");
    assert_eq!(report["recommendation"]["runtime"], "unsupported");
    assert_eq!(report["recommendation"]["code"], "UOS-DESKTOP-RUNTIME-004");
}

#[test]
fn unreadable_and_damaged_input_exits_with_status_one_and_a_code() {
    let scratch = Scratch::new("damaged");

    let missing = scratch.path.join("no-such-file");
    let outcome = inspect(missing.to_str().unwrap(), "", false);
    assert_eq!(outcome.status, 1);
    assert!(outcome.stdout.is_empty());
    assert!(
        outcome
            .stderr
            .starts_with("error[UOS-DESKTOP-RUNTIME-001]: ")
    );

    let image = pe(0x8664, 2);
    let truncated = scratch.file("cut.exe", &image[..0x80 + 10]);
    let outcome = inspect(&truncated, "wine", false);
    assert_eq!(outcome.status, 1);
    assert!(
        outcome
            .stderr
            .starts_with("error[UOS-WINDOWS-INSPECT-001]: ")
    );

    // With --json the error is a JSON document on standard output.
    let outcome = inspect(&truncated, "wine", true);
    assert_eq!(outcome.status, 1);
    let report: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(report["error"]["code"], "UOS-WINDOWS-INSPECT-001");
    assert!(report["error"]["message"].as_str().unwrap().contains("truncated"));
}

#[test]
fn wrong_command_lines_exit_with_status_two() {
    let scratch = Scratch::new("usage");
    let path = scratch.file("tool", &elf_executable());

    let outcome = inspect(&path, "wine,dosbox", false);
    assert_eq!(outcome.status, 2);
    assert!(outcome.stderr.contains("dosbox"));

    let outcome = run(&["inspect", path.as_str(), "--host-arch", "pdp11"]);
    assert_eq!(outcome.status, 2);
    assert!(outcome.stderr.contains("pdp11"));

    assert_eq!(run(&["inspect"]).status, 2);
    assert_eq!(run(&["launch", path.as_str()]).status, 2);
    assert_eq!(run(&[]).status, 2);
}

#[test]
fn inspect_without_overrides_detects_the_host() {
    let scratch = Scratch::new("detect");
    let path = scratch.file("run.sh", b"#!/bin/sh\necho hello\n");
    let outcome = run(&["inspect", path.as_str(), "--json"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(report["inspection"]["detected"]["format"], "script");
    assert_eq!(report["inspection"]["detected"]["interpreter"], "/bin/sh");
    assert_eq!(report["recommendation"]["runtime"], "native-linux");
    assert!(report["host"]["arch"].is_string());
    assert!(report["host"]["available"].is_array());
}

#[test]
fn inspecting_never_changes_or_runs_the_file() {
    let scratch = Scratch::new("inert");
    // If this script were ever run it would create a marker file.
    let marker = scratch.path.join("marker");
    let script = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
    let path = scratch.file("install.sh", script.as_bytes());
    let before = std::fs::read(&path).unwrap();

    let outcome = inspect(&path, "", false);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(!marker.exists());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn valid_profile_is_accepted() {
    let scratch = Scratch::new("profile-ok");
    let path = scratch.file("profile.json", VALID_PROFILE.as_bytes());
    let outcome = run(&["profile", "validate", path.as_str()]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(outcome.stdout.starts_with("valid: com.example.application"));
    assert!(outcome.stderr.is_empty());
}

#[test]
fn invalid_profiles_exit_with_status_five_and_a_code() {
    let scratch = Scratch::new("profile-bad");

    let with_secret = VALID_PROFILE.replace(
        "\"schema_version\": 1,",
        "\"schema_version\": 1, \"wine_password\": \"hunter2\",",
    );
    let path = scratch.file("secret.json", with_secret.as_bytes());
    let outcome = run(&["profile", "validate", path.as_str()]);
    assert_eq!(outcome.status, 5);
    assert!(
        outcome
            .stderr
            .starts_with("error[UOS-DESKTOP-PROFILE-004]: ")
    );
    assert!(outcome.stderr.contains("wine_password"));
    assert!(!outcome.stderr.contains("hunter2"));
    assert!(outcome.stdout.is_empty());

    let unknown_field = VALID_PROFILE.replace(
        "\"schema_version\": 1,",
        "\"schema_version\": 1, \"colour\": \"blue\",",
    );
    let path = scratch.file("unknown.json", unknown_field.as_bytes());
    let outcome = run(&["profile", "validate", path.as_str()]);
    assert_eq!(outcome.status, 5);
    assert!(
        outcome
            .stderr
            .starts_with("error[UOS-DESKTOP-PROFILE-002]: ")
    );

    let bad_runtime = VALID_PROFILE.replace("\"runtime\": \"wine\"", "\"runtime\": \"darling\"");
    let path = scratch.file("runtime.json", bad_runtime.as_bytes());
    let outcome = run(&["profile", "validate", path.as_str()]);
    assert_eq!(outcome.status, 5);
    assert!(
        outcome
            .stderr
            .starts_with("error[UOS-DESKTOP-PROFILE-003]: ")
    );
}

#[test]
fn unreadable_profile_exits_with_status_one() {
    let scratch = Scratch::new("profile-missing");
    let missing = scratch.path.join("absent.json");
    let outcome = run(&["profile", "validate", missing.to_str().unwrap()]);
    assert_eq!(outcome.status, 1);
    assert!(
        outcome
            .stderr
            .starts_with("error[UOS-DESKTOP-PROFILE-001]: ")
    );
}
