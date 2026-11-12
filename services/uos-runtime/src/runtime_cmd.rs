// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The command that starts a runtime: Wine directly, or Proton through
//! `umu-run`, always inside the sandbox.
//!
//! The environment is built from nothing. What goes into it:
//!
//! * a fixed `PATH`, the application's private `HOME`, and the user's name;
//! * the locale, time-zone and GPU-selection variables of
//!   [`crate::launch_host::PASSTHROUGH`], when set;
//! * the display, authority and runtime-directory settings, rebuilt from the
//!   session endpoints that were found and checked;
//! * the runtime's own variables (`WINEPREFIX`, `WINEDLLOVERRIDES`,
//!   `WINEDEBUG`, or `GAMEID`, `PROTONPATH`, ...), as upstream documents
//!   them;
//! * variables the user adds to one launch by name.
//!
//! Nothing else of the caller's environment is passed on.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::codes::Code;
use crate::grant::{GrantContext, GrantRefusal};
use crate::launch_host::LaunchHost;
use crate::limits::{MAX_ENVIRONMENT_VALUE_BYTES, MAX_EXTRA_ENVIRONMENT, MAX_LAUNCH_ARGUMENTS};
use crate::metadata::{
    Grant, MetadataError, NtsyncDevice, PrefixArch, RuntimeKind, SyncMode, SyncRecord,
};
use crate::paths::{AppPaths, PathsError};
use crate::plan::CommandSpec;
use crate::profile::{GraphicsBackend, SyncBackend};
use crate::sandbox::{self, Mount, SUPERVISOR_PATH, SandboxError, SandboxRequest, XAUTHORITY_PATH};
use crate::timefmt::Timestamp;

/// The `PATH` every runtime is started with.
pub const RUNTIME_PATH: &str = "/usr/bin:/bin";

/// The `WINEDEBUG` value used unless the caller names another: errors are
/// kept, "fixme" notices are dropped.
pub const DEFAULT_WINE_DEBUG: &str = "fixme-all";

/// Variables this module sets itself. A launch cannot override them.
const RESERVED: [&str; 19] = [
    "HOME",
    "PATH",
    "USER",
    "LOGNAME",
    "WINEPREFIX",
    "WINEARCH",
    "WINEDLLOVERRIDES",
    "WINEDEBUG",
    "WINESERVER",
    "WINELOADER",
    "WINEDLLPATH",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "GAMEID",
    "STORE",
    "PROTONPATH",
    "PROTON_VERB",
];

/// Why a prefix operation or a launch cannot be planned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The effective user is root.
    RunningAsRoot,
    /// The sandbox cannot be built.
    Sandbox(SandboxError),
    /// The runtime, or a program it needs, is not installed.
    RuntimeMissing {
        /// The runtime.
        runtime: RuntimeKind,
        /// The program that was not found.
        program: &'static str,
    },
    /// A program needed for the operation is not installed.
    ToolMissing {
        /// The program.
        program: &'static str,
    },
    /// The runtime's version could not be determined.
    VersionUnknown,
    /// The profile asks for something this build does not launch.
    Unsupported {
        /// What, in plain language.
        reason: String,
    },
    /// The application has no prefix.
    PrefixMissing,
    /// The application already has a prefix.
    PrefixExists,
    /// The prefix is recorded as belonging to something else.
    PrefixOwner {
        /// What differs.
        detail: String,
    },
    /// The profile pins a runtime version that is not the one installed.
    PinMismatch {
        /// The version the profile pins.
        pinned: String,
        /// The version that is installed.
        installed: String,
    },
    /// The prefix is on a runtime version that is not the one installed.
    RuntimeMismatch {
        /// The version the prefix is on.
        recorded: String,
        /// The version that is installed.
        installed: String,
    },
    /// A migration was asked for and the prefix is pinned.
    Pinned {
        /// The version the prefix is pinned to.
        version: String,
    },
    /// A Proton profile does not pin a version.
    ProtonUnpinned,
    /// A directory cannot be granted.
    Grant {
        /// The directory as requested.
        path: String,
        /// Why.
        refusal: GrantRefusal,
    },
    /// The application's program file cannot be used.
    Program {
        /// The file as named by the profile.
        path: String,
        /// Why.
        refusal: GrantRefusal,
    },
    /// An extra environment variable is not allowed.
    Environment {
        /// The variable's name. The value is never reported.
        name: String,
        /// What is wrong.
        problem: &'static str,
    },
    /// The arguments for the application are not acceptable.
    Arguments {
        /// What is wrong.
        problem: &'static str,
    },
    /// The named backup does not exist, or the name is not a backup name.
    BackupMissing {
        /// The name.
        name: String,
    },
    /// The prefix metadata is not valid.
    Metadata(MetadataError),
    /// The runtime manager's directories cannot be used.
    Paths(PathsError),
}

