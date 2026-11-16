// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! TUR's pinned index: every file `tur-rehost` may fetch, where from, and
//! the SHA-256 it must have.
//!
//! The index is `data/index.json`, written by `data/pin-index.py` from the
//! projects' own release pages and registries and reviewed like code. It
//! is compiled into the program (and may be replaced with `--index` for a
//! test). Nothing is fetched that is not in it: no "latest", no version
//! range, no file whose digest TUR has not recorded. A runtime version the
//! index lacks is a refusal (`TUR-REHOST-INDEX-002`), not a download.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::codes::{Code, Failure};

/// The index compiled into the program.
pub const BUILT_IN: &str = include_str!("../data/index.json");

/// The largest file the index may pin: 1 GiB.
pub const MAX_ARTIFACT_SIZE: u64 = 1 << 30;

/// One file to fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// Where it is fetched from: an `https` URL on the project's own
    /// release location or registry.
    pub url: String,
    /// Its SHA-256, 64 lower-case hexadecimal digits.
    pub sha256: String,
    /// Its size in bytes.
    pub size: u64,
    /// Where the digest comes from: the project's checksum file, a
    /// published asset digest, or computed when the index was written.
    pub hash_source: String,
    /// npm's own SHA-512 integrity string, kept for provenance; the
    /// SHA-256 above is what is checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm_integrity: Option<String>,
}

/// One Electron release, as TUR pinned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElectronRelease {
    /// Its Node ABI (`NODE_MODULE_VERSION`).
    pub abi: u32,
    /// The Chromium it is built on.
    pub chromium: String,
    /// The Node.js it is built on.
    pub node: String,
    /// The Linux x86-64 release archive, unchanged: it carries
    /// `libffmpeg.so` with Chromium's proprietary codecs (H.264, AAC).
    pub linux_x64: Artifact,
    /// Electron's own `libffmpeg.so` built with open codecs only, for
    /// `--codecs open-only`.
    pub ffmpeg_open_codecs: Artifact,
    /// The headers native modules are built against.
    pub headers: Artifact,
}

/// How TUR supplies the Linux build of a native module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// A per-platform npm package's `linux-x64` sibling, whole.
    NpmPackage,
    /// Files of the module's own npm tarball (its `prebuilds/`).
    NpmPrebuild,
    /// An archive the module publishes with its release (prebuild-install,
    /// node-pre-gyp naming).
    ReleasePrebuild,
    /// The module's npm source, built on this computer in a sandbox without
    /// network (`crate::build`).
    SourceBuild,
}

impl Tier {
    /// The tier as written in JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Tier::NpmPackage => "npm-package",
            Tier::NpmPrebuild => "npm-prebuild",
            Tier::ReleasePrebuild => "release-prebuild",
            Tier::SourceBuild => "source-build",
        }
    }
}

/// A package a source build needs at build time (`node-addon-api`, `nan`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPackage {
    /// `name@version`.
    pub package: String,
    /// The range the module declares, as it declares it.
    pub range: String,
    /// Its licence, as the registry states it.
    pub licence: Option<String>,
    /// Its npm tarball.
    pub artifact: Artifact,
}

/// The Linux build of one native module at one version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleEntry {
    /// How it is supplied.
    pub tier: Tier,
    /// Its licence, as the registry states it.
    pub licence: Option<String>,
    /// The file fetched: the npm tarball, or the release archive.
    pub artifact: Artifact,
    /// `npm-prebuild` and `release-prebuild`: the members taken, relative to
    /// the archive's root (npm's `package/` is not part of them).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// `source-build`: whether the source has a `binding.gyp`.
    #[serde(default)]
    pub node_gyp: bool,
    /// `source-build`: the module's own `os` field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<String>,
    /// `source-build`: what the build needs besides the source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_packages: Vec<BuildPackage>,
}

/// The index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    /// `1`.
    pub schema_version: u32,
    /// The day the index was written.
    pub checked: String,
    /// What wrote it.
    pub generated_by: String,
    /// Electron releases by version (`43.7.7`).
    pub electron: BTreeMap<String, ElectronRelease>,
    /// Native modules and helper packages by `name@version`.
    pub modules: BTreeMap<String, ModuleEntry>,
}

