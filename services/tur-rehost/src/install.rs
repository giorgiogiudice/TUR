// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Planning and carrying out a re-host: open the package, detect, plan
//! what will be fetched, then (on `install`) fetch, lay out, swap native
//! modules, record.
//!
//! Every refusal happens in the plan, before anything is fetched or
//! written outside the scratch directory: a package that is native code, a
//! runtime family not built yet, a runtime version TUR has not pinned, a
//! native module with no Linux build TUR can supply, a build the person
//! has not allowed. The installation is staged in `staging/<app-id>/` and
//! renamed into `apps/<app-id>/` only when complete; a failure removes
//! the staging directory (`TUR-REHOST-APP-003`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::codes::{Code, Failure};
use crate::detect;
use crate::fetch::{self, Fetched};
use crate::finding::{Family, Finding, PayloadKind, Platform};
use crate::index::{Artifact, ElectronRelease, Index};
use crate::natives::{self, Context, Extracted, ModulePlan, Resolution, Store};
use crate::paths::{AppId, AppPaths, Roots, safe_relative};
use crate::record::{
    AppRecord, FetchRecord, ModuleRecord, PackageRecord, RuntimeRecord, SCHEMA_VERSION,
    desktop_entry, now_utc,
};
use crate::runner::Runner;
use crate::sandbox::{tool_arguments, tool_environment};
use crate::unpack::{self, Tools};

/// What the person is told about the runtime before anything is fetched
/// (docs/architecture/rehost.md, section 5).
pub const CODEC_NOTICE_UPSTREAM: &str = "TUR downloads Electron's own Linux build from the Electron project's releases on GitHub, unchanged, and checks it against the SHA-256 TUR pinned. It is not part of TUR. It contains media codecs (H.264 and AAC) that TUR does not ship; the download is made by this computer, at your request, as with Proton.";

/// The notice when the open-codecs build is chosen.
pub const CODEC_NOTICE_OPEN: &str = "TUR downloads Electron's own Linux build from the Electron project's releases on GitHub and replaces its media library with Electron's open-codecs build of the same version (also checked against TUR's pin): H.264 and AAC video and sound will not play inside this application.";

/// Which media library the Electron runtime keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Codecs {
    /// Electron's Linux release unchanged.
    Upstream,
    /// `libffmpeg.so` replaced by Electron's open-codecs build.
    OpenOnly,
}

impl Codecs {
    /// As written in records and on the command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            Codecs::Upstream => "upstream",
            Codecs::OpenOnly => "open-only",
        }
    }
}

/// The person's choices for one re-host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The identifier; `local.<file name>` when not given.
    pub app_id: Option<AppId>,
    /// The name shown; the payload's own when not given.
    pub name: Option<String>,
    /// Which media library to keep.
    pub codecs: Codecs,
    /// Whether native modules may be built on this computer.
    pub allow_builds: bool,
    /// Modules the person accepts running without (`name@version`,
    /// `file:<name>`).
    pub allow_unresolved: Vec<String>,
    /// The program the launcher entry starts (`tur-rehost`).
    pub launcher: String,
}

/// The programs and places an operation uses.
pub struct Environment<'a> {
    /// The roots.
    pub roots: &'a Roots,
    /// TUR's index.
    pub index: &'a Index,
    /// The archive tools and the sandbox.
    pub tools: &'a Tools,
    /// curl.
    pub curl: PathBuf,
    /// podman, for source builds.
    pub podman: Option<PathBuf>,
    /// What starts programs.
    pub runner: &'a mut dyn Runner,
}

/// A file the plan will fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedDownload {
    /// What it is.
    pub what: String,
    /// Where from.
    pub url: String,
    /// Its size.
    pub size: u64,
    /// Its pinned SHA-256.
    pub sha256: String,
    /// Its licence, when known.
    pub licence: Option<String>,
}

/// The runtime the plan lays the payload under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimePlan {
    /// `electron`.
    pub family: String,
    /// The payload's exact version.
    pub version: String,
    /// The Node ABI.
    pub abi: u32,
    /// Which media library is kept.
    pub codecs: Codecs,
    /// What the person is told.
    pub notice: String,
}

