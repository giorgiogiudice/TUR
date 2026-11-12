// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! A zip central-directory reader that lists member names and nothing else.
//!
//! No member is ever decompressed, so there is no decompression bomb to
//! defend against. What remains attacker-controlled is the end-of-central-
//! directory record: the entry count and the directory's offset and size.
//! Each is checked against the file size and a hard cap before use.

use crate::arch::Endianness;
use crate::error::{FormatFamily, InspectError};
use crate::limits::{MAX_CENTRAL_DIRECTORY, MAX_ZIP_ENTRIES};
use crate::reader::{Reader, u16_at, u32_at, u64_at};

const LITTLE: Endianness = Endianness::Little;
/// Size of the end-of-central-directory record without its comment.
const EOCD_LEN: usize = 22;
/// The longest comment that can follow the end-of-central-directory record.
const MAX_COMMENT: usize = 0xffff;
/// Size of a central directory file header without its variable fields.
const ENTRY_LEN: usize = 46;

fn truncated(detail: &'static str) -> InspectError {
    InspectError::Truncated {
        format: FormatFamily::Zip,
        detail,
    }
}

fn malformed(detail: &str) -> InspectError {
    InspectError::Malformed {
        format: FormatFamily::Zip,
        detail: detail.to_owned(),
    }
}

fn limit(detail: String) -> InspectError {
    InspectError::LimitExceeded {
        format: FormatFamily::Zip,
        detail,
    }
}

/// Whether `head` starts like a zip archive (a local file header, or the
/// end-of-central-directory record of an empty archive).
pub(crate) fn is_zip(head: &[u8]) -> bool {
    head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06")
}

/// Calls `visit` with the raw name of every member listed in the central
/// directory and returns the number of members.
pub(crate) fn for_each_name(
    reader: &mut Reader<'_>,
    visit: &mut dyn FnMut(&[u8]),
) -> Result<u64, InspectError> {
    let size = reader.size();
    let tail_len = usize::try_from(size).map_or(EOCD_LEN + MAX_COMMENT, |fits| {
        fits.min(EOCD_LEN + MAX_COMMENT)
    });
    let tail_start = size.saturating_sub(u64::try_from(tail_len).unwrap_or(u64::MAX));
    let tail = reader
        .read(tail_start, tail_len)?
        .ok_or_else(|| truncated("the end-of-central-directory record"))?;
    let eocd_at =
        find_eocd(&tail).ok_or_else(|| truncated("the end-of-central-directory record"))?;
    let eocd = tail
        .get(eocd_at..)
        .ok_or_else(|| truncated("the end-of-central-directory record"))?;
    let fields = u16_at(eocd, 10, LITTLE)
        .zip(u32_at(eocd, 12, LITTLE))
        .zip(u32_at(eocd, 16, LITTLE));
    let Some(((entries, directory_size), directory_offset)) = fields else {
        return Err(truncated("the end-of-central-directory record"));
    };
    let mut entries = u64::from(entries);
    let mut directory_size = u64::from(directory_size);
    let mut directory_offset = u64::from(directory_offset);

    let uses_zip64 =
        entries == 0xffff || directory_size == 0xffff_ffff || directory_offset == 0xffff_ffff;
    if uses_zip64 {
        let eocd_position = tail_start.saturating_add(u64::try_from(eocd_at).unwrap_or(u64::MAX));
        if let Some(found) = zip64_record(reader, eocd_position)? {
            (entries, directory_size, directory_offset) = found;
        }
    }

    if entries > MAX_ZIP_ENTRIES {
        return Err(limit(format!(
            "the archive claims {entries} entries; at most {MAX_ZIP_ENTRIES} are read"
        )));
    }
    let directory_end = directory_offset
        .checked_add(directory_size)
        .ok_or_else(|| malformed("the central directory's offset and size overflow"))?;
    if directory_end > size {
        return Err(truncated("the central directory"));
    }
    if directory_size > MAX_CENTRAL_DIRECTORY {
        return Err(limit(format!(
            "the central directory is {directory_size} bytes; at most {MAX_CENTRAL_DIRECTORY} are read"
        )));
    }
    let directory_len = usize::try_from(directory_size)
        .map_err(|_| malformed("the central directory is too large"))?;
    let directory = reader
        .read(directory_offset, directory_len)?
        .ok_or_else(|| truncated("the central directory"))?;

    let mut at: usize = 0;
    for _ in 0..entries {
        let header = directory
            .get(at..)
            .filter(|rest| rest.len() >= ENTRY_LEN)
            .ok_or_else(|| {
                malformed("the central directory ends before its claimed entry count")
            })?;
        if !header.starts_with(b"PK\x01\x02") {
            return Err(malformed("a central directory entry has a bad signature"));
        }
        let lengths = u16_at(header, 28, LITTLE)
            .zip(u16_at(header, 30, LITTLE))
            .zip(u16_at(header, 32, LITTLE));
        let Some(((name_len, extra_len), comment_len)) = lengths else {
            return Err(malformed("a central directory entry is cut short"));
        };
        let name_end = ENTRY_LEN + usize::from(name_len);
        let name = header
            .get(ENTRY_LEN..name_end)
            .ok_or_else(|| malformed("an entry name runs past the central directory"))?;
        visit(name);
        at = at
            .saturating_add(name_end)
            .saturating_add(usize::from(extra_len))
            .saturating_add(usize::from(comment_len));
    }
    Ok(entries)
}

