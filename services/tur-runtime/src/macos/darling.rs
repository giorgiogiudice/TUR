// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! TUR's build of Darling on this computer.
//!
//! Darling is not in the system image. TUR's CI builds it from source, at a
//! pinned commit with TUR's patch queue applied, on the image's own pinned
//! base (`third-party/darling/`), and publishes the result to the
//! repository's package registry as one archive. The manifest compiled into
//! this program ([`MANIFEST`], `services/tur-runtime/darling/runtime.json`)
//! names that archive by its SHA-256 and size. It is fetched when a person
//! first installs a macOS application, checked against the manifest before
//! anything in it is used, and unpacked into the runtime manager's own data:
//!
//! ```text
//! $XDG_DATA_HOME/tur/runtime/darling/<artifact-id>/tree/opt/tur-darling/   the build
//! $XDG_DATA_HOME/tur/runtime/darling/<artifact-id>/verified.json           written last
//! $XDG_DATA_HOME/tur/runtime/darling/staging/<artifact-id>/                while fetching
//! ```
//!
//! The tree is shown to an application's sandbox read-only, at
//! [`crate::sandbox::RUNTIME_TREE_PATH`], the path it was built for. It runs
//! without set-user-ID and without a kernel module ([`DarlingMode`]).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::MacError;
use crate::arch::Arch;
use crate::codes::Code;
use crate::plan::{Check, CommandSpec, Operation, Plan, Step};
use crate::paths::{RuntimeDirs, plain_text};
use crate::runtime_cmd::RUNTIME_PATH;
use crate::sandbox::RUNTIME_TREE_PATH;
use crate::timefmt::Timestamp;

/// The manifest of TUR's Darling builds, compiled into this program.
pub const MANIFEST: &str = include_str!("../../darling/runtime.json");

/// The version of the manifest's format.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The version of `verified.json`.
pub const VERIFIED_SCHEMA_VERSION: u32 = 1;

/// The largest archive a manifest may name: 4 GiB.
pub const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// How long a download may take, in seconds.
pub const DOWNLOAD_SECONDS: u64 = 3600;

/// How long unpacking may take, in seconds.
pub const UNPACK_SECONDS: u64 = 1800;

/// The largest answer of the registry's token service that is read.
pub const MAX_TOKEN_RESPONSE_BYTES: usize = 64 * 1024;

/// The largest `verified.json` that is read.
const MAX_VERIFIED_BYTES: u64 = 64 * 1024;

/// How Darling runs inside the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DarlingMode {
    /// `DARLING_NONROOT=1`: no namespaces of Darling's own; the prefix is a
    /// copy of Darling's system, and the guest's `/proc` is the sandbox's.
    /// Fits the sandbox every runtime gets, which forbids nested user
    /// namespaces. The default.
    Nonroot,
    /// `DARLING_ROOTLESS=1`: Darling makes a user, mount and PID namespace
    /// of its own and overlays the prefix on its system. Needs nested user
    /// namespaces, and mounting `/proc` inside bubblewrap's sandbox is
    /// expected to be refused by the kernel; offered to be measured.
    Rootless,
}

impl DarlingMode {
    /// The mode's name, as the command line takes it.
    pub const fn as_str(self) -> &'static str {
        match self {
            DarlingMode::Nonroot => "nonroot",
            DarlingMode::Rootless => "rootless",
        }
    }

    /// The mode for a name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "nonroot" => Some(DarlingMode::Nonroot),
            "rootless" => Some(DarlingMode::Rootless),
            _ => None,
        }
    }

    /// The variable that selects the mode in Darling's launcher.
    pub const fn variable(self) -> &'static str {
        match self {
            DarlingMode::Nonroot => "DARLING_NONROOT",
            DarlingMode::Rootless => "DARLING_ROOTLESS",
        }
    }
}

/// How an application's Darling prefix is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrefixLayout {
    /// The application's own layer (its own copies of Darling's directories,
    /// its account files, and whatever it writes) over Darling's read-only
    /// system, as an overlay that bubblewrap mounts in the sandbox. Darling,
    /// in its nonroot mode, finds every system file already in place and
    /// copies nothing. The default.
    Layered,
    /// An empty directory that Darling makes a prefix of in its own way: a
    /// full copy of its system in the nonroot mode, its own overlay in the
    /// rootless mode.
    Darling,
}

impl PrefixLayout {
    /// The layout's name, as the command line takes it.
    pub const fn as_str(self) -> &'static str {
        match self {
            PrefixLayout::Layered => "layered",
            PrefixLayout::Darling => "darling",
        }
    }

    /// The layout for a name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "layered" => Some(PrefixLayout::Layered),
            "darling" => Some(PrefixLayout::Darling),
            _ => None,
        }
    }
}

