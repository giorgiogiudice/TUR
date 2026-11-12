// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The program, run for real, against stand-ins for `flatpak`, `curl` and
//! `tur-runtime` (tests/fakes): every flow from the plan to the removal,
//! each refusal, and what is left on disk afterwards.
//!
//! The stand-ins install nothing real; the apps workflow does that with the
//! real programs. What these tests prove is this program's own behaviour:
//! what it runs, in which order, what it refuses before running anything,
//! what it records and what it leaves.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;
use tur_apps::digest::hash_file;
use tur_catalogue::{
    Catalogue, Date, HostFacts, Preferences, RouteRequest, parse_host_facts, recommended, route,
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_tur-apps");
const REPOSITORY_CATALOGUE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalogue");
const HOST: &str = r#"{ "arch": "x86_64", "flatpak": true, "wine": true }"#;
const BODY: &[u8] = b"MZ this stands in for a Windows installer";

/// A scratch world: a home, the stand-ins, a catalogue.
struct World {
    root: PathBuf,
    catalogue: PathBuf,
}

struct Run {
    status: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_str(self.stdout.trim())
            .unwrap_or_else(|error| panic!("not JSON ({error}): {}\n{}", self.stdout, self.stderr))
    }

    /// The JSON lines of an event stream.
    fn events(&self) -> Vec<Value> {
        self.stdout
            .lines()
            .map(|line| {
                serde_json::from_str(line)
                    .unwrap_or_else(|error| panic!("not a JSON line ({error}): {line}"))
            })
            .collect()
    }
}

