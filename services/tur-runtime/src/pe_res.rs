// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The parts of a PE image needed to tell installers apart and to describe
//! an installed program: where the image ends and appended data begins, and
//! the resource tree with its version information.
//!
//! [`crate::pe`] reads the headers that decide which runtime a file needs.
//! This module reads further, and only for files the user asked to install
//! or that an installer put into a prefix. Both are untrusted: every offset
//! is checked against the file, every count is capped, and the resource tree
//! is walked to its fixed depth of three and never deeper, so no cycle in it
//! can be followed.

use std::fs::{self, File};
use std::path::Path;

use crate::arch::Endianness;
use crate::error::{FormatFamily, InspectError};
use crate::reader::{Reader, u16_at, u32_at};

const LITTLE: Endianness = Endianness::Little;

/// Resource type of one icon image.
pub(crate) const RT_ICON: u32 = 3;
/// Resource type of an icon group: the directory of an icon's images.
pub(crate) const RT_GROUP_ICON: u32 = 14;
/// Resource type of the version information.
const RT_VERSION: u32 = 16;

/// Sections accepted in a section table. The format allows 96.
const MAX_SECTIONS: u16 = 96;
/// Entries accepted in one resource directory.
const MAX_DIRECTORY_ENTRIES: usize = 4096;
/// Bytes of version information read.
const MAX_VERSION_BYTES: usize = 64 * 1024;
/// UTF-16 units kept from one version string.
const MAX_VERSION_STRING_UNITS: usize = 256;
/// The bit that marks a resource directory entry as a subdirectory, and a
/// resource name as a string.
const HIGH_BIT: u32 = 0x8000_0000;

fn truncated(detail: &'static str) -> InspectError {
    InspectError::Truncated {
        format: FormatFamily::Pe,
        detail,
    }
}

fn malformed(detail: &str) -> InspectError {
    InspectError::Malformed {
        format: FormatFamily::Pe,
        detail: detail.to_owned(),
    }
}

/// Opens a regular file for bounded reading.
///
/// # Errors
///
/// [`InspectError::Io`] when the path is not a regular file or cannot be
/// opened.
pub(crate) fn open(path: &Path) -> Result<Reader<'static>, InspectError> {
    let metadata = fs::metadata(path).map_err(|error| InspectError::io(path, &error))?;
    if !metadata.is_file() {
        return Err(InspectError::Io {
            path: path.display().to_string(),
            message: "it is not a regular file".to_owned(),
        });
    }
    let file = File::open(path).map_err(|error| InspectError::io(path, &error))?;
    Ok(Reader::from_file(file, metadata.len()))
}

/// The position of the first occurrence of `needle` in `haystack`.
pub(crate) fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// `text` as UTF-16, little-endian, without a terminator.
pub(crate) fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[derive(Debug, Clone, Copy)]
struct Section {
    address: u32,
    virtual_size: u32,
    raw_offset: u32,
    raw_size: u32,
}

/// Where the parts of a PE image are in its file.
#[derive(Debug, Clone)]
pub(crate) struct PeLayout {
    sections: Vec<Section>,
    /// The address of the resource directory, when the image has one.
    resources: Option<u32>,
    /// The offset at which the image's sections end. What follows is data
    /// appended to the program: an installer's payload, a signature.
    pub(crate) overlay_offset: u64,
}

impl PeLayout {
    /// The file offset of a virtual address, when a section holds it on
    /// disk.
    fn file_offset(&self, address: u32) -> Option<u64> {
        self.sections.iter().find_map(|section| {
            let within = address.checked_sub(section.address)?;
            let span = section.virtual_size.max(section.raw_size);
            (within < span && within < section.raw_size)
                .then(|| u64::from(section.raw_offset) + u64::from(within))
        })
    }
}

