// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Where things are: this program's own records, and the places Flatpak and
//! `tur-runtime` put an application.
//!
//! Everything is per user. Nothing here is written outside the user's own
//! data, state and cache directories.

use std::path::{Path, PathBuf};

use crate::codes::{Code, Failure};

/// The directories of one user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// `$HOME`.
    pub home: PathBuf,
    /// `$XDG_DATA_HOME`, or `$HOME/.local/share`.
    pub data_home: PathBuf,
    /// `$XDG_CACHE_HOME`, or `$HOME/.cache`.
    pub cache_home: PathBuf,
    /// The per-user Flatpak installation: `$FLATPAK_USER_DIR`, or
    /// `$XDG_DATA_HOME/flatpak` (flatpak(1), "ENVIRONMENT").
    pub flatpak_user_dir: PathBuf,
}

impl Dirs {
    /// The directories from the environment, read through `lookup`.
    ///
    /// Only absolute paths are accepted; a relative value is ignored, as the
    /// XDG base directory specification says.
    pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Result<Dirs, Failure> {
        let absolute = |name: &str| {
            lookup(name)
                .filter(|value| value.starts_with('/'))
                .map(PathBuf::from)
        };
        let home = absolute("HOME").ok_or_else(|| {
            Failure::new(
                Code::Unreadable,
                "HOME is not set to an absolute path, so there is nowhere to install for you",
            )
        })?;
        let data_home = absolute("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
        let cache_home = absolute("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"));
        let flatpak_user_dir =
            absolute("FLATPAK_USER_DIR").unwrap_or_else(|| data_home.join("flatpak"));
        Ok(Dirs {
            home,
            data_home,
            cache_home,
            flatpak_user_dir,
        })
    }

    /// The directories of the user running this program.
    pub fn current() -> Result<Dirs, Failure> {
        Dirs::from_env(|name| std::env::var(name).ok())
    }

    /// This program's own data: `$XDG_DATA_HOME/tur/apps`.
    pub fn apps_root(&self) -> PathBuf {
        self.data_home.join("tur/apps")
    }

    /// The records of installed applications.
    pub fn records(&self) -> PathBuf {
        self.apps_root().join("installed")
    }

    /// The record of one application.
    pub fn record(&self, id: &str) -> PathBuf {
        self.records().join(format!("{id}.json"))
    }

    /// Where the data of removed applications is kept, one directory per
    /// application and removal.
    pub fn kept(&self, id: &str) -> PathBuf {
        self.apps_root().join("kept").join(id)
    }

    /// Where an installer is downloaded to, until it has been installed.
    pub fn downloads(&self, id: &str) -> PathBuf {
        self.cache_home.join("tur/apps/downloads").join(id)
    }

    /// A Flatpak application's data directory, which Flatpak keeps unless
    /// it is told to delete it: `$HOME/.var/app/<id>`.
    pub fn flatpak_data(&self, flatpak_id: &str) -> PathBuf {
        self.home.join(".var/app").join(flatpak_id)
    }

    /// Where a per-user Flatpak application is deployed.
    pub fn flatpak_app(&self, flatpak_id: &str) -> PathBuf {
        self.flatpak_user_dir.join("app").join(flatpak_id)
    }

    /// The launcher entry a per-user Flatpak application exports.
    pub fn flatpak_desktop_entry(&self, flatpak_id: &str) -> PathBuf {
        self.flatpak_user_dir
            .join("exports/share/applications")
            .join(format!("{flatpak_id}.desktop"))
    }

    /// The root of what `tur-runtime` keeps for its applications.
    pub fn runtime_root(&self) -> PathBuf {
        self.data_home.join("tur/runtime")
    }

    /// The directory `tur-runtime` keeps for one Windows application: its
    /// prefix, its private home, its record and its profile.
    pub fn runtime_app(&self, app_id: &str) -> PathBuf {
        self.runtime_root().join("prefixes").join(app_id)
    }

