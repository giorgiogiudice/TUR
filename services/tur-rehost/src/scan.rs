// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Streaming searches through a file for version strings and markers.
//!
//! The prototype mapped each file into memory and ran regular expressions
//! over it. Here a file is read in 4 MiB chunks that overlap by 4 KiB
//! (no memory map: unsafe code is forbidden in this crate), and each
//! pattern is a small hand-written matcher (no regular-expression crate).
//! A match is accepted in the chunk where it starts, and only when its
//! start lies before the overlap, unless the chunk is the file's last:
//! every match is seen exactly once, and complete, because no pattern
//! here is longer than the overlap. At most [`SCAN_CAP`] bytes of a file
//! are read by one search.
//!
//! Also here: Electron's fuse wire, read from and written to a binary,
//! and the strings of a PE version resource.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::sniff;

/// The most bytes of one file a search reads (1 GiB). An Electron binary
/// is about 200 MB.
pub const SCAN_CAP: u64 = 1 << 30;

/// The size of one chunk read.
const CHUNK: usize = 4 << 20;
/// How many bytes of a chunk are carried into the next. It is larger than
/// the longest match of every pattern here.
const OVERLAP: usize = 4096;

/// The sentinel that precedes Electron's fuse wire in its binary.
pub const FUSE_SENTINEL: &[u8] = b"dL7pKGdnNz796PbbjQWNKmHXBZaB9tsX";

/// The fuses of Electron's fuse wire, in wire order.
pub const FUSE_NAMES: [&str; 8] = [
    "RunAsNode",
    "EnableCookieEncryption",
    "EnableNodeOptionsEnvironmentVariable",
    "EnableNodeCliInspectArguments",
    "EnableEmbeddedAsarIntegrityValidation",
    "OnlyLoadAppFromAsar",
    "LoadBrowserProcessSpecificV8Snapshot",
    "GrantFileProtocolExtraPrivileges",
];

/// The keys of a PE version resource that [`pe_version_strings`] reads.
const PE_VERSION_KEYS: [&str; 7] = [
    "FileVersion",
    "ProductVersion",
    "ProductName",
    "FileDescription",
    "CompanyName",
    "OriginalFilename",
    "InternalName",
];

/// Reads a regular file in overlapping chunks, at most `cap` bytes, and
/// hands each chunk to `visit` with the file offset of its first byte and
/// the end (exclusive) of the range of starts it owns. `visit` returns
/// `false` to stop. A file that cannot be opened is not visited.
fn stream(path: &Path, cap: u64, mut visit: impl FnMut(&[u8], u64, usize) -> bool) {
    let Some((mut file, size)) = sniff::open_regular(path) else {
        return;
    };
    let total = size.min(cap);
    let first = usize::try_from(total).unwrap_or(usize::MAX).min(CHUNK + OVERLAP);
    let mut buffer: Vec<u8> = Vec::with_capacity(first);
    let mut base: u64 = 0;
    let mut consumed: u64 = 0;
    loop {
        let room = u64::try_from((CHUNK + OVERLAP).saturating_sub(buffer.len())).unwrap_or(0);
        let want = room.min(total - consumed);
        let before = buffer.len();
        if file.by_ref().take(want).read_to_end(&mut buffer).is_err() {
            return;
        }
        let got = u64::try_from(buffer.len() - before).unwrap_or(0);
        consumed += got;
        let last = got < want || consumed >= total;
        let owned = if last {
            buffer.len()
        } else {
            buffer.len().saturating_sub(OVERLAP)
        };
        if !visit(&buffer, base, owned) || last {
            return;
        }
        let dropped = buffer.len().saturating_sub(OVERLAP);
        buffer.drain(..dropped);
        base += u64::try_from(dropped).unwrap_or(0);
    }
}

/// The file offset of the first occurrence of `needle` (at most 4096
/// bytes long) in the first [`SCAN_CAP`] bytes of a regular file.
pub fn find_first(path: &Path, needle: &[u8]) -> Option<u64> {
    if needle.is_empty() || needle.len() > OVERLAP {
        return None;
    }
    let mut found = None;
    stream(path, SCAN_CAP, |buffer, base, owned| {
        if let Some(at) = sniff::find_bytes(buffer, needle, 0)
            && at < owned
        {
            found = Some(base + u64::try_from(at).unwrap_or(0));
            return false;
        }
        true
    });
    found
}

