// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! End-to-end tests of the `installer` and `app` commands.
//!
//! The binary is given a scratch home and scratch XDG directories, and the
//! runtime and the sandbox are named on the command line, so these tests
//! need neither Wine nor bubblewrap. Installations are dry runs. What is
//! carried out for real is what starts no runtime: identifying a file,
//! listing and showing applications, and removing one whose record says it
//! has no uninstaller.
//!
//! Installing real applications is the job of the `windows-runtime`
//! workflow, `tests/windows-runtime/apps.py`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;
use tur_runtime::app_record::{
    APP_RECORD_SCHEMA_VERSION, AppRecord, InstallMethod, InstallRecord, LauncherRecord,
    SourceRecord, UninstallMethod, UninstallRecord, parse_record,
};
use tur_runtime::installer::InstallerKind;
use tur_runtime::metadata::{
    PREFIX_SCHEMA_VERSION, PrefixArch, PrefixMetadata, RuntimeKind, SyncRecord,
};
use tur_runtime::profile::GraphicsBackend;
use tur_runtime::timefmt::Timestamp;

const APP: &str = "com.example.app";
/// Names the runtime and the sandbox, so that nothing has to be installed.
const HOST: [&str; 6] = [
    "--runtime-program",
    "/usr/bin/wine",
    "--runtime-version",
    "wine-9.0",
    "--sandbox-program",
    "/usr/bin/bwrap",
];

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

/// A Windows program header with no sections, followed by `appended`.
fn program(appended: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 0x80];
    out[..2].copy_from_slice(b"MZ");
    out[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    out.extend_from_slice(b"PE\0\0");
    out.extend_from_slice(&0x8664u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 14]);
    out.extend_from_slice(&240u16.to_le_bytes());
    out.extend_from_slice(&0x0022u16.to_le_bytes());
    let mut optional = vec![0u8; 240];
    optional[..2].copy_from_slice(&0x20bu16.to_le_bytes());
    optional[68..70].copy_from_slice(&2u16.to_le_bytes());
    optional[108..112].copy_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&optional);
    out.extend_from_slice(appended);
    out
}

/// A program that carries the NSIS first header.
fn nsis_installer() -> Vec<u8> {
    let mut header = vec![0u8; 4];
    header.extend_from_slice(b"\xef\xbe\xad\xdeNullsoftInst");
    header.extend_from_slice(&[0x5d; 32]);
    program(&header)
}

/// A scratch home directory with its own data and state directories.
struct Home {
    root: PathBuf,
}

