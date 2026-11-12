// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The single door through which the inventory reads the machine.
//!
//! Every fact comes through the [`Probe`] trait, which can only read a file,
//! list a directory, read a symbolic link and run one of a fixed set of
//! read-only commands ([`ProbeCommand`]). There is no operation that writes, so
//! nothing built on a probe can change device ownership, bind or unbind a
//! driver, or write to sysfs.
//!
//! * [`FsProbe`] reads a real directory tree: the running system, or a fixture
//!   directory laid out like a small `/sys` and `/proc`.
//! * [`FakeProbe`] is an in-memory tree for tests.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// The most directory entries returned by one listing.
pub const MAX_DIR_ENTRIES: usize = 4096;

/// The longest path a probe accepts, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;

/// The most bytes of one file kept when a fixture tree is loaded into memory.
const MAX_FIXTURE_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The most entries visited when a fixture tree is loaded into memory.
const MAX_FIXTURE_ENTRIES: usize = 100_000;

/// The most symbolic links followed while resolving one path in a
/// [`FakeProbe`], as in the kernel.
const MAX_LINK_HOPS: usize = 40;

/// Why a probe could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// The file, directory, link or command does not exist.
    NotFound,
    /// Reading it is not permitted. Common for serial numbers, which sysfs
    /// shows only to the administrator.
    Denied,
    /// A command produced more output than its limit allows.
    TooLarge,
    /// A command did not finish in time and was stopped.
    TimedOut,
    /// Any other failure, with a short reason.
    Failed(String),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("not found"),
            Self::Denied => f.write_str("permission denied"),
            Self::TooLarge => f.write_str("output larger than the limit"),
            Self::TimedOut => f.write_str("timed out"),
            Self::Failed(reason) => write!(f, "failed: {reason}"),
        }
    }
}

impl std::error::Error for ProbeError {}

fn io_error(error: io::Error) -> ProbeError {
    match error.kind() {
        io::ErrorKind::NotFound => ProbeError::NotFound,
        io::ErrorKind::PermissionDenied => ProbeError::Denied,
        kind => ProbeError::Failed(kind.to_string()),
    }
}

/// The read-only commands a probe may run. The set is closed: a probe cannot
/// be asked to run anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProbeCommand {
    /// `udevadm info --export-db`: the udev database, for device properties.
    UdevExportDb,
    /// `fwupdmgr get-devices --json`: firmware devices and available releases.
    FwupdGetDevices,
    /// `journalctl --dmesg --boot ...`: kernel messages of the current boot,
    /// for driver and firmware failures. Readable only by users allowed to
    /// read the system journal.
    KernelLog,
}

impl ProbeCommand {
    /// Every command, in a fixed order.
    pub const ALL: [ProbeCommand; 3] = [
        ProbeCommand::UdevExportDb,
        ProbeCommand::FwupdGetDevices,
        ProbeCommand::KernelLog,
    ];

    /// The file, under `commands/` in a fixture directory, that holds this
    /// command's recorded output.
    pub const fn fixture_file(self) -> &'static str {
        match self {
            ProbeCommand::UdevExportDb => "udevadm-info-export-db.txt",
            ProbeCommand::FwupdGetDevices => "fwupdmgr-get-devices.json",
            ProbeCommand::KernelLog => "journalctl-kernel.txt",
        }
    }

    /// The program run on a live system. The path is absolute so that the
    /// caller's `PATH` cannot substitute another program.
    pub const fn program(self) -> &'static str {
        match self {
            ProbeCommand::UdevExportDb => "/usr/bin/udevadm",
            ProbeCommand::FwupdGetDevices => "/usr/bin/fwupdmgr",
            ProbeCommand::KernelLog => "/usr/bin/journalctl",
        }
    }

    /// The fixed arguments given to [`program`](Self::program).
    pub const fn arguments(self) -> &'static [&'static str] {
        match self {
            ProbeCommand::UdevExportDb => &["info", "--export-db"],
            ProbeCommand::FwupdGetDevices => &["get-devices", "--json"],
            ProbeCommand::KernelLog => &[
                "--dmesg",
                "--boot",
                "--output=cat",
                "--no-pager",
                "--quiet",
                "--lines=20000",
            ],
        }
    }

    /// The most output accepted, in bytes. More is an error, not a truncation.
    pub const fn output_limit(self) -> usize {
        match self {
            ProbeCommand::UdevExportDb => 32 * 1024 * 1024,
            ProbeCommand::FwupdGetDevices | ProbeCommand::KernelLog => 8 * 1024 * 1024,
        }
    }

    /// How long the command may run before it is stopped.
    pub const fn timeout(self) -> Duration {
        match self {
            ProbeCommand::UdevExportDb | ProbeCommand::KernelLog => Duration::from_secs(10),
            ProbeCommand::FwupdGetDevices => Duration::from_secs(20),
        }
    }
}