/// The position in `tail` of the last end-of-central-directory record whose
/// comment fits in what follows it.
fn find_eocd(tail: &[u8]) -> Option<usize> {
    let last = tail.len().checked_sub(EOCD_LEN)?;
    (0..=last).rev().find(|&at| {
        tail.get(at..).is_some_and(|rest| {
            rest.starts_with(b"PK\x05\x06")
                && u16_at(rest, 20, LITTLE)
                    .is_some_and(|comment| EOCD_LEN + usize::from(comment) <= rest.len())
        })
    })
}

/// Reads the zip64 end-of-central-directory record, if a zip64 locator sits
/// immediately before the classic record at `eocd_position`. Returns the
/// entry count and the central directory's size and offset.
fn zip64_record(
    reader: &mut Reader<'_>,
    eocd_position: u64,
) -> Result<Option<(u64, u64, u64)>, InspectError> {
    let Some(locator_at) = eocd_position.checked_sub(20) else {
        return Ok(None);
    };
    let Some(locator) = reader.read(locator_at, 20)? else {
        return Ok(None);
    };
    if !locator.starts_with(b"PK\x06\x07") {
        return Ok(None);
    }
    let record_at = u64_at(&locator, 8, LITTLE).ok_or_else(|| truncated("the zip64 locator"))?;
    let record = reader
        .read(record_at, 56)?
        .ok_or_else(|| truncated("the zip64 end-of-central-directory record"))?;
    if !record.starts_with(b"PK\x06\x06") {
        return Err(malformed(
            "the zip64 locator does not point at a zip64 record",
        ));
    }
    let fields = u64_at(&record, 32, LITTLE)
        .zip(u64_at(&record, 40, LITTLE))
        .zip(u64_at(&record, 48, LITTLE));
    let Some(((entries, directory_size), directory_offset)) = fields else {
        return Err(truncated("the zip64 end-of-central-directory record"));
    };
    Ok(Some((entries, directory_size, directory_offset)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::{zip, zip_with, zip64_claiming};

    fn names(bytes: &[u8]) -> Result<Vec<String>, InspectError> {
        let mut found = Vec::new();
        let mut reader = Reader::from_bytes(bytes);
        for_each_name(&mut reader, &mut |name| {
            found.push(String::from_utf8_lossy(name).into_owned());
        })?;
        Ok(found)
    }

    #[test]
    fn lists_member_names_in_directory_order() {
        let bytes = zip(&["AndroidManifest.xml", "classes.dex", "lib/x86_64/libfoo.so"]);
        assert!(is_zip(&bytes));
        assert_eq!(
            names(&bytes).unwrap(),
            ["AndroidManifest.xml", "classes.dex", "lib/x86_64/libfoo.so"]
        );
    }

    #[test]
    fn zip64_end_record_is_followed() {
        let bytes = zip_with(&["a.txt", "b.txt"], true);
        assert_eq!(names(&bytes).unwrap(), ["a.txt", "b.txt"]);
    }

    #[test]
    fn empty_archive_has_no_members() {
        let bytes = zip(&[]);
        assert!(is_zip(&bytes));
        assert_eq!(names(&bytes).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn archive_comment_is_skipped() {
        let mut bytes = zip(&["a.txt"]);
        let comment_len_at = bytes.len() - 2;
        bytes[comment_len_at..].copy_from_slice(&5u16.to_le_bytes());
        bytes.extend_from_slice(b"hello");
        assert_eq!(names(&bytes).unwrap(), ["a.txt"]);
    }

    #[test]
    fn missing_end_record_is_truncated() {
        let bytes = zip(&["a.txt"]);
        assert_eq!(
            names(&bytes[..bytes.len() - 1]),
            Err(truncated("the end-of-central-directory record"))
        );
        assert_eq!(
            names(b"PK\x03\x04"),
            Err(truncated("the end-of-central-directory record"))
        );
    }

    #[test]
    fn entry_count_larger_than_the_directory_is_malformed() {
        let mut bytes = zip(&["a.txt"]);
        // Total-entries field of the end record: 22 bytes from the end, + 10.
        let count_at = bytes.len() - EOCD_LEN + 10;
        bytes[count_at..count_at + 2].copy_from_slice(&0xfffeu16.to_le_bytes());
        assert!(matches!(
            names(&bytes),
            Err(InspectError::Malformed {
                format: FormatFamily::Zip,
                ..
            })
        ));
    }

    #[test]
    fn zip64_claiming_a_huge_entry_count_is_refused() {
        for claimed in [MAX_ZIP_ENTRIES + 1, 1 << 40, u64::MAX] {
            let bytes = zip64_claiming(claimed, 46, 0);
            assert!(matches!(
                names(&bytes),
                Err(InspectError::LimitExceeded {
                    format: FormatFamily::Zip,
                    ..
                })
            ));
        }
    }

    #[test]
    fn zip64_directory_range_is_checked() {
        // Offset and size that overflow when added.
        let bytes = zip64_claiming(1, u64::MAX, u64::MAX);
        assert!(matches!(names(&bytes), Err(InspectError::Malformed { .. })));
        // A directory that would lie far outside the file.
        let bytes = zip64_claiming(1, 46, 1 << 50);
        assert_eq!(names(&bytes), Err(truncated("the central directory")));
        // A directory larger than the cap, inside a file that is not.
        let bytes = zip64_claiming(1, 1 << 40, 0);
        assert!(matches!(
            names(&bytes),
            Err(InspectError::Truncated { .. } | InspectError::LimitExceeded { .. })
        ));
    }

    #[test]
    fn directory_offset_outside_the_file_is_truncated() {
        let mut bytes = zip(&["a.txt"]);
        let offset_at = bytes.len() - EOCD_LEN + 16;
        bytes[offset_at..offset_at + 4].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
        assert_eq!(names(&bytes), Err(truncated("the central directory")));
    }

    #[test]
    fn bad_entry_signature_is_malformed() {
        let mut bytes = zip(&["a.txt"]);
        // The central directory starts after the one local header: 30 + 5.
        bytes[35..39].copy_from_slice(b"XXXX");
        assert!(matches!(names(&bytes), Err(InspectError::Malformed { .. })));
    }
}
