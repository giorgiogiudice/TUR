// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Which host paths an application may be shown (specification §12.2).
//!
//! A Windows program under Wine can read and write whatever its Linux process
//! can. The sandbox decides what that is, and this module decides what may go
//! into the sandbox. Every rule is applied to the path with all symbolic
//! links resolved, because that is what gets mounted:
//!
//! * `/`, the home directory and anything that contains the home directory
//!   are refused.
//! * A protected location (keys, keyrings, browser profiles, other
//!   applications' prefixes, the user's runtime directory) is refused, and so
//!   is anything inside it and anything that contains it.
//! * Any hidden entry of the home directory is refused. Credentials and
//!   application profiles live there by convention, and no list of them can
//!   be complete.
//! * System directories and kernel file systems are refused: the sandbox
//!   provides its own view of them.
//!
//! A grant is checked when the user makes it and again at every launch. At
//! launch the recorded path must still resolve to itself, so an application
//! that can write inside one granted directory cannot turn another into a
//! link to somewhere else.

use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::metadata::Grant;
use crate::paths::{AppPaths, RuntimeDirs, is_plain_absolute};

/// Locations under the home directory that are named in refusals. Every other
/// hidden entry of the home directory is refused too, without a name.
const PROTECTED_IN_HOME: [(&str, &str); 22] = [
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

/// Directories the sandbox provides its own view of. A grant may not be one
/// of them, lie inside one or contain one.
const SYSTEM_TREES: [&str; 18] = [
    "/proc",
    "/sys",
    "/dev",
    "/usr",
    "/etc",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/lib32",
    "/libx32",
    "/boot",
    "/root",
    "/run/user",
    "/var/lib",
    "/var/log",
    "/var/cache",
    "/var/spool",
];

/// Directories shared by every user and program, refused as a whole. A
/// directory inside one can be granted.
const SHARED_TEMPORARY: [&str; 2] = ["/tmp", "/var/tmp"];

/// Kernel file systems a program file is never taken from.
const KERNEL_TREES: [&str; 3] = ["/proc", "/sys", "/dev"];

/// Why a path may not be shown to an application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantRefusal {
    /// The path is relative, has a `.` or `..` component, holds a control
    /// character, is not UTF-8 or is too long.
    NotPlain,
    /// The path does not exist or cannot be resolved.
    Missing,
    /// The path is not a directory (for a grant) or not a regular file (for a
    /// program).
    WrongKind,
    /// A recorded grant no longer resolves to itself.
    Moved {
        /// Where it resolves now.
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
    /// The path is a system directory, lies inside one or contains one.
    System,
}

impl fmt::Display for GrantRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrantRefusal::NotPlain => f.write_str(
                "it must be an absolute path without \".\" or \"..\" components or control \
                 characters",
            ),
            GrantRefusal::Missing => f.write_str("it does not exist or cannot be resolved"),
            GrantRefusal::WrongKind => f.write_str("it is not the kind of file that is needed"),
            GrantRefusal::Moved { resolved } => write!(
                f,
                "it now resolves to {resolved:?}, which is not what was granted"
            ),
            GrantRefusal::Root => f.write_str("it is the root of the file system"),
            GrantRefusal::ContainsHome => {
                f.write_str("it is the home directory or contains the home directory")
            }
            GrantRefusal::HiddenInHome => f.write_str(
                "it is in a hidden part of the home directory, where credentials and \
                 application profiles are kept",
            ),
            GrantRefusal::Protected { what } => {
                write!(f, "it would expose {what}")
            }
            GrantRefusal::System => f.write_str(
                "it is a system directory, which the sandbox already provides read-only or \
                 replaces",
            ),
        }
    }
}

struct Protected {
    path: PathBuf,
    what: &'static str,
}

/// The locations the grant rules protect on this machine, for this user.
pub struct GrantContext {
    /// The home directory as named and with symbolic links resolved.
    homes: Vec<PathBuf>,
    protected: Vec<Protected>,
}

/// `path` and, when it exists and differs, `path` with links resolved.
fn both_forms(path: &Path) -> Vec<PathBuf> {
    let mut forms = vec![path.to_path_buf()];
    if let Ok(resolved) = fs::canonicalize(path)
        && resolved != path
    {
        forms.push(resolved);
    }
    forms
}

