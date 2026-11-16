// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What a file is, read from its bytes and never executed.
//!
//! The machine-code format and processor architectures of a program (PE,
//! ELF, Mach-O), the kind of container a download is (zip, 7z, dmg, ...),
//! the headers of a PE image, and the central directory of a zip archive,
//! standing alone or appended to a program. Ported from the prototype's
//! `binfmt.py` and `unpack.py` (tests/experiments/rehost/rehostlib/).
//!
//! Every read is bounded, every offset taken from a file is checked before
//! it is used, and a symbolic link is never followed (the rules of
//! docs/architecture/runtime-manager.md, "Hostile input"). Nothing is
//! decompressed: a zip member can be read only when it is stored.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use serde::{Serialize, Serializer};

use crate::finding::{BinaryFormat, BinaryKind};

/// How many bytes of a file's head [`binary_kind`] reads.
pub const HEAD_LEN: u64 = 4096;

/// The most bytes of PE headers (COFF header, optional header, section
/// table) that [`pe_info`] reads.
const PE_HEADERS_MAX: u64 = 64 << 10;
/// How far from the end of a file [`zip_tail`] looks for the end record.
const ZIP_TAIL_WINDOW: u64 = 1 << 20;
/// The largest central directory [`zip_names`] reads.
const ZIP_DIRECTORY_MAX: u64 = 16 << 20;
/// The longest member name kept.
const ZIP_NAME_MAX: usize = 4096;
/// How many members [`zip_read_stored`] looks through.
const ZIP_LOOKUP_ENTRIES: usize = 100_000;
/// A Mach-O universal header lists at most this many architectures. More
/// means a Java class file, which shares the magic number.
const FAT_ARCH_MAX: u32 = 44;

// ------------------------------------------------------------- bounded reads

/// Opens a regular file without following a symbolic link: the file is
/// examined with `symlink_metadata` first, and the opened file must be the
/// same one (device and inode). Devices, pipes and sockets are refused
/// before they are opened. Returns the file and its size.
pub(crate) fn open_regular(path: &Path) -> Option<(File, u64)> {
    let before = std::fs::symlink_metadata(path).ok()?;
    if !before.file_type().is_file() {
        return None;
    }
    let file = File::open(path).ok()?;
    let after = file.metadata().ok()?;
    if !after.is_file() || after.dev() != before.dev() || after.ino() != before.ino() {
        return None;
    }
    Some((file, after.len()))
}

/// The size of a regular file; `None` for anything else, a symbolic link
/// included.
pub(crate) fn regular_len(path: &Path) -> Option<u64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    meta.file_type().is_file().then_some(meta.len())
}

/// At most `max` bytes of an open file from `offset`. `None` when the
/// offset is past the end. The buffer is never larger than what the file
/// holds from there.
pub(crate) fn read_range(file: &mut File, size: u64, offset: u64, max: u64) -> Option<Vec<u8>> {
    if offset > size {
        return None;
    }
    let want = max.min(size - offset);
    let capacity = usize::try_from(want).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buffer = Vec::with_capacity(capacity);
    file.by_ref().take(want).read_to_end(&mut buffer).ok()?;
    Some(buffer)
}

/// At most `max` bytes of a regular file from `offset` (see
/// [`open_regular`] and [`read_range`]).
pub(crate) fn read_at(path: &Path, offset: u64, max: u64) -> Option<Vec<u8>> {
    let (mut file, size) = open_regular(path)?;
    read_range(&mut file, size, offset, max)
}

/// The first position at or after `from` where `needle` occurs in `hay`.
pub(crate) fn find_bytes(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let first = *needle.first()?;
    let last_start = hay.len().checked_sub(needle.len())?;
    let mut at = from;
    while at <= last_start {
        let offset = hay.get(at..=last_start)?.iter().position(|&byte| byte == first)?;
        let candidate = at + offset;
        if hay.get(candidate..candidate + needle.len()) == Some(needle) {
            return Some(candidate);
        }
        at = candidate + 1;
    }
    None
}

/// The last position where `needle` occurs in `hay`.
pub(crate) fn rfind_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len())
        .rev()
        .find(|&at| hay.get(at..at + needle.len()) == Some(needle))
}

fn array<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// A little-endian `u16` at `at`, when the bytes are there.
pub(crate) fn u16_le(bytes: &[u8], at: usize) -> Option<u16> {
    array(bytes, at).map(u16::from_le_bytes)
}

/// A big-endian `u16` at `at`, when the bytes are there.
pub(crate) fn u16_be(bytes: &[u8], at: usize) -> Option<u16> {
    array(bytes, at).map(u16::from_be_bytes)
}

/// A little-endian `u32` at `at`, when the bytes are there.
pub(crate) fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    array(bytes, at).map(u32::from_le_bytes)
}

/// A big-endian `u32` at `at`, when the bytes are there.
pub(crate) fn u32_be(bytes: &[u8], at: usize) -> Option<u32> {
    array(bytes, at).map(u32::from_be_bytes)
}

