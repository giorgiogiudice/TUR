// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Native Node modules: the Linux build of each one a Windows or macOS
//! Electron payload carries, and where it goes (docs/reports/exp-rehost-1.md,
//! section 4).
//!
//! For every package that owns a foreign `.node` file, the first that
//! applies:
//!
//! | Resolution | When | What happens |
//! |---|---|---|
//! | `not-for-linux` | the package's `os` field excludes Linux (`fsevents`) | nothing: Linux never loads it |
//! | `carried` | the package already holds a Linux x86-64 build | that build is copied over the foreign one |
//! | `npm-package` | the binary lives in a per-platform npm package (`@scope/x-win32-x64-msvc`) and TUR's index pins its `linux-x64` sibling | the sibling is fetched, verified and laid beside it |
//! | `npm-prebuild` | the index pins a Linux build in the module's own npm tarball | those files are fetched and laid in place |
//! | `release-prebuild` | the index pins the module's release archive | its `.node` file goes where the module's loader looks |
//! | `source-build` | the index pins the module's npm source, and the person allowed builds | built on this computer, without network ([`crate::build`]) |
//! | `needs-build` | as above, and builds are not allowed | a refusal (`TUR-REHOST-NATIVE-002`), unless the person accepted running without it |
//! | `unresolved` | no package.json above the file, or nothing pinned | a refusal (`TUR-REHOST-NATIVE-001`), unless accepted |
//!
//! "Accepted" means the module is on the `--allow-unresolved` list, which
//! `tur-apps` fills from the catalogue record that measured the application
//! working without it (a module the application does not load at start).
//! Nothing is ever guessed or fetched outside TUR's index.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::asar::Asar;
use crate::codes::{Code, Failure};
use crate::finding::{BinaryFormat, Finding, NativeModule, PayloadKind};
use crate::index::{Artifact, Index, ModuleEntry, Tier};
use crate::paths::safe_relative;

/// Per-platform package name parts, matched as whole words: `darwin-x64`
/// contains `win-x64`, which run 43768570046 replaced by mistake.
pub const PLATFORM_TOKENS: [&str; 11] = [
    "win32-x64-msvc",
    "win32-x64-gnu",
    "win32-x64",
    "windows-x64",
    "win-x64",
    "darwin-x64",
    "darwin-arm64",
    "darwin-universal",
    "macos-x64",
    "macos-arm64",
    "osx-x64",
];

/// The Linux names tried in place of a platform token, in order.
pub const LINUX_TOKENS: [&str; 3] = ["linux-x64-gnu", "linux-x64-glibc", "linux-x64"];

/// How a module's Linux build is supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resolution {
    /// The package says it is not for Linux.
    NotForLinux,
    /// A Linux build the package already carries.
    Carried,
    /// The per-platform package's Linux sibling, from npm.
    NpmPackage,
    /// Files of the module's own npm tarball.
    NpmPrebuild,
    /// The module's release archive.
    ReleasePrebuild,
    /// Built from the module's npm source on this computer.
    SourceBuild,
    /// Would be built from source; builds are not allowed.
    NeedsBuild,
    /// No Linux build TUR can supply.
    Unresolved,
}

impl Resolution {
    /// Whether the module still lacks a Linux build after this.
    pub const fn leaves_it_foreign(self) -> bool {
        matches!(self, Resolution::NeedsBuild | Resolution::Unresolved)
    }
}

/// A file fetched for a module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Download {
    /// What it is.
    pub what: String,
    /// The pinned file.
    pub artifact: Artifact,
    /// Its licence, as the registry states it.
    pub licence: Option<String>,
}

/// What happens to one module (or one per-platform helper package).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModulePlan {
    /// `name@version`, when the package can be identified.
    pub identity: Option<String>,
    /// The package's directory inside the payload.
    pub package_dir: Option<String>,
    /// The foreign files, inside the payload.
    pub files: Vec<String>,
    /// How it is resolved.
    pub resolution: Resolution,
    /// Why, in words.
    pub detail: String,
    /// The index entry used (`name@version`), which may be a sibling's.
    pub source: Option<String>,
    /// What is fetched for it.
    pub downloads: Vec<Download>,
    /// Whether the person accepted running without a Linux build of it.
    pub accepted: bool,
    /// Where files were put, once applied.
    pub applied: Vec<String>,
    /// Whether this is a helper program's package rather than a Node
    /// module (`@vscode/ripgrep-win32-x64`).
    pub helper: bool,
}