impl GrantContext {
    /// Builds the context for a user.
    ///
    /// * `home`: the user's home directory.
    /// * `dirs`: the runtime manager's own directories, which hold every
    ///   application's prefix.
    /// * `runtime_dir`: the user's `XDG_RUNTIME_DIR`, which holds the keyring,
    ///   GnuPG and session-bus sockets.
    /// * `agent_socket`: the SSH agent socket, wherever it is.
    pub fn new(
        home: &Path,
        dirs: &RuntimeDirs,
        runtime_dir: Option<&Path>,
        agent_socket: Option<&Path>,
    ) -> Self {
        let homes = both_forms(home);
        let mut protected = Vec::new();
        for home in &homes {
            for (relative, what) in PROTECTED_IN_HOME {
                protected.push(Protected {
                    path: home.join(relative),
                    what,
                });
            }
        }
        let own = "the runtime manager's data, which holds other applications' prefixes";
        for root in [dirs.data_root(), dirs.state_root()] {
            for path in both_forms(root) {
                protected.push(Protected { path, what: own });
            }
        }
        if let Some(runtime_dir) = runtime_dir {
            for path in both_forms(runtime_dir) {
                protected.push(Protected {
                    path,
                    what: "the user's runtime directory, which holds keyring and session sockets",
                });
            }
        }
        if let Some(socket) = agent_socket {
            for path in both_forms(socket) {
                protected.push(Protected {
                    path,
                    what: "the SSH agent socket",
                });
            }
        }
        GrantContext { homes, protected }
    }

    /// Checks a directory the user wants to grant. Returns the path to
    /// record: the directory with every symbolic link resolved.
    ///
    /// # Errors
    ///
    /// The [`GrantRefusal`] that applies.
    pub fn admit(&self, requested: &str) -> Result<String, GrantRefusal> {
        if !is_plain_absolute(requested) {
            return Err(GrantRefusal::NotPlain);
        }
        let resolved = fs::canonicalize(requested).map_err(|_| GrantRefusal::Missing)?;
        let text = resolved
            .to_str()
            .filter(|text| is_plain_absolute(text))
            .ok_or(GrantRefusal::NotPlain)?
            .to_owned();
        self.judge_directory(&resolved)?;
        Ok(text)
    }

    /// Checks a recorded grant against the file system as it is now. Returns
    /// the directory to mount.
    ///
    /// # Errors
    ///
    /// [`GrantRefusal::Moved`] when the path no longer resolves to itself,
    /// and otherwise the refusal that applies.
    pub fn recheck(&self, grant: &Grant) -> Result<PathBuf, GrantRefusal> {
        if !is_plain_absolute(&grant.path) {
            return Err(GrantRefusal::NotPlain);
        }
        let recorded = Path::new(&grant.path);
        let resolved = fs::canonicalize(recorded).map_err(|_| GrantRefusal::Missing)?;
        if resolved != recorded {
            return Err(GrantRefusal::Moved {
                resolved: resolved.display().to_string(),
            });
        }
        self.judge_directory(&resolved)?;
        Ok(resolved)
    }

    /// Checks the program file of an application that lies outside its
    /// prefix. Returns the file with every symbolic link resolved; only that
    /// one file is shown to the application, never its directory.
    ///
    /// # Errors
    ///
    /// The [`GrantRefusal`] that applies.
    pub fn admit_program(&self, binary: &str, app: &AppPaths) -> Result<ProgramFile, GrantRefusal> {
        if !is_plain_absolute(binary) {
            return Err(GrantRefusal::NotPlain);
        }
        let resolved = fs::canonicalize(binary).map_err(|_| GrantRefusal::Missing)?;
        if resolved
            .to_str()
            .is_none_or(|text| !is_plain_absolute(text))
        {
            return Err(GrantRefusal::NotPlain);
        }
        let is_file = fs::metadata(&resolved).is_ok_and(|metadata| metadata.is_file());
        if !is_file {
            return Err(GrantRefusal::WrongKind);
        }
        // A program installed inside the application's own prefix is already
        // visible, at the prefix's own path.
        for prefix in both_forms(&app.prefix) {
            if let Ok(inside) = resolved.strip_prefix(&prefix) {
                return Ok(ProgramFile::InPrefix(app.prefix.join(inside)));
            }
        }
        if KERNEL_TREES.iter().any(|tree| resolved.starts_with(tree)) {
            return Err(GrantRefusal::System);
        }
        if let Some(found) = self
            .protected
            .iter()
            .find(|entry| resolved.starts_with(&entry.path))
        {
            return Err(GrantRefusal::Protected { what: found.what });
        }
        if self.hidden_in_home(&resolved) {
            return Err(GrantRefusal::HiddenInHome);
        }
        Ok(ProgramFile::Outside(resolved))
    }

