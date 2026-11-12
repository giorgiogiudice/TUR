// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The least-privilege sandbox every Windows program is started in
//! (specification §12.1, §12.2).
//!
//! The sandbox is a bubblewrap command line. Inside it the application sees:
//!
//! * the system (`/usr`, `/etc`) read-only;
//! * its own prefix and its own private home directory, read-write;
//! * the directories the user has granted, and nothing else of the user's
//!   files: the real home directory is not there at all;
//! * a private `/tmp`, a private `/dev` and a `/proc` of its own processes;
//! * the session's display and audio sockets, the GPU device nodes and, when
//!   it exists, the `ntsync` device.
//!
//! It does not see the session bus, the keyring, the SSH or GnuPG agents,
//! browser profiles or any other application's prefix. There is no way to
//! start an application without the sandbox.
//!
//! [`build`] returns both the argument list and the list of [`Mount`]s it was
//! generated from, so that a plan can be inspected.

use std::fmt;
use std::path::Path;

use serde::Serialize;

use crate::codes::Code;
use crate::grant::{GrantContext, GrantRefusal};
use crate::launch_host::{LaunchHost, SystemEntry};
use crate::metadata::{Grant, GrantAccess, NtsyncDevice};
use crate::paths::{AppPaths, plain_text};

/// Where this program is mounted inside the sandbox, to supervise Wine.
pub const SUPERVISOR_PATH: &str = "/run/tur-runtime/supervisor";

/// Where the X authority file is mounted inside the sandbox.
pub const XAUTHORITY_PATH: &str = "/run/tur-runtime/Xauthority";

/// GPU device nodes passed through when they exist.
const GPU_DEVICES: [&str; 6] = [
    "/dev/dri",
    "/dev/nvidiactl",
    "/dev/nvidia0",
    "/dev/nvidia-modeset",
    "/dev/nvidia-uvm",
    "/dev/nvidia-uvm-tools",
];

/// Parts of `/sys` that graphics drivers and Wine read to find devices and
/// processors. They describe hardware and hold nothing of the user's.
const SYS_DIRECTORIES: [&str; 5] = [
    "/sys/block",
    "/sys/bus",
    "/sys/class",
    "/sys/dev",
    "/sys/devices",
];

/// Name-resolution state needed when the network is shared and
/// `/etc/resolv.conf` is a link into it.
const RESOLVER_DIRECTORY: &str = "/run/systemd/resolve";

/// How a path appears inside the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MountKind {
    /// A host path, read-only.
    ReadOnly,
    /// A host path, read-write.
    ReadWrite,
    /// A host device node or directory of device nodes.
    Device,
    /// An empty in-memory file system.
    Tmpfs,
    /// An empty in-memory file system only the user can enter.
    PrivateTmpfs,
    /// A `/proc` that shows the sandbox's own processes.
    Proc,
    /// A minimal `/dev`.
    Dev,
    /// A symbolic link created inside the sandbox.
    Link,
}

/// One entry of the sandbox's file-system view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mount {
    /// How the path appears.
    pub kind: MountKind,
    /// The host path that is mounted, or the target of a link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The path inside the sandbox.
    pub target: String,
    /// Whether the entry is skipped when the host path does not exist.
    pub optional: bool,
    /// Why the entry is there.
    pub purpose: &'static str,
}

impl Mount {
    fn host(kind: MountKind, path: &str, optional: bool, purpose: &'static str) -> Self {
        Mount {
            kind,
            source: Some(path.to_owned()),
            target: path.to_owned(),
            optional,
            purpose,
        }
    }

    fn virtual_fs(kind: MountKind, target: &str, purpose: &'static str) -> Self {
        Mount {
            kind,
            source: None,
            target: target.to_owned(),
            optional: false,
            purpose,
        }
    }

