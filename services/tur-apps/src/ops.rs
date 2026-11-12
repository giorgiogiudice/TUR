// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The operations: install, update, remove, list, launch, and switching
//! sources on and off.
//!
//! Each operation reports what it does as events (see `events`) and returns
//! a JSON document of its result, or a [`Failure`]. Nothing is done before
//! the plan has been shown, and an installation runs only with the user's
//! confirmation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};

use crate::codes::{Code, Failure};
use crate::digest::hash_file;
use crate::download::fetch_installer;
use crate::events::{Emitter, Event};
use crate::exec::{Program, Stream};
use crate::flatpak;
use crate::paths::display;
use crate::plan::{self, Context, Execution, Plan, Request, WindowsFile};
use crate::records::{self, FlatpakPart, Record, Source, WindowsPart, exists};
use crate::runtime;
use crate::tree;

/// What happens to an application's data when it is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataChoice {
    /// Keep it: a Flatpak's data stays where it is; a Windows
    /// application's documents and settings are copied aside first.
    Keep,
    /// Delete it with the application.
    Delete,
}

/// Shows a plan: the plan event, or the plan as text.
pub fn show_plan(emitter: &mut Emitter, plan: &Plan) {
    if emitter.is_json() {
        emitter.emit(Event::Plan {
            plan: serde_json::to_value(plan).unwrap_or_default(),
        });
    } else {
        emitter.text(&plan::render(plan));
    }
}

fn flatpak_refs(flatpak: &Program) -> Result<Vec<String>, Failure> {
    flatpak::installed_refs(flatpak)
}

fn flatpak_apps(flatpak: &Program) -> Vec<flatpak::ListedApp> {
    flatpak
        .run(&flatpak::list_apps_arguments())
        .ok()
        .filter(|outcome| outcome.success())
        .map(|outcome| flatpak::parse_apps(&outcome.stdout))
        .unwrap_or_default()
}

/// Forwards what a program prints as activity of a step: Flatpak's
/// operation lines as they are, and every other line as it is.
fn forward_lines<'a>(
    emitter: &'a mut Emitter,
    step: &'a str,
    stdout_too: bool,
) -> impl FnMut(Stream, &str) + 'a {
    move |stream, line| {
        if line.trim().is_empty() || (stream == Stream::Stdout && !stdout_too) {
            return;
        }
        let text = match flatpak::activity(line) {
            Some((what, reference)) => format!("{what} {reference}"),
            None => line.trim().to_owned(),
        };
        emitter.activity(step, text);
    }
}

/// Installs an application by its plan, after `confirm` has said yes.
pub fn install(
    context: &Context,
    request: &Request,
    emitter: &mut Emitter,
    confirm: &mut dyn FnMut(&Plan) -> bool,
) -> Result<Value, Failure> {
    let dirs = &context.dirs;
    if records::load(dirs, &request.id)?.is_some() {
        return Err(Failure::new(
            Code::AlreadyInstalled,
            format!(
                "{} is already installed; `tur-apps update {}` updates it, `tur-apps remove {}` removes it",
                request.id, request.id, request.id
            ),
        ));
    }
    let plan = plan::make(context, request)?;
    show_plan(emitter, &plan);
    if !confirm(&plan) {
        return Err(Failure::new(
            Code::ConfirmationNeeded,
            "Nothing was installed: confirm with --yes, or answer yes when asked.",
        ));
    }
    let record = match &plan.execution {
        Execution::Flatpak {
            app_id,
            remote,
            reference,
        } => install_flatpak(context, &plan, app_id, remote, reference, emitter)?,
        Execution::Windows {
            runtime_app_id,
            file,
            portable,
        } => install_windows(context, &plan, runtime_app_id, file, *portable, emitter)?,
    };
    emitter.started("record", "Recording the installation");
    records::save(dirs, &record)?;
    emitter.finished("record", true);
    Ok(json!({
        "id": record.id,
        "name": record.name,
        "route": record.route,
        "version": record.version,
        "source": record.source,
        "record": display(&dirs.record(&record.id), dirs),
        "flatpak": record.flatpak,
        "windows": record.windows,
    }))
}

