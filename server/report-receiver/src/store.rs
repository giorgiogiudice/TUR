// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The data directory.
//!
//! ```text
//! <data_dir>/
//!   lock                       held while anything is read or changed
//!   reports/<id>.json          a report's record: identifiers, signature,
//!                              state, times; nothing personal
//!   tokens/<sha256 of token>   the report id the status token belongs to
//!   bundles/<id>.bin           the raw bundle, sealed (crypto.rs)
//!   summaries/<id>.bin         the summary waiting to be filed, sealed
//!   signatures.json            signature -> issue number
//!   tmp/                       files being written
//! ```
//!
//! Every write goes to `tmp/`, is synced, and is renamed into place, so a
//! crash leaves either the old or the new file. The status token itself is
//! never stored: only its SHA-256. Every operation takes the lock, so the
//! running service and an administrator's command (`sweep`, `mark-fixed`,
//! `decrypt`) never interleave.

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::crypto::{hex, random_bytes};

/// A report's state, as the person sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Stored; the summary is not filed yet.
    Received,
    /// The summary is in the tracker.
    Filed,
    /// A maintainer marked it fixed.
    Fixed,
}

impl State {
    /// The state as the status answer gives it.
    pub const fn as_str(self) -> &'static str {
        match self {
            State::Received => "received",
            State::Filed => "filed",
            State::Fixed => "fixed",
        }
    }
}

/// How a report's summary was filed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filing {
    /// Not yet.
    Pending,
    /// As a new issue.
    Issue,
    /// As a comment on the issue of its signature.
    Comment,
}

/// What the receiver keeps about a report. Nothing in it is personal: the
/// error code, component and version are checked against fixed forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Version of this format.
    pub schema_version: u32,
    /// The report's identifier.
    pub report_id: String,
    /// SHA-256 of the status token.
    pub token_sha256: String,
    /// When it was received.
    pub received_at: u64,
    /// When its bundle is deleted.
    pub bundle_expires_at: u64,
    /// When this record is deleted.
    pub record_expires_at: u64,
    /// When its bundle was deleted, once it has been.
    pub bundle_deleted_at: Option<u64>,
    /// The key its bundle and summary are sealed with.
    pub key_id: String,
    /// Its duplicate signature.
    pub signature: String,
    /// Its error code.
    pub error_code: Option<String>,
    /// Its component.
    pub component: String,
    /// The version it is about.
    pub version: String,
    /// Its state.
    pub state: State,
    /// How it was filed.
    pub filing: Filing,
    /// The issue it was filed as, or commented on.
    pub issue: Option<u64>,
    /// Failed filing attempts so far.
    pub attempts: u32,
    /// When to try filing next.
    pub next_attempt_at: u64,
    /// Why the last attempt failed: a short code such as `http-503`.
    pub last_failure: Option<String>,
    /// The version that fixes it.
    pub fixed_in: Option<String>,
}

/// The data directory.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

/// Proof that the store's lock is held. Released when dropped.
#[derive(Debug)]
pub struct Locked {
    _file: File,
}

const DIRECTORIES: [&str; 5] = ["reports", "tokens", "bundles", "summaries", "tmp"];

/// Whether `id` is a report identifier: `R-` and 16 hexadecimal digits.
pub fn is_report_id(id: &str) -> bool {
    tur_diag::report::is_report_id(id)
}

/// Whether `text` is 64 lower-case hexadecimal digits.
pub fn is_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn other(detail: String) -> io::Error {
    io::Error::other(detail)
}

/// Writes `data` to the new file `temporary`, syncs it, renames it to
/// `target` and syncs `directory`.
fn write_then_rename(
    temporary: &Path,
    target: &Path,
    directory: &Path,
    data: &[u8],
) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temporary)?;
    file.write_all(data)?;
    file.sync_all()?;
    fs::rename(temporary, target)?;
    File::open(directory)?.sync_all()
}

impl Store {
    /// Opens the data directory, creating it and its subdirectories with
    /// mode 0700.
    pub fn open(root: &Path) -> io::Result<Store> {
        let mut builder = DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(root)?;
        for name in DIRECTORIES {
            builder.create(root.join(name))?;
        }
        Ok(Store {
            root: root.to_path_buf(),
        })
    }

