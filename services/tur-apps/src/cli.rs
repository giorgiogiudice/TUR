// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The command line, and the D-Bus methods it corresponds to.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Where the catalogue is installed on TUR.
pub const INSTALLED_CATALOGUE: &str = "/usr/share/tur/catalogue";

/// Installs, lists, updates and removes applications by the route the
/// catalogue's router chooses: Flatpak from a remote, or a Windows
/// application through tur-runtime. Every installation shows its plan
/// first.
#[derive(Parser, Debug)]
#[command(name = "tur-apps", version)]
pub struct Cli {
    /// The catalogue directory.
    #[arg(long, global = true, value_name = "DIR", default_value = INSTALLED_CATALOGUE)]
    pub catalogue: PathBuf,
    /// A JSON file of facts about this computer for the router, instead of
    /// looking at it.
    #[arg(long, global = true, value_name = "FILE")]
    pub host: Option<PathBuf>,
    /// A JSON file of the user's routing preferences.
    #[arg(long, global = true, value_name = "FILE")]
    pub prefs: Option<PathBuf>,
    /// The flatpak program.
    #[arg(long, global = true, value_name = "PATH")]
    pub flatpak_program: Option<PathBuf>,
    /// The curl program, used to download pinned installers and to ask
    /// Flathub's API for sizes.
    #[arg(long, global = true, value_name = "PATH")]
    pub curl_program: Option<PathBuf>,
    /// The tur-runtime program.
    #[arg(long, global = true, value_name = "PATH")]
    pub tur_runtime_program: Option<PathBuf>,
    /// The wine program, whose presence the router is told about.
    #[arg(long, global = true, value_name = "PATH")]
    pub wine_program: Option<PathBuf>,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The commands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show what installing an application would do, and do nothing.
    Plan {
        /// The catalogue identifier.
        id: String,
        /// A route the router offers, instead of the one it recommends:
        /// flatpak or wine.
        #[arg(long, value_name = "ROUTE")]
        route: Option<String>,
        /// An installer file you have, for a Windows application.
        #[arg(long, value_name = "FILE")]
        installer: Option<PathBuf>,
        /// Ask no remote for sizes, commits or permissions.
        #[arg(long)]
        offline: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Install an application: show the plan, then carry it out once you
    /// confirm.
    Install {
        /// The catalogue identifier.
        id: String,
        /// A route the router offers, instead of the one it recommends.
        #[arg(long, value_name = "ROUTE")]
        route: Option<String>,
        /// An installer file you have, for a Windows application.
        #[arg(long, value_name = "FILE")]
        installer: Option<PathBuf>,
        /// Confirm in advance: carry the plan out without asking.
        #[arg(long)]
        yes: bool,
        /// Write progress and the result as JSON lines.
        #[arg(long)]
        events: bool,
    },
    /// List everything installed for you, whatever it runs as.
    List {
        /// Ask the sources whether updates are available.
        #[arg(long)]
        check_updates: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Update an application installed by tur-apps.
    Update {
        /// The catalogue identifier.
        id: String,
        /// Write progress and the result as JSON lines.
        #[arg(long)]
        events: bool,
    },
    /// Remove an application installed by tur-apps. Its settings and
    /// documents are kept unless you say otherwise.
    Remove {
        /// The catalogue identifier.
        id: String,
        /// Keep its settings and documents (the default).
        #[arg(long, conflicts_with = "delete_data")]
        keep_data: bool,
        /// Remove its settings and documents too.
        #[arg(long)]
        delete_data: bool,
        /// Write progress and the result as JSON lines.
        #[arg(long)]
        events: bool,
    },
    /// Start an application installed by tur-apps.
    Launch {
        /// The catalogue identifier.
        id: String,
    },
    /// Show the Flatpak sources, or switch one on or off, for you.
    Sources {
        /// Switch a source on or off.
        #[command(subcommand)]
        action: Option<SourceAction>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

/// Switching a source.
#[derive(Subcommand, Debug)]
pub enum SourceAction {
    /// Switch a source on: flathub or fedora.
    Enable {
        /// The source.
        name: String,
    },
    /// Switch a source off. What was installed from it stays installed.
    Disable {
        /// The source.
        name: String,
    },
}

/// The proposed D-Bus interface's methods (`dbus/org.tur.Apps1.xml`), the
/// subcommand each one does the work of, and whether it changes anything.
/// `Cancel` belongs to the service alone: a command line is cancelled by
/// interrupting it.
pub const DBUS_METHODS: [(&str, Option<&str>, bool); 9] = [
    ("Plan", Some("plan"), false),
    ("Install", Some("install"), true),
    ("Update", Some("update"), true),
    ("Remove", Some("remove"), true),
    ("Launch", Some("launch"), true),
    ("ListInstalled", Some("list"), false),
    ("GetSources", Some("sources"), false),
    ("SetSource", Some("sources"), true),
    ("Cancel", None, true),
];
