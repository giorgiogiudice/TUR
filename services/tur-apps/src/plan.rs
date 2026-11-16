// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The plan: what installing an application will do, worked out before
//! anything is done.
//!
//! A plan starts with the router's answer for the application on this
//! computer. It says which source the application comes from, what is
//! downloaded from where (with the size and the digest when they are
//! known), where it is installed, what it may reach, and how it is removed.
//! Schema: `schemas/app-install-plan.schema.json`.
//!
//! A plan is refused, with a stable code and a plain sentence, when the
//! router has no route, when the route is not one this program installs by
//! yet, and when what would be installed cannot be verified.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tur_catalogue::{
    Arch, Catalogue, Date, Edition, EditionKind, Entry, HostFacts, Installer, Licence, Obtain,
    Preferences, PrerequisiteCode, Route, RouteOption, RouteRequest, parse_host_facts,
    parse_preferences, recommended, rehost_evidence, route,
};

use crate::codes::{Code, Failure};
use crate::digest::{Hashed, hash_file};
use crate::download::{FlathubSummary, fetch_flathub_summary};
use crate::events::human_size;
use crate::exec::Program;
use crate::flatpak::{self, Access, KnownSource, Metadata};
use crate::paths::{Dirs, display};
use crate::rehost::{self, RehostOptions};

/// The version of the plan format.
pub const PLAN_VERSION: u32 = 1;

/// The largest host-facts or preferences file, in bytes.
const MAX_INPUT_BYTES: u64 = 1024 * 1024;

/// The programs and places a plan is made with.
#[derive(Debug, Clone)]
pub struct Context {
    /// The catalogue directory.
    pub catalogue: PathBuf,
    /// The user's directories.
    pub dirs: Dirs,
    /// `flatpak`.
    pub flatpak: Program,
    /// `curl`, for the one module that downloads.
    pub curl: Program,
    /// `tur-runtime`.
    pub tur_runtime: Program,
    /// `tur-rehost`, whose presence is also a fact the router needs.
    pub tur_rehost: Program,
    /// Wine, whose presence is a fact the router needs.
    pub wine: PathBuf,
    /// A host-facts file that replaces detection.
    pub host_file: Option<PathBuf>,
    /// A preferences file.
    pub preferences_file: Option<PathBuf>,
}

/// What the user asked for.
#[derive(Debug, Clone, Default)]
pub struct Request {
    /// The catalogue identifier.
    pub id: String,
    /// A route the user chose instead of the router's recommendation.
    pub route: Option<Route>,
    /// An installer file the user supplies: for the re-hosted route, the
    /// publisher's Windows or macOS package.
    pub installer: Option<PathBuf>,
    /// Do not ask any remote for sizes, commits or permissions.
    pub offline: bool,
    /// The person allows native modules of a re-hosted application to be
    /// built from source on this computer. It is the router's host fact
    /// `rehost_builds` when the facts are looked at here (not when a
    /// host-facts file states them), and it is passed to `tur-rehost` as
    /// `--allow-builds`.
    pub allow_builds: bool,
}

/// The application, as the catalogue names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Application {
    /// The catalogue identifier.
    pub id: String,
    /// The name.
    pub name: String,
    /// The publisher.
    pub publisher: String,
    /// The category.
    pub category: String,
}

/// The facts about this computer the router was given.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostReport {
    /// The facts.
    pub facts: HostFacts,
    /// `detected`: flatpak, Wine, tur-rehost and the architecture were
    /// looked at, and whether the person allowed source builds was taken
    /// from `--allow-builds`; the rest assumed as the router's defaults.
    /// `stated`: read from a file.
    pub how: &'static str,
}

/// Where the application comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanSource {
    /// `flatpak-remote`, `publisher-download` or `user-file`.
    pub kind: &'static str,
    /// Its name for people.
    pub name: String,
    /// For a Flatpak, the remote.
    pub remote: Option<String>,
    /// For a download, the address.
    pub url: Option<String>,
    /// For a file, its path.
    pub file: Option<String>,
    /// How what is installed is checked before it is used.
    pub verification: String,
}

/// One thing that is downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Download {
    /// What it is.
    pub what: String,
    /// Where it comes from: a URL, or a remote and its address.
    pub from: String,
    /// The download's size in bytes, when known.
    pub size: Option<u64>,
    /// The size once installed, in bytes, when known.
    pub installed_size: Option<u64>,
    /// The SHA-256 the file must have, for a file.
    pub sha256: Option<String>,
    /// The commit that is installed, for a Flatpak.
    pub commit: Option<String>,
    /// Where the size came from, or why it is not known.
    pub size_source: String,
}

/// A place on the computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Place {
    /// What is there.
    pub what: String,
    /// The path, with the home folder written `~`.
    pub path: String,
}

/// How the application is removed, and what happens to its data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Removal {
    /// The command that removes it.
    pub command: String,
    /// What removal does, in plain language.
    pub text: String,
    /// Where the application keeps the user's data and settings.
    pub data: Vec<Place>,
    /// Whether the data is kept unless the user says otherwise. Always
    /// true.
    pub data_kept_by_default: bool,
}

/// One step of the installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// Its identifier: `download`, `verify`, `install`, `record`.
    pub id: &'static str,
    /// What it does.
    pub text: String,
    /// The command it runs, when it runs one: the program and its
    /// arguments.
    pub command: Vec<String>,
}

/// What a plan carries out, which is not part of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Execution {
    /// Install a Flatpak.
    Flatpak {
        /// The application's Flatpak identifier.
        app_id: String,
        /// The remote.
        remote: String,
        /// The reference.
        reference: String,
    },
    /// Install a Windows application through `tur-runtime`.
    Windows {
        /// The identifier `tur-runtime` keeps it under.
        runtime_app_id: String,
        /// The installer.
        file: WindowsFile,
        /// The file is the program itself.
        portable: bool,
    },
    /// Re-host a Windows or macOS package through `tur-rehost`.
    Rehost {
        /// The identifier `tur-rehost` keeps it under.
        app_id: String,
        /// The person's package.
        package: PathBuf,
        /// Its digest and size, when the plan was made.
        hashed: Hashed,
        /// Whether it is the package TUR measured.
        measured: bool,
        /// What `tur-rehost` is allowed.
        options: RehostOptions,
    },
}

