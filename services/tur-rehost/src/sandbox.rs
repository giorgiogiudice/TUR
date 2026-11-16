// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The two bubblewrap sandboxes `tur-rehost` uses.
//!
//! - **The tool sandbox**, for every archive tool that opens a package or a
//!   download (bsdtar, 7-Zip, innoextract, msiextract, xz): no network, no
//!   home directory, nothing writable but the one output directory, the
//!   input file read-only. A hostile archive that climbs out with `..` or a
//!   link lands in a file system where nothing else is writable.
//! - **The application sandbox**, for the re-hosted application (the same
//!   boundary as a Windows application's, runtime-manager.md, "The
//!   sandbox"): `/usr` and `/etc` read-only, the application's own
//!   directory read-only, its private home read-write, the session's
//!   display and sound sockets one by one, the network shared. It does not
//!   see the person's home directory, the session bus, the keyring, other
//!   applications or TUR's records. Chromium's own sandbox stays on: user
//!   namespaces stay available inside (no `--disable-userns`), and
//!   `--no-sandbox` is never passed.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where an X authority file is shown inside the application sandbox.
pub const XAUTHORITY_INSIDE: &str = "/tmp/.tur-Xauthority";

/// The directories of a merged-`/usr` root, recreated as the links (or
/// directories) they are on the host.
const ROOT_LINKS: [&str; 5] = ["/bin", "/sbin", "/lib", "/lib64", "/lib32"];

/// Variables copied from the caller when set and free of control
/// characters. Nothing else is inherited.
pub const COPIED_VARIABLES: [&str; 13] = [
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "TZ",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "DRI_PRIME",
];

/// The parts of the person's session an application may be shown, read
/// from the environment and checked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Session {
    /// `XDG_RUNTIME_DIR`, when absolute.
    pub runtime_dir: Option<PathBuf>,
    /// `WAYLAND_DISPLAY`, only as a plain socket name in the runtime
    /// directory.
    pub wayland: Option<String>,
    /// The number of a local X display (`DISPLAY=:0`).
    pub x_display: Option<u32>,
    /// The X authority file, only `$HOME/.Xauthority` or a file directly in
    /// the runtime directory.
    pub xauthority: Option<PathBuf>,
    /// The accessibility bus socket from `AT_SPI_BUS_ADDRESS`
    /// (`unix:path=...`), so that P@x and screen readers see the
    /// application, as they see the publisher's Linux build.
    pub accessibility_socket: Option<PathBuf>,
    /// The user's login name.
    pub user: Option<String>,
    /// [`COPIED_VARIABLES`] that are set.
    pub copied: Vec<(String, String)>,
}

