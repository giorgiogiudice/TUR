// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! PE/COFF inspection, reached through the DOS header's `e_lfanew` field.
//!
//! Read: the COFF file header and the fixed part of the optional header
//! (machine, PE32 or PE32+, subsystem, DLL flag) and the CLR data directory.
//! Not read: sections, imports, resources, version information, signatures.

use serde::Serialize;

use crate::arch::{Arch, Endianness};
use crate::error::{FormatFamily, InspectError};
use crate::reader::{Reader, u16_at, u32_at};

const LITTLE: Endianness = Endianness::Little;
const IMAGE_FILE_DLL: u16 = 0x2000;
/// Index of the CLR (.NET) header in the data directory table.
const CLR_DIRECTORY: u32 = 14;

/// The optional-header flavour of a PE image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PeImageFormat {
    /// PE32: 32-bit address space.
    Pe32,
    /// PE32+: 64-bit address space.
    Pe32Plus,
    /// A ROM image.
    Rom,
    /// The file has no optional header (for example a COFF object).
    Absent,
    /// An optional header with an unrecognised magic number.
    Unknown,
}

impl PeImageFormat {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            PeImageFormat::Pe32 => "PE32",
            PeImageFormat::Pe32Plus => "PE32+",
            PeImageFormat::Rom => "ROM image",
            PeImageFormat::Absent => "no optional header",
            PeImageFormat::Unknown => "unrecognised optional header",
        }
    }
}

/// The environment a PE image is built to run in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PeSubsystem {
    /// No subsystem was read or the value is zero.
    Unknown,
    /// Native: runs on the NT kernel interface without Win32. Kernel drivers
    /// and boot-time system programs.
    Native,
    /// A graphical Win32 program.
    WindowsGui,
    /// A console Win32 program.
    WindowsConsole,
    /// An OS/2 console program.
    Os2Console,
    /// A POSIX-subsystem console program.
    PosixConsole,
    /// A Windows 9x native driver.
    NativeWindows,
    /// A Windows CE program.
    WindowsCe,
    /// A UEFI application.
    EfiApplication,
    /// A UEFI boot-service driver.
    EfiBootServiceDriver,
    /// A UEFI runtime driver.
    EfiRuntimeDriver,
    /// A UEFI ROM image.
    EfiRom,
    /// An Xbox program.
    Xbox,
    /// A Windows boot application.
    WindowsBootApplication,
    /// Any other value.
    Other,
}

impl PeSubsystem {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            PeSubsystem::Unknown => "unknown subsystem",
            PeSubsystem::Native => "native subsystem (kernel mode)",
            PeSubsystem::WindowsGui => "graphical",
            PeSubsystem::WindowsConsole => "console",
            PeSubsystem::Os2Console => "OS/2 console",
            PeSubsystem::PosixConsole => "POSIX console",
            PeSubsystem::NativeWindows => "Windows 9x native driver",
            PeSubsystem::WindowsCe => "Windows CE",
            PeSubsystem::EfiApplication => "UEFI application",
            PeSubsystem::EfiBootServiceDriver => "UEFI boot-service driver",
            PeSubsystem::EfiRuntimeDriver => "UEFI runtime driver",
            PeSubsystem::EfiRom => "UEFI ROM image",
            PeSubsystem::Xbox => "Xbox",
            PeSubsystem::WindowsBootApplication => "Windows boot application",
            PeSubsystem::Other => "unrecognised subsystem",
        }
    }

    /// Whether the image runs under UEFI firmware rather than an operating system.
    pub const fn is_efi(self) -> bool {
        matches!(
            self,
            PeSubsystem::EfiApplication
                | PeSubsystem::EfiBootServiceDriver
                | PeSubsystem::EfiRuntimeDriver
                | PeSubsystem::EfiRom
        )
    }

    /// Whether the image runs in, or directly on, a Windows kernel.
    pub const fn requires_windows_kernel(self) -> bool {
        matches!(self, PeSubsystem::Native | PeSubsystem::NativeWindows)
    }

    fn from_raw(raw: u16) -> PeSubsystem {
        match raw {
            0 => PeSubsystem::Unknown,
            1 => PeSubsystem::Native,
            2 => PeSubsystem::WindowsGui,
            3 => PeSubsystem::WindowsConsole,
            5 => PeSubsystem::Os2Console,
            7 => PeSubsystem::PosixConsole,
            8 => PeSubsystem::NativeWindows,
            9 => PeSubsystem::WindowsCe,
            10 => PeSubsystem::EfiApplication,
            11 => PeSubsystem::EfiBootServiceDriver,
            12 => PeSubsystem::EfiRuntimeDriver,
            13 => PeSubsystem::EfiRom,
            14 => PeSubsystem::Xbox,
            16 => PeSubsystem::WindowsBootApplication,
            _ => PeSubsystem::Other,
        }
    }
}

