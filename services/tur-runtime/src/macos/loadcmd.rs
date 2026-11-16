// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The load commands of one Mach-O image: the libraries it links, the
//! platform and macOS version it was built for, whether it carries Swift
//! code, whether it is encrypted and whether it has an entry point.
//!
//! Written from the published layout of the Mach-O format (`mach-o/loader.h`
//! as documented in the open-source `cctools` and LLVM's `BinaryFormat`
//! descriptions, read as structure layouts only). Nothing of Apple's is
//! copied here, and nothing is executed or mapped. The image is untrusted:
//! every count and size is checked against the bytes that were read, the
//! load-command block is read once and capped, and strings are bounded and
//! must be printable.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::arch::Endianness;
use crate::error::{FormatFamily, InspectError};
use crate::macho::MachOSlice;
use crate::reader::{Reader, u32_at};

/// The most bytes of load commands one image may declare.
pub const MAX_LOAD_COMMAND_BYTES: u32 = 4 * 1024 * 1024;
/// The most load commands one image may declare.
pub const MAX_LOAD_COMMANDS: u32 = 16_384;
/// The most linked libraries that are listed; the rest are counted.
pub const MAX_LISTED_LIBRARIES: usize = 1024;
/// The longest library or search path that is kept, in bytes.
pub const MAX_NAME_BYTES: usize = 1024;
/// The most Swift section names that are listed.
pub const MAX_SWIFT_SECTIONS: usize = 64;

/// Set on load commands the dynamic linker must understand.
const LC_REQ_DYLD: u32 = 0x8000_0000;
const LC_SEGMENT: u32 = 0x1;
const LC_UNIXTHREAD: u32 = 0x5;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_LOAD_WEAK_DYLIB: u32 = 0x18 | LC_REQ_DYLD;
const LC_SEGMENT_64: u32 = 0x19;
const LC_RPATH: u32 = 0x1c | LC_REQ_DYLD;
const LC_REEXPORT_DYLIB: u32 = 0x1f | LC_REQ_DYLD;
const LC_LAZY_LOAD_DYLIB: u32 = 0x20;
const LC_ENCRYPTION_INFO: u32 = 0x21;
const LC_LOAD_UPWARD_DYLIB: u32 = 0x23 | LC_REQ_DYLD;
const LC_VERSION_MIN_MACOSX: u32 = 0x24;
const LC_VERSION_MIN_IPHONEOS: u32 = 0x25;
const LC_ENCRYPTION_INFO_64: u32 = 0x2c;
const LC_BUILD_VERSION: u32 = 0x32;
const LC_MAIN: u32 = 0x28 | LC_REQ_DYLD;

/// How an image links a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkKind {
    /// `LC_LOAD_DYLIB`: the image does not load without it.
    Load,
    /// `LC_LOAD_WEAK_DYLIB`: the image loads without it.
    Weak,
    /// `LC_REEXPORT_DYLIB`: a library this one re-exports.
    Reexport,
    /// `LC_LAZY_LOAD_DYLIB`: loaded on first use.
    Lazy,
    /// `LC_LOAD_UPWARD_DYLIB`: a library that also links this one.
    Upward,
}

impl LinkKind {
    /// Whether the image fails to load when the library is missing.
    pub const fn required(self) -> bool {
        matches!(self, LinkKind::Load | LinkKind::Reexport | LinkKind::Upward)
    }
}

/// One library an image links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkedLibrary {
    /// The install name, for example
    /// `/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit`.
    pub path: String,
    /// How it is linked.
    pub kind: LinkKind,
}