/// Where a Windows application's installer comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsFile {
    /// Downloaded from the publisher's location the catalogue pins.
    Download(Installer),
    /// A file the user supplied, already hashed.
    Supplied {
        /// The file.
        path: PathBuf,
        /// Its digest and size.
        hashed: Hashed,
        /// Whether that digest is the one the catalogue pins.
        pinned: bool,
    },
}

/// A plan.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    /// [`PLAN_VERSION`].
    pub schema_version: u32,
    /// Always `plan`.
    pub kind: &'static str,
    /// The application.
    pub application: Application,
    /// The route it is installed by.
    pub route: Route,
    /// `router` when the router recommended it, `you` when it was chosen.
    pub chosen_by: &'static str,
    /// The router's option for that route, as the router returned it: its
    /// reason and reason codes, rating and source, caveats, prerequisites.
    pub router: RouteOption,
    /// The facts the router was given.
    pub host: HostReport,
    /// Where it comes from.
    pub source: PlanSource,
    /// What is downloaded.
    pub downloads: Vec<Download>,
    /// Where it is installed.
    pub installs: Vec<Place>,
    /// What it may reach on this computer.
    pub access: Vec<Access>,
    /// Whether `access` is the application's own statement (`true`), or
    /// could not be read (`false`, for example in an offline plan).
    pub access_known: bool,
    /// How it is removed.
    pub removal: Removal,
    /// The steps, in order.
    pub steps: Vec<Step>,
    /// Anything else the user should know.
    pub notes: Vec<String>,
    /// What is carried out.
    #[serde(skip)]
    pub execution: Execution,
}

/// Reads a small input file.
fn read_input(path: &Path, what: &str) -> Result<String, Failure> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|error| {
        Failure::new(
            Code::Unreadable,
            format!("cannot read the {what} file {}: {error}", path.display()),
        )
    })?;
    let mut text = String::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| {
            Failure::new(
                Code::InputInvalid,
                format!("cannot read the {what} file {}: {error}", path.display()),
            )
        })?;
    if text.len() as u64 > MAX_INPUT_BYTES {
        return Err(Failure::new(
            Code::InputInvalid,
            format!("the {what} file {} is too large", path.display()),
        ));
    }
    Ok(text)
}

/// Loads the catalogue.
pub fn load_catalogue(directory: &Path) -> Result<Catalogue, Failure> {
    Catalogue::load(directory, Date::today_utc()).map_err(|problems| {
        let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
        Failure::new(
            Code::Unreadable,
            format!(
                "the catalogue in {} cannot be used: {}",
                directory.display(),
                lines.join("; ")
            ),
        )
    })
}

/// The facts the router is given: read from the host-facts file, or
/// looked at here. `allow_builds` is the person's permission to build a
/// re-hosted application's native modules from source; it is a fact only
/// when the facts are looked at here.
pub fn host_facts(context: &Context, allow_builds: bool) -> Result<HostReport, Failure> {
    if let Some(path) = &context.host_file {
        let text = read_input(path, "host-facts")?;
        let facts = parse_host_facts(&text).map_err(|reason| {
            Failure::new(
                Code::InputInvalid,
                format!(
                    "the host-facts file {} is not valid: {reason}",
                    path.display()
                ),
            )
        })?;
        return Ok(HostReport {
            facts,
            how: "stated",
        });
    }
    let arch = match std::env::consts::ARCH {
        "x86_64" => Arch::X86_64,
        "aarch64" => Arch::Aarch64,
        _ => Arch::Other,
    };
    Ok(HostReport {
        facts: HostFacts {
            arch,
            flatpak: context.flatpak.exists(),
            wine: context.wine.is_file(),
            rehost: context.tur_rehost.exists(),
            rehost_builds: allow_builds,
            ..HostFacts::default()
        },
        how: "detected",
    })
}

fn preferences(context: &Context) -> Result<Preferences, Failure> {
    let Some(path) = &context.preferences_file else {
        return Ok(Preferences::default());
    };
    let text = read_input(path, "preferences")?;
    parse_preferences(&text).map_err(|reason| {
        Failure::new(
            Code::InputInvalid,
            format!(
                "the preferences file {} is not valid: {reason}",
                path.display()
            ),
        )
    })
}

/// The routes this program installs by.
pub const BUILT_ROUTES: [Route; 3] = [Route::Flatpak, Route::Rehost, Route::Wine];

/// Why a route is not one this program installs by, in a sentence.
fn not_built(route: Route) -> &'static str {
    match route {
        Route::WindowsGuest => {
            "Installing into the Windows guest is not built yet: the guest's own installation path (windows-guest.md, section 9.1) comes first."
        }
        Route::AndroidRuntime => "Installing Android applications is not built yet.",
        Route::Darling => "Installing macOS applications is not built yet.",
        Route::Proton => {
            "Installing games through Proton is not built yet: TUR does not ship Proton, and the store that sells the game installs it."
        }
        Route::Native => {
            "Installing a distribution package is not built yet: TUR's system is an image, and packages are added to it with the system, not by this program."
        }
        Route::DistroContainer => {
            "Installing a package of another distribution in a container is not built yet."
        }
        Route::Web => "Installing a web application is not built yet.",
        Route::Flatpak | Route::Rehost | Route::Wine | Route::NoRoute => {
            "This route is not built."
        }
    }
}

