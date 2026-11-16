// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The plans that install, launch and remove a macOS application.
//!
//! One application, one directory, as for Windows applications:
//!
//! ```text
//! prefixes/<app-id>/prefix/darling/          the Darling prefix (DPREFIX)
//! prefixes/<app-id>/prefix/darling.workdir/  Darling's own scratch directory
//! prefixes/<app-id>/prefix/unpacked/         a package while it is unpacked
//! prefixes/<app-id>/home/                    the application's private HOME
//! prefixes/<app-id>/mac-application.json     the record, written last
//! launchers/<app-id>/tur-darling-<app-id>.desktop
//! ```
//!
//! Every command runs in the sandbox of [`crate::sandbox`]: the system
//! read-only, the prefix and the private home read-write, nothing else of the
//! person's files, no session bus. Darling's tree is shown read-only at
//! [`RUNTIME_TREE_PATH`], and the prefix a second time at
//! [`PREFIX_ALIAS_PATH`], so that the Unix sockets Darling makes in it have
//! short paths. Darling's own view of "the host's root"
//! (`/Volumes/SystemRoot`) is therefore the sandbox, not the computer.
//!
//! Darling runs unprivileged: its launcher is not set-user-ID here, and the
//! mode is chosen with `DARLING_NONROOT` or `DARLING_ROOTLESS`
//! ([`DarlingMode`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::MacError;
use super::darling::DarlingMode;
use super::record::{MacAppRecord, record_path};
use crate::codes::Code;
use crate::desktop::{escape_value, is_plain_launcher, quote_argument};
use crate::grant::GrantContext;
use crate::launch_host::LaunchHost;
use crate::paths::{AppPaths, plain_text};
use crate::plan::{Check, CommandSpec, Operation, Plan, Step};
use crate::runtime_cmd::{RUNTIME_PATH, check_arguments, check_extra_environment};
use crate::sandbox::{self, PREFIX_ALIAS_PATH, RUNTIME_TREE_PATH, SandboxRequest, XAUTHORITY_PATH};

/// The Darling prefix's name inside the application's prefix directory.
pub const PREFIX_DIRECTORY: &str = "darling";

/// Where a package is unpacked, inside the application's prefix directory.
pub const UNPACKED_DIRECTORY: &str = "unpacked";

/// Where Darling shows the sandbox's root to macOS programs.
pub const SYSTEM_ROOT: &str = "/Volumes/SystemRoot";

/// How long creating a Darling prefix may take, in seconds: Darling copies
/// its system into it.
pub const INITIALISE_SECONDS: u64 = 1200;

/// How long unpacking or copying may take, in seconds.
pub const COPY_SECONDS: u64 = 900;

/// Variables a launch may not set: the runtime manager's own, and those
/// that choose Darling's prefix or mode.
const RESERVED_PREFIXES: [&str; 3] = ["DARLING_", "DPREFIX", "LD_"];

/// Darling's variables a launch may set: the choice of AppKit's display
/// backend (`x11` or `wayland`).
const ALLOWED_DARLING: [&str; 1] = ["DARLING_APPKIT_BACKEND"];

/// Everything a Darling or tool command in an application's sandbox is made
/// from.
pub struct Setup<'a> {
    /// The machine and session.
    pub host: &'a LaunchHost,
    /// The application's paths.
    pub app: &'a AppPaths,
    /// The grant rules for this user.
    pub context: &'a GrantContext,
    /// TUR's build of Darling on this computer: what is shown at
    /// [`RUNTIME_TREE_PATH`].
    pub tree: &'a Path,
    /// How Darling runs.
    pub mode: DarlingMode,
    /// Whether the application shares the host's network.
    pub network: bool,
}

fn path_text(path: &Path, what: &str) -> Result<String, MacError> {
    plain_text(path).map(str::to_owned).ok_or_else(|| {
        MacError::new(
            Code::StorageUnusable,
            format!("the path of {what} is not a plain absolute path"),
        )
    })
}