fn install_flatpak(
    context: &Context,
    plan: &Plan,
    app_id: &str,
    remote: &str,
    reference: &str,
    emitter: &mut Emitter,
) -> Result<Record, Failure> {
    let flatpak = &context.flatpak;
    let before: BTreeSet<String> = flatpak_refs(flatpak)?.into_iter().collect();
    emitter.started(
        "install",
        format!(
            "Installing {} from {}",
            plan.application.name, plan.source.name
        ),
    );
    let outcome = {
        let mut forward = forward_lines(emitter, "install", true);
        flatpak.run_streaming(&flatpak::install_arguments(remote, reference), &mut forward)?
    };
    let after: BTreeSet<String> = flatpak_refs(flatpak)?.into_iter().collect();
    let added: Vec<String> = after.difference(&before).cloned().collect();
    if !outcome.success() || !after.contains(reference) {
        // Take away what this attempt brought in, so a failure leaves
        // nothing behind.
        if !added.is_empty() {
            let _ = flatpak.run(&flatpak::uninstall_arguments(&added, false));
        }
        emitter.finished("install", false);
        return Err(Failure::new(
            Code::StepFailed,
            format!(
                "flatpak did not install {reference}: {}. What it had added was removed again.",
                outcome.tail()
            ),
        ));
    }
    emitter.finished("install", true);
    let version = flatpak_apps(flatpak)
        .into_iter()
        .find(|listed| listed.application == app_id)
        .and_then(|listed| listed.version);
    let commit = flatpak
        .run(&flatpak::commit_arguments(reference))
        .ok()
        .filter(|outcome| outcome.success())
        .and_then(|outcome| flatpak::parse_commit(&outcome.stdout));
    Ok(Record {
        schema_version: records::RECORD_VERSION,
        id: plan.application.id.clone(),
        name: plan.application.name.clone(),
        route: "flatpak".to_owned(),
        source: Source {
            kind: "flatpak-remote".to_owned(),
            remote: Some(remote.to_owned()),
            url: plan.source.url.clone(),
            file_name: None,
            sha256: None,
            size: None,
            pinned: false,
        },
        version,
        installed_at: records::now_rfc3339(),
        flatpak: Some(FlatpakPart {
            app_id: app_id.to_owned(),
            reference: reference.to_owned(),
            remote: remote.to_owned(),
            commit,
            added_refs: added,
        }),
        windows: None,
    })
}

fn install_windows(
    context: &Context,
    plan: &Plan,
    runtime_app_id: &str,
    file: &WindowsFile,
    portable: bool,
    emitter: &mut Emitter,
) -> Result<Record, Failure> {
    let dirs = &context.dirs;
    let downloads = dirs.downloads(&plan.application.id);
    // The private directory the download went into, removed afterwards.
    let mut download_directory: Option<std::path::PathBuf> = None;
    let (path, source) = match file {
        WindowsFile::Download(pin) => {
            context.curl.require("to download the installer")?;
            emitter.started(
                "download",
                format!("Downloading {} from {}", pin.file_name(), pin.host()),
            );
            let total = pin.size;
            let fetched = fetch_installer(&context.curl.path, pin, &downloads, &mut |done| {
                emitter.emit(Event::Progress {
                    step: "download".to_owned(),
                    done,
                    total: Some(total),
                });
            });
            let fetched = match fetched {
                Ok(fetched) => fetched,
                Err(failure) => {
                    emitter.finished("download", false);
                    return Err(failure);
                }
            };
            emitter.finished("download", true);
            download_directory = fetched.path.parent().map(std::path::Path::to_path_buf);
            emitter.started(
                "verify",
                format!("SHA-256 {} matches the catalogue", fetched.hashed.sha256),
            );
            emitter.finished("verify", true);
            let source = Source {
                kind: "publisher-download".to_owned(),
                remote: None,
                url: Some(pin.url.clone()),
                file_name: Some(pin.file_name().to_owned()),
                sha256: Some(fetched.hashed.sha256.clone()),
                size: Some(fetched.hashed.size),
                pinned: true,
            };
            (fetched.path, source)
        }
        WindowsFile::Supplied {
            path,
            hashed,
            pinned,
        } => {
            // The file is hashed again just before it is used: it is the
            // user's file, and it may have changed since the plan.
            emitter.started("verify", format!("Checking {} again", path.display()));
            let now = hash_file(path)?;
            if &now != hashed {
                emitter.finished("verify", false);
                return Err(Failure::new(
                    Code::NotVerified,
                    format!(
                        "{} changed after the plan was made (its SHA-256 is now {}). Nothing in it was run.",
                        path.display(),
                        now.sha256
                    ),
                ));
            }
            emitter.finished("verify", true);
            let source = Source {
                kind: "user-file".to_owned(),
                remote: None,
                url: None,
                file_name: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned),
                sha256: Some(hashed.sha256.clone()),
                size: Some(hashed.size),
                pinned: *pinned,
            };
            (path.clone(), source)
        }
    };

    emitter.started(
        "install",
        format!(
            "Installing {} with tur-runtime, unattended, in its sandbox",
            plan.application.name
        ),
    );
    let arguments =
        runtime::install_arguments(&path, runtime_app_id, &plan.application.name, portable);
    let outcome = {
        let mut forward = forward_lines(emitter, "install", false);
        context.tur_runtime.run_streaming(&arguments, &mut forward)
    };
    if let Some(directory) = &download_directory {
        let _ = fs::remove_dir_all(directory);
    }
    let outcome = outcome?;
    if !outcome.success() {
        emitter.finished("install", false);
        let why = runtime::parse_error(&outcome.stdout).unwrap_or_else(|| outcome.tail());
        return Err(Failure::new(
            Code::StepFailed,
            format!(
                "tur-runtime did not install {} ({why}); it rolled its own installation back.",
                plan.application.name
            ),
        ));
    }
    emitter.finished("install", true);
    let installed = runtime::parse_install_report(&outcome.stdout);
    Ok(Record {
        schema_version: records::RECORD_VERSION,
        id: plan.application.id.clone(),
        name: plan.application.name.clone(),
        route: "wine".to_owned(),
        source,
        version: installed.version,
        installed_at: records::now_rfc3339(),
        flatpak: None,
        windows: Some(WindowsPart {
            runtime_app_id: runtime_app_id.to_owned(),
            main_executable: installed.main_executable,
        }),
    })
}

