// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The catalogue entry: one application, the editions it exists in, what each
//! edition requires, and what is known about how well each route runs it.
//!
//! The types here are the Rust form of
//! `schemas/app-catalogue-entry.schema.json`. An entry is written as one TOML
//! file. Unknown fields are refused at every level, so a misspelt field is an
//! error and not a silently ignored fact.
//!
//! Parsing only checks shape. The rules across fields, and the honesty rule
//! for ratings, are in [`crate::validate`].

use serde::{Deserialize, Serialize};

/// The version of the entry format this crate reads.
pub const SCHEMA_VERSION: u32 = 1;

/// One application, or one class of applications, in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The version of the entry format. Must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The stable application ID: lower case, reverse-DNS style, at least
    /// two segments. It never changes once published.
    pub id: String,
    /// Whether the entry describes one application or a class of them.
    #[serde(default)]
    pub entry_kind: EntryKind,
    /// The name shown to the user.
    pub name: String,
    /// Other names people search for.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Who publishes the application.
    pub publisher: String,
    /// What kind of application it is. Games are routed differently.
    pub category: Category,
    /// One line saying what the application is.
    #[serde(default)]
    pub summary: Option<String>,
    /// The publisher's page about the application.
    #[serde(default)]
    pub homepage: Option<String>,
    /// The ways the application exists. At least one.
    pub editions: Vec<Edition>,
}

/// Whether an entry is one application or a class of applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind {
    /// One application with its own identifiers.
    #[default]
    Application,
    /// A class of applications that share a routing answer, such as banking
    /// applications for Android. Its editions carry no identifiers.
    Class,
}

/// The category of an application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// A game. Games use Proton and are never offered a Windows guest.
    Game,
    /// Office suites, notes, documents.
    Productivity,
    /// Image, video, audio and 3D creation.
    Creative,
    /// Chat, calls, mail.
    Communication,
    /// Programming tools.
    Development,
    /// Small tools: archivers, system utilities.
    Utility,
    /// Players and streaming services.
    Media,
    /// Web browsers and other internet clients.
    Internet,
    /// Banking, payment and wallet applications.
    Finance,
    /// Learning and reference.
    Education,
}

impl Category {
    /// Every category, in schema order.
    pub const ALL: [Category; 10] = [
        Category::Game,
        Category::Productivity,
        Category::Creative,
        Category::Communication,
        Category::Development,
        Category::Utility,
        Category::Media,
        Category::Internet,
        Category::Finance,
        Category::Education,
    ];

    /// The category as written in an entry.
    pub const fn as_str(self) -> &'static str {
        match self {
            Category::Game => "game",
            Category::Productivity => "productivity",
            Category::Creative => "creative",
            Category::Communication => "communication",
            Category::Development => "development",
            Category::Utility => "utility",
            Category::Media => "media",
            Category::Internet => "internet",
            Category::Finance => "finance",
            Category::Education => "education",
        }
    }
}

/// One way an application exists.
///
/// Which fields apply depends on [`Edition::kind`]; the validator refuses a
/// field that its kind does not use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    /// What kind of edition this is.
    pub kind: EditionKind,
    /// Flatpak: the Flatpak application ID.
    #[serde(default)]
    pub flatpak_id: Option<String>,
    /// Flatpak: the remote that carries it. `flathub` when absent.
    #[serde(default)]
    pub remote: Option<String>,
    /// Native Linux, Windows, macOS: where the edition is obtained.
    #[serde(default)]
    pub obtain: Option<Obtain>,
    /// Native Linux: the package name. Android: the package name.
    #[serde(default)]
    pub package: Option<String>,
    /// Native Linux from another distribution: which distribution.
    #[serde(default)]
    pub distribution: Option<String>,
    /// The identifier in the store named by `obtain`, such as a Steam app ID.
    #[serde(default)]
    pub store_id: Option<String>,
    /// The publisher's download page. A page for a person to read, never a
    /// direct link to a program or archive.
    #[serde(default)]
    pub download_page: Option<String>,
    /// Windows, from the publisher's page: the publisher's own installer of
    /// a freely redistributable application, pinned by its SHA-256, which
    /// the application centre may download when the user asks for the
    /// application (ADR-0019). Never present for proprietary software.
    #[serde(default)]
    pub installer: Option<Installer>,
    /// Android: the stores that carry the package.
    #[serde(default)]
    pub stores: Vec<AndroidStore>,
    /// Web: the address of the web application.
    #[serde(default)]
    pub url: Option<String>,
    /// Web: whether the web application is the publisher's primary form.
    #[serde(default)]
    pub primary: bool,
    /// Whether the edition is open source or proprietary, when known.
    #[serde(default)]
    pub licence: Option<Licence>,
    /// Flatpak and native Linux: who maintains the package, when known.
    #[serde(default)]
    pub packaging: Option<Packaging>,
    /// The known requirements that decide routing.
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    /// What is known about how well each route runs this edition.
    #[serde(default)]
    pub records: Vec<Record>,
    /// The public pages the facts of this edition were checked against.
    #[serde(default)]
    pub references: Vec<Reference>,
    /// Free-text notes about the edition.
    #[serde(default)]
    pub notes: Option<String>,
}

