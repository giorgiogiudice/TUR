// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Electron's asar archive: read it, add unpacked files, write it back.
//!
//! An archive starts with four little-endian `u32`s: 4, the header size,
//! the header's payload size and the JSON size. The JSON header follows at
//! byte 16, padded to four bytes; file data starts at `8 + header size`.
//! The header is `{"files": {name: node}}`: a directory node has `files`;
//! a file node has `size`, `offset` (a decimal string, relative to the
//! start of the data), and optionally `unpacked` (the file then lives in
//! `<archive>.unpacked/<path>`), `executable` and `integrity`; a link node
//! has `link`.
//!
//! The header is untrusted: it is at most [`HEADER_MAX`] bytes, a name
//! that is empty, `.`, `..` or holds `/`, `\` or NUL is never used, a
//! walk stops at [`MAX_ENTRIES`] entries and [`MAX_DEPTH`] levels, and
//! the unpacked directory is never left through a symbolic link.
//! Changing an archive keeps its data section byte for byte and rewrites
//! only the header (the prototype's `binfmt.Asar`).

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::sniff;

/// The largest JSON header read (64 MiB).
pub const HEADER_MAX: u64 = 64 << 20;
/// The most entries [`Asar::walk`] returns.
pub const MAX_ENTRIES: usize = 500_000;
/// The deepest directory level [`Asar::walk`] descends to.
pub const MAX_DEPTH: usize = 64;
/// The largest file [`Asar::read_json`] parses (4 MiB).
const JSON_MAX: u64 = 4 << 20;

/// Why an archive could not be opened.
#[derive(Debug)]
pub enum AsarError {
    /// Reading the file failed.
    Io(io::Error),
    /// The path is not a regular file (a symbolic link is not followed).
    NotAFile,
    /// The file ends before its prefix or its header does.
    Truncated,
    /// The prefix is not an asar prefix, or its sizes contradict each
    /// other or the file.
    BadPrefix,
    /// The JSON header is larger than [`HEADER_MAX`].
    HeaderTooLarge(u64),
    /// The JSON header is not valid JSON, or not an object.
    BadHeader(String),
}

impl fmt::Display for AsarError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsarError::Io(error) => write!(formatter, "the archive could not be read: {error}"),
            AsarError::NotAFile => formatter.write_str("not a regular file"),
            AsarError::Truncated => formatter.write_str("the archive is truncated"),
            AsarError::BadPrefix => formatter.write_str("not an asar archive"),
            AsarError::HeaderTooLarge(size) => {
                write!(formatter, "the archive's header is too large ({size} bytes)")
            }
            AsarError::BadHeader(detail) => {
                write!(formatter, "the archive's header is not valid: {detail}")
            }
        }
    }
}

impl std::error::Error for AsarError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AsarError::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for AsarError {
    fn from(error: io::Error) -> Self {
        AsarError::Io(error)
    }
}

/// One entry of an archive, as [`Asar::walk`] lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsarEntry {
    /// Its path inside the archive, `/`-separated.
    pub path: String,
    /// Its size in bytes (0 for a directory or a link).
    pub size: u64,
    /// Whether it lives in `<archive>.unpacked/`.
    pub unpacked: bool,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// Whether it is a symbolic link inside the archive.
    pub is_link: bool,
}

/// An opened asar archive.
#[derive(Debug, Clone)]
pub struct Asar {
    /// The archive file.
    pub path: PathBuf,
    /// The parsed JSON header.
    pub header: Value,
    /// Where file data starts in the archive: `8 + header size`.
    pub data_offset: u64,
    /// `<archive>.unpacked`, where unpacked files live.
    pub unpacked_dir: PathBuf,
}

/// Whether a name can be used as one component of a path inside an
/// archive: not empty, not `.` or `..`, and without `/`, `\` or NUL.
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

/// The components of a relative path inside an archive. The empty path is
/// the root (no components); an absolute path, or one with an unsafe
/// component (see [`is_safe_name`]), is refused. One trailing `/` is
/// allowed.
pub(crate) fn components(rel: &str) -> Option<Vec<&str>> {
    if rel.is_empty() {
        return Some(Vec::new());
    }
    if rel.starts_with('/') {
        return None;
    }
    let trimmed = rel.strip_suffix('/').unwrap_or(rel);
    trimmed
        .split('/')
        .map(|part| is_safe_name(part).then_some(part))
        .collect()
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
}