    /// Appends the bubblewrap options for this entry.
    fn push_arguments(&self, arguments: &mut Vec<String>) {
        let source = self.source.clone().unwrap_or_default();
        let option = match (self.kind, self.optional) {
            (MountKind::ReadOnly, false) => "--ro-bind",
            (MountKind::ReadOnly, true) => "--ro-bind-try",
            (MountKind::ReadWrite, _) => "--bind",
            (MountKind::Device, false) => "--dev-bind",
            (MountKind::Device, true) => "--dev-bind-try",
            (MountKind::Tmpfs, _) => "--tmpfs",
            (MountKind::PrivateTmpfs, _) => {
                arguments.extend(["--perms".to_owned(), "0700".to_owned()]);
                "--tmpfs"
            }
            (MountKind::Proc, _) => "--proc",
            (MountKind::Dev, _) => "--dev",
            (MountKind::Link, _) => "--symlink",
        };
        arguments.push(option.to_owned());
        if self.source.is_some() {
            arguments.push(source);
        }
        arguments.push(self.target.clone());
    }
}

/// Why a sandbox could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxError {
    /// bubblewrap is not installed.
    Missing,
    /// A recorded grant is refused.
    Grant {
        /// The granted path.
        path: String,
        /// Why it is refused now.
        refusal: GrantRefusal,
    },
    /// A path that has to be mounted is not a plain absolute path.
    Path {
        /// What the path is for.
        role: &'static str,
    },
}

impl SandboxError {
    /// The stable error code for this failure.
    pub fn code(&self) -> Code {
        match self {
            SandboxError::Missing => Code::SandboxMissing,
            SandboxError::Grant { .. } => Code::GrantChanged,
            SandboxError::Path { .. } => Code::StorageUnusable,
        }
    }
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxError::Missing => f.write_str(
                "bubblewrap (bwrap) is not installed, and nothing is started without the sandbox",
            ),
            SandboxError::Grant { path, refusal } => write!(
                f,
                "the granted directory {path:?} can no longer be shown to the application: \
                 {refusal}"
            ),
            SandboxError::Path { role } => write!(
                f,
                "the path of {role} is not a plain absolute path, so it cannot be mounted"
            ),
        }
    }
}

impl std::error::Error for SandboxError {}

/// What a sandbox is built from.
pub struct SandboxRequest<'a> {
    /// The machine and session.
    pub host: &'a LaunchHost,
    /// The application's own paths.
    pub app: &'a AppPaths,
    /// The grant rules for this user.
    pub context: &'a GrantContext,
    /// The recorded grants. Each is checked again here.
    pub grants: &'a [Grant],
    /// Whether the application shares the host's network.
    pub network: bool,
    /// Whether the program inside may create user namespaces of its own.
    /// Proton's launcher needs them; plain Wine does not.
    pub nested_namespaces: bool,
    /// The application's program file, when it lies outside the prefix.
    pub program_file: Option<&'a Path>,
    /// Whether this program is mounted inside to supervise Wine.
    pub supervisor: bool,
    /// The directory the application starts in, inside the sandbox.
    pub working_directory: &'a Path,
}

/// A sandbox: the bubblewrap command line up to and including `--`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    /// The `bwrap` program.
    pub program: String,
    /// Its arguments, ending with `--`. The command to run follows.
    pub arguments: Vec<String>,
    /// The file-system view the arguments were generated from.
    pub mounts: Vec<Mount>,
}

fn text<'a>(path: &'a Path, role: &'static str) -> Result<&'a str, SandboxError> {
    plain_text(path).ok_or(SandboxError::Path { role })
}