fn not_installed(id: &str) -> Failure {
    Failure::new(
        Code::NotInstalled,
        format!("{id} is not installed by tur-apps; `tur-apps list` shows what is"),
    )
}

/// Removes an application. Its data is kept, or deleted, as `choice` says.
pub fn remove(
    context: &Context,
    id: &str,
    choice: DataChoice,
    emitter: &mut Emitter,
) -> Result<Value, Failure> {
    let dirs = &context.dirs;
    let record = records::load(dirs, id)?.ok_or_else(|| not_installed(id))?;
    let (removed, kept, findings) = match (&record.flatpak, &record.windows) {
        (Some(part), _) => remove_flatpak(context, &record, part, choice, emitter)?,
        (None, Some(part)) => remove_windows(context, &record, part, choice, emitter)?,
        (None, None) => {
            return Err(Failure::new(
                Code::RecordInvalid,
                format!("the record of {id} names nothing installed"),
            ));
        }
    };
    records::remove(dirs, id)?;
    let result = json!({
        "id": record.id,
        "name": record.name,
        "route": record.route,
        "data": choice,
        "removed": removed,
        "kept": kept,
        "left_behind": findings,
    });
    if !findings.is_empty() {
        return Err(Failure::new(
            Code::LeftBehind,
            format!(
                "{} was removed, and {} thing(s) of it are still there: {}",
                record.name,
                findings.len(),
                findings.join("; ")
            ),
        )
        .with_detail(result));
    }
    Ok(result)
}

type Removed = (Vec<String>, Vec<String>, Vec<String>);

