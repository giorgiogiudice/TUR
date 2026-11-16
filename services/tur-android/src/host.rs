// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What this machine has and runs of Android: read from `/proc`, `/sys` and
//! systemd, never changed here.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::run;

/// Waydroid's program.
pub(crate) const WAYDROID: &str = "/usr/bin/waydroid";
/// Waydroid's version, in its own source.
const WAYDROID_CONFIG: &str = "/usr/lib/waydroid/tools/config/__init__.py";
/// The container service of the Waydroid package.
pub(crate) const SERVICE: &str = "waydroid-container.service";
/// The mount unit of binderfs of the Fedora package.
pub(crate) const BINDERFS_UNIT: &str = "dev-binderfs.mount";
/// Where binderfs is mounted.
pub(crate) const BINDERFS: &str = "/dev/binderfs";
/// Waydroid's state directory.
pub(crate) const WAYDROID_STATE: &str = "/var/lib/waydroid";
/// Waydroid's configuration, written by `waydroid init`.
pub(crate) const WAYDROID_CFG: &str = "/var/lib/waydroid/waydroid.cfg";
/// TUR's state directory for Android.
pub(crate) const TUR_STATE: &str = "/var/lib/tur/android";
/// The flag that lets the container service start (its drop-in
/// `ConditionPathExists=`). Present only while Android is switched on.
pub(crate) const ENABLED_FLAG: &str = "/var/lib/tur/android/enabled";
/// TUR's record of the switch-on.
pub(crate) const STATE_FILE: &str = "/var/lib/tur/android/state.json";
/// Where the images are kept, one directory per pinned set.
pub(crate) const IMAGES_ROOT: &str = "/var/lib/waydroid/tur-images";
/// The directory Waydroid takes pre-installed images from; TUR makes it a
/// link to the active set.
pub(crate) const PREINSTALLED: &str = "/etc/waydroid-extra/images";
/// The name of the container's bridge.
pub(crate) const BRIDGE: &str = "waydroid0";

/// The effective user ID of this process.
pub(crate) fn effective_uid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}

/// The kernel offers binderfs (the Rust or the C binder driver).
pub(crate) fn binder_filesystem() -> bool {
    fs::read_to_string("/proc/filesystems")
        .map(|text| {
            text.lines()
                .any(|line| line.split_whitespace().last() == Some("binder"))
        })
        .unwrap_or(false)
}

/// The version of Waydroid installed, if it is.
pub(crate) fn waydroid_version() -> Option<String> {
    if !Path::new(WAYDROID).exists() {
        return None;
    }
    let source = fs::read_to_string(WAYDROID_CONFIG).unwrap_or_default();
    let version = source
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = "))
        .map(|value| value.trim().trim_matches('"').to_owned());
    Some(version.unwrap_or_else(|| "unknown".to_owned()))
}

/// A systemd unit's state.
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct UnitState {
    /// `active`, `inactive`, `failed` and so on.
    pub(crate) active: String,
    /// `loaded`, `not-found`, `masked`.
    pub(crate) load: String,
}

/// The state of `unit` as systemd has it.
pub(crate) fn unit_state(unit: &str) -> UnitState {
    let output = run::run(
        "systemctl",
        &["show", "--property=ActiveState,LoadState", unit],
        Duration::from_secs(20),
    );
    let mut state = UnitState::default();
    for line in output.text().lines() {
        if let Some(value) = line.strip_prefix("ActiveState=") {
            state.active = value.to_owned();
        } else if let Some(value) = line.strip_prefix("LoadState=") {
            state.load = value.to_owned();
        }
    }
    if state.active.is_empty() {
        state.active = "unknown".to_owned();
    }
    state
}

/// A mount that belongs to Android.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Mount {
    /// Where it is mounted.
    pub(crate) target: String,
    /// The filesystem type.
    pub(crate) fstype: String,
}

/// Reads mounts of Android from the text of `/proc/self/mountinfo`.
pub(crate) fn android_mounts_in(mountinfo: &str) -> Vec<Mount> {
    let mut mounts = Vec::new();
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        let Some(separator) = fields.iter().position(|field| *field == "-") else {
            continue;
        };
        let (Some(target), Some(fstype)) = (fields.get(4), fields.get(separator + 1)) else {
            continue;
        };
        let target = target.replace("\\040", " ");
        if *fstype == "binder"
            || target.starts_with(WAYDROID_STATE)
            || target.starts_with(BINDERFS)
            || target.contains("waydroid")
        {
            mounts.push(Mount {
                target,
                fstype: (*fstype).to_owned(),
            });
        }
    }
    mounts
}