impl Edition {
    /// An edition of the given kind with every other field empty.
    pub fn new(kind: EditionKind) -> Self {
        Edition {
            kind,
            flatpak_id: None,
            remote: None,
            obtain: None,
            package: None,
            distribution: None,
            store_id: None,
            download_page: None,
            installer: None,
            stores: Vec::new(),
            url: None,
            primary: false,
            licence: None,
            packaging: None,
            requirements: Vec::new(),
            records: Vec::new(),
            references: Vec::new(),
            notes: None,
        }
    }

    /// The routes that can run this edition. A compatibility record must
    /// name one of them.
    pub fn routes(&self) -> &'static [Route] {
        match self.kind {
            EditionKind::Flatpak => &[Route::Flatpak],
            EditionKind::LinuxNative => {
                if self.obtain == Some(Obtain::OtherDistributionPackage) {
                    &[Route::DistroContainer]
                } else {
                    &[Route::Native]
                }
            }
            EditionKind::Windows => &[Route::Proton, Route::Wine, Route::WindowsGuest],
            EditionKind::Android => &[Route::AndroidRuntime],
            EditionKind::Web => &[Route::Web],
            EditionKind::Macos => &[Route::Darling],
        }
    }

    /// Whether the edition lists `requirement`.
    pub fn requires(&self, requirement: Requirement) -> bool {
        self.requirements.contains(&requirement)
    }

    /// The edition and where it comes from, in a few words, for example
    /// `Windows edition from Steam (1245620)`.
    pub fn describe(&self) -> String {
        let source = match self.kind {
            EditionKind::Flatpak => {
                let id = self.flatpak_id.as_deref().unwrap_or("no application ID");
                let remote = self.remote.as_deref().unwrap_or("flathub");
                format!("{id} from {remote}")
            }
            EditionKind::Android => {
                let package = self.package.as_deref().unwrap_or("no package name");
                let stores: Vec<&str> = self.stores.iter().map(|store| store.label()).collect();
                if stores.is_empty() {
                    package.to_owned()
                } else {
                    format!("{package} from {}", stores.join(", "))
                }
            }
            EditionKind::Web => self.url.clone().unwrap_or_else(|| "no address".to_owned()),
            EditionKind::LinuxNative | EditionKind::Windows | EditionKind::Macos => {
                self.describe_obtain()
            }
        };
        format!("{} edition: {source}", self.kind.label())
    }

    fn describe_obtain(&self) -> String {
        let Some(obtain) = self.obtain else {
            return "source not recorded".to_owned();
        };
        match obtain {
            Obtain::FedoraPackage => {
                format!("Fedora package {}", self.package.as_deref().unwrap_or("?"))
            }
            Obtain::OtherDistributionPackage => format!(
                "{} package {}",
                self.distribution
                    .as_deref()
                    .unwrap_or("another distribution's"),
                self.package.as_deref().unwrap_or("?")
            ),
            Obtain::PublisherPage => format!(
                "the publisher's page {}",
                self.download_page.as_deref().unwrap_or("(not recorded)")
            ),
            Obtain::Steam
            | Obtain::Epic
            | Obtain::Gog
            | Obtain::MicrosoftStore
            | Obtain::MacAppStore => match self.store_id.as_deref() {
                Some(id) => format!("{} ({id})", obtain.label()),
                None => obtain.label().to_owned(),
            },
        }
    }
}