/// The path a macOS program sees for a path of the sandbox.
///
/// # Errors
///
/// [`MacError`] when the path is not a plain absolute path.
pub fn mac_path(path: &Path) -> Result<String, MacError> {
    Ok(format!("{SYSTEM_ROOT}{}", path_text(path, "a file shown to Darling")?))
}

/// The main program of an installed application, as macOS sees it.
pub fn mac_program(record: &MacAppRecord) -> String {
    format!("/Applications/{}/{}", record.bundle, record.executable)
}

impl Setup<'_> {
    /// The Darling prefix on this system.
    pub fn darling_prefix(&self) -> PathBuf {
        self.app.prefix.join(PREFIX_DIRECTORY)
    }

    /// Where packages are unpacked.
    pub fn unpacked(&self) -> PathBuf {
        self.app.prefix.join(UNPACKED_DIRECTORY)
    }

    /// The `Applications` directory of the Darling prefix.
    pub fn applications(&self) -> PathBuf {
        self.darling_prefix().join("Applications")
    }

    fn session_environment(&self) -> Result<BTreeMap<String, String>, MacError> {
        let host = self.host;
        let mut environment: BTreeMap<String, String> = host.passthrough.clone();
        environment.insert("PATH".to_owned(), RUNTIME_PATH.to_owned());
        environment.insert("HOME".to_owned(), path_text(&self.app.home, "the private home")?);
        environment.insert("USER".to_owned(), host.user.clone());
        environment.insert("LOGNAME".to_owned(), host.user.clone());
        if let Some(x11) = &host.sockets.x11 {
            environment.insert("DISPLAY".to_owned(), x11.display.clone());
            if host.sockets.xauthority.is_some() {
                environment.insert("XAUTHORITY".to_owned(), XAUTHORITY_PATH.to_owned());
            }
        }
        if let Some(runtime_dir) = &host.runtime_dir {
            environment.insert(
                "XDG_RUNTIME_DIR".to_owned(),
                path_text(runtime_dir, "the runtime directory")?,
            );
            if let Some(wayland) = &host.sockets.wayland {
                environment.insert("WAYLAND_DISPLAY".to_owned(), wayland.name.clone());
            }
        }
        Ok(environment)
    }

    fn sandboxed(
        &self,
        darling: bool,
        program: String,
        arguments: &[String],
        shown: Option<&Path>,
        environment: BTreeMap<String, String>,
    ) -> Result<CommandSpec, MacError> {
        if self.host.euid == 0 {
            return Err(MacError::new(
                Code::RunningAsRoot,
                "macOS software is never run as root",
            ));
        }
        let sandbox = sandbox::build(&SandboxRequest {
            host: self.host,
            app: self.app,
            context: self.context,
            grants: &[],
            network: self.network,
            nested_namespaces: darling && self.mode == DarlingMode::Rootless,
            program_file: shown,
            supervisor: false,
            working_directory: &self.app.home,
            runtime_tree: darling.then_some(self.tree),
            prefix_alias: darling,
        })
        .map_err(|error| MacError::new(error.code(), error.to_string()))?;
        let mut outer = sandbox.arguments;
        outer.push(program);
        outer.extend(arguments.iter().cloned());
        Ok(CommandSpec {
            program: sandbox.program,
            arguments: outer,
            environment,
            working_directory: None,
        })
    }

    /// Darling's launcher, in the application's sandbox, with `arguments`
    /// (`shell ...`). `shown` is a file outside the prefix that it reads,
    /// shown read-only.
    ///
    /// # Errors
    ///
    /// [`MacError`] when the user is root, bubblewrap is missing, a path is
    /// not plain or an extra variable is not allowed.
    pub fn darling_command(
        &self,
        arguments: &[String],
        shown: Option<&Path>,
        extra: &[(String, String)],
    ) -> Result<CommandSpec, MacError> {
        check_extra_environment(extra)
            .map_err(|error| MacError::new(error.code(), error.to_string()))?;
        if let Some((name, _)) = extra.iter().find(|(name, _)| {
            !ALLOWED_DARLING.contains(&name.as_str())
                && RESERVED_PREFIXES
                    .iter()
                    .any(|reserved| name.starts_with(reserved))
        }) {
            return Err(MacError::new(
                Code::EnvironmentRefused,
                format!("the variable {name} chooses Darling's prefix, mode or loader and is set by the runtime manager"),
            ));
        }
        check_arguments(arguments)
            .map_err(|error| MacError::new(error.code(), error.to_string()))?;
        let mut environment = self.session_environment()?;
        environment.insert(
            "DPREFIX".to_owned(),
            format!("{PREFIX_ALIAS_PATH}/{PREFIX_DIRECTORY}"),
        );
        environment.insert(self.mode.variable().to_owned(), "1".to_owned());
        for (name, value) in extra {
            environment.insert(name.clone(), value.clone());
        }
        self.sandboxed(
            true,
            format!("{RUNTIME_TREE_PATH}/bin/darling"),
            arguments,
            shown,
            environment,
        )
    }

    /// A program of this system (7-Zip, `cp`, `find`), in the application's
    /// sandbox without Darling: whatever a hostile package does to it lands
    /// in a file system where only the prefix and the private home can be
    /// written.
    ///
    /// # Errors
    ///
    /// As [`Setup::darling_command`].
    pub fn tool_command(
        &self,
        program: &Path,
        arguments: &[String],
        shown: Option<&Path>,
    ) -> Result<CommandSpec, MacError> {
        let mut environment = BTreeMap::new();
        environment.insert("PATH".to_owned(), RUNTIME_PATH.to_owned());
        environment.insert("HOME".to_owned(), path_text(&self.app.home, "the private home")?);
        if let Some(lang) = self.host.passthrough.get("LANG") {
            environment.insert("LANG".to_owned(), lang.clone());
        }
        self.sandboxed(
            false,
            path_text(program, "a program")?,
            arguments,
            shown,
            environment,
        )
    }
}

