// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Prefix operations, each as a plan (specification §7.2, §12.2, §15.2).
//!
//! One application, one prefix. The operations are create, verify, repair,
//! reset, back up, restore, remove, migrate and configure. Each is planned by
//! a method of [`Planner`], which reads no clock and starts no program, and
//! is carried out by [`crate::plan::run_plan`].
//!
//! A prefix is tied to the runtime version it is on:
//!
//! * Wine brings a prefix up to date by itself when it notices a newer
//!   `wine.inf`. Every prefix created here has that switched off (the file
//!   `.update-timestamp` holds `disable`, which `wineboot` honours), so the
//!   only thing that updates a prefix is the explicit migration below.
//! * [`Planner::migrate`] takes a backup, runs `wineboot --update` with the
//!   installed runtime and records the move. A pinned prefix refuses it.
//! * [`Planner::repair`] runs the same update with the runtime the prefix is
//!   already on, and refuses when another version is installed.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::casefold::{CaseFolding, chattr_command};
use crate::grant::GrantContext;
use crate::launch_host::{LaunchHost, RuntimeVersion};
use crate::limits::MAX_METADATA_BYTES;
use crate::metadata::{
    Grant, GrantAccess, MetadataError, MigrationRecord, PREFIX_SCHEMA_VERSION, PrefixArch,
    PrefixMetadata, RuntimeKind, SyncRecord, is_backup_name, parse_metadata,
};
use crate::paths::{AppPaths, plain_text};
use crate::plan::{Check, CommandSpec, Operation, Plan, Step};
use crate::profile::{GraphicsBackend, is_slug, slug};
use crate::runtime_cmd::{DEFAULT_WINE_DEBUG, Invocation, PlanError, RUNTIME_PATH, Task, assemble};
use crate::sandbox::SandboxError;
use crate::timefmt::Timestamp;

/// The file in a prefix that tells Wine whether to update the prefix on its
/// own, and the content that tells it not to.
const UPDATE_TIMESTAMP: (&str, &str) = (".update-timestamp", "disable\n");

/// Reads an application's prefix metadata.
///
/// Returns `Ok(None)` when the application has no directory at all.
///
/// # Errors
///
/// [`PlanError::Metadata`] when the directory exists and its metadata is
/// missing, unreadable or not valid; [`PlanError::PrefixOwner`] when the
/// metadata names another application.
pub fn load_metadata(app: &AppPaths) -> Result<Option<PrefixMetadata>, PlanError> {
    if fs::symlink_metadata(&app.app_dir).is_err() {
        return Ok(None);
    }
    let unreadable = |error: std::io::Error| {
        PlanError::Metadata(MetadataError::Shape(format!(
            "cannot read {}: {error}",
            app.metadata.display()
        )))
    };
    let file = fs::File::open(&app.metadata).map_err(unreadable)?;
    let limit = u64::try_from(MAX_METADATA_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut text = String::new();
    file.take(limit)
        .read_to_string(&mut text)
        .map_err(unreadable)?;
    let metadata = parse_metadata(&text)?;
    if metadata.app_id != app.app_id {
        return Err(PlanError::PrefixOwner {
            detail: format!("its metadata names application {}", metadata.app_id),
        });
    }
    Ok(Some(metadata))
}

/// The prefix name used when none is given: the application identifier with
/// its dots turned into hyphens.
pub fn default_prefix_id(app_id: &str) -> String {
    slug(app_id)
}

/// The settings a prefix is created with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixSettings {
    /// The prefix name used by the application's profile.
    pub prefix_id: String,
    /// Wine or Proton.
    pub runtime: RuntimeKind,
    /// The runtime version the prefix is created with.
    pub version: RuntimeVersion,
    /// Whether the prefix is pinned to that version.
    pub pinned: bool,
    /// The Direct3D translation setting.
    pub graphics: GraphicsBackend,
    /// Whether the application shares the host's network.
    pub network: bool,
    /// The granted host directories, already admitted.
    pub grants: Vec<Grant>,
    /// Whether drive C: is made case-insensitive where the file system can.
    pub case_folding: CaseFolding,
}

/// Plans prefix operations for one application.
pub struct Planner<'a> {
    /// The machine and session.
    pub host: &'a LaunchHost,
    /// The application's paths.
    pub app: &'a AppPaths,
    /// The grant rules for this user.
    pub context: &'a GrantContext,
    /// The time the plan is made; it names backups and stamps metadata.
    pub now: Timestamp,
}

