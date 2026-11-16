// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What the detector says about an opened package: the types only.
//!
//! [`crate::detect::detect`] walks a directory (a package opened by
//! [`crate::unpack`], or a directory or `.app` bundle the person named) and
//! returns [`Finding`]s, the most significant first. Nothing is executed:
//! versions are read from version strings in programs, property lists,
//! JSON files and archive headers (docs/reports/exp-rehost-1.md, 2.2).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

/// The portable runtime a package is built on, or the statement that it is
/// native code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    /// Electron: `app.asar` or `app/` beside Chromium and Node.js.
    Electron,
    /// NW.js: `package.nw` or the application beside `nw.dll`.
    Nwjs,
    /// .NET (Core and later), with a `runtimeconfig.json`.
    Dotnet,
    /// .NET Framework: Windows only, never re-hosted.
    DotnetFramework,
    /// A Java program: jars, a jpackage image, a launcher with a jar.
    Jvm,
    /// A Godot game: a `.pck` beside or inside the program.
    Godot,
    /// A LÖVE game: a `.love` archive, or one fused to `love.exe`.
    Love,
    /// A Ren'Py visual novel: `renpy/` and `game/`.
    Renpy,
    /// A frozen Python program (PyInstaller).
    Pyinstaller,
    /// Native code with no portable runtime: never re-hosted.
    Native,
}

impl Family {
    /// Every family, in the order of this list.
    pub const ALL: [Family; 10] = [
        Family::Electron,
        Family::Nwjs,
        Family::Dotnet,
        Family::DotnetFramework,
        Family::Jvm,
        Family::Godot,
        Family::Love,
        Family::Renpy,
        Family::Pyinstaller,
        Family::Native,
    ];

    /// The family as written in JSON and in catalogue records, for example
    /// `electron`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Family::Electron => "electron",
            Family::Nwjs => "nwjs",
            Family::Dotnet => "dotnet",
            Family::DotnetFramework => "dotnet-framework",
            Family::Jvm => "jvm",
            Family::Godot => "godot",
            Family::Love => "love",
            Family::Renpy => "renpy",
            Family::Pyinstaller => "pyinstaller",
            Family::Native => "native",
        }
    }

    /// The family's name for people.
    pub const fn label(self) -> &'static str {
        match self {
            Family::Electron => "Electron",
            Family::Nwjs => "NW.js",
            Family::Dotnet => ".NET",
            Family::DotnetFramework => ".NET Framework",
            Family::Jvm => "Java",
            Family::Godot => "Godot",
            Family::Love => "LÖVE",
            Family::Renpy => "Ren'Py",
            Family::Pyinstaller => "frozen Python",
            Family::Native => "native code",
        }
    }
}

/// The operating system a package was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    /// Windows: PE programs.
    Windows,
    /// macOS: Mach-O programs, `.app` bundles.
    Macos,
    /// Linux: ELF programs.
    Linux,
    /// No platform-specific program at all (a plain `.love`, a jar).
    Any,
    /// Not known.
    Unknown,
}

impl Platform {
    /// The platform as written in JSON, for example `windows`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Macos => "macos",
            Platform::Linux => "linux",
            Platform::Any => "any",
            Platform::Unknown => "unknown",
        }
    }
}

/// What the detector concludes about the package, before anything is
/// fetched. A fact about the package, never about whether the application
/// works: that needs a measurement (catalogue-and-routing.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// The payload can be laid under the Linux runtime as it is.
    Rehostable,
    /// The payload can be re-hosted once platform pieces are swapped: see
    /// the reasons.
    RehostableWithWork,
    /// Not re-hostable: native code, a Windows-only framework, an encrypted
    /// pack. The reasons say which.
    NotRehostable,
}

/// The machine-code format of a file, read from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BinaryFormat {
    /// Windows PE.
    Pe,
    /// Linux and Unix ELF.
    Elf,
    /// macOS Mach-O, thin or universal.
    MachO,
}

impl BinaryFormat {
    /// The platform programs of this format are built for.
    pub const fn platform(self) -> Platform {
        match self {
            BinaryFormat::Pe => Platform::Windows,
            BinaryFormat::Elf => Platform::Linux,
            BinaryFormat::MachO => Platform::Macos,
        }
    }
}

