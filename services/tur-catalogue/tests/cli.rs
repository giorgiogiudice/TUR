// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! End-to-end tests of the `tur-catalogue` binary against the seed
//! catalogue: its output and exit status.
//!
//! Nothing here is executed except the `tur-catalogue` binary itself, and
//! that binary only reads.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

const EXIT_OK: i32 = 0;
const EXIT_UNREADABLE: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_NO_ROUTE: i32 = 3;
const EXIT_NEEDS_SETUP: i32 = 4;
const EXIT_INVALID: i32 = 5;
const EXIT_NOT_FOUND: i32 = 6;

/// A scratch directory removed on drop.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let process = std::process::id();
        let path =
            std::env::temp_dir().join(format!("tur-catalogue-cli-{process}-{unique}-{label}"));
        std::fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }

    /// Writes a file and returns its path as text.
    fn file(&self, name: &str, text: &str) -> String {
        let path = self.path.join(name);
        std::fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_owned()
    }

    fn path(&self) -> String {
        self.path.to_str().unwrap().to_owned()
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
    let output = Command::new(env!("CARGO_BIN_EXE_tur-catalogue"))
        .args(arguments)
        .output()
        .unwrap();
    Outcome {
        status: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn repository() -> String {
    format!("{}/../..", env!("CARGO_MANIFEST_DIR"))
}

/// The seed catalogue of this repository.
fn seed() -> String {
    format!("{}/catalogue", repository())
}

/// Runs a command against the seed catalogue.
fn on_seed(arguments: &[&str]) -> Outcome {
    let seed = seed();
    let mut all = vec!["--catalogue", seed.as_str()];
    all.extend_from_slice(arguments);
    run(&all)
}

fn json(outcome: &Outcome) -> Value {
    serde_json::from_str(&outcome.stdout)
        .unwrap_or_else(|error| panic!("not JSON ({error}): {}", outcome.stdout))
}

fn option_routes(report: &Value) -> Vec<String> {
    report["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| option["route"].as_str().unwrap().to_owned())
        .collect()
}

const FULL_HOST: &str = r#"{
  "arch": "x86_64", "flatpak": true, "distro_containers": true, "wine": true, "proton": true,
  "android_runtime": true, "google_play": true, "windows_guest": true, "windows_licence": true,
  "darling_enabled": false, "gpu": "discrete", "memory_mib": 32768
}"#;

fn entry_text(id: &str, name: &str, record: &str) -> String {
    format!(
        "schema_version = 1\nid = \"{id}\"\nname = \"{name}\"\npublisher = \"Example\"\ncategory = \"utility\"\n\n[[editions]]\nkind = \"windows\"\nobtain = \"publisher-page\"\ndownload_page = \"https://example.org/download\"\n{record}"
    )
}

#[test]
fn validate_accepts_the_seed_catalogue() {
    let seed = seed();
    let outcome = run(&["validate", &seed]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    assert!(outcome.stdout.starts_with("valid: "), "{}", outcome.stdout);
    let applications: usize = outcome
        .stdout
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert!(applications >= 25, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("0 rated by TUR tests"),
        "{}",
        outcome.stdout
    );
    assert!(outcome.stderr.is_empty(), "{}", outcome.stderr);

    // The directory can also be given with --catalogue, and the report
    // files of TUR tests checked against the repository.
    let repository = repository();
    let outcome = on_seed(&["validate", "--repository", &repository]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
}

#[test]
fn validate_refuses_a_tur_test_claim_without_a_report() {
    let scratch = Scratch::new("claim");
    let record = "\n[[editions.records]]\nroute = \"wine\"\nrating = \"good\"\nreported_by = \"tur-test\"\nversion_tested = \"1.0\"\ndate = \"2026-11-12\"\n";
    scratch.file(
        "org.example.app.toml",
        &entry_text("org.example.app", "Example", record),
    );
    let outcome = run(&["validate", &scratch.path()]);
    assert_eq!(outcome.status, EXIT_INVALID);
    assert!(
        outcome
            .stderr
            .contains("error[UOS-DESKTOP-CATALOGUE-006]: org.example.app.toml:"),
        "{}",
        outcome.stderr
    );
    assert!(outcome.stderr.contains("test_report"), "{}", outcome.stderr);
    assert!(outcome.stdout.is_empty(), "{}", outcome.stdout);
}

#[test]
fn validate_checks_that_a_named_test_report_exists() {
    let scratch = Scratch::new("report");
    let record = "\n[[editions.records]]\nroute = \"wine\"\nrating = \"good\"\nreported_by = \"tur-test\"\ntest_report = \"docs/reports/compat/example.md\"\nversion_tested = \"1.0\"\ndate = \"2026-11-12\"\n";
    std::fs::create_dir_all(scratch.path.join("catalogue")).unwrap();
    scratch.file(
        "catalogue/org.example.app.toml",
        &entry_text("org.example.app", "Example", record),
    );
    let catalogue = format!("{}/catalogue", scratch.path());

    // Without the repository the file is not checked, and the report says so.
    let outcome = run(&["validate", &catalogue]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    assert!(
        outcome.stdout.contains("1 rated by TUR tests"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("were not checked"),
        "{}",
        outcome.stdout
    );

    let outcome = run(&["validate", &catalogue, "--repository", &scratch.path()]);
    assert_eq!(outcome.status, EXIT_INVALID);
    assert!(
        outcome.stderr.contains("UOS-DESKTOP-CATALOGUE-006"),
        "{}",
        outcome.stderr
    );
    assert!(
        outcome.stderr.contains("does not exist"),
        "{}",
        outcome.stderr
    );

    std::fs::create_dir_all(scratch.path.join("docs/reports/compat")).unwrap();
    scratch.file("docs/reports/compat/example.md", "# Test report\n");
    let outcome = run(&["validate", &catalogue, "--repository", &scratch.path()]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
}

#[test]
fn validate_reports_every_problem_with_its_code() {
    let scratch = Scratch::new("bad");
    scratch.file(
        "org.example.one.toml",
        &entry_text("org.example.one", "Same Name", ""),
    );
    scratch.file(
        "org.example.two.toml",
        &entry_text("org.example.two", "same-name", ""),
    );
    scratch.file(
        "org.example.three.toml",
        &entry_text("org.example.three", "Three", "colour = \"red\"\n"),
    );
    let unsourced = "\n[[editions.records]]\nroute = \"flatpak\"\nrating = \"good\"\nreported_by = \"nobody\"\ndate = \"2026-11-12\"\n";
    scratch.file(
        "org.example.four.toml",
        &entry_text("org.example.four", "Four", unsourced),
    );
    let outcome = run(&["validate", &scratch.path()]);
    assert_eq!(outcome.status, EXIT_INVALID);
    for expected in [
        "error[UOS-DESKTOP-CATALOGUE-005]: org.example.two.toml:",
        "error[UOS-DESKTOP-CATALOGUE-002]: org.example.three.toml:",
        "error[UOS-DESKTOP-CATALOGUE-006]: org.example.four.toml:",
        "error[UOS-DESKTOP-CATALOGUE-007]: org.example.four.toml:",
        "4 problems found",
    ] {
        assert!(
            outcome.stderr.contains(expected),
            "missing {expected} in:\n{}",
            outcome.stderr
        );
    }
}

#[test]
fn validate_fails_on_a_missing_directory() {
    let scratch = Scratch::new("missing");
    let absent = format!("{}/absent", scratch.path());
    let outcome = run(&["validate", &absent]);
    assert_eq!(outcome.status, EXIT_UNREADABLE);
    assert!(
        outcome.stderr.contains("error[UOS-DESKTOP-CATALOGUE-001]"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn search_finds_applications_and_tolerates_typos() {
    let outcome = on_seed(&["search", "elden", "ring"]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    assert!(
        outcome
            .stdout
            .starts_with("com.bandainamco.eldenring  Elden Ring (game, "),
        "{}",
        outcome.stdout
    );

    let outcome = on_seed(&["search", "FOTOSHOP"]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    assert!(
        outcome.stdout.starts_with("com.adobe.photoshop  "),
        "{}",
        outcome.stdout
    );

    // An alias that matched is named.
    let outcome = on_seed(&["search", "bg3"]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    assert!(
        outcome
            .stdout
            .starts_with("com.larian.baldursgate3  Baldur's Gate 3 (game, Larian Studios)"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("[matched \"BG3\"]"),
        "{}",
        outcome.stdout
    );

    let outcome = on_seed(&["search", "vs code", "--json"]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    let report = json(&outcome);
    assert_eq!(report["query"], "vs code");
    assert_eq!(report["hits"][0]["id"], "com.microsoft.vscode");
    assert_eq!(report["hits"][0]["kind"], "exact");
    assert_eq!(report["hits"][0]["matched"], "VS Code");
}

#[test]
fn search_says_when_nothing_matches() {
    let outcome = on_seed(&["search", "qqqqzzzzqqqq"]);
    assert_eq!(outcome.status, EXIT_NOT_FOUND);
    assert!(
        outcome.stderr.contains("error[UOS-DESKTOP-CATALOGUE-010]"),
        "{}",
        outcome.stderr
    );
    assert!(outcome.stdout.is_empty());

    let outcome = on_seed(&["search", "qqqqzzzzqqqq", "--json"]);
    assert_eq!(outcome.status, EXIT_NOT_FOUND);
    assert_eq!(json(&outcome)["hits"].as_array().unwrap().len(), 0);
}

#[test]
fn show_prints_an_entry() {
    let outcome = on_seed(&["show", "org.mozilla.firefox"]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    for expected in [
        "Firefox (org.mozilla.firefox)",
        "Publisher: Mozilla",
        "Category: internet",
        "Flatpak edition: org.mozilla.firefox from flathub",
        "Native Linux edition: Fedora package firefox",
        "Compatibility: no record; every route is unknown.",
    ] {
        assert!(
            outcome.stdout.contains(expected),
            "missing {expected} in:\n{}",
            outcome.stdout
        );
    }

    // A rating is shown with the source that reported it.
    let outcome = on_seed(&["show", "com.microsoft.visualstudio"]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    assert!(
        outcome
            .stdout
            .contains("Compatibility, wine: broken, reported by WineHQ AppDB as \"Garbage\""),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("not tested by TUR"),
        "{}",
        outcome.stdout
    );

    let outcome = on_seed(&["show", "com.riotgames.valorant", "--json"]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    let entry = json(&outcome);
    assert_eq!(entry["id"], "com.riotgames.valorant");
    assert_eq!(entry["category"], "game");
    assert_eq!(entry["editions"][0]["requirements"][0], "kernel-anti-cheat");
}

#[test]
fn show_and_route_fail_for_an_unknown_application() {
    for command in ["show", "route"] {
        let outcome = on_seed(&[command, "org.example.absent"]);
        assert_eq!(outcome.status, EXIT_NOT_FOUND);
        assert!(
            outcome.stderr.contains("error[UOS-DESKTOP-CATALOGUE-009]"),
            "{}",
            outcome.stderr
        );
        let outcome = on_seed(&[command, "org.example.absent", "--json"]);
        assert_eq!(outcome.status, EXIT_NOT_FOUND);
        assert_eq!(json(&outcome)["error"]["code"], "UOS-DESKTOP-CATALOGUE-009");
    }
}

#[test]
fn route_prefers_the_native_linux_edition() {
    let outcome = on_seed(&["route", "org.mozilla.firefox"]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    assert!(
        outcome.stdout.contains("1. Flatpak  [recommended]"),
        "{}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("Host: assumed, not detected"),
        "{}",
        outcome.stdout
    );
    assert!(
        !outcome.stdout.contains("Windows guest"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn route_states_no_route_and_dual_boot_for_a_game_with_kernel_anti_cheat() {
    let outcome = on_seed(&["route", "com.riotgames.valorant"]);
    assert_eq!(outcome.status, EXIT_NO_ROUTE);
    for expected in [
        "1. No route  [recommended]",
        "This game's anti-cheat will not work",
        "Least-bad alternative: Dual boot",
        "2. Proton (through umu-launcher)",
        "Outlook: cannot work on this machine",
        "Rating: broken, reported by AreWeAntiCheatYet as \"Denied\"",
    ] {
        assert!(
            outcome.stdout.contains(expected),
            "missing {expected} in:\n{}",
            outcome.stdout
        );
    }
    assert!(
        outcome.stderr.contains("error[UOS-DESKTOP-CATALOGUE-011]"),
        "{}",
        outcome.stderr
    );

    let outcome = on_seed(&["route", "com.riotgames.valorant", "--json"]);
    assert_eq!(outcome.status, EXIT_NO_ROUTE);
    let report = json(&outcome);
    assert_eq!(report["recommended"], "no-route");
    assert_eq!(option_routes(&report), ["no-route", "proton"]);
    assert_eq!(report["options"][0]["alternative"]["kind"], "dual-boot");
    assert_eq!(report["options"][1]["outlook"], "blocked");
    assert_eq!(report["host_assumed"], true);
}

#[test]
fn route_reads_host_facts_and_reports_the_rating_with_its_source() {
    let scratch = Scratch::new("host");
    let host = scratch.file("host.json", FULL_HOST);

    // Without host facts Proton is not installed: the route exists and
    // needs something first.
    let outcome = on_seed(&["route", "com.bandainamco.eldenring", "--json"]);
    assert_eq!(outcome.status, EXIT_NEEDS_SETUP, "{}", outcome.stderr);
    let report = json(&outcome);
    assert_eq!(
        report["options"][0]["prerequisites"][0]["code"],
        "install-proton"
    );

    let outcome = on_seed(&[
        "route",
        "com.bandainamco.eldenring",
        "--host",
        &host,
        "--json",
    ]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    let report = json(&outcome);
    assert_eq!(report["application"]["id"], "com.bandainamco.eldenring");
    assert_eq!(report["host_assumed"], false);
    assert_eq!(report["host"]["proton"], true);
    assert_eq!(report["recommended"], "proton");
    assert_eq!(option_routes(&report), ["proton"]);
    let option = &report["options"][0];
    assert_eq!(option["rating"], "usable");
    assert_eq!(option["rating_source"]["kind"], "external");
    assert_eq!(option["rating_source"]["name"], "ProtonDB");
    assert_eq!(option["rating_source"]["source_rating"], "gold");
    assert_eq!(option["recommended"], true);
    assert_eq!(option["prerequisites"].as_array().unwrap().len(), 0);
}

#[test]
fn route_lists_wine_before_the_guest_and_honours_never_use_a_virtual_machine() {
    let scratch = Scratch::new("prefs");
    let host = scratch.file("host.json", FULL_HOST);
    let no_vm = scratch.file("prefs.json", r#"{"never_use_virtual_machine": true}"#);

    let outcome = on_seed(&["route", "com.microsoft.office", "--host", &host, "--json"]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    let report = json(&outcome);
    assert_eq!(option_routes(&report), ["wine", "windows-guest", "web"]);
    assert_eq!(report["recommended"], "windows-guest");

    let outcome = on_seed(&[
        "route",
        "com.microsoft.office",
        "--host",
        &host,
        "--prefs",
        &no_vm,
        "--json",
    ]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    let report = json(&outcome);
    assert_eq!(option_routes(&report), ["wine", "web"]);
    assert_eq!(report["recommended"], "web");
    let caveats = report["options"][0]["caveats"].as_array().unwrap();
    let lost = caveats
        .iter()
        .find(|caveat| caveat["code"] == "guest-not-offered")
        .expect("the loss of the guest is stated");
    assert!(lost["text"].as_str().unwrap().contains("What is lost"));

    // Asking for the guest: offered after Wine, even where Wine is rated usable.
    let outcome = on_seed(&[
        "route",
        "com.adobe.photoshop",
        "--host",
        &host,
        "--offer-guest",
        "--json",
    ]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    let report = json(&outcome);
    assert_eq!(option_routes(&report), ["wine", "windows-guest"]);
    assert_eq!(report["recommended"], "wine");
}

#[test]
fn route_honours_a_pinned_route() {
    let scratch = Scratch::new("pin");
    let host = scratch.file("host.json", FULL_HOST);
    let pinned = scratch.file(
        "prefs.json",
        r#"{"pinned_routes": {"com.discord.discord": "web"}}"#,
    );
    let outcome = on_seed(&[
        "route",
        "com.discord.discord",
        "--host",
        &host,
        "--prefs",
        &pinned,
        "--json",
    ]);
    assert_eq!(outcome.status, EXIT_OK, "{}", outcome.stderr);
    let report = json(&outcome);
    assert_eq!(report["recommended"], "web");
    assert_eq!(option_routes(&report)[0], "flatpak");
    let web = report["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["route"] == "web")
        .unwrap();
    assert_eq!(web["pinned"], true);
}

#[test]
fn route_refuses_bad_host_facts_and_preferences() {
    let scratch = Scratch::new("input");
    let unknown_field = scratch.file("host.json", r#"{"wine": true, "bottles": true}"#);
    let outcome = on_seed(&["route", "org.mozilla.firefox", "--host", &unknown_field]);
    assert_eq!(outcome.status, EXIT_UNREADABLE);
    assert!(
        outcome.stderr.contains("error[UOS-DESKTOP-CATALOGUE-012]"),
        "{}",
        outcome.stderr
    );

    let bad_pin = scratch.file("prefs.json", r#"{"pinned_routes": {"a.b": "no-route"}}"#);
    let outcome = on_seed(&[
        "route",
        "org.mozilla.firefox",
        "--prefs",
        &bad_pin,
        "--json",
    ]);
    assert_eq!(outcome.status, EXIT_UNREADABLE);
    assert_eq!(json(&outcome)["error"]["code"], "UOS-DESKTOP-CATALOGUE-012");

    let absent = format!("{}/absent.json", scratch.path());
    let outcome = on_seed(&["route", "org.mozilla.firefox", "--host", &absent]);
    assert_eq!(outcome.status, EXIT_UNREADABLE);
}

#[test]
fn a_wrong_command_line_is_a_usage_error() {
    assert_eq!(run(&[]).status, EXIT_USAGE);
    assert_eq!(on_seed(&["search"]).status, EXIT_USAGE);
    assert_eq!(on_seed(&["route"]).status, EXIT_USAGE);
    assert_eq!(
        on_seed(&["install", "org.mozilla.firefox"]).status,
        EXIT_USAGE
    );
    assert_eq!(
        on_seed(&["launch", "org.mozilla.firefox"]).status,
        EXIT_USAGE
    );
}