impl Home {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let process = std::process::id();
        let created =
            std::env::temp_dir().join(format!("tur-runtime-app-{process}-{unique}-{label}"));
        fs::create_dir_all(&created).unwrap();
        // The temporary directory may be reached through a link.
        let root = fs::canonicalize(&created).unwrap();
        let home = Home { root };
        for directory in ["home/Downloads", "home/.ssh"] {
            fs::create_dir_all(home.root.join(directory)).unwrap();
        }
        fs::write(home.root.join("home/Downloads/setup.bin"), nsis_installer()).unwrap();
        fs::write(home.root.join("home/Downloads/tool.bin"), program(b"")).unwrap();
        fs::write(home.root.join("home/Downloads/notes.txt"), b"plain text").unwrap();
        fs::write(home.root.join("home/.ssh/id_ed25519"), nsis_installer()).unwrap();
        home
    }

    fn path(&self, relative: &str) -> String {
        self.root.join(relative).to_str().unwrap().to_owned()
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

    /// Runs a command with the host named on the command line. The options
    /// go before a `--`, so that they are not taken for the application's
    /// arguments.
    fn run_named(&self, arguments: &[&str]) -> Outcome {
        let split = arguments
            .iter()
            .position(|argument| *argument == "--")
            .unwrap_or(arguments.len());
        let (before, after) = arguments.split_at(split);
        let mut all = before.to_vec();
        all.extend(HOST);
        all.extend(after);
        self.run(&all)
    }

    /// Puts an installed application on disk the way `app install` leaves
    /// one: a prefix with its metadata, a record, a profile, a launcher
    /// entry and a log. Returns the record.
    fn write_application(&self, app_id: &str) -> AppRecord {
        let app = self.data("prefixes").join(app_id);
        let launchers = self.data("launchers").join(app_id);
        let logs = self.root.join("state/tur/runtime/logs").join(app_id);
        fs::create_dir_all(app.join("prefix/drive_c/App")).unwrap();
        fs::create_dir_all(app.join("home")).unwrap();
        fs::create_dir_all(&launchers).unwrap();
        fs::create_dir_all(&logs).unwrap();
        fs::write(app.join("prefix/drive_c/App/app.exe"), program(b"")).unwrap();
        fs::write(app.join("home/document.txt"), b"written by the application").unwrap();
        fs::write(logs.join("operations.log"), b"# log\n").unwrap();
        let metadata = PrefixMetadata {
            schema_version: PREFIX_SCHEMA_VERSION,
            app_id: app_id.to_owned(),
            prefix_id: "example-app".to_owned(),
            runtime: RuntimeKind::Wine,
            runtime_version: "wine-9.0".to_owned(),
            runtime_build: "wine-9.0 (test)".to_owned(),
            pinned: false,
            architecture: PrefixArch::Win64,
            created_at: Timestamp::from_unix(1_794_478_500),
            last_run_at: None,
            launch_count: 0,
            graphics_backend: GraphicsBackend::Auto,
            sync: SyncRecord::not_checked(),
            network: true,
            grants: Vec::new(),
            migrations: Vec::new(),
        };
        fs::write(app.join("metadata.json"), metadata.to_json()).unwrap();
        let text = |path: PathBuf| path.to_str().unwrap().to_owned();
        let entry = launchers.join(format!("tur-wine-{app_id}.desktop"));
        fs::write(&entry, "[Desktop Entry]\nType=Application\nName=Example\n").unwrap();
        let record = AppRecord {
            schema_version: APP_RECORD_SCHEMA_VERSION,
            app_id: app_id.to_owned(),
            name: "Example".to_owned(),
            version: Some("1.2".to_owned()),
            publisher: Some("Example Ltd".to_owned()),
            main_executable: text(app.join("prefix/drive_c/App/app.exe")),
            main_executable_windows: "C:\\App\\app.exe".to_owned(),
            main_executable_reason: "it is the only program that was installed".to_owned(),
            prefix: text(app.join("prefix")),
            prefix_id: "example-app".to_owned(),
            runtime: RuntimeKind::Wine,
            runtime_version: "wine-9.0".to_owned(),
            installed_at: Timestamp::from_unix(1_794_478_500),
            // As a record written before case folding was recorded.
            case_folding: None,
            install: InstallRecord {
                kind: InstallerKind::Nsis,
                method: InstallMethod::UnattendedInstaller,
                arguments: vec!["/S".to_owned()],
                source: SourceRecord {
                    file_name: "setup.exe".to_owned(),
                    sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                        .to_owned(),
                    size: 3,
                },
            },
            uninstall: UninstallRecord {
                method: UninstallMethod::Uninstaller,
                program: Some("C:\\App\\uninstall.exe".to_owned()),
                arguments: vec!["/S".to_owned()],
                registry_key: Some("HKLM\\Example".to_owned()),
            },
            launcher: LauncherRecord {
                desktop_entry: text(entry),
                icon: None,
                installed: false,
            },
            profile: text(app.join("profile.json")),
            executables: vec!["C:\\App\\app.exe".to_owned()],
            shortcuts: Vec::new(),
        };
        let profile = format!(
            r#"{{
                "schema_version": 1,
                "app_id": "{app_id}",
                "display_name": "Example",
                "binary": "{}",
                "format": "pe",
                "architecture": "x86_64",
                "runtime": "wine",
                "prefix_id": "example-app",
                "isolation_profile": "desktop-standard",
                "fallback": "windows-vm-offer",
                "test_status": "untested"
            }}"#,
            record.main_executable
        );
        fs::write(app.join("profile.json"), profile).unwrap();
        fs::write(app.join("application.json"), record.to_json()).unwrap();
        record
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_failed(outcome: &Outcome, status: i32, code: &str) {
    assert_eq!(
        outcome.status, status,
        "{}\n{}",
        outcome.stdout, outcome.stderr
    );
    assert!(
        outcome.stderr.starts_with(&format!("error[{code}]: ")),
        "{}",
        outcome.stderr
    );
    assert!(outcome.stdout.is_empty(), "{}", outcome.stdout);
}