/// A little-endian `u64` at `at`, when the bytes are there.
pub(crate) fn u64_le(bytes: &[u8], at: usize) -> Option<u64> {
    array(bytes, at).map(u64::from_le_bytes)
}

// -------------------------------------------------------------- binary kind

fn pe_machine(machine: u16) -> String {
    match machine {
        0x8664 => "x86_64".to_owned(),
        0x14c => "x86".to_owned(),
        0xaa64 => "arm64".to_owned(),
        0x1c4 => "arm".to_owned(),
        other => format!("{other:#x}"),
    }
}

fn elf_machine(machine: u16) -> String {
    match machine {
        62 => "x86_64".to_owned(),
        3 => "x86".to_owned(),
        183 => "arm64".to_owned(),
        40 => "arm".to_owned(),
        243 => "riscv".to_owned(),
        other => other.to_string(),
    }
}

fn macho_cpu(cpu: u32) -> String {
    match cpu {
        0x0100_0007 => "x86_64".to_owned(),
        0x0100_000c => "arm64".to_owned(),
        7 => "x86".to_owned(),
        12 => "arm".to_owned(),
        18 => "ppc".to_owned(),
        0x0100_0012 => "ppc64".to_owned(),
        other => format!("{other:#x}"),
    }
}

/// The PE machine field, when the head holds the PE signature.
fn pe_machine_of_head(head: &[u8]) -> Option<u16> {
    let lfanew = usize::try_from(u32_le(head, 0x3c)?).ok()?;
    if head.get(lfanew..lfanew.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    u16_le(head, lfanew.checked_add(4)?)
}

fn universal(head: &[u8], wide: bool) -> Option<BinaryKind> {
    let count = u32_be(head, 4)?;
    if count > FAT_ARCH_MAX {
        return None;
    }
    let count = usize::try_from(count).ok()?;
    let step = if wide { 32 } else { 20 };
    let archs = (0..count)
        .map_while(|index| u32_be(head, 8 + index * step))
        .map(macho_cpu)
        .collect();
    Some(BinaryKind {
        format: BinaryFormat::MachO,
        archs,
    })
}

/// The machine-code format and architectures of a file, from its first
/// bytes (at least 20 are needed).
///
/// PE: the machine of the PE header that `e_lfanew` points to, or `?` when
/// the head holds no PE header. ELF: `e_machine`, in the byte order
/// `EI_DATA` names. Mach-O: thin images in either byte order, 32 or 64
/// bit, and universal images (`cafebabe`, `cafebabf`); a universal header
/// that claims more than 44 architectures is a Java class file and gives
/// `None`. An unknown machine is written as its number.
pub fn binary_kind_of_head(head: &[u8]) -> Option<BinaryKind> {
    if head.len() < 20 {
        return None;
    }
    if head.starts_with(b"MZ") {
        let arch = pe_machine_of_head(head).map_or_else(|| "?".to_owned(), pe_machine);
        return Some(BinaryKind {
            format: BinaryFormat::Pe,
            archs: vec![arch],
        });
    }
    if head.starts_with(b"\x7fELF") {
        let machine = if head.get(5) == Some(&1) {
            u16_le(head, 18)?
        } else {
            u16_be(head, 18)?
        };
        return Some(BinaryKind {
            format: BinaryFormat::Elf,
            archs: vec![elf_machine(machine)],
        });
    }
    let magic = head.get(..4)?;
    let cpu = match magic {
        b"\xcf\xfa\xed\xfe" | b"\xce\xfa\xed\xfe" => u32_le(head, 4)?,
        b"\xfe\xed\xfa\xcf" | b"\xfe\xed\xfa\xce" => u32_be(head, 4)?,
        b"\xca\xfe\xba\xbe" => return universal(head, false),
        b"\xca\xfe\xba\xbf" => return universal(head, true),
        _ => return None,
    };
    Some(BinaryKind {
        format: BinaryFormat::MachO,
        archs: vec![macho_cpu(cpu)],
    })
}

/// The machine-code format of a regular file (a symbolic link is not
/// followed), from at most its first 4096 bytes.
pub fn binary_kind(path: &Path) -> Option<BinaryKind> {
    let head = read_at(path, 0, HEAD_LEN)?;
    binary_kind_of_head(&head)
}

// ---------------------------------------------------------------- container

/// The kind of container a file is, from its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Container {
    /// A zip archive (also a `.love`, a jar, an `.appx`).
    Zip,
    /// A 7-Zip archive.
    SevenZip,
    /// An OLE compound file: an MSI database.
    Ole,
    /// A xar archive: a macOS `.pkg`.
    Xar,
    /// An `ar` archive: a `.deb`.
    Ar,
    /// A Godot pack.
    Pck,
    /// A gzip, bzip2, xz or zstd stream, usually a compressed tar.
    TarCompressed,
    /// A plain tar archive.
    Tar,
    /// A Windows PE program (an installer, or the application itself).
    Pe,
    /// An ELF program.
    Elf,
    /// An AppImage (an ELF program with the AppImage type 2 mark).
    AppImage,
    /// A Mach-O program.
    MachO,
    /// A macOS disk image (UDIF, with its `koly` trailer).
    Dmg,
    /// None of these.
    Other,
}