/// `base` joined with a relative path, when every directory on the way
/// (`base` included) is a real directory and not a symbolic link. The
/// last component may be anything, or nothing yet: a caller that reads it
/// refuses a link itself (see [`sniff::read_at`]).
pub(crate) fn resolve_below(base: &Path, rel: &str) -> Option<PathBuf> {
    let parts = components(rel)?;
    let (last, parents) = parts.split_last()?;
    if !is_real_dir(base) {
        return None;
    }
    let mut path = base.to_path_buf();
    for part in parents {
        path.push(part);
        if !is_real_dir(&path) {
            return None;
        }
    }
    path.push(last);
    Some(path)
}

/// Parses JSON after an optional UTF-8 byte-order mark.
pub(crate) fn parse_json(raw: &[u8]) -> Option<Value> {
    let body = raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw);
    serde_json::from_slice(body).ok()
}

fn invalid(detail: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, detail.to_owned())
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The prefix, the JSON and its padding: everything before the data.
fn header_bytes(json: &[u8]) -> io::Result<Vec<u8>> {
    let padded = json.len().next_multiple_of(4);
    let word = |value: usize| u32::try_from(value).map_err(|_| invalid("the header is too large"));
    let mut out = Vec::with_capacity(16 + padded);
    for value in [4, word(padded + 8)?, word(padded + 4)?, word(json.len())?] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(json);
    out.resize(16 + padded, 0);
    Ok(out)
}

/// The `files` map of a directory node, created when the node has none.
fn files_mut(node: &mut Value) -> io::Result<&mut Map<String, Value>> {
    let object = node
        .as_object_mut()
        .ok_or_else(|| invalid("the archive's header is malformed"))?;
    object
        .entry("files")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| invalid("the archive's header is malformed"))
}

/// Puts `leaf` at `parts` in a header, creating directory nodes on the
/// way. A file in the way of a directory, or a directory where the leaf
/// goes, is an error.
fn insert_node(header: &mut Value, parts: &[&str], leaf: Value) -> io::Result<()> {
    let (last, parents) = parts
        .split_last()
        .ok_or_else(|| invalid("an empty path"))?;
    let mut node = header;
    for part in parents {
        let files = files_mut(node)?;
        let child = files
            .entry((*part).to_owned())
            .or_insert_with(|| json!({ "files": {} }));
        if !child.get("files").is_some_and(Value::is_object) {
            return Err(invalid("a file is in the way of a directory"));
        }
        node = child;
    }
    let files = files_mut(node)?;
    if files.get(*last).is_some_and(|existing| existing.get("files").is_some()) {
        return Err(invalid("a directory is in the way"));
    }
    files.insert((*last).to_owned(), leaf);
    Ok(())
}

/// Whether [`insert_node`] would succeed, without changing anything.
fn check_insert(header: &Value, parts: &[&str]) -> io::Result<()> {
    let (last, parents) = parts
        .split_last()
        .ok_or_else(|| invalid("an empty path"))?;
    let mut node = header;
    for part in parents {
        match node.get("files").and_then(|files| files.get(*part)) {
            None => return Ok(()),
            Some(child) if child.get("files").is_some_and(Value::is_object) => node = child,
            Some(_) => return Err(invalid("a file is in the way of a directory")),
        }
    }
    let existing = node.get("files").and_then(|files| files.get(*last));
    if existing.is_some_and(|existing| existing.get("files").is_some()) {
        return Err(invalid("a directory is in the way"));
    }
    Ok(())
}

/// Makes sure `path` is a real directory, creating it when it is missing.
fn ensure_real_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(()),
        Ok(_) => Err(invalid("not a directory (or a symbolic link)")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(path),
        Err(error) => Err(error),
    }
}

