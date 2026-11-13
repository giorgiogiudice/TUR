// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The read-only sandbox: the only place P@x runs a tool program or a command
//! (AI control layer design, sections 5.4, 17 item 3 and 19 item 1).
//!
//! A sandbox is a bubblewrap command line, built here as a [`SandboxPlan`]:
//! a pure value, made without touching the machine, that can be inspected and
//! tested. Running it is the work of [`crate::exec`].
//!
//! # What a command sees
//!
//! * The system, read-only: `/usr`, `/etc` and the top-level links into
//!   `/usr`; the hardware description under `/sys`; the system logs; the
//!   device database.
//! * A `/proc` of its own processes, a minimal `/dev`, an empty `/tmp`.
//! * The folders the user granted for the task, read-only.
//! * A fixed environment. Nothing of the caller's environment is passed on.
//!
//! # What it does not see
//!
//! The root of the sandbox is an empty in-memory file system, so everything
//! that is not listed above is absent, not merely unreadable: the home
//! directory, other accounts, `/run` (and with it the session bus, the
//! system bus, the Wayland socket, the keyring, the GnuPG and SSH agents and
//! the service manager's socket), `/var` apart from the logs, and `/tmp`
//! (and with it the X11 socket). The network namespace is new and empty,
//! which also removes the abstract sockets of the display server and the
//! session bus. There is no network option in this release.
//!
//! The design says "the file system is read-only". That alone would not be
//! enough: a socket in a read-only mount can still be connected to, and a
//! connection to the system bus, to the service manager or to a container
//! daemon changes things. So this module lists what is visible instead of
//! listing what is hidden.
//!
//! # Grants
//!
//! [`GrantRules`] decides which folders a task may be given. Keys, keyrings,
//! password stores, browser profiles, every hidden entry of the home
//! directory, the home directory as a whole, other accounts and the system's
//! own trees are refused. The rules are the ones `tur-runtime` applies to a
//! Windows program's grants (`services/tur-runtime/src/grant.rs`), made
//! stricter in three places: all of `/run`, `/var` and `/tmp` is refused,
//! nothing may be granted read-write, and everything under the parent of the
//! home directory other than the home directory itself is refused. This
//! module does not depend on that crate: its sandbox is built around a Wine
//! prefix, a display and a GPU, none of which a read-only command has.
//!
//! # The system's own links
//!
//! On an image-based system (Fedora Atomic, and so TUR) `/home` is a link to
//! `var/home`, and `/mnt`, `/opt`, `/srv`, `/root` and `/usr/local` are links
//! into `/var` too. A person writes `/home/ada/Documents`. A grant may not be
//! reached through a link, because a link inside a granted tree could point
//! anywhere; but these few links are the system's, owned by the
//! administrator in directories only the administrator can write, and are
//! the way every path of the person's begins. So [`GrantRules`] resolves
//! them, and only them ([`SYSTEM_LINKS`], [`SystemLink`]): a grant named
//! through one of them is accepted if the rest of the path has no link, and
//! is judged under the name the person would use, so `/var/home/ada/Documents`
//! and `/home/ada/Documents` get the same answer and `/var/roothome` is
//! `/root`. The sandbox shows the granted folder at its real place and adds
//! the same link, so that a command can name it either way.

use std::fmt;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

/// The sandbox program. Never looked up along a search path.
pub const BWRAP: &str = "/usr/bin/bwrap";

/// The program that applies the resource limits, inside the sandbox.
pub const PRLIMIT: &str = "/usr/bin/prlimit";

/// The longest path accepted, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;

/// The most arguments one command may have, the program included.
pub const MAX_ARGUMENTS: usize = 256;

/// The longest single argument, in bytes.
pub const MAX_ARGUMENT_BYTES: usize = 8192;

/// The most folders one task may be granted.
pub const MAX_GRANTS: usize = 32;

/// The system directories every sandbox shows read-only. A directory that
/// does not exist on the machine is skipped.
pub const SYSTEM_VIEW: [(&str, &str); 8] = [
    ("/usr", "system"),
    ("/etc", "system configuration"),
    ("/bin", "system"),
    ("/sbin", "system"),
    ("/lib", "system"),
    ("/lib64", "system"),
    ("/lib32", "system"),
    ("/libx32", "system"),
];

/// State the system publishes for reading: the hardware description, the
/// logs and the device database. None of it holds anything of the user's.
pub const STATE_VIEW: [(&str, &str); 8] = [
    ("/sys/block", "hardware description"),
    ("/sys/bus", "hardware description"),
    ("/sys/class", "hardware description"),
    ("/sys/dev", "hardware description"),
    ("/sys/devices", "hardware description"),
    ("/var/log", "system logs"),
    ("/run/log/journal", "system logs"),
    ("/run/udev", "device database"),
];

/// Trees no grant may be, lie in or contain. The sandbox provides its own
/// view of some; the others hold sockets, other accounts or other programs'
/// data.
pub const REFUSED_TREES: [&str; 16] = [
    "/proc", "/sys", "/dev", "/usr", "/etc", "/bin", "/sbin", "/lib", "/lib64", "/lib32",
    "/libx32", "/boot", "/root", "/run", "/var", "/tmp",
];

/// The directory that holds home directories. A grant inside it is accepted
/// only inside the user's own home directory.
pub const HOMES_TREE: &str = "/home";

/// The system's own links that a grant may be named through (see the module
/// documentation). Each is used only if, on the machine, it is a symbolic
/// link owned by the administrator in a directory only the administrator can
/// write, and its target passes [`SystemLink::new`]. No other link is ever
/// followed.
pub const SYSTEM_LINKS: [&str; 6] = ["/home", "/mnt", "/opt", "/srv", "/root", "/usr/local"];

/// The tree an image-based system keeps its writable state in, where the
/// system's links point.
const STATE_TREE: &str = "/var";

/// One of the system's links, resolved: `name` is the path a person writes,
/// `target` where it leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemLink {
    name: PathBuf,
    target: PathBuf,
}

impl SystemLink {
    /// A link from `name`, one of [`SYSTEM_LINKS`], to `target`, its resolved
    /// form. `None` if either is not a plain absolute path, if `name` is not
    /// on the list, or if the target could widen what is granted: the root,
    /// one of the names on the list or a path inside one, or a path that is
    /// or overlaps a system tree ([`REFUSED_TREES`]) and is not strictly
    /// inside `/var`, where an image-based system keeps these folders.
    pub fn new(name: &str, target: &Path) -> Option<SystemLink> {
        let target_text = target.to_str()?;
        if !SYSTEM_LINKS.contains(&name) || !is_plain_absolute(target_text) {
            return None;
        }
        if target.parent().is_none() || target == Path::new(name) {
            return None;
        }
        if SYSTEM_LINKS
            .iter()
            .any(|other| overlaps(target, Path::new(other)))
        {
            return None;
        }
        let in_state = target.starts_with(STATE_TREE) && target != Path::new(STATE_TREE);
        let touches_system = REFUSED_TREES
            .iter()
            .any(|tree| overlaps(target, Path::new(tree)));
        if touches_system && !in_state {
            return None;
        }
        // Not a tree the sandbox shows from `/var`, such as the logs.
        if STATE_VIEW
            .iter()
            .any(|(shown, _)| overlaps(target, Path::new(shown)))
        {
            return None;
        }
        Some(SystemLink {
            name: PathBuf::from(name),
            target: target.to_path_buf(),
        })
    }