impl Index {
    /// The index compiled into the program.
    pub fn built_in() -> Result<Index, Failure> {
        Index::parse(BUILT_IN, "the built-in index")
    }

    /// Reads an index from a file (at most 16 MiB).
    pub fn read(path: &Path) -> Result<Index, Failure> {
        let size = std::fs::metadata(path)
            .map_err(|error| {
                Failure::new(
                    Code::IndexInvalid,
                    format!("cannot read the index {}: {error}", path.display()),
                )
            })?
            .len();
        if size > 16 << 20 {
            return Err(Failure::new(
                Code::IndexInvalid,
                format!("the index {} is larger than 16 MiB", path.display()),
            ));
        }
        let text = std::fs::read_to_string(path).map_err(|error| {
            Failure::new(
                Code::IndexInvalid,
                format!("cannot read the index {}: {error}", path.display()),
            )
        })?;
        Index::parse(&text, &path.display().to_string())
    }

    /// Parses and validates an index.
    pub fn parse(text: &str, origin: &str) -> Result<Index, Failure> {
        let index: Index = serde_json::from_str(text).map_err(|error| {
            Failure::new(
                Code::IndexInvalid,
                format!("{origin} is not a valid index: {error}"),
            )
        })?;
        let problems = index.problems();
        if let Some(first) = problems.first() {
            return Err(Failure::new(
                Code::IndexInvalid,
                format!("{origin}: {first} ({} problem(s) in all)", problems.len()),
            ));
        }
        Ok(index)
    }

    /// Everything wrong with the index; empty when it is valid.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.schema_version != 1 {
            out.push(format!("schema_version is {}, not 1", self.schema_version));
        }
        for (version, release) in &self.electron {
            if !is_version(version) {
                out.push(format!("electron {version:?} is not a version"));
            }
            for (what, artifact) in [
                ("linux_x64", &release.linux_x64),
                ("ffmpeg_open_codecs", &release.ffmpeg_open_codecs),
                ("headers", &release.headers),
            ] {
                if let Some(problem) = artifact_problem(artifact) {
                    out.push(format!("electron {version} {what}: {problem}"));
                }
            }
        }
        for (identity, module) in &self.modules {
            if split_identity(identity).is_none() {
                out.push(format!("module {identity:?} is not name@version"));
            }
            if let Some(problem) = artifact_problem(&module.artifact) {
                out.push(format!("module {identity}: {problem}"));
            }
            for file in &module.files {
                if crate::paths::safe_relative(file).is_none() {
                    out.push(format!("module {identity}: file {file:?} is not a safe path"));
                }
            }
            for package in &module.build_packages {
                if split_identity(&package.package).is_none() {
                    out.push(format!(
                        "module {identity}: build package {:?} is not name@version",
                        package.package
                    ));
                }
                if let Some(problem) = artifact_problem(&package.artifact) {
                    out.push(format!("module {identity} build package: {problem}"));
                }
            }
        }
        out
    }

    /// The pinned Electron release of `version`.
    pub fn electron(&self, version: &str) -> Option<&ElectronRelease> {
        self.electron.get(version)
    }

    /// The pinned Linux build of `name@version`.
    pub fn module(&self, identity: &str) -> Option<&ModuleEntry> {
        self.modules.get(identity)
    }
}

/// Splits `name@version` (the name may be scoped: `@scope/name@1.0.0`).
pub fn split_identity(identity: &str) -> Option<(&str, &str)> {
    let at = identity.rfind('@')?;
    if at == 0 {
        return None;
    }
    let (name, version) = (&identity[..at], &identity[at + 1..]);
    let name_ok = !name.is_empty()
        && name.len() <= 214
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"@/._-".contains(&byte))
        && !name.contains("..");
    if name_ok && is_version(version) {
        Some((name, version))
    } else {
        None
    }
}

/// Whether `text` is a version token: 1 to 64 of letters, digits, `.`,
/// `-`, `+`, starting with a digit.
pub fn is_version(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text.as_bytes()[0].is_ascii_digit()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
}

