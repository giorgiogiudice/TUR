// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The few file-system operations the store needs, behind a trait.
//!
//! [`StdFs`] is the real file system. [`MemFs`] keeps files in memory and can
//! be told to fail at a chosen operation, part-way through a write, which is
//! how the crash-safety tests simulate losing power at every point of an
//! update.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// The largest file the store reads, in bytes. A larger file is reported as
/// damaged rather than loaded.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The file-system operations the store uses.
///
/// Every method that changes something is durable when it returns, except
/// [`FileSystem::rename`] and [`FileSystem::remove_file`], which become
/// durable with the next [`FileSystem::sync_dir`] on the directory.
pub trait FileSystem {
    /// Create a directory and its parents, readable by the owner only.
    fn create_dir_all(&self, dir: &Path) -> io::Result<()>;

    /// The names of the regular files directly inside `dir`, sorted. A
    /// directory that does not exist has no files. Symbolic links are not
    /// followed and not listed.
    fn list_files(&self, dir: &Path) -> io::Result<Vec<String>>;

    /// The contents of a file, or `None` if it does not exist. At most
    /// [`MAX_FILE_BYTES`] plus one byte are returned, so the caller can tell
    /// that a file is too large without reading all of it.
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>>;

    /// Create or replace a file with these bytes, readable by the owner only,
    /// and flush it to the device.
    fn write_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;

    /// Rename a file, replacing the destination atomically.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Remove a file. Returns whether it existed.
    fn remove_file(&self, path: &Path) -> io::Result<bool>;

    /// Flush a directory, making renames and removals inside it durable.
    fn sync_dir(&self, dir: &Path) -> io::Result<()>;
}

impl<T: FileSystem + ?Sized> FileSystem for &T {
    fn create_dir_all(&self, dir: &Path) -> io::Result<()> {
        (**self).create_dir_all(dir)
    }

    fn list_files(&self, dir: &Path) -> io::Result<Vec<String>> {
        (**self).list_files(dir)
    }

    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        (**self).read(path)
    }

    fn write_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        (**self).write_synced(path, bytes)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        (**self).rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<bool> {
        (**self).remove_file(path)
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        (**self).sync_dir(dir)
    }
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdFs;

impl FileSystem for StdFs {
    fn create_dir_all(&self, dir: &Path) -> io::Result<()> {
        DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }

    fn list_files(&self, dir: &Path) -> io::Result<Vec<String>> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry?;
            // `file_type` does not follow a symbolic link.
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            if let Some(name) = name.to_str() {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    fn write_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<bool> {
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        File::open(dir)?.sync_all()
    }
}

/// Where a [`MemFs`] is to fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashPoint {
    /// The number of the changing operation that fails, counting from 1 since
    /// the file system was created.
    pub at_operation: u64,
    /// If the failing operation is a write: how many bytes of it reach the
    /// file before the failure. Ignored for other operations, which fail
    /// before they change anything.
    pub bytes_written: usize,
}

#[derive(Debug, Clone, Default)]
struct MemFsState {
    files: BTreeMap<PathBuf, Vec<u8>>,
    dirs: BTreeSet<PathBuf>,
    operations: u64,
    crash: Option<CrashPoint>,
    crashed: bool,
}

/// An in-memory file system for tests, with fault injection.
///
/// It counts the operations that change something. When the count reaches the
/// configured [`CrashPoint`] the operation fails, and so does every operation
/// after it, reads included, until [`MemFs::restart`]: the process is dead.
/// What the files hold at that moment is what a restarted process finds.
///
/// A write that is cut short leaves the bytes that were written, so a
/// temporary file can be found torn. A rename either happened or did not.
#[derive(Debug, Clone, Default)]
pub struct MemFs {
    state: RefCell<MemFsState>,
}

impl MemFs {
    /// An empty file system.
    pub fn new() -> Self {
        MemFs::default()
    }

    /// Fail at this point.
    pub fn crash_at(&self, point: CrashPoint) {
        self.state.borrow_mut().crash = Some(point);
    }

    /// Clear the failure, as a restarted process would find the file system.
    pub fn restart(&self) {
        let mut state = self.state.borrow_mut();
        state.crash = None;
        state.crashed = false;
    }

    /// Whether the configured failure has happened.
    pub fn has_crashed(&self) -> bool {
        self.state.borrow().crashed
    }

    /// How many changing operations have been attempted.
    pub fn operations(&self) -> u64 {
        self.state.borrow().operations
    }

