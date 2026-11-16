// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Where `tur-rehost` keeps things, and the application identifier that
//! names them.
//!
//! ```text
//! $XDG_DATA_HOME/tur/rehost/
//!     apps/<app-id>/record.json     written by tur-rehost only; not shown to the application
//!     apps/<app-id>/app/            the Linux runtime with the payload under it; read-only in the sandbox
//!     apps/<app-id>/home/           the application's private HOME; the only place it can write
//!     staging/<app-id>/             an installation in progress; renamed into apps/ when complete
//!     launchers/<app-id>/           the staged launcher entry
//! $XDG_CACHE_HOME/tur/rehost/
//!     downloads/<sha256>            files fetched and verified against the pinned index
//!     work/<app-id>-<pid>/          a package being opened; removed afterwards
//!     builds/<module>-<electron>/   a native module being built; removed afterwards
//! $XDG_STATE_HOME/tur/rehost/
//!     logs/<app-id>/                one log per launch, and operations.log
//! ```
//!
//! The same model as a Windows prefix (runtime-manager.md, "Prefix
//! layout"): one application, one directory; the record sits beside what
//! the application can see, so it cannot change what is recorded about it.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::codes::{Code, Failure};

/// A validated application identifier: lower case, dot-separated, at least
/// two segments of `a-z`, `0-9` and `-`, at most 255 characters. It becomes
/// a directory name, so nothing else is allowed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AppId(String);

impl AppId {
    /// Parses and validates an identifier.
    pub fn parse(text: &str) -> Result<AppId, Failure> {
        let invalid = |why: &str| {
            Failure::new(
                Code::InputInvalid,
                format!("the application identifier is not allowed: {why}"),
            )
        };
        if text.is_empty() || text.len() > 255 {
            return Err(invalid("it must be 1 to 255 characters"));
        }
        let segments: Vec<&str> = text.split('.').collect();
        if segments.len() < 2 {
            return Err(invalid("it needs at least two dot-separated segments"));
        }
        for segment in segments {
            if segment.is_empty() || segment.starts_with('-') || segment.ends_with('-') {
                return Err(invalid("a segment is empty or starts or ends with '-'"));
            }
            if !segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            {
                return Err(invalid("only a-z, 0-9, '-' and '.' are allowed"));
            }
        }
        Ok(AppId(text.to_owned()))
    }

    /// The identifier as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// An identifier made from a file or application name, under `local.`:
    /// `Foo Bar.exe` gives `local.foo-bar`.
    pub fn from_name(name: &str) -> AppId {
        let stem = name
            .rsplit_once('.')
            .filter(|(stem, extension)| !stem.is_empty() && extension.len() <= 4)
            .map_or(name, |(stem, _)| stem);
        let mut out = String::new();
        for character in stem.chars() {
            let lower = character.to_ascii_lowercase();
            if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
                out.push(lower);
            } else if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
            if out.len() >= 64 {
                break;
            }
        }
        let trimmed = out.trim_end_matches('-');
        let segment = if trimmed.is_empty() { "app" } else { trimmed };
        AppId(format!("local.{segment}"))
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The three roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    /// `$XDG_DATA_HOME/tur/rehost`.
    pub data: PathBuf,
    /// `$XDG_CACHE_HOME/tur/rehost`.
    pub cache: PathBuf,
    /// `$XDG_STATE_HOME/tur/rehost`.
    pub state: PathBuf,
}

