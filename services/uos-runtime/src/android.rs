// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Android package detection from zip member names.
//!
//! An APK is recognised by its member names, and its native ABIs by the
//! directories under `lib/`. Nothing is decompressed: the binary
//! `AndroidManifest.xml` is not decoded, and the packages inside an XAPK or
//! APKS archive are not opened.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::inspect::Detected;
use crate::limits::{MAX_LISTED_NAME_LEN, MAX_LISTED_PACKAGES};

/// ABI directory names that hold ARM code.
pub const ARM_ABIS: [&str; 3] = ["arm64-v8a", "armeabi-v7a", "armeabi"];

/// The kind of Android package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AndroidPackageKind {
    /// An installable application package: a zip with `AndroidManifest.xml`
    /// at its root.
    Apk,
    /// An Android App Bundle, the publishing format: a zip with
    /// `base/manifest/AndroidManifest.xml`.
    Aab,
    /// An XAPK: a zip of APK files with a `manifest.json`.
    Xapk,
    /// An APKS archive as produced by `bundletool`: a zip of APK files with a
    /// `toc.pb`.
    Apks,
    /// A zip that holds APK files in a layout that is neither of the above.
    ApkArchive,
}

impl AndroidPackageKind {
    /// A label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            AndroidPackageKind::Apk => "Android application package (APK)",
            AndroidPackageKind::Aab => "Android App Bundle (AAB)",
            AndroidPackageKind::Xapk => "Android package archive (XAPK)",
            AndroidPackageKind::Apks => "Android package set (APKS)",
            AndroidPackageKind::ApkArchive => "archive of Android packages",
        }
    }
}

/// Whether a package carries native (processor-specific) code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeCode {
    /// There are no native libraries: the package runs on any processor.
    Absent,
    /// There are native libraries; see the ABI list.
    Present,
    /// The package holds other packages, which were not opened.
    NotInspected,
}

/// What was read from an Android package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AndroidInfo {
    /// The kind of package.
    pub kind: AndroidPackageKind,
    /// Whether `classes.dex` is present (APK only).
    pub has_dex: bool,
    /// Whether native code is present.
    pub native_code: NativeCode,
    /// The native ABIs found under `lib/<abi>/`, sorted, for example
    /// `arm64-v8a` or `x86_64`.
    pub native_abis: Vec<String>,
    /// How many APK files the archive holds (XAPK, APKS and similar).
    pub nested_package_count: u64,
    /// The names of the first nested APK files.
    pub nested_packages: Vec<String>,
    /// The number of members in the zip archive.
    pub entries: u64,
}

impl AndroidInfo {
    /// Whether every native ABI in the package is an ARM ABI.
    pub fn is_arm_only(&self) -> bool {
        !self.native_abis.is_empty()
            && self
                .native_abis
                .iter()
                .all(|abi| ARM_ABIS.contains(&abi.as_str()))
    }
}

/// A zip archive that is not an Android package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ZipInfo {
    /// The number of members in the archive.
    pub entries: u64,
}

/// Facts gathered from zip member names, one name at a time.
#[derive(Default)]
pub(crate) struct ZipFacts {
    root_manifest: bool,
    classes_dex: bool,
    classes_jar: bool,
    bundle_manifest: bool,
    manifest_json: bool,
    toc_pb: bool,
    jar_manifest: bool,
    abis: BTreeSet<String>,
    bundle_abis: BTreeSet<String>,
    nested_package_count: u64,
    nested_packages: Vec<String>,
}

impl ZipFacts {
    /// Records one member name.
    pub(crate) fn visit(&mut self, raw: &[u8]) {
        let Ok(name) = std::str::from_utf8(raw) else {
            return;
        };
        match name {
            "AndroidManifest.xml" => self.root_manifest = true,
            "classes.dex" => self.classes_dex = true,
            "classes.jar" => self.classes_jar = true,
            "base/manifest/AndroidManifest.xml" => self.bundle_manifest = true,
            "manifest.json" => self.manifest_json = true,
            "toc.pb" => self.toc_pb = true,
            "META-INF/MANIFEST.MF" => self.jar_manifest = true,
            _ => {}
        }

        let mut parts = name.split('/');
        let path = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        );
        match path {
            (Some("lib"), Some(abi), Some(file), None, _)
                if !file.is_empty() && is_abi_name(abi) =>
            {
                self.abis.insert(abi.to_owned());
            }
            (Some(module), Some("lib"), Some(abi), Some(file), None)
                if !module.is_empty() && !file.is_empty() && is_abi_name(abi) =>
            {
                self.bundle_abis.insert(abi.to_owned());
            }
            _ => {}
        }

