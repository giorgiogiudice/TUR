// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The entry points: identify a path, or bytes already in memory.
//!
//! Identification goes by content only. The file name and its extension are
//! never consulted (specification §2.3), and nothing is executed.

use std::fs::{self, File};
use std::path::Path;

use serde::Serialize;

use crate::android::{AndroidInfo, ZipFacts, ZipInfo};
use crate::arch::Endianness;
use crate::bundle::{self, AppBundleInfo};
use crate::elf::{self, ElfInfo};
use crate::error::InspectError;
use crate::limits::HEAD_LEN;
use crate::macho::{self, MachOInfo};
use crate::ole::{self, OleInfo};
use crate::pe::{self, MzInfo, PeInfo};
use crate::reader::{Reader, u32_at};
use crate::script::{self, ScriptInfo};
use crate::zip;

/// What a file or directory was identified as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "format", rename_all = "kebab-case")]
pub enum Detected {
    /// An ELF file, the Linux executable format.
    Elf(ElfInfo),
    /// A PE image, the Windows executable format.
    Pe(PeInfo),
    /// A file with a DOS `MZ` header that is not a PE image.
    Mz(MzInfo),
    /// A Mach-O file, thin or universal, the macOS executable format.
    MachO(MachOInfo),
    /// A macOS application bundle directory.
    MacAppBundle(AppBundleInfo),
    /// An Android package: APK, AAB, XAPK or APKS.
    Android(AndroidInfo),
    /// An OLE compound file, which includes Windows Installer packages.
    Ole(OleInfo),
    /// A script with an interpreter line.
    Script(ScriptInfo),
    /// A zip archive that is not an Android package.
    Zip(ZipInfo),
    /// Nothing this crate recognises. This is a statement of ignorance, not a
    /// guess: the file may still be a valid program of some other kind.
    Unknown,
}

/// The result of inspecting one file or directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inspection {
    /// The size of the file in bytes; zero for a directory.
    pub file_size: u64,
    /// What the input was identified as.
    pub detected: Detected,
    /// Plain-language remarks about things that were seen but not followed,
    /// or that a reader of the report should know.
    pub notes: Vec<String>,
}

/// Inspects the file or directory at `path`.
///
/// A directory is examined as a macOS application bundle. A symbolic link at
/// `path` itself is followed, because the caller named it; links inside a
/// bundle are not.
///
/// # Errors
///
/// [`InspectError::Io`] when the path cannot be read or is neither a regular
/// file nor a directory; the other variants when a recognised format is
/// truncated, malformed or exceeds an inspection limit.
pub fn inspect(path: &Path) -> Result<Inspection, InspectError> {
    let metadata = fs::metadata(path).map_err(|error| InspectError::io(path, &error))?;
    if metadata.is_dir() {
        return inspect_directory(path);
    }
    inspect_file(path)
}

/// Inspects bytes already in memory, as if they were the whole of a file.
///
/// # Errors
///
/// As for [`inspect`], except that there is no I/O to fail.
pub fn inspect_bytes(data: &[u8]) -> Result<Inspection, InspectError> {
    inspect_reader(&mut Reader::from_bytes(data))
}

/// Inspects a regular file. Anything else, a device or a pipe for example, is
/// refused before it is opened.
pub(crate) fn inspect_file(path: &Path) -> Result<Inspection, InspectError> {
    let metadata = fs::metadata(path).map_err(|error| InspectError::io(path, &error))?;
    if !metadata.is_file() {
        return Err(InspectError::Io {
            path: path.display().to_string(),
            message: "it is neither a regular file nor a directory".to_owned(),
        });
    }
    let file = File::open(path).map_err(|error| InspectError::io(path, &error))?;
    let mut reader = Reader::from_file(file, metadata.len());
    inspect_reader(&mut reader).map_err(|error| error.with_path(path))
}

