// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! ELF inspection: the file header, the program header table, the interpreter
//! path and the one dynamic-section flag that separates a position-independent
//! executable from a shared library.
//!
//! Section headers, symbols and the list of needed libraries are not read.

use serde::Serialize;

use crate::arch::{Arch, Endianness};
use crate::error::{FormatFamily, InspectError};
use crate::limits::{MAX_DYNAMIC_BYTES, MAX_INTERPRETER_LEN, MAX_PROGRAM_HEADERS};
use crate::reader::{Reader, u16_at, u32_at, u64_at};

const ET_DYN: u16 = 3;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PN_XNUM: u16 = 0xffff;
const DT_NULL: u64 = 0;
const DT_FLAGS_1: u64 = 0x6fff_fffb;
const DF_1_PIE: u64 = 0x0800_0000;

/// Whether an ELF file uses 32-bit or 64-bit structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ElfClass {
    /// `ELFCLASS32`.
    Elf32,
    /// `ELFCLASS64`.
    Elf64,
}

impl ElfClass {
    /// A label for reports: `32-bit` or `64-bit`.
    pub const fn label(self) -> &'static str {
        match self {
            ElfClass::Elf32 => "32-bit",
            ElfClass::Elf64 => "64-bit",
        }
    }
}

/// What kind of object an ELF file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ElfType {
    /// `ET_REL`: an object file for the linker.
    Relocatable,
    /// `ET_EXEC`: an executable loaded at a fixed address.
    Executable,
    /// `ET_DYN` that names an interpreter or carries the `DF_1_PIE` flag: a
    /// position-independent executable.
    PieExecutable,
    /// `ET_DYN` with neither: a shared library.
    SharedObject,
    /// `ET_CORE`: a core dump.
    Core,
    /// Any other type value.
    Other,
}

impl ElfType {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            ElfType::Relocatable => "relocatable object",
            ElfType::Executable => "executable",
            ElfType::PieExecutable => "position-independent executable",
            ElfType::SharedObject => "shared object",
            ElfType::Core => "core dump",
            ElfType::Other => "unrecognised type",
        }
    }
}

/// How an ELF file is linked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Linkage {
    /// No interpreter and no dynamic section, or a static position-independent
    /// executable.
    Static,
    /// The file names an interpreter or has a dynamic section.
    Dynamic,
    /// The program header table is absent or was not followed.
    Unknown,
}

impl Linkage {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Linkage::Static => "statically linked",
            Linkage::Dynamic => "dynamically linked",
            Linkage::Unknown => "linkage not determined",
        }
    }
}

/// The operating-system ABI byte of the ELF identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ElfOsAbi {
    /// System V, which is also what most Linux programs declare.
    #[serde(rename = "sysv")]
    SystemV,
    /// GNU/Linux.
    #[serde(rename = "gnu-linux")]
    Gnu,
    /// FreeBSD.
    #[serde(rename = "freebsd")]
    FreeBsd,
    /// NetBSD.
    #[serde(rename = "netbsd")]
    NetBsd,
    /// OpenBSD.
    #[serde(rename = "openbsd")]
    OpenBsd,
    /// Solaris.
    #[serde(rename = "solaris")]
    Solaris,
    /// A standalone or embedded image.
    #[serde(rename = "standalone")]
    Standalone,
    /// Any other value.
    #[serde(rename = "other")]
    Other,
}

impl ElfOsAbi {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            ElfOsAbi::SystemV => "System V",
            ElfOsAbi::Gnu => "GNU/Linux",
            ElfOsAbi::FreeBsd => "FreeBSD",
            ElfOsAbi::NetBsd => "NetBSD",
            ElfOsAbi::OpenBsd => "OpenBSD",
            ElfOsAbi::Solaris => "Solaris",
            ElfOsAbi::Standalone => "standalone",
            ElfOsAbi::Other => "unrecognised",
        }
    }

    /// Whether a Linux kernel loads files that declare this ABI.
    pub const fn is_linux_compatible(self) -> bool {
        matches!(self, ElfOsAbi::SystemV | ElfOsAbi::Gnu)
    }
}

