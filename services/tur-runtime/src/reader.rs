// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Bounded, offset-checked access to the bytes being inspected.
//!
//! Every parser reads through [`Reader`]. It refuses a range that is not
//! wholly inside the file, caps each read and keeps a total budget, so no
//! header field can cause a large allocation or an unbounded amount of I/O.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::arch::Endianness;
use crate::error::{FormatFamily, InspectError};
use crate::limits::{MAX_SINGLE_READ, MAX_TOTAL_READ};

enum Backing<'a> {
    File(File),
    Bytes(&'a [u8]),
}

/// A file or byte slice, read only through checked ranges.
pub(crate) struct Reader<'a> {
    backing: Backing<'a>,
    size: u64,
    budget: u64,
}

impl<'a> Reader<'a> {
    /// A reader over bytes already in memory.
    pub(crate) fn from_bytes(data: &'a [u8]) -> Self {
        Reader {
            backing: Backing::Bytes(data),
            size: u64::try_from(data.len()).unwrap_or(u64::MAX),
            budget: MAX_TOTAL_READ,
        }
    }

    /// A reader over an open regular file of the given size.
    pub(crate) fn from_file(file: File, size: u64) -> Self {
        Reader {
            backing: Backing::File(file),
            size,
            budget: MAX_TOTAL_READ,
        }
    }

    /// The size of the input in bytes.
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    /// Reads exactly `len` bytes at `offset`.
    ///
    /// Returns `Ok(None)` when the range is not wholly inside the input, and
    /// an error when a limit is hit or the operating system fails the read.
    pub(crate) fn read(
        &mut self,
        offset: u64,
        len: usize,
    ) -> Result<Option<Vec<u8>>, InspectError> {
        let wanted = u64::try_from(len).unwrap_or(u64::MAX);
        let Some(end) = offset.checked_add(wanted) else {
            return Ok(None);
        };
        if end > self.size {
            return Ok(None);
        }
        if len > MAX_SINGLE_READ || wanted > self.budget {
            return Err(InspectError::LimitExceeded {
                format: FormatFamily::Unknown,
                detail: format!("reading {len} more bytes would pass the per-file read limit"),
            });
        }
        self.budget -= wanted;
        match &mut self.backing {
            Backing::Bytes(data) => {
                let slice = usize::try_from(offset)
                    .ok()
                    .and_then(|start| data.get(start..start.checked_add(len)?));
                Ok(slice.map(<[u8]>::to_vec))
            }
            Backing::File(file) => {
                let mut buffer = vec![0u8; len];
                file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
                file.read_exact(&mut buffer).map_err(io_error)?;
                Ok(Some(buffer))
            }
        }
    }

    /// Reads at most `max` bytes at `offset`: fewer when the input ends first,
    /// none when `offset` is at or past the end.
    pub(crate) fn read_up_to(&mut self, offset: u64, max: usize) -> Result<Vec<u8>, InspectError> {
        let available = self.size.saturating_sub(offset);
        let len = usize::try_from(available).map_or(max, |fits| fits.min(max));
        Ok(self.read(offset, len)?.unwrap_or_default())
    }
}

fn io_error(error: std::io::Error) -> InspectError {
    InspectError::Io {
        path: String::new(),
        message: error.to_string(),
    }
}

/// A 16-bit field at `at`, or `None` when it is not inside `buffer`.
pub(crate) fn u16_at(buffer: &[u8], at: usize, endian: Endianness) -> Option<u16> {
    let bytes: [u8; 2] = buffer.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(match endian {
        Endianness::Little => u16::from_le_bytes(bytes),
        Endianness::Big => u16::from_be_bytes(bytes),
    })
}

/// A 32-bit field at `at`, or `None` when it is not inside `buffer`.
pub(crate) fn u32_at(buffer: &[u8], at: usize, endian: Endianness) -> Option<u32> {
    let bytes: [u8; 4] = buffer.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(match endian {
        Endianness::Little => u32::from_le_bytes(bytes),
        Endianness::Big => u32::from_be_bytes(bytes),
    })
}

/// A 64-bit field at `at`, or `None` when it is not inside `buffer`.
pub(crate) fn u64_at(buffer: &[u8], at: usize, endian: Endianness) -> Option<u64> {
    let bytes: [u8; 8] = buffer.get(at..at.checked_add(8)?)?.try_into().ok()?;
    Some(match endian {
        Endianness::Little => u64::from_le_bytes(bytes),
        Endianness::Big => u64::from_be_bytes(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_inside_the_input() {
        let data = [1u8, 2, 3, 4, 5];
        let mut reader = Reader::from_bytes(&data);
        assert_eq!(reader.size(), 5);
        assert_eq!(reader.read(1, 3).unwrap().unwrap(), [2, 3, 4]);
        assert_eq!(reader.read(0, 5).unwrap(), Some(data.to_vec()));
        assert_eq!(reader.read(5, 0).unwrap(), Some(Vec::new()));
        assert_eq!(reader.read(3, 3).unwrap(), None);
        assert_eq!(reader.read(6, 0).unwrap(), None);
        assert_eq!(reader.read(u64::MAX, 2).unwrap(), None);
        assert_eq!(reader.read_up_to(3, 100).unwrap(), [4, 5]);
        assert_eq!(reader.read_up_to(9, 100).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn total_reads_are_budgeted() {
        let data = [0u8; 1024];
        let mut reader = Reader::from_bytes(&data);
        reader.budget = 1500;
        assert!(reader.read(0, 1024).unwrap().is_some());
        let error = reader.read(0, 1024).unwrap_err();
        assert!(matches!(error, InspectError::LimitExceeded { .. }));
    }

    #[test]
    fn fields_respect_byte_order_and_bounds() {
        let buffer = [0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];
        assert_eq!(u16_at(&buffer, 0, Endianness::Big), Some(0x1234));
        assert_eq!(u16_at(&buffer, 0, Endianness::Little), Some(0x3412));
        assert_eq!(u32_at(&buffer, 4, Endianness::Big), Some(0x9abc_def0));
        assert_eq!(u32_at(&buffer, 4, Endianness::Little), Some(0xf0de_bc9a));
        assert_eq!(
            u64_at(&buffer, 0, Endianness::Big),
            Some(0x1234_5678_9abc_def0)
        );
        assert_eq!(u16_at(&buffer, 7, Endianness::Big), None);
        assert_eq!(u32_at(&buffer, 5, Endianness::Big), None);
        assert_eq!(u64_at(&buffer, 1, Endianness::Big), None);
        assert_eq!(u16_at(&buffer, usize::MAX, Endianness::Big), None);
    }
}
