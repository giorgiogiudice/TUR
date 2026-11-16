// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! End-to-end tests of the `mac` commands.
//!
//! The binary is given a scratch home and scratch XDG directories. The Mach-O
//! programs are made here, byte by byte, from the published layout of the
//! format; none is Apple's. Nothing starts Darling: what is carried out for
//! real is what reads (identify, route, status, list, show), the refusals,
//! dry runs, and the removal of an application whose files are put on disk
//! by the test. Installing and starting real applications in Darling is the
//! job of the `macos-darling` workflow (tests/macos-darling/integration.py).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;
use tur_runtime::macos::darling::{DarlingMode, PrefixLayout};
use tur_runtime::macos::record::{
    DarlingRecord, LauncherRecord, MacAppRecord, RECORD_FILE, RECORD_SCHEMA_VERSION,
    SourceRecord,
};

const X86_64: u32 = 0x0100_0007;
const ARM64: u32 = 0x0100_000c;
const APPKIT: &str = "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit";
const SWIFTUI: &str = "/System/Library/Frameworks/SwiftUI.framework/Versions/A/SwiftUI";

fn put(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// `LC_LOAD_DYLIB` for `path`.
fn dylib(path: &str) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, 0xc);
    put(&mut out, 0);
    put(&mut out, 24);
    put(&mut out, 2);
    put(&mut out, 0x0001_0000);
    put(&mut out, 0x0001_0000);
    out.extend_from_slice(path.as_bytes());
    out.push(0);
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
    let size = u32::try_from(out.len()).unwrap();
    out[4..8].copy_from_slice(&size.to_le_bytes());
    out
}

/// `LC_MAIN`.
fn main_command() -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, 0x8000_0028);
    put(&mut out, 24);
    out.extend_from_slice(&[0u8; 16]);
    out
}

/// A thin 64-bit little-endian program linking `libraries`.
fn program(cpu: u32, libraries: &[&str]) -> Vec<u8> {
    let mut commands = vec![main_command()];
    commands.extend(libraries.iter().map(|path| dylib(path)));
    let body: Vec<u8> = commands.concat();
    let mut out = Vec::new();
    put(&mut out, 0xfeed_facf);
    put(&mut out, cpu);
    put(&mut out, 3);
    put(&mut out, 2);
    put(&mut out, u32::try_from(commands.len()).unwrap());
    put(&mut out, u32::try_from(body.len()).unwrap());
    put(&mut out, 0);
    put(&mut out, 0);
    out.extend_from_slice(&body);
    out.extend_from_slice(&[0xc3u8; 64]);
    out
}

/// A universal file of the two programs.
fn universal(first: (u32, Vec<u8>), second: (u32, Vec<u8>)) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0xcafe_babeu32.to_be_bytes());
    out.extend_from_slice(&2u32.to_be_bytes());
    let first_at = 4096usize;
    let second_at = (first_at + first.1.len()).div_ceil(4096) * 4096;
    for ((cpu, image), at) in [(&first, first_at), (&second, second_at)] {
        out.extend_from_slice(&cpu.to_be_bytes());
        out.extend_from_slice(&3u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(at).unwrap().to_be_bytes());
        out.extend_from_slice(&u32::try_from(image.len()).unwrap().to_be_bytes());
        out.extend_from_slice(&12u32.to_be_bytes());
    }
    out.resize(first_at, 0);
    out.extend_from_slice(&first.1);
    out.resize(second_at, 0);
    out.extend_from_slice(&second.1);
    out
}

struct Outcome {
    status: i32,
    stdout: String,
    stderr: String,
}

impl Outcome {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|error| panic!("{error}: {}\n{}", self.stdout, self.stderr))
    }
}

/// A scratch home with its own data and state directories.
struct Home {
    root: PathBuf,
}