    fn hidden_in_home(&self, resolved: &Path) -> bool {
        self.homes.iter().any(|home| {
            resolved.strip_prefix(home).is_ok_and(|rest| {
                matches!(
                    rest.components().next(),
                    Some(Component::Normal(name)) if name.as_encoded_bytes().starts_with(b".")
                )
            })
        })
    }

    /// Applies every rule to a resolved path that is to be mounted as a
    /// directory.
    fn judge_directory(&self, resolved: &Path) -> Result<(), GrantRefusal> {
        let is_dir = fs::metadata(resolved).is_ok_and(|metadata| metadata.is_dir());
        if !is_dir {
            return Err(GrantRefusal::WrongKind);
        }
        self.judge(resolved)
    }

    /// The rules themselves. `resolved` is absolute and has no links.
    pub(crate) fn judge(&self, resolved: &Path) -> Result<(), GrantRefusal> {
        if resolved.parent().is_none() {
            return Err(GrantRefusal::Root);
        }
        if self.homes.iter().any(|home| home.starts_with(resolved)) {
            return Err(GrantRefusal::ContainsHome);
        }
        if let Some(found) = self
            .protected
            .iter()
            .find(|entry| overlaps(resolved, &entry.path))
        {
            return Err(GrantRefusal::Protected { what: found.what });
        }
        if self.hidden_in_home(resolved) {
            return Err(GrantRefusal::HiddenInHome);
        }
        if SYSTEM_TREES
            .iter()
            .any(|tree| overlaps(resolved, Path::new(tree)))
        {
            return Err(GrantRefusal::System);
        }
        if SHARED_TEMPORARY
            .iter()
            .any(|shared| resolved == Path::new(shared))
        {
            return Err(GrantRefusal::System);
        }
        Ok(())
    }
}

/// Whether mounting `candidate` would expose `protected`: the two are equal,
/// or one contains the other.
fn overlaps(candidate: &Path, protected: &Path) -> bool {
    candidate.starts_with(protected) || protected.starts_with(candidate)
}

/// Where an application's program file is, relative to its prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramFile {
    /// Inside the application's prefix; the path is the one the application
    /// sees.
    InPrefix(PathBuf),
    /// Outside the prefix; this one file has to be made visible.
    Outside(PathBuf),
}