impl Container {
    /// The container as written in JSON, for example `7z` or `mach-o`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Container::Zip => "zip",
            Container::SevenZip => "7z",
            Container::Ole => "ole",
            Container::Xar => "xar",
            Container::Ar => "ar",
            Container::Pck => "pck",
            Container::TarCompressed => "tar-compressed",
            Container::Tar => "tar",
            Container::Pe => "pe",
            Container::Elf => "elf",
            Container::AppImage => "appimage",
            Container::MachO => "mach-o",
            Container::Dmg => "dmg",
            Container::Other => "other",
        }
    }
}

impl Serialize for Container {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The kind of container a regular file is, from its first 1024 bytes and,
/// for a disk image, its last 512 (the rules of the prototype's
/// `unpack.sniff`). A symbolic link, a directory or a device is an error.
pub fn sniff(path: &Path) -> io::Result<Container> {
    let (mut file, size) = open_regular(path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"))?;
    let head = read_range(&mut file, size, 0, 1024)
        .ok_or_else(|| io::Error::other("the file could not be read"))?;
    let container = if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        Container::Zip
    } else if head.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Container::SevenZip
    } else if head.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
        Container::Ole
    } else if head.starts_with(b"xar!") {
        Container::Xar
    } else if head.starts_with(b"!<arch>\n") {
        Container::Ar
    } else if head.starts_with(b"GDPC") {
        Container::Pck
    } else if head.starts_with(b"\x1f\x8b")
        || head.starts_with(b"BZh")
        || head.starts_with(b"\xfd7zXZ\0")
        || head.starts_with(b"\x28\xb5\x2f\xfd")
    {
        Container::TarCompressed
    } else if head.len() > 262 && head.get(257..262) == Some(&b"ustar"[..]) {
        Container::Tar
    } else if head.starts_with(b"MZ") {
        Container::Pe
    } else if head.starts_with(b"\x7fELF") {
        if head.get(8..11) == Some(&b"AI\x02"[..]) {
            Container::AppImage
        } else {
            Container::Elf
        }
    } else if size > 512
        && read_range(&mut file, size, size - 512, 4).is_some_and(|trailer| trailer == b"koly")
    {
        Container::Dmg
    } else if binary_kind_of_head(&head).is_some_and(|kind| kind.format == BinaryFormat::MachO) {
        Container::MachO
    } else {
        Container::Other
    };
    Ok(container)
}

// ----------------------------------------------------------------------- PE

/// What the headers of a PE image say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeInfo {
    /// The machine: `x86_64`, `x86`, `arm64`, `arm`, or its number.
    pub machine: String,
    /// The optional header's subsystem (2 a Windows GUI program, 3 a
    /// console program); 0 when there is no PE32 or PE32+ optional header.
    pub subsystem: u16,
    /// Whether the image has a CLR header (data directory 14): a .NET
    /// assembly.
    pub clr: bool,
    /// Whether the image is a DLL (characteristic 0x2000).
    pub is_dll: bool,
    /// Where data appended after the last section starts: the largest
    /// `PointerToRawData + SizeOfRawData` of the section table. It can lie
    /// past the end of a damaged file.
    pub overlay_offset: u64,
    /// How many bytes follow the last section (0 when none do).
    pub overlay_size: u64,
    /// The file's size.
    pub size: u64,
}

/// One entry of a PE section table.
pub(crate) struct PeSection {
    /// The section's name (`.text`, `pck`, ...).
    pub(crate) name: String,
    /// Where its data starts in the file.
    pub(crate) raw: u64,
    /// How many bytes of data it has in the file.
    pub(crate) size: u64,
}

/// [`PeInfo`] with the section table and where the Authenticode
/// signature starts.
pub(crate) struct PeDetails {
    /// What [`pe_info`] returns.
    pub(crate) info: PeInfo,
    /// The section table, as far as it was read.
    pub(crate) sections: Vec<PeSection>,
    /// The file offset of the Authenticode signature (data directory 4),
    /// 0 when there is none.
    pub(crate) signature_offset: u64,
}