/// What the load commands of one image say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LoadCommands {
    /// The number of load commands the header declares.
    pub count: u32,
    /// The linked libraries, in order, at most [`MAX_LISTED_LIBRARIES`].
    pub libraries: Vec<LinkedLibrary>,
    /// Linked libraries beyond the listed ones.
    pub unlisted_libraries: u64,
    /// The run-path search paths (`LC_RPATH`).
    pub rpaths: Vec<String>,
    /// The platform the image was built for: `macos`, `mac-catalyst`, `ios`,
    /// `tvos`, `watchos`, `ios-simulator`, `visionos`, `driverkit` or
    /// `other-<n>`. `None` when the image does not say.
    pub platform: Option<String>,
    /// The oldest system version it runs on, for example `11.0`.
    pub minimum_os: Option<String>,
    /// The SDK version it was built with.
    pub sdk: Option<String>,
    /// Names of sections whose name marks Swift metadata (`__swift5_types`
    /// and the like), sorted, at most [`MAX_SWIFT_SECTIONS`].
    pub swift_sections: Vec<String>,
    /// Whether an encryption command says the code is encrypted.
    pub encrypted: bool,
    /// Whether the image has an entry point (`LC_MAIN` or `LC_UNIXTHREAD`).
    pub entry_point: bool,
    /// Things that were seen and skipped: names that are not printable,
    /// lists that were cut.
    pub notes: Vec<String>,
}

fn truncated(detail: &'static str) -> InspectError {
    InspectError::Truncated {
        format: FormatFamily::MachO,
        detail,
    }
}

fn malformed(detail: String) -> InspectError {
    InspectError::Malformed {
        format: FormatFamily::MachO,
        detail,
    }
}

/// Reads the load commands of `slice`, which [`crate::macho`] found in the
/// file behind `reader`.
///
/// # Errors
///
/// [`InspectError`] when the slice holds no Mach-O image, when the header or
/// the load commands are cut short, when a load command is malformed, or when
/// the image declares more load commands than the limits allow.
pub(crate) fn read_load_commands(
    reader: &mut Reader<'_>,
    slice: &MachOSlice,
) -> Result<LoadCommands, InspectError> {
    let endian = slice
        .endianness
        .ok_or_else(|| malformed("the slice holds no Mach-O image".to_owned()))?;
    let wide = slice.bits == 64;
    let header_len: u64 = if wide { 32 } else { 28 };
    let header = reader
        .read(slice.offset, 28)?
        .ok_or_else(|| truncated("the header"))?;
    let count = u32_at(&header, 16, endian).ok_or_else(|| truncated("the header"))?;
    let size = u32_at(&header, 20, endian).ok_or_else(|| truncated("the header"))?;
    if count > MAX_LOAD_COMMANDS || size > MAX_LOAD_COMMAND_BYTES {
        return Err(InspectError::LimitExceeded {
            format: FormatFamily::MachO,
            detail: format!(
                "the image declares {count} load commands in {size} bytes; at most \
                 {MAX_LOAD_COMMANDS} in {MAX_LOAD_COMMAND_BYTES} bytes are read"
            ),
        });
    }
    if u64::from(size) > slice.size.saturating_sub(header_len) {
        return Err(truncated("the load commands"));
    }
    let start = slice
        .offset
        .checked_add(header_len)
        .ok_or_else(|| malformed("the image's offset overflows".to_owned()))?;
    let block_len =
        usize::try_from(size).map_err(|_| malformed("the load commands' size".to_owned()))?;
    let block = reader
        .read(start, block_len)?
        .ok_or_else(|| truncated("the load commands"))?;
    parse_block(&block, count, endian, wide)
}

