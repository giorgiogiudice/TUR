// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Installing a Windows application, and removing it again.
//!
//! An installation is three plans, carried out one after the other, with
//! the prefix looked at in between:
//!
//! 1. Create a fresh prefix for the application ([`crate::prefix`]).
//! 2. Run the installer in that prefix, inside the sandbox, in its
//!    documented unattended mode and with a time limit
//!    ([`plan_installer_run`]). A portable archive is unpacked and a plain
//!    program copied instead; neither runs anything of the application.
//! 3. Write down what was installed ([`describe_installation`],
//!    [`plan_record`]): the application record, the profile it is launched
//!    from, and a launcher entry with the program's icon.
//!
//! The third plan cannot be made before the second has run: what an
//! installer installs is only known afterwards ([`crate::discover`]).
//!
//! A removal runs the application's own uninstaller when it has one
//! ([`plan_uninstaller`]), then deletes everything kept for the application
//! ([`plan_removal`]) and checks that it is gone ([`plan_removal_check`]).
//! The uninstaller is a courtesy: an application lives entirely in its own
//! directory, so deleting that directory is what removes it, whether its
//! uninstaller worked or not.
//!
//! The installer, and whatever it installs, is untrusted. It runs in the
//! same sandbox as any application: it sees its own prefix and its own
//! file, read-only, and nothing else of the user's files.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::app_record::{
    APP_RECORD_SCHEMA_VERSION, AppRecord, InstallMethod, InstallRecord, LauncherRecord,
    MAX_RECORDED_ITEMS, RecordError, ShortcutRecord, SourceRecord, UninstallMethod,
    UninstallRecord, as_text, is_text, load_record,
};
use crate::arch::Arch;
use crate::casefold::CaseFoldRecord;
use crate::desktop::{DesktopEntry, EntryExtras, desktop_entry_with};
use crate::discover::{
    ChoiceError, Discovery, ShortcutPlace, choose_main, primary_entry, windows_path,
};
use crate::grant::{GrantRefusal, ProgramFile};
use crate::icon::{ExtractedIcon, extract_icon};
use crate::inspect::{Detected, inspect};
use crate::installer::{InstallerInfo, InstallerKind};
use crate::limits::MAX_INSTALLER_SECONDS;
use crate::metadata::{PrefixMetadata, RuntimeKind};
use crate::paths::{AppPaths, RuntimeDirs, plain_text};
use crate::pe_res::version_info;
use crate::plan::{Check, Operation, Plan, Step};
use crate::prefix::Planner;
use crate::profile::{
    AppProfile, Fallback, PROFILE_SCHEMA_VERSION, ProfileArch, ProfileFormat, ProfileRuntime,
    RuntimeChannel, SyncBackend, TestStatus, is_app_id, is_display_name,
};
use crate::runtime_cmd::{DEFAULT_WINE_DEBUG, Invocation, PlanError, Task, assemble};
use crate::sandbox::SandboxError;
use crate::shortcut::is_drive_path;
use crate::winreg::{UninstallEntry, read_uninstall_entries};

/// Arguments kept from an uninstall command line.
const MAX_UNINSTALL_ARGUMENTS: usize = 32;

/// The directory under `Program Files` that a portable application is put
/// into.
const PROGRAM_FILES: &str = "Program Files";

fn text(path: &Path, role: &'static str) -> Result<String, PlanError> {
    plain_text(path)
        .map(str::to_owned)
        .ok_or(PlanError::Sandbox(SandboxError::Path { role }))
}

fn request_error(problem: &str) -> PlanError {
    PlanError::InstallRequest {
        problem: problem.to_owned(),
    }
}

/// The invocation for installing and uninstalling. No granted directory is
/// shown to an installer.
fn invocation<'a>(planner: &'a Planner<'_>, metadata: &'a PrefixMetadata) -> Invocation<'a> {
    Invocation {
        host: planner.host,
        app: planner.app,
        context: planner.context,
        runtime: metadata.runtime,
        runtime_version: &metadata.runtime_version,
        architecture: metadata.architecture,
        graphics: metadata.graphics_backend,
        sync: &metadata.sync,
        grants: &[],
        network: metadata.network,
        wine_debug: DEFAULT_WINE_DEBUG,
        extra_environment: &[],
    }
}

/// What the caller asks of one installation.
#[derive(Debug, Clone, Copy)]
pub struct InstallRequest<'a> {
    /// The installer file, as an absolute path.
    pub installer: &'a str,
    /// What the file was identified as.
    pub info: &'a InstallerInfo,
    /// Whether the caller says that a plain program is the application
    /// itself, to be copied, and not an installer of an unknown kind.
    pub portable: bool,
    /// How long the installer may take, in seconds.
    pub limit_seconds: u64,
}

/// The plan that puts an application into its prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallerRun {
    /// The plan: one time-limited command, inside the sandbox.
    pub plan: Plan,
    /// What the command does with the file.
    pub method: InstallMethod,
    /// The arguments the installer is given, when it is run.
    pub arguments: Vec<String>,
}

/// A path of this system as the runtime's `Z:` drive shows it.
fn drive_z_path(path: &str) -> String {
    format!("Z:{}", path.replace('/', "\\"))
}

/// The name a copied program gets in the prefix: its own, with `.exe`
/// appended when it lacks it, because that is what makes a file a program
/// to Windows software.
fn program_file_name(file: &Path) -> Result<String, PlanError> {
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_text(name, 200))
        .ok_or_else(|| request_error("the file has no usable name"))?;
    if name.to_ascii_lowercase().ends_with(".exe") {
        Ok(name.to_owned())
    } else {
        Ok(format!("{name}.exe"))
    }
}

fn check_limit(limit_seconds: u64) -> Result<(), PlanError> {
    if limit_seconds == 0 || limit_seconds > MAX_INSTALLER_SECONDS {
        return Err(request_error(
            "the time limit must be between one second and one day",
        ));
    }
    Ok(())
}

/// Plans the step that puts the application into its prefix: the installer
/// in its unattended mode, or the unpacking of an archive, or the copying of
/// a program. `metadata` describes the prefix, which need not exist yet.
///
/// # Errors
///
/// [`PlanError::NoUnattendedMode`] for a plain program that the caller did
/// not declare portable; [`PlanError::Program`] when the installer file is
/// in a protected location or is not a regular file;
/// [`PlanError::ToolMissing`] when unpacking or copying needs a program
/// that is not installed; and whatever [`assemble`] refuses.
pub fn plan_installer_run(
    planner: &Planner<'_>,
    metadata: &PrefixMetadata,
    request: &InstallRequest<'_>,
) -> Result<InstallerRun, PlanError> {
    let app = planner.app;
    if planner.host.euid == 0 {
        return Err(PlanError::RunningAsRoot);
    }
    if metadata.runtime != RuntimeKind::Wine {
        return Err(PlanError::Unsupported {
            reason: "applications are installed with the wine runtime only".to_owned(),
        });
    }
    check_limit(request.limit_seconds)?;
    let file = match planner.context.admit_program(request.installer, app) {
        Ok(ProgramFile::Outside(file)) => file,
        Ok(ProgramFile::InPrefix(_)) => {
            return Err(request_error(
                "the installer lies inside the prefix it would be installed into",
            ));
        }
        Err(refusal) => {
            return Err(PlanError::Program {
                path: request.installer.to_owned(),
                refusal,
            });
        }
    };
    let file_text = text(&file, "the installer")?;
    let kind = request.info.kind;
    // What the file is was read from its content. The caller's word that it
    // is the application itself does not turn an installer into a program.
    if request.portable && kind != InstallerKind::PlainExe {
        return Err(PlanError::InstallRequest {
            problem: format!(
                "the file was said to be the application itself (--portable), and it is a {}; \
                 leave the option out to install it",
                kind.label()
            ),
        });
    }
    let unattended: Vec<String> = kind
        .unattended()
        .map(|unattended| {
            unattended
                .arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect()
        })
        .unwrap_or_default();
    let target_directory = app
        .prefix
        .join("drive_c")
        .join(PROGRAM_FILES)
        .join(&metadata.prefix_id);
    let invocation = invocation(planner, metadata);

    let (label, method, arguments, assembled, note) = match kind {
        InstallerKind::Msi => {
            // Windows Installer packages are run by the runtime's msiexec,
            // which is given the package by its Windows path.
            let mut command = vec![
                "msiexec".to_owned(),
                "/i".to_owned(),
                drive_z_path(&file_text),
            ];
            command.extend(unattended.iter().cloned());
            let assembled = assemble(
                &invocation,
                Task::Tool {
                    arguments: &command,
                    shown: Some(&file),
                },
            )?;
            (
                "run the installer",
                InstallMethod::UnattendedInstaller,
                unattended,
                assembled,
                "The package is installed by the runtime's msiexec, without its user interface.",
            )
        }
        InstallerKind::Nsis | InstallerKind::InnoSetup | InstallerKind::SevenZipInstaller => {
            let assembled = assemble(
                &invocation,
                Task::Application {
                    program: &file,
                    outside_prefix: true,
                    arguments: &unattended,
                },
            )?;
            (
                "run the installer",
                InstallMethod::UnattendedInstaller,
                unattended,
                assembled,
                "The installer is run in its documented unattended mode.",
            )
        }
        InstallerKind::PortableZip => {
            let unzip = planner
                .host
                .unzip
                .as_deref()
                .ok_or(PlanError::ToolMissing { program: "unzip" })?;
            let command = vec![
                "-q".to_owned(),
                "-o".to_owned(),
                file_text.clone(),
                "-d".to_owned(),
                text(&target_directory, "the application's directory")?,
            ];
            let assembled = assemble(
                &invocation,
                Task::Host {
                    program: unzip,
                    arguments: &command,
                    shown: Some(&file),
                },
            )?;
            (
                "unpack the archive",
                InstallMethod::Unpacked,
                Vec::new(),
                assembled,
                "The archive is unpacked inside the sandbox; nothing in it is run.",
            )
        }
        InstallerKind::PlainExe => {
            if !request.portable {
                return Err(PlanError::NoUnattendedMode { kind });
            }
            let install = planner
                .host
                .install
                .as_deref()
                .ok_or(PlanError::ToolMissing { program: "install" })?;
            let target = target_directory.join(program_file_name(&file)?);
            let command = vec![
                "-D".to_owned(),
                "-m".to_owned(),
                "0644".to_owned(),
                "--".to_owned(),
                file_text.clone(),
                text(&target, "the application's program")?,
            ];
            let assembled = assemble(
                &invocation,
                Task::Host {
                    program: install,
                    arguments: &command,
                    shown: Some(&file),
                },
            )?;
            (
                "copy the program",
                InstallMethod::Copied,
                Vec::new(),
                assembled,
                "The program is copied into the prefix inside the sandbox; it is not run.",
            )
        }
    };

    let mut plan = Plan::new(Operation::Install, &app.app_id);
    plan.steps.push(Step::RunBounded {
        label: label.to_owned(),
        command: assembled.sandboxed,
        limit_seconds: request.limit_seconds,
    });
    plan.notes.push(note.to_owned());
    plan.notes.push(
        "What is recorded about the application, and its launcher entry, depend on what this \
         step puts into the prefix; they are planned once it has run."
            .to_owned(),
    );
    Ok(InstallerRun {
        plan,
        method,
        arguments,
    })
}