/// The mounts of Android in this mount namespace.
pub(crate) fn android_mounts() -> Vec<Mount> {
    android_mounts_in(&fs::read_to_string("/proc/self/mountinfo").unwrap_or_default())
}

/// Network interfaces of Android: the bridge and anything named for
/// Waydroid.
pub(crate) fn android_interfaces() -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir("/sys/class/net")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.contains("waydroid"))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

/// A process of Android.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Process {
    /// Its process ID.
    pub(crate) pid: u32,
    /// Its user ID.
    pub(crate) uid: Option<u32>,
    /// Its command line, shortened.
    pub(crate) command: String,
    /// Whether it runs inside the Android container.
    pub(crate) in_container: bool,
}

/// Whether a command line and control group belong to Android.
pub(crate) fn is_android_process(command: &str, cgroup: &str) -> (bool, bool) {
    let in_container = cgroup.contains("lxc.payload.waydroid");
    let words: Vec<&str> = command.split(' ').collect();
    let names_waydroid = words.iter().any(|word| {
        word.ends_with("/waydroid") || *word == "waydroid" || word.ends_with("waydroid.py")
    });
    let lxc = words
        .first()
        .is_some_and(|word| word.ends_with("lxc-start") || word.ends_with("lxc-monitord"))
        && command.contains("waydroid");
    let dnsmasq =
        words.first().is_some_and(|word| word.ends_with("dnsmasq")) && command.contains("waydroid");
    (
        in_container || names_waydroid || lxc || dnsmasq,
        in_container,
    )
}

/// Every process of Android: Waydroid's own, LXC's, the container's
/// network service and every process inside the container.
pub(crate) fn android_processes() -> Vec<Process> {
    let me = std::process::id();
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return found;
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if pid == me {
            continue;
        }
        let dir = entry.path();
        let raw = fs::read(dir.join("cmdline")).unwrap_or_default();
        let command: String = String::from_utf8_lossy(&raw)
            .split('\0')
            .filter(|part| !part.is_empty())
            .collect::<Vec<&str>>()
            .join(" ");
        if command.contains("tur-android") {
            continue;
        }
        let cgroup = fs::read_to_string(dir.join("cgroup")).unwrap_or_default();
        let (android, in_container) = is_android_process(&command, &cgroup);
        if !android {
            continue;
        }
        let uid = fs::read_to_string(dir.join("status"))
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find(|line| line.starts_with("Uid:"))
                    .and_then(|line| line.split_whitespace().nth(1)?.parse().ok())
            });
        let mut short: String = command.chars().take(160).collect();
        if short.is_empty() {
            short = format!("[pid {pid}]");
        }
        found.push(Process {
            pid,
            uid,
            command: short,
            in_container,
        });
    }
    found.sort_by_key(|process| process.pid);
    found
}

/// The memory the Android container's control group holds now, in bytes.
pub(crate) fn container_memory() -> Option<u64> {
    for process in android_processes().iter().filter(|p| p.in_container) {
        let Ok(cgroup) = fs::read_to_string(format!("/proc/{}/cgroup", process.pid)) else {
            continue;
        };
        let Some(path) = cgroup.lines().find_map(|line| line.strip_prefix("0::")) else {
            continue;
        };
        // The payload's own group, not a group below it.
        let top = match path.find("lxc.payload.waydroid") {
            Some(at) => {
                let end = path[at..].find('/').map_or(path.len(), |slash| at + slash);
                &path[..end]
            }
            None => continue,
        };
        let file = PathBuf::from(format!("/sys/fs/cgroup{top}/memory.current"));
        if let Ok(text) = fs::read_to_string(file) {
            return text.trim().parse().ok();
        }
    }
    None
}

/// Free bytes on the filesystem that holds `path`.
pub(crate) fn free_bytes(path: &str) -> Option<u64> {
    let output = run::run(
        "df",
        &["--output=avail", "-B1", path],
        Duration::from_secs(20),
    );
    output.text().lines().nth(1)?.trim().parse().ok()
}

