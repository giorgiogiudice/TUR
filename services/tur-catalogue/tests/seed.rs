// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Tests against the real seed catalogue in `catalogue/` and the published
//! schema in `schemas/`. A bad entry, an unsourced rating or a schema that
//! has drifted from the Rust types fails here, and so fails CI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use tur_catalogue::{
    AlternativeKind, AndroidStore, Catalogue, Category, CaveatCode, Date, Edition, EditionKind,
    Entry, EntryKind, HostFacts, Licence, Obtain, Outlook, Packaging, Preferences, Rating,
    RatingSource, Record, Reference, Reporter, Requirement, Route, RouteOption, RouteRequest,
    SCHEMA_VERSION, recommended, route, search,
};

/// The sources a seed rating may name. Each is described, with what it was
/// used for, in `catalogue/README.md` and in the architecture document.
const DOCUMENTED_SOURCES: [&str; 4] = [
    "ProtonDB",
    "Valve Steam Deck compatibility",
    "WineHQ AppDB",
    "AreWeAntiCheatYet",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn seed() -> Catalogue {
    let directory = repository_root().join("catalogue");
    match Catalogue::load(&directory, Date::today_utc()) {
        Ok(catalogue) => catalogue,
        Err(problems) => {
            let lines: Vec<String> = problems.iter().map(|problem| problem.to_string()).collect();
            panic!("the seed catalogue is not valid:\n{}", lines.join("\n"));
        }
    }
}

fn entry<'a>(catalogue: &'a Catalogue, id: &str) -> &'a Entry {
    catalogue
        .get(id)
        .unwrap_or_else(|| panic!("the seed catalogue has no entry {id}"))
}

fn routes(options: &[RouteOption]) -> Vec<Route> {
    options.iter().map(|option| option.route).collect()
}

fn run(entry: &Entry, host: &HostFacts, preferences: &Preferences) -> Vec<RouteOption> {
    route(&RouteRequest::new(entry), host, preferences)
}

fn full_host() -> HostFacts {
    HostFacts {
        distro_containers: true,
        wine: true,
        proton: true,
        android_runtime: true,
        google_play: true,
        windows_guest: true,
        windows_licence: true,
        darling_enabled: true,
        memory_mib: Some(32 * 1024),
        ..HostFacts::default()
    }
}

#[test]
fn the_seed_catalogue_loads_and_validates() {
    let catalogue = seed();
    assert!(
        catalogue.len() >= 25,
        "the seed holds {} applications",
        catalogue.len()
    );
    let categories: BTreeSet<Category> = catalogue
        .entries()
        .iter()
        .map(|entry| entry.category)
        .collect();
    for category in [
        Category::Game,
        Category::Productivity,
        Category::Creative,
        Category::Communication,
        Category::Development,
        Category::Utility,
        Category::Media,
        Category::Finance,
    ] {
        assert!(
            categories.contains(&category),
            "no seed application in category {}",
            category.as_str()
        );
    }
    let kinds: BTreeSet<EditionKind> = catalogue
        .entries()
        .iter()
        .flat_map(|entry| entry.editions.iter().map(|edition| edition.kind))
        .collect();
    assert_eq!(kinds.len(), EditionKind::ALL.len(), "{kinds:?}");
}

#[test]
fn no_seed_record_claims_tur_s_own_test_without_a_test_report() {
    let catalogue = seed();
    // A record that claims a TUR test without naming a report does not
    // load at all (the validator refuses it). One that names a report must
    // name a file that exists in this repository.
    let missing = catalogue.missing_test_reports(&repository_root());
    assert!(missing.is_empty(), "{missing:?}");

    for entry in catalogue.entries() {
        for record in entry
            .editions
            .iter()
            .flat_map(|edition| edition.records.iter())
        {
            match record.reported_by {
                Reporter::Nobody => assert_eq!(
                    record.rating,
                    Rating::Unknown,
                    "{}: a rating without a source",
                    entry.id
                ),
                Reporter::External => {
                    assert!(record.source_name.is_some(), "{}", entry.id);
                    assert!(record.source_url.is_some(), "{}", entry.id);
                    assert!(record.test_report.is_none(), "{}", entry.id);
                }
                Reporter::TurTest => {
                    let report = record
                        .test_report
                        .as_ref()
                        .expect("checked by the validator");
                    assert!(
                        repository_root().join(report).is_file(),
                        "{}: test report {report} does not exist",
                        entry.id
                    );
                }
            }
        }
    }
}

