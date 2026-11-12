// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! End-to-end tests of the `prefix`, `launch` and `desktop-entry` commands.
//!
//! The binary is given a scratch home and scratch XDG directories, and the
//! runtime and the sandbox are named on the command line, so these tests
//! need neither Wine nor bubblewrap. Launches are dry runs. The prefix
//! operations carried out for real are the ones that start no runtime:
//! verify, backup, restore, remove, grant, revoke and pin.
//!
//! Running a Windows program for real is the job of the `windows-runtime`
//! workflow, `tests/windows-runtime/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;
use uos_runtime::metadata::{
    PREFIX_SCHEMA_VERSION, PrefixArch, PrefixMetadata, RuntimeKind, SyncRecord, parse_metadata,
};
use uos_runtime::profile::GraphicsBackend;
use uos_runtime::timefmt::Timestamp;

const APP: &str = "com.example.app";
/// A value in the test's own environment that must never reach a plan.
const CANARY: &str = "hunter2-canary-value";
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
            std::env::temp_dir().join(format!("uos-runtime-launch-{process}-{unique}-{label}"));
        fs::create_dir_all(&created).unwrap();
        // The temporary directory may be reached through a link.
        let root = fs::canonicalize(&created).unwrap();
        let home = Home { root };
        for directory in ["home/Games", "home/Saves", "home/.ssh", "home/.config/tool"] {
            fs::create_dir_all(home.root.join(directory)).unwrap();
        }
        fs::write(home.root.join("home/Games/app.exe"), b"MZ").unwrap();
        fs::write(home.root.join("home/.ssh/id_ed25519"), b"not a key").unwrap();
        home
    }

    fn path(&self, relative: &str) -> String {
        self.root.join(relative).to_str().unwrap().to_owned()
    }

    fn app_dir(&self) -> PathBuf {
        self.root.join("data/uos/runtime/prefixes").join(APP)
    }

    fn run(&self, arguments: &[&str]) -> Outcome {
        let output = Command::new(env!("CARGO_BIN_EXE_uos-runtime"))
            .args(arguments)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("LANG", "en_GB.UTF-8")
            .env("UOS_CANARY", CANARY)
            .env("SSH_AUTH_SOCK", self.root.join("agent.socket"))
            .env("LD_PRELOAD", "")
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

    /// Writes a profile for the scratch application and returns its path.
    fn profile(&self, extra_members: &str) -> String {
        let text = format!(
            r#"{{
                "schema_version": 1,
                "app_id": "{APP}",
                "display_name": "Example Application",
                "binary": "{}",
                "format": "pe",
                "architecture": "x86_64",
                "runtime": "wine",
                {extra_members}
                "prefix_id": "example-app",
                "isolation_profile": "desktop-standard",
                "fallback": "none",
                "test_status": "untested"
            }}"#,
            self.path("home/Games/app.exe")
        );
        let path = self.root.join("profile.json");
        fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_owned()
    }

    /// Puts something shaped like a prefix on disk, with valid metadata.
    fn write_prefix(&self, runtime_version: &str) -> PrefixMetadata {
        let app = self.app_dir();
        fs::create_dir_all(app.join("prefix/drive_c")).unwrap();
        fs::create_dir_all(app.join("home")).unwrap();
        fs::write(
            app.join("prefix/system.reg"),
            b"WINE REGISTRY Version 2\n\n#arch=win64\n",
        )
        .unwrap();
        let metadata = PrefixMetadata {
            schema_version: PREFIX_SCHEMA_VERSION,
            app_id: APP.to_owned(),
            prefix_id: "example-app".to_owned(),
            runtime: RuntimeKind::Wine,
            runtime_version: runtime_version.to_owned(),
            runtime_build: format!("{runtime_version} (test)"),
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
        metadata
    }

    fn metadata(&self) -> PrefixMetadata {
        let text = fs::read_to_string(self.app_dir().join("metadata.json")).unwrap();
        parse_metadata(&text).unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_refused(outcome: &Outcome, code: &str) {
    assert_eq!(outcome.status, 6, "{}\n{}", outcome.stdout, outcome.stderr);
    assert!(
        outcome.stderr.starts_with(&format!("error[{code}]: ")),
        "{}",
        outcome.stderr
    );
    assert!(outcome.stdout.is_empty(), "{}", outcome.stdout);
}

fn step_kinds(plan: &Value) -> Vec<String> {
    plan["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["step"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn create_dry_run_prints_the_plan_and_changes_nothing() {
    let home = Home::new("create-dry");
    let saves = home.path("home/Saves");
    let outcome = home.run_named(&[
        "prefix",
        "create",
        APP,
        "--grant-rw",
        saves.as_str(),
        "--pin",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report = outcome.json();
    assert_eq!(report["dry_run"], true);
    assert!(report.get("report").is_none());
    let plan = &report["plan"];
    assert_eq!(plan["operation"], "create");
    assert_eq!(plan["app_id"], APP);
    assert_eq!(
        step_kinds(plan),
        [
            "create-dir",
            "create-dir",
            "create-dir",
            "run",
            "require",
            "write-file",
            "write-file"
        ]
    );
    let command = &plan["steps"][3]["command"];
    assert_eq!(command["program"], "/usr/bin/bwrap");
    let arguments: Vec<&str> = command["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|argument| argument.as_str().unwrap())
        .collect();
    assert_eq!(
        arguments[arguments.len() - 3..],
        ["/usr/bin/wine", "wineboot", "--init"]
    );
    // The metadata that would be written records the grant and the pin.
    let written = plan["steps"][6]["contents"].as_str().unwrap();
    let metadata = parse_metadata(written).unwrap();
    assert_eq!(metadata.prefix_id, "com-example-app");
    assert_eq!(metadata.runtime_version, "wine-9.0");
    assert!(metadata.pinned);
    assert_eq!(metadata.grants.len(), 1);
    assert_eq!(metadata.grants[0].path, saves);

    // Text form.
    let outcome = home.run_named(&["prefix", "create", APP, "--dry-run"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(outcome.stdout.starts_with("Plan: create com.example.app (7 steps)\n"));
    assert!(outcome.stdout.contains("  4. create the prefix: /usr/bin/bwrap "));

    // Nothing was created.
    assert!(!home.root.join("data").exists());
    assert!(!home.root.join("state").exists());
}

#[test]
fn refusals_have_codes_and_exit_status_six() {
    let home = Home::new("refusals");

    for operation in ["verify", "repair", "reset", "backup", "migrate", "remove", "pin"] {
        let outcome = home.run_named(&["prefix", operation, APP]);
        assert_refused(&outcome, "UOS-WINDOWS-PREFIX-001");
    }
    let outcome = home.run_named(&["prefix", "restore", APP, "20261112T101500Z-manual"]);
    assert_refused(&outcome, "UOS-WINDOWS-PREFIX-008");
    let outcome = home.run_named(&["prefix", "restore", APP, "../../../etc/passwd"]);
    assert_refused(&outcome, "UOS-WINDOWS-PREFIX-008");

    // Hostile grants, at creation.
    let home_directory = home.path("home");
    let keys = home.path("home/.ssh");
    let hidden = home.path("home/.config/tool");
    let dotted = home.path("home/Games/../.ssh");
    for directory in [
        "/",
        home_directory.as_str(),
        keys.as_str(),
        hidden.as_str(),
        dotted.as_str(),
        "/usr",
        "/proc/self/root",
        "relative",
    ] {
        let outcome = home.run_named(&["prefix", "create", APP, "--grant", directory, "--dry-run"]);
        assert_refused(&outcome, "UOS-SECURITY-SANDBOX-003");
        let outcome =
            home.run_named(&["prefix", "create", APP, "--grant-rw", directory, "--dry-run"]);
        assert_refused(&outcome, "UOS-SECURITY-SANDBOX-003");
    }

    // An identifier that is not one.
    for bad in ["../escape", "app", "Com.Example.App"] {
        let outcome = home.run_named(&["prefix", "create", bad, "--dry-run"]);
        assert_refused(&outcome, "UOS-DESKTOP-PROFILE-003");
    }

    // With --json a refusal is a JSON document on standard output.
    let outcome = home.run_named(&["prefix", "verify", APP, "--json"]);
    assert_eq!(outcome.status, 6);
    assert_eq!(outcome.json()["error"]["code"], "UOS-WINDOWS-PREFIX-001");
    assert!(outcome.stderr.is_empty());

    // A prefix that exists is not created again.
    home.write_prefix("wine-9.0");
    let outcome = home.run_named(&["prefix", "create", APP, "--dry-run"]);
    assert_refused(&outcome, "UOS-WINDOWS-PREFIX-002");

    assert!(!home.root.join("state").exists());
}

#[test]
fn launch_dry_run_shows_the_whole_command_and_runs_nothing() {
    let home = Home::new("launch-dry");
    let profile = home.profile("");
    let outcome = home.run_named(&[
        "launch",
        profile.as_str(),
        "--dry-run",
        "--json",
        "--env",
        "UOS_TEST_VALUE=from the launch",
        "--",
        "--first",
        "second argument",
    ]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report = outcome.json();
    let launch = &report["launch"];
    assert_eq!(launch["runtime"], "wine");
    assert_eq!(launch["runtime_version"], "wine-9.0");
    assert_eq!(launch["creates_prefix"], true);
    assert_eq!(launch["prefix"], home.app_dir().join("prefix").to_str().unwrap());

    let application = &launch["application"];
    assert_eq!(application["program"], "/usr/bin/wine");
    assert_eq!(
        application["arguments"],
        serde_json::json!([home.path("home/Games/app.exe"), "--first", "second argument"])
    );
    assert_eq!(application["working_directory"], home.path("home/Games"));

    // The environment is built from an allow-list: exactly these names.
    let environment = application["environment"].as_object().unwrap();
    let names: Vec<&str> = environment.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "HOME",
            "LANG",
            "LOGNAME",
            "PATH",
            "UOS_TEST_VALUE",
            "USER",
            "WINEDEBUG",
            "WINEDLLOVERRIDES",
            "WINEPREFIX"
        ]
    );
    assert_eq!(environment["UOS_TEST_VALUE"], "from the launch");
    assert_eq!(environment["LANG"], "en_GB.UTF-8");
    assert_eq!(environment["PATH"], "/usr/bin:/bin");
    assert_eq!(
        environment["HOME"],
        home.app_dir().join("home").to_str().unwrap()
    );
    // Nothing else from the caller's environment appears anywhere.
    assert!(!outcome.stdout.contains(CANARY));
    assert!(!outcome.stdout.contains("UOS_CANARY"));
    assert!(!outcome.stdout.contains("SSH_AUTH_SOCK"));
    assert!(!outcome.stdout.contains("agent.socket"));
    assert!(!outcome.stdout.contains("LD_PRELOAD"));

    // The synchronisation decision is recorded with what the probe found.
    assert_eq!(launch["sync"]["requested"], "auto");
    assert_ne!(launch["sync"]["ntsync_device"], "not-checked");
    // Wine 9.0 cannot use ntsync, whatever the device probe found.
    assert_eq!(launch["sync"]["selected"], "server");

    // The sandbox: no part of the real home directory, only the one program
    // file from it.
    let real_home = home.path("home");
    let from_home: Vec<&str> = launch["sandbox"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|mount| mount["source"].as_str())
        .filter(|source| Path::new(source).starts_with(&real_home))
        .collect();
    assert_eq!(from_home, [home.path("home/Games/app.exe")]);
    let plan = &launch["plan"];
    assert_eq!(step_kinds(plan).last().map(String::as_str), Some("launch"));

    // Text form.
    let outcome = home.run_named(&["launch", profile.as_str(), "--dry-run"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(
        outcome
            .stdout
            .starts_with("Launch: com.example.app (Example Application) with wine wine-9.0\n")
    );
    assert!(outcome.stdout.contains("  program: /usr/bin/wine\n"));
    assert!(outcome.stdout.contains("    WINEDLLOVERRIDES=winemenubuilder.exe=\n"));
    assert!(outcome.stdout.contains("    read-only /usr: system\n"));
    assert!(outcome.stdout.contains("Plan: launch com.example.app (10 steps)\n"));
    assert!(!outcome.stdout.contains(CANARY));

    // A dry run creates nothing.
    assert!(!home.root.join("data").exists());
    assert!(!home.root.join("state").exists());
}

#[test]
fn a_launch_never_changes_the_runtime_version_of_a_prefix() {
    let home = Home::new("launch-versions");

    // The profile pins a version that is not the installed one.
    let pinned = home.profile("\"runtime_version\": \"wine-8.0\",");
    let outcome = home.run_named(&["launch", pinned.as_str()]);
    assert_refused(&outcome, "UOS-WINDOWS-RUNTIME-005");
    assert!(outcome.stderr.contains("wine-8.0"));
    assert!(outcome.stderr.contains("wine-9.0"));
    // Not even with --migrate: the pin is about the runtime, not the prefix.
    let outcome = home.run_named(&["launch", pinned.as_str(), "--migrate"]);
    assert_refused(&outcome, "UOS-WINDOWS-RUNTIME-005");

    // The prefix is on Wine 8.0 and Wine 9.0 is installed.
    let profile = home.profile("");
    let before = home.write_prefix("wine-8.0");
    let outcome = home.run_named(&["launch", profile.as_str()]);
    assert_refused(&outcome, "UOS-WINDOWS-PREFIX-006");
    assert!(outcome.stderr.contains("migration"));
    let outcome = home.run_named(&["launch", profile.as_str(), "--json"]);
    assert_eq!(outcome.status, 6);
    assert_eq!(outcome.json()["error"]["code"], "UOS-WINDOWS-PREFIX-006");
    // The refusal changed nothing.
    assert_eq!(home.metadata(), before);

    // Asked to migrate, the plan backs the prefix up before updating it.
    let outcome = home.run_named(&["launch", profile.as_str(), "--migrate", "--dry-run", "--json"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report = outcome.json();
    let launch = &report["launch"];
    assert_eq!(launch["migrates_prefix"], true);
    let steps = launch["plan"]["steps"].as_array().unwrap();
    let labels: Vec<&str> = steps
        .iter()
        .filter_map(|step| step["label"].as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "archive the prefix",
            "update the prefix from wine-8.0 to wine-9.0"
        ]
    );
    assert_eq!(home.metadata(), before);

    // A pinned prefix is not migrated even when asked.
    let outcome = home.run(&["prefix", "pin", APP]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(home.metadata().pinned);
    let outcome = home.run_named(&["launch", profile.as_str(), "--migrate", "--dry-run"]);
    assert_refused(&outcome, "UOS-WINDOWS-PREFIX-007");
    let outcome = home.run_named(&["prefix", "migrate", APP, "--dry-run"]);
    assert_refused(&outcome, "UOS-WINDOWS-PREFIX-007");
    let outcome = home.run(&["prefix", "unpin", APP]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(!home.metadata().pinned);
    let outcome = home.run_named(&["prefix", "migrate", APP, "--dry-run"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(outcome.stdout.starts_with("Plan: migrate com.example.app (7 steps)\n"));

    // With the version the prefix is on, the launch is planned and the
    // prefix is reused.
    let outcome = home.run(&[
        "launch",
        profile.as_str(),
        "--dry-run",
        "--json",
        "--runtime-program",
        "/usr/bin/wine",
        "--runtime-version",
        "wine-8.0",
        "--sandbox-program",
        "/usr/bin/bwrap",
    ]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report = outcome.json();
    let launch = &report["launch"];
    assert_eq!(launch["creates_prefix"], false);
    assert_eq!(launch["migrates_prefix"], false);
    assert_eq!(
        step_kinds(&launch["plan"]),
        ["create-dir", "write-file", "launch"]
    );
}

#[test]
fn launch_refuses_bad_environment_profiles_and_programs() {
    let home = Home::new("launch-refusals");
    let profile = home.profile("");

    let outcome = home.run_named(&["launch", profile.as_str(), "--env", "NOEQUALS", "--dry-run"]);
    assert_eq!(outcome.status, 2);
    for variable in [
        "LD_PRELOAD=/tmp/evil.so",
        "WINEPREFIX=/home/ada/.wine",
        "HOME=/home/ada",
        "9BAD=x",
    ] {
        let outcome =
            home.run_named(&["launch", profile.as_str(), "--env", variable, "--dry-run"]);
        assert_refused(&outcome, "UOS-SECURITY-SANDBOX-005");
    }

    // A profile that is not valid is reported as the profile commands do.
    let invalid = home.root.join("invalid.json");
    fs::write(&invalid, b"{ \"schema_version\": 1 }").unwrap();
    let outcome = home.run_named(&["launch", invalid.to_str().unwrap(), "--dry-run"]);
    assert_eq!(outcome.status, 5);
    assert!(outcome.stderr.starts_with("error[UOS-DESKTOP-PROFILE-002]: "));
    let missing = home.root.join("missing.json");
    let outcome = home.run_named(&["launch", missing.to_str().unwrap(), "--dry-run"]);
    assert_eq!(outcome.status, 1);
    assert!(outcome.stderr.starts_with("error[UOS-DESKTOP-PROFILE-001]: "));

    // A program file in a protected directory.
    let text = fs::read_to_string(&profile).unwrap();
    let hostile = text.replace("home/Games/app.exe", "home/.ssh/id_ed25519");
    assert_ne!(hostile, text);
    fs::write(&profile, hostile).unwrap();
    let outcome = home.run_named(&["launch", profile.as_str(), "--dry-run"]);
    assert_refused(&outcome, "UOS-SECURITY-SANDBOX-006");

    // A profile for a runtime that is not launched here.
    let native = text
        .replace("\"runtime\": \"wine\"", "\"runtime\": \"windows-vm\"")
        .replace("\"prefix_id\": \"example-app\",", "");
    fs::write(&profile, native).unwrap();
    let outcome = home.run_named(&["launch", profile.as_str(), "--dry-run"]);
    assert_refused(&outcome, "UOS-WINDOWS-RUNTIME-009");

    assert!(!home.root.join("data").exists());
}

#[test]
fn prefix_operations_that_start_no_runtime_are_carried_out() {
    let home = Home::new("prefix-real");
    let before = home.write_prefix("wine-9.0");
    fs::write(home.app_dir().join("prefix/drive_c/save.dat"), b"progress").unwrap();
    fs::write(home.app_dir().join("home/note.txt"), b"private home").unwrap();

    // Grants: admitted, recorded resolved, replaced, revoked.
    let saves = home.path("home/Saves");
    let outcome = home.run(&["prefix", "grant", APP, saves.as_str()]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(outcome.stdout.contains("done: configure com.example.app"));
    assert!(outcome.stdout.contains(&format!("granted: {saves}")));
    let metadata = home.metadata();
    assert_eq!(metadata.grants.len(), 1);
    assert_eq!(metadata.grants[0].path, saves);
    let access = serde_json::to_value(metadata.grants[0].access).unwrap();
    assert_eq!(access, "read-only");

    let outcome = home.run(&["prefix", "grant", APP, saves.as_str(), "--read-write"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let metadata = home.metadata();
    assert_eq!(metadata.grants.len(), 1);
    let access = serde_json::to_value(metadata.grants[0].access).unwrap();
    assert_eq!(access, "read-write");

    let keys = home.path("home/.ssh");
    let outcome = home.run(&["prefix", "grant", APP, keys.as_str()]);
    assert_refused(&outcome, "UOS-SECURITY-SANDBOX-003");
    assert_eq!(home.metadata().grants.len(), 1);

    // The grant shows up in the sandbox of the next launch plan.
    let profile = home.profile("");
    let outcome = home.run_named(&["launch", profile.as_str(), "--dry-run", "--json"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let report = outcome.json();
    let launch = &report["launch"];
    let granted: Vec<&Value> = launch["sandbox"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|mount| mount["purpose"] == "granted by the user")
        .collect();
    assert_eq!(granted.len(), 1);
    assert_eq!(granted[0]["kind"], "read-write");
    assert_eq!(granted[0]["source"], saves.as_str());

    // Metadata edited to name a protected directory refuses the launch.
    let edited = fs::read_to_string(home.app_dir().join("metadata.json"))
        .unwrap()
        .replace(&saves, &keys);
    let good = fs::read_to_string(home.app_dir().join("metadata.json")).unwrap();
    fs::write(home.app_dir().join("metadata.json"), edited).unwrap();
    let outcome = home.run_named(&["launch", profile.as_str(), "--dry-run"]);
    assert_refused(&outcome, "UOS-SECURITY-SANDBOX-004");
    fs::write(home.app_dir().join("metadata.json"), good).unwrap();

    // Verification: this hand-made prefix is incomplete, and is reported so.
    let outcome = home.run(&["prefix", "verify", APP]);
    assert_eq!(outcome.status, 8, "{}\n{}", outcome.stdout, outcome.stderr);
    assert!(outcome.stdout.contains("finding: expected that "));
    assert!(outcome.stdout.contains("user.reg is a file"));
    assert!(outcome.stderr.starts_with("error[UOS-WINDOWS-PREFIX-004]: "));
    let outcome = home.run(&["prefix", "verify", APP, "--json"]);
    assert_eq!(outcome.status, 8);
    let report = outcome.json();
    assert_eq!(report["report"]["findings"].as_array().unwrap().len(), 3);
    assert_eq!(report["report"]["completed"], 8);

    // Backup.
    let outcome = home.run(&["prefix", "backup", APP, "--json"]);
    assert_eq!(outcome.status, 0, "{}\n{}", outcome.stdout, outcome.stderr);
    let report = outcome.json();
    let name = report["backup"].as_str().unwrap().to_owned();
    assert!(name.ends_with("-manual"), "{name}");
    let archive = home
        .root
        .join("data/uos/runtime/backups")
        .join(APP)
        .join(format!("{name}.tar.gz"));
    assert!(archive.is_file());
    let recorded = home.metadata();
    // What the operation ran is in the operations log.
    let log = home
        .root
        .join("state/uos/runtime/logs")
        .join(APP)
        .join("operations.log");
    let logged = fs::read_to_string(&log).unwrap();
    assert!(logged.contains("archive the prefix: /usr/bin/tar --create"));
    assert!(logged.contains("(exit status 0)"));

    // Remove: the application's directory goes, the backup stays.
    let outcome = home.run(&["prefix", "remove", APP]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(!home.app_dir().exists());
    assert!(!log.exists());
    assert!(archive.is_file());
    assert!(home.root.join("home/Saves").is_dir());
    assert!(home.root.join("home/.ssh/id_ed25519").is_file());

    // Restore: everything is back as it was archived.
    let outcome = home.run(&["prefix", "restore", APP, name.as_str()]);
    assert_eq!(outcome.status, 0, "{}\n{}", outcome.stdout, outcome.stderr);
    assert_eq!(home.metadata(), recorded);
    assert_ne!(recorded, before);
    assert_eq!(
        fs::read(home.app_dir().join("prefix/drive_c/save.dat")).unwrap(),
        b"progress"
    );
    assert_eq!(
        fs::read(home.app_dir().join("home/note.txt")).unwrap(),
        b"private home"
    );
    assert!(!home.root.join("data/uos/runtime/staging").join(APP).exists());

    // Restore over an existing prefix replaces it.
    fs::write(home.app_dir().join("prefix/drive_c/save.dat"), b"corrupted").unwrap();
    let outcome = home.run(&["prefix", "restore", APP, name.as_str()]);
    assert_eq!(outcome.status, 0, "{}\n{}", outcome.stdout, outcome.stderr);
    assert_eq!(
        fs::read(home.app_dir().join("prefix/drive_c/save.dat")).unwrap(),
        b"progress"
    );

    // Revoke.
    let outcome = home.run(&["prefix", "revoke", APP, saves.as_str()]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(home.metadata().grants.is_empty());
    let outcome = home.run(&["prefix", "revoke", APP, saves.as_str()]);
    assert_eq!(outcome.status, 6);

    let logged = fs::read_to_string(&log).unwrap();
    assert!(logged.contains("unpack the backup: /usr/bin/tar --extract"));
}

#[test]
fn desktop_entry_is_printed_and_nothing_is_installed() {
    let home = Home::new("desktop-entry");
    let profile = home.profile("");
    let outcome = home.run(&["desktop-entry", profile.as_str()]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines[0], "[Desktop Entry]");
    assert!(lines.contains(&"Type=Application"));
    assert!(lines.contains(&"Name=Example Application"));
    assert!(lines.contains(&format!("Exec=uos-runtime launch \"{profile}\"").as_str()));
    assert!(lines.contains(&"X-UOS-Runtime=wine"));
    assert!(lines.contains(&"X-UOS-App-Id=com.example.app"));

    let outcome = home.run(&[
        "desktop-entry",
        profile.as_str(),
        "--launcher",
        "sh -c evil;",
    ]);
    assert_eq!(outcome.status, 6);
    assert!(outcome.stdout.is_empty());

    assert!(!home.root.join("data").exists());
    assert!(!home.root.join("home/.local").exists());
}

#[test]
fn the_supervisor_passes_the_exit_status_on() {
    let home = Home::new("supervise");
    let outcome = home.run(&["supervise", "--", "/bin/sh", "-c", "echo supervised; exit 9"]);
    assert_eq!(outcome.status, 9);
    assert_eq!(outcome.stdout, "supervised\n");
    let outcome = home.run(&["supervise", "--", "/nonexistent/program"]);
    assert_eq!(outcome.status, 7);
    assert!(outcome.stderr.starts_with("error[UOS-WINDOWS-RUNTIME-010]: "));
    assert_eq!(home.run(&["supervise"]).status, 2);
}