    /// Takes the lock, waiting for it.
    pub fn lock(&self) -> io::Result<Locked> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(self.root.join("lock"))?;
        file.lock()?;
        Ok(Locked { _file: file })
    }

    fn path(&self, directory: &str, name: &str) -> PathBuf {
        self.root.join(directory).join(name)
    }

    /// Writes `data` to `directory/name` atomically, with mode 0600.
    fn write(&self, directory: &str, name: &str, data: &[u8]) -> io::Result<()> {
        let mut suffix = [0u8; 8];
        random_bytes(&mut suffix).map_err(other)?;
        let temporary = self.path("tmp", &format!("{name}.{}", hex(&suffix)));
        let result = write_then_rename(
            &temporary,
            &self.path(directory, name),
            &self.root.join(directory),
            data,
        );
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    fn read(&self, directory: &str, name: &str) -> io::Result<Option<Vec<u8>>> {
        match fs::read(self.path(directory, name)) {
            Ok(data) => Ok(Some(data)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn remove(&self, directory: &str, name: &str) -> io::Result<bool> {
        match fs::remove_file(self.path(directory, name)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn names(&self, directory: &str) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(self.root.join(directory))? {
            names.push(entry?.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }

    /// Stores a new report: its sealed bundle and summary, its token's
    /// hash, and its record last, so that a record never names files that
    /// are not there.
    pub fn put_report(
        &self,
        lock: &Locked,
        record: &Record,
        bundle: &[u8],
        summary: &[u8],
    ) -> io::Result<()> {
        let id = &record.report_id;
        if !is_report_id(id) || !is_hex64(&record.token_sha256) {
            return Err(other("a malformed report id or token hash".to_owned()));
        }
        self.write("bundles", &format!("{id}.bin"), bundle)?;
        self.write("summaries", &format!("{id}.bin"), summary)?;
        self.write("tokens", &record.token_sha256, id.as_bytes())?;
        self.write_record(lock, record)
    }

    /// Replaces a report's record.
    pub fn write_record(&self, _lock: &Locked, record: &Record) -> io::Result<()> {
        if !is_report_id(&record.report_id) {
            return Err(other("a malformed report id".to_owned()));
        }
        let mut data =
            serde_json::to_vec_pretty(record).map_err(|error| other(error.to_string()))?;
        data.push(b'\n');
        self.write("reports", &format!("{}.json", record.report_id), &data)
    }

    /// A report's record.
    pub fn record(&self, _lock: &Locked, id: &str) -> io::Result<Option<Record>> {
        if !is_report_id(id) {
            return Ok(None);
        }
        match self.read("reports", &format!("{id}.json"))? {
            Some(data) => serde_json::from_slice(&data)
                .map(Some)
                .map_err(|error| other(format!("record {id}: {error}"))),
            None => Ok(None),
        }
    }

    /// Every record, oldest first.
    pub fn records(&self, lock: &Locked) -> io::Result<Vec<Record>> {
        let mut records = Vec::new();
        for name in self.names("reports")? {
            if let Some(id) = name.strip_suffix(".json")
                && let Some(record) = self.record(lock, id)?
            {
                records.push(record);
            }
        }
        records.sort_by(|a, b| {
            a.received_at
                .cmp(&b.received_at)
                .then_with(|| a.report_id.cmp(&b.report_id))
        });
        Ok(records)
    }

    /// The report a status token's hash belongs to.
    pub fn report_for_token(
        &self,
        _lock: &Locked,
        token_sha256: &str,
    ) -> io::Result<Option<String>> {
        if !is_hex64(token_sha256) {
            return Ok(None);
        }
        Ok(self
            .read("tokens", token_sha256)?
            .and_then(|data| String::from_utf8(data).ok())
            .filter(|id| is_report_id(id)))
    }

    /// A report's sealed bundle.
    pub fn bundle(&self, _lock: &Locked, id: &str) -> io::Result<Option<Vec<u8>>> {
        if !is_report_id(id) {
            return Ok(None);
        }
        self.read("bundles", &format!("{id}.bin"))
    }

    /// A report's sealed summary.
    pub fn summary(&self, _lock: &Locked, id: &str) -> io::Result<Option<Vec<u8>>> {
        if !is_report_id(id) {
            return Ok(None);
        }
        self.read("summaries", &format!("{id}.bin"))
    }

    /// Deletes a report's bundle. Whether there was one.
    pub fn remove_bundle(&self, _lock: &Locked, id: &str) -> io::Result<bool> {
        self.remove("bundles", &format!("{id}.bin"))
    }

    /// Deletes a report's summary. Whether there was one.
    pub fn remove_summary(&self, _lock: &Locked, id: &str) -> io::Result<bool> {
        self.remove("summaries", &format!("{id}.bin"))
    }

    /// Deletes everything about a report.
    pub fn remove_report(&self, lock: &Locked, record: &Record) -> io::Result<()> {
        self.remove_bundle(lock, &record.report_id)?;
        self.remove_summary(lock, &record.report_id)?;
        self.remove("tokens", &record.token_sha256)?;
        self.remove("reports", &format!("{}.json", record.report_id))?;
        Ok(())
    }

    /// Files that belong to no record: left by a crash between writes.
    /// Returns how many were removed.
    pub fn remove_orphans(&self, lock: &Locked) -> io::Result<usize> {
        let known: Vec<String> = self
            .records(lock)?
            .into_iter()
            .map(|record| record.report_id)
            .collect();
        let mut removed = 0;
        for directory in ["bundles", "summaries"] {
            for name in self.names(directory)? {
                let id = name.strip_suffix(".bin").unwrap_or(&name);
                if !known.iter().any(|known| known == id) && self.remove(directory, &name)? {
                    removed += 1;
                }
            }
        }
        for name in self.names("tokens")? {
            let owner = self.report_for_token(lock, &name)?;
            if !owner.is_some_and(|id| known.contains(&id)) && self.remove("tokens", &name)? {
                removed += 1;
            }
        }
        for name in self.names("tmp")? {
            if self.remove("tmp", &name)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// The issue each signature was filed as.
    pub fn signatures(&self, _lock: &Locked) -> io::Result<BTreeMap<String, u64>> {
        match fs::read(self.root.join("signatures.json")) {
            Ok(data) => serde_json::from_slice(&data).map_err(|error| other(error.to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(error) => Err(error),
        }
    }

    /// Replaces the map of signatures to issues.
    pub fn write_signatures(&self, _lock: &Locked, map: &BTreeMap<String, u64>) -> io::Result<()> {
        let mut data = serde_json::to_vec_pretty(map).map_err(|error| other(error.to_string()))?;
        data.push(b'\n');
        let mut suffix = [0u8; 8];
        random_bytes(&mut suffix).map_err(other)?;
        let temporary = self.path("tmp", &format!("signatures.{}", hex(&suffix)));
        let result = write_then_rename(
            &temporary,
            &self.root.join("signatures.json"),
            &self.root,
            &data,
        );
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    /// The names of the files in one of the store's directories, for tests
    /// and for the administrator.
    pub fn list(&self, directory: &str) -> io::Result<Vec<String>> {
        if !DIRECTORIES.contains(&directory) {
            return Err(other(format!("{directory} is not a store directory")));
        }
        self.names(directory)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn record(id: &str, received_at: u64) -> Record {
        Record {
            schema_version: 1,
            report_id: id.to_owned(),
            token_sha256: "a".repeat(64),
            received_at,
            bundle_expires_at: received_at + 90 * 86_400,
            record_expires_at: received_at + 365 * 86_400,
            bundle_deleted_at: None,
            key_id: "k1".to_owned(),
            signature: "0011223344556677".to_owned(),
            error_code: None,
            component: "audio".to_owned(),
            version: "0.2.0".to_owned(),
            state: State::Received,
            filing: Filing::Pending,
            issue: None,
            attempts: 0,
            next_attempt_at: received_at,
            last_failure: None,
            fixed_in: None,
        }
    }

    #[test]
    fn a_report_is_stored_privately_and_found_by_its_token_hash() {
        let root = std::env::temp_dir().join(format!("tur-receiver-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let store = Store::open(&root).expect("opens");
        let mode = fs::metadata(&root).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let lock = store.lock().expect("locks");
        let first = record("R-0123456789abcdef", 10);
        store
            .put_report(&lock, &first, b"sealed bundle", b"sealed summary")
            .expect("stored");
        let file = root.join("bundles/R-0123456789abcdef.bin");
        assert_eq!(
            fs::metadata(file).expect("metadata").permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            store
                .report_for_token(&lock, &"a".repeat(64))
                .expect("read")
                .as_deref(),
            Some("R-0123456789abcdef")
        );
        assert_eq!(
            store
                .report_for_token(&lock, "../../etc/passwd")
                .expect("read"),
            None
        );
        assert_eq!(store.record(&lock, "../reports/x").expect("read"), None);
        assert_eq!(store.records(&lock).expect("read"), vec![first.clone()]);
        assert!(store.list("tmp").expect("listed").is_empty());

        let mut map = BTreeMap::new();
        map.insert("0011223344556677".to_owned(), 7);
        store.write_signatures(&lock, &map).expect("written");
        assert_eq!(store.signatures(&lock).expect("read"), map);

        // A crash after the bundle was written but before the record.
        fs::write(root.join("bundles/R-fedcba9876543210.bin"), b"orphan").expect("written");
        assert_eq!(store.remove_orphans(&lock).expect("swept"), 1);
        assert!(
            store
                .bundle(&lock, "R-0123456789abcdef")
                .expect("read")
                .is_some()
        );

        store.remove_report(&lock, &first).expect("removed");
        assert!(store.records(&lock).expect("read").is_empty());
        assert!(store.list("bundles").expect("listed").is_empty());
        assert!(store.list("tokens").expect("listed").is_empty());
        drop(lock);
        let _ = fs::remove_dir_all(root);
    }
}