/// The headers of a PE image with its section table (at most 64 KiB of
/// headers are read). `None` when the file is not a PE image.
pub(crate) fn pe_details(path: &Path) -> Option<PeDetails> {
    let (mut file, size) = open_regular(path)?;
    let head = read_range(&mut file, size, 0, 64)?;
    if head.len() < 64 || !head.starts_with(b"MZ") {
        return None;
    }
    let lfanew = u64::from(u32_le(&head, 0x3c)?);
    if lfanew.checked_add(24)? > size {
        return None;
    }
    let headers = read_range(&mut file, size, lfanew, PE_HEADERS_MAX)?;
    if !headers.starts_with(b"PE\0\0") {
        return None;
    }
    let machine = u16_le(&headers, 4)?;
    let section_count = usize::from(u16_le(&headers, 6)?);
    let optional_size = usize::from(u16_le(&headers, 20)?);
    let characteristics = u16_le(&headers, 22)?;
    let optional_end = 24 + optional_size;
    let optional = headers
        .get(24..optional_end.min(headers.len()))
        .unwrap_or_default();
    let magic = u16_le(optional, 0).unwrap_or(0);
    let mut subsystem = 0;
    let mut clr = false;
    let mut signature_offset = 0;
    if magic == 0x10b || magic == 0x20b {
        let directories = if magic == 0x20b { 112 } else { 96 };
        subsystem = u16_le(optional, 68).unwrap_or(0);
        let clr_rva = u32_le(optional, directories + 8 * 14).unwrap_or(0);
        let clr_size = u32_le(optional, directories + 8 * 14 + 4).unwrap_or(0);
        clr = clr_rva != 0 && clr_size != 0;
        signature_offset = u64::from(u32_le(optional, directories + 8 * 4).unwrap_or(0));
    }
    let mut sections = Vec::new();
    let mut end: u64 = 0;
    for index in 0..section_count {
        let start = optional_end + index * 40;
        let Some(entry) = headers.get(start..start + 40) else {
            break;
        };
        let name: String = entry
            .iter()
            .take(8)
            .take_while(|&&byte| byte != 0)
            .map(|&byte| char::from(byte))
            .collect();
        let raw_size = u64::from(u32_le(entry, 16)?);
        let raw = u64::from(u32_le(entry, 20)?);
        end = end.max(raw + raw_size);
        sections.push(PeSection {
            name,
            raw,
            size: raw_size,
        });
    }
    Some(PeDetails {
        info: PeInfo {
            machine: pe_machine(machine),
            subsystem,
            clr,
            is_dll: characteristics & 0x2000 != 0,
            overlay_offset: end,
            overlay_size: size.saturating_sub(end),
            size,
        },
        sections,
        signature_offset,
    })
}

/// The headers of a PE image: machine, subsystem, CLR header, DLL flag and
/// where appended data (the overlay) starts. At most 64 KiB of headers
/// are read. `None` when the file is not a PE image.
pub fn pe_info(path: &Path) -> Option<PeInfo> {
    pe_details(path).map(|details| details.info)
}

// ---------------------------------------------------------------------- zip

/// Where a zip archive lies in a file: alone, or appended to a program (a
/// fused LÖVE game, a launch4j jar, a self-extracting archive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipTail {
    /// Where the archive starts: the base its member offsets are
    /// relative to.
    pub start: u64,
    /// Where its end record (with its comment) ends. Bytes after it (an
    /// Authenticode signature) are not part of the archive.
    pub end: u64,
    /// How many members the end record announces.
    pub entries: u64,
    /// The file offset of the central directory.
    pub directory: u64,
    /// The central directory's size in bytes.
    pub directory_size: u64,
}

fn end_record_at(
    file: &mut File,
    size: u64,
    tail: &[u8],
    base: u64,
    at: usize,
) -> Option<ZipTail> {
    let record = tail.get(at..at.checked_add(22)?)?;
    let entries = u16_le(record, 10)?;
    let directory_size = u64::from(u32_le(record, 12)?);
    let directory_offset = u64::from(u32_le(record, 16)?);
    let comment = u64::from(u16_le(record, 20)?);
    let end_record = base.checked_add(u64::try_from(at).ok()?)?;
    let start = end_record
        .checked_sub(directory_size)?
        .checked_sub(directory_offset)?;
    if entries == 0 || start >= end_record || directory_size >= size {
        return None;
    }
    let directory = start.checked_add(directory_offset)?;
    let signature = read_range(file, size, directory, 4)?;
    if signature != b"PK\x01\x02" {
        return None;
    }
    Some(ZipTail {
        start,
        end: end_record + 22 + comment,
        entries: u64::from(entries),
        directory,
        directory_size,
    })
}

/// Where a zip archive at the end of a file lies, as the prototype's
/// `binfmt.zip_tail`: the end record is searched for in the last 1 MiB
/// (from the end backwards, so trailing signature data is allowed), and
/// the central directory's signature must be where it says. Zip64
/// archives are not read. `None` when the file does not end in a zip
/// archive.
pub fn zip_tail(path: &Path) -> Option<ZipTail> {
    let (mut file, size) = open_regular(path)?;
    let window = size.min(ZIP_TAIL_WINDOW);
    let base = size - window;
    let tail = read_range(&mut file, size, base, window)?;
    let mut limit = tail.len();
    while let Some(at) = rfind_bytes(tail.get(..limit)?, b"PK\x05\x06") {
        if let Some(found) = end_record_at(&mut file, size, &tail, base, at) {
            return Some(found);
        }
        limit = at;
    }
    None
}

