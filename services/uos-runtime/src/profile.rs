// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The application profile (specification §7.3).
//!
//! A profile records how one application is run: its binary, format,
//! architecture, runtime and the runtime's settings. The interchange format
//! is JSON, described by `schemas/app-profile.schema.json`; the types here
//! mirror that schema and a test keeps the two in step.
//!
//! Validation is stricter than deserialisation. Unknown fields are refused,
//! every enumerated value is checked, and a profile must never hold a secret:
//! any key that looks like a credential, and any value that looks like a
//! private key or a URL with a password in it, is rejected.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::android::AndroidPackageKind;
use crate::arch::Arch;
use crate::codes::Code;
use crate::inspect::{Detected, Inspection};
use crate::limits::MAX_PROFILE_BYTES;
use crate::macho::MachOInfo;
use crate::ole::OleKind;
use crate::recommend::{Reason, Recommendation, Runtime};

/// The profile schema version this build reads and writes.
pub const PROFILE_SCHEMA_VERSION: u32 = 1;

/// The most entries `known_requirements` may hold.
pub const MAX_KNOWN_REQUIREMENTS: usize = 64;

/// The container format of the application's binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileFormat {
    /// A Linux ELF program.
    Elf,
    /// A script with an interpreter line.
    Script,
    /// A Windows PE program.
    Pe,
    /// A Windows Installer package.
    Msi,
    /// A bare Mach-O program.
    MachO,
    /// A macOS application bundle.
    AppBundle,
    /// An Android application package.
    Apk,
    /// An archive of Android packages (XAPK, APKS).
    ApkSet,
}

impl ProfileFormat {
    /// Every format, in schema order.
    pub const ALL: [ProfileFormat; 8] = [
        ProfileFormat::Elf,
        ProfileFormat::Script,
        ProfileFormat::Pe,
        ProfileFormat::Msi,
        ProfileFormat::MachO,
        ProfileFormat::AppBundle,
        ProfileFormat::Apk,
        ProfileFormat::ApkSet,
    ];

    /// The runtimes that can host this format.
    pub const fn runtimes(self) -> &'static [ProfileRuntime] {
        match self {
            ProfileFormat::Elf | ProfileFormat::Script => &[ProfileRuntime::Native],
            ProfileFormat::Pe | ProfileFormat::Msi => &[
                ProfileRuntime::Wine,
                ProfileRuntime::Proton,
                ProfileRuntime::WindowsVm,
            ],
            ProfileFormat::MachO | ProfileFormat::AppBundle => &[ProfileRuntime::Darling],
            ProfileFormat::Apk | ProfileFormat::ApkSet => &[ProfileRuntime::Android],
        }
    }

    /// Whether this is a Windows format.
    pub const fn is_windows(self) -> bool {
        matches!(self, ProfileFormat::Pe | ProfileFormat::Msi)
    }
}

/// The processor architecture of the application's code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProfileArch {
    /// 64-bit x86.
    #[serde(rename = "x86_64")]
    X86_64,
    /// 32-bit x86.
    #[serde(rename = "i386")]
    I386,
    /// 64-bit ARM.
    #[serde(rename = "aarch64")]
    Aarch64,
    /// 32-bit ARM.
    #[serde(rename = "arm")]
    Arm,
    /// A universal Mach-O file holding several architectures.
    #[serde(rename = "universal")]
    Universal,
    /// The application does not depend on the architecture, or carries code
    /// for several: scripts, installers and Android packages.
    #[serde(rename = "any")]
    Any,
}

impl ProfileArch {
    /// Every architecture value, in schema order.
    pub const ALL: [ProfileArch; 6] = [
        ProfileArch::X86_64,
        ProfileArch::I386,
        ProfileArch::Aarch64,
        ProfileArch::Arm,
        ProfileArch::Universal,
        ProfileArch::Any,
    ];
}

/// The runtime that hosts the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileRuntime {
    /// A native Linux process.
    Native,
    /// Wine.
    Wine,
    /// Proton, for tested gaming profiles.
    Proton,
    /// Darling.
    Darling,
    /// The Android runtime.
    Android,
    /// A full Windows virtual machine.
    WindowsVm,
}

impl ProfileRuntime {
    /// Every runtime, in schema order.
    pub const ALL: [ProfileRuntime; 6] = [
        ProfileRuntime::Native,
        ProfileRuntime::Wine,
        ProfileRuntime::Proton,
        ProfileRuntime::Darling,
        ProfileRuntime::Android,
        ProfileRuntime::WindowsVm,
    ];

    /// Whether the runtime keeps a per-application prefix.
    pub const fn uses_prefix(self) -> bool {
        matches!(self, ProfileRuntime::Wine | ProfileRuntime::Proton)
    }
}

/// The release channel of the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeChannel {
    /// The released runtime.
    Stable,
    /// A runtime under test.
    Testing,
    /// An experimental runtime.
    Experimental,
}

impl RuntimeChannel {
    /// Every channel, in schema order.
    pub const ALL: [RuntimeChannel; 3] = [
        RuntimeChannel::Stable,
        RuntimeChannel::Testing,
        RuntimeChannel::Experimental,
    ];
}

/// The Direct3D translation used by a Windows application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphicsBackend {
    /// Let the runtime choose.
    Auto,
    /// DXVK (Direct3D 8 to 11 on Vulkan).
    Dxvk,
    /// VKD3D-Proton (Direct3D 12 on Vulkan).
    Vkd3dProton,
    /// Wine's built-in WineD3D.
    Wined3d,
    /// The application uses a native graphics API.
    Native,
}

impl GraphicsBackend {
    /// Every backend, in schema order.
    pub const ALL: [GraphicsBackend; 5] = [
        GraphicsBackend::Auto,
        GraphicsBackend::Dxvk,
        GraphicsBackend::Vkd3dProton,
        GraphicsBackend::Wined3d,
        GraphicsBackend::Native,
    ];
}