/// Makes the plan for a request, or says why there is none.
pub fn make(context: &Context, request: &Request) -> Result<Plan, Failure> {
    let catalogue = load_catalogue(&context.catalogue)?;
    let Some(entry) = catalogue.get(&request.id) else {
        return Err(Failure::new(
            Code::UnknownApplication,
            format!(
                "no application in the catalogue has the identifier \"{}\"; `tur-catalogue search` finds it by name",
                request.id
            ),
        ));
    };
    let host = host_facts(context, request.allow_builds)?;
    let preferences = preferences(context)?;
    let options = route(&RouteRequest::new(entry), &host.facts, &preferences);
    let usable = |option: &&RouteOption| option.outlook != tur_catalogue::Outlook::Blocked;

    let (chosen, chosen_by) = match request.route {
        Some(wanted) => {
            let Some(found) = options
                .iter()
                .filter(usable)
                .find(|option| option.route == wanted)
            else {
                let offered: Vec<&str> = options
                    .iter()
                    .filter(usable)
                    .filter(|option| option.route != Route::NoRoute)
                    .map(|option| option.route.as_str())
                    .collect();
                return Err(Failure::new(
                    Code::RouteNotOffered,
                    format!(
                        "the router does not offer {} for {} on this computer; it offers: {}",
                        wanted.as_str(),
                        entry.name,
                        if offered.is_empty() {
                            "nothing".to_owned()
                        } else {
                            offered.join(", ")
                        }
                    ),
                )
                .with_detail(serde_json::to_value(&options).unwrap_or_default()));
            };
            (found.clone(), "you")
        }
        None => (recommended(&options).clone(), "router"),
    };

    if chosen.route == Route::NoRoute {
        let alternative = chosen
            .alternative
            .as_ref()
            .map(|alternative| format!(" {}", alternative.text))
            .unwrap_or_default();
        return Err(Failure::new(
            Code::NoRoute,
            format!("{}{alternative} Nothing was installed.", chosen.reason),
        )
        .with_detail(serde_json::to_value(&chosen).unwrap_or_default()));
    }
    if !BUILT_ROUTES.contains(&chosen.route) {
        // A route once, even when two editions offer it (the Windows and
        // the macOS package, both re-hosted).
        let mut built: Vec<&str> = Vec::new();
        for option in options
            .iter()
            .filter(usable)
            .filter(|option| BUILT_ROUTES.contains(&option.route))
        {
            if !built.contains(&option.route.as_str()) {
                built.push(option.route.as_str());
            }
        }
        let instead = if built.is_empty() {
            String::new()
        } else {
            format!(
                " The router also offers a way this program can install by: use --route {}.",
                built.join(" or --route ")
            )
        };
        return Err(Failure::new(
            Code::RouteNotBuilt,
            format!(
                "The router's way for {} is: {}. {}{instead} Nothing was installed.",
                entry.name,
                chosen.route.label(),
                not_built(chosen.route)
            ),
        )
        .with_detail(serde_json::to_value(&chosen).unwrap_or_default()));
    }
    if !chosen.prerequisites.is_empty() {
        let needed: Vec<&str> = chosen
            .prerequisites
            .iter()
            .map(|prerequisite| prerequisite.text.as_str())
            .collect();
        let how = if chosen
            .prerequisites
            .iter()
            .any(|prerequisite| prerequisite.code == PrerequisiteCode::AllowRehostBuilds)
        {
            " To allow the builds, add --allow-builds."
        } else {
            ""
        };
        return Err(Failure::new(
            Code::ProgramMissing,
            format!("{}{how} Nothing was installed.", needed.join(" ")),
        )
        .with_detail(serde_json::to_value(&chosen).unwrap_or_default()));
    }
    let Some(edition) = chosen
        .edition_index
        .and_then(|index| entry.editions.get(index))
    else {
        return Err(Failure::new(
            Code::RouteNotBuilt,
            "the router's option names no edition",
        ));
    };

    let application = Application {
        id: entry.id.clone(),
        name: entry.name.clone(),
        publisher: entry.publisher.clone(),
        category: entry.category.as_str().to_owned(),
    };
    let base = Base {
        application,
        chosen,
        chosen_by,
        host,
    };
    match base.chosen.route {
        Route::Flatpak => flatpak_plan(context, request, entry, edition, base),
        Route::Rehost => rehost_plan(context, request, entry, edition, base),
        _ => windows_plan(context, request, entry, edition, base),
    }
}

/// What every plan starts from.
struct Base {
    application: Application,
    chosen: RouteOption,
    chosen_by: &'static str,
    host: HostReport,
}

fn place(what: &str, path: &Path, dirs: &Dirs) -> Place {
    Place {
        what: what.to_owned(),
        path: display(path, dirs),
    }
}

fn command(program: &Program, arguments: Vec<String>) -> Vec<String> {
    let mut line = vec![program.path.display().to_string()];
    line.extend(arguments);
    line
}

/// Whether the remote is there and switched on, for this user.
pub fn remote_state(flatpak: &Program, name: &str) -> Result<Option<flatpak::Remote>, Failure> {
    let outcome = flatpak.run(&flatpak::remotes_arguments())?;
    if !outcome.success() {
        return Err(Failure::new(
            Code::StepFailed,
            format!("flatpak could not list your sources: {}", outcome.tail()),
        ));
    }
    Ok(flatpak::parse_remotes(&outcome.stdout)
        .into_iter()
        .find(|remote| remote.name == name))
}

fn source_title(name: &str) -> String {
    flatpak::known_source(name).map_or_else(|| name.to_owned(), |source| source.title.to_owned())
}

