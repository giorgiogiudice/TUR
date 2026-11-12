// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Mach-O inspection: thin images of either width and byte order, and
//! universal ("fat") files that hold several images.
//!
//! Only the fixed header of each image is read (processor and file type).
//! Load commands, linked libraries and code signatures are not.

use serde::Serialize;

use crate::arch::{Arch, Endianness};
use crate::error::{FormatFamily, InspectError};
use crate::limits::MAX_FAT_ARCHS;
use crate::reader::{Reader, u32_at, u64_at};

const MAGIC_32: u32 = 0xfeed_face;
const MAGIC_32_SWAPPED: u32 = 0xcefa_edfe;
const MAGIC_64: u32 = 0xfeed_facf;
const MAGIC_64_SWAPPED: u32 = 0xcffa_edfe;
const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_MAGIC_64: u32 = 0xcafe_babf;
/// The flag in a processor type that marks a 64-bit ABI.
const CPU_ARCH_ABI64: u32 = 0x0100_0000;

/// What kind of object a Mach-O image is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachOFileType {
    /// `MH_OBJECT`: an object file for the linker.
    Object,
    /// `MH_EXECUTE`: a program.
    Executable,
    /// `MH_CORE`: a core dump.
    Core,
    /// `MH_DYLIB`: a dynamic library.
    Dylib,
    /// `MH_DYLINKER`: the dynamic linker itself.
    Dylinker,
    /// `MH_BUNDLE`: a loadable plug-in.
    Bundle,
    /// `MH_DSYM`: debug symbols.
    Dsym,
    /// `MH_KEXT_BUNDLE`: a kernel extension.
    Kext,
    /// Any other file type value.
    Other,
    /// A slice of a universal file that does not hold a Mach-O image, for
    /// example a static library archive.
    NotMachO,
}

impl MachOFileType {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            MachOFileType::Object => "object file",
            MachOFileType::Executable => "executable",
            MachOFileType::Core => "core dump",
            MachOFileType::Dylib => "dynamic library",
            MachOFileType::Dylinker => "dynamic linker",
            MachOFileType::Bundle => "loadable bundle",
            MachOFileType::Dsym => "debug symbols",
            MachOFileType::Kext => "kernel extension",
            MachOFileType::Other => "unrecognised file type",
            MachOFileType::NotMachO => "not a Mach-O image",
        }
    }

    fn from_raw(raw: u32) -> MachOFileType {
        match raw {
            1 => MachOFileType::Object,
            2 => MachOFileType::Executable,
            4 => MachOFileType::Core,
            6 => MachOFileType::Dylib,
            7 => MachOFileType::Dylinker,
            8 => MachOFileType::Bundle,
            0xa => MachOFileType::Dsym,
            0xb => MachOFileType::Kext,
            _ => MachOFileType::Other,
        }
    }
}

/// One Mach-O image: the whole of a thin file, or one slice of a universal file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MachOSlice {
    /// The processor the image is for.
    pub arch: Arch,
    /// The raw processor type.
    pub cpu_type: u32,
    /// The raw processor subtype.
    pub cpu_subtype: u32,
    /// 32 or 64.
    pub bits: u8,
    /// Byte order of the image, when the slice holds a Mach-O image.
    pub endianness: Option<Endianness>,
    /// The kind of object.
    pub file_type: MachOFileType,
    /// The raw file type, when the slice holds a Mach-O image.
    pub file_type_raw: Option<u32>,
    /// Where the image starts in the file.
    pub offset: u64,
    /// The image's size in bytes.
    pub size: u64,
}

/// What was read from a Mach-O file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MachOInfo {
    /// Whether the file is a universal file holding several images.
    pub universal: bool,
    /// The images, in file order. A thin file has exactly one.
    pub slices: Vec<MachOSlice>,
}

impl MachOInfo {
    /// Whether any image is a kernel extension.
    pub fn has_kext(&self) -> bool {
        self.slices
            .iter()
            .any(|slice| slice.file_type == MachOFileType::Kext)
    }

    /// Whether the file holds a program for `arch`.
    pub fn has_executable_for(&self, arch: Arch) -> bool {
        self.slices
            .iter()
            .any(|slice| slice.arch == arch && slice.file_type == MachOFileType::Executable)
    }

    /// The architectures of every image, in file order.
    pub fn architectures(&self) -> Vec<Arch> {
        self.slices.iter().map(|slice| slice.arch).collect()
    }
}

