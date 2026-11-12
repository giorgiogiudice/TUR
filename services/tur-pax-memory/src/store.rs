// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Where records are kept: one small file per record.
//!
//! ```text
//! <store>/memory/state.json              memory switch and entry counter
//! <store>/memory/entries/m-<hex>.json    one memory entry each
//! <store>/schedules/state.json           schedule and run counters
//! <store>/schedules/definitions/s-<hex>.json   one schedule each, with its standing approvals
//! <store>/schedules/runs/r-<hex>.json    one run record each
//! ```
//!
//! # Crash safety
//!
//! A record is replaced by writing a temporary file next to it, flushing that
//! file, renaming it over the record and flushing the directory. Whatever
//! instant the machine stops at, a record file holds either the complete old
//! bytes or the complete new bytes: a rename replaces the name atomically, and
//! the temporary file, the only one that can be torn, is never read as a
//! record. One record per file means damage to one file cannot reach another.
//!
//! A temporary file left by an interrupted write is removed by
//! [`Store::sweep`], which every changing operation of the memory and schedule
//! layers calls first, so the text of an entry that was never completed does
//! not linger where the user cannot see it.
//!
//! # Limits
//!
//! There is no lock. Each file is replaced atomically, so two processes
//! writing at once cannot tear a record, but they can both read the same
//! counter and one write can replace the other. A single writer at a time is
//! assumed.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::fs::{FileSystem, MAX_FILE_BYTES};

/// A group of records of one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Collection {
    /// The memory switch and the entry counter: one record, `state`.
    MemoryState,
    /// Memory entries.
    MemoryEntries,
    /// The schedule and run counters: one record, `state`.
    ScheduleState,
    /// Schedules, each with its standing approvals.
    Schedules,
    /// Run records.
    Runs,
}

impl Collection {
    /// Every collection.
    pub const ALL: [Collection; 5] = [
        Collection::MemoryState,
        Collection::MemoryEntries,
        Collection::ScheduleState,
        Collection::Schedules,
        Collection::Runs,
    ];

    /// The directory of the collection, relative to the store.
    pub const fn dir(self) -> &'static str {
        match self {
            Collection::MemoryState => "memory",
            Collection::MemoryEntries => "memory/entries",
            Collection::ScheduleState => "schedules",
            Collection::Schedules => "schedules/definitions",
            Collection::Runs => "schedules/runs",
        }
    }
}

/// The identifier of the single record in a state collection.
pub const STATE_ID: &str = "state";

/// A failure of the store itself: the file system refused, or an identifier
/// cannot be a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError {
    /// What went wrong. Never contains record contents.
    pub detail: String,
}

impl StoreError {
    fn new(detail: impl Into<String>) -> Self {
        StoreError {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for StoreError {}

/// Whether `id` may name a record: 1 to 64 characters, starting with a
/// lower-case letter, then lower-case letters, digits and hyphens.
///
/// Nothing in that set can name another directory or a hidden file.
pub fn is_valid_record_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn check_id(id: &str) -> Result<(), StoreError> {
    if is_valid_record_id(id) {
        Ok(())
    } else {
        Err(StoreError::new("the record identifier is not valid"))
    }
}

/// A place that keeps records as bytes, by collection and identifier.
///
/// The store does not interpret the bytes. The memory and schedule layers
/// parse and check them.
pub trait Store {
    /// Create or replace a record. On return the record is durable; if the
    /// call fails or the machine stops, the record is either as it was before
    /// or complete, never partial.
    fn put(&mut self, collection: Collection, id: &str, bytes: &[u8]) -> Result<(), StoreError>;

    /// The bytes of a record, or `None` if there is none.
    fn get(&self, collection: Collection, id: &str) -> Result<Option<Vec<u8>>, StoreError>;

    /// Remove a record and anything left of an interrupted write of it.
    /// Returns whether the record existed.
    fn remove(&mut self, collection: Collection, id: &str) -> Result<bool, StoreError>;

    /// The identifiers of the records in a collection, sorted.
    fn ids(&self, collection: Collection) -> Result<Vec<String>, StoreError>;

    /// Remove what interrupted writes left behind in a collection. Returns how
    /// many leftovers were removed.
    fn sweep(&mut self, collection: Collection) -> Result<usize, StoreError>;
}

/// A store that keeps everything in memory. Nothing survives the process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryStore {
    records: BTreeMap<(Collection, String), Vec<u8>>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        MemoryStore::default()
    }
}

impl Store for MemoryStore {
    fn put(&mut self, collection: Collection, id: &str, bytes: &[u8]) -> Result<(), StoreError> {
        check_id(id)?;
        self.records
            .insert((collection, id.to_owned()), bytes.to_vec());
        Ok(())
    }

    fn get(&self, collection: Collection, id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        check_id(id)?;
        Ok(self.records.get(&(collection, id.to_owned())).cloned())
    }