/// One member of a central directory.
struct CentralEntry {
    name: String,
    flags: u16,
    method: u16,
    size: u64,
    local: u64,
}

fn central_entries(path: &Path, tail: &ZipTail, max_entries: usize) -> Option<Vec<CentralEntry>> {
    if tail.directory_size > ZIP_DIRECTORY_MAX {
        return None;
    }
    let directory = read_at(path, tail.directory, tail.directory_size)?;
    let wanted = usize::try_from(tail.entries)
        .unwrap_or(usize::MAX)
        .min(max_entries);
    let mut entries = Vec::new();
    let mut position = 0usize;
    while entries.len() < wanted {
        let Some(fixed) = directory.get(position..position.saturating_add(46)) else {
            break;
        };
        if !fixed.starts_with(b"PK\x01\x02") {
            break;
        }
        let flags = u16_le(fixed, 8)?;
        let method = u16_le(fixed, 10)?;
        let size = u64::from(u32_le(fixed, 24)?);
        let name_len = usize::from(u16_le(fixed, 28)?);
        let extra_len = usize::from(u16_le(fixed, 30)?);
        let comment_len = usize::from(u16_le(fixed, 32)?);
        let local = u64::from(u32_le(fixed, 42)?);
        let name_start = position + 46;
        let name_end = name_start + name_len;
        let Some(raw_name) = directory.get(name_start..name_end) else {
            break;
        };
        if name_len <= ZIP_NAME_MAX {
            entries.push(CentralEntry {
                name: String::from_utf8_lossy(raw_name).into_owned(),
                flags,
                method,
                size,
                local,
            });
        }
        position = name_end + extra_len + comment_len;
    }
    Some(entries)
}

/// The member names of a zip archive, read from its central directory (no
/// member is decompressed). The directory must be at most 16 MiB; names
/// longer than 4096 bytes are skipped; at most `max_entries` names are
/// returned. Reading stops at the first malformed record.
pub fn zip_names(path: &Path, tail: &ZipTail, max_entries: usize) -> Option<Vec<String>> {
    let entries = central_entries(path, tail, max_entries)?;
    Some(entries.into_iter().map(|entry| entry.name).collect())
}