impl Roots {
    /// The roots from the XDG variables, falling back to `$HOME/.local/share`,
    /// `$HOME/.cache` and `$HOME/.local/state`. A relative value is ignored,
    /// as the base directory specification says.
    pub fn from_environment() -> Result<Roots, Failure> {
        let absolute = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        let home = absolute("HOME").ok_or_else(|| {
            Failure::new(
                Code::Unreadable,
                "HOME is not set to an absolute path: there is nowhere to keep applications",
            )
        })?;
        let data = absolute("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
        let cache = absolute("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"));
        let state = absolute("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state"));
        Ok(Roots {
            data: data.join("tur/rehost"),
            cache: cache.join("tur/rehost"),
            state: state.join("tur/rehost"),
        })
    }

    /// All three roots under one directory (`--data-dir`, and tests).
    pub fn under(base: &Path) -> Roots {
        Roots {
            data: base.join("data"),
            cache: base.join("cache"),
            state: base.join("state"),
        }
    }

    /// `apps/`.
    pub fn apps(&self) -> PathBuf {
        self.data.join("apps")
    }

    /// `apps/<app-id>/`.
    pub fn app(&self, id: &AppId) -> PathBuf {
        self.apps().join(id.as_str())
    }

    /// `staging/<app-id>/`.
    pub fn staging(&self, id: &AppId) -> PathBuf {
        self.data.join("staging").join(id.as_str())
    }

    /// `launchers/<app-id>/`.
    pub fn launcher(&self, id: &AppId) -> PathBuf {
        self.data.join("launchers").join(id.as_str())
    }

    /// `downloads/`.
    pub fn downloads(&self) -> PathBuf {
        self.cache.join("downloads")
    }

    /// `work/<label>-<pid>/`, a fresh directory name for one operation.
    pub fn work(&self, label: &str) -> PathBuf {
        self.cache
            .join("work")
            .join(format!("{label}-{}", std::process::id()))
    }

    /// `builds/`.
    pub fn builds(&self) -> PathBuf {
        self.cache.join("builds")
    }

    /// `logs/<app-id>/`.
    pub fn logs(&self, id: &AppId) -> PathBuf {
        self.state.join("logs").join(id.as_str())
    }
}

/// The parts of one application's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// `apps/<app-id>/` (or the staging directory while it is installed).
    pub root: PathBuf,
    /// `record.json`.
    pub record: PathBuf,
    /// `app/`: the runtime and the payload.
    pub app: PathBuf,
    /// `home/`: the private HOME.
    pub home: PathBuf,
}

impl AppPaths {
    /// The parts of the directory `root`.
    pub fn new(root: PathBuf) -> AppPaths {
        AppPaths {
            record: root.join("record.json"),
            app: root.join("app"),
            home: root.join("home"),
            root,
        }
    }
}

/// Whether `path` lies inside `root`, by components, without resolving
/// links (callers never follow them).
pub fn is_inside(path: &Path, root: &Path) -> bool {
    path.starts_with(root) && path != root
}

/// A relative path from a package or an index, made safe to join: `/`
/// separated, no empty, `.` or `..` component, no NUL, not absolute.
pub fn safe_relative(text: &str) -> Option<PathBuf> {
    if text.is_empty() || text.starts_with('/') || text.contains('\0') || text.contains('\\') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in text.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_held_to_the_rules() {
        assert!(AppId::parse("com.vscodium.vscodium").is_ok());
        assert!(AppId::parse("io.podman-desktop.podman-desktop").is_ok());
        for bad in [
            "",
            "single",
            "Com.example.app",
            "com..app",
            "com.example.",
            "com.-x.app",
            "com.example/app",
            "../etc.passwd",
            "com.exa mple.app",
        ] {
            assert!(AppId::parse(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn a_local_identifier_comes_from_the_file_name() {
        assert_eq!(
            AppId::from_name("VSCodium-win32-x64-1.143.17232.zip").as_str(),
            "local.vscodium-win32-x64-1-143-17232"
        );
        assert_eq!(AppId::from_name("Foo Bar.exe").as_str(), "local.foo-bar");
        assert_eq!(AppId::from_name("???.dmg").as_str(), "local.app");
        assert!(AppId::parse(AppId::from_name("Zulip-5.14.2-x64.zip").as_str()).is_ok());
    }

    #[test]
    fn relative_paths_from_packages_cannot_climb_out() {
        assert_eq!(
            safe_relative("node_modules/x/build/Release/x.node"),
            Some(PathBuf::from("node_modules/x/build/Release/x.node"))
        );
        for bad in ["", "/etc/passwd", "a/../b", "./a", "a//b", "a\\b", "a/\0"] {
            assert!(safe_relative(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn the_layout_keeps_the_record_beside_what_the_application_sees() {
        let roots = Roots::under(Path::new("/x"));
        let id = AppId::parse("com.example.app").unwrap();
        let paths = AppPaths::new(roots.app(&id));
        assert_eq!(
            paths.record,
            PathBuf::from("/x/data/apps/com.example.app/record.json")
        );
        assert!(is_inside(&paths.home, &paths.root));
        assert!(!is_inside(&paths.record, &paths.app));
        assert!(!is_inside(&paths.root, &paths.root));
    }
}
