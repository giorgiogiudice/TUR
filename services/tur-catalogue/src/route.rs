// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The router: the routing ladder of ADR-0007 as a pure function.
//!
//! [`route`] takes a catalogue entry, facts about the machine and the user's
//! preferences, and returns the ways the application can run, in ladder
//! order, each with its rating, the reason it is listed, the caveats the
//! user must see before choosing, and what must be installed first.
//!
//! The router decides nothing by itself beyond that order. It reads its
//! three inputs and returns values: it does not touch the file system, start
//! a process or use the network, and so it cannot install or launch
//! anything.
//!
//! # The ladder
//!
//! | Rung | Route | Offered when |
//! |---|---|---|
//! | 1 | Flatpak, then native Linux | A Linux edition exists |
//! | 2 | Web application, primary | The web application is the publisher's primary form |
//! | 3 | Container of another distribution | The Linux edition is another distribution's package |
//! | 4 | Proton (games) or Wine (other Windows applications) | A Windows edition exists |
//! | 4 | Windows guest, directly after its compatibility layer | Not a game; and the layer is rated poor or broken, or is unrated with a known obstacle, or the user asks; and virtual machines are not forbidden |
//! | 5 | Android runtime | An Android edition exists |
//! | 6 | Web application, not primary | No better route is free of known obstacles, or the user pinned it |
//! | 7 | Darling | A macOS edition exists and the user has enabled Darling |
//!
//! A Linux edition whose record says it is clearly worse than another route
//! is listed after that route. Routes that cannot work on this machine are
//! listed last, marked [`Outlook::Blocked`], with the caveat that says why.
//! When nothing else is left, the list starts with [`Route::NoRoute`], with
//! the reason and the least-bad alternative.

use serde::Serialize;

use crate::host::{Arch, GpuClass, HostFacts, Preferences};
use crate::model::{
    AndroidStore, Category, Edition, EditionKind, Entry, Licence, Obtain, Packaging, Rating,
    Record, Reporter, Requirement, Route,
};

/// The memory, in mebibytes, the Windows guest tier is expected to need in
/// total. ADR-0007 gives 16 GB as an indicative figure, to be measured.
pub const GUEST_MEMORY_MIB: u64 = 16 * 1024;

/// A machine sold with 16 GB reports a little less; this is the least
/// installed memory the router accepts as meeting [`GUEST_MEMORY_MIB`].
const GUEST_MEMORY_FLOOR_MIB: u64 = 15_000;

/// What the user asked for.
#[derive(Debug, Clone, Copy)]
pub struct RouteRequest<'a> {
    /// The application, as the catalogue describes it.
    pub entry: &'a Entry,
    /// The user asks for the Windows guest to be offered as well, whatever
    /// the rating of the compatibility layer.
    pub offer_windows_guest: bool,
}

impl<'a> RouteRequest<'a> {
    /// A plain request for `entry`.
    pub fn new(entry: &'a Entry) -> Self {
        RouteRequest {
            entry,
            offer_windows_guest: false,
        }
    }
}

/// Whether anything known stands in the way of a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outlook {
    /// Nothing known stands in the way. This is not a promise that the
    /// application works: see the rating and its source.
    Clear,
    /// A poor or broken rating, or a known obstacle that no rating
    /// contradicts, makes success doubtful.
    Doubtful,
    /// The route cannot work on this machine. Listed so that the user sees
    /// why; never recommended.
    Blocked,
}

/// How much a caveat matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// Worth knowing.
    Note,
    /// Expect a limitation.
    Warning,
    /// A known requirement the route does not meet, or a limit of this
    /// machine: the application probably will not work fully.
    Obstacle,
    /// The route cannot work.
    Blocker,
}

/// What a caveat is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaveatCode {
    /// A known requirement of the edition. The caveat names it.
    Requirement,
    /// The route runs x86-64 software and the machine is not x86-64.
    Architecture,
    /// The memory the Windows guest needs.
    GuestMemory,
    /// The route is experimental.
    Experimental,
    /// The edition comes only from a store TUR cannot integrate.
    ClosedStore,
    /// The package is maintained by volunteers, not by the publisher.
    CommunityPackage,
    /// The route's record says poor or broken.
    ReportedInadequate,
    /// A Windows guest would be offered, and the user forbids virtual
    /// machines. Says what is lost.
    GuestNotOffered,
    /// A Windows guest is never offered for a game.
    GuestUnsuitableForGames,
    /// The route the user pinned for this application is not available.
    PinUnavailable,
}

/// Something the user must see before choosing a route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Caveat {
    /// What the caveat is about.
    pub code: CaveatCode,
    /// How much it matters.
    pub severity: Severity,
    /// The requirement, for [`CaveatCode::Requirement`].
    pub requirement: Option<Requirement>,
    /// The caveat in plain language.
    pub text: String,
}

/// What a prerequisite asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrerequisiteCode {
    /// Flatpak must be installed.
    InstallFlatpak,
    /// A tool that runs containers of other distributions must be installed.
    InstallContainerTool,
    /// Wine must be installed.
    InstallWine,
    /// `umu-launcher` and a Proton build must be installed.
    InstallProton,
    /// The Android runtime must be enabled.
    EnableAndroidRuntime,
    /// Google Play must be opted into.
    OptIntoGooglePlay,
    /// The Windows guest must be set up.
    SetUpWindowsGuest,
    /// A Windows licence must be supplied.
    ProvideWindowsLicence,
}

/// Something that must be installed or enabled before a route can be used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prerequisite {
    /// What is asked for.
    pub code: PrerequisiteCode,
    /// The prerequisite in plain language.
    pub text: String,
}

/// Where the rating of a route comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RatingSource {
    /// The catalogue holds no rating from anyone.
    Unrated,
    /// Reported by a named external source. Not tested by TUR.
    External {
        /// The name of the source.
        name: String,
        /// The page that was checked.
        url: String,
        /// The source's own rating, in its own words.
        source_rating: Option<String>,
        /// The version the source tested, when it says.
        version_tested: Option<String>,
        /// The day the source was checked.
        date: String,
    },
    /// Tested by TUR.
    TurTest {
        /// The path of the test report in the repository.
        report: String,
        /// The version tested.
        version_tested: Option<String>,
        /// The day of the test.
        date: String,
    },
}

/// What kind of alternative a no-route result points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AlternativeKind {
    /// Install Windows beside TUR and run the application there.
    DualBoot,
    /// Use the application on an Android device certified by Google.
    CertifiedAndroidDevice,
    /// Use a Mac.
    Mac,
    /// Use another computer.
    AnotherComputer,
    /// No alternative is known.
    NoneKnown,
}

/// The least-bad alternative when no route runs the application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Alternative {
    /// The kind of alternative.
    pub kind: AlternativeKind,
    /// The alternative in plain language.
    pub text: String,
}

/// One way to run the application, or the statement that there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteOption {
    /// The route. [`Route::NoRoute`] states that nothing runs the
    /// application on this machine.
    pub route: Route,
    /// The position, in the entry, of the edition this route runs.
    pub edition_index: Option<usize>,
    /// The kind of that edition.
    pub edition_kind: Option<EditionKind>,
    /// The edition and where it is obtained, in a few words.
    pub edition: Option<String>,
    /// The rating of this route for this edition.
    pub rating: Rating,
    /// Where the rating comes from.
    pub rating_source: RatingSource,
    /// Whether anything known stands in the way.
    pub outlook: Outlook,
    /// The option TUR would use by default. Exactly one option has it.
    pub recommended: bool,
    /// The user pinned this route for this application.
    pub pinned: bool,
    /// Why the option is listed, in plain language.
    pub reason: String,
    /// What the user must see before choosing.
    pub caveats: Vec<Caveat>,
    /// What must be installed or enabled first.
    pub prerequisites: Vec<Prerequisite>,
    /// For [`Route::NoRoute`]: the least-bad alternative.
    pub alternative: Option<Alternative>,
}

impl RouteOption {
    /// The most severe caveat of the option, if it has any.
    pub fn worst_severity(&self) -> Option<Severity> {
        self.caveats.iter().map(|caveat| caveat.severity).max()
    }

    /// Whether the option carries a caveat for `requirement`.
    pub fn caveat_for(&self, requirement: Requirement) -> Option<&Caveat> {
        self.caveats
            .iter()
            .find(|caveat| caveat.requirement == Some(requirement))
    }
}

/// Returns the option a caller should use by default: the one marked
/// recommended.
///
/// # Panics
///
/// Never for a list returned by [`route`], which always holds exactly one
/// recommended option.
pub fn recommended(options: &[RouteOption]) -> &RouteOption {
    options
        .iter()
        .find(|option| option.recommended)
        .expect("route() marks exactly one option as recommended")
}

/// How much a known requirement matters on a route, on this machine.
///
/// This is the requirement table of the routing rules. [`Severity::Blocker`]
/// means the route cannot work; [`Severity::Obstacle`] means the route does
/// not meet the requirement and the application probably will not work
/// fully.
pub fn requirement_severity(requirement: Requirement, route: Route, host: &HostFacts) -> Severity {
    requirement_caveat(requirement, route, host).0
}