#[test]
fn every_seed_rating_names_a_documented_external_source() {
    let catalogue = seed();
    let readme = std::fs::read_to_string(repository_root().join("catalogue/README.md")).unwrap();
    let document = std::fs::read_to_string(
        repository_root().join("docs/architecture/catalogue-and-routing.md"),
    )
    .unwrap();
    let mut used = BTreeSet::new();
    for entry in catalogue.entries() {
        for record in entry
            .editions
            .iter()
            .flat_map(|edition| edition.records.iter())
        {
            if record.reported_by != Reporter::External {
                continue;
            }
            let name = record.source_name.as_deref().unwrap();
            assert!(
                DOCUMENTED_SOURCES.contains(&name),
                "{}: source \"{name}\" is not one of the documented sources",
                entry.id
            );
            assert!(
                record.source_rating.is_some() || record.rating == Rating::Unknown,
                "{}",
                entry.id
            );
            used.insert(name.to_owned());
        }
    }
    assert!(!used.is_empty(), "the seed holds no sourced rating at all");
    for name in used {
        let short = name.replace(" AppDB", "");
        assert!(
            readme.contains(&short),
            "README.md does not describe {name}"
        );
        assert!(
            document.contains(&short),
            "catalogue-and-routing.md does not list {name}"
        );
    }
}

#[test]
fn the_seed_covers_the_applications_with_no_good_route() {
    let catalogue = seed();
    let stock = HostFacts::default();
    let nothing_pinned = Preferences::default();

    // A game with kernel anti-cheat: no route, and dual boot is stated, on a
    // stock machine and on one with every runtime installed.
    for id in ["com.riotgames.valorant", "com.ea.apexlegends"] {
        for host in [stock.clone(), full_host()] {
            let options = run(entry(&catalogue, id), &host, &nothing_pinned);
            assert_eq!(routes(&options), [Route::NoRoute, Route::Proton], "{id}");
            assert!(options[0].recommended);
            let alternative = options[0].alternative.as_ref().unwrap();
            assert_eq!(alternative.kind, AlternativeKind::DualBoot, "{id}");
            assert!(
                options[0].reason.contains("Dual boot is the only way"),
                "{id}"
            );
            assert_eq!(options[1].outlook, Outlook::Blocked, "{id}");
        }
    }

    // The banking class and a wallet: Play Integrity, so no route.
    for id in ["tur.class.android-banking", "com.google.wallet"] {
        let options = run(entry(&catalogue, id), &full_host(), &nothing_pinned);
        assert_eq!(
            routes(&options),
            [Route::NoRoute, Route::AndroidRuntime],
            "{id}"
        );
        let alternative = options[0].alternative.as_ref().unwrap();
        assert_eq!(
            alternative.kind,
            AlternativeKind::CertifiedAndroidDevice,
            "{id}"
        );
        let caveat = options[1].caveat_for(Requirement::PlayIntegrity).unwrap();
        assert!(caveat.text.contains("will refuse to run"), "{id}");
    }
    assert_eq!(
        entry(&catalogue, "tur.class.android-banking").entry_kind,
        EntryKind::Class
    );

    // A Mac-only creative application: no route on a stock machine; with
    // Darling enabled, one experimental and doubtful route.
    let final_cut = entry(&catalogue, "com.apple.finalcutpro");
    assert!(
        final_cut
            .editions
            .iter()
            .all(|edition| edition.kind == EditionKind::Macos)
    );
    let options = run(final_cut, &stock, &nothing_pinned);
    assert_eq!(routes(&options), [Route::NoRoute]);
    assert_eq!(
        options[0].alternative.as_ref().unwrap().kind,
        AlternativeKind::Mac
    );
    let options = run(final_cut, &full_host(), &nothing_pinned);
    assert_eq!(routes(&options), [Route::Darling]);
    assert_eq!(options[0].outlook, Outlook::Doubtful);
    assert!(
        options[0]
            .caveats
            .iter()
            .any(|caveat| caveat.code == CaveatCode::Experimental)
    );
}

