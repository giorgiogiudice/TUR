// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Inspecting an Android package before it is installed
//! (android-runtime.md, sections 6.1 and 6.2). Nothing in the package is
//! run.
//!
//! What kind of file it is and which processors its native code is for is
//! read by the runtime manager's inspector (`tur-runtime`), from the zip's
//! member names. The manifest is then taken out of the zip with Python's
//! standard `zipfile` module (Python is on the system because Waydroid is a
//! Python program), size-capped, and read here.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tur_runtime::Detected;
use tur_runtime::android::AndroidPackageKind;

use crate::axml::{self, ManifestFacts};
use crate::codes::Code;
use crate::run;

/// The largest manifest read.
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

/// Takes one member out of a zip archive and writes it to standard output.
/// Exit status 3: the member is larger than the limit.
const EXTRACT: &str = "\
import sys, zipfile
limit = int(sys.argv[3])
with zipfile.ZipFile(sys.argv[1]) as archive:
    info = archive.getinfo(sys.argv[2])
    if info.file_size > limit:
        sys.exit(3)
    with archive.open(info) as member:
        sys.stdout.buffer.write(member.read(limit + 1))
";

/// An Android package that may be installed.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Package {
    /// Where it is.
    pub(crate) path: PathBuf,
    /// Its SHA-256.
    pub(crate) sha256: String,
    /// Its size in bytes.
    pub(crate) size: u64,
    /// The native ABIs it carries; empty when it has no native code.
    pub(crate) native_abis: Vec<String>,
    /// What its manifest says.
    #[serde(flatten)]
    pub(crate) manifest: ManifestFacts,
}

/// Why a package cannot be installed.
#[derive(Debug, Clone)]
pub(crate) struct Refusal {
    /// The code.
    pub(crate) code: Code,
    /// What is wrong and what can be done, in a sentence or two.
    pub(crate) message: String,
}

fn refuse(code: Code, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        message: message.into(),
    }
}

/// Inspects the package at `path` for an Android system of `api_level`.
pub(crate) fn inspect(path: &Path, api_level: u32) -> Result<Package, Refusal> {
    let inspection = tur_runtime::inspect(path).map_err(|error| {
        refuse(
            Code::PkgInvalid,
            format!("{} cannot be read as a package: {error}", path.display()),
        )
    })?;
    let info = match inspection.detected {
        Detected::Android(info) => info,
        _ => {
            return Err(refuse(
                Code::PkgInvalid,
                format!(
                    "{} is not an Android package: it has no AndroidManifest.xml.",
                    path.display()
                ),
            ));
        }
    };
    match info.kind {
        AndroidPackageKind::Apk => {}
        AndroidPackageKind::Aab => {
            return Err(refuse(
                Code::PkgBundle,
                "This is an Android App Bundle, the format developers upload to a store. It \
                 cannot be installed directly; ask for the APK file instead.",
            ));
        }
        AndroidPackageKind::Xapk | AndroidPackageKind::Apks | AndroidPackageKind::ApkArchive => {
            return Err(refuse(
                Code::PkgSplitSet,
                "This is a set of split packages. TUR cannot install split sets yet; a single \
                 APK of the same application can be installed.",
            ));
        }
    }
    if info.is_arm_only() {
        return Err(refuse(
            Code::ArmOnly,
            format!(
                "This package contains code for ARM processors only ({}). This computer's \
                 processor cannot run it, and TUR ships no ARM translation. Look for a version \
                 of the application built for x86_64, or for all processors.",
                info.native_abis.join(", ")
            ),
        ));
    }
    let manifest_bytes = extract_manifest(path)?;
    let manifest = axml::parse(&manifest_bytes).map_err(|reason| {
        refuse(
            Code::PkgInvalid,
            format!("The package's manifest cannot be read: {reason}."),
        )
    })?;
    if let Some(min_sdk) = manifest.min_sdk
        && min_sdk > api_level
    {
        return Err(refuse(
            Code::PkgNewerAndroid,
            format!(
                "{} needs Android API level {min_sdk} or newer; the Android here is level \
                 {api_level}.",
                manifest.package
            ),
        ));
    }
    let (sha256, size) = tur_runtime::sha256::sha256_file(path).map_err(|error| {
        refuse(
            Code::PkgInvalid,
            format!("{} cannot be read: {error}", path.display()),
        )
    })?;
    Ok(Package {
        path: path.to_path_buf(),
        sha256,
        size,
        native_abis: info.native_abis,
        manifest,
    })
}

fn extract_manifest(path: &Path) -> Result<Vec<u8>, Refusal> {
    let limit = MAX_MANIFEST_BYTES.to_string();
    let file = path.to_string_lossy();
    let output = run::run(
        "python3",
        &["-I", "-c", EXTRACT, &file, "AndroidManifest.xml", &limit],
        Duration::from_secs(60),
    );
    if output.status == Some(3) {
        return Err(refuse(
            Code::PkgInvalid,
            "The package's manifest is larger than any real manifest.",
        ));
    }
    if !output.ok() || output.stdout.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(refuse(
            Code::PkgInvalid,
            format!(
                "The package's manifest cannot be taken out of it: {}",
                output.account()
            ),
        ));
    }
    Ok(output.stdout)
}