impl Asar {
    /// Opens an archive: checks its prefix (4, then sizes that agree with
    /// each other and with the file) and parses its JSON header, which
    /// must be at most [`HEADER_MAX`] bytes.
    pub fn open(path: &Path) -> Result<Asar, AsarError> {
        let (mut file, size) = sniff::open_regular(path).ok_or(AsarError::NotAFile)?;
        let prefix = sniff::read_range(&mut file, size, 0, 16).ok_or(AsarError::Truncated)?;
        if prefix.len() < 16 {
            return Err(AsarError::Truncated);
        }
        let word = |at: usize| sniff::u32_le(&prefix, at).map_or(0, u64::from);
        let (four, header_size, json_size) = (word(0), word(4), word(12));
        if four != 4 {
            return Err(AsarError::BadPrefix);
        }
        if json_size > HEADER_MAX {
            return Err(AsarError::HeaderTooLarge(json_size));
        }
        if json_size > header_size || 8 + header_size > size {
            return Err(AsarError::BadPrefix);
        }
        if 16 + json_size > size {
            return Err(AsarError::Truncated);
        }
        let raw = sniff::read_range(&mut file, size, 16, json_size).ok_or(AsarError::Truncated)?;
        if u64::try_from(raw.len()).ok() != Some(json_size) {
            return Err(AsarError::Truncated);
        }
        let header: Value = serde_json::from_slice(&raw)
            .map_err(|error| AsarError::BadHeader(error.to_string()))?;
        if !header.is_object() {
            return Err(AsarError::BadHeader("not a JSON object".to_owned()));
        }
        Ok(Asar {
            path: path.to_path_buf(),
            header,
            data_offset: 8 + header_size,
            unpacked_dir: sibling(path, ".unpacked"),
        })
    }

    /// The header node at a relative path (`""` is the root). `None` when
    /// the path is missing, absolute or unsafe.
    pub fn node(&self, rel: &str) -> Option<&Value> {
        let mut node = &self.header;
        for part in components(rel)? {
            node = node.get("files")?.get(part)?;
        }
        Some(node)
    }

    /// Whether the archive has an entry at a relative path.
    pub fn exists(&self, rel: &str) -> bool {
        self.node(rel).is_some()
    }

    /// The names in a directory of the archive, sorted; unsafe names are
    /// left out. Empty when the path is not a directory.
    pub fn list(&self, rel: &str) -> Vec<String> {
        let Some(files) = self
            .node(rel)
            .and_then(|node| node.get("files"))
            .and_then(Value::as_object)
        else {
            return Vec::new();
        };
        let mut names: Vec<String> = files.keys().filter(|name| is_safe_name(name)).cloned().collect();
        names.sort();
        names
    }