/// What an installation is recorded with, beyond what is found in the
/// prefix.
#[derive(Debug, Clone)]
pub struct RecordOptions<'a> {
    /// The name the caller gives the application, instead of the one its
    /// installer registered.
    pub name: Option<&'a str>,
    /// The main program, named by the caller instead of chosen by rule.
    pub main_executable: Option<&'a str>,
    /// The `tur-runtime` program the launcher entry starts, by name or
    /// absolute path.
    pub launcher: &'a str,
    /// The file the application was installed from.
    pub source: SourceRecord,
    /// What was done with that file.
    pub method: InstallMethod,
    /// The arguments the installer was given.
    pub arguments: Vec<String>,
    /// What became of making the prefix's drive C: case-insensitive.
    pub case_folding: Option<CaseFoldRecord>,
}

/// Everything that is written down about a finished installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The application record.
    pub record: AppRecord,
    /// The profile the application is launched from.
    pub profile: AppProfile,
    /// The launcher entry.
    pub entry: DesktopEntry,
    /// The icon taken from the main program, when it has one.
    pub icon: Option<ExtractedIcon>,
    /// Things the user should know about what was installed.
    pub notes: Vec<String>,
}

/// Whether `text` is a Windows Installer product code: a GUID in braces.
fn is_product_code(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 38
        && bytes.first() == Some(&b'{')
        && bytes.last() == Some(&b'}')
        && bytes.get(1..37).is_some_and(|inner| {
            inner
                .iter()
                .all(|byte| byte.is_ascii_hexdigit() || *byte == b'-')
        })
}

/// Whether `path` is a Windows path on the prefix's `C:` drive, with no
/// component that leads upwards.
fn is_c_drive_path(path: &str) -> bool {
    is_drive_path(path)
        && path
            .as_bytes()
            .first()
            .is_some_and(|drive| drive.eq_ignore_ascii_case(&b'c'))
        && !path.split(['\\', '/']).any(|part| part == "..")
}

/// Splits what follows the program of a Windows command line into
/// arguments: at white space, except inside double quotes.
fn split_arguments(rest: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    for character in rest.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            space if space.is_whitespace() && !quoted => {
                if started {
                    arguments.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            other => {
                current.push(other);
                started = true;
            }
        }
    }
    if started {
        arguments.push(current);
    }
    arguments
        .iter()
        .filter_map(|argument| as_text(argument, 1024))
        .take(MAX_UNINSTALL_ARGUMENTS)
        .collect()
}

/// Splits an uninstall command line into its program and arguments. The
/// program is either quoted or ends at the first `.exe`.
fn split_command(command: &str) -> Option<(String, Vec<String>)> {
    let command = command.trim();
    let (program, rest) = if let Some(inner) = command.strip_prefix('"') {
        let end = inner.find('"')?;
        (inner.get(..end)?, inner.get(end + 1..)?)
    } else {
        // Lower-casing ASCII keeps byte offsets.
        let end = command.to_ascii_lowercase().find(".exe")? + 4;
        let rest = command.get(end..)?;
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            return None;
        }
        (command.get(..end)?, rest)
    };
    Some((as_text(program, 2048)?, split_arguments(rest)))
}

/// How an application installed by `kind` is removed, given the uninstall
/// entry its installer left.
fn uninstall_record(kind: InstallerKind, entry: Option<&UninstallEntry>) -> UninstallRecord {
    let nothing_to_run = UninstallRecord {
        method: UninstallMethod::RemovePrefix,
        program: None,
        arguments: Vec::new(),
        registry_key: None,
    };
    let Some(entry) = entry else {
        return nothing_to_run;
    };
    if kind.unattended().is_none() {
        return nothing_to_run;
    }
    let registry_key = as_text(&entry.identity(), 1024);
    let command = entry.uninstall_string.as_deref().and_then(split_command);

    // A Windows Installer product is removed by its product code, whatever
    // started the installation.
    let through_installer = command.as_ref().is_some_and(|(program, _)| {
        program.rsplit(['\\', '/']).next().is_some_and(|name| {
            name.eq_ignore_ascii_case("msiexec.exe") || name.eq_ignore_ascii_case("msiexec")
        })
    });
    if (entry.windows_installer || through_installer) && is_product_code(&entry.key) {
        return UninstallRecord {
            method: UninstallMethod::MsiProduct,
            program: Some(entry.key.clone()),
            arguments: InstallerKind::Msi
                .unattended_uninstall()
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
            registry_key,
        };
    }
    let Some((program, mut arguments)) = command else {
        return nothing_to_run;
    };
    if through_installer || !is_c_drive_path(&program) {
        return nothing_to_run;
    }
    for argument in kind.unattended_uninstall() {
        if !arguments
            .iter()
            .any(|present| present.eq_ignore_ascii_case(argument))
        {
            arguments.push((*argument).to_owned());
        }
    }
    arguments.truncate(MAX_RECORDED_ITEMS);
    UninstallRecord {
        method: UninstallMethod::Uninstaller,
        program: Some(program),
        arguments,
        registry_key,
    }
}