fn flatpak_plan(
    context: &Context,
    request: &Request,
    entry: &Entry,
    edition: &Edition,
    base: Base,
) -> Result<Plan, Failure> {
    let dirs = &context.dirs;
    let Some(flatpak_id) = edition.flatpak_id.clone() else {
        return Err(Failure::new(
            Code::RouteNotBuilt,
            "the catalogue's Flatpak edition has no Flatpak identifier",
        ));
    };
    let remote = edition
        .remote
        .clone()
        .unwrap_or_else(|| "flathub".to_owned());
    let title = source_title(&remote);
    context.flatpak.require("to install Linux applications")?;

    let enable_hint = match flatpak::known_source(&remote) {
        Some(KnownSource { name, .. }) => {
            format!("switch it on with `tur-apps sources enable {name}`")
        }
        None => format!("the catalogue names the remote \"{remote}\", which TUR cannot switch on"),
    };
    let state = remote_state(&context.flatpak, &remote)?;
    let remote_url = match state {
        Some(found) if !found.disabled => found.url,
        _ => {
            return Err(Failure::new(
                Code::SourceOff,
                format!(
                    "{} comes from {title}, which is not switched on for you: {enable_hint}. Nothing was changed.",
                    entry.name
                ),
            ));
        }
    };

    let reference = flatpak::app_ref(&flatpak_id);
    let mut notes = Vec::new();
    let mut metadata: Option<Metadata> = None;
    let mut commit = None;
    let mut summary = FlathubSummary::default();
    let mut runtime_download: Option<Download> = None;
    if request.offline {
        notes.push(format!(
            "Offline plan: {title} was not asked for the size, the commit or the permissions. The plan made at installation asks it."
        ));
    } else {
        let outcome = context
            .flatpak
            .run(&flatpak::remote_metadata_arguments(&remote, &reference))?;
        if !outcome.success() {
            return Err(Failure::new(
                Code::NotVerified,
                format!(
                    "{title} does not have {reference} for this computer, or could not be asked: {}. Nothing was changed.",
                    outcome.tail()
                ),
            ));
        }
        let read = flatpak::parse_metadata(&outcome.stdout);
        if let Ok(outcome) = context
            .flatpak
            .run(&flatpak::remote_commit_arguments(&remote, &reference))
            && outcome.success()
        {
            commit = flatpak::parse_commit(&outcome.stdout);
        }
        if remote == "flathub"
            && let Some(found) =
                fetch_flathub_summary(&context.curl.path, &flatpak_id, flatpak::BRANCH)
        {
            summary = found;
        }
        if let Some(runtime) = &read.runtime {
            let runtime_ref = format!("runtime/{runtime}");
            let installed = flatpak::installed_refs(&context.flatpak).unwrap_or_default();
            if !installed.contains(&runtime_ref) {
                let mut parts = runtime.split('/');
                let runtime_id = parts.next().unwrap_or_default().to_owned();
                let runtime_branch = parts.nth(1).unwrap_or(flatpak::BRANCH).to_owned();
                let sizes = if remote == "flathub" {
                    fetch_flathub_summary(&context.curl.path, &runtime_id, &runtime_branch)
                        .unwrap_or_default()
                } else {
                    FlathubSummary::default()
                };
                runtime_download = Some(Download {
                    what: format!(
                        "the runtime {runtime}, which {} needs and which is not installed yet",
                        entry.name
                    ),
                    from: format!("{title} ({remote_url})"),
                    size: sizes.download_size,
                    installed_size: sizes.installed_size,
                    sha256: None,
                    commit: None,
                    size_source: if sizes.download_size.is_some() {
                        "Flathub's API".to_owned()
                    } else {
                        "not known".to_owned()
                    },
                });
            }
        }
        metadata = Some(read);
        notes.push(
            "Flatpak may also download related extensions, such as graphics drivers and translations, whose size is not known before it starts.".to_owned(),
        );
    }

    let mut downloads = vec![Download {
        what: format!("{} ({flatpak_id})", entry.name),
        from: format!("{title} ({remote_url})"),
        size: summary.download_size,
        installed_size: summary.installed_size,
        sha256: None,
        commit: commit.clone(),
        size_source: if summary.download_size.is_some() {
            "Flathub's API".to_owned()
        } else {
            "not known".to_owned()
        },
    }];
    downloads.extend(runtime_download);

    let (access, access_known) = match &metadata {
        Some(metadata) => (flatpak::access(metadata), true),
        None => (Vec::new(), false),
    };
    let data = dirs.flatpak_data(&flatpak_id);
    let install_line = command(
        &context.flatpak,
        flatpak::install_arguments(&remote, &reference),
    );
    Ok(Plan {
        schema_version: PLAN_VERSION,
        kind: "plan",
        application: base.application,
        route: base.chosen.route,
        chosen_by: base.chosen_by,
        router: base.chosen,
        host: base.host,
        source: PlanSource {
            kind: "flatpak-remote",
            name: title.clone(),
            remote: Some(remote.clone()),
            url: Some(remote_url),
            file: None,
            verification: format!(
                "Flatpak checks everything it downloads against {title}'s signing key, which was imported when the source was switched on."
            ),
        },
        downloads,
        installs: vec![
            place(
                "the application, for you only",
                &dirs.flatpak_app(&flatpak_id),
                dirs,
            ),
            place(
                "its launcher entry",
                &dirs.flatpak_desktop_entry(&flatpak_id),
                dirs,
            ),
        ],
        access,
        access_known,
        removal: Removal {
            command: format!("tur-apps remove {} [--keep-data | --delete-data]", entry.id),
            text: format!(
                "Removes {reference} with `flatpak uninstall --user`, then the runtimes and extensions this installation brought in that no other application uses. Its data in {} is kept unless you say to delete it.",
                display(&data, dirs)
            ),
            data: vec![place("its settings and documents", &data, dirs)],
            data_kept_by_default: true,
        },
        steps: vec![
            Step {
                id: "install",
                text: format!("Install {reference} from {title}, for you only"),
                command: install_line,
            },
            Step {
                id: "record",
                text: format!(
                    "Record the installation in {}",
                    display(&dirs.record(&entry.id), dirs)
                ),
                command: Vec::new(),
            },
        ],
        notes,
        execution: Execution::Flatpak {
            app_id: flatpak_id,
            remote,
            reference,
        },
    })
}