/// The outcome of parsing a file that starts with the universal magic number.
pub(crate) enum Universal {
    MachO(MachOInfo),
    /// The magic number is shared with Java class files; this is one of those.
    JavaClass,
}

fn truncated(detail: &'static str) -> InspectError {
    InspectError::Truncated {
        format: FormatFamily::MachO,
        detail,
    }
}

fn malformed(detail: &str) -> InspectError {
    InspectError::Malformed {
        format: FormatFamily::MachO,
        detail: detail.to_owned(),
    }
}

/// Whether `magic`, the first four bytes read big-endian, starts a thin image.
pub(crate) fn is_thin_magic(magic: u32) -> bool {
    matches!(
        magic,
        MAGIC_32 | MAGIC_32_SWAPPED | MAGIC_64 | MAGIC_64_SWAPPED
    )
}

/// Whether `magic`, the first four bytes read big-endian, starts a universal file.
pub(crate) fn is_fat_magic(magic: u32) -> bool {
    matches!(magic, FAT_MAGIC | FAT_MAGIC_64)
}

/// Parses a thin Mach-O file whose first bytes are `head`.
pub(crate) fn parse_thin(reader: &Reader<'_>, head: &[u8]) -> Result<MachOInfo, InspectError> {
    let slice = image_header(head, 0, reader.size()).ok_or_else(|| truncated("the header"))?;
    Ok(MachOInfo {
        universal: false,
        slices: vec![slice],
    })
}

/// Reads the fixed header of a Mach-O image from `bytes`. `None` when the
/// bytes do not start with a Mach-O magic number or the header is cut short.
fn image_header(bytes: &[u8], offset: u64, size: u64) -> Option<MachOSlice> {
    let (bits, endian) = match u32_at(bytes, 0, Endianness::Big)? {
        MAGIC_32 => (32u8, Endianness::Big),
        MAGIC_32_SWAPPED => (32, Endianness::Little),
        MAGIC_64 => (64, Endianness::Big),
        MAGIC_64_SWAPPED => (64, Endianness::Little),
        _ => return None,
    };
    let header_len = if bits == 64 { 32 } else { 28 };
    let header = bytes.get(..header_len)?;
    let cpu_type = u32_at(header, 4, endian)?;
    let cpu_subtype = u32_at(header, 8, endian)?;
    let file_type_raw = u32_at(header, 12, endian)?;
    Some(MachOSlice {
        arch: arch(cpu_type),
        cpu_type,
        cpu_subtype,
        bits,
        endianness: Some(endian),
        file_type: MachOFileType::from_raw(file_type_raw),
        file_type_raw: Some(file_type_raw),
        offset,
        size,
    })
}

/// Parses a universal file whose first bytes are `head`.
pub(crate) fn parse_fat(
    reader: &mut Reader<'_>,
    head: &[u8],
    notes: &mut Vec<String>,
) -> Result<Universal, InspectError> {
    let magic = u32_at(head, 0, Endianness::Big).ok_or_else(|| truncated("the header"))?;
    let wide = magic == FAT_MAGIC_64;
    let count = u32_at(head, 4, Endianness::Big)
        .ok_or_else(|| truncated("the universal header's architecture count"))?;
    if count == 0 {
        return Err(malformed("the universal header lists no architectures"));
    }
    if count > MAX_FAT_ARCHS {
        // A Java class file stores its minor and major version here. Major
        // versions start at 45 and have stayed far below 256.
        let class_major = count & 0xffff;
        if !wide && (45..256).contains(&class_major) {
            return Ok(Universal::JavaClass);
        }
        return Err(InspectError::LimitExceeded {
            format: FormatFamily::MachO,
            detail: format!(
                "the universal header claims {count} architectures; at most {MAX_FAT_ARCHS} are accepted"
            ),
        });
    }

    let entry_len: usize = if wide { 32 } else { 20 };
    let mut slices = Vec::new();
    let mut at: u64 = 8;
    for _ in 0..count {
        let entry = reader
            .read(at, entry_len)?
            .ok_or_else(|| truncated("the universal architecture table"))?;
        at = at.saturating_add(if wide { 32 } else { 20 });
        let declared = fat_entry(&entry, wide).ok_or_else(|| truncated("an architecture entry"))?;
        let end = declared
            .offset
            .checked_add(declared.size)
            .ok_or_else(|| malformed("an image's offset and size overflow"))?;
        if end > reader.size() {
            return Err(truncated("an image named by the universal header"));
        }
        let wanted = usize::try_from(declared.size).map_or(32, |size| size.min(32));
        let image = reader.read_up_to(declared.offset, wanted)?;
        let slice = match image_header(&image, declared.offset, declared.size) {
            Some(found) => {
                if found.cpu_type != declared.cpu_type {
                    notes.push(format!(
                        "A universal slice is declared as {} but its own header says {}; the \
                         slice's own header is reported.",
                        arch(declared.cpu_type),
                        found.arch
                    ));
                }
                found
            }
            None => MachOSlice {
                arch: arch(declared.cpu_type),
                cpu_type: declared.cpu_type,
                cpu_subtype: declared.cpu_subtype,
                bits: if (declared.cpu_type & CPU_ARCH_ABI64) != 0 {
                    64
                } else {
                    32
                },
                endianness: None,
                file_type: MachOFileType::NotMachO,
                file_type_raw: None,
                offset: declared.offset,
                size: declared.size,
            },
        };
        slices.push(slice);
    }
    Ok(Universal::MachO(MachOInfo {
        universal: true,
        slices,
    }))
}

