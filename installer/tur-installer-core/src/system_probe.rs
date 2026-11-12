// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The probe that asks the running system.
//!
//! This is the only place in the crate that starts a process, and the only
//! program it starts is `lsblk`, which reads kernel state and writes nothing.
//! The command-line tool does not use this probe: every subcommand takes its
//! input from files. It exists so the installer service has an implementation
//! of [`Probe`] to build on.
//!
//! **Not exercised by tests.** Running it would inspect the disks of whatever
//! machine runs the tests, which the project rules forbid on development and
//! production hosts. It is to be tested in a disposable virtual machine by
//! the boot-test work.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use crate::inventory::{BY_ID_DIR, BY_PATH_DIR, DiskLinks, InventoryError, Probe};

/// Reads the block-device inventory of the running system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProbe;

impl Probe for SystemProbe {
    fn lsblk_json(&self) -> Result<String, InventoryError> {
        let unreadable = |reason: String| InventoryError::Unreadable {
            what: "the output of lsblk".to_owned(),
            reason,
        };
        let output = Command::new("lsblk")
            .args(["--json", "--bytes", "--output-all"])
            .output()
            .map_err(|error| unreadable(error.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(unreadable(format!(
                "lsblk exited with {}: {}",
                output.status,
                stderr.trim()
            )));
        }
        String::from_utf8(output.stdout).map_err(|error| unreadable(error.to_string()))
    }

    fn disk_links(&self) -> Result<DiskLinks, InventoryError> {
        Ok(DiskLinks {
            by_id: read_links(Path::new(BY_ID_DIR))?,
            by_path: read_links(Path::new(BY_PATH_DIR))?,
        })
    }
}

/// Maps each symbolic link in `directory` to the device node it points at.
/// A directory that does not exist yields an empty map.
fn read_links(directory: &Path) -> Result<BTreeMap<String, String>, InventoryError> {
    let unreadable = |error: io::Error| InventoryError::Unreadable {
        what: directory.display().to_string(),
        reason: error.to_string(),
    };
    let mut links = BTreeMap::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(links),
        Err(error) => return Err(unreadable(error)),
    };
    for entry in entries {
        let entry = entry.map_err(unreadable)?;
        if let Ok(target) = fs::read_link(entry.path())
            && let Some(kernel_name) = target.file_name()
        {
            links.insert(
                entry.file_name().to_string_lossy().into_owned(),
                format!("/dev/{}", kernel_name.to_string_lossy()),
            );
        }
    }
    Ok(links)
}