/// What was read from an ELF file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElfInfo {
    /// 32-bit or 64-bit structures.
    pub class: ElfClass,
    /// Byte order.
    pub endianness: Endianness,
    /// The machine the code is for.
    pub machine: Arch,
    /// The raw `e_machine` value.
    pub machine_raw: u16,
    /// The kind of object.
    pub file_type: ElfType,
    /// The raw `e_type` value.
    pub file_type_raw: u16,
    /// Static or dynamic linking.
    pub linkage: Linkage,
    /// The interpreter (dynamic loader) path from `PT_INTERP`, if present.
    pub interpreter: Option<String>,
    /// The operating-system ABI.
    pub os_abi: ElfOsAbi,
    /// The raw operating-system ABI byte.
    pub os_abi_raw: u8,
    /// The AppImage type (1 or 2) when the AppImage magic is at offset 8.
    pub app_image: Option<u8>,
}

struct Header {
    file_type: u16,
    machine: u16,
    phoff: u64,
    phentsize: u16,
    phnum: u16,
}

fn truncated(detail: &'static str) -> InspectError {
    InspectError::Truncated {
        format: FormatFamily::Elf,
        detail,
    }
}

fn malformed(detail: impl Into<String>) -> InspectError {
    InspectError::Malformed {
        format: FormatFamily::Elf,
        detail: detail.into(),
    }
}

/// Parses an ELF file whose first bytes are `head`.
pub(crate) fn parse(
    reader: &mut Reader<'_>,
    head: &[u8],
    notes: &mut Vec<String>,
) -> Result<ElfInfo, InspectError> {
    let ident = head
        .get(..16)
        .ok_or_else(|| truncated("the identification header"))?;
    let class = match ident[4] {
        1 => ElfClass::Elf32,
        2 => ElfClass::Elf64,
        other => return Err(malformed(format!("unknown class byte {other}"))),
    };
    let endian = match ident[5] {
        1 => Endianness::Little,
        2 => Endianness::Big,
        other => return Err(malformed(format!("unknown data-encoding byte {other}"))),
    };
    let os_abi_raw = ident[7];
    let app_image = match (ident[8], ident[9], ident[10]) {
        (b'A', b'I', kind @ 1..=2) => Some(kind),
        _ => None,
    };

    let wide = class == ElfClass::Elf64;
    let header_len = if wide { 64 } else { 52 };
    let header = head
        .get(..header_len)
        .and_then(|bytes| read_header(bytes, wide, endian))
        .ok_or_else(|| truncated("the file header"))?;

    let mut interp: Option<(u64, u64)> = None;
    let mut dynamic: Option<(u64, u64)> = None;
    let mut table_followed = false;
    if header.phnum == PN_XNUM {
        notes.push(
            "The ELF program header count is stored in a section header; it was not followed."
                .to_owned(),
        );
    } else if header.phnum > 0 {
        let entry_len: u16 = if wide { 56 } else { 32 };
        if header.phentsize < entry_len {
            return Err(malformed(format!(
                "program header entry size {} is too small",
                header.phentsize
            )));
        }
        if header.phnum > MAX_PROGRAM_HEADERS {
            notes.push(format!(
                "Only the first {MAX_PROGRAM_HEADERS} ELF program headers were examined."
            ));
        }
        for index in 0..header.phnum.min(MAX_PROGRAM_HEADERS) {
            let at = u64::from(index)
                .checked_mul(u64::from(header.phentsize))
                .and_then(|relative| header.phoff.checked_add(relative))
                .ok_or_else(|| malformed("the program header offset overflows"))?;
            let entry = reader
                .read(at, usize::from(entry_len))?
                .ok_or_else(|| truncated("the program header table"))?;
            let (kind, offset, size) =
                segment(&entry, wide, endian).ok_or_else(|| truncated("a program header"))?;
            match kind {
                PT_INTERP => interp = Some((offset, size)),
                PT_DYNAMIC => dynamic = Some((offset, size)),
                _ => {}
            }
        }
        table_followed = true;
    }

    let interpreter = interp
        .map(|(offset, size)| read_interpreter(reader, offset, size))
        .transpose()?;
    let pie_flag = match dynamic {
        Some((offset, size)) if header.file_type == ET_DYN => {
            has_pie_flag(reader, offset, size, wide, endian)?
        }
        _ => false,
    };

    let file_type = match header.file_type {
        1 => ElfType::Relocatable,
        2 => ElfType::Executable,
        ET_DYN if interpreter.is_some() || pie_flag => ElfType::PieExecutable,
        ET_DYN => ElfType::SharedObject,
        4 => ElfType::Core,
        _ => ElfType::Other,
    };
    let static_pie = header.file_type == ET_DYN && pie_flag && interpreter.is_none();
    let linkage = if !table_followed {
        Linkage::Unknown
    } else if interpreter.is_some() || (dynamic.is_some() && !static_pie) {
        Linkage::Dynamic
    } else {
        Linkage::Static
    };

    let machine = machine(header.machine, wide);
    if header.machine == 62 && !wide {
        notes.push(
            "This is an x32 program (x86-64 code with 32-bit pointers), reported as another \
             architecture because Linux distributions rarely support it."
                .to_owned(),
        );
    }

    Ok(ElfInfo {
        class,
        endianness: endian,
        machine,
        machine_raw: header.machine,
        file_type,
        file_type_raw: header.file_type,
        linkage,
        interpreter,
        os_abi: os_abi(os_abi_raw),
        os_abi_raw,
        app_image,
    })
}

