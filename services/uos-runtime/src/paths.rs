// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Where the runtime manager keeps an application's prefix, backups and logs.
//!
//! ```text
//! $XDG_DATA_HOME/uos/runtime/
//!     prefixes/<app-id>/metadata.json     written by uos-runtime only
//!     prefixes/<app-id>/prefix/           the Wine prefix (WINEPREFIX)
//!     prefixes/<app-id>/home/             the application's private HOME
//!     backups/<app-id>/<time>-<reason>.tar.gz
//!     staging/<app-id>/                   scratch space for restore
//! $XDG_STATE_HOME/uos/runtime/
//!     logs/<app-id>/<time>-<pid>.log      one file per launch
//!     logs/<app-id>/operations.log        output of prefix operations
//! ```
//!
//! Only `prefix/` and `home/` are ever made visible to the application. The
//! metadata file sits beside them, outside the sandbox, so an application
//! cannot change its own grants.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::limits::MAX_PATH_BYTES;
use crate::profile::is_app_id;

/// The directory under the XDG base directories that belongs to this tool.
const OWN_SUBDIRECTORY: [&str; 2] = ["uos", "runtime"];

/// Why the runtime manager's directories could not be worked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathsError {
    /// Neither the XDG variable nor `HOME` names an absolute directory.
    NoHome,
    /// A directory name is not valid UTF-8, holds a control character, has a
    /// `..` component or is too long.
    Unusable {
        /// The variable the value came from.
        variable: &'static str,
    },
    /// The text is not a valid application identifier.
    BadAppId,
}

impl fmt::Display for PathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathsError::NoHome => f.write_str(
                "cannot find the user's data directory: HOME is not set to an absolute path",
            ),
            PathsError::Unusable { variable } => write!(
                f,
                "the directory named by {variable} cannot be used: it must be an absolute path \
                 without \"..\" or control characters"
            ),
            PathsError::BadAppId => f.write_str(
                "the application identifier must be lower-case segments joined by dots, such as \
                 com.example.app",
            ),
        }
    }
}

impl std::error::Error for PathsError {}

/// Whether `text` is an absolute path this crate is willing to plan with:
/// UTF-8, no control characters, no `.` or `..` component, bounded length.
pub(crate) fn is_plain_absolute(text: &str) -> bool {
    text.starts_with('/')
        && text.len() <= MAX_PATH_BYTES
        && !text.chars().any(char::is_control)
        && !text
            .split('/')
            .any(|component| component == ".." || component == ".")
}

/// The path as text, when it is a plain absolute path.
pub(crate) fn plain_text(path: &Path) -> Option<&str> {
    path.to_str().filter(|text| is_plain_absolute(text))
}

/// The runtime manager's two root directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDirs {
    data_root: PathBuf,
    state_root: PathBuf,
}

impl RuntimeDirs {
    /// The directories under the given XDG data and state directories.
    pub fn new(data_home: &Path, state_home: &Path) -> Self {
        let own: PathBuf = OWN_SUBDIRECTORY.iter().collect();
        RuntimeDirs {
            data_root: data_home.join(&own),
            state_root: state_home.join(&own),
        }
    }

    /// Resolves the directories from environment variables, following the XDG
    /// Base Directory rules: `XDG_DATA_HOME` and `XDG_STATE_HOME` when they
    /// hold an absolute path, otherwise `$HOME/.local/share` and
    /// `$HOME/.local/state`.
    ///
    /// # Errors
    ///
    /// [`PathsError`] when no usable directory can be found.
    pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, PathsError> {
        let home = lookup("HOME").filter(|value| value.starts_with('/'));
        let resolve = |variable: &'static str, fallback: [&str; 2]| {
            let explicit = lookup(variable).filter(|value| value.starts_with('/'));
            let (value, source) = match (explicit, &home) {
                (Some(value), _) => (PathBuf::from(value), variable),
                (None, Some(home)) => (Path::new(home).join(fallback[0]).join(fallback[1]), "HOME"),
                (None, None) => return Err(PathsError::NoHome),
            };
            if plain_text(&value).is_none() {
                return Err(PathsError::Unusable { variable: source });
            }
            Ok(value)
        };
        let data_home = resolve("XDG_DATA_HOME", [".local", "share"])?;
        let state_home = resolve("XDG_STATE_HOME", [".local", "state"])?;
        Ok(RuntimeDirs::new(&data_home, &state_home))
    }

    /// The root of everything this tool stores as data.
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    /// The root of everything this tool stores as state (logs).
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    /// The directory holding one directory per application.
    pub fn prefixes(&self) -> PathBuf {
        self.data_root.join("prefixes")
    }

    /// The paths of one application.
    ///
    /// # Errors
    ///
    /// [`PathsError::BadAppId`] when `app_id` is not a valid identifier. The
    /// identifier becomes a directory name, so nothing else is accepted.
    pub fn app(&self, app_id: &str) -> Result<AppPaths, PathsError> {
        if !is_app_id(app_id) {
            return Err(PathsError::BadAppId);
        }
        let app_dir = self.prefixes().join(app_id);
        Ok(AppPaths {
            app_id: app_id.to_owned(),
            metadata: app_dir.join("metadata.json"),
            prefix: app_dir.join("prefix"),
            home: app_dir.join("home"),
            app_dir,
            backups: self.data_root.join("backups").join(app_id),
            staging: self.data_root.join("staging").join(app_id),
            logs: self.state_root.join("logs").join(app_id),
        })
    }
}