        if is_package_name(name) {
            self.nested_package_count = self.nested_package_count.saturating_add(1);
            if self.nested_packages.len() < MAX_LISTED_PACKAGES && is_listable(name) {
                self.nested_packages.push(name.to_owned());
            }
        }
    }

    /// Decides what the archive is.
    pub(crate) fn classify(self, entries: u64, notes: &mut Vec<String>) -> Detected {
        if self.root_manifest && (self.classes_dex || !self.classes_jar) {
            if !self.classes_dex {
                notes.push(
                    "The package has no classes.dex: it is a split, resource-only or \
                     native-only APK."
                        .to_owned(),
                );
            }
            let native_abis: Vec<String> = self.abis.into_iter().collect();
            return Detected::Android(AndroidInfo {
                kind: AndroidPackageKind::Apk,
                has_dex: self.classes_dex,
                native_code: presence(&native_abis),
                native_abis,
                nested_package_count: 0,
                nested_packages: Vec::new(),
                entries,
            });
        }
        if self.bundle_manifest {
            let native_abis: Vec<String> = self.bundle_abis.into_iter().collect();
            return Detected::Android(AndroidInfo {
                kind: AndroidPackageKind::Aab,
                has_dex: false,
                native_code: presence(&native_abis),
                native_abis,
                nested_package_count: 0,
                nested_packages: Vec::new(),
                entries,
            });
        }
        if self.nested_package_count > 0 {
            let kind = if self.manifest_json {
                AndroidPackageKind::Xapk
            } else if self.toc_pb {
                AndroidPackageKind::Apks
            } else {
                AndroidPackageKind::ApkArchive
            };
            return Detected::Android(AndroidInfo {
                kind,
                has_dex: false,
                native_code: NativeCode::NotInspected,
                native_abis: Vec::new(),
                nested_package_count: self.nested_package_count,
                nested_packages: self.nested_packages,
                entries,
            });
        }
        if self.root_manifest {
            notes.push(
                "The archive has an AndroidManifest.xml and a classes.jar: it looks like an \
                 Android library (AAR), which is not an application."
                    .to_owned(),
            );
        } else if self.jar_manifest {
            notes.push(
                "The archive has a META-INF/MANIFEST.MF: it may be a Java archive.".to_owned(),
            );
        }
        Detected::Zip(ZipInfo { entries })
    }
}

fn presence(abis: &[String]) -> NativeCode {
    if abis.is_empty() {
        NativeCode::Absent
    } else {
        NativeCode::Present
    }
}

/// Whether `name` is a plausible ABI directory name. The value ends up in
/// reports, so it is restricted to a short, harmless alphabet.
fn is_abi_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 24
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Whether `name` is a member that is itself an APK file.
fn is_package_name(name: &str) -> bool {
    name.strip_suffix(".apk")
        .is_some_and(|stem| !stem.is_empty() && !stem.ends_with('/'))
}