impl Session {
    /// Reads the session from the environment. Values that do not pass the
    /// rules above are dropped, never mounted.
    pub fn from_environment() -> Session {
        let get = |name: &str| std::env::var(name).ok().filter(|value| clean(value));
        let home = get("HOME").map(PathBuf::from);
        let runtime_dir = get("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        let wayland = get("WAYLAND_DISPLAY")
            .filter(|name| !name.is_empty() && !name.contains('/') && name != "." && name != "..");
        let x_display = get("DISPLAY").and_then(|value| local_display(&value));
        let xauthority = get("XAUTHORITY").map(PathBuf::from).filter(|path| {
            let in_home = home
                .as_ref()
                .is_some_and(|home| *path == home.join(".Xauthority"));
            let in_runtime = runtime_dir
                .as_ref()
                .is_some_and(|runtime| path.parent() == Some(runtime.as_path()));
            in_home || in_runtime
        });
        let accessibility_socket = get("AT_SPI_BUS_ADDRESS").and_then(|address| {
            let path = address
                .strip_prefix("unix:path=")?
                .split(',')
                .next()?
                .to_owned();
            let path = PathBuf::from(path);
            let in_runtime = runtime_dir
                .as_ref()
                .is_some_and(|runtime| path.starts_with(runtime));
            (path.is_absolute() && in_runtime && !path.components().any(|c| c.as_os_str() == ".."))
                .then_some(path)
        });
        let copied = COPIED_VARIABLES
            .iter()
            .filter_map(|name| get(name).map(|value| ((*name).to_owned(), value)))
            .collect();
        Session {
            runtime_dir,
            wayland,
            x_display,
            xauthority,
            accessibility_socket,
            user: get("USER").filter(|user| !user.is_empty()),
            copied,
        }
    }
}

fn clean(value: &str) -> bool {
    !value.chars().any(char::is_control)
}

/// The display number of a local X display: `:0`, `:1.0`, `unix:0`.
fn local_display(value: &str) -> Option<u32> {
    let rest = value.strip_prefix("unix").unwrap_or(value);
    let number = rest.strip_prefix(':')?;
    let number = number.split('.').next()?;
    if number.is_empty() || number.len() > 5 || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// How the host's root links look: for each of `/bin`, `/sbin`, `/lib`,
/// `/lib64`, `/lib32`, the link target when it is a link, `None` when it is
/// a directory; absent when it does not exist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RootLayout {
    /// `(path, link target)` pairs.
    pub entries: Vec<(String, Option<String>)>,
}

impl RootLayout {
    /// The layout of this machine.
    pub fn of_host() -> RootLayout {
        let mut entries = Vec::new();
        for path in ROOT_LINKS {
            match std::fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    if let Ok(target) = std::fs::read_link(path)
                        && let Some(text) = target.to_str()
                    {
                        entries.push((path.to_owned(), Some(text.to_owned())));
                    }
                }
                Ok(metadata) if metadata.is_dir() => entries.push((path.to_owned(), None)),
                _ => {}
            }
        }
        RootLayout { entries }
    }

    /// A merged-`/usr` layout, for tests.
    pub fn merged() -> RootLayout {
        RootLayout {
            entries: vec![
                ("/bin".to_owned(), Some("usr/bin".to_owned())),
                ("/sbin".to_owned(), Some("usr/sbin".to_owned())),
                ("/lib".to_owned(), Some("usr/lib".to_owned())),
                ("/lib64".to_owned(), Some("usr/lib64".to_owned())),
            ],
        }
    }

    fn push(&self, arguments: &mut Vec<String>) {
        for (path, target) in &self.entries {
            match target {
                Some(target) => push3(arguments, "--symlink", target, path),
                None => push3(arguments, "--ro-bind", path, path),
            }
        }
    }
}

fn push3(arguments: &mut Vec<String>, option: &str, first: &str, second: &str) {
    arguments.push(option.to_owned());
    arguments.push(first.to_owned());
    arguments.push(second.to_owned());
}

fn push_base(arguments: &mut Vec<String>, root: &RootLayout) {
    push3(arguments, "--ro-bind", "/usr", "/usr");
    root.push(arguments);
    push3(arguments, "--ro-bind", "/etc", "/etc");
    arguments.extend(["--proc".to_owned(), "/proc".to_owned()]);
    arguments.extend(["--dev".to_owned(), "/dev".to_owned()]);
    arguments.extend(["--tmpfs".to_owned(), "/tmp".to_owned()]);
}

/// The bubblewrap arguments of the tool sandbox, up to and including `--`:
/// `inputs` are shown read-only at their own paths, `output` read-write,
/// and the tool starts in `output`. Paths must be valid UTF-8 (the caller
/// checks).
pub fn tool_arguments(root: &RootLayout, inputs: &[&str], output: &str) -> Vec<String> {
    let mut arguments: Vec<String> = [
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--disable-userns",
    ]
    .iter()
    .map(|flag| (*flag).to_owned())
    .collect();
    // --disable-userns needs a user namespace of its own.
    arguments.insert(1, "--unshare-user".to_owned());
    push_base(&mut arguments, root);
    for input in inputs {
        push3(&mut arguments, "--ro-bind", input, input);
    }
    push3(&mut arguments, "--bind", output, output);
    arguments.extend(["--chdir".to_owned(), output.to_owned()]);
    arguments.push("--".to_owned());
    arguments
}

/// The environment of a tool in the tool sandbox.
pub fn tool_environment() -> Vec<(String, String)> {
    vec![
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
        ("HOME".to_owned(), "/tmp".to_owned()),
    ]
}

/// What the application sandbox is built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSandbox<'a> {
    /// The application's `app/` directory, shown read-only.
    pub app: &'a str,
    /// Its private home, shown read-write.
    pub home: &'a str,
    /// The session.
    pub session: &'a Session,
    /// The host's root layout.
    pub root: &'a RootLayout,
}

