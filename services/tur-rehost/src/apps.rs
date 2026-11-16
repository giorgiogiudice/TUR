// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Re-hosted applications once they are set up: list, show, launch,
//! remove.
//!
//! A launch checks the payload against the record first (TUR's own hash,
//! because Electron's embedded check is off), then starts the program in
//! the application sandbox ([`crate::sandbox::app_arguments`]), as a
//! native window. Removal deletes the application's directory (runtime,
//! payload, private home, record), its launcher entry and its logs, then
//! checks that each is gone: everything the application could write was
//! inside its own directory, so nothing is left elsewhere.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::codes::{Code, Failure};
use crate::paths::{AppId, AppPaths, Roots, safe_relative};
use crate::record::{AppRecord, now_utc};
use crate::sandbox::{AppSandbox, RootLayout, Session, app_arguments, running_as_root};

/// One line of `list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listed {
    /// The identifier.
    pub app_id: String,
    /// The name.
    pub name: String,
    /// The application's version.
    pub app_version: Option<String>,
    /// `electron 43.7.7`.
    pub runtime: String,
    /// The package's platform.
    pub platform: String,
    /// When it was set up.
    pub installed_at: String,
}

/// Every re-hosted application with a valid record, by identifier.
/// Directories without one are not applications (an interrupted removal)
/// and are skipped.
pub fn list(roots: &Roots) -> Vec<Listed> {
    let Ok(listing) = std::fs::read_dir(roots.apps()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in listing.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(id) = AppId::parse(&name) else {
            continue;
        };
        let paths = AppPaths::new(roots.app(&id));
        if let Ok(record) = AppRecord::read(&paths.record, &id) {
            out.push(Listed {
                app_id: record.app_id.clone(),
                name: record.name.clone(),
                app_version: record.app_version.clone(),
                runtime: format!("{} {}", record.runtime.family, record.runtime.version),
                platform: record.package.platform.clone(),
                installed_at: record.installed_at.clone(),
            });
        }
    }
    out.sort_by(|a, b| a.app_id.cmp(&b.app_id));
    out
}

/// The record of `id`.
pub fn show(roots: &Roots, id: &AppId) -> Result<AppRecord, Failure> {
    AppRecord::read(&AppPaths::new(roots.app(id)).record, id)
}

/// How a launch went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Launched {
    /// The application.
    pub app_id: String,
    /// The program's exit status (or 128 + the signal).
    pub exit_status: i32,
    /// The launch log.
    pub log: String,
}

/// What a launch needs.
pub struct LaunchRequest<'a> {
    /// The roots.
    pub roots: &'a Roots,
    /// The application.
    pub id: &'a AppId,
    /// bubblewrap.
    pub bwrap: &'a Path,
    /// Start Chromium with its accessibility tree on, so that P@x and
    /// screen readers see the page (docs/reports/exp-rehost-1.md, 6.1).
    pub accessibility: bool,
    /// Arguments for the application.
    pub args: &'a [String],
    /// The session.
    pub session: &'a Session,
}

/// The program and arguments a launch runs: `bwrap`, its arguments, the
/// program, Chromium's flags and the application's arguments; and the
/// environment. Pure, for the plan and the tests.
pub fn launch_command(
    request: &LaunchRequest<'_>,
    record: &AppRecord,
    root: &RootLayout,
) -> Result<(Vec<String>, Vec<(String, String)>), Failure> {
    let paths = AppPaths::new(request.roots.app(request.id));
    let text = |path: &Path| {
        path.to_str().map(str::to_owned).ok_or_else(|| {
            Failure::new(Code::InputInvalid, "the application's directory is not valid UTF-8")
        })
    };
    let app = text(&paths.app)?;
    let home = text(&paths.home)?;
    let program = safe_relative(&record.program).ok_or_else(|| {
        Failure::new(Code::RecordInvalid, "the record names a program outside the application")
    })?;
    let (mut arguments, env) = app_arguments(&AppSandbox {
        app: &app,
        home: &home,
        session: request.session,
        root,
    });
    arguments.push(text(&paths.app.join(program))?);
    if request.accessibility {
        arguments.push("--force-renderer-accessibility".to_owned());
    }
    arguments.extend(request.args.iter().cloned());
    Ok((arguments, env))
}

