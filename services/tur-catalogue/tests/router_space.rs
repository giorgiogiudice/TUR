// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The routing invariants, asserted over an enumerated input space.
//!
//! The router's inputs are small enough to enumerate: edition kinds, the
//! eight requirement flags, a rating, host facts and preferences. Each test
//! below walks one slice of that space completely and applies every
//! invariant to every result:
//!
//! | Invariant | Checked by |
//! |---|---|
//! | The result is never empty, and says "no route" exactly when nothing can work | [`never_empty_and_no_route_is_explicit`] |
//! | A guest never precedes a compatibility layer for the same edition | [`guest_never_precedes_its_layer`] |
//! | A forbidden route never appears | [`forbidden_routes_never_appear`] |
//! | Every option with a blocking requirement carries its caveat | [`requirements_carry_their_caveats`] |
//! | Exactly one option is recommended, and never a blocked one | [`one_sound_recommendation`] |
//! | Blocked options come last; no option is listed twice | [`blocked_last_and_no_duplicates`] |
//! | A native Linux edition leads unless its record is adverse | [`linux_leads_unless_adverse`] |
//! | The guest is offered only for a stated reason | [`guest_only_for_a_reason`] |
//!
//! The `*_is_detected` tests show that each of the four invariants the
//! specification names fails on a hand-made bad result, so a green run is
//! not the result of a check that cannot fail.

use std::collections::BTreeMap;
use std::sync::Mutex;

use tur_catalogue::{
    ALTERNATIVE_TEXT_IDS, AndroidStore, Arch, CAVEAT_TEXT_IDS, Category, CaveatCode, Edition, EditionKind, Entry, EntryKind, GpuClass,
    HostFacts, Licence, Obtain, Outlook, Preferences, Rating, RatingSource, ReasonCode, ReasonPart,
    Record, Reporter, Requirement, Route, RouteOption, RouteRequest, SCHEMA_VERSION, Severity,
    requirement_severity, route,
};

const APP_ID: &str = "org.example.app";

/// One point of the input space.
struct Case<'a> {
    entry: &'a Entry,
    offer_windows_guest: bool,
    host: &'a HostFacts,
    preferences: &'a Preferences,
}

impl Case<'_> {
    fn describe(&self) -> String {
        let editions: Vec<String> = self
            .entry
            .editions
            .iter()
            .map(|edition| {
                let requirements: Vec<&str> = edition
                    .requirements
                    .iter()
                    .map(|requirement| requirement.as_str())
                    .collect();
                let ratings: Vec<String> = edition
                    .records
                    .iter()
                    .map(|record| format!("{}={}", record.route.as_str(), record.rating.as_str()))
                    .collect();
                format!(
                    "{}[{}]{{{}}}",
                    edition.kind.as_str(),
                    requirements.join(","),
                    ratings.join(",")
                )
            })
            .collect();
        format!(
            "category={} editions={} offer_guest={} host={:?} preferences={:?}",
            self.entry.category.as_str(),
            editions.join(" "),
            self.offer_windows_guest,
            self.host,
            self.preferences
        )
    }
}

fn entry(category: Category, editions: Vec<Edition>) -> Entry {
    Entry {
        schema_version: SCHEMA_VERSION,
        id: APP_ID.to_owned(),
        entry_kind: EntryKind::Application,
        name: "Example".to_owned(),
        aliases: Vec::new(),
        publisher: "Example Project".to_owned(),
        category,
        summary: None,
        homepage: None,
        editions,
    }
}

fn edition(kind: EditionKind) -> Edition {
    let mut edition = Edition::new(kind);
    match kind {
        EditionKind::Flatpak => edition.flatpak_id = Some("org.example.App".to_owned()),
        EditionKind::LinuxNative => {
            edition.obtain = Some(Obtain::FedoraPackage);
            edition.package = Some("example".to_owned());
        }
        EditionKind::Windows => {
            edition.obtain = Some(Obtain::PublisherPage);
            edition.download_page = Some("https://example.org/download".to_owned());
        }
        EditionKind::Android => {
            edition.package = Some("org.example.app".to_owned());
            edition.stores = vec![AndroidStore::FDroid];
        }
        EditionKind::Web => edition.url = Some("https://app.example.org/".to_owned()),
        EditionKind::Macos => {
            edition.obtain = Some(Obtain::PublisherPage);
            edition.download_page = Some("https://example.org/mac".to_owned());
        }
    }
    edition
}

fn debian() -> Edition {
    let mut edition = Edition::new(EditionKind::LinuxNative);
    edition.obtain = Some(Obtain::OtherDistributionPackage);
    edition.distribution = Some("debian".to_owned());
    edition.package = Some("example".to_owned());
    edition
}

fn record(route: Route, rating: Rating) -> Record {
    Record {
        route,
        rating,
        reported_by: Reporter::External,
        source_name: Some("Example DB".to_owned()),
        source_url: Some("https://db.example.org/app".to_owned()),
        source_rating: Some("Tier".to_owned()),
        test_report: None,
        version_tested: None,
        date: "2026-11-12".to_owned(),
        notes: None,
    }
}

/// The requirement set whose bit `n` selects `Requirement::ALL[n]`.
fn requirement_set(mask: u32) -> Vec<Requirement> {
    Requirement::ALL
        .into_iter()
        .enumerate()
        .filter(|(bit, _)| mask & (1 << bit) != 0)
        .map(|(_, requirement)| requirement)
        .collect()
}