/// Reads the layout of a PE image. Returns `Ok(None)` when the input is not
/// one.
///
/// # Errors
///
/// [`InspectError`] when the headers are cut short or hold impossible
/// values, or a read limit is hit.
pub(crate) fn layout(reader: &mut Reader<'_>) -> Result<Option<PeLayout>, InspectError> {
    let head = reader.read_up_to(0, 64)?;
    if !head.starts_with(b"MZ") {
        return Ok(None);
    }
    let Some(lfanew) = u32_at(&head, 0x3c, LITTLE) else {
        return Ok(None);
    };
    let lfanew = u64::from(lfanew);
    let Some(file_header) = reader.read(lfanew, 24)? else {
        return Ok(None);
    };
    if !file_header.starts_with(b"PE\0\0") {
        return Ok(None);
    }
    let section_count = u16_at(&file_header, 6, LITTLE).unwrap_or(0);
    let optional_len = u16_at(&file_header, 20, LITTLE).unwrap_or(0);
    if section_count > MAX_SECTIONS {
        return Err(malformed("the section table claims more than 96 sections"));
    }
    let optional_at = lfanew + 24;
    let optional = reader
        .read(optional_at, usize::from(optional_len))?
        .ok_or_else(|| truncated("the optional header"))?;
    // The data directories start 96 bytes into a PE32 optional header and
    // 112 bytes into a PE32+ one; their count is the field before them, and
    // the resource directory is the third.
    let table_at: Option<usize> = match u16_at(&optional, 0, LITTLE) {
        Some(0x10b) => Some(96),
        Some(0x20b) => Some(112),
        _ => None,
    };
    let resources = table_at.and_then(|table_at| {
        let count = u32_at(&optional, table_at - 4, LITTLE)?;
        if count <= 2 {
            return None;
        }
        let address = u32_at(&optional, table_at + 16, LITTLE)?;
        let size = u32_at(&optional, table_at + 20, LITTLE)?;
        (address != 0 && size != 0).then_some(address)
    });

    let sections_at = optional_at + u64::from(optional_len);
    let table = reader
        .read(sections_at, usize::from(section_count) * 40)?
        .ok_or_else(|| truncated("the section table"))?;
    let mut sections = Vec::new();
    let mut overlay_offset: u64 = 0;
    for index in 0..usize::from(section_count) {
        let at = index * 40;
        let fields = u32_at(&table, at + 8, LITTLE)
            .zip(u32_at(&table, at + 12, LITTLE))
            .zip(u32_at(&table, at + 16, LITTLE))
            .zip(u32_at(&table, at + 20, LITTLE));
        let Some((((virtual_size, address), raw_size), raw_offset)) = fields else {
            return Err(truncated("the section table"));
        };
        if raw_size != 0 {
            overlay_offset = overlay_offset.max(u64::from(raw_offset) + u64::from(raw_size));
        }
        sections.push(Section {
            address,
            virtual_size,
            raw_offset,
            raw_size,
        });
    }
    Ok(Some(PeLayout {
        sections,
        resources,
        overlay_offset: overlay_offset.min(reader.size()),
    }))
}

struct Entry {
    /// The numeric identifier, or `None` when the entry is named by a string.
    id: Option<u32>,
    target: u32,
}

impl Entry {
    /// The offset of the subdirectory the entry leads to, when it leads to
    /// one.
    fn subdirectory(&self) -> Option<u32> {
        (self.target & HIGH_BIT != 0).then_some(self.target & !HIGH_BIT)
    }
}

/// Reads one resource directory. Offsets are relative to `base`, the start
/// of the resource section in the file.
fn directory(reader: &mut Reader<'_>, base: u64, offset: u32) -> Result<Vec<Entry>, InspectError> {
    let at = base + u64::from(offset);
    let header = reader
        .read(at, 16)?
        .ok_or_else(|| truncated("a resource directory"))?;
    let named = u16_at(&header, 12, LITTLE).unwrap_or(0);
    let numbered = u16_at(&header, 14, LITTLE).unwrap_or(0);
    let count = usize::from(named) + usize::from(numbered);
    if count > MAX_DIRECTORY_ENTRIES {
        return Err(malformed("a resource directory claims too many entries"));
    }
    let entries = reader
        .read(at + 16, count * 8)?
        .ok_or_else(|| truncated("a resource directory"))?;
    let mut found = Vec::with_capacity(count);
    for index in 0..count {
        let fields = u32_at(&entries, index * 8, LITTLE).zip(u32_at(&entries, index * 8 + 4, LITTLE));
        let Some((name, target)) = fields else {
            return Err(truncated("a resource directory"));
        };
        found.push(Entry {
            id: (name & HIGH_BIT == 0).then_some(name),
            target,
        });
    }
    Ok(found)
}