/// Read-only access to the machine's description of its hardware.
///
/// Paths are absolute, `/`-separated and never contain `..`. Implementations
/// must bound everything they return.
pub trait Probe {
    /// Read at most `limit` bytes of a file. A longer file is cut at `limit`.
    fn read(&self, path: &str, limit: usize) -> Result<Vec<u8>, ProbeError>;

    /// The names in a directory, sorted, at most [`MAX_DIR_ENTRIES`] of them.
    /// Names that are not UTF-8 are left out.
    fn list(&self, path: &str) -> Result<Vec<String>, ProbeError>;

    /// The target of a symbolic link, as stored.
    fn read_link(&self, path: &str) -> Result<String, ProbeError>;

    /// The standard output of a read-only command.
    fn run(&self, command: ProbeCommand) -> Result<Vec<u8>, ProbeError>;
}

/// Whether a path is one a probe accepts: absolute, bounded, without NUL and
/// without a `..` component.
fn is_safe_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= MAX_PATH_BYTES
        && !path.contains('\0')
        && !path.split('/').any(|component| component == "..")
}

/// A probe over a real directory tree.
///
/// [`FsProbe::host`] reads the running system and runs the real commands.
/// [`FsProbe::fixture`] reads a directory laid out like the root of a small
/// system (`sys/`, `proc/`, `etc/machine-id`, `usr/lib/modules/`) and takes
/// command output from files in its `commands/` directory; it runs nothing.
///
/// A fixture directory is trusted input from a developer: a symbolic link in
/// it that points outside the directory is followed by the operating system.
#[derive(Debug, Clone)]
pub struct FsProbe {
    root: PathBuf,
    host_commands: bool,
}

impl FsProbe {
    /// A probe of the running system.
    pub fn host() -> Self {
        Self {
            root: PathBuf::from("/"),
            host_commands: true,
        }
    }

    /// A probe of a fixture directory. No command is run.
    pub fn fixture(dir: impl Into<PathBuf>) -> Self {
        Self {
            root: dir.into(),
            host_commands: false,
        }
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, ProbeError> {
        if !is_safe_path(path) {
            return Err(ProbeError::NotFound);
        }
        Ok(self.root.join(path.trim_start_matches('/')))
    }
}

impl Probe for FsProbe {
    fn read(&self, path: &str, limit: usize) -> Result<Vec<u8>, ProbeError> {
        let full = self.resolve(path)?;
        let metadata = fs::metadata(&full).map_err(io_error)?;
        if !metadata.is_file() {
            return Err(ProbeError::Failed("not a regular file".to_owned()));
        }
        let file = fs::File::open(&full).map_err(io_error)?;
        let mut data = Vec::new();
        file.take(u64::try_from(limit).unwrap_or(u64::MAX))
            .read_to_end(&mut data)
            .map_err(io_error)?;
        Ok(data)
    }

    fn list(&self, path: &str) -> Result<Vec<String>, ProbeError> {
        let full = self.resolve(path)?;
        let mut names = Vec::new();
        for entry in fs::read_dir(full).map_err(io_error)? {
            let Ok(entry) = entry else { continue };
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_owned());
            }
            if names.len() >= MAX_DIR_ENTRIES {
                break;
            }
        }
        names.sort();
        Ok(names)
    }

    fn read_link(&self, path: &str) -> Result<String, ProbeError> {
        let full = self.resolve(path)?;
        let target = fs::read_link(full).map_err(io_error)?;
        target
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| ProbeError::Failed("link target is not UTF-8".to_owned()))
    }

    fn run(&self, command: ProbeCommand) -> Result<Vec<u8>, ProbeError> {
        let limit = command.output_limit();
        if self.host_commands {
            return run_bounded(
                command.program(),
                command.arguments(),
                limit,
                command.timeout(),
            );
        }
        let path = format!("/commands/{}", command.fixture_file());
        let data = self.read(&path, limit.saturating_add(1))?;
        if data.len() > limit {
            return Err(ProbeError::TooLarge);
        }
        Ok(data)
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_until(child: &mut Child, deadline: Instant) -> Result<ExitStatus, ProbeError> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    stop(child);
                    return Err(ProbeError::TimedOut);
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                stop(child);
                return Err(io_error(error));
            }
        }
    }
}

