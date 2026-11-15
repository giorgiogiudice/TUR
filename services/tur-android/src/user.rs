// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The person's side: their Android session, and the applications in it.
//!
//! Everything here runs as the person, in their desktop session. The
//! Android session is Waydroid's (`waydroid session start`), started as a
//! transient unit of the person's own service manager so that it outlives
//! this command and ends with the person's session. Applications are
//! installed, listed, started and removed through Waydroid's command line;
//! each result is checked here, because Waydroid's commands report many
//! failures only in their log.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::apk;
use crate::codes::Code;
use crate::host::{self, ENABLED_FLAG, WAYDROID, WAYDROID_CFG};
use crate::outcome::{Done, Failure};
use crate::pins::Pins;
use crate::run::{self, seconds};

/// The transient unit the Android session runs in.
pub(crate) const SESSION_UNIT: &str = "tur-android-session.service";

/// An application as Waydroid lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct App {
    /// Its label.
    pub(crate) name: String,
    /// Its package name, the identifier used to start or remove it.
    pub(crate) id: String,
}

/// Reads what `waydroid app list` prints.
pub(crate) fn parse_app_list(text: &str) -> Vec<App> {
    let mut apps = Vec::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("Name:") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("packageName:") {
            apps.push(App {
                name: name.take().unwrap_or_default(),
                id: value.trim().to_owned(),
            });
        }
    }
    apps
}

/// A launcher entry Waydroid wrote for an application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct LauncherEntry {
    /// The application's identifier.
    pub(crate) id: String,
    /// Its label.
    pub(crate) name: String,
    /// The desktop entry file.
    pub(crate) file: String,
    /// The icon file, when the entry names one that exists.
    pub(crate) icon: Option<String>,
    /// Hidden from the launcher (Waydroid hides Android's own applications).
    pub(crate) hidden: bool,
}

/// Reads one of Waydroid's desktop entries.
pub(crate) fn parse_entry(id: &str, file: &Path, text: &str) -> LauncherEntry {
    let mut name = String::new();
    let mut icon = None;
    let mut hidden = false;
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(value) = line.strip_prefix("Name=") {
            name = value.to_owned();
        } else if let Some(value) = line.strip_prefix("Icon=") {
            icon = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("NoDisplay=") {
            hidden = value.trim() == "true";
        }
    }
    LauncherEntry {
        id: id.to_owned(),
        name,
        file: file.display().to_string(),
        icon: icon.filter(|path| Path::new(path).is_file()),
        hidden,
    }
}

fn data_home() -> PathBuf {
    match env::var_os("XDG_DATA_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/share"),
    }
}

/// The launcher entry of `id`, if Waydroid wrote one.
pub(crate) fn launcher_entry(id: &str) -> Option<LauncherEntry> {
    let file = data_home()
        .join("applications")
        .join(format!("waydroid.{id}.desktop"));
    let text = fs::read_to_string(&file).ok()?;
    Some(parse_entry(id, &file, &text))
}

