// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The Flatpak route, through the `flatpak` command line.
//!
//! Everything is per user (`--user`): nothing is installed for other users
//! and nothing needs administrator rights. Facts about the command line,
//! read from flatpak 1.18.4 (Fedora 45) and 1.14.6 (Ubuntu 24.04, where CI
//! runs), on which this module relies:
//!
//! * without a terminal, `flatpak list`, `remotes` and `remote-ls` print
//!   their columns separated by tabs, with no header line, and nothing at
//!   all when there is no row (`app/flatpak-table-printer.c`);
//! * `--noninteractive` asks nothing and prints one line per operation,
//!   `Installing <ref>`, `Updating <ref>` or `Uninstalling <ref>`, and on
//!   failure `Error: Failed to <op> <ref>: <reason>` on standard error
//!   (`app/flatpak-quiet-transaction.c`). There is no progress in numbers:
//!   the application centre shows the activity, not a percentage;
//! * `--json` exists only from flatpak 1.17 and is not used;
//! * sizes in `list` and `remote-info` are written for people, by the
//!   locale; only `info --show-size` prints bytes. Every run gets
//!   `LC_ALL=C.UTF-8` (see `exec`);
//! * the exit status is 1 for every failure.

use std::collections::BTreeMap;

use serde::Serialize;

/// The architecture Flatpak names for this machine. Rust's names for the
/// two architectures TUR supports, `x86_64` and `aarch64`, are Flatpak's.
pub fn arch() -> &'static str {
    std::env::consts::ARCH
}

/// The branch applications are installed from. Flathub publishes
/// applications on `stable`.
pub const BRANCH: &str = "stable";

/// The full reference of an application: `app/<id>/<arch>/<branch>`.
pub fn app_ref(flatpak_id: &str) -> String {
    format!("app/{flatpak_id}/{}/{BRANCH}", arch())
}

/// A source of Flatpak applications that TUR knows how to switch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct KnownSource {
    /// The remote's name, as an edition names it.
    pub name: &'static str,
    /// Its title for people.
    pub title: &'static str,
    /// What `flatpak remote-add` is given.
    pub location: &'static str,
    /// What it carries, in a few words.
    pub carries: &'static str,
}

/// The sources TUR can switch on. Flathub's location is the one
/// `https://flathub.org/setup` gives; Fedora's is the one Fedora's own
/// `flatpak-add-fedora-repos.service` adds system-wide on Fedora 45.
pub const KNOWN_SOURCES: [KnownSource; 2] = [
    KnownSource {
        name: "flathub",
        title: "Flathub",
        location: "https://dl.flathub.org/repo/flathub.flatpakrepo",
        carries: "Linux applications from their publishers and from volunteers",
    },
    KnownSource {
        name: "fedora",
        title: "Fedora Flatpaks",
        location: "oci+https://registry.fedoraproject.org",
        carries: "Linux applications built by Fedora from Fedora's packages",
    },
];

/// The known source of a remote name.
pub fn known_source(name: &str) -> Option<&'static KnownSource> {
    KNOWN_SOURCES.iter().find(|source| source.name == name)
}

/// `flatpak remotes`: every per-user remote, switched off ones too.
pub fn remotes_arguments() -> Vec<String> {
    strings(&["remotes", "--user", "--show-disabled", "--columns=name,url,options"])
}

/// `flatpak remote-add` for a known source.
pub fn remote_add_arguments(source: &KnownSource) -> Vec<String> {
    let mut arguments = strings(&["remote-add", "--user", "--if-not-exists"]);
    arguments.push(format!("--title={}", source.title));
    arguments.push(source.name.to_owned());
    arguments.push(source.location.to_owned());
    arguments
}

/// `flatpak remote-modify --enable` or `--disable`.
pub fn remote_modify_arguments(name: &str, enable: bool) -> Vec<String> {
    let switch = if enable { "--enable" } else { "--disable" };
    strings(&["remote-modify", "--user", switch, name])
}

/// `flatpak remote-info --show-metadata`: the application's metadata, as
/// the remote publishes it, which lists the permissions it asks for.
pub fn remote_metadata_arguments(remote: &str, reference: &str) -> Vec<String> {
    strings(&["remote-info", "--user", "--show-metadata", remote, reference])
}