fn full_host() -> HostFacts {
    HostFacts {
        arch: Arch::X86_64,
        flatpak: true,
        distro_containers: true,
        wine: true,
        proton: true,
        android_runtime: true,
        arm_translation: false,
        google_play: true,
        windows_guest: true,
        windows_licence: true,
        darling_enabled: true,
        gpu: GpuClass::Discrete,
        memory_mib: Some(32 * 1024),
    }
}

fn pinned(route: Route) -> Preferences {
    let mut preferences = Preferences::default();
    preferences.pinned_routes.insert(APP_ID.to_owned(), route);
    preferences
}

fn position(options: &[RouteOption], wanted: impl Fn(&RouteOption) -> bool) -> Option<usize> {
    options.iter().position(wanted)
}

// ---------------------------------------------------------------------------
// The invariants. Each takes the case and the router's answer and panics,
// with the case described, when the invariant does not hold.
// ---------------------------------------------------------------------------

fn never_empty_and_no_route_is_explicit(case: &Case<'_>, options: &[RouteOption]) {
    assert!(!options.is_empty(), "empty result for {}", case.describe());
    let statements = options
        .iter()
        .filter(|option| option.route == Route::NoRoute)
        .count();
    let anything_usable = options
        .iter()
        .any(|option| option.route != Route::NoRoute && option.outlook != Outlook::Blocked);
    assert_eq!(
        statements,
        usize::from(!anything_usable),
        "\"no route\" must be stated exactly when nothing can work: {}",
        case.describe()
    );
    if !anything_usable {
        let statement = &options[0];
        assert_eq!(statement.route, Route::NoRoute, "{}", case.describe());
        assert!(statement.reason.len() > 20, "{}", case.describe());
        let alternative = statement.alternative.as_ref().expect("an alternative");
        assert!(!alternative.text.is_empty(), "{}", case.describe());
        // Every blocked route's blocker is repeated in the statement.
        for option in &options[1..] {
            let blocker = option
                .caveats
                .iter()
                .find(|caveat| caveat.severity == Severity::Blocker)
                .expect("a blocked option has a blocker");
            assert!(
                statement.reason.contains(&blocker.text),
                "the reason omits a blocker: {}",
                case.describe()
            );
        }
    }
    for option in options {
        assert!(!option.reason.is_empty(), "{}", case.describe());
        for caveat in &option.caveats {
            assert!(!caveat.text.is_empty(), "{}", case.describe());
        }
        for prerequisite in &option.prerequisites {
            assert!(!prerequisite.text.is_empty(), "{}", case.describe());
        }
        if option.route != Route::NoRoute {
            assert!(option.alternative.is_none(), "{}", case.describe());
            let index = option.edition_index.expect("a route runs an edition");
            let edition = &case.entry.editions[index];
            assert_eq!(option.edition_kind, Some(edition.kind));
            assert!(
                edition.routes().contains(&option.route),
                "{} cannot run a {} edition: {}",
                option.route.as_str(),
                edition.kind.as_str(),
                case.describe()
            );
        }
    }
}

fn guest_never_precedes_its_layer(case: &Case<'_>, options: &[RouteOption]) {
    for (guest_position, guest) in options.iter().enumerate() {
        if guest.route != Route::WindowsGuest {
            continue;
        }
        let layer_position = position(options, |option| {
            option.route.is_compatibility_layer() && option.edition_index == guest.edition_index
        });
        match layer_position {
            Some(layer_position) => assert!(
                layer_position < guest_position,
                "the guest precedes the compatibility layer: {}",
                case.describe()
            ),
            None => panic!(
                "the guest is listed without its compatibility layer: {}",
                case.describe()
            ),
        }
    }
}

fn forbidden_routes_never_appear(case: &Case<'_>, options: &[RouteOption]) {
    let game = case.entry.category == Category::Game;
    for option in options {
        let forbidden = match option.route {
            // Forbidden by preference, and never for a game.
            Route::WindowsGuest => case.preferences.never_use_virtual_machine || game,
            // Only when the user has enabled it.
            Route::Darling => !case.host.darling_enabled,
            // Proton for games, Wine otherwise.
            Route::Proton => !game,
            Route::Wine => game,
            _ => false,
        };
        assert!(
            !forbidden,
            "{} is forbidden and appears: {}",
            option.route.as_str(),
            case.describe()
        );
    }
}

fn requirements_carry_their_caveats(case: &Case<'_>, options: &[RouteOption]) {
    for option in options {
        let Some(index) = option.edition_index else {
            continue;
        };
        let edition = &case.entry.editions[index];
        for requirement in &edition.requirements {
            let expected = requirement_severity(*requirement, option.route, case.host);
            let caveat = option.caveat_for(*requirement).unwrap_or_else(|| {
                panic!(
                    "{} has no caveat for {}: {}",
                    option.route.as_str(),
                    requirement.as_str(),
                    case.describe()
                )
            });
            assert_eq!(caveat.code, CaveatCode::Requirement);
            assert_eq!(
                caveat.severity,
                expected,
                "{} on {}: {}",
                requirement.as_str(),
                option.route.as_str(),
                case.describe()
            );
        }
        // A blocker and a blocked outlook always go together.
        let has_blocker = option
            .caveats
            .iter()
            .any(|caveat| caveat.severity == Severity::Blocker);
        assert_eq!(
            option.outlook == Outlook::Blocked,
            has_blocker,
            "{}",
            case.describe()
        );
    }
}