/// Whether a member name is short and printable enough to show in a report.
fn is_listable(name: &str) -> bool {
    name.len() <= MAX_LISTED_NAME_LEN && !name.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::inspect_bytes;
    use crate::testdata::zip;

    fn android(names: &[&str]) -> AndroidInfo {
        match inspect_bytes(&zip(names)).unwrap().detected {
            Detected::Android(info) => info,
            other => panic!("expected an Android package, got {other:?}"),
        }
    }

    #[test]
    fn apk_with_only_arm_libraries() {
        let info = android(&[
            "AndroidManifest.xml",
            "classes.dex",
            "resources.arsc",
            "lib/arm64-v8a/libgame.so",
            "lib/armeabi-v7a/libgame.so",
            "lib/arm64-v8a/libother.so",
        ]);
        assert_eq!(info.kind, AndroidPackageKind::Apk);
        assert!(info.has_dex);
        assert_eq!(info.native_code, NativeCode::Present);
        assert_eq!(info.native_abis, ["arm64-v8a", "armeabi-v7a"]);
        assert!(info.is_arm_only());
        assert_eq!(info.entries, 6);
    }

    #[test]
    fn apk_with_every_common_abi() {
        let info = android(&[
            "AndroidManifest.xml",
            "classes.dex",
            "lib/x86/libapp.so",
            "lib/x86_64/libapp.so",
            "lib/arm64-v8a/libapp.so",
            "lib/armeabi-v7a/libapp.so",
        ]);
        assert_eq!(
            info.native_abis,
            ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"]
        );
        assert!(!info.is_arm_only());
    }

    #[test]
    fn apk_without_native_code() {
        let info = android(&["AndroidManifest.xml", "classes.dex", "res/layout/main.xml"]);
        assert_eq!(info.native_code, NativeCode::Absent);
        assert!(info.native_abis.is_empty());
        assert!(!info.is_arm_only());
    }

    #[test]
    fn lib_entries_that_are_not_abi_directories_are_ignored() {
        let info = android(&[
            "AndroidManifest.xml",
            "classes.dex",
            "lib/",
            "lib/x86_64/",
            "lib/readme.txt",
            "lib/bad abi/libx.so",
            "lib/x86_64/nested/libdeep.so",
            "assets/lib/arm64-v8a/libnot.so.txt/x",
        ]);
        assert_eq!(info.native_code, NativeCode::Absent);
    }

    #[test]
    fn split_apk_without_dex_is_still_an_apk() {
        let bytes = zip(&["AndroidManifest.xml", "lib/arm64-v8a/libapp.so"]);
        let inspection = inspect_bytes(&bytes).unwrap();
        let Detected::Android(info) = &inspection.detected else {
            panic!("expected an Android package");
        };
        assert_eq!(info.kind, AndroidPackageKind::Apk);
        assert!(!info.has_dex);
        assert!(
            inspection
                .notes
                .iter()
                .any(|note| note.contains("classes.dex"))
        );
    }

    #[test]
    fn app_bundle_lists_abis_of_every_module() {
        let info = android(&[
            "BundleConfig.pb",
            "base/manifest/AndroidManifest.xml",
            "base/dex/classes.dex",
            "base/lib/arm64-v8a/libapp.so",
            "feature/lib/x86_64/libfeature.so",
        ]);
        assert_eq!(info.kind, AndroidPackageKind::Aab);
        assert_eq!(info.native_abis, ["arm64-v8a", "x86_64"]);
        assert_eq!(info.native_code, NativeCode::Present);
    }

    #[test]
    fn archives_of_packages() {
        let xapk = android(&[
            "manifest.json",
            "icon.png",
            "com.example.app.apk",
            "config.arm64_v8a.apk",
            "Android/obb/com.example.app/main.1.com.example.app.obb",
        ]);
        assert_eq!(xapk.kind, AndroidPackageKind::Xapk);
        assert_eq!(xapk.native_code, NativeCode::NotInspected);
        assert_eq!(xapk.nested_package_count, 2);
        assert_eq!(
            xapk.nested_packages,
            ["com.example.app.apk", "config.arm64_v8a.apk"]
        );

        let apks = android(&["toc.pb", "splits/base-master.apk", "splits/base-x86_64.apk"]);
        assert_eq!(apks.kind, AndroidPackageKind::Apks);
        assert_eq!(apks.nested_package_count, 2);

        let other = android(&["info.json", "base.apk"]);
        assert_eq!(other.kind, AndroidPackageKind::ApkArchive);
    }

    #[test]
    fn plain_archives_are_not_android_packages() {
        for names in [
            &["readme.txt", "src/main.c"][..],
            &[
                "AndroidManifest.xml",
                "classes.jar",
                "res/values/strings.xml",
            ][..],
            &["META-INF/MANIFEST.MF", "com/example/Main.class"][..],
            &[".apk", "dir/.apk"][..],
        ] {
            let inspection = inspect_bytes(&zip(names)).unwrap();
            assert!(
                matches!(inspection.detected, Detected::Zip(_)),
                "{names:?} was detected as {:?}",
                inspection.detected
            );
        }
    }

    #[test]
    fn hostile_names_do_not_reach_the_report() {
        let long = format!("{}.apk", "a".repeat(MAX_LISTED_NAME_LEN));
        let info = android(&[
            "manifest.json",
            "evil\u{1b}[31m.apk",
            long.as_str(),
            "ok.apk",
        ]);
        assert_eq!(info.nested_package_count, 3);
        assert_eq!(info.nested_packages, ["ok.apk"]);
    }

    #[test]
    fn listed_packages_are_capped() {
        let owned: Vec<String> = (0..MAX_LISTED_PACKAGES + 10)
            .map(|index| format!("split{index}.apk"))
            .collect();
        let names: Vec<&str> = owned.iter().map(String::as_str).collect();
        let info = android(&names);
        assert_eq!(info.nested_packages.len(), MAX_LISTED_PACKAGES);
        assert_eq!(
            info.nested_package_count,
            u64::try_from(MAX_LISTED_PACKAGES + 10).unwrap()
        );
    }
}
