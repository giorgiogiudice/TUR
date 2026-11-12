// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! [`Host`] on a real TUR system.
//!
//! Other software is reached through command lines only: `grub2-editenv`,
//! `bootc`, `ostree`, `systemctl`, `loginctl`, `journalctl`, `mount`. Each
//! call has a time limit and none goes through a shell. Not covered by the
//! unit tests: the boot test and the rollback scenarios exercise it in a
//! virtual machine.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::host::{Entry, Host, Layout, Session};
use crate::json::{self, Value};
use crate::model::{self, BootcStatus, Deployment, EnvBlock};

const GRUBENV: &str = "/boot/grub2/grubenv";
const STATE_DIR: &str = "/var/lib/tur/boot-health";
const RUN_DIR: &str = "/run/tur";
const JOURNAL_SOCKET: &str = "/run/systemd/journal/socket";
/// The most output kept from one command.
const OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;

/// What a command left behind.
struct Output {
    /// The exit status; `None` if a signal ended it.
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Output {
    fn ok(&self) -> bool {
        self.status == Some(0)
    }

    fn describe(&self, what: &str) -> String {
        let status = match self.status {
            Some(code) => format!("exit status {code}"),
            None => "killed by a signal".to_owned(),
        };
        let stderr = self.stderr.trim();
        let tail: String = stderr
            .chars()
            .rev()
            .take(400)
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        format!("{what}: {status}: {tail}")
    }
}

/// Runs a program with a time limit and without a shell.
fn run(program: &str, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let read_out = thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(stream) = stdout {
            let _ = stream.take(OUTPUT_LIMIT).read_to_end(&mut buffer);
        }
        buffer
    });
    let read_err = thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(stream) = stderr {
            let _ = stream.take(OUTPUT_LIMIT).read_to_end(&mut buffer);
        }
        buffer
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // The readers are left behind: something the program
                    // started may still hold the pipes open.
                    return Err(format!(
                        "{program}: no result within {} s",
                        timeout.as_secs()
                    ));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(format!("{program}: {error}")),
        }
    };
    let stdout = read_out.join().unwrap_or_default();
    let stderr = read_err.join().unwrap_or_default();
    Ok(Output {
        status: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

fn seconds(count: u64) -> Duration {
    Duration::from_secs(count)
}

fn read_trimmed(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
}

/// `KEY=value` lines as a map.
fn properties(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.to_owned(), value.to_owned());
        }
    }
    map
}

/// The last mount whose mount point is `target` or contains it, ignoring
/// automount triggers: (mount point, filesystem type, source).
fn mount_of(mountinfo: &str, target: &str) -> Option<(String, String, String)> {
    let mut best: Option<(String, String, String)> = None;
    for line in mountinfo.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let Some(point) = left.split(' ').nth(4) else {
            continue;
        };
        let mut right = right.split(' ');
        let (Some(fstype), Some(source)) = (right.next(), right.next()) else {
            continue;
        };
        if fstype == "autofs" {
            continue;
        }
        let contains = point == "/"
            || target == point
            || target
                .strip_prefix(point)
                .is_some_and(|rest| rest.starts_with('/'));
        if !contains {
            continue;
        }
        let longer = match &best {
            Some((current, _, _)) => point.len() >= current.len(),
            None => true,
        };
        if longer {
            best = Some((point.to_owned(), fstype.to_owned(), source.to_owned()));
        }
    }
    best
}

/// The machine this program runs on.
pub(crate) struct SystemHost {
    /// Whether `/boot` has been made writable in this process's namespace.
    boot_writable: bool,
}

impl SystemHost {
    pub(crate) fn new() -> SystemHost {
        SystemHost {
            boot_writable: false,
        }
    }

    /// Makes `/boot` writable for this process. `/boot` is mounted read-only;
    /// the units that write the block run in a private mount namespace
    /// (`PrivateMounts=yes`), where remounting it affects nobody else. In
    /// the namespace everybody shares, this refuses.
    fn make_boot_writable(&mut self) -> Result<(), String> {
        if self.boot_writable {
            return Ok(());
        }
        // Reading the block first makes the automount behind /boot mount it.
        let _ = fs::metadata(GRUBENV);
        let own = fs::read_link("/proc/self/ns/mnt").ok();
        let shared = fs::read_link("/proc/1/ns/mnt").ok();
        if own.is_none() || own == shared {
            return Err("refusing to remount /boot outside a private mount namespace".to_owned());
        }
        let output = run("mount", &["-o", "remount,rw", "/boot"], seconds(20))?;
        if !output.ok() {
            return Err(output.describe("mount -o remount,rw /boot"));
        }
        self.boot_writable = true;
        Ok(())
    }