impl PlanError {
    /// The stable error code for this failure.
    pub fn code(&self) -> Code {
        match self {
            PlanError::RunningAsRoot => Code::RunningAsRoot,
            PlanError::Sandbox(error) => error.code(),
            PlanError::RuntimeMissing {
                runtime: RuntimeKind::Wine,
                ..
            } => Code::WineMissing,
            PlanError::RuntimeMissing {
                runtime: RuntimeKind::Proton,
                ..
            } => Code::ProtonLauncherMissing,
            PlanError::ToolMissing { .. } => Code::ToolMissing,
            PlanError::VersionUnknown => Code::RuntimeVersionUnknown,
            PlanError::Unsupported { .. } | PlanError::Arguments { .. } => Code::LaunchUnsupported,
            PlanError::PrefixMissing => Code::PrefixMissing,
            PlanError::PrefixExists => Code::PrefixExists,
            PlanError::PrefixOwner { .. } => Code::PrefixOwner,
            PlanError::PinMismatch { .. } => Code::RuntimePinMismatch,
            PlanError::RuntimeMismatch { .. } => Code::PrefixRuntimeMismatch,
            PlanError::Pinned { .. } => Code::PrefixPinned,
            PlanError::ProtonUnpinned => Code::ProtonUnpinned,
            PlanError::Grant { .. } => Code::GrantRefused,
            PlanError::Program { .. } => Code::BinaryRefused,
            PlanError::Environment { .. } => Code::EnvironmentRefused,
            PlanError::BackupMissing { .. } => Code::BackupMissing,
            PlanError::Metadata(error) => error.code(),
            PlanError::Paths(PathsError::BadAppId) => Code::ProfileInvalid,
            PlanError::Paths(_) => Code::StorageUnusable,
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::RunningAsRoot => f.write_str(
                "refusing to run as root: Windows software is only ever started as an ordinary \
                 user",
            ),
            PlanError::Sandbox(error) => write!(f, "{error}"),
            PlanError::RuntimeMissing { runtime, program } => write!(
                f,
                "{} cannot be started: {program} was not found in /usr/bin, /usr/local/bin or /bin",
                runtime.label()
            ),
            PlanError::ToolMissing { program } => write!(
                f,
                "{program} is needed for this operation and was not found in /usr/bin, \
                 /usr/local/bin or /bin"
            ),
            PlanError::VersionUnknown => f.write_str(
                "the runtime did not report a version that can be recorded, so the prefix cannot \
                 be tied to it",
            ),
            PlanError::Unsupported { reason } => write!(f, "this profile cannot be launched: {reason}"),
            PlanError::PrefixMissing => f.write_str("the application has no prefix"),
            PlanError::PrefixExists => f.write_str("the application already has a prefix"),
            PlanError::PrefixOwner { detail } => {
                write!(f, "the prefix does not belong to this profile: {detail}")
            }
            PlanError::PinMismatch { pinned, installed } => write!(
                f,
                "the profile pins runtime version {pinned} and the installed runtime is \
                 {installed}; nothing was started"
            ),
            PlanError::RuntimeMismatch { recorded, installed } => write!(
                f,
                "the prefix is on runtime version {recorded} and the installed runtime is \
                 {installed}; a prefix is never changed by a runtime upgrade on its own. Ask for \
                 a migration, which takes a backup first, or install {recorded}"
            ),
            PlanError::Pinned { version } => write!(
                f,
                "the prefix is pinned to runtime version {version} and will not be migrated; \
                 unpin it first"
            ),
            PlanError::ProtonUnpinned => f.write_str(
                "a Proton profile must pin a Proton version (runtime_version): without one the \
                 launcher follows the latest build, which would change the prefix without being \
                 asked",
            ),
            PlanError::Grant { path, refusal } => {
                write!(f, "{path:?} cannot be granted to an application: {refusal}")
            }
            PlanError::Program { path, refusal } => {
                write!(f, "the program file {path:?} cannot be used: {refusal}")
            }
            PlanError::Environment { name, problem } => {
                write!(f, "the environment variable {name:?} cannot be set: {problem}")
            }
            PlanError::Arguments { problem } => {
                write!(f, "the arguments for the application cannot be used: {problem}")
            }
            PlanError::BackupMissing { name } => {
                write!(f, "there is no backup named {name:?} for this application")
            }
            PlanError::Metadata(error) => write!(f, "{error}"),
            PlanError::Paths(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for PlanError {}

impl From<SandboxError> for PlanError {
    fn from(error: SandboxError) -> Self {
        PlanError::Sandbox(error)
    }
}

impl From<MetadataError> for PlanError {
    fn from(error: MetadataError) -> Self {
        PlanError::Metadata(error)
    }
}

impl From<PathsError> for PlanError {
    fn from(error: PathsError) -> Self {
        PlanError::Paths(error)
    }
}

/// Whether a Wine version uses the `ntsync` driver when it is available.
///
/// Upstream Wine does from 11.0 on ("The NTSync Linux kernel module is used
/// when available", Wine 11.0 release notes). Earlier stable releases do not.
pub fn wine_has_ntsync(version: &str) -> bool {
    version
        .strip_prefix("wine-")
        .and_then(|rest| rest.split(|character: char| !character.is_ascii_digit()).next())
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 11)
}

/// Decides the synchronisation implementation for one launch and records why
/// (specification §8.2).
///
/// The decision rests on what the device probe found and on whether the
/// runtime can use the device, never on a kernel version. When `ntsync`
/// cannot be used the launch falls back and says so; it does not fail.
pub fn decide_sync(
    requested: SyncBackend,
    runtime: RuntimeKind,
    runtime_version: &str,
    device: NtsyncDevice,
    now: Timestamp,
) -> SyncRecord {
    let device_problem = match device {
        NtsyncDevice::Present => None,
        NtsyncDevice::Absent => Some("there is no /dev/ntsync device on this system"),
        NtsyncDevice::Inaccessible => Some("/dev/ntsync exists and could not be opened"),
        NtsyncDevice::NotChecked => Some("the ntsync device was not looked for"),
    };
    let (selected, reason) = match (runtime, requested) {
        (RuntimeKind::Wine, SyncBackend::Auto | SyncBackend::Ntsync) => {
            match (device_problem, wine_has_ntsync(runtime_version)) {
                (None, true) => (
                    SyncMode::Ntsync,
                    format!(
                        "/dev/ntsync was opened and {runtime_version} uses it when it is \
                         available."
                    ),
                ),
                (None, false) => (
                    SyncMode::Server,
                    format!(
                        "/dev/ntsync is available, but {runtime_version} is older than Wine \
                         11.0, the first release that uses it; falling back to the Wine server."
                    ),
                ),
                (Some(problem), _) => (
                    SyncMode::Server,
                    format!("Falling back to the Wine server: {problem}."),
                ),
            }
        }
        (RuntimeKind::Wine, SyncBackend::Esync | SyncBackend::Fsync) => (
            SyncMode::Server,
            "esync and fsync are not part of upstream Wine; using the Wine server.".to_owned(),
        ),
        (RuntimeKind::Proton, SyncBackend::Auto) => (
            SyncMode::RuntimeDefault,
            "The choice is left to the Proton build.".to_owned(),
        ),
        (RuntimeKind::Proton, SyncBackend::Ntsync) => match device_problem {
            None => (
                SyncMode::Ntsync,
                "/dev/ntsync was opened; PROTON_USE_NTSYNC=1 asks the Proton build to use it."
                    .to_owned(),
            ),
            Some(problem) => (
                SyncMode::RuntimeDefault,
                format!("Leaving the choice to the Proton build: {problem}."),
            ),
        },
        (RuntimeKind::Proton, SyncBackend::Esync) => (
            SyncMode::Esync,
            "PROTON_NO_FSYNC=1 leaves esync as Proton's implementation.".to_owned(),
        ),
        (RuntimeKind::Proton, SyncBackend::Fsync) => (
            SyncMode::Fsync,
            "fsync is Proton's default where the kernel supports it; nothing is overridden."
                .to_owned(),
        ),
    };
    SyncRecord {
        requested,
        selected,
        ntsync_device: device,
        checked_at: Some(now),
        reason,
    }
}

/// The value of `WINEDLLOVERRIDES` for a plain-Wine launch.
///
/// * DXVK and VKD3D-Proton are native DLLs placed in the prefix. Their
///   upstream instructions ask for native overrides; `n,b` is used so that
///   Wine's own implementation is the fallback when the DLLs are absent,
///   because installing them into a prefix is not built yet.
/// * `winemenubuilder.exe` is disabled: desktop entries are generated by
///   `uos-runtime`, not written by Wine.
/// * While a prefix is created, `mscoree` and `mshtml` are disabled so that
///   Wine does not offer to download Mono and Gecko.
pub fn dll_overrides(graphics: GraphicsBackend, creating: bool) -> String {
    let mut entries = Vec::new();
    match graphics {
        GraphicsBackend::Dxvk => entries.push("d3d8,d3d9,d3d10core,d3d11,dxgi=n,b"),
        GraphicsBackend::Vkd3dProton => entries.push("d3d12,d3d12core=n,b"),
        GraphicsBackend::Auto | GraphicsBackend::Wined3d | GraphicsBackend::Native => {}
    }
    if creating {
        entries.push("mscoree,mshtml=");
    }
    entries.push("winemenubuilder.exe=");
    entries.join(";")
}

/// Checks the extra environment variables of one launch.
///
/// # Errors
///
/// [`PlanError::Environment`] naming the first variable that is not allowed.
pub fn check_extra_environment(extra: &[(String, String)]) -> Result<(), PlanError> {
    let refuse = |name: &str, problem: &'static str| {
        Err(PlanError::Environment {
            name: name.chars().filter(|c| !c.is_control()).take(64).collect(),
            problem,
        })
    };
    if extra.len() > MAX_EXTRA_ENVIRONMENT {
        return refuse("", "too many variables were given");
    }
    for (index, (name, value)) in extra.iter().enumerate() {
        let mut bytes = name.bytes();
        let well_formed = name.len() <= 128
            && bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        if !well_formed {
            return refuse(name, "the name must be letters, digits and underscores");
        }
        if RESERVED.contains(&name.as_str()) {
            return refuse(name, "the runtime manager sets this variable itself");
        }
        if name.starts_with("LD_") {
            return refuse(name, "variables that change how programs are loaded are not accepted");
        }
        if value.len() > MAX_ENVIRONMENT_VALUE_BYTES || value.chars().any(char::is_control) {
            return refuse(name, "the value is too long or holds a control character");
        }
        let earlier = extra.get(..index).unwrap_or(&[]);
        if earlier.iter().any(|(other, _)| other == name) {
            return refuse(name, "the variable is given twice");
        }
    }
    Ok(())
}

/// Checks the arguments passed on to the application.
///
/// # Errors
///
/// [`PlanError::Arguments`] when there are too many or one holds a NUL byte.
pub fn check_arguments(arguments: &[String]) -> Result<(), PlanError> {
    if arguments.len() > MAX_LAUNCH_ARGUMENTS {
        return Err(PlanError::Arguments {
            problem: "there are too many",
        });
    }
    if arguments.iter().any(|argument| argument.contains('\0')) {
        return Err(PlanError::Arguments {
            problem: "one of them holds a NUL byte",
        });
    }
    Ok(())
}

/// What the runtime is started to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task<'a> {
    /// Create the prefix (`wineboot --init`, or `umu-run ""`).
    Initialise,
    /// Bring the prefix's system files up to the installed runtime
    /// (`wineboot --update`, or `umu-run ""`).
    Update,
    /// Run the application.
    Application {
        /// The program file, as the application sees it.
        program: &'a Path,
        /// Whether the file lies outside the prefix and has to be shown.
        outside_prefix: bool,
        /// Arguments passed on to the program.
        arguments: &'a [String],
    },
}

/// Everything a runtime command is assembled from.
pub struct Invocation<'a> {
    /// The machine and session.
    pub host: &'a LaunchHost,
    /// The application's own paths.
    pub app: &'a AppPaths,
    /// The grant rules for this user.
    pub context: &'a GrantContext,
    /// Wine or Proton.
    pub runtime: RuntimeKind,
    /// The runtime version; for Proton it selects the build.
    pub runtime_version: &'a str,
    /// The architecture of the prefix.
    pub architecture: PrefixArch,
    /// The Direct3D translation setting.
    pub graphics: GraphicsBackend,
    /// The synchronisation decision.
    pub sync: &'a SyncRecord,
    /// The recorded grants.
    pub grants: &'a [Grant],
    /// Whether the host's network is shared.
    pub network: bool,
    /// The value of `WINEDEBUG`.
    pub wine_debug: &'a str,
    /// Variables added to this launch by the user.
    pub extra_environment: &'a [(String, String)],
}