/// Whether a regular file holds `needle` (at most 4096 bytes long) in its
/// first [`SCAN_CAP`] bytes.
pub fn contains(path: &Path, needle: &[u8]) -> bool {
    find_first(path, needle).is_some()
}

/// A matcher: given a buffer and the position of its pattern's literal
/// prefix, the end of the match and the captured value.
type Matcher = fn(&[u8], usize) -> Option<(usize, String)>;

/// The distinct values a matcher captures, in order of appearance, at most
/// `limit` of them. Candidates are the positions of `literal`; matches do
/// not overlap.
fn scan_values(path: &Path, literal: &[u8], limit: usize, matcher: Matcher) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    if limit == 0 {
        return found;
    }
    let mut resume: u64 = 0;
    stream(path, SCAN_CAP, |buffer, base, owned| {
        let mut from = usize::try_from(resume.saturating_sub(base)).unwrap_or(usize::MAX);
        while let Some(at) = sniff::find_bytes(buffer, literal, from) {
            if at >= owned {
                break;
            }
            match matcher(buffer, at) {
                Some((end, value)) => {
                    resume = base + u64::try_from(end).unwrap_or(0);
                    if !found.contains(&value) {
                        found.push(value);
                        if found.len() >= limit {
                            return false;
                        }
                    }
                    from = end;
                }
                None => from = at + 1,
            }
        }
        true
    });
    found
}

/// How many bytes from `at` satisfy `accept`, counting at most `max`.
fn run(buffer: &[u8], at: usize, max: usize, accept: fn(u8) -> bool) -> usize {
    buffer
        .get(at..)
        .unwrap_or_default()
        .iter()
        .take(max)
        .take_while(|&&byte| accept(byte))
        .count()
}

fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

/// `[0-9]+` with at most nine digits: a longer run is not a version
/// component, and refusing it keeps every match shorter than the overlap.
fn number(buffer: &[u8], at: usize) -> Option<usize> {
    let count = run(buffer, at, 10, is_digit);
    (1..=9).contains(&count).then_some(at + count)
}

/// `literal` at `at`.
fn expect(buffer: &[u8], at: usize, literal: &[u8]) -> Option<usize> {
    let end = at.checked_add(literal.len())?;
    (buffer.get(at..end)? == literal).then_some(end)
}

/// `[0-9]+\.[0-9]+\.[0-9]+`.
fn triple(buffer: &[u8], at: usize) -> Option<usize> {
    let at = number(buffer, at)?;
    let at = expect(buffer, at, b".")?;
    let at = number(buffer, at)?;
    let at = expect(buffer, at, b".")?;
    number(buffer, at)
}

fn text(buffer: &[u8], start: usize, end: usize) -> Option<String> {
    Some(String::from_utf8_lossy(buffer.get(start..end)?).into_owned())
}

/// `Chrome/[0-9.]{6,20} Electron/(...)` from `at`, the position of
/// `Chrome/`.
fn electron_user_agent(buffer: &[u8], at: usize) -> Option<(usize, String)> {
    let mut position = at + b"Chrome/".len();
    let chrome = run(buffer, position, 21, |byte| byte.is_ascii_digit() || byte == b'.');
    if !(6..=20).contains(&chrome) {
        return None;
    }
    position += chrome;
    let start = expect(buffer, position, b" Electron/")?;
    let mut end = triple(buffer, start)?;
    // (?:-[a-z]+\.[0-9]+)?
    if let Some(after) = expect(buffer, end, b"-") {
        let letters = run(buffer, after, 17, |byte| byte.is_ascii_lowercase());
        if (1..=16).contains(&letters)
            && let Some(dot) = expect(buffer, after + letters, b".")
            && let Some(next) = number(buffer, dot)
        {
            end = next;
        }
    }
    // (?:\+[a-z0-9]+)?
    if let Some(after) = expect(buffer, end, b"+") {
        let tag = run(buffer, after, 33, |byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit()
        });
        if (1..=32).contains(&tag) {
            end = after + tag;
        }
    }
    Some((end, text(buffer, start, end)?))
}

/// `Chrome/([0-9]{2,3}\.0\.[0-9]{4}\.[0-9]{1,3})` from `at`.
fn chrome_version(buffer: &[u8], at: usize) -> Option<(usize, String)> {
    let start = at + b"Chrome/".len();
    let major = run(buffer, start, 4, is_digit);
    if !(2..=3).contains(&major) {
        return None;
    }
    let position = expect(buffer, start + major, b".0.")?;
    if run(buffer, position, 5, is_digit) != 4 {
        return None;
    }
    let position = expect(buffer, position + 4, b".")?;
    let patch = run(buffer, position, 3, is_digit);
    if patch == 0 {
        return None;
    }
    let end = position + patch;
    Some((end, text(buffer, start, end)?))
}