/// The severity and the wording of a requirement on a route.
fn requirement_caveat(
    requirement: Requirement,
    route: Route,
    host: &HostFacts,
) -> (Severity, String) {
    let (severity, text): (Severity, &str) = match requirement {
        Requirement::KernelAntiCheat => match route {
            Route::Proton | Route::Wine => (
                Severity::Blocker,
                "Its anti-cheat will not work here: it runs only on real Windows, with a kernel driver that a compatibility layer cannot provide. Dual boot is the only way.",
            ),
            Route::WindowsGuest => (
                Severity::Blocker,
                "Its kernel-level anti-cheat is not expected to accept a virtual machine. Dual boot is the only way.",
            ),
            Route::AndroidRuntime => (
                Severity::Blocker,
                "Its anti-cheat detects that this is not a certified phone: it fails, or puts the account at risk of a ban.",
            ),
            Route::Darling => (
                Severity::Blocker,
                "Its anti-cheat needs a kernel component that Darling cannot load.",
            ),
            Route::Flatpak | Route::Native | Route::DistroContainer => (
                Severity::Warning,
                "The catalogue lists kernel-level anti-cheat for this edition; check the publisher's notes for Linux before relying on it.",
            ),
            Route::Web | Route::NoRoute => (
                Severity::Note,
                "Other editions of this application use kernel-level anti-cheat; the web edition does not load it.",
            ),
        },
        Requirement::HardwareAttestedDrm => match route {
            Route::Flatpak
            | Route::Native
            | Route::DistroContainer
            | Route::Web
            | Route::NoRoute => (
                Severity::Warning,
                "Its copy protection asks for hardware attestation: on Linux, protected content may be limited to a lower quality or refused.",
            ),
            Route::Proton
            | Route::Wine
            | Route::WindowsGuest
            | Route::AndroidRuntime
            | Route::Darling => (
                Severity::Obstacle,
                "Its copy protection needs hardware attestation, which this route cannot provide: protected content is limited to low quality or refused.",
            ),
        },
        Requirement::PlayIntegrity => match route {
            Route::AndroidRuntime => (
                Severity::Blocker,
                "Needs a phone certified by Google (Play Integrity): it will refuse to run here. No container or virtual machine passes that check.",
            ),
            _ => (
                Severity::Note,
                "The Android edition needs Google's Play Integrity check; this route is not affected by it.",
            ),
        },
        Requirement::ArmOnlyNativeCode => {
            if host.arch == Arch::Aarch64 {
                (
                    Severity::Note,
                    "Built for ARM processors only; this machine has one.",
                )
            } else {
                match route {
                    Route::AndroidRuntime if host.arm_translation => (
                        Severity::Warning,
                        "Built for ARM processors only: it runs through the ARM translation layer you supplied. Expect it to be slower and to crash at times.",
                    ),
                    Route::AndroidRuntime => (
                        Severity::Blocker,
                        "Built for ARM processors only: it does not run on this machine as shipped. TUR may not ship an ARM translation layer; you can supply one.",
                    ),
                    Route::Web | Route::NoRoute => (
                        Severity::Note,
                        "Other editions are built for ARM processors only; the web edition is not affected.",
                    ),
                    _ => (
                        Severity::Blocker,
                        "Built for ARM processors only: it does not run on this machine.",
                    ),
                }
            }
        }
        Requirement::KernelDriver => match route {
            Route::Proton | Route::Wine => (
                Severity::Obstacle,
                "Needs a Windows kernel driver, which a compatibility layer cannot load: the parts that depend on it will not work.",
            ),
            Route::WindowsGuest => (
                Severity::Note,
                "Its Windows kernel driver is installed inside the Windows guest, not on TUR.",
            ),
            Route::AndroidRuntime | Route::Darling => (
                Severity::Blocker,
                "Needs a kernel driver of its own operating system; TUR never loads one into its kernel.",
            ),
            Route::Flatpak | Route::Native | Route::DistroContainer => (
                Severity::Warning,
                "Needs a Linux kernel module, which TUR's system image may not provide.",
            ),
            Route::Web | Route::NoRoute => (
                Severity::Note,
                "Other editions need a kernel driver; the web edition does not.",
            ),
        },
        Requirement::HardwareDongle => match route {
            Route::Proton | Route::Wine | Route::AndroidRuntime | Route::Darling => (
                Severity::Obstacle,
                "Needs a hardware dongle, whose driver is not expected to work on this route.",
            ),
            Route::WindowsGuest => (
                Severity::Warning,
                "Needs its hardware dongle, which must be passed through to the Windows guest.",
            ),
            Route::Flatpak | Route::Native | Route::DistroContainer => {
                (Severity::Note, "Needs its hardware dongle connected.")
            }
            Route::Web | Route::NoRoute => (
                Severity::Note,
                "Other editions need a hardware dongle; check whether the web edition does.",
            ),
        },
        Requirement::HeavyGpu => {
            if route == Route::WindowsGuest {
                (
                    Severity::Obstacle,
                    "Relies on the graphics processor, and the Windows guest has no usable GPU acceleration: expect it to be slow or to fail.",
                )
            } else {
                match host.gpu {
                    GpuClass::None => (
                        Severity::Obstacle,
                        "Relies on the graphics processor, and this machine has none that can accelerate it.",
                    ),
                    GpuClass::Integrated => (
                        Severity::Warning,
                        "Relies on the graphics processor; expect reduced performance on integrated graphics.",
                    ),
                    GpuClass::Discrete => (
                        Severity::Note,
                        "Relies on the graphics processor; this machine has a discrete one.",
                    ),
                    GpuClass::Unknown => (
                        Severity::Note,
                        "Relies on the graphics processor; performance depends on this machine's graphics hardware and driver.",
                    ),
                }
            }
        }
        Requirement::OnlineAccount => (
            Severity::Note,
            "Needs an online account with its publisher.",
        ),
    };
    let mut text = text.to_owned();
    if requirement == Requirement::KernelAntiCheat && route == Route::Proton {
        text = "This game's anti-cheat will not work: it runs only on real Windows, with a kernel driver that a compatibility layer cannot provide. Dual boot is the only way.".to_owned();
    }
    (severity, text)
}

/// The record the router uses for a route of an edition: TUR's own test
/// before an external report, then the most recent, then the first written.
fn best_record(edition: &Edition, route: Route) -> Option<&Record> {
    let mut best: Option<&Record> = None;
    for record in edition
        .records
        .iter()
        .filter(|record| record.route == route)
    {
        let better = match best {
            None => true,
            Some(current) => {
                let rank = |record: &Record| match record.reported_by {
                    Reporter::TurTest => 2u8,
                    Reporter::External => 1,
                    Reporter::Nobody => 0,
                };
                (rank(record), record.date.as_str()) > (rank(current), current.date.as_str())
            }
        };
        if better {
            best = Some(record);
        }
    }
    best
}

fn rating_of(edition: &Edition, route: Route) -> (Rating, RatingSource) {
    let Some(record) = best_record(edition, route) else {
        return (Rating::Unknown, RatingSource::Unrated);
    };
    let source = match record.reported_by {
        Reporter::Nobody => RatingSource::Unrated,
        Reporter::External => RatingSource::External {
            name: record.source_name.clone().unwrap_or_default(),
            url: record.source_url.clone().unwrap_or_default(),
            source_rating: record.source_rating.clone(),
            version_tested: record.version_tested.clone(),
            date: record.date.clone(),
        },
        Reporter::TurTest => RatingSource::TurTest {
            report: record.test_report.clone().unwrap_or_default(),
            version_tested: record.version_tested.clone(),
            date: record.date.clone(),
        },
    };
    (record.rating, source)
}

fn caveat(code: CaveatCode, severity: Severity, text: impl Into<String>) -> Caveat {
    Caveat {
        code,
        severity,
        requirement: None,
        text: text.into(),
    }
}

fn prerequisite(code: PrerequisiteCode, text: &str) -> Prerequisite {
    Prerequisite {
        code,
        text: text.to_owned(),
    }
}