fn one_sound_recommendation(case: &Case<'_>, options: &[RouteOption]) {
    let recommended: Vec<&RouteOption> =
        options.iter().filter(|option| option.recommended).collect();
    assert_eq!(recommended.len(), 1, "{}", case.describe());
    let chosen = recommended[0];
    assert!(
        chosen.route == Route::NoRoute || chosen.outlook != Outlook::Blocked,
        "a blocked route is recommended: {}",
        case.describe()
    );

    let pinned: Vec<&RouteOption> = options.iter().filter(|option| option.pinned).collect();
    assert!(pinned.len() <= 1, "{}", case.describe());
    let pin = case.preferences.pinned_route(&case.entry.id);
    if let Some(pinned) = pinned.first() {
        assert_eq!(Some(pinned.route), pin, "{}", case.describe());
        assert!(pinned.recommended, "{}", case.describe());
    } else {
        // Not pinned: the first clear option, else the first doubtful one.
        let first_clear = position(options, |option| option.outlook == Outlook::Clear);
        let first_doubtful = position(options, |option| option.outlook == Outlook::Doubtful);
        let expected = first_clear.or(first_doubtful).unwrap_or(0);
        assert!(options[expected].recommended, "{}", case.describe());
        // A pin that is not honoured is explained.
        if pin.is_some() {
            assert!(
                options[0]
                    .caveats
                    .iter()
                    .any(|caveat| caveat.code == CaveatCode::PinUnavailable),
                "an unused pin is not explained: {}",
                case.describe()
            );
        }
    }
}

fn blocked_last_and_no_duplicates(case: &Case<'_>, options: &[RouteOption]) {
    let routes: Vec<&RouteOption> = options
        .iter()
        .filter(|option| option.route != Route::NoRoute)
        .collect();
    let first_blocked = routes
        .iter()
        .position(|option| option.outlook == Outlook::Blocked)
        .unwrap_or(routes.len());
    assert!(
        routes[first_blocked..]
            .iter()
            .all(|option| option.outlook == Outlook::Blocked),
        "a usable route follows a blocked one: {}",
        case.describe()
    );
    for (index, option) in routes.iter().enumerate() {
        let repeated = routes[index + 1..].iter().any(|other| {
            other.route == option.route && other.edition_index == option.edition_index
        });
        assert!(!repeated, "an option is listed twice: {}", case.describe());
    }
}

fn linux_leads_unless_adverse(case: &Case<'_>, options: &[RouteOption]) {
    let sound_linux = options.iter().any(|option| {
        matches!(option.route, Route::Flatpak | Route::Native)
            && option.outlook != Outlook::Blocked
            && !option.rating.is_adverse()
    });
    if sound_linux {
        assert!(
            matches!(options[0].route, Route::Flatpak | Route::Native),
            "a native Linux edition does not lead: {}",
            case.describe()
        );
    }
    // A Linux edition is only ever listed after a non-Linux route that is
    // not blocked when its own record is adverse and the other's supports.
    for (index, option) in options.iter().enumerate() {
        let leading_linux = matches!(option.route, Route::Flatpak | Route::Native);
        if !leading_linux || option.outlook == Outlook::Blocked {
            continue;
        }
        let overtaken_by = options[..index].iter().find(|other| {
            !other.route.is_linux() && other.route != Route::NoRoute && !is_primary_web(case, other)
        });
        if let Some(other) = overtaken_by {
            let justified = option.rating.is_adverse()
                && options[..index]
                    .iter()
                    .any(|better| !better.route.is_linux() && better.rating.is_supporting());
            assert!(
                justified,
                "{} overtakes a Linux edition without an adverse record: {}",
                other.route.as_str(),
                case.describe()
            );
        }
    }
}

fn is_primary(case: &Case<'_>, option: &RouteOption) -> bool {
    option
        .edition_index
        .is_some_and(|index| case.entry.editions[index].primary)
}

fn is_primary_web(case: &Case<'_>, option: &RouteOption) -> bool {
    option.route == Route::Web && is_primary(case, option)
}

fn is_secondary_web(case: &Case<'_>, option: &RouteOption) -> bool {
    option.route == Route::Web && !is_primary(case, option)
}

fn guest_only_for_a_reason(case: &Case<'_>, options: &[RouteOption]) {
    let asked = case.offer_windows_guest
        || case.preferences.pinned_route(&case.entry.id) == Some(Route::WindowsGuest);
    for guest in options {
        if guest.route != Route::WindowsGuest {
            continue;
        }
        let layer = options
            .iter()
            .find(|option| {
                option.route.is_compatibility_layer() && option.edition_index == guest.edition_index
            })
            .expect("checked by guest_never_precedes_its_layer");
        let known_obstacle = layer.caveats.iter().any(|caveat| {
            caveat.severity == Severity::Obstacle
                && caveat.requirement != Some(Requirement::HeavyGpu)
        });
        let reason = layer.rating.is_adverse()
            || (layer.rating == Rating::Unknown && known_obstacle)
            || asked;
        assert!(
            reason,
            "the guest is offered without a reason: {}",
            case.describe()
        );
        assert_ne!(layer.outlook, Outlook::Blocked, "{}", case.describe());
    }
    // What is lost is stated when the guest is withheld by preference.
    for option in options {
        let told = option
            .caveats
            .iter()
            .any(|caveat| caveat.code == CaveatCode::GuestNotOffered);
        if told {
            assert!(
                case.preferences.never_use_virtual_machine,
                "{}",
                case.describe()
            );
            assert!(option.route.is_compatibility_layer(), "{}", case.describe());
        }
    }
}