/// A plan: what will happen, before anything happens.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    /// The package.
    pub package: String,
    /// The application identifier.
    pub app_id: String,
    /// The outermost container.
    pub container: String,
    /// What was opened.
    pub layers: Vec<String>,
    /// Containers that could not be opened.
    pub problems: Vec<String>,
    /// The detector's most significant finding.
    pub finding: Option<Finding>,
    /// How many other findings there were.
    pub other_findings: usize,
    /// The runtime, when the package can be re-hosted.
    pub runtime: Option<RuntimePlan>,
    /// Native modules and helper packages.
    pub modules: Vec<ModulePlan>,
    /// Everything that will be fetched.
    pub downloads: Vec<PlannedDownload>,
    /// Their total size in bytes.
    pub download_bytes: u64,
    /// Whether the build environment is needed (and, if it is not there
    /// yet, built once from Fedora's packages).
    pub needs_build_environment: bool,
    /// What stands in the way, with codes. Empty when the plan can run.
    pub blockers: Vec<Failure>,
    /// Whether the plan can run.
    pub ready: bool,
}

impl Plan {
    /// The exit status of a plan: 0 when ready, otherwise that of its first
    /// blocker.
    pub fn exit_status(&self) -> u8 {
        self.blockers
            .first()
            .map_or(crate::codes::EXIT_OK, |failure| failure.code.exit_status())
    }
}

/// What `install` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// The plan that was carried out.
    pub plan: Plan,
    /// The record written.
    pub record: AppRecord,
    /// Where the application lives.
    pub directory: String,
}

/// A plan with what is needed to carry it out.
pub struct Prepared {
    /// The plan.
    pub plan: Plan,
    work: PathBuf,
    release: Option<ElectronRelease>,
}

impl Prepared {
    /// Removes the scratch directory.
    pub fn discard(self) {
        let _ = std::fs::remove_dir_all(&self.work);
    }
}

fn refuse(plan: &mut Plan, code: Code, message: impl Into<String>) {
    plan.blockers.push(Failure::new(code, message));
}

/// Opens, detects and plans. Nothing is fetched.
pub fn prepare(
    package: &Path,
    options: &Options,
    environment: &mut Environment<'_>,
) -> Result<Prepared, Failure> {
    let file_name = package
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "package".to_owned());
    let app_id = options
        .app_id
        .clone()
        .unwrap_or_else(|| AppId::from_name(&file_name));
    let work = environment.roots.work(app_id.as_str());
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|error| {
        Failure::new(Code::StepFailed, format!("cannot create {}: {error}", work.display()))
    })?;
    let opened = match unpack::open(package, &work, environment.tools, environment.runner) {
        Ok(opened) => opened,
        Err(failure) => {
            let _ = std::fs::remove_dir_all(&work);
            return Err(failure);
        }
    };
    let findings = detect::detect(&opened.root);
    let mut plan = Plan {
        package: package.display().to_string(),
        app_id: app_id.as_str().to_owned(),
        container: opened.container.clone(),
        layers: opened.layers.clone(),
        problems: opened.problems.clone(),
        finding: findings.first().cloned(),
        other_findings: findings.len().saturating_sub(1),
        runtime: None,
        modules: Vec::new(),
        downloads: Vec::new(),
        download_bytes: 0,
        needs_build_environment: false,
        blockers: Vec::new(),
        ready: false,
    };
    let release = plan_finding(&mut plan, options, environment.index);
    plan.ready = plan.blockers.is_empty();
    Ok(Prepared {
        plan,
        work,
        release,
    })
}