impl ModulePlan {
    /// The condition this plan raises, when it blocks the installation.
    pub fn blocker(&self) -> Option<Code> {
        if self.accepted {
            return None;
        }
        match self.resolution {
            Resolution::Unresolved => Some(Code::ModuleUnresolved),
            Resolution::NeedsBuild => Some(Code::BuildNotAllowed),
            _ => None,
        }
    }

    /// How the person names this module in `--allow-unresolved`.
    pub fn names(&self) -> Vec<String> {
        let mut out: Vec<String> = self.identity.iter().cloned().collect();
        for file in &self.files {
            if let Some(base) = file.rsplit('/').next() {
                out.push(format!("file:{base}"));
            }
        }
        out
    }
}

/// What the planner needs besides the payload.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// TUR's index.
    pub index: &'a Index,
    /// The payload's Electron ABI.
    pub abi: u32,
    /// Whether the person allowed source builds.
    pub allow_builds: bool,
    /// Modules the person accepts running without (`name@version` or
    /// `file:<name>`).
    pub allow_unresolved: &'a [String],
}

/// The payload's files, wherever they are: an asar archive, or a
/// directory (which may keep its modules in an inner `node_modules.asar`).
pub enum Store {
    /// `app.asar` (and `app.asar.unpacked/`).
    Asar(Box<Asar>),
    /// `app/`.
    Directory(PathBuf),
}

impl Store {
    /// Opens the payload whose modules live at `modules_root`.
    pub fn open(modules_root: &Path, kind: PayloadKind) -> Result<Store, Failure> {
        match kind {
            PayloadKind::Asar => Asar::open(modules_root)
                .map(|asar| Store::Asar(Box::new(asar)))
                .map_err(|error| {
                    Failure::new(
                        Code::Unreadable,
                        format!("cannot read {}: {error}", modules_root.display()),
                    )
                }),
            PayloadKind::Directory => Ok(Store::Directory(modules_root.to_path_buf())),
        }
    }

    /// Reads a JSON file of the payload (at most 4 MiB).
    pub fn read_json(&self, rel: &str) -> Option<Value> {
        match self {
            Store::Asar(asar) => asar.read_json(rel),
            Store::Directory(root) => {
                let path = root.join(safe_relative(rel)?);
                if let Ok(metadata) = std::fs::symlink_metadata(&path)
                    && metadata.is_file()
                    && metadata.len() <= 4 << 20
                {
                    let text = std::fs::read(&path).ok()?;
                    let text = text.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&text);
                    return serde_json::from_slice(text).ok();
                }
                // Modules kept in an inner archive (VS Code's
                // node_modules.asar beside node_modules.asar.unpacked/).
                let (head, tail) = rel.split_once(".asar.unpacked/")?;
                let inner = root.join(safe_relative(&format!("{head}.asar"))?);
                Asar::open(&inner).ok()?.read_json(tail)
            }
        }
    }

    /// Reads a file of the payload, up to `limit` bytes.
    pub fn read(&self, rel: &str, limit: u64) -> Option<Vec<u8>> {
        match self {
            Store::Asar(asar) => asar.read(rel, limit),
            Store::Directory(root) => {
                let path = root.join(safe_relative(rel)?);
                let metadata = std::fs::symlink_metadata(&path).ok()?;
                if !metadata.is_file() || metadata.len() > limit {
                    return None;
                }
                std::fs::read(path).ok()
            }
        }
    }

    /// Puts a file into the payload: into the archive's unpacked
    /// directory (a native module must be a real file), or the directory.
    pub fn put(&mut self, rel: &str, data: &[u8], executable: bool) -> Result<(), Failure> {
        let failed = |error: std::io::Error| {
            Failure::new(Code::StepFailed, format!("cannot write {rel} into the payload: {error}"))
        };
        match self {
            Store::Asar(asar) => asar.put_unpacked(rel, data, executable).map_err(failed),
            Store::Directory(root) => {
                let relative = safe_relative(rel).ok_or_else(|| {
                    Failure::new(Code::StepFailed, format!("{rel:?} is not a safe path"))
                })?;
                let path = root.join(relative);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(failed)?;
                }
                // Never write through a link the package left there.
                if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                    std::fs::remove_file(&path).map_err(failed)?;
                }
                std::fs::write(&path, data).map_err(failed)?;
                use std::os::unix::fs::PermissionsExt;
                let mode = if executable { 0o755 } else { 0o644 };
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).map_err(failed)
            }
        }
    }

    /// Writes the archive's new header, when anything changed.
    pub fn save(&mut self) -> Result<(), Failure> {
        match self {
            Store::Asar(asar) => asar.save().map_err(|error| {
                Failure::new(Code::StepFailed, format!("cannot rewrite the payload archive: {error}"))
            }),
            Store::Directory(_) => Ok(()),
        }
    }
}