/// Parses a block of `count` load commands.
///
/// # Errors
///
/// [`InspectError`] when a command is cut short or malformed.
pub(crate) fn parse_block(
    block: &[u8],
    count: u32,
    endian: Endianness,
    wide: bool,
) -> Result<LoadCommands, InspectError> {
    let mut out = LoadCommands {
        count,
        ..LoadCommands::default()
    };
    let mut swift = BTreeSet::new();
    let mut unprintable = 0u64;
    let mut at: usize = 0;
    for index in 0..count {
        let command_type = u32_at(block, at, endian).ok_or_else(|| truncated("a load command"))?;
        let declared = u32_at(block, at.saturating_add(4), endian)
            .ok_or_else(|| truncated("a load command"))?;
        let length = usize::try_from(declared)
            .map_err(|_| malformed(format!("load command {index} has the size {declared}")))?;
        if length < 8 || !length.is_multiple_of(4) {
            return Err(malformed(format!(
                "load command {index} has the size {declared}, which is not a multiple of four \
                 of at least eight"
            )));
        }
        let end = at
            .checked_add(length)
            .ok_or_else(|| malformed(format!("load command {index} overflows")))?;
        let command = block
            .get(at..end)
            .ok_or_else(|| truncated("a load command"))?;
        match command_type {
            LC_LOAD_DYLIB | LC_LOAD_WEAK_DYLIB | LC_REEXPORT_DYLIB | LC_LAZY_LOAD_DYLIB
            | LC_LOAD_UPWARD_DYLIB => {
                let kind = match command_type {
                    LC_LOAD_DYLIB => LinkKind::Load,
                    LC_LOAD_WEAK_DYLIB => LinkKind::Weak,
                    LC_REEXPORT_DYLIB => LinkKind::Reexport,
                    LC_LAZY_LOAD_DYLIB => LinkKind::Lazy,
                    _ => LinkKind::Upward,
                };
                match string_at(command, endian) {
                    Some(path) if out.libraries.len() < MAX_LISTED_LIBRARIES => {
                        out.libraries.push(LinkedLibrary { path, kind });
                    }
                    Some(_) => out.unlisted_libraries = out.unlisted_libraries.saturating_add(1),
                    None => unprintable = unprintable.saturating_add(1),
                }
            }
            LC_RPATH => match string_at(command, endian) {
                Some(path) if out.rpaths.len() < MAX_LISTED_LIBRARIES => out.rpaths.push(path),
                Some(_) => {}
                None => unprintable = unprintable.saturating_add(1),
            },
            LC_BUILD_VERSION => {
                let platform = u32_at(command, 8, endian)
                    .ok_or_else(|| truncated("a build-version command"))?;
                let minimum = u32_at(command, 12, endian)
                    .ok_or_else(|| truncated("a build-version command"))?;
                let sdk = u32_at(command, 16, endian)
                    .ok_or_else(|| truncated("a build-version command"))?;
                // The first platform named wins; a second one (a "zippered"
                // image for macOS and Mac Catalyst) is noted.
                if out.platform.is_none() {
                    out.platform = Some(platform_name(platform));
                    out.minimum_os = Some(version_text(minimum));
                    out.sdk = Some(version_text(sdk));
                } else {
                    out.notes.push(format!(
                        "The image is also built for {}.",
                        platform_name(platform)
                    ));
                }
            }
            LC_VERSION_MIN_MACOSX | LC_VERSION_MIN_IPHONEOS => {
                let minimum = u32_at(command, 8, endian)
                    .ok_or_else(|| truncated("a minimum-version command"))?;
                let sdk = u32_at(command, 12, endian)
                    .ok_or_else(|| truncated("a minimum-version command"))?;
                if out.platform.is_none() {
                    out.platform = Some(
                        if command_type == LC_VERSION_MIN_MACOSX {
                            "macos"
                        } else {
                            "ios"
                        }
                        .to_owned(),
                    );
                    out.minimum_os = Some(version_text(minimum));
                    out.sdk = Some(version_text(sdk));
                }
            }
            LC_SEGMENT_64 => {
                section_names(command, endian, SegmentLayout::WIDE, &mut swift)?;
            }
            LC_SEGMENT => {
                section_names(command, endian, SegmentLayout::NARROW, &mut swift)?;
            }
            LC_ENCRYPTION_INFO | LC_ENCRYPTION_INFO_64 => {
                let id = u32_at(command, 16, endian)
                    .ok_or_else(|| truncated("an encryption command"))?;
                out.encrypted |= id != 0;
            }
            LC_MAIN | LC_UNIXTHREAD => out.entry_point = true,
            _ => {}
        }
        at = end;
    }
    // A 64-bit image aligns its commands to eight bytes; that is not
    // enforced (some linkers do not), only remarked.
    if wide && !at.is_multiple_of(8) {
        out.notes
            .push("The load commands do not end on an eight-byte boundary.".to_owned());
    }
    if unprintable > 0 {
        out.notes.push(format!(
            "{unprintable} library or search paths were not printable or too long and were skipped."
        ));
    }
    if out.unlisted_libraries > 0 {
        out.notes.push(format!(
            "{} more linked libraries were not listed.",
            out.unlisted_libraries
        ));
    }
    out.swift_sections = swift.into_iter().take(MAX_SWIFT_SECTIONS).collect();
    Ok(out)
}

