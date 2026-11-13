// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The application centre's fixtures, checked against the router.
//!
//! `tur-centre` (`desktop/apps/tur-centre`) is built, tested and photographed
//! in CI without this crate: it answers from files recorded under
//! `desktop/apps/tur-centre/fixtures`
//! (docs/architecture/desktop-experience.md, section 8.10). Those files are
//! the output of the `tur-catalogue` binary for the seed catalogue. This test
//! runs the binary again for every recorded request and fails when a file
//! differs from what the binary prints, when one is missing, and when one is
//! there that nothing produces. A picture of the application centre can
//! therefore never show a rating, a caveat or a sentence the router does not
//! give.
//!
//! Nobody can run the binary on a development machine (AGENTS.md), so when
//! the files must change this test prints what they should contain, as
//! `TUR-FIXTURE` lines, and
//! `desktop/apps/tur-centre/fixtures/update.py JOB_ID` turns those lines of
//! the job's log back into the files.
//!
//! Nothing here is executed except the `tur-catalogue` binary itself, and
//! that binary only reads.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;
use serde_json::Value;
use tur_catalogue::{
    ALTERNATIVE_TEXT_IDS, AlternativeKind, CAVEAT_TEXT_IDS, Catalogue, Category, CaveatCode, Code,
    Date, EditionKind, MatchKind, Obtain, Outlook, PrerequisiteCode, Rating, RatingSource,
    ReasonCode, Reporter, Requirement, Route, Severity,
};

/// The identifier every file under `desktop/` carries in its first lines.
const LICENCE: &str = "SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0";

/// Says where a fixture file comes from.
const GENERATED_BY: &str = "services/tur-catalogue/tests/desktop_fixtures.rs writes this from the output of tur-catalogue for the seed catalogue; do not edit";

/// The machine the fixtures of the set `default` are routed for: the one the
/// drawings of the design assume. It has the compatibility layers and enough
/// memory, and neither Windows nor Android is set up.
const STATED_HOST: &str = r#"{
  "arch": "x86_64", "flatpak": true, "distro_containers": true, "wine": true, "proton": true,
  "android_runtime": false, "arm_translation": false, "google_play": false,
  "windows_guest": false, "windows_licence": false, "darling_enabled": false,
  "gpu": "integrated", "memory_mib": 32768
}"#;

/// What stands in the recorded arguments for the host-facts file, whose real
/// path differs from machine to machine.
const HOST_PLACEHOLDER: &str = "<host-facts>";

/// One directory of fixtures: the answers for one machine.
struct Set {
    /// The directory, under the fixtures root.
    directory: &'static str,
    /// The host facts the routes are computed for; `None` passes no
    /// `--host`, so the router assumes the machine and says so.
    host: Option<&'static str>,
    /// The searches to record. Every application a search finds is recorded
    /// as well, so that a result row always has its route.
    searches: &'static [&'static str],
    /// The applications to record: `show` and `route` of each.
    applications: &'static [&'static str],
    /// Identifiers no application has: the error of `show` and of `route`.
    unknown: &'static [&'static str],
}