/// A file's machine-code format and the processor architectures it holds
/// code for (`x86_64`, `x86`, `arm64`, `arm`, ... ; `?` when not read).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BinaryKind {
    /// The format.
    pub format: BinaryFormat,
    /// The architectures, in the order the file lists them.
    pub archs: Vec<String>,
}

impl BinaryKind {
    /// Whether the file holds code for x86-64 Linux.
    pub fn is_linux_x86_64(&self) -> bool {
        self.format == BinaryFormat::Elf && self.archs.iter().any(|arch| arch == "x86_64")
    }
}

/// Where a version was read from, and what it said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    /// The place, in a few words: `user-agent string in Joplin.exe`.
    pub from: String,
    /// The value read there.
    pub value: String,
}

/// Where a file of an Electron payload lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Location {
    /// Inside the asar archive's data.
    Packed,
    /// Beside the asar archive, in `<archive>.unpacked/`.
    Unpacked,
    /// In a plain directory payload (`resources/app/`).
    Directory,
}

/// A native Node module (`*.node`) of an Electron or NW.js payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NativeModule {
    /// Its path inside the payload, `/`-separated, relative to the payload's
    /// root (the asar archive's root, or the `app/` directory).
    pub path: String,
    /// Where the file lives.
    pub location: Location,
    /// The machine code it holds.
    pub kind: BinaryKind,
    /// Its size in bytes.
    pub size: u64,
    /// The npm package that owns it: the `name` of the nearest
    /// `package.json` above it. `None` when there is none, which makes the
    /// module a dead end (docs/reports/exp-rehost-1.md, 3.2).
    pub package: Option<String>,
    /// That package's `version`.
    pub version: Option<String>,
    /// The directory of that `package.json`, relative to the payload root.
    pub package_dir: Option<String>,
    /// That package's `os` field: the systems it declares, `!` for an
    /// exclusion. Empty when it has none.
    pub os: Vec<String>,
}

impl NativeModule {
    /// `name@version` of the owning package, when both are known.
    pub fn identity(&self) -> Option<String> {
        match (&self.package, &self.version) {
            (Some(name), Some(version)) => Some(format!("{name}@{version}")),
            _ => None,
        }
    }

    /// Whether the owning package says it is not for Linux, by npm's rules
    /// for `os`: a list of systems, or of exclusions (`!win32`).
    pub fn declared_not_for_linux(&self) -> bool {
        if self.os.iter().any(|system| system == "!linux") {
            return true;
        }
        let allowed: Vec<&String> = self
            .os
            .iter()
            .filter(|system| !system.starts_with('!'))
            .collect();
        !allowed.is_empty() && !allowed.iter().any(|system| system.as_str() == "linux")
    }
}

/// A program or library beside the payload that is built for one platform
/// and is not a Node module: a helper program (ripgrep, ffmpeg) or a
/// library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sidecar {
    /// Its path, `/`-separated, relative to the Electron `resources`
    /// directory (or to the application directory for other families).
    pub path: String,
    /// The machine code it holds.
    pub kind: BinaryKind,
    /// Its size in bytes.
    pub size: u64,
}

/// How an Electron payload is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PayloadKind {
    /// `resources/app.asar`, with `app.asar.unpacked/` beside it.
    Asar,
    /// `resources/app/`, a plain directory.
    Directory,
}

/// What an Electron package holds besides its version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ElectronPayload {
    /// The directory of the publisher's program (Windows), or the `.app`
    /// bundle (macOS). Absolute.
    pub app_dir: PathBuf,
    /// The `resources` directory (Windows) or `Contents/Resources` (macOS).
    /// Absolute.
    pub resources: PathBuf,
    /// The publisher's program: its file name (Windows), or
    /// `CFBundleExecutable` (macOS).
    pub program: Option<String>,
    /// How the payload is stored.
    pub kind: PayloadKind,
    /// The archive or directory that holds the native modules: `app.asar`,
    /// or `app-x64.asar` of a universal macOS build, or `app/`. Absolute.
    pub modules_root: PathBuf,
    /// `productName`, or `name`, of the payload's `package.json`.
    pub name: Option<String>,
    /// `name` of the payload's `package.json`.
    pub package_name: Option<String>,
    /// `version` of the payload's `package.json`: the application's version.
    pub app_version: Option<String>,
    /// `main` of the payload's `package.json`.
    pub main: Option<String>,
    /// The Chromium version the publisher's binary names.
    pub chromium: Option<String>,
    /// The fuse wire of the publisher's binary, by fuse name: `0` off, `1`
    /// on, `r` removed. `None` when the binary has none.
    pub fuses: Option<BTreeMap<String, String>>,
    /// macOS: `CFBundleIdentifier`.
    pub bundle_id: Option<String>,
    /// `electron-updater` when the package carries `app-update.yml`.
    pub updater: Option<String>,
}