/// Routes one case and applies every invariant to the answer.
/// Every sentence seen anywhere in the space, by its identifier and its
/// parameters: one identifier with one set of parameters must always be
/// the same sentence, or a translation by identifier would say something
/// the English does not.
static SENTENCES: Mutex<BTreeMap<(String, String), String>> = Mutex::new(BTreeMap::new());

fn same_sentence_every_time(kind: &str, id: &str, params: &str, text: &str) {
    let mut seen = SENTENCES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let key = (format!("{kind}:{id}"), params.to_owned());
    match seen.get(&key) {
        Some(known) => assert_eq!(
            known, text,
            "{kind} {id} with {params} was written as two different sentences"
        ),
        None => {
            seen.insert(key, text.to_owned());
        }
    }
}

/// The reason, every caveat and the alternative carry stable identifiers
/// (issue #41): every identifier is one the router lists, the reason's
/// first part is its code, and an identifier with its parameters is always
/// the same English sentence.
fn sentences_carry_stable_identifiers(case: &Case<'_>, options: &[RouteOption]) {
    for option in options {
        assert!(!option.reason_parts.is_empty(), "{}", case.describe());
        assert_eq!(
            option.reason_code,
            option.reason_parts[0].code,
            "{}",
            case.describe()
        );
        let codes: Vec<&str> = option
            .reason_parts
            .iter()
            .map(|part| part.code.as_str())
            .collect();
        // The no-route reason repeats each blocking caveat's sentence, so
        // the caveats' parameters are part of what decides its text.
        let caveat_params: Vec<(&str, &tur_catalogue::Params)> = option
            .caveats
            .iter()
            .map(|caveat| (caveat.text_id, &caveat.params))
            .collect();
        let params = format!(
            "{}{}",
            serde_json::to_string(&option.reason_parts).unwrap_or_default(),
            serde_json::to_string(&caveat_params).unwrap_or_default()
        );
        same_sentence_every_time("reason", &codes.join("+"), &params, &option.reason);
        for caveat in &option.caveats {
            assert!(
                CAVEAT_TEXT_IDS.contains(&caveat.text_id),
                "{} is not a listed caveat identifier",
                caveat.text_id
            );
            let params = serde_json::to_string(&caveat.params).unwrap_or_default();
            same_sentence_every_time("caveat", caveat.text_id, &params, &caveat.text);
        }
        if let Some(alternative) = &option.alternative {
            assert!(ALTERNATIVE_TEXT_IDS.contains(&alternative.text_id));
            same_sentence_every_time("alternative", alternative.text_id, "", &alternative.text);
        }
        // A blocked route named in the no-route reason names its caveat,
        // and that caveat is among the statement's.
        for part in &option.reason_parts {
            if part.code == ReasonCode::RouteBlocked {
                let caveat = part.params.get("caveat").expect("a caveat parameter");
                assert!(
                    option.caveats.iter().any(|known| known.text_id == caveat),
                    "{}",
                    case.describe()
                );
            }
        }
    }
}

fn check(case: &Case<'_>) -> Vec<RouteOption> {
    let request = RouteRequest {
        entry: case.entry,
        offer_windows_guest: case.offer_windows_guest,
    };
    let options = route(&request, case.host, case.preferences);
    sentences_carry_stable_identifiers(case, &options);
    never_empty_and_no_route_is_explicit(case, &options);
    guest_never_precedes_its_layer(case, &options);
    forbidden_routes_never_appear(case, &options);
    requirements_carry_their_caveats(case, &options);
    one_sound_recommendation(case, &options);
    blocked_last_and_no_duplicates(case, &options);
    linux_leads_unless_adverse(case, &options);
    guest_only_for_a_reason(case, &options);
    options
}

// ---------------------------------------------------------------------------
// The enumerations.
// ---------------------------------------------------------------------------

const LAYER_RATINGS: [Option<Rating>; 6] = [
    None,
    Some(Rating::Unknown),
    Some(Rating::Good),
    Some(Rating::Usable),
    Some(Rating::Poor),
    Some(Rating::Broken),
];