/// Why TUR will not download an edition's installer, and what to do.
fn installer_needed(entry: &Entry, edition: &Edition) -> Failure {
    let get_it = match (edition.obtain, &edition.download_page, &edition.store_id) {
        (Some(Obtain::PublisherPage), Some(page), _) => format!("Get the installer from {page}"),
        (Some(obtain), _, Some(id)) => format!("Get it from {} ({id})", obtain.label()),
        (Some(obtain), _, None) => format!("Get it from {}", obtain.label()),
        (None, _, _) => "Get the installer from its publisher".to_owned(),
    };
    let why = if edition.licence == Some(Licence::OpenSource) {
        "no download of it has been checked and pinned in the catalogue yet"
    } else {
        "TUR never downloads software it may not redistribute"
    };
    Failure::new(
        Code::InstallerFileNeeded,
        format!(
            "TUR does not download {}: {why}. {get_it}, then give it with `tur-apps install {} --installer FILE`. Nothing was installed.",
            entry.name, entry.id
        ),
    )
}

fn windows_plan(
    context: &Context,
    request: &Request,
    entry: &Entry,
    edition: &Edition,
    base: Base,
) -> Result<Plan, Failure> {
    let dirs = &context.dirs;
    if edition.kind != EditionKind::Windows {
        return Err(Failure::new(
            Code::RouteNotBuilt,
            "the router's Wine option names an edition that is not for Windows",
        ));
    }
    context
        .tur_runtime
        .require("to install Windows applications")?;
    let app_id = entry.id.clone();
    let pin = edition.installer.as_ref();

    let (file, source, downloads, mut steps, portable) = match (&request.installer, pin) {
        (Some(path), _) => {
            let hashed = hash_file(path)?;
            let pinned = match pin {
                Some(pin) if pin.sha256 == hashed.sha256 && pin.size == hashed.size => true,
                Some(pin) => {
                    return Err(Failure::new(
                        Code::NotVerified,
                        format!(
                            "{} is not the installer the catalogue pins for {} {}: it is {} bytes with SHA-256 {}, and the pinned file is {} bytes with SHA-256 {}. Nothing in it was run.",
                            path.display(),
                            entry.name,
                            pin.version,
                            hashed.size,
                            hashed.sha256,
                            pin.size,
                            pin.sha256
                        ),
                    ));
                }
                None => false,
            };
            let verification = if pinned {
                format!(
                    "Its SHA-256, {}, is the one the catalogue pins ({}).",
                    hashed.sha256,
                    pin.map(|pin| pin.hash_source.as_str()).unwrap_or_default()
                )
            } else {
                format!(
                    "TUR cannot check this file against its publisher: the catalogue pins no download for {}. It is installed as you gave it, inside the sandbox, and its SHA-256 ({}) is recorded.",
                    entry.name, hashed.sha256
                )
            };
            let source = PlanSource {
                kind: "user-file",
                name: "a file you gave".to_owned(),
                remote: None,
                url: None,
                file: Some(path.display().to_string()),
                verification,
            };
            let steps = vec![Step {
                id: "verify",
                text: format!(
                    "Checked now: {} is {} with SHA-256 {}",
                    path.display(),
                    human_size(hashed.size),
                    hashed.sha256
                ),
                command: Vec::new(),
            }];
            let portable = pin.is_some_and(|pin| pin.portable);
            let file = WindowsFile::Supplied {
                path: path.clone(),
                hashed,
                pinned,
            };
            (file, source, Vec::new(), steps, portable)
        }
        (None, Some(pin)) if edition.licence == Some(Licence::OpenSource) => {
            let source = PlanSource {
                kind: "publisher-download",
                name: format!("the publisher's download ({})", pin.host()),
                remote: None,
                url: Some(pin.url.clone()),
                file: None,
                verification: format!(
                    "The file must be {} bytes with SHA-256 {}, the digest the catalogue pins ({}, checked {}). Nothing in it runs before it matches; a file that does not is deleted.",
                    pin.size, pin.sha256, pin.hash_source, pin.checked
                ),
            };
            let downloads = vec![Download {
                what: format!(
                    "{} {} installer, {} (licence: {})",
                    entry.name,
                    pin.version,
                    pin.file_name(),
                    pin.licence
                ),
                from: pin.url.clone(),
                size: Some(pin.size),
                installed_size: None,
                sha256: Some(pin.sha256.clone()),
                commit: None,
                size_source: "the catalogue's pin".to_owned(),
            }];
            let steps = vec![
                Step {
                    id: "download",
                    text: format!(
                        "Download {} ({}) from {}",
                        pin.file_name(),
                        human_size(pin.size),
                        pin.host()
                    ),
                    command: command(
                        &context.curl,
                        crate::download::installer_arguments(
                            &pin.url,
                            &dirs
                                .downloads(&app_id)
                                .join(format!(".{}.part", pin.file_name())),
                            pin.size,
                        ),
                    ),
                },
                Step {
                    id: "verify",
                    text: format!("Check its size and SHA-256 against {}", pin.sha256),
                    command: Vec::new(),
                },
            ];
            (
                WindowsFile::Download(pin.clone()),
                source,
                downloads,
                steps,
                pin.portable,
            )
        }
        (None, _) => return Err(installer_needed(entry, edition)),
    };

    let file_path = match &file {
        WindowsFile::Download(pin) => dirs.downloads(&app_id).join(pin.file_name()),
        WindowsFile::Supplied { path, .. } => path.clone(),
    };
    steps.push(Step {
        id: "install",
        text: "Install it with tur-runtime into a Wine prefix of its own, inside the sandbox, unattended"
            .to_owned(),
        command: command(
            &context.tur_runtime,
            crate::runtime::install_arguments(&file_path, &app_id, &entry.name, portable),
        ),
    });
    steps.push(Step {
        id: "record",
        text: format!(
            "Record the installation in {}",
            display(&dirs.record(&entry.id), dirs)
        ),
        command: Vec::new(),
    });
    let mut notes = Vec::new();
    if matches!(file, WindowsFile::Download(_)) {
        notes.push(format!(
            "The installer is downloaded into a new directory under {} that only you can read, and deleted with it after the installation; its SHA-256 is kept in the record.",
            display(&dirs.download_root, dirs)
        ));
    }
    notes.push(
        "The launcher entry is staged by tur-runtime and not yet added to the launcher (docs/architecture/runtime-manager.md, \"The launcher entry\").".to_owned(),
    );

    let home = dirs.runtime_home(&app_id);
    let profiles = dirs.runtime_profiles(&app_id);
    Ok(Plan {
        schema_version: PLAN_VERSION,
        kind: "plan",
        application: base.application,
        route: base.chosen.route,
        chosen_by: base.chosen_by,
        router: base.chosen,
        host: base.host,
        source,
        downloads,
        installs: vec![
            place(
                "its own Wine prefix, private home and record",
                &dirs.runtime_app(&app_id),
                dirs,
            ),
            place(
                "its launcher entry and icon (staged)",
                &dirs.runtime_launcher(&app_id),
                dirs,
            ),
        ],
        access: windows_access(),
        access_known: true,
        removal: Removal {
            command: format!("tur-apps remove {} [--keep-data | --delete-data]", entry.id),
            text: format!(
                "Unless you say to delete them, its documents (its private home) and its Windows settings are first copied to {}; then `tur-runtime app uninstall` runs its own uninstaller and deletes its prefix, private home, record, launcher entry and logs, and checks that nothing is left.",
                display(&dirs.kept(&entry.id), dirs)
            ),
            data: vec![
                place("its documents (its private home)", &home, dirs),
                place("its Windows settings and profile folders", &profiles, dirs),
            ],
            data_kept_by_default: true,
        },
        steps,
        notes,
        execution: Execution::Windows {
            runtime_app_id: app_id,
            file,
            portable,
        },
    })
}

