// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What kind of macOS package a file is, from its content, and where the
//! application bundles are once a package has been unpacked.
//!
//! | Kind | Recognised by |
//! |---|---|
//! | `app-bundle` | A directory with `Contents/Info.plist` or `Contents/MacOS` |
//! | `disk-image` | The UDIF trailer (`koly`) in the last 512 bytes; or, for a raw image, an HFS+ (`H+`, `HX` at 1024) or APFS (`NXSB` at 32) signature |
//! | `installer-package` | The xar signature `xar!`: a flat `.pkg` |
//! | `zip-archive` | A zip archive, as applications are often published |
//! | `program` | A Mach-O file, not inside a bundle |
//!
//! Nothing is mounted. A disk image or a zip archive is unpacked by 7-Zip
//! inside the sandbox ([`super::plan`]); an installer package is installed by
//! Darling's own `installer`, inside the application's prefix and the
//! sandbox, because a package may carry scripts that only a macOS-like
//! system can run.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::arch::Endianness;
use crate::bundle::is_bundle;
use crate::error::InspectError;
use crate::macho;
use crate::reader::u32_at;

/// The deepest level at which bundles are looked for in an unpacked package.
pub const MAX_SEARCH_DEPTH: usize = 6;
/// The most directory entries the search visits.
pub const MAX_SEARCH_ENTRIES: usize = 200_000;
/// The most bundles that are listed.
pub const MAX_LISTED_BUNDLES: usize = 32;

/// What kind of macOS package a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageKind {
    /// An application bundle, `Example.app`.
    AppBundle,
    /// A disk image, `.dmg`: UDIF or a raw HFS+ or APFS image.
    DiskImage,
    /// A flat installer package, `.pkg` (a xar archive).
    InstallerPackage,
    /// A zip archive.
    ZipArchive,
    /// A Mach-O program on its own.
    Program,
    /// Not a macOS package this route knows.
    NotMacSoftware,
}

impl PackageKind {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            PackageKind::AppBundle => "macOS application bundle",
            PackageKind::DiskImage => "macOS disk image",
            PackageKind::InstallerPackage => "macOS installer package",
            PackageKind::ZipArchive => "zip archive",
            PackageKind::Program => "macOS program (Mach-O)",
            PackageKind::NotMacSoftware => "not a macOS package",
        }
    }

    /// Whether 7-Zip unpacks it before the application is looked for.
    pub const fn is_unpacked_by_7zip(self) -> bool {
        matches!(self, PackageKind::DiskImage | PackageKind::ZipArchive)
    }
}

/// Identifies the package at `path` from its content. The name is not
/// consulted.
///
/// # Errors
///
/// [`InspectError::Io`] when the path cannot be read.
pub fn identify(path: &Path) -> Result<PackageKind, InspectError> {
    let metadata = fs::metadata(path).map_err(|error| InspectError::io(path, &error))?;
    if metadata.is_dir() {
        return Ok(if is_bundle(path) {
            PackageKind::AppBundle
        } else {
            PackageKind::NotMacSoftware
        });
    }
    if !metadata.is_file() {
        return Err(InspectError::Io {
            path: path.display().to_string(),
            message: "it is neither a regular file nor a directory".to_owned(),
        });
    }
    let mut file = File::open(path).map_err(|error| InspectError::io(path, &error))?;
    let size = metadata.len();
    let mut head = vec![0u8; usize::try_from(size.min(4096)).unwrap_or(4096)];
    file.read_exact(&mut head)
        .map_err(|error| InspectError::io(path, &error))?;
    let mut tail = Vec::new();
    if size >= 512 {
        file.seek(SeekFrom::Start(size - 512))
            .map_err(|error| InspectError::io(path, &error))?;
        tail = vec![0u8; 512];
        file.read_exact(&mut tail)
            .map_err(|error| InspectError::io(path, &error))?;
    }
    Ok(classify(&head, &tail))
}

