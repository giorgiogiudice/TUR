// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Test support shared by the unit tests: fixtures, a deterministic clock
//! and an in-memory file system that can fail on demand.
//!
//! The fixtures are hand-written to the shape of `lsblk --json --bytes
//! --output-all`. They were not captured from a real machine.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::inventory::{DiskLinks, Inventory};
use crate::plan::{InstallMode, LayoutOptions, PlanRequest};
use crate::preflight::{BootMode, EnvironmentFacts, REQUIRED_CPU_FEATURES, SecureBootState};
use crate::session::FileSystem;
use crate::time::{Clock, Timestamp};
use crate::units::GIB;

/// A workstation: an NVMe disk with Windows, a SATA SSD with Linux, the USB
/// stick the live environment runs from, an empty card reader, the live
/// image's loop device and a compressed RAM swap device.
pub(crate) const WORKSTATION_LSBLK: &str = include_str!("../tests/fixtures/lsblk-workstation.json");
pub(crate) const WORKSTATION_LINKS: &str = include_str!("../tests/fixtures/links-workstation.json");

/// Awkward hardware: a disk with no serial number, two enclosures with the
/// same serial number, two paths to one multipath disk, and an opened
/// LVM-on-LUKS layout.
pub(crate) const ODDITIES_LSBLK: &str = include_str!("../tests/fixtures/lsblk-oddities.json");
pub(crate) const ODDITIES_LINKS: &str = include_str!("../tests/fixtures/links-oddities.json");

/// Selects the workstation's NVMe disk by its stable link.
pub(crate) const NVME_SELECTOR: &str =
    "/dev/disk/by-id/nvme-Samsung_SSD_980_PRO_1TB_S5GXNX0T123456A";
/// Selects the workstation's SATA SSD by its serial number.
pub(crate) const SATA_SELECTOR: &str = "serial=S4EWNX0M700001B";

pub(crate) fn links(text: &str) -> DiskLinks {
    serde_json::from_str(text).expect("the links fixture parses")
}

pub(crate) fn workstation() -> Inventory {
    Inventory::from_lsblk_json(WORKSTATION_LSBLK, &links(WORKSTATION_LINKS))
        .expect("the workstation fixture parses")
}

pub(crate) fn oddities() -> Inventory {
    Inventory::from_lsblk_json(ODDITIES_LSBLK, &links(ODDITIES_LINKS))
        .expect("the oddities fixture parses")
}

/// Facts of a machine that passes every environment check.
pub(crate) fn good_environment() -> EnvironmentFacts {
    EnvironmentFacts {
        boot_mode: Some(BootMode::Uefi),
        secure_boot: Some(SecureBootState::Enabled),
        cpu_architecture: Some("x86_64".to_owned()),
        cpu_features: Some(
            REQUIRED_CPU_FEATURES
                .iter()
                .map(|feature| (*feature).to_owned())
                .chain(["avx2".to_owned()])
                .collect(),
        ),
        ram_bytes: Some(16 * GIB),
        safe_graphics: Some(false),
        network_available: Some(true),
    }
}

pub(crate) fn erase_request(options: LayoutOptions) -> PlanRequest {
    PlanRequest {
        mode: InstallMode::EraseDisk,
        options,
    }
}

/// A clock that starts at a fixed instant and advances by a fixed step each
/// time it is read.
pub(crate) struct ManualClock {
    now: Cell<u64>,
}

impl ManualClock {
    const START_MILLIS: u64 = 1_794_464_220_000;
    const STEP_MILLIS: u64 = 250;

    pub(crate) fn new() -> ManualClock {
        ManualClock {
            now: Cell::new(Self::START_MILLIS),
        }
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        let now = self.now.get();
        self.now.set(now + Self::STEP_MILLIS);
        Timestamp::from_unix_millis(now)
    }
}

#[derive(Default)]
struct MemState {
    files: BTreeMap<PathBuf, Vec<u8>>,
    directories: BTreeSet<PathBuf>,
    operations: Vec<String>,
    fail_from: Option<usize>,
    torn_append: Option<usize>,
}

/// An in-memory file system.
///
/// Every operation that changes something is numbered and logged. After
/// [`MemFs::fail_from`], the operation with that number and all later ones
/// fail without effect, which models a crash at that point. The one
/// exception models a torn append: if the first failing operation is an
/// append, a prefix of its data can be kept.
#[derive(Default)]
pub(crate) struct MemFs {
    state: RefCell<MemState>,
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{} does not exist", path.display()),
    )
}

impl MemFs {
    pub(crate) fn new() -> MemFs {
        MemFs::default()
    }