/// What a Windows application may reach, from the sandbox `tur-runtime`
/// starts it in (docs/architecture/runtime-manager.md, "The sandbox").
fn windows_access() -> Vec<Access> {
    let item = |kind: &'static str, permission: &str, text: &str| Access {
        kind,
        permission: permission.to_owned(),
        text: text.to_owned(),
    };
    vec![
        item(
            "files",
            "prefix-and-private-home",
            "It sees only its own Windows prefix and a private home. None of your files, unless you grant it a folder.",
        ),
        item(
            "network",
            "shared-network",
            "It can use the network (tur-runtime shares it by default).",
        ),
        item(
            "display",
            "x11",
            "It draws its windows through X11, which lets it see other X11 windows and keystrokes.",
        ),
    ]
}

/// The code a refusal by `tur-rehost plan` is reported with, from its exit
/// status (1 an input, 3 not re-hostable, 4 something must be allowed or
/// installed, 6 refused) and its blockers' codes.
fn rehost_refusal(status: Option<i32>, codes: &[&str]) -> Code {
    match status {
        Some(1) => Code::InputInvalid,
        Some(4) => Code::ProgramMissing,
        Some(6) if codes.contains(&"TUR-REHOST-APP-002") => Code::AlreadyInstalled,
        Some(3 | 6) => Code::NotVerified,
        _ => Code::StepFailed,
    }
}