fn read_header(bytes: &[u8], wide: bool, endian: Endianness) -> Option<Header> {
    let file_type = u16_at(bytes, 16, endian)?;
    let machine = u16_at(bytes, 18, endian)?;
    if wide {
        Some(Header {
            file_type,
            machine,
            phoff: u64_at(bytes, 32, endian)?,
            phentsize: u16_at(bytes, 54, endian)?,
            phnum: u16_at(bytes, 56, endian)?,
        })
    } else {
        Some(Header {
            file_type,
            machine,
            phoff: u64::from(u32_at(bytes, 28, endian)?),
            phentsize: u16_at(bytes, 42, endian)?,
            phnum: u16_at(bytes, 44, endian)?,
        })
    }
}

/// The type, file offset and file size of one program header.
fn segment(entry: &[u8], wide: bool, endian: Endianness) -> Option<(u32, u64, u64)> {
    let kind = u32_at(entry, 0, endian)?;
    if wide {
        Some((kind, u64_at(entry, 8, endian)?, u64_at(entry, 32, endian)?))
    } else {
        Some((
            kind,
            u64::from(u32_at(entry, 4, endian)?),
            u64::from(u32_at(entry, 16, endian)?),
        ))
    }
}

fn read_interpreter(
    reader: &mut Reader<'_>,
    offset: u64,
    size: u64,
) -> Result<String, InspectError> {
    if size == 0 || size > MAX_INTERPRETER_LEN {
        return Err(malformed(format!(
            "an interpreter path of {size} bytes is not plausible"
        )));
    }
    let len = usize::try_from(size).map_err(|_| malformed("the interpreter path is too long"))?;
    let raw = reader
        .read(offset, len)?
        .ok_or_else(|| truncated("the interpreter path"))?;
    let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
    let text = std::str::from_utf8(&raw[..end])
        .map_err(|_| malformed("the interpreter path is not valid UTF-8"))?;
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(malformed(
            "the interpreter path is empty or has control characters",
        ));
    }
    Ok(text.to_owned())
}

/// Whether the dynamic section carries `DT_FLAGS_1` with `DF_1_PIE`.
fn has_pie_flag(
    reader: &mut Reader<'_>,
    offset: u64,
    size: u64,
    wide: bool,
    endian: Endianness,
) -> Result<bool, InspectError> {
    let len = usize::try_from(size.min(MAX_DYNAMIC_BYTES))
        .map_err(|_| malformed("the dynamic section is too long"))?;
    let table = reader
        .read(offset, len)?
        .ok_or_else(|| truncated("the dynamic section"))?;
    let entry_len = if wide { 16 } else { 8 };
    for entry in table.chunks_exact(entry_len) {
        let pair = if wide {
            u64_at(entry, 0, endian).zip(u64_at(entry, 8, endian))
        } else {
            u32_at(entry, 0, endian)
                .zip(u32_at(entry, 4, endian))
                .map(|(tag, value)| (u64::from(tag), u64::from(value)))
        };
        let Some((tag, value)) = pair else {
            break;
        };
        if tag == DT_NULL {
            break;
        }
        if tag == DT_FLAGS_1 {
            return Ok((value & DF_1_PIE) != 0);
        }
    }
    Ok(false)
}