impl World {
    fn new(label: &str, catalogue: Option<&str>) -> World {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "tur-apps-cli-{}-{unique}-{label}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        for directory in ["home", "state", "bin", "catalogue"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        for (name, text) in [
            ("flatpak", include_str!("fakes/flatpak")),
            ("curl", include_str!("fakes/curl")),
            ("tur-runtime", include_str!("fakes/tur-runtime")),
        ] {
            let path = root.join("bin").join(name);
            fs::write(&path, text).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(root.join("host.json"), HOST).unwrap();
        let catalogue = match catalogue {
            Some(_) => root.join("catalogue"),
            None => PathBuf::from(REPOSITORY_CATALOGUE),
        };
        World { root, catalogue }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn data(&self) -> PathBuf {
        self.home().join(".local/share")
    }

    fn state(&self, name: &str) -> PathBuf {
        self.root.join("state").join(name)
    }

    fn write_entry(&self, id: &str, text: &str) {
        let header = "# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\nschema_version = 1\n";
        fs::write(
            self.catalogue.join(format!("{id}.toml")),
            format!("{header}id = \"{id}\"\n{text}"),
        )
        .unwrap();
    }

    fn run(&self, arguments: &[&str]) -> Run {
        let bin = self.root.join("bin");
        let output = Command::new(PROGRAM)
            .arg("--catalogue")
            .arg(&self.catalogue)
            .arg("--host")
            .arg(self.root.join("host.json"))
            .arg("--flatpak-program")
            .arg(bin.join("flatpak"))
            .arg("--curl-program")
            .arg(bin.join("curl"))
            .arg("--tur-runtime-program")
            .arg(bin.join("tur-runtime"))
            .args(arguments)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .env("FAKE_STATE", self.root.join("state"))
            .output()
            .expect("the program runs");
        Run {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn log(&self, name: &str) -> String {
        fs::read_to_string(self.state(name)).unwrap_or_default()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A catalogue of four invented applications: a Flatpak; a free Windows
/// application whose installer is pinned to `BODY`; a proprietary Windows
/// application with no pin; and a game with kernel anti-cheat.
fn test_world(label: &str) -> World {
    let world = World::new(label, Some("own"));
    let body = world.root.join("body");
    fs::write(&body, BODY).unwrap();
    let hashed = hash_file(&body).unwrap();
    world.write_entry(
        "org.example.viewer",
        "name = \"Example Viewer\"\npublisher = \"Example\"\ncategory = \"utility\"\n\n[[editions]]\nkind = \"flatpak\"\nflatpak_id = \"org.example.Viewer\"\nlicence = \"open-source\"\n",
    );
    world.write_entry(
        "org.example.tool",
        &format!(
            "name = \"Example Tool\"\npublisher = \"Example\"\ncategory = \"utility\"\n\n[[editions]]\nkind = \"windows\"\nobtain = \"publisher-page\"\ndownload_page = \"https://example.org/download\"\nlicence = \"open-source\"\n\n[editions.installer]\nurl = \"https://downloads.example.org/1.0/tool-1.0-setup.exe\"\nsha256 = \"{}\"\nsize = {}\nversion = \"1.0\"\nlicence = \"MIT\"\nlicence_url = \"https://example.org/licence\"\nhash_source = \"made by the test\"\nchecked = \"2026-11-12\"\n",
            hashed.sha256, hashed.size
        ),
    );
    world.write_entry(
        "org.example.closed",
        "name = \"Closed Suite\"\npublisher = \"Example Corp\"\ncategory = \"productivity\"\n\n[[editions]]\nkind = \"windows\"\nobtain = \"publisher-page\"\ndownload_page = \"https://example.com/buy\"\nlicence = \"proprietary\"\n",
    );
    world.write_entry(
        "org.example.shooter",
        "name = \"Example Shooter\"\npublisher = \"Example Games\"\ncategory = \"game\"\n\n[[editions]]\nkind = \"windows\"\nobtain = \"steam\"\nstore_id = \"12345\"\nrequirements = [\"kernel-anti-cheat\"]\n",
    );
    world
}

fn switch_flathub_on(world: &World) {
    let run = world.run(&["sources", "enable", "flathub", "--json"]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    assert_eq!(run.json()["state"], "on");
}

#[test]
fn no_route_is_refused_with_the_router_s_own_reason() {
    let world = World::new("no-route", None);
    let run = world.run(&["plan", "com.riotgames.valorant", "--json"]);
    assert_eq!(run.status, 3, "{}", run.stderr);
    let error = &run.json()["error"];
    assert_eq!(error["code"], "TUR-DESKTOP-APPS-003");

    // The router's answer for the same machine, through the library.
    let catalogue = Catalogue::load(Path::new(REPOSITORY_CATALOGUE), Date::today_utc()).unwrap();
    let entry = catalogue.get("com.riotgames.valorant").unwrap();
    let host: HostFacts = parse_host_facts(HOST).unwrap();
    let options = route(&RouteRequest::new(entry), &host, &Preferences::default());
    let router = recommended(&options);
    let message = error["message"].as_str().unwrap();
    assert!(message.starts_with(&router.reason), "{message}");
    assert!(message.ends_with("Nothing was installed."));
    assert_eq!(error["detail"]["reason_code"], "no-route");
    assert_eq!(error["detail"]["route"], "no-route");
    assert_eq!(
        error["detail"]["alternative"]["text_id"],
        "dual-boot.only-way"
    );
    // Nothing ran.
    assert_eq!(world.log("flatpak.log"), "");
    assert_eq!(world.log("runtime.log"), "");
    assert_eq!(world.log("curl.log"), "");

    // The same through install.
    let run = world.run(&["install", "com.riotgames.valorant", "--yes", "--events"]);
    assert_eq!(run.status, 3);
    let events = run.events();
    assert_eq!(events.last().unwrap()["event"], "error");
    assert_eq!(
        events.last().unwrap()["error"]["code"],
        "TUR-DESKTOP-APPS-003"
    );
}

#[test]
fn routes_that_are_not_built_are_refused_and_built_ones_are_named() {
    let world = World::new("not-built", None);
    // PuTTY's recommended route is Fedora's package; the Windows edition
    // is offered too and can be installed with --route wine.
    let run = world.run(&["plan", "uk.org.greenend.chiark.putty", "--json"]);
    assert_eq!(run.status, 3);
    let error = &run.json()["error"];
    assert_eq!(error["code"], "TUR-DESKTOP-APPS-004");
    assert!(error["message"].as_str().unwrap().contains("--route wine"));

    // A route the router does not offer.
    let run = world.run(&[
        "plan",
        "uk.org.greenend.chiark.putty",
        "--route",
        "android-runtime",
        "--json",
    ]);
    assert_eq!(run.status, 3);
    assert_eq!(run.json()["error"]["code"], "TUR-DESKTOP-APPS-014");

    let run = world.run(&["plan", "org.example.missing", "--json"]);
    assert_eq!(run.status, 6);
    assert_eq!(run.json()["error"]["code"], "TUR-DESKTOP-APPS-002");

    let run = world.run(&["plan", "uk.org.greenend.chiark.putty", "--route", "warp"]);
    assert_eq!(run.status, 1);
    assert!(run.stderr.contains("TUR-DESKTOP-APPS-017"));
}

#[test]
fn a_pinned_windows_installer_is_planned_with_its_size_digest_and_source() {
    let world = World::new("windows-plan", None);
    let run = world.run(&[
        "plan",
        "org.notepad-plus-plus.notepad-plus-plus",
        "--json",
    ]);
    assert_eq!(run.status, 0, "{}", run.stdout);
    let plan = run.json();
    assert_eq!(plan["kind"], "plan");
    assert_eq!(plan["route"], "wine");
    assert_eq!(plan["chosen_by"], "router");
    assert_eq!(plan["router"]["reason_code"], "windows-wine");
    assert_eq!(plan["source"]["kind"], "publisher-download");
    let download = &plan["downloads"][0];
    assert_eq!(
        download["from"],
        "https://github.com/notepad-plus-plus/notepad-plus-plus/releases/download/v8.9.9.1/npp.8.9.9.1.Installer.x64.exe"
    );
    assert_eq!(download["size"], 7_048_536);
    assert_eq!(
        download["sha256"],
        "ce3267f5c4e7e42f847e00f4b20234da6150647203033cdde34a69593efc3152"
    );
    let steps: Vec<&str> = plan["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["id"].as_str().unwrap())
        .collect();
    assert_eq!(steps, ["download", "verify", "install", "record"]);
    assert_eq!(plan["removal"]["data_kept_by_default"], true);
    assert!(
        plan["access"]
            .as_array()
            .unwrap()
            .iter()
            .any(|access| access["kind"] == "network")
    );
    // A plan downloads nothing and runs nothing.
    assert_eq!(world.log("curl.log"), "");
    assert_eq!(world.log("runtime.log"), "");

    // The text form says the same things.
    let run = world.run(&["plan", "org.notepad-plus-plus.notepad-plus-plus"]);
    assert_eq!(run.status, 0);
    assert!(run.stdout.contains("SHA-256 ce3267f5"));
    assert!(run.stdout.contains("Removing it:"));
}

#[test]
fn a_flatpak_is_planned_installed_listed_and_removed_keeping_its_data() {
    let world = test_world("flatpak");
    // Flathub off: refused, nothing changed.
    let run = world.run(&["plan", "org.example.viewer", "--json"]);
    assert_eq!(run.status, 3);
    assert_eq!(run.json()["error"]["code"], "TUR-DESKTOP-APPS-008");
    assert!(run.json()["error"]["message"]
        .as_str()
        .unwrap()
        .contains("tur-apps sources enable flathub"));

    switch_flathub_on(&world);
    fs::write(
        world.state("api.json"),
        r#"{"download_size":1091006,"installed_size":2502656,"branches":{}}"#,
    )
    .unwrap();
    let run = world.run(&["plan", "org.example.viewer", "--json"]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    let plan = run.json();
    assert_eq!(plan["route"], "flatpak");
    assert_eq!(plan["source"]["remote"], "flathub");
    assert_eq!(plan["downloads"][0]["size"], 1_091_006);
    assert_eq!(plan["downloads"][0]["commit"], "a".repeat(64));
    assert!(
        plan["downloads"][1]["what"]
            .as_str()
            .unwrap()
            .contains("org.example.Platform")
    );
    let permissions: Vec<&str> = plan["access"]
        .as_array()
        .unwrap()
        .iter()
        .map(|access| access["permission"].as_str().unwrap())
        .collect();
    assert_eq!(permissions, ["xdg-pictures:ro", "network", "ipc", "x11", "wayland"]);

    // Without confirmation, nothing is installed.
    let run = world.run(&["install", "org.example.viewer", "--events"]);
    assert_eq!(run.status, 3);
    assert_eq!(run.events().last().unwrap()["error"]["code"], "TUR-DESKTOP-APPS-013");
    assert!(!world.log("flatpak.log").contains("install --user"));

    let run = world.run(&["install", "org.example.viewer", "--yes", "--events"]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    let events = run.events();
    assert_eq!(events[0]["event"], "plan");
    assert_eq!(events[0]["schema_version"], 1);
    assert!(events.iter().any(|event| event["event"] == "activity"
        && event["text"] == "installing app/org.example.Viewer/x86_64/stable"));
    let last = events.last().unwrap();
    assert_eq!(last["event"], "result");
    assert_eq!(last["result"]["version"], "1.0");
    let added = last["result"]["flatpak"]["added_refs"].as_array().unwrap();
    assert_eq!(added.len(), 2, "{added:?}");
    assert!(world.data().join("tur/apps/installed/org.example.viewer.json").is_file());

    // Installing again is refused.
    let run = world.run(&["install", "org.example.viewer", "--yes", "--events"]);
    assert_eq!(run.status, 3);

    // The list.
    let run = world.run(&["list", "--json", "--check-updates"]);
    assert_eq!(run.status, 0);
    let listed = &run.json()["applications"][0];
    assert_eq!(listed["id"], "org.example.viewer");
    assert_eq!(listed["installed_by"], "tur-apps");
    assert_eq!(listed["size"], 12345);
    assert_eq!(listed["state"], "installed");
    assert_eq!(listed["update"]["checked"], true);
    assert_eq!(listed["update"]["available"], false);

    // The application saved something; removal keeps it by default.
    let data = world.home().join(".var/app/org.example.Viewer");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("notes.txt"), b"mine").unwrap();
    let run = world.run(&["remove", "org.example.viewer", "--events"]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    let result = &run.events().last().unwrap()["result"];
    assert_eq!(result["data"], "keep");
    assert_eq!(result["left_behind"].as_array().unwrap().len(), 0);
    assert_eq!(fs::read(data.join("notes.txt")).unwrap(), b"mine");
    // The runtime it brought in went with it; no application is left.
    assert_eq!(fs::read_to_string(world.state("refs")).unwrap().trim(), "");
    assert!(!world.data().join("flatpak/app/org.example.Viewer").exists());
    assert!(!world.data().join("tur/apps/installed/org.example.viewer.json").exists());
    let run = world.run(&["remove", "org.example.viewer", "--events"]);
    assert_eq!(run.status, 6);

    // Installed again and removed with its data: nothing is left at all.
    let run = world.run(&["install", "org.example.viewer", "--yes", "--events"]);
    assert_eq!(run.status, 0);
    let run = world.run(&["remove", "org.example.viewer", "--delete-data", "--events"]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    assert!(!data.exists());
    assert!(world.log("flatpak.log").contains("--delete-data app/org.example.Viewer/x86_64/stable"));
}

#[test]
fn a_failed_flatpak_installation_takes_back_what_it_added() {
    let world = test_world("flatpak-fails");
    switch_flathub_on(&world);
    fs::write(world.state("fail-install"), b"").unwrap();
    let run = world.run(&["install", "org.example.viewer", "--yes", "--events"]);
    assert_eq!(run.status, 7, "{}", run.stdout);
    assert_eq!(run.events().last().unwrap()["error"]["code"], "TUR-DESKTOP-APPS-010");
    // The runtime the attempt installed was removed again; no record.
    assert_eq!(fs::read_to_string(world.state("refs")).unwrap().trim(), "");
    assert!(!world.data().join("tur/apps/installed/org.example.viewer.json").exists());
}

#[test]
fn a_windows_application_is_downloaded_checked_installed_and_removed_keeping_documents() {
    let world = test_world("windows");
    fs::write(world.state("download"), BODY).unwrap();
    let run = world.run(&["install", "org.example.tool", "--yes", "--events"]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    let events = run.events();
    let kinds: Vec<String> = events
        .iter()
        .map(|event| {
            format!(
                "{}:{}",
                event["event"].as_str().unwrap(),
                event["step"].as_str().unwrap_or("")
            )
        })
        .collect();
    let position = |wanted: &str| kinds.iter().position(|kind| kind == wanted).unwrap();
    // Downloaded, then checked, then installed, then recorded.
    assert!(position("step-finished:download") < position("step-finished:verify"));
    assert!(position("step-finished:verify") < position("step-started:install"));
    assert!(position("step-finished:install") < position("step-finished:record"));
    let progress = events
        .iter()
        .find(|event| event["event"] == "progress")
        .expect("a progress event");
    assert_eq!(progress["total"], BODY.len());
    // tur-runtime was given the checked file, under the catalogue's id.
    let log = world.log("runtime.log");
    assert!(log.contains("--app-id org.example.tool --name Example Tool --json"), "{log}");
    assert_eq!(fs::read(world.state("installed-file")).unwrap(), BODY);
    // The download is gone; the record keeps its digest.
    assert!(!world.home().join(".cache/tur/apps/downloads/org.example.tool").exists());
    let result = &events.last().unwrap()["result"];
    assert_eq!(result["source"]["pinned"], true);
    assert_eq!(result["source"]["size"], BODY.len());

    // The application keeps a document in its private home.
    let home = world.data().join("tur/runtime/prefixes/org.example.tool/home");
    fs::create_dir_all(home.join("Documents")).unwrap();
    fs::write(home.join("Documents/letter.txt"), b"Dear reader").unwrap();

    let run = world.run(&["list", "--json"]);
    let listed = &run.json()["applications"][0];
    assert_eq!(listed["route"], "wine");
    assert_eq!(listed["version"], "1.0");
    assert!(listed["size"].as_u64().unwrap() > 0);

    let run = world.run(&["remove", "org.example.tool", "--events"]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    let result = &run.events().last().unwrap()["result"];
    assert_eq!(result["data"], "keep");
    let kept_root = world.data().join("tur/apps/kept/org.example.tool");
    let kept: Vec<PathBuf> = fs::read_dir(&kept_root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(
        fs::read(kept[0].join("home/Documents/letter.txt")).unwrap(),
        b"Dear reader"
    );
    assert_eq!(
        fs::read_to_string(kept[0].join("windows-profiles/user/AppData/settings.ini")).unwrap(),
        "settings\n"
    );
    // Removed by tur-runtime, with the backups kept; nothing else left.
    assert!(world.log("runtime.log").contains("app uninstall org.example.tool --json\n"));
    assert!(!world.data().join("tur/runtime/prefixes/org.example.tool").exists());
    assert!(!world.data().join("tur/runtime/launchers/org.example.tool").exists());
    assert!(!world.data().join("tur/apps/installed/org.example.tool.json").exists());
}

#[test]
fn a_windows_application_removed_with_its_data_leaves_nothing() {
    let world = test_world("windows-delete");
    fs::write(world.state("download"), BODY).unwrap();
    assert_eq!(world.run(&["install", "org.example.tool", "--yes", "--events"]).status, 0);
    let run = world.run(&["remove", "org.example.tool", "--delete-data", "--events"]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    assert!(world.log("runtime.log").contains("app uninstall org.example.tool --json --remove-backups"));
    assert!(!world.data().join("tur/apps/kept").exists());
    assert!(!world.data().join("tur/runtime/prefixes/org.example.tool").exists());
}

#[test]
fn a_corrupted_download_is_refused_before_anything_runs() {
    let world = test_world("corrupted");
    let mut wrong = BODY.to_vec();
    wrong[0] ^= 0xff;
    fs::write(world.state("download"), &wrong).unwrap();
    let run = world.run(&["install", "org.example.tool", "--yes", "--events"]);
    assert_eq!(run.status, 5, "{}", run.stdout);
    let error = &run.events().last().unwrap()["error"];
    assert_eq!(error["code"], "TUR-DESKTOP-APPS-006");
    assert!(error["message"].as_str().unwrap().contains("Nothing in it was run"));
    // tur-runtime was never called; nothing is kept, nothing recorded.
    assert_eq!(world.log("runtime.log"), "");
    assert!(world.log("curl.log").contains("https://downloads.example.org/1.0/tool-1.0-setup.exe"));
    let downloads = world.home().join(".cache/tur/apps/downloads/org.example.tool");
    assert!(!downloads.exists() || fs::read_dir(&downloads).unwrap().count() == 0);
    assert!(!world.data().join("tur/apps/installed/org.example.tool.json").exists());

    // A shorter file is refused the same way.
    fs::write(world.state("download"), &BODY[..10]).unwrap();
    let run = world.run(&["install", "org.example.tool", "--yes", "--events"]);
    assert_eq!(run.status, 5);
    assert_eq!(world.log("runtime.log"), "");

    // A failed download is a failed step, and nothing ran either.
    fs::remove_file(world.state("download")).unwrap();
    let run = world.run(&["install", "org.example.tool", "--yes", "--events"]);
    assert_eq!(run.status, 7);
    assert_eq!(run.events().last().unwrap()["error"]["code"], "TUR-DESKTOP-APPS-007");
    assert_eq!(world.log("runtime.log"), "");
}

#[test]
fn a_file_you_give_is_checked_from_the_hash_on() {
    let world = test_world("supplied");
    // For an application with a pin, a file that is not the pinned one is
    // refused before anything runs.
    let wrong = world.root.join("tool-setup.exe");
    fs::write(&wrong, b"not it").unwrap();
    let run = world.run(&[
        "install",
        "org.example.tool",
        "--installer",
        wrong.to_str().unwrap(),
        "--yes",
        "--events",
    ]);
    assert_eq!(run.status, 5);
    assert_eq!(world.log("runtime.log"), "");
    assert_eq!(world.log("curl.log"), "");

    // The pinned file, given by hand, is installed without a download.
    let right = world.root.join("tool-1.0-setup.exe");
    fs::write(&right, BODY).unwrap();
    let run = world.run(&[
        "install",
        "org.example.tool",
        "--installer",
        right.to_str().unwrap(),
        "--yes",
        "--events",
    ]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    let result = &run.events().last().unwrap()["result"];
    assert_eq!(result["source"]["kind"], "user-file");
    assert_eq!(result["source"]["pinned"], true);
    assert_eq!(world.log("curl.log"), "");
    // The user's own file is left where it was.
    assert!(right.is_file());
}

#[test]
fn proprietary_software_is_never_downloaded_and_takes_your_file() {
    let world = test_world("proprietary");
    let run = world.run(&["plan", "org.example.closed", "--json"]);
    assert_eq!(run.status, 3);
    let error = &run.json()["error"];
    assert_eq!(error["code"], "TUR-DESKTOP-APPS-005");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("never downloads"), "{message}");
    assert!(message.contains("https://example.com/buy"));
    assert!(message.contains("--installer FILE"));

    let file = world.root.join("ClosedSuiteSetup.exe");
    fs::write(&file, b"the user's own installer").unwrap();
    let run = world.run(&[
        "plan",
        "org.example.closed",
        "--installer",
        file.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(run.status, 0, "{}", run.stdout);
    let plan = run.json();
    assert_eq!(plan["source"]["kind"], "user-file");
    assert!(plan["downloads"].as_array().unwrap().is_empty());
    assert!(plan["source"]["verification"]
        .as_str()
        .unwrap()
        .contains("cannot check this file against its publisher"));
    assert_eq!(world.log("curl.log"), "");

    // The game with kernel anti-cheat has no route at all.
    let run = world.run(&["plan", "org.example.shooter", "--json"]);
    assert_eq!(run.status, 3);
    assert_eq!(run.json()["error"]["code"], "TUR-DESKTOP-APPS-003");
}

#[test]
fn updates_are_found_and_applied_for_a_flatpak_and_refused_for_windows() {
    let world = test_world("updates");
    switch_flathub_on(&world);
    assert_eq!(world.run(&["install", "org.example.viewer", "--yes", "--events"]).status, 0);
    fs::write(world.state("updates"), "org.example.Viewer\t2.0\tflathub\n").unwrap();
    let run = world.run(&["list", "--check-updates", "--json"]);
    let update = &run.json()["applications"][0]["update"];
    assert_eq!(update["available"], true);
    assert_eq!(update["version"], "2.0");
    let run = world.run(&["update", "org.example.viewer", "--events"]);
    assert_eq!(run.status, 0, "{}{}", run.stdout, run.stderr);
    assert!(world.log("flatpak.log").contains("update --user --noninteractive app/org.example.Viewer/x86_64/stable"));

    fs::write(world.state("download"), BODY).unwrap();
    assert_eq!(world.run(&["install", "org.example.tool", "--yes", "--events"]).status, 0);
    let run = world.run(&["update", "org.example.tool", "--events"]);
    assert_eq!(run.status, 3);
    assert_eq!(run.events().last().unwrap()["error"]["code"], "TUR-DESKTOP-APPS-004");
}

#[test]
fn sources_are_listed_switched_on_and_off() {
    let world = test_world("sources");
    let run = world.run(&["sources", "--json"]);
    assert_eq!(run.status, 0);
    let sources = run.json()["sources"].as_array().unwrap().clone();
    assert_eq!(sources[0]["name"], "flathub");
    assert_eq!(sources[0]["state"], "not-added");
    assert_eq!(sources[1]["name"], "fedora");
    switch_flathub_on(&world);
    assert!(world.log("flatpak.log").contains(
        "remote-add --user --if-not-exists --title=Flathub flathub https://dl.flathub.org/repo/flathub.flatpakrepo"
    ));
    let run = world.run(&["sources", "disable", "flathub", "--json"]);
    assert_eq!(run.status, 0);
    let run = world.run(&["sources", "--json"]);
    assert_eq!(run.json()["sources"][0]["state"], "off");
    let run = world.run(&["sources", "enable", "nowhere", "--json"]);
    assert_eq!(run.status, 1);
}