/// The kind of package whose first bytes are `head` (up to 4096) and whose
/// last 512 bytes are `tail` (empty for a shorter file).
pub fn classify(head: &[u8], tail: &[u8]) -> PackageKind {
    if tail.starts_with(b"koly") {
        return PackageKind::DiskImage;
    }
    if head.starts_with(b"xar!") {
        return PackageKind::InstallerPackage;
    }
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return PackageKind::ZipArchive;
    }
    if let Some(magic) = u32_at(head, 0, Endianness::Big)
        && (macho::is_thin_magic(magic) || macho::is_fat_magic(magic))
    {
        return PackageKind::Program;
    }
    let hfs = head
        .get(1024..1026)
        .is_some_and(|signature| signature == b"H+" || signature == b"HX");
    let apfs = head.get(32..36).is_some_and(|signature| signature == b"NXSB");
    if hfs || apfs {
        return PackageKind::DiskImage;
    }
    PackageKind::NotMacSoftware
}

/// An application bundle found in an unpacked package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoundBundle {
    /// The bundle's path.
    pub path: PathBuf,
    /// Its path relative to where the search started.
    pub relative: String,
}

/// Everything the search found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Search {
    /// The application bundles, outermost first, at most
    /// [`MAX_LISTED_BUNDLES`]. A bundle inside another is not listed.
    pub bundles: Vec<FoundBundle>,
    /// Installer packages found (a disk image that holds a `.pkg`).
    pub packages: Vec<PathBuf>,
    /// Partition images 7-Zip left to be unpacked in turn (`*.hfs`,
    /// `*.hfsx`, `*.apfs`, `*.img`).
    pub partitions: Vec<PathBuf>,
    /// Whether a limit stopped the search.
    pub cut_short: bool,
}

/// Looks for application bundles, installer packages and partition images
/// under `root`. Symbolic links are neither followed nor listed, so nothing
/// outside `root` is reached; directories named `__MACOSX` (the resource
/// forks a zip from a Mac carries) are skipped.
pub fn search(root: &Path) -> Search {
    let mut found = Search::default();
    let mut visited = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((directory, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut names: Vec<(PathBuf, String)> = Vec::new();
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_SEARCH_ENTRIES {
                found.cut_short = true;
                break;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            names.push((entry.path(), name));
        }
        names.sort();
        for (path, name) in names {
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if metadata.is_dir() {
                if name == "__MACOSX" {
                    continue;
                }
                if lower.ends_with(".app") && is_bundle(&path) {
                    if found.bundles.len() < MAX_LISTED_BUNDLES {
                        let relative = path
                            .strip_prefix(root)
                            .map(|rest| rest.display().to_string())
                            .unwrap_or_else(|_| name.clone());
                        found.bundles.push(FoundBundle { path, relative });
                    } else {
                        found.cut_short = true;
                    }
                    continue;
                }
                if depth + 1 < MAX_SEARCH_DEPTH {
                    stack.push((path, depth + 1));
                } else {
                    found.cut_short = true;
                }
            } else if metadata.is_file() {
                if lower.ends_with(".pkg") || lower.ends_with(".mpkg") {
                    found.packages.push(path);
                } else if [".hfs", ".hfsx", ".apfs", ".img"]
                    .iter()
                    .any(|suffix| lower.ends_with(suffix))
                {
                    found.partitions.push(path);
                }
            }
        }
    }
    found.bundles.sort_by_key(|bundle| bundle.relative.clone());
    found
}

/// Chooses the bundle to install: the one named, or the only one.
///
/// # Errors
///
/// A message that lists the bundles when none, or more than one without a
/// choice, was found.
pub fn choose<'a>(found: &'a Search, wanted: Option<&str>) -> Result<&'a FoundBundle, String> {
    let names: Vec<&str> = found
        .bundles
        .iter()
        .map(|bundle| bundle.relative.as_str())
        .collect();
    if let Some(wanted) = wanted {
        let matching: Vec<&FoundBundle> = found
            .bundles
            .iter()
            .filter(|bundle| {
                bundle.relative == wanted
                    || bundle
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == wanted)
            })
            .collect();
        return match matching.as_slice() {
            [one] => Ok(*one),
            [] => Err(format!(
                "no application named {wanted:?} was found; found: {}",
                list(&names)
            )),
            _ => Err(format!(
                "several applications are named {wanted:?}; name one by its path: {}",
                list(&names)
            )),
        };
    }
    match found.bundles.as_slice() {
        [one] => Ok(one),
        [] => Err("the package holds no application bundle".to_owned()),
        _ => Err(format!(
            "the package holds several applications; choose one with --app: {}",
            list(&names)
        )),
    }
}

