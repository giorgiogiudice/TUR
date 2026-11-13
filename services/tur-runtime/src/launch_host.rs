// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Facts about the machine and the session that a launch is planned for.
//!
//! Planning is a pure function of a [`LaunchHost`]. [`LaunchHost::detect`]
//! gathers the facts by reading the environment and the file system; the only
//! program it can start is the runtime itself, to ask for its version
//! ([`probe_wine_version`]), and only when the caller asks.
//!
//! Nothing from the caller's environment reaches an application unless it is
//! named here: [`PASSTHROUGH`] lists the variables copied as they are, and
//! the display, audio and authority settings are rebuilt from values that
//! have been checked.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::limits::{MAX_ENVIRONMENT_VALUE_BYTES, MAX_VERSION_OUTPUT_BYTES};
use crate::metadata::{NtsyncDevice, is_build_text};
use crate::paths::plain_text;
use crate::profile::is_version;

/// Environment variables copied from the caller to the application when they
/// are set: locale, time zone, the kind of session, and GPU selection.
pub const PASSTHROUGH: [&str; 16] = [
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
    "__NV_PRIME_RENDER_OFFLOAD",
    "__GLX_VENDOR_LIBRARY_NAME",
    "__VK_LAYER_NV_optimus",
];

/// Directories searched for the sandbox and runtime launchers. They are
/// system directories, so what is found there is also visible inside the
/// sandbox.
pub const PROGRAM_DIRECTORIES: [&str; 3] = ["/usr/bin", "/usr/local/bin", "/bin"];

/// Top-level entries that are links into `/usr` on a merged-`/usr` system and
/// directories of their own elsewhere.
const SYSTEM_ENTRIES: [&str; 6] = ["/bin", "/sbin", "/lib", "/lib64", "/lib32", "/libx32"];

/// The `ntsync` character device (specification §8.2).
const NTSYNC_DEVICE: &str = "/dev/ntsync";

/// A Wine installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WineInstall {
    /// The `wine` launcher.
    pub program: PathBuf,
    /// The `wineserver` program, when it was found beside the launcher.
    pub server: Option<PathBuf>,
}

/// What a runtime reported about its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeVersion {
    /// The version as a token, such as `wine-9.0`.
    pub version: String,
    /// The whole first line of the report.
    pub build: String,
}

/// How a top-level system entry is reproduced inside the sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemEntry {
    /// A real directory, mounted read-only.
    Directory(String),
    /// A symbolic link, recreated with the same target.
    Link {
        /// The entry, such as `/bin`.
        path: String,
        /// The link's target, such as `usr/bin`.
        target: String,
    },
}

/// A local X11 display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11Display {
    /// The value of `DISPLAY`, such as `:0`.
    pub display: String,
    /// The display's socket, such as `/tmp/.X11-unix/X0`.
    pub socket: PathBuf,
}

/// A Wayland display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaylandDisplay {
    /// The value of `WAYLAND_DISPLAY`, a socket name such as `wayland-0`.
    pub name: String,
    /// The socket in the user's runtime directory.
    pub socket: PathBuf,
}

/// The display and audio endpoints of the session that exist right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sockets {
    /// The local X11 display, if there is one.
    pub x11: Option<X11Display>,
    /// The X authority file, when it is in one of the places a session puts
    /// it.
    pub xauthority: Option<PathBuf>,
    /// The Wayland display, if there is one.
    pub wayland: Option<WaylandDisplay>,
    /// The PipeWire socket.
    pub pipewire: Option<PathBuf>,
    /// The PulseAudio-protocol socket.
    pub pulse: Option<PathBuf>,
}

/// The machine and session a launch is planned for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchHost {
    /// The effective user ID. Nothing is launched for user 0.
    pub euid: u32,
    /// The user's login name.
    pub user: String,
    /// The user's home directory.
    pub home: PathBuf,
    /// The user's `XDG_RUNTIME_DIR`.
    pub runtime_dir: Option<PathBuf>,
    /// The SSH agent socket, which is never shown to an application.
    pub agent_socket: Option<PathBuf>,
    /// The variables of [`PASSTHROUGH`] that are set, with their values.
    pub passthrough: BTreeMap<String, String>,
    /// The `bwrap` program.
    pub bwrap: Option<PathBuf>,
    /// The Wine installation.
    pub wine: Option<WineInstall>,
    /// The `umu-run` program.
    pub umu_run: Option<PathBuf>,
    /// The `tar` program, used for backups.
    pub tar: Option<PathBuf>,
    /// The `unzip` program, used to unpack a portable application.
    pub unzip: Option<PathBuf>,
    /// The `install` program, used to copy a program file into a prefix.
    pub install: Option<PathBuf>,
    /// e2fsprogs' `chattr`, used to make a new prefix's drive C:
    /// case-insensitive.
    pub chattr: Option<PathBuf>,
    /// e2fsprogs' `lsattr`, used to tell whether a prefix's drive C: is
    /// case-insensitive.
    pub lsattr: Option<PathBuf>,
    /// What looking for the `ntsync` device found.
    pub ntsync: NtsyncDevice,
    /// How `/bin`, `/lib` and the like are laid out.
    pub system: Vec<SystemEntry>,
    /// The session's display and audio endpoints.
    pub sockets: Sockets,
    /// This program's own file, which supervises Wine inside the sandbox.
    pub supervisor: PathBuf,
}