/// Builds the sandbox for one application.
///
/// # Errors
///
/// [`SandboxError`] when bubblewrap is missing, when a recorded grant no
/// longer passes the grant rules, or when a path cannot be mounted.
pub fn build(request: &SandboxRequest<'_>) -> Result<Sandbox, SandboxError> {
    let host = request.host;
    let program = host.bwrap.as_deref().ok_or(SandboxError::Missing)?;
    let program = text(program, "bubblewrap")?.to_owned();

    let mut mounts = vec![
        Mount::host(MountKind::ReadOnly, "/usr", false, "system"),
        Mount::host(MountKind::ReadOnly, "/etc", false, "system configuration"),
    ];
    for entry in &host.system {
        mounts.push(match entry {
            SystemEntry::Directory(path) => Mount::host(MountKind::ReadOnly, path, false, "system"),
            SystemEntry::Link { path, target } => Mount {
                kind: MountKind::Link,
                source: Some(target.clone()),
                target: path.clone(),
                optional: false,
                purpose: "system",
            },
        });
    }
    mounts.push(Mount::virtual_fs(MountKind::Proc, "/proc", "own processes"));
    mounts.push(Mount::virtual_fs(MountKind::Dev, "/dev", "minimal devices"));
    for device in GPU_DEVICES {
        mounts.push(Mount::host(MountKind::Device, device, true, "GPU"));
    }
    if host.ntsync == NtsyncDevice::Present {
        mounts.push(Mount::host(
            MountKind::Device,
            "/dev/ntsync",
            false,
            "Windows synchronisation driver",
        ));
    }
    for directory in SYS_DIRECTORIES {
        mounts.push(Mount::host(
            MountKind::ReadOnly,
            directory,
            true,
            "hardware description",
        ));
    }
    mounts.push(Mount::virtual_fs(
        MountKind::Tmpfs,
        "/tmp",
        "private temporary files",
    ));

    // The session: one socket at a time, never the directory that holds them.
    let sockets = &host.sockets;
    if let Some(x11) = &sockets.x11 {
        let socket = text(&x11.socket, "the X11 socket")?;
        mounts.push(Mount::host(
            MountKind::ReadOnly,
            socket,
            false,
            "X11 display",
        ));
    }
    if let Some(runtime_dir) = &host.runtime_dir {
        let runtime_dir = text(runtime_dir, "the runtime directory")?;
        mounts.push(Mount::virtual_fs(
            MountKind::PrivateTmpfs,
            runtime_dir,
            "private runtime directory",
        ));
        let session = [
            (
                sockets.wayland.as_ref().map(|display| &display.socket),
                "Wayland display",
            ),
            (sockets.pipewire.as_ref(), "audio (PipeWire)"),
            (sockets.pulse.as_ref(), "audio (PulseAudio protocol)"),
        ];
        for (socket, purpose) in session {
            if let Some(socket) = socket {
                // Only what lies directly in the runtime directory is taken
                // from it, whatever the session said.
                if !socket.starts_with(runtime_dir) {
                    return Err(SandboxError::Path { role: purpose });
                }
                let socket = text(socket, purpose)?;
                mounts.push(Mount::host(MountKind::ReadOnly, socket, false, purpose));
            }
        }
    }
    if let Some(authority) = &sockets.xauthority {
        mounts.push(Mount {
            kind: MountKind::ReadOnly,
            source: Some(text(authority, "the X authority file")?.to_owned()),
            target: XAUTHORITY_PATH.to_owned(),
            optional: false,
            purpose: "X11 authority",
        });
    }
    if request.network {
        mounts.push(Mount::host(
            MountKind::ReadOnly,
            RESOLVER_DIRECTORY,
            true,
            "name resolution",
        ));
    }

    // The application's own storage.
    let prefix = text(&request.app.prefix, "the prefix")?;
    let home = text(&request.app.home, "the private home directory")?;
    mounts.push(Mount::host(MountKind::ReadWrite, prefix, false, "prefix"));
    mounts.push(Mount::host(
        MountKind::ReadWrite,
        home,
        false,
        "private home",
    ));
    if request.supervisor {
        mounts.push(Mount {
            kind: MountKind::ReadOnly,
            source: Some(text(&host.supervisor, "tur-runtime")?.to_owned()),
            target: SUPERVISOR_PATH.to_owned(),
            optional: false,
            purpose: "supervisor",
        });
    }
    if let Some(file) = request.program_file {
        let file = text(file, "the program file")?;
        mounts.push(Mount::host(
            MountKind::ReadOnly,
            file,
            false,
            "program file",
        ));
    }

    // What the user granted, checked again against the file system as it is.
    for grant in request.grants {
        let resolved = request
            .context
            .recheck(grant)
            .map_err(|refusal| SandboxError::Grant {
                path: grant.path.clone(),
                refusal,
            })?;
        let path = text(&resolved, "a granted directory")?;
        let kind = match grant.access {
            GrantAccess::ReadOnly => MountKind::ReadOnly,
            GrantAccess::ReadWrite => MountKind::ReadWrite,
        };
        mounts.push(Mount::host(kind, path, false, "granted by the user"));
    }

    let mut arguments: Vec<String> = [
        "--unshare-user",
        "--unshare-ipc",
        "--unshare-pid",
        "--unshare-uts",
        "--unshare-cgroup-try",
    ]
    .iter()
    .map(|option| (*option).to_owned())
    .collect();
    if !request.network {
        arguments.push("--unshare-net".to_owned());
    }
    if !request.nested_namespaces {
        arguments.push("--disable-userns".to_owned());
    }
    arguments.push("--die-with-parent".to_owned());
    arguments.push("--new-session".to_owned());
    for mount in &mounts {
        mount.push_arguments(&mut arguments);
    }
    // The root of the sandbox is an in-memory file system; nothing is to be
    // written to it.
    arguments.extend(["--remount-ro".to_owned(), "/".to_owned()]);
    let working_directory = text(request.working_directory, "the working directory")?;
    arguments.extend(["--chdir".to_owned(), working_directory.to_owned()]);
    arguments.push("--".to_owned());

    Ok(Sandbox {
        program,
        arguments,
        mounts,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    use super::*;
    use crate::grant::tests::{Machine, exposes};
    use crate::launch_host::tests::fixture;

    /// One file-system option of a bubblewrap command line.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct Parsed {
        pub(crate) option: String,
        pub(crate) source: Option<String>,
        pub(crate) target: String,
    }

    /// Reads a bubblewrap argument list back, independently of the code that
    /// wrote it. Returns the options without operands and the file-system
    /// options, and panics on anything it does not know: an option this test
    /// has not considered must not slip into the command line.
    pub(crate) fn parse(arguments: &[String]) -> (Vec<String>, Vec<Parsed>) {
        let mut flags = Vec::new();
        let mut entries = Vec::new();
        let mut at = 0;
        let operand = |index: usize| arguments.get(index).cloned().unwrap();
        while at < arguments.len() {
            let option = arguments[at].as_str();
            match option {
                "--" => {
                    assert_eq!(at, arguments.len() - 1, "\"--\" must come last");
                    at += 1;
                }
                "--unshare-user"
                | "--unshare-ipc"
                | "--unshare-pid"
                | "--unshare-uts"
                | "--unshare-cgroup-try"
                | "--unshare-net"
                | "--disable-userns"
                | "--die-with-parent"
                | "--new-session" => {
                    flags.push(option.to_owned());
                    at += 1;
                }
                "--perms" => {
                    assert_eq!(operand(at + 1), "0700");
                    assert_eq!(arguments[at + 2], "--tmpfs");
                    at += 2;
                }
                "--tmpfs" | "--proc" | "--dev" | "--remount-ro" | "--chdir" => {
                    entries.push(Parsed {
                        option: option.to_owned(),
                        source: None,
                        target: operand(at + 1),
                    });
                    at += 2;
                }
                "--ro-bind" | "--ro-bind-try" | "--bind" | "--dev-bind" | "--dev-bind-try"
                | "--symlink" => {
                    entries.push(Parsed {
                        option: option.to_owned(),
                        source: Some(operand(at + 1)),
                        target: operand(at + 2),
                    });
                    at += 3;
                }
                other => panic!("unexpected bubblewrap option {other:?}"),
            }
        }
        (flags, entries)
    }

    /// The host paths a parsed command line makes visible, with whether each
    /// is writable.
    pub(crate) fn host_paths(entries: &[Parsed]) -> Vec<(PathBuf, bool)> {
        entries
            .iter()
            .filter(|entry| entry.option.contains("bind"))
            .map(|entry| {
                (
                    PathBuf::from(entry.source.clone().unwrap()),
                    entry.option == "--bind" || entry.option.starts_with("--dev-bind"),
                )
            })
            .collect()
    }

    /// Asserts everything that must hold for any sandbox on `machine`,
    /// whatever was granted.
    pub(crate) fn assert_boundary(machine: &Machine, app: &AppPaths, arguments: &[String]) {
        let (flags, entries) = parse(arguments);
        for required in [
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-uts",
            "--die-with-parent",
            "--new-session",
        ] {
            assert!(
                flags.iter().any(|flag| flag == required),
                "{required} is missing"
            );
        }

        let sockets = ["wayland-0", "pipewire-0", "pulse/native", "xauth_AbCdEf"]
            .map(|name| machine.runtime_dir.join(name));
        let paths = host_paths(&entries);
        for (source, writable) in &paths {
            // No secret, and no parent of one.
            for secret in machine.secrets() {
                assert!(
                    !exposes(source, &secret),
                    "mounting {source:?} exposes {secret:?}"
                );
            }
            // Not the home directory, nor anything that contains it.
            assert!(
                !machine.home.starts_with(source),
                "mounting {source:?} exposes the home directory"
            );
            // From the runtime directory, only the session's own sockets.
            if source.starts_with(&machine.runtime_dir) {
                assert!(
                    sockets.contains(source),
                    "{source:?} is not a session socket"
                );
                assert!(!writable);
            }
            // From the runtime manager's data, only this application's
            // prefix and private home.
            if source.starts_with(machine.dirs.data_root())
                || source.starts_with(machine.dirs.state_root())
            {
                assert!(
                    *source == app.prefix || *source == app.home,
                    "{source:?} is another application's data"
                );
            }
            // Nothing of the system is writable.
            if *writable {
                assert!(
                    ![
                        "/usr", "/etc", "/bin", "/lib", "/lib64", "/sys", "/proc", "/boot"
                    ]
                    .iter()
                    .any(|system| source.starts_with(system)),
                    "{source:?} is writable"
                );
            }
        }
        // The metadata file is not visible: only its two siblings are.
        assert!(
            !paths
                .iter()
                .any(|(source, _)| app.metadata.starts_with(source))
        );

        // /tmp, /proc and /dev are the sandbox's own.
        for (option, target) in [("--tmpfs", "/tmp"), ("--proc", "/proc"), ("--dev", "/dev")] {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.option == option && entry.target == target),
                "{option} {target} is missing"
            );
        }
        assert!(
            !paths.iter().any(|(source, _)| source == Path::new("/tmp")
                || source == Path::new("/proc")
                || source == Path::new("/dev")
                || source == Path::new("/run")
                || source == Path::new("/var")
                || source == Path::new("/home")
                || source == Path::new("/")),
            "a whole system tree is mounted"
        );
    }

    fn request<'a>(
        machine_host: &'a LaunchHost,
        app: &'a AppPaths,
        context: &'a GrantContext,
        grants: &'a [Grant],
    ) -> SandboxRequest<'a> {
        SandboxRequest {
            host: machine_host,
            app,
            context,
            grants,
            network: true,
            nested_namespaces: false,
            program_file: None,
            supervisor: true,
            working_directory: &app.prefix,
        }
    }

    fn grant(path: &str, access: GrantAccess) -> Grant {
        Grant {
            path: path.to_owned(),
            access,
        }
    }

    #[test]
    fn the_default_sandbox_shows_nothing_of_the_home_directory() {
        let machine = Machine::new("sandbox-default");
        let host = fixture(&machine.home, &machine.runtime_dir);
        let app = machine.dirs.app("com.example.app").unwrap();
        let context = machine.context();
        let sandbox = build(&request(&host, &app, &context, &[])).unwrap();
        assert_eq!(sandbox.program, "/usr/bin/bwrap");
        assert_boundary(&machine, &app, &sandbox.arguments);

        let (flags, entries) = parse(&sandbox.arguments);
        assert!(flags.contains(&"--disable-userns".to_owned()));
        assert!(!flags.contains(&"--unshare-net".to_owned()));

        // The only writable host paths are the prefix and the private home.
        let writable: Vec<PathBuf> = entries
            .iter()
            .filter(|entry| entry.option == "--bind")
            .map(|entry| PathBuf::from(entry.source.clone().unwrap()))
            .collect();
        assert_eq!(writable, [app.prefix.clone(), app.home.clone()]);

        // Every host path under the home directory is one of those two.
        for (source, _) in host_paths(&entries) {
            if source.starts_with(&machine.home) {
                assert!(writable.contains(&source), "{source:?}");
            }
        }

        let has = |option: &str, source: &str, target: &str| {
            entries.iter().any(|entry| {
                entry.option == option
                    && entry.source.as_deref() == Some(source)
                    && entry.target == target
            })
        };
        assert!(has("--ro-bind", "/usr", "/usr"));
        assert!(has("--ro-bind", "/etc", "/etc"));
        assert!(has("--symlink", "usr/bin", "/bin"));
        assert!(has("--symlink", "usr/lib64", "/lib64"));
        assert!(has("--ro-bind", "/lib32", "/lib32"));
        assert!(has("--dev-bind-try", "/dev/dri", "/dev/dri"));
        assert!(has("--ro-bind", "/tmp/.X11-unix/X0", "/tmp/.X11-unix/X0"));
        assert!(has("--ro-bind", "/usr/bin/tur-runtime", SUPERVISOR_PATH));
        let authority = machine.runtime_dir.join("xauth_AbCdEf");
        assert!(has(
            "--ro-bind",
            authority.to_str().unwrap(),
            XAUTHORITY_PATH
        ));
        assert!(has("--ro-bind-try", RESOLVER_DIRECTORY, RESOLVER_DIRECTORY));
        // No ntsync device on this host, so none is passed through.
        assert!(!entries.iter().any(|entry| entry.target == "/dev/ntsync"));

        // The X11 socket is mounted after /tmp is replaced, the session
        // sockets after the runtime directory is, and the root is sealed
        // last.
        let position = |option: &str, target: &str| {
            entries
                .iter()
                .position(|entry| entry.option == option && entry.target == target)
                .unwrap()
        };
        assert!(position("--tmpfs", "/tmp") < position("--ro-bind", "/tmp/.X11-unix/X0"));
        let runtime = machine.runtime_dir.to_str().unwrap();
        let wayland = machine.runtime_dir.join("wayland-0");
        assert!(position("--tmpfs", runtime) < position("--ro-bind", wayland.to_str().unwrap()));
        assert_eq!(entries[entries.len() - 2].option, "--remount-ro");
        assert_eq!(entries[entries.len() - 2].target, "/");
        assert_eq!(entries[entries.len() - 1].option, "--chdir");
        assert_eq!(sandbox.arguments.last().map(String::as_str), Some("--"));

        // The structured list and the argument list agree.
        let binds = sandbox
            .mounts
            .iter()
            .filter(|mount| mount.source.is_some())
            .count();
        let parsed_binds = entries
            .iter()
            .filter(|entry| entry.source.is_some())
            .count();
        assert_eq!(binds, parsed_binds);
    }

    #[test]
    fn options_follow_the_request() {
        let machine = Machine::new("sandbox-options");
        let mut host = fixture(&machine.home, &machine.runtime_dir);
        host.ntsync = NtsyncDevice::Present;
        let app = machine.dirs.app("com.example.app").unwrap();
        let context = machine.context();
        let program = machine.home.join("Games/setup.exe");
        let sandbox = build(&SandboxRequest {
            network: false,
            nested_namespaces: true,
            supervisor: false,
            program_file: Some(program.as_path()),
            ..request(&host, &app, &context, &[])
        })
        .unwrap();
        assert_boundary(&machine, &app, &sandbox.arguments);
        let (flags, entries) = parse(&sandbox.arguments);
        assert!(flags.contains(&"--unshare-net".to_owned()));
        assert!(!flags.contains(&"--disable-userns".to_owned()));
        assert!(!entries.iter().any(|entry| entry.target == SUPERVISOR_PATH));
        assert!(
            !entries
                .iter()
                .any(|entry| entry.target == RESOLVER_DIRECTORY)
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.option == "--dev-bind" && entry.target == "/dev/ntsync")
        );
        // The program file is one read-only file, not its directory.
        let files: Vec<&Parsed> = entries
            .iter()
            .filter(|entry| entry.source.as_deref() == program.to_str())
            .collect();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].option, "--ro-bind");
        assert!(
            !entries
                .iter()
                .any(|entry| entry.source.as_deref() == machine.home.join("Games").to_str())
        );
    }

    #[test]
    fn a_session_without_displays_or_a_runtime_directory() {
        let machine = Machine::new("sandbox-headless");
        let mut host = fixture(&machine.home, &machine.runtime_dir);
        host.runtime_dir = None;
        host.sockets = crate::launch_host::Sockets::default();
        host.system.clear();
        let app = machine.dirs.app("com.example.app").unwrap();
        let context = machine.context();
        let sandbox = build(&request(&host, &app, &context, &[])).unwrap();
        assert_boundary(&machine, &app, &sandbox.arguments);
        let (_, entries) = parse(&sandbox.arguments);
        assert!(
            !entries
                .iter()
                .any(|entry| entry.target.starts_with("/run/user")
                    || entry.target.starts_with("/tmp/.X11-unix"))
        );
    }

    #[test]
    fn the_boundary_holds_for_every_grant_that_is_accepted() {
        let machine = Machine::new("sandbox-grants");
        let host = fixture(&machine.home, &machine.runtime_dir);
        let app = machine.dirs.app("com.example.app").unwrap();
        let context = machine.context();
        let candidates = machine.candidates();

        // Each request on its own, as the user would make it: admitted first,
        // then recorded, then mounted.
        let mut accepted = Vec::new();
        for (requested, acceptable) in &candidates {
            for access in GrantAccess::ALL {
                let Ok(recorded) = context.admit(requested) else {
                    assert!(!*acceptable, "{requested:?} should have been admitted");
                    continue;
                };
                assert!(*acceptable, "{requested:?} should have been refused");
                let grants = [grant(&recorded, access)];
                let sandbox = build(&request(&host, &app, &context, &grants)).unwrap();
                assert_boundary(&machine, &app, &sandbox.arguments);
                let (_, entries) = parse(&sandbox.arguments);
                let expected = match access {
                    GrantAccess::ReadOnly => "--ro-bind",
                    GrantAccess::ReadWrite => "--bind",
                };
                assert!(
                    entries.iter().any(|entry| entry.option == expected
                        && entry.source.as_deref() == Some(recorded.as_str())
                        && entry.target == recorded),
                    "{recorded:?} is not mounted {expected}"
                );
                if !accepted.contains(&recorded) {
                    accepted.push(recorded);
                }
            }
        }
        assert!(accepted.len() >= 5, "{accepted:?}");

        // Everything acceptable at once.
        let all: Vec<Grant> = accepted
            .iter()
            .map(|path| grant(path, GrantAccess::ReadWrite))
            .collect();
        let sandbox = build(&request(&host, &app, &context, &all)).unwrap();
        assert_boundary(&machine, &app, &sandbox.arguments);
    }

    #[test]
    fn a_hostile_grant_written_into_the_metadata_is_never_mounted() {
        let machine = Machine::new("sandbox-hostile");
        let host = fixture(&machine.home, &machine.runtime_dir);
        let app = machine.dirs.app("com.example.app").unwrap();
        let context = machine.context();

        // Every candidate, recorded exactly as written, bypassing admission:
        // this is what hand-edited or corrupted metadata looks like.
        let mut built = 0;
        for (path, acceptable) in machine.candidates() {
            for access in GrantAccess::ALL {
                let grants = [grant(&path, access)];
                match build(&request(&host, &app, &context, &grants)) {
                    Ok(sandbox) => {
                        assert!(acceptable, "{path:?} was mounted");
                        assert_boundary(&machine, &app, &sandbox.arguments);
                        built += 1;
                    }
                    Err(error) => {
                        assert!(matches!(error, SandboxError::Grant { .. }), "{error:?}");
                        assert_eq!(error.code(), Code::GrantChanged);
                        assert!(error.to_string().contains("can no longer be shown"));
                    }
                }
            }
        }
        assert!(built >= 5, "only {built} sandboxes were built");

        // One hostile entry among good ones refuses the whole launch.
        let mixed = [
            grant(&machine.path("Saves"), GrantAccess::ReadWrite),
            grant(&machine.path(".ssh"), GrantAccess::ReadOnly),
        ];
        assert!(build(&request(&host, &app, &context, &mixed)).is_err());
    }

    #[test]
    fn a_grant_that_became_a_link_is_refused_at_the_next_launch() {
        let machine = Machine::new("sandbox-swapped");
        let host = fixture(&machine.home, &machine.runtime_dir);
        let app = machine.dirs.app("com.example.app").unwrap();
        let context = machine.context();
        let recorded = context.admit(&machine.path("Games/installed")).unwrap();
        let grants = [
            grant(&machine.path("Games"), GrantAccess::ReadWrite),
            grant(&recorded, GrantAccess::ReadOnly),
        ];
        assert!(build(&request(&host, &app, &context, &grants)).is_ok());

        // The application, which can write to Games, swaps the nested
        // directory for a link to the user's keys.
        fs::remove_dir(machine.home.join("Games/installed")).unwrap();
        symlink(
            machine.home.join(".ssh"),
            machine.home.join("Games/installed"),
        )
        .unwrap();
        let error = build(&request(&host, &app, &context, &grants)).unwrap_err();
        assert!(
            matches!(
                &error,
                SandboxError::Grant {
                    refusal: GrantRefusal::Moved { .. },
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn hostile_session_values_cannot_name_files_to_mount() {
        let machine = Machine::new("sandbox-session");
        let app = machine.dirs.app("com.example.app").unwrap();
        let context = machine.context();

        // A Wayland "socket" outside the runtime directory.
        let mut host = fixture(&machine.home, &machine.runtime_dir);
        host.sockets.wayland = Some(crate::launch_host::WaylandDisplay {
            name: "wayland-0".to_owned(),
            socket: machine.home.join(".ssh/id_ed25519"),
        });
        assert!(matches!(
            build(&request(&host, &app, &context, &[])),
            Err(SandboxError::Path { .. })
        ));

        // A path with "..".
        let mut host = fixture(&machine.home, &machine.runtime_dir);
        host.sockets.pipewire = Some(machine.runtime_dir.join("../../home/ada/.ssh"));
        assert!(matches!(
            build(&request(&host, &app, &context, &[])),
            Err(SandboxError::Path { .. })
        ));

        // No bubblewrap, no sandbox, no launch.
        let mut host = fixture(&machine.home, &machine.runtime_dir);
        host.bwrap = None;
        let error = build(&request(&host, &app, &context, &[])).unwrap_err();
        assert_eq!(error, SandboxError::Missing);
        assert_eq!(error.code(), Code::SandboxMissing);
    }

    #[test]
    fn mounts_serialise_for_plan_output() {
        let machine = Machine::new("sandbox-json");
        let host = fixture(&machine.home, &machine.runtime_dir);
        let app = machine.dirs.app("com.example.app").unwrap();
        let context = machine.context();
        let sandbox = build(&request(&host, &app, &context, &[])).unwrap();
        let json = serde_json::to_value(&sandbox.mounts).unwrap();
        assert_eq!(json[0]["kind"], "read-only");
        assert_eq!(json[0]["source"], "/usr");
        assert_eq!(json[0]["target"], "/usr");
        assert_eq!(json[0]["optional"], false);
        assert_eq!(json[0]["purpose"], "system");
        let proc_entry = json
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["kind"] == "proc")
            .unwrap();
        assert!(proc_entry.get("source").is_none());
    }
}