/// The graphics device Android would use: the first render node and its
/// kernel driver.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Graphics {
    /// The render node, such as `/dev/dri/renderD128`.
    pub(crate) render_node: Option<String>,
    /// Its kernel driver, such as `i915` or `virtio_gpu`.
    pub(crate) driver: Option<String>,
}

impl Graphics {
    /// Whether Android should draw in software here: no render node, a
    /// driver Waydroid does not support, or a virtual display adapter.
    pub(crate) fn needs_software(&self) -> bool {
        match self.driver.as_deref() {
            None => true,
            Some(driver) => matches!(
                driver,
                "nvidia" | "virtio_gpu" | "virtio-gpu" | "vmwgfx" | "qxl" | "bochs" | "simpledrm"
            ),
        }
    }
}

/// The first render node and its driver.
pub(crate) fn graphics() -> Graphics {
    let mut nodes: Vec<String> = fs::read_dir("/dev/dri")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("renderD"))
                .collect()
        })
        .unwrap_or_default();
    nodes.sort();
    let Some(node) = nodes.first() else {
        return Graphics {
            render_node: None,
            driver: None,
        };
    };
    let driver = fs::read_link(format!("/sys/class/drm/{node}/device/driver"))
        .ok()
        .and_then(|target| {
            target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
    Graphics {
        render_node: Some(format!("/dev/dri/{node}")),
        driver,
    }
}

/// SELinux is enforcing.
pub(crate) fn selinux_enforcing() -> Option<bool> {
    fs::read_to_string("/sys/fs/selinux/enforce")
        .ok()
        .map(|text| text.trim() == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_of_android_are_found_and_others_are_not() {
        let mountinfo = "\
22 1 252:3 / / rw,relatime shared:1 - ext4 /dev/vda3 rw
40 22 0:44 / /dev/binderfs rw,relatime shared:20 - binder binder rw,max=1048576
41 22 7:0 / /var/lib/waydroid/rootfs ro,relatime shared:21 - ext4 /dev/loop0 ro
42 22 0:45 / /home/a\\040b rw - tmpfs tmpfs rw
";
        let mounts = android_mounts_in(mountinfo);
        assert_eq!(
            mounts,
            [
                Mount {
                    target: "/dev/binderfs".to_owned(),
                    fstype: "binder".to_owned()
                },
                Mount {
                    target: "/var/lib/waydroid/rootfs".to_owned(),
                    fstype: "ext4".to_owned()
                },
            ]
        );
    }

    #[test]
    fn processes_of_android_are_recognised() {
        let session = is_android_process(
            "/usr/bin/python3 /usr/bin/waydroid session start",
            "0::/user.slice",
        );
        assert_eq!(session, (true, false));
        let init = is_android_process("/init second_stage", "0::/lxc.payload.waydroid2/init.scope");
        assert_eq!(init, (true, true));
        let lxc = is_android_process("lxc-start -P /var/lib/waydroid/lxc -F -n waydroid", "0::/");
        assert!(lxc.0);
        let dnsmasq = is_android_process(
            "dnsmasq --pid-file=/run/waydroid-lxc/dnsmasq.pid --interface=waydroid0",
            "0::/system.slice",
        );
        assert!(dnsmasq.0);
        let other = is_android_process("/usr/bin/plasmashell", "0::/user.slice");
        assert_eq!(other, (false, false));
        let unrelated_dnsmasq = is_android_process("dnsmasq --conf-file=/etc/x", "0::/");
        assert!(!unrelated_dnsmasq.0);
    }

    #[test]
    fn virtual_and_missing_graphics_need_software_rendering() {
        let virtual_gpu = Graphics {
            render_node: Some("/dev/dri/renderD128".to_owned()),
            driver: Some("virtio_gpu".to_owned()),
        };
        assert!(virtual_gpu.needs_software());
        let none = Graphics {
            render_node: None,
            driver: None,
        };
        assert!(none.needs_software());
        let intel = Graphics {
            render_node: Some("/dev/dri/renderD128".to_owned()),
            driver: Some("i915".to_owned()),
        };
        assert!(!intel.needs_software());
    }

    #[test]
    fn this_process_is_not_root_or_has_a_uid() {
        assert!(effective_uid().is_some());
    }
}