/// What was read from a PE image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PeInfo {
    /// The machine the code is for.
    pub machine: Arch,
    /// The raw COFF machine value.
    pub machine_raw: u16,
    /// PE32, PE32+ or another optional-header flavour.
    pub image_format: PeImageFormat,
    /// The subsystem the image is built for.
    pub subsystem: PeSubsystem,
    /// The raw subsystem value, when an optional header holds one.
    pub subsystem_raw: Option<u16>,
    /// Whether the DLL characteristic is set.
    pub is_dll: bool,
    /// Whether a CLR header is present, which makes this a .NET assembly.
    pub is_dotnet: bool,
    /// Whether the image needs a Windows kernel: a native-subsystem image such
    /// as a `.sys` driver. Wine cannot host these.
    pub requires_windows_kernel: bool,
}

/// What follows the DOS header of an `MZ` file that is not a PE image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MzNewHeader {
    /// There is no pointer to a newer header: a plain MS-DOS program.
    None,
    /// The pointer leads outside the file. The file may be an MS-DOS program
    /// or a truncated Windows program; the two cannot be told apart.
    OutsideFile,
    /// An `NE` header: 16-bit Windows or OS/2.
    Ne,
    /// An `LE` header: a Windows 3.x or 9x virtual device driver, or a DOS
    /// extender program.
    Le,
    /// An `LX` header: 32-bit OS/2.
    Lx,
    /// The pointer leads to something that is none of the above.
    Unrecognised,
}

impl MzNewHeader {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            MzNewHeader::None => "MS-DOS program",
            MzNewHeader::OutsideFile => "header pointer leads outside the file",
            MzNewHeader::Ne => "16-bit Windows or OS/2 program (NE)",
            MzNewHeader::Le => "linear executable (LE)",
            MzNewHeader::Lx => "32-bit OS/2 program (LX)",
            MzNewHeader::Unrecognised => "no recognised newer header",
        }
    }
}

/// An `MZ` file that is not a PE image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MzInfo {
    /// What the DOS header's `e_lfanew` field leads to.
    pub new_header: MzNewHeader,
}

/// The result of parsing a file that starts with `MZ`.
pub(crate) enum Parsed {
    Pe(PeInfo),
    Mz(MzInfo),
}

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

fn mz(new_header: MzNewHeader) -> Parsed {
    Parsed::Mz(MzInfo { new_header })
}

/// Parses a file whose first bytes, `head`, start with `MZ`.
pub(crate) fn parse(reader: &mut Reader<'_>, head: &[u8]) -> Result<Parsed, InspectError> {
    let Some(lfanew) = u32_at(head, 0x3c, LITTLE) else {
        return Ok(mz(MzNewHeader::None));
    };
    if lfanew == 0 {
        return Ok(mz(MzNewHeader::None));
    }
    let lfanew = u64::from(lfanew);
    let Some(signature) = reader.read(lfanew, 4)? else {
        return Ok(mz(MzNewHeader::OutsideFile));
    };
    if signature != b"PE\0\0" {
        let kind = if signature.starts_with(b"NE") {
            MzNewHeader::Ne
        } else if signature.starts_with(b"LE") {
            MzNewHeader::Le
        } else if signature.starts_with(b"LX") {
            MzNewHeader::Lx
        } else {
            MzNewHeader::Unrecognised
        };
        return Ok(mz(kind));
    }

    let coff_at = lfanew
        .checked_add(4)
        .ok_or_else(|| malformed("the header offset overflows"))?;
    let coff = reader
        .read(coff_at, 20)?
        .ok_or_else(|| truncated("the COFF file header"))?;
    let fields = u16_at(&coff, 0, LITTLE)
        .zip(u16_at(&coff, 16, LITTLE))
        .zip(u16_at(&coff, 18, LITTLE));
    let Some(((machine_raw, optional_len), characteristics)) = fields else {
        return Err(truncated("the COFF file header"));
    };

    let optional = if optional_len == 0 {
        Vec::new()
    } else {
        let optional_at = coff_at
            .checked_add(20)
            .ok_or_else(|| malformed("the header offset overflows"))?;
        reader
            .read(optional_at, usize::from(optional_len))?
            .ok_or_else(|| truncated("the optional header"))?
    };

    let image_format = match u16_at(&optional, 0, LITTLE) {
        None => PeImageFormat::Absent,
        Some(0x10b) => PeImageFormat::Pe32,
        Some(0x20b) => PeImageFormat::Pe32Plus,
        Some(0x107) => PeImageFormat::Rom,
        Some(_) => PeImageFormat::Unknown,
    };
    let directories = match image_format {
        PeImageFormat::Pe32 => Some((92, 96)),
        PeImageFormat::Pe32Plus => Some((108, 112)),
        _ => None,
    };
    let (subsystem_raw, is_dotnet) = match directories {
        Some((count_at, table_at)) => {
            let subsystem = u16_at(&optional, 68, LITTLE).ok_or_else(|| {
                malformed("the optional header is too small to hold a subsystem")
            })?;
            let count = u32_at(&optional, count_at, LITTLE).unwrap_or(0);
            let clr = count > CLR_DIRECTORY && directory_present(&optional, table_at + 14 * 8);
            (Some(subsystem), clr)
        }
        None => (None, false),
    };
    let subsystem = subsystem_raw.map_or(PeSubsystem::Unknown, PeSubsystem::from_raw);

    Ok(Parsed::Pe(PeInfo {
        machine: machine(machine_raw),
        machine_raw,
        image_format,
        subsystem,
        subsystem_raw,
        is_dll: (characteristics & IMAGE_FILE_DLL) != 0,
        is_dotnet,
        requires_windows_kernel: subsystem.requires_windows_kernel(),
    }))
}