/// Builds the option for one route of one edition: its rating, caveats,
/// prerequisites and outlook.
fn evaluate(
    route: Route,
    index: usize,
    edition: &Edition,
    host: &HostFacts,
    reason: String,
) -> RouteOption {
    let (rating, rating_source) = rating_of(edition, route);
    let mut caveats = Vec::new();
    // An obstacle that a good rating from elsewhere cannot outweigh, because
    // it comes from this machine or from how the edition is distributed.
    let mut firm_obstacle = false;

    let needs_x86_64 = matches!(
        route,
        Route::Proton | Route::Wine | Route::WindowsGuest | Route::Darling
    );
    if needs_x86_64 && host.arch != Arch::X86_64 {
        caveats.push(caveat(
            CaveatCode::Architecture,
            Severity::Blocker,
            format!(
                "This route runs software built for x86-64 processors, and this machine is {}.",
                host.arch.as_str()
            ),
        ));
    }

    for requirement in Requirement::ALL {
        if !edition.requires(requirement) {
            continue;
        }
        let (severity, text) = requirement_caveat(requirement, route, host);
        if severity == Severity::Obstacle && requirement == Requirement::HeavyGpu {
            firm_obstacle = true;
        }
        caveats.push(Caveat {
            code: CaveatCode::Requirement,
            severity,
            requirement: Some(requirement),
            text,
        });
    }

    if let Some(obtain) = edition.obtain
        && obtain.is_closed_store()
    {
        if route == Route::WindowsGuest {
            caveats.push(caveat(
                CaveatCode::ClosedStore,
                Severity::Note,
                format!(
                    "This edition comes from {}; inside the Windows guest you install it from there yourself.",
                    obtain.label()
                ),
            ));
        } else {
            firm_obstacle = true;
            caveats.push(caveat(
                CaveatCode::ClosedStore,
                Severity::Obstacle,
                format!(
                    "This edition is distributed only through {}, which TUR cannot integrate, and store packages are not expected to install on this route.",
                    obtain.label()
                ),
            ));
        }
    }

    if edition.packaging == Some(Packaging::Community) {
        caveats.push(caveat(
            CaveatCode::CommunityPackage,
            Severity::Note,
            "This package is maintained by its packagers, not by the application's publisher.",
        ));
    }

    if route == Route::Darling {
        caveats.push(caveat(
            CaveatCode::Experimental,
            Severity::Obstacle,
            "Experimental: Darling comes with no promise that this application works, and it does not run mainstream macOS graphical applications.",
        ));
    }

    if route == Route::WindowsGuest {
        match host.memory_mib {
            Some(memory) if memory < GUEST_MEMORY_FLOOR_MIB => {
                firm_obstacle = true;
                caveats.push(caveat(
                    CaveatCode::GuestMemory,
                    Severity::Obstacle,
                    format!(
                        "Needs about 16 GB of memory while the Windows guest runs; this machine has about {} GB.",
                        (memory + 512) / 1024
                    ),
                ));
            }
            Some(_) => caveats.push(caveat(
                CaveatCode::GuestMemory,
                Severity::Note,
                "Needs about 16 GB of memory while the Windows guest runs; this machine has that much.",
            )),
            None => caveats.push(caveat(
                CaveatCode::GuestMemory,
                Severity::Warning,
                "Needs about 16 GB of memory while the Windows guest runs.",
            )),
        }
    }

    if rating.is_adverse() {
        let text = match &rating_source {
            RatingSource::External {
                name,
                source_rating,
                date,
                ..
            } => match source_rating {
                Some(words) => format!(
                    "Reported {} by {name} (\"{words}\"), checked {date}. Not tested by TUR.",
                    rating.as_str()
                ),
                None => format!(
                    "Reported {} by {name}, checked {date}. Not tested by TUR.",
                    rating.as_str()
                ),
            },
            RatingSource::TurTest { report, date, .. } => format!(
                "Rated {} in TUR's own test of {date} ({report}).",
                rating.as_str()
            ),
            RatingSource::Unrated => format!("Rated {}.", rating.as_str()),
        };
        caveats.push(caveat(
            CaveatCode::ReportedInadequate,
            Severity::Warning,
            text,
        ));
    }

    let blocked = caveats
        .iter()
        .any(|caveat| caveat.severity == Severity::Blocker);
    let obstacle = caveats
        .iter()
        .any(|caveat| caveat.severity == Severity::Obstacle);
    let outlook = if blocked {
        Outlook::Blocked
    } else if rating.is_adverse() || firm_obstacle || (obstacle && !rating.is_supporting()) {
        Outlook::Doubtful
    } else {
        Outlook::Clear
    };

    RouteOption {
        route,
        edition_index: Some(index),
        edition_kind: Some(edition.kind),
        edition: Some(edition.describe()),
        rating,
        rating_source,
        outlook,
        recommended: false,
        pinned: false,
        reason,
        caveats,
        prerequisites: prerequisites(route, edition, host),
        alternative: None,
    }
}

/// What must be installed or enabled before `route` can be used.
fn prerequisites(route: Route, edition: &Edition, host: &HostFacts) -> Vec<Prerequisite> {
    let mut list = Vec::new();
    match route {
        Route::Flatpak => {
            if !host.flatpak {
                list.push(prerequisite(
                    PrerequisiteCode::InstallFlatpak,
                    "Flatpak must be installed.",
                ));
            }
        }
        Route::DistroContainer => {
            if !host.distro_containers {
                list.push(prerequisite(
                    PrerequisiteCode::InstallContainerTool,
                    "A tool that runs containers of other distributions must be installed.",
                ));
            }
        }
        Route::Wine => {
            if !host.wine {
                list.push(prerequisite(
                    PrerequisiteCode::InstallWine,
                    "Wine must be installed.",
                ));
            }
        }
        Route::Proton => {
            if !host.proton {
                list.push(prerequisite(
                    PrerequisiteCode::InstallProton,
                    "umu-launcher must be installed, and it must fetch a Proton build: TUR does not ship Proton.",
                ));
            }
        }
        Route::AndroidRuntime => {
            if !host.android_runtime {
                list.push(prerequisite(
                    PrerequisiteCode::EnableAndroidRuntime,
                    "The Android runtime must be enabled: it is off by default.",
                ));
            }
            let play_only = !edition.stores.is_empty()
                && edition
                    .stores
                    .iter()
                    .all(|store| *store == AndroidStore::GooglePlay);
            if play_only && !host.google_play {
                list.push(prerequisite(
                    PrerequisiteCode::OptIntoGooglePlay,
                    "The catalogue lists this package on Google Play only: Google Play must be opted into, under Google's terms.",
                ));
            }
        }
        Route::WindowsGuest => {
            if !host.windows_guest {
                list.push(prerequisite(
                    PrerequisiteCode::SetUpWindowsGuest,
                    "The Windows guest must be set up.",
                ));
            }
            if !host.windows_licence {
                list.push(prerequisite(
                    PrerequisiteCode::ProvideWindowsLicence,
                    "You must supply a Windows licence: TUR does not include one.",
                ));
            }
        }
        Route::Native | Route::Darling | Route::Web | Route::NoRoute => {}
    }
    list
}

/// The options for a Windows edition: the compatibility layer, and the
/// Windows guest directly after it when the rules allow one.
fn windows_options(
    request: &RouteRequest<'_>,
    index: usize,
    edition: &Edition,
    host: &HostFacts,
    preferences: &Preferences,
) -> Vec<RouteOption> {
    let entry = request.entry;
    let game = entry.category == Category::Game;
    let (layer_route, reason) = if game {
        (
            Route::Proton,
            "A Windows edition exists and this is a game: Proton, through umu-launcher, is the first route for games.",
        )
    } else {
        (
            Route::Wine,
            "A Windows edition exists: Wine, a compatibility layer, comes before a virtual machine.",
        )
    };
    let mut layer = evaluate(layer_route, index, edition, host, reason.to_owned());

    if game {
        if layer.outlook == Outlook::Doubtful {
            layer.caveats.push(caveat(
                CaveatCode::GuestUnsuitableForGames,
                Severity::Note,
                "A Windows guest is not offered for games: it has no usable GPU acceleration.",
            ));
        }
        return vec![layer];
    }
    if layer.outlook == Outlook::Blocked {
        return vec![layer];
    }

    // An obstacle the guest would get round. Heavy GPU use is not one: the
    // guest has no usable GPU acceleration either.
    let known_obstacle = layer.caveats.iter().any(|caveat| {
        caveat.severity == Severity::Obstacle && caveat.requirement != Some(Requirement::HeavyGpu)
    });
    let asked = request.offer_windows_guest
        || preferences.pinned_route(&entry.id) == Some(Route::WindowsGuest);
    let why = if layer.rating.is_adverse() {
        Some(format!(
            "Offered because Wine is rated {} for this application.",
            layer.rating.as_str()
        ))
    } else if layer.rating == Rating::Unknown && known_obstacle {
        Some(
            "Offered because a known requirement stands in Wine's way and no rating says that Wine runs it all the same."
                .to_owned(),
        )
    } else if asked {
        Some("Offered because you asked for it.".to_owned())
    } else {
        None
    };
    let Some(why) = why else {
        return vec![layer];
    };

    if preferences.never_use_virtual_machine {
        layer.caveats.push(caveat(
            CaveatCode::GuestNotOffered,
            Severity::Warning,
            "A Windows guest would be offered for this application as well, and is not, because you have chosen never to use a virtual machine. What is lost: the route that runs it on real Windows, which is the one expected to work where Wine does not.",
        ));
        return vec![layer];
    }
    let guest = evaluate(
        Route::WindowsGuest,
        index,
        edition,
        host,
        format!(
            "{why} It runs in real Windows, in a virtual machine, and its windows appear on the desktop."
        ),
    );
    vec![layer, guest]
}