/// The first plan of an installation: the application's directories, and
/// the package unpacked when it is a disk image or a zip archive.
///
/// # Errors
///
/// [`MacError`] when a command cannot be made.
pub fn plan_prepare(
    setup: &Setup<'_>,
    package: &Path,
    unpack: Option<&Path>,
) -> Result<Plan, MacError> {
    let app = setup.app;
    let mut plan = Plan::new(Operation::Install, &app.app_id);
    for path in [&app.prefix, &app.home, &app.logs] {
        plan.steps.push(Step::CreateDir { path: path.clone() });
    }
    if let Some(seven_zip) = unpack {
        plan.steps.push(Step::CreateDir {
            path: setup.unpacked(),
        });
        plan.steps.push(unpack_step(setup, seven_zip, package, &setup.unpacked(), true)?);
    }
    Ok(plan)
}

fn unpack_step(
    setup: &Setup<'_>,
    seven_zip: &Path,
    archive: &Path,
    into: &Path,
    shown: bool,
) -> Result<Step, MacError> {
    let arguments = vec![
        "x".to_owned(),
        "-y".to_owned(),
        "-bso0".to_owned(),
        "-bsp0".to_owned(),
        format!("-o{}", path_text(into, "the unpacking directory")?),
        "--".to_owned(),
        path_text(archive, "the package")?,
    ];
    Ok(Step::RunBounded {
        label: "unpack the package with 7-Zip, in the sandbox".to_owned(),
        command: setup.tool_command(seven_zip, &arguments, shown.then_some(archive))?,
        limit_seconds: COPY_SECONDS,
    })
}