/// The publisher's own installer of a freely redistributable Windows
/// application: where it is, and what it must be.
///
/// This is the one place the catalogue names a file to download. It exists
/// so that "Install" can work for free software without the user fetching a
/// file by hand. The file is still fetched only when the user asks for the
/// application, and nothing in it runs before its SHA-256 has been checked
/// against [`Installer::sha256`] (ADR-0019). An edition that is not open
/// source may not carry one: the validator refuses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installer {
    /// The publisher's own download location of the file: an `https` URL
    /// that ends in the file's name.
    pub url: String,
    /// The SHA-256 of the file, as 64 lower-case hexadecimal digits.
    pub sha256: String,
    /// The size of the file in bytes.
    pub size: u64,
    /// The version of the application the file installs.
    pub version: String,
    /// The licence the application is published under, as its publisher
    /// states it, for example `MIT` or `GPL-3.0-or-later`.
    pub licence: String,
    /// The page the licence was read on.
    pub licence_url: String,
    /// Where the SHA-256 was taken from: the publisher's checksum file, or
    /// the digest its release page publishes.
    pub hash_source: String,
    /// The day the location, the digest and the licence were checked,
    /// `YYYY-MM-DD`.
    pub checked: String,
    /// The file is the application itself, a single program, and not an
    /// installer: it is copied into place and never run to install.
    #[serde(default)]
    pub portable: bool,
}

impl Installer {
    /// The name of the file: the last segment of the URL's path, without
    /// its query or fragment. Empty when the URL has no such segment.
    pub fn file_name(&self) -> &str {
        let without_scheme = self.url.strip_prefix("https://").unwrap_or(&self.url);
        let path_start = without_scheme.find('/').unwrap_or(without_scheme.len());
        let path = &without_scheme[path_start..];
        let path_end = path.find(['?', '#']).unwrap_or(path.len());
        let path = &path[..path_end];
        path.rsplit('/').next().unwrap_or("")
    }

    /// The host the file is downloaded from, without a port. Empty when
    /// the URL is not an `https` URL.
    pub fn host(&self) -> &str {
        let Some(rest) = self.url.strip_prefix("https://") else {
            return "";
        };
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        authority.split(':').next().unwrap_or("")
    }
}

/// The kind of an edition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditionKind {
    /// A Linux edition packaged as a Flatpak.
    Flatpak,
    /// A Linux edition that is not a Flatpak: a distribution package, a
    /// native build in a store, or the publisher's own Linux build.
    LinuxNative,
    /// A Windows edition.
    Windows,
    /// An Android edition.
    Android,
    /// A web application.
    Web,
    /// A macOS edition.
    Macos,
}

impl EditionKind {
    /// Every edition kind, in schema order.
    pub const ALL: [EditionKind; 6] = [
        EditionKind::Flatpak,
        EditionKind::LinuxNative,
        EditionKind::Windows,
        EditionKind::Android,
        EditionKind::Web,
        EditionKind::Macos,
    ];

    /// The kind as written in an entry.
    pub const fn as_str(self) -> &'static str {
        match self {
            EditionKind::Flatpak => "flatpak",
            EditionKind::LinuxNative => "linux-native",
            EditionKind::Windows => "windows",
            EditionKind::Android => "android",
            EditionKind::Web => "web",
            EditionKind::Macos => "macos",
        }
    }

    /// The kind as the start of a phrase, for example `Windows`.
    pub const fn label(self) -> &'static str {
        match self {
            EditionKind::Flatpak => "Flatpak",
            EditionKind::LinuxNative => "Native Linux",
            EditionKind::Windows => "Windows",
            EditionKind::Android => "Android",
            EditionKind::Web => "Web",
            EditionKind::Macos => "macOS",
        }
    }
}

/// Where a native Linux, Windows or macOS edition is obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Obtain {
    /// A package of the base system's distribution.
    FedoraPackage,
    /// A package of another distribution, which runs in a container of it.
    OtherDistributionPackage,
    /// Steam.
    Steam,
    /// The Epic Games Store.
    Epic,
    /// GOG.
    Gog,
    /// The publisher's own download page.
    PublisherPage,
    /// The Microsoft Store, which TUR cannot integrate (ADR-0011, rule 5).
    MicrosoftStore,
    /// Apple's App Store, which TUR cannot integrate (ADR-0011, rule 5).
    MacAppStore,
}

impl Obtain {
    /// Every source, in schema order.
    pub const ALL: [Obtain; 8] = [
        Obtain::FedoraPackage,
        Obtain::OtherDistributionPackage,
        Obtain::Steam,
        Obtain::Epic,
        Obtain::Gog,
        Obtain::PublisherPage,
        Obtain::MicrosoftStore,
        Obtain::MacAppStore,
    ];