/// The position of an option on the ladder: lower comes first.
fn ladder_key(
    option: &RouteOption,
    entry: &Entry,
    preferences: &Preferences,
) -> (u8, u8, u8, u8, usize, u8) {
    let edition = option
        .edition_index
        .and_then(|index| entry.editions.get(index));
    let primary = edition.is_some_and(|edition| edition.primary);
    let rung = match option.route {
        Route::Flatpak | Route::Native => 0,
        Route::Web if primary => 1,
        Route::DistroContainer => 2,
        Route::Proton | Route::Wine | Route::WindowsGuest => 3,
        Route::AndroidRuntime => 4,
        Route::Web => 5,
        Route::Darling => 6,
        Route::NoRoute => 7,
    };
    let open_source = edition.is_some_and(|edition| edition.licence == Some(Licence::OpenSource));
    let licence_order = u8::from(preferences.prefer_open_source && !open_source);
    let kind_order = u8::from(option.route == Route::Native);
    let pair_order = u8::from(option.route == Route::WindowsGuest);
    (
        u8::from(option.outlook == Outlook::Blocked),
        rung,
        licence_order,
        kind_order,
        option.edition_index.unwrap_or(usize::MAX),
        pair_order,
    )
}

/// Moves a Linux edition whose record says poor or broken to just after the
/// first later route, other than Darling, whose record says good or usable:
/// "a native Linux edition wins, unless its record says it is clearly worse
/// than another route".
fn demote_clearly_worse(options: &mut Vec<RouteOption>) {
    let mut position = options.len();
    while position > 0 {
        position -= 1;
        let option = &options[position];
        let candidate = option.route.is_linux()
            && option.rating.is_adverse()
            && option.outlook != Outlook::Blocked;
        if !candidate {
            continue;
        }
        // Darling is experimental and stays last: it never overtakes.
        let better = options[position + 1..].iter().position(|other| {
            other.outlook != Outlook::Blocked
                && other.rating.is_supporting()
                && !other.route.is_linux()
                && other.route != Route::Darling
        });
        let Some(offset) = better else {
            continue;
        };
        let mut target = position + 1 + offset;
        let better_label = options[target].route.label();
        let better_rating = options[target].rating.as_str();
        // Keep a compatibility layer and its guest together.
        let followed_by_its_guest = options.get(target + 1).is_some_and(|next| {
            next.route == Route::WindowsGuest && next.edition_index == options[target].edition_index
        });
        if followed_by_its_guest {
            target += 1;
        }
        let mut moved = options.remove(position);
        moved.reason.push_str(&format!(
            " Listed after {better_label} because this edition is rated {} and that route is rated {better_rating}.",
            moved.rating.as_str()
        ));
        options.insert(target, moved);
    }
}

/// The explicit statement that nothing runs the application, with the
/// reason and the least-bad alternative.
fn no_route(entry: &Entry, host: &HostFacts, blocked: &[RouteOption]) -> RouteOption {
    let mut reason = format!("No route on TUR runs {} on this machine.", entry.name);
    let mut caveats: Vec<Caveat> = Vec::new();
    for option in blocked {
        let blocker = option
            .caveats
            .iter()
            .find(|caveat| caveat.severity == Severity::Blocker);
        if let Some(blocker) = blocker {
            reason.push_str(&format!(" {}: {}", option.route.label(), blocker.text));
            if !caveats.iter().any(|known| known.text == blocker.text) {
                caveats.push(blocker.clone());
            }
        }
    }
    let has_edition = |kind: EditionKind| entry.editions.iter().any(|edition| edition.kind == kind);
    let macos_without_darling = has_edition(EditionKind::Macos) && !host.darling_enabled;
    if macos_without_darling {
        reason.push_str(
            " The macOS edition has no route: Darling, TUR's experimental macOS layer, is not enabled, and a macOS virtual machine is not an option, because Apple's licence allows macOS guests only on Apple hardware.",
        );
    }
    if entry.editions.is_empty() {
        reason.push_str(" The catalogue lists no edition of it.");
    }

    let blocked_kind = |kind: EditionKind| {
        blocked
            .iter()
            .any(|option| option.edition_kind == Some(kind))
    };
    let anti_cheat = blocked.iter().any(|option| {
        option.edition_kind == Some(EditionKind::Windows)
            && option
                .caveat_for(Requirement::KernelAntiCheat)
                .is_some_and(|caveat| caveat.severity == Severity::Blocker)
    });
    let alternative = if anti_cheat {
        Alternative {
            kind: AlternativeKind::DualBoot,
            text: "Dual boot: install Windows beside TUR and run it there. That is the only way."
                .to_owned(),
        }
    } else if blocked_kind(EditionKind::Windows) && host.arch != Arch::X86_64 {
        Alternative {
            kind: AlternativeKind::AnotherComputer,
            text: "Use a computer with an x86-64 processor, or one that runs Windows.".to_owned(),
        }
    } else if blocked_kind(EditionKind::Windows) {
        Alternative {
            kind: AlternativeKind::DualBoot,
            text: "Run it on Windows itself: dual boot, or another computer.".to_owned(),
        }
    } else if blocked_kind(EditionKind::Android) {
        Alternative {
            kind: AlternativeKind::CertifiedAndroidDevice,
            text: "Use it on a phone or tablet certified by Google.".to_owned(),
        }
    } else if blocked_kind(EditionKind::Macos) || macos_without_darling {
        Alternative {
            kind: AlternativeKind::Mac,
            text: "Use a Mac. Darling can be enabled as an experimental feature, with no promise that this application works."
                .to_owned(),
        }
    } else if !blocked.is_empty() {
        Alternative {
            kind: AlternativeKind::AnotherComputer,
            text: "Use a computer whose processor the application is built for.".to_owned(),
        }
    } else {
        Alternative {
            kind: AlternativeKind::NoneKnown,
            text: "No alternative is known.".to_owned(),
        }
    };

    RouteOption {
        route: Route::NoRoute,
        edition_index: None,
        edition_kind: None,
        edition: None,
        rating: Rating::Unknown,
        rating_source: RatingSource::Unrated,
        outlook: Outlook::Blocked,
        recommended: false,
        pinned: false,
        reason,
        caveats,
        prerequisites: Vec::new(),
        alternative: Some(alternative),
    }
}