    fn editenv(&mut self, action: &str, arguments: &[String]) -> Result<(), String> {
        self.make_boot_writable()?;
        let mut args: Vec<&str> = vec![GRUBENV, action];
        args.extend(arguments.iter().map(String::as_str));
        let output = run("grub2-editenv", &args, seconds(20))?;
        if !output.ok() {
            return Err(output.describe(&format!("grub2-editenv {action}")));
        }
        // The block must be on disk before anything depends on it.
        if let Ok(file) = fs::File::open(GRUBENV) {
            let _ = file.sync_all();
        }
        if let Ok(directory) = fs::File::open("/boot/grub2") {
            let _ = directory.sync_all();
        }
        Ok(())
    }

    fn state_path(name: &str) -> PathBuf {
        Path::new(STATE_DIR).join(name)
    }

    fn release_value(text: &str, key: &str) -> Option<String> {
        text.lines().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix('='))
                .map(|value| value.trim().trim_matches('"').to_owned())
        })
    }

    fn deployment_build_id(deployment: &Deployment) -> Option<String> {
        let path = format!(
            "/sysroot/ostree/deploy/{}/deploy/{}/usr/lib/tur/release",
            deployment.stateroot,
            deployment.name()
        );
        let text = fs::read_to_string(path).ok()?;
        SystemHost::release_value(&text, "TUR_BUILD_ID")
    }
}

fn write_durably(path: &Path, content: &str) -> Result<(), String> {
    let fail = |error: std::io::Error| format!("{}: {error}", path.display());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(fail)?;
    }
    let temporary = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temporary).map_err(fail)?;
        file.write_all(content.as_bytes()).map_err(fail)?;
        file.sync_all().map_err(fail)?;
    }
    fs::rename(&temporary, path).map_err(fail)?;
    if let Some(parent) = path.parent()
        && let Ok(directory) = fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}

impl Host for SystemHost {
    fn cmdline(&self) -> String {
        read_trimmed("/proc/cmdline").unwrap_or_default()
    }

    fn boot_id(&self) -> String {
        read_trimmed("/proc/sys/kernel/random/boot_id")
            .unwrap_or_default()
            .replace('-', "")
    }

    fn build_id(&self) -> String {
        fs::read_to_string("/usr/lib/tur/release")
            .ok()
            .and_then(|text| SystemHost::release_value(&text, "TUR_BUILD_ID"))
            .unwrap_or_default()
    }

    fn now_unix(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or(0)
    }

    fn uptime_ms(&self) -> u64 {
        read_trimmed("/proc/uptime")
            .and_then(|text| {
                text.split_whitespace()
                    .next()
                    .and_then(|first| first.parse::<f64>().ok())
            })
            .map(|uptime| (uptime * 1000.0) as u64)
            .unwrap_or(0)
    }

    fn sleep_ms(&mut self, milliseconds: u64) {
        thread::sleep(Duration::from_millis(milliseconds));
    }

    fn env_list(&mut self) -> Result<EnvBlock, String> {
        let output = run("grub2-editenv", &[GRUBENV, "list"], seconds(20))?;
        if !output.ok() {
            return Err(output.describe("grub2-editenv list"));
        }
        Ok(model::parse_env_list(&output.stdout))
    }

    fn env_set(&mut self, variables: &[(&str, &str)]) -> Result<(), String> {
        let arguments: Vec<String> = variables
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        self.editenv("set", &arguments)
    }

    fn env_unset(&mut self, names: &[&str]) -> Result<(), String> {
        let arguments: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        self.editenv("unset", &arguments)
    }

    fn env_create(&mut self) -> Result<(), String> {
        self.editenv("create", &[])
    }

    fn bootc_status(&mut self) -> Result<BootcStatus, String> {
        let output = run("bootc", &["status", "--format=json"], seconds(60))?;
        if !output.ok() {
            return Err(output.describe("bootc status"));
        }
        let mut status = BootcStatus::parse(&output.stdout)?;
        for deployment in [&mut status.booted, &mut status.staged, &mut status.rollback]
            .into_iter()
            .flatten()
        {
            deployment.build_id = SystemHost::deployment_build_id(deployment);
        }
        Ok(status)
    }