fn plan_finding(plan: &mut Plan, options: &Options, index: &Index) -> Option<ElectronRelease> {
    let Some(finding) = plan.finding.clone() else {
        let code = if plan.problems.is_empty() { Code::NothingRecognised } else { Code::ContainerNotOpened };
        refuse(plan, code, "no portable runtime was recognised in this package");
        return None;
    };
    match finding.family {
        Family::Native | Family::DotnetFramework => {
            let route = match finding.platform {
                Platform::Windows => "it may run through Wine",
                Platform::Macos => "applications written for the Mac alone cannot run on TUR",
                _ => "it is not a re-hosting case",
            };
            refuse(
                plan,
                Code::NativeProgram,
                format!(
                    "this is native code ({}): nothing portable to lay under another runtime; {route}",
                    finding.toolkit.clone().unwrap_or_else(|| finding.family.label().to_owned())
                ),
            );
            return None;
        }
        Family::Electron => {}
        other => {
            refuse(
                plan,
                Code::FamilyNotBuilt,
                format!(
                    "{} was recognised ({}); re-hosting it is measured (docs/reports/exp-rehost-1.md, section 5) but not built yet: only Electron is",
                    other.label(),
                    finding.summary()
                ),
            );
            return None;
        }
    }
    if finding.platform == Platform::Linux {
        refuse(plan, Code::AlreadyLinux, "this package is already built for Linux: use it as it is");
        return None;
    }
    let Some(payload) = finding.electron.clone() else {
        refuse(plan, Code::NothingRecognised, "the Electron payload was not found");
        return None;
    };
    let Some(version) = finding.version.clone() else {
        refuse(plan, Code::VersionUnknown, "the Electron version could not be read from the package");
        return None;
    };
    if version.contains('+') {
        refuse(
            plan,
            Code::VendorBuild,
            format!("Electron {version} is a vendor's build (for example castlabs' with Widevine): the stock Linux build lacks what the vendor added"),
        );
        return None;
    }
    let Some(release) = index.electron(&version).cloned() else {
        refuse(
            plan,
            Code::RuntimeNotPinned,
            format!("Electron {version} is not in TUR's pinned index ({} versions, checked {}): it is not fetched", index.electron.len(), index.checked),
        );
        return None;
    };
    let notice = match options.codecs {
        Codecs::Upstream => CODEC_NOTICE_UPSTREAM,
        Codecs::OpenOnly => CODEC_NOTICE_OPEN,
    };
    plan.runtime = Some(RuntimePlan {
        family: "electron".to_owned(),
        version: version.clone(),
        abi: release.abi,
        codecs: options.codecs,
        notice: notice.to_owned(),
    });
    push_download(plan, &format!("Electron {version} for Linux (MIT, with Chromium's and Node.js's licences)"), &release.linux_x64, Some("MIT"));
    if options.codecs == Codecs::OpenOnly {
        push_download(plan, &format!("Electron {version}'s open-codecs media library"), &release.ffmpeg_open_codecs, Some("LGPL-2.1-or-later"));
    }
    let store = match Store::open(&payload.modules_root, payload.kind) {
        Ok(store) => store,
        Err(failure) => {
            plan.blockers.push(failure);
            return None;
        }
    };
    let context = Context {
        index,
        abi: release.abi,
        allow_builds: options.allow_builds,
        allow_unresolved: &options.allow_unresolved,
    };
    let mut modules = natives::plan_modules(&finding, &store, &context);
    modules.extend(natives::plan_helpers(&finding, &store, &context));
    let mut builds = false;
    for module in &modules {
        if module.resolution == Resolution::SourceBuild {
            builds = true;
        }
        for download in &module.downloads {
            push_download(plan, &download.what, &download.artifact, download.licence.as_deref());
        }
        if let Some(code) = module.blocker() {
            let name = module
                .identity
                .clone()
                .or_else(|| module.files.first().cloned())
                .unwrap_or_default();
            refuse(plan, code, format!("{name}: {}", module.detail));
        }
    }
    if builds {
        push_download(plan, &format!("Electron {version}'s headers, to build native modules against"), &release.headers, Some("MIT"));
        plan.needs_build_environment = true;
    }
    plan.modules = modules;
    Some(release)
}

fn push_download(plan: &mut Plan, what: &str, artifact: &Artifact, licence: Option<&str>) {
    if plan.downloads.iter().any(|known| known.sha256 == artifact.sha256) {
        return;
    }
    plan.download_bytes += artifact.size;
    plan.downloads.push(PlannedDownload {
        what: what.to_owned(),
        url: artifact.url.clone(),
        size: artifact.size,
        sha256: artifact.sha256.clone(),
        licence: licence.map(str::to_owned),
    });
}

/// Files of the Windows package's program directory that belong to
/// Electron or to the installer, not to the application: not copied.
fn is_runtime_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let suffixes = [".exe", ".dll", ".pak", ".bin", ".dat", ".ico", ".html", ".lib", ".pdb"];
    let exact = ["license", "licenses", "version", "vk_swiftshader_icd.json", "chrome-sandbox", "chrome_crashpad_handler"];
    if suffixes.iter().any(|suffix| lower.ends_with(suffix)) || exact.contains(&lower.as_str()) {
        return true;
    }
    if lower.starts_with("squirrel") || lower.starts_with("uninstall") {
        return true;
    }
    // libfoo.so, libfoo.so.1, libfoo.so.1.2
    if let Some(at) = lower.find(".so") {
        let rest = &lower[at + 3..];
        return rest.split('.').all(|part| part.chars().all(|c| c.is_ascii_digit()));
    }
    false
}