/// One published build of Darling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// The build's identifier, also the name of its directory here:
    /// `darling-<commit>-q<queue>-fc<release>-<architecture>`.
    pub id: String,
    /// The processor it is built for, `x86_64`.
    pub architecture: String,
    /// The Fedora release of the base it was built on and links against.
    pub fedora_release: String,
    /// The digest of the pinned base image it was built on.
    pub base_digest: String,
    /// Darling's commit.
    pub darling_commit: String,
    /// The SHA-256 of TUR's patch queue (`third-party/darling/patches`) as
    /// applied: the `series` file and every patch it names.
    pub queue_sha256: String,
    /// The package in the registry, `ghcr.io/<owner>/<name>`.
    pub repository: String,
    /// The tag it was published under.
    pub tag: String,
    /// The archive's SHA-256, which is also its digest in the registry.
    /// `None` until the build has been published.
    pub sha256: Option<String>,
    /// The archive's size in bytes, `None` until published.
    pub size: Option<u64>,
    /// The size of the unpacked tree in bytes, when measured.
    pub unpacked_size: Option<u64>,
    /// The SHA-256 of the corresponding source, published beside the
    /// archive in the same registry package: Darling at the pinned commit
    /// with its submodules, and TUR's patch queue (owner's decision, 16
    /// November 2026). A published build always has it.
    pub source_sha256: Option<String>,
    /// The source archive's size in bytes.
    pub source_size: Option<u64>,
    /// The path the build expects to be shown at.
    pub install_prefix: String,
    /// The licences of what is in the archive, as an SPDX expression.
    pub licence: String,
}

/// The manifest: every build this program knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The file's licence and what it is, for people.
    #[serde(rename = "$comment", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// [`MANIFEST_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The builds, one per architecture at most.
    pub artifacts: Vec<Artifact>,
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_token(text: &str, max: usize, extra: &[u8]) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || extra.contains(&byte))
}

impl Manifest {
    /// Parses and checks a manifest.
    ///
    /// # Errors
    ///
    /// A message naming the field that is not valid.
    pub fn parse(text: &str) -> Result<Self, String> {
        let manifest: Manifest =
            serde_json::from_str(text).map_err(|error| format!("not a valid manifest: {error}"))?;
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(format!(
                "schema_version {} is not {MANIFEST_SCHEMA_VERSION}",
                manifest.schema_version
            ));
        }
        for artifact in &manifest.artifacts {
            artifact.check()?;
        }
        Ok(manifest)
    }

    /// The manifest compiled into this program.
    ///
    /// # Errors
    ///
    /// As [`Manifest::parse`]; a test keeps the compiled one valid.
    pub fn compiled() -> Result<Self, String> {
        Manifest::parse(MANIFEST)
    }

    /// The build for a processor.
    pub fn for_arch(&self, arch: Arch) -> Option<&Artifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.architecture == arch.as_str())
    }
}

impl Artifact {
    fn check(&self) -> Result<(), String> {
        let what = &self.id;
        if !is_token(&self.id, 128, b".-_") {
            return Err(format!("the build id {:?} is not a plain name", self.id));
        }
        if Arch::from_name(&self.architecture).is_none() {
            return Err(format!("{what}: unknown architecture"));
        }
        if !is_token(&self.fedora_release, 8, b"") {
            return Err(format!("{what}: fedora_release is not a release number"));
        }
        let base = self.base_digest.strip_prefix("sha256:").unwrap_or("");
        if !is_hex64(base) {
            return Err(format!("{what}: base_digest is not a sha256 digest"));
        }
        if self.darling_commit.len() != 40 || !self.darling_commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{what}: darling_commit is not a full commit id"));
        }
        if !is_hex64(&self.queue_sha256) {
            return Err(format!("{what}: queue_sha256 is not a sha256 digest"));
        }
        let Some(path) = self.repository.strip_prefix("ghcr.io/") else {
            return Err(format!("{what}: the repository is not in ghcr.io"));
        };
        if !is_token(path, 255, b"._-/") || path.starts_with('/') || path.contains("..") {
            return Err(format!("{what}: the repository path is not valid"));
        }
        if !self
            .tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || self.tag.is_empty()
            || self.tag.len() > 128
        {
            return Err(format!("{what}: the tag is not valid"));
        }
        match (&self.sha256, self.size) {
            (None, None) => {}
            (Some(digest), Some(size)) => {
                if !is_hex64(digest) {
                    return Err(format!("{what}: sha256 is not a sha256 digest"));
                }
                if size == 0 || size > MAX_ARCHIVE_BYTES {
                    return Err(format!("{what}: the size is not allowed"));
                }
            }
            _ => return Err(format!("{what}: sha256 and size go together")),
        }
        match (&self.source_sha256, self.source_size) {
            (None, None) => {
                if self.sha256.is_some() {
                    return Err(format!(
                        "{what}: a published build is published with its source"
                    ));
                }
            }
            (Some(digest), Some(size)) => {
                if !is_hex64(digest) || size == 0 {
                    return Err(format!("{what}: the source's digest or size is not valid"));
                }
            }
            _ => return Err(format!("{what}: source_sha256 and source_size go together")),
        }
        if self.install_prefix != RUNTIME_TREE_PATH {
            return Err(format!(
                "{what}: built for {}, not for {RUNTIME_TREE_PATH}",
                self.install_prefix
            ));
        }
        Ok(())
    }

    /// Whether the build has been published.
    pub fn published(&self) -> bool {
        self.sha256.is_some()
    }

    /// The registry's host and the repository's path in it.
    fn registry_path(&self) -> (&str, &str) {
        let path = self
            .repository
            .strip_prefix("ghcr.io/")
            .unwrap_or(&self.repository);
        ("ghcr.io", path)
    }

    /// The address of the registry's token service for an anonymous pull.
    pub fn token_url(&self) -> String {
        let (host, path) = self.registry_path();
        format!("https://{host}/token?scope=repository:{path}:pull&service={host}")
    }

    /// The address of the archive, by digest, in the registry.
    pub fn blob_url(&self) -> Option<String> {
        let (host, path) = self.registry_path();
        let digest = self.sha256.as_ref()?;
        Some(format!("https://{host}/v2/{path}/blobs/sha256:{digest}"))
    }

    /// The address of the build's corresponding source, by digest, in the
    /// same registry package.
    pub fn source_url(&self) -> Option<String> {
        let (host, path) = self.registry_path();
        let digest = self.source_sha256.as_ref()?;
        Some(format!("https://{host}/v2/{path}/blobs/sha256:{digest}"))
    }
}