    /// Every entry, directories before what they hold, names sorted. Unsafe
    /// names are skipped with everything below them; at most
    /// [`MAX_ENTRIES`] entries, at most [`MAX_DEPTH`] levels.
    pub fn walk(&self) -> Vec<AsarEntry> {
        fn push_children<'a>(stack: &mut Vec<(String, &'a Value, usize)>, prefix: &str, node: &'a Value, depth: usize) {
            let Some(files) = node.get("files").and_then(Value::as_object) else {
                return;
            };
            let mut children: Vec<(&String, &Value)> =
                files.iter().filter(|(name, _)| is_safe_name(name)).collect();
            children.sort_by_key(|(name, _)| *name);
            for (name, child) in children.into_iter().rev() {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                stack.push((path, child, depth));
            }
        }

        let mut entries = Vec::new();
        let mut stack: Vec<(String, &Value, usize)> = Vec::new();
        push_children(&mut stack, "", &self.header, 1);
        while let Some((path, node, depth)) = stack.pop() {
            if entries.len() >= MAX_ENTRIES {
                break;
            }
            let is_dir = node.get("files").is_some_and(Value::is_object);
            let is_link = node.get("link").is_some();
            let size = node.get("size").and_then(Value::as_u64).unwrap_or(0);
            let unpacked = node.get("unpacked").and_then(Value::as_bool).unwrap_or(false);
            if is_dir && depth < MAX_DEPTH {
                push_children(&mut stack, &path, node, depth + 1);
            }
            entries.push(AsarEntry {
                path,
                size,
                unpacked,
                is_dir,
                is_link,
            });
        }
        entries
    }

    /// At most `limit` bytes of a file of the archive. A packed file is
    /// read from the data section (its range must lie inside the file); an
    /// unpacked one from `<archive>.unpacked/`, never through a symbolic
    /// link. `None` for a directory, a link, or a missing or unsafe path.
    pub fn read(&self, rel: &str, limit: u64) -> Option<Vec<u8>> {
        let node = self.node(rel)?;
        if node.get("files").is_some() || node.get("link").is_some() {
            return None;
        }
        if node.get("unpacked").and_then(Value::as_bool) == Some(true) {
            let path = resolve_below(&self.unpacked_dir, rel)?;
            return sniff::read_at(&path, 0, limit);
        }
        let offset = match node.get("offset")? {
            Value::String(text) => text.parse::<u64>().ok()?,
            Value::Number(number) => number.as_u64()?,
            _ => return None,
        };
        let size = node.get("size")?.as_u64()?;
        let want = size.min(limit);
        let start = self.data_offset.checked_add(offset)?;
        let bytes = sniff::read_at(&self.path, start, want)?;
        (u64::try_from(bytes.len()).ok()? == want).then_some(bytes)
    }

    /// A JSON file of the archive (at most 4 MiB, a UTF-8 byte-order mark
    /// allowed).
    pub fn read_json(&self, rel: &str) -> Option<Value> {
        parse_json(&self.read(rel, JSON_MAX)?)
    }

    /// Adds or replaces a file that lives in `<archive>.unpacked/` (what a
    /// native module must be: the loader needs a real file). Directory
    /// nodes are created as needed; the file's node becomes `size`,
    /// `unpacked` and, when asked, `executable`, which drops any integrity
    /// record. The file is written with mode 0755 or 0644; no directory on
    /// the way may be a symbolic link. Call [`Asar::save`] to write the
    /// header.
    pub fn put_unpacked(&mut self, rel: &str, data: &[u8], executable: bool) -> io::Result<()> {
        let parts = components(rel)
            .filter(|parts| !parts.is_empty())
            .ok_or_else(|| invalid("not a safe relative path"))?;
        check_insert(&self.header, &parts)?;
        let (last, parents) = parts
            .split_last()
            .ok_or_else(|| invalid("an empty path"))?;
        let mut directory = self.unpacked_dir.clone();
        ensure_real_dir(&directory)?;
        for part in parents {
            directory.push(part);
            ensure_real_dir(&directory)?;
        }
        let target = directory.join(last);
        match fs::symlink_metadata(&target) {
            Ok(meta) if meta.file_type().is_dir() => return Err(invalid("a directory is in the way")),
            Ok(_) => fs::remove_file(&target)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mode = if executable { 0o755 } else { 0o644 };
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&target)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;

        let mut leaf = Map::new();
        leaf.insert("size".to_owned(), Value::from(u64::try_from(data.len()).unwrap_or(u64::MAX)));
        leaf.insert("unpacked".to_owned(), Value::Bool(true));
        if executable {
            leaf.insert("executable".to_owned(), Value::Bool(true));
        }
        insert_node(&mut self.header, &parts, Value::Object(leaf))
    }

    /// Writes the archive with the current header: the compact JSON,
    /// padded to four bytes, behind a new prefix, then the original data
    /// section copied unchanged. The new archive is written beside the old
    /// one and renamed over it; the data offset is updated.
    pub fn save(&mut self) -> io::Result<()> {
        let json = serde_json::to_vec(&self.header).map_err(io::Error::other)?;
        let head = header_bytes(&json)?;
        let temporary = sibling(&self.path, ".tur-new");
        if let Err(error) = self.write_copy(&temporary, &head) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        fs::rename(&temporary, &self.path)?;
        self.data_offset = u64::try_from(head.len()).unwrap_or(u64::MAX);
        Ok(())
    }

    fn write_copy(&self, temporary: &Path, head: &[u8]) -> io::Result<()> {
        let (mut source, size) =
            sniff::open_regular(&self.path).ok_or_else(|| invalid("the archive is not a regular file"))?;
        match fs::symlink_metadata(temporary) {
            Ok(meta) if meta.file_type().is_dir() => return Err(invalid("a directory is in the way")),
            Ok(_) => fs::remove_file(temporary)?,
            Err(_) => {}
        }
        let mut out = OpenOptions::new().write(true).create_new(true).open(temporary)?;
        out.write_all(head)?;
        source.seek(SeekFrom::Start(self.data_offset.min(size)))?;
        let data_len = size.saturating_sub(self.data_offset);
        io::copy(&mut io::Read::take(&mut source, data_len), &mut out)?;
        out.sync_all()?;
        fs::set_permissions(temporary, source.metadata()?.permissions())?;
        Ok(())
    }

    /// Writes a new archive at `dest` with every file packed, in the order
    /// given, replacing any file there. A path that is empty, absolute or
    /// unsafe is an error.
    pub fn create(dest: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
        let mut header = json!({ "files": {} });
        let mut offset: u64 = 0;
        for (rel, data) in files {
            let parts = components(rel)
                .filter(|parts| !parts.is_empty())
                .ok_or_else(|| invalid("not a safe relative path"))?;
            let size = u64::try_from(data.len()).unwrap_or(u64::MAX);
            let mut leaf = Map::new();
            leaf.insert("size".to_owned(), Value::from(size));
            leaf.insert("offset".to_owned(), Value::String(offset.to_string()));
            insert_node(&mut header, &parts, Value::Object(leaf))?;
            offset = offset.saturating_add(size);
        }
        let json = serde_json::to_vec(&header).map_err(io::Error::other)?;
        let head = header_bytes(&json)?;
        let mut out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(dest)?;
        out.write_all(&head)?;
        for (_, data) in files {
            out.write_all(data)?;
        }
        out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sniff::tests::Scratch;

    #[test]
    fn an_archive_round_trips_through_put_unpacked_and_save() {
        let scratch = Scratch::new("asar-round-trip");
        let path = scratch.path.join("app.asar");
        let files: [(&str, &[u8]); 3] = [
            ("package.json", b"\xef\xbb\xbf{\"name\":\"x\",\"version\":\"1.0.0\"}"),
            ("lib/a.txt", b"hello"),
            ("lib/deep/b.txt", b"world!"),
        ];
        Asar::create(&path, &files).expect("create");

        let mut asar = Asar::open(&path).expect("open");
        assert_eq!(asar.read("lib/a.txt", 100).as_deref(), Some(&b"hello"[..]));
        assert_eq!(asar.read("lib/a.txt", 3).as_deref(), Some(&b"hel"[..]));
        assert_eq!(
            asar.read_json("package.json")
                .and_then(|json| json.get("name").and_then(Value::as_str).map(str::to_owned)),
            Some("x".to_owned())
        );
        assert!(asar.exists("lib/deep"));
        assert_eq!(asar.read("lib", 100), None);

        asar.put_unpacked("lib/native/x.node", b"binary bytes", true).expect("put");
        asar.put_unpacked("lib/a.txt", b"replaced", false).expect("replace");
        asar.save().expect("save");

        let reopened = Asar::open(&path).expect("reopen");
        assert_eq!(reopened.data_offset, asar.data_offset);
        assert_eq!(reopened.read("lib/deep/b.txt", 100).as_deref(), Some(&b"world!"[..]));
        assert_eq!(reopened.read("lib/a.txt", 100).as_deref(), Some(&b"replaced"[..]));
        assert_eq!(
            reopened.read("lib/native/x.node", 100).as_deref(),
            Some(&b"binary bytes"[..])
        );
        assert_eq!(reopened.list("lib"), ["a.txt", "deep", "native"]);
        assert_eq!(reopened.list(""), ["lib", "package.json"]);

        let node = reopened.node("lib/native/x.node").expect("node");
        assert_eq!(node.get("unpacked"), Some(&Value::Bool(true)));
        assert_eq!(node.get("executable"), Some(&Value::Bool(true)));
        assert_eq!(node.get("size").and_then(Value::as_u64), Some(12));

        let unpacked = scratch.path.join("app.asar.unpacked/lib/native/x.node");
        let mode = fs::metadata(&unpacked).expect("unpacked file").permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        let plain = scratch.path.join("app.asar.unpacked/lib/a.txt");
        let mode = fs::metadata(&plain).expect("unpacked file").permissions().mode();
        assert_eq!(mode & 0o777, 0o644);

        let walked: Vec<(String, bool, bool)> = reopened
            .walk()
            .into_iter()
            .map(|entry| (entry.path, entry.is_dir, entry.unpacked))
            .collect();
        assert_eq!(
            walked,
            [
                ("lib".to_owned(), true, false),
                ("lib/a.txt".to_owned(), false, true),
                ("lib/deep".to_owned(), true, false),
                ("lib/deep/b.txt".to_owned(), false, false),
                ("lib/native".to_owned(), true, false),
                ("lib/native/x.node".to_owned(), false, true),
                ("package.json".to_owned(), false, false),
            ]
        );
        assert!(!scratch.path.join("app.asar.tur-new").exists());
    }

    #[test]
    fn hostile_names_are_refused_and_skipped() {
        let scratch = Scratch::new("asar-hostile");
        let path = scratch.path.join("evil.asar");
        let header = json!({
            "files": {
                "..": { "size": 1, "offset": "0" },
                "a/b": { "size": 1, "offset": "0" },
                "": { "size": 1, "offset": "0" },
                "nul\u{0}": { "size": 1, "offset": "0" },
                "back\\slash": { "size": 1, "offset": "0" },
                "ok": { "size": 1, "offset": "0" },
                "far": { "size": 4, "offset": "99999" },
                "dir": { "files": { "..": { "files": {} } } }
            }
        });
        let json = serde_json::to_vec(&header).expect("json");
        let mut bytes = header_bytes(&json).expect("prefix");
        bytes.push(b'!');
        fs::write(&path, &bytes).expect("write");

        let asar = Asar::open(&path).expect("open");
        let names: Vec<String> = asar.walk().into_iter().map(|entry| entry.path).collect();
        assert_eq!(names, ["dir", "far", "ok"]);
        assert_eq!(asar.list(""), ["dir", "far", "ok"]);
        assert_eq!(asar.read("ok", 10).as_deref(), Some(&b"!"[..]));
        assert_eq!(asar.read("far", 10), None);
        assert_eq!(asar.node(".."), None);
        assert_eq!(asar.node("dir/.."), None);
        assert_eq!(asar.read("/ok", 10), None);
        assert_eq!(asar.read("a/b", 10), None);

        let mut asar = asar;
        assert!(asar.put_unpacked("../escape.node", b"x", false).is_err());
        assert!(asar.put_unpacked("/abs.node", b"x", false).is_err());
        assert!(asar.put_unpacked("a/../b.node", b"x", false).is_err());
        assert!(asar.put_unpacked("", b"x", false).is_err());
        assert!(asar.put_unpacked("ok/inside.node", b"x", false).is_err());
        assert!(!scratch.path.join("escape.node").exists());

        let bad: [(&str, &[u8]); 1] = [("../x", b"x")];
        assert!(Asar::create(&scratch.path.join("bad.asar"), &bad).is_err());
    }

    #[test]
    fn an_unpacked_file_behind_a_link_is_not_read() {
        let scratch = Scratch::new("asar-link");
        let path = scratch.path.join("app.asar");
        let files: [(&str, &[u8]); 1] = [("a.txt", b"a")];
        Asar::create(&path, &files).expect("create");
        let mut asar = Asar::open(&path).expect("open");
        asar.put_unpacked("mod/x.node", b"real", false).expect("put");
        asar.save().expect("save");

        // Replace the unpacked directory `mod` by a link to elsewhere.
        let elsewhere = scratch.path.join("elsewhere");
        fs::create_dir_all(&elsewhere).expect("dir");
        fs::write(elsewhere.join("x.node"), b"outside").expect("write");
        let unpacked = scratch.path.join("app.asar.unpacked/mod");
        fs::remove_dir_all(&unpacked).expect("remove");
        std::os::unix::fs::symlink(&elsewhere, &unpacked).expect("symlink");
        assert_eq!(asar.read("mod/x.node", 100), None);
        assert!(asar.put_unpacked("mod/y.node", b"y", false).is_err());
        assert!(!elsewhere.join("y.node").exists());
    }

    #[test]
    fn malformed_archives_are_refused() {
        let scratch = Scratch::new("asar-malformed");
        let short = scratch.path.join("short.asar");
        fs::write(&short, b"\x04\0\0\0").expect("write");
        assert!(matches!(Asar::open(&short), Err(AsarError::Truncated)));

        let wrong = scratch.path.join("wrong.asar");
        fs::write(&wrong, [5u8; 32]).expect("write");
        assert!(matches!(Asar::open(&wrong), Err(AsarError::BadPrefix)));

        let mut oversized = Vec::new();
        for value in [4u32, 0xffff_fff0, 0xffff_ffec, 0xffff_ffe0] {
            oversized.extend_from_slice(&value.to_le_bytes());
        }
        let huge = scratch.path.join("huge.asar");
        fs::write(&huge, &oversized).expect("write");
        assert!(matches!(Asar::open(&huge), Err(AsarError::HeaderTooLarge(_))));

        let not_json = scratch.path.join("text.asar");
        let mut bytes = header_bytes(b"{not json}").expect("prefix");
        bytes.extend_from_slice(b"data");
        fs::write(&not_json, &bytes).expect("write");
        assert!(matches!(Asar::open(&not_json), Err(AsarError::BadHeader(_))));

        assert!(matches!(Asar::open(&scratch.path), Err(AsarError::NotAFile)));
        assert!(!AsarError::Truncated.to_string().is_empty());
    }

    #[test]
    fn components_refuse_absolute_and_parent_paths() {
        assert_eq!(components(""), Some(Vec::new()));
        assert_eq!(components("a/b/"), Some(vec!["a", "b"]));
        assert_eq!(components("/a"), None);
        assert_eq!(components("a/../b"), None);
        assert_eq!(components("a//b"), None);
        assert_eq!(components("a\\b"), None);
        assert!(is_safe_name("app.asar"));
        assert!(!is_safe_name("."));
    }
}