/// Starts the application and waits for it.
pub fn launch(request: &LaunchRequest<'_>) -> Result<Launched, Failure> {
    if running_as_root() {
        return Err(Failure::new(
            Code::RunningAsRoot,
            "tur-rehost does not start applications as root",
        ));
    }
    let record = show(request.roots, request.id)?;
    let paths = AppPaths::new(request.roots.app(request.id));
    if let Some(expected) = &record.payload_sha256 {
        let payload = safe_relative(&record.payload)
            .map(|relative| paths.app.join(relative))
            .ok_or_else(|| Failure::new(Code::RecordInvalid, "the record names a payload outside the application"))?;
        let archive = if payload.is_file() {
            payload
        } else {
            paths.app.join("resources").join("app.asar")
        };
        let main = archive
            .parent()
            .map(|directory| directory.join("app.asar"))
            .filter(|path| path.is_file())
            .unwrap_or(archive);
        let (actual, _) = crate::fetch::hash_file(&main).map_err(|error| {
            Failure::new(Code::PayloadChanged, format!("the payload cannot be read: {error}"))
        })?;
        if actual != *expected {
            return Err(Failure::new(
                Code::PayloadChanged,
                format!(
                    "the payload of {} is not the one that was set up (SHA-256 {actual}, recorded {expected}): it was not started",
                    request.id
                ),
            ));
        }
    }
    let (arguments, env) = launch_command(request, &record, &RootLayout::of_host())?;
    let logs = request.roots.logs(request.id);
    std::fs::create_dir_all(&logs).map_err(|error| {
        Failure::new(Code::StepFailed, format!("cannot create {}: {error}", logs.display()))
    })?;
    let log_path: PathBuf = logs.join(format!(
        "{}-{}.log",
        now_utc().replace(':', ""),
        std::process::id()
    ));
    let mut header = format!(
        "tur-rehost launch {}\nprogram: {}\nenvironment names: {}\n\n",
        request.id,
        record.program,
        env.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(" ")
    );
    header.truncate(64 * 1024);
    std::fs::write(&log_path, header).map_err(|error| {
        Failure::new(Code::StepFailed, format!("cannot write the launch log: {error}"))
    })?;
    let log = std::fs::OpenOptions::new()
        .append(true)
        .open(&log_path)
        .map_err(|error| Failure::new(Code::StepFailed, format!("cannot open the launch log: {error}")))?;
    let log_for_errors = log.try_clone().map_err(|error| {
        Failure::new(Code::StepFailed, format!("cannot open the launch log: {error}"))
    })?;
    let status = Command::new(request.bwrap)
        .args(&arguments)
        .env_clear()
        .envs(env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_for_errors))
        .status()
        .map_err(|error| {
            Failure::new(
                Code::SandboxMissing,
                format!("the sandbox ({}) cannot be started: {error}", request.bwrap.display()),
            )
        })?;
    use std::os::unix::process::ExitStatusExt;
    let exit_status = status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1);
    Ok(Launched {
        app_id: request.id.as_str().to_owned(),
        exit_status,
        log: log_path.to_string_lossy().into_owned(),
    })
}

/// What a removal did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Removed {
    /// The application.
    pub app_id: String,
    /// What was deleted.
    pub deleted: Vec<String>,
    /// What was still there afterwards (empty when all went).
    pub left_behind: Vec<String>,
}