#[test]
fn seed_applications_route_as_the_ladder_says() {
    let catalogue = seed();
    let stock = HostFacts::default();
    let nothing_pinned = Preferences::default();
    let no_virtual_machine = Preferences {
        never_use_virtual_machine: true,
        ..Preferences::default()
    };

    // A native Linux edition wins.
    for id in [
        "org.mozilla.firefox",
        "org.libreoffice.libreoffice",
        "org.gimp.gimp",
    ] {
        let options = run(entry(&catalogue, id), &full_host(), &nothing_pinned);
        assert_eq!(options[0].route, Route::Flatpak, "{id}");
        assert!(options[0].recommended, "{id}");
        assert!(!routes(&options).contains(&Route::WindowsGuest), "{id}");
    }

    // A Windows game: Proton, with the rating and who reported it.
    let options = run(
        entry(&catalogue, "com.bandainamco.eldenring"),
        &stock,
        &nothing_pinned,
    );
    assert_eq!(routes(&options), [Route::Proton]);
    assert_eq!(options[0].rating, Rating::Usable);
    match &options[0].rating_source {
        RatingSource::External {
            name,
            source_rating,
            ..
        } => {
            assert_eq!(name, "ProtonDB");
            assert_eq!(source_rating.as_deref(), Some("gold"));
        }
        other => panic!("the rating must be attributed to its source, not {other:?}"),
    }
    assert!(!options[0].prerequisites.is_empty());

    // A Windows productivity application that Wine is reported not to run:
    // Wine first, then the guest, and the web edition because neither is a
    // sure thing without a virtual machine.
    let office = entry(&catalogue, "com.microsoft.office");
    let options = run(office, &full_host(), &nothing_pinned);
    assert_eq!(
        routes(&options),
        [Route::Wine, Route::WindowsGuest, Route::Web]
    );
    assert_eq!(options[0].rating, Rating::Broken);
    assert_eq!(recommended(&options).route, Route::WindowsGuest);
    let options = run(office, &full_host(), &no_virtual_machine);
    assert_eq!(routes(&options), [Route::Wine, Route::Web]);
    assert_eq!(recommended(&options).route, Route::Web);
    assert!(
        options[0]
            .caveats
            .iter()
            .any(|caveat| caveat.code == CaveatCode::GuestNotOffered)
    );

    // A Windows application that Wine is reported to run: no guest.
    let options = run(
        entry(&catalogue, "com.adobe.photoshop"),
        &stock,
        &nothing_pinned,
    );
    assert_eq!(routes(&options), [Route::Wine]);
    assert_eq!(options[0].rating, Rating::Usable);

    // The web edition is the publisher's primary form: listed first, the
    // Android edition after it with its copy-protection caveat.
    let options = run(
        entry(&catalogue, "com.netflix.netflix"),
        &full_host(),
        &nothing_pinned,
    );
    assert_eq!(routes(&options), [Route::Web, Route::AndroidRuntime]);
    assert!(options[0].recommended);
    assert!(
        options[1]
            .caveat_for(Requirement::HardwareAttestedDrm)
            .is_some()
    );

    // A package of another distribution runs in a container, after the
    // Flatpak and the publisher's own build.
    let options = run(
        entry(&catalogue, "org.signal.signal"),
        &full_host(),
        &nothing_pinned,
    );
    assert_eq!(
        routes(&options)[..3],
        [Route::Flatpak, Route::Native, Route::DistroContainer]
    );
}