/// `(?:nwjs|nw\.js|node-webkit)[ /]v?([0-9]+\.[0-9]+\.[0-9]+)` from `at`,
/// a position of `n`.
fn nw_version(buffer: &[u8], at: usize) -> Option<(usize, String)> {
    let position = [&b"nwjs"[..], b"nw.js", b"node-webkit"]
        .iter()
        .find_map(|word| expect(buffer, at, word))?;
    if !matches!(buffer.get(position), Some(b' ' | b'/')) {
        return None;
    }
    let mut start = position + 1;
    if buffer.get(start) == Some(&b'v') {
        start += 1;
    }
    let end = triple(buffer, start)?;
    Some((end, text(buffer, start, end)?))
}

/// `LOVE ([0-9]+\.[0-9]+(?:\.[0-9]+)?)` from `at`.
fn love_version(buffer: &[u8], at: usize) -> Option<(usize, String)> {
    let start = at + b"LOVE ".len();
    let position = number(buffer, start)?;
    let position = expect(buffer, position, b".")?;
    let mut end = number(buffer, position)?;
    if let Some(dot) = expect(buffer, end, b".")
        && let Some(next) = number(buffer, dot)
    {
        end = next;
    }
    Some((end, text(buffer, start, end)?))
}

/// `\.NETFramework,Version=v([0-9.]+)` from `at` (at most 20 characters
/// are taken).
fn dotnet_framework_version(buffer: &[u8], at: usize) -> Option<(usize, String)> {
    let start = at + b".NETFramework,Version=v".len();
    let length = run(buffer, start, 20, |byte| byte.is_ascii_digit() || byte == b'.');
    if length == 0 {
        return None;
    }
    let end = start + length;
    Some((end, text(buffer, start, end)?))
}

/// Electron versions named by user-agent strings in a binary (the
/// prototype's `Chrome/[0-9.]{6,20} Electron/([0-9]+\.[0-9]+\.[0-9]+
/// (?:-[a-z]+\.[0-9]+)?(?:\+[a-z0-9]+)?)`), distinct, in order of
/// appearance, at most `limit`. Version components longer than nine
/// digits are not matched.
pub fn electron_versions(path: &Path, limit: usize) -> Vec<String> {
    scan_values(path, b"Chrome/", limit, electron_user_agent)
}

/// Chromium versions (`Chrome/([0-9]{2,3}\.0\.[0-9]{4}\.[0-9]{1,3})`),
/// distinct, in order of appearance, at most `limit`.
pub fn chrome_versions(path: &Path, limit: usize) -> Vec<String> {
    scan_values(path, b"Chrome/", limit, chrome_version)
}

/// NW.js versions (`(?:nwjs|nw\.js|node-webkit)[ /]v?([0-9]+\.[0-9]+\.
/// [0-9]+)`), distinct, in order of appearance, at most `limit`.
pub fn nw_versions(path: &Path, limit: usize) -> Vec<String> {
    scan_values(path, b"n", limit, nw_version)
}

/// LÖVE versions (`LOVE ([0-9]+\.[0-9]+(?:\.[0-9]+)?)`), distinct, in
/// order of appearance, at most `limit`.
pub fn love_versions(path: &Path, limit: usize) -> Vec<String> {
    scan_values(path, b"LOVE ", limit, love_version)
}

/// .NET Framework target versions (`\.NETFramework,Version=v([0-9.]+)`),
/// distinct, in order of appearance, at most `limit`.
pub fn dotnet_framework_versions(path: &Path, limit: usize) -> Vec<String> {
    scan_values(
        path,
        b".NETFramework,Version=v",
        limit,
        dotnet_framework_version,
    )
}

fn fuse_name(index: usize) -> String {
    FUSE_NAMES
        .get(index)
        .map_or_else(|| format!("fuse{index}"), |name| (*name).to_owned())
}

/// A fuse state as text: `0`, `1`, `r`, or `?` for a byte that is not a
/// printable ASCII character (so a hostile binary cannot put control
/// characters in a report).
fn fuse_state(byte: u8) -> String {
    if byte.is_ascii_graphic() {
        char::from(byte).to_string()
    } else {
        "?".to_owned()
    }
}