fn list(names: &[&str]) -> String {
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::TempDir;

    #[test]
    fn kinds_are_told_by_content() {
        let mut udif_tail = vec![0u8; 512];
        udif_tail[..4].copy_from_slice(b"koly");
        assert_eq!(classify(&[0u8; 4096], &udif_tail), PackageKind::DiskImage);
        assert_eq!(classify(b"xar!\0\x1c\0\x01", &[]), PackageKind::InstallerPackage);
        assert_eq!(classify(b"PK\x03\x04rest", &[]), PackageKind::ZipArchive);
        assert_eq!(
            classify(&0xcffa_edfeu32.to_be_bytes(), &[]),
            PackageKind::Program
        );
        assert_eq!(
            classify(&0xcafe_babeu32.to_be_bytes(), &[]),
            PackageKind::Program
        );
        let mut hfs = vec![0u8; 4096];
        hfs[1024..1026].copy_from_slice(b"H+");
        assert_eq!(classify(&hfs, &[]), PackageKind::DiskImage);
        let mut apfs = vec![0u8; 4096];
        apfs[32..36].copy_from_slice(b"NXSB");
        assert_eq!(classify(&apfs, &[]), PackageKind::DiskImage);
        assert_eq!(classify(b"#!/bin/sh\n", &[]), PackageKind::NotMacSoftware);
        assert_eq!(classify(&[], &[]), PackageKind::NotMacSoftware);
        assert!(PackageKind::DiskImage.is_unpacked_by_7zip());
        assert!(!PackageKind::InstallerPackage.is_unpacked_by_7zip());
    }

    fn bundle(root: &Path, relative: &str) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.join("Contents").join("MacOS")).unwrap();
        path
    }

    #[test]
    fn files_and_directories_are_identified() {
        let scratch = TempDir::new("mac-identify");
        let app = bundle(scratch.path(), "Example.app");
        assert_eq!(identify(&app).unwrap(), PackageKind::AppBundle);
        assert_eq!(identify(scratch.path()).unwrap(), PackageKind::NotMacSoftware);

        // A disk image is told by its trailer, whatever it is called.
        let mut bytes = vec![0u8; 8192];
        let end = bytes.len();
        bytes[end - 512..end - 508].copy_from_slice(b"koly");
        let file = scratch.path().join("notes.txt");
        fs::write(&file, &bytes).unwrap();
        assert_eq!(identify(&file).unwrap(), PackageKind::DiskImage);

        // A short file has no trailer.
        fs::write(&file, b"xar!").unwrap();
        assert_eq!(identify(&file).unwrap(), PackageKind::InstallerPackage);
        assert!(identify(&scratch.path().join("missing")).is_err());
    }

    #[test]
    fn the_search_finds_outer_bundles_packages_and_partitions() {
        let scratch = TempDir::new("mac-search");
        let root = scratch.path();
        bundle(root, "Example.app");
        // A helper bundle inside the application is not listed.
        bundle(root, "Example.app/Contents/Helpers/Helper.app");
        bundle(root, "Extras/Tool.app");
        // A Mac's resource forks in a zip.
        bundle(root, "__MACOSX/Example.app");
        fs::write(root.join("Install Example.pkg"), b"xar!").unwrap();
        fs::write(root.join("4.apfs"), b"").unwrap();
        // A link is neither followed nor listed.
        std::os::unix::fs::symlink("/", root.join("Applications")).unwrap();

        let found = search(root);
        let names: Vec<&str> = found
            .bundles
            .iter()
            .map(|bundle| bundle.relative.as_str())
            .collect();
        assert_eq!(names, ["Example.app", "Extras/Tool.app"]);
        assert_eq!(found.packages.len(), 1);
        assert_eq!(found.partitions.len(), 1);
        assert!(!found.cut_short);

        assert!(choose(&found, None).unwrap_err().contains("several"));
        assert_eq!(
            choose(&found, Some("Tool.app")).unwrap().relative,
            "Extras/Tool.app"
        );
        assert_eq!(
            choose(&found, Some("Example.app")).unwrap().relative,
            "Example.app"
        );
        assert!(choose(&found, Some("Other.app")).unwrap_err().contains("found:"));
        assert!(
            choose(&Search::default(), None)
                .unwrap_err()
                .contains("no application")
        );
    }
}
