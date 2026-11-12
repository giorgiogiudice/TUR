// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! OLE compound file inspection, enough to recognise a Windows Installer
//! package.
//!
//! Many unrelated formats share the compound-file magic number (older Office
//! documents among them). A Windows Installer file is told apart by the class
//! identifier of its root storage, which is the only thing read beyond the
//! header. The installer's tables, including its target architecture, are not.

use serde::Serialize;

use crate::arch::Endianness;
use crate::error::{FormatFamily, InspectError};
use crate::reader::{Reader, u16_at, u32_at};

const LITTLE: Endianness = Endianness::Little;

/// The magic number at the start of every OLE compound file.
pub(crate) const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// Root storage class of an installer package, `{000C1084-0000-0000-C000-000000000046}`.
pub(crate) const CLSID_MSI: [u8; 16] = [
    0x84, 0x10, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];
/// Root storage class of an installer patch, `{000C1086-0000-0000-C000-000000000046}`.
const CLSID_MSP: [u8; 16] = [
    0x86, 0x10, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];
/// Root storage class of an installer transform, `{000C1082-0000-0000-C000-000000000046}`.
const CLSID_MST: [u8; 16] = [
    0x82, 0x10, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// What an OLE compound file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OleKind {
    /// A Windows Installer package (`.msi`).
    MsiInstaller,
    /// A Windows Installer patch (`.msp`).
    MsiPatch,
    /// A Windows Installer transform (`.mst`).
    MsiTransform,
    /// A compound file of some other kind, for example an older Office
    /// document. It is not identified further.
    Unidentified,
}

impl OleKind {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            OleKind::MsiInstaller => "Windows Installer package (MSI)",
            OleKind::MsiPatch => "Windows Installer patch (MSP)",
            OleKind::MsiTransform => "Windows Installer transform (MST)",
            OleKind::Unidentified => "OLE compound file, not identified further",
        }
    }
}

/// What was read from an OLE compound file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OleInfo {
    /// What the file holds.
    pub kind: OleKind,
    /// The class identifier of the root storage, when it is not all zeros.
    pub root_clsid: Option<String>,
}

fn truncated(detail: &'static str) -> InspectError {
    InspectError::Truncated {
        format: FormatFamily::Ole,
        detail,
    }
}

fn malformed(detail: &str) -> InspectError {
    InspectError::Malformed {
        format: FormatFamily::Ole,
        detail: detail.to_owned(),
    }
}

/// Parses an OLE compound file whose first bytes are `head`.
pub(crate) fn parse(reader: &mut Reader<'_>, head: &[u8]) -> Result<OleInfo, InspectError> {
    let header = head.get(..512).ok_or_else(|| truncated("the header"))?;
    let sector_shift = u16_at(header, 30, LITTLE).ok_or_else(|| truncated("the header"))?;
    if sector_shift != 9 && sector_shift != 12 {
        return Err(malformed("the sector size is neither 512 nor 4096 bytes"));
    }
    let first_directory = u32_at(header, 48, LITTLE).ok_or_else(|| truncated("the header"))?;
    if first_directory >= 0xffff_fffa {
        return Err(malformed("the file has no directory"));
    }
    // Sector n starts at (n + 1) sectors: the header occupies the first one.
    let directory_at = (u64::from(first_directory) + 1)
        .checked_mul(1u64 << sector_shift)
        .ok_or_else(|| malformed("the directory offset overflows"))?;
    let root = reader
        .read(directory_at, 128)?
        .ok_or_else(|| truncated("the root directory entry"))?;
    let clsid = root
        .get(80..96)
        .ok_or_else(|| truncated("the root directory entry"))?;

    let kind = if clsid == CLSID_MSI {
        OleKind::MsiInstaller
    } else if clsid == CLSID_MSP {
        OleKind::MsiPatch
    } else if clsid == CLSID_MST {
        OleKind::MsiTransform
    } else {
        OleKind::Unidentified
    };
    let root_clsid = if clsid.iter().all(|&byte| byte == 0) {
        None
    } else {
        format_guid(clsid)
    };
    Ok(OleInfo { kind, root_clsid })
}

/// Formats a class identifier the usual way, for example
/// `000C1084-0000-0000-C000-000000000046`.
fn format_guid(raw: &[u8]) -> Option<String> {
    let first = u32_at(raw, 0, LITTLE)?;
    let second = u16_at(raw, 4, LITTLE)?;
    let third = u16_at(raw, 6, LITTLE)?;
    let rest = raw.get(8..16)?;
    let mut text = format!("{first:08X}-{second:04X}-{third:04X}-");
    for (index, byte) in rest.iter().enumerate() {
        if index == 2 {
            text.push('-');
        }
        text.push_str(&format!("{byte:02X}"));
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{Detected, inspect_bytes};
    use crate::testdata::ole;

    fn info(bytes: &[u8]) -> OleInfo {
        match inspect_bytes(bytes).unwrap().detected {
            Detected::Ole(info) => info,
            other => panic!("expected an OLE compound file, got {other:?}"),
        }
    }

    #[test]
    fn installer_package_is_recognised_by_its_root_class() {
        let found = info(&ole(&CLSID_MSI, 9));
        assert_eq!(found.kind, OleKind::MsiInstaller);
        assert_eq!(
            found.root_clsid.as_deref(),
            Some("000C1084-0000-0000-C000-000000000046")
        );
        // Version 4 files use 4096-byte sectors.
        assert_eq!(info(&ole(&CLSID_MSI, 12)).kind, OleKind::MsiInstaller);
    }

    #[test]
    fn patches_transforms_and_other_documents() {
        assert_eq!(info(&ole(&CLSID_MSP, 9)).kind, OleKind::MsiPatch);
        assert_eq!(info(&ole(&CLSID_MST, 9)).kind, OleKind::MsiTransform);
        let document = info(&ole(&[0u8; 16], 9));
        assert_eq!(document.kind, OleKind::Unidentified);
        assert_eq!(document.root_clsid, None);
        let other = info(&ole(&[0x11u8; 16], 9));
        assert_eq!(other.kind, OleKind::Unidentified);
        assert_eq!(
            other.root_clsid.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
    }

    #[test]
    fn damaged_headers_are_typed_errors() {
        let bytes = ole(&CLSID_MSI, 9);
        assert_eq!(inspect_bytes(&bytes[..100]), Err(truncated("the header")));
        assert_eq!(
            inspect_bytes(&bytes[..600]),
            Err(truncated("the root directory entry"))
        );

        let mut bad_shift = bytes.clone();
        bad_shift[30..32].copy_from_slice(&63u16.to_le_bytes());
        assert!(matches!(
            inspect_bytes(&bad_shift),
            Err(InspectError::Malformed {
                format: FormatFamily::Ole,
                ..
            })
        ));

        let mut no_directory = bytes.clone();
        no_directory[48..52].copy_from_slice(&0xffff_fffeu32.to_le_bytes());
        assert!(matches!(
            inspect_bytes(&no_directory),
            Err(InspectError::Malformed { .. })
        ));

        let mut far_directory = bytes;
        far_directory[48..52].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
        assert_eq!(
            inspect_bytes(&far_directory),
            Err(truncated("the root directory entry"))
        );
    }
}