/// Removes the application: its directory, staging, launcher entry and
/// logs, then checks each is gone.
pub fn uninstall(roots: &Roots, id: &AppId) -> Result<Removed, Failure> {
    let directory = roots.app(id);
    if !directory.exists() && !roots.staging(id).exists() && !roots.launcher(id).exists() {
        return Err(Failure::new(Code::NotInstalled, format!("no application {id} is re-hosted")));
    }
    let targets = [
        directory,
        roots.staging(id),
        roots.launcher(id),
        roots.logs(id),
    ];
    let mut deleted = Vec::new();
    for target in &targets {
        // A link in place of a directory is removed, never followed.
        let Ok(metadata) = std::fs::symlink_metadata(target) else {
            continue;
        };
        let result = if metadata.is_dir() {
            std::fs::remove_dir_all(target)
        } else {
            std::fs::remove_file(target)
        };
        if result.is_ok() {
            deleted.push(target.to_string_lossy().into_owned());
        }
    }
    let left_behind: Vec<String> = targets
        .iter()
        .filter(|target| std::fs::symlink_metadata(target).is_ok())
        .map(|target| target.to_string_lossy().into_owned())
        .collect();
    let removed = Removed {
        app_id: id.as_str().to_owned(),
        deleted,
        left_behind,
    };
    if removed.left_behind.is_empty() {
        Ok(removed)
    } else {
        Err(Failure::new(
            Code::LeftBehind,
            format!("still there after the removal: {}", removed.left_behind.join(", ")),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{PackageRecord, RuntimeRecord};
    use std::collections::BTreeMap;

    fn record(id: &str) -> AppRecord {
        AppRecord {
            schema_version: crate::record::SCHEMA_VERSION,
            app_id: id.to_owned(),
            name: "Example".to_owned(),
            app_version: Some("1.0.0".to_owned()),
            runtime: RuntimeRecord {
                family: "electron".to_owned(),
                version: "43.7.7".to_owned(),
                url: "https://github.com/electron/electron/releases/download/v43.7.7/electron-v43.7.7-linux-x64.zip".to_owned(),
                sha256: "0".repeat(64),
                codecs: "upstream".to_owned(),
            },
            package: PackageRecord {
                file_name: "Example.zip".to_owned(),
                size: 1,
                sha256: "0".repeat(64),
                platform: "windows".to_owned(),
                layers: vec!["zip".to_owned()],
            },
            program: "Example".to_owned(),
            payload: "resources/app.asar".to_owned(),
            payload_sha256: None,
            fuses_publisher: BTreeMap::new(),
            fuses_set: BTreeMap::new(),
            modules: Vec::new(),
            system_helpers: Vec::new(),
            fetched: Vec::new(),
            updater: "off".to_owned(),
            installed_at: "2026-11-16T00:00:00Z".to_owned(),
            desktop_entry: "/x/tur-rehost-com.example.app.desktop".to_owned(),
        }
    }

    fn scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("tur-rehost-apps-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn a_launch_runs_the_program_in_the_sandbox_with_accessibility_when_asked() {
        let base = scratch("launch");
        let roots = Roots::under(&base);
        let id = AppId::parse("com.example.app").unwrap();
        let session = Session::default();
        let request = LaunchRequest {
            roots: &roots,
            id: &id,
            bwrap: Path::new("/usr/bin/bwrap"),
            accessibility: true,
            args: &["--a-flag".to_owned()],
            session: &session,
        };
        let (arguments, env) = launch_command(&request, &record("com.example.app"), &RootLayout::merged()).unwrap();
        let separator = arguments.iter().position(|a| a == "--").unwrap();
        let app = roots.app(&id).join("app");
        assert_eq!(arguments[separator + 1], app.join("Example").to_string_lossy());
        assert_eq!(arguments[separator + 2], "--force-renderer-accessibility");
        assert_eq!(arguments[separator + 3], "--a-flag");
        assert!(!arguments.contains(&"--no-sandbox".to_owned()));
        assert!(env.iter().any(|(name, value)| name == "HOME" && value.ends_with("com.example.app/home")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn removal_deletes_everything_of_the_application_and_says_so() {
        let base = scratch("remove");
        let roots = Roots::under(&base);
        let id = AppId::parse("com.example.app").unwrap();
        let paths = AppPaths::new(roots.app(&id));
        std::fs::create_dir_all(paths.home.join("Documents")).unwrap();
        std::fs::create_dir_all(&paths.app).unwrap();
        std::fs::write(&paths.record, serde_json::to_string(&record("com.example.app")).unwrap()).unwrap();
        std::fs::create_dir_all(roots.launcher(&id)).unwrap();
        std::fs::create_dir_all(roots.logs(&id)).unwrap();
        assert_eq!(list(&roots).len(), 1);
        let removed = uninstall(&roots, &id).unwrap();
        assert!(removed.left_behind.is_empty());
        assert_eq!(removed.deleted.len(), 3);
        assert!(!roots.app(&id).exists());
        assert!(list(&roots).is_empty());
        assert_eq!(uninstall(&roots, &id).unwrap_err().code, Code::NotInstalled);
        let _ = std::fs::remove_dir_all(&base);
    }
}