    fn remove(&mut self, collection: Collection, id: &str) -> Result<bool, StoreError> {
        check_id(id)?;
        Ok(self.records.remove(&(collection, id.to_owned())).is_some())
    }

    fn ids(&self, collection: Collection) -> Result<Vec<String>, StoreError> {
        Ok(self
            .records
            .keys()
            .filter(|(owner, _)| *owner == collection)
            .map(|(_, id)| id.clone())
            .collect())
    }

    fn sweep(&mut self, _collection: Collection) -> Result<usize, StoreError> {
        Ok(0)
    }
}

const RECORD_SUFFIX: &str = ".json";
const TEMPORARY_SUFFIX: &str = ".json.tmp";

/// A store that keeps each record in its own file under one directory.
#[derive(Debug, Clone)]
pub struct FileStore<F: FileSystem> {
    root: PathBuf,
    fs: F,
}

impl<F: FileSystem> FileStore<F> {
    /// A store rooted at `root`. Nothing is created until something is
    /// written.
    pub fn new(root: impl Into<PathBuf>, fs: F) -> Self {
        FileStore {
            root: root.into(),
            fs,
        }
    }

    /// The store directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory of a collection.
    pub fn collection_dir(&self, collection: Collection) -> PathBuf {
        self.root.join(collection.dir())
    }

    /// The file that holds a record.
    pub fn record_path(&self, collection: Collection, id: &str) -> PathBuf {
        self.collection_dir(collection)
            .join(format!("{id}{RECORD_SUFFIX}"))
    }

    /// The temporary file used while a record is being replaced. Its name
    /// starts with a dot and does not end in `.json`, so it is never listed
    /// as a record.
    pub fn temporary_path(&self, collection: Collection, id: &str) -> PathBuf {
        self.collection_dir(collection)
            .join(format!(".{id}{TEMPORARY_SUFFIX}"))
    }

    fn failed(&self, action: &str, err: &std::io::Error) -> StoreError {
        StoreError::new(format!(
            "could not {action} under {}: {err}",
            self.root.display()
        ))
    }
}

impl<F: FileSystem> Store for FileStore<F> {
    fn put(&mut self, collection: Collection, id: &str, bytes: &[u8]) -> Result<(), StoreError> {
        check_id(id)?;
        let dir = self.collection_dir(collection);
        let temporary = self.temporary_path(collection, id);
        let record = self.record_path(collection, id);
        self.fs
            .create_dir_all(&dir)
            .map_err(|err| self.failed("create the directory", &err))?;
        self.fs
            .write_synced(&temporary, bytes)
            .map_err(|err| self.failed("write a temporary file", &err))?;
        self.fs
            .rename(&temporary, &record)
            .map_err(|err| self.failed("rename a record into place", &err))?;
        self.fs
            .sync_dir(&dir)
            .map_err(|err| self.failed("flush the directory", &err))
    }

    fn get(&self, collection: Collection, id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        check_id(id)?;
        self.fs
            .read(&self.record_path(collection, id))
            .map_err(|err| self.failed("read a record", &err))
    }

    fn remove(&mut self, collection: Collection, id: &str) -> Result<bool, StoreError> {
        check_id(id)?;
        let dir = self.collection_dir(collection);
        // The temporary file goes first: if the machine stops in between, the
        // record is still listed and the removal is visibly unfinished.
        let had_temporary = self
            .fs
            .remove_file(&self.temporary_path(collection, id))
            .map_err(|err| self.failed("remove a temporary file", &err))?;
        let had_record = self
            .fs
            .remove_file(&self.record_path(collection, id))
            .map_err(|err| self.failed("remove a record", &err))?;
        if had_temporary || had_record {
            self.fs
                .sync_dir(&dir)
                .map_err(|err| self.failed("flush the directory", &err))?;
        }
        Ok(had_record)
    }

    fn ids(&self, collection: Collection) -> Result<Vec<String>, StoreError> {
        let names = self
            .fs
            .list_files(&self.collection_dir(collection))
            .map_err(|err| self.failed("list a directory", &err))?;
        let mut ids: Vec<String> = names
            .iter()
            .filter(|name| !name.starts_with('.'))
            .filter_map(|name| name.strip_suffix(RECORD_SUFFIX))
            .filter(|id| is_valid_record_id(id))
            .map(str::to_owned)
            .collect();
        ids.sort();
        Ok(ids)
    }