/// The paths that belong to one application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// The application identifier the paths were derived from.
    pub app_id: String,
    /// The application's directory, holding everything below except backups,
    /// staging and logs.
    pub app_dir: PathBuf,
    /// The metadata file. Never visible to the application.
    pub metadata: PathBuf,
    /// The Wine prefix.
    pub prefix: PathBuf,
    /// The private home directory the application sees as `HOME`.
    pub home: PathBuf,
    /// Where backups of this application are kept.
    pub backups: PathBuf,
    /// Scratch space used while restoring a backup.
    pub staging: PathBuf,
    /// The application's log directory.
    pub logs: PathBuf,
}

impl AppPaths {
    /// The archive a backup with this name is stored in.
    pub fn backup_archive(&self, name: &str) -> PathBuf {
        self.backups.join(format!("{name}.tar.gz"))
    }

    /// The log file of prefix operations (creation, repair, migration).
    pub fn operations_log(&self) -> PathBuf {
        self.logs.join("operations.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn defaults_follow_the_xdg_rules() {
        let dirs = RuntimeDirs::from_env(lookup(&[("HOME", "/home/ada")])).unwrap();
        assert_eq!(
            dirs.data_root(),
            Path::new("/home/ada/.local/share/uos/runtime")
        );
        assert_eq!(
            dirs.state_root(),
            Path::new("/home/ada/.local/state/uos/runtime")
        );

        let dirs = RuntimeDirs::from_env(lookup(&[
            ("HOME", "/home/ada"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_STATE_HOME", "relative/ignored"),
        ]))
        .unwrap();
        assert_eq!(dirs.data_root(), Path::new("/data/uos/runtime"));
        assert_eq!(
            dirs.state_root(),
            Path::new("/home/ada/.local/state/uos/runtime")
        );
    }

    #[test]
    fn unusable_environments_are_refused() {
        assert_eq!(RuntimeDirs::from_env(lookup(&[])), Err(PathsError::NoHome));
        assert_eq!(
            RuntimeDirs::from_env(lookup(&[("HOME", "relative")])),
            Err(PathsError::NoHome)
        );
        assert_eq!(
            RuntimeDirs::from_env(lookup(&[("HOME", "/home/ada/../root")])),
            Err(PathsError::Unusable { variable: "HOME" })
        );
        assert_eq!(
            RuntimeDirs::from_env(lookup(&[
                ("HOME", "/home/ada"),
                ("XDG_DATA_HOME", "/data/\u{1b}[31m")
            ])),
            Err(PathsError::Unusable {
                variable: "XDG_DATA_HOME"
            })
        );
    }

    #[test]
    fn application_paths_are_separate_and_predictable() {
        let dirs = RuntimeDirs::new(Path::new("/d"), Path::new("/s"));
        let app = dirs.app("com.example.app").unwrap();
        assert_eq!(app.app_id, "com.example.app");
        assert_eq!(
            app.app_dir,
            Path::new("/d/uos/runtime/prefixes/com.example.app")
        );
        assert_eq!(app.metadata, app.app_dir.join("metadata.json"));
        assert_eq!(app.prefix, app.app_dir.join("prefix"));
        assert_eq!(app.home, app.app_dir.join("home"));
        assert_eq!(
            app.backups,
            Path::new("/d/uos/runtime/backups/com.example.app")
        );
        assert_eq!(
            app.staging,
            Path::new("/d/uos/runtime/staging/com.example.app")
        );
        assert_eq!(app.logs, Path::new("/s/uos/runtime/logs/com.example.app"));
        assert_eq!(
            app.backup_archive("20261112T101500Z-manual"),
            app.backups.join("20261112T101500Z-manual.tar.gz")
        );
        assert_eq!(app.operations_log(), app.logs.join("operations.log"));

        // The metadata is not inside either directory the application sees.
        assert!(!app.metadata.starts_with(&app.prefix));
        assert!(!app.metadata.starts_with(&app.home));

        let other = dirs.app("com.example.other").unwrap();
        assert!(!other.prefix.starts_with(&app.app_dir));
        assert!(!app.prefix.starts_with(&other.app_dir));
    }

    #[test]
    fn identifiers_that_could_leave_the_directory_are_refused() {
        let dirs = RuntimeDirs::new(Path::new("/d"), Path::new("/s"));
        for bad in [
            "",
            "app",
            "..",
            "../x",
            "com/example",
            "com.example/../../x",
            ".hidden.app",
            "com..app",
            "Com.Example",
            "com.example.app\n",
            "com.example.app ",
        ] {
            assert_eq!(dirs.app(bad), Err(PathsError::BadAppId), "{bad:?}");
        }
    }

    #[test]
    fn plain_absolute_paths() {
        assert!(is_plain_absolute("/home/ada/Games"));
        assert!(is_plain_absolute("/"));
        assert!(!is_plain_absolute("home/ada"));
        assert!(!is_plain_absolute("/home/ada/../root"));
        assert!(!is_plain_absolute("/home/./ada"));
        assert!(!is_plain_absolute("/home/ada\n"));
        assert!(!is_plain_absolute(&format!(
            "/{}",
            "a".repeat(MAX_PATH_BYTES)
        )));
        assert_eq!(plain_text(Path::new("/a/b")), Some("/a/b"));
        assert_eq!(plain_text(Path::new("a/b")), None);
    }
}
