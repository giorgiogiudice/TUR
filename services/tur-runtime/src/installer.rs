// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What kind of installer a file is, decided from its content.
//!
//! The file name is never consulted. What is looked at:
//!
//! | Kind | Recognised by |
//! |---|---|
//! | Windows Installer package | The OLE compound-file magic and the root storage class of a package ([`crate::ole`]) |
//! | NSIS | The NSIS first header (`0xDEADBEEF`, `NullsoftInst`) in the data appended to the program |
//! | Inno Setup | The Inno Setup loader's offset table (`rDlPtS`) inside the program image |
//! | 7-Zip's installer | A 7z archive appended to a program whose version resource calls it `7-Zip Installer` |
//! | Portable archive | A zip archive with at least one `.exe` member |
//! | Plain program | A Windows program with none of the above |
//!
//! Each kind that has a documented unattended mode carries the arguments for
//! it and where they are documented. Nothing is executed here.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::arch::Arch;
use crate::codes::Code;
use crate::error::InspectError;
use crate::inspect::{Detected, inspect};
use crate::ole::OleKind;
use crate::pe_res::{self, find, utf16le};
use crate::zip;

/// Bytes of appended data searched for an installer's signature.
const OVERLAY_SEARCH_BYTES: usize = 64 * 1024;
/// Bytes of the program image searched for an installer's signature.
const IMAGE_SEARCH_BYTES: usize = 8 * 1024 * 1024;

/// The NSIS first header: the signature `0xDEADBEEF`, then `NullsoftInst`.
const NSIS_MAGIC: &[u8] = b"\xef\xbe\xad\xdeNullsoftInst";
/// The identifier of the Inno Setup loader's offset table.
const INNO_MAGIC: &[u8] = b"rDlPtS";
/// The first bytes of a 7z archive.
const SEVEN_ZIP_MAGIC: &[u8] = b"7z\xbc\xaf\x27\x1c";
/// What 7-Zip's own installer calls itself in its version resource.
const SEVEN_ZIP_INSTALLER: &str = "7-Zip Installer";

/// A kind of installer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallerKind {
    /// A Windows Installer package, run by `msiexec`.
    Msi,
    /// An installer built with the Nullsoft Scriptable Install System.
    Nsis,
    /// An installer built with Inno Setup.
    InnoSetup,
    /// The installer 7-Zip ships for itself.
    SevenZipInstaller,
    /// A zip archive that holds a program to be unpacked, not installed.
    PortableZip,
    /// A Windows program of no recognised installer kind. It may be the
    /// application itself or an installer this build does not know.
    PlainExe,
}

impl InstallerKind {
    /// Every kind, in schema order.
    pub const ALL: [InstallerKind; 6] = [
        InstallerKind::Msi,
        InstallerKind::Nsis,
        InstallerKind::InnoSetup,
        InstallerKind::SevenZipInstaller,
        InstallerKind::PortableZip,
        InstallerKind::PlainExe,
    ];

    /// The kind's machine-readable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            InstallerKind::Msi => "msi",
            InstallerKind::Nsis => "nsis",
            InstallerKind::InnoSetup => "inno-setup",
            InstallerKind::SevenZipInstaller => "seven-zip-installer",
            InstallerKind::PortableZip => "portable-zip",
            InstallerKind::PlainExe => "plain-exe",
        }
    }

    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            InstallerKind::Msi => "Windows Installer package (MSI)",
            InstallerKind::Nsis => "NSIS installer",
            InstallerKind::InnoSetup => "Inno Setup installer",
            InstallerKind::SevenZipInstaller => "7-Zip's own installer",
            InstallerKind::PortableZip => "portable zip archive",
            InstallerKind::PlainExe => "Windows program, no installer kind recognised",
        }
    }

    /// The arguments that make an installer of this kind run without asking
    /// anything, and where they are documented. `None` for the kinds that
    /// are not run as installers.
    pub const fn unattended(self) -> Option<Unattended> {
        match self {
            InstallerKind::Msi => Some(Unattended {
                arguments: &["/qn"],
                documented_at: "https://learn.microsoft.com/windows-server/administration/windows-commands/msiexec",
            }),
            InstallerKind::Nsis => Some(Unattended {
                arguments: &["/S"],
                documented_at: "https://nsis.sourceforge.io/Docs/Chapter3.html",
            }),
            InstallerKind::InnoSetup => Some(Unattended {
                arguments: &["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"],
                documented_at: "https://jrsoftware.org/ishelp/topic_setupcmdline.htm",
            }),
            InstallerKind::SevenZipInstaller => Some(Unattended {
                arguments: &["/S"],
                documented_at: "https://www.7-zip.org/faq.html",
            }),
            InstallerKind::PortableZip | InstallerKind::PlainExe => None,
        }
    }

    /// The arguments that make the uninstaller of an application installed
    /// by this kind run without asking anything.
    pub const fn unattended_uninstall(self) -> &'static [&'static str] {
        match self {
            InstallerKind::Msi => &["/qn"],
            InstallerKind::Nsis | InstallerKind::SevenZipInstaller => &["/S"],
            InstallerKind::InnoSetup => &["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"],
            InstallerKind::PortableZip | InstallerKind::PlainExe => &[],
        }
    }
}