/// Run a program with a cleared environment, no standard input and a bound on
/// both its output and its running time, and return its standard output.
///
/// A non-zero exit status is an error: partial output from a failed command is
/// not used.
fn run_bounded(
    program: &str,
    arguments: &[&str],
    limit: usize,
    timeout: Duration,
) -> Result<Vec<u8>, ProbeError> {
    let deadline = Instant::now() + timeout;
    let mut child = Command::new(program)
        .args(arguments)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(io_error)?;
    let Some(stdout) = child.stdout.take() else {
        stop(&mut child);
        return Err(ProbeError::Failed("no output pipe".to_owned()));
    };

    // The pipe is read on a second thread so that a command which neither
    // writes nor exits can still be stopped at the deadline.
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut data = Vec::new();
        let outcome = stdout.take(cap).read_to_end(&mut data).map(|_| data);
        let _ = sender.send(outcome);
    });

    match receiver.recv_timeout(timeout) {
        Ok(Ok(data)) if data.len() <= limit => {
            let status = wait_until(&mut child, deadline)?;
            if status.success() {
                Ok(data)
            } else {
                Err(ProbeError::Failed(format!("{program} ended with {status}")))
            }
        }
        Ok(Ok(_)) => {
            stop(&mut child);
            Err(ProbeError::TooLarge)
        }
        Ok(Err(error)) => {
            stop(&mut child);
            Err(io_error(error))
        }
        Err(_) => {
            stop(&mut child);
            Err(ProbeError::TimedOut)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    File(Vec<u8>),
    Link(String),
    Dir,
}

/// An in-memory tree that behaves like a small filesystem, for tests.
///
/// Symbolic links are resolved the way the kernel resolves them, so code that
/// works against a `FakeProbe` meets the same link structure it would meet in
/// sysfs. Command output is the content of `/commands/<file>`
/// ([`ProbeCommand::fixture_file`]), as in a fixture directory.
///
/// ```
/// use tur_device_manager::probe::{FakeProbe, Probe};
///
/// let probe = FakeProbe::new()
///     .file("/sys/devices/pci0000:00/0000:00:02.0/vendor", "0x8086\n")
///     .link(
///         "/sys/bus/pci/devices/0000:00:02.0",
///         "../../../devices/pci0000:00/0000:00:02.0",
///     );
/// assert_eq!(
///     probe.read("/sys/bus/pci/devices/0000:00:02.0/vendor", 64).unwrap(),
///     b"0x8086\n"
/// );
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FakeProbe {
    nodes: BTreeMap<String, Node>,
    denied: BTreeSet<String>,
}

fn join(components: &[String]) -> String {
    format!("/{}", components.join("/"))
}

fn normalise(path: &str) -> String {
    let components: Vec<String> = path
        .split('/')
        .filter(|component| !component.is_empty())
        .map(str::to_owned)
        .collect();
    join(&components)
}

impl FakeProbe {
    /// An empty tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a fixture directory into memory. Files are read up to 16 MiB;
    /// symbolic links are kept as links and not followed.
    pub fn from_dir(root: &Path) -> io::Result<Self> {
        let mut probe = Self::new();
        let mut pending = vec![(root.to_path_buf(), String::new())];
        let mut visited = 0_usize;
        while let Some((dir, logical)) = pending.pop() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                visited += 1;
                if visited > MAX_FIXTURE_ENTRIES {
                    return Err(io::Error::other("fixture tree is too large"));
                }
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                let child = format!("{logical}/{name}");
                let file_type = entry.file_type()?;
                if file_type.is_symlink() {
                    let target = fs::read_link(entry.path())?;
                    if let Some(target) = target.to_str() {
                        probe.nodes.insert(child, Node::Link(target.to_owned()));
                    }
                } else if file_type.is_dir() {
                    probe.nodes.insert(child.clone(), Node::Dir);
                    pending.push((entry.path(), child));
                } else if file_type.is_file() {
                    let mut data = Vec::new();
                    fs::File::open(entry.path())?
                        .take(MAX_FIXTURE_FILE_BYTES)
                        .read_to_end(&mut data)?;
                    probe.nodes.insert(child, Node::File(data));
                }
            }
        }
        Ok(probe)
    }

    /// Add or replace a file.
    #[must_use]
    pub fn file(mut self, path: &str, content: impl AsRef<[u8]>) -> Self {
        self.nodes
            .insert(normalise(path), Node::File(content.as_ref().to_vec()));
        self
    }

    /// Add or replace a symbolic link. The target is stored as given.
    #[must_use]
    pub fn link(mut self, path: &str, target: &str) -> Self {
        self.nodes
            .insert(normalise(path), Node::Link(target.to_owned()));
        self
    }

    /// Add an empty directory. Parent directories of files and links exist
    /// without being added.
    #[must_use]
    pub fn dir(mut self, path: &str) -> Self {
        self.nodes.insert(normalise(path), Node::Dir);
        self
    }

    /// Set the recorded output of a command.
    #[must_use]
    pub fn command(self, command: ProbeCommand, output: impl AsRef<[u8]>) -> Self {
        let path = format!("/commands/{}", command.fixture_file());
        self.file(&path, output)
    }

    /// Make reading or listing a path fail with [`ProbeError::Denied`].
    #[must_use]
    pub fn deny(mut self, path: &str) -> Self {
        self.denied.insert(normalise(path));
        self
    }

    /// Remove a path and everything below it.
    #[must_use]
    pub fn remove(mut self, path: &str) -> Self {
        let path = normalise(path);
        let prefix = format!("{path}/");
        self.nodes
            .retain(|key, _| *key != path && !key.starts_with(&prefix));
        self
    }

    /// Resolve a path to the canonical path of what it names, following
    /// symbolic links. The last component is followed only if `follow_last`.
    fn resolve(&self, path: &str, follow_last: bool) -> Option<String> {
        if !is_safe_path(path) {
            return None;
        }
        let mut pending: Vec<String> = path
            .split('/')
            .filter(|component| !component.is_empty())
            .rev()
            .map(str::to_owned)
            .collect();
        let mut done: Vec<String> = Vec::new();
        let mut hops = 0_usize;
        while let Some(component) = pending.pop() {
            match component.as_str() {
                "." => continue,
                ".." => {
                    done.pop();
                    continue;
                }
                _ => {}
            }
            done.push(component);
            let Some(Node::Link(target)) = self.nodes.get(&join(&done)) else {
                continue;
            };
            if pending.is_empty() && !follow_last {
                break;
            }
            hops += 1;
            if hops > MAX_LINK_HOPS {
                return None;
            }
            done.pop();
            if target.starts_with('/') {
                done.clear();
            }
            for part in target.split('/').filter(|part| !part.is_empty()).rev() {
                pending.push(part.to_owned());
            }
        }
        Some(join(&done))
    }

    fn child_prefix(canonical: &str) -> String {
        if canonical == "/" {
            "/".to_owned()
        } else {
            format!("{canonical}/")
        }
    }

    fn is_dir(&self, canonical: &str) -> bool {
        match self.nodes.get(canonical) {
            Some(Node::Dir) => true,
            Some(_) => false,
            None => {
                let prefix = Self::child_prefix(canonical);
                canonical == "/" || self.nodes.keys().any(|key| key.starts_with(&prefix))
            }
        }
    }
}