    /// The source as written in an entry.
    pub const fn as_str(self) -> &'static str {
        match self {
            Obtain::FedoraPackage => "fedora-package",
            Obtain::OtherDistributionPackage => "other-distribution-package",
            Obtain::Steam => "steam",
            Obtain::Epic => "epic",
            Obtain::Gog => "gog",
            Obtain::PublisherPage => "publisher-page",
            Obtain::MicrosoftStore => "microsoft-store",
            Obtain::MacAppStore => "mac-app-store",
        }
    }

    /// The source's name in a sentence.
    pub const fn label(self) -> &'static str {
        match self {
            Obtain::FedoraPackage => "the Fedora repositories",
            Obtain::OtherDistributionPackage => "another distribution's repositories",
            Obtain::Steam => "Steam",
            Obtain::Epic => "the Epic Games Store",
            Obtain::Gog => "GOG",
            Obtain::PublisherPage => "the publisher's page",
            Obtain::MicrosoftStore => "the Microsoft Store",
            Obtain::MacAppStore => "Apple's App Store",
        }
    }

    /// Whether the source is a store TUR cannot lawfully integrate.
    pub const fn is_closed_store(self) -> bool {
        matches!(self, Obtain::MicrosoftStore | Obtain::MacAppStore)
    }

    /// Whether the source identifies the edition by a store identifier.
    pub const fn uses_store_id(self) -> bool {
        matches!(
            self,
            Obtain::Steam
                | Obtain::Epic
                | Obtain::Gog
                | Obtain::MicrosoftStore
                | Obtain::MacAppStore
        )
    }
}

/// A store that carries an Android package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AndroidStore {
    /// F-Droid, an open-source store.
    FDroid,
    /// Google Play, a proprietary store the user opts into (ADR-0011).
    GooglePlay,
    /// The publisher's own page offers the package.
    PublisherPage,
}

impl AndroidStore {
    /// Every Android store, in schema order.
    pub const ALL: [AndroidStore; 3] = [
        AndroidStore::FDroid,
        AndroidStore::GooglePlay,
        AndroidStore::PublisherPage,
    ];

    /// The store as written in an entry.
    pub const fn as_str(self) -> &'static str {
        match self {
            AndroidStore::FDroid => "f-droid",
            AndroidStore::GooglePlay => "google-play",
            AndroidStore::PublisherPage => "publisher-page",
        }
    }

    /// The store's name.
    pub const fn label(self) -> &'static str {
        match self {
            AndroidStore::FDroid => "F-Droid",
            AndroidStore::GooglePlay => "Google Play",
            AndroidStore::PublisherPage => "the publisher's page",
        }
    }
}

/// Whether an edition is open source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Licence {
    /// Published under an open-source licence.
    OpenSource,
    /// Proprietary.
    Proprietary,
}

impl Licence {
    /// Both values, in schema order.
    pub const ALL: [Licence; 2] = [Licence::OpenSource, Licence::Proprietary];

    /// The value as written in an entry.
    pub const fn as_str(self) -> &'static str {
        match self {
            Licence::OpenSource => "open-source",
            Licence::Proprietary => "proprietary",
        }
    }
}

/// Who maintains a Linux package of an application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Packaging {
    /// The application's publisher.
    Publisher,
    /// Packagers other than the publisher: a distribution or volunteers.
    Community,
}

impl Packaging {
    /// Both values, in schema order.
    pub const ALL: [Packaging; 2] = [Packaging::Publisher, Packaging::Community];

    /// The value as written in an entry.
    pub const fn as_str(self) -> &'static str {
        match self {
            Packaging::Publisher => "publisher",
            Packaging::Community => "community",
        }
    }
}

/// A known requirement of an edition that decides routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Requirement {
    /// Anti-cheat that works only on real Windows: it loads a Windows kernel
    /// driver, or its publisher has refused to enable it for Proton.
    KernelAntiCheat,
    /// Copy protection that needs hardware attestation, such as Widevine
    /// level 1.
    HardwareAttestedDrm,
    /// Google's Play Integrity device verdict.
    PlayIntegrity,
    /// Native code built for ARM processors only.
    ArmOnlyNativeCode,
    /// A kernel driver of the edition's own operating system.
    KernelDriver,
    /// A hardware licence dongle.
    HardwareDongle,
    /// Heavy use of the graphics processor.
    HeavyGpu,
    /// An online account with the publisher.
    OnlineAccount,
}

impl Requirement {
    /// Every requirement, in schema order.
    pub const ALL: [Requirement; 8] = [
        Requirement::KernelAntiCheat,
        Requirement::HardwareAttestedDrm,
        Requirement::PlayIntegrity,
        Requirement::ArmOnlyNativeCode,
        Requirement::KernelDriver,
        Requirement::HardwareDongle,
        Requirement::HeavyGpu,
        Requirement::OnlineAccount,
    ];