/// What follows bubblewrap's own arguments in a planned command.
fn command_inside_sandbox(step: &Value) -> Vec<String> {
    let arguments: Vec<String> = step["command"]["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|argument| argument.as_str().unwrap().to_owned())
        .collect();
    let split = arguments
        .iter()
        .position(|argument| argument == "--")
        .unwrap();
    arguments[split + 1..].to_vec()
}

#[test]
fn an_installer_is_identified_by_content_and_nothing_is_run() {
    let home = Home::new("identify");
    // The file is called .bin: its name plays no part.
    let installer = home.path("home/Downloads/setup.bin");
    let outcome = home.run(&["installer", "identify", &installer]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(outcome.stdout.contains("Kind: NSIS installer (nsis)\n"));
    assert!(
        outcome
            .stdout
            .contains("Unattended mode: /S (documented at https://")
    );

    let outcome = home.run(&["installer", "identify", &installer, "--json"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report = outcome.json();
    assert_eq!(report["path"], installer.as_str());
    assert_eq!(report["installer"]["kind"], "nsis");
    assert_eq!(report["installer"]["architecture"], "x86_64");
    assert_eq!(report["installer"]["unattended"]["arguments"][0], "/S");
    assert!(
        report["installer"]["evidence"]
            .as_str()
            .unwrap()
            .contains("NSIS first header")
    );

    let tool = home.path("home/Downloads/tool.bin");
    let report = home.run(&["installer", "identify", &tool, "--json"]).json();
    assert_eq!(report["installer"]["kind"], "plain-exe");
    assert!(report["installer"].get("unattended").is_none());

    // What cannot be installed, and what cannot be read.
    let notes = home.path("home/Downloads/notes.txt");
    assert_failed(
        &home.run(&["installer", "identify", &notes]),
        3,
        "TUR-WINDOWS-APP-001",
    );
    let outcome = home.run(&["installer", "identify", &notes, "--json"]);
    assert_eq!(outcome.status, 3);
    assert_eq!(outcome.json()["error"]["code"], "TUR-WINDOWS-APP-001");
    let absent = home.path("home/Downloads/absent.exe");
    assert_failed(
        &home.run(&["installer", "identify", &absent]),
        1,
        "TUR-RUNTIME-MANAGER-001",
    );
    // Nothing was created anywhere.
    assert!(!home.root.join("data").exists());
    assert!(!home.root.join("state").exists());
}

#[test]
fn install_dry_run_prints_both_plans_and_changes_nothing() {
    let home = Home::new("install-dry-run");
    let installer = home.path("home/Downloads/setup.bin");
    let outcome = home.run_named(&[
        "app",
        "install",
        &installer,
        "--app-id",
        APP,
        "--dry-run",
        "--json",
    ]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report = outcome.json();
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["app_id"], APP);
    assert_eq!(report["installer"]["kind"], "nsis");
    let plans = report["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 2);
    // First a fresh prefix, then the installer in it.
    assert_eq!(plans[0]["operation"], "create");
    assert_eq!(plans[1]["operation"], "install");
    let steps = plans[1]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["step"], "run-bounded");
    assert_eq!(steps[0]["limit_seconds"], 900);
    assert_eq!(steps[0]["command"]["program"], "/usr/bin/bwrap");
    let inside = command_inside_sandbox(&steps[0]);
    assert_eq!(
        inside[inside.len() - 3..],
        ["/usr/bin/wine", installer.as_str(), "/S"]
    );
    // The installer's own file is shown, read-only; its directory and the
    // home directory are not.
    let arguments = steps[0]["command"]["arguments"].as_array().unwrap();
    let shown = arguments
        .windows(3)
        .any(|window| window[0] == "--ro-bind" && window[1] == installer.as_str());
    assert!(shown);
    let downloads = home.path("home/Downloads");
    let home_directory = home.path("home");
    for window in arguments.windows(2) {
        let mounts = window[0]
            .as_str()
            .is_some_and(|option| option.contains("bind"));
        if mounts {
            assert_ne!(window[1], downloads.as_str());
            assert_ne!(window[1], home_directory.as_str());
        }
    }

    // Text form; a time limit; an identifier made from the file's name.
    let outcome = home.run_named(&["app", "install", &installer, "--timeout", "60", "--dry-run"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(outcome.stdout.contains("Kind: NSIS installer (nsis)\n"));
    assert!(
        outcome
            .stdout
            .contains("Plan: create local.setup (7 steps)\n")
    );
    assert!(
        outcome
            .stdout
            .contains("Plan: install local.setup (1 steps)\n")
    );
    assert!(
        outcome
            .stdout
            .contains("  1. run the installer, within 60 seconds: /usr/bin/bwrap ")
    );

    // A plain program is copied when the caller says it is the application.
    let tool = home.path("home/Downloads/tool.bin");
    let outcome = home.run_named(&[
        "app",
        "install",
        &tool,
        "--app-id",
        APP,
        "--portable",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report = outcome.json();
    let inside = command_inside_sandbox(&report["plans"][1]["steps"][0]);
    let target = home
        .data("prefixes")
        .join(APP)
        .join("prefix/drive_c/Program Files/com-example-app/tool.bin.exe");
    assert_eq!(
        inside,
        [
            "/usr/bin/install",
            "-D",
            "-m",
            "0644",
            "--",
            tool.as_str(),
            target.to_str().unwrap()
        ]
    );

    // A dry run creates nothing.
    assert!(!home.root.join("data").exists());
    assert!(!home.root.join("state").exists());
}

#[test]
fn installations_are_refused_with_codes_before_anything_is_changed() {
    let home = Home::new("install-refused");
    let installer = home.path("home/Downloads/setup.bin");
    let tool = home.path("home/Downloads/tool.bin");
    let notes = home.path("home/Downloads/notes.txt");
    let key = home.path("home/.ssh/id_ed25519");

    // Not something that can be installed.
    assert_failed(
        &home.run_named(&["app", "install", &notes, "--app-id", APP]),
        3,
        "TUR-WINDOWS-APP-001",
    );
    // A program of no known installer kind is not run.
    let outcome = home.run_named(&["app", "install", &tool, "--app-id", APP]);
    assert_failed(&outcome, 6, "TUR-WINDOWS-APP-002");
    assert!(outcome.stderr.contains("--portable"));
    // An installer taken from a protected place.
    assert_failed(
        &home.run_named(&["app", "install", &key, "--app-id", APP]),
        6,
        "TUR-SECURITY-SANDBOX-006",
    );
    // A time limit of nothing.
    assert_failed(
        &home.run_named(&[
            "app",
            "install",
            &installer,
            "--app-id",
            APP,
            "--timeout",
            "0",
        ]),
        6,
        "TUR-WINDOWS-APP-010",
    );
    // An identifier that could leave the directory.
    assert_failed(
        &home.run_named(&["app", "install", &installer, "--app-id", "../escape"]),
        6,
        "TUR-RUNTIME-PROFILE-003",
    );
    // Without a sandbox nothing is installed.
    let outcome = home.run(&[
        "app",
        "install",
        &installer,
        "--app-id",
        APP,
        "--runtime-program",
        "/usr/bin/wine",
        "--runtime-version",
        "wine-9.0",
        "--sandbox-program",
        "/nonexistent/../bwrap",
    ]);
    assert_eq!(outcome.status, 6, "{}", outcome.stderr);
    assert!(!home.root.join("data").exists());

    // An application that already has a prefix is not installed over.
    home.write_application(APP);
    assert_failed(
        &home.run_named(&["app", "install", &installer, "--app-id", APP]),
        6,
        "TUR-WINDOWS-PREFIX-002",
    );
    let outcome = home.run_named(&["app", "install", &installer, "--app-id", APP, "--json"]);
    assert_eq!(outcome.status, 6);
    assert_eq!(outcome.json()["error"]["code"], "TUR-WINDOWS-PREFIX-002");
}

#[test]
fn installed_applications_are_listed_shown_and_launched_from_their_record() {
    let home = Home::new("list");
    assert_eq!(home.run(&["app", "list"]).stdout, "");
    assert_eq!(
        home.run(&["app", "list", "--json"]).json()["applications"],
        Value::Array(Vec::new())
    );
    for command in ["show", "launch", "uninstall"] {
        assert_failed(&home.run(&["app", command, APP]), 6, "TUR-WINDOWS-APP-007");
    }

    let record = home.write_application(APP);
    let other = home.write_application("org.example.other");
    let outcome = home.run(&["app", "list"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert_eq!(
        outcome.stdout,
        "com.example.app\tExample\t1.2\norg.example.other\tExample\t1.2\n"
    );
    let listed = home.run(&["app", "list", "--json"]).json();
    assert_eq!(listed["applications"].as_array().unwrap().len(), 2);
    assert_eq!(listed["applications"][1]["app_id"], "org.example.other");
    assert_eq!(listed["problems"], Value::Array(Vec::new()));

    let outcome = home.run(&["app", "show", APP, "--json"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert_eq!(parse_record(&outcome.stdout).unwrap(), record);
    let outcome = home.run(&["app", "show", "org.example.other"]);
    assert!(outcome.stdout.starts_with("Example (org.example.other)\n"));
    assert!(
        outcome
            .stdout
            .contains("  installer kind: NSIS installer\n")
    );
    // A record from before case folding was recorded is still read.
    assert!(
        outcome
            .stdout
            .contains("  drive C: not recorded (installed before case folding was recorded)\n")
    );
    assert!(outcome.stdout.contains(&format!(
        "  installed from: setup.exe (SHA-256 {})\n",
        other.install.source.sha256
    )));

    // `app launch` is `launch` with the application's own profile.
    let outcome = home.run_named(&["app", "launch", APP, "--dry-run", "--json", "--", "/safe"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let launch = outcome.json();
    assert_eq!(
        launch["launch"]["application"]["arguments"],
        serde_json::json!([record.main_executable, "/safe"])
    );
    assert_eq!(launch["launch"]["creates_prefix"], false);

    // A prefix that was not made by an installation is not an application.
    fs::remove_file(home.data("prefixes").join(APP).join("application.json")).unwrap();
    let outcome = home.run(&["app", "show", APP]);
    assert_failed(&outcome, 6, "TUR-WINDOWS-APP-007");
    assert!(outcome.stderr.contains("prefix remove"));
    // A record that is not valid is reported, not trusted.
    fs::write(
        home.data("prefixes")
            .join("org.example.other")
            .join("application.json"),
        other
            .to_json()
            .replace("org.example.other", "org.example.else"),
    )
    .unwrap();
    assert_failed(
        &home.run(&["app", "show", "org.example.other"]),
        6,
        "TUR-WINDOWS-APP-008",
    );
    let outcome = home.run(&["app", "list"]);
    assert_eq!(outcome.stdout, "");
    assert!(
        outcome
            .stderr
            .starts_with("warning[TUR-WINDOWS-APP-008]: org.example.other: ")
    );
}

#[test]
fn uninstall_runs_the_uninstaller_then_removes_everything_and_checks() {
    let home = Home::new("uninstall");
    let record = home.write_application(APP);
    let kept = home.write_application("org.example.other");
    let backups = home.data("backups").join(APP);
    fs::create_dir_all(&backups).unwrap();
    fs::write(backups.join("20261112T101500Z-manual.tar.gz"), b"archive").unwrap();

    // The plan: the application's own uninstaller inside the sandbox, with
    // a time limit; then the deletion; then the check.
    let outcome = home.run_named(&["app", "uninstall", APP, "--dry-run", "--json"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report = outcome.json();
    let plans = report["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 3);
    let uninstaller = &plans[0]["steps"][0];
    assert_eq!(uninstaller["step"], "run-bounded");
    let inside = command_inside_sandbox(uninstaller);
    assert_eq!(
        inside[inside.len() - 3..],
        ["/usr/bin/wine", "C:\\App\\uninstall.exe", "/S"]
    );
    let removed: Vec<&str> = plans[1]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| {
            assert_eq!(step["step"], "remove-tree");
            step["path"].as_str().unwrap()
        })
        .collect();
    let app_dir = home.data("prefixes").join(APP);
    let launchers = home.data("launchers").join(APP);
    let logs = home.root.join("state/tur/runtime/logs").join(APP);
    assert_eq!(
        removed,
        [
            app_dir.to_str().unwrap(),
            home.data("staging").join(APP).to_str().unwrap(),
            logs.to_str().unwrap(),
            launchers.to_str().unwrap(),
        ]
    );
    assert_eq!(plans[2]["steps"].as_array().unwrap().len(), 4);
    assert_eq!(plans[2]["steps"][0]["check"]["kind"], "absent");
    // The dry run removed nothing.
    assert!(app_dir.is_dir());

    // Carried out without the uninstaller, which would need the runtime.
    let outcome = home.run_named(&["app", "uninstall", APP, "--skip-uninstaller", "--json"]);
    assert_eq!(outcome.status, 0, "{}\n{}", outcome.stdout, outcome.stderr);
    let report = outcome.json();
    assert_eq!(report["uninstaller"]["ran"], false);
    assert_eq!(report["left_behind"], Value::Array(Vec::new()));
    assert_eq!(report["removed"].as_array().unwrap().len(), 4);
    assert_eq!(report["kept"][0], backups.to_str().unwrap());
    for gone in [&app_dir, &launchers, &logs] {
        assert!(!gone.exists(), "{}", gone.display());
    }
    assert!(!PathBuf::from(&record.main_executable).exists());
    // Backups are kept, and so is every other application.
    assert!(backups.join("20261112T101500Z-manual.tar.gz").is_file());
    assert!(PathBuf::from(&kept.main_executable).is_file());
    assert!(PathBuf::from(&kept.launcher.desktop_entry).is_file());
    let listed = home.run(&["app", "list"]);
    assert_eq!(listed.stdout, "org.example.other\tExample\t1.2\n");
    // Removed is removed.
    assert_failed(
        &home.run_named(&["app", "uninstall", APP]),
        6,
        "TUR-WINDOWS-APP-007",
    );

    // With the backups, in text form.
    let other_backups = home.data("backups").join("org.example.other");
    fs::create_dir_all(&other_backups).unwrap();
    let outcome = home.run_named(&[
        "app",
        "uninstall",
        "org.example.other",
        "--skip-uninstaller",
        "--remove-backups",
    ]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert_eq!(outcome.stdout, "removed: Example (org.example.other)\n");
    assert!(!other_backups.exists());
    assert_eq!(home.run(&["app", "list"]).stdout, "");
}