/// The fixtures the application centre uses.
const SETS: [Set; 2] = [
    Set {
        directory: "default",
        host: Some(STATED_HOST),
        searches: &[
            "office",
            "ms office",
            "ofice",
            "krita",
            "valorant",
            "banking app",
            "elden ring",
            "photothing",
        ],
        applications: &[
            "com.microsoft.office",
            "com.riotgames.valorant",
            "org.kde.krita",
            "tur.class.android-banking",
            "com.bandainamco.eldenring",
            // Installing (D-11): the Flatpak the end-to-end test installs,
            // Chrome offered to people coming from Windows, and a Windows
            // application whose installer TUR can download.
            "net.sourceforge.chromium-bsu",
            "com.google.chrome",
            "org.notepad-plus-plus.notepad-plus-plus",
        ],
        unknown: &["org.example.missing"],
    },
    Set {
        directory: "assumed-host",
        host: None,
        searches: &[],
        applications: &["com.netflix.netflix"],
        unknown: &[],
    },
];

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
            std::env::temp_dir().join(format!("tur-catalogue-fixtures-{process}-{unique}-{label}"));
        std::fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }

    /// Writes a file and returns its path as text.
    fn file(&self, name: &str, text: &str) -> String {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures_root() -> PathBuf {
    repository_root().join("desktop/apps/tur-centre/fixtures")
}

/// One run of the binary, as it is recorded.
struct Invocation {
    command: &'static str,
    subject: String,
    host: &'static str,
    arguments: Vec<String>,
    exit_status: i32,
    stdout: String,
    stderr: String,
}

/// Runs `tur-catalogue <command> --json [--host FILE] -- <subject>` against
/// the seed catalogue: the arguments the application centre passes.
fn invoke(command: &'static str, subject: &str, host_file: Option<&str>) -> Invocation {
    let mut arguments = vec![command.to_owned(), "--json".to_owned()];
    let mut recorded = arguments.clone();
    let routed = command == "route";
    if routed && let Some(file) = host_file {
        arguments.extend(["--host".to_owned(), file.to_owned()]);
        recorded.extend(["--host".to_owned(), HOST_PLACEHOLDER.to_owned()]);
    }
    for list in [&mut arguments, &mut recorded] {
        list.extend(["--".to_owned(), subject.to_owned()]);
    }

    let seed = repository_root().join("catalogue");
    let output = Command::new(env!("CARGO_BIN_EXE_tur-catalogue"))
        .arg("--catalogue")
        .arg(&seed)
        .args(&arguments)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    if !stdout.trim().is_empty() {
        let parsed: Result<Value, _> = serde_json::from_str(&stdout);
        assert!(
            parsed.is_ok(),
            "tur-catalogue {arguments:?} did not print JSON: {stdout}"
        );
    }
    Invocation {
        command,
        subject: subject.to_owned(),
        host: if !routed {
            "not used"
        } else if host_file.is_some() {
            "stated"
        } else {
            "assumed"
        },
        arguments: recorded,
        exit_status: output.status.code().unwrap(),
        stdout,
        stderr,
    }
}

/// Runs `tur-catalogue validate --json` against the seed catalogue: the
/// counts the application centre's Home view reads, to say whether TUR has
/// rated any application itself (desktop-experience.md, 8.1).
fn invoke_validate() -> Invocation {
    let arguments = vec!["validate".to_owned(), "--json".to_owned()];
    let seed = repository_root().join("catalogue");
    let output = Command::new(env!("CARGO_BIN_EXE_tur-catalogue"))
        .arg("--catalogue")
        .arg(&seed)
        .args(&arguments)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["valid"], true, "{stdout}");
    Invocation {
        command: "validate",
        subject: String::new(),
        host: "not used",
        arguments,
        exit_status: output.status.code().unwrap(),
        stdout,
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// A JSON string literal.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

/// A JSON list of strings, on one line.
fn quoted_list(values: &[String]) -> String {
    let items: Vec<String> = values.iter().map(|value| quoted(value)).collect();
    format!("[{}]", items.join(", "))
}

/// The binary's output as a JSON value inside a larger document: its own
/// text, with every line after the first moved in by `by` spaces. Nothing is
/// parsed and written again, so the order of the fields and every character
/// are the router's.
fn embedded(output: &str, by: usize) -> String {
    let text = output.trim_end();
    if text.is_empty() {
        return "null".to_owned();
    }
    let pad = " ".repeat(by);
    let mut lines = text.lines();
    let mut out = lines.next().unwrap_or_default().to_owned();
    for line in lines {
        out.push('\n');
        out.push_str(&pad);
        out.push_str(line);
    }
    out
}

/// The opening lines of every fixture file. JSON has no comments, so the
/// licence identifier is a string, in the first lines where the licence
/// boundary check of `desktop/` looks for it.
fn header() -> String {
    let mut out = String::from("{\n");
    let _ = writeln!(out, "  \"licence\": {},", quoted(LICENCE));
    let _ = writeln!(out, "  \"generated_by\": {},", quoted(GENERATED_BY));
    out
}

/// A fixture file: the recorded runs, in the order given.
fn document(invocations: &[Invocation]) -> String {
    let mut out = header();
    out.push_str("  \"invocations\": [\n");
    for (position, invocation) in invocations.iter().enumerate() {
        out.push_str("    {\n");
        let _ = writeln!(out, "      \"command\": {},", quoted(invocation.command));
        let _ = writeln!(out, "      \"subject\": {},", quoted(&invocation.subject));
        let _ = writeln!(out, "      \"host\": {},", quoted(invocation.host));
        let _ = writeln!(
            out,
            "      \"arguments\": {},",
            quoted_list(&invocation.arguments)
        );
        let _ = writeln!(out, "      \"exit_status\": {},", invocation.exit_status);
        let _ = writeln!(out, "      \"stderr\": {},", quoted(&invocation.stderr));
        let _ = writeln!(out, "      \"stdout\": {}", embedded(&invocation.stdout, 6));
        out.push_str(if position + 1 == invocations.len() {
            "    }\n"
        } else {
            "    },\n"
        });
    }
    out.push_str("  ]\n}\n");
    out
}

/// The name a value has in the router's JSON.
fn name_of<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value).unwrap() {
        Value::String(name) => name,
        Value::Object(fields) => fields["kind"].as_str().unwrap().to_owned(),
        other => panic!("{other} is not a name"),
    }
}