    /// The path a person writes, such as `/home`.
    pub fn name(&self) -> &Path {
        &self.name
    }

    /// Where it leads, such as `/var/home`.
    pub fn target(&self) -> &Path {
        &self.target
    }
}

/// The system's links on this machine (see [`SYSTEM_LINKS`]). Only a link
/// owned by the administrator, in a directory owned by the administrator
/// that nobody else can write, is taken.
pub fn system_links() -> Vec<SystemLink> {
    SYSTEM_LINKS
        .iter()
        .filter_map(|name| {
            let path = Path::new(name);
            let link = fs::symlink_metadata(path).ok()?;
            if !link.file_type().is_symlink() || link.uid() != 0 {
                return None;
            }
            let parent = fs::metadata(path.parent()?).ok()?;
            if parent.uid() != 0 || parent.mode() & 0o022 != 0 {
                return None;
            }
            SystemLink::new(name, &fs::canonicalize(path).ok()?)
        })
        .collect()
}

/// `path` with a leading part rewritten from one form of a link to the
/// other: from a link's name to its target when `to_target`, else from its
/// target to its name. Unchanged if no link applies.
fn rewrite(links: &[SystemLink], path: &Path, to_target: bool) -> PathBuf {
    for link in links {
        let (from, to) = if to_target {
            (&link.name, &link.target)
        } else {
            (&link.target, &link.name)
        };
        if let Ok(rest) = path.strip_prefix(from) {
            return if rest.as_os_str().is_empty() {
                to.clone()
            } else {
                to.join(rest)
            };
        }
    }
    path.to_path_buf()
}

/// Locations under the home directory that are named in refusals. Every
/// other hidden entry of the home directory is refused too, without a name.
pub const PROTECTED_IN_HOME: [(&str, &str); 22] = [
    (".ssh", "the SSH directory"),
    (".gnupg", "the GnuPG directory"),
    (".pki", "the certificate store"),
    (".password-store", "the password store"),
    (".local/share/keyrings", "the desktop keyring"),
    (".local/share/kwalletd", "the KDE wallet"),
    (".mozilla", "a browser profile directory"),
    (".thunderbird", "a mail profile directory"),
    (".config/google-chrome", "a browser profile directory"),
    (".config/chromium", "a browser profile directory"),
    (".config/BraveSoftware", "a browser profile directory"),
    (".config/microsoft-edge", "a browser profile directory"),
    (".config/vivaldi", "a browser profile directory"),
    (".config/opera", "a browser profile directory"),
    (".var/app", "the data of Flatpak applications"),
    ("snap", "the data of snap applications"),
    (".aws", "cloud credentials"),
    (".kube", "cluster credentials"),
    (".docker", "registry credentials"),
    (".config/gh", "source-hosting credentials"),
    (".netrc", "stored network passwords"),
    (".git-credentials", "stored source-hosting passwords"),
];

/// The environment of every sandboxed process. It is fixed: no variable is
/// copied from the caller.
pub const ENVIRONMENT: [(&str, &str); 5] = [
    ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
    ("HOME", "/tmp"),
    ("LANG", "C.UTF-8"),
    ("LC_ALL", "C.UTF-8"),
    ("TERM", "dumb"),
];

/// Whether `text` is a plain absolute path: it starts at the root, is at most
/// [`MAX_PATH_BYTES`] long, has no empty, `.` or `..` component and no
/// control character.
///
/// ```
/// use tur_pax::sandbox::is_plain_absolute;
///
/// assert!(is_plain_absolute("/home/ada/Documents"));
/// assert!(!is_plain_absolute("/home/ada/../root"));
/// assert!(!is_plain_absolute("Documents"));
/// ```
pub fn is_plain_absolute(text: &str) -> bool {
    if text == "/" {
        return true;
    }
    text.starts_with('/')
        && text.len() <= MAX_PATH_BYTES
        && !text.chars().any(char::is_control)
        && text[1..]
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// Whether one path is the other or lies inside it, in either direction.
fn overlaps(one: &Path, other: &Path) -> bool {
    one.starts_with(other) || other.starts_with(one)
}

/// Whether a plain absolute path lies in the part of the system every
/// sandbox shows, so that it needs no grant.
pub fn is_system_visible(path: &str) -> bool {
    is_plain_absolute(path)
        && SYSTEM_VIEW
            .iter()
            .any(|(tree, _)| Path::new(path).starts_with(tree))
}

/// Why a path may not be shown to a sandboxed command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantRefusal {
    /// The path is relative, has an empty, `.` or `..` component, holds a
    /// control character or is too long.
    NotPlain,
    /// The path does not exist or cannot be resolved.
    Missing,
    /// The path is neither a directory nor a regular file.
    WrongKind,
    /// The path is reached through a symbolic link.
    Link {
        /// Where it resolves.
        resolved: String,
    },
    /// The path is the root directory.
    Root,
    /// The path is the home directory or contains it.
    ContainsHome,
    /// The path is, or lies inside, a hidden entry of the home directory.
    HiddenInHome,
    /// The path is a protected location, lies inside one or contains one.
    Protected {
        /// What is protected.
        what: &'static str,
    },
    /// The path is a system tree, lies inside one or contains one.
    System,
    /// The path lies among the home directories, outside the user's own.
    OtherAccount,
    /// More folders were granted than a task may have.
    TooMany,
}

impl fmt::Display for GrantRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrantRefusal::NotPlain => f.write_str(
                "it must be an absolute path without empty, \".\" or \"..\" components or \
                 control characters",
            ),
            GrantRefusal::Missing => f.write_str("it does not exist or cannot be resolved"),
            GrantRefusal::WrongKind => f.write_str("it is neither a folder nor a regular file"),
            GrantRefusal::Link { resolved } => write!(
                f,
                "it is reached through a link and resolves to {resolved:?}; name the folder \
                 itself"
            ),
            GrantRefusal::Root => f.write_str("it is the root of the file system"),
            GrantRefusal::ContainsHome => {
                f.write_str("it is the home directory or contains the home directory")
            }
            GrantRefusal::HiddenInHome => f.write_str(
                "it is in a hidden part of the home directory, where credentials and \
                 application profiles are kept",
            ),
            GrantRefusal::Protected { what } => write!(f, "it would expose {what}"),
            GrantRefusal::System => f.write_str(
                "it belongs to the system, which the sandbox already shows read-only or hides \
                 on purpose",
            ),
            GrantRefusal::OtherAccount => {
                f.write_str("it lies among the home directories, outside your own")
            }
            GrantRefusal::TooMany => f.write_str("a task may be granted at most 32 folders"),
        }
    }
}