struct FatEntry {
    cpu_type: u32,
    cpu_subtype: u32,
    offset: u64,
    size: u64,
}

fn fat_entry(entry: &[u8], wide: bool) -> Option<FatEntry> {
    let big = Endianness::Big;
    let cpu_type = u32_at(entry, 0, big)?;
    let cpu_subtype = u32_at(entry, 4, big)?;
    let (offset, size) = if wide {
        (u64_at(entry, 8, big)?, u64_at(entry, 16, big)?)
    } else {
        (
            u64::from(u32_at(entry, 8, big)?),
            u64::from(u32_at(entry, 12, big)?),
        )
    };
    Some(FatEntry {
        cpu_type,
        cpu_subtype,
        offset,
        size,
    })
}

fn arch(cpu_type: u32) -> Arch {
    match cpu_type {
        7 => Arch::I386,
        0x0100_0007 => Arch::X86_64,
        12 => Arch::Arm,
        0x0100_000c => Arch::Aarch64,
        18 => Arch::Ppc,
        0x0100_0012 => Arch::Ppc64,
        _ => Arch::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{Detected, inspect_bytes};
    use crate::testdata::{
        CPU_ARM64, CPU_I386, CPU_PPC, CPU_X86_64, macho_fat, macho_thin, raw_fat_header,
    };

    fn macho(bytes: &[u8]) -> MachOInfo {
        match inspect_bytes(bytes).unwrap().detected {
            Detected::MachO(info) => info,
            other => panic!("expected Mach-O, got {other:?}"),
        }
    }

    #[test]
    fn thin_images_of_both_widths_and_byte_orders() {
        let info = macho(&macho_thin(CPU_X86_64, 2, true, false));
        assert!(!info.universal);
        assert_eq!(info.slices.len(), 1);
        let slice = &info.slices[0];
        assert_eq!(slice.arch, Arch::X86_64);
        assert_eq!(slice.bits, 64);
        assert_eq!(slice.endianness, Some(Endianness::Little));
        assert_eq!(slice.file_type, MachOFileType::Executable);
        assert_eq!(slice.file_type_raw, Some(2));
        assert_eq!(slice.offset, 0);
        assert_eq!(slice.size, 32);

        let info = macho(&macho_thin(CPU_ARM64, 2, true, false));
        assert_eq!(info.slices[0].arch, Arch::Aarch64);

        let info = macho(&macho_thin(CPU_I386, 2, false, false));
        assert_eq!(info.slices[0].arch, Arch::I386);
        assert_eq!(info.slices[0].bits, 32);
        assert_eq!(info.slices[0].endianness, Some(Endianness::Little));

        let info = macho(&macho_thin(CPU_PPC, 2, false, true));
        assert_eq!(info.slices[0].arch, Arch::Ppc);
        assert_eq!(info.slices[0].bits, 32);
        assert_eq!(info.slices[0].endianness, Some(Endianness::Big));

        let info = macho(&macho_thin(0x0100_0012, 2, true, true));
        assert_eq!(info.slices[0].arch, Arch::Ppc64);
        assert_eq!(info.slices[0].bits, 64);
        assert_eq!(info.slices[0].endianness, Some(Endianness::Big));
    }

    #[test]
    fn file_types_are_named() {
        for (raw, expected) in [
            (1u32, MachOFileType::Object),
            (2, MachOFileType::Executable),
            (6, MachOFileType::Dylib),
            (8, MachOFileType::Bundle),
            (0xb, MachOFileType::Kext),
            (0x99, MachOFileType::Other),
        ] {
            let info = macho(&macho_thin(CPU_X86_64, raw, true, false));
            assert_eq!(info.slices[0].file_type, expected);
            assert_eq!(info.has_kext(), expected == MachOFileType::Kext);
        }
    }

    #[test]
    fn universal_file_lists_every_architecture() {
        let info = macho(&macho_fat(&[(CPU_X86_64, 2), (CPU_ARM64, 2)], false));
        assert!(info.universal);
        assert_eq!(info.architectures(), [Arch::X86_64, Arch::Aarch64]);
        assert!(info.has_executable_for(Arch::X86_64));
        assert!(info.has_executable_for(Arch::Aarch64));
        assert!(!info.has_executable_for(Arch::I386));
        assert!(info.slices[1].offset > info.slices[0].offset);

        let info = macho(&macho_fat(
            &[(CPU_I386, 6), (CPU_X86_64, 6), (CPU_ARM64, 6)],
            true,
        ));
        assert_eq!(
            info.architectures(),
            [Arch::I386, Arch::X86_64, Arch::Aarch64]
        );
        assert!(
            info.slices
                .iter()
                .all(|slice| slice.file_type == MachOFileType::Dylib)
        );
    }

    #[test]
    fn a_million_architectures_is_refused_without_reading_them() {
        let bytes = raw_fat_header(1_000_000);
        assert!(matches!(
            inspect_bytes(&bytes),
            Err(InspectError::LimitExceeded {
                format: FormatFamily::MachO,
                ..
            })
        ));
        let bytes = raw_fat_header(u32::MAX);
        assert!(matches!(
            inspect_bytes(&bytes),
            Err(InspectError::LimitExceeded { .. })
        ));
    }

    #[test]
    fn empty_universal_header_is_malformed() {
        assert!(matches!(
            inspect_bytes(&raw_fat_header(0)),
            Err(InspectError::Malformed { .. })
        ));
    }

    #[test]
    fn java_class_files_are_not_mistaken_for_universal_binaries() {
        // 0xCAFEBABE, minor version 0, major version 52 (Java 8).
        let inspection = inspect_bytes(&raw_fat_header(52)).unwrap();
        assert_eq!(inspection.detected, Detected::Unknown);
        assert!(inspection.notes.iter().any(|note| note.contains("Java")));
    }

    #[test]
    fn slice_outside_the_file_is_truncated() {
        let mut bytes = macho_fat(&[(CPU_X86_64, 2)], false);
        // The first entry's offset field is at 8 + 8.
        bytes[16..20].copy_from_slice(&0xffff_ff00u32.to_be_bytes());
        assert_eq!(
            inspect_bytes(&bytes),
            Err(truncated("an image named by the universal header"))
        );
    }

    #[test]
    fn sixty_four_bit_slice_range_overflow_is_malformed() {
        let mut bytes = macho_fat(&[(CPU_X86_64, 2)], true);
        // fat_arch_64: offset at 8 + 8, size at 8 + 16.
        bytes[16..24].copy_from_slice(&u64::MAX.to_be_bytes());
        bytes[24..32].copy_from_slice(&u64::MAX.to_be_bytes());
        assert!(matches!(
            inspect_bytes(&bytes),
            Err(InspectError::Malformed {
                format: FormatFamily::MachO,
                ..
            })
        ));
    }

    #[test]
    fn slice_that_is_not_mach_o_is_reported_as_such() {
        let mut bytes = macho_fat(&[(CPU_X86_64, 2)], false);
        let offset = usize::try_from(macho(&bytes).slices[0].offset).unwrap();
        bytes[offset..offset + 8].copy_from_slice(b"!<arch>\n");
        let info = macho(&bytes);
        assert_eq!(info.slices[0].file_type, MachOFileType::NotMachO);
        assert_eq!(info.slices[0].arch, Arch::X86_64);
        assert_eq!(info.slices[0].bits, 64);
        assert_eq!(info.slices[0].endianness, None);
        assert!(!info.has_executable_for(Arch::X86_64));
    }

    #[test]
    fn truncated_thin_header_is_a_typed_error() {
        let bytes = macho_thin(CPU_X86_64, 2, true, false);
        assert_eq!(inspect_bytes(&bytes[..20]), Err(truncated("the header")));
    }
}