/// Every launcher entry Waydroid wrote.
pub(crate) fn launcher_entries() -> Vec<LauncherEntry> {
    let dir = data_home().join("applications");
    let mut entries: Vec<LauncherEntry> = fs::read_dir(&dir)
        .map(|list| {
            list.filter_map(Result::ok)
                .filter_map(|entry| {
                    let file = entry.file_name().to_string_lossy().into_owned();
                    let id = file.strip_prefix("waydroid.")?.strip_suffix(".desktop")?;
                    // Waydroid's own entries for the whole Android screen.
                    if id.is_empty() || !id.contains('.') {
                        return None;
                    }
                    let text = fs::read_to_string(entry.path()).ok()?;
                    Some(parse_entry(id, &entry.path(), &text))
                })
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries
}

/// What TUR recorded of a package the person installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Installed {
    /// The package's SHA-256.
    pub(crate) sha256: String,
    /// Its version code.
    pub(crate) version_code: Option<u64>,
    /// Its version name.
    pub(crate) version_name: Option<String>,
    /// The file it was installed from.
    pub(crate) file: String,
    /// When, in seconds since 1970.
    pub(crate) installed_at: u64,
}

fn record_path() -> PathBuf {
    data_home().join("tur/android/installed.json")
}

fn read_record() -> BTreeMap<String, Installed> {
    fs::read_to_string(record_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_record(record: &BTreeMap<String, Installed>) {
    let path = record_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, serde_json::to_string_pretty(record).unwrap_or_default());
}

/// The person's session, as far as Android needs it.
#[derive(Debug, Clone)]
struct Session {
    runtime_dir: PathBuf,
    wayland: String,
    pulse: Option<String>,
}

/// Android support is switched on.
pub(crate) fn enabled() -> bool {
    Path::new(ENABLED_FLAG).exists() && Path::new(WAYDROID_CFG).exists()
}

fn require_enabled() -> Result<(), Failure> {
    if host::waydroid_version().is_none() {
        return Err(Failure::refused(
            Code::WaydroidMissing,
            "Waydroid, the Android runtime, is not installed in this system image.",
        ));
    }
    if enabled() {
        Ok(())
    } else {
        Err(Failure::refused(
            Code::NotEnabled,
            "Android is switched off. Switch it on first: `tur-android enable` explains what \
             that downloads and costs before it does anything.",
        ))
    }
}

/// Checks that this runs in the person's desktop session, with a Wayland
/// display and a sound server: Waydroid binds both into the container, and
/// the container does not start without the sound server's socket
/// (measured, exp-android-1, section 6).
fn session() -> Result<Session, Failure> {
    if host::effective_uid() == Some(0) {
        return Err(Failure::refused(
            Code::NoSession,
            "Android applications belong to a person: run this as yourself, not as root.",
        ));
    }
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let wayland = env::var("WAYLAND_DISPLAY").ok().filter(|v| !v.is_empty());
    let (Some(runtime_dir), Some(wayland)) = (runtime_dir, wayland) else {
        return Err(Failure::refused(
            Code::NoSession,
            "There is no Wayland desktop session here (XDG_RUNTIME_DIR or WAYLAND_DISPLAY is \
             not set). Android applications appear as windows on your desktop, so run this \
             from your desktop session.",
        ));
    };
    if !runtime_dir.join(&wayland).exists() && !Path::new(&wayland).exists() {
        return Err(Failure::refused(
            Code::NoSession,
            format!("The Wayland display {wayland} does not exist."),
        ));
    }
    let pulse = env::var("PULSE_RUNTIME_PATH").ok().filter(|v| !v.is_empty());
    let pulse_dir = pulse
        .as_ref()
        .map_or_else(|| runtime_dir.join("pulse"), PathBuf::from);
    if !pulse_dir.join("native").exists() {
        return Err(Failure::refused(
            Code::NoSoundServer,
            format!(
                "Your session has no sound server socket ({}). Waydroid passes it to Android and \
                 the container does not start without it. PipeWire normally provides it; check \
                 that pipewire-pulse is running.",
                pulse_dir.join("native").display()
            ),
        ));
    }
    Ok(Session {
        runtime_dir,
        wayland,
        pulse,
    })
}

/// The applications Waydroid lists now, or `None` when Android is not
/// running or not ready.
fn apps_now() -> Option<Vec<App>> {
    let output = run::run(WAYDROID, &["app", "list"], Duration::from_secs(45));
    let apps = parse_app_list(&output.text());
    if apps.is_empty() { None } else { Some(apps) }
}

fn user_unit_state(unit: &str) -> String {
    let output = run::run(
        "systemctl",
        &["--user", "show", "--property=ActiveState", "--value", unit],
        Duration::from_secs(20),
    );
    output.text().trim().to_owned()
}

fn user_journal(unit: &str) -> String {
    run::run(
        "journalctl",
        &["--user", "--no-pager", "-o", "cat", "-n", "40", "-u", unit],
        Duration::from_secs(20),
    )
    .text()
}

/// Makes sure Android is running in the person's session and ready, and
/// returns its applications and, if it had to be started, how long that
/// took.
fn ensure_running(limit: Duration) -> Result<(Vec<App>, Option<f64>), Failure> {
    let session = session()?;
    if let Some(apps) = apps_now() {
        return Ok((apps, None));
    }
    let started = Instant::now();
    let _ = run::run(
        "systemctl",
        &["--user", "reset-failed", SESSION_UNIT],
        Duration::from_secs(20),
    );
    let mut command = Command::new("systemd-run");
    command
        .arg("--user")
        .arg(format!("--unit={SESSION_UNIT}"))
        .arg("--collect")
        .arg("--quiet")
        .arg(format!(
            "--setenv=XDG_RUNTIME_DIR={}",
            session.runtime_dir.display()
        ))
        .arg(format!("--setenv=WAYLAND_DISPLAY={}", session.wayland));
    if let Some(pulse) = &session.pulse {
        command.arg(format!("--setenv=PULSE_RUNTIME_PATH={pulse}"));
    }
    command.args(["--", WAYDROID, "session", "start"]);
    let start = run::run_command(&mut command, Duration::from_secs(60));
    if !start.ok() && !start.stderr.contains("already") {
        return Err(Failure::failed(
            Code::ContainerFailed,
            format!("The Android session could not be started: {}", start.account()),
        ));
    }
    loop {
        thread::sleep(Duration::from_secs(3));
        if let Some(apps) = apps_now() {
            return Ok((apps, Some(seconds(started.elapsed()))));
        }
        let state = user_unit_state(SESSION_UNIT);
        if started.elapsed() > Duration::from_secs(15)
            && (state == "failed" || state == "inactive")
        {
            return Err(Failure::failed(
                Code::ContainerFailed,
                format!(
                    "The Android session ended while starting ({state}). {}",
                    user_journal(SESSION_UNIT)
                ),
            ));
        }
        if started.elapsed() >= limit {
            return Err(Failure::failed(
                Code::BootTimeout,
                format!(
                    "Android did not finish starting within {} seconds. {}",
                    limit.as_secs(),
                    user_journal(SESSION_UNIT)
                ),
            ));
        }
    }
}

/// Waits until `check` holds or `limit` passes.
fn poll<T>(limit: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let started = Instant::now();
    loop {
        if let Some(found) = check() {
            return Some(found);
        }
        if started.elapsed() >= limit {
            return None;
        }
        thread::sleep(Duration::from_secs(2));
    }
}

/// Installs the package at `path`.
pub(crate) fn install(path: &Path, pins: &Pins, limit: Duration) -> Result<Done, Failure> {
    let package = apk::inspect(path, pins.api_level).map_err(|refusal| {
        Failure::refused(refusal.code, refusal.message).with("file", json!(path.display().to_string()))
    })?;
    require_enabled()?;
    let id = package.manifest.package.clone();
    let facts = json!(package);
    let (_, boot) = ensure_running(limit).map_err(|f| f.with("package", facts.clone()))?;
    let started = Instant::now();
    let file = path.to_string_lossy().into_owned();
    let install = run::run(WAYDROID, &["app", "install", &file], Duration::from_secs(300));
    let listed = poll(Duration::from_secs(90), || {
        apps_now().and_then(|apps| apps.into_iter().find(|app| app.id == id))
    });
    let Some(app) = listed else {
        return Err(Failure::failed(
            Code::InstallFailed,
            format!(
                "Android did not install {id}: waydroid app install {}",
                install.account()
            ),
        )
        .with("package", facts));
    };
    let install_seconds = seconds(started.elapsed());
    let mut record = read_record();
    record.insert(
        id.clone(),
        Installed {
            sha256: package.sha256.clone(),
            version_code: package.manifest.version_code,
            version_name: package.manifest.version_name.clone(),
            file: path.display().to_string(),
            installed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        },
    );
    write_record(&record);
    let entry = poll(Duration::from_secs(30), || launcher_entry(&id));
    let mut done = Done::new(format!(
        "{} ({id}) is installed{}.",
        app.name,
        if entry.is_some() {
            " and is in the application launcher"
        } else {
            ""
        }
    ))
    .with("id", json!(id))
    .with("name", json!(app.name))
    .with("package", facts)
    .with("install_seconds", json!(install_seconds))
    .with("launcher_entry", json!(entry));
    if let Some(boot) = boot {
        done = done.with("android_start_seconds", json!(boot));
    }
    let warnings: Vec<_> = package
        .manifest
        .required_features
        .iter()
        .map(|feature| {
            json!({
                "code": "TUR-ANDROID-DEVICE-001",
                "message": format!("The package requires {feature}, which this computer may not provide."),
            })
        })
        .collect();
    if !warnings.is_empty() {
        done = done.with("warnings", json!(warnings));
    }
    Ok(done)
}

/// Lists the Android applications in the launcher. Android need not be
/// running: the entries Waydroid wrote are read.
pub(crate) fn list(all: bool) -> Done {
    let record = read_record();
    let apps: Vec<_> = launcher_entries()
        .into_iter()
        .filter(|entry| all || !entry.hidden)
        .map(|entry| {
            let installed = record.get(&entry.id);
            json!({
                "id": entry.id,
                "name": entry.name,
                "launcher_entry": entry.file,
                "icon": entry.icon,
                "hidden": entry.hidden,
                "installed_by_you": installed.is_some(),
                "version_name": installed.and_then(|i| i.version_name.clone()),
            })
        })
        .collect();
    let on = enabled();
    let message = if apps.is_empty() {
        if on {
            "No Android applications are installed.".to_owned()
        } else {
            "Android is switched off and no Android applications are installed.".to_owned()
        }
    } else {
        let names: Vec<String> = apps
            .iter()
            .map(|app| format!("{} ({})", app["name"].as_str().unwrap_or_default(), app["id"].as_str().unwrap_or_default()))
            .collect();
        format!("{} Android application(s): {}", apps.len(), names.join(", "))
    };
    Done::new(message)
        .with("enabled", json!(on))
        .with("apps", json!(apps))
}

fn find_running(id: &str, limit: Duration) -> Result<(App, Option<f64>), Failure> {
    require_enabled()?;
    let (apps, boot) = ensure_running(limit)?;
    match apps.into_iter().find(|app| app.id == id) {
        Some(app) => Ok((app, boot)),
        None => Err(Failure::refused(
            Code::AppUnknown,
            format!("No Android application {id} is installed."),
        )),
    }
}

/// Starts the application `id` as a window on the desktop.
pub(crate) fn launch(id: &str, limit: Duration) -> Result<Done, Failure> {
    let (app, boot) = find_running(id, limit)?;
    let launch = run::run(WAYDROID, &["app", "launch", id], Duration::from_secs(60));
    if !launch.ok() || launch.stderr.contains("Failed") {
        return Err(Failure::failed(
            Code::LaunchFailed,
            format!("Android did not start {id}: waydroid app launch {}", launch.account()),
        ));
    }
    let mut done = Done::new(format!("{} ({id}) was started.", app.name))
        .with("id", json!(id))
        .with("name", json!(app.name));
    if let Some(boot) = boot {
        done = done.with("android_start_seconds", json!(boot));
    }
    Ok(done)
}

/// Removes the application `id` and its data.
pub(crate) fn remove(id: &str, limit: Duration) -> Result<Done, Failure> {
    let (app, _) = find_running(id, limit)?;
    let remove = run::run(WAYDROID, &["app", "remove", id], Duration::from_secs(120));
    let gone = poll(Duration::from_secs(60), || {
        apps_now().filter(|apps| apps.iter().all(|a| a.id != id))
    });
    if gone.is_none() {
        return Err(Failure::failed(
            Code::RemoveFailed,
            format!("Android did not remove {id}: waydroid app remove {}", remove.account()),
        ));
    }
    let mut record = read_record();
    record.remove(id);
    write_record(&record);
    let entry_gone = poll(Duration::from_secs(20), || {
        launcher_entry(id).is_none().then_some(())
    })
    .is_some();
    Ok(Done::new(format!("{} ({id}) and its data were removed.", app.name))
        .with("id", json!(id))
        .with("launcher_entry_removed", json!(entry_gone)))
}

/// The person's Android session, for `status`.
pub(crate) fn session_facts() -> serde_json::Value {
    if host::effective_uid() == Some(0) {
        return json!(null);
    }
    json!({
        "unit": SESSION_UNIT,
        "state": user_unit_state(SESSION_UNIT),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_list_is_read() {
        let text = "Name: Settings\npackageName: com.android.settings\ncategories:\n\tandroid.intent.category.LAUNCHER\nName: Editor\npackageName: org.billthefarmer.editor\ncategories:\n";
        assert_eq!(
            parse_app_list(text),
            [
                App {
                    name: "Settings".to_owned(),
                    id: "com.android.settings".to_owned()
                },
                App {
                    name: "Editor".to_owned(),
                    id: "org.billthefarmer.editor".to_owned()
                },
            ]
        );
        assert!(parse_app_list("[14:03:01] WayDroid session is stopped\n").is_empty());
    }

    #[test]
    fn a_launcher_entry_is_read() {
        let text = "[Desktop Entry]\nType=Application\nName=Editor\nExec=waydroid app launch org.billthefarmer.editor\nIcon=/nonexistent/icon.png\nCategories=X-WayDroid-App;\n\n[Desktop Action app_settings]\nName=App Settings\n";
        let entry = parse_entry("org.billthefarmer.editor", Path::new("/x.desktop"), text);
        assert_eq!(entry.name, "Editor");
        assert_eq!(entry.icon, None, "an icon that does not exist is not named");
        assert!(!entry.hidden);
        let hidden = parse_entry("com.android.contacts", Path::new("/y"), "[Desktop Entry]\nName=Contacts\nNoDisplay=true\n");
        assert!(hidden.hidden);
    }
}