/// Where builds are kept on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The store under the runtime manager's data directory.
    pub fn new(dirs: &RuntimeDirs) -> Self {
        Store {
            root: dirs.data_root().join("darling"),
        }
    }

    /// The store's directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory of one build.
    pub fn artifact_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// The unpacked archive of one build.
    pub fn tree(&self, id: &str) -> PathBuf {
        self.artifact_dir(id).join("tree")
    }

    /// The build itself: what is shown at [`RUNTIME_TREE_PATH`].
    pub fn install_root(&self, id: &str) -> PathBuf {
        self.tree(id).join(RUNTIME_TREE_PATH.trim_start_matches('/'))
    }

    /// The record of the check that admitted one build.
    pub fn verified(&self, id: &str) -> PathBuf {
        self.artifact_dir(id).join("verified.json")
    }

    /// Scratch space while one build is fetched.
    pub fn staging(&self, id: &str) -> PathBuf {
        self.root.join("staging").join(id)
    }

    /// The archive while it is fetched.
    pub fn download(&self, id: &str) -> PathBuf {
        self.staging(id).join("darling.tar.xz")
    }

    /// The file that holds the registry's pull token while it is used.
    pub fn header_file(&self, id: &str) -> PathBuf {
        self.staging(id).join("authorization")
    }
}

/// Where a build came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// The repository's package registry.
    Registry,
    /// A file the person or a test gave.
    File,
}

/// What was checked when a build was admitted. Written last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedRecord {
    /// [`VERIFIED_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The build.
    pub id: String,
    /// The archive's SHA-256, as checked.
    pub sha256: String,
    /// The archive's size, as checked.
    pub size: u64,
    /// Darling's commit.
    pub darling_commit: String,
    /// Where the archive came from.
    pub source: Source,
    /// When it was checked, in UTC.
    pub verified_at: String,
}

/// Where the OS image keeps Darling (image/darling/darling.sh): read-only,
/// updated with the image.
pub const SYSTEM_TREE: &str = "/usr/lib/tur/darling";

/// The record of which build the image carries.
pub const SYSTEM_BUILD_FILE: &str = "/usr/lib/tur/darling.json";

/// The build of Darling that is part of the OS image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemBuild {
    /// The build's identifier, as in the manifest.
    pub id: String,
    /// Darling's commit.
    pub darling_commit: String,
    /// The archive's SHA-256, as the image build checked it.
    pub sha256: String,
    /// The corresponding source's SHA-256.
    #[serde(default)]
    pub source_sha256: String,
    /// Where it is: [`SYSTEM_TREE`], or a test's tree.
    #[serde(skip_deserializing)]
    pub root: String,
}

/// The image's build of Darling, when the image carries one: its record
/// parses, names a build, and the launcher is in the tree.
pub fn system_build_at(record: &Path, tree: &Path) -> Option<SystemBuild> {
    let file = fs::File::open(record).ok()?;
    let mut text = String::new();
    file.take(MAX_VERIFIED_BYTES).read_to_string(&mut text).ok()?;
    let mut build: SystemBuild = serde_json::from_str(&text).ok()?;
    let launcher = tree.join("bin").join("darling");
    let present = fs::symlink_metadata(launcher).is_ok_and(|metadata| metadata.file_type().is_file());
    if !present || !is_token(&build.id, 128, b".-_") || !is_hex64(&build.sha256) {
        return None;
    }
    build.root = plain_text(tree)?.to_owned();
    Some(build)
}

/// The image's build of Darling on this computer.
pub fn system_build() -> Option<SystemBuild> {
    system_build_at(Path::new(SYSTEM_BUILD_FILE), Path::new(SYSTEM_TREE))
}

/// The state of TUR's Darling on this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeState {
    /// Darling is part of the OS image and is updated with it. Nothing is
    /// fetched.
    System,
    /// This program knows no published build for this processor.
    NotPublished,
    /// A build is published and has not been fetched.
    NotFetched,
    /// The build is here and was checked when it was fetched.
    Ready,
    /// Something is here and it is not a checked build: fetch it again.
    Damaged,
}

/// What `mac runtime status` reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeStatus {
    /// The state.
    pub state: RuntimeState,
    /// The build this program would use here.
    pub artifact: Option<Artifact>,
    /// The record of the check, when the build is here.
    pub verified: Option<VerifiedRecord>,
    /// Where the build is, when it is here.
    pub install_root: Option<String>,
    /// The image's build, when the image carries Darling.
    pub system: Option<SystemBuild>,
    /// The state in plain language.
    pub message: String,
}

/// Reads `verified.json` of a build: at most a few kilobytes, and it must
/// name the build.
fn read_verified(store: &Store, id: &str) -> Option<VerifiedRecord> {
    let file = fs::File::open(store.verified(id)).ok()?;
    let mut text = String::new();
    file.take(MAX_VERIFIED_BYTES).read_to_string(&mut text).ok()?;
    let record: VerifiedRecord = serde_json::from_str(&text).ok()?;
    (record.schema_version == VERIFIED_SCHEMA_VERSION && record.id == id && is_hex64(&record.sha256))
        .then_some(record)
}

