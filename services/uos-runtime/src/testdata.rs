// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Test inputs, built byte by byte.
//!
//! No third-party binary is stored in the repository. Each builder writes the
//! smallest structure the corresponding parser reads, with real field offsets.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::ole;

/// Mach-O processor type: 32-bit x86.
pub(crate) const CPU_I386: u32 = 7;
/// Mach-O processor type: x86-64.
pub(crate) const CPU_X86_64: u32 = 0x0100_0007;
/// Mach-O processor type: 64-bit ARM.
pub(crate) const CPU_ARM64: u32 = 0x0100_000c;
/// Mach-O processor type: 32-bit PowerPC.
pub(crate) const CPU_PPC: u32 = 18;

fn put16(out: &mut Vec<u8>, value: u16, big: bool) {
    if big {
        out.extend_from_slice(&value.to_be_bytes());
    } else {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn put32(out: &mut Vec<u8>, value: u32, big: bool) {
    if big {
        out.extend_from_slice(&value.to_be_bytes());
    } else {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn put64(out: &mut Vec<u8>, value: u64, big: bool) {
    if big {
        out.extend_from_slice(&value.to_be_bytes());
    } else {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// A description of a minimal ELF file.
pub(crate) struct ElfSpec {
    pub(crate) wide: bool,
    pub(crate) big_endian: bool,
    pub(crate) machine: u16,
    pub(crate) file_type: u16,
    pub(crate) os_abi: u8,
    pub(crate) interpreter: Option<&'static str>,
    /// When set, a dynamic section is emitted with this `DT_FLAGS_1` value.
    pub(crate) dynamic_flags: Option<u64>,
    pub(crate) app_image: Option<u8>,
    pub(crate) no_program_headers: bool,
}

impl ElfSpec {
    /// A dynamically linked position-independent executable for x86-64 Linux.
    pub(crate) fn linux_x86_64() -> Self {
        ElfSpec {
            wide: true,
            big_endian: false,
            machine: 62,
            file_type: 3,
            os_abi: 0,
            interpreter: Some("/lib64/ld-linux-x86-64.so.2"),
            dynamic_flags: Some(0x0800_0000),
            app_image: None,
            no_program_headers: false,
        }
    }

    /// The file's bytes: header, program headers (interpreter, dynamic, load,
    /// in that order) and the data they point at.
    pub(crate) fn build(&self) -> Vec<u8> {
        let big = self.big_endian;
        let mut segments: Vec<(u32, Vec<u8>)> = Vec::new();
        if !self.no_program_headers {
            if let Some(path) = self.interpreter {
                let mut data = path.as_bytes().to_vec();
                data.push(0);
                segments.push((3, data));
            }
            if let Some(flags) = self.dynamic_flags {
                let mut data = Vec::new();
                for (tag, value) in [(1u64, 1u64), (0x6fff_fffb, flags), (0, 0)] {
                    if self.wide {
                        put64(&mut data, tag, big);
                        put64(&mut data, value, big);
                    } else {
                        put32(&mut data, tag as u32, big);
                        put32(&mut data, value as u32, big);
                    }
                }
                segments.push((2, data));
            }
            segments.push((1, Vec::new()));
        }

        let header_len: u16 = if self.wide { 64 } else { 52 };
        let entry_len: u16 = if self.wide { 56 } else { 32 };
        let count = u16::try_from(segments.len()).unwrap();
        let table_at = if count == 0 { 0 } else { u64::from(header_len) };

        let mut out = b"\x7fELF".to_vec();
        out.push(if self.wide { 2 } else { 1 });
        out.push(if big { 2 } else { 1 });
        out.push(1);
        out.push(self.os_abi);
        let mut padding = [0u8; 8];
        if let Some(kind) = self.app_image {
            padding[..3].copy_from_slice(&[b'A', b'I', kind]);
        }
        out.extend_from_slice(&padding);
        put16(&mut out, self.file_type, big);
        put16(&mut out, self.machine, big);
        put32(&mut out, 1, big);
        if self.wide {
            put64(&mut out, 0x1000, big); // e_entry
            put64(&mut out, table_at, big); // e_phoff
            put64(&mut out, 0, big); // e_shoff
        } else {
            put32(&mut out, 0x1000, big);
            put32(&mut out, table_at as u32, big);
            put32(&mut out, 0, big);
        }
        put32(&mut out, 0, big); // e_flags
        put16(&mut out, header_len, big); // e_ehsize
        put16(&mut out, entry_len, big); // e_phentsize
        put16(&mut out, count, big); // e_phnum
        put16(&mut out, 0, big); // e_shentsize
        put16(&mut out, 0, big); // e_shnum
        put16(&mut out, 0, big); // e_shstrndx

        let mut data_at = u64::from(header_len) + u64::from(entry_len) * u64::from(count);
        let mut blobs = Vec::new();
        for (kind, data) in &segments {
            let size = u64::try_from(data.len()).unwrap();
            put32(&mut out, *kind, big); // p_type
            if self.wide {
                put32(&mut out, 4, big); // p_flags
                put64(&mut out, data_at, big); // p_offset
                put64(&mut out, 0, big); // p_vaddr
                put64(&mut out, 0, big); // p_paddr
                put64(&mut out, size, big); // p_filesz
                put64(&mut out, size, big); // p_memsz
                put64(&mut out, 1, big); // p_align
            } else {
                put32(&mut out, data_at as u32, big); // p_offset
                put32(&mut out, 0, big); // p_vaddr
                put32(&mut out, 0, big); // p_paddr
                put32(&mut out, size as u32, big); // p_filesz
                put32(&mut out, size as u32, big); // p_memsz
                put32(&mut out, 4, big); // p_flags
                put32(&mut out, 1, big); // p_align
            }
            data_at += size;
            blobs.extend_from_slice(data);
        }
        out.extend_from_slice(&blobs);
        out
    }
}

/// A description of a minimal PE image. `e_lfanew` is always `0x80`.
pub(crate) struct PeSpec {
    pub(crate) machine: u16,
    /// PE32+ when true, PE32 otherwise.
    pub(crate) plus: bool,
    pub(crate) subsystem: u16,
    pub(crate) characteristics: u16,
    /// Whether the CLR data directory is populated.
    pub(crate) clr: bool,
}

impl PeSpec {
    /// A graphical Win32 program for x86-64.
    pub(crate) fn windows_gui_x86_64() -> Self {
        PeSpec {
            machine: 0x8664,
            plus: true,
            subsystem: 2,
            characteristics: 0x0022,
            clr: false,
        }
    }

    /// The file's bytes: DOS header, signature, COFF header, optional header.
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut out = vec![0u8; 0x80];
        out[..2].copy_from_slice(b"MZ");
        out[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        out.extend_from_slice(b"PE\0\0");

        let optional_len: u16 = if self.plus { 240 } else { 224 };
        put16(&mut out, self.machine, false);
        put16(&mut out, 0, false); // NumberOfSections
        put32(&mut out, 0, false); // TimeDateStamp
        put32(&mut out, 0, false); // PointerToSymbolTable
        put32(&mut out, 0, false); // NumberOfSymbols
        put16(&mut out, optional_len, false); // SizeOfOptionalHeader
        put16(&mut out, self.characteristics, false);

        let mut optional = vec![0u8; usize::from(optional_len)];
        let magic: u16 = if self.plus { 0x20b } else { 0x10b };
        optional[..2].copy_from_slice(&magic.to_le_bytes());
        optional[68..70].copy_from_slice(&self.subsystem.to_le_bytes());
        let (count_at, table_at) = if self.plus { (108, 112) } else { (92, 96) };
        optional[count_at..count_at + 4].copy_from_slice(&16u32.to_le_bytes());
        if self.clr {
            let clr_at = table_at + 14 * 8;
            optional[clr_at..clr_at + 4].copy_from_slice(&0x2000u32.to_le_bytes());
            optional[clr_at + 4..clr_at + 8].copy_from_slice(&72u32.to_le_bytes());
        }
        out.extend_from_slice(&optional);
        out
    }
}

/// An `MZ` file whose `e_lfanew` points at `signature` instead of a PE header.
pub(crate) fn mz_with(signature: &[u8; 4]) -> Vec<u8> {
    let mut out = vec![0u8; 0x40];
    out[..2].copy_from_slice(b"MZ");
    out[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    out.extend_from_slice(signature);
    out.extend_from_slice(&[0u8; 60]);
    out
}

/// A thin Mach-O header and nothing after it.
pub(crate) fn macho_thin(cpu_type: u32, file_type: u32, wide: bool, big: bool) -> Vec<u8> {
    let magic: u32 = if wide { 0xfeed_facf } else { 0xfeed_face };
    let mut out = Vec::new();
    // magic, cputype, cpusubtype, filetype, ncmds, sizeofcmds, flags
    for field in [magic, cpu_type, 3, file_type, 0, 0, 0] {
        put32(&mut out, field, big);
    }
    if wide {
        put32(&mut out, 0, big); // reserved
    }
    out
}

/// A universal file with one little-endian thin image per `(cpu type, file
/// type)` pair, using the 64-bit architecture table when `wide`.
pub(crate) fn macho_fat(slices: &[(u32, u32)], wide: bool) -> Vec<u8> {
    let entry_len = if wide { 32 } else { 20 };
    let mut out = Vec::new();
    put32(&mut out, if wide { 0xcafe_babf } else { 0xcafe_babe }, true);
    put32(&mut out, u32::try_from(slices.len()).unwrap(), true);
    let mut offset = u64::try_from(8 + entry_len * slices.len()).unwrap();
    let mut images = Vec::new();
    for &(cpu_type, file_type) in slices {
        let image = macho_thin(cpu_type, file_type, (cpu_type & 0x0100_0000) != 0, false);
        let size = u64::try_from(image.len()).unwrap();
        put32(&mut out, cpu_type, true);
        put32(&mut out, 3, true); // cpusubtype
        if wide {
            put64(&mut out, offset, true);
            put64(&mut out, size, true);
            put32(&mut out, 0, true); // align
            put32(&mut out, 0, true); // reserved
        } else {
            put32(&mut out, offset as u32, true);
            put32(&mut out, size as u32, true);
            put32(&mut out, 0, true); // align
        }
        offset += size;
        images.extend_from_slice(&image);
    }
    out.extend_from_slice(&images);
    out
}

/// Only the eight bytes of a universal header, claiming `count` architectures.
pub(crate) fn raw_fat_header(count: u32) -> Vec<u8> {
    let mut out = Vec::new();
    put32(&mut out, 0xcafe_babe, true);
    put32(&mut out, count, true);
    out
}

/// A zip archive with one empty stored member per name.
pub(crate) fn zip(names: &[&str]) -> Vec<u8> {
    zip_with(names, false)
}

/// As [`zip`]; when `zip64`, the real counts and offsets are in a zip64
/// end-of-central-directory record and the classic record is saturated.
pub(crate) fn zip_with(names: &[&str], zip64: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for name in names {
        let name_len = u16::try_from(name.len()).unwrap();
        let local_at = u32::try_from(out.len()).unwrap();

        out.extend_from_slice(b"PK\x03\x04");
        put16(&mut out, 20, false); // version needed
        put16(&mut out, 0, false); // flags
        put16(&mut out, 0, false); // method: stored
        put16(&mut out, 0, false); // time
        put16(&mut out, 0, false); // date
        put32(&mut out, 0, false); // crc-32
        put32(&mut out, 0, false); // compressed size
        put32(&mut out, 0, false); // uncompressed size
        put16(&mut out, name_len, false);
        put16(&mut out, 0, false); // extra length
        out.extend_from_slice(name.as_bytes());

        directory.extend_from_slice(b"PK\x01\x02");
        put16(&mut directory, 20, false); // version made by
        put16(&mut directory, 20, false); // version needed
        put16(&mut directory, 0, false); // flags
        put16(&mut directory, 0, false); // method
        put16(&mut directory, 0, false); // time
        put16(&mut directory, 0, false); // date
        put32(&mut directory, 0, false); // crc-32
        put32(&mut directory, 0, false); // compressed size
        put32(&mut directory, 0, false); // uncompressed size
        put16(&mut directory, name_len, false);
        put16(&mut directory, 0, false); // extra length
        put16(&mut directory, 0, false); // comment length
        put16(&mut directory, 0, false); // disk number
        put16(&mut directory, 0, false); // internal attributes
        put32(&mut directory, 0, false); // external attributes
        put32(&mut directory, local_at, false);
        directory.extend_from_slice(name.as_bytes());
    }

    let entries = u64::try_from(names.len()).unwrap();
    let directory_at = u64::try_from(out.len()).unwrap();
    let directory_len = u64::try_from(directory.len()).unwrap();
    out.extend_from_slice(&directory);
    if zip64 {
        let record_at = u64::try_from(out.len()).unwrap();
        zip64_records(&mut out, record_at, entries, directory_len, directory_at);
        end_record(&mut out, 0xffff, 0xffff_ffff, 0xffff_ffff);
    } else {
        end_record(
            &mut out,
            u16::try_from(entries).unwrap(),
            u32::try_from(directory_len).unwrap(),
            u32::try_from(directory_at).unwrap(),
        );
    }
    out
}

/// A file that starts like a zip and ends with zip64 records making the given
/// claims. There is no central directory behind them.
pub(crate) fn zip64_claiming(entries: u64, directory_size: u64, directory_offset: u64) -> Vec<u8> {
    let mut out = b"PK\x03\x04".to_vec();
    out.extend_from_slice(&[0u8; 26]);
    let record_at = u64::try_from(out.len()).unwrap();
    zip64_records(
        &mut out,
        record_at,
        entries,
        directory_size,
        directory_offset,
    );
    end_record(&mut out, 0xffff, 0xffff_ffff, 0xffff_ffff);
    out
}

/// Appends a zip64 end-of-central-directory record and its locator.
fn zip64_records(out: &mut Vec<u8>, record_at: u64, entries: u64, size: u64, offset: u64) {
    out.extend_from_slice(b"PK\x06\x06");
    put64(out, 44, false); // size of the rest of this record
    put16(out, 45, false); // version made by
    put16(out, 45, false); // version needed
    put32(out, 0, false); // this disk
    put32(out, 0, false); // disk with the central directory
    put64(out, entries, false); // entries on this disk
    put64(out, entries, false); // entries in total
    put64(out, size, false); // central directory size
    put64(out, offset, false); // central directory offset

    out.extend_from_slice(b"PK\x06\x07");
    put32(out, 0, false); // disk with the zip64 record
    put64(out, record_at, false);
    put32(out, 1, false); // number of disks
}

/// Appends a classic end-of-central-directory record with no comment.
fn end_record(out: &mut Vec<u8>, entries: u16, directory_size: u32, directory_offset: u32) {
    out.extend_from_slice(b"PK\x05\x06");
    put16(out, 0, false); // this disk
    put16(out, 0, false); // disk with the central directory
    put16(out, entries, false); // entries on this disk
    put16(out, entries, false); // entries in total
    put32(out, directory_size, false);
    put32(out, directory_offset, false);
    put16(out, 0, false); // comment length
}

/// An OLE compound file with a header and a root directory entry carrying
/// `clsid`. `sector_shift` is 9 (512-byte sectors) or 12 (4096-byte sectors).
pub(crate) fn ole(clsid: &[u8; 16], sector_shift: u16) -> Vec<u8> {
    let sector = 1usize << sector_shift;
    let mut out = vec![0u8; sector + 128];
    out[..8].copy_from_slice(&ole::MAGIC);
    out[30..32].copy_from_slice(&sector_shift.to_le_bytes());
    // The directory starts in sector 0, which follows the header sector.
    out[48..52].copy_from_slice(&0u32.to_le_bytes());
    out[sector + 80..sector + 96].copy_from_slice(clsid);
    out
}

/// One valid sample of every format the crate recognises, with a name that
/// is safe to use as a file name.
pub(crate) fn samples() -> Vec<(&'static str, Vec<u8>)> {
    let static_executable = ElfSpec {
        file_type: 2,
        interpreter: None,
        dynamic_flags: None,
        ..ElfSpec::linux_x86_64()
    };
    let static_pie = ElfSpec {
        interpreter: None,
        ..ElfSpec::linux_x86_64()
    };
    let i386 = ElfSpec {
        wide: false,
        machine: 3,
        interpreter: Some("/lib/ld-linux.so.2"),
        ..ElfSpec::linux_x86_64()
    };
    let big_endian = ElfSpec {
        wide: false,
        big_endian: true,
        machine: 20,
        interpreter: Some("/lib/ld.so.1"),
        ..ElfSpec::linux_x86_64()
    };
    let app_image = ElfSpec {
        app_image: Some(2),
        ..ElfSpec::linux_x86_64()
    };
    let console = PeSpec {
        machine: 0x014c,
        plus: false,
        subsystem: 3,
        ..PeSpec::windows_gui_x86_64()
    };
    let driver = PeSpec {
        subsystem: 1,
        ..PeSpec::windows_gui_x86_64()
    };
    let dotnet = PeSpec {
        clr: true,
        ..PeSpec::windows_gui_x86_64()
    };
    vec![
        ("elf64-pie", ElfSpec::linux_x86_64().build()),
        ("elf64-static", static_executable.build()),
        ("elf64-static-pie", static_pie.build()),
        ("elf32-i386", i386.build()),
        ("elf32-big-endian", big_endian.build()),
        ("elf64-appimage", app_image.build()),
        ("pe32plus-gui", PeSpec::windows_gui_x86_64().build()),
        ("pe32-console", console.build()),
        ("pe32plus-driver", driver.build()),
        ("pe32plus-dotnet", dotnet.build()),
        ("mz-ne", mz_with(b"NE\x05\x00")),
        ("macho-x86_64", macho_thin(CPU_X86_64, 2, true, false)),
        ("macho-arm64", macho_thin(CPU_ARM64, 2, true, false)),
        ("macho-ppc", macho_thin(CPU_PPC, 2, false, true)),
        ("macho-kext", macho_thin(CPU_X86_64, 0xb, true, false)),
        (
            "macho-universal",
            macho_fat(&[(CPU_X86_64, 2), (CPU_ARM64, 2)], false),
        ),
        (
            "macho-universal-64",
            macho_fat(&[(CPU_I386, 2), (CPU_X86_64, 2)], true),
        ),
        (
            "apk-arm",
            zip(&[
                "AndroidManifest.xml",
                "classes.dex",
                "lib/arm64-v8a/libapp.so",
                "lib/armeabi-v7a/libapp.so",
            ]),
        ),
        (
            "apk-no-native",
            zip(&["AndroidManifest.xml", "classes.dex"]),
        ),
        (
            "aab",
            zip(&[
                "base/manifest/AndroidManifest.xml",
                "base/lib/x86_64/libapp.so",
            ]),
        ),
        (
            "xapk",
            zip(&["manifest.json", "base.apk", "config.x86_64.apk"]),
        ),
        ("apks", zip(&["toc.pb", "splits/base-master.apk"])),
        ("zip", zip(&["readme.txt", "src/main.c"])),
        (
            "zip64",
            zip_with(&["AndroidManifest.xml", "classes.dex"], true),
        ),
        ("msi", ole(&ole::CLSID_MSI, 9)),
        ("msi-4096", ole(&ole::CLSID_MSI, 12)),
        ("script", b"#!/usr/bin/env python3\nprint('hi')\n".to_vec()),
    ]
}

/// A directory under the system temporary directory, removed on drop.
pub(crate) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates a new, empty directory whose name includes `label`.
    pub(crate) fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let process = std::process::id();
        let path = std::env::temp_dir().join(format!("uos-runtime-{process}-{unique}-{label}"));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    /// The directory's path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