/// Unpacks the partition images 7-Zip left (a disk image's HFS+ or APFS
/// volume), each into a directory beside it, and deletes the image.
///
/// # Errors
///
/// [`MacError`] when a command cannot be made.
pub fn plan_unpack_partitions(
    setup: &Setup<'_>,
    partitions: &[PathBuf],
    seven_zip: &Path,
) -> Result<Plan, MacError> {
    let mut plan = Plan::new(Operation::Install, &setup.app.app_id);
    for image in partitions {
        let mut into = image.clone().into_os_string();
        into.push(".d");
        let into = PathBuf::from(into);
        plan.steps.push(Step::CreateDir { path: into.clone() });
        plan.steps.push(unpack_step(setup, seven_zip, image, &into, false)?);
        plan.steps.push(Step::RemoveTree {
            path: image.clone(),
        });
    }
    Ok(plan)
}

/// Creates the Darling prefix: Darling's first start in it sets it up and
/// copies its system into it ([`DarlingMode::Nonroot`]).
///
/// # Errors
///
/// [`MacError`] when the command cannot be made.
pub fn plan_initialise(setup: &Setup<'_>) -> Result<Plan, MacError> {
    let mut plan = Plan::new(Operation::Create, &setup.app.app_id);
    plan.steps.push(Step::RunBounded {
        label: "create the Darling prefix".to_owned(),
        command: setup.darling_command(
            &["shell".to_owned(), "/usr/bin/true".to_owned()],
            None,
            &[],
        )?,
        limit_seconds: INITIALISE_SECONDS,
    });
    plan.steps.push(Step::Require {
        check: Check::Directory {
            path: setup.applications(),
        },
    });
    plan.notes.push(format!(
        "Darling runs in its {} mode, without set-user-ID and without a kernel module.",
        setup.mode.as_str()
    ));
    Ok(plan)
}

/// Copies an application bundle into the prefix's `/Applications` and makes
/// the programs in its `Contents/MacOS` directories executable (7-Zip does
/// not always keep the permission bits of a disk image's files).
///
/// # Errors
///
/// [`MacError`] when a command cannot be made or the name is not plain.
pub fn plan_copy_bundle(
    setup: &Setup<'_>,
    bundle: &Path,
    shown: bool,
    cp: &Path,
    find: &Path,
) -> Result<(Plan, String), MacError> {
    let name = bundle
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| name.ends_with(".app") && !name.contains('/') && !name.chars().any(char::is_control))
        .ok_or_else(|| {
            MacError::new(
                Code::MacosNoApplicationFound,
                "the application bundle's name is not a plain .app name",
            )
        })?
        .to_owned();
    let target = setup.applications().join(&name);
    let mut plan = Plan::new(Operation::Install, &setup.app.app_id);
    plan.steps.push(Step::RunBounded {
        label: "copy the application into the prefix, in the sandbox".to_owned(),
        command: setup.tool_command(
            cp,
            &[
                "--archive".to_owned(),
                "--no-dereference".to_owned(),
                "--reflink=auto".to_owned(),
                "--".to_owned(),
                path_text(bundle, "the bundle")?,
                path_text(&target, "the bundle's place in the prefix")?,
            ],
            shown.then_some(bundle),
        )?,
        limit_seconds: COPY_SECONDS,
    });
    plan.steps.push(Step::RunBounded {
        label: "make the bundle's programs executable, in the sandbox".to_owned(),
        command: setup.tool_command(
            find,
            &[
                path_text(&target, "the bundle")?,
                "-path".to_owned(),
                "*/Contents/MacOS/*".to_owned(),
                "-type".to_owned(),
                "f".to_owned(),
                "-exec".to_owned(),
                "chmod".to_owned(),
                "u+x".to_owned(),
                "{}".to_owned(),
                "+".to_owned(),
            ],
            None,
        )?,
        limit_seconds: COPY_SECONDS,
    });
    plan.steps.push(Step::Require {
        check: Check::Directory {
            path: target.clone(),
        },
    });
    Ok((plan, name))
}