/// The platform token of a per-platform package name, as a whole word.
pub fn platform_token(name: &str) -> Option<&'static str> {
    let bytes = name.as_bytes();
    PLATFORM_TOKENS.into_iter().find(|token| {
        let needle = token.as_bytes();
        if bytes.len() < needle.len() {
            return false;
        }
        let boundary = |byte: Option<&u8>| byte.is_none_or(|byte| !byte.is_ascii_alphanumeric());
        (0..=bytes.len() - needle.len()).any(|start| {
            let end = start + needle.len();
            &bytes[start..end] == needle
                && boundary(start.checked_sub(1).and_then(|before| bytes.get(before)))
                && boundary(bytes.get(end))
        })
    })
}

/// Lower is better among candidate Linux builds a package carries.
fn rank(path: &str, abi: u32) -> u32 {
    let lower = path.to_ascii_lowercase();
    if lower.contains("musl") || lower.contains("arm") || lower.contains("ia32") {
        return 99;
    }
    let mut score = 10;
    if lower.contains("napi") {
        score = 1;
    }
    let abi_text = format!("abi{abi}");
    let abi_dash = format!("-v{abi}-");
    if abi > 0 && (lower.contains(&abi_text) || lower.contains(&abi_dash)) {
        score = if lower.contains("electron") { 0 } else { 2 };
    } else if lower.contains("abi") {
        score = 50;
    }
    if lower.contains("electron") && score > 2 {
        score = 3;
    }
    score
}

fn sibling_base(package_dir: &str, name: &str, sibling: &str) -> Option<String> {
    let base = package_dir.strip_suffix(name)?;
    let joined = format!("{base}{sibling}");
    safe_relative(&joined).map(|_| joined)
}

/// Plans every foreign native module of an Electron finding.
pub fn plan_modules(finding: &Finding, store: &Store, context: &Context<'_>) -> Vec<ModulePlan> {
    let mut groups: BTreeMap<String, Vec<&NativeModule>> = BTreeMap::new();
    for module in &finding.natives {
        let key = module
            .package_dir
            .clone()
            .unwrap_or_else(|| format!("?{}", module.path));
        groups.entry(key).or_default().push(module);
    }
    let mut plans = Vec::new();
    for modules in groups.values() {
        let foreign: Vec<&NativeModule> = modules
            .iter()
            .copied()
            .filter(|module| module.kind.format != BinaryFormat::Elf)
            .collect();
        let Some(first) = foreign.first() else {
            continue;
        };
        let carried: Vec<&NativeModule> = modules
            .iter()
            .copied()
            .filter(|module| module.kind.is_linux_x86_64() && rank(&module.path, context.abi) < 50)
            .collect();
        let mut plan = ModulePlan {
            identity: first.identity(),
            package_dir: first.package_dir.clone(),
            files: foreign.iter().map(|module| module.path.clone()).collect(),
            resolution: Resolution::Unresolved,
            detail: String::new(),
            source: None,
            downloads: Vec::new(),
            accepted: false,
            applied: Vec::new(),
            helper: false,
        };
        resolve(&mut plan, first, &carried, context);
        plan.accepted = plan.resolution.leaves_it_foreign()
            && plan
                .names()
                .iter()
                .any(|name| context.allow_unresolved.contains(name));
        let _ = store;
        plans.push(plan);
    }
    plans
}

