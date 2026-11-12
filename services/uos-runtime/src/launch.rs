// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The launch plan (specification §7.2, §8.2, §12.2, §15.2).
//!
//! [`plan_launch`] turns an application profile, the prefix's metadata and
//! the host's facts into everything a launch is: the command with its
//! program, arguments, environment and working directory; the sandbox it runs
//! in; the synchronisation decision; and the steps to carry out, including
//! the creation of the prefix when the application has none.
//!
//! A launch never changes the runtime version of a prefix by itself. When
//! the installed runtime is not the one the prefix is on, the launch is
//! refused, unless the caller asks for a migration, which takes a backup
//! first. A profile that pins a runtime version is refused when another
//! version is installed.

use std::path::PathBuf;

use serde::Serialize;

use crate::grant::ProgramFile;
use crate::launch_host::RuntimeVersion;
use crate::metadata::{PrefixMetadata, RuntimeKind, SyncRecord};
use crate::plan::{CommandSpec, Operation, Plan, Step};
use crate::prefix::{Planner, PrefixSettings};
use crate::profile::{AppProfile, GraphicsBackend, ProfileFormat, ProfileRuntime, SyncBackend};
use crate::runtime_cmd::{Invocation, PlanError, Task, assemble, decide_sync, dll_overrides};
use crate::sandbox::Mount;

/// What the caller asks of one launch.
#[derive(Debug, Clone, Copy)]
pub struct LaunchRequest<'a> {
    /// The application's profile, already validated.
    pub profile: &'a AppProfile,
    /// The installed Wine's version. Not used for Proton, whose version is
    /// the one the profile pins.
    pub installed: Option<&'a RuntimeVersion>,
    /// The metadata of the application's prefix, when it has one.
    pub existing: Option<&'a PrefixMetadata>,
    /// Arguments passed on to the application.
    pub arguments: &'a [String],
    /// Environment variables added to this launch.
    pub extra_environment: &'a [(String, String)],
    /// The value of `WINEDEBUG`.
    pub wine_debug: &'a str,
    /// Whether to migrate the prefix when it is on another runtime version.
    pub migrate: bool,
    /// The identifier of the launching process, which names the log file.
    pub process_id: u32,
}

/// Everything a launch is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchPlan {
    /// The runtime.
    pub runtime: RuntimeKind,
    /// The runtime version the launch uses.
    pub runtime_version: String,
    /// The prefix.
    pub prefix: PathBuf,
    /// The file the application's output is captured to.
    pub log: PathBuf,
    /// The command as the application's side of the sandbox sees it:
    /// program, arguments, environment and working directory.
    pub application: CommandSpec,
    /// The DLL overrides in effect (plain Wine only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dll_overrides: Option<String>,
    /// The Direct3D translation setting.
    pub graphics_backend: GraphicsBackend,
    /// The synchronisation decision, with the probe result and the reason.
    pub sync: SyncRecord,
    /// The sandbox's file-system view.
    pub sandbox: Vec<Mount>,
    /// Whether the prefix is created by this launch.
    pub creates_prefix: bool,
    /// Whether the prefix is migrated by this launch.
    pub migrates_prefix: bool,
    /// The steps to carry out. The last one starts the application.
    pub plan: Plan,
}

fn unsupported(reason: &str) -> PlanError {
    PlanError::Unsupported {
        reason: reason.to_owned(),
    }
}

fn owner(detail: String) -> PlanError {
    PlanError::PrefixOwner { detail }
}

