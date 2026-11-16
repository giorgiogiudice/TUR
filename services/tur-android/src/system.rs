// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Switching Android on and off: the parts that need administrator rights.
//!
//! Switching on (android-runtime.md, section 6.4): fetch the pinned images,
//! check each against its digest, unpack them under Waydroid's state
//! directory, point Waydroid's pre-installed image directory at them, let
//! the container service start (its drop-in in the image refuses to start
//! without TUR's flag), initialise Waydroid and set its properties.
//!
//! Switching off: stop every Android session and the container, stop the
//! service and unmount binderfs, remove the container's bridge if it is
//! left, take the flag away so nothing can start the service again, and then
//! look: no process, mount or network interface of Android may remain.

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::codes::Code;
use crate::host::{
    self, BINDERFS, BINDERFS_UNIT, BRIDGE, ENABLED_FLAG, IMAGES_ROOT, PREINSTALLED, SERVICE,
    STATE_FILE, TUR_STATE, WAYDROID, WAYDROID_CFG, WAYDROID_STATE,
};
use crate::outcome::{Done, Failure};
use crate::pins::{self, ImagePin, Pins};
use crate::run::{self, seconds};

/// Unpacks one member of a zip archive to a file; Python's `zipfile`
/// checks the member's CRC as it reads.
const UNPACK: &str = "\
import shutil, sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as archive:
    with archive.open(sys.argv[2]) as member, open(sys.argv[3], 'wb') as out:
        shutil.copyfileobj(member, out, 1024 * 1024)
";

/// How Android draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Graphics {
    /// Decide from the graphics device of this computer.
    Auto,
    /// Use the graphics device.
    Hardware,
    /// Draw in software.
    Software,
}

/// What switching on was asked to do.
#[derive(Debug, Clone)]
pub(crate) struct EnableOptions {
    /// How Android draws.
    pub(crate) graphics: Graphics,
    /// A mirror to fetch the pinned files from instead of their publisher:
    /// the same file names under this address. The digests are checked all
    /// the same.
    pub(crate) mirror: Option<String>,
    /// Where the pins are.
    pub(crate) pins: PathBuf,
}

fn step(message: impl Into<String>) -> Failure {
    Failure::failed(Code::StepFailed, message)
}