fn resolve(plan: &mut ModulePlan, first: &NativeModule, carried: &[&NativeModule], context: &Context<'_>) {
    let (Some(name), Some(version), Some(_)) = (&first.package, &first.version, &first.package_dir)
    else {
        plan.detail = "no package.json above the file: the module cannot be identified (it may be the application's own, or bundled)".to_owned();
        return;
    };
    if first.declared_not_for_linux() {
        plan.resolution = Resolution::NotForLinux;
        plan.detail = format!("{name} declares os {:?}: Linux never loads it", first.os);
        return;
    }
    if let Some(best) = carried.iter().min_by_key(|module| rank(&module.path, context.abi)) {
        plan.resolution = Resolution::Carried;
        plan.detail = format!("the package carries a Linux build: {}", best.path);
        plan.source = Some(best.path.clone());
        return;
    }
    if let Some(token) = platform_token(name) {
        for linux in LINUX_TOKENS {
            let sibling = name.replacen(token, linux, 1);
            let identity = format!("{sibling}@{version}");
            if let Some(entry) = context.index.module(&identity)
                && entry.tier == Tier::NpmPackage
            {
                plan.resolution = Resolution::NpmPackage;
                plan.detail = format!("{identity}, the Linux sibling of {name}, from npm");
                plan.source = Some(identity.clone());
                plan.downloads.push(download(&identity, entry));
                return;
            }
        }
    }
    let identity = format!("{name}@{version}");
    let Some(entry) = context.index.module(&identity) else {
        plan.detail = format!("{identity}: TUR's index pins no Linux build of it");
        return;
    };
    plan.source = Some(identity.clone());
    match entry.tier {
        Tier::NpmPrebuild => {
            plan.resolution = Resolution::NpmPrebuild;
            plan.detail = format!("{} from the npm tarball of {identity}", entry.files.join(", "));
            plan.downloads.push(download(&identity, entry));
        }
        Tier::ReleasePrebuild => {
            plan.resolution = Resolution::ReleasePrebuild;
            plan.detail = format!("the release archive {}", entry.artifact.url);
            plan.downloads.push(download(&identity, entry));
        }
        Tier::SourceBuild if !entry.node_gyp => {
            plan.detail = format!("{identity} has no Linux build and is not a node-gyp project");
        }
        Tier::SourceBuild if context.allow_builds => {
            plan.resolution = Resolution::SourceBuild;
            plan.detail = format!(
                "built from the npm source of {identity} on this computer, without network, with Fedora's compiler"
            );
            plan.downloads.push(download(&identity, entry));
            for package in &entry.build_packages {
                plan.downloads.push(Download {
                    what: format!("{} (needed to build {identity})", package.package),
                    artifact: package.artifact.clone(),
                    licence: package.licence.clone(),
                });
            }
        }
        Tier::SourceBuild => {
            plan.resolution = Resolution::NeedsBuild;
            plan.detail = format!(
                "{identity} has no published Linux build: it must be built from source on this computer, and builds are not allowed"
            );
        }
        Tier::NpmPackage => {
            plan.detail = format!("{identity} is pinned as a sibling package, not as this module's build");
        }
    }
}

fn download(identity: &str, entry: &ModuleEntry) -> Download {
    Download {
        what: format!("{identity} ({})", entry.tier.as_str()),
        artifact: entry.artifact.clone(),
        licence: entry.licence.clone(),
    }
}

/// Plans the per-platform npm packages that carry a helper program rather
/// than a Node module (`@vscode/ripgrep-win32-x64` holds `rg.exe`; run
/// 43754196480: MarkText died with "Could not find
/// @vscode/ripgrep-linux-x64").
pub fn plan_helpers(finding: &Finding, store: &Store, context: &Context<'_>) -> Vec<ModulePlan> {
    let Some(payload) = &finding.electron else {
        return Vec::new();
    };
    let prefix = match payload.kind {
        PayloadKind::Asar => payload
            .modules_root
            .file_name()
            .map(|name| format!("{}.unpacked/", name.to_string_lossy())),
        PayloadKind::Directory => Some("app/".to_owned()),
    };
    let Some(prefix) = prefix else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut plans = Vec::new();
    for sidecar in &finding.sidecars {
        if sidecar.kind.format == BinaryFormat::Elf {
            continue;
        }
        let Some(inside) = sidecar.path.strip_prefix(&prefix) else {
            continue;
        };
        let Some(at) = inside.rfind("node_modules/") else {
            continue;
        };
        let rest = &inside[at + "node_modules/".len()..];
        let mut parts = rest.split('/');
        let Some(first) = parts.next() else {
            continue;
        };
        let name = if first.starts_with('@') {
            match parts.next() {
                Some(second) => format!("{first}/{second}"),
                None => continue,
            }
        } else {
            first.to_owned()
        };
        let package_dir = format!("{}{name}", &inside[..at + "node_modules/".len()]);
        let Some(token) = platform_token(&name) else {
            continue;
        };
        if !seen.insert(package_dir.clone()) {
            continue;
        }
        let version = store
            .read_json(&format!("{package_dir}/package.json"))
            .and_then(|json| json.get("version").and_then(Value::as_str).map(str::to_owned));
        let mut plan = ModulePlan {
            identity: version.as_ref().map(|version| format!("{name}@{version}")),
            package_dir: Some(package_dir.clone()),
            files: vec![inside.to_owned()],
            resolution: Resolution::Unresolved,
            detail: String::new(),
            source: None,
            downloads: Vec::new(),
            accepted: false,
            applied: Vec::new(),
            helper: true,
        };
        if let Some(version) = &version {
            for linux in LINUX_TOKENS {
                let identity = format!("{}@{version}", name.replacen(token, linux, 1));
                if let Some(entry) = context.index.module(&identity)
                    && entry.tier == Tier::NpmPackage
                {
                    plan.resolution = Resolution::NpmPackage;
                    plan.detail = format!("{identity}, the Linux build of the helper program, from npm");
                    plan.source = Some(identity.clone());
                    plan.downloads.push(download(&identity, entry));
                    break;
                }
            }
        }
        if plan.resolution == Resolution::Unresolved {
            plan.detail = format!(
                "the helper package {name} has no Linux sibling in TUR's index; the program it carries will not run"
            );
            // A helper program missing is not loaded at start by itself:
            // reported, and accepted when the person accepts the package.
            plan.accepted = plan
                .names()
                .iter()
                .any(|name| context.allow_unresolved.contains(name));
        }
        plans.push(plan);
    }
    plans
}