    /// The requirement as written in an entry.
    pub const fn as_str(self) -> &'static str {
        match self {
            Requirement::KernelAntiCheat => "kernel-anti-cheat",
            Requirement::HardwareAttestedDrm => "hardware-attested-drm",
            Requirement::PlayIntegrity => "play-integrity",
            Requirement::ArmOnlyNativeCode => "arm-only-native-code",
            Requirement::KernelDriver => "kernel-driver",
            Requirement::HardwareDongle => "hardware-dongle",
            Requirement::HeavyGpu => "heavy-gpu",
            Requirement::OnlineAccount => "online-account",
        }
    }

    /// The requirement in a few plain words.
    pub const fn label(self) -> &'static str {
        match self {
            Requirement::KernelAntiCheat => "kernel-level anti-cheat",
            Requirement::HardwareAttestedDrm => "copy protection that needs hardware attestation",
            Requirement::PlayIntegrity => "Google's Play Integrity device check",
            Requirement::ArmOnlyNativeCode => "native code for ARM processors only",
            Requirement::KernelDriver => "a kernel driver",
            Requirement::HardwareDongle => "a hardware dongle",
            Requirement::HeavyGpu => "heavy use of the graphics processor",
            Requirement::OnlineAccount => "an online account",
        }
    }
}

/// A way of running an application, or the statement that there is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Route {
    /// A Flatpak on the host.
    Flatpak,
    /// A native Linux program on the host that is not a Flatpak.
    Native,
    /// A package of another distribution, in a container of that distribution.
    DistroContainer,
    /// Proton through `umu-launcher`: the compatibility layer for games.
    Proton,
    /// Wine: the compatibility layer for other Windows applications.
    Wine,
    /// The seamless Windows guest: a virtual machine.
    WindowsGuest,
    /// The Android runtime.
    AndroidRuntime,
    /// Darling: experimental, and only when the user has enabled it.
    Darling,
    /// The web application, in the host's browser.
    Web,
    /// No route runs the application on TUR. Never valid in a catalogue
    /// record; the router returns it with a reason and an alternative.
    NoRoute,
}

impl Route {
    /// Every route, in ladder order, ending with [`Route::NoRoute`].
    pub const ALL: [Route; 10] = [
        Route::Flatpak,
        Route::Native,
        Route::DistroContainer,
        Route::Proton,
        Route::Wine,
        Route::WindowsGuest,
        Route::AndroidRuntime,
        Route::Darling,
        Route::Web,
        Route::NoRoute,
    ];

    /// The routes a catalogue record or a pin may name: all but
    /// [`Route::NoRoute`].
    pub const RUNNABLE: [Route; 9] = [
        Route::Flatpak,
        Route::Native,
        Route::DistroContainer,
        Route::Proton,
        Route::Wine,
        Route::WindowsGuest,
        Route::AndroidRuntime,
        Route::Darling,
        Route::Web,
    ];

    /// The route as written in an entry, for example `windows-guest`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Route::Flatpak => "flatpak",
            Route::Native => "native",
            Route::DistroContainer => "distro-container",
            Route::Proton => "proton",
            Route::Wine => "wine",
            Route::WindowsGuest => "windows-guest",
            Route::AndroidRuntime => "android-runtime",
            Route::Darling => "darling",
            Route::Web => "web",
            Route::NoRoute => "no-route",
        }
    }

    /// The route's name for people.
    pub const fn label(self) -> &'static str {
        match self {
            Route::Flatpak => "Flatpak",
            Route::Native => "Native Linux",
            Route::DistroContainer => "Container of another distribution",
            Route::Proton => "Proton (through umu-launcher)",
            Route::Wine => "Wine",
            Route::WindowsGuest => "Windows guest (virtual machine)",
            Route::AndroidRuntime => "Android runtime",
            Route::Darling => "Darling (experimental)",
            Route::Web => "Web application",
            Route::NoRoute => "No route",
        }
    }

    /// Parses a route as written in an entry.
    pub fn from_name(name: &str) -> Option<Route> {
        Route::ALL.into_iter().find(|route| route.as_str() == name)
    }

    /// Whether the route is a virtual machine.
    pub const fn is_virtual_machine(self) -> bool {
        matches!(self, Route::WindowsGuest)
    }

    /// Whether the route is a compatibility layer for Windows software.
    pub const fn is_compatibility_layer(self) -> bool {
        matches!(self, Route::Proton | Route::Wine)
    }

    /// Whether the route runs a Linux edition.
    pub const fn is_linux(self) -> bool {
        matches!(
            self,
            Route::Flatpak | Route::Native | Route::DistroContainer
        )
    }
}

/// How well a route runs an edition: the catalogue's small fixed scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rating {
    /// Runs as intended, with no workaround worth mentioning.
    Good,
    /// Runs and is usable; needs tweaks or has minor problems.
    Usable,
    /// Runs, with problems that get in the way of normal use.
    Poor,
    /// Does not start, or cannot be used for its purpose.
    Broken,
    /// Not known. The only rating that needs no source.
    Unknown,
}