    fn booted_is_default(&mut self) -> Result<bool, String> {
        let output = run("ostree", &["admin", "status", "--is-default"], seconds(30))?;
        match output.stdout.trim() {
            "default" => Ok(true),
            "not-default" => Ok(false),
            _ => Err(output.describe("ostree admin status --is-default")),
        }
    }

    fn rollback(&mut self) -> Result<(), String> {
        let output = run("bootc", &["rollback"], seconds(180))?;
        if output.ok() {
            Ok(())
        } else {
            Err(output.describe("bootc rollback"))
        }
    }

    fn lock_finalization(&mut self) -> Result<(), String> {
        let output = run("ostree", &["admin", "lock-finalization"], seconds(30))?;
        if output.ok() {
            Ok(())
        } else {
            Err(output.describe("ostree admin lock-finalization"))
        }
    }

    fn layout(&mut self) -> Result<Layout, String> {
        // Looking at the directory makes the automount behind /boot mount it.
        let _ = fs::metadata("/boot/grub2");
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")
            .map_err(|error| format!("/proc/self/mountinfo: {error}"))?;
        let Some((_, fstype, source)) = mount_of(&mountinfo, "/boot/grub2") else {
            return Err("no mount found for /boot/grub2".to_owned());
        };
        let device = source.rsplit('/').next().unwrap_or("").to_owned();
        let plain_partition = source.starts_with("/dev/")
            && !device.is_empty()
            && !device.starts_with("dm-")
            && !device.starts_with("md")
            && !source.starts_with("/dev/mapper/")
            && Path::new(&format!("/sys/class/block/{device}/partition")).exists();
        Ok(Layout {
            fstype,
            source,
            plain_partition,
        })
    }

    fn read_state(&self, name: &str) -> Option<String> {
        fs::read_to_string(SystemHost::state_path(name)).ok()
    }

    fn write_state(&mut self, name: &str, content: &str) -> Result<(), String> {
        write_durably(&SystemHost::state_path(name), content)
    }