/// `flatpak remote-info --show-commit`: the commit that would be installed.
pub fn remote_commit_arguments(remote: &str, reference: &str) -> Vec<String> {
    strings(&["remote-info", "--user", "--show-commit", remote, reference])
}

/// `flatpak install`, without questions.
pub fn install_arguments(remote: &str, reference: &str) -> Vec<String> {
    strings(&["install", "--user", "--noninteractive", remote, reference])
}

/// `flatpak update` of one application, without questions.
pub fn update_arguments(reference: &str) -> Vec<String> {
    strings(&["update", "--user", "--noninteractive", reference])
}

/// `flatpak uninstall`, without questions; with `delete_data`, the
/// application's data in `~/.var/app` and its permissions go too.
pub fn uninstall_arguments(references: &[String], delete_data: bool) -> Vec<String> {
    let mut arguments = strings(&["uninstall", "--user", "--noninteractive"]);
    if delete_data {
        arguments.push("--delete-data".to_owned());
    }
    arguments.extend(references.iter().cloned());
    arguments
}

/// `flatpak list` of everything installed per user, applications and
/// runtimes, by reference.
pub fn list_refs_arguments() -> Vec<String> {
    strings(&["list", "--user", "--columns=ref"])
}

/// `flatpak list` of the per-user applications with what this program
/// shows of them.
pub fn list_apps_arguments() -> Vec<String> {
    strings(&[
        "list",
        "--user",
        "--app",
        "--columns=application,version,branch,origin,runtime",
    ])
}

/// `flatpak info --show-size`: the installed size in bytes.
pub fn size_arguments(reference: &str) -> Vec<String> {
    strings(&["info", "--user", "--show-size", reference])
}

/// `flatpak info --show-commit`: the installed commit.
pub fn commit_arguments(reference: &str) -> Vec<String> {
    strings(&["info", "--user", "--show-commit", reference])
}

/// `flatpak remote-ls --updates`: the per-user applications that have an
/// update. This asks the remotes.
pub fn updates_arguments() -> Vec<String> {
    strings(&[
        "remote-ls",
        "--user",
        "--updates",
        "--app",
        "--columns=application,version,origin",
    ])
}

/// `flatpak run`.
pub fn run_arguments(flatpak_id: &str) -> Vec<String> {
    strings(&["run", "--user", flatpak_id])
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|item| (*item).to_owned()).collect()
}

/// The rows of a table `flatpak` printed without a terminal: one row per
/// line, columns separated by tabs. Empty lines are skipped.
pub fn parse_table(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split('\t').map(|cell| cell.trim().to_owned()).collect())
        .collect()
}

/// One per-user remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Remote {
    /// Its name.
    pub name: String,
    /// Its address.
    pub url: String,
    /// Whether it is switched off.
    pub disabled: bool,
}

/// Reads `flatpak remotes --columns=name,url,options`.
pub fn parse_remotes(text: &str) -> Vec<Remote> {
    parse_table(text)
        .into_iter()
        .filter_map(|row| {
            let name = row.first()?.clone();
            let url = row.get(1).cloned().unwrap_or_default();
            let options = row.get(2).cloned().unwrap_or_default();
            let disabled = options.split(',').any(|option| option.trim() == "disabled");
            Some(Remote {
                name,
                url,
                disabled,
            })
        })
        .collect()
}

/// One per-user application as `flatpak list` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedApp {
    /// Its Flatpak identifier.
    pub application: String,
    /// Its version, when the application states one.
    pub version: Option<String>,
    /// Its branch.
    pub branch: String,
    /// The remote it came from.
    pub origin: String,
    /// The runtime it uses, `name/arch/branch`.
    pub runtime: Option<String>,
}

/// Reads `flatpak list --app --columns=application,version,branch,origin,runtime`.
pub fn parse_apps(text: &str) -> Vec<ListedApp> {
    let cell = |row: &[String], index: usize| {
        row.get(index)
            .filter(|value| !value.is_empty() && *value != "-")
            .cloned()
    };
    parse_table(text)
        .into_iter()
        .filter_map(|row| {
            Some(ListedApp {
                application: cell(&row, 0)?,
                version: cell(&row, 1),
                branch: cell(&row, 2).unwrap_or_default(),
                origin: cell(&row, 3).unwrap_or_default(),
                runtime: cell(&row, 4),
            })
        })
        .collect()
}

