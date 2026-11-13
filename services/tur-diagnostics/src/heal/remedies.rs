// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The remedies, one function per [`RemedyKind`], and their undo.
//!
//! A remedy does exactly the operation its kind names, with parameters from
//! the catalogue and subjects from the check: nothing comes from a request
//! but the fault's code and the remedy's name. What is needed to undo it is
//! returned as [`UndoData`] and kept with the fault's record; a file a
//! remedy replaces is copied first to the data directory, readable by root
//! only, because it may hold the person's own settings.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::catalogue::{Check, Fault, Remedy, RemedyKind};
use super::checks::Finding;
use super::machine::Machine;

/// The subdirectory of the data directory that holds copies of replaced
/// files.
pub const BACKUP_DIR: &str = "backups";

/// What undoing a remedy takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum UndoData {
    /// The remedy changed no setting and no file.
    NothingChanged,
    /// Switch network time off again.
    NetworkTimeOff,
    /// Put back the copy kept in this file of the data directory.
    RestoreCopy {
        /// The copy's file name under `backups/`.
        backup: String,
        /// The file it replaces.
        path: String,
    },
    /// The remedy cannot be undone.
    Irreversible,
}

/// The copy of a replaced file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum KeptCopy {
    /// It was a symbolic link to this target.
    Link {
        /// The target, as written.
        target: String,
    },
    /// It was a regular file.
    File {
        /// Its content.
        content: Vec<u8>,
        /// Its permission bits.
        mode: u32,
    },
    /// Nothing was there.
    Missing,
}

/// What applying a remedy did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// How to undo it.
    pub undo: UndoData,
    /// What was done, for the record.
    pub detail: String,
}

fn command(machine: &dyn Machine, program: &str, args: &[&str]) -> Result<String, String> {
    let line = format!("{program} {}", args.join(" "));
    match machine.run(program, args) {
        Ok(output) if output.success() => Ok(line),
        Ok(output) => Err(format!(
            "{line} exited with {}: {}",
            output
                .exit_code
                .map_or_else(|| "a signal".to_owned(), |code| code.to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(format!("{line} could not be run: {error:?}")),
    }
}

/// Applies `remedy` of `fault` to what `finding` found. `id` names this
/// application; a copy of a replaced file is kept under that name in
/// `data_dir`.
///
/// # Errors
///
/// A step of the remedy failed. What was already done is in the message.
pub fn apply(
    machine: &dyn Machine,
    data_dir: &Path,
    fault: &Fault,
    remedy: &Remedy,
    finding: &Finding,
    id: &str,
) -> Result<Applied, String> {
    match remedy.kind {
        RemedyKind::RestartUnits => {
            if finding.subjects.is_empty() {
                return Err("the check named no failed unit".to_owned());
            }
            let mut done = Vec::new();
            for unit in &finding.subjects {
                done.push(command(machine, "systemctl", &["reset-failed", unit])?);
                done.push(command(machine, "systemctl", &["start", unit])?);
            }
            Ok(Applied {
                undo: UndoData::NothingChanged,
                detail: done.join("; "),
            })
        }
        RemedyKind::SetNetworkTime => Ok(Applied {
            undo: UndoData::NetworkTimeOff,
            detail: command(machine, "timedatectl", &["set-ntp", "true"])?,
        }),
        RemedyKind::RestoreResolverLink => {
            let Check::ResolverNotManaged {
                path,
                restore_target,
                ..
            } = &fault.check
            else {
                return Err("the fault's check is not a resolver check".to_owned());
            };
            let copy = match machine.read_link(path) {
                Ok(target) => KeptCopy::Link { target },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => KeptCopy::Missing,
                Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => KeptCopy::File {
                    content: machine
                        .read_file(path)
                        .map_err(|error| format!("{path} cannot be read: {error:?}"))?,
                    mode: machine
                        .mode(path)
                        .map_err(|error| format!("{path}: {error}"))?,
                },
                Err(error) => return Err(format!("{path}: {error}")),
            };
            let backup = format!("{id}.json");
            keep_copy(data_dir, &backup, &copy)?;
            machine
                .replace_with_link(path, restore_target)
                .map_err(|error| format!("{path} could not be replaced: {error}"))?;
            machine.relabel(path);
            Ok(Applied {
                undo: UndoData::RestoreCopy {
                    backup,
                    path: path.clone(),
                },
                detail: format!(
                    "kept a copy of {path} as {BACKUP_DIR}/{id}.json; {path} -> {restore_target}"
                ),
            })
        }
        RemedyKind::VacuumJournal => {
            let keep = remedy
                .keep_bytes
                .ok_or_else(|| "the catalogue gives no size to keep".to_owned())?;
            let size = format!("--vacuum-size={keep}");
            Ok(Applied {
                undo: UndoData::Irreversible,
                detail: command(machine, "journalctl", &["--rotate", &size])?,
            })
        }
    }
}

fn keep_copy(data_dir: &Path, name: &str, copy: &KeptCopy) -> Result<(), String> {
    let directory = data_dir.join(BACKUP_DIR);
    super::state::private_dir(&directory)?;
    let text = serde_json::to_vec(copy).map_err(|error| error.to_string())?;
    let path = directory.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(&text)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Undoes an applied remedy.
///
/// # Errors
///
/// It cannot be undone, or a step failed.
pub fn undo(machine: &dyn Machine, data_dir: &Path, undo: &UndoData) -> Result<String, String> {
    match undo {
        UndoData::NothingChanged => {
            Err("this remedy changed no setting and no file: there is nothing to undo".to_owned())
        }
        UndoData::Irreversible => Err("this remedy cannot be undone".to_owned()),
        UndoData::NetworkTimeOff => command(machine, "timedatectl", &["set-ntp", "false"]),
        UndoData::RestoreCopy { backup, path } => {
            let file = data_dir.join(BACKUP_DIR).join(backup);
            let text = fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?;
            let copy: KeptCopy =
                serde_json::from_slice(&text).map_err(|error| format!("{}: {error}", file.display()))?;
            let put_back = match &copy {
                KeptCopy::Link { target } => machine.replace_with_link(path, target),
                KeptCopy::File { content, mode } => machine.replace_with_file(path, content, *mode),
                KeptCopy::Missing => {
                    return Err(format!(
                        "{path} did not exist before; it is left as the remedy made it"
                    ));
                }
            };
            put_back.map_err(|error| format!("{path} could not be put back: {error}"))?;
            machine.relabel(path);
            // The copy has served; a failure to remove it leaves a small file.
            let _ = fs::remove_file(&file);
            Ok(format!("put back {path} from {BACKUP_DIR}/{backup}"))
        }
    }
}