impl Planner<'_> {
    fn refuse_root(&self) -> Result<(), PlanError> {
        if self.host.euid == 0 {
            Err(PlanError::RunningAsRoot)
        } else {
            Ok(())
        }
    }

    fn plan(&self, operation: Operation, steps: Vec<Step>) -> Plan {
        Plan {
            operation,
            app_id: self.app.app_id.clone(),
            steps,
            notes: Vec::new(),
        }
    }

    /// The invocation for prefix maintenance. No granted directory is shown
    /// to the runtime while it sets a prefix up.
    fn maintenance<'b>(&'b self, metadata: &'b PrefixMetadata) -> Invocation<'b> {
        Invocation {
            host: self.host,
            app: self.app,
            context: self.context,
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

    /// Checks a directory the user wants to grant.
    ///
    /// # Errors
    ///
    /// [`PlanError::Grant`] with the reason the directory is refused.
    pub fn admit_grant(&self, requested: &str, access: GrantAccess) -> Result<Grant, PlanError> {
        let path = self
            .context
            .admit(requested)
            .map_err(|refusal| PlanError::Grant {
                path: requested.to_owned(),
                refusal,
            })?;
        Ok(Grant { path, access })
    }

    /// The metadata of a prefix about to be created.
    fn fresh_metadata(&self, settings: &PrefixSettings) -> Result<PrefixMetadata, PlanError> {
        let metadata = PrefixMetadata {
            schema_version: PREFIX_SCHEMA_VERSION,
            app_id: self.app.app_id.clone(),
            prefix_id: settings.prefix_id.clone(),
            runtime: settings.runtime,
            runtime_version: settings.version.version.clone(),
            runtime_build: settings.version.build.clone(),
            pinned: settings.pinned,
            architecture: PrefixArch::Win64,
            created_at: self.now,
            last_run_at: None,
            launch_count: 0,
            graphics_backend: settings.graphics,
            sync: SyncRecord::not_checked(),
            network: settings.network,
            grants: settings.grants.clone(),
            migrations: Vec::new(),
        };
        metadata.validate()?;
        Ok(metadata)
    }

    /// The steps that record what a prefix is: Wine's own updating switched
    /// off, then the metadata.
    fn seal_steps(&self, metadata: &PrefixMetadata) -> Vec<Step> {
        let mut steps = Vec::new();
        if metadata.runtime == RuntimeKind::Wine {
            steps.push(Step::WriteFile {
                path: self.app.prefix.join(UPDATE_TIMESTAMP.0),
                contents: UPDATE_TIMESTAMP.1.to_owned(),
            });
        }
        steps.push(Step::WriteFile {
            path: self.app.metadata.clone(),
            contents: metadata.to_json(),
        });
        steps
    }

    fn directory_steps(&self) -> Vec<Step> {
        [&self.app.prefix, &self.app.home, &self.app.logs]
            .into_iter()
            .map(|path| Step::CreateDir { path: path.clone() })
            .collect()
    }

    /// The prefix's drive C:, which Wine creates when it is not there.
    pub fn drive_c(&self) -> std::path::PathBuf {
        self.app.prefix.join("drive_c")
    }

    /// Creates `path` empty and asks its file system to fold case in it.
    fn case_fold_step(&self, path: std::path::PathBuf) -> Step {
        let command = self
            .host
            .chattr
            .as_deref()
            .and_then(|chattr| chattr_command(chattr, &path));
        Step::CaseFold { path, command }
    }

    fn registry_requirement(&self) -> Step {
        Step::Require {
            check: Check::File {
                path: self.app.prefix.join("system.reg"),
            },
        }
    }

    /// The steps that create a prefix described by `metadata`.
    ///
    /// With [`CaseFolding::Try`] and Wine, `drive_c` is created empty and
    /// made case-insensitive before Wine is started: the attribute can only
    /// be given to an empty directory, and everything Wine then creates on
    /// drive C: inherits it. Wine leaves a `drive_c` that is already there
    /// as it is (`setup_config_dir` in `dlls/ntdll/unix/server.c`). Proton
    /// builds its prefix its own way, and its own script already sets the
    /// attribute on the `drive_c` it creates (`set_dir_casefold_bit` in
    /// Valve's `proton`), so a Proton prefix is left to it.
    pub(crate) fn create_steps(
        &self,
        metadata: &PrefixMetadata,
        case_folding: CaseFolding,
    ) -> Result<Vec<Step>, PlanError> {
        let assembled = assemble(&self.maintenance(metadata), Task::Initialise)?;
        let mut steps = self.directory_steps();
        if case_folding == CaseFolding::Try && metadata.runtime == RuntimeKind::Wine {
            steps.push(self.case_fold_step(self.drive_c()));
        }
        steps.push(Step::Run {
            label: "create the prefix".to_owned(),
            command: assembled.sandboxed,
        });
        // The runtime exiting cleanly is not proof that a prefix exists.
        steps.push(self.registry_requirement());
        steps.extend(self.seal_steps(metadata));
        Ok(steps)
    }

    /// Plans the creation of a prefix. Returns the plan and the metadata it
    /// will write.
    ///
    /// # Errors
    ///
    /// [`PlanError::PrefixExists`] when `exists` says the application already
    /// has a directory, and whatever [`assemble`] refuses.
    pub fn create(
        &self,
        settings: &PrefixSettings,
        exists: bool,
    ) -> Result<(Plan, PrefixMetadata), PlanError> {
        self.refuse_root()?;
        if exists {
            return Err(PlanError::PrefixExists);
        }
        let metadata = self.fresh_metadata(settings)?;
        let mut plan = self.plan(
            Operation::Create,
            self.create_steps(&metadata, settings.case_folding)?,
        );
        plan.notes.push(format!(
            "The prefix is tied to {}; a later runtime upgrade does not change it.",
            metadata.runtime_version
        ));
        plan.notes
            .push(case_folding_note(settings.case_folding, metadata.runtime));
        Ok((plan, metadata))
    }

    /// Plans a read-only check of a prefix.
    ///
    /// # Errors
    ///
    /// [`PlanError::RunningAsRoot`].
    pub fn verify(&self, metadata: &PrefixMetadata) -> Result<Plan, PlanError> {
        self.refuse_root()?;
        let prefix = &self.app.prefix;
        let mut checks = vec![
            Check::Directory {
                path: prefix.clone(),
            },
            Check::Directory {
                path: self.app.home.clone(),
            },
            Check::OwnedByUser {
                path: prefix.clone(),
            },
            Check::File {
                path: prefix.join("system.reg"),
            },
            Check::File {
                path: prefix.join("user.reg"),
            },
            Check::Directory {
                path: prefix.join("drive_c").join("windows"),
            },
        ];
        if metadata.runtime == RuntimeKind::Wine {
            checks.push(Check::FileContains {
                path: prefix.join("system.reg"),
                text: format!("#arch={}", metadata.architecture.winearch()),
            });
            checks.push(Check::FileContains {
                path: prefix.join(UPDATE_TIMESTAMP.0),
                text: UPDATE_TIMESTAMP.1.trim_end().to_owned(),
            });
        }
        let steps = checks
            .into_iter()
            .map(|check| Step::Check { check })
            .collect();
        Ok(self.plan(Operation::Verify, steps))
    }

    fn require_same_version(
        metadata: &PrefixMetadata,
        installed: &RuntimeVersion,
    ) -> Result<(), PlanError> {
        if metadata.runtime_version == installed.version {
            Ok(())
        } else {
            Err(PlanError::RuntimeMismatch {
                recorded: metadata.runtime_version.clone(),
                installed: installed.version.clone(),
            })
        }
    }

    /// Plans a repair: the runtime the prefix is on restores the prefix's
    /// system files and registry defaults.
    ///
    /// # Errors
    ///
    /// [`PlanError::RuntimeMismatch`] when another runtime version is
    /// installed: that would be a migration, not a repair.
    pub fn repair(
        &self,
        metadata: &PrefixMetadata,
        installed: &RuntimeVersion,
    ) -> Result<Plan, PlanError> {
        self.refuse_root()?;
        Self::require_same_version(metadata, installed)?;
        let assembled = assemble(&self.maintenance(metadata), Task::Update)?;
        let mut steps = self.directory_steps();
        steps.push(Step::Run {
            label: "restore the prefix's system files".to_owned(),
            command: assembled.sandboxed,
        });
        steps.push(self.registry_requirement());
        steps.extend(self.seal_steps(metadata));
        Ok(self.plan(Operation::Repair, steps))
    }

    /// The name a backup taken now would have.
    pub fn backup_name(&self, reason: &str) -> String {
        format!("{}-{reason}", self.now.compact())
    }

    /// The steps that archive the prefix, its private home and its metadata.
    pub(crate) fn backup_steps(&self, name: &str) -> Result<Vec<Step>, PlanError> {
        if !is_backup_name(name) {
            return Err(PlanError::BackupMissing {
                name: name.to_owned(),
            });
        }
        let archive = self.app.backup_archive(name);
        let archive_text = text(&archive)?;
        let directory = text(&self.app.app_dir)?;
        let mut arguments = vec![
            "--create",
            "--gzip",
            "--file",
            archive_text.as_str(),
            "--directory",
            directory.as_str(),
            "--",
            "metadata.json",
            "prefix",
            "home",
        ];
        // An installed application also has a record and a profile. They
        // go into the archive when they are there, so that a restore brings
        // back the application and not only its prefix.
        for (file, member) in [
            (&self.app.record, "application.json"),
            (&self.app.profile, "profile.json"),
        ] {
            if fs::symlink_metadata(file).is_ok_and(|metadata| metadata.is_file()) {
                arguments.push(member);
            }
        }
        let command = self.tar(&arguments)?;
        Ok(vec![
            Step::CreateDir {
                path: self.app.backups.clone(),
            },
            Step::Run {
                label: "archive the prefix".to_owned(),
                command,
            },
            Step::Require {
                check: Check::File { path: archive },
            },
        ])
    }

    fn tar(&self, arguments: &[&str]) -> Result<CommandSpec, PlanError> {
        let program = self
            .host
            .tar
            .as_deref()
            .ok_or(PlanError::ToolMissing { program: "tar" })?;
        let mut environment = BTreeMap::new();
        environment.insert("PATH".to_owned(), RUNTIME_PATH.to_owned());
        environment.insert("LC_ALL".to_owned(), "C".to_owned());
        Ok(CommandSpec {
            program: text(program)?,
            arguments: arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
            environment,
            working_directory: None,
        })
    }

    /// Plans a backup. Returns the plan and the backup's name.
    ///
    /// # Errors
    ///
    /// [`PlanError::ToolMissing`] when `tar` is not installed, and
    /// [`PlanError::BackupMissing`] when `reason` cannot be part of a name.
    pub fn backup(&self, reason: &str) -> Result<(Plan, String), PlanError> {
        self.refuse_root()?;
        let name = self.backup_name(reason);
        let plan = self.plan(Operation::Backup, self.backup_steps(&name)?);
        Ok((plan, name))
    }

    /// Plans the replacement of the prefix by a backup.
    ///
    /// The archive is unpacked beside the prefix and checked to belong to
    /// this application before anything is replaced. `prefix_exists` says
    /// whether there is a prefix to move out of the way.
    ///
    /// An archive does not keep the case-folding attribute. With
    /// [`CaseFolding::Try`], given when the prefix being replaced has a
    /// case-insensitive drive C:, the unpacked `drive_c` is created and made
    /// case-insensitive before the archive is unpacked into it, so that the
    /// restored prefix is as the replaced one was.
    ///
    /// # Errors
    ///
    /// [`PlanError::BackupMissing`] when `name` is not a backup name.
    pub fn restore(
        &self,
        name: &str,
        prefix_exists: bool,
        case_folding: CaseFolding,
    ) -> Result<Plan, PlanError> {
        self.refuse_root()?;
        if !is_backup_name(name) {
            return Err(PlanError::BackupMissing {
                name: name.to_owned(),
            });
        }
        let archive = self.app.backup_archive(name);
        let incoming = self.app.staging.join("incoming");
        let replaced = self.app.staging.join("replaced");
        let archive_text = text(&archive)?;
        let directory = text(&incoming)?;
        let unpack = self.tar(&[
            "--extract",
            "--gzip",
            "--file",
            archive_text.as_str(),
            "--directory",
            directory.as_str(),
        ])?;
        let mut steps = vec![
            Step::Require {
                check: Check::File { path: archive },
            },
            Step::RemoveTree {
                path: self.app.staging.clone(),
            },
            Step::CreateDir {
                path: incoming.clone(),
            },
        ];
        if case_folding == CaseFolding::Try {
            steps.push(self.case_fold_step(incoming.join("prefix").join("drive_c")));
        }
        steps.extend([
            Step::Run {
                label: "unpack the backup".to_owned(),
                command: unpack,
            },
            Step::Require {
                check: Check::FileContains {
                    path: incoming.join("metadata.json"),
                    text: format!("\"app_id\": \"{}\"", self.app.app_id),
                },
            },
            Step::Require {
                check: Check::Directory {
                    path: incoming.join("prefix"),
                },
            },
        ]);
        if prefix_exists {
            steps.push(Step::Rename {
                from: self.app.app_dir.clone(),
                to: replaced,
            });
        } else if let Some(parent) = self.app.app_dir.parent() {
            steps.push(Step::CreateDir {
                path: parent.to_path_buf(),
            });
        }
        steps.push(Step::Rename {
            from: incoming,
            to: self.app.app_dir.clone(),
        });
        steps.push(Step::RemoveTree {
            path: self.app.staging.clone(),
        });
        let mut plan = self.plan(Operation::Restore, steps);
        plan.notes.push(
            "The restored prefix keeps the runtime version it was backed up with.".to_owned(),
        );
        Ok(plan)
    }

    /// Plans the removal of the prefix, its private home, its metadata and
    /// its logs. Backups are kept.
    ///
    /// # Errors
    ///
    /// [`PlanError::PrefixMissing`] when `exists` says there is nothing to
    /// remove.
    pub fn remove(&self, exists: bool) -> Result<Plan, PlanError> {
        self.refuse_root()?;
        if !exists {
            return Err(PlanError::PrefixMissing);
        }
        let steps = [
            &self.app.app_dir,
            &self.app.staging,
            &self.app.logs,
            &self.app.launchers,
        ]
        .into_iter()
        .map(|path| Step::RemoveTree { path: path.clone() })
        .collect();
        let mut plan = self.plan(Operation::Remove, steps);
        plan.notes.push(format!(
            "Backups in {} are kept.",
            self.app.backups.display()
        ));
        Ok(plan)
    }

    /// Plans a reset: a fresh prefix with the same settings and grants,
    /// created with the installed runtime. Unless `backup` is false, the old
    /// prefix is archived first.
    ///
    /// # Errors
    ///
    /// [`PlanError::Pinned`] when the prefix is pinned to another version
    /// than the one installed.
    ///
    /// `case_folding` is what the old prefix had: a reset keeps drive C:
    /// case-insensitive, or not, as it was.
    pub fn reset(
        &self,
        metadata: &PrefixMetadata,
        installed: &RuntimeVersion,
        backup: bool,
        case_folding: CaseFolding,
    ) -> Result<Plan, PlanError> {
        self.refuse_root()?;
        if metadata.pinned && metadata.runtime_version != installed.version {
            return Err(PlanError::Pinned {
                version: metadata.runtime_version.clone(),
            });
        }
        let fresh = PrefixMetadata {
            runtime_version: installed.version.clone(),
            runtime_build: installed.build.clone(),
            created_at: self.now,
            last_run_at: None,
            launch_count: 0,
            sync: SyncRecord::not_checked(),
            migrations: Vec::new(),
            ..metadata.clone()
        };
        fresh.validate()?;
        let mut steps = Vec::new();
        let mut notes = Vec::new();
        if backup {
            let name = self.backup_name("before-reset");
            steps.extend(self.backup_steps(&name)?);
            notes.push(format!("The old prefix is kept as backup {name}."));
        } else {
            notes.push("The old prefix is deleted without a backup.".to_owned());
        }
        for path in [&self.app.prefix, &self.app.home] {
            steps.push(Step::RemoveTree { path: path.clone() });
        }
        steps.extend(self.create_steps(&fresh, case_folding)?);
        notes.push(case_folding_note(case_folding, fresh.runtime));
        let mut plan = self.plan(Operation::Reset, steps);
        plan.notes = notes;
        Ok(plan)
    }

    /// The steps that move a prefix to the installed runtime version, and
    /// the metadata afterwards. No steps when it is already on that version.
    pub(crate) fn migrate_steps(
        &self,
        metadata: &PrefixMetadata,
        installed: &RuntimeVersion,
    ) -> Result<(Vec<Step>, PrefixMetadata), PlanError> {
        if metadata.runtime_version == installed.version {
            return Ok((Vec::new(), metadata.clone()));
        }
        if metadata.pinned {
            return Err(PlanError::Pinned {
                version: metadata.runtime_version.clone(),
            });
        }
        let backup = self.backup_name("before-migration");
        let mut moved = metadata.clone();
        moved.runtime_version.clone_from(&installed.version);
        moved.runtime_build.clone_from(&installed.build);
        moved.remember_migration(MigrationRecord {
            at: self.now,
            from_version: metadata.runtime_version.clone(),
            to_version: installed.version.clone(),
            backup: backup.clone(),
        });
        moved.validate()?;

        let mut steps = self.backup_steps(&backup)?;
        let assembled = assemble(&self.maintenance(&moved), Task::Update)?;
        steps.push(Step::Run {
            label: format!(
                "update the prefix from {} to {}",
                metadata.runtime_version, installed.version
            ),
            command: assembled.sandboxed,
        });
        steps.push(self.registry_requirement());
        steps.extend(self.seal_steps(&moved));
        Ok((steps, moved))
    }

    /// Plans an explicit migration to the installed runtime version: a
    /// backup, then the runtime's own prefix update, then the new record.
    ///
    /// # Errors
    ///
    /// [`PlanError::Pinned`] when the prefix is pinned.
    pub fn migrate(
        &self,
        metadata: &PrefixMetadata,
        installed: &RuntimeVersion,
    ) -> Result<Plan, PlanError> {
        self.refuse_root()?;
        let (steps, moved) = self.migrate_steps(metadata, installed)?;
        let mut plan = self.plan(Operation::Migrate, steps);
        plan.notes.push(match moved.migrations.last() {
            Some(record) if !plan.steps.is_empty() => format!(
                "The prefix as it is now is kept as backup {}.",
                record.backup
            ),
            _ => format!(
                "The prefix is already on {}; there is nothing to do.",
                metadata.runtime_version
            ),
        });
        Ok(plan)
    }

    /// Plans a change to the recorded settings: grants, pinning, network.
    ///
    /// # Errors
    ///
    /// [`PlanError::Metadata`] when the new record is not valid.
    pub fn configure(&self, metadata: &PrefixMetadata) -> Result<Plan, PlanError> {
        self.refuse_root()?;
        metadata.validate()?;
        let steps = vec![Step::WriteFile {
            path: self.app.metadata.clone(),
            contents: metadata.to_json(),
        }];
        Ok(self.plan(Operation::Configure, steps))
    }
}

/// What a plan says about the case folding of drive C:.
fn case_folding_note(case_folding: CaseFolding, runtime: RuntimeKind) -> String {
    match (case_folding, runtime) {
        (CaseFolding::Try, RuntimeKind::Wine) => "Drive C: is made case-insensitive where the \
             file system can (ext4 with the casefold feature); where it cannot, it is an \
             ordinary directory, Wine emulates case-insensitivity, and the report says why."
            .to_owned(),
        (CaseFolding::Try, RuntimeKind::Proton) => {
            "Drive C: is left to Proton, whose own script asks for case folding on the \
             drive_c it creates; that was not run here."
                .to_owned()
        }
        (CaseFolding::Off, _) => {
            "Drive C: is not made case-insensitive; Wine emulates case-insensitivity.".to_owned()
        }
    }
}

fn text(path: &Path) -> Result<String, PlanError> {
    plain_text(path)
        .map(str::to_owned)
        .ok_or(PlanError::Sandbox(SandboxError::Path {
            role: "the runtime manager's data",
        }))
}

/// Adds a grant to a metadata record, replacing an existing grant of the
/// same directory.
pub fn with_grant(metadata: &PrefixMetadata, grant: Grant) -> PrefixMetadata {
    let mut changed = metadata.clone();
    changed
        .grants
        .retain(|existing| existing.path != grant.path);
    changed.grants.push(grant);
    changed
}

/// Removes the grant of `path` from a metadata record. Returns `None` when
/// there is no such grant.
pub fn without_grant(metadata: &PrefixMetadata, path: &str) -> Option<PrefixMetadata> {
    // The user may name the directory through a link; the record holds it
    // resolved.
    let resolved = fs::canonicalize(path).ok();
    let matches = |grant: &Grant| {
        let recorded = Path::new(&grant.path);
        recorded == Path::new(path) || resolved.as_deref() == Some(recorded)
    };
    if !metadata.grants.iter().any(matches) {
        return None;
    }
    let mut changed = metadata.clone();
    changed.grants.retain(|grant| !matches(grant));
    Some(changed)
}

/// Whether `text` can be the reason part of a backup name.
pub fn is_backup_reason(text: &str) -> bool {
    is_slug(text)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::codes::Code;
    use crate::metadata::tests::sample;
    use crate::plan::fake::FakeExecutor;
    use crate::plan::run_plan;
    use crate::runtime_cmd::tests::{NOW, Setup};
    use crate::sandbox::SUPERVISOR_PATH;

    pub(crate) fn wine_9() -> RuntimeVersion {
        RuntimeVersion {
            version: "wine-9.0".to_owned(),
            build: "wine-9.0 (Ubuntu 9.0~repack-4build3)".to_owned(),
        }
    }

    pub(crate) fn wine_11() -> RuntimeVersion {
        RuntimeVersion {
            version: "wine-11.0".to_owned(),
            build: "wine-11.0".to_owned(),
        }
    }

    pub(crate) fn planner(setup: &Setup) -> Planner<'_> {
        Planner {
            host: &setup.host,
            app: &setup.app,
            context: &setup.context,
            now: NOW,
        }
    }

    fn settings() -> PrefixSettings {
        PrefixSettings {
            prefix_id: "example-app".to_owned(),
            runtime: RuntimeKind::Wine,
            version: wine_9(),
            pinned: false,
            graphics: GraphicsBackend::Auto,
            network: true,
            grants: Vec::new(),
            case_folding: CaseFolding::Off,
        }
    }

    /// Metadata for the application of [`Setup`].
    pub(crate) fn recorded() -> PrefixMetadata {
        PrefixMetadata {
            app_id: "com.example.app".to_owned(),
            prefix_id: "example-app".to_owned(),
            ..sample()
        }
    }

    fn kinds(plan: &Plan) -> Vec<&'static str> {
        plan.steps
            .iter()
            .map(|step| match step {
                Step::CreateDir { .. } => "create-dir",
                Step::CaseFold { .. } => "case-fold",
                Step::WriteFile { .. } => "write-file",
                Step::WriteBytes { .. } => "write-bytes",
                Step::RemoveTree { .. } => "remove-tree",
                Step::Rename { .. } => "rename",
                Step::Run { .. } => "run",
                Step::RunBounded { .. } => "run-bounded",
                Step::Launch { .. } => "launch",
                Step::Check { .. } => "check",
                Step::Require { .. } => "require",
            })
            .collect()
    }

    fn run_command(plan: &Plan, index: usize) -> &CommandSpec {
        match &plan.steps[index] {
            Step::Run { command, .. } => command,
            other => panic!("step {index} is {other:?}"),
        }
    }

    fn written(plan: &Plan, path: &Path) -> String {
        plan.steps
            .iter()
            .rev()
            .find_map(|step| match step {
                Step::WriteFile {
                    path: target,
                    contents,
                } if target == path => Some(contents.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("nothing is written to {}", path.display()))
    }

    #[test]
    fn creating_a_prefix() {
        let setup = Setup::new("prefix-create");
        let (plan, metadata) = planner(&setup).create(&settings(), false).unwrap();
        assert_eq!(plan.operation, Operation::Create);
        assert_eq!(plan.app_id, "com.example.app");
        assert_eq!(
            kinds(&plan),
            [
                "create-dir",
                "create-dir",
                "create-dir",
                "run",
                "require",
                "write-file",
                "write-file"
            ]
        );
        assert_eq!(
            plan.steps[0],
            Step::CreateDir {
                path: setup.app.prefix.clone()
            }
        );
        assert_eq!(
            plan.steps[1],
            Step::CreateDir {
                path: setup.app.home.clone()
            }
        );

        // The prefix is created by Wine itself, inside the sandbox, under
        // the supervisor that waits for the Wine server.
        let command = run_command(&plan, 3);
        assert_eq!(command.program, "/usr/bin/bwrap");
        let tail: Vec<&str> = command
            .arguments
            .iter()
            .rev()
            .take(8)
            .rev()
            .map(String::as_str)
            .collect();
        assert_eq!(
            tail,
            [
                SUPERVISOR_PATH,
                "supervise",
                "--settle",
                "/usr/bin/wineserver",
                "--",
                "/usr/bin/wine",
                "wineboot",
                "--init"
            ]
        );
        assert_eq!(command.environment["WINEARCH"], "win64");
        assert_eq!(
            command.environment["WINEPREFIX"],
            setup.app.prefix.to_str().unwrap()
        );

        // Wine's own prefix updating is switched off, then the metadata is
        // written last, outside the prefix.
        assert_eq!(
            written(&plan, &setup.app.prefix.join(".update-timestamp")),
            "disable\n"
        );
        assert_eq!(written(&plan, &setup.app.metadata), metadata.to_json());
        assert!(
            matches!(plan.steps.last(), Some(Step::WriteFile { path, .. }) if *path == setup.app.metadata)
        );

        assert_eq!(metadata.app_id, "com.example.app");
        assert_eq!(metadata.prefix_id, "example-app");
        assert_eq!(metadata.runtime, RuntimeKind::Wine);
        assert_eq!(metadata.runtime_version, "wine-9.0");
        assert_eq!(
            metadata.runtime_build,
            "wine-9.0 (Ubuntu 9.0~repack-4build3)"
        );
        assert_eq!(metadata.architecture, PrefixArch::Win64);
        assert_eq!(metadata.created_at, NOW);
        assert_eq!(metadata.last_run_at, None);
        assert_eq!(metadata.launch_count, 0);
        assert!(metadata.grants.is_empty());

        // The plan runs to the end through an executor.
        let mut executor = FakeExecutor::default();
        let report = run_plan(&plan, &mut executor);
        assert!(report.succeeded());
        assert_eq!(executor.steps, plan.steps);

        // If Wine fails, no metadata is written.
        let mut executor = FakeExecutor {
            statuses: vec![1],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.failure.unwrap().step, 3);
        assert!(
            !executor
                .steps
                .iter()
                .any(|step| matches!(step, Step::WriteFile { .. }))
        );

        // If Wine exits cleanly and there is no registry, none is written
        // either.
        let mut executor = FakeExecutor {
            failing_checks: vec!["system.reg".to_owned()],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.failure.unwrap().step, 4);
        assert!(
            !executor
                .steps
                .iter()
                .any(|step| matches!(step, Step::WriteFile { .. }))
        );
    }

    #[test]
    fn a_case_insensitive_drive_c_is_made_before_wine_writes_into_it() {
        let setup = Setup::new("prefix-casefold");
        let folded = PrefixSettings {
            case_folding: CaseFolding::Try,
            ..settings()
        };
        let (plan, _) = planner(&setup).create(&folded, false).unwrap();
        assert_eq!(
            kinds(&plan),
            [
                "create-dir",
                "create-dir",
                "create-dir",
                "case-fold",
                "run",
                "require",
                "write-file",
                "write-file"
            ]
        );
        let drive_c = setup.app.prefix.join("drive_c");
        match &plan.steps[3] {
            Step::CaseFold {
                path,
                command: Some(command),
            } => {
                assert_eq!(path, &drive_c);
                assert_eq!(command.program, "/usr/bin/chattr");
                assert_eq!(command.arguments, ["+F", "--", drive_c.to_str().unwrap()]);
                assert_eq!(command.environment["LC_ALL"], "C");
            }
            other => panic!("{other:?}"),
        }
        // Wine is started after drive C: exists, never before.
        assert_eq!(run_command(&plan, 4).environment["WINEARCH"], "win64");
        assert!(
            plan.notes
                .iter()
                .any(|note| note.contains("where the file system can"))
        );

        // A file system that cannot fold case: the prefix is created all the
        // same, and the report says why it is case-sensitive.
        let mut executor = FakeExecutor {
            case_fold: Some((
                1,
                "chattr: Operation not supported while setting flags on drive_c",
            )),
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert!(report.succeeded());
        let record = report.case_folding.unwrap();
        assert!(record.requested && !record.casefolded);
        assert_eq!(
            record.reason,
            crate::casefold::CaseFoldReason::NoCasefoldFeature
        );
        assert_eq!(record.reason.code(), Some(Code::CaseFoldUnsupported));
        // And one that can.
        let report = run_plan(&plan, &mut FakeExecutor::default());
        assert!(report.case_folding.unwrap().casefolded);

        // Without chattr the step stays, with nothing to run.
        let mut without_chattr = Setup::new("prefix-casefold-no-chattr");
        without_chattr.host.chattr = None;
        let (plan, _) = planner(&without_chattr).create(&folded, false).unwrap();
        assert!(matches!(
            &plan.steps[3],
            Step::CaseFold { command: None, .. }
        ));
        let report = run_plan(&plan, &mut FakeExecutor::default());
        assert!(report.succeeded());
        assert_eq!(
            report.case_folding.unwrap().reason.code(),
            Some(Code::CaseFoldFailed)
        );

        // Not asked for: no step, and nothing in the report.
        let (plan, _) = planner(&setup).create(&settings(), false).unwrap();
        assert!(!kinds(&plan).contains(&"case-fold"));
        assert_eq!(
            run_plan(&plan, &mut FakeExecutor::default()).case_folding,
            None
        );
        assert!(
            plan.notes
                .iter()
                .any(|note| note.contains("not made case-insensitive"))
        );

        // Proton builds its own prefix; it is left alone.
        let proton = PrefixSettings {
            runtime: RuntimeKind::Proton,
            version: RuntimeVersion {
                version: "GE-Proton10-20".to_owned(),
                build: "GE-Proton10-20".to_owned(),
            },
            ..folded.clone()
        };
        let (plan, _) = planner(&setup).create(&proton, false).unwrap();
        assert!(!kinds(&plan).contains(&"case-fold"));

        // A reset keeps what the old prefix had.
        let metadata = recorded();
        let plan = planner(&setup)
            .reset(&metadata, &wine_9(), false, CaseFolding::Try)
            .unwrap();
        let fold = kinds(&plan).iter().position(|kind| *kind == "case-fold");
        let wine = kinds(&plan).iter().position(|kind| *kind == "run");
        assert!(fold.is_some() && fold < wine, "{:?}", kinds(&plan));

        // A restore makes the unpacked drive C: case-insensitive before the
        // archive is unpacked into it.
        let plan = planner(&setup)
            .restore("20261112T101500Z-manual", true, CaseFolding::Try)
            .unwrap();
        let fold = kinds(&plan).iter().position(|kind| *kind == "case-fold");
        let unpack = kinds(&plan).iter().position(|kind| *kind == "run");
        assert!(fold.is_some() && fold < unpack, "{:?}", kinds(&plan));
        assert!(matches!(
            &plan.steps[fold.unwrap()],
            Step::CaseFold { path, .. } if *path == setup.app.staging.join("incoming/prefix/drive_c")
        ));
    }

    #[test]
    fn creation_is_refused_when_it_should_be() {
        let setup = Setup::new("prefix-create-refused");
        assert_eq!(
            planner(&setup).create(&settings(), true).unwrap_err(),
            PlanError::PrefixExists
        );
        let bad = PrefixSettings {
            prefix_id: "Not A Slug".to_owned(),
            ..settings()
        };
        assert!(matches!(
            planner(&setup).create(&bad, false),
            Err(PlanError::Metadata(_))
        ));

        let mut root = Setup::new("prefix-create-root");
        root.host.euid = 0;
        let as_root = planner(&root);
        assert_eq!(
            as_root.create(&settings(), false).unwrap_err(),
            PlanError::RunningAsRoot
        );
        // Root is refused for every operation, not only the ones that start
        // Wine.
        let metadata = recorded();
        assert_eq!(as_root.verify(&metadata), Err(PlanError::RunningAsRoot));
        assert_eq!(
            as_root.repair(&metadata, &wine_9()),
            Err(PlanError::RunningAsRoot)
        );
        assert_eq!(
            as_root.reset(&metadata, &wine_9(), true, CaseFolding::Off),
            Err(PlanError::RunningAsRoot)
        );
        assert_eq!(
            as_root.backup("manual").unwrap_err(),
            PlanError::RunningAsRoot
        );
        assert_eq!(
            as_root.restore("20261112T101500Z-manual", true, CaseFolding::Off),
            Err(PlanError::RunningAsRoot)
        );
        assert_eq!(as_root.remove(true), Err(PlanError::RunningAsRoot));
        assert_eq!(
            as_root.migrate(&metadata, &wine_11()),
            Err(PlanError::RunningAsRoot)
        );
        assert_eq!(as_root.configure(&metadata), Err(PlanError::RunningAsRoot));
    }

    #[test]
    fn a_proton_prefix_is_created_through_umu_run() {
        let setup = Setup::new("prefix-create-proton");
        let proton = PrefixSettings {
            runtime: RuntimeKind::Proton,
            version: RuntimeVersion {
                version: "GE-Proton10-20".to_owned(),
                build: "GE-Proton10-20".to_owned(),
            },
            ..settings()
        };
        let (plan, _) = planner(&setup).create(&proton, false).unwrap();
        let command = run_command(&plan, 3);
        let tail: Vec<&str> = command
            .arguments
            .iter()
            .rev()
            .take(3)
            .rev()
            .map(String::as_str)
            .collect();
        assert_eq!(tail, ["--", "/usr/bin/umu-run", ""]);
        assert_eq!(command.environment["PROTONPATH"], "GE-Proton10-20");
        // Proton keeps its own record of the prefix version; Wine's switch
        // is not written.
        assert_eq!(
            kinds(&plan),
            [
                "create-dir",
                "create-dir",
                "create-dir",
                "run",
                "require",
                "write-file"
            ]
        );
    }

    #[test]
    fn verification_only_checks() {
        let setup = Setup::new("prefix-verify");
        let plan = planner(&setup).verify(&recorded()).unwrap();
        assert_eq!(plan.operation, Operation::Verify);
        assert_eq!(plan.steps.len(), 8);
        assert!(kinds(&plan).iter().all(|kind| *kind == "check"));
        let text = plan.render_text();
        assert!(text.contains("system.reg is a file"));
        assert!(text.contains("system.reg contains \"#arch=win64\""));
        assert!(text.contains(".update-timestamp contains \"disable\""));
        assert!(text.contains("belongs to the current user"));

        // Findings are reported; nothing is changed and the plan finishes.
        let mut executor = FakeExecutor {
            failing_checks: vec!["user.reg".to_owned(), "update-timestamp".to_owned()],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.completed, 8);
        assert_eq!(report.findings.len(), 2);
        assert!(!report.succeeded());
    }

    #[test]
    fn repair_uses_the_runtime_the_prefix_is_on() {
        let setup = Setup::new("prefix-repair");
        let plan = planner(&setup).repair(&recorded(), &wine_9()).unwrap();
        assert_eq!(
            kinds(&plan),
            [
                "create-dir",
                "create-dir",
                "create-dir",
                "run",
                "require",
                "write-file",
                "write-file"
            ]
        );
        let command = run_command(&plan, 3);
        assert_eq!(
            command.arguments.last().map(String::as_str),
            Some("--update")
        );
        assert!(!command.environment.contains_key("WINEARCH"));
        // The record is unchanged.
        assert_eq!(written(&plan, &setup.app.metadata), recorded().to_json());

        // With another Wine installed, a repair would change the prefix.
        let error = planner(&setup).repair(&recorded(), &wine_11()).unwrap_err();
        assert_eq!(
            error,
            PlanError::RuntimeMismatch {
                recorded: "wine-9.0".to_owned(),
                installed: "wine-11.0".to_owned()
            }
        );
        assert_eq!(error.code(), Code::PrefixRuntimeMismatch);
    }

    #[test]
    fn backups_are_named_by_time_and_reason() {
        let setup = Setup::new("prefix-backup");
        let (plan, name) = planner(&setup).backup("manual").unwrap();
        assert_eq!(name, "20261112T101500Z-manual");
        assert_eq!(kinds(&plan), ["create-dir", "run", "require"]);
        let command = run_command(&plan, 1);
        assert_eq!(command.program, "/usr/bin/tar");
        let archive = setup.app.backups.join("20261112T101500Z-manual.tar.gz");
        assert_eq!(
            command.arguments,
            [
                "--create",
                "--gzip",
                "--file",
                archive.to_str().unwrap(),
                "--directory",
                setup.app.app_dir.to_str().unwrap(),
                "--",
                "metadata.json",
                "prefix",
                "home"
            ]
        );
        // tar gets a minimal environment of its own.
        assert_eq!(
            command.environment.keys().collect::<Vec<_>>(),
            ["LC_ALL", "PATH"]
        );
        // The archive is outside everything the application can see.
        assert!(!archive.starts_with(&setup.app.prefix));
        assert!(!archive.starts_with(&setup.app.home));

        assert!(matches!(
            planner(&setup).backup("Not A Reason"),
            Err(PlanError::BackupMissing { .. })
        ));
        let mut without_tar = Setup::new("prefix-backup-no-tar");
        without_tar.host.tar = None;
        let error = planner(&without_tar).backup("manual").unwrap_err();
        assert_eq!(error, PlanError::ToolMissing { program: "tar" });
        assert_eq!(error.code(), Code::ToolMissing);
    }

    #[test]
    fn restore_checks_the_archive_before_replacing_anything() {
        let setup = Setup::new("prefix-restore");
        let plan = planner(&setup)
            .restore("20261112T101500Z-manual", true, CaseFolding::Off)
            .unwrap();
        assert_eq!(
            kinds(&plan),
            [
                "require",
                "remove-tree",
                "create-dir",
                "run",
                "require",
                "require",
                "rename",
                "rename",
                "remove-tree"
            ]
        );
        let incoming = setup.app.staging.join("incoming");
        assert_eq!(
            plan.steps[4],
            Step::Require {
                check: Check::FileContains {
                    path: incoming.join("metadata.json"),
                    text: "\"app_id\": \"com.example.app\"".to_owned(),
                }
            }
        );
        assert_eq!(
            plan.steps[6],
            Step::Rename {
                from: setup.app.app_dir.clone(),
                to: setup.app.staging.join("replaced"),
            }
        );
        assert_eq!(
            plan.steps[7],
            Step::Rename {
                from: incoming,
                to: setup.app.app_dir.clone(),
            }
        );
        // The check matches what metadata really looks like on disk.
        assert!(
            recorded()
                .to_json()
                .contains("\"app_id\": \"com.example.app\"")
        );

        // An archive of another application stops the plan before the
        // existing prefix is touched.
        let mut executor = FakeExecutor {
            failing_checks: vec!["app_id".to_owned()],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.failure.unwrap().step, 4);
        assert!(
            !executor
                .steps
                .iter()
                .any(|step| matches!(step, Step::Rename { .. }))
        );

        // With no prefix in place there is nothing to move away.
        let plan = planner(&setup)
            .restore("20261112T101500Z-manual", false, CaseFolding::Off)
            .unwrap();
        assert_eq!(
            kinds(&plan),
            [
                "require",
                "remove-tree",
                "create-dir",
                "run",
                "require",
                "require",
                "create-dir",
                "rename",
                "remove-tree"
            ]
        );

        for name in [
            "",
            "latest",
            "../../etc/passwd",
            "20261112T101500Z-manual.tar.gz",
        ] {
            let error = planner(&setup)
                .restore(name, true, CaseFolding::Off)
                .unwrap_err();
            assert_eq!(error.code(), Code::BackupMissing, "{name:?}");
        }
    }

    #[test]
    fn removal_deletes_the_application_and_keeps_backups() {
        let setup = Setup::new("prefix-remove");
        let plan = planner(&setup).remove(true).unwrap();
        assert_eq!(
            plan.steps,
            [
                Step::RemoveTree {
                    path: setup.app.app_dir.clone()
                },
                Step::RemoveTree {
                    path: setup.app.staging.clone()
                },
                Step::RemoveTree {
                    path: setup.app.logs.clone()
                },
                // The launcher entry of an installed application goes too.
                Step::RemoveTree {
                    path: setup.app.launchers.clone()
                },
            ]
        );
        assert!(plan.notes[0].contains("are kept"));
        // Nothing outside this application's own directories is deleted.
        let other = setup.machine.dirs.app("com.example.other").unwrap();
        for step in &plan.steps {
            let Step::RemoveTree { path } = step else {
                panic!("{step:?}");
            };
            assert!(!other.app_dir.starts_with(path));
            assert!(!setup.app.backups.starts_with(path));
            assert!(
                path.starts_with(setup.machine.dirs.data_root())
                    || path.starts_with(setup.machine.dirs.state_root())
            );
        }
        assert_eq!(
            planner(&setup).remove(false).unwrap_err(),
            PlanError::PrefixMissing
        );
    }

    #[test]
    fn reset_backs_up_then_recreates_with_the_same_settings() {
        let setup = Setup::new("prefix-reset");
        let before = PrefixMetadata {
            launch_count: 7,
            last_run_at: Some(Timestamp::from_unix(1_794_000_000)),
            created_at: Timestamp::from_unix(1_793_000_000),
            grants: vec![Grant {
                path: setup.machine.path("Saves"),
                access: GrantAccess::ReadWrite,
            }],
            graphics_backend: GraphicsBackend::Dxvk,
            ..recorded()
        };
        let plan = planner(&setup)
            .reset(&before, &wine_9(), true, CaseFolding::Off)
            .unwrap();
        assert_eq!(
            kinds(&plan),
            [
                "create-dir",
                "run",
                "require",
                "remove-tree",
                "remove-tree",
                "create-dir",
                "create-dir",
                "create-dir",
                "run",
                "require",
                "write-file",
                "write-file"
            ]
        );
        assert_eq!(run_command(&plan, 1).program, "/usr/bin/tar");
        assert!(plan.notes[0].contains("20261112T101500Z-before-reset"));
        // Only the prefix and the private home are deleted, never the
        // application directory with its metadata, and never before the
        // backup.
        assert_eq!(
            plan.steps[3],
            Step::RemoveTree {
                path: setup.app.prefix.clone()
            }
        );
        assert_eq!(
            plan.steps[4],
            Step::RemoveTree {
                path: setup.app.home.clone()
            }
        );
        let after = parse_metadata(&written(&plan, &setup.app.metadata)).unwrap();
        assert_eq!(after.grants, before.grants);
        assert_eq!(after.graphics_backend, GraphicsBackend::Dxvk);
        assert_eq!(after.prefix_id, before.prefix_id);
        assert_eq!(after.created_at, NOW);
        assert_eq!(after.launch_count, 0);
        assert_eq!(after.last_run_at, None);

        // If the backup fails, nothing is deleted.
        let mut executor = FakeExecutor {
            statuses: vec![2],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.failure.unwrap().step, 1);
        assert!(
            !executor
                .steps
                .iter()
                .any(|step| matches!(step, Step::RemoveTree { .. }))
        );

        let without = planner(&setup)
            .reset(&before, &wine_9(), false, CaseFolding::Off)
            .unwrap();
        assert_eq!(kinds(&without)[0], "remove-tree");
        assert!(without.notes[0].contains("without a backup"));

        // A reset with a newer Wine gives a prefix on the newer Wine, unless
        // the prefix is pinned.
        let newer = planner(&setup)
            .reset(&before, &wine_11(), true, CaseFolding::Off)
            .unwrap();
        let after = parse_metadata(&written(&newer, &setup.app.metadata)).unwrap();
        assert_eq!(after.runtime_version, "wine-11.0");
        let pinned = PrefixMetadata {
            pinned: true,
            ..before
        };
        assert_eq!(
            planner(&setup)
                .reset(&pinned, &wine_11(), true, CaseFolding::Off)
                .unwrap_err(),
            PlanError::Pinned {
                version: "wine-9.0".to_owned()
            }
        );
        assert!(
            planner(&setup)
                .reset(&pinned, &wine_9(), true, CaseFolding::Off)
                .is_ok()
        );
    }

    #[test]
    fn migration_is_explicit_backed_up_and_recorded() {
        let setup = Setup::new("prefix-migrate");
        let plan = planner(&setup).migrate(&recorded(), &wine_11()).unwrap();
        assert_eq!(plan.operation, Operation::Migrate);
        assert_eq!(
            kinds(&plan),
            [
                "create-dir",
                "run",
                "require",
                "run",
                "require",
                "write-file",
                "write-file"
            ]
        );
        // The backup comes first.
        assert_eq!(run_command(&plan, 1).program, "/usr/bin/tar");
        assert!(
            run_command(&plan, 1)
                .arguments
                .iter()
                .any(|argument| argument.ends_with("20261112T101500Z-before-migration.tar.gz"))
        );
        // Then the runtime's own prefix update.
        let update = run_command(&plan, 3);
        assert_eq!(
            update.arguments.last().map(String::as_str),
            Some("--update")
        );
        assert!(
            plan.steps[3]
                .describe()
                .contains("from wine-9.0 to wine-11.0")
        );
        // Then the record: the new version, and where it came from.
        let after = parse_metadata(&written(&plan, &setup.app.metadata)).unwrap();
        assert_eq!(after.runtime_version, "wine-11.0");
        assert_eq!(after.runtime_build, "wine-11.0");
        assert_eq!(
            after.migrations,
            [MigrationRecord {
                at: NOW,
                from_version: "wine-9.0".to_owned(),
                to_version: "wine-11.0".to_owned(),
                backup: "20261112T101500Z-before-migration".to_owned(),
            }]
        );
        assert_eq!(after.created_at, recorded().created_at);
        assert!(plan.notes[0].contains("20261112T101500Z-before-migration"));

        // A failed backup stops the migration before Wine touches the prefix.
        let mut executor = FakeExecutor {
            statuses: vec![2],
            ..FakeExecutor::default()
        };
        let report = run_plan(&plan, &mut executor);
        assert_eq!(report.failure.unwrap().step, 1);
        assert_eq!(executor.steps.len(), 2);

        // Already on the installed version: nothing to do.
        let nothing = planner(&setup).migrate(&recorded(), &wine_9()).unwrap();
        assert!(nothing.steps.is_empty());
        assert!(nothing.notes[0].contains("nothing to do"));

        // A pinned prefix is never migrated.
        let pinned = PrefixMetadata {
            pinned: true,
            ..recorded()
        };
        let error = planner(&setup).migrate(&pinned, &wine_11()).unwrap_err();
        assert_eq!(
            error,
            PlanError::Pinned {
                version: "wine-9.0".to_owned()
            }
        );
        assert_eq!(error.code(), Code::PrefixPinned);
    }

    #[test]
    fn grants_are_admitted_recorded_and_revoked() {
        let setup = Setup::new("prefix-grants");
        let planner = planner(&setup);
        let saves = planner
            .admit_grant(&setup.machine.path("Saves"), GrantAccess::ReadOnly)
            .unwrap();
        assert_eq!(saves.path, setup.machine.path("Saves"));

        let error = planner
            .admit_grant(&setup.machine.path(".ssh"), GrantAccess::ReadOnly)
            .unwrap_err();
        assert_eq!(error.code(), Code::GrantRefused);
        assert!(error.to_string().contains("the SSH directory"));
        for hostile in ["/", "/usr", "relative", "/proc/self/root"] {
            assert!(
                planner
                    .admit_grant(hostile, GrantAccess::ReadWrite)
                    .is_err(),
                "{hostile}"
            );
        }

        let granted = with_grant(&recorded(), saves.clone());
        assert_eq!(granted.grants, std::slice::from_ref(&saves));
        // Granting the same directory again replaces the entry.
        let upgraded = with_grant(
            &granted,
            Grant {
                access: GrantAccess::ReadWrite,
                ..saves.clone()
            },
        );
        assert_eq!(upgraded.grants.len(), 1);
        assert_eq!(upgraded.grants[0].access, GrantAccess::ReadWrite);

        let plan = planner.configure(&upgraded).unwrap();
        assert_eq!(plan.operation, Operation::Configure);
        assert_eq!(kinds(&plan), ["write-file"]);
        assert_eq!(written(&plan, &setup.app.metadata), upgraded.to_json());

        let revoked = without_grant(&upgraded, &saves.path).unwrap();
        assert!(revoked.grants.is_empty());
        assert_eq!(without_grant(&revoked, &saves.path), None);
        assert_eq!(without_grant(&upgraded, "/somewhere/else"), None);

        // A record that is not valid is not written.
        let invalid = PrefixMetadata {
            prefix_id: String::new(),
            ..recorded()
        };
        assert!(matches!(
            planner.configure(&invalid),
            Err(PlanError::Metadata(_))
        ));
    }

    #[test]
    fn metadata_is_loaded_from_beside_the_prefix() {
        let setup = Setup::new("prefix-load");
        // No directory: no prefix.
        assert_eq!(load_metadata(&setup.app), Ok(None));

        // A directory without metadata is damaged, not absent.
        fs::create_dir_all(&setup.app.prefix).unwrap();
        let error = load_metadata(&setup.app).unwrap_err();
        assert_eq!(error.code(), Code::PrefixMetadataInvalid);

        fs::write(&setup.app.metadata, recorded().to_json()).unwrap();
        assert_eq!(load_metadata(&setup.app), Ok(Some(recorded())));

        // Metadata for another application in this application's directory.
        fs::write(&setup.app.metadata, sample().to_json()).unwrap();
        let error = load_metadata(&setup.app).unwrap_err();
        assert_eq!(error.code(), Code::PrefixOwner);

        fs::write(&setup.app.metadata, b"{ not json").unwrap();
        assert!(matches!(
            load_metadata(&setup.app),
            Err(PlanError::Metadata(_))
        ));
        fs::write(&setup.app.metadata, " ".repeat(MAX_METADATA_BYTES + 10)).unwrap();
        assert!(matches!(
            load_metadata(&setup.app),
            Err(PlanError::Metadata(MetadataError::TooLarge { .. }))
        ));
    }

    #[test]
    fn default_prefix_names() {
        assert_eq!(default_prefix_id("com.example.app"), "com-example-app");
        assert_eq!(
            default_prefix_id("local.setup_wizard"),
            "local-setup-wizard"
        );
        assert!(is_slug(&default_prefix_id(&format!(
            "a.{}",
            "b".repeat(200)
        ))));
        assert!(is_backup_reason("before-reset"));
        assert!(!is_backup_reason("Before Reset"));
    }
}