/// The bubblewrap arguments of the application sandbox, up to and
/// including `--`, and the application's whole environment.
pub fn app_arguments(sandbox: &AppSandbox<'_>) -> (Vec<String>, Vec<(String, String)>) {
    let mut arguments: Vec<String> = [
        "--unshare-user",
        "--unshare-ipc",
        "--unshare-pid",
        "--unshare-uts",
        "--unshare-cgroup-try",
        "--die-with-parent",
        "--new-session",
    ]
    .iter()
    .map(|flag| (*flag).to_owned())
    .collect();
    push_base(&mut arguments, sandbox.root);
    // Chromium keeps its shared memory here.
    arguments.extend(["--tmpfs".to_owned(), "/dev/shm".to_owned()]);
    push3(&mut arguments, "--dev-bind-try", "/dev/dri", "/dev/dri");
    for path in ["/sys/block", "/sys/bus", "/sys/class", "/sys/dev", "/sys/devices"] {
        push3(&mut arguments, "--ro-bind-try", path, path);
    }
    push3(
        &mut arguments,
        "--ro-bind-try",
        "/run/systemd/resolve",
        "/run/systemd/resolve",
    );

    let session = sandbox.session;
    let mut env: Vec<(String, String)> = vec![
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ("HOME".to_owned(), sandbox.home.to_owned()),
    ];
    if let Some(user) = &session.user {
        env.push(("USER".to_owned(), user.clone()));
        env.push(("LOGNAME".to_owned(), user.clone()));
    }
    env.extend(session.copied.iter().cloned());

    if let Some(runtime) = session.runtime_dir.as_ref().and_then(|path| path.to_str()) {
        arguments.extend([
            "--perms".to_owned(),
            "0700".to_owned(),
            "--tmpfs".to_owned(),
            runtime.to_owned(),
        ]);
        env.push(("XDG_RUNTIME_DIR".to_owned(), runtime.to_owned()));
        if let Some(name) = &session.wayland {
            let socket = format!("{runtime}/{name}");
            push3(&mut arguments, "--ro-bind-try", &socket, &socket);
            env.push(("WAYLAND_DISPLAY".to_owned(), name.clone()));
        }
        for sound in ["pipewire-0", "pulse/native"] {
            let socket = format!("{runtime}/{sound}");
            push3(&mut arguments, "--ro-bind-try", &socket, &socket);
        }
        if let Some(socket) = session.accessibility_socket.as_ref().and_then(|p| p.to_str()) {
            push3(&mut arguments, "--ro-bind-try", socket, socket);
            env.push(("AT_SPI_BUS_ADDRESS".to_owned(), format!("unix:path={socket}")));
        }
    }
    if let Some(number) = session.x_display {
        let socket = format!("/tmp/.X11-unix/X{number}");
        push3(&mut arguments, "--ro-bind-try", &socket, &socket);
        env.push(("DISPLAY".to_owned(), format!(":{number}")));
        if let Some(file) = session.xauthority.as_ref().and_then(|p| p.to_str()) {
            push3(&mut arguments, "--ro-bind-try", file, XAUTHORITY_INSIDE);
            env.push(("XAUTHORITY".to_owned(), XAUTHORITY_INSIDE.to_owned()));
        }
    }

    push3(&mut arguments, "--ro-bind", sandbox.app, sandbox.app);
    push3(&mut arguments, "--bind", sandbox.home, sandbox.home);
    arguments.extend(["--chdir".to_owned(), sandbox.app.to_owned()]);
    arguments.push("--".to_owned());
    (arguments, env)
}

/// Whether the effective user is root, read from `/proc/self/status`
/// (`Uid:` real, effective, saved, file system). Unknown counts as root:
/// the check refuses rather than guesses.
pub fn running_as_root() -> bool {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return true;
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|ids| ids.split_whitespace().nth(1).map(|id| id == "0"))
        .unwrap_or(true)
}