/// A path the user granted for a task, already accepted by [`GrantRules`].
///
/// The only way to build one outside this crate is [`GrantRules::admit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    path: String,
}

impl Grant {
    /// The granted path. It is plain, absolute and free of links.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// A grant that was not checked. For the tests that prove a plan refuses
    /// one.
    #[cfg(test)]
    pub(crate) fn unchecked(path: &str) -> Grant {
        Grant {
            path: path.to_owned(),
        }
    }
}

/// The rules that decide which paths a task may be granted, for one user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRules {
    /// The home directory, as named, with links resolved, and under the name
    /// of a system link. Empty when no usable home directory is known: then
    /// nothing among the home directories can be granted.
    homes: Vec<PathBuf>,
    /// The system's links on this machine ([`system_links`]).
    links: Vec<SystemLink>,
}

impl GrantRules {
    /// The rules for a user whose home directory is `home`, with the
    /// system's links of this machine ([`system_links`]).
    ///
    /// A home directory that is not a plain absolute path, or that is or
    /// contains a system tree or the directory of home directories (`/`,
    /// `/home`, `/var`, `/root`), is not used: it would turn the rules for
    /// the inside of a home directory into rules for the whole machine.
    pub fn new(home: Option<&Path>) -> Self {
        let links = system_links();
        let resolved = home.and_then(|home| fs::canonicalize(home).ok());
        Self::build(home, resolved.as_deref(), links)
    }

    /// The rules for a home directory, without looking at the file system
    /// and with no system link.
    pub fn for_home(home: &str) -> Self {
        Self::build(Some(Path::new(home)), None, Vec::new())
    }

    /// The rules for a home directory and the system's links given, without
    /// looking at the file system. Each link is checked with
    /// [`SystemLink::new`]; one that fails is left out.
    pub fn with_links(home: &str, links: &[(&str, &str)]) -> Self {
        let links = links
            .iter()
            .filter_map(|(name, target)| SystemLink::new(name, Path::new(target)))
            .collect();
        Self::build(Some(Path::new(home)), None, links)
    }

    fn build(home: Option<&Path>, resolved: Option<&Path>, links: Vec<SystemLink>) -> Self {
        let mut homes: Vec<PathBuf> = Vec::new();
        if let Some(home) = home {
            let mut forms = vec![home.to_path_buf()];
            forms.extend(resolved.map(Path::to_path_buf));
            // The same home directory under the other name of a link.
            for form in forms.clone() {
                forms.push(rewrite(&links, &form, true));
                forms.push(rewrite(&links, &form, false));
            }
            for form in forms {
                if is_usable_home(&form) && !homes.contains(&form) {
                    homes.push(form);
                }
            }
        }
        GrantRules { homes, links }
    }

    /// The home directories the rules protect.
    pub fn homes(&self) -> &[PathBuf] {
        &self.homes
    }

    /// The system's links the rules resolve.
    pub fn links(&self) -> &[SystemLink] {
        &self.links
    }

    /// Apply every rule to a path, without looking at the file system.
    ///
    /// The path is taken as it is written, so it must already be free of
    /// links; [`GrantRules::admit`] makes sure of that. A path that lies in
    /// the target of a system link is judged under the link's name, the way
    /// a person writes it: `/var/home/ada/Documents` as
    /// `/home/ada/Documents`, `/var/roothome` as `/root`.
    ///
    /// # Errors
    ///
    /// The [`GrantRefusal`] that applies.
    pub fn judge(&self, path: &str) -> Result<(), GrantRefusal> {
        if !is_plain_absolute(path) {
            return Err(GrantRefusal::NotPlain);
        }
        let named = rewrite(&self.links, Path::new(path), false);
        let path = named.as_path();
        if path.parent().is_none() {
            return Err(GrantRefusal::Root);
        }
        if self.homes.iter().any(|home| home.starts_with(path)) {
            return Err(GrantRefusal::ContainsHome);
        }
        // Inside the home directory: nothing hidden, nothing protected.
        for home in &self.homes {
            let Ok(rest) = path.strip_prefix(home) else {
                continue;
            };
            let hidden = matches!(
                rest.components().next(),
                Some(Component::Normal(name)) if name.as_encoded_bytes().starts_with(b".")
            );
            for (relative, what) in PROTECTED_IN_HOME {
                if overlaps(path, &home.join(relative)) {
                    return Err(GrantRefusal::Protected { what });
                }
            }
            if hidden {
                return Err(GrantRefusal::HiddenInHome);
            }
            return Ok(());
        }
        // Outside it: nothing of the system, nothing of another account.
        if REFUSED_TREES
            .iter()
            .any(|tree| overlaps(path, Path::new(tree)))
        {
            return Err(GrantRefusal::System);
        }
        if overlaps(path, Path::new(HOMES_TREE)) {
            return Err(GrantRefusal::OtherAccount);
        }
        for home in &self.homes {
            if let Some(parent) = home.parent()
                && parent.parent().is_some()
                && path.starts_with(parent)
            {
                return Err(GrantRefusal::OtherAccount);
            }
        }
        Ok(())
    }

    /// Check a path the user wants to grant, against the rules and against
    /// the file system as it is. The path must exist, must be a directory or
    /// a regular file, and must not be reached through a symbolic link other
    /// than one of the system's links at its start ([`SYSTEM_LINKS`]). The
    /// grant holds the path with that link resolved, which is where the
    /// folder really is.
    ///
    /// # Errors
    ///
    /// The [`GrantRefusal`] that applies.
    pub fn admit(&self, requested: &str) -> Result<Grant, GrantRefusal> {
        if !is_plain_absolute(requested) {
            return Err(GrantRefusal::NotPlain);
        }
        let resolved = fs::canonicalize(requested).map_err(|_| GrantRefusal::Missing)?;
        // Only the system's own link at the start may lead elsewhere.
        if resolved != rewrite(&self.links, Path::new(requested), true) {
            return Err(GrantRefusal::Link {
                resolved: resolved.display().to_string(),
            });
        }
        let metadata = fs::metadata(&resolved).map_err(|_| GrantRefusal::Missing)?;
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(GrantRefusal::WrongKind);
        }
        let path = resolved.to_str().ok_or(GrantRefusal::NotPlain)?;
        self.judge(path)?;
        Ok(Grant {
            path: path.to_owned(),
        })
    }
}

fn is_usable_home(home: &Path) -> bool {
    home.to_str().is_some_and(is_plain_absolute)
        && home.parent().is_some()
        && !REFUSED_TREES
            .iter()
            .any(|tree| Path::new(tree).starts_with(home))
        && !Path::new(HOMES_TREE).starts_with(home)
}

/// How a path appears inside the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MountKind {
    /// A host path, read-only, skipped if it does not exist.
    System,
    /// A host path the user granted, read-only. It must exist.
    Granted,
    /// An empty in-memory file system that vanishes with the sandbox.
    Tmpfs,
    /// A `/proc` that shows the sandbox's own processes.
    Proc,
    /// A minimal `/dev`.
    Dev,
}