/// How an installer is run without asking anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Unattended {
    /// The arguments.
    pub arguments: &'static [&'static str],
    /// The page that documents them.
    pub documented_at: &'static str,
}

/// What a file was identified as, for installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallerInfo {
    /// The kind.
    pub kind: InstallerKind,
    /// The kind's label.
    pub label: &'static str,
    /// The architecture of the installer program itself, when it is one. An
    /// installer for a 64-bit application is often a 32-bit program.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<Arch>,
    /// The size of the file in bytes.
    pub file_size: u64,
    /// What in the file's content identified it.
    pub evidence: String,
    /// How it is run unattended, when its kind has a documented way.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unattended: Option<Unattended>,
    /// Remarks a reader should know.
    pub notes: Vec<String>,
}

/// Why a file cannot be installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentifyError {
    /// The file could not be read, or is a damaged file of a known format.
    Inspect(InspectError),
    /// The file was read and is not something this build installs.
    NotInstallable {
        /// What the file is, in plain language.
        what: String,
    },
}

impl IdentifyError {
    /// The stable error code for this failure.
    pub fn code(&self) -> Code {
        match self {
            IdentifyError::Inspect(error) => error.code(),
            IdentifyError::NotInstallable { .. } => Code::InstallerUnrecognised,
        }
    }
}

impl fmt::Display for IdentifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentifyError::Inspect(error) => write!(f, "{error}"),
            IdentifyError::NotInstallable { what } => write!(
                f,
                "this file cannot be installed as a Windows application: it is {what}"
            ),
        }
    }
}

impl std::error::Error for IdentifyError {}

impl From<InspectError> for IdentifyError {
    fn from(error: InspectError) -> Self {
        IdentifyError::Inspect(error)
    }
}

fn not_installable(what: &str) -> IdentifyError {
    IdentifyError::NotInstallable {
        what: what.to_owned(),
    }
}