fn remove_flatpak(
    context: &Context,
    record: &Record,
    part: &FlatpakPart,
    choice: DataChoice,
    emitter: &mut Emitter,
) -> Result<Removed, Failure> {
    let dirs = &context.dirs;
    let flatpak = &context.flatpak;
    flatpak.require("to remove a Linux application")?;
    let data = dirs.flatpak_data(&part.app_id);
    let delete = choice == DataChoice::Delete;
    emitter.started(
        "uninstall",
        format!(
            "Removing {}{}",
            record.name,
            if delete {
                ", with its data"
            } else {
                "; its data is kept"
            }
        ),
    );
    let installed: BTreeSet<String> = flatpak_refs(flatpak)?.into_iter().collect();
    if installed.contains(&part.reference) {
        let outcome = {
            let mut forward = forward_lines(emitter, "uninstall", true);
            flatpak.run_streaming(
                &flatpak::uninstall_arguments(std::slice::from_ref(&part.reference), delete),
                &mut forward,
            )?
        };
        if !outcome.success() {
            emitter.finished("uninstall", false);
            return Err(Failure::new(
                Code::StepFailed,
                format!(
                    "flatpak did not remove {}: {}",
                    part.reference,
                    outcome.tail()
                ),
            ));
        }
    } else if delete {
        // Already gone (removed outside tur-apps): delete its data here.
        let _ = fs::remove_dir_all(&data);
    }
    emitter.finished("uninstall", true);

    // What this installation brought in and nothing else uses any more.
    let remaining: Vec<String> = flatpak_apps(flatpak)
        .into_iter()
        .filter_map(|listed| listed.runtime)
        .map(|runtime| runtime.split('/').next().unwrap_or_default().to_owned())
        .collect();
    let still_needed = |reference: &str| {
        let id = flatpak::ref_id(reference);
        remaining
            .iter()
            .any(|runtime| id == runtime || id.starts_with(&format!("{runtime}.")))
    };
    let now: BTreeSet<String> = flatpak_refs(flatpak)?.into_iter().collect();
    let unused: Vec<String> = part
        .added_refs
        .iter()
        .filter(|reference| **reference != part.reference)
        .filter(|reference| now.contains(*reference) && !still_needed(reference))
        .cloned()
        .collect();
    if !unused.is_empty() {
        emitter.started(
            "unused",
            format!(
                "Removing {} runtime(s) and extension(s) only it used",
                unused.len()
            ),
        );
        let outcome = {
            let mut forward = forward_lines(emitter, "unused", true);
            flatpak.run_streaming(&flatpak::uninstall_arguments(&unused, false), &mut forward)?
        };
        emitter.finished("unused", outcome.success());
    }

    let after: BTreeSet<String> = flatpak_refs(flatpak)?.into_iter().collect();
    let mut findings = Vec::new();
    for reference in &part.added_refs {
        if after.contains(reference) && (*reference == part.reference || !still_needed(reference)) {
            findings.push(format!("{reference} is still installed"));
        }
    }
    for path in [
        dirs.flatpak_app(&part.app_id),
        dirs.flatpak_desktop_entry(&part.app_id),
    ] {
        if exists(&path) {
            findings.push(format!("{} is still there", display(&path, dirs)));
        }
    }
    let mut kept = Vec::new();
    if delete {
        if exists(&data) {
            findings.push(format!("{} is still there", display(&data, dirs)));
        }
    } else if exists(&data) {
        kept.push(display(&data, dirs));
    }
    let mut removed = vec![part.reference.clone()];
    removed.extend(unused);
    if delete {
        removed.push(display(&data, dirs));
    }
    Ok((removed, kept, findings))
}