/// Works out everything that is recorded about an installation: which
/// program is the main one, what the application is called, how it is
/// removed, and the profile, launcher entry and icon that go with it.
///
/// This reads the prefix (the main program's headers, version information
/// and icon) and writes nothing.
///
/// # Errors
///
/// [`PlanError::NothingInstalled`] when the installer added no program;
/// [`PlanError::MainProgramUnclear`] when no rule singles out a main
/// program, or the one the caller named is not there;
/// [`PlanError::Program`] when the chosen file is not a Windows program
/// inside the prefix; [`PlanError::InstallRequest`] when the given name is
/// not allowed.
pub fn describe_installation(
    planner: &Planner<'_>,
    metadata: &PrefixMetadata,
    info: &InstallerInfo,
    discovery: &Discovery,
    options: RecordOptions<'_>,
) -> Result<Installed, PlanError> {
    let app = planner.app;
    if options.name.is_some_and(|name| !is_display_name(name)) {
        return Err(request_error(
            "the name must be 1 to 200 characters, without control characters",
        ));
    }
    let main = choose_main(discovery, options.main_executable).map_err(|error| match error {
        ChoiceError::NothingInstalled => PlanError::NothingInstalled,
        ChoiceError::Unclear { candidates } => PlanError::MainProgramUnclear {
            requested: None,
            candidates,
        },
        ChoiceError::NotFound {
            requested,
            candidates,
        } => PlanError::MainProgramUnclear {
            requested: Some(requested),
            candidates,
        },
    })?;

    // The program as a file of this system. It was found by a walk that
    // follows no link; it is checked again here to be a regular file that
    // resolves to itself inside the prefix.
    let found = app.prefix.join("drive_c").join(&main.executable);
    let found_text = text(&found, "the application's main program")?;
    let not_a_program = |refusal: GrantRefusal| PlanError::Program {
        path: found_text.clone(),
        refusal,
    };
    let program = match planner.context.admit_program(&found_text, app) {
        Ok(ProgramFile::InPrefix(program)) => program,
        Ok(ProgramFile::Outside(_)) => return Err(not_a_program(GrantRefusal::WrongKind)),
        Err(refusal) => return Err(not_a_program(refusal)),
    };
    let program_text = text(&program, "the application's main program")?;
    let (architecture, needs_dotnet) = match inspect(&program).map(|inspection| inspection.detected)
    {
        Ok(Detected::Pe(pe)) if !pe.is_dll => match pe.machine {
            Arch::X86_64 => (ProfileArch::X86_64, pe.is_dotnet),
            Arch::I386 => (ProfileArch::I386, pe.is_dotnet),
            _ => return Err(not_a_program(GrantRefusal::WrongKind)),
        },
        _ => return Err(not_a_program(GrantRefusal::WrongKind)),
    };
    // Nothing is installed for an application. One that needs a runtime of
    // its own is recorded as needing it, and the user is told.
    let mut notes = Vec::new();
    let mut known_requirements = Vec::new();
    if needs_dotnet {
        known_requirements.push("dotnet".to_owned());
        notes.push(
            "The main program is a .NET program. It needs a .NET runtime in its prefix, and \
             none is installed for it: it will most likely not start."
                .to_owned(),
        );
    }

    // What the application is called: the caller's word, then the
    // installer's, then the program's own.
    let entry = primary_entry(discovery, &main.executable);
    let version = version_info(&program).unwrap_or_default();
    let shortcut_name = discovery
        .shortcuts
        .iter()
        .find(|shortcut| {
            shortcut.place != ShortcutPlace::Elsewhere
                && shortcut.executable.as_deref() == Some(main.executable.as_str())
        })
        .map(|shortcut| shortcut.name.as_str());
    let file_stem = Path::new(&main.executable)
        .file_stem()
        .and_then(|stem| stem.to_str());
    let name = [
        options.name,
        entry.and_then(|entry| entry.display_name.as_deref()),
        version.product_name.as_deref(),
        shortcut_name,
        version.file_description.as_deref(),
        file_stem,
    ]
    .into_iter()
    .flatten()
    .find_map(|candidate| as_text(candidate, 200))
    .unwrap_or_else(|| "Application".to_owned());
    let application_version = [
        entry.and_then(|entry| entry.display_version.as_deref()),
        version.product_version.as_deref(),
        version.file_version.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(|candidate| as_text(candidate, 100));
    let publisher = [
        entry.and_then(|entry| entry.publisher.as_deref()),
        version.company_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(|candidate| as_text(candidate, 200));

    let profile = AppProfile {
        schema_version: PROFILE_SCHEMA_VERSION,
        app_id: app.app_id.clone(),
        display_name: name.clone(),
        binary: program_text.clone(),
        format: ProfileFormat::Pe,
        architecture,
        runtime: ProfileRuntime::Wine,
        runtime_channel: Some(RuntimeChannel::Stable),
        // Not pinned: the prefix records the version it is on, and a launch
        // refuses another one until the prefix is migrated.
        runtime_version: None,
        prefix_id: Some(metadata.prefix_id.clone()),
        graphics_backend: Some(metadata.graphics_backend),
        sync_backend: Some(SyncBackend::Auto),
        isolation_profile: "desktop-standard".to_owned(),
        known_requirements,
        fallback: Fallback::WindowsVmOffer,
        // Installed is not tested: nothing here says that it works.
        test_status: TestStatus::Untested,
    };
    profile
        .validate()
        .map_err(|error| PlanError::InstallRequest {
            problem: error.to_string(),
        })?;

    // The launcher entry, and the icon when the program has one that can
    // be read. A program without an icon gets the theme's generic one.
    let profile_path = text(&app.profile, "the application's profile")?;
    let stem = format!("tur-{}-{}", metadata.runtime.as_str(), app.app_id);
    let icon = extract_icon(&program).ok().flatten();
    let icon_path = match &icon {
        Some(_) => Some(text(
            &app.launchers.join(format!("{stem}.png")),
            "the application's icon",
        )?),
        None => None,
    };
    let window_class = Path::new(&main.executable)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_lowercase);
    let desktop_entry = desktop_entry_with(
        &profile,
        &profile_path,
        options.launcher,
        &EntryExtras {
            icon: icon_path.as_deref(),
            window_class: window_class.as_deref(),
        },
    )?;
    let entry_path = text(
        &app.launchers.join(&desktop_entry.file_name),
        "the application's launcher entry",
    )?;

    let record = AppRecord {
        schema_version: APP_RECORD_SCHEMA_VERSION,
        app_id: app.app_id.clone(),
        name,
        version: application_version,
        publisher,
        main_executable: program_text,
        main_executable_windows: windows_path(&main.executable),
        main_executable_reason: main.reason,
        prefix: text(&app.prefix, "the prefix")?,
        prefix_id: metadata.prefix_id.clone(),
        runtime: metadata.runtime,
        runtime_version: metadata.runtime_version.clone(),
        installed_at: planner.now,
        case_folding: options.case_folding,
        install: InstallRecord {
            kind: info.kind,
            method: options.method,
            arguments: options.arguments,
            source: options.source,
        },
        uninstall: uninstall_record(info.kind, entry),
        launcher: LauncherRecord {
            desktop_entry: entry_path,
            icon: icon_path,
            installed: false,
        },
        profile: profile_path,
        executables: discovery
            .executables
            .iter()
            .map(|relative| windows_path(relative))
            .filter(|path| is_text(path, 4096))
            .take(MAX_RECORDED_ITEMS)
            .collect(),
        shortcuts: discovery
            .shortcuts
            .iter()
            .filter(|shortcut| shortcut.place != ShortcutPlace::Elsewhere)
            .filter_map(|shortcut| {
                Some(ShortcutRecord {
                    name: as_text(&shortcut.name, 1024)?,
                    target: as_text(shortcut.target.as_deref()?, 4096)?,
                    arguments: shortcut
                        .arguments
                        .as_deref()
                        .and_then(|arguments| as_text(arguments, 4096)),
                })
            })
            .take(MAX_RECORDED_ITEMS)
            .collect(),
    };
    record.validate()?;
    Ok(Installed {
        record,
        profile,
        entry: desktop_entry,
        icon,
        notes,
    })
}

/// Where a launcher entry and its icon would be installed on a system
/// whose desktop this build integrates with, relative to the user's data
/// directory (`$XDG_DATA_HOME`): the entry among the user's applications,
/// the icon in the user's icon theme.
pub fn launcher_destinations(installed: &Installed) -> (String, Option<String>) {
    let entry = format!("applications/{}", installed.entry.file_name);
    let icon = installed.icon.as_ref().map(|icon| {
        let stem = installed
            .entry
            .file_name
            .strip_suffix(".desktop")
            .unwrap_or(&installed.entry.file_name);
        format!(
            "icons/hicolor/{}x{}/apps/{stem}.png",
            icon.width, icon.height
        )
    });
    (entry, icon)
}

/// Plans the writing of everything recorded about a finished installation.
/// The application record is written last: an application has a record only
/// when the rest is in place.
pub fn plan_record(planner: &Planner<'_>, installed: &Installed) -> Plan {
    let app = planner.app;
    let mut plan = Plan::new(Operation::Install, &app.app_id);
    plan.steps.push(Step::CreateDir {
        path: app.launchers.clone(),
    });
    let mut profile = serde_json::to_string_pretty(&installed.profile).unwrap_or_default();
    profile.push('\n');
    plan.steps.push(Step::WriteFile {
        path: app.profile.clone(),
        contents: profile,
    });
    if let (Some(icon), Some(path)) = (&installed.icon, &installed.record.launcher.icon) {
        plan.steps.push(Step::WriteBytes {
            path: PathBuf::from(path),
            bytes: icon.png.clone(),
        });
    }
    plan.steps.push(Step::WriteFile {
        path: PathBuf::from(&installed.record.launcher.desktop_entry),
        contents: installed.entry.contents.clone(),
    });
    plan.steps.push(Step::WriteFile {
        path: app.record.clone(),
        contents: installed.record.to_json(),
    });
    let (entry, icon) = launcher_destinations(installed);
    plan.notes.push(format!(
        "The launcher entry is staged in {} and is not shown by the desktop. Installed, it \
         would be $XDG_DATA_HOME/{entry}{}.",
        app.launchers.display(),
        icon.map_or(String::new(), |icon| format!(
            ", with its icon at $XDG_DATA_HOME/{icon}"
        ))
    ));
    plan
}

/// Plans the deletion of what a failed installation left: the prefix and
/// anything staged. The logs are kept, because they say what went wrong.
pub fn plan_rollback(planner: &Planner<'_>) -> Plan {
    let app = planner.app;
    let mut plan = Plan::new(Operation::Remove, &app.app_id);
    for path in [&app.app_dir, &app.staging, &app.launchers] {
        plan.steps.push(Step::RemoveTree { path: path.clone() });
    }
    plan.notes
        .push(format!("The logs in {} are kept.", app.logs.display()));
    plan
}

/// Plans the run of the application's own uninstaller, with a time limit.
/// Returns `Ok(None)` when the application has none.
///
/// # Errors
///
/// [`PlanError::Record`] when the record names an uninstaller that is not a
/// product code or a program on the prefix's `C:` drive, and whatever
/// [`assemble`] refuses.
pub fn plan_uninstaller(
    planner: &Planner<'_>,
    metadata: &PrefixMetadata,
    record: &AppRecord,
    limit_seconds: u64,
) -> Result<Option<Plan>, PlanError> {
    if planner.host.euid == 0 {
        return Err(PlanError::RunningAsRoot);
    }
    check_limit(limit_seconds)?;
    let invalid = |problem: &'static str| {
        PlanError::Record(RecordError::Invalid {
            field: "uninstall",
            problem,
        })
    };
    let program = record.uninstall.program.as_deref();
    let mut command: Vec<String> = match record.uninstall.method {
        UninstallMethod::RemovePrefix => return Ok(None),
        UninstallMethod::MsiProduct => {
            let code = program
                .filter(|code| is_product_code(code))
                .ok_or_else(|| invalid("must name a product code in braces"))?;
            vec!["msiexec".to_owned(), "/x".to_owned(), code.to_owned()]
        }
        UninstallMethod::Uninstaller => {
            let program = program
                .filter(|program| is_c_drive_path(program))
                .ok_or_else(|| invalid("must name a program on the prefix's C: drive"))?;
            vec![program.to_owned()]
        }
    };
    command.extend(record.uninstall.arguments.iter().cloned());
    let assembled = assemble(
        &invocation(planner, metadata),
        Task::Tool {
            arguments: &command,
            shown: None,
        },
    )?;
    let mut plan = Plan::new(Operation::Uninstall, &planner.app.app_id);
    plan.steps.push(Step::RunBounded {
        label: "run the application's uninstaller".to_owned(),
        command: assembled.sandboxed,
        limit_seconds,
    });
    plan.notes.push(
        "The uninstaller runs inside the sandbox. Whatever it does or leaves, the prefix is \
         deleted afterwards."
            .to_owned(),
    );
    Ok(Some(plan))
}