fn inspect_directory(path: &Path) -> Result<Inspection, InspectError> {
    let mut notes = Vec::new();
    let detected = if bundle::is_bundle(path) {
        Detected::MacAppBundle(bundle::inspect_bundle(path, &mut notes)?)
    } else {
        notes.push(
            "This is a directory without the layout of a macOS application bundle.".to_owned(),
        );
        Detected::Unknown
    };
    Ok(Inspection {
        file_size: 0,
        detected,
        notes,
    })
}

fn inspect_reader(reader: &mut Reader<'_>) -> Result<Inspection, InspectError> {
    let head = reader.read_up_to(0, HEAD_LEN)?;
    let mut notes = Vec::new();
    let detected = detect(reader, &head, &mut notes)?;
    Ok(Inspection {
        file_size: reader.size(),
        detected,
        notes,
    })
}

fn detect(
    reader: &mut Reader<'_>,
    head: &[u8],
    notes: &mut Vec<String>,
) -> Result<Detected, InspectError> {
    if head.starts_with(b"\x7fELF") {
        return elf::parse(reader, head, notes).map(Detected::Elf);
    }
    if head.starts_with(b"MZ") {
        return Ok(match pe::parse(reader, head)? {
            pe::Parsed::Pe(info) => Detected::Pe(info),
            pe::Parsed::Mz(info) => Detected::Mz(info),
        });
    }
    if let Some(magic) = u32_at(head, 0, Endianness::Big) {
        if macho::is_thin_magic(magic) {
            return macho::parse_thin(reader, head).map(Detected::MachO);
        }
        if macho::is_fat_magic(magic) {
            return Ok(match macho::parse_fat(reader, head, notes)? {
                macho::Universal::MachO(info) => Detected::MachO(info),
                macho::Universal::JavaClass => {
                    notes.push(
                        "This is a Java class file. It shares its magic number with universal \
                         Mach-O files but is not one."
                            .to_owned(),
                    );
                    Detected::Unknown
                }
            });
        }
    }
    if zip::is_zip(head) {
        let mut facts = ZipFacts::default();
        let entries = zip::for_each_name(reader, &mut |name| facts.visit(name))?;
        return Ok(facts.classify(entries, notes));
    }
    if head.starts_with(&ole::MAGIC) {
        return ole::parse(reader, head).map(Detected::Ole);
    }
    if let Some(found) = script::parse(head) {
        return Ok(Detected::Script(found));
    }
    Ok(Detected::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::{ElfSpec, PeSpec, TempDir, samples};

    #[test]
    fn every_sample_is_identified() {
        for (name, bytes) in samples() {
            let inspection =
                inspect_bytes(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_ne!(inspection.detected, Detected::Unknown, "{name}");
            assert_eq!(inspection.file_size, u64::try_from(bytes.len()).unwrap());
        }
    }

    #[test]
    fn every_prefix_of_every_sample_returns_without_panicking() {
        for (name, bytes) in samples() {
            for len in 0..bytes.len() {
                // A typed error, "unknown" or a partial identification are all
                // acceptable for a truncated file. A panic is not.
                let outcome = inspect_bytes(&bytes[..len]);
                if let Ok(inspection) = outcome {
                    assert_eq!(inspection.file_size, u64::try_from(len).unwrap(), "{name}");
                }
            }
        }
    }

    #[test]
    fn single_byte_corruption_never_panics() {
        for (_, bytes) in samples() {
            let mut corrupted = bytes.clone();
            for at in 0..bytes.len().min(700) {
                let original = bytes[at];
                for value in [0x00, 0xff, 0x7f, original ^ 0x80, original.wrapping_add(1)] {
                    corrupted[at] = value;
                    let _ = inspect_bytes(&corrupted);
                }
                corrupted[at] = original;
            }
        }
    }

    #[test]
    fn pseudo_random_bodies_behind_real_magic_numbers_never_panic() {
        let magics: [&[u8]; 9] = [
            b"\x7fELF\x02\x01\x01",
            b"\x7fELF\x01\x02\x01",
            b"MZ",
            b"\xcf\xfa\xed\xfe",
            b"\xfe\xed\xfa\xce",
            b"\xca\xfe\xba\xbe\x00\x00\x00\x03",
            b"PK\x03\x04",
            b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1",
            b"#!/",
        ];
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        for round in 0..400usize {
            for magic in magics {
                let mut bytes = magic.to_vec();
                let len = 16 + (round * 7) % 900;
                while bytes.len() < len {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    bytes.extend_from_slice(&state.to_le_bytes());
                }
                if round.is_multiple_of(3) {
                    // Give zip parsing an end record to find.
                    bytes.extend_from_slice(b"PK\x05\x06");
                    bytes.extend_from_slice(&state.to_le_bytes());
                    bytes.extend_from_slice(&state.to_be_bytes());
                    bytes.extend_from_slice(&[0, 0]);
                }
                let _ = inspect_bytes(&bytes);
            }
        }
    }

    #[test]
    fn unrecognised_and_empty_input_is_unknown() {
        let inputs: [&[u8]; 6] = [b"", b"\x00", b"hello world\n", b"\x7fEL", b"PK", b"M"];
        for bytes in inputs {
            let inspection = inspect_bytes(bytes).unwrap();
            assert_eq!(inspection.detected, Detected::Unknown);
        }
    }

    #[test]
    fn the_file_name_extension_is_not_consulted() {
        let temp = TempDir::new("extension");
        let elf_named_exe = temp.path().join("setup.exe");
        fs::write(&elf_named_exe, ElfSpec::linux_x86_64().build()).unwrap();
        assert!(matches!(
            inspect(&elf_named_exe).unwrap().detected,
            Detected::Elf(_)
        ));

        let pe_named_sh = temp.path().join("run.sh");
        fs::write(&pe_named_sh, PeSpec::windows_gui_x86_64().build()).unwrap();
        assert!(matches!(
            inspect(&pe_named_sh).unwrap().detected,
            Detected::Pe(_)
        ));

        let text_named_app = temp.path().join("notes.apk");
        fs::write(&text_named_app, b"just some text").unwrap();
        assert_eq!(
            inspect(&text_named_app).unwrap().detected,
            Detected::Unknown
        );
    }

    #[test]
    fn files_on_disk_match_in_memory_inspection() {
        let temp = TempDir::new("on-disk");
        for (name, bytes) in samples() {
            let path = temp.path().join(name);
            fs::write(&path, &bytes).unwrap();
            assert_eq!(inspect(&path), inspect_bytes(&bytes), "{name}");
        }
    }

    #[test]
    fn unreadable_paths_are_io_errors() {
        let temp = TempDir::new("missing");
        let missing = temp.path().join("does-not-exist");
        let error = inspect(&missing).unwrap_err();
        assert!(matches!(error, InspectError::Io { .. }));
        assert!(error.to_string().contains("does-not-exist"));
    }

    #[cfg(unix)]
    #[test]
    fn special_files_are_refused_without_being_opened() {
        let error = inspect(Path::new("/dev/null")).unwrap_err();
        assert!(matches!(error, InspectError::Io { .. }));
    }

    #[test]
    fn json_output_is_tagged_with_the_format() {
        let inspection = inspect_bytes(&ElfSpec::linux_x86_64().build()).unwrap();
        let json = serde_json::to_value(inspection).unwrap();
        assert_eq!(json["detected"]["format"], "elf");
        assert_eq!(json["detected"]["machine"], "x86_64");
        assert_eq!(json["detected"]["class"], "elf64");
        assert_eq!(json["detected"]["file_type"], "pie-executable");
        assert_eq!(
            json["detected"]["interpreter"],
            "/lib64/ld-linux-x86-64.so.2"
        );

        let unknown = inspect_bytes(b"nothing").unwrap();
        let json = serde_json::to_value(unknown).unwrap();
        assert_eq!(json["detected"]["format"], "unknown");

        let pe = inspect_bytes(&PeSpec::windows_gui_x86_64().build()).unwrap();
        let json = serde_json::to_value(pe).unwrap();
        assert_eq!(json["detected"]["format"], "pe");
        assert_eq!(json["detected"]["image_format"], "pe32-plus");
        assert_eq!(json["detected"]["subsystem"], "windows-gui");
    }
}