impl Home {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let created = std::env::temp_dir().join(format!(
            "tur-runtime-mac-{}-{unique}-{label}",
            std::process::id()
        ));
        fs::create_dir_all(created.join("home/Downloads")).unwrap();
        Home {
            root: fs::canonicalize(&created).unwrap(),
        }
    }

    /// An application bundle in Downloads whose main program is `bytes`.
    fn bundle(&self, name: &str, bytes: &[u8]) -> String {
        let bundle = self.root.join("home/Downloads").join(format!("{name}.app"));
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
                 <key>CFBundleExecutable</key><string>{name}</string>\
                 <key>CFBundleIdentifier</key><string>org.example.{}</string>\
                 <key>CFBundleName</key><string>{name}</string>\
                 </dict></plist>",
                name.to_lowercase()
            ),
        )
        .unwrap();
        fs::write(bundle.join("Contents/MacOS").join(name), bytes).unwrap();
        bundle.to_str().unwrap().to_owned()
    }

    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.root.join("home/Downloads").join(name);
        fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_owned()
    }

    fn data(&self, relative: &str) -> PathBuf {
        self.root.join("data/tur/runtime").join(relative)
    }

    fn run(&self, arguments: &[&str]) -> Outcome {
        let output = Command::new(env!("CARGO_BIN_EXE_tur-runtime"))
            .args(arguments)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("LANG", "en_GB.UTF-8")
            .output()
            .unwrap();
        Outcome {
            status: output.status.code().unwrap(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn refusal_code(document: &Value) -> Option<String> {
    document["application"]["findings"]
        .as_array()?
        .iter()
        .find(|finding| {
            matches!(
                finding["severity"].as_str(),
                Some("refusal" | "refused-unless-allowed")
            )
        })
        .and_then(|finding| finding["code"].as_str().map(str::to_owned))
}

#[test]
fn identify_tells_runnable_from_refused() {
    let home = Home::new("identify");
    let intel = home.bundle(
        "Intel",
        &universal(
            (ARM64, program(ARM64, &[APPKIT])),
            (X86_64, program(X86_64, &[APPKIT])),
        ),
    );
    let outcome = home.run(&["mac", "identify", "--json", "--", &intel]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let document = outcome.json();
    assert_eq!(document["kind"], "app-bundle");
    assert_eq!(document["verdict"], "runnable");
    assert_eq!(document["application"]["program"]["slice"], "x86_64");
    assert_eq!(
        document["application"]["program"]["frameworks"],
        serde_json::json!(["AppKit"])
    );

    let silicon = home.bundle("Silicon", &program(ARM64, &[APPKIT]));
    let outcome = home.run(&["mac", "identify", "--json", "--", &silicon]);
    assert_eq!(outcome.status, 3);
    let document = outcome.json();
    assert_eq!(document["verdict"], "refused");
    assert_eq!(refusal_code(&document).as_deref(), Some("TUR-RUNTIME-MACOS-002"));

    let swiftui = home.bundle("Panels", &program(X86_64, &[APPKIT, SWIFTUI]));
    let outcome = home.run(&["mac", "identify", "--json", "--", &swiftui]);
    assert_eq!(outcome.status, 3);
    assert_eq!(
        refusal_code(&outcome.json()).as_deref(),
        Some("TUR-RUNTIME-MACOS-004")
    );

    // A disk image is told by its trailer and not opened.
    let mut image = vec![0u8; 8192];
    image[8192 - 512..8192 - 508].copy_from_slice(b"koly");
    let dmg = home.file("Example.dmg", &image);
    let outcome = home.run(&["mac", "identify", "--json", "--", &dmg]);
    assert_eq!(outcome.status, 0);
    assert_eq!(outcome.json()["kind"], "disk-image");

    // Not a Mac package: a refusal with its code, in JSON and in text.
    let text = home.file("notes.txt", b"plain text");
    let outcome = home.run(&["mac", "identify", "--json", "--", &text]);
    assert_eq!(outcome.status, 3);
    assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-001");
    let outcome = home.run(&["mac", "identify", "--", &text]);
    assert!(outcome.stderr.starts_with("error[TUR-RUNTIME-MACOS-001]"));
}

#[test]
fn route_order_and_refusal() {
    let home = Home::new("route");
    let intel = home.bundle("Intel", &program(X86_64, &[APPKIT]));
    let outcome = home.run(&["mac", "route", "--json", &intel]);
    assert_eq!(outcome.status, 3);
    let decision = outcome.json();
    assert!(decision["chosen"].is_null());
    assert_eq!(decision["refusal"], "TUR-RUNTIME-MACOS-020");

    let outcome = home.run(&["mac", "route", "--json", "--darling-measured", &intel]);
    assert_eq!(outcome.status, 0);
    assert_eq!(outcome.json()["chosen"], "darling");

    let outcome = home.run(&[
        "mac",
        "route",
        "--json",
        "--darling-measured",
        "--rehost",
        "portable",
        &intel,
    ]);
    assert_eq!(outcome.json()["chosen"], "rehost");

    let outcome = home.run(&["mac", "route", "--json", "--linux-edition", &intel]);
    assert_eq!(outcome.json()["chosen"], "linux-edition");

    // Measured or not, an Apple-silicon-only program has no Darling route.
    let silicon = home.bundle("Silicon", &program(ARM64, &[APPKIT]));
    let outcome = home.run(&["mac", "route", "--json", "--darling-measured", &silicon]);
    assert_eq!(outcome.status, 3);
    assert!(
        outcome.json()["message"]
            .as_str()
            .unwrap()
            .contains("Apple silicon")
    );

    let outcome = home.run(&["mac", "route", "--rehost", "maybe", &intel]);
    assert_eq!(outcome.status, 2);
}

#[test]
fn install_refuses_before_anything_is_made() {
    let home = Home::new("install-refusals");
    let silicon = home.bundle("Silicon", &program(ARM64, &[APPKIT]));
    let outcome = home.run(&["mac", "install", "--json", "--dry-run", &silicon]);
    assert_eq!(outcome.status, 3, "{}", outcome.stdout);
    assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-002");

    let text = home.file("notes.txt", b"plain text");
    let outcome = home.run(&["mac", "install", "--json", &text]);
    assert_eq!(outcome.status, 3);
    assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-001");

    let bare = home.file("tool", &program(X86_64, &[]));
    let outcome = home.run(&["mac", "install", "--json", &bare]);
    assert_eq!(outcome.status, 3);
    assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-001");

    let intel = home.bundle("Intel", &program(X86_64, &[APPKIT]));
    let outcome = home.run(&["mac", "install", "--json", "--darling-mode", "root", &intel]);
    assert_eq!(outcome.status, 2);
    assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-018");

    // On any system, with the build not published and not here, the
    // installation stops before it makes anything.
    let outcome = home.run(&[
        "mac",
        "install",
        "--json",
        "--any-system",
        "--sandbox-program",
        "/usr/bin/bwrap",
        &intel,
    ]);
    assert_eq!(outcome.status, 4, "{}", outcome.stdout);
    assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-008");
    assert!(!home.data("prefixes").exists());
    let outcome = home.run(&["mac", "install", "--json", "--any-system", "--no-fetch", &intel]);
    assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-007");
}

#[test]
fn the_runtime_is_imported_only_with_its_digest() {
    let home = Home::new("runtime");
    let outcome = home.run(&["mac", "runtime", "status", "--json"]);
    assert_eq!(outcome.status, 0);
    if std::env::consts::ARCH == "x86_64" {
        assert_eq!(outcome.json()["state"], "not-published");
        // The manifest names no published build yet: an archive is taken
        // only with its digest, and a wrong one is refused.
        let archive = home.file("darling.tar.xz", b"not really an archive");
        let outcome = home.run(&[
            "mac", "runtime", "fetch", "--json", "--any-system", "--from-file", &archive,
        ]);
        assert_eq!(outcome.status, 1, "{}", outcome.stdout);
        assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-008");
        let wrong = "0".repeat(64);
        let outcome = home.run(&[
            "mac",
            "runtime",
            "fetch",
            "--json",
            "--any-system",
            "--from-file",
            &archive,
            "--sha256",
            &wrong,
        ]);
        assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-009");
        assert!(!home.data("darling").exists());
    }
}

/// Puts an installed macOS application on disk the way `mac install` leaves
/// one.
fn write_application(home: &Home, app_id: &str) -> MacAppRecord {
    let app = home.data("prefixes").join(app_id);
    let launchers = home.data("launchers").join(app_id);
    let logs = home.root.join("state/tur/runtime/logs").join(app_id);
    fs::create_dir_all(app.join("prefix/darling/Applications/Example.app/Contents/MacOS")).unwrap();
    fs::create_dir_all(app.join("home/Documents")).unwrap();
    fs::create_dir_all(&launchers).unwrap();
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("20261116T120000Z-1.log"), b"launched\n").unwrap();
    let entry = launchers.join(format!("tur-darling-{app_id}.desktop"));
    fs::write(&entry, b"[Desktop Entry]\n").unwrap();
    let record = MacAppRecord {
        schema_version: RECORD_SCHEMA_VERSION,
        app_id: app_id.to_owned(),
        name: "Example".to_owned(),
        version: Some("1.0".to_owned()),
        bundle_identifier: Some("org.example.example".to_owned()),
        bundle: "Example.app".to_owned(),
        executable: "Contents/MacOS/Example".to_owned(),
        slice: "x86_64".to_owned(),
        runtime: "darling".to_owned(),
        darling: DarlingRecord {
            artifact: "darling-3fcd2e1aa850-qc89ea8ce-fc45-x86_64".to_owned(),
            commit: "3fcd2e1aa85079789d19d82415695bdecc512241".to_owned(),
            mode: DarlingMode::Nonroot,
            layout: PrefixLayout::Layered,
        },
        network: true,
        allowed_swift: false,
        installed_at: "2026-11-16T12:00:00Z".to_owned(),
        source: SourceRecord {
            kind: "app-bundle".to_owned(),
            name: "Example.app".to_owned(),
            size: 0,
            sha256: None,
        },
        method: "copied-bundle".to_owned(),
        findings: Vec::new(),
        launcher: LauncherRecord {
            desktop_entry: entry.to_str().unwrap().to_owned(),
            installed: false,
        },
    };
    fs::write(app.join(RECORD_FILE), serde_json::to_string(&record).unwrap()).unwrap();
    record
}

#[test]
fn list_show_launch_refusal_and_uninstall() {
    let home = Home::new("installed");
    let record = write_application(&home, "org.example.example");

    let outcome = home.run(&["mac", "list", "--json"]);
    assert_eq!(outcome.status, 0);
    assert_eq!(outcome.json()["applications"][0]["app_id"], "org.example.example");

    let outcome = home.run(&["mac", "show", "--json", "org.example.example"]);
    assert_eq!(outcome.status, 0);
    assert_eq!(outcome.json()["bundle"], "Example.app");

    // Its Darling build is not here: the launch says so and starts nothing.
    let outcome = home.run(&[
        "mac",
        "launch",
        "--json",
        "--any-system",
        "--sandbox-program",
        "/usr/bin/bwrap",
        "org.example.example",
    ]);
    assert_eq!(outcome.status, 4, "{}", outcome.stdout);
    assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-007");

    // A dry run of the removal names every path and touches none.
    let outcome = home.run(&["mac", "uninstall", "--json", "--dry-run", "org.example.example"]);
    assert_eq!(outcome.status, 0);
    assert!(home.data("prefixes/org.example.example").exists());

    let outcome = home.run(&["mac", "uninstall", "--json", "org.example.example"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stdout);
    assert!(!home.data("prefixes/org.example.example").exists());
    assert!(!home.data("launchers/org.example.example").exists());
    assert!(!home.root.join("state/tur/runtime/logs/org.example.example").exists());
    assert!(!PathBuf::from(&record.launcher.desktop_entry).exists());

    // Gone, and a Windows application's directory is never removed here.
    let outcome = home.run(&["mac", "uninstall", "--json", "org.example.example"]);
    assert_eq!(outcome.status, 6);
    assert_eq!(outcome.json()["error"]["code"], "TUR-RUNTIME-MACOS-012");
    fs::create_dir_all(home.data("prefixes/com.example.windows/prefix/drive_c")).unwrap();
    let outcome = home.run(&["mac", "uninstall", "--json", "com.example.windows"]);
    assert_eq!(outcome.status, 6);
    assert!(home.data("prefixes/com.example.windows/prefix/drive_c").exists());
}