/// Where the parts of a segment command are.
#[derive(Debug, Clone, Copy)]
struct SegmentLayout {
    /// Offset of the section count in the command.
    count_at: usize,
    /// Size of the command before its first section.
    header: usize,
    /// Size of one section entry.
    entry: usize,
}

impl SegmentLayout {
    /// `segment_command_64` and `section_64`.
    const WIDE: SegmentLayout = SegmentLayout {
        count_at: 64,
        header: 72,
        entry: 80,
    };
    /// `segment_command` and `section`.
    const NARROW: SegmentLayout = SegmentLayout {
        count_at: 48,
        header: 56,
        entry: 68,
    };
}

/// Adds the names of the command's sections that mark Swift metadata.
fn section_names(
    command: &[u8],
    endian: Endianness,
    layout: SegmentLayout,
    swift: &mut BTreeSet<String>,
) -> Result<(), InspectError> {
    let count = u32_at(command, layout.count_at, endian)
        .ok_or_else(|| truncated("a segment command"))?;
    let count = usize::try_from(count)
        .map_err(|_| malformed("a segment declares too many sections".to_owned()))?;
    let needed = count
        .checked_mul(layout.entry)
        .and_then(|bytes| bytes.checked_add(layout.header))
        .ok_or_else(|| malformed("a segment declares too many sections".to_owned()))?;
    if needed > command.len() {
        return Err(malformed(format!(
            "a segment declares {count} sections, which do not fit in its command"
        )));
    }
    for index in 0..count {
        let start = layout.header + index * layout.entry;
        let Some(raw) = command.get(start..start + 16) else {
            break;
        };
        if let Some(name) = fixed_name(raw)
            && name.starts_with("__swift")
            && swift.len() < MAX_SWIFT_SECTIONS
        {
            swift.insert(name);
        }
    }
    Ok(())
}

/// A sixteen-byte name field: the bytes up to the first NUL, when printable.
fn fixed_name(raw: &[u8]) -> Option<String> {
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    let name = raw.get(..end)?;
    if name.is_empty() || !name.iter().all(|byte| byte.is_ascii_graphic()) {
        return None;
    }
    String::from_utf8(name.to_vec()).ok()
}