impl ProgramFile {
    /// The path the application is started from.
    pub fn path(&self) -> &Path {
        match self {
            ProgramFile::InPrefix(path) | ProgramFile::Outside(path) => path,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::metadata::GrantAccess;
    use crate::testdata::TempDir;

    /// A scratch "machine": a home directory with protected locations in it,
    /// ordinary directories, and symbolic links that lead out of them.
    pub(crate) struct Machine {
        pub(crate) temp: TempDir,
        pub(crate) home: PathBuf,
        pub(crate) dirs: RuntimeDirs,
        pub(crate) runtime_dir: PathBuf,
        pub(crate) outside: PathBuf,
    }

    impl Machine {
        pub(crate) fn new(label: &str) -> Self {
            let temp = TempDir::new(label);
            // The scratch directory itself may be reached through a link
            // (/tmp on some systems), so resolve it once.
            let root = fs::canonicalize(temp.path()).unwrap();
            let home = root.join("home/ada");
            let runtime_dir = root.join("run/user/1000");
            let outside = root.join("srv/shared");
            for directory in [
                ".ssh",
                ".gnupg",
                ".mozilla/firefox",
                ".config/google-chrome/Default",
                ".config/other-tool",
                ".local/share/keyrings",
                ".local/share/tur/runtime/prefixes/com.example.other/prefix",
                ".local/state/tur/runtime/logs",
                ".var/app/org.example.Browser",
                "snap/firefox",
                "Documents/taxes",
                "Games/installed",
                "Saves",
            ] {
                fs::create_dir_all(home.join(directory)).unwrap();
            }
            fs::create_dir_all(runtime_dir.join("keyring")).unwrap();
            fs::create_dir_all(runtime_dir.join("gnupg")).unwrap();
            fs::create_dir_all(&outside).unwrap();
            fs::write(home.join(".ssh/id_ed25519"), b"not a real key").unwrap();
            fs::write(home.join("Documents/taxes/2026.txt"), b"private").unwrap();
            fs::write(home.join("Games/setup.exe"), b"MZ").unwrap();
            fs::write(runtime_dir.join("bus"), b"").unwrap();

            // Links that lead out of ordinary directories.
            symlink(home.join(".ssh"), home.join("Games/keys")).unwrap();
            symlink("/", home.join("Games/everything")).unwrap();
            symlink(&home, home.join("Games/home")).unwrap();
            symlink("..", home.join("Documents/up")).unwrap();
            symlink(home.join("Documents"), home.join("Games/documents")).unwrap();
            symlink(home.join("Games"), root.join("games-link")).unwrap();
            symlink(home.join(".ssh/id_ed25519"), home.join("Games/key.exe")).unwrap();
            symlink(
                home.join(".local/share/tur/runtime"),
                outside.join("prefixes"),
            )
            .unwrap();

            let dirs = RuntimeDirs::new(&home.join(".local/share"), &home.join(".local/state"));
            Machine {
                temp,
                home,
                dirs,
                runtime_dir,
                outside,
            }
        }

        pub(crate) fn context(&self) -> GrantContext {
            GrantContext::new(
                &self.home,
                &self.dirs,
                Some(&self.runtime_dir),
                Some(&self.runtime_dir.join("ssh-agent.socket")),
            )
        }

        pub(crate) fn root(&self) -> PathBuf {
            fs::canonicalize(self.temp.path()).unwrap()
        }

        pub(crate) fn path(&self, relative: &str) -> String {
            self.home.join(relative).to_str().unwrap().to_owned()
        }

        /// Every path a sandbox must never expose, as absolute paths.
        pub(crate) fn secrets(&self) -> Vec<PathBuf> {
            let mut secrets: Vec<PathBuf> = [
                ".ssh",
                ".ssh/id_ed25519",
                ".gnupg",
                ".mozilla",
                ".mozilla/firefox",
                ".config/google-chrome",
                ".config/google-chrome/Default",
                ".local/share/keyrings",
                ".local/share/tur/runtime/prefixes/com.example.other",
                ".local/share/tur/runtime/prefixes/com.example.other/prefix",
                ".var/app/org.example.Browser",
                "snap/firefox",
            ]
            .iter()
            .map(|relative| self.home.join(relative))
            .collect();
            secrets.push(self.runtime_dir.join("keyring"));
            secrets.push(self.runtime_dir.join("gnupg"));
            secrets.push(self.runtime_dir.join("bus"));
            secrets.push(self.runtime_dir.join("ssh-agent.socket"));
            secrets
        }

        /// Grant requests a hostile or careless caller might make, with
        /// whether each is acceptable.
        pub(crate) fn candidates(&self) -> Vec<(String, bool)> {
            let root = self.root();
            let outside = self.outside.to_str().unwrap().to_owned();
            let mut candidates = vec![
                // Acceptable.
                (self.path("Games"), true),
                (self.path("Games/installed"), true),
                (self.path("Documents"), true),
                (self.path("Documents/taxes"), true),
                (self.path("Saves"), true),
                (outside.clone(), true),
                // A link to an ordinary directory resolves to that directory.
                (self.path("Games/documents"), true),
                (root.join("games-link").to_str().unwrap().to_owned(), true),
                // Too broad.
                ("/".to_owned(), false),
                (root.to_str().unwrap().to_owned(), false),
                (root.join("home").to_str().unwrap().to_owned(), false),
                (self.path(""), false),
                // Protected locations, their contents and their parents.
                (self.path(".ssh"), false),
                (self.path(".gnupg"), false),
                (self.path(".mozilla"), false),
                (self.path(".mozilla/firefox"), false),
                (self.path(".config"), false),
                (self.path(".config/google-chrome"), false),
                (self.path(".config/google-chrome/Default"), false),
                (self.path(".config/other-tool"), false),
                (self.path(".local"), false),
                (self.path(".local/share"), false),
                (self.path(".local/share/keyrings"), false),
                (self.path(".local/share/tur"), false),
                (self.path(".local/share/tur/runtime/prefixes"), false),
                (
                    self.path(".local/share/tur/runtime/prefixes/com.example.other/prefix"),
                    false,
                ),
                (self.path(".local/state/tur/runtime/logs"), false),
                (self.path(".var"), false),
                (self.path(".var/app/org.example.Browser"), false),
                (self.path("snap"), false),
                (self.path("snap/firefox"), false),
                (self.runtime_dir.to_str().unwrap().to_owned(), false),
                (
                    self.runtime_dir
                        .join("keyring")
                        .to_str()
                        .unwrap()
                        .to_owned(),
                    false,
                ),
                (
                    self.runtime_dir
                        .parent()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                    false,
                ),
                // ".." in any position.
                (self.path("Games/../.ssh"), false),
                (self.path("Games/.."), false),
                (self.path("Games/../Games"), false),
                (format!("{outside}/../../home/ada/.ssh"), false),
                ("/..".to_owned(), false),
                (self.path("Games/./installed"), false),
                // Links that lead somewhere protected or too broad.
                (self.path("Games/keys"), false),
                (self.path("Games/everything"), false),
                (self.path("Games/home"), false),
                (self.path("Documents/up"), false),
                (format!("{outside}/prefixes"), false),
                (
                    format!("{outside}/prefixes/prefixes/com.example.other"),
                    false,
                ),
                ("/proc/self/root".to_owned(), false),
                // System directories and kernel file systems.
                ("/usr".to_owned(), false),
                ("/usr/bin".to_owned(), false),
                ("/etc".to_owned(), false),
                ("/proc".to_owned(), false),
                ("/proc/self".to_owned(), false),
                ("/sys".to_owned(), false),
                ("/dev".to_owned(), false),
                ("/dev/shm".to_owned(), false),
                ("/run".to_owned(), false),
                ("/var".to_owned(), false),
                ("/tmp".to_owned(), false),
                // Not directories, not there, not paths.
                (self.path("Games/setup.exe"), false),
                (self.path("Games/missing"), false),
                (self.path("Games/key.exe"), false),
                ("relative/path".to_owned(), false),
                (String::new(), false),
                (self.path("Games\n"), false),
                (format!("/{}", "a".repeat(5000)), false),
            ];
            // The same requests with a trailing slash must not change the answer.
            let with_slash: Vec<(String, bool)> = candidates
                .iter()
                .filter(|(path, _)| path.len() > 1 && !path.ends_with('/') && path.len() < 4000)
                .map(|(path, acceptable)| (format!("{path}/"), *acceptable))
                .collect();
            candidates.extend(with_slash);
            candidates
        }
    }

    /// Whether mounting `source` would expose `secret`.
    pub(crate) fn exposes(source: &Path, secret: &Path) -> bool {
        let source = fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
        let secret = fs::canonicalize(secret).unwrap_or_else(|_| secret.to_path_buf());
        secret.starts_with(&source)
    }

    #[test]
    fn every_candidate_gets_the_expected_answer() {
        let machine = Machine::new("grant-candidates");
        let context = machine.context();
        for (requested, acceptable) in machine.candidates() {
            let answer = context.admit(&requested);
            assert_eq!(answer.is_ok(), acceptable, "{requested:?} gave {answer:?}");
        }
    }

    #[test]
    fn no_admitted_directory_exposes_a_secret() {
        let machine = Machine::new("grant-secrets");
        let context = machine.context();
        let secrets = machine.secrets();
        let mut admitted = 0;
        for (requested, _) in machine.candidates() {
            let Ok(recorded) = context.admit(&requested) else {
                continue;
            };
            admitted += 1;
            for secret in &secrets {
                assert!(
                    !exposes(Path::new(&recorded), secret),
                    "{requested:?} was admitted as {recorded:?} and exposes {secret:?}"
                );
            }
            // What is recorded is fully resolved, so mounting it cannot be
            // redirected through a link later.
            assert_eq!(fs::canonicalize(&recorded).unwrap(), Path::new(&recorded));
        }
        assert!(admitted >= 8, "only {admitted} candidates were admitted");
    }

    #[test]
    fn refusals_say_why() {
        let machine = Machine::new("grant-reasons");
        let context = machine.context();
        let cases = [
            ("/".to_owned(), GrantRefusal::Root),
            (machine.path(""), GrantRefusal::ContainsHome),
            (
                machine.path(".ssh"),
                GrantRefusal::Protected {
                    what: "the SSH directory",
                },
            ),
            (
                machine.path("Games/keys"),
                GrantRefusal::Protected {
                    what: "the SSH directory",
                },
            ),
            (
                machine.path(".config/other-tool"),
                GrantRefusal::HiddenInHome,
            ),
            (machine.path("Games/everything"), GrantRefusal::Root),
            (machine.path("Games/home"), GrantRefusal::ContainsHome),
            (machine.path("Games/../.ssh"), GrantRefusal::NotPlain),
            (machine.path("Games/missing"), GrantRefusal::Missing),
            (machine.path("Games/setup.exe"), GrantRefusal::WrongKind),
            ("/usr/bin".to_owned(), GrantRefusal::System),
        ];
        for (requested, expected) in cases {
            assert_eq!(context.admit(&requested), Err(expected), "{requested:?}");
        }
        for refusal in [
            GrantRefusal::NotPlain,
            GrantRefusal::Missing,
            GrantRefusal::WrongKind,
            GrantRefusal::Moved {
                resolved: "/x".to_owned(),
            },
            GrantRefusal::Root,
            GrantRefusal::ContainsHome,
            GrantRefusal::HiddenInHome,
            GrantRefusal::Protected { what: "a thing" },
            GrantRefusal::System,
        ] {
            assert!(refusal.to_string().starts_with("it "));
        }
    }

    #[test]
    fn the_rules_for_an_ordinary_home_directory() {
        // Nothing here has to exist: the rules are applied to resolved paths.
        let dirs = RuntimeDirs::new(
            Path::new("/home/ada/.local/share"),
            Path::new("/home/ada/.local/state"),
        );
        let context = GrantContext::new(
            Path::new("/home/ada"),
            &dirs,
            Some(Path::new("/run/user/1000")),
            Some(Path::new("/tmp/ssh-XXXXabcd/agent.1234")),
        );
        for allowed in [
            "/home/ada/Games",
            "/home/ada/Documents/work",
            "/home/bob",
            "/mnt/data",
            "/run/media/ada/USB",
            "/tmp/scratch",
            "/var/home/ada/Games",
            "/srv",
        ] {
            assert_eq!(context.judge(Path::new(allowed)), Ok(()), "{allowed}");
        }
        let refused = [
            ("/", GrantRefusal::Root),
            ("/home", GrantRefusal::ContainsHome),
            ("/home/ada", GrantRefusal::ContainsHome),
            ("/home/ada/.cache", GrantRefusal::HiddenInHome),
            ("/home/ada/.local/share/Steam", GrantRefusal::HiddenInHome),
            ("/var/tmp", GrantRefusal::System),
            ("/usr/share/games", GrantRefusal::System),
            ("/var", GrantRefusal::System),
            ("/boot/efi", GrantRefusal::System),
        ];
        for (path, expected) in refused {
            assert_eq!(context.judge(Path::new(path)), Err(expected), "{path}");
        }
        for (path, what) in [
            ("/home/ada/.ssh", "the SSH directory"),
            ("/home/ada/.ssh/keys", "the SSH directory"),
            (
                "/home/ada/snap/firefox/common",
                "the data of snap applications",
            ),
            ("/tmp/ssh-XXXXabcd", "the SSH agent socket"),
            ("/tmp", "the SSH agent socket"),
            (
                "/run",
                "the user's runtime directory, which holds keyring and session sockets",
            ),
            (
                "/run/user/1000/gnupg",
                "the user's runtime directory, which holds keyring and session sockets",
            ),
        ] {
            assert_eq!(
                context.judge(Path::new(path)),
                Err(GrantRefusal::Protected { what }),
                "{path}"
            );
        }
    }

    #[test]
    fn admitted_paths_are_recorded_resolved() {
        let machine = Machine::new("grant-resolved");
        let context = machine.context();
        assert_eq!(
            context.admit(&machine.path("Games/documents")),
            Ok(machine.path("Documents"))
        );
        assert_eq!(
            context.admit(&format!("{}/", machine.path("Saves"))),
            Ok(machine.path("Saves"))
        );
    }

    #[test]
    fn a_recorded_grant_must_still_resolve_to_itself() {
        let machine = Machine::new("grant-recheck");
        let context = machine.context();
        let grant = |path: String| Grant {
            path,
            access: GrantAccess::ReadWrite,
        };

        let saves = grant(machine.path("Saves"));
        assert_eq!(context.recheck(&saves), Ok(machine.home.join("Saves")));

        // An application with write access to Games replaces a granted
        // sub-directory with a link to the user's documents.
        let nested = grant(machine.path("Games/installed"));
        assert!(context.recheck(&nested).is_ok());
        fs::remove_dir(machine.home.join("Games/installed")).unwrap();
        symlink(
            machine.home.join("Documents"),
            machine.home.join("Games/installed"),
        )
        .unwrap();
        assert_eq!(
            context.recheck(&nested),
            Err(GrantRefusal::Moved {
                resolved: machine.path("Documents")
            })
        );

        // Metadata edited by hand cannot smuggle a protected path in.
        for path in [
            machine.path(".ssh"),
            machine.path(""),
            "/".to_owned(),
            machine.path("Games/../.ssh"),
            machine.path("Games/keys"),
            machine.path("Games/missing"),
            machine.path("Games/setup.exe"),
            "relative".to_owned(),
        ] {
            let answer = context.recheck(&grant(path.clone()));
            assert!(answer.is_err(), "{path:?} gave {answer:?}");
        }
    }

    #[test]
    fn program_files() {
        let machine = Machine::new("grant-program");
        let context = machine.context();
        let app = machine.dirs.app("com.example.app").unwrap();
        fs::create_dir_all(app.prefix.join("drive_c/Program Files/App")).unwrap();
        fs::write(app.prefix.join("drive_c/Program Files/App/app.exe"), b"MZ").unwrap();

        // Outside the prefix: the one file, resolved.
        assert_eq!(
            context.admit_program(&machine.path("Games/setup.exe"), &app),
            Ok(ProgramFile::Outside(machine.home.join("Games/setup.exe")))
        );
        // Inside the application's own prefix: nothing extra to show.
        let installed = app.prefix.join("drive_c/Program Files/App/app.exe");
        let answer = context
            .admit_program(installed.to_str().unwrap(), &app)
            .unwrap();
        assert_eq!(answer, ProgramFile::InPrefix(installed.clone()));
        assert_eq!(answer.path(), installed);

        // A program in another application's prefix, in a protected
        // directory, or reached through a link into one.
        let other = machine.dirs.app("com.example.other").unwrap();
        fs::write(other.prefix.join("other.exe"), b"MZ").unwrap();
        fs::write(machine.home.join(".config/other-tool/tool.exe"), b"MZ").unwrap();
        let refused = [
            (
                other.prefix.join("other.exe").to_str().unwrap().to_owned(),
                GrantRefusal::Protected {
                    what: "the runtime manager's data, which holds other applications' prefixes",
                },
            ),
            (
                machine.path(".ssh/id_ed25519"),
                GrantRefusal::Protected {
                    what: "the SSH directory",
                },
            ),
            (
                machine.path("Games/key.exe"),
                GrantRefusal::Protected {
                    what: "the SSH directory",
                },
            ),
            (
                machine.path(".config/other-tool/tool.exe"),
                GrantRefusal::HiddenInHome,
            ),
            (machine.path("Games"), GrantRefusal::WrongKind),
            (machine.path("Games/missing.exe"), GrantRefusal::Missing),
            (
                machine.path("Games/../Games/setup.exe"),
                GrantRefusal::NotPlain,
            ),
            ("/proc/self/environ".to_owned(), GrantRefusal::System),
            ("setup.exe".to_owned(), GrantRefusal::NotPlain),
        ];
        for (binary, expected) in refused {
            assert_eq!(
                context.admit_program(&binary, &app),
                Err(expected),
                "{binary:?}"
            );
        }
    }
}