/// The state of Darling on this computer: the image's build when there is
/// one; otherwise the fetched build for `arch`, the fallback.
pub fn status(
    store: &Store,
    manifest: &Manifest,
    arch: Arch,
    system: Option<&SystemBuild>,
) -> RuntimeStatus {
    if let Some(system) = system {
        return RuntimeStatus {
            state: RuntimeState::System,
            artifact: manifest.for_arch(arch).cloned(),
            verified: None,
            install_root: Some(system.root.clone()),
            message: format!(
                "Darling {} is part of this system, at {}, and is updated with it.",
                short(&system.darling_commit),
                system.root
            ),
            system: Some(system.clone()),
        };
    }
    let Some(artifact) = manifest.for_arch(arch).cloned() else {
        return RuntimeStatus {
            state: RuntimeState::NotPublished,
            artifact: None,
            verified: None,
            install_root: None,
            system: None,
            message: format!(
                "TUR has no build of Darling for {} processors.",
                arch.as_str()
            ),
        };
    };
    let launcher = store.install_root(&artifact.id).join("bin").join("darling");
    let verified = read_verified(store, &artifact.id);
    let launcher_present =
        fs::symlink_metadata(&launcher).is_ok_and(|metadata| metadata.file_type().is_file());
    let anything = fs::symlink_metadata(store.artifact_dir(&artifact.id)).is_ok();
    let expected = artifact.sha256.clone();
    let (state, message) = match (&verified, launcher_present) {
        (Some(record), true) if Some(&record.sha256) == expected.as_ref() => (
            RuntimeState::Ready,
            format!("Darling {} is here and was checked.", short(&artifact.darling_commit)),
        ),
        (Some(record), true) if expected.is_none() && record.source == Source::File => (
            RuntimeState::Ready,
            format!(
                "Darling {} is here, imported from a file and checked against the digest given \
                 with it; this program's manifest names no published build yet.",
                short(&artifact.darling_commit)
            ),
        ),
        _ if anything => (
            RuntimeState::Damaged,
            "Something is in this build's directory that is not a checked build; fetch it again."
                .to_owned(),
        ),
        _ if !artifact.published() => (
            RuntimeState::NotPublished,
            "TUR has not published this build of Darling yet; a build can be imported from a \
             file with its digest."
                .to_owned(),
        ),
        _ => (
            RuntimeState::NotFetched,
            "Darling is fetched when the first macOS application is installed.".to_owned(),
        ),
    };
    let install_root = (state == RuntimeState::Ready)
        .then(|| store.install_root(&artifact.id).display().to_string());
    RuntimeStatus {
        state,
        verified: if state == RuntimeState::Ready {
            verified
        } else {
            None
        },
        install_root,
        artifact: Some(artifact),
        system: None,
        message,
    }
}

/// Whether the build `id` is here and was checked when it was admitted: an
/// application installed with it can be started with it, even after this
/// program's manifest has moved on to a newer build.
pub fn is_ready(store: &Store, id: &str) -> bool {
    let launcher = store.install_root(id).join("bin").join("darling");
    read_verified(store, id).is_some()
        && fs::symlink_metadata(launcher).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

fn path_text(path: &Path, what: &str) -> Result<String, MacError> {
    plain_text(path).map(str::to_owned).ok_or_else(|| {
        MacError::new(
            Code::StorageUnusable,
            format!("the path of {what} is not a plain absolute path"),
        )
    })
}

fn environment() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::from([("PATH".to_owned(), RUNTIME_PATH.to_owned())])
}

/// The command that asks the registry for an anonymous pull token. Its
/// answer is read by this program ([`parse_token`]), never shown.
///
/// # Errors
///
/// [`MacError`] when `curl` is not a plain path.
pub fn token_command(curl: &Path, artifact: &Artifact) -> Result<CommandSpec, MacError> {
    Ok(CommandSpec {
        program: path_text(curl, "curl")?,
        arguments: vec![
            "--proto".to_owned(),
            "=https".to_owned(),
            "--tlsv1.2".to_owned(),
            "--fail".to_owned(),
            "--silent".to_owned(),
            "--show-error".to_owned(),
            "--max-time".to_owned(),
            "60".to_owned(),
            "--max-filesize".to_owned(),
            MAX_TOKEN_RESPONSE_BYTES.to_string(),
            artifact.token_url(),
        ],
        environment: environment(),
        working_directory: None,
    })
}

/// The token in the registry's answer, `{"token": "..."}`.
///
/// # Errors
///
/// [`MacError`] when the answer is not that, or the token holds characters
/// a bearer token does not.
pub fn parse_token(answer: &[u8]) -> Result<String, MacError> {
    let refused = || {
        MacError::new(
            Code::MacosRuntimeNotPublished,
            "the package registry did not hand out a pull token for TUR's Darling build",
        )
    };
    if answer.len() > MAX_TOKEN_RESPONSE_BYTES {
        return Err(refused());
    }
    let value: serde_json::Value = serde_json::from_slice(answer).map_err(|_| refused())?;
    let token = value
        .get("token")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(refused)?;
    let plain = !token.is_empty()
        && token.len() <= 8192
        && token.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'=' | b'+' | b'/')
        });
    if plain { Ok(token.to_owned()) } else { Err(refused()) }
}