/// The bytes of a member stored without compression (method 0), at most
/// `limit` of them. `None` when the member is missing, encrypted or
/// compressed: there is no inflate here, so a deflated `conf.lua` or
/// `MANIFEST.MF` cannot be read, and callers must say so rather than
/// guess.
pub fn zip_read_stored(path: &Path, tail: &ZipTail, name: &str, limit: u64) -> Option<Vec<u8>> {
    let entries = central_entries(path, tail, ZIP_LOOKUP_ENTRIES)?;
    let entry = entries.into_iter().find(|entry| entry.name == name)?;
    if entry.method != 0 || entry.flags & 1 != 0 {
        return None;
    }
    let (mut file, size) = open_regular(path)?;
    let local = tail.start.checked_add(entry.local)?;
    let header = read_range(&mut file, size, local, 30)?;
    if !header.starts_with(b"PK\x03\x04") {
        return None;
    }
    let name_len = u64::from(u16_le(&header, 26)?);
    let extra_len = u64::from(u16_le(&header, 28)?);
    let data = local.checked_add(30 + name_len + extra_len)?;
    let want = entry.size.min(limit);
    let bytes = read_range(&mut file, size, data, want)?;
    (u64::try_from(bytes.len()).ok()? == want).then_some(bytes)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A directory under the system's temporary directory, removed when
    /// dropped. Its name holds the process id and a counter, so tests
    /// running in parallel never share one.
    pub(crate) struct Scratch {
        pub(crate) path: PathBuf,
    }

    impl Scratch {
        pub(crate) fn new(tag: &str) -> Scratch {
            let number = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "tur-rehost-{tag}-{}-{number}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("scratch directory");
            Scratch { path }
        }

        /// Writes a file below the scratch directory, creating parents.
        pub(crate) fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
            let path = self.path.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent directory");
            }
            fs::write(&path, data).expect("write fixture");
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    /// A minimal PE image: DOS header, PE signature, a COFF header with no
    /// sections and no optional header, then `extra`, padded with zeros
    /// to `size` bytes.
    pub(crate) fn pe_image(machine: u16, extra: &[u8], size: usize) -> Vec<u8> {
        let mut image = vec![0u8; 0x40];
        image[0] = b'M';
        image[1] = b'Z';
        image[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        image.extend_from_slice(b"PE\0\0");
        image.extend_from_slice(&machine.to_le_bytes());
        image.extend_from_slice(&[0u8; 18]);
        image.extend_from_slice(extra);
        if image.len() < size {
            image.resize(size, 0);
        }
        image
    }

    /// A thin 64-bit little-endian Mach-O head for `cpu`, padded to `size`.
    pub(crate) fn macho_image(cpu: u32, size: usize) -> Vec<u8> {
        let mut image = vec![0xcf, 0xfa, 0xed, 0xfe];
        image.extend_from_slice(&cpu.to_le_bytes());
        image.resize(size.max(32), 0);
        image
    }

    /// A zip archive with every member stored, offsets relative to its
    /// own start (CRCs are left at zero: nothing here checks them).
    pub(crate) fn stored_zip(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut body = Vec::new();
        let mut directory = Vec::new();
        for (name, data) in members {
            let offset = u32::try_from(body.len()).expect("small archive");
            let size = u32::try_from(data.len()).expect("small member");
            let name_len = u16::try_from(name.len()).expect("short name");
            body.extend_from_slice(b"PK\x03\x04");
            body.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            body.extend_from_slice(&[0, 0, 0, 0]);
            body.extend_from_slice(&size.to_le_bytes());
            body.extend_from_slice(&size.to_le_bytes());
            body.extend_from_slice(&name_len.to_le_bytes());
            body.extend_from_slice(&[0, 0]);
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(data);

            directory.extend_from_slice(b"PK\x01\x02");
            directory.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            directory.extend_from_slice(&[0, 0, 0, 0]);
            directory.extend_from_slice(&size.to_le_bytes());
            directory.extend_from_slice(&size.to_le_bytes());
            directory.extend_from_slice(&name_len.to_le_bytes());
            directory.extend_from_slice(&[0u8; 12]);
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let count = u16::try_from(members.len()).expect("few members");
        let directory_offset = u32::try_from(body.len()).expect("small archive");
        let directory_size = u32::try_from(directory.len()).expect("small directory");
        let mut archive = body;
        archive.extend_from_slice(&directory);
        archive.extend_from_slice(b"PK\x05\x06");
        archive.extend_from_slice(&[0, 0, 0, 0]);
        archive.extend_from_slice(&count.to_le_bytes());
        archive.extend_from_slice(&count.to_le_bytes());
        archive.extend_from_slice(&directory_size.to_le_bytes());
        archive.extend_from_slice(&directory_offset.to_le_bytes());
        archive.extend_from_slice(&[0, 0]);
        archive
    }

    fn archs(kind: &BinaryKind) -> Vec<&str> {
        kind.archs.iter().map(String::as_str).collect()
    }

    #[test]
    fn pe_heads_name_their_machine_or_a_question_mark() {
        let kind = binary_kind_of_head(&pe_image(0x8664, &[], 128)).expect("pe");
        assert_eq!(kind.format, BinaryFormat::Pe);
        assert_eq!(archs(&kind), ["x86_64"]);
        let kind = binary_kind_of_head(&pe_image(0xaa64, &[], 128)).expect("pe");
        assert_eq!(archs(&kind), ["arm64"]);
        let mut no_header = vec![0u8; 64];
        no_header[0] = b'M';
        no_header[1] = b'Z';
        no_header[0x3c..0x40].copy_from_slice(&0x7fff_fff0u32.to_le_bytes());
        let kind = binary_kind_of_head(&no_header).expect("pe without header");
        assert_eq!(archs(&kind), ["?"]);
        assert_eq!(binary_kind_of_head(b"MZ"), None);
    }

    #[test]
    fn elf_heads_respect_their_byte_order() {
        let mut little = vec![0u8; 64];
        little[..4].copy_from_slice(b"\x7fELF");
        little[4] = 2;
        little[5] = 1;
        little[18..20].copy_from_slice(&62u16.to_le_bytes());
        let kind = binary_kind_of_head(&little).expect("elf");
        assert_eq!(kind.format, BinaryFormat::Elf);
        assert_eq!(archs(&kind), ["x86_64"]);
        assert!(kind.is_linux_x86_64());

        let mut big = little.clone();
        big[5] = 2;
        big[18..20].copy_from_slice(&40u16.to_be_bytes());
        assert_eq!(archs(&binary_kind_of_head(&big).expect("elf")), ["arm"]);

        little[18..20].copy_from_slice(&243u16.to_le_bytes());
        assert_eq!(archs(&binary_kind_of_head(&little).expect("elf")), ["riscv"]);
    }

    #[test]
    fn thin_mach_o_heads_in_both_byte_orders() {
        let kind = binary_kind_of_head(&macho_image(0x0100_000c, 64)).expect("mach-o");
        assert_eq!(kind.format, BinaryFormat::MachO);
        assert_eq!(archs(&kind), ["arm64"]);

        let mut ppc = vec![0xfe, 0xed, 0xfa, 0xce];
        ppc.extend_from_slice(&18u32.to_be_bytes());
        ppc.resize(32, 0);
        assert_eq!(archs(&binary_kind_of_head(&ppc).expect("mach-o")), ["ppc"]);

        let mut x86 = vec![0xce, 0xfa, 0xed, 0xfe];
        x86.extend_from_slice(&7u32.to_le_bytes());
        x86.resize(32, 0);
        assert_eq!(archs(&binary_kind_of_head(&x86).expect("mach-o")), ["x86"]);
    }

    #[test]
    fn universal_heads_list_every_architecture_and_java_classes_are_refused() {
        let mut fat = vec![0xca, 0xfe, 0xba, 0xbe];
        fat.extend_from_slice(&2u32.to_be_bytes());
        for cpu in [0x0100_0007u32, 0x0100_000c] {
            fat.extend_from_slice(&cpu.to_be_bytes());
            fat.extend_from_slice(&[0u8; 16]);
        }
        let kind = binary_kind_of_head(&fat).expect("universal");
        assert_eq!(kind.format, BinaryFormat::MachO);
        assert_eq!(archs(&kind), ["x86_64", "arm64"]);

        let mut wide = vec![0xca, 0xfe, 0xba, 0xbf];
        wide.extend_from_slice(&2u32.to_be_bytes());
        for cpu in [0x0100_000cu32, 0x0100_0007] {
            wide.extend_from_slice(&cpu.to_be_bytes());
            wide.extend_from_slice(&[0u8; 28]);
        }
        assert_eq!(
            archs(&binary_kind_of_head(&wide).expect("universal")),
            ["arm64", "x86_64"]
        );

        // A Java class file: minor version 0, major 52 (Java 8).
        let mut class = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 52];
        class.resize(64, 0);
        assert_eq!(binary_kind_of_head(&class), None);

        // A count the head cannot hold lists only what is there.
        let mut short = vec![0xca, 0xfe, 0xba, 0xbe];
        short.extend_from_slice(&40u32.to_be_bytes());
        short.extend_from_slice(&0x0100_0007u32.to_be_bytes());
        short.resize(24, 0);
        assert_eq!(archs(&binary_kind_of_head(&short).expect("universal")), ["x86_64"]);
    }

    #[test]
    fn unknown_and_short_heads_are_not_programs() {
        assert_eq!(binary_kind_of_head(&[0u8; 64]), None);
        assert_eq!(binary_kind_of_head(b"\x7fELF"), None);
        assert_eq!(binary_kind_of_head(&[]), None);
    }

    #[test]
    fn binary_kind_reads_regular_files_and_never_follows_links() {
        let scratch = Scratch::new("sniff-kind");
        let program = scratch.write("a.exe", &pe_image(0x14c, &[], 8192));
        assert_eq!(
            binary_kind(&program).map(|kind| kind.archs),
            Some(vec!["x86".to_owned()])
        );
        let link = scratch.path.join("link.exe");
        std::os::unix::fs::symlink(&program, &link).expect("symlink");
        assert_eq!(binary_kind(&link), None);
        assert_eq!(binary_kind(&scratch.path), None);
    }

    #[test]
    fn containers_are_told_apart_by_their_content() {
        let scratch = Scratch::new("sniff-container");
        let mut tar = vec![0u8; 600];
        tar[257..262].copy_from_slice(b"ustar");
        let mut dmg = vec![0u8; 2048];
        dmg[2048 - 512..2048 - 508].copy_from_slice(b"koly");
        let mut appimage = vec![0u8; 64];
        appimage[..4].copy_from_slice(b"\x7fELF");
        appimage[8..11].copy_from_slice(b"AI\x02");
        let cases: [(&str, Vec<u8>, Container); 9] = [
            ("a.zip", stored_zip(&[("x", &b"y"[..])]), Container::Zip),
            ("a.7z", b"7z\xbc\xaf\x27\x1c rest".to_vec(), Container::SevenZip),
            ("a.msi", b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1....".to_vec(), Container::Ole),
            ("a.tar", tar, Container::Tar),
            ("a.dmg", dmg, Container::Dmg),
            ("a.AppImage", appimage, Container::AppImage),
            ("a.exe", pe_image(0x8664, &[], 256), Container::Pe),
            ("a.bin", macho_image(0x0100_0007, 64), Container::MachO),
            ("a.txt", b"hello there".to_vec(), Container::Other),
        ];
        for (name, data, expected) in cases {
            let path = scratch.write(name, &data);
            assert_eq!(sniff(&path).expect("sniff"), expected, "{name}");
        }
        assert!(sniff(&scratch.path).is_err());
        assert_eq!(
            serde_json::to_string(&Container::SevenZip).expect("json"),
            "\"7z\""
        );
        assert_eq!(
            serde_json::to_string(&Container::TarCompressed).expect("json"),
            "\"tar-compressed\""
        );
        assert_eq!(Container::MachO.as_str(), "mach-o");
    }

    #[test]
    fn pe_info_reads_subsystem_clr_dll_and_overlay() {
        let mut image = vec![0u8; 0x40];
        image[0] = b'M';
        image[1] = b'Z';
        image[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        image.extend_from_slice(b"PE\0\0");
        image.extend_from_slice(&0x8664u16.to_le_bytes());
        image.extend_from_slice(&1u16.to_le_bytes());
        image.extend_from_slice(&[0u8; 12]);
        image.extend_from_slice(&240u16.to_le_bytes());
        image.extend_from_slice(&0x2022u16.to_le_bytes());
        let mut optional = vec![0u8; 240];
        optional[..2].copy_from_slice(&0x20bu16.to_le_bytes());
        optional[68..70].copy_from_slice(&2u16.to_le_bytes());
        optional[224..228].copy_from_slice(&0x2000u32.to_le_bytes());
        optional[228..232].copy_from_slice(&0x48u32.to_le_bytes());
        image.extend_from_slice(&optional);
        let mut section = vec![0u8; 40];
        section[..5].copy_from_slice(b".text");
        section[16..20].copy_from_slice(&0x200u32.to_le_bytes());
        section[20..24].copy_from_slice(&0x200u32.to_le_bytes());
        image.extend_from_slice(&section);
        image.resize(0x600, 0);

        let scratch = Scratch::new("sniff-pe");
        let path = scratch.write("lib.dll", &image);
        let info = pe_info(&path).expect("pe info");
        assert_eq!(info.machine, "x86_64");
        assert_eq!(info.subsystem, 2);
        assert!(info.clr);
        assert!(info.is_dll);
        assert_eq!(info.overlay_offset, 0x400);
        assert_eq!(info.overlay_size, 0x200);
        assert_eq!(info.size, 0x600);

        let plain = scratch.write("plain.exe", &pe_image(0x14c, &[], 4096));
        let info = pe_info(&plain).expect("pe info");
        assert!(!info.clr);
        assert!(!info.is_dll);
        assert_eq!(info.subsystem, 0);
        let text = scratch.write("text.exe", b"MZ but nothing else at all, really");
        assert_eq!(pe_info(&text), None);
    }

    #[test]
    fn a_zip_appended_to_a_program_is_found_despite_trailing_bytes() {
        let program = pe_image(0x8664, &[], 5000);
        let members: [(&str, &[u8]); 2] = [
            ("main.lua", b"print(1)"),
            ("conf.lua", b"t.version = \"11.5\""),
        ];
        let archive = stored_zip(&members);
        let mut file = program.clone();
        file.extend_from_slice(&archive);
        file.extend_from_slice(&[0x30u8; 300]);
        let scratch = Scratch::new("sniff-zip");
        let path = scratch.write("game.exe", &file);
        let tail = zip_tail(&path).expect("zip tail");
        assert_eq!(tail.start, 5000);
        assert_eq!(tail.end, 5000 + u64::try_from(archive.len()).expect("len"));
        assert_eq!(tail.entries, 2);
        assert_eq!(
            zip_names(&path, &tail, 100).expect("names"),
            ["main.lua", "conf.lua"]
        );
        assert_eq!(zip_names(&path, &tail, 1).expect("names"), ["main.lua"]);
        assert_eq!(
            zip_read_stored(&path, &tail, "main.lua", 1024).as_deref(),
            Some(&b"print(1)"[..])
        );
        assert_eq!(
            zip_read_stored(&path, &tail, "main.lua", 5).as_deref(),
            Some(&b"print"[..])
        );
        assert_eq!(zip_read_stored(&path, &tail, "missing.lua", 1024), None);

        let plain = scratch.write("plain.bin", &program);
        assert_eq!(zip_tail(&plain), None);
    }

    #[test]
    fn a_compressed_member_is_not_read() {
        let mut archive = stored_zip(&[("conf.lua", &b"x"[..])]);
        // Mark the member deflated in its central directory record.
        let directory = rfind_bytes(&archive, b"PK\x01\x02").expect("directory");
        archive[directory + 10] = 8;
        let scratch = Scratch::new("sniff-deflated");
        let path = scratch.write("a.love", &archive);
        let tail = zip_tail(&path).expect("zip tail");
        assert_eq!(zip_read_stored(&path, &tail, "conf.lua", 1024), None);
    }

    #[test]
    fn byte_search_finds_first_and_last() {
        assert_eq!(find_bytes(b"abcabc", b"bc", 0), Some(1));
        assert_eq!(find_bytes(b"abcabc", b"bc", 2), Some(4));
        assert_eq!(find_bytes(b"abcabc", b"bd", 0), None);
        assert_eq!(find_bytes(b"ab", b"abc", 0), None);
        assert_eq!(find_bytes(b"abc", b"", 0), None);
        assert_eq!(rfind_bytes(b"abcabc", b"abc"), Some(3));
        assert_eq!(rfind_bytes(b"abc", b"x"), None);
    }
}