    /// Makes operation number `operation` (counting from zero) and all later
    /// ones fail. If that operation is an append, the first `torn_append`
    /// bytes of its data are written before it fails.
    pub(crate) fn fail_from(&self, operation: usize, torn_append: Option<usize>) {
        let mut state = self.state.borrow_mut();
        state.fail_from = Some(operation);
        state.torn_append = torn_append;
    }

    /// The log of operations that changed, or tried to change, something.
    pub(crate) fn operations(&self) -> Vec<String> {
        self.state.borrow().operations.clone()
    }

    pub(crate) fn file(&self, path: &Path) -> Option<Vec<u8>> {
        self.state.borrow().files.get(path).cloned()
    }

    /// Sets a file's contents directly, bypassing the operation log.
    pub(crate) fn set_file(&self, path: &Path, data: &[u8]) {
        self.state
            .borrow_mut()
            .files
            .insert(path.to_path_buf(), data.to_vec());
    }

    /// What a machine would find after the crash: the same files and
    /// directories, with no failure pending.
    pub(crate) fn survivors(&self) -> MemFs {
        let state = self.state.borrow();
        MemFs {
            state: RefCell::new(MemState {
                files: state.files.clone(),
                directories: state.directories.clone(),
                ..MemState::default()
            }),
        }
    }

    /// Logs an operation and says whether it may take effect.
    fn admit(&self, description: String) -> io::Result<()> {
        let mut state = self.state.borrow_mut();
        let number = state.operations.len();
        state.operations.push(description);
        match state.fail_from {
            Some(limit) if number >= limit => Err(io::Error::other("injected crash")),
            _ => Ok(()),
        }
    }
}

impl FileSystem for MemFs {
    fn create_dir_all(&self, directory: &Path) -> io::Result<()> {
        self.admit(format!("create_dir_all {}", name_of(directory)))?;
        self.state
            .borrow_mut()
            .directories
            .insert(directory.to_path_buf());
        Ok(())
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.file(path).ok_or_else(|| not_found(path))
    }

    fn write_and_sync(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.admit(format!("write_and_sync {}", name_of(path)))?;
        self.set_file(path, data);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.admit(format!("rename {} -> {}", name_of(from), name_of(to)))?;
        let mut state = self.state.borrow_mut();
        let data = state.files.remove(from).ok_or_else(|| not_found(from))?;
        state.files.insert(to.to_path_buf(), data);
        Ok(())
    }

    fn sync_dir(&self, directory: &Path) -> io::Result<()> {
        self.admit(format!("sync_dir {}", name_of(directory)))
    }

    fn append_and_sync(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let number = self.state.borrow().operations.len();
        let admitted = self.admit(format!("append {}", name_of(path)));
        let mut state = self.state.borrow_mut();
        let kept = match &admitted {
            Ok(()) => data.len(),
            Err(_) if state.fail_from == Some(number) => {
                state.torn_append.unwrap_or(0).min(data.len())
            }
            Err(_) => 0,
        };
        if kept > 0 {
            state
                .files
                .entry(path.to_path_buf())
                .or_default()
                .extend_from_slice(&data[..kept]);
        }
        admitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_advances_by_a_fixed_step() {
        let clock = ManualClock::new();
        let first = clock.now();
        let second = clock.now();
        assert_eq!(first.to_rfc3339(), "2026-11-12T06:17:00.000Z");
        assert_eq!(second.millis_since(first), 250);
    }

    #[test]
    fn the_memory_file_system_fails_from_the_chosen_operation() {
        let file_system = MemFs::new();
        let directory = Path::new("/session");
        let log = directory.join("log");
        file_system.create_dir_all(directory).unwrap();
        file_system.append_and_sync(&log, b"one\n").unwrap();
        file_system.fail_from(3, Some(2));
        file_system.append_and_sync(&log, b"two\n").unwrap();
        // Operation 3 is torn: two of its bytes land.
        assert!(file_system.append_and_sync(&log, b"three\n").is_err());
        // Later operations fail without effect.
        assert!(file_system.append_and_sync(&log, b"four\n").is_err());
        assert!(
            file_system
                .write_and_sync(&directory.join("x"), b"x")
                .is_err()
        );
        assert!(file_system.sync_dir(directory).is_err());
        assert!(file_system.rename(&log, &directory.join("y")).is_err());
        assert_eq!(file_system.file(&log).unwrap(), b"one\ntwo\nth");
        assert_eq!(file_system.operations().len(), 8);

        let survivors = file_system.survivors();
        assert_eq!(survivors.read(&log).unwrap(), b"one\ntwo\nth");
        assert!(survivors.operations().is_empty());
        survivors.append_and_sync(&log, b"!").unwrap();
        assert_eq!(
            survivors.read(&directory.join("x")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