/// One entry of the sandbox's file-system view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// How the path appears.
    pub kind: MountKind,
    /// The path, the same inside the sandbox as outside.
    pub path: String,
    /// Why the entry is there.
    pub purpose: &'static str,
}

impl Mount {
    /// Whether the entry shows a path of the host.
    pub fn is_host_path(&self) -> bool {
        matches!(self.kind, MountKind::System | MountKind::Granted)
    }

    fn push_arguments(&self, arguments: &mut Vec<String>) {
        let option = match self.kind {
            MountKind::System => "--ro-bind-try",
            MountKind::Granted => "--ro-bind",
            MountKind::Tmpfs => "--tmpfs",
            MountKind::Proc => "--proc",
            MountKind::Dev => "--dev",
        };
        arguments.push(option.to_owned());
        if self.is_host_path() {
            arguments.push(self.path.clone());
        }
        arguments.push(self.path.clone());
    }
}

/// The resource and time limits of one sandboxed process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Wall-clock seconds before the process is killed.
    pub wall_seconds: u64,
    /// Processor seconds.
    pub cpu_seconds: u64,
    /// Address space, in bytes.
    pub address_space_bytes: u64,
    /// Processes and threads.
    pub processes: u64,
    /// Open files.
    pub open_files: u64,
    /// The largest file the process may write, in bytes. It can only write
    /// to the in-memory `/tmp`.
    pub file_size_bytes: u64,
    /// Bytes kept of each of standard output and standard error. The rest is
    /// read and dropped.
    pub output_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            wall_seconds: 20,
            cpu_seconds: 10,
            address_space_bytes: 4 * 1024 * 1024 * 1024,
            processes: 128,
            open_files: 256,
            file_size_bytes: 16 * 1024 * 1024,
            output_bytes: 64 * 1024,
        }
    }
}

/// Why a sandbox could not be planned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxError {
    /// A grant is refused.
    Grant {
        /// The granted path.
        path: String,
        /// Why it is refused.
        refusal: GrantRefusal,
    },
    /// The command cannot be run.
    Command(&'static str),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxError::Grant { path, refusal } => {
                write!(f, "{path:?} cannot be shown to a command: {refusal}")
            }
            SandboxError::Command(reason) => write!(f, "the command cannot be run: {reason}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// A read-only sandbox for one command: the complete bubblewrap command line
/// and what it was generated from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPlan {
    /// The sandbox program, [`BWRAP`].
    pub program: String,
    /// Its options, ending with `--`.
    pub arguments: Vec<String>,
    /// What runs inside: the limits wrapper, then the command.
    pub command: Vec<String>,
    /// The command as it was asked for.
    pub argv: Vec<String>,
    /// The file-system view the options were generated from, in order.
    pub mounts: Vec<Mount>,
    /// The environment of the command.
    pub environment: Vec<(String, String)>,
    /// The limits.
    pub limits: Limits,
    /// What the command reads on standard input. Nothing, if `None`.
    pub stdin: Option<Vec<u8>>,
    /// The system's links the sandbox has, as on the host, so that a granted
    /// folder can be named either way: (name, target), such as
    /// (`/home`, `/var/home`). Only a link that leads to a granted folder is
    /// made, and it shows nothing by itself.
    pub links: Vec<(String, String)>,
}

impl SandboxPlan {
    /// Plan the read-only sandbox for a command.
    ///
    /// `argv` is the program and its arguments. It is never parsed, never
    /// joined into one string and never given to a shell: each element
    /// becomes one argument of the process.
    ///
    /// # Errors
    ///
    /// [`SandboxError::Command`] if the command has no program, too many or
    /// too long arguments, or an argument with a NUL character;
    /// [`SandboxError::Grant`] if a grant does not pass the rules.
    pub fn read_only(
        rules: &GrantRules,
        grants: &[Grant],
        argv: &[String],
        limits: Limits,
    ) -> Result<SandboxPlan, SandboxError> {
        check_command(argv)?;
        if grants.len() > MAX_GRANTS {
            return Err(SandboxError::Grant {
                path: String::new(),
                refusal: GrantRefusal::TooMany,
            });
        }

        let mut mounts = Vec::new();
        for (path, purpose) in SYSTEM_VIEW {
            mounts.push(Mount {
                kind: MountKind::System,
                path: path.to_owned(),
                purpose,
            });
        }
        mounts.push(Mount {
            kind: MountKind::Proc,
            path: "/proc".to_owned(),
            purpose: "own processes",
        });
        mounts.push(Mount {
            kind: MountKind::Dev,
            path: "/dev".to_owned(),
            purpose: "minimal devices",
        });
        for (path, purpose) in STATE_VIEW {
            mounts.push(Mount {
                kind: MountKind::System,
                path: path.to_owned(),
                purpose,
            });
        }
        // The rules are applied again here, whatever admitted the grant.
        for grant in grants {
            rules
                .judge(&grant.path)
                .map_err(|refusal| SandboxError::Grant {
                    path: grant.path.clone(),
                    refusal,
                })?;
            if !mounts.iter().any(|mount| mount.path == grant.path) {
                mounts.push(Mount {
                    kind: MountKind::Granted,
                    path: grant.path.clone(),
                    purpose: "granted for the task",
                });
            }
        }
        // The system's links that lead to a granted folder, at the top level
        // only: a link under a directory the sandbox shows from the host is
        // there already.
        let mut links = Vec::new();
        for link in rules.links() {
            let (Some(name), Some(target)) = (link.name().to_str(), link.target().to_str()) else {
                continue;
            };
            let top_level = link.name().parent() == Some(Path::new("/"));
            let leads_to_a_grant = mounts.iter().any(|mount| {
                mount.kind == MountKind::Granted && Path::new(&mount.path).starts_with(target)
            });
            let taken = mounts
                .iter()
                .any(|mount| overlaps(Path::new(&mount.path), link.name()));
            if top_level && leads_to_a_grant && !taken {
                links.push((name.to_owned(), target.to_owned()));
            }
        }
        // Last, so that nothing above can put anything into it.
        mounts.push(Mount {
            kind: MountKind::Tmpfs,
            path: "/tmp".to_owned(),
            purpose: "private temporary files",
        });

        let environment: Vec<(String, String)> = ENVIRONMENT
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();

        let mut arguments: Vec<String> = [
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-uts",
            "--unshare-cgroup-try",
            "--unshare-net",
            "--disable-userns",
            "--die-with-parent",
            "--new-session",
            "--hostname",
            "tur-pax-sandbox",
            "--clearenv",
        ]
        .iter()
        .map(|option| (*option).to_owned())
        .collect();
        for (name, value) in &environment {
            arguments.extend(["--setenv".to_owned(), name.clone(), value.clone()]);
        }
        for mount in &mounts {
            mount.push_arguments(&mut arguments);
        }
        for (name, target) in &links {
            arguments.extend(["--symlink".to_owned(), target.clone(), name.clone()]);
        }
        // The root of the sandbox is an in-memory file system. Nothing is to
        // be written to it.
        arguments.extend(["--remount-ro".to_owned(), "/".to_owned()]);
        arguments.extend(["--chdir".to_owned(), "/".to_owned()]);
        arguments.push("--".to_owned());

        let mut command = vec![
            PRLIMIT.to_owned(),
            format!("--cpu={}", limits.cpu_seconds),
            format!("--as={}", limits.address_space_bytes),
            format!("--nproc={}", limits.processes),
            format!("--nofile={}", limits.open_files),
            format!("--fsize={}", limits.file_size_bytes),
            "--core=0".to_owned(),
            "--".to_owned(),
        ];
        command.extend(argv.iter().cloned());

        Ok(SandboxPlan {
            program: BWRAP.to_owned(),
            arguments,
            command,
            argv: argv.to_vec(),
            mounts,
            environment,
            limits,
            stdin: None,
            links,
        })
    }