/// Identifies the installer at `path` from its content. Nothing is executed.
///
/// # Errors
///
/// [`IdentifyError::Inspect`] when the file cannot be read or is damaged,
/// and [`IdentifyError::NotInstallable`] when it is not a Windows installer,
/// program or archive of programs.
pub fn identify(path: &Path) -> Result<InstallerInfo, IdentifyError> {
    let inspection = inspect(path)?;
    let file_size = inspection.file_size;
    let info = |kind: InstallerKind, architecture: Option<Arch>, evidence: String| InstallerInfo {
        kind,
        label: kind.label(),
        architecture,
        file_size,
        evidence,
        unattended: kind.unattended(),
        notes: Vec::new(),
    };
    match &inspection.detected {
        Detected::Ole(ole) => match ole.kind {
            OleKind::MsiInstaller => Ok(info(
                InstallerKind::Msi,
                None,
                "an OLE compound file whose root storage has the class of an installer package"
                    .to_owned(),
            )),
            OleKind::MsiPatch => Err(not_installable(
                "a Windows Installer patch, which changes an installed application",
            )),
            OleKind::MsiTransform => Err(not_installable(
                "a Windows Installer transform, which modifies a package",
            )),
            OleKind::Unidentified => Err(not_installable(
                "an OLE compound file that is not an installer package",
            )),
        },
        Detected::Zip(_) => {
            let programs = count_programs(path)?;
            if programs == 0 {
                return Err(not_installable(
                    "a zip archive with no Windows program in it",
                ));
            }
            let mut found = info(
                InstallerKind::PortableZip,
                None,
                format!("a zip archive with {programs} member(s) named like a Windows program"),
            );
            found.notes.push(
                "A portable archive is unpacked into the application's prefix; nothing in it is \
                 run during installation."
                    .to_owned(),
            );
            Ok(found)
        }
        Detected::Pe(pe) => {
            if pe.is_dll {
                return Err(not_installable("a Windows library (DLL), not a program"));
            }
            if pe.requires_windows_kernel {
                return Err(not_installable(
                    "a program for the Windows kernel, which no compatibility layer can host",
                ));
            }
            if !matches!(pe.machine, Arch::X86_64 | Arch::I386) {
                return Err(not_installable(
                    "a Windows program for a processor architecture that cannot be run here",
                ));
            }
            let (kind, evidence, note) = scan_program(path)?;
            let mut found = info(kind, Some(pe.machine), evidence);
            found.notes.extend(note);
            if pe.machine == Arch::I386 {
                found.notes.push(
                    "The installer is a 32-bit program, whatever it installs: the runtime needs \
                     its 32-bit support to run it."
                        .to_owned(),
                );
            }
            Ok(found)
        }
        Detected::Mz(_) => Err(not_installable(
            "an MS-DOS or 16-bit Windows program",
        )),
        Detected::Elf(_) | Detected::Script(_) => {
            Err(not_installable("a Linux program, which needs no installer"))
        }
        Detected::MachO(_) | Detected::MacAppBundle(_) => {
            Err(not_installable("a macOS program"))
        }
        Detected::Android(_) => Err(not_installable("an Android package")),
        Detected::Unknown => Err(not_installable("of a format that was not recognised")),
    }
}

/// How many members of the zip archive at `path` are named like a Windows
/// program.
fn count_programs(path: &Path) -> Result<u64, InspectError> {
    let mut reader = pe_res::open(path)?;
    let mut programs: u64 = 0;
    zip::for_each_name(&mut reader, &mut |name: &[u8]| {
        if name.len() > 4
            && name
                .get(name.len() - 4..)
                .is_some_and(|end| end.eq_ignore_ascii_case(b".exe"))
        {
            programs += 1;
        }
    })
    .map_err(|error| error.with_path(path))?;
    Ok(programs)
}