/// Directories beside a Windows program that belong to Electron or the
/// installer.
const RUNTIME_DIRECTORIES: [&str; 6] = ["locales", "resources", "swiftshader", "$PLUGINSDIR", "$TEMP", "$R0"];

/// A program name made safe for a file name.
pub fn program_name(program: Option<&str>, name: Option<&str>) -> String {
    let stem = program
        .map(|program| program.strip_suffix(".exe").or_else(|| program.strip_suffix(".EXE")).unwrap_or(program))
        .or(name)
        .unwrap_or("app");
    let mut out = String::new();
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
            out.push(character);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches(|c| c == '-' || c == '.').to_owned();
    if out.is_empty() || out.eq_ignore_ascii_case("electron") || out.len() > 128 {
        "app".to_owned()
    } else {
        out
    }
}

/// Copies a tree without following links. A link whose target is relative
/// and does not climb out is kept as a link; any other link is skipped.
pub fn copy_tree(from: &Path, to: &Path, depth: usize) -> std::io::Result<()> {
    if depth > 64 {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(from)?;
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(from)?;
        let safe = target.is_relative()
            && !target
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir));
        if safe {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::os::unix::fs::symlink(target, to)?;
        }
        return Ok(());
    }
    if metadata.is_dir() {
        std::fs::create_dir_all(to)?;
        let mut entries: Vec<PathBuf> = std::fs::read_dir(from)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for entry in entries {
            if let Some(name) = entry.file_name() {
                copy_tree(&entry, &to.join(name), depth + 1)?;
            }
        }
        return Ok(());
    }
    if metadata.is_file() {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to)?;
    }
    Ok(())
}

/// Carries out a prepared plan. On failure everything staged is removed.
pub fn install(
    prepared: Prepared,
    options: &Options,
    environment: &mut Environment<'_>,
) -> Result<Report, Failure> {
    let Prepared {
        mut plan,
        work,
        release,
    } = prepared;
    let result = attempt(&mut plan, release.as_ref(), options, environment, &work);
    let _ = std::fs::remove_dir_all(&work);
    match result {
        Ok((record, directory)) => Ok(Report {
            plan,
            record,
            directory,
        }),
        Err(failure) => {
            if let Ok(id) = AppId::parse(&plan.app_id)
                && failure.code != Code::AlreadyInstalled
            {
                let _ = std::fs::remove_dir_all(environment.roots.staging(&id));
                let _ = std::fs::remove_dir_all(environment.roots.launcher(&id));
            }
            Err(failure)
        }
    }
}

fn attempt(
    plan: &mut Plan,
    release: Option<&ElectronRelease>,
    options: &Options,
    environment: &mut Environment<'_>,
    work: &Path,
) -> Result<(AppRecord, String), Failure> {
    if let Some(first) = plan.blockers.first() {
        return Err(first.clone());
    }
    let Some(release) = release else {
        return Err(Failure::new(Code::NothingRecognised, "nothing to re-host"));
    };
    let id = AppId::parse(&plan.app_id)?;
    if environment.roots.app(&id).exists() {
        return Err(Failure::new(
            Code::AlreadyInstalled,
            format!("{id} is already re-hosted: remove it first, or give another identifier"),
        ));
    }
    carry_out(plan, release, &id, options, environment, work)
}

fn step(error: std::io::Error, what: &str) -> Failure {
    Failure::new(Code::StepFailed, format!("{what}: {error}"))
}