fn remove_windows(
    context: &Context,
    record: &Record,
    part: &WindowsPart,
    choice: DataChoice,
    emitter: &mut Emitter,
) -> Result<Removed, Failure> {
    let dirs = &context.dirs;
    context
        .tur_runtime
        .require("to remove a Windows application")?;
    let app_id = &part.runtime_app_id;
    let mut kept = Vec::new();
    if choice == DataChoice::Keep {
        // tur-runtime deletes the private home with the application. What
        // the user made in it is copied aside first; if that fails, nothing
        // is removed.
        let target = dirs.kept(&record.id).join(records::stamp());
        emitter.started(
            "keep",
            format!(
                "Copying {}'s documents and settings to {}",
                record.name,
                display(&target, dirs)
            ),
        );
        let mut copied_any = false;
        for (source, name) in [
            (dirs.runtime_home(app_id), "home"),
            (dirs.runtime_profiles(app_id), "windows-profiles"),
        ] {
            if !exists(&source) {
                continue;
            }
            match tree::copy(&source, &target.join(name)) {
                Ok(copied) => {
                    copied_any = true;
                    emitter.activity(
                        "keep",
                        format!(
                            "{name}: {} file(s), {} byte(s){}",
                            copied.files,
                            copied.bytes,
                            if copied.skipped.is_empty() {
                                String::new()
                            } else {
                                format!("; links not followed: {}", copied.skipped.join(", "))
                            }
                        ),
                    );
                }
                Err(failure) => {
                    emitter.finished("keep", false);
                    let _ = fs::remove_dir_all(&target);
                    return Err(Failure::new(
                        Code::StepFailed,
                        format!(
                            "{}'s data could not be copied aside, so nothing was removed: {}",
                            record.name, failure.message
                        ),
                    ));
                }
            }
        }
        emitter.finished("keep", true);
        if copied_any {
            kept.push(display(&target, dirs));
        }
    }

    emitter.started(
        "uninstall",
        format!("Removing {} with tur-runtime", record.name),
    );
    let outcome = {
        let mut forward = forward_lines(emitter, "uninstall", false);
        context.tur_runtime.run_streaming(
            &runtime::uninstall_arguments(app_id, choice == DataChoice::Delete),
            &mut forward,
        )?
    };
    let report: Value = serde_json::from_str(outcome.stdout.trim()).unwrap_or(Value::Null);
    let mut findings: Vec<String> = Vec::new();
    match outcome.status {
        Some(0) => {}
        Some(8) => {
            if let Some(left) = report["left_behind"].as_array() {
                findings.extend(left.iter().map(|finding| {
                    finding["found"]
                        .as_str()
                        .map_or_else(|| finding.to_string(), str::to_owned)
                }));
            }
            if findings.is_empty() {
                findings
                    .push(runtime::parse_error(&outcome.stdout).unwrap_or_else(|| outcome.tail()));
            }
        }
        _ => {
            emitter.finished("uninstall", false);
            let why = runtime::parse_error(&outcome.stdout).unwrap_or_else(|| outcome.tail());
            return Err(Failure::new(
                Code::StepFailed,
                format!("tur-runtime did not remove {} ({why})", record.name),
            ));
        }
    }
    emitter.finished("uninstall", findings.is_empty());
    if let Some(also_kept) = report["kept"].as_array() {
        kept.extend(
            also_kept
                .iter()
                .filter_map(Value::as_str)
                .map(|path| display(&PathBuf::from(path), dirs)),
        );
    }
    for path in [dirs.runtime_app(app_id), dirs.runtime_launcher(app_id)] {
        if exists(&path) {
            findings.push(format!("{} is still there", display(&path, dirs)));
        }
    }
    let removed = report["removed"]
        .as_array()
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(|path| display(&PathBuf::from(path), dirs))
                .collect()
        })
        .unwrap_or_else(|| vec![display(&dirs.runtime_app(app_id), dirs)]);
    Ok((removed, kept, findings))
}

/// Updates an application.
pub fn update(context: &Context, id: &str, emitter: &mut Emitter) -> Result<Value, Failure> {
    let dirs = &context.dirs;
    let mut record = records::load(dirs, id)?.ok_or_else(|| not_installed(id))?;
    let Some(part) = record.flatpak.clone() else {
        return Err(Failure::new(
            Code::RouteNotBuilt,
            format!(
                "Updating a Windows application is not built yet: tur-runtime has no update. To move {} to a newer version, remove it (its data is kept) and install it again.",
                record.name
            ),
        ));
    };
    let flatpak = &context.flatpak;
    flatpak.require("to update a Linux application")?;
    let before = record.version.clone();
    emitter.started("update", format!("Updating {}", record.name));
    let outcome = {
        let mut forward = forward_lines(emitter, "update", true);
        flatpak.run_streaming(&flatpak::update_arguments(&part.reference), &mut forward)?
    };
    if !outcome.success() {
        emitter.finished("update", false);
        return Err(Failure::new(
            Code::StepFailed,
            format!(
                "flatpak did not update {}: {}",
                part.reference,
                outcome.tail()
            ),
        ));
    }
    emitter.finished("update", true);
    record.version = flatpak_apps(flatpak)
        .into_iter()
        .find(|listed| listed.application == part.app_id)
        .and_then(|listed| listed.version)
        .or(record.version);
    let commit = flatpak
        .run(&flatpak::commit_arguments(&part.reference))
        .ok()
        .filter(|outcome| outcome.success())
        .and_then(|outcome| flatpak::parse_commit(&outcome.stdout));
    let changed = commit.is_some() && commit != part.commit;
    if let Some(flatpak_part) = record.flatpak.as_mut() {
        flatpak_part.commit = commit.or(part.commit);
    }
    records::save(dirs, &record)?;
    Ok(json!({
        "id": record.id,
        "updated": changed,
        "version_before": before,
        "version": record.version,
    }))
}

/// What is known about an update of one application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateState {
    /// Whether anyone was asked.
    pub checked: bool,
    /// Whether an update is known: `None` when not checked.
    pub available: Option<bool>,
    /// The version an update brings, when known.
    pub version: Option<String>,
    /// Where the answer comes from.
    pub source: Option<String>,
}