/// Plans a launch.
///
/// # Errors
///
/// A [`PlanError`] with a stable code when the launch is refused: the user
/// is root, the profile is not one this build launches, the runtime or the
/// sandbox is missing, the prefix belongs to something else, a pinned or
/// recorded runtime version does not match, a grant or the program file is
/// refused.
pub fn plan_launch(
    planner: &Planner<'_>,
    request: &LaunchRequest<'_>,
) -> Result<LaunchPlan, PlanError> {
    let host = planner.host;
    let app = planner.app;
    let profile = request.profile;
    if host.euid == 0 {
        return Err(PlanError::RunningAsRoot);
    }

    // What the profile asks for.
    let runtime = match profile.runtime {
        ProfileRuntime::Wine => RuntimeKind::Wine,
        ProfileRuntime::Proton => RuntimeKind::Proton,
        ProfileRuntime::Native
        | ProfileRuntime::Darling
        | ProfileRuntime::Android
        | ProfileRuntime::WindowsVm => {
            return Err(unsupported(
                "only the wine and proton runtimes are launched by this build",
            ));
        }
    };
    match profile.format {
        ProfileFormat::Pe => {}
        ProfileFormat::Msi => {
            return Err(unsupported(
                "installer packages are not run yet; only programs are",
            ));
        }
        _ => return Err(unsupported("the binary is not a Windows program")),
    }
    if profile.app_id != app.app_id {
        return Err(owner(format!(
            "the profile is for {} and the paths are for {}",
            profile.app_id, app.app_id
        )));
    }
    let prefix_id = profile
        .prefix_id
        .as_deref()
        .ok_or_else(|| unsupported("the profile names no prefix"))?;

    // The runtime version this launch would use.
    let installed = match runtime {
        RuntimeKind::Wine => {
            let installed = request.installed.ok_or(PlanError::VersionUnknown)?;
            if let Some(pinned) = &profile.runtime_version
                && *pinned != installed.version
            {
                return Err(PlanError::PinMismatch {
                    pinned: pinned.clone(),
                    installed: installed.version.clone(),
                });
            }
            installed.clone()
        }
        // umu-run fetches the build it is told to use, so the pin is the
        // version. Without a pin it would follow the latest build.
        RuntimeKind::Proton => {
            let pinned = profile
                .runtime_version
                .as_ref()
                .ok_or(PlanError::ProtonUnpinned)?;
            RuntimeVersion {
                version: pinned.clone(),
                build: pinned.clone(),
            }
        }
    };

    // The prefix: reuse it, migrate it when asked, or create it.
    let mut steps = Vec::new();
    let mut notes = Vec::new();
    let mut creates_prefix = false;
    let mut migrates_prefix = false;
    let current = match request.existing {
        Some(existing) => {
            if existing.app_id != profile.app_id {
                return Err(owner(format!(
                    "it belongs to application {}",
                    existing.app_id
                )));
            }
            if existing.prefix_id != prefix_id {
                return Err(owner(format!(
                    "it is named {} and the profile asks for {prefix_id}",
                    existing.prefix_id
                )));
            }
            if existing.runtime != runtime {
                return Err(owner(format!(
                    "it was created for {} and the profile asks for {}",
                    existing.runtime.label(),
                    runtime.label()
                )));
            }
            if existing.runtime_version == installed.version {
                existing.clone()
            } else if request.migrate {
                let (migration, moved) = planner.migrate_steps(existing, &installed)?;
                steps.extend(migration);
                migrates_prefix = true;
                notes.push(format!(
                    "The prefix is migrated from {} to {} after a backup.",
                    existing.runtime_version, installed.version
                ));
                moved
            } else {
                return Err(PlanError::RuntimeMismatch {
                    recorded: existing.runtime_version.clone(),
                    installed: installed.version.clone(),
                });
            }
        }
        None => {
            let settings = PrefixSettings {
                prefix_id: prefix_id.to_owned(),
                runtime,
                version: installed.clone(),
                pinned: false,
                graphics: profile.graphics_backend.unwrap_or(GraphicsBackend::Auto),
                network: true,
                grants: Vec::new(),
            };
            let (creation, created) = planner.create(&settings, false)?;
            steps.extend(creation.steps);
            creates_prefix = true;
            notes.push("The application has no prefix yet; one is created first.".to_owned());
            created
        }
    };

    // The program file.
    let program = planner
        .context
        .admit_program(&profile.binary, app)
        .map_err(|refusal| PlanError::Program {
            path: profile.binary.clone(),
            refusal,
        })?;
    let outside_prefix = matches!(program, ProgramFile::Outside(_));

    // Settings for this launch, and the record of them.
    let graphics = profile.graphics_backend.unwrap_or(current.graphics_backend);
    let sync = decide_sync(
        profile.sync_backend.unwrap_or(SyncBackend::Auto),
        runtime,
        &installed.version,
        host.ntsync,
        planner.now,
    );
    let record = PrefixMetadata {
        last_run_at: Some(planner.now),
        launch_count: current.launch_count.saturating_add(1),
        graphics_backend: graphics,
        sync: sync.clone(),
        ..current
    };
    record.validate()?;

    let assembled = assemble(
        &Invocation {
            host,
            app,
            context: planner.context,
            runtime,
            runtime_version: &installed.version,
            architecture: record.architecture,
            graphics,
            sync: &sync,
            grants: &record.grants,
            network: record.network,
            wine_debug: request.wine_debug,
            extra_environment: request.extra_environment,
        },
        Task::Application {
            program: program.path(),
            outside_prefix,
            arguments: request.arguments,
        },
    )?;

    let log = app.logs.join(format!(
        "{}-{}.log",
        planner.now.compact(),
        request.process_id
    ));
    steps.push(Step::CreateDir {
        path: app.logs.clone(),
    });
    // The record is written before the application starts and is not visible
    // to it: the last-run time is the time the launch began.
    steps.push(Step::WriteFile {
        path: app.metadata.clone(),
        contents: record.to_json(),
    });
    steps.push(Step::Launch {
        command: assembled.sandboxed,
        log: log.clone(),
    });

    notes.push(format!("Synchronisation: {}", sync.reason));
    let dll_overrides = match runtime {
        RuntimeKind::Wine => Some(dll_overrides(graphics, false)),
        RuntimeKind::Proton => None,
    };
    match (runtime, graphics) {
        (RuntimeKind::Wine, GraphicsBackend::Dxvk | GraphicsBackend::Vkd3dProton) => notes.push(
            "The DLL overrides prefer the translation layer's DLLs. Installing them into the \
             prefix is not built yet; without them Wine's own implementation is used."
                .to_owned(),
        ),
        (RuntimeKind::Proton, GraphicsBackend::Dxvk | GraphicsBackend::Vkd3dProton) => notes.push(
            "Proton uses DXVK and VKD3D-Proton by default; nothing is overridden.".to_owned(),
        ),
        _ => {}
    }
    if host.sockets.x11.is_none() && host.sockets.wayland.is_none() {
        notes.push("No display was found; a graphical program will not show a window.".to_owned());
    }

    Ok(LaunchPlan {
        runtime,
        runtime_version: installed.version,
        prefix: app.prefix.clone(),
        log,
        application: assembled.application,
        dll_overrides,
        graphics_backend: graphics,
        sync,
        sandbox: assembled.mounts,
        creates_prefix,
        migrates_prefix,
        plan: Plan {
            operation: Operation::Launch,
            app_id: app.app_id.clone(),
            steps,
            notes,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::Code;
    use crate::grant::GrantRefusal;
    use crate::metadata::{Grant, GrantAccess, NtsyncDevice, SyncMode, parse_metadata};
    use crate::plan::fake::FakeExecutor;
    use crate::plan::run_plan;
    use crate::prefix::tests::{planner, recorded, wine_9, wine_11};
    use crate::profile::{ProfileArch, TestStatus, parse_profile};
    use crate::runtime_cmd::DEFAULT_WINE_DEBUG;
    use crate::runtime_cmd::tests::{NOW, Setup};
    use crate::sandbox::tests::{assert_boundary, parse};

    fn profile(setup: &Setup) -> AppProfile {
        let text = format!(
            r#"{{
                "schema_version": 1,
                "app_id": "com.example.app",
                "display_name": "Example",
                "binary": "{}",
                "format": "pe",
                "architecture": "x86_64",
                "runtime": "wine",
                "prefix_id": "example-app",
                "isolation_profile": "desktop-standard",
                "fallback": "none",
                "test_status": "untested"
            }}"#,
            setup.machine.path("Games/setup.exe")
        );
        let profile = parse_profile(&text).unwrap();
        assert_eq!(profile.architecture, ProfileArch::X86_64);
        assert_eq!(profile.test_status, TestStatus::Untested);
        profile
    }

    fn request<'a>(
        profile: &'a AppProfile,
        installed: &'a RuntimeVersion,
        existing: Option<&'a PrefixMetadata>,
    ) -> LaunchRequest<'a> {
        LaunchRequest {
            profile,
            installed: Some(installed),
            existing,
            arguments: &[],
            extra_environment: &[],
            wine_debug: DEFAULT_WINE_DEBUG,
            migrate: false,
            process_id: 4242,
        }
    }

    fn launch_step(plan: &LaunchPlan) -> (&CommandSpec, &PathBuf) {
        match plan.plan.steps.last() {
            Some(Step::Launch { command, log }) => (command, log),
            other => panic!("the last step is {other:?}"),
        }
    }

    #[test]
    fn launching_into_an_existing_prefix() {
        let setup = Setup::new("launch-existing");
        let profile = profile(&setup);
        let installed = wine_9();
        let existing = recorded();
        let passed = ["--windowed".to_owned()];
        let plan = plan_launch(
            &planner(&setup),
            &LaunchRequest {
                arguments: &passed,
                ..request(&profile, &installed, Some(&existing))
            },
        )
        .unwrap();

        assert_eq!(plan.runtime, RuntimeKind::Wine);
        assert_eq!(plan.runtime_version, "wine-9.0");
        assert_eq!(plan.prefix, setup.app.prefix);
        assert!(!plan.creates_prefix);
        assert!(!plan.migrates_prefix);
        assert_eq!(plan.dll_overrides.as_deref(), Some("winemenubuilder.exe="));

        // The full command: program, arguments, environment, working
        // directory.
        let binary = setup.machine.path("Games/setup.exe");
        assert_eq!(plan.application.program, "/usr/bin/wine");
        assert_eq!(plan.application.arguments, [binary.as_str(), "--windowed"]);
        assert_eq!(
            plan.application.environment["WINEPREFIX"],
            setup.app.prefix.to_str().unwrap()
        );
        assert_eq!(
            plan.application.working_directory,
            Some(setup.machine.path("Games"))
        );

        // The steps: nothing but the log directory, the record and the
        // launch itself. The prefix is reused, not recreated.
        assert_eq!(plan.plan.operation, Operation::Launch);
        assert_eq!(plan.plan.steps.len(), 3);
        assert_eq!(
            plan.plan.steps[0],
            Step::CreateDir {
                path: setup.app.logs.clone()
            }
        );
        let Step::WriteFile { path, contents } = &plan.plan.steps[1] else {
            panic!("{:?}", plan.plan.steps[1]);
        };
        assert_eq!(*path, setup.app.metadata);
        let record = parse_metadata(contents).unwrap();
        assert_eq!(record.last_run_at, Some(NOW));
        assert_eq!(record.launch_count, 1);
        assert_eq!(record.created_at, existing.created_at);
        assert_eq!(record.runtime_version, "wine-9.0");
        assert_eq!(record.sync, plan.sync);

        let (command, log) = launch_step(&plan);
        assert_eq!(command.program, "/usr/bin/bwrap");
        assert_eq!(*log, setup.app.logs.join("20261112T101500Z-4242.log"));
        assert_eq!(plan.log, *log);
        let split = command
            .arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap();
        assert_boundary(&setup.machine, &setup.app, &command.arguments[..=split]);

        // ntsync: no device on this host, so the launch falls back and says
        // so, in the plan and in the record.
        assert_eq!(plan.sync.selected, SyncMode::Server);
        assert_eq!(plan.sync.ntsync_device, NtsyncDevice::Absent);
        assert!(
            plan.plan
                .notes
                .iter()
                .any(|note| note.contains("/dev/ntsync"))
        );

        // A second launch counts on from the record of the first.
        let again = plan_launch(
            &planner(&setup),
            &request(&profile, &installed, Some(&record)),
        )
        .unwrap();
        let Step::WriteFile { contents, .. } = &again.plan.steps[1] else {
            panic!("{:?}", again.plan.steps[1]);
        };
        assert_eq!(parse_metadata(contents).unwrap().launch_count, 2);

        // Through an executor: the application's exit status is the result.
        let mut executor = FakeExecutor {
            statuses: vec![42],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan.plan, &mut executor);
        assert_eq!(report.exit_status, Some(42));
        assert_eq!(report.failure, None);
    }

    #[test]
    fn a_first_launch_creates_the_prefix() {
        let setup = Setup::new("launch-create");
        let profile = profile(&setup);
        let installed = wine_9();
        let plan = plan_launch(&planner(&setup), &request(&profile, &installed, None)).unwrap();
        assert!(plan.creates_prefix);
        let descriptions: Vec<String> = plan.plan.steps.iter().map(Step::describe).collect();
        assert_eq!(plan.plan.steps.len(), 10, "{descriptions:#?}");
        assert!(descriptions[3].starts_with("create the prefix: "));
        assert!(descriptions[3].ends_with("wineboot --init"));
        assert!(descriptions[9].starts_with("launch, logging to "));
        // The record written before the launch is the created prefix, run
        // once.
        let Step::WriteFile { contents, .. } = &plan.plan.steps[8] else {
            panic!("{:?}", plan.plan.steps[8]);
        };
        let record = parse_metadata(contents).unwrap();
        assert_eq!(record.prefix_id, "example-app");
        assert_eq!(record.created_at, NOW);
        assert_eq!(record.launch_count, 1);
        assert!(record.grants.is_empty());

        // If creating the prefix fails, the application is not started.
        let mut executor = FakeExecutor {
            statuses: vec![1],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan.plan, &mut executor);
        assert_eq!(report.exit_status, None);
        assert!(report.failure.is_some());
        assert!(
            !executor
                .steps
                .iter()
                .any(|step| matches!(step, Step::Launch { .. }))
        );
    }

    #[test]
    fn a_runtime_upgrade_never_changes_a_prefix_silently() {
        let setup = Setup::new("launch-upgrade");
        let profile = profile(&setup);
        let existing = recorded();

        // Wine was upgraded from 9.0 to 11.0 under the prefix.
        let newer = wine_11();
        let error = plan_launch(
            &planner(&setup),
            &request(&profile, &newer, Some(&existing)),
        )
        .unwrap_err();
        assert_eq!(
            error,
            PlanError::RuntimeMismatch {
                recorded: "wine-9.0".to_owned(),
                installed: "wine-11.0".to_owned()
            }
        );
        assert_eq!(error.code(), Code::PrefixRuntimeMismatch);
        assert!(error.to_string().contains("migration"));

        // Asked to migrate: a backup, Wine's update, the new record, then
        // the launch.
        let plan = plan_launch(
            &planner(&setup),
            &LaunchRequest {
                migrate: true,
                ..request(&profile, &newer, Some(&existing))
            },
        )
        .unwrap();
        assert!(plan.migrates_prefix);
        assert_eq!(plan.runtime_version, "wine-11.0");
        let descriptions: Vec<String> = plan.plan.steps.iter().map(Step::describe).collect();
        assert_eq!(plan.plan.steps.len(), 10, "{descriptions:#?}");
        assert!(descriptions[1].starts_with("archive the prefix: /usr/bin/tar --create"));
        assert!(descriptions[3].starts_with("update the prefix from wine-9.0 to wine-11.0: "));
        assert!(descriptions[9].starts_with("launch, "));
        let backup_at = descriptions
            .iter()
            .position(|text| text.starts_with("archive the prefix"))
            .unwrap();
        let update_at = descriptions
            .iter()
            .position(|text| text.starts_with("update the prefix"))
            .unwrap();
        assert!(backup_at < update_at);
        let Step::WriteFile { contents, .. } = &plan.plan.steps[8] else {
            panic!("{:?}", plan.plan.steps[8]);
        };
        let record = parse_metadata(contents).unwrap();
        assert_eq!(record.runtime_version, "wine-11.0");
        assert_eq!(record.migrations.len(), 1);
        assert_eq!(record.migrations[0].from_version, "wine-9.0");
        // With Wine 11 the device decides; there is none here.
        assert_eq!(plan.sync.selected, SyncMode::Server);

        // A pinned prefix is not migrated, even when asked.
        let pinned = PrefixMetadata {
            pinned: true,
            ..recorded()
        };
        let error = plan_launch(
            &planner(&setup),
            &LaunchRequest {
                migrate: true,
                ..request(&profile, &newer, Some(&pinned))
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), Code::PrefixPinned);
    }

    #[test]
    fn a_pinned_profile_needs_the_pinned_runtime() {
        let setup = Setup::new("launch-pin");
        let mut profile = profile(&setup);
        profile.runtime_version = Some("wine-8.0".to_owned());
        let installed = wine_9();
        for existing in [None, Some(&recorded())] {
            for migrate in [false, true] {
                let error = plan_launch(
                    &planner(&setup),
                    &LaunchRequest {
                        migrate,
                        ..request(&profile, &installed, existing)
                    },
                )
                .unwrap_err();
                assert_eq!(
                    error,
                    PlanError::PinMismatch {
                        pinned: "wine-8.0".to_owned(),
                        installed: "wine-9.0".to_owned()
                    }
                );
                assert_eq!(error.code(), Code::RuntimePinMismatch);
            }
        }
        // With the pinned version installed the launch goes ahead.
        profile.runtime_version = Some("wine-9.0".to_owned());
        assert!(
            plan_launch(
                &planner(&setup),
                &request(&profile, &installed, Some(&recorded()))
            )
            .is_ok()
        );
        // Without knowing the installed version nothing is launched.
        let error = plan_launch(
            &planner(&setup),
            &LaunchRequest {
                installed: None,
                ..request(&profile, &installed, Some(&recorded()))
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), Code::RuntimeVersionUnknown);
    }

    #[test]
    fn a_prefix_is_never_shared_with_another_application_or_runtime() {
        let setup = Setup::new("launch-owner");
        let profile = profile(&setup);
        let installed = wine_9();
        let cases = [
            PrefixMetadata {
                app_id: "com.example.other".to_owned(),
                ..recorded()
            },
            PrefixMetadata {
                prefix_id: "another-prefix".to_owned(),
                ..recorded()
            },
            PrefixMetadata {
                runtime: RuntimeKind::Proton,
                ..recorded()
            },
        ];
        for existing in &cases {
            let error = plan_launch(
                &planner(&setup),
                &request(&profile, &installed, Some(existing)),
            )
            .unwrap_err();
            assert_eq!(error.code(), Code::PrefixOwner, "{existing:?}");
        }
    }

    #[test]
    fn profiles_this_build_does_not_launch() {
        let setup = Setup::new("launch-unsupported");
        let installed = wine_9();
        let base = profile(&setup);

        let mut msi = base.clone();
        msi.format = ProfileFormat::Msi;
        let mut vm = base.clone();
        vm.runtime = ProfileRuntime::WindowsVm;
        vm.prefix_id = None;
        let mut native = base.clone();
        native.runtime = ProfileRuntime::Native;
        native.format = ProfileFormat::Elf;
        for profile in [msi, vm, native] {
            let error =
                plan_launch(&planner(&setup), &request(&profile, &installed, None)).unwrap_err();
            assert_eq!(error.code(), Code::LaunchUnsupported, "{profile:?}");
        }

        let mut other_app = base.clone();
        other_app.app_id = "com.example.other".to_owned();
        let error =
            plan_launch(&planner(&setup), &request(&other_app, &installed, None)).unwrap_err();
        assert_eq!(error.code(), Code::PrefixOwner);
    }

    #[test]
    fn nothing_is_launched_as_root() {
        let mut setup = Setup::new("launch-root");
        setup.host.euid = 0;
        let profile = profile(&setup);
        let installed = wine_9();
        for existing in [None, Some(&recorded())] {
            let error = plan_launch(&planner(&setup), &request(&profile, &installed, existing))
                .unwrap_err();
            assert_eq!(error, PlanError::RunningAsRoot);
            assert_eq!(error.code(), Code::RunningAsRoot);
        }
    }

    #[test]
    fn the_program_file_must_be_usable() {
        let setup = Setup::new("launch-program");
        let installed = wine_9();
        let existing = recorded();
        let mut profile = profile(&setup);

        for (binary, refusal) in [
            (
                setup.machine.path(".ssh/id_ed25519"),
                GrantRefusal::Protected {
                    what: "the SSH directory",
                },
            ),
            (
                setup.machine.path("Games/missing.exe"),
                GrantRefusal::Missing,
            ),
            (setup.machine.path("Games"), GrantRefusal::WrongKind),
        ] {
            profile.binary.clone_from(&binary);
            let error = plan_launch(
                &planner(&setup),
                &request(&profile, &installed, Some(&existing)),
            )
            .unwrap_err();
            assert_eq!(
                error,
                PlanError::Program {
                    path: binary,
                    refusal
                }
            );
            assert_eq!(error.code(), Code::BinaryRefused);
        }
    }

    #[test]
    fn grants_from_the_record_are_mounted_and_rechecked() {
        let setup = Setup::new("launch-grants");
        let profile = profile(&setup);
        let installed = wine_9();
        let granted = PrefixMetadata {
            grants: vec![
                Grant {
                    path: setup.machine.path("Saves"),
                    access: GrantAccess::ReadWrite,
                },
                Grant {
                    path: setup.machine.path("Documents"),
                    access: GrantAccess::ReadOnly,
                },
            ],
            ..recorded()
        };
        let plan = plan_launch(
            &planner(&setup),
            &request(&profile, &installed, Some(&granted)),
        )
        .unwrap();
        let (command, _) = launch_step(&plan);
        let split = command
            .arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap();
        let sandbox_arguments = &command.arguments[..=split];
        assert_boundary(&setup.machine, &setup.app, sandbox_arguments);
        let (_, entries) = parse(sandbox_arguments);
        let saves = setup.machine.path("Saves");
        let documents = setup.machine.path("Documents");
        assert!(entries.iter().any(
            |entry| entry.option == "--bind" && entry.source.as_deref() == Some(saves.as_str())
        ));
        assert!(entries.iter().any(|entry| entry.option == "--ro-bind"
            && entry.source.as_deref() == Some(documents.as_str())));

        // Metadata that names a protected directory refuses the launch.
        let hostile = PrefixMetadata {
            grants: vec![Grant {
                path: setup.machine.path(".ssh"),
                access: GrantAccess::ReadOnly,
            }],
            ..recorded()
        };
        let error = plan_launch(
            &planner(&setup),
            &request(&profile, &installed, Some(&hostile)),
        )
        .unwrap_err();
        assert_eq!(error.code(), Code::GrantChanged);
    }

    #[test]
    fn a_proton_launch_goes_through_umu_run_with_the_pinned_build() {
        let setup = Setup::new("launch-proton");
        let mut profile = profile(&setup);
        profile.runtime = ProfileRuntime::Proton;

        // Without a pin the launcher would follow the latest build.
        let error = plan_launch(&planner(&setup), &request(&profile, &wine_9(), None)).unwrap_err();
        assert_eq!(error, PlanError::ProtonUnpinned);
        assert_eq!(error.code(), Code::ProtonUnpinned);

        profile.runtime_version = Some("GE-Proton10-20".to_owned());
        profile.graphics_backend = Some(GraphicsBackend::Dxvk);
        let plan = plan_launch(
            &planner(&setup),
            &LaunchRequest {
                installed: None,
                ..request(&profile, &wine_9(), None)
            },
        )
        .unwrap();
        assert_eq!(plan.runtime, RuntimeKind::Proton);
        assert_eq!(plan.runtime_version, "GE-Proton10-20");
        assert_eq!(plan.dll_overrides, None);
        assert_eq!(plan.application.program, "/usr/bin/umu-run");
        assert_eq!(plan.application.environment["PROTONPATH"], "GE-Proton10-20");
        assert_eq!(plan.application.environment["GAMEID"], "umu-default");
        assert_eq!(plan.sync.selected, SyncMode::RuntimeDefault);
        assert!(plan.creates_prefix);
        assert!(
            plan.plan
                .notes
                .iter()
                .any(|note| note.contains("by default"))
        );

        // A prefix on another Proton build is not moved by a launch.
        let existing = PrefixMetadata {
            runtime: RuntimeKind::Proton,
            runtime_version: "GE-Proton9-27".to_owned(),
            runtime_build: "GE-Proton9-27".to_owned(),
            ..recorded()
        };
        let error = plan_launch(
            &planner(&setup),
            &request(&profile, &wine_9(), Some(&existing)),
        )
        .unwrap_err();
        assert_eq!(
            error,
            PlanError::RuntimeMismatch {
                recorded: "GE-Proton9-27".to_owned(),
                installed: "GE-Proton10-20".to_owned()
            }
        );

        let mut missing = Setup::new("launch-proton-missing");
        missing.host.umu_run = None;
        let error =
            plan_launch(&planner(&missing), &request(&profile, &wine_9(), None)).unwrap_err();
        assert_eq!(error.code(), Code::ProtonLauncherMissing);
    }

    #[test]
    fn the_plan_serialises_for_dry_runs() {
        let setup = Setup::new("launch-json");
        let profile = profile(&setup);
        let installed = wine_9();
        let existing = recorded();
        let plan = plan_launch(
            &planner(&setup),
            &request(&profile, &installed, Some(&existing)),
        )
        .unwrap();
        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(json["runtime"], "wine");
        assert_eq!(json["runtime_version"], "wine-9.0");
        assert_eq!(json["application"]["program"], "/usr/bin/wine");
        assert_eq!(json["application"]["environment"]["PATH"], "/usr/bin:/bin");
        assert_eq!(json["sync"]["selected"], "server");
        assert_eq!(json["sync"]["ntsync_device"], "absent");
        assert_eq!(json["graphics_backend"], "auto");
        assert_eq!(json["dll_overrides"], "winemenubuilder.exe=");
        assert_eq!(json["creates_prefix"], false);
        assert_eq!(json["plan"]["operation"], "launch");
        assert_eq!(json["plan"]["steps"][2]["step"], "launch");
        assert_eq!(json["sandbox"][0]["target"], "/usr");
    }
}