    fn sweep(&mut self, collection: Collection) -> Result<usize, StoreError> {
        let dir = self.collection_dir(collection);
        let names = self
            .fs
            .list_files(&dir)
            .map_err(|err| self.failed("list a directory", &err))?;
        let mut removed = 0;
        for name in names {
            if name.starts_with('.') && name.ends_with(TEMPORARY_SUFFIX) {
                let existed = self
                    .fs
                    .remove_file(&dir.join(&name))
                    .map_err(|err| self.failed("remove a temporary file", &err))?;
                if existed {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.fs
                .sync_dir(&dir)
                .map_err(|err| self.failed("flush the directory", &err))?;
        }
        Ok(removed)
    }
}

/// Whether record bytes are too large to be a record of this crate.
pub(crate) fn is_oversized(bytes: &[u8]) -> bool {
    u64::try_from(bytes.len()).is_ok_and(|length| length > MAX_FILE_BYTES)
}

/// The default store directory for a user: `$XDG_DATA_HOME/tur/pax`, or
/// `$HOME/.local/share/tur/pax` when `XDG_DATA_HOME` is unset, empty or not an
/// absolute path, as the XDG Base Directory specification requires.
///
/// The two values are passed in, so the function reads no environment.
pub fn default_store_dir(xdg_data_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let base = match xdg_data_home {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        _ => {
            let home = home.filter(|dir| dir.is_absolute())?;
            home.join(".local").join("share")
        }
    };
    Some(base.join("tur").join("pax"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemFs;

    #[test]
    fn record_identifiers_cannot_escape_the_directory() {
        for id in ["state", "m-0000002a", "a", "s-1"] {
            assert!(is_valid_record_id(id), "{id}");
        }
        let long = "a".repeat(65);
        for id in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            "a.json",
            ".hidden",
            "-a",
            "1a",
            "A",
            "a b",
            "a\0",
            "é",
            long.as_str(),
        ] {
            assert!(!is_valid_record_id(id), "{id:?}");
        }
    }

    fn exercise(store: &mut dyn Store) {
        let c = Collection::MemoryEntries;
        assert_eq!(store.ids(c).expect("lists"), Vec::<String>::new());
        assert_eq!(store.get(c, "m-1").expect("reads"), None);
        store.put(c, "m-2", b"two").expect("writes");
        store.put(c, "m-1", b"one").expect("writes");
        store.put(Collection::Runs, "r-1", b"run").expect("writes");
        assert_eq!(store.ids(c).expect("lists"), ["m-1", "m-2"]);
        assert_eq!(store.get(c, "m-1").expect("reads"), Some(b"one".to_vec()));
        store.put(c, "m-1", b"uno").expect("replaces");
        assert_eq!(store.get(c, "m-1").expect("reads"), Some(b"uno".to_vec()));
        assert!(store.remove(c, "m-1").expect("removes"));
        assert!(!store.remove(c, "m-1").expect("removes"));
        assert_eq!(store.ids(c).expect("lists"), ["m-2"]);
        assert_eq!(store.ids(Collection::Runs).expect("lists"), ["r-1"]);
        assert!(store.put(c, "../x", b"no").is_err());
        assert!(store.get(c, "a/b").is_err());
        assert!(store.remove(c, "..").is_err());
    }

    #[test]
    fn both_stores_behave_the_same() {
        exercise(&mut MemoryStore::new());
        let fs = MemFs::new();
        exercise(&mut FileStore::new("/store", &fs));
        // Nothing but record files is left behind.
        let files: Vec<String> = fs
            .files()
            .keys()
            .map(|path| path.display().to_string())
            .collect();
        assert_eq!(
            files,
            [
                "/store/memory/entries/m-2.json",
                "/store/schedules/runs/r-1.json"
            ]
        );
    }

    #[test]
    fn the_file_store_ignores_what_is_not_a_record() {
        let fs = MemFs::new();
        let mut store = FileStore::new("/store", &fs);
        store
            .put(Collection::MemoryEntries, "m-1", b"one")
            .expect("writes");
        let dir = store.collection_dir(Collection::MemoryEntries);
        fs.set_file(&dir.join(".m-9.json.tmp"), b"torn");
        fs.set_file(&dir.join("notes.txt"), b"other");
        fs.set_file(&dir.join("UPPER.json"), b"other");
        assert_eq!(
            store.ids(Collection::MemoryEntries).expect("lists"),
            ["m-1"]
        );
        // Sweeping removes the leftover of the interrupted write and nothing else.
        assert_eq!(store.sweep(Collection::MemoryEntries).expect("sweeps"), 1);
        assert_eq!(fs.file(&dir.join(".m-9.json.tmp")), None);
        assert!(fs.file(&dir.join("notes.txt")).is_some());
        assert_eq!(store.sweep(Collection::MemoryEntries).expect("sweeps"), 0);
    }

    #[test]
    fn default_directory_follows_xdg() {
        let xdg = Path::new("/data");
        let home = Path::new("/home/u");
        assert_eq!(
            default_store_dir(Some(xdg), Some(home)),
            Some(PathBuf::from("/data/tur/pax"))
        );
        assert_eq!(
            default_store_dir(None, Some(home)),
            Some(PathBuf::from("/home/u/.local/share/tur/pax"))
        );
        // A relative XDG_DATA_HOME is ignored.
        assert_eq!(
            default_store_dir(Some(Path::new("data")), Some(home)),
            Some(PathBuf::from("/home/u/.local/share/tur/pax"))
        );
        assert_eq!(default_store_dir(None, None), None);
        assert_eq!(default_store_dir(None, Some(Path::new("home"))), None);
    }
}