/// Electron's fuse wire in a binary: the sentinel, a version byte that
/// must be 1, a length byte of at most 32, then one byte per fuse (`0`
/// off, `1` on, `r` removed). Fuses past [`FUSE_NAMES`] are named
/// `fuse<N>`. `None` when the binary has no wire.
pub fn read_fuses(path: &Path) -> Option<BTreeMap<String, String>> {
    let at = find_first(path, FUSE_SENTINEL)?;
    let start = at.checked_add(u64::try_from(FUSE_SENTINEL.len()).ok()?)?;
    let wire = sniff::read_at(path, start, 2 + 32)?;
    let (&version, rest) = wire.split_first()?;
    let (&length, states) = rest.split_first()?;
    if version != 1 || length > 32 {
        return None;
    }
    Some(
        states
            .iter()
            .take(usize::from(length))
            .enumerate()
            .map(|(index, &state)| (fuse_name(index), fuse_state(state)))
            .collect(),
    )
}

/// Sets fuses on an Electron binary, in place: for each fuse named in
/// `wanted` with the value `0` or `1`, whose current state is `0` or `1`
/// and differs, the one byte is rewritten. Removed fuses (`r`), fuses the
/// binary does not have and other values are left alone. Returns what
/// changed, as `name` to `0->1` or `1->0`; empty when the binary has no
/// wire.
///
/// Run only on the Linux Electron binary TUR fetched, never on a
/// publisher's file.
pub fn write_fuses(
    path: &Path,
    wanted: &BTreeMap<String, String>,
) -> io::Result<BTreeMap<String, String>> {
    let mut changed = BTreeMap::new();
    let before = std::fs::symlink_metadata(path)?;
    if !before.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let Some(at) = find_first(path, FUSE_SENTINEL) else {
        return Ok(changed);
    };
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let after = file.metadata()?;
    if after.dev() != before.dev() || after.ino() != before.ino() {
        return Err(io::Error::other("the file changed while it was opened"));
    }
    let start = at + u64::try_from(FUSE_SENTINEL.len()).unwrap_or(0);
    file.seek(SeekFrom::Start(start))?;
    let mut wire = Vec::with_capacity(34);
    file.by_ref().take(34).read_to_end(&mut wire)?;
    let (Some(&version), Some(&length)) = (wire.first(), wire.get(1)) else {
        return Ok(changed);
    };
    if version != 1 || length > 32 {
        return Ok(changed);
    }
    let states: Vec<u8> = wire.iter().skip(2).take(usize::from(length)).copied().collect();
    for (index, state) in states.into_iter().enumerate() {
        let name = fuse_name(index);
        let target = match wanted.get(&name).map(String::as_str) {
            Some("0") => b'0',
            Some("1") => b'1',
            _ => continue,
        };
        if (state == b'0' || state == b'1') && state != target {
            let offset = start + 2 + u64::try_from(index).unwrap_or(0);
            file.seek(SeekFrom::Start(offset))?;
            file.write_all(&[target])?;
            let from = char::from(state);
            let to = char::from(target);
            changed.insert(name, format!("{from}->{to}"));
        }
    }
    file.flush()?;
    Ok(changed)
}

fn utf16z(key: &str) -> Vec<u8> {
    key.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// The value after a version-resource key: 32-bit aligned UTF-16LE, ended
/// by a NUL; printable only, at most 120 characters.
fn version_value(path: &Path, after_key: u64) -> Option<String> {
    let start = after_key.checked_add((4 - after_key % 4) % 4)?;
    let raw = sniff::read_at(path, start, 512)?;
    let mut end = sniff::find_bytes(&raw, b"\0\0\0", 0)?;
    if end % 2 == 1 {
        end += 1;
    }
    let units: Vec<u16> = raw
        .get(..end)?
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let decoded = String::from_utf16(&units).ok()?;
    let value = decoded.trim_matches(['\0', ' ']);
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.chars().take(120).collect())
}