    /// Every file and its contents.
    pub fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        self.state.borrow().files.clone()
    }

    /// The contents of one file.
    pub fn file(&self, path: &Path) -> Option<Vec<u8>> {
        self.state.borrow().files.get(path).cloned()
    }

    /// Put a file in place directly, without counting an operation. Used to
    /// plant damaged files.
    pub fn set_file(&self, path: &Path, bytes: &[u8]) {
        let mut state = self.state.borrow_mut();
        if let Some(parent) = path.parent() {
            add_dir(&mut state.dirs, parent);
        }
        state.files.insert(path.to_path_buf(), bytes.to_vec());
    }

    // Count one changing operation. Returns the number of bytes a write may
    // put down before failing, or `None` if the operation proceeds in full.
    fn step(&self) -> io::Result<Option<usize>> {
        let mut state = self.state.borrow_mut();
        if state.crashed {
            return Err(crashed());
        }
        state.operations += 1;
        if let Some(point) = state.crash
            && state.operations == point.at_operation
        {
            state.crashed = true;
            return Ok(Some(point.bytes_written));
        }
        Ok(None)
    }

    fn check_alive(&self) -> io::Result<()> {
        if self.state.borrow().crashed {
            Err(crashed())
        } else {
            Ok(())
        }
    }
}

fn crashed() -> io::Error {
    io::Error::other("simulated crash")
}

fn add_dir(dirs: &mut BTreeSet<PathBuf>, dir: &Path) {
    for ancestor in dir.ancestors() {
        dirs.insert(ancestor.to_path_buf());
    }
}

impl FileSystem for MemFs {
    fn create_dir_all(&self, dir: &Path) -> io::Result<()> {
        if self.step()?.is_some() {
            return Err(crashed());
        }
        add_dir(&mut self.state.borrow_mut().dirs, dir);
        Ok(())
    }

    fn list_files(&self, dir: &Path) -> io::Result<Vec<String>> {
        self.check_alive()?;
        let state = self.state.borrow();
        let mut names = Vec::new();
        for path in state.files.keys() {
            if path.parent() == Some(dir)
                && let Some(name) = path.file_name().and_then(|name| name.to_str())
            {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        self.check_alive()?;
        Ok(self.state.borrow().files.get(path).cloned())
    }

    fn write_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let cut = self.step()?;
        let mut state = self.state.borrow_mut();
        let parent_exists = path
            .parent()
            .is_some_and(|parent| state.dirs.contains(parent));
        if !parent_exists {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the directory does not exist",
            ));
        }
        match cut {
            Some(written) => {
                let kept = written.min(bytes.len());
                state.files.insert(path.to_path_buf(), bytes[..kept].to_vec());
                Err(crashed())
            }
            None => {
                state.files.insert(path.to_path_buf(), bytes.to_vec());
                Ok(())
            }
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if self.step()?.is_some() {
            return Err(crashed());
        }
        let mut state = self.state.borrow_mut();
        match state.files.remove(from) {
            Some(bytes) => {
                state.files.insert(to.to_path_buf(), bytes);
                Ok(())
            }
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the file to rename does not exist",
            )),
        }
    }

    fn remove_file(&self, path: &Path) -> io::Result<bool> {
        if self.step()?.is_some() {
            return Err(crashed());
        }
        Ok(self.state.borrow_mut().files.remove(path).is_some())
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        if self.step()?.is_some() {
            return Err(crashed());
        }
        if self.state.borrow().dirs.contains(dir) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the directory does not exist",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_fs_behaves_like_a_file_system() {
        let fs = MemFs::new();
        let dir = Path::new("/store/memory");
        let file = dir.join("a.json");
        assert_eq!(fs.list_files(dir).expect("lists"), Vec::<String>::new());
        assert!(fs.write_synced(&file, b"x").is_err(), "no directory yet");
        fs.create_dir_all(dir).expect("creates");
        fs.write_synced(&file, b"hello").expect("writes");
        assert_eq!(fs.read(&file).expect("reads"), Some(b"hello".to_vec()));
        assert_eq!(fs.list_files(dir).expect("lists"), ["a.json"]);
        // Files in a subdirectory are not listed with their parent's files.
        assert_eq!(
            fs.list_files(Path::new("/store")).expect("lists"),
            Vec::<String>::new()
        );
        fs.rename(&file, &dir.join("b.json")).expect("renames");
        assert_eq!(fs.read(&file).expect("reads"), None);
        assert!(fs.remove_file(&dir.join("b.json")).expect("removes"));
        assert!(!fs.remove_file(&dir.join("b.json")).expect("removes"));
        fs.sync_dir(dir).expect("syncs");
    }

    #[test]
    fn a_crash_tears_the_write_and_stops_everything_after_it() {
        let fs = MemFs::new();
        let dir = Path::new("/store");
        let file = dir.join("a.json");
        fs.create_dir_all(dir).expect("creates");
        fs.crash_at(CrashPoint {
            at_operation: 2,
            bytes_written: 3,
        });
        assert!(fs.write_synced(&file, b"hello").is_err());
        assert!(fs.has_crashed());
        assert!(fs.read(&file).is_err(), "the process is dead");
        assert!(fs.rename(&file, &dir.join("b.json")).is_err());
        fs.restart();
        assert_eq!(fs.read(&file).expect("reads"), Some(b"hel".to_vec()));
        assert_eq!(fs.operations(), 2);
    }
}