impl Rating {
    /// Every rating, best first, ending with [`Rating::Unknown`].
    pub const ALL: [Rating; 5] = [
        Rating::Good,
        Rating::Usable,
        Rating::Poor,
        Rating::Broken,
        Rating::Unknown,
    ];

    /// The rating as written in an entry.
    pub const fn as_str(self) -> &'static str {
        match self {
            Rating::Good => "good",
            Rating::Usable => "usable",
            Rating::Poor => "poor",
            Rating::Broken => "broken",
            Rating::Unknown => "unknown",
        }
    }

    /// Whether the rating says the route works: `good` or `usable`.
    pub const fn is_supporting(self) -> bool {
        matches!(self, Rating::Good | Rating::Usable)
    }

    /// Whether the rating says the route is inadequate: `poor` or `broken`.
    pub const fn is_adverse(self) -> bool {
        matches!(self, Rating::Poor | Rating::Broken)
    }
}

/// Who a compatibility record comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reporter {
    /// Nobody: the record only states that the rating is unknown.
    Nobody,
    /// A named external source, with the URL that was checked.
    External,
    /// TUR's own test, with a reference to its test report.
    TurTest,
    /// TUR's compatibility autopilot (`tur-runtime app autopilot`): the
    /// application passed TUR's check in a throwaway copy of its prefix with
    /// the settings in `remedies`. Never a rating: the record's rating is
    /// `unknown`, and the router shows the settings beside the route.
    TurAutopilot,
}

impl Reporter {
    /// The reporters whose records can give a route its rating, in schema
    /// order. This is the list the application centre's vocabulary carries
    /// (`desktop/apps/tur-centre/fixtures/vocabulary.json`); a
    /// `tur-autopilot` record never rates a route, see [`Reporter::EVERY`].
    pub const ALL: [Reporter; 3] = [Reporter::Nobody, Reporter::External, Reporter::TurTest];

    /// Every reporter, in schema order.
    pub const EVERY: [Reporter; 4] = [
        Reporter::Nobody,
        Reporter::External,
        Reporter::TurTest,
        Reporter::TurAutopilot,
    ];

    /// The reporter as written in an entry.
    pub const fn as_str(self) -> &'static str {
        match self {
            Reporter::Nobody => "nobody",
            Reporter::External => "external",
            Reporter::TurTest => "tur-test",
            Reporter::TurAutopilot => "tur-autopilot",
        }
    }
}

/// What is known about how well one route runs one edition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// The route the record is about.
    pub route: Route,
    /// The rating, on the catalogue's scale.
    pub rating: Rating,
    /// Who the record comes from.
    pub reported_by: Reporter,
    /// External: the name of the source, for example `ProtonDB`.
    #[serde(default)]
    pub source_name: Option<String>,
    /// External: the page that was checked.
    #[serde(default)]
    pub source_url: Option<String>,
    /// External: the source's own rating, in its own words, for example
    /// `Gold`.
    #[serde(default)]
    pub source_rating: Option<String>,
    /// TUR test: the path of the test report in the repository.
    #[serde(default)]
    pub test_report: Option<String>,
    /// The version of the application that was tested, when the source says.
    #[serde(default)]
    pub version_tested: Option<String>,
    /// The day the test was made or the source was checked, `YYYY-MM-DD`.
    pub date: String,
    /// Free-text notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// TUR autopilot: the remedies the application passed with, as
    /// `tur-runtime` names them (`dll-builtin:d3dx9_43`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remedies: Vec<String>,
    /// TUR autopilot: the runtime version it was measured with, such as
    /// `wine-9.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    /// TUR autopilot: the SHA-256 of the evidence in its result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_sha256: Option<String>,
}

/// A public page that a fact of an edition was checked against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    /// What the page is and what it shows.
    pub title: String,
    /// The page.
    pub url: String,
    /// The day it was checked, `YYYY-MM-DD`.
    pub checked: String,
}