/// Orders the ways `request.entry` can run on `host` under `preferences`.
///
/// The list is in ladder order (see the module documentation) and is never
/// empty. Exactly one option is marked [`RouteOption::recommended`]: the
/// route the user pinned, when it is listed and not blocked; otherwise the
/// first option with a clear outlook; otherwise the first doubtful one;
/// otherwise the [`Route::NoRoute`] statement that leads the list when
/// nothing can work.
///
/// A Windows guest is never listed before the compatibility layer of the
/// same edition, pinned or not: the pin changes which option is
/// recommended, not the order.
pub fn route(
    request: &RouteRequest<'_>,
    host: &HostFacts,
    preferences: &Preferences,
) -> Vec<RouteOption> {
    let entry = request.entry;
    let pin = preferences.pinned_route(&entry.id);
    let mut options: Vec<RouteOption> = Vec::new();

    for (index, edition) in entry.editions.iter().enumerate() {
        match edition.kind {
            EditionKind::Flatpak => options.push(evaluate(
                Route::Flatpak,
                index,
                edition,
                host,
                "A Linux edition exists as a Flatpak: a native Linux edition comes first."
                    .to_owned(),
            )),
            EditionKind::LinuxNative => {
                if edition.obtain == Some(Obtain::OtherDistributionPackage) {
                    options.push(evaluate(
                        Route::DistroContainer,
                        index,
                        edition,
                        host,
                        "A Linux edition exists as a package of another distribution: it runs in a container of that distribution.".to_owned(),
                    ));
                } else {
                    options.push(evaluate(
                        Route::Native,
                        index,
                        edition,
                        host,
                        "A native Linux edition exists: a native Linux edition comes first."
                            .to_owned(),
                    ));
                }
            }
            EditionKind::Windows => {
                options.extend(windows_options(request, index, edition, host, preferences));
            }
            EditionKind::Android => options.push(evaluate(
                Route::AndroidRuntime,
                index,
                edition,
                host,
                "An Android edition exists: it runs in the Android runtime.".to_owned(),
            )),
            EditionKind::Web => {
                let reason = if edition.primary {
                    "The web application is the publisher's primary form of this application."
                } else if pin == Some(Route::Web) {
                    "A web edition exists, and you pinned it."
                } else {
                    "A web edition exists, and no better route is free of known obstacles."
                };
                options.push(evaluate(
                    Route::Web,
                    index,
                    edition,
                    host,
                    reason.to_owned(),
                ));
            }
            EditionKind::Macos => {
                if host.darling_enabled {
                    options.push(evaluate(
                        Route::Darling,
                        index,
                        edition,
                        host,
                        "A macOS edition exists and you have enabled Darling. It is listed last: an edition for Windows, Linux or the web is preferred.".to_owned(),
                    ));
                }
            }
        }
    }

    // A web edition that is not the publisher's primary form is offered only
    // when nothing better is free of known obstacles, or when it is pinned.
    let is_secondary_web = |option: &RouteOption| {
        option.route == Route::Web
            && !option
                .edition_index
                .and_then(|index| entry.editions.get(index))
                .is_some_and(|edition| edition.primary)
    };
    let better_exists = options.iter().any(|option| {
        option.outlook == Outlook::Clear
            && !matches!(option.route, Route::WindowsGuest | Route::Darling)
            && !is_secondary_web(option)
    });
    if better_exists && pin != Some(Route::Web) {
        options.retain(|option| !is_secondary_web(option));
    }

    options.sort_by_key(|option| ladder_key(option, entry, preferences));
    demote_clearly_worse(&mut options);

    let anything_usable = options
        .iter()
        .any(|option| option.outlook != Outlook::Blocked);
    if !anything_usable {
        let statement = no_route(entry, host, &options);
        options.insert(0, statement);
    }

    let mut pinned_position = None;
    if let Some(pinned_route) = pin {
        pinned_position = options
            .iter()
            .position(|option| option.route == pinned_route && option.outlook != Outlook::Blocked);
        match pinned_position {
            Some(position) => options[position].pinned = true,
            None => {
                let why =
                    if pinned_route.is_virtual_machine() && preferences.never_use_virtual_machine {
                        "you have also chosen never to use a virtual machine"
                    } else if options.iter().any(|option| option.route == pinned_route) {
                        "it cannot run this application on this machine"
                    } else {
                        "the rules do not offer it for this application on this machine"
                    };
                options[0].caveats.push(caveat(
                    CaveatCode::PinUnavailable,
                    Severity::Note,
                    format!(
                        "You pinned {} for this application; it is not used because {why}.",
                        pinned_route.label()
                    ),
                ));
            }
        }
    }

    let recommended_position = pinned_position
        .or_else(|| {
            options
                .iter()
                .position(|option| option.outlook == Outlook::Clear)
        })
        .or_else(|| {
            options
                .iter()
                .position(|option| option.outlook == Outlook::Doubtful)
        })
        .unwrap_or(0);
    options[recommended_position].recommended = true;
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntryKind, SCHEMA_VERSION};

    fn entry(category: Category, editions: Vec<Edition>) -> Entry {
        Entry {
            schema_version: SCHEMA_VERSION,
            id: "org.example.app".to_owned(),
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

    fn flatpak() -> Edition {
        let mut edition = Edition::new(EditionKind::Flatpak);
        edition.flatpak_id = Some("org.example.App".to_owned());
        edition
    }

    fn fedora() -> Edition {
        let mut edition = Edition::new(EditionKind::LinuxNative);
        edition.obtain = Some(Obtain::FedoraPackage);
        edition.package = Some("example".to_owned());
        edition
    }

    fn debian() -> Edition {
        let mut edition = Edition::new(EditionKind::LinuxNative);
        edition.obtain = Some(Obtain::OtherDistributionPackage);
        edition.distribution = Some("debian".to_owned());
        edition.package = Some("example".to_owned());
        edition
    }

    fn windows() -> Edition {
        let mut edition = Edition::new(EditionKind::Windows);
        edition.obtain = Some(Obtain::PublisherPage);
        edition.download_page = Some("https://example.org/download".to_owned());
        edition
    }

    fn android() -> Edition {
        let mut edition = Edition::new(EditionKind::Android);
        edition.package = Some("org.example.app".to_owned());
        edition.stores = vec![AndroidStore::FDroid];
        edition
    }

    fn web(primary: bool) -> Edition {
        let mut edition = Edition::new(EditionKind::Web);
        edition.url = Some("https://app.example.org/".to_owned());
        edition.primary = primary;
        edition
    }

    fn macos() -> Edition {
        let mut edition = Edition::new(EditionKind::Macos);
        edition.obtain = Some(Obtain::MacAppStore);
        edition.store_id = Some("123456".to_owned());
        edition
    }

    fn requiring(mut edition: Edition, requirements: &[Requirement]) -> Edition {
        edition.requirements = requirements.to_vec();
        edition
    }

    fn rated(mut edition: Edition, route: Route, rating: Rating) -> Edition {
        edition.records.push(Record {
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
        });
        edition
    }

    /// A machine with every runtime installed and enabled.
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

    fn run(entry: &Entry, host: &HostFacts, preferences: &Preferences) -> Vec<RouteOption> {
        route(&RouteRequest::new(entry), host, preferences)
    }

    fn routes(options: &[RouteOption]) -> Vec<Route> {
        options.iter().map(|option| option.route).collect()
    }

    fn pinned(route: Route) -> Preferences {
        let mut preferences = Preferences::default();
        preferences
            .pinned_routes
            .insert("org.example.app".to_owned(), route);
        preferences
    }

    fn no_vm() -> Preferences {
        Preferences {
            never_use_virtual_machine: true,
            ..Preferences::default()
        }
    }

    #[test]
    fn a_native_linux_edition_wins_when_one_exists() {
        let entry = entry(
            Category::Productivity,
            vec![
                macos(),
                web(true),
                android(),
                rated(windows(), Route::Wine, Rating::Good),
                debian(),
                fedora(),
                flatpak(),
            ],
        );
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(
            routes(&options),
            [
                Route::Flatpak,
                Route::Native,
                Route::Web,
                Route::DistroContainer,
                Route::Wine,
                Route::AndroidRuntime,
                Route::Darling,
            ]
        );
        assert!(options[0].recommended);
        assert_eq!(recommended(&options).route, Route::Flatpak);
        assert_eq!(options[0].outlook, Outlook::Clear);
    }

    #[test]
    fn a_native_linux_edition_loses_when_its_record_says_it_is_clearly_worse() {
        for native_rating in [Rating::Poor, Rating::Broken] {
            for other_rating in [Rating::Good, Rating::Usable] {
                let entry = entry(
                    Category::Productivity,
                    vec![
                        rated(flatpak(), Route::Flatpak, native_rating),
                        rated(windows(), Route::Wine, other_rating),
                        android(),
                    ],
                );
                let options = run(&entry, &full_host(), &Preferences::default());
                assert_eq!(
                    routes(&options),
                    [Route::Wine, Route::Flatpak, Route::AndroidRuntime]
                );
                assert_eq!(recommended(&options).route, Route::Wine);
                assert!(options[1].reason.contains("Listed after Wine"));
                assert_eq!(options[1].outlook, Outlook::Doubtful);
            }
        }
    }

    #[test]
    fn a_native_linux_edition_still_wins_when_it_is_not_clearly_worse() {
        // Usable against good is not "clearly worse"; neither is poor
        // against unknown, nor poor against poor.
        let cases = [
            (Some(Rating::Usable), Some(Rating::Good)),
            (Some(Rating::Poor), None),
            (Some(Rating::Poor), Some(Rating::Poor)),
            (None, Some(Rating::Good)),
        ];
        for (native_rating, wine_rating) in cases {
            let mut native = flatpak();
            if let Some(rating) = native_rating {
                native = rated(native, Route::Flatpak, rating);
            }
            let mut other = windows();
            if let Some(rating) = wine_rating {
                other = rated(other, Route::Wine, rating);
            }
            let entry = entry(Category::Productivity, vec![other, native]);
            let options = run(&entry, &full_host(), &Preferences::default());
            assert_eq!(options[0].route, Route::Flatpak);
        }
    }

    #[test]
    fn a_demoted_linux_edition_does_not_split_a_layer_from_its_guest() {
        let entry = entry(
            Category::Productivity,
            vec![
                rated(flatpak(), Route::Flatpak, Rating::Broken),
                rated(windows(), Route::Wine, Rating::Usable),
            ],
        );
        let request = RouteRequest {
            entry: &entry,
            offer_windows_guest: true,
        };
        let options = route(&request, &full_host(), &Preferences::default());
        assert_eq!(
            routes(&options),
            [Route::Wine, Route::WindowsGuest, Route::Flatpak]
        );
    }

    #[test]
    fn a_windows_application_gets_wine_and_no_guest_when_nothing_is_known_against_wine() {
        for rating in [None, Some(Rating::Good), Some(Rating::Usable)] {
            let mut edition = windows();
            if let Some(rating) = rating {
                edition = rated(edition, Route::Wine, rating);
            }
            let entry = entry(Category::Productivity, vec![edition]);
            let options = run(&entry, &full_host(), &Preferences::default());
            assert_eq!(routes(&options), [Route::Wine]);
            assert_eq!(options[0].outlook, Outlook::Clear);
            assert!(options[0].recommended);
        }
    }

    #[test]
    fn the_guest_is_offered_after_wine_when_wine_is_rated_poor_or_broken() {
        for rating in [Rating::Poor, Rating::Broken] {
            let entry = entry(
                Category::Productivity,
                vec![rated(windows(), Route::Wine, rating)],
            );
            let options = run(&entry, &full_host(), &Preferences::default());
            assert_eq!(routes(&options), [Route::Wine, Route::WindowsGuest]);
            assert_eq!(options[0].outlook, Outlook::Doubtful);
            assert_eq!(options[1].outlook, Outlook::Clear);
            // Wine is listed first; the guest is what TUR would use.
            assert_eq!(recommended(&options).route, Route::WindowsGuest);
            assert!(options[1].reason.contains("Wine is rated"));
            assert!(
                options[0]
                    .caveats
                    .iter()
                    .any(|caveat| caveat.code == CaveatCode::ReportedInadequate)
            );
        }
    }

    #[test]
    fn the_guest_is_offered_when_wine_is_unrated_and_has_a_known_obstacle() {
        for requirement in [Requirement::KernelDriver, Requirement::HardwareDongle] {
            let entry = entry(
                Category::Productivity,
                vec![requiring(windows(), &[requirement])],
            );
            let options = run(&entry, &full_host(), &Preferences::default());
            assert_eq!(routes(&options), [Route::Wine, Route::WindowsGuest]);
            assert_eq!(options[0].outlook, Outlook::Doubtful);
            assert_eq!(
                options[0].caveat_for(requirement).unwrap().severity,
                Severity::Obstacle
            );
            assert_eq!(recommended(&options).route, Route::WindowsGuest);

            // A record that says Wine runs it outweighs the inference.
            let entry = entry_with_rating(requirement, Rating::Usable);
            let options = run(&entry, &full_host(), &Preferences::default());
            assert_eq!(routes(&options), [Route::Wine]);
            assert_eq!(options[0].outlook, Outlook::Clear);
            assert!(options[0].caveat_for(requirement).is_some());
        }
    }

    fn entry_with_rating(requirement: Requirement, rating: Rating) -> Entry {
        entry(
            Category::Productivity,
            vec![rated(
                requiring(windows(), &[requirement]),
                Route::Wine,
                rating,
            )],
        )
    }

    #[test]
    fn the_guest_is_offered_when_the_user_asks() {
        let entry = entry(
            Category::Productivity,
            vec![rated(windows(), Route::Wine, Rating::Good)],
        );
        let request = RouteRequest {
            entry: &entry,
            offer_windows_guest: true,
        };
        let options = route(&request, &full_host(), &Preferences::default());
        assert_eq!(routes(&options), [Route::Wine, Route::WindowsGuest]);
        assert_eq!(recommended(&options).route, Route::Wine);
        assert!(options[1].reason.contains("you asked"));

        // Pinning the guest is asking for it. Wine is still listed first;
        // the pinned guest is the recommended option.
        let options = run(&entry, &full_host(), &pinned(Route::WindowsGuest));
        assert_eq!(routes(&options), [Route::Wine, Route::WindowsGuest]);
        assert!(options[1].pinned && options[1].recommended);
        assert!(!options[0].recommended);
    }

    #[test]
    fn the_guest_is_never_offered_when_virtual_machines_are_forbidden_and_the_loss_is_stated() {
        let poor = entry(
            Category::Productivity,
            vec![rated(windows(), Route::Wine, Rating::Poor)],
        );
        let options = run(&poor, &full_host(), &no_vm());
        assert_eq!(routes(&options), [Route::Wine]);
        let lost = options[0]
            .caveats
            .iter()
            .find(|caveat| caveat.code == CaveatCode::GuestNotOffered)
            .unwrap();
        assert!(lost.text.contains("What is lost"));
        assert!(lost.text.contains("never to use a virtual machine"));

        // Asking, or pinning, does not override the preference.
        let good = entry(
            Category::Productivity,
            vec![rated(windows(), Route::Wine, Rating::Good)],
        );
        let request = RouteRequest {
            entry: &good,
            offer_windows_guest: true,
        };
        let options = route(&request, &full_host(), &no_vm());
        assert_eq!(routes(&options), [Route::Wine]);

        let mut preferences = pinned(Route::WindowsGuest);
        preferences.never_use_virtual_machine = true;
        let options = run(&good, &full_host(), &preferences);
        assert_eq!(routes(&options), [Route::Wine]);
        assert!(options[0].recommended && !options[0].pinned);
        let note = options[0]
            .caveats
            .iter()
            .find(|caveat| caveat.code == CaveatCode::PinUnavailable)
            .unwrap();
        assert!(note.text.contains("never to use a virtual machine"));

        // Nothing is said about a guest when none would have been offered.
        let options = run(&good, &full_host(), &no_vm());
        assert!(
            options[0]
                .caveats
                .iter()
                .all(|caveat| caveat.code != CaveatCode::GuestNotOffered)
        );
    }

    #[test]
    fn the_guest_states_its_memory_and_licence_needs() {
        let entry = entry(
            Category::Productivity,
            vec![rated(windows(), Route::Wine, Rating::Poor)],
        );
        let mut host = full_host();
        host.windows_guest = false;
        host.windows_licence = false;
        host.memory_mib = Some(8 * 1024);
        let options = run(&entry, &host, &Preferences::default());
        let guest = &options[1];
        assert_eq!(guest.route, Route::WindowsGuest);
        let codes: Vec<PrerequisiteCode> = guest
            .prerequisites
            .iter()
            .map(|prerequisite| prerequisite.code)
            .collect();
        assert_eq!(
            codes,
            [
                PrerequisiteCode::SetUpWindowsGuest,
                PrerequisiteCode::ProvideWindowsLicence
            ]
        );
        let memory = guest
            .caveats
            .iter()
            .find(|caveat| caveat.code == CaveatCode::GuestMemory)
            .unwrap();
        assert_eq!(memory.severity, Severity::Obstacle);
        assert!(memory.text.contains("about 16 GB of memory"));
        assert!(memory.text.contains("about 8 GB"));
        assert_eq!(guest.outlook, Outlook::Doubtful);

        host.memory_mib = None;
        let options = run(&entry, &host, &Preferences::default());
        let memory = options[1]
            .caveats
            .iter()
            .find(|caveat| caveat.code == CaveatCode::GuestMemory)
            .unwrap();
        assert_eq!(memory.severity, Severity::Warning);
        assert_eq!(options[1].outlook, Outlook::Clear);
    }

    #[test]
    fn a_game_gets_proton_first_and_never_the_guest() {
        for rating in [
            None,
            Some(Rating::Good),
            Some(Rating::Usable),
            Some(Rating::Poor),
            Some(Rating::Broken),
        ] {
            let mut edition = requiring(windows(), &[Requirement::KernelDriver]);
            if let Some(rating) = rating {
                edition = rated(edition, Route::Proton, rating);
            }
            let entry = entry(Category::Game, vec![edition]);
            let request = RouteRequest {
                entry: &entry,
                offer_windows_guest: true,
            };
            for preferences in [Preferences::default(), pinned(Route::WindowsGuest)] {
                let options = route(&request, &full_host(), &preferences);
                assert_eq!(routes(&options), [Route::Proton]);
                assert!(options[0].recommended);
            }
        }
        let poor = entry(
            Category::Game,
            vec![rated(windows(), Route::Proton, Rating::Poor)],
        );
        let options = run(&poor, &full_host(), &Preferences::default());
        assert!(
            options[0]
                .caveats
                .iter()
                .any(|caveat| caveat.code == CaveatCode::GuestUnsuitableForGames)
        );
    }

    #[test]
    fn a_game_with_kernel_anti_cheat_has_no_route_and_dual_boot_is_stated() {
        let entry = entry(
            Category::Game,
            vec![requiring(windows(), &[Requirement::KernelAntiCheat])],
        );
        for preferences in [
            Preferences::default(),
            no_vm(),
            pinned(Route::Proton),
            pinned(Route::WindowsGuest),
        ] {
            let options = run(&entry, &full_host(), &preferences);
            assert_eq!(routes(&options), [Route::NoRoute, Route::Proton]);
            let statement = &options[0];
            assert!(statement.recommended);
            assert_eq!(
                statement.alternative.as_ref().unwrap().kind,
                AlternativeKind::DualBoot
            );
            assert!(statement.reason.contains("anti-cheat will not work"));
            assert!(statement.reason.contains("Dual boot is the only way"));
            assert!(
                statement
                    .caveats
                    .iter()
                    .any(|caveat| caveat.severity == Severity::Blocker)
            );
            let proton = &options[1];
            assert_eq!(proton.outlook, Outlook::Blocked);
            assert!(!proton.recommended && !proton.pinned);
            let blocker = proton.caveat_for(Requirement::KernelAntiCheat).unwrap();
            assert_eq!(blocker.severity, Severity::Blocker);
            assert!(
                blocker
                    .text
                    .starts_with("This game's anti-cheat will not work")
            );
        }
    }

    #[test]
    fn an_android_application_runs_in_the_android_runtime_with_its_caveats() {
        let plain = entry(Category::Utility, vec![android()]);
        let options = run(&plain, &full_host(), &Preferences::default());
        assert_eq!(routes(&options), [Route::AndroidRuntime]);
        assert_eq!(options[0].outlook, Outlook::Clear);
        assert!(options[0].caveats.is_empty());

        // Play Integrity: listed, blocked, with the caveat; no route.
        let bank = entry(
            Category::Finance,
            vec![requiring(android(), &[Requirement::PlayIntegrity])],
        );
        let options = run(&bank, &full_host(), &Preferences::default());
        assert_eq!(routes(&options), [Route::NoRoute, Route::AndroidRuntime]);
        let integrity = options[1].caveat_for(Requirement::PlayIntegrity).unwrap();
        assert_eq!(integrity.severity, Severity::Blocker);
        assert!(integrity.text.contains("certified by Google"));
        assert!(integrity.text.contains("will refuse to run"));
        assert_eq!(
            options[0].alternative.as_ref().unwrap().kind,
            AlternativeKind::CertifiedAndroidDevice
        );

        // ARM-only code: blocked on x86-64 as shipped; a warning with a
        // translation layer supplied by the user; a note on an ARM machine.
        let arm = entry(
            Category::Game,
            vec![requiring(android(), &[Requirement::ArmOnlyNativeCode])],
        );
        let options = run(&arm, &full_host(), &Preferences::default());
        assert_eq!(routes(&options), [Route::NoRoute, Route::AndroidRuntime]);
        let caveat = options[1]
            .caveat_for(Requirement::ArmOnlyNativeCode)
            .unwrap();
        assert_eq!(caveat.severity, Severity::Blocker);

        let mut host = full_host();
        host.arm_translation = true;
        let options = run(&arm, &host, &Preferences::default());
        assert_eq!(routes(&options), [Route::AndroidRuntime]);
        let caveat = options[0]
            .caveat_for(Requirement::ArmOnlyNativeCode)
            .unwrap();
        assert_eq!(caveat.severity, Severity::Warning);
        assert_eq!(options[0].outlook, Outlook::Clear);

        let mut host = full_host();
        host.arch = Arch::Aarch64;
        let options = run(&arm, &host, &Preferences::default());
        assert_eq!(routes(&options), [Route::AndroidRuntime]);
        let caveat = options[0]
            .caveat_for(Requirement::ArmOnlyNativeCode)
            .unwrap();
        assert_eq!(caveat.severity, Severity::Note);
    }

    #[test]
    fn an_android_application_states_what_must_be_enabled_first() {
        let mut edition = android();
        edition.stores = vec![AndroidStore::GooglePlay];
        let entry = entry(Category::Utility, vec![edition]);
        let options = run(&entry, &HostFacts::default(), &Preferences::default());
        let codes: Vec<PrerequisiteCode> = options[0]
            .prerequisites
            .iter()
            .map(|prerequisite| prerequisite.code)
            .collect();
        assert_eq!(
            codes,
            [
                PrerequisiteCode::EnableAndroidRuntime,
                PrerequisiteCode::OptIntoGooglePlay
            ]
        );
        let options = run(&entry, &full_host(), &Preferences::default());
        assert!(options[0].prerequisites.is_empty());
    }

    #[test]
    fn a_macos_only_application_has_no_route_unless_darling_is_enabled() {
        let entry = entry(Category::Creative, vec![macos()]);
        let mut host = full_host();
        host.darling_enabled = false;
        let options = run(&entry, &host, &Preferences::default());
        assert_eq!(routes(&options), [Route::NoRoute]);
        assert!(options[0].reason.contains("Darling"));
        assert!(options[0].reason.contains("Apple's licence"));
        assert_eq!(
            options[0].alternative.as_ref().unwrap().kind,
            AlternativeKind::Mac
        );

        // Enabled: offered, labelled experimental, doubtful.
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(routes(&options), [Route::Darling]);
        let experimental = options[0]
            .caveats
            .iter()
            .find(|caveat| caveat.code == CaveatCode::Experimental)
            .unwrap();
        assert!(experimental.text.starts_with("Experimental"));
        assert_eq!(options[0].outlook, Outlook::Doubtful);
        assert!(options[0].recommended);
    }

    #[test]
    fn a_macos_application_prefers_its_windows_linux_or_web_edition() {
        for other in [windows(), flatpak(), web(false), web(true), android()] {
            let other_kind = other.kind;
            let entry = entry(Category::Creative, vec![macos(), other]);
            let options = run(&entry, &full_host(), &Preferences::default());
            assert_eq!(options.len(), 2);
            assert_eq!(options[0].edition_kind, Some(other_kind));
            assert_eq!(options[1].route, Route::Darling);
            assert!(options[0].recommended);

            // Without Darling the other edition is the only option, and
            // Darling does not appear.
            let mut host = full_host();
            host.darling_enabled = false;
            let options = run(&entry, &host, &Preferences::default());
            assert_eq!(options.len(), 1);
            assert_eq!(options[0].edition_kind, Some(other_kind));
        }
    }

    #[test]
    fn a_web_edition_is_offered_when_nothing_better_exists() {
        // Alone.
        let only = entry(Category::Productivity, vec![web(false)]);
        let options = run(&only, &full_host(), &Preferences::default());
        assert_eq!(routes(&options), [Route::Web]);
        assert!(options[0].prerequisites.is_empty());

        // Beside a route with a clear outlook: not offered.
        for better in [flatpak(), fedora(), debian(), windows(), android()] {
            let entry = entry(Category::Productivity, vec![web(false), better]);
            let options = run(&entry, &full_host(), &Preferences::default());
            assert_eq!(options.len(), 1);
            assert_ne!(options[0].route, Route::Web);
        }

        // Beside routes that are doubtful or blocked: offered, recommended.
        let entry = entry(
            Category::Media,
            vec![
                web(false),
                requiring(android(), &[Requirement::PlayIntegrity]),
                rated(windows(), Route::Wine, Rating::Broken),
            ],
        );
        let options = run(&entry, &full_host(), &no_vm());
        assert_eq!(
            routes(&options),
            [Route::Wine, Route::Web, Route::AndroidRuntime]
        );
        assert_eq!(recommended(&options).route, Route::Web);

        // A Windows guest does not count as "better": the web edition stays.
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(
            routes(&options),
            [
                Route::Wine,
                Route::WindowsGuest,
                Route::Web,
                Route::AndroidRuntime
            ]
        );
        assert_eq!(recommended(&options).route, Route::WindowsGuest);
    }

    #[test]
    fn a_primary_web_edition_is_offered_alongside() {
        let entry = entry(
            Category::Productivity,
            vec![android(), windows(), web(true), flatpak()],
        );
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(
            routes(&options),
            [
                Route::Flatpak,
                Route::Web,
                Route::Wine,
                Route::AndroidRuntime
            ]
        );
        assert!(options[1].reason.contains("primary form"));

        // Without a Linux edition the primary web edition leads.
        let entry = entry_without_linux();
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(routes(&options), [Route::Web, Route::Wine]);
        assert!(options[0].recommended);
    }

    fn entry_without_linux() -> Entry {
        entry(Category::Productivity, vec![windows(), web(true)])
    }

    #[test]
    fn a_pinned_web_edition_is_offered_even_beside_a_better_route() {
        let entry = entry(Category::Productivity, vec![web(false), flatpak()]);
        let options = run(&entry, &full_host(), &pinned(Route::Web));
        assert_eq!(routes(&options), [Route::Flatpak, Route::Web]);
        assert!(options[1].pinned && options[1].recommended);
        assert!(options[1].reason.contains("you pinned it"));
    }

    #[test]
    fn nothing_workable_gives_an_explicit_no_route_never_an_empty_list() {
        // No editions at all.
        let empty = entry(Category::Utility, Vec::new());
        let options = run(&empty, &full_host(), &Preferences::default());
        assert_eq!(routes(&options), [Route::NoRoute]);
        assert!(options[0].recommended);
        assert!(options[0].reason.contains("no edition"));
        assert_eq!(
            options[0].alternative.as_ref().unwrap().kind,
            AlternativeKind::NoneKnown
        );

        // A Windows application on a machine that cannot run x86-64 code.
        let entry = entry(Category::Productivity, vec![windows()]);
        let mut host = full_host();
        host.arch = Arch::Aarch64;
        let options = run(&entry, &host, &Preferences::default());
        assert_eq!(routes(&options), [Route::NoRoute, Route::Wine]);
        assert_eq!(
            options[0].alternative.as_ref().unwrap().kind,
            AlternativeKind::AnotherComputer
        );
        assert!(!options[0].reason.is_empty());
        assert_eq!(options[1].worst_severity(), Some(Severity::Blocker));
    }

    #[test]
    fn a_blocked_route_is_listed_last_and_never_recommended() {
        let entry = entry(
            Category::Media,
            vec![
                requiring(android(), &[Requirement::PlayIntegrity]),
                web(false),
            ],
        );
        let options = run(&entry, &full_host(), &pinned(Route::AndroidRuntime));
        assert_eq!(routes(&options), [Route::Web, Route::AndroidRuntime]);
        assert!(options[0].recommended);
        assert_eq!(options[1].outlook, Outlook::Blocked);
        assert!(!options[1].pinned);
        assert!(
            options[0]
                .caveats
                .iter()
                .any(|caveat| caveat.code == CaveatCode::PinUnavailable)
        );
    }

    #[test]
    fn prefer_open_source_orders_editions_within_a_rung_only() {
        let mut open = fedora();
        open.licence = Some(Licence::OpenSource);
        let mut closed = flatpak();
        closed.licence = Some(Licence::Proprietary);
        let mut open_windows = windows();
        open_windows.licence = Some(Licence::OpenSource);
        let entry = entry(Category::Utility, vec![closed, open, open_windows]);

        let plain = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(routes(&plain), [Route::Flatpak, Route::Native, Route::Wine]);

        let preferences = Preferences {
            prefer_open_source: true,
            ..Preferences::default()
        };
        let preferred = run(&entry, &full_host(), &preferences);
        // The open-source native package moves ahead of the proprietary
        // Flatpak; the open-source Windows edition does not climb a rung.
        assert_eq!(
            routes(&preferred),
            [Route::Native, Route::Flatpak, Route::Wine]
        );
    }

    #[test]
    fn a_pinned_route_is_recommended_without_changing_the_order() {
        let entry = entry(
            Category::Utility,
            vec![flatpak(), fedora(), windows(), android()],
        );
        let unpinned = run(&entry, &full_host(), &Preferences::default());
        for pin in [
            Route::Flatpak,
            Route::Native,
            Route::Wine,
            Route::AndroidRuntime,
        ] {
            let options = run(&entry, &full_host(), &pinned(pin));
            assert_eq!(routes(&options), routes(&unpinned));
            let chosen = recommended(&options);
            assert_eq!(chosen.route, pin);
            assert!(chosen.pinned);
        }
        // A pin that does not apply is reported and ignored.
        let options = run(&entry, &full_host(), &pinned(Route::Proton));
        assert_eq!(recommended(&options).route, Route::Flatpak);
        assert!(
            options[0]
                .caveats
                .iter()
                .any(|caveat| caveat.code == CaveatCode::PinUnavailable)
        );
        assert!(options.iter().all(|option| !option.pinned));
    }

    #[test]
    fn prerequisites_name_what_is_not_installed() {
        let entry = entry(
            Category::Utility,
            vec![flatpak(), debian(), windows(), fedora()],
        );
        let host = HostFacts {
            flatpak: false,
            ..HostFacts::default()
        };
        let options = run(&entry, &host, &Preferences::default());
        let needs: Vec<(Route, Vec<PrerequisiteCode>)> = options
            .iter()
            .map(|option| {
                (
                    option.route,
                    option
                        .prerequisites
                        .iter()
                        .map(|prerequisite| prerequisite.code)
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            needs,
            [
                (Route::Flatpak, vec![PrerequisiteCode::InstallFlatpak]),
                (Route::Native, Vec::new()),
                (
                    Route::DistroContainer,
                    vec![PrerequisiteCode::InstallContainerTool]
                ),
                (Route::Wine, vec![PrerequisiteCode::InstallWine]),
            ]
        );
        let game = super::tests::entry(Category::Game, vec![windows()]);
        let options = run(&game, &host, &Preferences::default());
        assert_eq!(
            options[0].prerequisites[0].code,
            PrerequisiteCode::InstallProton
        );
        assert!(
            options[0].prerequisites[0]
                .text
                .contains("does not ship Proton")
        );
        let options = run(&entry, &full_host(), &Preferences::default());
        assert!(options.iter().all(|option| option.prerequisites.is_empty()));
    }

    #[test]
    fn the_rating_and_its_source_are_carried_and_the_best_record_is_used() {
        let mut edition = windows();
        edition = rated(edition, Route::Wine, Rating::Poor);
        edition.records[0].date = "2025-01-01".to_owned();
        edition = rated(edition, Route::Wine, Rating::Usable);
        // A record for another route of the same edition is not used.
        edition = rated(edition, Route::WindowsGuest, Rating::Broken);
        let entry = entry(Category::Productivity, vec![edition.clone()]);
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(options[0].rating, Rating::Usable);
        match &options[0].rating_source {
            RatingSource::External {
                name,
                url,
                source_rating,
                date,
                ..
            } => {
                assert_eq!(name, "Example DB");
                assert_eq!(url, "https://db.example.org/app");
                assert_eq!(source_rating.as_deref(), Some("Tier"));
                assert_eq!(date, "2026-11-12");
            }
            other => panic!("unexpected source {other:?}"),
        }

        // TUR's own test outranks a newer external report.
        edition.records.push(Record {
            route: Route::Wine,
            rating: Rating::Good,
            reported_by: Reporter::TurTest,
            source_name: None,
            source_url: None,
            source_rating: None,
            test_report: Some("docs/reports/compat/example.md".to_owned()),
            version_tested: Some("1.0".to_owned()),
            date: "2024-06-01".to_owned(),
            notes: None,
        });
        let entry = super::tests::entry(Category::Productivity, vec![edition]);
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(options[0].rating, Rating::Good);
        assert!(matches!(
            options[0].rating_source,
            RatingSource::TurTest { .. }
        ));

        // No record at all: unknown, unrated.
        let entry = super::tests::entry(Category::Productivity, vec![windows()]);
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(options[0].rating, Rating::Unknown);
        assert_eq!(options[0].rating_source, RatingSource::Unrated);
    }

    #[test]
    fn a_closed_store_edition_is_an_obstacle_for_a_layer_and_a_note_for_the_guest() {
        let mut edition = Edition::new(EditionKind::Windows);
        edition.obtain = Some(Obtain::MicrosoftStore);
        edition.store_id = Some("9NBLGGH4NNS1".to_owned());
        let entry = entry(Category::Productivity, vec![edition]);
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(routes(&options), [Route::Wine, Route::WindowsGuest]);
        let on_wine = options[0]
            .caveats
            .iter()
            .find(|caveat| caveat.code == CaveatCode::ClosedStore)
            .unwrap();
        assert_eq!(on_wine.severity, Severity::Obstacle);
        let in_guest = options[1]
            .caveats
            .iter()
            .find(|caveat| caveat.code == CaveatCode::ClosedStore)
            .unwrap();
        assert_eq!(in_guest.severity, Severity::Note);
    }

    #[test]
    fn a_community_package_is_noted() {
        let mut edition = flatpak();
        edition.packaging = Some(Packaging::Community);
        let entry = entry(Category::Utility, vec![edition]);
        let options = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(options[0].caveats[0].code, CaveatCode::CommunityPackage);
        assert_eq!(options[0].outlook, Outlook::Clear);
    }

    #[test]
    fn heavy_gpu_use_depends_on_the_machine_and_rules_out_the_guest_as_a_remedy() {
        let entry = entry(
            Category::Creative,
            vec![requiring(windows(), &[Requirement::HeavyGpu])],
        );
        for (gpu, severity, outlook) in [
            (GpuClass::None, Severity::Obstacle, Outlook::Doubtful),
            (GpuClass::Integrated, Severity::Warning, Outlook::Clear),
            (GpuClass::Discrete, Severity::Note, Outlook::Clear),
            (GpuClass::Unknown, Severity::Note, Outlook::Clear),
        ] {
            let mut host = full_host();
            host.gpu = gpu;
            let options = run(&entry, &host, &Preferences::default());
            // No guest: it has no usable GPU acceleration either.
            assert_eq!(routes(&options), [Route::Wine]);
            assert_eq!(
                options[0]
                    .caveat_for(Requirement::HeavyGpu)
                    .unwrap()
                    .severity,
                severity
            );
            assert_eq!(options[0].outlook, outlook);
        }
        // When the guest is offered for another reason, it says so.
        let request = RouteRequest {
            entry: &entry,
            offer_windows_guest: true,
        };
        let options = route(&request, &full_host(), &Preferences::default());
        let guest = &options[1];
        assert_eq!(
            guest.caveat_for(Requirement::HeavyGpu).unwrap().severity,
            Severity::Obstacle
        );
        assert_eq!(guest.outlook, Outlook::Doubtful);
        assert_eq!(recommended(&options).route, Route::Wine);
    }

    #[test]
    fn the_router_source_has_no_process_file_or_network_access() {
        // The router never launches or installs anything. It could not: this
        // file uses nothing that starts a process, opens a file or a socket.
        let source = include_str!("route.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        for forbidden in [
            concat!("std::", "process"),
            concat!("std::", "fs"),
            concat!("std::", "net"),
            concat!("Comm", "and::"),
            concat!("std::", "env"),
            concat!("un", "safe "),
        ] {
            assert!(
                !production.contains(forbidden),
                "route.rs must not use {forbidden}"
            );
        }
    }

    #[test]
    fn routing_is_deterministic() {
        let entry = entry(
            Category::Productivity,
            vec![
                macos(),
                web(false),
                android(),
                rated(windows(), Route::Wine, Rating::Poor),
                debian(),
                flatpak(),
            ],
        );
        let first = run(&entry, &full_host(), &Preferences::default());
        let second = run(&entry, &full_host(), &Preferences::default());
        assert_eq!(first, second);
        let json = serde_json::to_string(&first).unwrap();
        assert!(json.contains("\"route\":\"flatpak\""));
        assert!(json.contains("\"kind\":\"external\""));
    }
}