/// The plan for the re-hosted route: the person's own Windows or macOS
/// package, laid by `tur-rehost` under the Linux build of its portable
/// runtime. TUR never downloads the package; it downloads only the runtime
/// (and native modules), as `tur-rehost` says from TUR's pinned index.
fn rehost_plan(
    context: &Context,
    request: &Request,
    entry: &Entry,
    edition: &Edition,
    base: Base,
) -> Result<Plan, Failure> {
    let dirs = &context.dirs;
    let Some(evidence) = rehost_evidence(edition) else {
        return Err(Failure::new(
            Code::RouteNotBuilt,
            "the router's re-hosted option names an edition that TUR has not measured",
        ));
    };
    let measurement = edition.records.iter().find(|record| {
        record.route == Route::Rehost && record.rehost.as_ref() == Some(evidence)
    });
    context.tur_rehost.require(
        "to run the Windows or macOS package of an application on Linux's own build of its runtime",
    )?;
    // The package is the publisher's, proprietary or not pinned: TUR never
    // downloads it, and the person's own file is required.
    let Some(path) = &request.installer else {
        return Err(installer_needed(entry, edition));
    };
    let hashed = hash_file(path)?;
    let measured =
        hashed.sha256 == evidence.package_sha256 && hashed.size == evidence.package_size;
    let platform = edition.kind.label();
    let app_id = entry.id.clone();
    // The modules the measurement showed it starts without are allowed for
    // the package TUR measured, and for no other file.
    let options = RehostOptions {
        allow_builds: request.allow_builds,
        allow_unresolved: if measured {
            evidence.unresolved.clone()
        } else {
            Vec::new()
        },
    };
    let not_measured = if measured {
        String::new()
    } else {
        format!(
            "This file is not the package TUR measured ({}), so TUR has not tested it. ",
            evidence.package
        )
    };

    // What tur-rehost says about this file: whether it can be re-hosted, and
    // the exact downloads from TUR's pinned index.
    let report = if request.offline {
        None
    } else {
        let outcome = context
            .tur_rehost
            .run(&rehost::plan_arguments(path, &app_id, &options))?;
        match rehost::parse_plan(&outcome.stdout) {
            Some(report) if report.ready && outcome.success() => Some(report),
            Some(report) => {
                let codes: Vec<&str> = report
                    .blockers
                    .iter()
                    .map(|blocker| blocker.code.as_str())
                    .collect();
                let why: Vec<String> = report
                    .blockers
                    .iter()
                    .map(|blocker| format!("{} ({})", blocker.message, blocker.code))
                    .collect();
                let allow = if !request.allow_builds && codes.contains(&"TUR-REHOST-NATIVE-002")
                {
                    " To allow the builds, add --allow-builds."
                } else {
                    ""
                };
                return Err(Failure::new(
                    rehost_refusal(outcome.status, &codes),
                    format!(
                        "tur-rehost cannot re-host {} from {}: {}.{allow} {not_measured}Nothing was installed.",
                        entry.name,
                        path.display(),
                        if why.is_empty() {
                            outcome.tail()
                        } else {
                            why.join("; ")
                        }
                    ),
                )
                .with_detail(serde_json::from_str(outcome.stdout.trim()).unwrap_or_default()));
            }
            None => {
                let why = rehost::parse_failure(&outcome.stdout).unwrap_or_else(|| outcome.tail());
                return Err(Failure::new(
                    rehost_refusal(outcome.status, &[]),
                    format!(
                        "tur-rehost could not make a plan for {} ({why}). Nothing was installed.",
                        path.display()
                    ),
                ));
            }
        }
    };

    let planned_runtime = report.as_ref().and_then(|report| report.runtime.as_ref());
    let family = planned_runtime
        .map(|runtime| runtime.family.clone())
        .filter(|family| !family.is_empty())
        .unwrap_or_else(|| evidence.runtime.clone());
    let version = planned_runtime
        .map(|runtime| runtime.version.clone())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| evidence.runtime_version.clone());
    let runtime = rehost::runtime_name(&family);
    let chromium = matches!(family.as_str(), "electron" | "nwjs");

    let tested = measurement
        .map(|record| {
            let with = record
                .version_tested
                .as_deref()
                .map(|tested| format!(" with version {tested}"))
                .unwrap_or_default();
            format!(" Tested by TUR on {}{with}: its first window.", record.date)
        })
        .unwrap_or_default();
    let mut notes = vec![format!(
        "Runs natively through its portable runtime: the {platform} package, on Linux's own build of {runtime} {version}. Not the publisher's Linux version; the publisher does not support it.{tested}"
    )];
    if measured && !evidence.unresolved.is_empty() {
        notes.push(format!(
            "TUR's measurement showed that it starts without these native modules, which have no Linux build, so tur-rehost is allowed to leave them out; features that need them will not work: {}.",
            evidence.unresolved.join(", ")
        ));
    } else if !measured && !evidence.unresolved.is_empty() {
        notes.push(format!(
            "{not_measured}tur-rehost is not allowed to leave out a native module it cannot supply, as it is for the measured package: if one is missing, it refuses."
        ));
    } else if !measured {
        notes.push(not_measured.trim_end().to_owned());
    }
    match planned_runtime.map(|runtime| runtime.notice.as_str()) {
        Some(notice) if !notice.is_empty() => notes.push(notice.to_owned()),
        _ => notes.push(format!(
            "{runtime}'s Linux build is downloaded by this computer from the {runtime} project, at your request, and checked against the SHA-256 TUR pinned. It is not part of TUR{}.",
            if chromium {
                ", and it includes media codecs (H.264 and AAC) that TUR does not ship"
            } else {
                ""
            }
        )),
    }
    let builds = report
        .as_ref()
        .map_or(!evidence.native_builds.is_empty(), |report| {
            report.needs_build_environment
        });
    if builds {
        notes.push(
            "As you allowed, some of its native modules are built from source on this computer, without network access, with Fedora's compiler; Fedora's build tools are downloaded once from Fedora.".to_owned(),
        );
    }
    notes.push(format!(
        "Its own updater is off: it would fetch a {platform} update. To move to a newer version, remove it (its settings and documents are kept) and install the newer package."
    ));
    notes.push(
        "The launcher entry is staged by tur-rehost and not yet added to the launcher.".to_owned(),
    );
    if request.offline {
        notes.push(
            "Offline plan: tur-rehost did not open the file, so whether it can be re-hosted, and the exact downloads with their sizes and digests, are not known yet. The plan made at installation asks it.".to_owned(),
        );
    } else {
        notes.push(
            "To make this plan, tur-rehost opened the file in its sandbox, without running anything in it, and read TUR's pinned index.".to_owned(),
        );
    }

    let downloads = match &report {
        Some(report) => report
            .downloads
            .iter()
            .map(|download| Download {
                what: match &download.licence {
                    Some(licence) => format!("{} (licence: {licence})", download.what),
                    None => download.what.clone(),
                },
                from: download.url.clone(),
                size: Some(download.size),
                installed_size: None,
                sha256: Some(download.sha256.clone()),
                commit: None,
                size_source: "TUR's pinned index, through tur-rehost".to_owned(),
            })
            .collect(),
        None => vec![Download {
            what: format!(
                "{runtime} {version} for Linux, the runtime it runs on{}",
                if measured {
                    ""
                } else {
                    " (the version of the package TUR measured; this file may need another)"
                }
            ),
            from: if family == "electron" {
                "the Electron project's releases on GitHub".to_owned()
            } else {
                format!("the {runtime} project's own releases")
            },
            size: None,
            installed_size: None,
            sha256: None,
            commit: None,
            size_source: "not known: an offline plan does not ask tur-rehost, which reads the size and the digest from TUR's pinned index".to_owned(),
        }],
    };

    let verification = if measured {
        format!(
            "This is the package TUR measured: {}, {} bytes with SHA-256 {}, the file of the catalogue's record of TUR's test ({}). Nothing in it is run to install it: tur-rehost unpacks it in its sandbox and lays its files under the runtime.",
            evidence.package, evidence.package_size, evidence.package_sha256, evidence.evidence
        )
    } else {
        format!(
            "This file was not measured by TUR: its SHA-256 is {}, and the package TUR measured, {}, has {}. It is re-hosted as you gave it and its digest is recorded. Nothing in it is run to install it: tur-rehost unpacks it in its sandbox and lays its files under the runtime.",
            hashed.sha256, evidence.package, evidence.package_sha256
        )
    };
    let steps = vec![
        Step {
            id: "verify",
            text: format!(
                "Checked now: {} is {} with SHA-256 {}; {}",
                path.display(),
                human_size(hashed.size),
                hashed.sha256,
                if measured {
                    "it is the package TUR measured"
                } else {
                    "it is not the package TUR measured"
                }
            ),
            command: Vec::new(),
        },
        Step {
            id: "install",
            text: format!(
                "Re-host it with tur-rehost: download {runtime} {version} for Linux and check it against TUR's pinned SHA-256, unpack the package without running anything in it, lay its files under the runtime, and stage its launcher entry"
            ),
            command: command(
                &context.tur_rehost,
                rehost::install_arguments(path, &app_id, &entry.name, &options),
            ),
        },
        Step {
            id: "record",
            text: format!(
                "Record the installation in {}",
                display(&dirs.record(&entry.id), dirs)
            ),
            command: Vec::new(),
        },
    ];

    let home = dirs.rehost_home(&app_id);
    Ok(Plan {
        schema_version: PLAN_VERSION,
        kind: "plan",
        application: base.application,
        route: base.chosen.route,
        chosen_by: base.chosen_by,
        router: base.chosen,
        host: base.host,
        source: PlanSource {
            kind: "user-file",
            name: format!("the {platform} package you gave"),
            remote: None,
            url: None,
            file: Some(path.display().to_string()),
            verification,
        },
        downloads,
        installs: vec![
            place(
                "the application, laid under its runtime, with its private home and record",
                &dirs.rehost_app(&app_id),
                dirs,
            ),
            place(
                "its launcher entry (staged)",
                &dirs.rehost_desktop_entry(&app_id),
                dirs,
            ),
        ],
        access: rehost_access(),
        access_known: true,
        removal: Removal {
            command: format!("tur-apps remove {} [--keep-data | --delete-data]", entry.id),
            text: format!(
                "Unless you say to delete them, its settings and documents (its private home) are first copied to {}; then `tur-rehost uninstall` deletes its directory, private home, launcher entry and logs, and checks that nothing is left.",
                display(&dirs.kept(&entry.id), dirs)
            ),
            data: vec![place(
                "its settings and documents (its private home)",
                &home,
                dirs,
            )],
            data_kept_by_default: true,
        },
        steps,
        notes,
        execution: Execution::Rehost {
            app_id,
            package: path.clone(),
            hashed,
            measured,
            options,
        },
    })
}