/// What the detector found in one place of the package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// The runtime family.
    pub family: Family,
    /// For [`Family::Native`]: the toolkit, as the detector names it (`qt`,
    /// `flutter`, `tauri`, `unity`, `cef`, `webview2`, ... or `unknown`).
    pub toolkit: Option<String>,
    /// What the package was built for.
    pub platform: Platform,
    /// The directory the finding is about. Absolute.
    pub app_dir: PathBuf,
    /// Its depth below the root the detector was given.
    pub depth: usize,
    /// The architectures of the publisher's program.
    pub arch: Vec<String>,
    /// The runtime's version, when it was read.
    pub version: Option<String>,
    /// Where each version reading came from; the first is `version`.
    pub version_evidence: Vec<Evidence>,
    /// The verdict.
    pub verdict: Verdict,
    /// Machine-readable reasons, in kebab case (see `docs/architecture/rehost.md`).
    pub reasons: Vec<String>,
    /// Sentences for a person.
    pub notes: Vec<String>,
    /// Electron: the payload. `None` for other families.
    pub electron: Option<ElectronPayload>,
    /// Native Node modules (Electron, NW.js), or native libraries (.NET).
    pub natives: Vec<NativeModule>,
    /// Platform-specific programs and libraries that are not Node modules.
    pub sidecars: Vec<Sidecar>,
}

impl Finding {
    /// One line for a person, for example `electron 43.7.7, built for
    /// windows: rehostable-with-work [native-modules]`.
    pub fn summary(&self) -> String {
        let version = self.version.as_deref().unwrap_or("version unknown");
        let verdict = match self.verdict {
            Verdict::Rehostable => "rehostable",
            Verdict::RehostableWithWork => "rehostable-with-work",
            Verdict::NotRehostable => "not-rehostable",
        };
        let mut text = format!(
            "{} {version}, built for {}: {verdict}",
            self.family.as_str(),
            self.platform.as_str()
        );
        if !self.reasons.is_empty() {
            text.push_str(&format!(" [{}]", self.reasons.join(", ")));
        }
        text
    }

    /// The native modules built for another platform than Linux.
    pub fn foreign_natives(&self) -> impl Iterator<Item = &NativeModule> {
        self.natives
            .iter()
            .filter(|module| module.kind.format != BinaryFormat::Elf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(os: &[&str]) -> NativeModule {
        NativeModule {
            path: "node_modules/x/build/Release/x.node".to_owned(),
            location: Location::Unpacked,
            kind: BinaryKind {
                format: BinaryFormat::Pe,
                archs: vec!["x86_64".to_owned()],
            },
            size: 1024,
            package: Some("x".to_owned()),
            version: Some("1.0.0".to_owned()),
            package_dir: Some("node_modules/x".to_owned()),
            os: os.iter().map(|system| (*system).to_owned()).collect(),
        }
    }

    #[test]
    fn npm_os_lists_are_read_as_npm_reads_them() {
        assert!(!module(&[]).declared_not_for_linux());
        assert!(module(&["darwin"]).declared_not_for_linux());
        assert!(module(&["win32"]).declared_not_for_linux());
        assert!(!module(&["darwin", "linux"]).declared_not_for_linux());
        // An exclusion of another system is not an exclusion of Linux
        // (run 43754196480 read fs-xattr's ["!win32"] that way).
        assert!(!module(&["!win32"]).declared_not_for_linux());
        assert!(module(&["!linux"]).declared_not_for_linux());
    }

    #[test]
    fn a_module_has_an_identity_only_with_name_and_version() {
        assert_eq!(module(&[]).identity().as_deref(), Some("x@1.0.0"));
        let mut anonymous = module(&[]);
        anonymous.package = None;
        assert_eq!(anonymous.identity(), None);
    }

    #[test]
    fn every_family_is_written_once() {
        let mut names: Vec<&str> = Family::ALL.iter().map(|family| family.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Family::ALL.len());
    }
}