fn io_step(what: &str, error: &io::Error) -> Failure {
    step(format!("{what}: {error}"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn systemctl(arguments: &[&str]) -> run::Output {
    run::run("systemctl", arguments, Duration::from_secs(120))
}

fn journal(unit: &str) -> String {
    run::run(
        "journalctl",
        &["--no-pager", "-o", "cat", "-n", "30", "-u", unit],
        Duration::from_secs(20),
    )
    .text()
}

/// Gives Waydroid's state directory the labels of Fedora's policy for it
/// (`waydroid_data_t`). The policy labels a directory Waydroid itself, or a
/// person's own shell (`unconfined_t`), creates there; one created by
/// another process, such as a service, gets `var_lib_t`, and Waydroid's
/// daemon may then not write its log (measured: android-1, run 43748613587).
fn relabel() {
    if Path::new("/usr/sbin/restorecon").exists() {
        let _ = run::run(
            "/usr/sbin/restorecon",
            &["-R", WAYDROID_STATE],
            Duration::from_secs(300),
        );
    }
}

/// Loads the pins, as a failure with its code if they cannot be read.
pub(crate) fn load_pins(path: &Path) -> Result<Pins, Failure> {
    Pins::load(path).map_err(|reason| Failure::refused(Code::ImagePins, reason))
}

/// The checks made before anything is changed: Waydroid is in the image,
/// the kernel has binder and the pins can be read.
pub(crate) fn preflight(pins_path: &Path) -> Result<(String, Pins), Failure> {
    let Some(version) = host::waydroid_version() else {
        return Err(Failure::refused(
            Code::WaydroidMissing,
            "Waydroid, the Android runtime, is not installed in this system image, so Android \
             cannot be switched on here.",
        ));
    };
    if !host::binder_filesystem() {
        return Err(Failure::refused(
            Code::KernelNoBinder,
            "The kernel offers no binder filesystem, which Android needs. TUR does not add \
             kernel modules; Android cannot run with this kernel.",
        ));
    }
    let pins = load_pins(pins_path)?;
    Ok((version, pins))
}

/// The images of a set are unpacked and match what was recorded when they
/// were unpacked.
fn image_ready(dir: &Path, pin: &ImagePin) -> bool {
    let image = dir.join(&pin.member);
    let recorded = dir.join(format!("{}.sha256", pin.member));
    let size_ok = fs::metadata(&image).is_ok_and(|m| m.len() == pin.unpacked_size);
    let Ok(expected) = fs::read_to_string(recorded) else {
        return false;
    };
    size_ok
        && tur_runtime::sha256::sha256_file(&image)
            .is_ok_and(|(digest, _)| digest == expected.trim())
}

/// Fetches, checks and unpacks one image; returns the bytes downloaded.
fn fetch_image(dir: &Path, pin: &ImagePin, mirror: Option<&str>) -> Result<(u64, String), Failure> {
    if image_ready(dir, pin) {
        let digest = fs::read_to_string(dir.join(format!("{}.sha256", pin.member)))
            .unwrap_or_default()
            .trim()
            .to_owned();
        return Ok((0, digest));
    }
    let archive = dir.join(&pin.file);
    let part = dir.join(format!("{}.part", pin.file));
    let url = match mirror {
        Some(base) => format!("{}/{}", base.trim_end_matches('/'), pin.file),
        None => pin.url.clone(),
    };
    let part_text = part.to_string_lossy().into_owned();
    let download = run::run(
        "curl",
        &[
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--retry",
            "3",
            "--retry-delay",
            "5",
            "--connect-timeout",
            "30",
            "--output",
            &part_text,
            &url,
        ],
        Duration::from_secs(3 * 60 * 60),
    );
    if !download.ok() {
        let _ = fs::remove_file(&part);
        return Err(Failure::failed(
            Code::ImageDownload,
            format!(
                "The {} image could not be downloaded from {url}: curl {}",
                pin.name,
                download.account()
            ),
        ));
    }
    let (digest, size) = tur_runtime::sha256::sha256_file(&part)
        .map_err(|error| io_step("the downloaded image cannot be read", &error))?;
    if digest != pin.sha256 || size != pin.size {
        let _ = fs::remove_file(&part);
        return Err(Failure::failed(
            Code::ImageMismatch,
            format!(
                "The {} image downloaded from {url} is not the pinned one: SHA-256 {digest}, \
                 {size} bytes; the pin is {}, {} bytes. It was deleted.",
                pin.name, pin.sha256, pin.size
            ),
        ));
    }
    fs::rename(&part, &archive).map_err(|error| io_step("the image cannot be kept", &error))?;
    let image = dir.join(&pin.member);
    let image_part = dir.join(format!("{}.part", pin.member));
    let archive_text = archive.to_string_lossy().into_owned();
    let image_part_text = image_part.to_string_lossy().into_owned();
    let unpack = run::run(
        "python3",
        &[
            "-I",
            "-c",
            UNPACK,
            &archive_text,
            &pin.member,
            &image_part_text,
        ],
        Duration::from_secs(60 * 60),
    );
    let unpacked = fs::metadata(&image_part).map(|m| m.len()).unwrap_or(0);
    if !unpack.ok() || unpacked != pin.unpacked_size {
        let _ = fs::remove_file(&image_part);
        return Err(Failure::failed(
            Code::ImageMismatch,
            format!(
                "The {} image could not be unpacked ({unpacked} of {} bytes): python3 {}",
                pin.name,
                pin.unpacked_size,
                unpack.account()
            ),
        ));
    }
    fs::rename(&image_part, &image).map_err(|error| io_step("the image cannot be kept", &error))?;
    let (unpacked_digest, _) = tur_runtime::sha256::sha256_file(&image)
        .map_err(|error| io_step("the unpacked image cannot be read", &error))?;
    fs::write(
        dir.join(format!("{}.sha256", pin.member)),
        format!("{unpacked_digest}\n"),
    )
    .map_err(|error| io_step("the image's digest cannot be recorded", &error))?;
    let _ = fs::remove_file(&archive);
    Ok((size, unpacked_digest))
}

/// Points Waydroid's pre-installed image directory at `set`.
fn link_images(set: &Path) -> Result<(), Failure> {
    let link = Path::new(PREINSTALLED);
    match fs::symlink_metadata(link) {
        Ok(meta) if meta.file_type().is_symlink() => fs::remove_file(link)
            .map_err(|error| io_step("the old image link cannot be replaced", &error))?,
        Ok(_) => {
            return Err(step(format!(
                "{PREINSTALLED} exists and is not TUR's link. Waydroid would use the images in \
                 it instead of the pinned ones; move it away first."
            )));
        }
        Err(_) => {}
    }
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| io_step("the image link cannot be made", &error))?;
    }
    symlink(set, link).map_err(|error| io_step("the image link cannot be made", &error))
}

/// `cfg` with its `[properties]` section holding `set` and none of `unset`.
pub(crate) fn with_properties(cfg: &str, set: &[(&str, &str)], unset: &[&str]) -> String {
    let key_of = |line: &str| line.split('=').next().unwrap_or_default().trim().to_owned();
    let drop = |line: &str| {
        let key = key_of(line);
        set.iter().any(|(name, _)| *name == key) || unset.contains(&key.as_str())
    };
    let mut out: Vec<String> = Vec::new();
    let mut in_properties = false;
    let mut found = false;
    for line in cfg.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_properties = trimmed == "[properties]";
            out.push(line.to_owned());
            if in_properties {
                found = true;
                for (name, value) in set {
                    out.push(format!("{name} = {value}"));
                }
            }
            continue;
        }
        if in_properties && drop(trimmed) {
            continue;
        }
        out.push(line.to_owned());
    }
    if !found {
        if out.last().is_some_and(|line| !line.trim().is_empty()) {
            out.push(String::new());
        }
        out.push("[properties]".to_owned());
        for (name, value) in set {
            out.push(format!("{name} = {value}"));
        }
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

const SOFTWARE_PROPERTIES: [(&str, &str); 2] = [
    ("ro.hardware.gralloc", "default"),
    ("ro.hardware.egl", "swiftshader"),
];
const MULTI_WINDOW: (&str, &str) = ("persist.waydroid.multi_windows", "true");

/// Switches Android on. Must run as root; the person's consent was asked
/// before.
pub(crate) fn enable(options: &EnableOptions) -> Result<Done, Failure> {
    let started = Instant::now();
    let (version, pins) = preflight(&options.pins)?;
    let set = Path::new(IMAGES_ROOT).join(&pins.id);
    let ready = pins.images.iter().all(|pin| image_ready(&set, pin));
    if !ready {
        fs::create_dir_all(&set)
            .map_err(|error| io_step("the image directory cannot be made", &error))?;
        if let Some(free) = host::free_bytes(IMAGES_ROOT)
            && free < pins.needed_bytes()
        {
            return Err(Failure::refused(
                Code::ImageSpace,
                format!(
                    "Switching Android on needs {} of free space on the system disk; {} is \
                     free.",
                    pins::human(pins.needed_bytes()),
                    pins::human(free)
                ),
            )
            .with("free_bytes", json!(free))
            .with("needed_bytes", json!(pins.needed_bytes())));
        }
    }

    // The images.
    let fetch_started = Instant::now();
    let mut downloaded = 0u64;
    let mut digests = serde_json::Map::new();
    for pin in &pins.images {
        let (bytes, digest) = fetch_image(&set, pin, options.mirror.as_deref())?;
        downloaded += bytes;
        digests.insert(pin.member.clone(), json!(digest));
    }
    let fetch_seconds = seconds(fetch_started.elapsed());
    relabel();
    link_images(&set)?;

    // Let the service start: its drop-in checks for this flag.
    fs::create_dir_all(TUR_STATE)
        .map_err(|error| io_step("TUR's state directory cannot be made", &error))?;
    fs::write(ENABLED_FLAG, format!("{}\n", pins.id))
        .map_err(|error| io_step("the switch cannot be recorded", &error))?;

    // The service mounts binderfs and links the binder devices into /dev
    // before Waydroid looks for them.
    let setup_started = Instant::now();
    let start = systemctl(&["start", SERVICE]);
    if !start.ok() {
        return Err(Failure::failed(
            Code::ContainerFailed,
            format!(
                "The Android container service did not start: {}. {}",
                start.account(),
                journal(SERVICE)
            ),
        ));
    }
    let binderfs_mounted = host::android_mounts()
        .iter()
        .any(|mount| mount.fstype == "binder");
    if !binderfs_mounted || !Path::new("/dev/binder").exists() {
        return Err(Failure::failed(
            Code::KernelBinderDevices,
            format!(
                "binderfs is {}mounted at {BINDERFS} and /dev/binder {}: {}",
                if binderfs_mounted { "" } else { "not " },
                if Path::new("/dev/binder").exists() {
                    "exists"
                } else {
                    "is missing"
                },
                journal(BINDERFS_UNIT)
            ),
        ));
    }

    let init = run::run(WAYDROID, &["init", "-f"], Duration::from_secs(600));
    if !init.ok() || !Path::new(WAYDROID_CFG).exists() {
        return Err(step(format!("waydroid init {}", init.account())));
    }
    let graphics = host::graphics();
    let software = match options.graphics {
        Graphics::Software => true,
        Graphics::Hardware => false,
        Graphics::Auto => graphics.needs_software(),
    };
    let mut set_properties = vec![MULTI_WINDOW];
    let unset: Vec<&str> = if software {
        set_properties.extend(SOFTWARE_PROPERTIES);
        Vec::new()
    } else {
        SOFTWARE_PROPERTIES.iter().map(|(name, _)| *name).collect()
    };
    let cfg = fs::read_to_string(WAYDROID_CFG)
        .map_err(|error| io_step("Waydroid's configuration cannot be read", &error))?;
    fs::write(WAYDROID_CFG, with_properties(&cfg, &set_properties, &unset))
        .map_err(|error| io_step("Waydroid's configuration cannot be written", &error))?;
    relabel();
    let upgrade = run::run(
        WAYDROID,
        &["upgrade", "--offline"],
        Duration::from_secs(300),
    );
    if !upgrade.ok() {
        return Err(step(format!(
            "waydroid upgrade --offline {}",
            upgrade.account()
        )));
    }
    let restart = systemctl(&["restart", SERVICE]);
    if !restart.ok() {
        return Err(Failure::failed(
            Code::ContainerFailed,
            format!(
                "The Android container service did not restart: {}",
                restart.account()
            ),
        ));
    }
    let setup_seconds = seconds(setup_started.elapsed());

    let disk_bytes: u64 = pins
        .images
        .iter()
        .filter_map(|pin| fs::metadata(set.join(&pin.member)).ok())
        .map(|m| m.len())
        .sum();
    let state = json!({
        "schema_version": 1,
        "enabled_at": now(),
        "images": pins.id,
        "image_digests": digests,
        "waydroid": version,
        "software_rendering": software,
        "downloaded_bytes": downloaded,
    });
    fs::write(
        STATE_FILE,
        serde_json::to_string_pretty(&state).unwrap_or_default(),
    )
    .map_err(|error| io_step("the state cannot be recorded", &error))?;

    let mut done = Done::new(format!(
        "Android {} is switched on. {} downloaded, {} on disk.",
        pins.android_version,
        pins::human(downloaded),
        pins::human(disk_bytes)
    ))
    .with("android_version", json!(pins.android_version))
    .with("images", json!(pins.id))
    .with("image_digests", Value::Object(digests))
    .with("waydroid", json!(version))
    .with("downloaded_bytes", json!(downloaded))
    .with("disk_bytes", json!(disk_bytes))
    .with("download_and_unpack_seconds", json!(fetch_seconds))
    .with("setup_seconds", json!(setup_seconds))
    .with("total_seconds", json!(seconds(started.elapsed())))
    .with("graphics_device", json!(graphics))
    .with("software_rendering", json!(software));
    if software {
        done = done.with(
            "warnings",
            json!([{
                "code": Code::SoftwareRendering,
                "message": "Android draws in software on this computer: fine for ordinary \
                            applications, slow for games."
            }]),
        );
    }
    Ok(done)
}

/// Sends `signal` to `pid`.
fn signal(pid: u32, signal: &str) {
    let _ = run::run(
        "kill",
        &[&format!("-{signal}"), &pid.to_string()],
        Duration::from_secs(10),
    );
}

/// Waits until `done` holds or `limit` passes.
fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    loop {
        if done() {
            return true;
        }
        if started.elapsed() >= limit {
            return false;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Links in /dev that point into binderfs.
fn binder_links() -> Vec<PathBuf> {
    let mut links: Vec<PathBuf> = fs::read_dir("/dev")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| fs::read_link(path).is_ok_and(|target| target.starts_with(BINDERFS)))
                .collect()
        })
        .unwrap_or_default();
    links.sort();
    links
}