/// An assembled runtime command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    /// The command as the application's side of the sandbox sees it:
    /// program, arguments, environment, working directory.
    pub application: CommandSpec,
    /// The command that is actually started: bubblewrap, the supervisor where
    /// one is needed, then the application command.
    pub sandboxed: CommandSpec,
    /// The sandbox's file-system view.
    pub mounts: Vec<Mount>,
}

fn path_text(path: &Path, role: &'static str) -> Result<String, PlanError> {
    crate::paths::plain_text(path)
        .map(str::to_owned)
        .ok_or(PlanError::Sandbox(SandboxError::Path { role }))
}

/// The environment of a runtime command.
fn environment(invocation: &Invocation<'_>, creating: bool) -> Result<BTreeMap<String, String>, PlanError> {
    let host = invocation.host;
    let mut environment: BTreeMap<String, String> = host.passthrough.clone();
    let mut set = |name: &str, value: String| {
        environment.insert(name.to_owned(), value);
    };
    set("PATH", RUNTIME_PATH.to_owned());
    set("HOME", path_text(&invocation.app.home, "the private home directory")?);
    set("USER", host.user.clone());
    set("LOGNAME", host.user.clone());
    set("WINEPREFIX", path_text(&invocation.app.prefix, "the prefix")?);

    if let Some(x11) = &host.sockets.x11 {
        set("DISPLAY", x11.display.clone());
        if host.sockets.xauthority.is_some() {
            set("XAUTHORITY", XAUTHORITY_PATH.to_owned());
        }
    }
    if let Some(runtime_dir) = &host.runtime_dir {
        set("XDG_RUNTIME_DIR", path_text(runtime_dir, "the runtime directory")?);
        if let Some(wayland) = &host.sockets.wayland {
            set("WAYLAND_DISPLAY", wayland.name.clone());
        }
    }

    match invocation.runtime {
        RuntimeKind::Wine => {
            set("WINEDEBUG", invocation.wine_debug.to_owned());
            set("WINEDLLOVERRIDES", dll_overrides(invocation.graphics, creating));
            if creating {
                set("WINEARCH", invocation.architecture.winearch().to_owned());
            }
        }
        RuntimeKind::Proton => {
            // No application database yet, so no umu-database identifier is
            // known: "umu-default" applies no fixes.
            set("GAMEID", "umu-default".to_owned());
            set("STORE", "none".to_owned());
            set("PROTONPATH", invocation.runtime_version.to_owned());
            set("PROTON_VERB", "waitforexitandrun".to_owned());
            if invocation.graphics == GraphicsBackend::Wined3d {
                set("PROTON_USE_WINED3D", "1".to_owned());
            }
            match (invocation.sync.requested, invocation.sync.selected) {
                (SyncBackend::Ntsync, SyncMode::Ntsync) => {
                    set("PROTON_USE_NTSYNC", "1".to_owned());
                }
                (SyncBackend::Esync, _) => set("PROTON_NO_FSYNC", "1".to_owned()),
                _ => {}
            }
        }
    }
    for (name, value) in invocation.extra_environment {
        set(name, value.clone());
    }
    Ok(environment)
}