/// A Windows edition: both categories, every requirement set, every rating
/// of the compatibility layer, 24 hosts and 12 preference sets.
#[test]
fn windows_edition_space() {
    let mut hosts = Vec::new();
    for arch in [Arch::X86_64, Arch::Aarch64] {
        for installed in [false, true] {
            for memory_mib in [None, Some(8 * 1024), Some(32 * 1024)] {
                for gpu in [GpuClass::None, GpuClass::Discrete] {
                    hosts.push(HostFacts {
                        arch,
                        wine: installed,
                        proton: installed,
                        windows_guest: installed,
                        windows_licence: installed,
                        gpu,
                        memory_mib,
                        ..HostFacts::default()
                    });
                }
            }
        }
    }
    let mut cases = 0u64;
    let mut guests = 0u64;
    let mut statements = 0u64;
    for category in [Category::Game, Category::Productivity] {
        let layer = if category == Category::Game {
            Route::Proton
        } else {
            Route::Wine
        };
        let mut variants: Vec<(Preferences, bool)> = Vec::new();
        for never_use_virtual_machine in [false, true] {
            for pin in [None, Some(Route::WindowsGuest), Some(layer)] {
                for offer in [false, true] {
                    let mut preferences = match pin {
                        Some(route) => pinned(route),
                        None => Preferences::default(),
                    };
                    preferences.never_use_virtual_machine = never_use_virtual_machine;
                    variants.push((preferences, offer));
                }
            }
        }
        for mask in 0..256u32 {
            for rating in LAYER_RATINGS {
                let mut windows = edition(EditionKind::Windows);
                windows.requirements = requirement_set(mask);
                if let Some(rating) = rating {
                    windows.records.push(record(layer, rating));
                }
                let entry = entry(category, vec![windows]);
                for host in &hosts {
                    for (preferences, offer) in &variants {
                        let case = Case {
                            entry: &entry,
                            offer_windows_guest: *offer,
                            host,
                            preferences,
                        };
                        let options = check(&case);
                        cases += 1;
                        guests += u64::from(
                            options
                                .iter()
                                .any(|option| option.route == Route::WindowsGuest),
                        );
                        statements += u64::from(options[0].route == Route::NoRoute);

                        // Kernel anti-cheat: no route, and dual boot stated.
                        if mask & 1 != 0 {
                            assert_eq!(options[0].route, Route::NoRoute, "{}", case.describe());
                            assert_eq!(options.len(), 2, "{}", case.describe());
                            assert_eq!(options[1].route, layer, "{}", case.describe());
                            if host.arch == Arch::X86_64 {
                                let alternative = options[0].alternative.as_ref().unwrap();
                                assert_eq!(
                                    alternative.kind,
                                    tur_catalogue::AlternativeKind::DualBoot,
                                    "{}",
                                    case.describe()
                                );
                                assert!(options[0].reason.contains("Dual boot is the only way"));
                            }
                        }
                        // The compatibility layer is always listed.
                        assert!(
                            options.iter().any(|option| option.route == layer),
                            "{}",
                            case.describe()
                        );
                    }
                }
            }
        }
    }
    assert_eq!(Requirement::ALL[0], Requirement::KernelAntiCheat);
    assert_eq!(cases, 2 * 256 * 6 * 24 * 12);
    // The space is not degenerate: it holds guests and no-route results.
    assert!(guests > 10_000, "{guests}");
    assert!(statements > 10_000, "{statements}");
}