impl UpdateState {
    fn not_checked() -> Self {
        UpdateState {
            checked: false,
            available: None,
            version: None,
            source: None,
        }
    }
}

/// One installed application in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listed {
    /// The catalogue identifier, or the Flatpak or tur-runtime identifier
    /// when the catalogue does not know the application.
    pub id: String,
    /// Its name.
    pub name: String,
    /// `flatpak` or `wine`.
    pub route: String,
    /// `tur-apps` when this program installed it; `flatpak` or
    /// `tur-runtime` when it was installed with that program directly.
    pub installed_by: String,
    /// Where it came from, in a few words.
    pub from: String,
    /// The version, when known.
    pub version: Option<String>,
    /// The space it takes, in bytes, when known.
    pub size: Option<u64>,
    /// Where its data is.
    pub data: Vec<String>,
    /// `installed`, or `missing` when the record says installed and the
    /// application is not there.
    pub state: String,
    /// Whether an update is known.
    pub update: UpdateState,
}

/// The available Flatpak updates, by Flatpak identifier, or why they could
/// not be had.
fn flatpak_updates(flatpak: &Program) -> Result<BTreeMap<String, Option<String>>, String> {
    let outcome = flatpak
        .run(&flatpak::updates_arguments())
        .map_err(|failure| failure.message)?;
    if !outcome.success() {
        return Err(outcome.tail());
    }
    Ok(flatpak::parse_table(&outcome.stdout)
        .into_iter()
        .filter_map(|row| {
            let id = row.first()?.clone();
            let version = row
                .get(1)
                .filter(|version| !version.is_empty() && *version != "-")
                .cloned();
            Some((id, version))
        })
        .collect())
}