/// `FileVersion`, `ProductVersion`, `ProductName`, `FileDescription`,
/// `CompanyName`, `OriginalFilename` and `InternalName` of a PE version
/// resource, found by their UTF-16LE keys without walking the resource
/// tree (the prototype's `binfmt.pe_version_strings`). The last
/// occurrence of each key counts. The file is read once, in one streaming
/// pass over at most [`SCAN_CAP`] bytes; the whole file is searched,
/// because a version resource is not always near the end.
pub fn pe_version_strings(path: &Path) -> BTreeMap<String, String> {
    let needles: Vec<Vec<u8>> = PE_VERSION_KEYS.iter().map(|key| utf16z(key)).collect();
    let mut last: Vec<Option<u64>> = vec![None; needles.len()];
    stream(path, SCAN_CAP, |buffer, base, owned| {
        for (needle, slot) in needles.iter().zip(last.iter_mut()) {
            let mut from = 0;
            while let Some(at) = sniff::find_bytes(buffer, needle, from) {
                if at >= owned {
                    break;
                }
                *slot = Some(base + u64::try_from(at).unwrap_or(0));
                from = at + 1;
            }
        }
        true
    });
    let mut strings = BTreeMap::new();
    for ((key, needle), at) in PE_VERSION_KEYS.iter().zip(&needles).zip(&last) {
        let Some(at) = *at else {
            continue;
        };
        let after = at + u64::try_from(needle.len()).unwrap_or(0);
        if let Some(value) = version_value(path, after) {
            strings.insert((*key).to_owned(), value);
        }
    }
    strings
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sniff::tests::Scratch;

    #[test]
    fn electron_user_agents_are_read_with_their_suffixes() {
        let scratch = Scratch::new("scan-ua");
        let mut data = b"junk Chrome/138.0.7204.100 Electron/43.7.7\0".to_vec();
        data.extend_from_slice(b"Chrome/120.0.6099.291 Electron/28.3.3+wvcus Safari\0");
        data.extend_from_slice(b"Chrome/120.0.6099.291 Electron/30.0.0-beta.4\0");
        data.extend_from_slice(b"Chrome/138.0.7204.100 Electron/43.7.7\0");
        data.extend_from_slice(b"Chrome/1.2 Electron/1.2.3\0");
        data.extend_from_slice(b"Chrome/120.0.6099.291 Electron/12345678901.0.0\0");
        let path = scratch.write("bin", &data);
        assert_eq!(
            electron_versions(&path, 10),
            ["43.7.7", "28.3.3+wvcus", "30.0.0-beta.4"]
        );
        assert_eq!(electron_versions(&path, 1), ["43.7.7"]);
        assert_eq!(chrome_versions(&path, 10), ["138.0.7204.100", "120.0.6099.291"]);
    }

    #[test]
    fn chrome_versions_follow_the_prototype_pattern() {
        let scratch = Scratch::new("scan-chrome");
        let data = b"Chrome/1234.0.1234.1 Chrome/99.0.12345.1 Chrome/99.1.1234.1 Chrome/88.0.4324.1500";
        let path = scratch.write("bin", data);
        assert_eq!(chrome_versions(&path, 10), ["88.0.4324.150"]);
    }

    #[test]
    fn a_match_across_a_chunk_boundary_is_found_once() {
        let scratch = Scratch::new("scan-boundary");
        let needle = b"Chrome/138.0.7204.100 Electron/43.7.7\0";
        // The user-agent string starts inside the first chunk's overlap and
        // ends past it: the first chunk cannot see it whole, the second can.
        let mut data = vec![b'x'; CHUNK + OVERLAP - 20];
        data.extend_from_slice(needle);
        data.extend(std::iter::repeat_n(b'y', OVERLAP * 2));
        data.extend_from_slice(b"LOVE 11.5\0");
        data.extend_from_slice(FUSE_SENTINEL);
        data.extend_from_slice(b"\x01\x03r01");
        let path = scratch.write("bin", &data);
        assert_eq!(electron_versions(&path, 10), ["43.7.7"]);
        assert_eq!(love_versions(&path, 10), ["11.5"]);
        assert!(contains(&path, b"Electron/43.7.7"));
        assert!(!contains(&path, b"Electron/44"));
        assert_eq!(
            find_first(&path, b"Chrome/"),
            Some(u64::try_from(CHUNK + OVERLAP - 20).expect("offset"))
        );
        let fuses = read_fuses(&path).expect("fuses");
        assert_eq!(fuses.get("RunAsNode").map(String::as_str), Some("r"));
        assert_eq!(fuses.get("EnableCookieEncryption").map(String::as_str), Some("0"));
        assert_eq!(fuses.len(), 3);
    }

    #[test]
    fn nw_love_and_framework_versions() {
        let scratch = Scratch::new("scan-other");
        let data = b"node nwjs/v0.50.3 nw.js 0.70.1 node-webkit/0.8.6 nwjs 1.2 LOVE 11 LOVE 0.10.2 \
            .NETFramework,Version=v4.7.2\0";
        let path = scratch.write("bin", data);
        assert_eq!(nw_versions(&path, 10), ["0.50.3", "0.70.1", "0.8.6"]);
        assert_eq!(love_versions(&path, 10), ["0.10.2"]);
        assert_eq!(dotnet_framework_versions(&path, 10), ["4.7.2"]);
        assert!(electron_versions(&path, 10).is_empty());
    }

    #[test]
    fn fuses_are_read_and_only_zero_or_one_states_change() {
        let scratch = Scratch::new("scan-fuses");
        let mut data = b"head".to_vec();
        data.extend_from_slice(FUSE_SENTINEL);
        data.extend_from_slice(b"\x01\x0901r10r01\x01");
        data.extend_from_slice(b"tail");
        let path = scratch.write("electron", &data);
        let fuses = read_fuses(&path).expect("fuses");
        assert_eq!(fuses.len(), 9);
        assert_eq!(fuses.get("RunAsNode").map(String::as_str), Some("0"));
        assert_eq!(fuses.get("fuse8").map(String::as_str), Some("?"));

        let wanted: BTreeMap<String, String> = [
            ("RunAsNode", "1"),
            ("EnableCookieEncryption", "1"),
            ("EnableNodeOptionsEnvironmentVariable", "0"),
            ("EnableNodeCliInspectArguments", "0"),
            ("EnableEmbeddedAsarIntegrityValidation", "x"),
            ("NoSuchFuse", "1"),
        ]
        .iter()
        .map(|(name, state)| ((*name).to_owned(), (*state).to_owned()))
        .collect();
        let changed = write_fuses(&path, &wanted).expect("write");
        let expected: BTreeMap<String, String> = [
            ("RunAsNode".to_owned(), "0->1".to_owned()),
            ("EnableNodeCliInspectArguments".to_owned(), "1->0".to_owned()),
        ]
        .into_iter()
        .collect();
        assert_eq!(changed, expected);
        let after = std::fs::read(&path).expect("read back");
        let wire = sniff::find_bytes(&after, FUSE_SENTINEL, 0).expect("sentinel") + 32;
        assert_eq!(&after[wire..wire + 11], b"\x01\x0911r00r01\x01");
        assert_eq!(after.len(), data.len());

        let plain = scratch.write("plain", b"no wire here");
        assert_eq!(read_fuses(&plain), None);
        assert!(write_fuses(&plain, &wanted).expect("write").is_empty());
        assert!(write_fuses(&scratch.path, &wanted).is_err());
    }

    #[test]
    fn a_wire_with_another_version_is_not_read() {
        let scratch = Scratch::new("scan-fuse-version");
        let mut data = FUSE_SENTINEL.to_vec();
        data.extend_from_slice(b"\x02\x0201");
        let path = scratch.write("electron", &data);
        assert_eq!(read_fuses(&path), None);
    }

    #[test]
    fn pe_version_strings_take_the_last_aligned_value() {
        let scratch = Scratch::new("scan-version");
        let mut data = vec![0u8; 100];
        data.extend_from_slice(&utf16z("FileVersion"));
        data.extend_from_slice(&utf16z("1.0.0"));
        data.extend_from_slice(&[0, 0]);
        while data.len() % 4 != 2 {
            data.push(0);
        }
        // The last occurrence: its value starts on a 32-bit boundary.
        data.extend_from_slice(&utf16z("FileVersion"));
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
        data.extend_from_slice(&utf16z("11.5.0"));
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(&utf16z("ProductName"));
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
        data.extend_from_slice(&utf16z("Bad\u{1b}[31m"));
        data.extend_from_slice(&[0, 0, 0, 0]);
        let path = scratch.write("love.dll", &data);
        let strings = pe_version_strings(&path);
        assert_eq!(strings.get("FileVersion").map(String::as_str), Some("11.5.0"));
        assert_eq!(strings.get("ProductName"), None);
        assert_eq!(strings.len(), 1);
    }

    #[test]
    fn missing_files_and_links_are_not_scanned() {
        let scratch = Scratch::new("scan-missing");
        let target = scratch.write("bin", b"Chrome/138.0.7204.100 Electron/43.7.7\0");
        let link = scratch.path.join("link");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        assert!(electron_versions(&link, 3).is_empty());
        assert!(electron_versions(&scratch.path.join("none"), 3).is_empty());
        assert!(!contains(&scratch.path, b"x"));
        assert!(pe_version_strings(&link).is_empty());
    }
}