/// The Windows synchronisation-primitive implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncBackend {
    /// Let the runtime choose.
    Auto,
    /// The kernel's `ntsync` driver.
    Ntsync,
    /// Eventfd-based synchronisation.
    Esync,
    /// Futex-based synchronisation.
    Fsync,
}

impl SyncBackend {
    /// Every backend, in schema order.
    pub const ALL: [SyncBackend; 4] = [
        SyncBackend::Auto,
        SyncBackend::Ntsync,
        SyncBackend::Esync,
        SyncBackend::Fsync,
    ];
}

/// What to offer when the runtime cannot run the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Fallback {
    /// Nothing.
    #[serde(rename = "none")]
    NoFallback,
    /// Offer a full Windows virtual machine.
    #[serde(rename = "windows-vm-offer")]
    WindowsVmOffer,
}

impl Fallback {
    /// Every fallback, in schema order.
    pub const ALL: [Fallback; 2] = [Fallback::NoFallback, Fallback::WindowsVmOffer];
}

/// Whether the application has been tested with this profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TestStatus {
    /// Nobody has recorded a test.
    Untested,
    /// Tested and working.
    Works,
    /// Tested and partly working.
    Partial,
    /// Tested and not working.
    Broken,
}

impl TestStatus {
    /// Every status, in schema order.
    pub const ALL: [TestStatus; 4] = [
        TestStatus::Untested,
        TestStatus::Works,
        TestStatus::Partial,
        TestStatus::Broken,
    ];
}

/// An application profile, schema version 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppProfile {
    /// The schema version; always [`PROFILE_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// A stable identifier in reverse-DNS style, lower case, such as
    /// `com.example.application`.
    pub app_id: String,
    /// The name shown to the user.
    pub display_name: String,
    /// The absolute path of the program, package or bundle.
    pub binary: String,
    /// The container format of `binary`.
    pub format: ProfileFormat,
    /// The processor architecture of the application's code.
    pub architecture: ProfileArch,
    /// The runtime that hosts the application.
    pub runtime: ProfileRuntime,
    /// The runtime's release channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_channel: Option<RuntimeChannel>,
    /// The runtime version the profile is pinned to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    /// The application's own prefix; required for Wine and Proton.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_id: Option<String>,
    /// The Direct3D translation to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphics_backend: Option<GraphicsBackend>,
    /// The synchronisation implementation to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_backend: Option<SyncBackend>,
    /// The name of the isolation profile applied to the application.
    pub isolation_profile: String,
    /// Things the application is known to need, as short tokens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known_requirements: Vec<String>,
    /// What to offer when the runtime cannot run the application.
    pub fallback: Fallback,
    /// Whether the application has been tested with this profile.
    pub test_status: TestStatus,
}

/// Why a profile was rejected, or could not be drafted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// The text is larger than a profile may be.
    TooLarge {
        /// The size of the text in bytes.
        bytes: usize,
    },
    /// The text is not JSON.
    Syntax(String),
    /// The JSON is not shaped like a profile: a field is missing, unknown, of
    /// the wrong type, or holds a value outside its enumeration.
    Shape(String),
    /// A key looks like the name of a credential.
    SecretKey {
        /// The offending key.
        key: String,
    },
    /// A value looks like a credential.
    SecretValue {
        /// The field that holds it. The value itself is never reported.
        field: String,
    },
    /// A field holds a value that is not allowed.
    Invalid {
        /// The field.
        field: String,
        /// What is wrong with it.
        problem: String,
    },
    /// A profile cannot be drafted because the file has no supported runtime.
    NotLaunchable,
}