/// Files of an extracted archive to lay into the payload: `(path inside
/// the archive, bytes)`.
pub type Extracted = Vec<(String, Vec<u8>)>;

/// Lays a resolved module into the payload. `extracted` holds the files of
/// its downloaded archive, relative to the archive's root with npm's
/// `package/` taken off; `built` the `.node` files a source build produced.
pub fn apply(
    plan: &mut ModulePlan,
    store: &mut Store,
    context: &Context<'_>,
    extracted: &Extracted,
    built: &Extracted,
) -> Result<(), Failure> {
    let Some(package_dir) = plan.package_dir.clone() else {
        return Ok(());
    };
    match plan.resolution {
        Resolution::NotForLinux | Resolution::NeedsBuild | Resolution::Unresolved => Ok(()),
        Resolution::Carried => {
            let Some(source) = plan.source.clone() else {
                return Ok(());
            };
            let data = store.read(&source, 256 << 20).ok_or_else(|| {
                Failure::new(Code::StepFailed, format!("cannot read the carried Linux build {source}"))
            })?;
            over(plan, store, &data)
        }
        Resolution::NpmPackage => {
            let (Some(identity), Some(source)) = (plan.identity.clone(), plan.source.clone()) else {
                return Ok(());
            };
            let Some((name, _)) = crate::index::split_identity(&identity) else {
                return Ok(());
            };
            let Some((sibling, _)) = crate::index::split_identity(&source) else {
                return Ok(());
            };
            let base = sibling_base(&package_dir, name, sibling).ok_or_else(|| {
                Failure::new(Code::StepFailed, format!("cannot place {sibling} beside {package_dir}"))
            })?;
            for (rel, data) in extracted {
                let target = format!("{base}/{rel}");
                store.put(&target, data, is_executable(data, rel))?;
            }
            plan.applied.push(base);
            Ok(())
        }
        Resolution::NpmPrebuild => {
            let entry = plan
                .source
                .as_deref()
                .and_then(|identity| context.index.module(identity));
            let files: Vec<&String> = entry.map(|entry| entry.files.iter().collect()).unwrap_or_default();
            let mut first_node: Option<Vec<u8>> = None;
            for (rel, data) in extracted {
                if !files.iter().any(|file| *file == rel) {
                    continue;
                }
                let target = format!("{package_dir}/{rel}");
                store.put(&target, data, is_executable(data, rel))?;
                plan.applied.push(target);
                if first_node.is_none() && rel.ends_with(".node") {
                    first_node = Some(data.clone());
                }
            }
            match first_node {
                Some(data) => over(plan, store, &data),
                None => Err(Failure::new(
                    Code::StepFailed,
                    format!("the npm tarball of {} holds none of the pinned files", plan.identity.clone().unwrap_or_default()),
                )),
            }
        }
        Resolution::ReleasePrebuild | Resolution::SourceBuild => {
            let files = if plan.resolution == Resolution::SourceBuild { built } else { extracted };
            let package = store.read_json(&format!("{package_dir}/package.json"));
            let module_path = module_path(package.as_ref(), plan);
            let nodes: Vec<&(String, Vec<u8>)> = files.iter().filter(|(rel, _)| rel.ends_with(".node")).collect();
            if nodes.is_empty() {
                return Err(Failure::new(
                    Code::StepFailed,
                    format!("no Linux .node file for {}", plan.identity.clone().unwrap_or_default()),
                ));
            }
            let single = nodes.len() == 1;
            for (rel, data) in nodes {
                let base = rel.rsplit('/').next().unwrap_or(rel);
                let mut targets: Vec<String> = Vec::new();
                if plan.resolution == Resolution::SourceBuild {
                    targets.push(format!("{package_dir}/build/Release/{base}"));
                }
                if let Some(directory) = &module_path {
                    targets.push(normalise(&format!("{package_dir}/{directory}/{base}")));
                }
                for file in &plan.files {
                    let same_name = file.rsplit('/').next() == Some(base);
                    let under_build = format!("/{file}").contains("/build/");
                    let built_here = plan.resolution == Resolution::SourceBuild;
                    let wanted = (under_build && (same_name || single)) || (same_name && built_here);
                    if wanted {
                        targets.push(file.clone());
                    }
                }
                if targets.is_empty() {
                    targets.push(format!("{package_dir}/{rel}"));
                }
                let mut written = BTreeSet::new();
                for target in targets {
                    if safe_relative(&target).is_none() || !written.insert(target.clone()) {
                        continue;
                    }
                    store.put(&target, data, true)?;
                    plan.applied.push(target);
                }
            }
            Ok(())
        }
    }
}