fn names_of<T: Serialize>(values: &[T]) -> Vec<String> {
    values.iter().map(name_of).collect()
}

/// Every value of the router's types that the application centre shows in
/// words (desktop-experience.md, section 8.2). `tur-centre` has a test that
/// fails when one of these has no label, so a new value cannot reach the
/// screen unnamed (the design's risk 17).
///
/// The types that have no `ALL` table are listed here. Each list is guarded
/// the way `codes.rs` guards its table: the match fails to compile when a
/// variant is added, and the count fails when it is missing from the list.
fn vocabulary() -> String {
    let outlooks = [Outlook::Clear, Outlook::Doubtful, Outlook::Blocked];
    let count = |value: &Outlook| match value {
        Outlook::Clear | Outlook::Doubtful | Outlook::Blocked => 1usize,
    };
    assert_eq!(outlooks.iter().map(count).sum::<usize>(), 3);

    let severities = [
        Severity::Note,
        Severity::Warning,
        Severity::Obstacle,
        Severity::Blocker,
    ];
    let count = |value: &Severity| match value {
        Severity::Note | Severity::Warning | Severity::Obstacle | Severity::Blocker => 1usize,
    };
    assert_eq!(severities.iter().map(count).sum::<usize>(), 4);

    let caveat_codes = [
        CaveatCode::Requirement,
        CaveatCode::Architecture,
        CaveatCode::GuestMemory,
        CaveatCode::Experimental,
        CaveatCode::ClosedStore,
        CaveatCode::CommunityPackage,
        CaveatCode::ReportedInadequate,
        CaveatCode::GuestNotOffered,
        CaveatCode::GuestUnsuitableForGames,
        CaveatCode::PinUnavailable,
    ];
    let count = |value: &CaveatCode| match value {
        CaveatCode::Requirement
        | CaveatCode::Architecture
        | CaveatCode::GuestMemory
        | CaveatCode::Experimental
        | CaveatCode::ClosedStore
        | CaveatCode::CommunityPackage
        | CaveatCode::ReportedInadequate
        | CaveatCode::GuestNotOffered
        | CaveatCode::GuestUnsuitableForGames
        | CaveatCode::PinUnavailable => 1usize,
    };
    assert_eq!(caveat_codes.iter().map(count).sum::<usize>(), 10);

    let prerequisite_codes = [
        PrerequisiteCode::InstallFlatpak,
        PrerequisiteCode::InstallContainerTool,
        PrerequisiteCode::InstallWine,
        PrerequisiteCode::InstallProton,
        PrerequisiteCode::EnableAndroidRuntime,
        PrerequisiteCode::OptIntoGooglePlay,
        PrerequisiteCode::SetUpWindowsGuest,
        PrerequisiteCode::ProvideWindowsLicence,
    ];
    let count = |value: &PrerequisiteCode| match value {
        PrerequisiteCode::InstallFlatpak
        | PrerequisiteCode::InstallContainerTool
        | PrerequisiteCode::InstallWine
        | PrerequisiteCode::InstallProton
        | PrerequisiteCode::EnableAndroidRuntime
        | PrerequisiteCode::OptIntoGooglePlay
        | PrerequisiteCode::SetUpWindowsGuest
        | PrerequisiteCode::ProvideWindowsLicence => 1usize,
    };
    assert_eq!(prerequisite_codes.iter().map(count).sum::<usize>(), 8);

    let alternative_kinds = [
        AlternativeKind::DualBoot,
        AlternativeKind::CertifiedAndroidDevice,
        AlternativeKind::Mac,
        AlternativeKind::AnotherComputer,
        AlternativeKind::NoneKnown,
    ];
    let count = |value: &AlternativeKind| match value {
        AlternativeKind::DualBoot
        | AlternativeKind::CertifiedAndroidDevice
        | AlternativeKind::Mac
        | AlternativeKind::AnotherComputer
        | AlternativeKind::NoneKnown => 1usize,
    };
    assert_eq!(alternative_kinds.iter().map(count).sum::<usize>(), 5);

    let rating_sources = [
        RatingSource::Unrated,
        RatingSource::External {
            name: String::new(),
            url: String::new(),
            source_rating: None,
            version_tested: None,
            date: String::new(),
        },
        RatingSource::TurTest {
            report: String::new(),
            version_tested: None,
            date: String::new(),
        },
    ];
    let count = |value: &RatingSource| match value {
        RatingSource::Unrated | RatingSource::External { .. } | RatingSource::TurTest { .. } => {
            1usize
        }
    };
    assert_eq!(rating_sources.iter().map(count).sum::<usize>(), 3);

    let match_kinds = [
        MatchKind::Exact,
        MatchKind::Prefix,
        MatchKind::Words,
        MatchKind::Substring,
        MatchKind::Typo,
    ];
    let count = |value: &MatchKind| match value {
        MatchKind::Exact
        | MatchKind::Prefix
        | MatchKind::Words
        | MatchKind::Substring
        | MatchKind::Typo => 1usize,
    };
    assert_eq!(match_kinds.iter().map(count).sum::<usize>(), 5);

    let codes: Vec<String> = Code::ALL
        .iter()
        .map(|code| code.as_str().to_owned())
        .collect();

    // The identifiers of the router's sentences (issue #41), for the
    // application centre's translations. Additive: the centre reads the
    // lists it knows and ignores the others.
    let text_ids =
        |ids: &[&str]| -> Vec<String> { ids.iter().map(|id| (*id).to_owned()).collect() };

    let lists: [(&str, Vec<String>); 17] = [
        ("reason_codes", names_of(&ReasonCode::ALL)),
        ("caveat_text_ids", text_ids(&CAVEAT_TEXT_IDS)),
        ("alternative_text_ids", text_ids(&ALTERNATIVE_TEXT_IDS)),
        ("routes", names_of(&Route::ALL)),
        ("ratings", names_of(&Rating::ALL)),
        // Every reporter: the application centre names a record of TUR's
        // compatibility autopilot ("Works with these settings") in its
        // "All reports" list, although such a record never rates a route.
        ("reporters", names_of(&Reporter::EVERY)),
        ("rating_source_kinds", names_of(&rating_sources)),
        ("outlooks", names_of(&outlooks)),
        ("severities", names_of(&severities)),
        ("caveat_codes", names_of(&caveat_codes)),
        ("prerequisite_codes", names_of(&prerequisite_codes)),
        ("alternative_kinds", names_of(&alternative_kinds)),
        ("requirements", names_of(&Requirement::ALL)),
        ("categories", names_of(&Category::ALL)),
        ("edition_kinds", names_of(&EditionKind::ALL)),
        ("obtain", names_of(&Obtain::ALL)),
        ("match_kinds", names_of(&match_kinds)),
    ];

    let mut out = header();
    for (name, values) in &lists {
        let _ = writeln!(out, "  {}: {},", quoted(name), quoted_list(values));
    }
    let _ = writeln!(out, "  \"error_codes\": {}", quoted_list(&codes));
    out.push_str("}\n");
    out
}