impl ProfileError {
    /// The stable UOS error code for this failure.
    pub fn code(&self) -> Code {
        match self {
            ProfileError::TooLarge { .. } | ProfileError::Syntax(_) | ProfileError::Shape(_) => {
                Code::ProfileSyntax
            }
            ProfileError::SecretKey { .. } | ProfileError::SecretValue { .. } => {
                Code::ProfileSecret
            }
            ProfileError::Invalid { .. } => Code::ProfileInvalid,
            ProfileError::NotLaunchable => Code::NoRoute,
        }
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::TooLarge { bytes } => write!(
                f,
                "the profile is {bytes} bytes; at most {MAX_PROFILE_BYTES} are accepted"
            ),
            ProfileError::Syntax(detail) => write!(f, "the profile is not valid JSON: {detail}"),
            ProfileError::Shape(detail) => {
                write!(f, "the profile does not match the schema: {detail}")
            }
            ProfileError::SecretKey { key } => write!(
                f,
                "the key {key:?} looks like a credential; profiles must never hold secrets"
            ),
            ProfileError::SecretValue { field } => write!(
                f,
                "the value of {field:?} looks like a credential; profiles must never hold secrets"
            ),
            ProfileError::Invalid { field, problem } => write!(f, "field {field:?} {problem}"),
            ProfileError::NotLaunchable => {
                f.write_str("no profile can be drafted because the file has no supported runtime")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

fn invalid(field: &str, problem: impl Into<String>) -> ProfileError {
    ProfileError::Invalid {
        field: field.to_owned(),
        problem: problem.into(),
    }
}

fn check(holds: bool, field: &str, problem: &str) -> Result<(), ProfileError> {
    if holds {
        Ok(())
    } else {
        Err(invalid(field, problem))
    }
}

/// Parses and validates a profile from JSON text.
///
/// # Errors
///
/// A [`ProfileError`] naming the first problem found. Secrets are looked for
/// before anything else, so a profile with both a credential and another
/// defect is reported as holding a credential.
pub fn parse_profile(text: &str) -> Result<AppProfile, ProfileError> {
    if text.len() > MAX_PROFILE_BYTES {
        return Err(ProfileError::TooLarge { bytes: text.len() });
    }
    let value: Value =
        serde_json::from_str(text).map_err(|error| ProfileError::Syntax(error.to_string()))?;
    if !value.is_object() {
        return Err(ProfileError::Shape("a profile is a JSON object".to_owned()));
    }
    scan(&value, "profile")?;
    let profile: AppProfile =
        serde_json::from_value(value).map_err(|error| ProfileError::Shape(error.to_string()))?;
    profile.validate()?;
    Ok(profile)
}

/// Walks every key and value looking for secrets and nulls. The recursion is
/// bounded by the JSON parser's own nesting limit.
fn scan(value: &Value, field: &str) -> Result<(), ProfileError> {
    match value {
        Value::Null => Err(invalid(field, "is null; leave an unused field out instead")),
        Value::String(text) if looks_like_secret_value(text) => Err(ProfileError::SecretValue {
            field: field.to_owned(),
        }),
        Value::Array(items) => items.iter().try_for_each(|item| scan(item, field)),
        Value::Object(members) => {
            for (key, member) in members {
                if looks_like_secret_key(key) {
                    return Err(ProfileError::SecretKey { key: key.clone() });
                }
                scan(member, key)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Fragments that mark a key as naming a credential, compared after the key
/// is lower-cased and stripped of punctuation.
const SECRET_KEY_MARKERS: [&str; 12] = [
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "apikey",
    "privatekey",
    "credential",
    "accesskey",
    "authkey",
    "sessionkey",
    "bearer",
];

fn looks_like_secret_key(key: &str) -> bool {
    let normalised: String = key
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    SECRET_KEY_MARKERS
        .iter()
        .any(|marker| normalised.contains(*marker))
}

fn looks_like_secret_value(text: &str) -> bool {
    let private_key = text.contains("-----BEGIN") && text.contains("PRIVATE KEY");
    private_key || has_url_credentials(text)
}

/// Whether `text` holds a URL of the form `scheme://user:password@host`.
fn has_url_credentials(text: &str) -> bool {
    text.split("://").skip(1).any(|rest| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        authority
            .rsplit_once('@')
            .is_some_and(|(user_info, _)| user_info.contains(':'))
    })
}

impl AppProfile {
    /// Checks every rule that the type system does not already enforce.
    ///
    /// # Errors
    ///
    /// [`ProfileError::Invalid`] naming the first field that breaks a rule.
    pub fn validate(&self) -> Result<(), ProfileError> {
        check(
            self.schema_version == PROFILE_SCHEMA_VERSION,
            "schema_version",
            "is not a schema version this build supports",
        )?;
        check(
            is_app_id(&self.app_id),
            "app_id",
            "must be lower-case segments joined by dots, such as com.example.app",
        )?;
        check(
            is_display_name(&self.display_name),
            "display_name",
            "must be 1 to 200 characters with no control characters",
        )?;
        check(
            is_binary_path(&self.binary),
            "binary",
            "must be an absolute path with no control characters",
        )?;
        if let Some(version) = &self.runtime_version {
            check(
                is_version(version),
                "runtime_version",
                "must be 1 to 100 letters, digits, dots, plus signs, hyphens or underscores",
            )?;
        }
        if let Some(prefix) = &self.prefix_id {
            check(
                is_slug(prefix),
                "prefix_id",
                "must be 1 to 64 lower-case letters, digits or hyphens",
            )?;
        }
        check(
            is_slug(&self.isolation_profile),
            "isolation_profile",
            "must be 1 to 64 lower-case letters, digits or hyphens",
        )?;
        check(
            self.known_requirements.len() <= MAX_KNOWN_REQUIREMENTS,
            "known_requirements",
            "has too many entries",
        )?;
        for (index, requirement) in self.known_requirements.iter().enumerate() {
            check(
                is_requirement(requirement),
                "known_requirements",
                "entries must be short lower-case tokens such as win32 or dotnet",
            )?;
            let earlier = self.known_requirements.get(..index).unwrap_or(&[]);
            check(
                !earlier.contains(requirement),
                "known_requirements",
                "entries must be unique",
            )?;
        }
        check(
            self.format.runtimes().contains(&self.runtime),
            "runtime",
            "is not a runtime that can host this format",
        )?;
        check(
            !self.runtime.uses_prefix() || self.prefix_id.is_some(),
            "prefix_id",
            "is required for the wine and proton runtimes",
        )?;
        check(
            self.fallback == Fallback::NoFallback || self.format.is_windows(),
            "fallback",
            "can only offer a Windows virtual machine for Windows software",
        )?;
        Ok(())
    }
}

fn lower_or_digit(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit()
}

/// Whether `text` is at most `max` bytes, starts with a byte accepted by
/// `first` and continues with bytes accepted by `rest`.
fn is_token(text: &str, max: usize, first: fn(u8) -> bool, rest: fn(u8) -> bool) -> bool {
    let mut bytes = text.bytes();
    text.len() <= max && bytes.next().is_some_and(first) && bytes.all(rest)
}

/// `^[a-z0-9][a-z0-9-]{0,63}$`
fn is_slug(text: &str) -> bool {
    is_token(text, 64, lower_or_digit, |byte| {
        lower_or_digit(byte) || byte == b'-'
    })
}

/// `^[a-z0-9][a-z0-9._-]{0,63}$`
fn is_requirement(text: &str) -> bool {
    is_token(text, 64, lower_or_digit, |byte| {
        lower_or_digit(byte) || matches!(byte, b'.' | b'_' | b'-')
    })
}

/// `^[A-Za-z0-9][A-Za-z0-9._+-]{0,99}$`
fn is_version(text: &str) -> bool {
    is_token(
        text,
        100,
        |byte| byte.is_ascii_alphanumeric(),
        |byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'),
    )
}

/// `^[a-z0-9][a-z0-9_-]*(\.[a-z0-9][a-z0-9_-]*)+$`, at most 255 bytes.
fn is_app_id(text: &str) -> bool {
    let segment = |part: &str| {
        is_token(part, 255, lower_or_digit, |byte| {
            lower_or_digit(byte) || matches!(byte, b'_' | b'-')
        })
    };
    text.len() <= 255 && text.split('.').count() >= 2 && text.split('.').all(segment)
}

fn is_display_name(text: &str) -> bool {
    let length = text.chars().count();
    (1..=200).contains(&length) && !text.chars().any(char::is_control)
}

fn is_binary_path(text: &str) -> bool {
    let length = text.chars().count();
    text.starts_with('/') && (2..=4096).contains(&length) && !text.chars().any(char::is_control)
}

/// Drafts a profile for an inspected file from its recommendation.
///
/// The draft is a starting point: it is marked untested, its runtime version
/// is not pinned, and its identifier is derived from the bundle identifier
/// or, failing that, from the file name under the `local.` namespace.
///
/// # Errors
///
/// [`ProfileError::NotLaunchable`] when the recommendation is "unsupported"
/// or the file is not something a profile can describe, and
/// [`ProfileError::Invalid`] when `binary` is not an absolute UTF-8 path or
/// the architecture is not one a profile can name.
pub fn draft_profile(
    binary: &Path,
    inspection: &Inspection,
    recommendation: &Recommendation,
) -> Result<AppProfile, ProfileError> {
    let runtime = match recommendation.runtime {
        Runtime::NativeLinux => ProfileRuntime::Native,
        Runtime::Wine => ProfileRuntime::Wine,
        Runtime::Darling => ProfileRuntime::Darling,
        Runtime::Android => ProfileRuntime::Android,
        Runtime::WindowsVmOffer => ProfileRuntime::WindowsVm,
        Runtime::Unsupported => return Err(ProfileError::NotLaunchable),
    };
    let binary_text = binary
        .to_str()
        .ok_or_else(|| invalid("binary", "is not a valid UTF-8 path"))?;
    let stem = binary
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");

    let mut app_id: Option<String> = None;
    let mut display_name: Option<String> = None;
    let (format, architecture) = match &inspection.detected {
        Detected::Elf(elf) => (ProfileFormat::Elf, profile_arch(elf.machine)?),
        Detected::Script(_) => (ProfileFormat::Script, ProfileArch::Any),
        Detected::Pe(pe) => (ProfileFormat::Pe, profile_arch(pe.machine)?),
        Detected::Ole(ole) if ole.kind == OleKind::MsiInstaller => {
            (ProfileFormat::Msi, ProfileArch::Any)
        }
        Detected::MachO(macho) => (ProfileFormat::MachO, macho_arch(macho)?),
        Detected::MacAppBundle(bundle) => {
            app_id = bundle
                .identifier
                .as_deref()
                .map(str::to_ascii_lowercase)
                .filter(|identifier| is_app_id(identifier));
            display_name = bundle.name.clone().filter(|name| is_display_name(name));
            let program = bundle.executable.as_deref().map(|inner| &inner.detected);
            let Some(Detected::MachO(macho)) = program else {
                return Err(ProfileError::NotLaunchable);
            };
            (ProfileFormat::AppBundle, macho_arch(macho)?)
        }
        Detected::Android(android) => match android.kind {
            AndroidPackageKind::Apk => (ProfileFormat::Apk, ProfileArch::Any),
            AndroidPackageKind::Aab => return Err(ProfileError::NotLaunchable),
            AndroidPackageKind::Xapk
            | AndroidPackageKind::Apks
            | AndroidPackageKind::ApkArchive => (ProfileFormat::ApkSet, ProfileArch::Any),
        },
        _ => return Err(ProfileError::NotLaunchable),
    };

    let known_requirements: Vec<String> = recommendation
        .reasons
        .iter()
        .filter_map(|reason| match reason {
            Reason::PeDotnet => Some("dotnet"),
            Reason::PeRequiresWindowsKernel => Some("windows-kernel"),
            Reason::MsiInstaller => Some("msi"),
            Reason::AppImage => Some("appimage"),
            Reason::AndroidNeedsArmTranslation => Some("arm-translation"),
            _ => None,
        })
        .map(str::to_owned)
        .collect();

    let slug = slug(stem);
    let uses_prefix = runtime.uses_prefix();
    let profile = AppProfile {
        schema_version: PROFILE_SCHEMA_VERSION,
        app_id: app_id.unwrap_or_else(|| format!("local.{slug}")),
        display_name: display_name.unwrap_or_else(|| name_from(stem)),
        binary: binary_text.to_owned(),
        format,
        architecture,
        runtime,
        runtime_channel: Some(if recommendation.experimental {
            RuntimeChannel::Experimental
        } else {
            RuntimeChannel::Stable
        }),
        runtime_version: None,
        prefix_id: uses_prefix.then(|| slug.clone()),
        graphics_backend: uses_prefix.then_some(GraphicsBackend::Auto),
        sync_backend: uses_prefix.then_some(SyncBackend::Auto),
        isolation_profile: "desktop-standard".to_owned(),
        known_requirements,
        fallback: if runtime == ProfileRuntime::Wine {
            Fallback::WindowsVmOffer
        } else {
            Fallback::NoFallback
        },
        test_status: TestStatus::Untested,
    };
    profile.validate()?;
    Ok(profile)
}

fn profile_arch(arch: Arch) -> Result<ProfileArch, ProfileError> {
    match arch {
        Arch::X86_64 => Ok(ProfileArch::X86_64),
        Arch::I386 => Ok(ProfileArch::I386),
        Arch::Aarch64 => Ok(ProfileArch::Aarch64),
        Arch::Arm => Ok(ProfileArch::Arm),
        other => Err(invalid(
            "architecture",
            format!("cannot be {other}; a profile cannot name that architecture"),
        )),
    }
}

fn macho_arch(macho: &MachOInfo) -> Result<ProfileArch, ProfileError> {
    if macho.universal {
        return Ok(ProfileArch::Universal);
    }
    match macho.slices.first() {
        Some(slice) => profile_arch(slice.arch),
        None => Err(ProfileError::NotLaunchable),
    }
}

/// A lower-case identifier made from a file name: letters and digits kept,
/// every run of anything else turned into one hyphen.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for character in text.chars() {
        if out.len() >= 64 {
            break;
        }
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "application".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// A display name made from a file name.
fn name_from(stem: &str) -> String {
    let name: String = stem
        .chars()
        .filter(|character| !character.is_control())
        .take(200)
        .collect();
    if name.trim().is_empty() {
        "Application".to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{AppBundleInfo, ExecutableSource, PlistStatus};
    use crate::host::{Component, HostFacts};
    use crate::inspect::inspect_bytes;
    use crate::recommend::recommend;
    use crate::testdata::{CPU_ARM64, CPU_X86_64, ElfSpec, PeSpec, macho_fat, macho_thin, zip};

    /// The example from specification §7.3.
    const SPEC_EXAMPLE: &str = r#"{
        "schema_version": 1,
        "app_id": "com.example.application",
        "display_name": "Example Application",
        "binary": "/path/to/application.exe",
        "format": "pe",
        "architecture": "x86_64",
        "runtime": "wine",
        "runtime_channel": "stable",
        "runtime_version": "pinned-by-release-manifest",
        "prefix_id": "example-application",
        "graphics_backend": "dxvk",
        "sync_backend": "auto",
        "isolation_profile": "desktop-standard",
        "known_requirements": ["win32", "com"],
        "fallback": "windows-vm-offer",
        "test_status": "untested"
    }"#;

    const SCHEMA: &str = include_str!("../../../schemas/app-profile.schema.json");

    fn example() -> Value {
        serde_json::from_str(SPEC_EXAMPLE).unwrap()
    }

    /// The example with one member replaced, or removed when `value` is `None`.
    fn example_with(key: &str, value: Option<Value>) -> String {
        let mut profile = example();
        let members = profile.as_object_mut().unwrap();
        match value {
            Some(value) => members.insert(key.to_owned(), value),
            None => members.remove(key),
        };
        profile.to_string()
    }

    fn full_host() -> HostFacts {
        HostFacts::new(Arch::X86_64, &Component::ALL)
    }

    fn draft_for(path: &str, bytes: &[u8]) -> Result<AppProfile, ProfileError> {
        let inspection = inspect_bytes(bytes).unwrap();
        let recommendation = recommend(&inspection, &full_host());
        draft_profile(Path::new(path), &inspection, &recommendation)
    }

    #[test]
    fn specification_example_is_valid() {
        let profile = parse_profile(SPEC_EXAMPLE).unwrap();
        assert_eq!(profile.schema_version, 1);
        assert_eq!(profile.app_id, "com.example.application");
        assert_eq!(profile.format, ProfileFormat::Pe);
        assert_eq!(profile.architecture, ProfileArch::X86_64);
        assert_eq!(profile.runtime, ProfileRuntime::Wine);
        assert_eq!(profile.runtime_channel, Some(RuntimeChannel::Stable));
        assert_eq!(profile.graphics_backend, Some(GraphicsBackend::Dxvk));
        assert_eq!(profile.sync_backend, Some(SyncBackend::Auto));
        assert_eq!(profile.known_requirements, ["win32", "com"]);
        assert_eq!(profile.fallback, Fallback::WindowsVmOffer);
        assert_eq!(profile.test_status, TestStatus::Untested);
    }

    #[test]
    fn profiles_round_trip_through_json() {
        let profile = parse_profile(SPEC_EXAMPLE).unwrap();
        let text = serde_json::to_string_pretty(&profile).unwrap();
        assert_eq!(parse_profile(&text).unwrap(), profile);
        assert_eq!(serde_json::to_value(&profile).unwrap(), example());

        // Optional fields that are absent stay absent.
        let minimal = r#"{
            "schema_version": 1,
            "app_id": "org.example.tool",
            "display_name": "Tool",
            "binary": "/usr/bin/tool",
            "format": "elf",
            "architecture": "x86_64",
            "runtime": "native",
            "isolation_profile": "desktop-standard",
            "fallback": "none",
            "test_status": "works"
        }"#;
        let profile = parse_profile(minimal).unwrap();
        assert_eq!(profile.prefix_id, None);
        assert!(profile.known_requirements.is_empty());
        let value = serde_json::to_value(&profile).unwrap();
        let expected: Value = serde_json::from_str(minimal).unwrap();
        assert_eq!(value, expected);
    }

    #[test]
    fn every_enumerated_value_round_trips() {
        for runtime in ProfileRuntime::ALL {
            let format = ProfileFormat::ALL
                .into_iter()
                .find(|format| format.runtimes().contains(&runtime))
                .unwrap();
            let mut profile = parse_profile(SPEC_EXAMPLE).unwrap();
            profile.runtime = runtime;
            profile.format = format;
            if !format.is_windows() {
                profile.fallback = Fallback::NoFallback;
            }
            let text = serde_json::to_string(&profile).unwrap();
            assert_eq!(parse_profile(&text).unwrap(), profile);
        }
    }

    #[test]
    fn shape_errors_are_rejected() {
        let cases = [
            example_with("unexpected", Some(Value::from(1))),
            example_with("runtime", None),
            example_with("schema_version", None),
            example_with("test_status", None),
            example_with("runtime", Some(Value::from("dosbox"))),
            example_with("format", Some(Value::from("exe"))),
            example_with("architecture", Some(Value::from("x86-64"))),
            example_with("fallback", Some(Value::from("reboot"))),
            example_with("test_status", Some(Value::from("perfect"))),
            example_with("sync_backend", Some(Value::from("none"))),
            example_with("schema_version", Some(Value::from("1"))),
            example_with("known_requirements", Some(Value::from("win32"))),
            "[]".to_owned(),
            "\"profile\"".to_owned(),
        ];
        for text in cases {
            let error = parse_profile(&text).unwrap_err();
            assert!(
                matches!(error, ProfileError::Shape(_)),
                "{text} gave {error:?}"
            );
            assert_eq!(error.code(), Code::ProfileSyntax);
        }
    }

    #[test]
    fn syntax_errors_and_oversized_input_are_rejected() {
        for text in ["", "{", "{\"schema_version\": 1,}", "not json"] {
            assert!(matches!(parse_profile(text), Err(ProfileError::Syntax(_))));
        }
        let huge = " ".repeat(MAX_PROFILE_BYTES + 1);
        assert_eq!(
            parse_profile(&huge),
            Err(ProfileError::TooLarge {
                bytes: MAX_PROFILE_BYTES + 1
            })
        );
        let deep = "[".repeat(100_000);
        assert!(parse_profile(&deep).is_err());
    }

    #[test]
    fn invalid_field_values_are_rejected_by_name() {
        let cases = [
            ("schema_version", Value::from(2)),
            ("schema_version", Value::from(0)),
            ("app_id", Value::from("Example")),
            ("app_id", Value::from("com.Example.App")),
            ("app_id", Value::from("com..app")),
            ("app_id", Value::from(".com.app")),
            ("app_id", Value::from("com.example.app with space")),
            ("display_name", Value::from("")),
            ("display_name", Value::from("bad\u{1b}[31mname")),
            ("display_name", Value::from("x".repeat(201))),
            ("binary", Value::from("relative/path.exe")),
            ("binary", Value::from("/")),
            ("binary", Value::from("/tmp/a\nb")),
            ("runtime_version", Value::from("")),
            ("runtime_version", Value::from("1.0 beta")),
            ("prefix_id", Value::from("Has Capitals")),
            ("prefix_id", Value::from("-leading-hyphen")),
            ("prefix_id", Value::from("p".repeat(65))),
            ("isolation_profile", Value::from("")),
            ("isolation_profile", Value::from("desktop_standard")),
            ("known_requirements", Value::from(vec!["win32", "win32"])),
            ("known_requirements", Value::from(vec!["Win32"])),
            ("known_requirements", Value::from(vec![""])),
            ("runtime", Value::from("darling")),
            ("runtime", Value::from("native")),
            ("format", Value::from("elf")),
        ];
        for (field, value) in cases {
            let text = example_with(field, Some(value));
            match parse_profile(&text) {
                Err(ProfileError::Invalid { field: named, .. }) => {
                    let expected = if field == "format" { "runtime" } else { field };
                    assert_eq!(named, expected, "{text}");
                }
                other => panic!("{text} gave {other:?}"),
            }
        }

        let too_many: Vec<String> = (0..=MAX_KNOWN_REQUIREMENTS)
            .map(|index| format!("req{index}"))
            .collect();
        let text = example_with("known_requirements", Some(Value::from(too_many)));
        assert!(matches!(
            parse_profile(&text),
            Err(ProfileError::Invalid { .. })
        ));
    }

    #[test]
    fn cross_field_rules() {
        // Wine needs a prefix.
        let error = parse_profile(&example_with("prefix_id", None)).unwrap_err();
        assert_eq!(
            error,
            invalid("prefix_id", "is required for the wine and proton runtimes")
        );
        assert_eq!(error.code(), Code::ProfileInvalid);

        // A Windows VM offer makes no sense for a Linux program.
        let mut profile = parse_profile(SPEC_EXAMPLE).unwrap();
        profile.format = ProfileFormat::Elf;
        profile.runtime = ProfileRuntime::Native;
        let error = profile.validate().unwrap_err();
        assert!(matches!(error, ProfileError::Invalid { ref field, .. } if field == "fallback"));

        // A full Windows VM hosts Windows software without a prefix.
        let mut profile = parse_profile(SPEC_EXAMPLE).unwrap();
        profile.runtime = ProfileRuntime::WindowsVm;
        profile.prefix_id = None;
        profile.fallback = Fallback::NoFallback;
        assert_eq!(profile.validate(), Ok(()));
    }

    #[test]
    fn null_is_rejected_rather_than_treated_as_absent() {
        let text = example_with("prefix_id", Some(Value::Null));
        match parse_profile(&text) {
            Err(ProfileError::Invalid { field, .. }) => assert_eq!(field, "prefix_id"),
            other => panic!("gave {other:?}"),
        }
    }

    #[test]
    fn keys_that_look_like_credentials_are_rejected() {
        for key in [
            "password",
            "wine_password",
            "Passwd",
            "api_key",
            "API-KEY",
            "apiKey",
            "auth_token",
            "TOKEN",
            "client_secret",
            "private_key",
            "aws_access_key_id",
            "credentials",
            "session_key",
            "bearer",
            "pass phrase",
        ] {
            let text = example_with(key, Some(Value::from("hunter2")));
            let error = parse_profile(&text).unwrap_err();
            assert_eq!(
                error,
                ProfileError::SecretKey {
                    key: key.to_owned()
                }
            );
            assert_eq!(error.code(), Code::ProfileSecret);
            assert!(!error.to_string().contains("hunter2"));
        }

        // Nested keys are found too, before the unknown-field check.
        let nested = serde_json::json!({ "DB_PASSWORD": "hunter2" });
        let text = example_with("environment", Some(nested));
        assert_eq!(
            parse_profile(&text),
            Err(ProfileError::SecretKey {
                key: "DB_PASSWORD".to_owned()
            })
        );
    }

    #[test]
    fn values_that_look_like_credentials_are_rejected() {
        let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----";
        let cases = [
            ("display_name", Value::from(pem)),
            (
                "display_name",
                Value::from("https://user:hunter2@example.com/app"),
            ),
            (
                "known_requirements",
                Value::from(vec!["smb://admin:hunter2@server/share"]),
            ),
        ];
        for (field, value) in cases {
            let text = example_with(field, Some(value));
            let error = parse_profile(&text).unwrap_err();
            assert_eq!(
                error,
                ProfileError::SecretValue {
                    field: field.to_owned()
                }
            );
            assert!(!error.to_string().contains("hunter2"));
            assert!(!error.to_string().contains("AAAA"));
        }
        // Ordinary values with similar punctuation are fine.
        assert!(!looks_like_secret_value("https://example.com/a@b:c"));
        assert!(!looks_like_secret_value("user@example.com"));
        assert!(!looks_like_secret_value("/opt/app/bin/run"));
    }

    #[test]
    fn legitimate_field_names_are_not_mistaken_for_credentials() {
        let value = serde_json::to_value(parse_profile(SPEC_EXAMPLE).unwrap()).unwrap();
        for key in value.as_object().unwrap().keys() {
            assert!(!looks_like_secret_key(key), "{key}");
        }
    }

    fn schema_strings(schema: &Value, pointer: &str) -> Vec<String> {
        schema
            .pointer(pointer)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{pointer} is not an array in the schema"))
            .iter()
            .map(|entry| entry.as_str().unwrap().to_owned())
            .collect()
    }

    fn wire_names<T: Serialize>(values: &[T]) -> Vec<String> {
        values
            .iter()
            .map(|value| {
                serde_json::to_value(value)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn schema_file_matches_the_rust_types() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["properties"]["schema_version"]["const"],
            PROFILE_SCHEMA_VERSION
        );

        assert_eq!(
            schema_strings(&schema, "/properties/format/enum"),
            wire_names(&ProfileFormat::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/properties/architecture/enum"),
            wire_names(&ProfileArch::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/properties/runtime/enum"),
            wire_names(&ProfileRuntime::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/properties/runtime_channel/enum"),
            wire_names(&RuntimeChannel::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/properties/graphics_backend/enum"),
            wire_names(&GraphicsBackend::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/properties/sync_backend/enum"),
            wire_names(&SyncBackend::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/properties/fallback/enum"),
            wire_names(&Fallback::ALL)
        );
        assert_eq!(
            schema_strings(&schema, "/properties/test_status/enum"),
            wire_names(&TestStatus::ALL)
        );
        assert_eq!(
            schema["properties"]["known_requirements"]["maxItems"],
            MAX_KNOWN_REQUIREMENTS
        );

        // The schema's properties are exactly the fields of the type.
        let full = serde_json::to_value(parse_profile(SPEC_EXAMPLE).unwrap()).unwrap();
        let fields: Vec<&String> = full.as_object().unwrap().keys().collect();
        let properties: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
        assert_eq!(properties, fields);

        // The schema's required list is exactly the fields that are not optional.
        let minimal = AppProfile {
            runtime_channel: None,
            runtime_version: None,
            prefix_id: None,
            graphics_backend: None,
            sync_backend: None,
            known_requirements: Vec::new(),
            ..parse_profile(SPEC_EXAMPLE).unwrap()
        };
        let minimal = serde_json::to_value(minimal).unwrap();
        let mut needed: Vec<String> = minimal.as_object().unwrap().keys().cloned().collect();
        let mut required = schema_strings(&schema, "/required");
        needed.sort();
        required.sort();
        assert_eq!(required, needed);
    }

    #[test]
    fn schema_uses_the_same_wire_names_as_the_validator_for_runtimes() {
        assert_eq!(
            wire_names(&ProfileRuntime::ALL),
            [
                "native",
                "wine",
                "proton",
                "darling",
                "android",
                "windows-vm"
            ]
        );
        assert_eq!(
            wire_names(&ProfileFormat::ALL),
            [
                "elf",
                "script",
                "pe",
                "msi",
                "mach-o",
                "app-bundle",
                "apk",
                "apk-set"
            ]
        );
        assert_eq!(
            wire_names(&GraphicsBackend::ALL),
            ["auto", "dxvk", "vkd3d-proton", "wined3d", "native"]
        );
    }

    #[test]
    fn draft_for_a_windows_program() {
        let profile = draft_for(
            "/home/user/Downloads/Setup Wizard (x64).exe",
            &PeSpec::windows_gui_x86_64().build(),
        )
        .unwrap();
        assert_eq!(profile.schema_version, PROFILE_SCHEMA_VERSION);
        assert_eq!(profile.app_id, "local.setup-wizard-x64");
        assert_eq!(profile.display_name, "Setup Wizard (x64)");
        assert_eq!(
            profile.binary,
            "/home/user/Downloads/Setup Wizard (x64).exe"
        );
        assert_eq!(profile.format, ProfileFormat::Pe);
        assert_eq!(profile.architecture, ProfileArch::X86_64);
        assert_eq!(profile.runtime, ProfileRuntime::Wine);
        assert_eq!(profile.runtime_channel, Some(RuntimeChannel::Stable));
        assert_eq!(profile.runtime_version, None);
        assert_eq!(profile.prefix_id.as_deref(), Some("setup-wizard-x64"));
        assert_eq!(profile.graphics_backend, Some(GraphicsBackend::Auto));
        assert_eq!(profile.sync_backend, Some(SyncBackend::Auto));
        assert_eq!(profile.isolation_profile, "desktop-standard");
        assert!(profile.known_requirements.is_empty());
        assert_eq!(profile.fallback, Fallback::WindowsVmOffer);
        assert_eq!(profile.test_status, TestStatus::Untested);

        // A draft is itself a valid profile document.
        let text = serde_json::to_string(&profile).unwrap();
        assert_eq!(parse_profile(&text).unwrap(), profile);
    }

    #[test]
    fn draft_records_known_requirements() {
        let dotnet = PeSpec {
            clr: true,
            ..PeSpec::windows_gui_x86_64()
        };
        let profile = draft_for("/apps/tool.exe", &dotnet.build()).unwrap();
        assert_eq!(profile.known_requirements, ["dotnet"]);

        let driver = PeSpec {
            subsystem: 1,
            ..PeSpec::windows_gui_x86_64()
        };
        let profile = draft_for("/apps/filter.sys", &driver.build()).unwrap();
        assert_eq!(profile.runtime, ProfileRuntime::WindowsVm);
        assert_eq!(profile.known_requirements, ["windows-kernel"]);
        assert_eq!(profile.prefix_id, None);
        assert_eq!(profile.fallback, Fallback::NoFallback);

        let arm_apk = zip(&[
            "AndroidManifest.xml",
            "classes.dex",
            "lib/arm64-v8a/libapp.so",
        ]);
        let profile = draft_for("/apps/game.apk", &arm_apk).unwrap();
        assert_eq!(profile.format, ProfileFormat::Apk);
        assert_eq!(profile.architecture, ProfileArch::Any);
        assert_eq!(profile.runtime, ProfileRuntime::Android);
        assert_eq!(profile.known_requirements, ["arm-translation"]);
    }

    #[test]
    fn draft_for_native_and_macos_programs() {
        let profile = draft_for("/usr/local/bin/tool", &ElfSpec::linux_x86_64().build()).unwrap();
        assert_eq!(profile.format, ProfileFormat::Elf);
        assert_eq!(profile.runtime, ProfileRuntime::Native);
        assert_eq!(profile.app_id, "local.tool");
        assert_eq!(profile.prefix_id, None);
        assert_eq!(profile.graphics_backend, None);
        assert_eq!(profile.fallback, Fallback::NoFallback);

        let profile = draft_for("/apps/run.sh", b"#!/bin/sh\n").unwrap();
        assert_eq!(profile.format, ProfileFormat::Script);
        assert_eq!(profile.architecture, ProfileArch::Any);

        let thin = macho_thin(CPU_X86_64, 2, true, false);
        let profile = draft_for("/apps/mactool", &thin).unwrap();
        assert_eq!(profile.format, ProfileFormat::MachO);
        assert_eq!(profile.architecture, ProfileArch::X86_64);
        assert_eq!(profile.runtime, ProfileRuntime::Darling);
        assert_eq!(profile.runtime_channel, Some(RuntimeChannel::Experimental));

        let universal = macho_fat(&[(CPU_X86_64, 2), (CPU_ARM64, 2)], false);
        let profile = draft_for("/apps/mactool", &universal).unwrap();
        assert_eq!(profile.architecture, ProfileArch::Universal);
    }

    #[test]
    fn draft_for_an_app_bundle_uses_its_identifier_and_name() {
        let program = inspect_bytes(&macho_thin(CPU_X86_64, 2, true, false)).unwrap();
        let inspection = Inspection {
            file_size: 0,
            detected: Detected::MacAppBundle(AppBundleInfo {
                plist: PlistStatus::Parsed,
                identifier: Some("com.Example.Demo".to_owned()),
                name: Some("Demo App".to_owned()),
                version: Some("1.0".to_owned()),
                executable_name: Some("Demo".to_owned()),
                executable_source: Some(ExecutableSource::InfoPlist),
                executable: Some(Box::new(program)),
            }),
            notes: Vec::new(),
        };
        let recommendation = recommend(&inspection, &full_host());
        let profile =
            draft_profile(Path::new("/apps/Demo.app"), &inspection, &recommendation).unwrap();
        assert_eq!(profile.app_id, "com.example.demo");
        assert_eq!(profile.display_name, "Demo App");
        assert_eq!(profile.format, ProfileFormat::AppBundle);
        assert_eq!(profile.architecture, ProfileArch::X86_64);
        assert_eq!(profile.runtime, ProfileRuntime::Darling);
    }

    #[test]
    fn nothing_is_drafted_without_a_runtime() {
        assert_eq!(
            draft_for("/apps/unknown.bin", b"plain text"),
            Err(ProfileError::NotLaunchable)
        );
        let arm = macho_thin(CPU_ARM64, 2, true, false);
        assert_eq!(
            draft_for("/apps/armtool", &arm),
            Err(ProfileError::NotLaunchable)
        );
        assert_eq!(ProfileError::NotLaunchable.code(), Code::NoRoute);
    }

    #[test]
    fn draft_needs_an_absolute_path() {
        let error = draft_for("relative/tool", &ElfSpec::linux_x86_64().build()).unwrap_err();
        assert!(matches!(error, ProfileError::Invalid { ref field, .. } if field == "binary"));
    }

    #[test]
    fn slugs_and_names_from_awkward_file_names() {
        assert_eq!(slug("Setup Wizard (x64)"), "setup-wizard-x64");
        assert_eq!(slug("__init__"), "init");
        assert_eq!(slug("…"), "application");
        assert_eq!(slug(""), "application");
        assert_eq!(slug(&"a".repeat(500)).len(), 64);
        assert!(is_slug(&slug(&"-x-".repeat(100))));
        assert_eq!(name_from("  "), "Application");
        assert_eq!(name_from("a\u{7}b"), "ab");

        let profile = draft_for("/apps/….exe", &PeSpec::windows_gui_x86_64().build()).unwrap();
        assert_eq!(profile.app_id, "local.application");
        assert_eq!(profile.prefix_id.as_deref(), Some("application"));
    }
}