    /// The whole command line after the sandbox program: its options, then
    /// what runs inside.
    pub fn command_line(&self) -> Vec<String> {
        let mut line = self.arguments.clone();
        line.extend(self.command.iter().cloned());
        line
    }

    /// Whether `path` would exist inside the sandbox as something of the
    /// host's: it is, or lies inside, a host path the plan shows, and is not
    /// covered by the private `/tmp`.
    /// A path under one of the plan's links is taken where the link leads.
    pub fn shows(&self, path: &str) -> bool {
        let mut path = PathBuf::from(path);
        for (name, target) in &self.links {
            if let Ok(rest) = path.strip_prefix(name) {
                path = Path::new(target).join(rest);
                break;
            }
        }
        let path = path.as_path();
        let covered = self
            .mounts
            .iter()
            .any(|mount| mount.kind == MountKind::Tmpfs && path.starts_with(&mount.path));
        !covered
            && self
                .mounts
                .iter()
                .any(|mount| mount.is_host_path() && path.starts_with(&mount.path))
    }
}

fn check_command(argv: &[String]) -> Result<(), SandboxError> {
    let Some(program) = argv.first() else {
        return Err(SandboxError::Command("it names no program"));
    };
    if program.is_empty() {
        return Err(SandboxError::Command("it names no program"));
    }
    if argv.len() > MAX_ARGUMENTS {
        return Err(SandboxError::Command("it has too many arguments"));
    }
    if argv
        .iter()
        .any(|argument| argument.len() > MAX_ARGUMENT_BYTES)
    {
        return Err(SandboxError::Command("an argument is too long"));
    }
    if argv.iter().any(|argument| argument.contains('\0')) {
        return Err(SandboxError::Command(
            "an argument holds a character that cannot be passed to a program",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/ada";

    /// Paths that must never be visible in any sandbox, whatever is granted.
    const PROTECTED: &[&str] = &[
        "/home/ada/.ssh",
        "/home/ada/.ssh/id_ed25519",
        "/home/ada/.gnupg",
        "/home/ada/.gnupg/private-keys-v1.d",
        "/home/ada/.pki",
        "/home/ada/.password-store",
        "/home/ada/.local/share/keyrings",
        "/home/ada/.local/share/keyrings/login.keyring",
        "/home/ada/.local/share/kwalletd",
        "/home/ada/.mozilla",
        "/home/ada/.mozilla/firefox/abcd.default/logins.json",
        "/home/ada/.thunderbird",
        "/home/ada/.config/google-chrome",
        "/home/ada/.config/chromium/Default/Login Data",
        "/home/ada/.config/BraveSoftware",
        "/home/ada/.var/app",
        "/home/ada/snap",
        "/home/ada/.aws/credentials",
        "/home/ada/.kube/config",
        "/home/ada/.docker/config.json",
        "/home/ada/.config/gh/hosts.yml",
        "/home/ada/.netrc",
        "/home/ada/.git-credentials",
        "/home/ada/.bash_history",
        "/home/ada/.config",
        "/home/ada/.local",
        "/home/ada/.cache",
        "/home/grace",
        "/home/grace/Documents",
        "/root",
        "/root/.ssh",
        "/run/user",
        "/run/user/1000",
        "/run/user/1000/bus",
        "/run/user/1000/wayland-0",
        "/run/user/1000/keyring",
        "/run/user/1000/gnupg/S.gpg-agent",
        "/run/user/1000/pipewire-0",
        "/run/dbus/system_bus_socket",
        "/run/systemd/private",
        "/run/docker.sock",
        "/tmp",
        "/tmp/.X11-unix",
        "/tmp/.X11-unix/X0",
        "/tmp/ssh-XXXXabcd/agent.1234",
        "/var/lib",
        "/var/lib/sss",
        "/var/tmp",
        "/var/run",
        "/boot",
        "/proc/1/environ",
    ];

    /// Grants a hostile or careless caller might try. Every one is refused.
    const HOSTILE: &[&str] = &[
        "/",
        "/home",
        "/home/ada",
        "/home/ada/",
        "/home/ada/.",
        "/home/ada/..",
        "/home/ada/Documents/..",
        "/home/ada/Documents/../.ssh",
        "/home/ada/./.ssh",
        "/home/ada//.ssh",
        "home/ada/Documents",
        "",
        ".",
        "/home/ada/.ssh",
        "/home/ada/.ssh/keys",
        "/home/ada/.gnupg",
        "/home/ada/.local",
        "/home/ada/.local/share",
        "/home/ada/.local/share/keyrings",
        "/home/ada/.config",
        "/home/ada/.config/google-chrome/Default",
        "/home/ada/.mozilla/firefox",
        "/home/ada/.cache",
        "/home/ada/.hidden-project",
        "/home/ada/snap",
        "/home/ada/snap/firefox",
        "/home/ada/.netrc",
        "/home/grace",
        "/home/grace/Documents",
        "/root",
        "/root/notes",
        "/run",
        "/run/user",
        "/run/user/1000",
        "/run/user/1000/bus",
        "/run/dbus",
        "/run/media/ada/stick",
        "/tmp",
        "/tmp/.X11-unix",
        "/tmp/work",
        "/var",
        "/var/lib",
        "/var/tmp/work",
        "/var/home/ada/Documents",
        "/proc",
        "/proc/self",
        "/sys",
        "/dev",
        "/dev/shm",
        "/boot",
        "/usr",
        "/usr/share/doc",
        "/etc",
        "/etc/ssh",
        "/bin",
        "/home/ada/Docu\nments",
        "/home/ada/Documents\0",
    ];

    /// Grants a user may reasonably make.
    const ACCEPTABLE: &[&str] = &[
        "/home/ada/Documents",
        "/home/ada/Documents/reports/2026",
        "/home/ada/projects/tur",
        "/home/ada/Downloads/manual.pdf",
        "/mnt/data/shared",
        "/media/ada/stick",
        "/srv/files",
        "/opt/tool/share",
    ];

    fn rules() -> GrantRules {
        GrantRules::for_home(HOME)
    }

    fn command() -> Vec<String> {
        vec!["/usr/bin/uname".to_owned(), "-a".to_owned()]
    }

    fn plan_with(paths: &[&str]) -> Result<SandboxPlan, SandboxError> {
        let grants: Vec<Grant> = paths.iter().map(|path| Grant::unchecked(path)).collect();
        SandboxPlan::read_only(&rules(), &grants, &command(), Limits::default())
    }

    /// Every way a plan could put a protected path into the sandbox.
    fn assert_nothing_protected_is_shown(plan: &SandboxPlan, context: &str) {
        for protected in PROTECTED {
            assert!(
                !plan.shows(protected),
                "{context}: {protected} is visible in the sandbox"
            );
            // Nor may a host path that lies inside a protected one be shown:
            // that would expose a part of it.
            for mount in plan.mounts.iter().filter(|mount| mount.is_host_path()) {
                assert!(
                    !Path::new(&mount.path).starts_with(protected),
                    "{context}: {} lies inside {protected}",
                    mount.path
                );
            }
        }
        // The home directory itself is never shown, only folders inside it.
        for mount in plan.mounts.iter().filter(|mount| mount.is_host_path()) {
            assert!(
                !Path::new(HOME).starts_with(&mount.path),
                "{context}: {} is or contains the home directory",
                mount.path
            );
        }
    }

    fn assert_the_fixed_parts_hold(plan: &SandboxPlan, context: &str) {
        let line = plan.command_line();
        for required in [
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-uts",
            "--unshare-net",
            "--disable-userns",
            "--die-with-parent",
            "--new-session",
            "--clearenv",
        ] {
            assert!(
                plan.arguments.iter().any(|argument| argument == required),
                "{context}: {required} is missing"
            );
        }
        // Nothing writable from the host, no device, no shared network, no
        // way to keep privileges.
        let options = &plan.arguments;
        for forbidden in [
            "--bind",
            "--bind-try",
            "--dev-bind",
            "--dev-bind-try",
            "--share-net",
            "--cap-add",
            "--overlay",
            "--tmp-overlay",
            "--uid",
            "--gid",
            "--userns",
            "--pidns",
        ] {
            assert!(
                !options.iter().any(|argument| argument == forbidden),
                "{context}: {forbidden} is present"
            );
        }
        // The sandbox ends read-only, and the command comes after `--`.
        let end = options.len();
        assert_eq!(options[end - 1], "--", "{context}");
        assert_eq!(options[end - 3..end - 1], ["--chdir", "/"], "{context}");
        assert_eq!(
            options[end - 5..end - 3],
            ["--remount-ro", "/"],
            "{context}"
        );
        assert_eq!(
            options.iter().filter(|argument| *argument == "--").count(),
            1,
            "{context}"
        );
        assert_eq!(line[options.len()], PRLIMIT, "{context}");
        // The private /tmp is mounted after every host path.
        let last = plan.mounts.last().expect("a plan has mounts");
        assert_eq!((last.kind, last.path.as_str()), (MountKind::Tmpfs, "/tmp"));
        // The environment is the fixed one.
        let expected: Vec<(String, String)> = ENVIRONMENT
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(plan.environment, expected, "{context}");
        let set: Vec<&String> = options
            .iter()
            .enumerate()
            .filter(|(index, _)| *index > 0 && options[index - 1] == "--setenv")
            .map(|(_, name)| name)
            .collect();
        assert_eq!(set.len(), ENVIRONMENT.len(), "{context}");
        assert_eq!(plan.program, BWRAP);
    }

    #[test]
    fn a_plan_without_grants_shows_the_system_and_nothing_of_the_user() {
        let plan = plan_with(&[]).expect("a plan");
        assert_the_fixed_parts_hold(&plan, "no grants");
        assert_nothing_protected_is_shown(&plan, "no grants");
        for visible in ["/usr/bin/uname", "/etc/os-release", "/var/log/syslog"] {
            assert!(plan.shows(visible), "{visible}");
        }
        for absent in [
            "/home/ada/Documents",
            "/mnt/data",
            "/var/lib/flatpak",
            "/opt",
        ] {
            assert!(!plan.shows(absent), "{absent}");
        }
        // The command is passed on as given, after the limits.
        let tail = &plan.command[plan.command.len() - 2..];
        assert_eq!(tail, ["/usr/bin/uname", "-a"]);
        assert_eq!(plan.argv, command());
        assert!(plan.stdin.is_none());
    }

    #[test]
    fn every_hostile_grant_is_refused_by_the_rules_and_by_the_plan() {
        for &path in HOSTILE {
            assert!(rules().judge(path).is_err(), "{path:?} passed the rules");
            // A grant that reached the plan without being checked is still
            // refused there.
            match plan_with(&[path]) {
                Err(SandboxError::Grant { .. }) => {}
                other => panic!("{path:?} was not refused by the plan: {other:?}"),
            }
            // Hidden among acceptable grants, it still stops the plan.
            let mut mixed: Vec<&str> = ACCEPTABLE.to_vec();
            mixed.insert(3, path);
            assert!(plan_with(&mixed).is_err(), "{path:?} among others");
        }
    }

    #[test]
    fn no_set_of_acceptable_grants_exposes_a_protected_path() {
        // Every subset of the acceptable grants.
        let count = ACCEPTABLE.len();
        for mask in 0u32..(1 << count) {
            let chosen: Vec<&str> = (0..count)
                .filter(|bit| mask & (1 << bit) != 0)
                .map(|bit| ACCEPTABLE[bit])
                .collect();
            let context = format!("grants {chosen:?}");
            let plan = plan_with(&chosen).unwrap_or_else(|error| panic!("{context}: {error}"));
            assert_the_fixed_parts_hold(&plan, &context);
            assert_nothing_protected_is_shown(&plan, &context);
            for path in &chosen {
                assert!(plan.shows(path), "{context}: {path} is not shown");
                assert!(
                    plan.mounts
                        .iter()
                        .any(|mount| mount.kind == MountKind::Granted && mount.path == *path),
                    "{context}"
                );
            }
        }
    }

    #[test]
    fn a_grant_is_always_read_only_and_must_exist() {
        let plan = plan_with(&["/home/ada/Documents"]).expect("a plan");
        let position = plan
            .arguments
            .iter()
            .position(|argument| argument == "/home/ada/Documents")
            .expect("the grant is on the command line");
        assert_eq!(plan.arguments[position - 1], "--ro-bind");
        assert_eq!(plan.arguments[position + 1], "/home/ada/Documents");
        // The same grant twice is mounted once.
        let twice = plan_with(&["/home/ada/Documents", "/home/ada/Documents"]).expect("a plan");
        assert_eq!(twice.mounts, plan.mounts);
    }

    #[test]
    fn the_rules_say_why() {
        let rules = rules();
        let cases: &[(&str, GrantRefusal)] = &[
            ("/", GrantRefusal::Root),
            ("/home/ada", GrantRefusal::ContainsHome),
            ("/home", GrantRefusal::ContainsHome),
            ("/home/ada/.cache", GrantRefusal::HiddenInHome),
            (
                "/home/ada/.ssh",
                GrantRefusal::Protected {
                    what: "the SSH directory",
                },
            ),
            (
                "/home/ada/.local/share",
                GrantRefusal::Protected {
                    what: "the desktop keyring",
                },
            ),
            (
                "/home/ada/snap/firefox",
                GrantRefusal::Protected {
                    what: "the data of snap applications",
                },
            ),
            ("/home/grace/Documents", GrantRefusal::OtherAccount),
            ("/run/user/1000", GrantRefusal::System),
            ("/var/lib", GrantRefusal::System),
            ("/tmp/work", GrantRefusal::System),
            ("/usr/share/doc", GrantRefusal::System),
            ("relative", GrantRefusal::NotPlain),
            ("/home/ada/../grace", GrantRefusal::NotPlain),
        ];
        for (path, expected) in cases {
            assert_eq!(rules.judge(path).as_ref(), Err(expected), "{path}");
        }
        for path in ACCEPTABLE {
            assert_eq!(rules.judge(path), Ok(()), "{path}");
        }
    }

    #[test]
    fn a_home_directory_that_would_widen_the_rules_is_not_used() {
        for home in [
            "/", "/home", "/var", "/root", "/usr", "relative", "/a/../b", "",
        ] {
            let rules = GrantRules::for_home(home);
            assert!(rules.homes().is_empty(), "{home:?} was accepted as a home");
            // With no home known, nothing among the home directories passes.
            for path in ["/home/ada/Documents", "/home/ada/.ssh", "/root/x"] {
                assert!(rules.judge(path).is_err(), "{home:?}: {path}");
            }
        }
        // A home somewhere unusual protects its neighbours too.
        let rules = GrantRules::for_home("/export/people/ada");
        assert_eq!(rules.judge("/export/people/ada/Documents"), Ok(()));
        assert_eq!(
            rules.judge("/export/people/grace/Documents"),
            Err(GrantRefusal::OtherAccount)
        );
        assert_eq!(
            rules.judge("/export/people/ada/.ssh"),
            Err(GrantRefusal::Protected {
                what: "the SSH directory"
            })
        );
        assert_eq!(
            rules.judge("/export/people"),
            Err(GrantRefusal::ContainsHome)
        );
    }

    #[test]
    fn a_command_that_cannot_be_run_is_refused() {
        let rules = rules();
        let limits = Limits::default();
        let refused: Vec<Vec<String>> = vec![
            Vec::new(),
            vec![String::new()],
            vec!["/usr/bin/true".to_owned(); MAX_ARGUMENTS + 1],
            vec!["/usr/bin/echo".to_owned(), "a\0b".to_owned()],
            vec![
                "/usr/bin/echo".to_owned(),
                "x".repeat(MAX_ARGUMENT_BYTES + 1),
            ],
        ];
        for argv in refused {
            assert!(
                matches!(
                    SandboxPlan::read_only(&rules, &[], &argv, limits),
                    Err(SandboxError::Command(_))
                ),
                "{} arguments",
                argv.len()
            );
        }
        // Arguments that look like sandbox options are only arguments of the
        // command: they come after the `--`.
        let argv = vec![
            "--bind".to_owned(),
            "/".to_owned(),
            "/".to_owned(),
            "--share-net".to_owned(),
        ];
        let plan = SandboxPlan::read_only(&rules, &[], &argv, limits).expect("a plan");
        assert_the_fixed_parts_hold(&plan, "option-like arguments");
        assert_nothing_protected_is_shown(&plan, "option-like arguments");
        assert_eq!(plan.command[plan.command.len() - 4..], argv[..]);
    }

    #[test]
    fn the_limits_are_on_the_command_line() {
        let limits = Limits {
            wall_seconds: 3,
            cpu_seconds: 2,
            address_space_bytes: 1024,
            processes: 4,
            open_files: 16,
            file_size_bytes: 0,
            output_bytes: 100,
        };
        let plan = SandboxPlan::read_only(&rules(), &[], &command(), limits).expect("a plan");
        assert_eq!(
            plan.command[..8],
            [
                PRLIMIT,
                "--cpu=2",
                "--as=1024",
                "--nproc=4",
                "--nofile=16",
                "--fsize=0",
                "--core=0",
                "--"
            ]
        );
        assert_eq!(plan.limits, limits);
    }

    #[test]
    fn too_many_grants_are_refused() {
        let paths: Vec<String> = (0..=MAX_GRANTS)
            .map(|index| format!("/home/ada/Documents/{index}"))
            .collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        assert!(matches!(
            plan_with(&refs),
            Err(SandboxError::Grant {
                refusal: GrantRefusal::TooMany,
                ..
            })
        ));
    }

    /// The links of Fedora Atomic, and so of TUR, as `ls -l /` shows them.
    const ATOMIC_LINKS: &[(&str, &str)] = &[
        ("/home", "/var/home"),
        ("/mnt", "/var/mnt"),
        ("/opt", "/var/opt"),
        ("/srv", "/var/srv"),
        ("/root", "/var/roothome"),
        ("/usr/local", "/var/usrlocal"),
    ];

    #[test]
    fn on_an_image_based_system_a_folder_gets_the_same_answer_under_either_name() {
        // $HOME may hold either form; the answer is the same.
        for home in ["/var/home/ada", "/home/ada"] {
            let rules = GrantRules::with_links(home, ATOMIC_LINKS);
            assert_eq!(rules.links().len(), ATOMIC_LINKS.len(), "{home}");
            for form in ["/var/home/ada", "/home/ada"] {
                assert!(
                    rules.homes().iter().any(|known| known == Path::new(form)),
                    "{home}: {form}"
                );
            }
            let cases: &[(&str, Result<(), GrantRefusal>)] = &[
                ("/var/home/ada/Documents", Ok(())),
                ("/home/ada/Documents", Ok(())),
                ("/var/mnt/data/shared", Ok(())),
                ("/var/opt/tool/share", Ok(())),
                ("/var/srv/files", Ok(())),
                ("/var/home/ada", Err(GrantRefusal::ContainsHome)),
                ("/var/home", Err(GrantRefusal::ContainsHome)),
                ("/var", Err(GrantRefusal::ContainsHome)),
                ("/var/home/ada/.cache", Err(GrantRefusal::HiddenInHome)),
                (
                    "/var/home/ada/.ssh",
                    Err(GrantRefusal::Protected {
                        what: "the SSH directory",
                    }),
                ),
                ("/var/home/grace/Documents", Err(GrantRefusal::OtherAccount)),
                ("/var/roothome", Err(GrantRefusal::System)),
                ("/var/roothome/notes", Err(GrantRefusal::System)),
                ("/var/usrlocal/bin", Err(GrantRefusal::System)),
                ("/var/lib", Err(GrantRefusal::System)),
                ("/var/log", Err(GrantRefusal::System)),
                ("/var/tmp/work", Err(GrantRefusal::System)),
            ];
            for (path, expected) in cases {
                assert_eq!(&rules.judge(path), expected, "{home}: {path}");
            }
            // Everything refused without the links is still refused.
            for path in HOSTILE {
                if *path != "/var/home/ada/Documents" {
                    assert!(rules.judge(path).is_err(), "{home}: {path:?} passed");
                }
            }
        }
    }

    #[test]
    fn a_link_that_could_widen_a_grant_is_not_taken() {
        for (name, target) in [
            ("/home", "/"),
            ("/home", "/etc"),
            ("/home", "/etc/home"),
            ("/home", "/usr/home"),
            ("/home", "/run/home"),
            ("/home", "/var"),
            ("/home", "/var/log"),
            ("/home", "/var/log/home"),
            ("/home", "/home"),
            ("/home", "/mnt/home"),
            ("/mnt", "/home/ada"),
            ("/home", "var/home"),
            ("/home", "/var/../etc"),
            ("/media", "/run/media"),
            ("/var", "/var/x"),
            ("/etc", "/var/etc"),
        ] {
            assert!(
                SystemLink::new(name, Path::new(target)).is_none(),
                "{name} -> {target} was taken"
            );
        }
        for (name, target) in ATOMIC_LINKS
            .iter()
            .copied()
            .chain([("/home", "/data/home"), ("/srv", "/export/srv")])
        {
            let link = SystemLink::new(name, Path::new(target)).expect("a system link");
            assert_eq!(
                (link.name(), link.target()),
                (Path::new(name), Path::new(target))
            );
        }
        // Rules given such a link leave it out, and judge as without it.
        let rules = GrantRules::with_links(HOME, &[("/home", "/etc")]);
        assert!(rules.links().is_empty());
        assert_eq!(rules.judge("/etc/ada/Documents"), Err(GrantRefusal::System));
    }

    #[test]
    fn the_sandbox_has_the_link_a_granted_folder_is_named_through() {
        let rules = GrantRules::with_links("/var/home/ada", ATOMIC_LINKS);
        let grants = [
            Grant::unchecked("/var/home/ada/Documents"),
            Grant::unchecked("/var/mnt/data"),
        ];
        let plan =
            SandboxPlan::read_only(&rules, &grants, &command(), Limits::default()).expect("a plan");
        assert_the_fixed_parts_hold(&plan, "through links");
        // Two links, for the two grants, and none for /opt, /srv, /root or
        // /usr/local, which lead to nothing granted or are not at the top.
        assert_eq!(
            plan.links,
            [
                ("/home".to_owned(), "/var/home".to_owned()),
                ("/mnt".to_owned(), "/var/mnt".to_owned())
            ]
        );
        let line = plan.arguments.join(" ");
        assert!(line.contains("--symlink /var/home /home"), "{line}");
        assert!(
            line.contains("--ro-bind /var/home/ada/Documents /var/home/ada/Documents"),
            "{line}"
        );
        for shown in [
            "/home/ada/Documents",
            "/home/ada/Documents/report.txt",
            "/var/home/ada/Documents",
            "/mnt/data/x",
        ] {
            assert!(plan.shows(shown), "{shown}");
        }
        for hidden in [
            "/home/ada",
            "/home/ada/.ssh",
            "/home/ada/Pictures",
            "/home/grace/Documents",
            "/var/home/ada/.ssh",
            "/root",
            "/opt/tool",
        ] {
            assert!(!plan.shows(hidden), "{hidden}");
        }
        // With nothing granted there is no link at all.
        let plan =
            SandboxPlan::read_only(&rules, &[], &command(), Limits::default()).expect("a plan");
        assert!(plan.links.is_empty());
        assert!(
            !plan
                .arguments
                .iter()
                .any(|argument| argument == "--symlink")
        );
    }

    /// A scratch directory of the test's own, made fresh.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tur-pax-sandbox-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_grant_named_through_a_system_link_is_admitted_where_the_folder_really_is() {
        // A stand-in for the system's /home -> var/home: the real link
        // belongs to the administrator, which a test cannot make, so the
        // rules are given it.
        let root = scratch("admit");
        let real = root.join("var/home");
        let named = root.join("home");
        fs::create_dir_all(real.join("ada/Documents")).expect("folders");
        fs::create_dir_all(real.join("ada/.ssh")).expect("folders");
        fs::create_dir_all(root.join("elsewhere")).expect("folders");
        std::os::unix::fs::symlink(&real, &named).expect("a link");
        // A link inside the granted tree is still refused.
        std::os::unix::fs::symlink(root.join("elsewhere"), real.join("ada/Escape"))
            .expect("a link");
        std::os::unix::fs::symlink(real.join("ada/.ssh"), real.join("ada/Keys")).expect("a link");

        let link = SystemLink {
            name: named.clone(),
            target: real.clone(),
        };
        let home = named.join("ada");
        let rules = GrantRules::build(Some(home.as_path()), None, vec![link]);

        let through = named.join("ada/Documents");
        let grant = rules
            .admit(through.to_str().expect("text"))
            .expect("admitted");
        assert_eq!(Path::new(grant.path()), real.join("ada/Documents"));
        let direct = rules
            .admit(real.join("ada/Documents").to_str().expect("text"))
            .expect("admitted");
        assert_eq!(direct, grant);

        for refused in ["ada/Escape", "ada/Keys", "ada/Keys/id_ed25519"] {
            let path = named.join(refused);
            assert!(
                matches!(
                    rules.admit(path.to_str().expect("text")),
                    Err(GrantRefusal::Link { .. } | GrantRefusal::Missing)
                ),
                "{refused}"
            );
        }
        assert!(matches!(
            rules.admit(named.join("ada/.ssh").to_str().expect("text")),
            Err(GrantRefusal::Protected { .. })
        ));
        assert_eq!(
            rules.admit(named.join("ada").to_str().expect("text")),
            Err(GrantRefusal::ContainsHome)
        );
        // Without the link in the rules, the same name is refused as a link.
        let plain = GrantRules::build(Some(real.join("ada").as_path()), None, Vec::new());
        assert!(matches!(
            plain.admit(through.to_str().expect("text")),
            Err(GrantRefusal::Link { .. })
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_paths_are_told_from_the_others() {
        for plain in ["/", "/a", "/home/ada/My Documents", "/a/b.c/d-e_f"] {
            assert!(is_plain_absolute(plain), "{plain}");
        }
        for other in [
            "", "a", "a/b", "/a/", "//a", "/a//b", "/a/./b", "/a/../b", "/a\n", "/a\u{7f}",
        ] {
            assert!(!is_plain_absolute(other), "{other:?}");
        }
        assert!(!is_plain_absolute(&format!(
            "/{}",
            "a".repeat(MAX_PATH_BYTES)
        )));
        assert!(is_system_visible("/usr/bin/tur-device"));
        assert!(is_system_visible("/etc/os-release"));
        assert!(!is_system_visible("/usr/../home/ada"));
        assert!(!is_system_visible("/home/ada/bin/tool"));
        assert!(!is_system_visible("/usrlocal/bin/tool"));
    }
}