/// Assembles the command for `task`, inside the sandbox.
///
/// # Errors
///
/// [`PlanError`] when the user is root, the runtime or the sandbox is not
/// installed, an extra variable is not allowed, or a grant is refused.
pub fn assemble(invocation: &Invocation<'_>, task: Task<'_>) -> Result<Assembled, PlanError> {
    let host = invocation.host;
    if host.euid == 0 {
        return Err(PlanError::RunningAsRoot);
    }
    check_extra_environment(invocation.extra_environment)?;

    let creating = task == Task::Initialise;
    let environment = environment(invocation, creating)?;

    // The command on the application's side of the sandbox.
    let (program, mut arguments, supervisor) = match invocation.runtime {
        RuntimeKind::Wine => {
            let missing = |program| PlanError::RuntimeMissing {
                runtime: RuntimeKind::Wine,
                program,
            };
            let wine = host.wine.as_ref().ok_or_else(|| missing("wine"))?;
            let server = wine.server.as_deref().ok_or_else(|| missing("wineserver"))?;
            (
                path_text(&wine.program, "wine")?,
                Vec::new(),
                Some(path_text(server, "wineserver")?),
            )
        }
        RuntimeKind::Proton => {
            let launcher = host.umu_run.as_deref().ok_or(PlanError::RuntimeMissing {
                runtime: RuntimeKind::Proton,
                program: "umu-run",
            })?;
            (path_text(launcher, "umu-run")?, Vec::new(), None)
        }
    };
    let mut program_file = None;
    let mut working_directory = invocation.app.prefix.as_path();
    match (task, invocation.runtime) {
        (Task::Initialise, RuntimeKind::Wine) => {
            arguments.extend(["wineboot".to_owned(), "--init".to_owned()]);
        }
        (Task::Update, RuntimeKind::Wine) => {
            arguments.extend(["wineboot".to_owned(), "--update".to_owned()]);
        }
        // An empty program makes umu-run set the prefix up and stop.
        (Task::Initialise | Task::Update, RuntimeKind::Proton) => arguments.push(String::new()),
        (
            Task::Application {
                program: file,
                outside_prefix,
                arguments: passed,
            },
            _,
        ) => {
            check_arguments(passed)?;
            arguments.push(path_text(file, "the program file")?);
            arguments.extend(passed.iter().cloned());
            if outside_prefix {
                program_file = Some(file);
            }
            if let Some(parent) = file.parent() {
                working_directory = parent;
            }
        }
    }

    let sandbox = sandbox::build(&SandboxRequest {
        host,
        app: invocation.app,
        context: invocation.context,
        grants: invocation.grants,
        network: invocation.network,
        // umu-run starts Proton inside the Steam Linux Runtime container,
        // which needs user namespaces of its own.
        nested_namespaces: invocation.runtime == RuntimeKind::Proton,
        program_file,
        supervisor: supervisor.is_some(),
        working_directory,
    })?;

    let working_directory = path_text(working_directory, "the working directory")?;
    let application = CommandSpec {
        program: program.clone(),
        arguments: arguments.clone(),
        environment: environment.clone(),
        working_directory: Some(working_directory),
    };

    let mut outer = sandbox.arguments;
    if let Some(server) = supervisor {
        // Wine's server outlives the program by a few seconds and writes the
        // registry when it exits. The sandbox ends when its first process
        // does, so that process is the supervisor, which waits for the
        // server before it exits.
        outer.extend([
            SUPERVISOR_PATH.to_owned(),
            "supervise".to_owned(),
            "--settle".to_owned(),
            server,
            "--".to_owned(),
        ]);
    }
    outer.push(program);
    outer.extend(arguments);
    let sandboxed = CommandSpec {
        program: sandbox.program,
        arguments: outer,
        environment,
        // bubblewrap changes to the working directory inside the sandbox
        // (--chdir); outside it, the directory may not even exist yet.
        working_directory: None,
    };
    Ok(Assembled {
        application,
        sandboxed,
        mounts: sandbox.mounts,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::grant::tests::Machine;
    use crate::launch_host::tests::fixture;
    use crate::sandbox::tests::{assert_boundary, parse};

    pub(crate) const NOW: Timestamp = Timestamp::from_unix(1_794_478_500);

    fn sync(requested: SyncBackend, runtime: RuntimeKind, version: &str, device: NtsyncDevice) -> SyncRecord {
        decide_sync(requested, runtime, version, device, NOW)
    }

    #[test]
    fn ntsync_is_used_only_when_the_device_opens_and_the_runtime_can_use_it() {
        use NtsyncDevice::{Absent, Inaccessible, NotChecked, Present};
        use RuntimeKind::{Proton, Wine};
        use SyncBackend::{Auto, Esync, Fsync, Ntsync};

        let cases = [
            (Auto, Wine, "wine-11.0", Present, SyncMode::Ntsync),
            (Ntsync, Wine, "wine-11.0", Present, SyncMode::Ntsync),
            (Auto, Wine, "wine-12.3", Present, SyncMode::Ntsync),
            // The device is there; this Wine cannot use it.
            (Auto, Wine, "wine-9.0", Present, SyncMode::Server),
            (Ntsync, Wine, "wine-10.0", Present, SyncMode::Server),
            // This Wine could use it; the device is not there.
            (Auto, Wine, "wine-11.0", Absent, SyncMode::Server),
            (Ntsync, Wine, "wine-11.0", Absent, SyncMode::Server),
            (Ntsync, Wine, "wine-11.0", Inaccessible, SyncMode::Server),
            (Ntsync, Wine, "wine-11.0", NotChecked, SyncMode::Server),
            (Esync, Wine, "wine-11.0", Present, SyncMode::Server),
            (Fsync, Wine, "wine-11.0", Present, SyncMode::Server),
            (Auto, Proton, "GE-Proton10-20", Present, SyncMode::RuntimeDefault),
            (Auto, Proton, "GE-Proton10-20", Absent, SyncMode::RuntimeDefault),
            (Ntsync, Proton, "GE-Proton10-20", Present, SyncMode::Ntsync),
            (Ntsync, Proton, "GE-Proton10-20", Absent, SyncMode::RuntimeDefault),
            (Esync, Proton, "GE-Proton10-20", Absent, SyncMode::Esync),
            (Fsync, Proton, "GE-Proton10-20", Absent, SyncMode::Fsync),
        ];
        for (requested, runtime, version, device, expected) in cases {
            let record = sync(requested, runtime, version, device);
            assert_eq!(
                record.selected, expected,
                "{requested:?} {runtime:?} {version} {device:?}"
            );
            // The probe result and the reason are always recorded.
            assert_eq!(record.requested, requested);
            assert_eq!(record.ntsync_device, device);
            assert_eq!(record.checked_at, Some(NOW));
            assert!(!record.reason.is_empty());
            assert!(record.reason.len() <= 500);
        }
        let fallback = sync(Ntsync, Wine, "wine-11.0", Absent);
        assert!(fallback.reason.contains("no /dev/ntsync device"));
        let old = sync(Auto, Wine, "wine-9.0", Present);
        assert!(old.reason.contains("older than Wine 11.0"));
    }

    #[test]
    fn wine_versions_that_use_ntsync() {
        for version in ["wine-11.0", "wine-11.0-rc1", "wine-11.5", "wine-12.0", "wine-100.1"] {
            assert!(wine_has_ntsync(version), "{version}");
        }
        for version in [
            "wine-9.0",
            "wine-10.0",
            "wine-10.20",
            "wine-1.11",
            "wine-",
            "wine-x",
            "11.0",
            "GE-Proton11-1",
            "",
        ] {
            assert!(!wine_has_ntsync(version), "{version}");
        }
    }

    #[test]
    fn dll_overrides_follow_the_graphics_setting() {
        use GraphicsBackend::{Auto, Dxvk, Native, Vkd3dProton, Wined3d};
        assert_eq!(dll_overrides(Auto, false), "winemenubuilder.exe=");
        assert_eq!(dll_overrides(Wined3d, false), "winemenubuilder.exe=");
        assert_eq!(dll_overrides(Native, false), "winemenubuilder.exe=");
        assert_eq!(
            dll_overrides(Dxvk, false),
            "d3d8,d3d9,d3d10core,d3d11,dxgi=n,b;winemenubuilder.exe="
        );
        assert_eq!(
            dll_overrides(Vkd3dProton, false),
            "d3d12,d3d12core=n,b;winemenubuilder.exe="
        );
        assert_eq!(
            dll_overrides(Auto, true),
            "mscoree,mshtml=;winemenubuilder.exe="
        );
        // WINEDLLOVERRIDES allows no spaces.
        for graphics in GraphicsBackend::ALL {
            for creating in [false, true] {
                assert!(!dll_overrides(graphics, creating).contains(' '));
            }
        }
    }

    #[test]
    fn extra_environment_rules() {
        let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        assert_eq!(check_extra_environment(&[]), Ok(()));
        assert_eq!(
            check_extra_environment(&[pair("UOS_TEST_VALUE", "hello world"), pair("_x9", "")]),
            Ok(())
        );
        for (name, value) in [
            ("", "x"),
            ("9LIVES", "x"),
            ("WITH-HYPHEN", "x"),
            ("WITH SPACE", "x"),
            ("A=B", "x"),
            ("HOME", "/home/ada"),
            ("PATH", "/tmp"),
            ("WINEPREFIX", "/home/ada/.wine"),
            ("WINEDLLOVERRIDES", "x"),
            ("WINESERVER", "/tmp/evil"),
            ("XAUTHORITY", "/home/ada/.ssh/id_ed25519"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("PROTONPATH", "GE-Proton"),
            ("LD_PRELOAD", "/tmp/evil.so"),
            ("LD_LIBRARY_PATH", "/tmp"),
            ("OK_NAME", "bad\nvalue"),
            ("OK_NAME", "bad\u{1b}[0mvalue"),
        ] {
            let error = check_extra_environment(&[pair(name, value)]).unwrap_err();
            assert!(matches!(error, PlanError::Environment { .. }), "{name}");
            assert_eq!(error.code(), Code::EnvironmentRefused);
            // The value is never repeated.
            if !value.is_empty() && value != "x" {
                assert!(!error.to_string().contains(value), "{name}");
            }
        }
        assert!(check_extra_environment(&[pair("A", "1"), pair("A", "2")]).is_err());
        let long = "v".repeat(MAX_ENVIRONMENT_VALUE_BYTES + 1);
        assert!(check_extra_environment(&[pair("A", &long)]).is_err());
        let many: Vec<(String, String)> = (0..=MAX_EXTRA_ENVIRONMENT)
            .map(|index| pair(&format!("V{index}"), "x"))
            .collect();
        assert!(check_extra_environment(&many).is_err());
    }

    #[test]
    fn argument_rules() {
        assert_eq!(check_arguments(&[]), Ok(()));
        assert_eq!(
            check_arguments(&["--open".to_owned(), "C:\\file name.txt".to_owned()]),
            Ok(())
        );
        let error = check_arguments(&["a\0b".to_owned()]).unwrap_err();
        assert_eq!(error.code(), Code::LaunchUnsupported);
        let many = vec![String::new(); MAX_LAUNCH_ARGUMENTS + 1];
        assert!(check_arguments(&many).is_err());
    }

    /// An invocation on `machine` with default settings.
    pub(crate) struct Setup {
        pub(crate) machine: Machine,
        pub(crate) host: LaunchHost,
        pub(crate) app: AppPaths,
        pub(crate) context: GrantContext,
        pub(crate) sync: SyncRecord,
    }

    impl Setup {
        pub(crate) fn new(label: &str) -> Self {
            let machine = Machine::new(label);
            let host = fixture(&machine.home, &machine.runtime_dir);
            let app = machine.dirs.app("com.example.app").unwrap();
            let context = machine.context();
            Setup {
                machine,
                host,
                app,
                context,
                sync: SyncRecord::not_checked(),
            }
        }

        pub(crate) fn invocation(&self, runtime: RuntimeKind) -> Invocation<'_> {
            Invocation {
                host: &self.host,
                app: &self.app,
                context: &self.context,
                runtime,
                runtime_version: match runtime {
                    RuntimeKind::Wine => "wine-9.0",
                    RuntimeKind::Proton => "GE-Proton10-20",
                },
                architecture: PrefixArch::Win64,
                graphics: GraphicsBackend::Auto,
                sync: &self.sync,
                grants: &[],
                network: true,
                wine_debug: DEFAULT_WINE_DEBUG,
                extra_environment: &[],
            }
        }
    }

    fn names(environment: &BTreeMap<String, String>) -> Vec<&str> {
        environment.keys().map(String::as_str).collect()
    }

    #[test]
    fn a_wine_application_command() {
        let setup = Setup::new("cmd-wine");
        let program = setup.machine.home.join("Games/setup.exe");
        let passed = ["/silent".to_owned(), "two words".to_owned()];
        let assembled = assemble(
            &setup.invocation(RuntimeKind::Wine),
            Task::Application {
                program: &program,
                outside_prefix: true,
                arguments: &passed,
            },
        )
        .unwrap();

        let application = &assembled.application;
        assert_eq!(application.program, "/usr/bin/wine");
        assert_eq!(
            application.arguments,
            [program.to_str().unwrap(), "/silent", "two words"]
        );
        assert_eq!(
            application.working_directory.as_deref(),
            setup.machine.home.join("Games").to_str()
        );

        // The whole environment, by name: nothing else is passed on.
        let environment = &application.environment;
        assert_eq!(
            names(environment),
            [
                "DISPLAY",
                "HOME",
                "LANG",
                "LOGNAME",
                "PATH",
                "TZ",
                "USER",
                "WAYLAND_DISPLAY",
                "WINEDEBUG",
                "WINEDLLOVERRIDES",
                "WINEPREFIX",
                "XAUTHORITY",
                "XDG_RUNTIME_DIR",
            ]
        );
        assert_eq!(environment["PATH"], "/usr/bin:/bin");
        assert_eq!(environment["HOME"], setup.app.home.to_str().unwrap());
        assert_eq!(environment["WINEPREFIX"], setup.app.prefix.to_str().unwrap());
        assert_eq!(environment["USER"], "ada");
        assert_eq!(environment["DISPLAY"], ":0");
        assert_eq!(environment["WAYLAND_DISPLAY"], "wayland-0");
        assert_eq!(environment["XAUTHORITY"], XAUTHORITY_PATH);
        assert_eq!(environment["WINEDEBUG"], "fixme-all");
        assert_eq!(environment["WINEDLLOVERRIDES"], "winemenubuilder.exe=");
        // The real home directory appears nowhere in the environment.
        let home = setup.machine.home.to_str().unwrap();
        for (name, value) in environment {
            assert!(
                !value.starts_with(home) || value.starts_with(setup.app.app_dir.to_str().unwrap()),
                "{name}={value}"
            );
        }

        // What is started: bubblewrap, the supervisor, then Wine.
        let sandboxed = &assembled.sandboxed;
        assert_eq!(sandboxed.program, "/usr/bin/bwrap");
        assert_eq!(sandboxed.environment, application.environment);
        assert_eq!(sandboxed.working_directory, None);
        let split = sandboxed
            .arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap();
        let (sandbox_arguments, command) = sandboxed.arguments.split_at(split + 1);
        assert_boundary(&setup.machine, &setup.app, sandbox_arguments);
        assert_eq!(
            command,
            [
                SUPERVISOR_PATH,
                "supervise",
                "--settle",
                "/usr/bin/wineserver",
                "--",
                "/usr/bin/wine",
                program.to_str().unwrap(),
                "/silent",
                "two words",
            ]
        );
        let (_, entries) = parse(sandbox_arguments);
        let chdir = entries.iter().find(|entry| entry.option == "--chdir").unwrap();
        assert_eq!(Some(chdir.target.as_str()), setup.machine.home.join("Games").to_str());
        assert_eq!(assembled.mounts.len(), entries.len() - 2);
    }

    #[test]
    fn prefix_creation_and_update_commands() {
        let setup = Setup::new("cmd-wineboot");
        let created = assemble(&setup.invocation(RuntimeKind::Wine), Task::Initialise).unwrap();
        assert_eq!(created.application.arguments, ["wineboot", "--init"]);
        assert_eq!(created.application.environment["WINEARCH"], "win64");
        assert_eq!(
            created.application.environment["WINEDLLOVERRIDES"],
            "mscoree,mshtml=;winemenubuilder.exe="
        );
        assert_eq!(
            created.application.working_directory.as_deref(),
            setup.app.prefix.to_str()
        );
        let tail: Vec<&str> = created
            .sandboxed
            .arguments
            .iter()
            .rev()
            .take(3)
            .map(String::as_str)
            .collect();
        assert_eq!(tail, ["--init", "wineboot", "/usr/bin/wine"]);

        let updated = assemble(&setup.invocation(RuntimeKind::Wine), Task::Update).unwrap();
        assert_eq!(updated.application.arguments, ["wineboot", "--update"]);
        // WINEARCH is only given when a prefix is created.
        assert!(!updated.application.environment.contains_key("WINEARCH"));
    }

    #[test]
    fn a_proton_command_goes_through_umu_run() {
        let mut setup = Setup::new("cmd-proton");
        setup.sync = decide_sync(
            SyncBackend::Ntsync,
            RuntimeKind::Proton,
            "GE-Proton10-20",
            NtsyncDevice::Present,
            NOW,
        );
        let program = setup.app.prefix.join("drive_c/Game/game.exe");
        let invocation = Invocation {
            graphics: GraphicsBackend::Wined3d,
            ..setup.invocation(RuntimeKind::Proton)
        };
        let assembled = assemble(
            &invocation,
            Task::Application {
                program: &program,
                outside_prefix: false,
                arguments: &[],
            },
        )
        .unwrap();
        let application = &assembled.application;
        assert_eq!(application.program, "/usr/bin/umu-run");
        assert_eq!(application.arguments, [program.to_str().unwrap()]);
        let environment = &application.environment;
        assert_eq!(environment["WINEPREFIX"], setup.app.prefix.to_str().unwrap());
        assert_eq!(environment["GAMEID"], "umu-default");
        assert_eq!(environment["STORE"], "none");
        assert_eq!(environment["PROTONPATH"], "GE-Proton10-20");
        assert_eq!(environment["PROTON_VERB"], "waitforexitandrun");
        assert_eq!(environment["PROTON_USE_WINED3D"], "1");
        assert_eq!(environment["PROTON_USE_NTSYNC"], "1");
        for wine_only in ["WINEDEBUG", "WINEDLLOVERRIDES", "WINEARCH"] {
            assert!(!environment.contains_key(wine_only), "{wine_only}");
        }

        // No supervisor: Proton's own verb waits for its server. The
        // launcher needs namespaces of its own, so they are not disabled.
        let split = assembled
            .sandboxed
            .arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap();
        let (sandbox_arguments, command) = assembled.sandboxed.arguments.split_at(split + 1);
        assert_eq!(command, ["/usr/bin/umu-run", program.to_str().unwrap()]);
        let (flags, entries) = parse(sandbox_arguments);
        assert!(!flags.contains(&"--disable-userns".to_owned()));
        assert!(!entries.iter().any(|entry| entry.target == SUPERVISOR_PATH));
        // A program inside the prefix needs no mount of its own.
        assert!(
            !entries
                .iter()
                .any(|entry| entry.source.as_deref() == program.to_str())
        );
        assert_boundary(&setup.machine, &setup.app, sandbox_arguments);

        let created = assemble(&setup.invocation(RuntimeKind::Proton), Task::Initialise).unwrap();
        assert_eq!(created.application.arguments, [""]);
    }

    #[test]
    fn extra_variables_are_added_and_checked() {
        let setup = Setup::new("cmd-extra");
        let extra = [("UOS_TEST_VALUE".to_owned(), "from the launch".to_owned())];
        let invocation = Invocation {
            extra_environment: &extra,
            ..setup.invocation(RuntimeKind::Wine)
        };
        let assembled = assemble(&invocation, Task::Update).unwrap();
        assert_eq!(
            assembled.application.environment["UOS_TEST_VALUE"],
            "from the launch"
        );

        let hostile = [("WINEPREFIX".to_owned(), "/home/ada/.wine".to_owned())];
        let invocation = Invocation {
            extra_environment: &hostile,
            ..setup.invocation(RuntimeKind::Wine)
        };
        assert!(matches!(
            assemble(&invocation, Task::Update),
            Err(PlanError::Environment { .. })
        ));
    }

    #[test]
    fn nothing_is_assembled_for_root_or_without_the_programs() {
        let mut setup = Setup::new("cmd-refusals");
        setup.host.euid = 0;
        let error = assemble(&setup.invocation(RuntimeKind::Wine), Task::Initialise).unwrap_err();
        assert_eq!(error, PlanError::RunningAsRoot);
        assert_eq!(error.code(), Code::RunningAsRoot);
        assert!(error.to_string().contains("root"));
        // Root is refused for Proton too, and before anything else is looked at.
        setup.host.bwrap = None;
        setup.host.umu_run = None;
        assert_eq!(
            assemble(&setup.invocation(RuntimeKind::Proton), Task::Initialise),
            Err(PlanError::RunningAsRoot)
        );

        let mut setup = Setup::new("cmd-missing");
        setup.host.wine = None;
        let error = assemble(&setup.invocation(RuntimeKind::Wine), Task::Initialise).unwrap_err();
        assert_eq!(error.code(), Code::WineMissing);
        setup.host.umu_run = None;
        let error = assemble(&setup.invocation(RuntimeKind::Proton), Task::Initialise).unwrap_err();
        assert_eq!(error.code(), Code::ProtonLauncherMissing);

        let mut setup = Setup::new("cmd-no-server");
        setup.host.wine.as_mut().unwrap().server = None;
        let error = assemble(&setup.invocation(RuntimeKind::Wine), Task::Initialise).unwrap_err();
        assert!(error.to_string().contains("wineserver"));

        let mut setup = Setup::new("cmd-no-sandbox");
        setup.host.bwrap = None;
        let error = assemble(&setup.invocation(RuntimeKind::Wine), Task::Initialise).unwrap_err();
        assert_eq!(error, PlanError::Sandbox(SandboxError::Missing));
        assert_eq!(error.code(), Code::SandboxMissing);
    }

    #[test]
    fn a_session_without_a_display_sets_no_display_variables() {
        let mut setup = Setup::new("cmd-headless");
        setup.host.sockets = crate::launch_host::Sockets::default();
        setup.host.runtime_dir = None;
        let assembled = assemble(&setup.invocation(RuntimeKind::Wine), Task::Update).unwrap();
        for absent in ["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "XDG_RUNTIME_DIR"] {
            assert!(!assembled.application.environment.contains_key(absent), "{absent}");
        }
    }

    #[test]
    fn every_plan_error_has_a_code_and_a_message() {
        let errors = [
            PlanError::RunningAsRoot,
            PlanError::Sandbox(SandboxError::Missing),
            PlanError::ToolMissing { program: "tar" },
            PlanError::VersionUnknown,
            PlanError::Unsupported {
                reason: "x".to_owned(),
            },
            PlanError::PrefixMissing,
            PlanError::PrefixExists,
            PlanError::PrefixOwner {
                detail: "x".to_owned(),
            },
            PlanError::PinMismatch {
                pinned: "wine-8.0".to_owned(),
                installed: "wine-9.0".to_owned(),
            },
            PlanError::RuntimeMismatch {
                recorded: "wine-8.0".to_owned(),
                installed: "wine-9.0".to_owned(),
            },
            PlanError::Pinned {
                version: "wine-8.0".to_owned(),
            },
            PlanError::ProtonUnpinned,
            PlanError::Grant {
                path: "/".to_owned(),
                refusal: GrantRefusal::Root,
            },
            PlanError::Program {
                path: "/x".to_owned(),
                refusal: GrantRefusal::Missing,
            },
            PlanError::BackupMissing {
                name: "x".to_owned(),
            },
            PlanError::Paths(PathsError::NoHome),
        ];
        let mut codes: Vec<&str> = errors.iter().map(|error| error.code().as_str()).collect();
        for error in &errors {
            assert!(!error.to_string().is_empty());
        }
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), errors.len());
    }
}