/// Extracts `archive` into `output` with bsdtar in the tool sandbox.
fn extract(
    environment: &mut Environment<'_>,
    archive: &Path,
    output: &Path,
    strip_components: Option<u32>,
) -> Result<(), Failure> {
    std::fs::create_dir_all(output).map_err(|error| step(error, "cannot create a directory"))?;
    let (program, tool_args) = unpack::extract_command(environment.tools, archive, output, strip_components)?;
    let archive_text = archive
        .to_str()
        .ok_or_else(|| Failure::new(Code::InputInvalid, "a path is not valid UTF-8"))?;
    let output_text = output
        .to_str()
        .ok_or_else(|| Failure::new(Code::InputInvalid, "a path is not valid UTF-8"))?;
    let program_text = program
        .to_str()
        .ok_or_else(|| Failure::new(Code::InputInvalid, "a path is not valid UTF-8"))?;
    let mut args = tool_arguments(&environment.tools.root, &[archive_text], output_text);
    args.push(program_text.to_owned());
    args.extend(tool_args);
    let invocation = crate::runner::Invocation {
        purpose: format!("unpack {}", archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        program: environment.tools.bwrap.clone(),
        args,
        env: tool_environment(),
        cwd: None,
        timeout_seconds: unpack::TOOL_SECONDS,
    };
    let outcome = environment.runner.run(&invocation).map_err(|error| {
        Failure::new(Code::SandboxMissing, format!("the sandbox cannot be started: {error}"))
    })?;
    if outcome.succeeded() {
        Ok(())
    } else {
        Err(Failure::new(
            Code::StepFailed,
            format!("{} could not be unpacked: {}", archive.display(), outcome.last_line()),
        ))
    }
}

/// Every regular file below `root`, relative, `/`-separated, read into
/// memory (at most 512 MiB in all, 20 000 files).
fn read_tree(root: &Path) -> Result<Extracted, Failure> {
    let mut out = Vec::new();
    let mut total = 0u64;
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = stack.pop() {
        let listing = std::fs::read_dir(&directory).map_err(|error| step(error, "cannot read an unpacked archive"))?;
        for entry in listing.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.is_dir() {
                stack.push((path, relative));
            } else if metadata.is_file() {
                total += metadata.len();
                if total > 512 << 20 || out.len() > 20_000 {
                    return Err(Failure::new(Code::StepFailed, "an archive is larger than TUR accepts for a module"));
                }
                let data = std::fs::read(&path).map_err(|error| step(error, "cannot read an unpacked file"))?;
                out.push((relative, data));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn carry_out(
    plan: &mut Plan,
    release: &ElectronRelease,
    id: &AppId,
    options: &Options,
    environment: &mut Environment<'_>,
    work: &Path,
) -> Result<(AppRecord, String), Failure> {
    let finding = plan
        .finding
        .clone()
        .ok_or_else(|| Failure::new(Code::NothingRecognised, "nothing to re-host"))?;
    let payload = finding
        .electron
        .clone()
        .ok_or_else(|| Failure::new(Code::NothingRecognised, "the Electron payload was not found"))?;
    let roots = environment.roots;
    let staging = roots.staging(id);
    let _ = std::fs::remove_dir_all(&staging);
    let paths = AppPaths::new(staging.clone());
    std::fs::create_dir_all(&paths.app).map_err(|error| step(error, "cannot create the application's directory"))?;
    std::fs::create_dir_all(&paths.home).map_err(|error| step(error, "cannot create the application's home"))?;
    let downloads = roots.downloads();
    let mut fetched: Vec<Fetched> = Vec::new();
    let curl = environment.curl.clone();

    // 1. The runtime, at the payload's exact version.
    let (zip, record) = fetch::fetch(
        environment.runner,
        &curl,
        &downloads,
        &release.linux_x64,
        &format!("Electron {} for Linux", finding.version.clone().unwrap_or_default()),
        Some("MIT"),
    )?;
    fetched.push(record);
    extract(environment, &zip, &paths.app, None)?;
    let mut program = program_name(payload.program.as_deref(), payload.name.as_deref());
    if paths.app.join(&program).exists() {
        // A name Electron's own files already use (resources, locales).
        program = "app".to_owned();
    }
    let electron = paths.app.join("electron");
    let program_path = paths.app.join(&program);
    std::fs::rename(&electron, &program_path).map_err(|error| step(error, "the Electron archive has no program 'electron'"))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| step(error, "cannot make the program executable"))?;
    }
    let _ = std::fs::remove_file(paths.app.join("resources").join("default_app.asar"));

    // 2. The media library, when the person chose open codecs only.
    if options.codecs == Codecs::OpenOnly {
        let (archive, record) = fetch::fetch(
            environment.runner,
            &curl,
            &downloads,
            &release.ffmpeg_open_codecs,
            "Electron's open-codecs media library",
            Some("LGPL-2.1-or-later"),
        )?;
        fetched.push(record);
        let unpacked = work.join("ffmpeg");
        extract(environment, &archive, &unpacked, None)?;
        std::fs::copy(unpacked.join("libffmpeg.so"), paths.app.join("libffmpeg.so"))
            .map_err(|error| step(error, "cannot replace libffmpeg.so"))?;
    }

    // 3. The payload, and on Windows the application's files beside the
    //    program. The updater's configuration stays behind: the
    //    application's own updater is off (TUR re-hosts a new version).
    let resources = paths.app.join("resources");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&payload.resources)
        .map_err(|error| step(error, "cannot read the package's resources"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    names.sort();
    for source in names {
        let Some(name) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if name.ends_with(".lproj")
            || name.ends_with(".icns")
            || matches!(name.as_str(), "default_app.asar" | "electron.asar" | "app-update.yml")
        {
            continue;
        }
        copy_tree(&source, &resources.join(&name), 0).map_err(|error| step(error, "cannot copy the payload"))?;
    }
    if finding.platform == Platform::Windows {
        let mut beside: Vec<PathBuf> = std::fs::read_dir(&payload.app_dir)
            .map_err(|error| step(error, "cannot read the program's directory"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        beside.sort();
        for source in beside {
            let Some(name) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let is_file = std::fs::symlink_metadata(&source).is_ok_and(|m| m.is_file());
            if RUNTIME_DIRECTORIES.contains(&name.as_str()) || (is_file && is_runtime_file(&name)) {
                continue;
            }
            let target = paths.app.join(&name);
            if target.exists() {
                continue;
            }
            copy_tree(&source, &target, 0).map_err(|error| step(error, "cannot copy the application's files"))?;
        }
    }

    // 4. Fuses: the publisher's, on the Linux binary; the payload check
    //    off (its hash lives in the publisher's binary), TUR's own hash
    //    checked at each start instead.
    let publisher = payload.fuses.clone().unwrap_or_default();
    let mut wanted: BTreeMap<String, String> = publisher
        .iter()
        .filter(|(_, state)| matches!(state.as_str(), "0" | "1"))
        .map(|(name, state)| (name.clone(), state.clone()))
        .collect();
    wanted.insert("EnableEmbeddedAsarIntegrityValidation".to_owned(), "0".to_owned());
    if payload.kind == PayloadKind::Directory {
        wanted.insert("OnlyLoadAppFromAsar".to_owned(), "0".to_owned());
    }
    let fuses_set = crate::scan::write_fuses(&program_path, &wanted)
        .map_err(|error| step(error, "cannot set the fuses of the Linux binary"))?;

    // 5. Native modules and helper packages.
    let modules_relative = payload
        .modules_root
        .strip_prefix(&payload.resources)
        .map_err(|_| Failure::new(Code::StepFailed, "the payload is not inside the resources directory"))?
        .to_path_buf();
    let modules_root = resources.join(&modules_relative);
    let mut store = Store::open(&modules_root, payload.kind)?;
    let context = Context {
        index: environment.index,
        abi: release.abi,
        allow_builds: options.allow_builds,
        allow_unresolved: &options.allow_unresolved,
    };
    let mut modules = std::mem::take(&mut plan.modules);
    let mut environment_ready = false;
    let mut headers_dir: Option<PathBuf> = None;
    for (number, module) in modules.iter_mut().enumerate() {
        if module.resolution.leaves_it_foreign() || module.resolution == Resolution::NotForLinux {
            continue;
        }
        let mut extracted: Extracted = Vec::new();
        let mut built: Extracted = Vec::new();
        let scratch = work.join(format!("module-{number}"));
        if module.resolution == Resolution::SourceBuild {
            let podman = environment
                .podman
                .clone()
                .ok_or_else(|| Failure::new(Code::ProgramMissing, "podman is needed to build native modules and is not installed"))?;
            if !environment_ready {
                crate::build::ensure_environment(environment.runner, &podman, &work.join("environment"))?;
                environment_ready = true;
            }
            if headers_dir.is_none() {
                let (archive, record) = fetch::fetch(
                    environment.runner,
                    &curl,
                    &downloads,
                    &release.headers,
                    "Electron's headers",
                    Some("MIT"),
                )?;
                fetched.push(record);
                let unpacked = work.join("headers");
                extract(environment, &archive, &unpacked, None)?;
                headers_dir = Some(unpacked.join("node_headers"));
            }
            let source = scratch.join("src");
            let mut first = true;
            for download in module.downloads.clone() {
                let (archive, record) = fetch::fetch(environment.runner, &curl, &downloads, &download.artifact, &download.what, download.licence.as_deref())?;
                fetched.push(record);
                if first {
                    extract(environment, &archive, &source, Some(1))?;
                    first = false;
                } else {
                    let package = download.what.split(' ').next().unwrap_or_default().to_owned();
                    let Some((name, _)) = crate::index::split_identity(&package) else {
                        continue;
                    };
                    let target = safe_relative(name).map(|relative| source.join("node_modules").join(relative));
                    if let Some(target) = target {
                        extract(environment, &archive, &target, Some(1))?;
                    }
                }
            }
            let identity = module.identity.clone().unwrap_or_default();
            let headers = headers_dir.clone().unwrap_or_default();
            let (files, _) = crate::build::run_build(environment.runner, &podman, &identity, &source, &headers)?;
            built = files;
        } else {
            for download in module.downloads.clone() {
                let (archive, record) = fetch::fetch(environment.runner, &curl, &downloads, &download.artifact, &download.what, download.licence.as_deref())?;
                fetched.push(record);
                let is_npm = download.artifact.url.starts_with("https://registry.npmjs.org/");
                let unpacked = scratch.join("unpacked");
                extract(environment, &archive, &unpacked, is_npm.then_some(1))?;
                extracted = read_tree(&unpacked)?;
            }
        }
        natives::apply(module, &mut store, &context, &extracted, &built)?;
        let _ = std::fs::remove_dir_all(&scratch);
    }
    store.save()?;
    plan.modules = modules;

    // 6. Helper programs the system provides (ffmpeg, ffprobe) replace the
    //    package's Windows or macOS builds.
    let mut system_helpers = Vec::new();
    for sidecar in finding.sidecars.iter().filter(|s| s.kind.format != crate::finding::BinaryFormat::Elf) {
        let base = sidecar.path.rsplit('/').next().unwrap_or(&sidecar.path).to_owned();
        let stem = base.strip_suffix(".exe").unwrap_or(&base).to_ascii_lowercase();
        if !matches!(stem.as_str(), "ffmpeg" | "ffprobe") {
            continue;
        }
        let system = Path::new("/usr/bin").join(&stem);
        if !system.is_file() {
            continue;
        }
        let Some(relative) = safe_relative(&sidecar.path) else {
            continue;
        };
        let target = resources.join(&relative);
        let Some(directory) = target.parent() else {
            continue;
        };
        for name in [stem.as_str(), base.as_str()] {
            let link = directory.join(name);
            if std::fs::symlink_metadata(&link).is_ok() {
                let _ = std::fs::remove_file(&link);
            }
            std::os::unix::fs::symlink(&system, &link).map_err(|error| step(error, "cannot link a system helper"))?;
        }
        system_helpers.push(format!("{} -> {}", sidecar.path, system.display()));
    }

    // 7. TUR's own hash of the payload, checked at every start.
    let payload_relative = PathBuf::from("resources").join(&modules_relative);
    let payload_sha256 = if payload.kind == PayloadKind::Asar {
        let main = resources.join("app.asar");
        let hashed = if main.is_file() { main } else { modules_root.clone() };
        Some(fetch::hash_file(&hashed).map_err(|error| step(error, "cannot hash the payload"))?.0)
    } else {
        None
    };
    let package_path = Path::new(&plan.package);
    let (package_sha256, package_size) = if package_path.is_file() {
        fetch::hash_file(package_path).map_err(|error| step(error, "cannot hash the package"))?
    } else {
        (String::new(), 0)
    };

    // 8. The launcher entry, staged.
    let name = options
        .name
        .clone()
        .or_else(|| payload.name.clone())
        .unwrap_or_else(|| id.as_str().to_owned());
    let launcher_dir = roots.launcher(id);
    std::fs::create_dir_all(&launcher_dir).map_err(|error| step(error, "cannot create the launcher directory"))?;
    let entry_path = launcher_dir.join(format!("tur-rehost-{id}.desktop"));
    std::fs::write(&entry_path, desktop_entry(id, &name, finding.platform.as_str(), &options.launcher))
        .map_err(|error| step(error, "cannot write the launcher entry"))?;

    // 9. The record, last; then the directory moves into place.
    let record = AppRecord {
        schema_version: SCHEMA_VERSION,
        app_id: id.as_str().to_owned(),
        name,
        app_version: payload.app_version.clone(),
        runtime: RuntimeRecord {
            family: "electron".to_owned(),
            version: finding.version.clone().unwrap_or_default(),
            url: release.linux_x64.url.clone(),
            sha256: release.linux_x64.sha256.clone(),
            codecs: options.codecs.as_str().to_owned(),
        },
        package: PackageRecord {
            file_name: package_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: package_size,
            sha256: package_sha256,
            platform: finding.platform.as_str().to_owned(),
            layers: plan.layers.clone(),
        },
        program: program.clone(),
        payload: payload_relative.to_string_lossy().into_owned(),
        payload_sha256,
        fuses_publisher: publisher,
        fuses_set,
        modules: plan
            .modules
            .iter()
            .map(|module| ModuleRecord {
                module: module.names().into_iter().next().unwrap_or_default(),
                resolution: serde_json::to_value(module.resolution)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                applied: module.applied.clone(),
            })
            .collect(),
        system_helpers,
        fetched: fetched
            .iter()
            .map(|item| FetchRecord {
                what: item.what.clone(),
                url: item.url.clone(),
                sha256: item.sha256.clone(),
                size: item.size,
                licence: item.licence.clone(),
            })
            .collect(),
        updater: "off".to_owned(),
        installed_at: now_utc(),
        desktop_entry: entry_path.to_string_lossy().into_owned(),
    };
    let text = serde_json::to_string_pretty(&record)
        .map_err(|error| Failure::new(Code::StepFailed, format!("cannot write the record: {error}")))?;
    std::fs::write(&paths.record, text + "\n").map_err(|error| step(error, "cannot write the record"))?;
    let destination = roots.app(id);
    std::fs::create_dir_all(roots.apps()).map_err(|error| step(error, "cannot create the applications directory"))?;
    std::fs::rename(&staging, &destination).map_err(|error| step(error, "cannot move the application into place"))?;
    Ok((record, destination.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn electron_s_own_files_beside_the_program_are_not_copied() {
        for name in ["Joplin.exe", "ffmpeg.dll", "resources.pak", "icudtl.dat", "v8_context_snapshot.bin", "LICENSE", "LICENSES.chromium.html", "version", "vk_swiftshader_icd.json", "Squirrel.exe", "Uninstall Joplin.exe", "libEGL.so", "libvulkan.so.1"] {
            assert!(is_runtime_file(name), "{name}");
        }
        for name in ["notes.txt", "config.json", "resources.solib-data"] {
            assert!(!is_runtime_file(name), "{name}");
        }
    }

    #[test]
    fn the_program_is_named_after_the_publisher_s() {
        assert_eq!(program_name(Some("Joplin.exe"), None), "Joplin");
        assert_eq!(program_name(Some("Standard Notes.exe"), None), "Standard-Notes");
        assert_eq!(program_name(Some("electron.exe"), Some("x")), "app");
        assert_eq!(program_name(None, Some("draw.io")), "draw.io");
        assert_eq!(program_name(Some("../../evil"), None), "evil");
        assert_eq!(program_name(None, None), "app");
    }

    #[test]
    fn a_tree_is_copied_without_following_links_out() {
        let base = std::env::temp_dir().join(format!("tur-rehost-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let from = base.join("from");
        std::fs::create_dir_all(from.join("sub")).unwrap();
        std::fs::write(from.join("sub/file.txt"), b"x").unwrap();
        std::os::unix::fs::symlink("file.txt", from.join("sub/inside")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", from.join("sub/outside")).unwrap();
        std::os::unix::fs::symlink("../../escape", from.join("sub/climb")).unwrap();
        let to = base.join("to");
        copy_tree(&from, &to, 0).unwrap();
        assert_eq!(std::fs::read(to.join("sub/file.txt")).unwrap(), b"x");
        assert!(std::fs::symlink_metadata(to.join("sub/inside")).unwrap().file_type().is_symlink());
        assert!(std::fs::symlink_metadata(to.join("sub/outside")).is_err());
        assert!(std::fs::symlink_metadata(to.join("sub/climb")).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