/// Every fixture file as it should be: its path under the fixtures root, and
/// its content.
fn expected_files() -> BTreeMap<String, String> {
    let scratch = Scratch::new("host");
    let mut files = BTreeMap::new();
    files.insert("vocabulary.json".to_owned(), vocabulary());

    for set in &SETS {
        let host_file = set
            .host
            .map(|facts| scratch.file(&format!("{}/host-facts.json", set.directory), facts));
        let host_file = host_file.as_deref();

        files.insert(
            format!("{}/validate.json", set.directory),
            document(&[invoke_validate()]),
        );

        let mut applications: BTreeSet<String> =
            set.applications.iter().map(|id| (*id).to_owned()).collect();

        let mut searches = Vec::new();
        for query in set.searches {
            let invocation = invoke("search", query, None);
            let report: Value = serde_json::from_str(&invocation.stdout).unwrap();
            for hit in report["hits"].as_array().unwrap() {
                applications.insert(hit["id"].as_str().unwrap().to_owned());
            }
            searches.push(invocation);
        }
        if !searches.is_empty() {
            files.insert(
                format!("{}/search.json", set.directory),
                document(&searches),
            );
        }

        let unknown = set.unknown.iter().map(|id| (*id).to_owned());
        for id in applications.into_iter().chain(unknown) {
            files.insert(
                format!("{}/show-{id}.json", set.directory),
                document(&[invoke("show", &id, None)]),
            );
            files.insert(
                format!("{}/route-{id}.json", set.directory),
                document(&[invoke("route", &id, host_file)]),
            );
        }
    }
    files
}