/// The contents of the header file the download is made with. It holds the
/// token, so it is written with mode 0600 into the staging directory and
/// deleted with it; the token is never on a command line.
pub fn header_contents(token: &str) -> String {
    format!("Authorization: Bearer {token}\n")
}

/// The plan that downloads the archive into the staging directory. The
/// token was written to [`Store::header_file`] before.
///
/// # Errors
///
/// [`MacError`] when the build is not published or a path is not plain.
pub fn plan_download(store: &Store, artifact: &Artifact, curl: &Path) -> Result<Plan, MacError> {
    let (Some(url), Some(size)) = (artifact.blob_url(), artifact.size) else {
        return Err(MacError::new(
            Code::MacosRuntimeNotPublished,
            "this build of Darling is not published yet",
        ));
    };
    let mut plan = Plan::new(Operation::Install, &format!("darling/{}", artifact.id));
    plan.steps.push(Step::CreateDir {
        path: store.staging(&artifact.id),
    });
    let header = path_text(&store.header_file(&artifact.id), "the header file")?;
    let download = path_text(&store.download(&artifact.id), "the download")?;
    plan.steps.push(Step::RunBounded {
        label: "download TUR's build of Darling from the package registry".to_owned(),
        command: CommandSpec {
            program: path_text(curl, "curl")?,
            arguments: vec![
                "--proto".to_owned(),
                "=https".to_owned(),
                "--proto-redir".to_owned(),
                "=https".to_owned(),
                "--tlsv1.2".to_owned(),
                "--fail".to_owned(),
                "--location".to_owned(),
                "--silent".to_owned(),
                "--show-error".to_owned(),
                "--max-filesize".to_owned(),
                size.to_string(),
                "--header".to_owned(),
                format!("@{header}"),
                "--output".to_owned(),
                download,
                url,
            ],
            environment: environment(),
            working_directory: None,
        },
        limit_seconds: DOWNLOAD_SECONDS,
    });
    plan.notes.push(format!(
        "The archive is {size} bytes. Nothing in it is used before its size and SHA-256 match \
         the manifest."
    ));
    Ok(plan)
}

/// Checks a file against what the build must be: the manifest's digest and
/// size, or, for a build not yet published, the digest given with the file.
///
/// # Errors
///
/// [`MacError`] with [`Code::MacosRuntimeDigest`] when they differ, and
/// [`Code::Unreadable`] when the file cannot be read.
pub fn check_archive(
    path: &Path,
    artifact: &Artifact,
    given: Option<&str>,
) -> Result<(String, u64), MacError> {
    let expected = match (&artifact.sha256, given) {
        (Some(published), Some(given)) if published != given => {
            return Err(MacError::new(
                Code::MacosRuntimeDigest,
                format!(
                    "the digest given ({given}) is not the published build's ({published})"
                ),
            ));
        }
        (Some(published), _) => published.clone(),
        (None, Some(given)) if is_hex64(given) => given.to_owned(),
        (None, _) => {
            return Err(MacError::new(
                Code::MacosRuntimeNotPublished,
                "this build of Darling is not published yet; an archive can be imported only \
                 with its SHA-256 (--sha256)",
            ));
        }
    };
    let (digest, size) = crate::sha256::sha256_file(path).map_err(|error| {
        MacError::new(
            Code::Unreadable,
            format!("cannot read {}: {error}", path.display()),
        )
    })?;
    if let Some(published) = artifact.size
        && published != size
    {
        return Err(MacError::new(
            Code::MacosRuntimeDigest,
            format!("the archive has {size} bytes; the manifest says {published}"),
        ));
    }
    if digest != expected {
        return Err(MacError::new(
            Code::MacosRuntimeDigest,
            format!("the archive's SHA-256 is {digest}; the manifest says {expected}"),
        ));
    }
    Ok((digest, size))
}

/// The plan that unpacks a checked archive and admits the build: unpack
/// into the staging directory, require the launcher, move the tree into
/// place, write `verified.json` last, and delete the staging directory.
///
/// # Errors
///
/// [`MacError`] when a path is not plain or the record cannot be written.
pub fn plan_unpack(
    store: &Store,
    artifact: &Artifact,
    archive: &Path,
    tar: &Path,
    record: &VerifiedRecord,
) -> Result<Plan, MacError> {
    let id = &artifact.id;
    let staging_tree = store.staging(id).join("tree");
    let mut plan = Plan::new(Operation::Install, &format!("darling/{id}"));
    plan.steps.push(Step::RemoveTree {
        path: staging_tree.clone(),
    });
    plan.steps.push(Step::CreateDir {
        path: staging_tree.clone(),
    });
    plan.steps.push(Step::RunBounded {
        label: "unpack TUR's build of Darling".to_owned(),
        command: CommandSpec {
            program: path_text(tar, "tar")?,
            arguments: vec![
                "--extract".to_owned(),
                "--xz".to_owned(),
                "--no-same-owner".to_owned(),
                "--file".to_owned(),
                path_text(archive, "the archive")?,
                "--directory".to_owned(),
                path_text(&staging_tree, "the staging directory")?,
            ],
            environment: environment(),
            working_directory: None,
        },
        limit_seconds: UNPACK_SECONDS,
    });
    let launcher_inside = staging_tree
        .join(RUNTIME_TREE_PATH.trim_start_matches('/'))
        .join("bin")
        .join("darling");
    plan.steps.push(Step::Require {
        check: Check::File {
            path: launcher_inside,
        },
    });
    plan.steps.push(Step::RemoveTree {
        path: store.artifact_dir(id),
    });
    plan.steps.push(Step::CreateDir {
        path: store.artifact_dir(id),
    });
    plan.steps.push(Step::Rename {
        from: staging_tree,
        to: store.tree(id),
    });
    let text = serde_json::to_string_pretty(record).map_err(|error| {
        MacError::new(Code::StorageUnusable, format!("cannot write the record: {error}"))
    })?;
    plan.steps.push(Step::WriteFile {
        path: store.verified(id),
        contents: format!("{text}\n"),
    });
    plan.steps.push(Step::RemoveTree {
        path: store.staging(id),
    });
    Ok(plan)
}