/// A path as text, for a bubblewrap argument.
pub fn text(path: &Path) -> Option<&str> {
    path.to_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(arguments: &[String], option: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < arguments.len() {
            if arguments[at] == option && at + 2 < arguments.len() {
                out.push((arguments[at + 1].clone(), arguments[at + 2].clone()));
                at += 3;
            } else {
                at += 1;
            }
        }
        out
    }

    fn session() -> Session {
        Session {
            runtime_dir: Some(PathBuf::from("/run/user/1000")),
            wayland: Some("wayland-0".to_owned()),
            x_display: Some(0),
            xauthority: Some(PathBuf::from("/run/user/1000/xauth_abc")),
            accessibility_socket: Some(PathBuf::from("/run/user/1000/at-spi/bus_0")),
            user: Some("person".to_owned()),
            copied: vec![("LANG".to_owned(), "en_GB.UTF-8".to_owned())],
        }
    }

    #[test]
    fn the_application_sees_its_own_directory_read_only_and_writes_only_its_home() {
        let session = session();
        let root = RootLayout::merged();
        let (arguments, env) = app_arguments(&AppSandbox {
            app: "/home/person/.local/share/tur/rehost/apps/com.example.app/app",
            home: "/home/person/.local/share/tur/rehost/apps/com.example.app/home",
            session: &session,
            root: &root,
        });
        let writable = pairs(&arguments, "--bind");
        assert_eq!(
            writable,
            vec![(
                "/home/person/.local/share/tur/rehost/apps/com.example.app/home".to_owned(),
                "/home/person/.local/share/tur/rehost/apps/com.example.app/home".to_owned()
            )]
        );
        let read_only = pairs(&arguments, "--ro-bind");
        assert!(read_only.contains(&(
            "/home/person/.local/share/tur/rehost/apps/com.example.app/app".to_owned(),
            "/home/person/.local/share/tur/rehost/apps/com.example.app/app".to_owned()
        )));
        // Nothing of the person's home, and never the record beside the app.
        for (source, _) in read_only.iter().chain(writable.iter()) {
            assert!(
                !source.ends_with("record.json") && source != "/home/person",
                "{source}"
            );
        }
        // Chromium's sandbox needs user namespaces: never disabled here.
        assert!(!arguments.contains(&"--disable-userns".to_owned()));
        assert!(!arguments.contains(&"--no-sandbox".to_owned()));
        // The network is shared: these applications need it.
        assert!(!arguments.contains(&"--unshare-net".to_owned()));
        assert_eq!(arguments.last().map(String::as_str), Some("--"));
        let names: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
        for expected in ["PATH", "HOME", "WAYLAND_DISPLAY", "DISPLAY", "XAUTHORITY", "AT_SPI_BUS_ADDRESS", "LANG"] {
            assert!(names.contains(&expected), "{expected} missing");
        }
        assert!(!names.contains(&"DBUS_SESSION_BUS_ADDRESS"));
        assert!(!names.contains(&"SSH_AUTH_SOCK"));
    }

    #[test]
    fn the_runtime_directory_is_empty_but_for_the_session_s_own_sockets() {
        let session = session();
        let root = RootLayout::merged();
        let (arguments, _) = app_arguments(&AppSandbox {
            app: "/a/app",
            home: "/a/home",
            session: &session,
            root: &root,
        });
        let tmpfs_at = arguments
            .windows(2)
            .position(|pair| pair[0] == "--tmpfs" && pair[1] == "/run/user/1000")
            .expect("a private runtime directory");
        for (source, _) in pairs(&arguments, "--ro-bind-try") {
            if source.starts_with("/run/user/1000") {
                let at = arguments.iter().position(|argument| *argument == source).unwrap();
                assert!(at > tmpfs_at, "{source} is mounted before the runtime directory");
                assert!(
                    source.ends_with("wayland-0")
                        || source.ends_with("pipewire-0")
                        || source.ends_with("pulse/native")
                        || source.ends_with("at-spi/bus_0")
                        || source.ends_with("xauth_abc"),
                    "{source}"
                );
            }
        }
    }

    #[test]
    fn a_tool_has_no_network_and_writes_only_its_output() {
        let arguments = tool_arguments(&RootLayout::merged(), &["/w/in.zip"], "/w/out");
        assert!(arguments.contains(&"--unshare-all".to_owned()));
        assert!(arguments.contains(&"--disable-userns".to_owned()));
        assert_eq!(pairs(&arguments, "--bind"), vec![("/w/out".to_owned(), "/w/out".to_owned())]);
        assert!(pairs(&arguments, "--ro-bind").contains(&("/w/in.zip".to_owned(), "/w/in.zip".to_owned())));
        assert_eq!(arguments.last().map(String::as_str), Some("--"));
    }

    #[test]
    fn display_values_are_taken_only_when_local() {
        assert_eq!(local_display(":0"), Some(0));
        assert_eq!(local_display(":12.0"), Some(12));
        assert_eq!(local_display("unix:1"), Some(1));
        assert_eq!(local_display("remote.example:0"), None);
        assert_eq!(local_display(":../0"), None);
        assert_eq!(local_display(""), None);
    }
}