/// Reads `flatpak list --columns=ref`: every installed reference.
pub fn parse_refs(text: &str) -> Vec<String> {
    parse_table(text)
        .into_iter()
        .filter_map(|row| row.into_iter().next())
        .filter(|reference| reference.contains('/'))
        .collect()
}

/// The identifier in a reference: `org.x.Y` of `runtime/org.x.Y/x86_64/1`.
pub fn ref_id(reference: &str) -> &str {
    reference.split('/').nth(1).unwrap_or(reference)
}

/// Reads the number `info --show-size` prints.
pub fn parse_size(text: &str) -> Option<u64> {
    text.split_whitespace().next()?.parse().ok()
}

/// Reads the first word `info --show-commit` or `remote-info --show-commit`
/// prints: the commit checksum.
pub fn parse_commit(text: &str) -> Option<String> {
    let word = text.split_whitespace().next()?;
    (word.len() == 64 && word.bytes().all(|b| b.is_ascii_hexdigit())).then(|| word.to_owned())
}

/// Reads the lines a quiet transaction prints and returns, for each, what
/// is happening: `Installing <ref>` and so on.
pub fn activity(line: &str) -> Option<(&'static str, String)> {
    let line = line.trim();
    for (prefix, what) in [
        ("Installing ", "installing"),
        ("Updating ", "updating"),
        ("Uninstalling ", "uninstalling"),
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            let reference = rest.trim().trim_end_matches(['.', '…']);
            if reference.contains('/') {
                return Some((what, reference.to_owned()));
            }
        }
    }
    None
}

/// The permissions an application asks for, read from its metadata
/// (flatpak-metadata(5)): an `.ini`-style key file whose `[Context]` group
/// lists what it may reach, and whose bus policy groups list the D-Bus
/// names it may talk to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Metadata {
    /// `[Application] runtime`.
    pub runtime: Option<String>,
    /// `[Application] command`.
    pub command: Option<String>,
    /// `[Context] shared`: `network`, `ipc`.
    pub shared: Vec<String>,
    /// `[Context] sockets`: `x11`, `wayland`, `fallback-x11`,
    /// `pulseaudio`, `session-bus`, `system-bus`, `ssh-auth`, `pcsc`,
    /// `cups`, `gpg-agent`.
    pub sockets: Vec<String>,
    /// `[Context] devices`: `dri`, `all`, `kvm`, `shm`, `input`, `usb`.
    pub devices: Vec<String>,
    /// `[Context] filesystems`.
    pub filesystems: Vec<String>,
    /// `[Context] features`.
    pub features: Vec<String>,
    /// `[Session Bus Policy]`: name and policy.
    pub session_bus: BTreeMap<String, String>,
    /// `[System Bus Policy]`: name and policy.
    pub system_bus: BTreeMap<String, String>,
}

/// Reads an application's metadata key file.
pub fn parse_metadata(text: &str) -> Metadata {
    let mut metadata = Metadata::default();
    let mut group = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            group = name.to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let list = || -> Vec<String> {
            value
                .split(';')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect()
        };
        match (group.as_str(), key) {
            ("Application", "runtime") => metadata.runtime = Some(value.to_owned()),
            ("Application", "command") => metadata.command = Some(value.to_owned()),
            ("Context", "shared") => metadata.shared = list(),
            ("Context", "sockets") => metadata.sockets = list(),
            ("Context", "devices") => metadata.devices = list(),
            ("Context", "filesystems") => metadata.filesystems = list(),
            ("Context", "features") => metadata.features = list(),
            ("Session Bus Policy", name) => {
                metadata
                    .session_bus
                    .insert(name.to_owned(), value.to_owned());
            }
            ("System Bus Policy", name) => {
                metadata
                    .system_bus
                    .insert(name.to_owned(), value.to_owned());
            }
            _ => {}
        }
    }
    metadata
}

/// One thing an application may reach, in the words the application centre
/// uses for every route (desktop-experience.md, section 8.4): files,
/// devices, network, and the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Access {
    /// `files`, `devices`, `network`, `display`, `sound`, `services` or
    /// `other`.
    pub kind: &'static str,
    /// The permission as the metadata writes it.
    pub permission: String,
    /// What it means, in plain language.
    pub text: String,
}