/// The effective user ID from the text of `/proc/self/status`.
pub fn effective_uid(status: &str) -> Option<u32> {
    let line = status.lines().find_map(|line| line.strip_prefix("Uid:"))?;
    // Real, effective, saved and file-system user IDs, in that order.
    line.split_whitespace().nth(1)?.parse().ok()
}

/// The effective user ID of this process, or `None` when it cannot be read.
pub fn current_effective_uid() -> Option<u32> {
    effective_uid(&fs::read_to_string("/proc/self/status").ok()?)
}

/// The display number of a local X11 display: `:0` and `:0.0` give 0. A
/// display on another host, or anything else, gives `None`.
pub fn x11_display_number(display: &str) -> Option<u32> {
    let rest = display.strip_prefix(':')?;
    let number = rest
        .split_once('.')
        .map_or(rest, |(number, _screen)| number);
    if number.is_empty() || number.len() > 5 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let screen_ok = rest.split_once('.').is_none_or(|(_, screen)| {
        !screen.is_empty() && screen.len() <= 3 && screen.bytes().all(|byte| byte.is_ascii_digit())
    });
    if screen_ok { number.parse().ok() } else { None }
}

/// Whether `name` is a Wayland socket name: a plain file name. The Wayland
/// protocol also allows an absolute path here; that form is not accepted,
/// because it would let the environment name any file to be mounted.
pub fn is_wayland_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Whether `value` names an X authority file in one of the places a session
/// puts one: `$HOME/.Xauthority`, or a file directly in the runtime directory
/// whose name starts with `xauth` or `.mutter-Xwaylandauth`. Any other value
/// is ignored, so the environment cannot name an arbitrary file to be shown
/// to the application.
pub fn is_known_xauthority(value: &str, home: &Path, runtime_dir: Option<&Path>) -> bool {
    let path = Path::new(value);
    if plain_text(path).is_none() {
        return false;
    }
    if path == home.join(".Xauthority") {
        return true;
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return false;
    };
    runtime_dir == Some(parent)
        && (name.starts_with("xauth") || name.starts_with(".mutter-Xwaylandauth"))
}