/// What is wrong with the fixture files under `root`: one line for each file
/// that differs from `expected`, is missing, or is there and not expected.
fn differences(root: &Path, expected: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    for (path, content) in expected {
        match std::fs::read_to_string(root.join(path)) {
            Ok(actual) if &actual == content => {}
            Ok(_) => out.push(format!("{path}: differs from what the router prints")),
            Err(_) => out.push(format!("{path}: missing")),
        }
    }

    let mut directories = vec![String::new()];
    directories.extend(SETS.iter().map(|set| format!("{}/", set.directory)));
    for directory in directories {
        let Ok(listing) = std::fs::read_dir(root.join(&directory)) else {
            continue;
        };
        let mut names: Vec<String> = listing
            .filter_map(Result::ok)
            .filter_map(|item| item.file_name().into_string().ok())
            .filter(|name| name.ends_with(".json"))
            .collect();
        names.sort();
        for name in names {
            let path = format!("{directory}{name}");
            if !expected.contains_key(&path) {
                out.push(format!("{path}: stale: nothing produces it; remove it"));
            }
        }
    }
    out
}

/// Prints a file as lines that `update.py` turns back into the file: its
/// bytes in hexadecimal, so that no log viewer can alter them.
fn print_for_update(path: &str, content: &str) {
    for (number, chunk) in content.as_bytes().chunks(96).enumerate() {
        let mut hex = String::with_capacity(chunk.len() * 2);
        for byte in chunk {
            let _ = write!(hex, "{byte:02x}");
        }
        println!("TUR-FIXTURE {path} {number} {hex}");
    }
    println!("TUR-FIXTURE-END {path} {}", content.len());
}

#[test]
fn the_application_centre_s_fixtures_are_what_the_router_prints() {
    let expected = expected_files();
    let root = fixtures_root();
    let problems = differences(&root, &expected);
    if problems.is_empty() {
        return;
    }

    for problem in &problems {
        let (path, what) = problem.split_once(": ").unwrap();
        if what.starts_with("stale") {
            println!("TUR-FIXTURE-STALE {path}");
        } else {
            print_for_update(path, &expected[path]);
        }
    }
    panic!(
        "{} fixture file(s) of the application centre are not what tur-catalogue prints for the seed catalogue:\n  {}\nThe lines above that begin with TUR-FIXTURE hold the files as they should be. To accept them, look at what changed and why, then run\n  desktop/apps/tur-centre/fixtures/update.py JOB_ID\nwith the ID of this job, look at the difference, and commit it with the change that caused it. The pictures of the application centre are made from these files and will change with them.",
        problems.len(),
        problems.join("\n  ")
    );
}

#[test]
fn a_fixture_that_differs_is_missing_or_is_stale_is_reported() {
    let mut expected = BTreeMap::new();
    expected.insert("vocabulary.json".to_owned(), "{ \"a\": 1 }\n".to_owned());
    expected.insert(
        "default/search.json".to_owned(),
        "{ \"b\": 2 }\n".to_owned(),
    );
    expected.insert(
        "default/route-org.example.app.json".to_owned(),
        "{ \"c\": 3 }\n".to_owned(),
    );

    let same = Scratch::new("same");
    for (path, content) in &expected {
        same.file(path, content);
    }
    assert_eq!(differences(&same.path, &expected), Vec::<String>::new());

    let changed = Scratch::new("changed");
    for (path, content) in &expected {
        changed.file(path, content);
    }
    changed.file("default/search.json", "{ \"b\": 3 }\n");
    assert_eq!(
        differences(&changed.path, &expected),
        ["default/search.json: differs from what the router prints"]
    );

    let missing = Scratch::new("missing");
    missing.file("vocabulary.json", &expected["vocabulary.json"]);
    missing.file("default/search.json", &expected["default/search.json"]);
    assert_eq!(
        differences(&missing.path, &expected),
        ["default/route-org.example.app.json: missing"]
    );

    let stale = Scratch::new("stale");
    for (path, content) in &expected {
        stale.file(path, content);
    }
    stale.file("default/show-org.example.gone.json", "{}\n");
    stale.file("default/notes.txt", "not a fixture\n");
    assert_eq!(
        differences(&stale.path, &expected),
        ["default/show-org.example.gone.json: stale: nothing produces it; remove it"]
    );
}