    /// A Windows application's private home: what it saves as "My
    /// Documents" ends up here.
    pub fn runtime_home(&self, app_id: &str) -> PathBuf {
        self.runtime_app(app_id).join("home")
    }

    /// The Windows user profiles inside a Windows application's prefix:
    /// its settings (`AppData`) and its documents folders.
    pub fn runtime_profiles(&self, app_id: &str) -> PathBuf {
        self.runtime_app(app_id).join("prefix/drive_c/users")
    }

    /// The staged launcher entry of a Windows application.
    pub fn runtime_launcher(&self, app_id: &str) -> PathBuf {
        self.runtime_root().join("launchers").join(app_id)
    }
}

/// Shows a path with the user's home directory written as `~`.
pub fn display(path: &Path, dirs: &Dirs) -> String {
    match path.strip_prefix(&dirs.home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Whether `id` may name a file and a directory of this program: a
/// catalogue identifier, lower case, dot-separated, at least two segments,
/// each starting with a letter or digit. This is the grammar of the
/// catalogue and of `tur-runtime`'s application identifiers, so the one
/// identifier serves for both.
pub fn is_app_id(id: &str) -> bool {
    id.len() <= 255
        && id.split('.').count() >= 2
        && id.split('.').all(|segment| {
            let mut bytes = segment.bytes();
            bytes
                .next()
                .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
                && bytes
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn dirs(pairs: &[(&str, &str)]) -> Result<Dirs, Failure> {
        let map: BTreeMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        Dirs::from_env(|name| map.get(name).cloned())
    }

    #[test]
    fn defaults_follow_the_xdg_rules_and_flatpak_s() {
        let found = dirs(&[("HOME", "/home/u")]).unwrap();
        assert_eq!(found.data_home, PathBuf::from("/home/u/.local/share"));
        assert_eq!(found.cache_home, PathBuf::from("/home/u/.cache"));
        assert_eq!(
            found.flatpak_user_dir,
            PathBuf::from("/home/u/.local/share/flatpak")
        );
        assert_eq!(
            found.flatpak_data("org.example.App"),
            PathBuf::from("/home/u/.var/app/org.example.App")
        );
        assert_eq!(
            found.record("org.example.app"),
            PathBuf::from("/home/u/.local/share/tur/apps/installed/org.example.app.json")
        );
        assert_eq!(
            found.runtime_home("org.example.app"),
            PathBuf::from("/home/u/.local/share/tur/runtime/prefixes/org.example.app/home")
        );
    }

    #[test]
    fn explicit_absolute_values_win_and_relative_ones_are_ignored() {
        let found = dirs(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_CACHE_HOME", "relative"),
            ("FLATPAK_USER_DIR", "/fp"),
        ])
        .unwrap();
        assert_eq!(found.data_home, PathBuf::from("/data"));
        assert_eq!(found.cache_home, PathBuf::from("/home/u/.cache"));
        assert_eq!(found.flatpak_user_dir, PathBuf::from("/fp"));
        assert_eq!(dirs(&[]).unwrap_err().code, Code::Unreadable);
        assert_eq!(
            dirs(&[("HOME", "home")]).unwrap_err().code,
            Code::Unreadable
        );
    }

    #[test]
    fn a_path_under_home_is_shown_with_a_tilde() {
        let found = dirs(&[("HOME", "/home/u")]).unwrap();
        assert_eq!(
            display(Path::new("/home/u/.var/app/x"), &found),
            "~/.var/app/x"
        );
        assert_eq!(display(Path::new("/home/u"), &found), "~");
        assert_eq!(display(Path::new("/opt/x"), &found), "/opt/x");
    }

    #[test]
    fn application_identifiers_are_the_catalogue_s() {
        for good in [
            "org.example.app",
            "uk.org.greenend.chiark.putty",
            "org.7-zip.7zip",
        ] {
            assert!(is_app_id(good), "{good}");
        }
        for bad in [
            "",
            "app",
            "Org.example",
            "org..app",
            "org.example/../x",
            "org.-x",
            ".org.x",
        ] {
            assert!(!is_app_id(bad), "{bad}");
        }
    }
}