/// What of Android is left: processes, mounts, interfaces, and the state of
/// the two units.
pub(crate) fn leftovers() -> serde_json::Map<String, Value> {
    let mut facts = serde_json::Map::new();
    facts.insert("processes".to_owned(), json!(host::android_processes()));
    facts.insert("mounts".to_owned(), json!(host::android_mounts()));
    facts.insert("interfaces".to_owned(), json!(host::android_interfaces()));
    facts.insert("service".to_owned(), json!(host::unit_state(SERVICE)));
    facts.insert(
        "binderfs_unit".to_owned(),
        json!(host::unit_state(BINDERFS_UNIT)),
    );
    let links: Vec<String> = binder_links()
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    facts.insert("binder_links".to_owned(), json!(links));
    facts
}

/// Switches Android off. Must run as root.
pub(crate) fn disable(remove_images: bool) -> Result<Done, Failure> {
    let started = Instant::now();
    let was_enabled = Path::new(ENABLED_FLAG).exists();
    let mut steps: Vec<String> = Vec::new();

    // Nothing may start the service from here on.
    match fs::remove_file(ENABLED_FLAG) {
        Ok(()) => steps.push("the switch was turned off".to_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_step("the switch cannot be turned off", &error)),
    }

    // Every person's Android session ends as it does when the person signs
    // out: on SIGTERM it stops the container.
    let sessions: Vec<u32> = host::android_processes()
        .iter()
        .filter(|p| !p.in_container && p.command.contains("session start"))
        .map(|p| p.pid)
        .collect();
    for pid in &sessions {
        signal(*pid, "TERM");
    }
    if !sessions.is_empty() {
        steps.push(format!(
            "{} Android session(s) asked to stop",
            sessions.len()
        ));
        let stopped = wait_for(Duration::from_secs(30), || {
            host::android_processes()
                .iter()
                .all(|p| !sessions.contains(&p.pid))
        });
        if !stopped {
            for pid in &sessions {
                signal(*pid, "KILL");
            }
            steps.push("a session did not stop and was ended".to_owned());
        }
    }

    // The container, then the service.
    if host::unit_state(SERVICE).active == "active" && Path::new(WAYDROID).exists() {
        let stop = run::run(WAYDROID, &["container", "stop"], Duration::from_secs(90));
        steps.push(format!(
            "waydroid container stop: {}",
            if stop.ok() {
                "done".to_owned()
            } else {
                stop.account()
            }
        ));
    }
    let stop = systemctl(&["stop", SERVICE]);
    if !stop.ok() {
        steps.push(format!("systemctl stop {SERVICE}: {}", stop.account()));
    }
    let container_gone = wait_for(Duration::from_secs(30), || {
        host::android_processes().iter().all(|p| !p.in_container)
    });
    if !container_gone {
        let _ = run::run(
            "lxc-stop",
            &["-P", "/var/lib/waydroid/lxc", "-n", "waydroid", "-k"],
            Duration::from_secs(60),
        );
        steps.push("the container did not stop and was killed".to_owned());
    }

    // binderfs and its links.
    for link in binder_links() {
        let _ = fs::remove_file(&link);
    }
    let _ = systemctl(&["stop", BINDERFS_UNIT]);
    if host::android_mounts().iter().any(|m| m.target == BINDERFS) {
        let _ = run::run("umount", &[BINDERFS], Duration::from_secs(30));
    }
    // Any image or overlay mount Waydroid left, innermost first.
    let mut mounts = host::android_mounts();
    mounts.reverse();
    for mount in mounts {
        let unmount = run::run("umount", &[&mount.target], Duration::from_secs(30));
        steps.push(format!(
            "unmounted {}: {}",
            mount.target,
            if unmount.ok() {
                "done".to_owned()
            } else {
                unmount.account()
            }
        ));
    }

    // The container's bridge.
    if Path::new("/sys/class/net").join(BRIDGE).exists() {
        let delete = run::run("ip", &["link", "delete", BRIDGE], Duration::from_secs(30));
        steps.push(format!(
            "removed the {BRIDGE} bridge: {}",
            if delete.ok() {
                "done".to_owned()
            } else {
                delete.account()
            }
        ));
    }

    let mut freed = 0u64;
    if remove_images {
        if let Ok(meta) = fs::symlink_metadata(PREINSTALLED)
            && meta.file_type().is_symlink()
        {
            let _ = fs::remove_file(PREINSTALLED);
        }
        freed = directory_size(Path::new(IMAGES_ROOT));
        if let Err(error) = fs::remove_dir_all(IMAGES_ROOT)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(io_step("the Android images cannot be removed", &error));
        }
        steps.push(format!(
            "the Android images were removed ({})",
            pins::human(freed)
        ));
    }

    let facts = leftovers();
    let processes = facts["processes"].as_array().map_or(0, Vec::len);
    let mounts = facts["mounts"].as_array().map_or(0, Vec::len);
    let interfaces = facts["interfaces"].as_array().map_or(0, Vec::len);
    let service_active = facts["service"]["active"] == "active";
    let mut result = if processes + mounts + interfaces == 0 && !service_active {
        Ok(Done::new(if was_enabled {
            "Android is switched off. Nothing of Android is running, mounted or connected."
        } else {
            "Android was already switched off. Nothing of Android is running, mounted or \
             connected."
        }))
    } else {
        Err(Failure::failed(
            Code::Leftover,
            format!(
                "Android was switched off, but {processes} process(es), {mounts} mount(s) and \
                 {interfaces} network interface(s) of Android remain{}. They go at the next \
                 restart; Android will not start again until it is switched on.",
                if service_active {
                    ", and its service is still running"
                } else {
                    ""
                }
            ),
        ))
    };
    let extra = [
        ("was_enabled", json!(was_enabled)),
        ("steps", json!(steps)),
        ("removed_images_bytes", json!(freed)),
        ("seconds", json!(seconds(started.elapsed()))),
    ];
    let all = extra
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .chain(facts);
    for (key, value) in all {
        result = match result {
            Ok(done) => Ok(done.with(&key, value)),
            Err(failure) => Err(failure.with(&key, value)),
        };
    }
    result
}

fn directory_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => directory_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_are_set_in_their_section() {
        let cfg =
            "[waydroid]\narch = x86_64\n\n[properties]\nro.hardware.egl=swiftshader\nother = 1\n";
        let out = with_properties(cfg, &[MULTI_WINDOW], &["ro.hardware.egl"]);
        assert_eq!(
            out,
            "[waydroid]\narch = x86_64\n\n[properties]\npersist.waydroid.multi_windows = true\nother = 1\n"
        );
    }

    #[test]
    fn a_missing_section_is_added() {
        let cfg = "[waydroid]\narch = x86_64\n";
        let out = with_properties(cfg, &SOFTWARE_PROPERTIES, &[]);
        assert_eq!(
            out,
            "[waydroid]\narch = x86_64\n\n[properties]\nro.hardware.gralloc = default\nro.hardware.egl = swiftshader\n"
        );
    }

    #[test]
    fn a_property_set_twice_is_kept_once() {
        let cfg = "[properties]\npersist.waydroid.multi_windows = false\n";
        let out = with_properties(cfg, &[MULTI_WINDOW], &[]);
        assert_eq!(out, "[properties]\npersist.waydroid.multi_windows = true\n");
    }
}