/// The bytes of one resource: of type `kind`, with the identifier `wanted`,
/// or the first of its type when `wanted` is `None`; in the first language
/// listed. Returns `Ok(None)` when there is no such resource or it is larger
/// than `max` bytes.
///
/// # Errors
///
/// [`InspectError`] when the resource tree is cut short or a read limit is
/// hit.
pub(crate) fn resource(
    reader: &mut Reader<'_>,
    layout: &PeLayout,
    kind: u32,
    wanted: Option<u32>,
    max: usize,
) -> Result<Option<Vec<u8>>, InspectError> {
    let Some(base) = layout
        .resources
        .and_then(|address| layout.file_offset(address))
    else {
        return Ok(None);
    };
    let kinds = directory(reader, base, 0)?;
    let Some(kind_at) = kinds
        .iter()
        .find(|entry| entry.id == Some(kind))
        .and_then(Entry::subdirectory)
    else {
        return Ok(None);
    };
    let names = directory(reader, base, kind_at)?;
    let chosen = match wanted {
        Some(id) => names.iter().find(|entry| entry.id == Some(id)),
        None => names.first(),
    };
    let Some(name_at) = chosen.and_then(Entry::subdirectory) else {
        return Ok(None);
    };
    let languages = directory(reader, base, name_at)?;
    let Some(leaf) = languages
        .first()
        .filter(|entry| entry.subdirectory().is_none())
    else {
        return Ok(None);
    };
    let Some(data) = reader.read(base + u64::from(leaf.target), 16)? else {
        return Ok(None);
    };
    let Some((address, size)) = u32_at(&data, 0, LITTLE).zip(u32_at(&data, 4, LITTLE)) else {
        return Ok(None);
    };
    let Ok(size) = usize::try_from(size) else {
        return Ok(None);
    };
    if size > max {
        return Ok(None);
    }
    let Some(offset) = layout.file_offset(address) else {
        return Ok(None);
    };
    reader.read(offset, size)
}

/// What a program's version resource says about it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionInfo {
    /// `ProductName`.
    pub product_name: Option<String>,
    /// `CompanyName`.
    pub company_name: Option<String>,
    /// `FileDescription`.
    pub file_description: Option<String>,
    /// `ProductVersion`, or the product version of the fixed block.
    pub product_version: Option<String>,
    /// `FileVersion`, or the file version of the fixed block.
    pub file_version: Option<String>,
}