/// The string a `dylib_command` or `rpath_command` points at with the
/// offset in its third field: up to the first NUL inside the command.
fn string_at(command: &[u8], endian: Endianness) -> Option<String> {
    let offset = usize::try_from(u32_at(command, 8, endian)?).ok()?;
    if offset < 12 {
        return None;
    }
    let rest = command.get(offset..)?;
    let end = rest.iter().position(|byte| *byte == 0)?;
    let bytes = rest.get(..end)?;
    if bytes.is_empty() || bytes.len() > MAX_NAME_BYTES {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(text.to_owned())
}

/// A version packed as `xxxx.yy.zz` in 32 bits, as text: `11.0`, `10.13.4`.
pub fn version_text(packed: u32) -> String {
    let major = packed >> 16;
    let minor = (packed >> 8) & 0xff;
    let patch = packed & 0xff;
    if patch == 0 {
        format!("{major}.{minor}")
    } else {
        format!("{major}.{minor}.{patch}")
    }
}

/// The name of a platform number of `LC_BUILD_VERSION`.
fn platform_name(platform: u32) -> String {
    match platform {
        1 => "macos".to_owned(),
        2 => "ios".to_owned(),
        3 => "tvos".to_owned(),
        4 => "watchos".to_owned(),
        6 => "mac-catalyst".to_owned(),
        7 => "ios-simulator".to_owned(),
        10 => "driverkit".to_owned(),
        11 => "visionos".to_owned(),
        other => format!("other-{other}"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    //! Mach-O images are made here, byte by byte, from the published layout.
    //! None is Apple's.

    use super::*;

    /// A load command under construction.
    pub(crate) enum Command<'a> {
        /// A linked library of the given kind.
        Dylib(LinkKind, &'a str),
        /// A run-path search path.
        Rpath(&'a str),
        /// `LC_BUILD_VERSION` with a platform number, minimum and SDK.
        Build(u32, u32, u32),
        /// `LC_VERSION_MIN_MACOSX`.
        MinMacos(u32),
        /// `LC_SEGMENT_64` with sections of these names.
        Segment64(&'a [&'a str]),
        /// `LC_SEGMENT` with sections of these names.
        Segment32(&'a [&'a str]),
        /// `LC_ENCRYPTION_INFO_64` with this crypt id.
        Encryption(u32),
        /// `LC_MAIN`.
        Main,
        /// Raw bytes of a command, size field and all.
        Raw(Vec<u8>),
    }

    fn put(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn pad_to(out: &mut Vec<u8>, multiple: usize) {
        while !out.len().is_multiple_of(multiple) {
            out.push(0);
        }
    }

    fn name16(out: &mut Vec<u8>, name: &str) {
        let mut field = [0u8; 16];
        let bytes = name.as_bytes();
        let len = bytes.len().min(16);
        field[..len].copy_from_slice(&bytes[..len]);
        out.extend_from_slice(&field);
    }

    /// The bytes of one load command, little-endian.
    pub(crate) fn command(command: &Command<'_>) -> Vec<u8> {
        let mut out = Vec::new();
        match command {
            Command::Dylib(kind, path) => {
                let code = match kind {
                    LinkKind::Load => LC_LOAD_DYLIB,
                    LinkKind::Weak => LC_LOAD_WEAK_DYLIB,
                    LinkKind::Reexport => LC_REEXPORT_DYLIB,
                    LinkKind::Lazy => LC_LAZY_LOAD_DYLIB,
                    LinkKind::Upward => LC_LOAD_UPWARD_DYLIB,
                };
                put(&mut out, code);
                put(&mut out, 0); // size, below
                put(&mut out, 24); // name offset
                put(&mut out, 2); // timestamp
                put(&mut out, 0x0001_0000); // current version
                put(&mut out, 0x0001_0000); // compatibility version
                out.extend_from_slice(path.as_bytes());
                out.push(0);
                pad_to(&mut out, 8);
            }
            Command::Rpath(path) => {
                put(&mut out, LC_RPATH);
                put(&mut out, 0);
                put(&mut out, 12);
                out.extend_from_slice(path.as_bytes());
                out.push(0);
                pad_to(&mut out, 8);
            }
            Command::Build(platform, minimum, sdk) => {
                put(&mut out, LC_BUILD_VERSION);
                put(&mut out, 0);
                put(&mut out, *platform);
                put(&mut out, *minimum);
                put(&mut out, *sdk);
                put(&mut out, 0); // no tools
            }
            Command::MinMacos(minimum) => {
                put(&mut out, LC_VERSION_MIN_MACOSX);
                put(&mut out, 0);
                put(&mut out, *minimum);
                put(&mut out, *minimum);
            }
            Command::Segment64(sections) => {
                put(&mut out, LC_SEGMENT_64);
                put(&mut out, 0);
                name16(&mut out, "__TEXT");
                out.extend_from_slice(&[0u8; 32]); // vmaddr, vmsize, fileoff, filesize
                put(&mut out, 5); // maxprot
                put(&mut out, 5); // initprot
                put(&mut out, u32::try_from(sections.len()).unwrap());
                put(&mut out, 0); // flags
                for name in *sections {
                    name16(&mut out, name);
                    name16(&mut out, "__TEXT");
                    out.extend_from_slice(&[0u8; 48]);
                }
            }
            Command::Segment32(sections) => {
                put(&mut out, LC_SEGMENT);
                put(&mut out, 0);
                name16(&mut out, "__TEXT");
                out.extend_from_slice(&[0u8; 16]);
                put(&mut out, 5);
                put(&mut out, 5);
                put(&mut out, u32::try_from(sections.len()).unwrap());
                put(&mut out, 0);
                for name in *sections {
                    name16(&mut out, name);
                    name16(&mut out, "__TEXT");
                    out.extend_from_slice(&[0u8; 36]);
                }
            }
            Command::Encryption(id) => {
                put(&mut out, LC_ENCRYPTION_INFO_64);
                put(&mut out, 0);
                put(&mut out, 0x4000); // cryptoff
                put(&mut out, 0x1000); // cryptsize
                put(&mut out, *id);
                put(&mut out, 0); // pad
            }
            Command::Main => {
                put(&mut out, LC_MAIN);
                put(&mut out, 0);
                out.extend_from_slice(&[0u8; 16]); // entryoff, stacksize
            }
            Command::Raw(bytes) => return bytes.clone(),
        }
        let size = u32::try_from(out.len()).unwrap();
        out[4..8].copy_from_slice(&size.to_le_bytes());
        out
    }

    /// A thin little-endian image: header, then the commands.
    pub(crate) fn image(cpu_type: u32, file_type: u32, commands: &[Command<'_>]) -> Vec<u8> {
        let wide = (cpu_type & 0x0100_0000) != 0;
        let body: Vec<u8> = commands.iter().flat_map(command).collect();
        let mut out = Vec::new();
        put(&mut out, if wide { 0xfeed_facf } else { 0xfeed_face });
        put(&mut out, cpu_type);
        put(&mut out, 3); // subtype
        put(&mut out, file_type);
        put(&mut out, u32::try_from(commands.len()).unwrap());
        put(&mut out, u32::try_from(body.len()).unwrap());
        put(&mut out, 0x0020_0085); // flags
        if wide {
            put(&mut out, 0);
        }
        out.extend_from_slice(&body);
        // Some code after the commands, as a real image has.
        out.extend_from_slice(&[0xc3u8; 64]);
        out
    }

    /// A universal file holding the given thin images.
    pub(crate) fn universal(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0xcafe_babeu32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(images.len()).unwrap().to_be_bytes());
        let mut offset = 8 + 20 * images.len();
        // Slices start on a 4096-byte boundary, as in real files.
        offset = offset.div_ceil(4096) * 4096;
        let mut placed = Vec::new();
        for (cpu_type, image) in images {
            out.extend_from_slice(&cpu_type.to_be_bytes());
            out.extend_from_slice(&3u32.to_be_bytes());
            out.extend_from_slice(&u32::try_from(offset).unwrap().to_be_bytes());
            out.extend_from_slice(&u32::try_from(image.len()).unwrap().to_be_bytes());
            out.extend_from_slice(&12u32.to_be_bytes());
            placed.push((offset, image));
            offset = (offset + image.len()).div_ceil(4096) * 4096;
        }
        for (at, image) in placed {
            out.resize(at, 0);
            out.extend_from_slice(image);
        }
        out
    }

    fn parse(bytes: &[u8]) -> Result<LoadCommands, InspectError> {
        let count = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        let size = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
        let wide = bytes[0] == 0xcf;
        let start = if wide { 32 } else { 28 };
        parse_block(&bytes[start..start + size], count, Endianness::Little, wide)
    }

    const X86_64: u32 = 0x0100_0007;
    const I386: u32 = 7;

    #[test]
    fn libraries_versions_and_entry_point_are_read() {
        let bytes = image(
            X86_64,
            2,
            &[
                Command::Segment64(&["__text", "__stubs"]),
                Command::Build(1, 0x000b_0000, 0x000f_0200),
                Command::Main,
                Command::Dylib(
                    LinkKind::Load,
                    "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit",
                ),
                Command::Dylib(LinkKind::Weak, "/usr/lib/libobjc.A.dylib"),
                Command::Dylib(LinkKind::Reexport, "/usr/lib/libSystem.B.dylib"),
                Command::Rpath("@executable_path/../Frameworks"),
            ],
        );
        let commands = parse(&bytes).unwrap();
        assert_eq!(commands.count, 7);
        assert_eq!(commands.platform.as_deref(), Some("macos"));
        assert_eq!(commands.minimum_os.as_deref(), Some("11.0"));
        assert_eq!(commands.sdk.as_deref(), Some("15.2"));
        assert!(commands.entry_point);
        assert!(!commands.encrypted);
        assert!(commands.swift_sections.is_empty());
        assert_eq!(commands.libraries.len(), 3);
        assert_eq!(commands.libraries[0].kind, LinkKind::Load);
        assert!(commands.libraries[0].path.ends_with("/AppKit"));
        assert_eq!(commands.libraries[1].kind, LinkKind::Weak);
        assert!(!LinkKind::Weak.required());
        assert!(LinkKind::Reexport.required());
        assert_eq!(commands.rpaths, ["@executable_path/../Frameworks"]);
    }

    #[test]
    fn swift_sections_and_encryption_are_seen() {
        let bytes = image(
            X86_64,
            2,
            &[
                Command::Segment64(&["__text", "__swift5_types", "__swift5_proto"]),
                Command::Encryption(1),
            ],
        );
        let commands = parse(&bytes).unwrap();
        assert_eq!(commands.swift_sections, ["__swift5_proto", "__swift5_types"]);
        assert!(commands.encrypted);
        assert!(!commands.entry_point);

        // An encryption command with id 0 is a decrypted image.
        let bytes = image(X86_64, 2, &[Command::Encryption(0)]);
        assert!(!parse(&bytes).unwrap().encrypted);
    }

    #[test]
    fn thirty_two_bit_images_and_old_version_commands() {
        let bytes = image(
            I386,
            2,
            &[
                Command::Segment32(&["__swift2_proto"]),
                Command::MinMacos(0x000a_0d04),
            ],
        );
        let commands = parse(&bytes).unwrap();
        assert_eq!(commands.platform.as_deref(), Some("macos"));
        assert_eq!(commands.minimum_os.as_deref(), Some("10.13.4"));
        assert_eq!(commands.swift_sections, ["__swift2_proto"]);
    }

    #[test]
    fn a_second_platform_is_noted() {
        let bytes = image(
            X86_64,
            2,
            &[
                Command::Build(6, 0x000e_0000, 0x000f_0000),
                Command::Build(1, 0x000b_0000, 0x000f_0000),
            ],
        );
        let commands = parse(&bytes).unwrap();
        assert_eq!(commands.platform.as_deref(), Some("mac-catalyst"));
        assert!(commands.notes.iter().any(|note| note.contains("macos")));
    }

    #[test]
    fn malformed_commands_are_typed_errors() {
        // A command whose size is below eight.
        let mut raw = Vec::new();
        put(&mut raw, LC_MAIN);
        put(&mut raw, 4);
        let bytes = image(X86_64, 2, &[Command::Raw(raw)]);
        assert!(matches!(
            parse(&bytes),
            Err(InspectError::Malformed { .. })
        ));

        // A command that claims to run past the block.
        let mut raw = Vec::new();
        put(&mut raw, LC_MAIN);
        put(&mut raw, 4096);
        out_pad(&mut raw);
        let bytes = image(X86_64, 2, &[Command::Raw(raw)]);
        assert!(matches!(
            parse(&bytes),
            Err(InspectError::Truncated { .. })
        ));

        // More commands declared than the block holds.
        let mut bytes = image(X86_64, 2, &[Command::Main]);
        bytes[16..20].copy_from_slice(&3u32.to_le_bytes());
        assert!(matches!(
            parse(&bytes),
            Err(InspectError::Truncated { .. })
        ));

        // A segment whose section count does not fit in it.
        let mut segment = command(&Command::Segment64(&["__text"]));
        segment[64..68].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        let bytes = image(X86_64, 2, &[Command::Raw(segment)]);
        assert!(matches!(
            parse(&bytes),
            Err(InspectError::Malformed { .. })
        ));
    }

    fn out_pad(raw: &mut Vec<u8>) {
        raw.extend_from_slice(&[0u8; 8]);
    }

    #[test]
    fn hostile_names_are_skipped_not_trusted() {
        // A name offset that points outside the command, and a name with a
        // terminal escape in it.
        let mut outside = command(&Command::Dylib(LinkKind::Load, "/usr/lib/libz.1.dylib"));
        outside[8..12].copy_from_slice(&0x7fff_0000u32.to_le_bytes());
        let escape = command(&Command::Dylib(LinkKind::Load, "/usr/lib/\u{1b}[31mred"));
        let bytes = image(X86_64, 2, &[Command::Raw(outside), Command::Raw(escape)]);
        let commands = parse(&bytes).unwrap();
        assert!(commands.libraries.is_empty());
        assert!(commands.notes.iter().any(|note| note.contains("not printable")));
    }

    #[test]
    fn versions_are_written_as_people_write_them() {
        assert_eq!(version_text(0x000a_0d00), "10.13");
        assert_eq!(version_text(0x000a_0d04), "10.13.4");
        assert_eq!(version_text(0x001a_0000), "26.0");
    }

    #[test]
    fn a_whole_file_is_read_through_the_bounded_reader() {
        let thin = image(X86_64, 2, &[Command::Main]);
        let fat = universal(&[(X86_64, thin.clone()), (0x0100_000c, thin)]);
        let mut reader = Reader::from_bytes(&fat);
        let mut notes = Vec::new();
        let head = reader.read_up_to(0, 4096).unwrap();
        let info = match crate::macho::parse_fat(&mut reader, &head, &mut notes).unwrap() {
            crate::macho::Universal::MachO(info) => info,
            crate::macho::Universal::JavaClass => panic!("not Java"),
        };
        let slice = &info.slices[0];
        let commands = read_load_commands(&mut reader, slice).unwrap();
        assert!(commands.entry_point);

        fn thin_result(bytes: &[u8]) -> Result<LoadCommands, InspectError> {
            let mut reader = Reader::from_bytes(bytes);
            let head = reader.read_up_to(0, 4096).unwrap();
            let info = crate::macho::parse_thin(&reader, &head).unwrap();
            read_load_commands(&mut reader, &info.slices[0])
        }

        // A header that declares more command bytes than the slice holds.
        let mut thin = image(X86_64, 2, &[Command::Main]);
        thin[20..24].copy_from_slice(&0x0010_0000u32.to_le_bytes());
        assert!(matches!(
            thin_result(&thin),
            Err(InspectError::Truncated { .. })
        ));

        // A header that declares more than the limit.
        thin[20..24].copy_from_slice(&(MAX_LOAD_COMMAND_BYTES + 4).to_le_bytes());
        assert!(matches!(
            thin_result(&thin),
            Err(InspectError::LimitExceeded { .. })
        ));
    }
}