/// What an application's own uninstaller left in the prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UninstallerLeftovers {
    /// Whether the main program is still there.
    pub main_executable_present: bool,
    /// Whether the uninstall entry is still in the registry.
    pub uninstall_entry_present: bool,
    /// The recorded programs that are still there, as Windows paths.
    pub programs_left: Vec<String>,
}

/// Looks at what the application's uninstaller left behind, before the
/// prefix is deleted. This is a finding about the uninstaller, not about
/// the removal: the prefix goes either way.
pub fn survey_leftovers(app: &AppPaths, record: &AppRecord) -> UninstallerLeftovers {
    let drive_c = app.prefix.join("drive_c");
    let present = |path: &Path| fs::symlink_metadata(path).is_ok();
    let programs_left = record
        .executables
        .iter()
        .filter(|program| is_c_drive_path(program))
        .filter(|program| {
            let relative = program.get(3..).unwrap_or("").replace('\\', "/");
            present(&drive_c.join(relative))
        })
        .cloned()
        .collect();
    let uninstall_entry_present = record.uninstall.registry_key.as_deref().is_some_and(|key| {
        read_uninstall_entries(&app.prefix)
            .iter()
            .any(|entry| entry.identity() == key)
    });
    UninstallerLeftovers {
        main_executable_present: present(Path::new(&record.main_executable)),
        uninstall_entry_present,
        programs_left,
    }
}

/// Everything kept for one application: its directory (prefix, private
/// home, metadata, record, profile), the staging area, the logs and the
/// launcher entry; and its backups when `with_backups` is set.
pub fn application_paths(app: &AppPaths, with_backups: bool) -> Vec<PathBuf> {
    let mut paths = vec![
        app.app_dir.clone(),
        app.staging.clone(),
        app.logs.clone(),
        app.launchers.clone(),
    ];
    if with_backups {
        paths.push(app.backups.clone());
    }
    paths
}

/// Plans the deletion of everything kept for an application. Backups are
/// the user's safety copies and are kept unless `remove_backups` is set.
///
/// # Errors
///
/// [`PlanError::RunningAsRoot`].
pub fn plan_removal(planner: &Planner<'_>, remove_backups: bool) -> Result<Plan, PlanError> {
    if planner.host.euid == 0 {
        return Err(PlanError::RunningAsRoot);
    }
    let app = planner.app;
    let mut plan = Plan::new(Operation::Uninstall, &app.app_id);
    for path in application_paths(app, remove_backups) {
        plan.steps.push(Step::RemoveTree { path });
    }
    plan.notes.push(if remove_backups {
        "Backups of the prefix are deleted too.".to_owned()
    } else {
        format!(
            "Backups in {} are kept; --remove-backups deletes them too.",
            app.backups.display()
        )
    });
    plan.notes.push(
        "Everything the application wrote is inside its own directory, including documents \
         saved in its private home; they are deleted with it."
            .to_owned(),
    );
    Ok(plan)
}

/// Plans the check that nothing of an application is left: every path
/// [`plan_removal`] deletes must be gone.
pub fn plan_removal_check(planner: &Planner<'_>, remove_backups: bool) -> Plan {
    let mut plan = Plan::new(Operation::Uninstall, &planner.app.app_id);
    for path in application_paths(planner.app, remove_backups) {
        plan.steps.push(Step::Check {
            check: Check::Absent { path },
        });
    }
    plan
}