/// Whether `text` is 64 lower-case hexadecimal digits.
pub fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn artifact_problem(artifact: &Artifact) -> Option<String> {
    if !artifact.url.starts_with("https://")
        || artifact.url.len() > 2048
        || !artifact.url.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Some(format!("{:?} is not an https URL", artifact.url));
    }
    if !is_sha256(&artifact.sha256) {
        return Some("the SHA-256 is not 64 lower-case hexadecimal digits".to_owned());
    }
    if artifact.size == 0 || artifact.size > MAX_ARTIFACT_SIZE {
        return Some(format!("the size {} is not between 1 byte and 1 GiB", artifact.size));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_index_is_valid() {
        let index = Index::built_in().expect("data/index.json is valid");
        assert!(index.problems().is_empty());
        // The 19 Electron versions the experiment's 40 working variants
        // need (docs/reports/exp-rehost-1.md, 3.1).
        assert_eq!(index.electron.len(), 19);
        let release = index.electron("43.7.7").expect("VSCodium's Electron");
        assert_eq!(release.abi, 148);
        // The digest the experiment recorded and Electron's SHASUMS256.txt
        // published for the same file.
        assert_eq!(
            release.linux_x64.sha256,
            "4d0a48398c444258dbcf2f5f83b49ca5bc53583130f354e0c299dad5b22b5271"
        );
        assert!(release.linux_x64.url.starts_with(
            "https://github.com/electron/electron/releases/download/v43.7.7/"
        ));
    }

    #[test]
    fn every_pinned_file_comes_from_a_project_s_own_location() {
        let index = Index::built_in().unwrap();
        let allowed = [
            "https://github.com/",
            "https://artifacts.electronjs.org/",
            "https://registry.npmjs.org/",
        ];
        let mut urls: Vec<&str> = Vec::new();
        for release in index.electron.values() {
            urls.extend([
                release.linux_x64.url.as_str(),
                release.ffmpeg_open_codecs.url.as_str(),
                release.headers.url.as_str(),
            ]);
        }
        for module in index.modules.values() {
            urls.push(module.artifact.url.as_str());
            urls.extend(module.build_packages.iter().map(|p| p.artifact.url.as_str()));
        }
        for url in urls {
            assert!(
                allowed.iter().any(|prefix| url.starts_with(prefix)),
                "{url} is not on an allowed host"
            );
        }
    }

    #[test]
    fn source_builds_name_only_open_source_licences() {
        // Only modules published under an open licence are built
        // (docs/reports/exp-rehost-1.md, 9.2).
        let index = Index::built_in().unwrap();
        for (identity, module) in &index.modules {
            let licence = module.licence.as_deref().unwrap_or("");
            let open = ["MIT", "BSD", "Apache", "ISC", "LGPL", "MPL"]
                .iter()
                .any(|name| licence.contains(name));
            assert!(open, "{identity} has licence {licence:?}");
        }
    }

    #[test]
    fn identities_split_at_the_last_at_sign() {
        assert_eq!(
            split_identity("@vscode/spdlog@0.15.8"),
            Some(("@vscode/spdlog", "0.15.8"))
        );
        assert_eq!(
            split_identity("node-pty@1.2.0-beta.15"),
            Some(("node-pty", "1.2.0-beta.15"))
        );
        assert_eq!(split_identity("@vscode/spdlog"), None);
        assert_eq!(split_identity("x@"), None);
        assert_eq!(split_identity("../x@1.0.0"), None);
    }

    #[test]
    fn an_index_with_a_bad_pin_is_refused() {
        let text = r#"{"schema_version":1,"checked":"2026-11-16","generated_by":"test",
            "electron":{"1.0.0":{"abi":1,"chromium":"1","node":"1",
              "linux_x64":{"url":"http://example.org/e.zip","sha256":"00","size":1,"hash_source":"x"},
              "ffmpeg_open_codecs":{"url":"https://example.org/f.zip","sha256":"0000000000000000000000000000000000000000000000000000000000000000","size":1,"hash_source":"x"},
              "headers":{"url":"https://example.org/h.tgz","sha256":"0000000000000000000000000000000000000000000000000000000000000000","size":1,"hash_source":"x"}}},
            "modules":{}}"#;
        let failure = Index::parse(text, "test").unwrap_err();
        assert_eq!(failure.code, Code::IndexInvalid);
        assert!(failure.message.contains("https"), "{}", failure.message);
    }
}