impl Probe for FakeProbe {
    fn read(&self, path: &str, limit: usize) -> Result<Vec<u8>, ProbeError> {
        let canonical = self.resolve(path, true).ok_or(ProbeError::NotFound)?;
        if self.denied.contains(&canonical) {
            return Err(ProbeError::Denied);
        }
        match self.nodes.get(&canonical) {
            Some(Node::File(data)) => Ok(data.iter().take(limit).copied().collect()),
            Some(_) => Err(ProbeError::Failed("not a regular file".to_owned())),
            None if self.is_dir(&canonical) => {
                Err(ProbeError::Failed("not a regular file".to_owned()))
            }
            None => Err(ProbeError::NotFound),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<String>, ProbeError> {
        let canonical = self.resolve(path, true).ok_or(ProbeError::NotFound)?;
        if self.denied.contains(&canonical) {
            return Err(ProbeError::Denied);
        }
        if !self.is_dir(&canonical) {
            return Err(ProbeError::NotFound);
        }
        let prefix = Self::child_prefix(&canonical);
        let names: BTreeSet<String> = self
            .nodes
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(names.into_iter().take(MAX_DIR_ENTRIES).collect())
    }

    fn read_link(&self, path: &str) -> Result<String, ProbeError> {
        let canonical = self.resolve(path, false).ok_or(ProbeError::NotFound)?;
        match self.nodes.get(&canonical) {
            Some(Node::Link(target)) => Ok(target.clone()),
            Some(_) => Err(ProbeError::Failed("not a symbolic link".to_owned())),
            None if self.is_dir(&canonical) => {
                Err(ProbeError::Failed("not a symbolic link".to_owned()))
            }
            None => Err(ProbeError::NotFound),
        }
    }

    fn run(&self, command: ProbeCommand) -> Result<Vec<u8>, ProbeError> {
        let limit = command.output_limit();
        let path = format!("/commands/{}", command.fixture_file());
        let data = self.read(&path, limit.saturating_add(1))?;
        if data.len() > limit {
            return Err(ProbeError::TooLarge);
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_tree() -> FakeProbe {
        FakeProbe::new()
            .file("/sys/devices/pci0000:00/0000:00:02.0/vendor", "0x8086\n")
            .file(
                "/sys/devices/pci0000:00/0000:00:02.0/drm/card0/dev",
                "226:0\n",
            )
            .link(
                "/sys/devices/pci0000:00/0000:00:02.0/driver",
                "../../../bus/pci/drivers/i915",
            )
            .link(
                "/sys/bus/pci/devices/0000:00:02.0",
                "../../../devices/pci0000:00/0000:00:02.0",
            )
            .link(
                "/sys/class/drm/card0",
                "../../devices/pci0000:00/0000:00:02.0/drm/card0",
            )
            .link(
                "/sys/devices/pci0000:00/0000:00:02.0/drm/card0/device",
                "../../../0000:00:02.0",
            )
            .dir("/sys/firmware/efi")
    }

    #[test]
    fn fake_reads_through_links() {
        let probe = small_tree();
        assert_eq!(
            probe.read("/sys/bus/pci/devices/0000:00:02.0/vendor", 64),
            Ok(b"0x8086\n".to_vec())
        );
        // Through two links, the second in the middle of the path.
        assert_eq!(
            probe.read("/sys/class/drm/card0/device/vendor", 64),
            Ok(b"0x8086\n".to_vec())
        );
        assert_eq!(
            probe.read("/sys/class/drm/card0/device/vendor", 3),
            Ok(b"0x8".to_vec())
        );
        assert_eq!(
            probe.read("/sys/bus/pci/devices/0000:00:02.0/missing", 64),
            Err(ProbeError::NotFound)
        );
        assert!(matches!(
            probe.read("/sys/bus/pci/devices/0000:00:02.0", 64),
            Err(ProbeError::Failed(_))
        ));
    }

    #[test]
    fn fake_reads_links_without_following_the_last_one() {
        let probe = small_tree();
        assert_eq!(
            probe.read_link("/sys/bus/pci/devices/0000:00:02.0/driver"),
            Ok("../../../bus/pci/drivers/i915".to_owned())
        );
        assert_eq!(
            probe.read_link("/sys/class/drm/card0/device/driver"),
            Ok("../../../bus/pci/drivers/i915".to_owned())
        );
        assert!(matches!(
            probe.read_link("/sys/devices/pci0000:00/0000:00:02.0/vendor"),
            Err(ProbeError::Failed(_))
        ));
        assert_eq!(probe.read_link("/sys/nothing"), Err(ProbeError::NotFound));
    }

    #[test]
    fn fake_lists_directories() {
        let probe = small_tree();
        assert_eq!(
            probe.list("/sys/bus/pci/devices/0000:00:02.0"),
            Ok(vec![
                "driver".to_owned(),
                "drm".to_owned(),
                "vendor".to_owned()
            ])
        );
        assert_eq!(probe.list("/sys/firmware/efi"), Ok(Vec::new()));
        assert_eq!(probe.list("/sys/firmware/acpi"), Err(ProbeError::NotFound));
        assert_eq!(probe.list("/"), Ok(vec!["sys".to_owned()]));
        // A dangling link is not a directory.
        assert_eq!(
            probe.list("/sys/bus/pci/devices/0000:00:02.0/driver"),
            Err(ProbeError::NotFound)
        );
    }

    #[test]
    fn fake_denies_and_removes() {
        let probe = small_tree().deny("/sys/devices/pci0000:00/0000:00:02.0/vendor");
        assert_eq!(
            probe.read("/sys/bus/pci/devices/0000:00:02.0/vendor", 64),
            Err(ProbeError::Denied)
        );
        let probe = small_tree().remove("/sys/devices/pci0000:00/0000:00:02.0/drm");
        assert_eq!(
            probe.list("/sys/bus/pci/devices/0000:00:02.0"),
            Ok(vec!["driver".to_owned(), "vendor".to_owned()])
        );
    }

    #[test]
    fn fake_stops_at_link_loops_and_unsafe_paths() {
        let probe = FakeProbe::new()
            .link("/a", "b")
            .link("/b", "a")
            .file("/etc/secret", "x");
        assert_eq!(probe.read("/a", 8), Err(ProbeError::NotFound));
        assert_eq!(probe.read("/a/../etc/secret", 8), Err(ProbeError::NotFound));
        assert_eq!(probe.read("etc/secret", 8), Err(ProbeError::NotFound));
        assert_eq!(probe.read("/etc/sec\0ret", 8), Err(ProbeError::NotFound));
    }

    #[test]
    fn fake_runs_only_recorded_commands() {
        let probe = FakeProbe::new().command(ProbeCommand::KernelLog, "line\n");
        assert_eq!(probe.run(ProbeCommand::KernelLog), Ok(b"line\n".to_vec()));
        assert_eq!(
            probe.run(ProbeCommand::UdevExportDb),
            Err(ProbeError::NotFound)
        );
    }

    #[test]
    fn commands_are_fixed_and_read_only() {
        for command in ProbeCommand::ALL {
            assert!(command.program().starts_with("/usr/bin/"));
            assert!(command.output_limit() > 0);
            assert!(!command.fixture_file().contains('/'));
            // None of the fixed argument lists contains a verb that changes
            // the system.
            for argument in command.arguments() {
                for verb in [
                    "trigger",
                    "control",
                    "update",
                    "install",
                    "activate",
                    "unlock",
                    "rotate",
                    "vacuum",
                    "flush",
                    "sync",
                    "refresh",
                    "downgrade",
                    "reinstall",
                    "clear",
                    "modify",
                    "set-",
                    "enable",
                    "disable",
                ] {
                    assert!(!argument.contains(verb), "{command:?}: {argument}");
                }
            }
        }
    }

    #[test]
    fn fs_probe_rejects_unsafe_paths() {
        let probe = FsProbe::fixture("/nonexistent-tur-fixture");
        assert_eq!(probe.read("/../etc/passwd", 16), Err(ProbeError::NotFound));
        assert_eq!(probe.read("relative", 16), Err(ProbeError::NotFound));
        assert_eq!(probe.list("/sys"), Err(ProbeError::NotFound));
        assert_eq!(
            probe.run(ProbeCommand::UdevExportDb),
            Err(ProbeError::NotFound)
        );
    }

    // The three tests below start a harmless program (`echo`, `sleep`) to
    // exercise the bounding of command output and running time. They read
    // nothing about the machine's hardware.

    #[test]
    fn bounded_run_returns_output() {
        let output = run_bounded("/bin/echo", &["hello"], 64, Duration::from_secs(10));
        assert_eq!(output, Ok(b"hello\n".to_vec()));
    }

    #[test]
    fn bounded_run_refuses_too_much_output() {
        let output = run_bounded("/bin/echo", &["0123456789"], 4, Duration::from_secs(10));
        assert_eq!(output, Err(ProbeError::TooLarge));
    }

    #[test]
    fn bounded_run_stops_a_slow_command() {
        let started = Instant::now();
        let output = run_bounded("/bin/sleep", &["30"], 64, Duration::from_millis(200));
        assert_eq!(output, Err(ProbeError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn bounded_run_reports_failure() {
        assert_eq!(
            run_bounded("/nonexistent/tur-probe", &[], 64, Duration::from_secs(1)),
            Err(ProbeError::NotFound)
        );
        assert!(matches!(
            run_bounded("/bin/sleep", &["not-a-number"], 64, Duration::from_secs(10)),
            Err(ProbeError::Failed(_))
        ));
    }
}