/// One list of everything installed for this user, across routes.
///
/// With `check_updates`, the Flatpak remotes are asked for updates, and a
/// Windows application installed from a pinned download is compared with
/// the catalogue's current pin.
pub fn list(context: &Context, check_updates: bool) -> Result<Value, Failure> {
    let dirs = &context.dirs;
    let (records, mut problems) = records::load_all(dirs);
    let catalogue = plan::load_catalogue(&context.catalogue).ok();
    let flatpak_listed = if context.flatpak.exists() {
        flatpak_apps(&context.flatpak)
    } else {
        Vec::new()
    };
    let runtime_listed = if context.tur_runtime.exists() {
        context
            .tur_runtime
            .run(&runtime::list_arguments())
            .ok()
            .filter(|outcome| outcome.success())
            .map(|outcome| runtime::parse_list(&outcome.stdout))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let updates = if check_updates && context.flatpak.exists() {
        match flatpak_updates(&context.flatpak) {
            Ok(found) => Some(found),
            Err(why) => {
                problems.push(format!(
                    "the Flatpak sources could not be asked for updates: {why}"
                ));
                None
            }
        }
    } else {
        None
    };
    let flatpak_update = |flatpak_id: &str| match &updates {
        Some(found) => UpdateState {
            checked: true,
            available: Some(found.contains_key(flatpak_id)),
            version: found.get(flatpak_id).cloned().flatten(),
            source: Some("flatpak remote-ls --updates".to_owned()),
        },
        None => UpdateState::not_checked(),
    };

    let mut applications = Vec::new();
    let mut managed_flatpaks = BTreeSet::new();
    let mut managed_windows = BTreeSet::new();
    for record in &records {
        if let Some(part) = &record.flatpak {
            managed_flatpaks.insert(part.app_id.clone());
            let live = flatpak_listed
                .iter()
                .find(|listed| listed.application == part.app_id);
            let size = context
                .flatpak
                .run(&flatpak::size_arguments(&part.reference))
                .ok()
                .filter(|outcome| outcome.success())
                .and_then(|outcome| flatpak::parse_size(&outcome.stdout));
            applications.push(Listed {
                id: record.id.clone(),
                name: record.name.clone(),
                route: record.route.clone(),
                installed_by: "tur-apps".to_owned(),
                from: format!("Flatpak remote {}", part.remote),
                version: live
                    .and_then(|listed| listed.version.clone())
                    .or_else(|| record.version.clone()),
                size,
                data: vec![display(&dirs.flatpak_data(&part.app_id), dirs)],
                state: if live.is_some() {
                    "installed"
                } else {
                    "missing"
                }
                .to_owned(),
                update: flatpak_update(&part.app_id),
            });
        } else if let Some(part) = &record.windows {
            managed_windows.insert(part.runtime_app_id.clone());
            let live = runtime_listed
                .iter()
                .find(|listed| listed.app_id == part.runtime_app_id);
            let pin = catalogue
                .as_ref()
                .and_then(|catalogue| catalogue.get(&record.id))
                .and_then(|entry| {
                    entry
                        .editions
                        .iter()
                        .find_map(|edition| edition.installer.clone())
                });
            let update = if check_updates && record.source.pinned {
                match &pin {
                    Some(pin) => UpdateState {
                        checked: true,
                        available: Some(
                            record.source.sha256.as_deref() != Some(pin.sha256.as_str()),
                        ),
                        version: Some(pin.version.clone()),
                        source: Some("the catalogue's pinned download".to_owned()),
                    },
                    None => UpdateState::not_checked(),
                }
            } else {
                UpdateState::not_checked()
            };
            let from = match (&record.source.url, &record.source.file_name) {
                (Some(url), _) => format!("download from {url}"),
                (None, Some(name)) => format!("your file {name}"),
                (None, None) => "a file".to_owned(),
            };
            applications.push(Listed {
                id: record.id.clone(),
                name: record.name.clone(),
                route: record.route.clone(),
                installed_by: "tur-apps".to_owned(),
                from,
                version: live
                    .and_then(|listed| listed.version.clone())
                    .or_else(|| record.version.clone()),
                size: exists(&dirs.runtime_app(&part.runtime_app_id))
                    .then(|| tree::size(&dirs.runtime_app(&part.runtime_app_id))),
                data: vec![
                    display(&dirs.runtime_home(&part.runtime_app_id), dirs),
                    display(&dirs.runtime_profiles(&part.runtime_app_id), dirs),
                ],
                state: if live.is_some() {
                    "installed"
                } else {
                    "missing"
                }
                .to_owned(),
                update,
            });
        }
    }
    // What was installed with flatpak or tur-runtime directly.
    for listed in flatpak_listed
        .iter()
        .filter(|listed| !managed_flatpaks.contains(&listed.application))
    {
        let entry = catalogue.as_ref().and_then(|catalogue| {
            catalogue.entries().iter().find(|entry| {
                entry.editions.iter().any(|edition| {
                    edition.flatpak_id.as_deref() == Some(listed.application.as_str())
                })
            })
        });
        let reference = format!(
            "app/{}/{}/{}",
            listed.application,
            flatpak::arch(),
            listed.branch
        );
        let size = context
            .flatpak
            .run(&flatpak::size_arguments(&reference))
            .ok()
            .filter(|outcome| outcome.success())
            .and_then(|outcome| flatpak::parse_size(&outcome.stdout));
        applications.push(Listed {
            id: entry.map_or_else(|| listed.application.clone(), |entry| entry.id.clone()),
            name: entry.map_or_else(|| listed.application.clone(), |entry| entry.name.clone()),
            route: "flatpak".to_owned(),
            installed_by: "flatpak".to_owned(),
            from: format!("Flatpak remote {}", listed.origin),
            version: listed.version.clone(),
            size,
            data: vec![display(&dirs.flatpak_data(&listed.application), dirs)],
            state: "installed".to_owned(),
            update: flatpak_update(&listed.application),
        });
    }
    for listed in runtime_listed
        .iter()
        .filter(|listed| !managed_windows.contains(&listed.app_id))
    {
        applications.push(Listed {
            id: listed.app_id.clone(),
            name: listed.name.clone(),
            route: "wine".to_owned(),
            installed_by: "tur-runtime".to_owned(),
            from: "an installer given to tur-runtime".to_owned(),
            version: listed.version.clone(),
            size: Some(tree::size(&dirs.runtime_app(&listed.app_id))),
            data: vec![display(&dirs.runtime_home(&listed.app_id), dirs)],
            state: "installed".to_owned(),
            update: UpdateState::not_checked(),
        });
    }
    applications.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    Ok(json!({
        "schema_version": 1,
        "kind": "installed",
        "updates_checked": check_updates,
        "applications": applications,
        "problems": problems,
    }))
}

/// Starts an installed application and waits for it. Returns its exit
/// status.
pub fn launch(context: &Context, id: &str) -> Result<i32, Failure> {
    let record = records::load(&context.dirs, id)?.ok_or_else(|| not_installed(id))?;
    let (program, arguments) = match (&record.flatpak, &record.windows) {
        (Some(part), _) => (&context.flatpak, flatpak::run_arguments(&part.app_id)),
        (None, Some(part)) => (
            &context.tur_runtime,
            runtime::launch_arguments(&part.runtime_app_id),
        ),
        (None, None) => {
            return Err(Failure::new(
                Code::RecordInvalid,
                format!("the record of {id} names nothing installed"),
            ));
        }
    };
    program.require("to start the application")?;
    let mut command = std::process::Command::new(&program.path);
    command.args(&arguments);
    let status = crate::exec::spawn(&mut command)
        .and_then(|mut child| child.wait())
        .map_err(|error| {
            Failure::new(
                Code::StepFailed,
                format!("cannot start {}: {error}", record.name),
            )
        })?;
    Ok(status.code().unwrap_or(1))
}

/// The state of the Flatpak sources TUR knows, and of the user's others.
pub fn sources(context: &Context) -> Result<Value, Failure> {
    context.flatpak.require("to list Flatpak sources")?;
    let outcome = context.flatpak.run(&flatpak::remotes_arguments())?;
    if !outcome.success() {
        return Err(Failure::new(
            Code::StepFailed,
            format!("flatpak could not list your sources: {}", outcome.tail()),
        ));
    }
    let remotes = flatpak::parse_remotes(&outcome.stdout);
    let mut list = Vec::new();
    for known in flatpak::KNOWN_SOURCES {
        let found = remotes.iter().find(|remote| remote.name == known.name);
        list.push(json!({
            "name": known.name,
            "title": known.title,
            "carries": known.carries,
            "location": known.location,
            "state": match found {
                Some(remote) if remote.disabled => "off",
                Some(_) => "on",
                None => "not-added",
            },
        }));
    }
    for remote in remotes
        .iter()
        .filter(|remote| flatpak::known_source(&remote.name).is_none())
    {
        list.push(json!({
            "name": remote.name,
            "title": remote.name,
            "location": remote.url,
            "state": if remote.disabled { "off" } else { "on" },
        }));
    }
    Ok(json!({ "schema_version": 1, "kind": "sources", "scope": "user", "sources": list }))
}

/// Switches a known source on or off, for this user.
pub fn set_source(
    context: &Context,
    name: &str,
    on: bool,
    emitter: &mut Emitter,
) -> Result<Value, Failure> {
    let Some(known) = flatpak::known_source(name) else {
        let names: Vec<&str> = flatpak::KNOWN_SOURCES
            .iter()
            .map(|source| source.name)
            .collect();
        return Err(Failure::new(
            Code::InputInvalid,
            format!(
                "TUR knows the sources {}, not \"{name}\"",
                names.join(" and ")
            ),
        ));
    };
    context.flatpak.require("to switch a Flatpak source")?;
    let current = plan::remote_state(&context.flatpak, name)?;
    let arguments = match (&current, on) {
        (None, true) => Some(flatpak::remote_add_arguments(known)),
        (Some(remote), true) if remote.disabled => {
            Some(flatpak::remote_modify_arguments(name, true))
        }
        (Some(remote), false) if !remote.disabled => {
            Some(flatpak::remote_modify_arguments(name, false))
        }
        _ => None,
    };
    let changed = arguments.is_some();
    if let Some(arguments) = arguments {
        emitter.started(
            "source",
            format!(
                "Switching {} {}",
                known.title,
                if on { "on" } else { "off" }
            ),
        );
        let outcome = context.flatpak.run(&arguments)?;
        if !outcome.success() {
            emitter.finished("source", false);
            return Err(Failure::new(
                Code::StepFailed,
                format!(
                    "flatpak could not switch {} {}: {}",
                    known.title,
                    if on { "on" } else { "off" },
                    outcome.tail()
                ),
            ));
        }
        emitter.finished("source", true);
    }
    Ok(json!({ "name": name, "state": if on { "on" } else { "off" }, "changed": changed }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_choice_is_written_in_words() {
        assert_eq!(serde_json::to_value(DataChoice::Keep).unwrap(), "keep");
        assert_eq!(serde_json::to_value(DataChoice::Delete).unwrap(), "delete");
    }

    #[test]
    fn dirs_are_used_for_kept_data() {
        let dirs =
            crate::paths::Dirs::from_env(|name| (name == "HOME").then(|| "/home/u".to_owned()))
                .unwrap();
        assert!(
            dirs.kept("a.b")
                .starts_with("/home/u/.local/share/tur/apps/kept")
        );
    }
}