/// The records of every installed application, sorted by identifier, and a
/// line for every record that could not be read.
pub fn installed_applications(dirs: &RuntimeDirs) -> (Vec<AppRecord>, Vec<String>) {
    let mut records = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = fs::read_dir(dirs.prefixes()) else {
        return (records, problems);
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| is_app_id(name))
        .collect();
    names.sort();
    for name in names {
        let Ok(app) = dirs.app(&name) else {
            continue;
        };
        match load_record(&app) {
            Ok(Some(record)) => records.push(record),
            // A prefix made without an installation has no record.
            Ok(None) => {}
            Err(error) => problems.push(format!("{name}: {error}")),
        }
    }
    (records, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::Code;
    use crate::discover::tests::{link, put};
    use crate::discover::{discover, snapshot};
    use crate::icon::tests::{bitmap, group, read_png};
    use crate::installer::identify;
    use crate::installer::tests::{inno_installer, nsis_installer, seven_zip_installer};
    use crate::ole;
    use crate::pe_res::tests::{image, version_resource};
    use crate::plan::fake::FakeExecutor;
    use crate::plan::run_plan;
    use crate::prefix::tests::{planner, recorded};
    use crate::runtime_cmd::tests::{NOW, Setup};
    use crate::sandbox::SUPERVISOR_PATH;
    use crate::sandbox::tests::assert_boundary;
    use crate::testdata;

    const START_MENU: &str = "ProgramData/Microsoft/Windows/Start Menu/Programs";

    /// Writes an installer into the scratch home and identifies it.
    fn installer(setup: &Setup, name: &str, bytes: &[u8]) -> (String, InstallerInfo) {
        let path = setup.machine.home.join("Games").join(name);
        fs::write(&path, bytes).unwrap();
        let info = identify(&path).unwrap();
        (path.to_str().unwrap().to_owned(), info)
    }

    fn request<'a>(path: &'a str, info: &'a InstallerInfo) -> InstallRequest<'a> {
        InstallRequest {
            installer: path,
            info,
            portable: false,
            limit_seconds: 900,
        }
    }

    /// The command of a plan's one step: what follows the sandbox's own
    /// arguments. The sandbox is checked to be the boundary it must be.
    fn command_of(setup: &Setup, plan: &Plan) -> Vec<String> {
        let [
            Step::RunBounded {
                command,
                limit_seconds,
                ..
            },
        ] = plan.steps.as_slice()
        else {
            panic!("{:?}", plan.steps);
        };
        assert_eq!(*limit_seconds, 900);
        assert_eq!(command.program, "/usr/bin/bwrap");
        let split = command
            .arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap();
        let (sandbox, rest) = command.arguments.split_at(split + 1);
        assert_boundary(&setup.machine, &setup.app, sandbox);
        rest.to_vec()
    }

    const SUPERVISED: [&str; 6] = [
        SUPERVISOR_PATH,
        "supervise",
        "--settle",
        "/usr/bin/wineserver",
        "--",
        "/usr/bin/wine",
    ];

    fn supervised(rest: &[&str]) -> Vec<String> {
        SUPERVISED
            .iter()
            .chain(rest)
            .map(|argument| (*argument).to_owned())
            .collect()
    }

    #[test]
    fn each_kind_of_installer_is_run_in_its_unattended_mode() {
        let setup = Setup::new("install-run");
        let metadata = recorded();
        let cases: Vec<(&str, Vec<u8>, Vec<&str>)> = vec![
            ("nsis", nsis_installer(), vec!["/S"]),
            (
                "inno",
                inno_installer(),
                vec!["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"],
            ),
            ("seven-zip", seven_zip_installer(), vec!["/S"]),
        ];
        for (name, bytes, switches) in cases {
            let (path, info) = installer(&setup, name, &bytes);
            let run = plan_installer_run(&planner(&setup), &metadata, &request(&path, &info))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(run.method, InstallMethod::UnattendedInstaller);
            assert_eq!(run.arguments, switches);
            assert_eq!(run.plan.operation, Operation::Install);
            let mut expected = vec![path.as_str()];
            expected.extend(switches);
            assert_eq!(
                command_of(&setup, &run.plan),
                supervised(&expected),
                "{name}"
            );
        }
    }

    #[test]
    fn a_windows_installer_package_is_given_to_msiexec() {
        let setup = Setup::new("install-msi");
        let (path, info) = installer(&setup, "package", &testdata::ole(&ole::CLSID_MSI, 9));
        let run =
            plan_installer_run(&planner(&setup), &recorded(), &request(&path, &info)).unwrap();
        assert_eq!(run.arguments, ["/qn"]);
        let windows_path = format!("Z:{}", path.replace('/', "\\"));
        assert_eq!(
            command_of(&setup, &run.plan),
            supervised(&["msiexec", "/i", windows_path.as_str(), "/qn"])
        );
        // The package is shown to the sandbox, read-only, and nothing else
        // of its directory is.
        let Step::RunBounded { command, .. } = &run.plan.steps[0] else {
            panic!();
        };
        let shown = command
            .arguments
            .windows(3)
            .any(|window| window[0] == "--ro-bind" && window[1] == path && window[2] == path);
        assert!(shown, "{:?}", command.arguments);
    }

    #[test]
    fn a_portable_archive_is_unpacked_and_a_plain_program_copied_without_the_runtime() {
        let setup = Setup::new("install-portable");
        let metadata = recorded();
        let target = setup
            .app
            .prefix
            .join("drive_c/Program Files/example-app")
            .to_str()
            .unwrap()
            .to_owned();

        let (path, info) = installer(
            &setup,
            "archive",
            &testdata::zip(&["notepad++.exe", "updater/GUP.exe", "readme.txt"]),
        );
        let run = plan_installer_run(&planner(&setup), &metadata, &request(&path, &info)).unwrap();
        assert_eq!(run.method, InstallMethod::Unpacked);
        assert!(run.arguments.is_empty());
        assert_eq!(
            command_of(&setup, &run.plan),
            [
                "/usr/bin/unzip",
                "-q",
                "-o",
                path.as_str(),
                "-d",
                target.as_str()
            ]
        );

        // A plain program is not run as an installer...
        let (path, info) = installer(&setup, "tool", &image(0x8664, &[], b""));
        let error =
            plan_installer_run(&planner(&setup), &metadata, &request(&path, &info)).unwrap_err();
        assert_eq!(error.code(), Code::InstallerNoUnattendedMode);
        assert!(error.to_string().contains("--portable"));
        // ...unless the caller says it is the application itself.
        let portable = InstallRequest {
            portable: true,
            ..request(&path, &info)
        };
        let run = plan_installer_run(&planner(&setup), &metadata, &portable).unwrap();
        assert_eq!(run.method, InstallMethod::Copied);
        let copied = format!("{target}/tool.exe");
        assert_eq!(
            command_of(&setup, &run.plan),
            [
                "/usr/bin/install",
                "-D",
                "-m",
                "0644",
                "--",
                path.as_str(),
                copied.as_str()
            ]
        );

        // Without the programs that unpack and copy, the plan is refused.
        let mut bare = Setup::new("install-portable-bare");
        bare.host.unzip = None;
        bare.host.install = None;
        let (zip_path, zip_info) = installer(&bare, "archive", &testdata::zip(&["app.exe"]));
        let error = plan_installer_run(&planner(&bare), &metadata, &request(&zip_path, &zip_info))
            .unwrap_err();
        assert_eq!(error.code(), Code::ToolMissing);
        assert!(error.to_string().contains("unzip"));
        let (exe_path, exe_info) = installer(&bare, "tool", &image(0x8664, &[], b""));
        let portable = InstallRequest {
            portable: true,
            ..request(&exe_path, &exe_info)
        };
        let error = plan_installer_run(&planner(&bare), &metadata, &portable).unwrap_err();
        assert_eq!(error.code(), Code::ToolMissing);
    }

    #[test]
    fn installations_that_must_not_start_are_refused() {
        let mut setup = Setup::new("install-refused");
        let metadata = recorded();
        let (path, info) = installer(&setup, "setup", &nsis_installer());

        // An installer in a protected place, or one that is not there.
        let key = setup.machine.path(".ssh/id_ed25519");
        let hidden = InstallRequest {
            installer: &key,
            ..request(&path, &info)
        };
        let error = plan_installer_run(&planner(&setup), &metadata, &hidden).unwrap_err();
        assert_eq!(error.code(), Code::BinaryRefused);
        let absent = setup.machine.path("Games/absent.exe");
        let missing = InstallRequest {
            installer: &absent,
            ..request(&path, &info)
        };
        let error = plan_installer_run(&planner(&setup), &metadata, &missing).unwrap_err();
        assert_eq!(error.code(), Code::BinaryRefused);

        // A time limit of nothing, or of more than a day.
        for limit_seconds in [0, MAX_INSTALLER_SECONDS + 1] {
            let unlimited = InstallRequest {
                limit_seconds,
                ..request(&path, &info)
            };
            let error = plan_installer_run(&planner(&setup), &metadata, &unlimited).unwrap_err();
            assert_eq!(error.code(), Code::InstallRequestInvalid);
        }

        // An installer that the caller says is the application itself.
        let mistaken = InstallRequest {
            portable: true,
            ..request(&path, &info)
        };
        let error = plan_installer_run(&planner(&setup), &metadata, &mistaken).unwrap_err();
        assert_eq!(error.code(), Code::InstallRequestInvalid);
        assert!(error.to_string().contains("NSIS installer"), "{error}");

        // A prefix that is not Wine's.
        let proton = PrefixMetadata {
            runtime: RuntimeKind::Proton,
            runtime_version: "GE-Proton10-20".to_owned(),
            ..recorded()
        };
        let error =
            plan_installer_run(&planner(&setup), &proton, &request(&path, &info)).unwrap_err();
        assert_eq!(error.code(), Code::LaunchUnsupported);

        // No sandbox, no runtime, and never as root.
        setup.host.bwrap = None;
        let error =
            plan_installer_run(&planner(&setup), &metadata, &request(&path, &info)).unwrap_err();
        assert_eq!(error.code(), Code::SandboxMissing);
        let mut setup = Setup::new("install-refused-wine");
        setup.host.wine = None;
        let (path, info) = installer(&setup, "setup", &nsis_installer());
        let error =
            plan_installer_run(&planner(&setup), &metadata, &request(&path, &info)).unwrap_err();
        assert_eq!(error.code(), Code::WineMissing);
        setup.host.euid = 0;
        let error =
            plan_installer_run(&planner(&setup), &metadata, &request(&path, &info)).unwrap_err();
        assert_eq!(error.code(), Code::RunningAsRoot);
    }

    /// A program with version information and a two-colour icon.
    fn program(product: &str, version: &str, company: &str) -> Vec<u8> {
        let palette = [[0, 0, 0], [255, 0, 0]];
        let resources = vec![
            (3, 1, bitmap(4, 2, &palette, &[vec![1, 0], vec![0, 1]], &[])),
            (14, 1, group(&[(2, 4, 1)])),
            (
                16,
                1,
                version_resource(&[
                    ("CompanyName", company),
                    ("ProductName", product),
                    ("ProductVersion", version),
                ]),
            ),
        ];
        image(0x8664, &resources, b"")
    }

    /// A prefix after an installer like 7-Zip's has run in it.
    fn installed_prefix(setup: &Setup) -> Discovery {
        let prefix = &setup.app.prefix;
        put(prefix, "windows/system32/notepad.exe", b"MZ");
        fs::write(
            prefix.join("system.reg"),
            "WINE REGISTRY Version 2\n\n#arch=win64\n",
        )
        .unwrap();
        let before = snapshot(prefix);
        put(
            prefix,
            "Program Files/7-Zip/7zFM.exe",
            &program("7-Zip", "26.06", "Igor Pavlov"),
        );
        put(
            prefix,
            "Program Files/7-Zip/7z.exe",
            &image(0x8664, &[], b""),
        );
        put(
            prefix,
            "Program Files/7-Zip/Uninstall.exe",
            &image(0x8664, &[], b""),
        );
        link(
            prefix,
            &format!("{START_MENU}/7-Zip/7-Zip File Manager.lnk"),
            "C:\\Program Files\\7-Zip\\7zFM.exe",
        );
        fs::write(prefix.join("system.reg"), crate::winreg::tests::SYSTEM_REG).unwrap();
        discover(prefix, &before)
    }

    fn options(method: InstallMethod, arguments: &[&str]) -> RecordOptions<'static> {
        RecordOptions {
            name: None,
            main_executable: None,
            launcher: "tur-runtime",
            source: SourceRecord {
                file_name: "7z2606-x64.exe".to_owned(),
                sha256: "9b697f65ecc2d9b0152f5ee96d72dde522a7827690349d9307935dacf40f42ec"
                    .to_owned(),
                size: 1_659_704,
            },
            method,
            arguments: arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
            case_folding: None,
        }
    }

    fn info_of(kind: InstallerKind) -> InstallerInfo {
        InstallerInfo {
            kind,
            label: kind.label(),
            architecture: None,
            file_size: 0,
            evidence: "test".to_owned(),
            unattended: kind.unattended(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_finished_installation_is_described_from_what_is_in_the_prefix() {
        let setup = Setup::new("install-describe");
        let discovery = installed_prefix(&setup);
        let metadata = recorded();
        let installed = describe_installation(
            &planner(&setup),
            &metadata,
            &info_of(InstallerKind::SevenZipInstaller),
            &discovery,
            options(InstallMethod::UnattendedInstaller, &["/S"]),
        )
        .unwrap();

        let record = &installed.record;
        record.validate().unwrap();
        assert_eq!(record.app_id, "com.example.app");
        // The installer's own words, from its uninstall entry.
        assert_eq!(record.name, "7-Zip 26.06 (x64)");
        assert_eq!(record.version.as_deref(), Some("26.06"));
        assert_eq!(record.publisher.as_deref(), Some("Igor Pavlov"));
        let main = setup
            .app
            .prefix
            .join("drive_c/Program Files/7-Zip/7zFM.exe");
        assert_eq!(record.main_executable, main.to_str().unwrap());
        assert_eq!(
            record.main_executable_windows,
            "C:\\Program Files\\7-Zip\\7zFM.exe"
        );
        assert!(record.main_executable_reason.contains("7-Zip File Manager"));
        assert_eq!(record.prefix, setup.app.prefix.to_str().unwrap());
        assert_eq!(record.prefix_id, "example-app");
        assert_eq!(record.runtime, RuntimeKind::Wine);
        assert_eq!(record.runtime_version, "wine-9.0");
        assert_eq!(record.installed_at, NOW);
        assert_eq!(record.install.kind, InstallerKind::SevenZipInstaller);
        assert_eq!(record.install.method, InstallMethod::UnattendedInstaller);
        assert_eq!(record.install.arguments, ["/S"]);
        assert_eq!(record.install.source.file_name, "7z2606-x64.exe");
        assert_eq!(
            record.uninstall,
            UninstallRecord {
                method: UninstallMethod::Uninstaller,
                program: Some("C:\\Program Files\\7-Zip\\Uninstall.exe".to_owned()),
                arguments: vec!["/S".to_owned()],
                registry_key: Some("HKLM\\7-Zip".to_owned()),
            }
        );
        assert_eq!(
            record.executables,
            [
                "C:\\Program Files\\7-Zip\\7z.exe",
                "C:\\Program Files\\7-Zip\\7zFM.exe",
                "C:\\Program Files\\7-Zip\\Uninstall.exe",
            ]
        );
        assert_eq!(
            record.shortcuts,
            [ShortcutRecord {
                name: "7-Zip File Manager".to_owned(),
                target: "C:\\Program Files\\7-Zip\\7zFM.exe".to_owned(),
                arguments: None,
            }]
        );

        // The profile launches the main program from this prefix, and makes
        // no claim that it works.
        let profile = &installed.profile;
        assert_eq!(profile.app_id, "com.example.app");
        assert_eq!(profile.display_name, "7-Zip 26.06 (x64)");
        assert_eq!(profile.binary, record.main_executable);
        assert_eq!(profile.format, ProfileFormat::Pe);
        assert_eq!(profile.architecture, ProfileArch::X86_64);
        assert_eq!(profile.runtime, ProfileRuntime::Wine);
        assert_eq!(profile.runtime_version, None);
        assert_eq!(profile.prefix_id.as_deref(), Some("example-app"));
        assert_eq!(profile.test_status, TestStatus::Untested);
        assert!(profile.known_requirements.is_empty());
        assert!(installed.notes.is_empty());

        // The launcher entry and the icon, both staged under the runtime
        // manager's own directory.
        let launchers = setup.app.launchers.to_str().unwrap();
        assert_eq!(
            record.launcher.desktop_entry,
            format!("{launchers}/tur-wine-com.example.app.desktop")
        );
        let icon_path = format!("{launchers}/tur-wine-com.example.app.png");
        assert_eq!(record.launcher.icon.as_deref(), Some(icon_path.as_str()));
        assert!(!record.launcher.installed);
        assert_eq!(record.profile, setup.app.profile.to_str().unwrap());
        let contents = &installed.entry.contents;
        assert!(contents.contains("Name=7-Zip 26.06 (x64)\n"));
        assert!(contents.contains(&format!("Icon={icon_path}\n")));
        assert!(contents.contains("StartupWMClass=7zfm.exe\n"));
        assert!(contents.contains(&format!("Exec=tur-runtime launch \"{}\"\n", record.profile)));
        let icon = installed.icon.as_ref().unwrap();
        assert_eq!((icon.width, icon.height), (2, 2));
        let (_, _, pixels) = read_png(&icon.png);
        assert_eq!(&pixels[..4], [255, 0, 0, 255]);
        assert_eq!(
            launcher_destinations(&installed),
            (
                "applications/tur-wine-com.example.app.desktop".to_owned(),
                Some("icons/hicolor/2x2/apps/tur-wine-com.example.app.png".to_owned())
            )
        );

        // The plan writes the profile, the icon, the entry and, last, the
        // record; nothing else.
        let plan = plan_record(&planner(&setup), &installed);
        let written: Vec<&Path> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                Step::WriteFile { path, .. } | Step::WriteBytes { path, .. } => {
                    Some(path.as_path())
                }
                Step::CreateDir { .. } => None,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            written,
            [
                setup.app.profile.as_path(),
                Path::new(&icon_path),
                Path::new(&record.launcher.desktop_entry),
                setup.app.record.as_path(),
            ]
        );
        assert_eq!(
            plan.steps[0],
            Step::CreateDir {
                path: setup.app.launchers.clone()
            }
        );
        let Some(Step::WriteFile { contents, .. }) = plan.steps.last() else {
            panic!();
        };
        assert_eq!(crate::app_record::parse_record(contents).unwrap(), *record);
        assert!(
            plan.notes[0].contains("$XDG_DATA_HOME/applications/tur-wine-com.example.app.desktop")
        );
        assert!(plan.notes[0].contains("icons/hicolor/2x2/apps/"));
        let report = run_plan(&plan, &mut FakeExecutor::default());
        assert!(report.succeeded());
    }

    #[test]
    fn names_fall_back_to_the_program_and_callers_can_decide() {
        let setup = Setup::new("install-names");
        let prefix = &setup.app.prefix;
        fs::create_dir_all(prefix).unwrap();
        let before = snapshot(prefix);
        put(
            prefix,
            "Program Files/example-app/putty.exe",
            &program("PuTTY suite", "Release 0.85", "Simon Tatham"),
        );
        put(
            prefix,
            "Program Files/example-app/puttygen.exe",
            &image(0x8664, &[], b""),
        );
        let discovery = discover(prefix, &before);
        let describe = |options: RecordOptions<'_>| {
            describe_installation(
                &planner(&setup),
                &recorded(),
                &info_of(InstallerKind::PortableZip),
                &discovery,
                options,
            )
        };

        // Two programs side by side and nothing to tell them apart.
        let error = describe(options(InstallMethod::Unpacked, &[])).unwrap_err();
        assert_eq!(error.code(), Code::InstallMainProgramUnclear);
        let message = error.to_string();
        assert!(message.contains("--main-exe"), "{message}");
        assert!(
            message.contains("C:\\Program Files\\example-app\\putty.exe"),
            "{message}"
        );

        // Named by the caller: the record takes the program's own words.
        let named = RecordOptions {
            main_executable: Some("putty.exe"),
            ..options(InstallMethod::Unpacked, &[])
        };
        let installed = describe(named).unwrap();
        assert_eq!(installed.record.name, "PuTTY suite");
        assert_eq!(installed.record.version.as_deref(), Some("Release 0.85"));
        assert_eq!(installed.record.publisher.as_deref(), Some("Simon Tatham"));
        assert!(
            installed
                .record
                .main_executable_reason
                .contains("named by the caller")
        );
        // Nothing to run on removal: there was no installer.
        assert_eq!(
            installed.record.uninstall.method,
            UninstallMethod::RemovePrefix
        );
        assert!(installed.record.shortcuts.is_empty());

        // The caller's name wins; a name that is not allowed is refused.
        let renamed = RecordOptions {
            name: Some("My terminal"),
            main_executable: Some("putty.exe"),
            ..options(InstallMethod::Unpacked, &[])
        };
        assert_eq!(describe(renamed).unwrap().record.name, "My terminal");
        let bad = RecordOptions {
            name: Some("line\nbreak"),
            main_executable: Some("putty.exe"),
            ..options(InstallMethod::Unpacked, &[])
        };
        assert_eq!(
            describe(bad).unwrap_err().code(),
            Code::InstallRequestInvalid
        );
        let absent = RecordOptions {
            main_executable: Some("absent.exe"),
            ..options(InstallMethod::Unpacked, &[])
        };
        let error = describe(absent).unwrap_err();
        assert_eq!(error.code(), Code::InstallMainProgramUnclear);
        assert!(error.to_string().contains("\"absent.exe\""));

        // A program with no version information and no icon: its file name,
        // and the theme's generic icon.
        // What became of case folding is recorded as it was reported.
        let folded = RecordOptions {
            main_executable: Some("putty.exe"),
            case_folding: Some(CaseFoldRecord::attempted(
                crate::casefold::CaseFoldReason::Casefolded,
                None,
            )),
            ..options(InstallMethod::Unpacked, &[])
        };
        let record = describe(folded).unwrap().record;
        assert!(record.case_folding.as_ref().unwrap().casefolded);
        assert!(record.to_json().contains("\"reason\": \"casefolded\""));

        let bare = RecordOptions {
            main_executable: Some("puttygen.exe"),
            ..options(InstallMethod::Unpacked, &[])
        };
        let installed = describe(bare).unwrap();
        assert_eq!(installed.record.name, "puttygen");
        assert_eq!(installed.record.version, None);
        assert_eq!(installed.record.publisher, None);
        assert_eq!(installed.record.launcher.icon, None);
        assert_eq!(installed.icon, None);
        assert!(
            installed
                .entry
                .contents
                .contains("Icon=application-x-ms-dos-executable\n")
        );
        let plan = plan_record(&planner(&setup), &installed);
        assert!(
            !plan
                .steps
                .iter()
                .any(|step| matches!(step, Step::WriteBytes { .. }))
        );
    }

    #[test]
    fn an_installer_that_installs_nothing_or_no_program_is_reported() {
        let setup = Setup::new("install-nothing");
        let prefix = &setup.app.prefix;
        fs::create_dir_all(prefix).unwrap();
        let before = snapshot(prefix);
        let describe = |discovery: &Discovery| {
            describe_installation(
                &planner(&setup),
                &recorded(),
                &info_of(InstallerKind::Nsis),
                discovery,
                options(InstallMethod::UnattendedInstaller, &["/S"]),
            )
        };
        let error = describe(&discover(prefix, &before)).unwrap_err();
        assert_eq!(error.code(), Code::InstallNothingFound);

        // A file named like a program that is not one.
        put(prefix, "Program Files/App/app.exe", b"not a program");
        let error = describe(&discover(prefix, &before)).unwrap_err();
        assert_eq!(error.code(), Code::BinaryRefused);
        // A .NET program is installed, and said to need what it needs.
        let managed = testdata::PeSpec {
            clr: true,
            ..testdata::PeSpec::windows_gui_x86_64()
        }
        .build();
        put(prefix, "Program Files/App/app.exe", &managed);
        let installed = describe(&discover(prefix, &before)).unwrap();
        assert_eq!(installed.profile.known_requirements, ["dotnet"]);
        assert!(installed.notes[0].contains(".NET"), "{:?}", installed.notes);
        // A library named like a program.
        let library = testdata::PeSpec {
            characteristics: 0x2022,
            ..testdata::PeSpec::windows_gui_x86_64()
        }
        .build();
        put(prefix, "Program Files/App/app.exe", &library);
        let error = describe(&discover(prefix, &before)).unwrap_err();
        assert_eq!(error.code(), Code::BinaryRefused);
    }

    #[test]
    fn uninstall_commands_are_taken_apart() {
        assert_eq!(
            split_command("\"C:\\Program Files\\7-Zip\\Uninstall.exe\""),
            Some((
                "C:\\Program Files\\7-Zip\\Uninstall.exe".to_owned(),
                Vec::new()
            ))
        );
        assert_eq!(
            split_command("C:\\Program Files\\App\\unins000.exe /SILENT \"/LOG=C:\\a b.txt\"  x"),
            Some((
                "C:\\Program Files\\App\\unins000.exe".to_owned(),
                vec![
                    "/SILENT".to_owned(),
                    "/LOG=C:\\a b.txt".to_owned(),
                    "x".to_owned()
                ]
            ))
        );
        assert_eq!(
            split_command("MsiExec.exe /X{E5C4B1A0-0000-4000-8000-0123456789AB}"),
            Some((
                "MsiExec.exe".to_owned(),
                vec!["/X{E5C4B1A0-0000-4000-8000-0123456789AB}".to_owned()]
            ))
        );
        for bad in [
            "",
            "   ",
            "\"unterminated",
            "no-extension /S",
            "tool.exeextra /S",
        ] {
            assert_eq!(split_command(bad), None, "{bad:?}");
        }
        assert!(is_product_code("{E5C4B1A0-0000-4000-8000-0123456789AB}"));
        for bad in [
            "",
            "E5C4B1A0-0000-4000-8000-0123456789AB",
            "{E5C4B1A0-0000-4000-8000-0123456789A}",
            "{E5C4B1A0-0000-4000-8000-0123456789AB} /qn",
            "{E5C4B1A0-0000-4000-8000-0123456789AZ}",
        ] {
            assert!(!is_product_code(bad), "{bad:?}");
        }
        assert!(is_c_drive_path("C:\\Program Files\\App\\unins000.exe"));
        assert!(is_c_drive_path("c:/app/u.exe"));
        for bad in [
            "Z:\\usr\\bin\\true",
            "C:\\app\\..\\..\\x.exe",
            "\\\\host\\x.exe",
            "app.exe",
        ] {
            assert!(!is_c_drive_path(bad), "{bad:?}");
        }
    }

    fn entry(key: &str, uninstall: Option<&str>, windows_installer: bool) -> UninstallEntry {
        UninstallEntry {
            hive: Some(crate::winreg::Hive::Machine),
            key: key.to_owned(),
            display_name: Some("App".to_owned()),
            uninstall_string: uninstall.map(str::to_owned),
            windows_installer,
            ..UninstallEntry::default()
        }
    }

    #[test]
    fn the_way_an_application_is_removed_follows_from_its_uninstall_entry() {
        const CODE: &str = "{E5C4B1A0-0000-4000-8000-0123456789AB}";
        let record = |kind, entry: &UninstallEntry| uninstall_record(kind, Some(entry));

        // Inno Setup: its uninstaller, with the documented silent switches
        // added to whatever the entry already passes.
        let inno = record(
            InstallerKind::InnoSetup,
            &entry(
                "App_is1",
                Some("\"C:\\Program Files\\App\\unins000.exe\" /norestart"),
                false,
            ),
        );
        assert_eq!(inno.method, UninstallMethod::Uninstaller);
        assert_eq!(
            inno.program.as_deref(),
            Some("C:\\Program Files\\App\\unins000.exe")
        );
        assert_eq!(
            inno.arguments,
            ["/norestart", "/VERYSILENT", "/SUPPRESSMSGBOXES"]
        );
        assert_eq!(inno.registry_key.as_deref(), Some("HKLM\\App_is1"));

        // Windows Installer: the product code, from a package or from an
        // installer program that wraps one.
        for (kind, uninstall, flagged) in [
            (
                InstallerKind::Msi,
                Some("MsiExec.exe /X{E5C4B1A0-0000-4000-8000-0123456789AB}"),
                true,
            ),
            (InstallerKind::Msi, None, true),
            (
                InstallerKind::Nsis,
                Some("MsiExec.exe /I{E5C4B1A0-0000-4000-8000-0123456789AB}"),
                false,
            ),
        ] {
            let msi = record(kind, &entry(CODE, uninstall, flagged));
            assert_eq!(msi.method, UninstallMethod::MsiProduct, "{uninstall:?}");
            assert_eq!(msi.program.as_deref(), Some(CODE));
            assert_eq!(msi.arguments, ["/qn"]);
        }

        // Nothing to run: no entry, no command, a command outside the
        // prefix's C: drive, msiexec without a product code, or a kind that
        // has no installer.
        let nothing = [
            uninstall_record(InstallerKind::Nsis, None),
            record(InstallerKind::Nsis, &entry("App", None, false)),
            record(
                InstallerKind::Nsis,
                &entry("App", Some("Z:\\bin\\sh.exe -c x"), false),
            ),
            record(
                InstallerKind::Nsis,
                &entry("App", Some("C:\\a\\..\\..\\u.exe"), false),
            ),
            record(
                InstallerKind::Nsis,
                &entry("App", Some("msiexec.exe /x something"), false),
            ),
            record(
                InstallerKind::PortableZip,
                &entry("App", Some("\"C:\\App\\uninstall.exe\""), false),
            ),
        ];
        for found in nothing {
            assert_eq!(found.method, UninstallMethod::RemovePrefix, "{found:?}");
            assert_eq!(found.program, None);
            assert!(found.arguments.is_empty());
        }
    }

    #[test]
    fn the_uninstaller_runs_in_the_sandbox_with_a_time_limit() {
        let setup = Setup::new("uninstall-run");
        let metadata = recorded();
        let base = crate::app_record::tests::full();
        let with = |method, program: Option<&str>, arguments: &[&str]| AppRecord {
            uninstall: UninstallRecord {
                method,
                program: program.map(str::to_owned),
                arguments: arguments
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
                registry_key: None,
            },
            ..base.clone()
        };

        let record = with(
            UninstallMethod::Uninstaller,
            Some("C:\\Program Files\\7-Zip\\Uninstall.exe"),
            &["/S"],
        );
        let plan = plan_uninstaller(&planner(&setup), &metadata, &record, 900)
            .unwrap()
            .unwrap();
        assert_eq!(plan.operation, Operation::Uninstall);
        assert_eq!(
            command_of(&setup, &plan),
            supervised(&["C:\\Program Files\\7-Zip\\Uninstall.exe", "/S"])
        );

        let record = with(
            UninstallMethod::MsiProduct,
            Some("{E5C4B1A0-0000-4000-8000-0123456789AB}"),
            &["/qn"],
        );
        let plan = plan_uninstaller(&planner(&setup), &metadata, &record, 900)
            .unwrap()
            .unwrap();
        assert_eq!(
            command_of(&setup, &plan),
            supervised(&[
                "msiexec",
                "/x",
                "{E5C4B1A0-0000-4000-8000-0123456789AB}",
                "/qn"
            ])
        );

        // No uninstaller: nothing is planned.
        let record = with(UninstallMethod::RemovePrefix, None, &[]);
        assert_eq!(
            plan_uninstaller(&planner(&setup), &metadata, &record, 900),
            Ok(None)
        );

        // A record edited to run something else is refused.
        for (method, program) in [
            (UninstallMethod::Uninstaller, "Z:\\usr\\bin\\rm"),
            (UninstallMethod::Uninstaller, "C:\\app\\..\\..\\x.exe"),
            (UninstallMethod::MsiProduct, "C:\\app\\u.exe"),
            (UninstallMethod::MsiProduct, "{not-a-code}"),
        ] {
            let record = with(method, Some(program), &[]);
            let error = plan_uninstaller(&planner(&setup), &metadata, &record, 900).unwrap_err();
            assert_eq!(error.code(), Code::AppRecordInvalid, "{program}");
        }
        let record = with(UninstallMethod::Uninstaller, Some("C:\\app\\u.exe"), &[]);
        let error = plan_uninstaller(&planner(&setup), &metadata, &record, 0).unwrap_err();
        assert_eq!(error.code(), Code::InstallRequestInvalid);
    }

    #[test]
    fn removal_deletes_everything_kept_for_the_application_and_checks_it() {
        let setup = Setup::new("uninstall-remove");
        let app = &setup.app;
        let kept = planner(&setup).remove(true).unwrap();
        let plan = plan_removal(&planner(&setup), false).unwrap();
        assert_eq!(plan.operation, Operation::Uninstall);
        // The same paths `prefix remove` deletes.
        assert_eq!(plan.steps, kept.steps);
        assert_eq!(
            application_paths(app, false),
            [
                app.app_dir.clone(),
                app.staging.clone(),
                app.logs.clone(),
                app.launchers.clone()
            ]
        );
        assert!(plan.notes[0].contains("are kept"));
        let with_backups = plan_removal(&planner(&setup), true).unwrap();
        assert_eq!(
            with_backups.steps.last(),
            Some(&Step::RemoveTree {
                path: app.backups.clone()
            })
        );
        assert_eq!(with_backups.steps.len(), 5);

        // The check names every deleted path, and only those.
        for remove_backups in [false, true] {
            let check = plan_removal_check(&planner(&setup), remove_backups);
            let checked: Vec<PathBuf> = check
                .steps
                .iter()
                .map(|step| match step {
                    Step::Check {
                        check: Check::Absent { path },
                    } => path.clone(),
                    other => panic!("{other:?}"),
                })
                .collect();
            assert_eq!(checked, application_paths(app, remove_backups));
        }
        // Something left behind is a finding.
        let mut executor = FakeExecutor {
            failing_checks: vec!["launchers".to_owned()],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan_removal_check(&planner(&setup), false), &mut executor);
        assert_eq!(report.findings.len(), 1);
        assert!(report.findings[0].expected.ends_with("is gone"));

        // A failed installation is rolled back; its logs stay.
        let rollback = plan_rollback(&planner(&setup));
        assert_eq!(
            rollback.steps,
            [
                Step::RemoveTree {
                    path: app.app_dir.clone()
                },
                Step::RemoveTree {
                    path: app.staging.clone()
                },
                Step::RemoveTree {
                    path: app.launchers.clone()
                },
            ]
        );

        let mut root = Setup::new("uninstall-root");
        root.host.euid = 0;
        assert_eq!(
            plan_removal(&planner(&root), false).unwrap_err().code(),
            Code::RunningAsRoot
        );
    }

    #[test]
    fn what_an_uninstaller_left_is_surveyed() {
        let setup = Setup::new("uninstall-survey");
        let discovery = installed_prefix(&setup);
        let installed = describe_installation(
            &planner(&setup),
            &recorded(),
            &info_of(InstallerKind::SevenZipInstaller),
            &discovery,
            options(InstallMethod::UnattendedInstaller, &["/S"]),
        )
        .unwrap();
        let record = &installed.record;
        // Nothing has been uninstalled: everything is still there.
        let before = survey_leftovers(&setup.app, record);
        assert!(before.main_executable_present);
        assert!(before.uninstall_entry_present);
        assert_eq!(before.programs_left, record.executables);

        // An uninstaller that removed its programs and its entry, and left
        // one file.
        let directory = setup.app.prefix.join("drive_c/Program Files/7-Zip");
        fs::remove_file(directory.join("7zFM.exe")).unwrap();
        fs::remove_file(directory.join("Uninstall.exe")).unwrap();
        fs::write(
            setup.app.prefix.join("system.reg"),
            "WINE REGISTRY Version 2\n\n#arch=win64\n",
        )
        .unwrap();
        assert_eq!(
            survey_leftovers(&setup.app, record),
            UninstallerLeftovers {
                main_executable_present: false,
                uninstall_entry_present: false,
                programs_left: vec!["C:\\Program Files\\7-Zip\\7z.exe".to_owned()],
            }
        );
    }

    #[test]
    fn installed_applications_are_listed_from_their_records() {
        let setup = Setup::new("install-list");
        let dirs = &setup.machine.dirs;
        assert_eq!(installed_applications(dirs), (Vec::new(), Vec::new()));

        let write = |app_id: &str, contents: Option<String>| {
            let app = dirs.app(app_id).unwrap();
            fs::create_dir_all(&app.app_dir).unwrap();
            if let Some(contents) = contents {
                fs::write(&app.record, contents).unwrap();
            }
            app
        };
        let record_for = |app: &AppPaths| {
            let prefix = app.prefix.to_str().unwrap().to_owned();
            AppRecord {
                app_id: app.app_id.clone(),
                main_executable: format!("{prefix}/drive_c/App/app.exe"),
                prefix,
                profile: app.profile.to_str().unwrap().to_owned(),
                ..crate::app_record::tests::minimal()
            }
        };
        let second = dirs.app("org.example.second").unwrap();
        let first = dirs.app("org.example.first").unwrap();
        write("org.example.second", Some(record_for(&second).to_json()));
        write("org.example.first", Some(record_for(&first).to_json()));
        // A prefix without a record, a record that is not valid, and a
        // directory that is not an application's.
        write("org.example.bare", None);
        write("org.example.broken", Some("{}".to_owned()));
        fs::create_dir_all(dirs.prefixes().join("Not An Id")).unwrap();

        let (records, problems) = installed_applications(dirs);
        let ids: Vec<&str> = records
            .iter()
            .map(|record| record.app_id.as_str())
            .collect();
        assert_eq!(ids, ["org.example.first", "org.example.second"]);
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].starts_with("org.example.broken: "),
            "{problems:?}"
        );
    }
}