/// Turns a metadata file's permissions into what the plan shows. A
/// permission the metadata does not ask for is not listed; the first line
/// says what is never reachable without one.
pub fn access(metadata: &Metadata) -> Vec<Access> {
    let mut list = Vec::new();
    let mut add = |kind: &'static str, permission: String, text: String| {
        list.push(Access {
            kind,
            permission,
            text,
        });
    };
    for item in &metadata.filesystems {
        let (place, mode) = match item.split_once(':') {
            Some((place, mode)) => (place, mode),
            None => (item.as_str(), "rw"),
        };
        if place.starts_with('!') {
            continue;
        }
        let what = match place {
            "host" => "all your files and the system's".to_owned(),
            "host-os" => "the system's own files".to_owned(),
            "host-etc" => "the system's configuration".to_owned(),
            "home" | "~" => "every file in your home folder".to_owned(),
            other => other.to_owned(),
        };
        let how = match mode {
            "ro" => "can read",
            "create" => "can read, change and create",
            _ => "can read and change",
        };
        add("files", item.clone(), format!("It {how} {what}."));
    }
    for item in &metadata.devices {
        let text = match item.as_str() {
            "dri" => "It can use the graphics processor.".to_owned(),
            "all" => "It can use every device: cameras, microphones, USB devices and others."
                .to_owned(),
            "kvm" => "It can run virtual machines.".to_owned(),
            "input" => "It can read input devices directly.".to_owned(),
            "usb" => "It can use USB devices.".to_owned(),
            "shm" => "It can use shared memory with other programs.".to_owned(),
            other => format!("It can use the device \"{other}\"."),
        };
        add("devices", item.clone(), text);
    }
    for item in &metadata.shared {
        match item.as_str() {
            "network" => add(
                "network",
                item.clone(),
                "It can use the network.".to_owned(),
            ),
            "ipc" => add(
                "other",
                item.clone(),
                "It shares memory with the display server (needed by X11 programs).".to_owned(),
            ),
            other => add("other", other.to_owned(), format!("It shares \"{other}\".")),
        }
    }
    for item in &metadata.sockets {
        let (kind, text) = match item.as_str() {
            "x11" => (
                "display",
                "It draws its windows through X11, which lets it see other X11 windows and keystrokes.",
            ),
            "fallback-x11" => (
                "display",
                "It draws its windows through X11 only when no Wayland display is running.",
            ),
            "wayland" => ("display", "It draws its windows on the Wayland display."),
            "pulseaudio" => ("sound", "It can play sound and record from microphones."),
            "session-bus" => (
                "services",
                "It can talk to every service of your desktop session.",
            ),
            "system-bus" => ("services", "It can talk to every system service."),
            "ssh-auth" => ("other", "It can use your SSH keys through the SSH agent."),
            "gpg-agent" => ("other", "It can use your GPG keys through the GPG agent."),
            "cups" => ("devices", "It can print."),
            "pcsc" => ("devices", "It can use smart cards."),
            _ => ("other", "It uses a socket this program does not describe."),
        };
        add(kind, item.clone(), text.to_owned());
    }
    for (name, policy) in &metadata.session_bus {
        add(
            "services",
            format!("session-bus:{name}={policy}"),
            format!("It can {policy} the desktop service {name}."),
        );
    }
    for (name, policy) in &metadata.system_bus {
        add(
            "services",
            format!("system-bus:{name}={policy}"),
            format!("It can {policy} the system service {name}."),
        );
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_is_per_user_and_asks_nothing() {
        let reference = "app/org.example.App/x86_64/stable";
        for arguments in [
            remotes_arguments(),
            remote_add_arguments(&KNOWN_SOURCES[0]),
            remote_modify_arguments("flathub", false),
            remote_metadata_arguments("flathub", reference),
            remote_commit_arguments("flathub", reference),
            install_arguments("flathub", reference),
            update_arguments(reference),
            uninstall_arguments(&[reference.to_owned()], true),
            list_refs_arguments(),
            list_apps_arguments(),
            size_arguments(reference),
            commit_arguments(reference),
            updates_arguments(),
            run_arguments("org.example.App"),
        ] {
            assert!(arguments.contains(&"--user".to_owned()), "{arguments:?}");
            assert!(!arguments.contains(&"--system".to_owned()));
        }
        for arguments in [
            install_arguments("flathub", reference),
            update_arguments(reference),
            uninstall_arguments(&[reference.to_owned()], false),
        ] {
            assert!(arguments.contains(&"--noninteractive".to_owned()));
        }
        assert!(!uninstall_arguments(&[reference.to_owned()], false).contains(&"--delete-data".to_owned()));
        assert_eq!(
            remote_add_arguments(&KNOWN_SOURCES[0]),
            [
                "remote-add",
                "--user",
                "--if-not-exists",
                "--title=Flathub",
                "flathub",
                "https://dl.flathub.org/repo/flathub.flatpakrepo"
            ]
        );
    }

    #[test]
    fn tables_are_tab_separated_without_a_header() {
        let text = "flathub\thttps://dl.flathub.org/repo/\t\nfedora\toci+https://registry.fedoraproject.org\tdisabled,oci\n";
        let remotes = parse_remotes(text);
        assert_eq!(remotes.len(), 2);
        assert_eq!(remotes[0].name, "flathub");
        assert!(!remotes[0].disabled);
        assert!(remotes[1].disabled);
        assert!(parse_remotes("").is_empty());

        let apps = parse_apps(
            "net.sourceforge.Klavaro\t3.14\tstable\tflathub\torg.freedesktop.Platform/x86_64/24.08\norg.example.App\t-\tstable\tflathub\t-\n",
        );
        assert_eq!(apps[0].application, "net.sourceforge.Klavaro");
        assert_eq!(apps[0].version.as_deref(), Some("3.14"));
        assert_eq!(
            apps[0].runtime.as_deref(),
            Some("org.freedesktop.Platform/x86_64/24.08")
        );
        assert_eq!(apps[1].version, None);

        let refs = parse_refs("app/a.b.C/x86_64/stable\nruntime/org.freedesktop.Platform/x86_64/24.08\n");
        assert_eq!(refs.len(), 2);
        assert_eq!(ref_id(&refs[1]), "org.freedesktop.Platform");
        assert_eq!(parse_size("2523136\n"), Some(2_523_136));
        assert_eq!(parse_size("2.5 MB"), None);
        let commit = "a".repeat(64);
        assert_eq!(parse_commit(&format!("{commit}\n")), Some(commit));
        assert_eq!(parse_commit("error"), None);
    }

    #[test]
    fn the_quiet_transaction_s_lines_are_activities() {
        assert_eq!(
            activity("Installing app/net.sourceforge.Klavaro/x86_64/stable"),
            Some((
                "installing",
                "app/net.sourceforge.Klavaro/x86_64/stable".to_owned()
            ))
        );
        assert_eq!(
            activity("Uninstalling runtime/org.freedesktop.Platform/x86_64/24.08"),
            Some((
                "uninstalling",
                "runtime/org.freedesktop.Platform/x86_64/24.08".to_owned()
            ))
        );
        assert_eq!(activity("Installing…"), None);
        assert_eq!(activity("Info: x was skipped"), None);
    }

    #[test]
    fn permissions_are_read_from_the_metadata_and_described() {
        let text = "[Application]\nname=net.sourceforge.Klavaro\nruntime=org.freedesktop.Platform/x86_64/24.08\ncommand=klavaro\n\n[Context]\nshared=network;ipc;\nsockets=x11;wayland;pulseaudio;\ndevices=dri;\nfilesystems=xdg-documents:ro;home;!host;\n\n[Session Bus Policy]\norg.freedesktop.Notifications=talk\n";
        let metadata = parse_metadata(text);
        assert_eq!(metadata.command.as_deref(), Some("klavaro"));
        assert_eq!(metadata.sockets, ["x11", "wayland", "pulseaudio"]);
        assert_eq!(metadata.filesystems, ["xdg-documents:ro", "home", "!host"]);
        let listed = access(&metadata);
        let text_of = |permission: &str| {
            listed
                .iter()
                .find(|item| item.permission == permission)
                .map(|item| item.text.clone())
                .unwrap_or_default()
        };
        assert_eq!(text_of("xdg-documents:ro"), "It can read xdg-documents.");
        assert_eq!(text_of("home"), "It can read and change every file in your home folder.");
        assert!(!listed.iter().any(|item| item.permission == "!host"));
        assert_eq!(text_of("network"), "It can use the network.");
        assert!(text_of("x11").contains("keystrokes"));
        assert!(text_of("session-bus:org.freedesktop.Notifications=talk").contains("talk"));
        assert!(listed.iter().any(|item| item.kind == "sound"));
        assert!(access(&Metadata::default()).is_empty());
    }
}