/// Replaces foreign builds that sit where a build from source would put
/// them (`build/`, `bin/`): loaders look there first.
fn over(plan: &mut ModulePlan, store: &mut Store, data: &[u8]) -> Result<(), Failure> {
    let targets: Vec<String> = plan
        .files
        .iter()
        .filter(|file| {
            let rooted = format!("/{file}");
            rooted.contains("/build/") || rooted.contains("/bin/")
        })
        .cloned()
        .collect();
    for target in targets {
        store.put(&target, data, true)?;
        plan.applied.push(target);
    }
    Ok(())
}

/// node-pre-gyp's `binary.module_path` with Linux's values, when every
/// placeholder is known.
fn module_path(package: Option<&Value>, plan: &ModulePlan) -> Option<String> {
    let binary = package?.get("binary")?;
    let mut text = binary.get("module_path")?.as_str()?.to_owned();
    let (name, version) = plan
        .identity
        .as_deref()
        .and_then(crate::index::split_identity)
        .map(|(name, version)| (name.to_owned(), version.to_owned()))?;
    let short = name.rsplit('/').next().unwrap_or(&name).to_owned();
    let module_name = binary
        .get("module_name")
        .and_then(Value::as_str)
        .unwrap_or(&short)
        .to_owned();
    let url = plan.downloads.first().map(|d| d.artifact.url.clone()).unwrap_or_default();
    let napi = url
        .split("napi-v")
        .nth(1)
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
        .filter(|digits| !digits.is_empty())
        .unwrap_or_else(|| "6".to_owned());
    let values = [
        ("version", version),
        ("name", short),
        ("module_name", module_name),
        ("platform", "linux".to_owned()),
        ("arch", "x64".to_owned()),
        ("libc", "glibc".to_owned()),
        ("configuration", "Release".to_owned()),
        ("node_abi", "napi".to_owned()),
        ("napi_build_version", napi),
        ("node_abi_napi", "napi".to_owned()),
        ("toolset", String::new()),
    ];
    for (key, value) in values {
        text = text.replace(&format!("{{{key}}}"), &value);
    }
    if text.contains('{') {
        return None;
    }
    Some(text)
}