/// One string of a version resource.
///
/// A string is stored as a small header (total length, value length in
/// UTF-16 units, type), the key as NUL-terminated UTF-16, padding to a
/// 32-bit boundary, and the value.
fn version_string(blob: &[u8], key: &str) -> Option<String> {
    let mut pattern = utf16le(key);
    pattern.extend_from_slice(&[0, 0]);
    let at = find(blob, &pattern)?;
    let value_units = usize::from(u16_at(blob, at.checked_sub(4)?, LITTLE)?);
    if value_units == 0 {
        return None;
    }
    let start = (at + pattern.len()).div_ceil(4) * 4;
    let units: Vec<u16> = blob
        .get(start..)?
        .chunks_exact(2)
        .take(value_units.min(MAX_VERSION_STRING_UNITS))
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    let text: String = String::from_utf16_lossy(&units)
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The file and product versions of the fixed block, as dotted numbers.
fn fixed_versions(blob: &[u8]) -> Option<(String, String)> {
    let at = find(blob, &[0xbd, 0x04, 0xef, 0xfe])?;
    let dotted = |offset: usize| -> Option<String> {
        let high = u32_at(blob, at + offset, LITTLE)?;
        let low = u32_at(blob, at + offset + 4, LITTLE)?;
        Some(format!(
            "{}.{}.{}.{}",
            high >> 16,
            high & 0xffff,
            low >> 16,
            low & 0xffff
        ))
    };
    Some((dotted(8)?, dotted(16)?))
}

/// Reads what a version resource holds.
pub(crate) fn parse_version(blob: &[u8]) -> VersionInfo {
    let (fixed_file, fixed_product) = match fixed_versions(blob) {
        Some((file, product)) => (Some(file), Some(product)),
        None => (None, None),
    };
    VersionInfo {
        product_name: version_string(blob, "ProductName"),
        company_name: version_string(blob, "CompanyName"),
        file_description: version_string(blob, "FileDescription"),
        product_version: version_string(blob, "ProductVersion").or(fixed_product),
        file_version: version_string(blob, "FileVersion").or(fixed_file),
    }
}

/// The version information of the PE image at `path`. `None` when the file
/// is not a PE image, has no version resource, or cannot be read: a program
/// without one is ordinary, and nothing depends on it.
pub fn version_info(path: &Path) -> Option<VersionInfo> {
    let mut reader = open(path).ok()?;
    let found = layout(&mut reader).ok()??;
    let blob = resource(&mut reader, &found, RT_VERSION, None, MAX_VERSION_BYTES).ok()??;
    Some(parse_version(&blob))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testdata::TempDir;

    /// One resource of a test image: type, identifier and bytes.
    pub(crate) type TestResource = (u32, u32, Vec<u8>);

    fn put16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn put32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    /// A resource section holding `resources`, as it would be mapped at
    /// `address`. Types and identifiers must be given in ascending order.
    fn resource_section(resources: &[TestResource], address: u32) -> Vec<u8> {
        let mut kinds: Vec<u32> = resources.iter().map(|(kind, _, _)| *kind).collect();
        kinds.dedup();
        // Sizes: the root, one directory per type, one per resource (its
        // language level), then the data entries, then the data.
        let root_len = 16 + 8 * kinds.len();
        let kind_dirs_len: usize = kinds
            .iter()
            .map(|kind| 16 + 8 * resources.iter().filter(|(k, _, _)| k == kind).count())
            .sum();
        let language_dirs_at = root_len + kind_dirs_len;
        let data_entries_at = language_dirs_at + 24 * resources.len();
        let data_at = data_entries_at + 16 * resources.len();

        let mut out = Vec::new();
        // Root directory.
        out.extend_from_slice(&[0u8; 12]);
        put16(&mut out, 0);
        put16(&mut out, u16::try_from(kinds.len()).unwrap());
        let mut kind_dir_at = root_len;
        for kind in &kinds {
            put32(&mut out, *kind);
            put32(&mut out, HIGH_BIT | u32::try_from(kind_dir_at).unwrap());
            kind_dir_at += 16 + 8 * resources.iter().filter(|(k, _, _)| k == kind).count();
        }
        // One directory per type, listing its resources.
        for kind in &kinds {
            let members: Vec<usize> = (0..resources.len())
                .filter(|index| resources[*index].0 == *kind)
                .collect();
            out.extend_from_slice(&[0u8; 12]);
            put16(&mut out, 0);
            put16(&mut out, u16::try_from(members.len()).unwrap());
            for index in members {
                put32(&mut out, resources[index].1);
                put32(
                    &mut out,
                    HIGH_BIT | u32::try_from(language_dirs_at + 24 * index).unwrap(),
                );
            }
        }
        // One language directory per resource, with one entry.
        for index in 0..resources.len() {
            out.extend_from_slice(&[0u8; 12]);
            put16(&mut out, 0);
            put16(&mut out, 1);
            put32(&mut out, 1033);
            put32(&mut out, u32::try_from(data_entries_at + 16 * index).unwrap());
        }
        // Data entries, then the data.
        let mut offset = data_at;
        for (_, _, bytes) in resources {
            put32(&mut out, address + u32::try_from(offset).unwrap());
            put32(&mut out, u32::try_from(bytes.len()).unwrap());
            put32(&mut out, 0);
            put32(&mut out, 0);
            offset += bytes.len();
        }
        for (_, _, bytes) in resources {
            out.extend_from_slice(bytes);
        }
        assert_eq!(out.len(), offset);
        out
    }

    /// A PE32+ image with one code section, a resource section holding
    /// `resources`, and `overlay` appended after the last section.
    pub(crate) fn image(machine: u16, resources: &[TestResource], overlay: &[u8]) -> Vec<u8> {
        const RESOURCE_ADDRESS: u32 = 0x2000;
        let plus = machine == 0x8664;
        let optional_len: u16 = if plus { 240 } else { 224 };
        let section = resource_section(resources, RESOURCE_ADDRESS);
        let headers_len = 0x80 + 24 + usize::from(optional_len) + 2 * 40;
        let text_at = headers_len.div_ceil(0x200) * 0x200;
        let text = vec![0xccu8; 0x200];
        let resources_at = text_at + text.len();
        let resources_len = section.len().div_ceil(0x200) * 0x200;

        let mut out = vec![0u8; 0x80];
        out[..2].copy_from_slice(b"MZ");
        out[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        out.extend_from_slice(b"PE\0\0");
        put16(&mut out, machine);
        put16(&mut out, 2); // NumberOfSections
        put32(&mut out, 0);
        put32(&mut out, 0);
        put32(&mut out, 0);
        put16(&mut out, optional_len);
        put16(&mut out, 0x0022);

        let mut optional = vec![0u8; usize::from(optional_len)];
        let magic: u16 = if plus { 0x20b } else { 0x10b };
        optional[..2].copy_from_slice(&magic.to_le_bytes());
        optional[68..70].copy_from_slice(&2u16.to_le_bytes());
        let table_at = if plus { 112 } else { 96 };
        optional[table_at - 4..table_at].copy_from_slice(&16u32.to_le_bytes());
        if !resources.is_empty() {
            optional[table_at + 16..table_at + 20].copy_from_slice(&RESOURCE_ADDRESS.to_le_bytes());
            optional[table_at + 20..table_at + 24]
                .copy_from_slice(&u32::try_from(section.len()).unwrap().to_le_bytes());
        }
        out.extend_from_slice(&optional);

        let mut section_header = |name: &[u8], address: u32, size: usize, at: usize, raw: usize| {
            let mut header = [0u8; 40];
            header[..name.len()].copy_from_slice(name);
            header[8..12].copy_from_slice(&u32::try_from(size).unwrap().to_le_bytes());
            header[12..16].copy_from_slice(&address.to_le_bytes());
            header[16..20].copy_from_slice(&u32::try_from(raw).unwrap().to_le_bytes());
            header[20..24].copy_from_slice(&u32::try_from(at).unwrap().to_le_bytes());
            out.extend_from_slice(&header);
        };
        section_header(b".text", 0x1000, text.len(), text_at, text.len());
        section_header(
            b".rsrc",
            RESOURCE_ADDRESS,
            section.len(),
            resources_at,
            resources_len,
        );
        out.resize(text_at, 0);
        out.extend_from_slice(&text);
        out.extend_from_slice(&section);
        out.resize(resources_at + resources_len, 0);
        out.extend_from_slice(overlay);
        out
    }

    /// A version resource with a fixed block and the given strings.
    pub(crate) fn version_resource(strings: &[(&str, &str)]) -> Vec<u8> {
        fn pad(out: &mut Vec<u8>) {
            while !out.len().is_multiple_of(4) {
                out.push(0);
            }
        }
        let mut out = Vec::new();
        // VS_VERSIONINFO header and key.
        put16(&mut out, 0);
        put16(&mut out, 52);
        put16(&mut out, 0);
        out.extend_from_slice(&utf16le("VS_VERSION_INFO"));
        put16(&mut out, 0);
        pad(&mut out);
        // VS_FIXEDFILEINFO: signature, structure version, file version
        // 1.2.3.4, product version 5.6.7.8.
        put32(&mut out, 0xfeef_04bd);
        put32(&mut out, 0x0001_0000);
        put32(&mut out, (1 << 16) | 2);
        put32(&mut out, (3 << 16) | 4);
        put32(&mut out, (5 << 16) | 6);
        put32(&mut out, (7 << 16) | 8);
        out.extend_from_slice(&[0u8; 28]);
        pad(&mut out);
        for (key, value) in strings {
            let value_units = if value.is_empty() {
                0
            } else {
                value.encode_utf16().count() + 1
            };
            put16(&mut out, 0);
            put16(&mut out, u16::try_from(value_units).unwrap());
            put16(&mut out, 1);
            out.extend_from_slice(&utf16le(key));
            put16(&mut out, 0);
            pad(&mut out);
            if value_units > 0 {
                out.extend_from_slice(&utf16le(value));
                put16(&mut out, 0);
            }
            pad(&mut out);
        }
        out
    }

    #[test]
    fn the_layout_finds_where_the_image_ends() {
        let bytes = image(0x8664, &[], b"appended");
        let mut reader = Reader::from_bytes(&bytes);
        let wide = layout(&mut reader).unwrap().unwrap();
        let overlay = usize::try_from(wide.overlay_offset).unwrap();
        assert_eq!(&bytes[overlay..], b"appended");
        // A 32-bit image is read the same way.
        let bytes = image(0x014c, &[], b"more");
        let mut reader = Reader::from_bytes(&bytes);
        let narrow = layout(&mut reader).unwrap().unwrap();
        let overlay = usize::try_from(narrow.overlay_offset).unwrap();
        assert_eq!(&bytes[overlay..], b"more");
    }

    #[test]
    fn what_is_not_a_pe_image_has_no_layout() {
        for bytes in [
            &b""[..],
            b"MZ",
            b"#!/bin/sh\n",
            &crate::testdata::mz_with(b"NE\0\0"),
        ] {
            let mut reader = Reader::from_bytes(bytes);
            assert!(layout(&mut reader).unwrap().is_none());
        }
    }

    #[test]
    fn resources_are_found_by_type_and_identifier() {
        let resources = vec![
            (RT_ICON, 1, b"first icon".to_vec()),
            (RT_ICON, 7, b"seventh icon".to_vec()),
            (RT_GROUP_ICON, 100, b"group".to_vec()),
        ];
        let bytes = image(0x8664, &resources, b"");
        let mut reader = Reader::from_bytes(&bytes);
        let found = layout(&mut reader).unwrap().unwrap();
        let mut get = |kind, wanted| resource(&mut reader, &found, kind, wanted, 1024).unwrap();
        assert_eq!(get(RT_ICON, Some(7)).unwrap(), b"seventh icon");
        assert_eq!(get(RT_ICON, Some(1)).unwrap(), b"first icon");
        assert_eq!(get(RT_ICON, None).unwrap(), b"first icon");
        assert_eq!(get(RT_GROUP_ICON, None).unwrap(), b"group");
        assert!(get(RT_ICON, Some(2)).is_none());
        assert!(get(RT_VERSION, None).is_none());
        // A resource over the size limit is not returned.
        assert!(
            resource(&mut reader, &found, RT_ICON, Some(7), 4)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_resource_tree_cut_short_is_an_error_not_a_panic() {
        let resources = vec![(RT_ICON, 1, vec![0u8; 64])];
        let whole = image(0x8664, &resources, b"");
        let mut reader = Reader::from_bytes(&whole);
        let full = layout(&mut reader).unwrap().unwrap();
        let resources_start = usize::try_from(full.file_offset(0x2000).unwrap()).unwrap();
        // Every length from the start of the resource section on: the
        // layout is read from the headers, which stay whole.
        for length in resources_start..whole.len() {
            let mut reader = Reader::from_bytes(&whole[..length]);
            let _ = resource(&mut reader, &full, RT_ICON, Some(1), 1024);
        }
        // A directory that claims more entries than are allowed.
        let mut hostile = whole.clone();
        hostile[resources_start + 14..resources_start + 16].copy_from_slice(&0xffffu16.to_le_bytes());
        let mut reader = Reader::from_bytes(&hostile);
        assert!(resource(&mut reader, &full, RT_ICON, Some(1), 1024).is_err());
    }

    #[test]
    fn version_strings_and_the_fixed_block_are_read() {
        let blob = version_resource(&[
            ("CompanyName", "Example Ltd"),
            ("FileDescription", "Example program"),
            ("FileVersion", "26.06"),
            ("LegalCopyright", ""),
            ("ProductName", "Example"),
        ]);
        let info = parse_version(&blob);
        assert_eq!(info.company_name.as_deref(), Some("Example Ltd"));
        assert_eq!(info.file_description.as_deref(), Some("Example program"));
        assert_eq!(info.product_name.as_deref(), Some("Example"));
        assert_eq!(info.file_version.as_deref(), Some("26.06"));
        // No ProductVersion string: the fixed block's number is used.
        assert_eq!(info.product_version.as_deref(), Some("5.6.7.8"));
        // An empty value is absent, not the bytes that follow it.
        assert_eq!(version_string(&blob, "LegalCopyright"), None);
        assert_eq!(version_string(&blob, "Comments"), None);
        assert_eq!(parse_version(b""), VersionInfo::default());
        // Control characters never reach a report.
        let hostile = version_resource(&[("ProductName", "Evil\u{1b}[31m")]);
        assert_eq!(
            parse_version(&hostile).product_name.as_deref(),
            Some("Evil[31m")
        );
    }

    #[test]
    fn version_information_is_read_from_a_file() {
        let temp = TempDir::new("pe-version");
        let path = temp.path().join("program.exe");
        let resources = vec![(
            RT_VERSION,
            1,
            version_resource(&[("ProductName", "Example"), ("ProductVersion", "2.5")]),
        )];
        std::fs::write(&path, image(0x8664, &resources, b"")).unwrap();
        let info = version_info(&path).unwrap();
        assert_eq!(info.product_name.as_deref(), Some("Example"));
        assert_eq!(info.product_version.as_deref(), Some("2.5"));
        assert_eq!(info.file_version.as_deref(), Some("1.2.3.4"));
        assert!(version_info(&temp.path().join("absent.exe")).is_none());
        std::fs::write(&path, b"not a program").unwrap();
        assert!(version_info(&path).is_none());
    }
}