/// The record written when a build is admitted.
pub fn verified_record(
    artifact: &Artifact,
    sha256: String,
    size: u64,
    source: Source,
    now: Timestamp,
) -> VerifiedRecord {
    VerifiedRecord {
        schema_version: VERIFIED_SCHEMA_VERSION,
        id: artifact.id.clone(),
        sha256,
        size,
        darling_commit: artifact.darling_commit.clone(),
        source,
        verified_at: now.rfc3339(),
    }
}

/// The plan that removes every build kept here, and the staging area. The
/// applications' prefixes are not touched: each is a copy of the system it
/// was made with, and is removed with its application.
pub fn plan_remove(store: &Store, ids: &[String]) -> Plan {
    let mut plan = Plan::new(Operation::Remove, "darling");
    for id in ids {
        plan.steps.push(Step::RemoveTree {
            path: store.artifact_dir(id),
        });
        plan.steps.push(Step::RemoveTree {
            path: store.staging(id),
        });
        plan.steps.push(Step::Check {
            check: Check::Absent {
                path: store.artifact_dir(id),
            },
        });
    }
    plan
}

/// The builds kept here, by directory name.
pub fn kept(store: &Store) -> Vec<String> {
    let Ok(entries) = fs::read_dir(store.root()) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| name != "staging" && is_token(name, 128, b".-_"))
        .collect();
    ids.sort();
    ids
}

/// The release of the Fedora base this system is built on: `TUR_BASE_OS_VERSION_ID`
/// of `/usr/lib/tur/release` on TUR, `VERSION_ID` of `/etc/os-release` on
/// Fedora. `None` elsewhere.
pub fn system_release() -> Option<String> {
    if let Ok(text) = fs::read_to_string("/usr/lib/tur/release")
        && let Some(release) = release_from_tur(&text)
    {
        return Some(release);
    }
    let text = fs::read_to_string("/etc/os-release")
        .or_else(|_| fs::read_to_string("/usr/lib/os-release"))
        .ok()?;
    release_from_os_release(&text)
}

fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches('"'))
    })
}

/// `TUR_BASE_OS_VERSION_ID` from the text of `/usr/lib/tur/release`, when
/// the base is Fedora.
pub fn release_from_tur(text: &str) -> Option<String> {
    if value_of(text, "TUR_BASE_OS_ID")? != "fedora" {
        return None;
    }
    let release = value_of(text, "TUR_BASE_OS_VERSION_ID")?;
    is_token(release, 8, b"").then(|| release.to_owned())
}