/// Makes zip archives for tests, with stored (uncompressed) members.
#[cfg(test)]
pub(crate) mod zipfile {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for byte in data {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = (!(crc & 1)).wrapping_add(1);
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }

    /// A zip archive holding `members`, stored without compression.
    pub(crate) fn archive(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for (name, data) in members {
            let offset = u32::try_from(out.len()).unwrap();
            let crc = crc32(data);
            let size = u32::try_from(data.len()).unwrap();
            let name_len = u16::try_from(name.len()).unwrap();
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&name_len.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            directory.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            directory.extend_from_slice(&crc.to_le_bytes());
            directory.extend_from_slice(&size.to_le_bytes());
            directory.extend_from_slice(&size.to_le_bytes());
            directory.extend_from_slice(&name_len.to_le_bytes());
            directory.extend_from_slice(&[0; 12]);
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let directory_offset = u32::try_from(out.len()).unwrap();
        let directory_size = u32::try_from(directory.len()).unwrap();
        let count = u16::try_from(members.len()).unwrap();
        out.extend_from_slice(&directory);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&directory_size.to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn crc_of_a_known_string() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axml::build;
    use std::fs;

    fn write(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tur-android-apk-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn apk(manifest: &[u8], extra: &[(&str, &[u8])]) -> Vec<u8> {
        let mut members: Vec<(&str, &[u8])> =
            vec![("AndroidManifest.xml", manifest), ("classes.dex", &b"dex\n035\0"[..])];
        members.extend_from_slice(extra);
        zipfile::archive(&members)
    }

    #[test]
    fn a_package_without_native_code_is_accepted_with_its_identity() {
        let manifest = build::manifest("org.billthefarmer.editor", 21, false);
        let path = write("editor.apk", &apk(&manifest, &[]));
        let package = inspect(&path, 33).unwrap();
        assert_eq!(package.manifest.package, "org.billthefarmer.editor");
        assert_eq!(package.manifest.version_code, Some(198));
        assert!(package.native_abis.is_empty());
        assert_eq!(package.sha256.len(), 64);
        // The extension does not matter: detection is by content.
        let renamed = write("editor.bin", &apk(&manifest, &[]));
        assert!(inspect(&renamed, 33).is_ok());
    }

    #[test]
    fn a_package_with_x86_64_code_is_accepted() {
        let manifest = build::manifest("org.example.native", 21, true);
        let lib: &[u8] = b"\x7fELF";
        let bytes = apk(
            &manifest,
            &[("lib/x86_64/libx.so", lib), ("lib/arm64-v8a/libx.so", lib)],
        );
        let package = inspect(&write("native.apk", &bytes), 33).unwrap();
        assert_eq!(package.native_abis, ["arm64-v8a", "x86_64"]);
    }

    #[test]
    fn an_arm_only_package_is_refused_before_anything_is_installed() {
        let manifest = build::manifest("org.moire.opensudoku", 26, false);
        let lib: &[u8] = b"\x7fELF";
        let bytes = apk(&manifest, &[("lib/arm64-v8a/libx.so", lib)]);
        let refusal = inspect(&write("arm.apk", &bytes), 33).unwrap_err();
        assert_eq!(refusal.code, Code::ArmOnly);
        assert!(refusal.message.contains("arm64-v8a"));
    }

    #[test]
    fn a_package_for_a_newer_android_is_refused() {
        let manifest = build::manifest("org.example.new", 34, false);
        let refusal = inspect(&write("new.apk", &apk(&manifest, &[])), 33).unwrap_err();
        assert_eq!(refusal.code, Code::PkgNewerAndroid);
        assert!(refusal.message.contains("level 34"));
    }

    #[test]
    fn bundles_split_sets_and_other_files_are_refused() {
        let bundle = zipfile::archive(&[("base/manifest/AndroidManifest.xml", &b"x"[..])]);
        let refusal = inspect(&write("app.aab", &bundle), 33).unwrap_err();
        assert_eq!(refusal.code, Code::PkgBundle);

        let set = zipfile::archive(&[("base.apk", &b"x"[..]), ("split_config.x86_64.apk", &b"y"[..])]);
        let refusal = inspect(&write("app.apks", &set), 33).unwrap_err();
        assert_eq!(refusal.code, Code::PkgSplitSet);

        let refusal = inspect(&write("notes.txt", b"just text\n"), 33).unwrap_err();
        assert_eq!(refusal.code, Code::PkgInvalid);

        let damaged = apk(&b"not binary xml"[..], &[]);
        let refusal = inspect(&write("damaged.apk", &damaged), 33).unwrap_err();
        assert_eq!(refusal.code, Code::PkgInvalid);
        assert!(refusal.message.contains("manifest"));
    }
}