/// `a/./b/../c` → `a/c`, without touching the file system.
fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn is_executable(data: &[u8], rel: &str) -> bool {
    data.starts_with(b"\x7fELF") || rel.ends_with(".node")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{BinaryKind, ElectronPayload, Location, Platform, Verdict};

    fn module(path: &str, package: Option<(&str, &str, &str)>, format: BinaryFormat, os: &[&str]) -> NativeModule {
        NativeModule {
            path: path.to_owned(),
            location: Location::Unpacked,
            kind: BinaryKind {
                format,
                archs: vec!["x86_64".to_owned()],
            },
            size: 4096,
            package: package.map(|(name, _, _)| name.to_owned()),
            version: package.map(|(_, version, _)| version.to_owned()),
            package_dir: package.map(|(_, _, dir)| dir.to_owned()),
            os: os.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn finding(natives: Vec<NativeModule>) -> Finding {
        Finding {
            family: crate::finding::Family::Electron,
            toolkit: None,
            platform: Platform::Windows,
            app_dir: PathBuf::from("/w/app"),
            depth: 0,
            arch: vec!["x86_64".to_owned()],
            version: Some("43.7.7".to_owned()),
            version_evidence: Vec::new(),
            verdict: Verdict::RehostableWithWork,
            reasons: vec!["native-modules".to_owned()],
            notes: Vec::new(),
            electron: Some(ElectronPayload {
                app_dir: PathBuf::from("/w/app"),
                resources: PathBuf::from("/w/app/resources"),
                program: Some("App.exe".to_owned()),
                kind: PayloadKind::Directory,
                modules_root: PathBuf::from("/w/app/resources/app"),
                name: Some("App".to_owned()),
                package_name: Some("app".to_owned()),
                app_version: Some("1.0.0".to_owned()),
                main: Some("main.js".to_owned()),
                chromium: None,
                fuses: None,
                bundle_id: None,
                updater: None,
            }),
            natives,
            sidecars: Vec::new(),
        }
    }

    fn context<'a>(index: &'a Index, allow_builds: bool, allow: &'a [String]) -> Context<'a> {
        Context {
            index,
            abi: 148,
            allow_builds,
            allow_unresolved: allow,
        }
    }

    #[test]
    fn darwin_x64_is_not_win_x64() {
        assert_eq!(platform_token("@vscode/ripgrep-darwin-x64"), Some("darwin-x64"));
        assert_eq!(platform_token("@vscode/ripgrep-win32-x64"), Some("win32-x64"));
        assert_eq!(platform_token("sherpa-onnx-win-x64"), Some("win-x64"));
        assert_eq!(
            platform_token("@vscodium/os-proxy-resolver-win32-x64-msvc"),
            Some("win32-x64-msvc")
        );
        assert_eq!(platform_token("keytar"), None);
        assert_eq!(platform_token("darwinwin-x64x"), None);
    }

    #[test]
    fn every_seed_tier_is_resolved_from_the_built_in_index() {
        let index = Index::built_in().unwrap();
        let store = Store::Directory(PathBuf::from("/nonexistent"));
        let natives = vec![
            module("node_modules/keytar/build/Release/keytar.node", Some(("keytar", "7.9.0", "node_modules/keytar")), BinaryFormat::Pe, &[]),
            module("node_modules/fsevents/fsevents.node", Some(("fsevents", "2.3.3", "node_modules/fsevents")), BinaryFormat::MachO, &["darwin"]),
            module(
                "node_modules/@koromix/koffi-win32-x64/koffi.node",
                Some(("@koromix/koffi-win32-x64", "3.1.1", "node_modules/@koromix/koffi-win32-x64")),
                BinaryFormat::Pe,
                &[],
            ),
            module("node_modules/native-reg/build/Release/reg.node", Some(("native-reg", "1.1.1", "node_modules/native-reg")), BinaryFormat::Pe, &[]),
            module("node_modules/better-sqlite3/build/Release/better_sqlite3.node", Some(("better-sqlite3", "13.0.3", "node_modules/better-sqlite3")), BinaryFormat::Pe, &[]),
            module("registry.node", None, BinaryFormat::Pe, &[]),
            module(
                "node_modules/microtime/prebuilds/win32-x64/electron.napi.node",
                Some(("microtime", "3.1.1", "node_modules/microtime")),
                BinaryFormat::Pe,
                &[],
            ),
            module(
                "node_modules/microtime/prebuilds/linux-x64/electron.napi.node",
                Some(("microtime", "3.1.1", "node_modules/microtime")),
                BinaryFormat::Elf,
                &[],
            ),
        ];
        let finding = finding(natives);
        let plans = plan_modules(&finding, &store, &context(&index, false, &[]));
        let by = |dir: &str| {
            plans
                .iter()
                .find(|plan| plan.package_dir.as_deref() == Some(dir) || plan.files.iter().any(|f| f == dir))
                .unwrap_or_else(|| panic!("no plan for {dir}"))
        };
        assert_eq!(by("node_modules/keytar").resolution, Resolution::ReleasePrebuild);
        assert_eq!(by("node_modules/fsevents").resolution, Resolution::NotForLinux);
        let koffi = by("node_modules/@koromix/koffi-win32-x64");
        assert_eq!(koffi.resolution, Resolution::NpmPackage);
        assert_eq!(koffi.source.as_deref(), Some("@koromix/koffi-linux-x64@3.1.1"));
        assert_eq!(by("node_modules/native-reg").resolution, Resolution::NeedsBuild);
        assert_eq!(by("node_modules/native-reg").blocker(), Some(Code::BuildNotAllowed));
        assert_eq!(by("node_modules/better-sqlite3").resolution, Resolution::NpmPrebuild);
        assert_eq!(by("registry.node").resolution, Resolution::Unresolved);
        assert_eq!(by("registry.node").blocker(), Some(Code::ModuleUnresolved));
        assert_eq!(by("node_modules/microtime").resolution, Resolution::Carried);

        // With builds allowed, the source build is planned with what it needs.
        let allowed = plan_modules(&finding, &store, &context(&index, true, &[]));
        let reg = allowed
            .iter()
            .find(|plan| plan.package_dir.as_deref() == Some("node_modules/native-reg"))
            .unwrap();
        assert_eq!(reg.resolution, Resolution::SourceBuild);
        assert!(reg.downloads.iter().any(|d| d.what.starts_with("node-addon-api@")));

        // Accepting an unresolved module by its file name lifts the refusal.
        let accept = vec!["file:registry.node".to_owned()];
        let accepted = plan_modules(&finding, &store, &context(&index, false, &accept));
        let anonymous = accepted
            .iter()
            .find(|plan| plan.files.iter().any(|f| f == "registry.node"))
            .unwrap();
        assert!(anonymous.accepted);
        assert_eq!(anonymous.blocker(), None);
    }

    #[test]
    fn a_carried_build_is_copied_over_the_foreign_one_where_loaders_look() {
        let root = std::env::temp_dir().join(format!("tur-rehost-natives-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("node_modules/x/build/Release")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x/prebuilds/linux-x64")).unwrap();
        std::fs::write(root.join("node_modules/x/build/Release/x.node"), b"MZ foreign").unwrap();
        std::fs::write(root.join("node_modules/x/prebuilds/linux-x64/node.napi.node"), b"\x7fELF linux").unwrap();
        let index = Index::built_in().unwrap();
        let mut store = Store::Directory(root.clone());
        let mut plan = ModulePlan {
            identity: Some("x@1.0.0".to_owned()),
            package_dir: Some("node_modules/x".to_owned()),
            files: vec!["node_modules/x/build/Release/x.node".to_owned()],
            resolution: Resolution::Carried,
            detail: String::new(),
            source: Some("node_modules/x/prebuilds/linux-x64/node.napi.node".to_owned()),
            downloads: Vec::new(),
            accepted: false,
            applied: Vec::new(),
            helper: false,
        };
        apply(&mut plan, &mut store, &context(&index, false, &[]), &Vec::new(), &Vec::new()).unwrap();
        assert_eq!(
            std::fs::read(root.join("node_modules/x/build/Release/x.node")).unwrap(),
            b"\x7fELF linux"
        );
        assert_eq!(plan.applied, vec!["node_modules/x/build/Release/x.node".to_owned()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn node_pre_gyp_module_paths_are_filled_with_linux_values() {
        let package: Value = serde_json::from_str(
            r#"{"binary":{"module_name":"node_sqlite3","module_path":"./lib/binding/napi-v{napi_build_version}-{platform}-{libc}-{arch}"}}"#,
        )
        .unwrap();
        let plan = ModulePlan {
            identity: Some("sqlite3@5.1.6".to_owned()),
            package_dir: Some("node_modules/sqlite3".to_owned()),
            files: Vec::new(),
            resolution: Resolution::ReleasePrebuild,
            detail: String::new(),
            source: None,
            downloads: vec![Download {
                what: "sqlite3".to_owned(),
                artifact: Artifact {
                    url: "https://github.com/TryGhost/node-sqlite3/releases/download/v5.1.6/napi-v6-linux-glibc-x64.tar.gz".to_owned(),
                    sha256: "0".repeat(64),
                    size: 1,
                    hash_source: "test".to_owned(),
                    npm_integrity: None,
                },
                licence: None,
            }],
            accepted: false,
            applied: Vec::new(),
            helper: false,
        };
        assert_eq!(
            module_path(Some(&package), &plan).as_deref(),
            Some("./lib/binding/napi-v6-linux-glibc-x64")
        );
        assert_eq!(
            normalise("node_modules/sqlite3/./lib/binding/napi-v6-linux-glibc-x64/node_sqlite3.node"),
            "node_modules/sqlite3/lib/binding/napi-v6-linux-glibc-x64/node_sqlite3.node"
        );
    }

    #[test]
    fn rank_prefers_this_abi_and_refuses_other_processors() {
        assert!(rank("prebuilds/linux-x64/electron.abi148.node", 148) < rank("prebuilds/linux-x64/node.napi.node", 148));
        assert_eq!(rank("prebuilds/linux-arm64/node.napi.node", 148), 99);
        assert_eq!(rank("prebuilds/linux-x64/node.abi115.node", 148), 50);
    }
}