/// `VERSION_ID` from the text of an os-release file, when it is Fedora's.
pub fn release_from_os_release(text: &str) -> Option<String> {
    if value_of(text, "ID")? != "fedora" {
        return None;
    }
    let release = value_of(text, "VERSION_ID")?;
    is_token(release, 8, b"").then(|| release.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::TempDir;

    fn artifact() -> Artifact {
        Artifact {
            id: "darling-754fe00baf67-q4c9d2e1a-fc45-x86_64".to_owned(),
            architecture: "x86_64".to_owned(),
            fedora_release: "45".to_owned(),
            base_digest: format!("sha256:{}", "c".repeat(64)),
            darling_commit: "754fe00baf67db29a39769a49abfb895490d8abc".to_owned(),
            queue_sha256: "d".repeat(64),
            repository: "ghcr.io/giorgiogiudice/tur-darling-runtime".to_owned(),
            tag: "754fe00baf67-fc45-x86_64".to_owned(),
            sha256: Some("a".repeat(64)),
            size: Some(5),
            unpacked_size: None,
            source_sha256: Some("f".repeat(64)),
            source_size: Some(9),
            install_prefix: RUNTIME_TREE_PATH.to_owned(),
            licence: "GPL-3.0-or-later".to_owned(),
        }
    }

    #[test]
    fn the_compiled_manifest_is_valid() {
        let manifest = Manifest::compiled().unwrap();
        let build = manifest.for_arch(Arch::X86_64).unwrap();
        assert_eq!(build.install_prefix, RUNTIME_TREE_PATH);
        assert!(build.repository.starts_with("ghcr.io/giorgiogiudice/"));
        // TUR builds Darling for x86-64 only, for now.
        assert!(manifest.for_arch(Arch::Aarch64).is_none());
    }

    #[test]
    fn manifest_fields_are_checked() {
        let good = Manifest {
            comment: None,
            schema_version: 1,
            artifacts: vec![artifact()],
        };
        let text = serde_json::to_string(&good).unwrap();
        assert_eq!(Manifest::parse(&text).unwrap(), good);

        let broken: [fn(&mut Artifact); 10] = [
            |a| a.id = "../escape".to_owned(),
            |a| a.sha256 = Some("A".repeat(64)),
            |a| a.size = None,
            |a| a.repository = "docker.io/library/darling".to_owned(),
            |a| a.repository = "ghcr.io/../x".to_owned(),
            |a| a.install_prefix = "/usr".to_owned(),
            |a| a.darling_commit = "main".to_owned(),
            |a| a.size = Some(MAX_ARCHIVE_BYTES + 1),
            |a| {
                a.source_sha256 = None;
                a.source_size = None;
            },
            |a| a.source_size = None,
        ];
        for change in broken {
            let mut bad = artifact();
            change(&mut bad);
            let text = serde_json::to_string(&Manifest {
                comment: None,
                schema_version: 1,
                artifacts: vec![bad],
            })
            .unwrap();
            assert!(Manifest::parse(&text).is_err(), "{text}");
        }
        assert!(Manifest::parse("{\"schema_version\":1,\"artifacts\":[],\"x\":1}").is_err());
        assert!(Manifest::parse("{\"schema_version\":2,\"artifacts\":[]}").is_err());
    }

    #[test]
    fn registry_addresses() {
        let build = artifact();
        assert_eq!(
            build.token_url(),
            "https://ghcr.io/token?scope=repository:giorgiogiudice/tur-darling-runtime:pull&service=ghcr.io"
        );
        assert_eq!(
            build.blob_url().unwrap(),
            format!(
                "https://ghcr.io/v2/giorgiogiudice/tur-darling-runtime/blobs/sha256:{}",
                "a".repeat(64)
            )
        );
        assert_eq!(
            build.source_url().unwrap(),
            format!(
                "https://ghcr.io/v2/giorgiogiudice/tur-darling-runtime/blobs/sha256:{}",
                "f".repeat(64)
            )
        );
        let unpublished = Artifact {
            sha256: None,
            size: None,
            source_sha256: None,
            source_size: None,
            ..artifact()
        };
        assert!(unpublished.blob_url().is_none());
        assert!(unpublished.source_url().is_none());
    }

    #[test]
    fn tokens_are_read_and_never_trusted_blindly() {
        assert_eq!(parse_token(b"{\"token\":\"djE6Z2hjcg==\"}").unwrap(), "djE6Z2hjcg==");
        for bad in [
            &b"not json"[..],
            b"{\"access_token\":\"x\"}",
            b"{\"token\":\"\"}",
            b"{\"token\":\"a b\"}",
            b"{\"token\":\"x\\ny\"}",
        ] {
            assert!(parse_token(bad).is_err());
        }
        assert_eq!(header_contents("abc"), "Authorization: Bearer abc\n");
    }

    #[test]
    fn the_download_never_puts_the_token_on_the_command_line() {
        let dirs = RuntimeDirs::new(Path::new("/home/u/.local/share"), Path::new("/home/u/.local/state"));
        let store = Store::new(&dirs);
        let build = artifact();
        let plan = plan_download(&store, &build, Path::new("/usr/bin/curl")).unwrap();
        let Step::RunBounded { command, .. } = &plan.steps[1] else {
            panic!("expected the download");
        };
        assert!(command.arguments.contains(&"=https".to_owned()));
        let header = format!("@{}", store.header_file(&build.id).display());
        assert!(command.arguments.contains(&header));
        assert!(!command.arguments.iter().any(|argument| argument.contains("Bearer")));
        assert_eq!(command.environment.len(), 1);

        let unpublished = Artifact {
            sha256: None,
            size: None,
            ..artifact()
        };
        assert_eq!(
            plan_download(&store, &unpublished, Path::new("/usr/bin/curl"))
                .unwrap_err()
                .code,
            Code::MacosRuntimeNotPublished
        );
    }

    #[test]
    fn archives_are_checked_against_the_manifest_or_the_given_digest() {
        let scratch = TempDir::new("darling-archive");
        let file = scratch.path().join("darling.tar.xz");
        fs::write(&file, b"hello").unwrap();
        // SHA-256 of "hello".
        let digest = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let published = Artifact {
            sha256: Some(digest.to_owned()),
            size: Some(5),
            ..artifact()
        };
        assert_eq!(check_archive(&file, &published, None).unwrap().1, 5);
        let wrong = Artifact {
            sha256: Some("b".repeat(64)),
            ..published.clone()
        };
        assert_eq!(
            check_archive(&file, &wrong, None).unwrap_err().code,
            Code::MacosRuntimeDigest
        );
        let unpublished = Artifact {
            sha256: None,
            size: None,
            ..artifact()
        };
        assert_eq!(
            check_archive(&file, &unpublished, None).unwrap_err().code,
            Code::MacosRuntimeNotPublished
        );
        assert!(check_archive(&file, &unpublished, Some(digest)).is_ok());
        assert_eq!(
            check_archive(&file, &unpublished, Some("b".repeat(64).as_str()))
                .unwrap_err()
                .code,
            Code::MacosRuntimeDigest
        );
        assert_eq!(
            check_archive(&file, &published, Some("b".repeat(64).as_str()))
                .unwrap_err()
                .code,
            Code::MacosRuntimeDigest
        );
    }

    #[test]
    fn status_follows_what_is_on_disk() {
        let scratch = TempDir::new("darling-status");
        let dirs = RuntimeDirs::new(&scratch.path().join("data"), &scratch.path().join("state"));
        let store = Store::new(&dirs);
        let build = artifact();
        let manifest = Manifest {
            comment: None,
            schema_version: 1,
            artifacts: vec![build.clone()],
        };
        assert_eq!(status(&store, &manifest, Arch::X86_64, None).state, RuntimeState::NotFetched);
        assert_eq!(status(&store, &manifest, Arch::Aarch64, None).state, RuntimeState::NotPublished);

        // A tree without a record is not a build.
        let launcher = store.install_root(&build.id).join("bin");
        fs::create_dir_all(&launcher).unwrap();
        fs::write(launcher.join("darling"), b"").unwrap();
        assert_eq!(status(&store, &manifest, Arch::X86_64, None).state, RuntimeState::Damaged);

        let record = verified_record(
            &build,
            "a".repeat(64),
            5,
            Source::Registry,
            Timestamp::from_unix(1_794_478_500),
        );
        fs::write(store.verified(&build.id), serde_json::to_string(&record).unwrap()).unwrap();
        let ready = status(&store, &manifest, Arch::X86_64, None);
        assert_eq!(ready.state, RuntimeState::Ready);
        assert_eq!(ready.verified.unwrap().source, Source::Registry);
        assert_eq!(kept(&store), [build.id.clone()]);

        // A record of another digest is not this build.
        let other = VerifiedRecord {
            sha256: "e".repeat(64),
            ..record
        };
        fs::write(store.verified(&build.id), serde_json::to_string(&other).unwrap()).unwrap();
        assert_eq!(status(&store, &manifest, Arch::X86_64, None).state, RuntimeState::Damaged);
    }

    #[test]
    fn the_image_s_build_wins_and_nothing_is_fetched() {
        let scratch = TempDir::new("darling-system");
        let tree = scratch.path().join("usr/lib/tur/darling");
        let record = scratch.path().join("usr/lib/tur/darling.json");
        // No record, or no launcher: no system build.
        assert!(system_build_at(&record, &tree).is_none());
        fs::create_dir_all(tree.join("bin")).unwrap();
        fs::write(
            &record,
            format!(
                "{{\"schema_version\":1,\"id\":\"darling-3fcd2e1aa850-qc89ea8ce-fc45-x86_64\",\
                 \"darling_commit\":\"3fcd2e1aa85079789d19d82415695bdecc512241\",\
                 \"queue_sha256\":\"{q}\",\"sha256\":\"{a}\",\"source_sha256\":\"{f}\",\
                 \"repository\":\"ghcr.io/giorgiogiudice/tur-darling-runtime\",\"tag\":\"t\"}}",
                q = "c".repeat(64),
                a = "a".repeat(64),
                f = "f".repeat(64)
            ),
        )
        .unwrap();
        assert!(system_build_at(&record, &tree).is_none());
        fs::write(tree.join("bin/darling"), b"").unwrap();
        let system = system_build_at(&record, &tree).unwrap();
        assert_eq!(system.root, tree.to_str().unwrap());

        let dirs = RuntimeDirs::new(&scratch.path().join("data"), &scratch.path().join("state"));
        let store = Store::new(&dirs);
        let manifest = Manifest {
            comment: None,
            schema_version: 1,
            artifacts: vec![artifact()],
        };
        let found = status(&store, &manifest, Arch::X86_64, Some(&system));
        assert_eq!(found.state, RuntimeState::System);
        assert_eq!(found.install_root.as_deref(), tree.to_str());
        assert!(!store.root().exists());

        // An image record that names no valid build is ignored.
        fs::write(&record, "{\"id\":\"../x\",\"darling_commit\":\"x\",\"sha256\":\"x\"}").unwrap();
        assert!(system_build_at(&record, &tree).is_none());
    }

    #[test]
    fn unpacking_writes_the_record_last() {
        let dirs = RuntimeDirs::new(Path::new("/home/u/.local/share"), Path::new("/home/u/.local/state"));
        let store = Store::new(&dirs);
        let build = artifact();
        let record = verified_record(
            &build,
            "a".repeat(64),
            5,
            Source::File,
            Timestamp::from_unix(1_794_478_500),
        );
        let plan = plan_unpack(
            &store,
            &build,
            &store.download(&build.id),
            Path::new("/usr/bin/tar"),
            &record,
        )
        .unwrap();
        let written: Vec<usize> = plan
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| matches!(step, Step::WriteFile { .. }))
            .map(|(index, _)| index)
            .collect();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0], plan.steps.len() - 2);
        assert!(matches!(plan.steps.last(), Some(Step::RemoveTree { .. })));
        assert!(plan.steps.iter().any(|step| matches!(step, Step::Require { .. })));
    }

    #[test]
    fn the_system_release_is_read_from_tur_or_fedora() {
        let tur = "TUR_VERSION=0.1\nTUR_BASE_OS_ID=fedora\nTUR_BASE_OS_VERSION_ID=45\n";
        assert_eq!(release_from_tur(tur).as_deref(), Some("45"));
        assert_eq!(release_from_tur("TUR_BASE_OS_ID=debian\n"), None);
        let fedora = "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=45\n";
        assert_eq!(release_from_os_release(fedora).as_deref(), Some("45"));
        assert_eq!(release_from_os_release("ID=ubuntu\nVERSION_ID=\"24.04\"\n"), None);
    }

    #[test]
    fn modes_have_names_and_variables() {
        for mode in [DarlingMode::Nonroot, DarlingMode::Rootless] {
            assert_eq!(DarlingMode::from_name(mode.as_str()), Some(mode));
        }
        assert_eq!(DarlingMode::Nonroot.variable(), "DARLING_NONROOT");
        assert_eq!(DarlingMode::from_name("root"), None);
    }
}