/// An Android edition: every requirement set, both stores, 64 hosts.
#[test]
fn android_edition_space() {
    let mut hosts = Vec::new();
    for arch in [Arch::X86_64, Arch::Aarch64] {
        for android_runtime in [false, true] {
            for arm_translation in [false, true] {
                for google_play in [false, true] {
                    for gpu in GpuClass::ALL {
                        hosts.push(HostFacts {
                            arch,
                            android_runtime,
                            arm_translation,
                            google_play,
                            gpu,
                            ..HostFacts::default()
                        });
                    }
                }
            }
        }
    }
    let variants = [
        Preferences::default(),
        pinned(Route::AndroidRuntime),
        Preferences {
            never_use_virtual_machine: true,
            ..Preferences::default()
        },
    ];
    let mut cases = 0u64;
    for mask in 0..256u32 {
        for store in [AndroidStore::FDroid, AndroidStore::GooglePlay] {
            for rating in [None, Some(Rating::Good), Some(Rating::Broken)] {
                let mut android = edition(EditionKind::Android);
                android.requirements = requirement_set(mask);
                android.stores = vec![store];
                if let Some(rating) = rating {
                    android.records.push(record(Route::AndroidRuntime, rating));
                }
                let entry = entry(Category::Utility, vec![android]);
                for host in &hosts {
                    for preferences in &variants {
                        let case = Case {
                            entry: &entry,
                            offer_windows_guest: false,
                            host,
                            preferences,
                        };
                        let options = check(&case);
                        cases += 1;
                        let runtime = options
                            .iter()
                            .find(|option| option.route == Route::AndroidRuntime)
                            .expect("the Android runtime is always listed for an Android edition");
                        // Play Integrity always blocks, and says so.
                        if entry.editions[0].requires(Requirement::PlayIntegrity) {
                            let caveat = runtime.caveat_for(Requirement::PlayIntegrity).unwrap();
                            assert_eq!(caveat.severity, Severity::Blocker);
                            assert_eq!(runtime.outlook, Outlook::Blocked);
                            assert_eq!(options[0].route, Route::NoRoute);
                        }
                        // ARM-only code blocks exactly on a non-ARM machine
                        // without a translation layer.
                        if entry.editions[0].requires(Requirement::ArmOnlyNativeCode) {
                            let caveat =
                                runtime.caveat_for(Requirement::ArmOnlyNativeCode).unwrap();
                            let blocks = host.arch != Arch::Aarch64 && !host.arm_translation;
                            assert_eq!(caveat.severity == Severity::Blocker, blocks);
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 256 * 2 * 3 * 64 * 3);
}

/// A macOS edition, alone: every requirement set, three architectures,
/// Darling on and off.
#[test]
fn macos_edition_space() {
    let mut hosts = Vec::new();
    for arch in Arch::ALL {
        for darling_enabled in [false, true] {
            for gpu in [GpuClass::None, GpuClass::Discrete] {
                hosts.push(HostFacts {
                    arch,
                    darling_enabled,
                    gpu,
                    ..HostFacts::default()
                });
            }
        }
    }
    let variants = [Preferences::default(), pinned(Route::Darling)];
    let mut cases = 0u64;
    for mask in 0..256u32 {
        for obtain in [Obtain::MacAppStore, Obtain::PublisherPage] {
            for rating in [None, Some(Rating::Good), Some(Rating::Broken)] {
                let mut macos = edition(EditionKind::Macos);
                macos.obtain = Some(obtain);
                macos.requirements = requirement_set(mask);
                if let Some(rating) = rating {
                    macos.records.push(record(Route::Darling, rating));
                }
                let entry = entry(Category::Creative, vec![macos]);
                for host in &hosts {
                    for preferences in &variants {
                        let case = Case {
                            entry: &entry,
                            offer_windows_guest: false,
                            host,
                            preferences,
                        };
                        let options = check(&case);
                        cases += 1;
                        let darling = options.iter().find(|option| option.route == Route::Darling);
                        assert_eq!(
                            darling.is_some(),
                            host.darling_enabled,
                            "{}",
                            case.describe()
                        );
                        if let Some(darling) = darling {
                            // Always labelled experimental.
                            assert!(
                                darling
                                    .caveats
                                    .iter()
                                    .any(|caveat| caveat.code == CaveatCode::Experimental)
                            );
                            assert_eq!(
                                darling.outlook == Outlook::Blocked,
                                host.arch != Arch::X86_64
                                    || darling
                                        .caveats
                                        .iter()
                                        .any(|caveat| caveat.severity == Severity::Blocker)
                            );
                        } else {
                            assert_eq!(options.len(), 1);
                            assert_eq!(options[0].route, Route::NoRoute);
                            assert_eq!(
                                options[0].alternative.as_ref().unwrap().kind,
                                tur_catalogue::AlternativeKind::Mac
                            );
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 256 * 2 * 3 * 12 * 2);
}

/// A Linux or web edition, alone: every requirement set.
#[test]
fn linux_and_web_edition_space() {
    let hosts = [
        HostFacts::default(),
        full_host(),
        HostFacts {
            arch: Arch::Aarch64,
            flatpak: false,
            gpu: GpuClass::None,
            ..HostFacts::default()
        },
    ];
    let variants = [
        Preferences::default(),
        Preferences {
            prefer_open_source: true,
            never_use_virtual_machine: true,
            ..Preferences::default()
        },
        pinned(Route::Web),
    ];
    let templates: Vec<(Edition, Route)> = vec![
        (edition(EditionKind::Flatpak), Route::Flatpak),
        (edition(EditionKind::LinuxNative), Route::Native),
        (debian(), Route::DistroContainer),
        (edition(EditionKind::Web), Route::Web),
        (
            {
                let mut web = edition(EditionKind::Web);
                web.primary = true;
                web
            },
            Route::Web,
        ),
    ];
    let mut cases = 0u64;
    for (template, expected_route) in &templates {
        for mask in 0..256u32 {
            for rating in [None, Some(Rating::Good), Some(Rating::Poor)] {
                let mut only = template.clone();
                only.requirements = requirement_set(mask);
                if let Some(rating) = rating {
                    only.records.push(record(*expected_route, rating));
                }
                let entry = entry(Category::Utility, vec![only]);
                for host in &hosts {
                    for preferences in &variants {
                        let case = Case {
                            entry: &entry,
                            offer_windows_guest: false,
                            host,
                            preferences,
                        };
                        let options = check(&case);
                        cases += 1;
                        // The one edition is always listed through its route.
                        assert!(
                            options.iter().any(|option| option.route == *expected_route),
                            "{}",
                            case.describe()
                        );
                        // A web application is never blocked.
                        if *expected_route == Route::Web {
                            assert_eq!(options.len(), 1);
                            assert_ne!(options[0].outlook, Outlook::Blocked);
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 5 * 256 * 3 * 3 * 3);
}

/// Every combination of edition kinds in one entry, with one requirement
/// (or none) on every edition and four rating patterns.
#[test]
fn mixed_edition_space() {
    let hosts = [
        HostFacts::default(),
        full_host(),
        HostFacts {
            arch: Arch::Aarch64,
            darling_enabled: false,
            ..full_host()
        },
    ];
    let variants: Vec<(Preferences, bool)> = vec![
        (Preferences::default(), false),
        (Preferences::default(), true),
        (
            Preferences {
                never_use_virtual_machine: true,
                ..Preferences::default()
            },
            true,
        ),
        (
            Preferences {
                prefer_open_source: true,
                ..Preferences::default()
            },
            false,
        ),
        (pinned(Route::Web), false),
        (pinned(Route::WindowsGuest), false),
        (pinned(Route::Darling), false),
        (pinned(Route::Flatpak), false),
    ];
    // Rating patterns: (rating of Linux routes, rating of every other route).
    let patterns: [(Option<Rating>, Option<Rating>); 4] = [
        (None, None),
        (Some(Rating::Poor), Some(Rating::Good)),
        (Some(Rating::Broken), Some(Rating::Broken)),
        (Some(Rating::Good), Some(Rating::Poor)),
    ];
    let mut cases = 0u64;
    let mut demotions = 0u64;
    for subset in 1..128u32 {
        for primary in [false, true] {
            for category in [Category::Game, Category::Productivity] {
                for requirement in
                    std::iter::once(None).chain(Requirement::ALL.into_iter().map(Some))
                {
                    for (linux_rating, other_rating) in patterns {
                        let mut editions = Vec::new();
                        let mut templates = vec![
                            edition(EditionKind::Macos),
                            edition(EditionKind::Web),
                            edition(EditionKind::Android),
                            edition(EditionKind::Windows),
                            debian(),
                            edition(EditionKind::LinuxNative),
                            edition(EditionKind::Flatpak),
                        ];
                        templates[1].primary = primary;
                        templates[5].licence = Some(Licence::OpenSource);
                        for (bit, mut template) in templates.into_iter().enumerate() {
                            if subset & (1 << bit) == 0 {
                                continue;
                            }
                            template.requirements = requirement.into_iter().collect();
                            for route in template.routes() {
                                let route = *route;
                                let rating = if route.is_linux() {
                                    linux_rating
                                } else {
                                    other_rating
                                };
                                if let Some(rating) = rating {
                                    template.records.push(record(route, rating));
                                }
                            }
                            editions.push(template);
                        }
                        let entry = entry(category, editions);
                        for host in &hosts {
                            for (preferences, offer) in &variants {
                                let case = Case {
                                    entry: &entry,
                                    offer_windows_guest: *offer,
                                    host,
                                    preferences,
                                };
                                let options = check(&case);
                                cases += 1;
                                demotions += u64::from(
                                    options
                                        .iter()
                                        .any(|option| option.reason.contains("Listed after")),
                                );
                                // Routing is deterministic.
                                if cases.is_multiple_of(97) {
                                    assert_eq!(options, check(&case));
                                }
                                // A secondary web edition appears only when
                                // nothing better is clear, or when pinned.
                                let secondary_web =
                                    options.iter().any(|option| is_secondary_web(&case, option));
                                let better = options.iter().any(|option| {
                                    option.outlook == Outlook::Clear
                                        && !matches!(
                                            option.route,
                                            Route::WindowsGuest | Route::Darling
                                        )
                                        && !is_secondary_web(&case, option)
                                });
                                let web_pinned =
                                    preferences.pinned_route(APP_ID) == Some(Route::Web);
                                if secondary_web {
                                    assert!(!better || web_pinned, "{}", case.describe());
                                }
                                // Darling, when listed and not blocked, is
                                // the last route that is not blocked.
                                let last_usable = options
                                    .iter()
                                    .rfind(|option| {
                                        option.route != Route::NoRoute
                                            && option.outlook != Outlook::Blocked
                                    })
                                    .map(|option| option.route);
                                let darling_usable = options.iter().any(|option| {
                                    option.route == Route::Darling
                                        && option.outlook != Outlook::Blocked
                                });
                                if darling_usable {
                                    assert_eq!(
                                        last_usable,
                                        Some(Route::Darling),
                                        "{}",
                                        case.describe()
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 127 * 2 * 2 * 9 * 4 * 3 * 8);
    assert!(demotions > 1_000, "{demotions}");
}

// ---------------------------------------------------------------------------
// The invariant checks can fail: each detects a hand-made bad result.
// ---------------------------------------------------------------------------

fn option(route: Route, edition_index: usize, kind: EditionKind) -> RouteOption {
    RouteOption {
        route,
        edition_index: Some(edition_index),
        edition_kind: Some(kind),
        edition: Some("an edition".to_owned()),
        rating: Rating::Unknown,
        rating_source: RatingSource::Unrated,
        outlook: Outlook::Clear,
        recommended: false,
        pinned: false,
        reason: "A reason.".to_owned(),
        caveats: Vec::new(),
        prerequisites: Vec::new(),
        alternative: None,
        reason_code: ReasonCode::WindowsWine,
        reason_parts: vec![ReasonPart {
            code: ReasonCode::WindowsWine,
            params: Default::default(),
        }],
    }
}

fn with_bad_result(
    category: Category,
    editions: Vec<Edition>,
    preferences: Preferences,
    bad: Vec<RouteOption>,
    invariant: fn(&Case<'_>, &[RouteOption]),
) {
    let entry = entry(category, editions);
    let host = full_host();
    let case = Case {
        entry: &entry,
        offer_windows_guest: true,
        host: &host,
        preferences: &preferences,
    };
    invariant(&case, &bad);
}

#[test]
#[should_panic(expected = "the guest precedes the compatibility layer")]
fn a_guest_before_its_layer_is_detected() {
    let bad = vec![
        option(Route::WindowsGuest, 0, EditionKind::Windows),
        option(Route::Wine, 0, EditionKind::Windows),
    ];
    with_bad_result(
        Category::Productivity,
        vec![edition(EditionKind::Windows)],
        Preferences::default(),
        bad,
        guest_never_precedes_its_layer,
    );
}

#[test]
#[should_panic(expected = "windows-guest is forbidden and appears")]
fn a_forbidden_route_is_detected() {
    let bad = vec![
        option(Route::Wine, 0, EditionKind::Windows),
        option(Route::WindowsGuest, 0, EditionKind::Windows),
    ];
    with_bad_result(
        Category::Productivity,
        vec![edition(EditionKind::Windows)],
        Preferences {
            never_use_virtual_machine: true,
            ..Preferences::default()
        },
        bad,
        forbidden_routes_never_appear,
    );
}

#[test]
#[should_panic(expected = "wine has no caveat for kernel-anti-cheat")]
fn a_missing_blocking_caveat_is_detected() {
    let mut windows = edition(EditionKind::Windows);
    windows.requirements = vec![Requirement::KernelAntiCheat];
    let bad = vec![option(Route::Wine, 0, EditionKind::Windows)];
    with_bad_result(
        Category::Productivity,
        vec![windows],
        Preferences::default(),
        bad,
        requirements_carry_their_caveats,
    );
}

#[test]
#[should_panic(expected = "empty result")]
fn an_empty_result_is_detected() {
    with_bad_result(
        Category::Productivity,
        vec![edition(EditionKind::Windows)],
        Preferences::default(),
        Vec::new(),
        never_empty_and_no_route_is_explicit,
    );
}

#[test]
#[should_panic(expected = "\"no route\" must be stated exactly when nothing can work")]
fn a_silent_best_guess_is_detected() {
    // Nothing usable, and no statement that there is no route.
    let mut blocked = option(Route::Wine, 0, EditionKind::Windows);
    blocked.outlook = Outlook::Blocked;
    with_bad_result(
        Category::Productivity,
        vec![edition(EditionKind::Windows)],
        Preferences::default(),
        vec![blocked],
        never_empty_and_no_route_is_explicit,
    );
}

/// The requirement table itself, written out independently of the router:
/// which requirement blocks which route outright.
#[test]
fn the_blocking_cells_of_the_requirement_table() {
    let x86 = full_host();
    let arm = HostFacts {
        arch: Arch::Aarch64,
        ..full_host()
    };
    let translated = HostFacts {
        arm_translation: true,
        ..full_host()
    };
    let blockers: Vec<(Requirement, Route)> = Requirement::ALL
        .into_iter()
        .flat_map(|requirement| {
            Route::RUNNABLE
                .into_iter()
                .map(move |route| (requirement, route))
        })
        .filter(|(requirement, route)| {
            requirement_severity(*requirement, *route, &x86) == Severity::Blocker
        })
        .collect();
    use Requirement::{ArmOnlyNativeCode, KernelAntiCheat, KernelDriver, PlayIntegrity};
    assert_eq!(
        blockers,
        [
            (KernelAntiCheat, Route::Proton),
            (KernelAntiCheat, Route::Wine),
            (KernelAntiCheat, Route::WindowsGuest),
            (KernelAntiCheat, Route::AndroidRuntime),
            (KernelAntiCheat, Route::Darling),
            (PlayIntegrity, Route::AndroidRuntime),
            (ArmOnlyNativeCode, Route::Flatpak),
            (ArmOnlyNativeCode, Route::Native),
            (ArmOnlyNativeCode, Route::DistroContainer),
            (ArmOnlyNativeCode, Route::Proton),
            (ArmOnlyNativeCode, Route::Wine),
            (ArmOnlyNativeCode, Route::WindowsGuest),
            (ArmOnlyNativeCode, Route::AndroidRuntime),
            (ArmOnlyNativeCode, Route::Darling),
            (KernelDriver, Route::AndroidRuntime),
            (KernelDriver, Route::Darling),
        ]
    );
    // What blocks the Windows guest also blocks both compatibility layers,
    // on every host: so a guest can never be usable while its layer is not.
    for host in [&x86, &arm, &translated] {
        for requirement in Requirement::ALL {
            if requirement_severity(requirement, Route::WindowsGuest, host) == Severity::Blocker {
                for layer in [Route::Proton, Route::Wine] {
                    assert_eq!(
                        requirement_severity(requirement, layer, host),
                        Severity::Blocker
                    );
                }
            }
        }
    }
    // ARM-only code is no obstacle on an ARM machine, and a warning with a
    // translation layer supplied by the user.
    for route in Route::RUNNABLE {
        assert_eq!(
            requirement_severity(ArmOnlyNativeCode, route, &arm),
            Severity::Note
        );
    }
    assert_eq!(
        requirement_severity(ArmOnlyNativeCode, Route::AndroidRuntime, &translated),
        Severity::Warning
    );
    // A kernel driver and a dongle are obstacles for a layer, not for the guest.
    for layer in [Route::Proton, Route::Wine] {
        assert_eq!(
            requirement_severity(KernelDriver, layer, &x86),
            Severity::Obstacle
        );
        assert_eq!(
            requirement_severity(Requirement::HardwareDongle, layer, &x86),
            Severity::Obstacle
        );
    }
    assert_eq!(
        requirement_severity(KernelDriver, Route::WindowsGuest, &x86),
        Severity::Note
    );
    // Nothing blocks a web application.
    for requirement in Requirement::ALL {
        for host in [&x86, &arm] {
            assert!(requirement_severity(requirement, Route::Web, host) < Severity::Obstacle);
        }
    }
}