#[test]
fn a_fixture_file_keeps_the_router_s_text_and_carries_the_licence_identifier() {
    let invocation = Invocation {
        command: "route",
        subject: "org.example.app".to_owned(),
        host: "stated",
        arguments: vec!["route".to_owned(), "--json".to_owned()],
        exit_status: 4,
        stdout: "{\n  \"b\": 1,\n  \"a\": [\n    2\n  ]\n}\n".to_owned(),
        stderr: String::new(),
    };
    let text = document(&[invocation]);
    let head: Vec<&str> = text.lines().take(5).collect();
    assert!(
        head.iter().any(|line| line.contains(LICENCE)),
        "the licence identifier is in the first five lines:\n{text}"
    );
    // The fields keep the router's order: nothing is parsed and rewritten.
    assert!(
        text.contains(
            "      \"stdout\": {\n        \"b\": 1,\n        \"a\": [\n          2\n        ]\n      }\n"
        ),
        "{text}"
    );
    let parsed: Value = serde_json::from_str(&text).unwrap();
    let recorded = &parsed["invocations"][0];
    assert_eq!(recorded["command"], "route");
    assert_eq!(recorded["subject"], "org.example.app");
    assert_eq!(recorded["exit_status"], 4);
    assert_eq!(recorded["stdout"]["a"][0], 2);

    // Output that is empty is recorded as null, not as an empty document.
    let silent = Invocation {
        command: "show",
        subject: "x".to_owned(),
        host: "not used",
        arguments: Vec::new(),
        exit_status: 1,
        stdout: String::new(),
        stderr: "error[TUR-DESKTOP-CATALOGUE-001]: gone\n".to_owned(),
    };
    let parsed: Value = serde_json::from_str(&document(&[silent])).unwrap();
    assert!(parsed["invocations"][0]["stdout"].is_null());
    assert_eq!(
        parsed["invocations"][0]["stderr"],
        "error[TUR-DESKTOP-CATALOGUE-001]: gone\n"
    );

    let vocabulary: Value = serde_json::from_str(&vocabulary()).unwrap();
    assert_eq!(vocabulary["licence"], LICENCE);
    assert_eq!(vocabulary["routes"].as_array().unwrap().len(), 10);
    assert_eq!(vocabulary["ratings"].as_array().unwrap().len(), 5);
    assert_eq!(
        vocabulary["rating_source_kinds"],
        serde_json::json!(["unrated", "external", "tur-test"])
    );
}

/// The application centre's Home view carries one standing sentence "for as
/// long as it is true": "TUR has not tested applications itself yet. Every
/// rating here is what the named source reports." (desktop-experience.md,
/// section 8.1). `tur-centre` shows it only while `tur-catalogue validate
/// --json` counts no rating from TUR's own tests (task D-11). Its recorded
/// answer must be the catalogue's count, so that the pictures of Home say
/// what is true of the seed catalogue.
#[test]
fn the_home_view_reads_whether_tur_has_rated_anything() {
    let recorded: Value = serde_json::from_str(&invoke_validate().stdout).unwrap();
    let directory = repository_root().join("catalogue");
    let catalogue = match Catalogue::load(&directory, Date::today_utc()) {
        Ok(catalogue) => catalogue,
        Err(problems) => {
            let lines: Vec<String> = problems.iter().map(|problem| problem.to_string()).collect();
            panic!("the seed catalogue is not valid:\n{}", lines.join("\n"));
        }
    };
    assert_eq!(
        recorded["statistics"]["rated_by_tur_tests"].as_u64(),
        Some(catalogue.statistics().rated_by_tur_tests as u64),
        "validate --json gives the count the application centre's Home view decides on"
    );
}