/// Tells the installer kinds that are Windows programs apart. Returns the
/// kind, what identified it, and a remark when there is one.
fn scan_program(path: &Path) -> Result<(InstallerKind, String, Option<String>), InspectError> {
    let mut reader = pe_res::open(path)?;
    let Some(layout) = pe_res::layout(&mut reader).map_err(|error| error.with_path(path))? else {
        return Ok((
            InstallerKind::PlainExe,
            "a Windows program".to_owned(),
            None,
        ));
    };
    let overlay_at = layout.overlay_offset;
    let overlay = reader
        .read_up_to(overlay_at, OVERLAY_SEARCH_BYTES)
        .map_err(|error| error.with_path(path))?;
    if let Some(at) = find(&overlay, NSIS_MAGIC) {
        // The signature is four bytes into the header, after its flags.
        let header_at = overlay_at + u64::try_from(at.saturating_sub(4)).unwrap_or(0);
        return Ok((
            InstallerKind::Nsis,
            format!("the NSIS first header at offset {header_at}, in the data appended to the program"),
            None,
        ));
    }

    let image_len = usize::try_from(overlay_at).map_or(IMAGE_SEARCH_BYTES, |length| {
        length.min(IMAGE_SEARCH_BYTES)
    });
    let image = reader
        .read_up_to(0, image_len)
        .map_err(|error| error.with_path(path))?;
    if overlay.starts_with(SEVEN_ZIP_MAGIC) {
        if find(&image, &utf16le(SEVEN_ZIP_INSTALLER)).is_some() {
            return Ok((
                InstallerKind::SevenZipInstaller,
                format!(
                    "a 7z archive appended at offset {overlay_at} to a program whose version \
                     resource calls it \"{SEVEN_ZIP_INSTALLER}\""
                ),
                None,
            ));
        }
        return Ok((
            InstallerKind::PlainExe,
            format!("a Windows program with a 7z archive appended at offset {overlay_at}"),
            Some(
                "This looks like a 7z self-extracting archive. No unattended mode is known for \
                 it."
                .to_owned(),
            ),
        ));
    }
    if let Some(at) = find(&image, INNO_MAGIC) {
        return Ok((
            InstallerKind::InnoSetup,
            format!("the Inno Setup loader's offset table at offset {at}, inside the program image"),
            None,
        ));
    }
    Ok((
        InstallerKind::PlainExe,
        "a Windows program with no installer signature this build knows".to_owned(),
        None,
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ole;
    use crate::pe_res::tests::{image, version_resource};
    use crate::testdata::{self, PeSpec, TempDir};

    /// A program shaped like an NSIS installer.
    pub(crate) fn nsis_installer() -> Vec<u8> {
        let mut overlay = vec![0u8; 4];
        overlay.extend_from_slice(NSIS_MAGIC);
        overlay.extend_from_slice(&[0x5d; 64]);
        image(0x014c, &[], &overlay)
    }

    /// A program shaped like an Inno Setup installer.
    pub(crate) fn inno_installer() -> Vec<u8> {
        let mut table = INNO_MAGIC.to_vec();
        table.extend_from_slice(&[0xcd, 0xe6, 0xd7, 0x7b, 0x0b, 0x2a]);
        table.extend_from_slice(&[0u8; 52]);
        image(0x8664, &[(10, 11111, table)], b"zlb\x1a compressed setup data")
    }

    /// A program shaped like 7-Zip's own installer.
    pub(crate) fn seven_zip_installer() -> Vec<u8> {
        let version = version_resource(&[("FileDescription", SEVEN_ZIP_INSTALLER)]);
        let mut overlay = SEVEN_ZIP_MAGIC.to_vec();
        overlay.extend_from_slice(&[0u8; 32]);
        image(0x8664, &[(16, 1, version)], &overlay)
    }

    fn identify_bytes(label: &str, name: &str, bytes: &[u8]) -> Result<InstallerInfo, IdentifyError> {
        let temp = TempDir::new(label);
        let path = temp.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        identify(&path)
    }

    #[test]
    fn every_kind_is_recognised_from_content() {
        // The names are deliberately wrong or meaningless: only the content
        // decides.
        let cases: Vec<(&str, Vec<u8>, InstallerKind, Option<Arch>)> = vec![
            ("document.txt", testdata::ole(&ole::CLSID_MSI, 9), InstallerKind::Msi, None),
            ("a.msi", nsis_installer(), InstallerKind::Nsis, Some(Arch::I386)),
            ("b.zip", inno_installer(), InstallerKind::InnoSetup, Some(Arch::X86_64)),
            (
                "c",
                seven_zip_installer(),
                InstallerKind::SevenZipInstaller,
                Some(Arch::X86_64),
            ),
            (
                "d.exe",
                testdata::zip(&["app/readme.txt", "app/Program.EXE"]),
                InstallerKind::PortableZip,
                None,
            ),
            (
                "e.msi",
                image(0x8664, &[], b""),
                InstallerKind::PlainExe,
                Some(Arch::X86_64),
            ),
        ];
        for (name, bytes, kind, architecture) in cases {
            let info = identify_bytes("installer-kind", name, &bytes)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(info.kind, kind, "{name}");
            assert_eq!(info.architecture, architecture, "{name}");
            assert_eq!(info.label, kind.label());
            assert_eq!(info.unattended, kind.unattended(), "{name}");
            assert!(!info.evidence.is_empty());
            assert_eq!(info.file_size, u64::try_from(bytes.len()).unwrap());
        }
    }

    #[test]
    fn unattended_arguments_are_the_documented_ones() {
        let arguments = |kind: InstallerKind| kind.unattended().map(|found| found.arguments.to_vec());
        assert_eq!(arguments(InstallerKind::Msi), Some(vec!["/qn"]));
        assert_eq!(arguments(InstallerKind::Nsis), Some(vec!["/S"]));
        assert_eq!(
            arguments(InstallerKind::InnoSetup),
            Some(vec!["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
        );
        assert_eq!(arguments(InstallerKind::SevenZipInstaller), Some(vec!["/S"]));
        assert_eq!(arguments(InstallerKind::PortableZip), None);
        assert_eq!(arguments(InstallerKind::PlainExe), None);
        for kind in InstallerKind::ALL {
            if let Some(found) = kind.unattended() {
                assert!(found.documented_at.starts_with("https://"), "{kind:?}");
            }
            // The wire name is the one the schema lists.
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::from(kind.as_str())
            );
        }
        assert_eq!(
            InstallerKind::InnoSetup.unattended_uninstall(),
            ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]
        );
        assert!(InstallerKind::PortableZip.unattended_uninstall().is_empty());
    }

    #[test]
    fn a_thirty_two_bit_installer_says_so() {
        let info = identify_bytes("installer-32", "setup", &nsis_installer()).unwrap();
        assert!(info.notes.iter().any(|note| note.contains("32-bit")));
        let info = identify_bytes("installer-64", "setup", &inno_installer()).unwrap();
        assert!(!info.notes.iter().any(|note| note.contains("32-bit")));
    }

    #[test]
    fn a_7z_archive_on_another_program_is_not_called_an_installer() {
        let mut overlay = SEVEN_ZIP_MAGIC.to_vec();
        overlay.extend_from_slice(&[0u8; 32]);
        let bytes = image(0x8664, &[], &overlay);
        let info = identify_bytes("installer-sfx", "archive.exe", &bytes).unwrap();
        assert_eq!(info.kind, InstallerKind::PlainExe);
        assert_eq!(info.unattended, None);
        assert!(info.notes.iter().any(|note| note.contains("self-extracting")));
    }

    #[test]
    fn what_cannot_be_installed_is_refused_with_a_reason() {
        let cases: Vec<(Vec<u8>, &str)> = vec![
            (b"#!/bin/sh\necho\n".to_vec(), "Linux program"),
            (testdata::ElfSpec::linux_x86_64().build(), "Linux program"),
            (testdata::zip(&["readme.txt", "data/file.bin"]), "no Windows program"),
            (testdata::zip(&["AndroidManifest.xml", "classes.dex"]), "Android"),
            (testdata::ole(&[0u8; 16], 9), "not an installer package"),
            (b"plain text".to_vec(), "not recognised"),
            (testdata::mz_with(b"NE\0\0"), "16-bit"),
            (
                PeSpec {
                    machine: 0xaa64,
                    ..PeSpec::windows_gui_x86_64()
                }
                .build(),
                "processor architecture",
            ),
            (
                PeSpec {
                    characteristics: 0x2022,
                    ..PeSpec::windows_gui_x86_64()
                }
                .build(),
                "library",
            ),
            (
                PeSpec {
                    subsystem: 1,
                    ..PeSpec::windows_gui_x86_64()
                }
                .build(),
                "Windows kernel",
            ),
        ];
        for (bytes, expected) in cases {
            let error = identify_bytes("installer-refused", "file", &bytes).unwrap_err();
            assert_eq!(error.code(), Code::InstallerUnrecognised, "{expected}");
            let message = error.to_string();
            assert!(message.contains(expected), "{message}");
        }
        // A file that is not there is a read error, with its own code.
        let temp = TempDir::new("installer-absent");
        let error = identify(&temp.path().join("absent.exe")).unwrap_err();
        assert_eq!(error.code(), Code::Unreadable);
    }

    #[test]
    fn a_bare_program_header_is_a_plain_program() {
        // The synthetic programs of the inspection tests have no sections.
        let bytes = PeSpec::windows_gui_x86_64().build();
        let info = identify_bytes("installer-bare", "tool.exe", &bytes).unwrap();
        assert_eq!(info.kind, InstallerKind::PlainExe);
    }
}