    fn remove_state(&mut self, name: &str) -> Result<(), String> {
        let path = SystemHost::state_path(name);
        match fs::remove_file(&path) {
            Ok(()) => {
                if let Some(parent) = path.parent()
                    && let Ok(directory) = fs::File::open(parent)
                {
                    let _ = directory.sync_all();
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    fn remove_state_tree(&mut self, name: &str) -> Result<(), String> {
        let path = SystemHost::state_path(name);
        match fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    fn read_run(&self, name: &str) -> Option<String> {
        fs::read_to_string(Path::new(RUN_DIR).join(name)).ok()
    }

    fn write_run(&mut self, name: &str, content: &str) -> Result<(), String> {
        let path = Path::new(RUN_DIR).join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        fs::write(&path, content).map_err(|error| format!("{}: {error}", path.display()))
    }

    fn journal(&mut self, entry: &Entry) {
        let mut fields: Vec<(String, String)> = vec![
            ("MESSAGE".to_owned(), entry.message.clone()),
            ("MESSAGE_ID".to_owned(), entry.message_id.to_owned()),
            ("PRIORITY".to_owned(), entry.priority.to_string()),
            ("SYSLOG_IDENTIFIER".to_owned(), "tur-boot-health".to_owned()),
        ];
        fields.extend(entry.fields.iter().cloned());
        let mut datagram: Vec<u8> = Vec::new();
        for (name, value) in &fields {
            datagram.extend_from_slice(name.as_bytes());
            if value.contains('\n') {
                datagram.push(b'\n');
                datagram.extend_from_slice(&(value.len() as u64).to_le_bytes());
                datagram.extend_from_slice(value.as_bytes());
            } else {
                datagram.push(b'=');
                datagram.extend_from_slice(value.as_bytes());
            }
            datagram.push(b'\n');
        }
        let sent = UnixDatagram::unbound()
            .and_then(|socket| socket.send_to(&datagram, JOURNAL_SOCKET))
            .is_ok();
        if !sent {
            // No journal: say it where the unit's output goes.
            eprintln!("tur-boot-health: {}", entry.message);
        }
    }

    fn sync_journal(&mut self) {
        let _ = run("journalctl", &["--sync"], seconds(30));
    }

    fn console(&mut self, line: &str) {
        println!("tur-boot-health: {line}");
    }

    fn reboot(&mut self) -> Result<(), String> {
        let output = run("systemctl", &["--no-block", "reboot"], seconds(30))?;
        if output.ok() {
            Ok(())
        } else {
            Err(output.describe("systemctl reboot"))
        }
    }

    fn unit_show(
        &mut self,
        unit: &str,
        names: &[&str],
    ) -> Result<BTreeMap<String, String>, String> {
        let mut args: Vec<String> =
            vec!["show".to_owned(), "--no-pager".to_owned(), unit.to_owned()];
        for name in names {
            args.push("-p".to_owned());
            args.push((*name).to_owned());
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = run("systemctl", &args, seconds(20))?;
        if !output.ok() {
            return Err(output.describe(&format!("systemctl show {unit}")));
        }
        Ok(properties(&output.stdout))
    }

    fn seat_session(&mut self) -> Result<Option<Session>, String> {
        let seat = run(
            "loginctl",
            &["show-seat", "seat0", "-p", "ActiveSession", "--value"],
            seconds(20),
        )?;
        let id = seat.stdout.trim().to_owned();
        if !seat.ok() || id.is_empty() {
            return Ok(None);
        }
        let output = run(
            "loginctl",
            &[
                "show-session",
                &id,
                "-p",
                "Type",
                "-p",
                "Class",
                "-p",
                "State",
                "-p",
                "User",
            ],
            seconds(20),
        )?;
        if !output.ok() {
            return Ok(None);
        }
        let found = properties(&output.stdout);
        let text = |key: &str| found.get(key).cloned().unwrap_or_default();
        let Ok(uid) = text("User").parse::<u32>() else {
            return Ok(None);
        };
        Ok(Some(Session {
            id,
            kind: text("Type"),
            class: text("Class"),
            state: text("State"),
            uid,
        }))
    }

    fn compositor_pid(&mut self, uid: u32) -> Option<u32> {
        let entries = fs::read_dir("/proc").ok()?;
        let mut found: Vec<u32> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if metadata.uid() != uid {
                continue;
            }
            let comm = fs::read_to_string(path.join("comm")).unwrap_or_default();
            if comm.trim() == "kwin_wayland" || comm.trim() == "kwin_x11" {
                found.push(pid);
            }
        }
        found.sort_unstable();
        found.first().copied()
    }

    fn root_mount(&mut self) -> Option<(String, String)> {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo").ok()?;
        mount_of(&mountinfo, "/").map(|(_, fstype, source)| (fstype, source))
    }

    fn ostree_target(&mut self, ostree_argument: &str) -> Option<String> {
        let link = fs::read_link(format!("/sysroot{ostree_argument}")).ok()?;
        link.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    }

    fn path_readable(&mut self, path: &str) -> bool {
        let Ok(mut file) = fs::File::open(path) else {
            return false;
        };
        let mut byte = [0u8; 1];
        file.read(&mut byte).is_ok()
    }

    fn dir_writable(&mut self, path: &str) -> bool {
        let probe = Path::new(path).join(".tur-boot-health-probe");
        let written = fs::write(&probe, b"probe").is_ok();
        let _ = fs::remove_file(&probe);
        written
    }

    fn list_dir(&mut self, path: &str) -> Vec<String> {
        let mut names: Vec<String> = match fs::read_dir(path) {
            Ok(entries) => entries
                .flatten()
                .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        names
    }

    fn read_file(&mut self, path: &str) -> Option<String> {
        fs::read_to_string(path).ok()
    }

    fn run_check(&mut self, exec: &str, args: &[String], timeout_s: u64) -> Result<i32, String> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = run(exec, &args, seconds(timeout_s))?;
        match output.status {
            Some(code) => Ok(code),
            None => Err(format!("{exec}: killed by a signal")),
        }
    }

    fn failed_units(&mut self) -> Vec<String> {
        let Ok(output) = run(
            "systemctl",
            &[
                "list-units",
                "--failed",
                "--plain",
                "--no-legend",
                "--no-pager",
            ],
            seconds(20),
        ) else {
            return Vec::new();
        };
        output
            .stdout
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_owned)
            .collect()
    }

    fn watchdog(&mut self) -> Option<String> {
        read_trimmed("/sys/class/watchdog/watchdog0/identity")
    }

    fn journal_boots(&mut self) -> Vec<String> {
        let Ok(output) = run(
            "journalctl",
            &["--list-boots", "--no-pager", "--output=json"],
            seconds(60),
        ) else {
            return Vec::new();
        };
        let Ok(value) = json::parse(&output.stdout) else {
            return Vec::new();
        };
        value
            .as_array()
            .map(|boots| {
                boots
                    .iter()
                    .filter_map(|boot| boot.get("boot_id").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn export_journal(&mut self, boot_id: &str, name: &str, limit: u64) -> Result<String, String> {
        let path = SystemHost::state_path(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let mut child = Command::new("journalctl")
            .args(["--boot", boot_id, "--output=export", "--no-pager"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("journalctl: {error}"))?;
        let mut written: u64 = 0;
        {
            let mut file =
                fs::File::create(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            if let Some(stream) = child.stdout.take() {
                let mut limited = stream.take(limit);
                let mut buffer = [0u8; 65536];
                loop {
                    match limited.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            if file.write_all(&buffer[..count]).is_err() {
                                break;
                            }
                            written += count as u64;
                        }
                    }
                }
            }
            let _ = file.sync_all();
        }
        let _ = child.kill();
        let _ = child.wait();
        if written == 0 {
            let _ = fs::remove_file(&path);
            return Err(format!("the journal has nothing for boot {boot_id}"));
        }
        // Compress if the image has zstd; keep the plain export if not.
        let plain = path.to_string_lossy().into_owned();
        if let Ok(output) = run("zstd", &["-q", "-f", "--rm", &plain], seconds(120))
            && output.ok()
        {
            return Ok(format!("{name}.zst"));
        }
        Ok(name.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
31 1 0:28 / / ro,relatime shared:1 - overlay composefs ro,seclabel,lowerdir+=/run/x
40 31 252:3 / /sysroot ro,relatime shared:2 - ext4 /dev/vda3 rw,seclabel
41 31 252:3 /ostree/deploy/default/var /var rw,relatime shared:3 - ext4 /dev/vda3 rw,seclabel
50 31 0:40 / /boot rw,relatime shared:9 - autofs systemd-1 rw,fd=104
60 50 252:3 /boot /boot ro,relatime shared:10 - ext4 /dev/vda3 rw,seclabel
";

    #[test]
    fn the_mount_behind_a_path_is_found() {
        assert_eq!(
            mount_of(MOUNTINFO, "/boot/grub2"),
            Some((
                "/boot".to_owned(),
                "ext4".to_owned(),
                "/dev/vda3".to_owned()
            ))
        );
        assert_eq!(
            mount_of(MOUNTINFO, "/"),
            Some(("/".to_owned(), "overlay".to_owned(), "composefs".to_owned()))
        );
        assert_eq!(
            mount_of(MOUNTINFO, "/var/lib/tur"),
            Some(("/var".to_owned(), "ext4".to_owned(), "/dev/vda3".to_owned()))
        );
        // /bootstrap is not under /boot.
        assert_eq!(
            mount_of(MOUNTINFO, "/bootstrap").map(|(point, _, _)| point),
            Some("/".to_owned())
        );
    }

    #[test]
    fn an_automount_trigger_alone_is_not_the_filesystem() {
        let text = "\
31 1 0:28 / / ro shared:1 - overlay composefs ro
50 31 0:40 / /boot rw shared:9 - autofs systemd-1 rw
";
        assert_eq!(
            mount_of(text, "/boot/grub2").map(|(point, fstype, _)| (point, fstype)),
            Some(("/".to_owned(), "overlay".to_owned()))
        );
    }

    #[test]
    fn properties_and_release_values_are_read() {
        let map = properties("ActiveState=active\nNRestarts=0\nnoise\nExecStart={ path=/x ; }\n");
        assert_eq!(map.get("ActiveState").map(String::as_str), Some("active"));
        assert_eq!(
            map.get("ExecStart").map(String::as_str),
            Some("{ path=/x ; }")
        );
        assert_eq!(map.len(), 3);
        let release = "TUR_VERSION=0.1.0\nTUR_BUILD_ID=20261112.gabc\nTUR_BUILD_ID_X=no\n";
        assert_eq!(
            SystemHost::release_value(release, "TUR_BUILD_ID"),
            Some("20261112.gabc".to_owned())
        );
        assert_eq!(SystemHost::release_value(release, "MISSING"), None);
    }
}