/// The first of `names` found as a regular file in `directories`.
pub fn find_program(directories: &[&str], names: &[&str]) -> Option<PathBuf> {
    directories.iter().find_map(|directory| {
        names
            .iter()
            .map(|name| Path::new(directory).join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// Reads a runtime's version from the output of `wine --version`.
///
/// The first line is taken, for example `wine-9.0 (Ubuntu 9.0~repack-4build3)`;
/// its first word is the version token.
pub fn parse_wine_version(output: &str) -> Option<RuntimeVersion> {
    let build = output.lines().next()?.trim();
    let version = build.split_whitespace().next()?;
    if !is_version(version) || !version.starts_with("wine-") || !is_build_text(build) {
        return None;
    }
    Some(RuntimeVersion {
        version: version.to_owned(),
        build: build.to_owned(),
    })
}

/// Asks Wine for its version by running `wine --version`.
///
/// The command gets an empty environment apart from `PATH` and does not
/// create or touch a prefix. Returns `None` when Wine cannot be started or
/// its answer is not understood.
pub fn probe_wine_version(wine: &Path) -> Option<RuntimeVersion> {
    let output = Command::new(wine)
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let bytes = output
        .stdout
        .get(..MAX_VERSION_OUTPUT_BYTES)
        .unwrap_or(output.stdout.as_slice());
    parse_wine_version(&String::from_utf8_lossy(bytes))
}

/// Looks for the `ntsync` device the way a runtime would use it: it has to be
/// a character device, and it has to open. The kernel version is not
/// consulted (specification §8.2).
pub fn probe_ntsync(device: &Path) -> NtsyncDevice {
    let is_device =
        fs::metadata(device).is_ok_and(|metadata| metadata.file_type().is_char_device());
    if !is_device {
        return NtsyncDevice::Absent;
    }
    // Each open of the device is an independent instance that is discarded
    // when it is closed, so opening it here changes nothing.
    match fs::OpenOptions::new().read(true).write(true).open(device) {
        Ok(_) => NtsyncDevice::Present,
        Err(_) => NtsyncDevice::Inaccessible,
    }
}

fn system_entries() -> Vec<SystemEntry> {
    let mut entries = Vec::new();
    for path in SYSTEM_ENTRIES {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(path).ok();
            let target = target.as_deref().and_then(Path::to_str);
            if let Some(target) = target.filter(|text| !text.chars().any(char::is_control)) {
                entries.push(SystemEntry::Link {
                    path: path.to_owned(),
                    target: target.to_owned(),
                });
            }
        } else if metadata.is_dir() {
            entries.push(SystemEntry::Directory(path.to_owned()));
        }
    }
    entries
}

fn is_login_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn usable_value(value: &str) -> bool {
    value.len() <= MAX_ENVIRONMENT_VALUE_BYTES && !value.chars().any(char::is_control)
}

impl LaunchHost {
    /// Detects the running machine and session.
    ///
    /// Reads the environment, `/proc/self/status` and the file system. Starts
    /// no program. Returns `None` when the home directory or the effective
    /// user cannot be determined.
    pub fn detect() -> Option<Self> {
        let variable = |name: &str| std::env::var(name).ok().filter(|value| usable_value(value));
        let absolute = |name: &str| {
            variable(name)
                .map(PathBuf::from)
                .filter(|path| plain_text(path).is_some())
        };

        let euid = current_effective_uid()?;
        let home = absolute("HOME")?;
        let runtime_dir = absolute("XDG_RUNTIME_DIR").filter(|path| path.is_dir());
        let user = variable("USER")
            .filter(|name| is_login_name(name))
            .unwrap_or_else(|| "user".to_owned());

        let passthrough = PASSTHROUGH
            .iter()
            .filter_map(|name| Some(((*name).to_owned(), variable(name)?)))
            .collect();

        let x11 = variable("DISPLAY").and_then(|display| {
            let number = x11_display_number(&display)?;
            let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
            socket.exists().then_some(X11Display { display, socket })
        });
        let xauthority = variable("XAUTHORITY")
            .filter(|value| is_known_xauthority(value, &home, runtime_dir.as_deref()))
            .map(PathBuf::from)
            .filter(|path| path.is_file());
        let in_runtime_dir = |relative: &str| {
            runtime_dir
                .as_ref()
                .map(|directory| directory.join(relative))
                .filter(|path| path.exists())
        };
        let wayland = variable("WAYLAND_DISPLAY")
            .filter(|name| is_wayland_name(name))
            .and_then(|name| {
                let socket = in_runtime_dir(name.as_str())?;
                Some(WaylandDisplay { name, socket })
            });
        let sockets = Sockets {
            x11,
            xauthority,
            wayland,
            pipewire: in_runtime_dir("pipewire-0"),
            pulse: in_runtime_dir("pulse/native"),
        };

        let wine = find_program(&PROGRAM_DIRECTORIES, &["wine", "wine64"]).map(|program| {
            let server = program
                .parent()
                .map(|directory| directory.join("wineserver"))
                .filter(|candidate| candidate.is_file());
            WineInstall { program, server }
        });

        Some(LaunchHost {
            euid,
            user,
            home,
            runtime_dir,
            agent_socket: absolute("SSH_AUTH_SOCK"),
            passthrough,
            bwrap: find_program(&PROGRAM_DIRECTORIES, &["bwrap"]),
            wine,
            umu_run: find_program(&PROGRAM_DIRECTORIES, &["umu-run"]),
            tar: find_program(&PROGRAM_DIRECTORIES, &["tar"]),
            unzip: find_program(&PROGRAM_DIRECTORIES, &["unzip"]),
            install: find_program(&PROGRAM_DIRECTORIES, &["install"]),
            chattr: find_program(&PROGRAM_DIRECTORIES, &["chattr"]),
            lsattr: find_program(&PROGRAM_DIRECTORIES, &["lsattr"]),
            ntsync: probe_ntsync(Path::new(NTSYNC_DEVICE)),
            system: system_entries(),
            sockets,
            supervisor: std::env::current_exe().ok()?,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testdata::TempDir;

    /// A host with Wine, `umu-run` and bubblewrap installed, a merged `/usr`,
    /// an X11 and a Wayland display, and no `ntsync` device.
    pub(crate) fn fixture(home: &Path, runtime_dir: &Path) -> LaunchHost {
        let mut passthrough = BTreeMap::new();
        passthrough.insert("LANG".to_owned(), "en_GB.UTF-8".to_owned());
        passthrough.insert("TZ".to_owned(), "Europe/London".to_owned());
        LaunchHost {
            euid: 1000,
            user: "ada".to_owned(),
            home: home.to_path_buf(),
            runtime_dir: Some(runtime_dir.to_path_buf()),
            agent_socket: Some(runtime_dir.join("ssh-agent.socket")),
            passthrough,
            bwrap: Some(PathBuf::from("/usr/bin/bwrap")),
            wine: Some(WineInstall {
                program: PathBuf::from("/usr/bin/wine"),
                server: Some(PathBuf::from("/usr/bin/wineserver")),
            }),
            umu_run: Some(PathBuf::from("/usr/bin/umu-run")),
            tar: Some(PathBuf::from("/usr/bin/tar")),
            unzip: Some(PathBuf::from("/usr/bin/unzip")),
            install: Some(PathBuf::from("/usr/bin/install")),
            chattr: Some(PathBuf::from("/usr/bin/chattr")),
            lsattr: Some(PathBuf::from("/usr/bin/lsattr")),
            ntsync: NtsyncDevice::Absent,
            system: vec![
                SystemEntry::Link {
                    path: "/bin".to_owned(),
                    target: "usr/bin".to_owned(),
                },
                SystemEntry::Link {
                    path: "/lib64".to_owned(),
                    target: "usr/lib64".to_owned(),
                },
                SystemEntry::Directory("/lib32".to_owned()),
            ],
            sockets: Sockets {
                x11: Some(X11Display {
                    display: ":0".to_owned(),
                    socket: PathBuf::from("/tmp/.X11-unix/X0"),
                }),
                xauthority: Some(runtime_dir.join("xauth_AbCdEf")),
                wayland: Some(WaylandDisplay {
                    name: "wayland-0".to_owned(),
                    socket: runtime_dir.join("wayland-0"),
                }),
                pipewire: Some(runtime_dir.join("pipewire-0")),
                pulse: Some(runtime_dir.join("pulse/native")),
            },
            supervisor: PathBuf::from("/usr/bin/tur-runtime"),
        }
    }

    #[test]
    fn effective_uid_is_the_second_number() {
        let status =
            "Name:\ttur-runtime\nUmask:\t0022\nUid:\t1000\t1001\t1002\t1003\nGid:\t5\t6\t7\t8\n";
        assert_eq!(effective_uid(status), Some(1001));
        assert_eq!(effective_uid("Uid:\t0\t0\t0\t0\n"), Some(0));
        assert_eq!(effective_uid("Name:\tx\n"), None);
        assert_eq!(effective_uid("Uid:\t1000\n"), None);
        assert_eq!(effective_uid("Uid:\ta\tb\tc\td\n"), None);
        assert_eq!(effective_uid(""), None);
        // The process running the tests has an effective user.
        assert!(current_effective_uid().is_some());
    }

    #[test]
    fn only_local_x11_displays_have_a_socket() {
        assert_eq!(x11_display_number(":0"), Some(0));
        assert_eq!(x11_display_number(":1.0"), Some(1));
        assert_eq!(x11_display_number(":99"), Some(99));
        for other in [
            "",
            ":",
            "0",
            "host:0",
            "unix:0",
            ":0.",
            ":.0",
            ":-1",
            ":0/../../x",
            ":0 ",
            ":123456",
            ":0.0.0",
            "/tmp/.X11-unix/X0",
        ] {
            assert_eq!(x11_display_number(other), None, "{other:?}");
        }
    }

    #[test]
    fn wayland_names_are_plain_file_names() {
        assert!(is_wayland_name("wayland-0"));
        assert!(is_wayland_name("wayland-1"));
        for other in [
            "",
            ".",
            "..",
            "../bus",
            "/run/user/1000/bus",
            "/home/ada/.ssh/id_ed25519",
            "wayland-0/..",
            ".hidden",
            "way land",
            "wayland\n",
        ] {
            assert!(!is_wayland_name(other), "{other:?}");
        }
        assert!(!is_wayland_name(&"w".repeat(65)));
    }

    #[test]
    fn only_session_authority_files_are_accepted() {
        let home = Path::new("/home/ada");
        let runtime = Some(Path::new("/run/user/1000"));
        assert!(is_known_xauthority("/home/ada/.Xauthority", home, runtime));
        assert!(is_known_xauthority(
            "/run/user/1000/xauth_AbCdEf",
            home,
            runtime
        ));
        assert!(is_known_xauthority(
            "/run/user/1000/.mutter-Xwaylandauth.ABC123",
            home,
            runtime
        ));
        for other in [
            "/home/ada/.ssh/id_ed25519",
            "/home/ada/.gnupg/private-keys-v1.d/key",
            "/run/user/1000/keyring/control",
            "/run/user/1000/bus",
            "/run/user/1000/xauth/../bus",
            "/run/user/1001/xauth_AbCdEf",
            "/etc/shadow",
            ".Xauthority",
            "",
            "/home/ada/.Xauthority/../.ssh/id_ed25519",
        ] {
            assert!(!is_known_xauthority(other, home, runtime), "{other:?}");
        }
        assert!(!is_known_xauthority(
            "/run/user/1000/xauth_AbCdEf",
            home,
            None
        ));
    }

    #[test]
    fn wine_versions() {
        assert_eq!(
            parse_wine_version("wine-9.0 (Ubuntu 9.0~repack-4build3)\n"),
            Some(RuntimeVersion {
                version: "wine-9.0".to_owned(),
                build: "wine-9.0 (Ubuntu 9.0~repack-4build3)".to_owned(),
            })
        );
        assert_eq!(
            parse_wine_version("wine-11.0\n").unwrap().version,
            "wine-11.0"
        );
        assert_eq!(
            parse_wine_version("wine-10.18 (Staging)\nsecond line\n")
                .unwrap()
                .build,
            "wine-10.18 (Staging)"
        );
        for other in [
            "",
            "\n",
            "9.0",
            "Wine 9.0",
            "wine-9.0\u{1b}[31m (x)",
            "wine-9.0;rm",
            "usage: wine PROGRAM",
        ] {
            assert_eq!(parse_wine_version(other), None, "{other:?}");
        }
        assert_eq!(
            parse_wine_version(&format!("wine-{}", "9".repeat(300))),
            None
        );
    }

    #[test]
    fn programs_are_found_in_the_given_directories_only() {
        let temp = TempDir::new("launch-host-programs");
        let directory = temp.path().join("bin");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("wine64"), b"").unwrap();
        fs::create_dir_all(directory.join("bwrap")).unwrap();
        let directory = directory.to_str().unwrap();
        assert_eq!(
            find_program(&["/nonexistent", directory], &["wine", "wine64"]),
            Some(Path::new(directory).join("wine64"))
        );
        // A directory with the right name is not a program.
        assert_eq!(find_program(&[directory], &["bwrap"]), None);
        assert_eq!(find_program(&[], &["wine"]), None);
    }

    #[test]
    fn the_ntsync_probe_needs_a_character_device() {
        let temp = TempDir::new("launch-host-ntsync");
        let regular = temp.path().join("ntsync");
        fs::write(&regular, b"").unwrap();
        // A regular file with the device's name is not the device.
        assert_eq!(probe_ntsync(&regular), NtsyncDevice::Absent);
        assert_eq!(
            probe_ntsync(&temp.path().join("missing")),
            NtsyncDevice::Absent
        );
        // A character device that opens is reported present.
        assert_eq!(probe_ntsync(Path::new("/dev/null")), NtsyncDevice::Present);
        // Whatever this machine has, the probe gives an answer.
        let real = probe_ntsync(Path::new(NTSYNC_DEVICE));
        assert_ne!(real, NtsyncDevice::NotChecked);
    }

    #[test]
    fn detection_does_not_panic_and_never_reports_root_by_mistake() {
        if let Some(host) = LaunchHost::detect() {
            assert_eq!(Some(host.euid), current_effective_uid());
            assert!(host.home.is_absolute());
            assert!(
                host.passthrough
                    .keys()
                    .all(|name| PASSTHROUGH.contains(&name.as_str()))
            );
            for entry in &host.system {
                match entry {
                    SystemEntry::Directory(path) | SystemEntry::Link { path, .. } => {
                        assert!(SYSTEM_ENTRIES.contains(&path.as_str()));
                    }
                }
            }
        }
    }

    #[test]
    fn values_with_control_characters_are_not_usable() {
        assert!(usable_value("en_GB.UTF-8"));
        assert!(!usable_value("a\nb"));
        assert!(!usable_value("a\u{1b}[0m"));
        assert!(!usable_value(&"x".repeat(MAX_ENVIRONMENT_VALUE_BYTES + 1)));
        assert!(is_login_name("ada"));
        assert!(!is_login_name(""));
        assert!(!is_login_name("ada lovelace"));
        assert!(!is_login_name("../root"));
    }
}