/// Whether the data directory entry at `at` has a non-zero address and size.
fn directory_present(optional: &[u8], at: usize) -> bool {
    let address = u32_at(optional, at, LITTLE).unwrap_or(0);
    let size = at
        .checked_add(4)
        .and_then(|size_at| u32_at(optional, size_at, LITTLE))
        .unwrap_or(0);
    address != 0 && size != 0
}

fn machine(raw: u16) -> Arch {
    match raw {
        0x014c => Arch::I386,
        0x8664 => Arch::X86_64,
        0xaa64 => Arch::Aarch64,
        0x01c0 | 0x01c2 | 0x01c4 => Arch::Arm,
        0x0200 => Arch::Ia64,
        0x5064 => Arch::Riscv64,
        0x5032 => Arch::Riscv32,
        0x6264 => Arch::Loongarch64,
        0x01f0 | 0x01f1 => Arch::Ppc,
        0x0166 | 0x0266 | 0x0366 | 0x0466 => Arch::Mips,
        _ => Arch::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{Detected, inspect_bytes};
    use crate::testdata::PeSpec;

    fn pe(bytes: &[u8]) -> PeInfo {
        match inspect_bytes(bytes).unwrap().detected {
            Detected::Pe(info) => info,
            other => panic!("expected PE, got {other:?}"),
        }
    }

    fn mz_header(bytes: &[u8]) -> MzNewHeader {
        match inspect_bytes(bytes).unwrap().detected {
            Detected::Mz(info) => info.new_header,
            other => panic!("expected MZ, got {other:?}"),
        }
    }

    #[test]
    fn sixty_four_bit_gui_program() {
        let info = pe(&PeSpec::windows_gui_x86_64().build());
        assert_eq!(info.machine, Arch::X86_64);
        assert_eq!(info.machine_raw, 0x8664);
        assert_eq!(info.image_format, PeImageFormat::Pe32Plus);
        assert_eq!(info.subsystem, PeSubsystem::WindowsGui);
        assert_eq!(info.subsystem_raw, Some(2));
        assert!(!info.is_dll);
        assert!(!info.is_dotnet);
        assert!(!info.requires_windows_kernel);
    }

    #[test]
    fn thirty_two_bit_console_program() {
        let spec = PeSpec {
            machine: 0x014c,
            plus: false,
            subsystem: 3,
            ..PeSpec::windows_gui_x86_64()
        };
        let info = pe(&spec.build());
        assert_eq!(info.machine, Arch::I386);
        assert_eq!(info.image_format, PeImageFormat::Pe32);
        assert_eq!(info.subsystem, PeSubsystem::WindowsConsole);
    }

    #[test]
    fn native_subsystem_requires_a_windows_kernel() {
        let driver = PeSpec {
            subsystem: 1,
            ..PeSpec::windows_gui_x86_64()
        };
        let info = pe(&driver.build());
        assert_eq!(info.subsystem, PeSubsystem::Native);
        assert!(info.requires_windows_kernel);

        let vxd_era = PeSpec {
            machine: 0x014c,
            plus: false,
            subsystem: 8,
            ..PeSpec::windows_gui_x86_64()
        };
        assert!(pe(&vxd_era.build()).requires_windows_kernel);
    }

    #[test]
    fn efi_dll_dotnet_and_arm64() {
        let efi = PeSpec {
            subsystem: 10,
            ..PeSpec::windows_gui_x86_64()
        };
        let info = pe(&efi.build());
        assert_eq!(info.subsystem, PeSubsystem::EfiApplication);
        assert!(info.subsystem.is_efi());
        assert!(!info.requires_windows_kernel);

        let dll = PeSpec {
            characteristics: 0x2022,
            ..PeSpec::windows_gui_x86_64()
        };
        assert!(pe(&dll.build()).is_dll);

        for plus in [true, false] {
            let dotnet = PeSpec {
                plus,
                clr: true,
                ..PeSpec::windows_gui_x86_64()
            };
            assert!(pe(&dotnet.build()).is_dotnet);
        }

        let arm64 = PeSpec {
            machine: 0xaa64,
            ..PeSpec::windows_gui_x86_64()
        };
        assert_eq!(pe(&arm64.build()).machine, Arch::Aarch64);
    }

    #[test]
    fn clr_directory_beyond_the_declared_count_is_ignored() {
        let mut bytes = PeSpec {
            clr: true,
            ..PeSpec::windows_gui_x86_64()
        }
        .build();
        // NumberOfRvaAndSizes of a PE32+ optional header: 0x80 + 24 + 108.
        let count_at = 0x80 + 24 + 108;
        bytes[count_at..count_at + 4].copy_from_slice(&14u32.to_le_bytes());
        assert!(!pe(&bytes).is_dotnet);
    }

    #[test]
    fn header_pointer_near_u32_max_does_not_overflow() {
        for lfanew in [u32::MAX, u32::MAX - 1, u32::MAX - 3, u32::MAX - 24, 0x8000_0000] {
            let mut bytes = PeSpec::windows_gui_x86_64().build();
            bytes[0x3c..0x40].copy_from_slice(&lfanew.to_le_bytes());
            assert_eq!(mz_header(&bytes), MzNewHeader::OutsideFile);
        }
    }

    #[test]
    fn dos_and_sixteen_bit_programs() {
        let mut plain = [0u8; 0x40];
        plain[..2].copy_from_slice(b"MZ");
        assert_eq!(mz_header(&plain), MzNewHeader::None);
        assert_eq!(mz_header(b"MZ\x90\x00"), MzNewHeader::None);

        for (signature, expected) in [
            (b"NE\x05\x00", MzNewHeader::Ne),
            (b"LE\x00\x00", MzNewHeader::Le),
            (b"LX\x00\x00", MzNewHeader::Lx),
            (b"\x00\x00\x00\x00", MzNewHeader::Unrecognised),
        ] {
            let mut bytes = [0u8; 0x48];
            bytes[..2].copy_from_slice(b"MZ");
            bytes[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
            bytes[0x40..0x44].copy_from_slice(signature);
            assert_eq!(mz_header(&bytes), expected);
        }
    }

    #[test]
    fn truncated_headers_are_typed_errors() {
        let bytes = PeSpec::windows_gui_x86_64().build();
        // Signature present, COFF header cut short.
        assert_eq!(
            inspect_bytes(&bytes[..0x80 + 4 + 10]),
            Err(truncated("the COFF file header"))
        );
        // COFF header present, optional header cut short.
        assert_eq!(
            inspect_bytes(&bytes[..0x80 + 24 + 50]),
            Err(truncated("the optional header"))
        );
    }

    #[test]
    fn undersized_optional_header_is_malformed() {
        let mut bytes = PeSpec::windows_gui_x86_64().build();
        // SizeOfOptionalHeader is at e_lfanew + 4 + 16.
        bytes[0x80 + 20..0x80 + 22].copy_from_slice(&40u16.to_le_bytes());
        assert!(matches!(
            inspect_bytes(&bytes),
            Err(InspectError::Malformed {
                format: FormatFamily::Pe,
                ..
            })
        ));
    }

    #[test]
    fn image_without_an_optional_header() {
        let mut bytes = PeSpec::windows_gui_x86_64().build();
        bytes[0x80 + 20..0x80 + 22].copy_from_slice(&0u16.to_le_bytes());
        let info = pe(&bytes);
        assert_eq!(info.image_format, PeImageFormat::Absent);
        assert_eq!(info.subsystem, PeSubsystem::Unknown);
        assert_eq!(info.subsystem_raw, None);
    }
}