/// What a re-hosted application may reach, from the sandbox `tur-rehost`
/// starts it in (`services/tur-rehost/src/sandbox.rs`, the application
/// sandbox).
fn rehost_access() -> Vec<Access> {
    let item = |kind: &'static str, permission: &str, text: &str| Access {
        kind,
        permission: permission.to_owned(),
        text: text.to_owned(),
    };
    vec![
        item(
            "files",
            "own-directory-and-private-home",
            "It sees only its own directory, which it cannot change, and a private home. None of your files.",
        ),
        item(
            "network",
            "shared-network",
            "It can use the network (tur-rehost shares it: these applications need it).",
        ),
        item(
            "display",
            "wayland-or-x11",
            "It draws its windows through Wayland or X11, as its runtime chooses.",
        ),
        item(
            "sound",
            "sound-server",
            "It can play and record sound through your session's sound server.",
        ),
        item(
            "services",
            "accessibility",
            "Its windows are visible to P@x and to screen readers, as a Linux application's are. It does not see the session's other services or your keyring.",
        ),
    ]
}

/// The plan as text for a person.
pub fn render(plan: &Plan) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} ({}) by {}",
        plan.application.name, plan.application.id, plan.application.publisher
    );
    let _ = writeln!(
        out,
        "How it will run: {}{}",
        plan.route.label(),
        if plan.chosen_by == "you" {
            ", as you chose"
        } else {
            ", as the router recommends"
        }
    );
    let _ = writeln!(out, "  Why: {}", plan.router.reason);
    for caveat in &plan.router.caveats {
        let _ = writeln!(
            out,
            "  Before you install ({:?}): {}",
            caveat.severity, caveat.text
        );
    }
    let _ = writeln!(out, "Source: {}", plan.source.name);
    if let Some(url) = &plan.source.url {
        let _ = writeln!(out, "  {url}");
    }
    let _ = writeln!(out, "  {}", plan.source.verification);
    if plan.downloads.is_empty() {
        let _ = writeln!(out, "Downloads: nothing");
    }
    for download in &plan.downloads {
        let size = download
            .size
            .map_or_else(|| "size not known".to_owned(), human_size);
        let _ = writeln!(
            out,
            "Downloads: {} — {size}, from {}",
            download.what, download.from
        );
        if let Some(sha256) = &download.sha256 {
            let _ = writeln!(out, "  SHA-256 {sha256}");
        }
        if let Some(commit) = &download.commit {
            let _ = writeln!(out, "  commit {commit}");
        }
        if let Some(installed) = download.installed_size {
            let _ = writeln!(out, "  {} once installed", human_size(installed));
        }
    }
    for place in &plan.installs {
        let _ = writeln!(out, "Installs {}: {}", place.what, place.path);
    }
    if !plan.access_known {
        let _ = writeln!(out, "What it can reach: not known (not asked)");
    } else if plan.access.is_empty() {
        let _ = writeln!(out, "What it can reach: nothing beyond its own files");
    }
    for access in &plan.access {
        let _ = writeln!(out, "What it can reach ({}): {}", access.kind, access.text);
    }
    let _ = writeln!(out, "Removing it: {}", plan.removal.text);
    let _ = writeln!(out, "  {}", plan.removal.command);
    for step in &plan.steps {
        let _ = writeln!(out, "Step {}: {}", step.id, step.text);
    }
    for note in &plan.notes {
        let _ = writeln!(out, "Note: {note}");
    }
    out
}