fn machine(raw: u16, wide: bool) -> Arch {
    match (raw, wide) {
        (3, _) => Arch::I386,
        (62, true) => Arch::X86_64,
        (183, _) => Arch::Aarch64,
        (40, _) => Arch::Arm,
        (243, true) => Arch::Riscv64,
        (243, false) => Arch::Riscv32,
        (21, _) => Arch::Ppc64,
        (20, _) => Arch::Ppc,
        (22, true) => Arch::S390x,
        (8, true) => Arch::Mips64,
        (8, false) => Arch::Mips,
        (258, true) => Arch::Loongarch64,
        (2 | 18 | 43, _) => Arch::Sparc,
        (50, _) => Arch::Ia64,
        _ => Arch::Other,
    }
}

fn os_abi(raw: u8) -> ElfOsAbi {
    match raw {
        0 => ElfOsAbi::SystemV,
        3 => ElfOsAbi::Gnu,
        9 => ElfOsAbi::FreeBsd,
        2 => ElfOsAbi::NetBsd,
        12 => ElfOsAbi::OpenBsd,
        6 => ElfOsAbi::Solaris,
        255 => ElfOsAbi::Standalone,
        _ => ElfOsAbi::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{Detected, inspect_bytes};
    use crate::testdata::ElfSpec;

    fn elf(bytes: &[u8]) -> ElfInfo {
        match inspect_bytes(bytes).unwrap().detected {
            Detected::Elf(info) => info,
            other => panic!("expected ELF, got {other:?}"),
        }
    }

    #[test]
    fn dynamic_pie_for_x86_64() {
        let info = elf(&ElfSpec::linux_x86_64().build());
        assert_eq!(info.class, ElfClass::Elf64);
        assert_eq!(info.endianness, Endianness::Little);
        assert_eq!(info.machine, Arch::X86_64);
        assert_eq!(info.machine_raw, 62);
        assert_eq!(info.file_type, ElfType::PieExecutable);
        assert_eq!(info.linkage, Linkage::Dynamic);
        assert_eq!(
            info.interpreter.as_deref(),
            Some("/lib64/ld-linux-x86-64.so.2")
        );
        assert_eq!(info.os_abi, ElfOsAbi::SystemV);
        assert_eq!(info.app_image, None);
    }

    #[test]
    fn static_fixed_address_executable() {
        let spec = ElfSpec {
            file_type: 2,
            interpreter: None,
            dynamic_flags: None,
            ..ElfSpec::linux_x86_64()
        };
        let info = elf(&spec.build());
        assert_eq!(info.file_type, ElfType::Executable);
        assert_eq!(info.linkage, Linkage::Static);
        assert_eq!(info.interpreter, None);
    }

    #[test]
    fn shared_library_and_static_pie_are_told_apart() {
        let library = ElfSpec {
            interpreter: None,
            dynamic_flags: Some(0),
            ..ElfSpec::linux_x86_64()
        };
        let info = elf(&library.build());
        assert_eq!(info.file_type, ElfType::SharedObject);
        assert_eq!(info.linkage, Linkage::Dynamic);

        let static_pie = ElfSpec {
            interpreter: None,
            dynamic_flags: Some(DF_1_PIE),
            ..ElfSpec::linux_x86_64()
        };
        let info = elf(&static_pie.build());
        assert_eq!(info.file_type, ElfType::PieExecutable);
        assert_eq!(info.linkage, Linkage::Static);
    }

    #[test]
    fn thirty_two_bit_and_big_endian_layouts() {
        let i386 = ElfSpec {
            wide: false,
            machine: 3,
            interpreter: Some("/lib/ld-linux.so.2"),
            ..ElfSpec::linux_x86_64()
        };
        let info = elf(&i386.build());
        assert_eq!(info.class, ElfClass::Elf32);
        assert_eq!(info.machine, Arch::I386);
        assert_eq!(info.interpreter.as_deref(), Some("/lib/ld-linux.so.2"));
        assert_eq!(info.linkage, Linkage::Dynamic);

        let ppc = ElfSpec {
            wide: false,
            big_endian: true,
            machine: 20,
            file_type: 2,
            interpreter: Some("/lib/ld.so.1"),
            dynamic_flags: Some(0),
            ..ElfSpec::linux_x86_64()
        };
        let info = elf(&ppc.build());
        assert_eq!(info.class, ElfClass::Elf32);
        assert_eq!(info.endianness, Endianness::Big);
        assert_eq!(info.machine, Arch::Ppc);
        assert_eq!(info.file_type, ElfType::Executable);
        assert_eq!(info.interpreter.as_deref(), Some("/lib/ld.so.1"));

        let s390x = ElfSpec {
            big_endian: true,
            machine: 22,
            ..ElfSpec::linux_x86_64()
        };
        let info = elf(&s390x.build());
        assert_eq!(info.endianness, Endianness::Big);
        assert_eq!(info.machine, Arch::S390x);
        assert_eq!(info.file_type, ElfType::PieExecutable);
    }

    #[test]
    fn machines_are_named() {
        for (raw, expected) in [
            (183u16, Arch::Aarch64),
            (243, Arch::Riscv64),
            (21, Arch::Ppc64),
            (258, Arch::Loongarch64),
            (0x1234, Arch::Other),
        ] {
            let spec = ElfSpec {
                machine: raw,
                ..ElfSpec::linux_x86_64()
            };
            let info = elf(&spec.build());
            assert_eq!(info.machine, expected);
            assert_eq!(info.machine_raw, raw);
        }
    }

    #[test]
    fn core_relocatable_and_os_abi() {
        let core = ElfSpec {
            file_type: 4,
            interpreter: None,
            dynamic_flags: None,
            ..ElfSpec::linux_x86_64()
        };
        assert_eq!(elf(&core.build()).file_type, ElfType::Core);

        let object = ElfSpec {
            file_type: 1,
            interpreter: None,
            dynamic_flags: None,
            no_program_headers: true,
            ..ElfSpec::linux_x86_64()
        };
        let info = elf(&object.build());
        assert_eq!(info.file_type, ElfType::Relocatable);
        assert_eq!(info.linkage, Linkage::Unknown);

        let freebsd = ElfSpec {
            os_abi: 9,
            ..ElfSpec::linux_x86_64()
        };
        let info = elf(&freebsd.build());
        assert_eq!(info.os_abi, ElfOsAbi::FreeBsd);
        assert!(!info.os_abi.is_linux_compatible());
    }

    #[test]
    fn app_image_magic_is_detected() {
        let spec = ElfSpec {
            app_image: Some(2),
            ..ElfSpec::linux_x86_64()
        };
        assert_eq!(elf(&spec.build()).app_image, Some(2));
    }

    #[test]
    fn bad_identification_is_malformed() {
        let mut bytes = ElfSpec::linux_x86_64().build();
        bytes[4] = 7;
        assert!(matches!(
            inspect_bytes(&bytes),
            Err(InspectError::Malformed {
                format: FormatFamily::Elf,
                ..
            })
        ));
        let mut bytes = ElfSpec::linux_x86_64().build();
        bytes[5] = 0;
        assert!(matches!(
            inspect_bytes(&bytes),
            Err(InspectError::Malformed { .. })
        ));
    }

    #[test]
    fn program_header_table_outside_the_file_is_truncated() {
        let mut bytes = ElfSpec::linux_x86_64().build();
        // e_phoff (offset 32, 8 bytes) points far past the end of the file.
        bytes[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            inspect_bytes(&bytes),
            Err(InspectError::Malformed { .. } | InspectError::Truncated { .. })
        ));
        let mut bytes = ElfSpec::linux_x86_64().build();
        bytes[32..40].copy_from_slice(&0x7fff_ffff_ffff_0000u64.to_le_bytes());
        assert_eq!(
            inspect_bytes(&bytes),
            Err(truncated("the program header table"))
        );
    }

    #[test]
    fn interpreter_outside_the_file_is_truncated() {
        let mut bytes = ElfSpec::linux_x86_64().build();
        // The first program header is PT_INTERP; its p_offset is at 64 + 8.
        bytes[72..80].copy_from_slice(&0xffff_ffff_ffff_fff0u64.to_le_bytes());
        assert_eq!(
            inspect_bytes(&bytes),
            Err(truncated("the interpreter path"))
        );
    }

    #[test]
    fn implausible_interpreter_length_is_malformed() {
        let mut bytes = ElfSpec::linux_x86_64().build();
        // p_filesz of the PT_INTERP header is at 64 + 32.
        bytes[96..104].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            inspect_bytes(&bytes),
            Err(InspectError::Malformed { .. })
        ));
    }

    #[test]
    fn undersized_program_header_entries_are_malformed() {
        let mut bytes = ElfSpec::linux_x86_64().build();
        // e_phentsize is at offset 54.
        bytes[54..56].copy_from_slice(&8u16.to_le_bytes());
        assert!(matches!(
            inspect_bytes(&bytes),
            Err(InspectError::Malformed { .. })
        ));
    }
}