#[test]
fn every_seed_application_gets_an_answer_on_every_sample_machine() {
    let catalogue = seed();
    let hosts = [
        HostFacts::default(),
        full_host(),
        HostFacts {
            arch: tur_catalogue::Arch::Aarch64,
            ..full_host()
        },
    ];
    let preferences = [
        Preferences::default(),
        Preferences {
            never_use_virtual_machine: true,
            prefer_open_source: true,
            ..Preferences::default()
        },
    ];
    for entry in catalogue.entries() {
        for host in &hosts {
            for preferences in &preferences {
                for offer_windows_guest in [false, true] {
                    let request = RouteRequest {
                        entry,
                        offer_windows_guest,
                    };
                    let options = route(&request, host, preferences);
                    assert!(!options.is_empty(), "{}", entry.id);
                    let chosen: Vec<&RouteOption> =
                        options.iter().filter(|option| option.recommended).collect();
                    assert_eq!(chosen.len(), 1, "{}", entry.id);
                    for option in &options {
                        assert!(!option.reason.is_empty(), "{}", entry.id);
                        if preferences.never_use_virtual_machine {
                            assert_ne!(option.route, Route::WindowsGuest, "{}", entry.id);
                        }
                        // No rating is presented as TUR's own: TUR has
                        // tested nothing yet.
                        assert!(
                            !matches!(option.rating_source, RatingSource::TurTest { .. }),
                            "{}",
                            entry.id
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn seed_applications_are_found_by_name_alias_and_typo() {
    let catalogue = seed();
    for (query, id) in [
        ("elden ring", "com.bandainamco.eldenring"),
        ("ELDEN-RING", "com.bandainamco.eldenring"),
        ("photoshop", "com.adobe.photoshop"),
        ("fotoshop", "com.adobe.photoshop"),
        ("vs code", "com.microsoft.vscode"),
        ("visual studio", "com.microsoft.visualstudio"),
        ("libre office", "org.libreoffice.libreoffice"),
        ("cs2", "com.valvesoftware.cs2"),
        ("bank app", "tur.class.android-banking"),
        ("7zip", "org.7-zip.7zip"),
        ("firefx", "org.mozilla.firefox"),
        ("office 365", "com.microsoft.office"),
    ] {
        let hits = search(catalogue.entries(), query);
        assert!(!hits.is_empty(), "\"{query}\" finds nothing");
        assert_eq!(hits[0].entry.id, id, "\"{query}\"");
    }
    assert!(search(catalogue.entries(), "qqqqzzzzqqqq").is_empty());
}

// ---------------------------------------------------------------------------
// The published schema and the Rust types describe the same format.
// ---------------------------------------------------------------------------

fn schema() -> Value {
    let path = repository_root().join("schemas/app-catalogue-entry.schema.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn names(value: &Value) -> Vec<String> {
    let mut names: Vec<String> = value
        .as_array()
        .unwrap_or_else(|| panic!("not a list: {value}"))
        .iter()
        .map(|name| name.as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

fn sorted(values: &[&str]) -> Vec<String> {
    let mut values: Vec<String> = values.iter().map(|value| (*value).to_owned()).collect();
    values.sort();
    values
}

fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("not an object: {value}"))
        .keys()
        .cloned()
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

#[test]
fn the_schema_and_the_rust_types_name_the_same_fields() {
    let schema = schema();
    let mut edition = Edition::new(EditionKind::Windows);
    edition.records.push(Record {
        route: Route::Wine,
        rating: Rating::Unknown,
        reported_by: Reporter::Nobody,
        source_name: None,
        source_url: None,
        source_rating: None,
        test_report: None,
        version_tested: None,
        date: "2026-11-12".to_owned(),
        notes: None,
    });
    edition.references.push(Reference {
        title: "A page".to_owned(),
        url: "https://example.org/".to_owned(),
        checked: "2026-11-12".to_owned(),
    });
    let entry = Entry {
        schema_version: SCHEMA_VERSION,
        id: "org.example.app".to_owned(),
        entry_kind: EntryKind::Application,
        name: "Example".to_owned(),
        aliases: Vec::new(),
        publisher: "Example".to_owned(),
        category: Category::Utility,
        summary: None,
        homepage: None,
        editions: vec![edition],
    };
    let written = serde_json::to_value(&entry).unwrap();

    assert_eq!(keys(&written), keys(&schema["properties"]));
    let edition = &written["editions"][0];
    assert_eq!(
        keys(edition),
        keys(&schema["$defs"]["edition"]["properties"])
    );
    assert_eq!(
        keys(&edition["records"][0]),
        keys(&schema["$defs"]["record"]["properties"])
    );
    assert_eq!(
        keys(&edition["references"][0]),
        keys(&schema["$defs"]["reference"]["properties"])
    );

    // Unknown fields are refused at every level, in the schema as in Rust.
    for object in [
        &schema,
        &schema["$defs"]["edition"],
        &schema["$defs"]["record"],
        &schema["$defs"]["reference"],
    ] {
        assert_eq!(object["additionalProperties"], Value::Bool(false));
    }
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        SCHEMA_VERSION
    );
    assert_eq!(
        names(&schema["required"]),
        sorted(&[
            "schema_version",
            "id",
            "name",
            "publisher",
            "category",
            "editions"
        ])
    );
    assert_eq!(
        names(&schema["$defs"]["edition"]["required"]),
        sorted(&["kind"])
    );
    assert_eq!(
        names(&schema["$defs"]["record"]["required"]),
        sorted(&["route", "rating", "reported_by", "date"])
    );
    assert_eq!(
        names(&schema["$defs"]["reference"]["required"]),
        sorted(&["title", "url", "checked"])
    );
}

#[test]
fn the_schema_and_the_rust_types_list_the_same_values() {
    let schema = schema();
    let edition = &schema["$defs"]["edition"]["properties"];
    let record = &schema["$defs"]["record"]["properties"];

    let expected: BTreeMap<&str, (Vec<String>, Vec<String>)> = BTreeMap::from([
        (
            "category",
            (
                names(&schema["properties"]["category"]["enum"]),
                sorted(&Category::ALL.map(Category::as_str)),
            ),
        ),
        (
            "entry_kind",
            (
                names(&schema["properties"]["entry_kind"]["enum"]),
                sorted(&["application", "class"]),
            ),
        ),
        (
            "edition kind",
            (
                names(&edition["kind"]["enum"]),
                sorted(&EditionKind::ALL.map(EditionKind::as_str)),
            ),
        ),
        (
            "obtain",
            (
                names(&edition["obtain"]["enum"]),
                sorted(&Obtain::ALL.map(Obtain::as_str)),
            ),
        ),
        (
            "stores",
            (
                names(&edition["stores"]["items"]["enum"]),
                sorted(&AndroidStore::ALL.map(AndroidStore::as_str)),
            ),
        ),
        (
            "licence",
            (
                names(&edition["licence"]["enum"]),
                sorted(&Licence::ALL.map(Licence::as_str)),
            ),
        ),
        (
            "packaging",
            (
                names(&edition["packaging"]["enum"]),
                sorted(&Packaging::ALL.map(Packaging::as_str)),
            ),
        ),
        (
            "requirement",
            (
                names(&schema["$defs"]["requirement"]["enum"]),
                sorted(&Requirement::ALL.map(Requirement::as_str)),
            ),
        ),
        (
            "route",
            (
                names(&schema["$defs"]["route"]["enum"]),
                sorted(&Route::RUNNABLE.map(Route::as_str)),
            ),
        ),
        (
            "rating",
            (
                names(&schema["$defs"]["rating"]["enum"]),
                sorted(&Rating::ALL.map(Rating::as_str)),
            ),
        ),
        (
            "reported_by",
            (
                names(&record["reported_by"]["enum"]),
                sorted(&Reporter::ALL.map(Reporter::as_str)),
            ),
        ),
    ]);
    for (what, (in_schema, in_rust)) in expected {
        assert_eq!(in_schema, in_rust, "{what}");
    }
}