/// Installs a flat installer package with Darling's own `installer`, in the
/// prefix: the package's scripts run as they would on a Mac, inside
/// Darling and the sandbox.
///
/// # Errors
///
/// [`MacError`] when the command cannot be made.
pub fn plan_installer_package(
    setup: &Setup<'_>,
    package: &Path,
    shown: bool,
    limit_seconds: u64,
) -> Result<Plan, MacError> {
    let mut plan = Plan::new(Operation::Install, &setup.app.app_id);
    plan.steps.push(Step::RunBounded {
        label: "install the package with Darling's installer".to_owned(),
        command: setup.darling_command(
            &[
                "shell".to_owned(),
                "/usr/sbin/installer".to_owned(),
                "-pkg".to_owned(),
                mac_path(package)?,
                "-target".to_owned(),
                "/".to_owned(),
            ],
            shown.then_some(package),
            &[],
        )?,
        limit_seconds,
    });
    Ok(plan)
}

/// The desktop entry of an installed application.
///
/// # Errors
///
/// [`MacError`] when `launcher` cannot be written into an entry safely.
pub fn desktop_entry(record: &MacAppRecord, launcher: &str) -> Result<String, MacError> {
    if !is_plain_launcher(launcher) {
        return Err(MacError::new(
            Code::MacosRequestInvalid,
            "the launcher is not a plain program name or absolute path",
        ));
    }
    Ok(format!(
        "[Desktop Entry]\n\
         Version=1.5\n\
         Type=Application\n\
         Name={}\n\
         Comment=macOS application (Darling, experimental)\n\
         Exec={launcher} mac launch {}\n\
         Icon=application-x-executable\n\
         Terminal=false\n\
         StartupNotify=true\n\
         X-TUR-App-Id={}\n\
         X-TUR-Runtime=darling\n",
        escape_value(&record.name),
        escape_value(&quote_argument(&record.app_id)),
        record.app_id
    ))
}

/// The desktop entry's file.
pub fn desktop_entry_path(app: &AppPaths) -> PathBuf {
    app.launchers
        .join(format!("tur-darling-{}.desktop", app.app_id))
}

/// The last plan of an installation: the unpacked package deleted, the
/// launcher entry staged, and the record written last, so that an
/// application has a record only when everything else is in place.
///
/// # Errors
///
/// [`MacError`] when the record cannot be written.
pub fn plan_record(setup: &Setup<'_>, record: &MacAppRecord, entry: &str) -> Result<Plan, MacError> {
    let app = setup.app;
    let mut plan = Plan::new(Operation::Install, &app.app_id);
    plan.steps.push(Step::RemoveTree {
        path: setup.unpacked(),
    });
    plan.steps.push(Step::CreateDir {
        path: app.launchers.clone(),
    });
    plan.steps.push(Step::WriteFile {
        path: desktop_entry_path(app),
        contents: entry.to_owned(),
    });
    let text = serde_json::to_string_pretty(record).map_err(|error| {
        MacError::new(
            Code::StorageUnusable,
            format!("cannot write the record: {error}"),
        )
    })?;
    plan.steps.push(Step::WriteFile {
        path: record_path(app),
        contents: format!("{text}\n"),
    });
    Ok(plan)
}

/// Undoes a failed installation: everything but the log of what went wrong.
pub fn plan_rollback(app: &AppPaths) -> Plan {
    let mut plan = Plan::new(Operation::Remove, &app.app_id);
    for path in [&app.app_dir, &app.staging, &app.launchers] {
        plan.steps.push(Step::RemoveTree { path: path.clone() });
    }
    plan
}