/// Parses one entry from TOML.
///
/// Only the shape is checked here: field names, types and enumerated values.
/// The error is one line naming what is wrong and, when known, the line.
pub fn parse_entry(text: &str) -> Result<Entry, String> {
    toml::from_str::<Entry>(text).map_err(|error| {
        let message = error.message().to_owned();
        match error.span() {
            Some(span) => {
                let line = text
                    .bytes()
                    .take(span.start)
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                format!("line {line}: {message}")
            }
            None => message,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written<T: Serialize>(value: T) -> String {
        match serde_json::to_value(value).unwrap() {
            serde_json::Value::String(text) => text,
            other => panic!("not a string: {other}"),
        }
    }

    #[test]
    fn every_enumeration_is_written_as_its_as_str() {
        for value in Category::ALL {
            assert_eq!(written(value), value.as_str());
        }
        for value in EditionKind::ALL {
            assert_eq!(written(value), value.as_str());
        }
        for value in Obtain::ALL {
            assert_eq!(written(value), value.as_str());
        }
        for value in AndroidStore::ALL {
            assert_eq!(written(value), value.as_str());
        }
        for value in Licence::ALL {
            assert_eq!(written(value), value.as_str());
        }
        for value in Packaging::ALL {
            assert_eq!(written(value), value.as_str());
        }
        for value in Requirement::ALL {
            assert_eq!(written(value), value.as_str());
        }
        for value in Route::ALL {
            assert_eq!(written(value), value.as_str());
            assert_eq!(Route::from_name(value.as_str()), Some(value));
        }
        for value in Rating::ALL {
            assert_eq!(written(value), value.as_str());
        }
        for value in Reporter::EVERY {
            assert_eq!(written(value), value.as_str());
        }
        assert_eq!(written(EntryKind::Application), "application");
        assert_eq!(written(EntryKind::Class), "class");
        assert_eq!(Route::from_name("vm"), None);
    }

    #[test]
    fn runnable_routes_are_all_routes_but_no_route() {
        let expected: Vec<Route> = Route::ALL
            .into_iter()
            .filter(|route| *route != Route::NoRoute)
            .collect();
        assert_eq!(Route::RUNNABLE.to_vec(), expected);
    }

    const MINIMAL: &str = r#"
schema_version = 1
id = "org.example.app"
name = "Example"
publisher = "Example Project"
category = "utility"

[[editions]]
kind = "flatpak"
flatpak_id = "org.example.App"
"#;

    #[test]
    fn parses_a_minimal_entry_with_defaults() {
        let entry = parse_entry(MINIMAL).unwrap();
        assert_eq!(entry.id, "org.example.app");
        assert_eq!(entry.entry_kind, EntryKind::Application);
        assert!(entry.aliases.is_empty());
        assert_eq!(entry.editions.len(), 1);
        let edition = &entry.editions[0];
        assert_eq!(edition.kind, EditionKind::Flatpak);
        assert!(!edition.primary);
        assert!(edition.records.is_empty());
        assert_eq!(edition.routes(), [Route::Flatpak]);
        assert_eq!(
            edition.describe(),
            "Flatpak edition: org.example.App from flathub"
        );
    }

    #[test]
    fn parses_records_and_references() {
        let text = format!(
            "{MINIMAL}{}",
            r#"requirements = ["online-account"]

[[editions.records]]
route = "flatpak"
rating = "unknown"
reported_by = "nobody"
date = "2026-11-12"

[[editions.references]]
title = "Listing"
url = "https://example.org/app"
checked = "2026-11-12"
"#
        );
        let entry = parse_entry(&text).unwrap();
        let edition = &entry.editions[0];
        assert_eq!(edition.requirements, [Requirement::OnlineAccount]);
        assert!(edition.requires(Requirement::OnlineAccount));
        assert_eq!(edition.records[0].rating, Rating::Unknown);
        assert_eq!(edition.records[0].reported_by, Reporter::Nobody);
        assert_eq!(edition.references[0].checked, "2026-11-12");
    }

    #[test]
    fn rejects_unknown_fields_at_every_level() {
        let top = MINIMAL.replace("publisher =", "colour = \"red\"\npublisher =");
        assert!(parse_entry(&top).unwrap_err().contains("colour"));

        let edition = format!("{MINIMAL}colour = \"red\"\n");
        assert!(parse_entry(&edition).unwrap_err().contains("colour"));

        let record = format!(
            "{MINIMAL}\n[[editions.records]]\nroute = \"flatpak\"\nrating = \"unknown\"\nreported_by = \"nobody\"\ndate = \"2026-11-12\"\ncolour = \"red\"\n"
        );
        assert!(parse_entry(&record).unwrap_err().contains("colour"));

        let reference = format!(
            "{MINIMAL}\n[[editions.references]]\ntitle = \"x\"\nurl = \"https://example.org/\"\nchecked = \"2026-11-12\"\ncolour = \"red\"\n"
        );
        assert!(parse_entry(&reference).unwrap_err().contains("colour"));
    }

    #[test]
    fn rejects_values_outside_the_enumerations_and_wrong_types() {
        for (from, to) in [
            ("category = \"utility\"", "category = \"tool\""),
            ("kind = \"flatpak\"", "kind = \"snap\""),
            ("schema_version = 1", "schema_version = \"1\""),
            ("name = \"Example\"", "name = 7"),
        ] {
            let text = MINIMAL.replace(from, to);
            assert!(parse_entry(&text).is_err(), "{to}");
        }
        let missing = MINIMAL.replace("publisher = \"Example Project\"\n", "");
        assert!(parse_entry(&missing).unwrap_err().contains("publisher"));
        assert!(parse_entry("this is not TOML").is_err());
    }

    #[test]
    fn a_parse_error_names_the_line() {
        let text = MINIMAL.replace("kind = \"flatpak\"", "kind = \"snap\"");
        let error = parse_entry(&text).unwrap_err();
        assert!(error.starts_with("line "), "{error}");
    }

    const WITH_INSTALLER: &str = r#"
schema_version = 1
id = "org.example.app"
name = "Example"
publisher = "Example Project"
category = "utility"

[[editions]]
kind = "windows"
obtain = "publisher-page"
download_page = "https://example.org/download"
licence = "open-source"

[editions.installer]
url = "https://downloads.example.org:8443/releases/1.2/example-1.2-setup.exe?mirror=1#top"
sha256 = "9b697f65ecc2d9b0152f5ee96d72dde522a7827690349d9307935dacf40f42ec"
size = 1659704
version = "1.2"
licence = "MIT"
licence_url = "https://example.org/licence"
hash_source = "The publisher's sha256sums file"
checked = "2026-11-12"
"#;

    #[test]
    fn parses_a_pinned_installer_and_names_its_file_and_host() {
        let entry = parse_entry(WITH_INSTALLER).unwrap();
        let installer = entry.editions[0].installer.as_ref().unwrap();
        assert_eq!(installer.file_name(), "example-1.2-setup.exe");
        assert_eq!(installer.host(), "downloads.example.org");
        assert_eq!(installer.size, 1_659_704);
        assert!(!installer.portable);

        let unknown = WITH_INSTALLER.replace("checked =", "colour = \"red\"\nchecked =");
        assert!(parse_entry(&unknown).unwrap_err().contains("colour"));
        let missing = WITH_INSTALLER.replace("version = \"1.2\"\n", "");
        assert!(parse_entry(&missing).unwrap_err().contains("version"));

        let mut bare = installer.clone();
        bare.url = "https://example.org".to_owned();
        assert_eq!(bare.file_name(), "");
        assert_eq!(bare.host(), "example.org");
        bare.url = "http://example.org/a.exe".to_owned();
        assert_eq!(bare.host(), "");
    }

    #[test]
    fn linux_native_routes_depend_on_where_the_edition_comes_from() {
        let mut edition = Edition::new(EditionKind::LinuxNative);
        edition.obtain = Some(Obtain::FedoraPackage);
        assert_eq!(edition.routes(), [Route::Native]);
        edition.obtain = Some(Obtain::OtherDistributionPackage);
        assert_eq!(edition.routes(), [Route::DistroContainer]);
        assert_eq!(
            Edition::new(EditionKind::Windows).routes(),
            [Route::Proton, Route::Wine, Route::WindowsGuest]
        );
        assert_eq!(
            Edition::new(EditionKind::Android).routes(),
            [Route::AndroidRuntime]
        );
        assert_eq!(Edition::new(EditionKind::Web).routes(), [Route::Web]);
        assert_eq!(Edition::new(EditionKind::Macos).routes(), [Route::Darling]);
    }

    #[test]
    fn describes_where_an_edition_comes_from() {
        let mut windows = Edition::new(EditionKind::Windows);
        windows.obtain = Some(Obtain::Steam);
        windows.store_id = Some("1245620".to_owned());
        assert_eq!(windows.describe(), "Windows edition: Steam (1245620)");

        let mut android = Edition::new(EditionKind::Android);
        android.package = Some("org.example.app".to_owned());
        android.stores = vec![AndroidStore::FDroid, AndroidStore::GooglePlay];
        assert_eq!(
            android.describe(),
            "Android edition: org.example.app from F-Droid, Google Play"
        );

        let mut debian = Edition::new(EditionKind::LinuxNative);
        debian.obtain = Some(Obtain::OtherDistributionPackage);
        debian.distribution = Some("debian".to_owned());
        debian.package = Some("example".to_owned());
        assert_eq!(
            debian.describe(),
            "Native Linux edition: debian package example"
        );

        let class = Edition::new(EditionKind::Macos);
        assert_eq!(class.describe(), "macOS edition: source not recorded");
    }
}
