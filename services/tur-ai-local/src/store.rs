// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The downloaded models: `$XDG_DATA_HOME/tur/models/<id>/`.
//!
//! Each model has a folder named after its catalogue id, holding its files
//! under their publisher's names and `verified.json`, the record written
//! after every file was checked. A model is usable only when that record
//! names the digests the catalogue gives and every file has its catalogue
//! size. Partial downloads end in `.part` and are never used.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalogue::{Model, is_model_id};

/// The record of verification, inside a model's folder.
pub const VERIFIED_FILE: &str = "verified.json";

/// The suffix of a file being downloaded.
pub const PART_SUFFIX: &str = ".part";

/// The default models folder: `$XDG_DATA_HOME/tur/models`, or
/// `$HOME/.local/share/tur/models` when XDG_DATA_HOME is not set.
pub fn default_root() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        })?;
    Some(data.join("tur/models"))
}

/// One verified file in the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedFile {
    /// The file name.
    pub name: String,
    /// Its size.
    pub size_bytes: u64,
    /// Its SHA-256 digest.
    pub sha256: String,
}

/// The record of verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verified {
    /// The record format version.
    pub schema_version: u32,
    /// The catalogue id.
    pub model: String,
    /// The files, as checked.
    pub files: Vec<VerifiedFile>,
    /// When, in seconds since 1970.
    pub verified_at: u64,
}

string_enum! {
    /// Whether a model is on this computer.
    pub enum InstallState {
        /// Verified and unchanged in size.
        Installed => "installed",
        /// Not downloaded, or only partly.
        Absent => "absent",
        /// There, but the record or a size does not match the catalogue.
        Damaged => "damaged",
    }
}

/// The models folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    /// Its path.
    pub root: PathBuf,
}

impl Store {
    /// The folder of a model. `None` for a string that is not a model id,
    /// so that no path outside the store can be named.
    pub fn model_dir(&self, id: &str) -> Option<PathBuf> {
        is_model_id(id).then(|| self.root.join(id))
    }

    /// The path a file of a model is stored at.
    pub fn file_path(&self, model: &Model, name: &str) -> Option<PathBuf> {
        self.model_dir(&model.id).map(|dir| dir.join(name))
    }

    fn read_verified(&self, id: &str) -> Option<Verified> {
        let path = self.model_dir(id)?.join(VERIFIED_FILE);
        let text = fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Whether a model is installed, and if it is damaged, why.
    pub fn state(&self, model: &Model) -> (InstallState, Option<String>) {
        let Some(dir) = self.model_dir(&model.id) else {
            return (InstallState::Absent, None);
        };
        if !dir.is_dir() {
            return (InstallState::Absent, None);
        }
        let Some(verified) = self.read_verified(&model.id) else {
            let has_files = model.files().iter().any(|file| dir.join(&file.name).exists());
            return if has_files {
                (
                    InstallState::Damaged,
                    Some("the record of verification is missing or cannot be read".to_owned()),
                )
            } else {
                (InstallState::Absent, None)
            };
        };
        for file in model.files() {
            let recorded = verified.files.iter().find(|entry| entry.name == file.name);
            match recorded {
                Some(entry) if entry.sha256 == file.sha256 && entry.size_bytes == file.size_bytes => {}
                Some(_) => {
                    return (
                        InstallState::Damaged,
                        Some(format!("{} was verified as another file than the catalogue names", file.name)),
                    );
                }
                None => {
                    return (
                        InstallState::Damaged,
                        Some(format!("{} is not in the record of verification", file.name)),
                    );
                }
            }
            match fs::metadata(dir.join(&file.name)) {
                Ok(metadata) if metadata.len() == file.size_bytes => {}
                Ok(metadata) => {
                    return (
                        InstallState::Damaged,
                        Some(format!(
                            "{} has {} bytes; the catalogue gives {}",
                            file.name,
                            metadata.len(),
                            file.size_bytes
                        )),
                    );
                }
                Err(error) => {
                    return (InstallState::Damaged, Some(format!("{}: {error}", file.name)));
                }
            }
        }
        (InstallState::Installed, None)
    }

    /// Write the record of verification of a model whose files were just
    /// checked.
    pub fn write_verified(&self, model: &Model, now: u64) -> io::Result<()> {
        let dir = self
            .model_dir(&model.id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a model id"))?;
        let record = Verified {
            schema_version: 1,
            model: model.id.clone(),
            files: model
                .files()
                .iter()
                .map(|file| VerifiedFile {
                    name: file.name.clone(),
                    size_bytes: file.size_bytes,
                    sha256: file.sha256.clone(),
                })
                .collect(),
            verified_at: now,
        };
        let text = serde_json::to_string_pretty(&record)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary = dir.join(format!("{VERIFIED_FILE}{PART_SUFFIX}"));
        fs::write(&temporary, text)?;
        fs::rename(&temporary, dir.join(VERIFIED_FILE))
    }

    /// The model folders present, by name, whether or not the catalogue
    /// knows them.
    pub fn folders(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_model_id(name))
            .collect();
        names.sort();
        names
    }

    /// The bytes held by a model's folder.
    pub fn folder_bytes(&self, id: &str) -> u64 {
        let Some(dir) = self.model_dir(id) else {
            return 0;
        };
        let Ok(entries) = fs::read_dir(dir) else {
            return 0;
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.metadata().ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum()
    }

    /// Remove a model's folder. Returns the bytes freed, or `None` when the
    /// folder does not exist.
    pub fn remove(&self, id: &str) -> io::Result<Option<u64>> {
        let Some(dir) = self.model_dir(id) else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a model id"));
        };
        if !dir.is_dir() {
            return Ok(None);
        }
        // Never follow a link out of the store.
        if fs::symlink_metadata(&dir)?.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the model folder is a symbolic link",
            ));
        }
        let bytes = self.folder_bytes(id);
        fs::remove_dir_all(&dir)?;
        Ok(Some(bytes))
    }
}

/// Seconds since 1970, from the system clock.
pub fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Create a directory and its parents, readable by the owner only where it
/// is created.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}