/// Starts an installed application, through Darling's shell so that its
/// bundle is found where macOS would have it.
///
/// # Errors
///
/// [`MacError`] when the command cannot be made.
pub fn plan_launch(
    setup: &Setup<'_>,
    record: &MacAppRecord,
    arguments: &[String],
    extra: &[(String, String)],
    log: PathBuf,
) -> Result<Plan, MacError> {
    let app = setup.app;
    let mut plan = Plan::new(Operation::Launch, &app.app_id);
    plan.steps.push(Step::Require {
        check: Check::Directory {
            path: setup.applications().join(&record.bundle),
        },
    });
    plan.steps.push(Step::CreateDir {
        path: app.logs.clone(),
    });
    let mut passed = vec!["shell".to_owned(), mac_program(record)];
    passed.extend(arguments.iter().cloned());
    plan.steps.push(Step::Launch {
        command: setup.darling_command(&passed, None, extra)?,
        log,
    });
    Ok(plan)
}

/// Removes an installed application: its directory (the Darling prefix,
/// the private home, the record), staging, launcher entry and logs; then
/// checks that each is gone.
pub fn plan_uninstall(app: &AppPaths) -> Plan {
    let mut plan = Plan::new(Operation::Uninstall, &app.app_id);
    let paths = [&app.app_dir, &app.staging, &app.launchers, &app.logs];
    for path in paths {
        plan.steps.push(Step::RemoveTree { path: path.clone() });
    }
    for path in paths {
        plan.steps.push(Step::Check {
            check: Check::Absent { path: path.clone() },
        });
    }
    plan.notes.push(
        "Whatever the application saved in its private home is inside its directory and is \
         deleted with it."
            .to_owned(),
    );
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grant::tests::Machine;
    use crate::launch_host::tests::fixture;
    use crate::macos::record::tests::sample;
    use crate::sandbox::tests::{assert_boundary, parse};

    struct World {
        machine: Machine,
        host: LaunchHost,
        app: AppPaths,
        context: GrantContext,
        tree: PathBuf,
    }

    fn world(label: &str) -> World {
        let machine = Machine::new(label);
        let host = fixture(&machine.home, &machine.runtime_dir);
        let app = machine.dirs.app("ch.jakob.tabletool").unwrap();
        let context = machine.context();
        let tree = machine
            .dirs
            .data_root()
            .join("darling/darling-test/tree/opt/tur-darling");
        World {
            machine,
            host,
            app,
            context,
            tree,
        }
    }

    fn setup(world: &World) -> Setup<'_> {
        Setup {
            host: &world.host,
            app: &world.app,
            context: &world.context,
            tree: &world.tree,
            mode: DarlingMode::Nonroot,
            network: false,
        }
    }

    fn command(step: &Step) -> &CommandSpec {
        match step {
            Step::RunBounded { command, .. } | Step::Launch { command, .. } => command,
            other => panic!("not a command: {other:?}"),
        }
    }

    /// bubblewrap's own arguments, up to and including its `--`.
    fn bwrap_part(arguments: &[String]) -> &[String] {
        let end = arguments.iter().position(|a| a == "--").unwrap();
        &arguments[..=end]
    }

    #[test]
    fn darling_runs_unprivileged_in_the_sandbox_with_its_tree_read_only() {
        let world = world("mac-darling-command");
        let setup = setup(&world);
        let spec = setup
            .darling_command(&["shell".to_owned(), "/usr/bin/true".to_owned()], None, &[])
            .unwrap();
        assert_boundary(&world.machine, &world.app, bwrap_part(&spec.arguments));
        let (flags, entries) = parse(bwrap_part(&spec.arguments));
        // Nonroot mode needs no namespaces of Darling's own.
        assert!(flags.contains(&"--disable-userns".to_owned()));
        assert!(flags.contains(&"--unshare-net".to_owned()));
        let tree = entries
            .iter()
            .find(|entry| entry.target == RUNTIME_TREE_PATH)
            .unwrap();
        assert_eq!(tree.option, "--ro-bind");
        let separator = spec.arguments.iter().position(|a| a == "--").unwrap();
        assert_eq!(
            spec.arguments[separator + 1],
            format!("{RUNTIME_TREE_PATH}/bin/darling")
        );
        assert_eq!(
            spec.environment.get("DPREFIX").map(String::as_str),
            Some("/run/tur-runtime/prefix/darling")
        );
        assert_eq!(
            spec.environment.get("DARLING_NONROOT").map(String::as_str),
            Some("1")
        );
        assert!(!spec.environment.contains_key("DARLING_ROOTLESS"));
        assert!(!spec.environment.contains_key("DBUS_SESSION_BUS_ADDRESS"));

        // Rootless mode may make user namespaces.
        let rootless = Setup {
            mode: DarlingMode::Rootless,
            ..setup
        };
        let spec = rootless
            .darling_command(&["shell".to_owned()], None, &[])
            .unwrap();
        let (flags, _) = parse(bwrap_part(&spec.arguments));
        assert!(!flags.contains(&"--disable-userns".to_owned()));
        assert_eq!(
            spec.environment.get("DARLING_ROOTLESS").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn variables_that_choose_the_prefix_or_the_loader_are_refused() {
        let world = world("mac-environment");
        let setup = setup(&world);
        for name in ["DPREFIX", "DARLING_ROOTLESS", "LD_PRELOAD", "HOME"] {
            let error = setup
                .darling_command(&["shell".to_owned()], None, &[(name.to_owned(), "x".to_owned())])
                .unwrap_err();
            assert_eq!(error.code, Code::EnvironmentRefused, "{name}");
        }
        let error = setup
            .darling_command(
                &["shell".to_owned()],
                None,
                &[("DARLING_APPKIT_BACKEND_X".to_owned(), "x".to_owned())],
            )
            .unwrap_err();
        assert_eq!(error.code, Code::EnvironmentRefused);
        let spec = setup
            .darling_command(
                &["shell".to_owned()],
                None,
                &[
                    ("MESA_DEBUG".to_owned(), "1".to_owned()),
                    ("DARLING_APPKIT_BACKEND".to_owned(), "wayland".to_owned()),
                ],
            )
            .unwrap();
        assert_eq!(spec.environment.get("MESA_DEBUG").map(String::as_str), Some("1"));
        assert_eq!(
            spec.environment.get("DARLING_APPKIT_BACKEND").map(String::as_str),
            Some("wayland")
        );
    }

    #[test]
    fn root_is_refused() {
        let mut world = world("mac-root");
        world.host.euid = 0;
        let setup = setup(&world);
        assert_eq!(
            setup.darling_command(&[], None, &[]).unwrap_err().code,
            Code::RunningAsRoot
        );
    }

    #[test]
    fn a_disk_image_is_unpacked_by_7zip_in_the_sandbox_without_darling() {
        let world = world("mac-prepare");
        let setup = setup(&world);
        let package = world.machine.home.join("Downloads/TableTool.dmg");
        let plan = plan_prepare(&setup, &package, Some(Path::new("/usr/bin/7z"))).unwrap();
        let spec = command(plan.steps.last().unwrap());
        assert_boundary(&world.machine, &world.app, bwrap_part(&spec.arguments));
        let (_, entries) = parse(bwrap_part(&spec.arguments));
        // The package itself is shown, read-only, and not its directory;
        // Darling's tree is not there.
        let shown = entries
            .iter()
            .find(|entry| entry.source.as_deref() == package.to_str())
            .unwrap();
        assert_eq!(shown.option, "--ro-bind");
        assert!(!entries.iter().any(|entry| entry.target == RUNTIME_TREE_PATH));
        let separator = spec.arguments.iter().position(|a| a == "--").unwrap();
        assert_eq!(spec.arguments[separator + 1], "/usr/bin/7z");
        assert!(spec.arguments.contains(&format!("-o{}", setup.unpacked().display())));

        // A bundle is not unpacked.
        let plan = plan_prepare(&setup, &package, None).unwrap();
        assert!(plan.steps.iter().all(|step| matches!(step, Step::CreateDir { .. })));
    }

    #[test]
    fn a_bundle_is_copied_and_its_programs_made_executable() {
        let world = world("mac-copy");
        let setup = setup(&world);
        let bundle = setup.unpacked().join("Table Tool.app");
        let (plan, name) = plan_copy_bundle(
            &setup,
            &bundle,
            false,
            Path::new("/usr/bin/cp"),
            Path::new("/usr/bin/find"),
        )
        .unwrap();
        assert_eq!(name, "Table Tool.app");
        let copy = command(&plan.steps[0]);
        assert!(copy.arguments.contains(&"--no-dereference".to_owned()));
        assert!(
            copy.arguments
                .contains(&setup.applications().join("Table Tool.app").display().to_string())
        );
        assert!(matches!(plan.steps.last(), Some(Step::Require { .. })));

        let error = plan_copy_bundle(
            &setup,
            Path::new("/x/notabundle"),
            false,
            Path::new("/usr/bin/cp"),
            Path::new("/usr/bin/find"),
        )
        .unwrap_err();
        assert_eq!(error.code, Code::MacosNoApplicationFound);
    }

    #[test]
    fn an_installer_package_is_installed_by_darling_from_the_sandbox_root() {
        let world = world("mac-pkg");
        let setup = setup(&world);
        let package = world.machine.home.join("Downloads/Example.pkg");
        let plan = plan_installer_package(&setup, &package, true, 900).unwrap();
        let spec = command(&plan.steps[0]);
        assert_boundary(&world.machine, &world.app, bwrap_part(&spec.arguments));
        let mac = format!("/Volumes/SystemRoot{}", package.display());
        assert!(spec.arguments.contains(&mac));
        assert!(spec.arguments.contains(&"/usr/sbin/installer".to_owned()));
    }

    #[test]
    fn launching_goes_through_darling_s_shell_with_the_mac_path() {
        let world = world("mac-launch");
        let setup = setup(&world);
        let record = sample("ch.jakob.tabletool");
        let log = world.app.logs.join("launch.log");
        let plan = plan_launch(&setup, &record, &["--flag".to_owned()], &[], log.clone()).unwrap();
        let Step::Launch { command, log: written } = &plan.steps[2] else {
            panic!("expected the launch");
        };
        assert_eq!(*written, log);
        let tail: Vec<&str> = command
            .arguments
            .iter()
            .rev()
            .take(3)
            .map(String::as_str)
            .collect();
        assert_eq!(
            tail,
            ["--flag", "/Applications/Table Tool.app/Contents/MacOS/Table Tool", "shell"]
        );
    }

    #[test]
    fn removal_leaves_nothing_and_checks_it() {
        let world = world("mac-uninstall");
        let plan = plan_uninstall(&world.app);
        let removed: Vec<&PathBuf> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                Step::RemoveTree { path } => Some(path),
                _ => None,
            })
            .collect();
        assert!(removed.contains(&&world.app.app_dir));
        assert!(removed.contains(&&world.app.logs));
        assert!(removed.contains(&&world.app.launchers));
        let checked = plan
            .steps
            .iter()
            .filter(|step| matches!(step, Step::Check { check: Check::Absent { .. } }))
            .count();
        assert_eq!(checked, removed.len());
    }

    #[test]
    fn the_desktop_entry_launches_through_tur_runtime() {
        let record = sample("ch.jakob.tabletool");
        let entry = desktop_entry(&record, "tur-runtime").unwrap();
        assert!(entry.contains("Exec=tur-runtime mac launch \"ch.jakob.tabletool\"\n"));
        assert!(entry.contains("X-TUR-Runtime=darling\n"));
        assert!(entry.contains("Name=Table Tool\n"));
        assert!(desktop_entry(&record, "tur runtime; rm -rf /").is_err());
        let hostile = MacAppRecord {
            name: "Evil\nExec=rm -rf /".to_owned(),
            ..record
        };
        let entry = desktop_entry(&hostile, "tur-runtime").unwrap();
        assert_eq!(entry.matches("\nExec=").count(), 1);
    }
}
