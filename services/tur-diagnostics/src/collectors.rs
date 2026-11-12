// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What `tur-diag collect` gathers (specification §6.4, layout from §29.1).
//!
//! A collector is data: where its output goes in the bundle, and either a
//! command line or a file to read. Nothing here is conditional code, so the
//! whole table can be reviewed, and tested, in one place.
//!
//! No collector reads below a home directory, and file collectors may read
//! only below a short list of system locations ([`is_allowed_path`]). The
//! same check runs again when a collector is executed.

use std::time::Duration;

use crate::host::{Host, Limits, ReadError, RunError};
use crate::manifest::{MissingEntry, MissingReason};
use crate::redact::Format;

/// Time limit for an ordinary collector, in seconds.
const DEFAULT_TIMEOUT_SECONDS: u64 = 20;
/// Time limit for collectors that read the journal, in seconds.
const JOURNAL_TIMEOUT_SECONDS: u64 = 60;
/// The default size limit for one collector's output: 64 MiB.
pub const DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024;
/// How much of a failed command's standard error is quoted as detail.
const STDERR_EXCERPT_CHARS: usize = 300;

/// The only locations a file collector may read, and the only absolute paths
/// a command collector may be given as an argument.
pub const ALLOWED_PATH_PREFIXES: &[&str] = &["/proc/", "/sys/", "/usr/lib/", "/etc/os-release"];

/// Where a collector's content comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The standard output of a command, run without a shell.
    Command {
        /// The program to run, looked up on the search path.
        program: &'static str,
        /// Its arguments.
        args: &'static [&'static str],
    },
    /// The content of a file.
    File {
        /// The absolute path of the file.
        path: &'static str,
    },
}

/// One thing to collect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collector {
    /// The bundle directory the output goes in, for example `hardware`.
    pub category: &'static str,
    /// The file name within that directory, for example `pci.txt`.
    pub file: &'static str,
    /// Where the content comes from.
    pub source: Source,
    /// How the content is structured.
    pub format: Format,
    /// Whether this is about an earlier boot, which `--since-boot` excludes.
    pub previous_boot: bool,
    /// Lines that start with one of these prefixes (after leading white
    /// space) are dropped before anything else happens. For free-form names
    /// that a person chose and no rule could recognise.
    pub omit_lines: &'static [&'static str],
    /// The command is killed after this many seconds.
    pub timeout_seconds: u64,
}

impl Collector {
    /// A collector that runs a command.
    pub const fn command(
        category: &'static str,
        file: &'static str,
        program: &'static str,
        args: &'static [&'static str],
    ) -> Self {
        Collector {
            category,
            file,
            source: Source::Command { program, args },
            format: Format::Text,
            previous_boot: false,
            omit_lines: &[],
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
        }
    }

    /// A collector that reads a file.
    pub const fn file(category: &'static str, file: &'static str, path: &'static str) -> Self {
        Collector {
            category,
            file,
            source: Source::File { path },
            format: Format::Text,
            previous_boot: false,
            omit_lines: &[],
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
        }
    }

    /// Marks the output as one JSON document per line.
    pub const fn json_lines(mut self) -> Self {
        self.format = Format::JsonLines;
        self
    }

    /// Marks the collector as being about an earlier boot.
    pub const fn about_previous_boot(mut self) -> Self {
        self.previous_boot = true;
        self
    }

    /// Drops output lines that start with one of `prefixes`.
    pub const fn omitting_lines(mut self, prefixes: &'static [&'static str]) -> Self {
        self.omit_lines = prefixes;
        self
    }

    /// Sets the time limit.
    pub const fn with_timeout_seconds(mut self, seconds: u64) -> Self {
        self.timeout_seconds = seconds;
        self
    }

    /// The path of this collector's output inside the bundle.
    pub fn bundle_path(&self) -> String {
        format!("{}/{}", self.category, self.file)
    }

    /// The command line or file path, as recorded in the manifest.
    pub fn describe_source(&self) -> String {
        match self.source {
            Source::Command { program, args } => {
                let mut line = program.to_owned();
                for arg in args {
                    line.push(' ');
                    line.push_str(arg);
                }
                line
            }
            Source::File { path } => path.to_owned(),
        }
    }

    /// The first path this collector names that it is not allowed to touch.
    pub fn forbidden_path(&self) -> Option<&'static str> {
        match self.source {
            Source::File { path } => (!is_allowed_path(path)).then_some(path),
            Source::Command { args, .. } => args
                .iter()
                .copied()
                .find(|arg| arg.starts_with('/') && !is_allowed_path(arg)),
        }
    }
}

/// Whether a collector may read `path`: an absolute path without `.` or `..`
/// components, below one of [`ALLOWED_PATH_PREFIXES`].
pub fn is_allowed_path(path: &str) -> bool {
    let clean = path.starts_with('/')
        && !path
            .split('/')
            .any(|component| component == "." || component == "..");
    clean
        && ALLOWED_PATH_PREFIXES.iter().any(|prefix| {
            if prefix.ends_with('/') {
                path.starts_with(*prefix)
            } else {
                path == *prefix
            }
        })
}

/// The collectors `tur-diag collect` runs.
///
/// Commands are chosen and given arguments so that they print as little
/// personal data as possible in the first place; redaction is the second
/// line of defence, not the first. In particular:
///
/// * `lsblk` and `blkid` are asked for named columns and tags only, which
///   leaves out filesystem and partition labels.
/// * `nmcli` is asked for device state without connection names.
/// * `bluetoothctl show` loses its `Name:` and `Alias:` lines.
/// * `/etc/fstab` is not read: it may hold credentials for network shares.
///   The mount table from `/proc/mounts` carries what is mounted and how.
pub const COLLECTORS: &[Collector] = &[
    // system/
    Collector::command("system", "kernel.txt", "uname", &["-a"]),
    Collector::file("system", "kernel-cmdline.txt", "/proc/cmdline"),
    Collector::file("system", "os-release.txt", "/etc/os-release"),
    Collector::file("system", "tur-release.txt", "/usr/lib/tur/release"),
    Collector::file("system", "modules.txt", "/proc/modules"),
    Collector::command(
        "system",
        "deployment.json",
        "bootc",
        &["status", "--format=json"],
    ),
    Collector::command(
        "system",
        "failed-units.txt",
        "systemctl",
        &[
            "list-units",
            "--state=failed",
            "--all",
            "--no-legend",
            "--plain",
            "--no-pager",
        ],
    ),
    Collector::command(
        "system",
        "systemd-analyze.txt",
        "systemd-analyze",
        &["critical-chain", "--no-pager"],
    ),
    // boot/
    Collector::command(
        "boot",
        "current-journal.jsonl",
        "journalctl",
        &["--boot=0", "--output=json", "--no-pager", "--lines=50000"],
    )
    .json_lines()
    .with_timeout_seconds(JOURNAL_TIMEOUT_SECONDS),
    Collector::command(
        "boot",
        "previous-journal.jsonl",
        "journalctl",
        &["--boot=-1", "--output=json", "--no-pager", "--lines=20000"],
    )
    .json_lines()
    .about_previous_boot()
    .with_timeout_seconds(JOURNAL_TIMEOUT_SECONDS),
    Collector::command("boot", "kernel-ring-buffer.txt", "dmesg", &[]),
    Collector::command(
        "boot",
        "pstore/listing.txt",
        "ls",
        &["-lA", "/sys/fs/pstore"],
    )
    .about_previous_boot(),
    Collector::command(
        "boot",
        "boot-health.json",
        "tur-boot-health",
        &["status", "--json"],
    ),
    // hardware/
    Collector::command("hardware", "pci.txt", "lspci", &["-nnk"]),
    Collector::command("hardware", "usb.txt", "lsusb", &[]),
    Collector::command(
        "hardware",
        "block-devices.json",
        "lsblk",
        &[
            "--json",
            "--bytes",
            "--output",
            "NAME,KNAME,TYPE,SIZE,RO,RM,ROTA,TRAN,VENDOR,MODEL,REV,SERIAL,WWN,FSTYPE,FSVER,UUID,\
             PARTUUID,PARTTYPE,PTTYPE,MOUNTPOINTS,PHY-SEC,LOG-SEC,STATE",
        ],
    ),
    Collector::command("hardware", "gpu-drm.txt", "ls", &["-l", "/sys/class/drm"]),
    Collector::command("hardware", "gpu-vulkan.txt", "vulkaninfo", &["--summary"]),
    Collector::command("hardware", "audio.txt", "wpctl", &["status"]),
    Collector::command("hardware", "network.txt", "nmcli", &["general", "status"]),
    Collector::command(
        "hardware",
        "network-devices.txt",
        "nmcli",
        &["--fields", "DEVICE,TYPE,STATE", "device", "status"],
    ),
    Collector::command("hardware", "bluetooth.txt", "bluetoothctl", &["show"])
        .omitting_lines(&["Name:", "Alias:"]),
    Collector::command("hardware", "firmware.txt", "fwupdmgr", &["get-devices"]),
    // storage/
    Collector::command(
        "storage",
        "filesystems.txt",
        "blkid",
        &["-s", "TYPE", "-s", "UUID", "-s", "PARTUUID", "-s", "PTTYPE"],
    ),
    Collector::file("storage", "mounts.txt", "/proc/mounts"),
    // services/
    Collector::command(
        "services",
        "tur-service-status.txt",
        "systemctl",
        &[
            "list-units",
            "tur-*",
            "--all",
            "--no-legend",
            "--plain",
            "--no-pager",
        ],
    ),
];

/// Options for one collection run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectOptions {
    /// Leave out everything about earlier boots.
    pub since_boot: bool,
    /// Size limit for one collector's output, in bytes.
    pub max_bytes: usize,
}

impl Default for CollectOptions {
    fn default() -> Self {
        CollectOptions {
            since_boot: false,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// The unredacted output of one collector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collected {
    /// The path the output will have inside the bundle.
    pub path: String,
    /// How the content is structured.
    pub format: Format,
    /// The content. Bytes that are not UTF-8 have been replaced.
    pub text: String,
}

/// Runs every collector. A collector that fails becomes an entry in the
/// second list; it never stops the others.
pub fn run_collectors(
    host: &dyn Host,
    collectors: &[Collector],
    options: &CollectOptions,
) -> (Vec<Collected>, Vec<MissingEntry>) {
    let mut collected = Vec::new();
    let mut missing = Vec::new();
    for collector in collectors {
        let (output, problem) = run_collector(host, collector, options);
        if let Some(output) = output {
            collected.push(output);
        }
        if let Some(problem) = problem {
            missing.push(problem);
        }
    }
    (collected, missing)
}

/// Runs one collector. Returns its output, a missing-data entry, or both
/// when the output is present but was cut short.
pub fn run_collector(
    host: &dyn Host,
    collector: &Collector,
    options: &CollectOptions,
) -> (Option<Collected>, Option<MissingEntry>) {
    let absent = |reason: MissingReason, detail: String| {
        let entry = MissingEntry {
            path: collector.bundle_path(),
            source: collector.describe_source(),
            reason,
            detail,
        };
        (None, Some(entry))
    };

    if let Some(path) = collector.forbidden_path() {
        return absent(
            MissingReason::ForbiddenPath,
            format!("{path} is outside the locations a collector may read"),
        );
    }
    if options.since_boot && collector.previous_boot {
        return absent(
            MissingReason::ExcludedByOption,
            "--since-boot leaves out data from earlier boots".to_owned(),
        );
    }

    let (data, truncated) = match collector.source {
        Source::Command { program, args } => {
            let limits = Limits {
                timeout: Duration::from_secs(collector.timeout_seconds),
                max_bytes: options.max_bytes,
            };
            match host.run(program, args, limits) {
                Ok(output) if output.success() => (output.stdout, output.truncated),
                Ok(output) => {
                    let status = match output.exit_code {
                        Some(code) => format!("exit code {code}"),
                        None => "ended by a signal".to_owned(),
                    };
                    let stderr = excerpt(&output.stderr);
                    let detail = if stderr.is_empty() {
                        status
                    } else {
                        format!("{status}: {stderr}")
                    };
                    return absent(MissingReason::NonZeroExit, detail);
                }
                Err(RunError::NotFound) => {
                    return absent(
                        MissingReason::CommandNotFound,
                        format!("{program} is not installed"),
                    );
                }
                Err(RunError::PermissionDenied) => {
                    return absent(
                        MissingReason::PermissionDenied,
                        format!("{program} may not be executed by this user"),
                    );
                }
                Err(RunError::TimedOut) => {
                    let seconds = collector.timeout_seconds;
                    return absent(
                        MissingReason::TimedOut,
                        format!("killed after {seconds} seconds"),
                    );
                }
                Err(RunError::Io(error)) => return absent(MissingReason::IoError, error),
            }
        }
        Source::File { path } => match host.read_file(path, options.max_bytes) {
            Ok(content) => (content.data, content.truncated),
            Err(ReadError::NotFound) => {
                return absent(
                    MissingReason::FileNotFound,
                    format!("{path} does not exist"),
                );
            }
            Err(ReadError::PermissionDenied) => {
                return absent(
                    MissingReason::PermissionDenied,
                    format!("{path} may not be read by this user"),
                );
            }
            Err(ReadError::Io(error)) => return absent(MissingReason::IoError, error),
        },
    };

    let mut text = String::from_utf8_lossy(&data).into_owned();
    let mut problem = None;
    if truncated {
        // Drop the last, incomplete line.
        let complete = text.rfind('\n').map_or(0, |index| index + 1);
        text.truncate(complete);
        let limit = options.max_bytes;
        problem = Some(MissingEntry {
            path: collector.bundle_path(),
            source: collector.describe_source(),
            reason: MissingReason::Truncated,
            detail: format!("output exceeded {limit} bytes; the end was dropped"),
        });
    }
    if !collector.omit_lines.is_empty() {
        text = omit_lines(&text, collector.omit_lines);
    }
    let output = Collected {
        path: collector.bundle_path(),
        format: collector.format,
        text,
    };
    (Some(output), problem)
}

/// Removes the lines that start with one of `prefixes` after leading white space.
fn omit_lines(text: &str, prefixes: &[&str]) -> String {
    let mut kept = String::with_capacity(text.len());
    for line in text.lines() {
        let content = line.trim_start();
        if prefixes.iter().any(|prefix| content.starts_with(*prefix)) {
            continue;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    kept
}

/// The first line of a command's standard error, shortened.
fn excerpt(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    line.trim().chars().take(STDERR_EXCERPT_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::host::{CommandOutput, FakeHost};

    const OPTIONS: CollectOptions = CollectOptions {
        since_boot: false,
        max_bytes: 1024,
    };

    #[test]
    fn no_collector_touches_a_home_directory_or_anything_outside_the_allow_list() {
        const FORBIDDEN_WORDS: [&str; 9] = [
            "home",
            "/root",
            "~",
            "$",
            "/Users",
            "/run/user",
            "/run/media",
            "/media",
            "/mnt",
        ];
        for collector in COLLECTORS {
            let source = collector.describe_source();
            assert_eq!(collector.forbidden_path(), None, "{source}");
            for word in FORBIDDEN_WORDS {
                assert!(!source.contains(word), "{source} mentions {word}");
            }
            match collector.source {
                Source::File { path } => {
                    assert!(is_allowed_path(path), "{path} is not on the allow list");
                }
                Source::Command { program, args } => {
                    assert!(
                        !program.contains('/'),
                        "{program} must be found on the search path"
                    );
                    assert!(
                        !matches!(
                            program,
                            "sh" | "bash" | "env" | "sudo" | "pkexec" | "cat" | "find"
                        ),
                        "{program} could read anything"
                    );
                    for arg in args {
                        if arg.contains('/') && !arg.contains(',') {
                            assert!(
                                is_allowed_path(arg),
                                "{source}: {arg} is not on the allow list"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_allow_list_rejects_home_directories_and_path_tricks() {
        for path in [
            "/proc/cmdline",
            "/proc/sys/kernel/osrelease",
            "/sys/class/drm",
            "/usr/lib/tur/release",
            "/etc/os-release",
        ] {
            assert!(is_allowed_path(path), "{path}");
        }
        for path in [
            "/home/alice/.ssh/id_ed25519",
            "/var/home/alice/Documents",
            "/root/.bash_history",
            "/etc/shadow",
            "/etc/os-release.d/extra",
            "/etc/NetworkManager/system-connections/home.nmconnection",
            "/proc/../home/alice",
            "/sys/./../etc/shadow",
            "/usr/lib/../../home/alice",
            "proc/cmdline",
            "/proc",
            "",
            "~/.config",
        ] {
            assert!(!is_allowed_path(path), "{path}");
        }
    }

    #[test]
    fn collector_paths_are_unique_and_follow_the_bundle_layout() {
        let mut paths = BTreeSet::new();
        for collector in COLLECTORS {
            let path = collector.bundle_path();
            assert!(
                matches!(
                    collector.category,
                    "system" | "boot" | "hardware" | "storage" | "services"
                ),
                "{path}"
            );
            assert!(
                !collector.file.starts_with('/') && !collector.file.contains(".."),
                "{path}"
            );
            assert!(
                collector.timeout_seconds > 0 && collector.timeout_seconds <= 120,
                "{path}"
            );
            assert!(paths.insert(path.clone()), "{path} is produced twice");
        }
        for expected in [
            "system/kernel.txt",
            "system/kernel-cmdline.txt",
            "system/os-release.txt",
            "system/tur-release.txt",
            "system/failed-units.txt",
            "system/systemd-analyze.txt",
            "boot/current-journal.jsonl",
            "boot/previous-journal.jsonl",
            "boot/kernel-ring-buffer.txt",
            "services/tur-service-status.txt",
        ] {
            assert!(paths.contains(expected), "{expected} is missing");
        }
    }

    #[test]
    fn journals_are_json_lines_and_everything_else_is_text() {
        for collector in COLLECTORS {
            let expected = if collector.file.ends_with(".jsonl") {
                Format::JsonLines
            } else {
                Format::Text
            };
            assert_eq!(collector.format, expected, "{}", collector.bundle_path());
        }
    }

    #[test]
    fn a_successful_command_is_collected() {
        let host = FakeHost::new().with_command("uname -a", "Linux test 6.17.1\n");
        let collector = Collector::command("system", "kernel.txt", "uname", &["-a"]);
        let (output, problem) = run_collector(&host, &collector, &OPTIONS);
        let output = output.expect("output is present");
        assert_eq!(output.path, "system/kernel.txt");
        assert_eq!(output.text, "Linux test 6.17.1\n");
        assert_eq!(output.format, Format::Text);
        assert_eq!(problem, None);
    }

    #[test]
    fn a_file_is_collected() {
        let host = FakeHost::new().with_file("/proc/cmdline", "quiet rhgb\n");
        let collector = Collector::file("system", "kernel-cmdline.txt", "/proc/cmdline");
        let (output, problem) = run_collector(&host, &collector, &OPTIONS);
        assert_eq!(output.expect("output is present").text, "quiet rhgb\n");
        assert_eq!(problem, None);
    }

    fn reason_for(host: &FakeHost, collector: &Collector) -> (MissingReason, String) {
        let (output, problem) = run_collector(host, collector, &OPTIONS);
        assert_eq!(output, None, "a failed collector must not produce a file");
        let problem = problem.expect("a failed collector is recorded");
        assert_eq!(problem.path, collector.bundle_path());
        assert_eq!(problem.source, collector.describe_source());
        (problem.reason, problem.detail)
    }

    #[test]
    fn every_kind_of_failure_is_recorded_with_its_reason() {
        let command = Collector::command("hardware", "pci.txt", "lspci", &["-nnk"]);
        let file = Collector::file("system", "tur-release.txt", "/usr/lib/tur/release");

        let (reason, detail) = reason_for(&FakeHost::new(), &command);
        assert_eq!(reason, MissingReason::CommandNotFound);
        assert_eq!(detail, "lspci is not installed");

        let host = FakeHost::new().with_failing_command("lspci", 3, "\nlspci: cannot open\nmore\n");
        let (reason, detail) = reason_for(&host, &command);
        assert_eq!(reason, MissingReason::NonZeroExit);
        assert_eq!(detail, "exit code 3: lspci: cannot open");

        let killed = CommandOutput {
            exit_code: None,
            ..CommandOutput::default()
        };
        let host = FakeHost::new().with_command_result("lspci", Ok(killed));
        let (reason, detail) = reason_for(&host, &command);
        assert_eq!(reason, MissingReason::NonZeroExit);
        assert_eq!(detail, "ended by a signal");

        let host = FakeHost::new().with_command_result("lspci", Err(RunError::TimedOut));
        let (reason, detail) = reason_for(&host, &command);
        assert_eq!(reason, MissingReason::TimedOut);
        assert_eq!(detail, "killed after 20 seconds");

        let host = FakeHost::new().with_command_result("lspci", Err(RunError::PermissionDenied));
        assert_eq!(
            reason_for(&host, &command).0,
            MissingReason::PermissionDenied
        );

        let host =
            FakeHost::new().with_command_result("lspci", Err(RunError::Io("boom".to_owned())));
        assert_eq!(
            reason_for(&host, &command),
            (MissingReason::IoError, "boom".to_owned())
        );

        let (reason, detail) = reason_for(&FakeHost::new(), &file);
        assert_eq!(reason, MissingReason::FileNotFound);
        assert_eq!(detail, "/usr/lib/tur/release does not exist");

        let host =
            FakeHost::new().with_file_error("/usr/lib/tur/release", ReadError::PermissionDenied);
        assert_eq!(reason_for(&host, &file).0, MissingReason::PermissionDenied);

        let host = FakeHost::new()
            .with_file_error("/usr/lib/tur/release", ReadError::Io("bad".to_owned()));
        assert_eq!(reason_for(&host, &file).0, MissingReason::IoError);
    }

    #[test]
    fn a_collector_outside_the_allow_list_is_refused_before_it_runs() {
        let host = FakeHost::new()
            .with_file("/home/alice/.ssh/id_ed25519", "PRIVATE KEY")
            .with_command("ls", "Documents\n");
        let file = Collector::file("system", "key.txt", "/home/alice/.ssh/id_ed25519");
        assert_eq!(file.forbidden_path(), Some("/home/alice/.ssh/id_ed25519"));
        assert_eq!(reason_for(&host, &file).0, MissingReason::ForbiddenPath);

        let command = Collector::command("system", "home.txt", "ls", &["-l", "/home/alice"]);
        assert_eq!(command.forbidden_path(), Some("/home/alice"));
        assert_eq!(reason_for(&host, &command).0, MissingReason::ForbiddenPath);
    }

    #[test]
    fn since_boot_excludes_collectors_about_earlier_boots() {
        let host = FakeHost::new().with_command("journalctl", "{}\n");
        let previous = Collector::command("boot", "previous-journal.jsonl", "journalctl", &[])
            .json_lines()
            .about_previous_boot();
        let since_boot = CollectOptions {
            since_boot: true,
            max_bytes: 1024,
        };
        let (output, problem) = run_collector(&host, &previous, &since_boot);
        assert_eq!(output, None);
        assert_eq!(
            problem.expect("the exclusion is recorded").reason,
            MissingReason::ExcludedByOption
        );

        let (output, problem) = run_collector(&host, &previous, &OPTIONS);
        assert_eq!(
            output.expect("collected without --since-boot").format,
            Format::JsonLines
        );
        assert_eq!(problem, None);
    }

    #[test]
    fn truncated_output_is_kept_to_the_last_whole_line_and_recorded() {
        let host = FakeHost::new().with_command("dmesg", "line one\nline two\nline three\n");
        let collector = Collector::command("boot", "kernel-ring-buffer.txt", "dmesg", &[]);
        let options = CollectOptions {
            since_boot: false,
            max_bytes: 14,
        };
        let (output, problem) = run_collector(&host, &collector, &options);
        assert_eq!(output.expect("partial output is kept").text, "line one\n");
        let problem = problem.expect("the truncation is recorded");
        assert_eq!(problem.reason, MissingReason::Truncated);
        assert_eq!(
            problem.detail,
            "output exceeded 14 bytes; the end was dropped"
        );
    }

    #[test]
    fn omitted_lines_never_reach_the_output() {
        let host = FakeHost::new().with_command(
            "bluetoothctl",
            "Controller 3C:52:82:AA:0B:1F (public)\n\tName: alice-laptop\n\tAlias: Alice's laptop\n\tPowered: yes\n",
        );
        let collector = COLLECTORS
            .iter()
            .find(|collector| collector.file == "bluetooth.txt")
            .expect("there is a Bluetooth collector");
        let (output, _) = run_collector(&host, collector, &OPTIONS);
        assert_eq!(
            output.expect("output is present").text,
            "Controller 3C:52:82:AA:0B:1F (public)\n\tPowered: yes\n"
        );
    }

    #[test]
    fn one_failing_collector_does_not_stop_the_others() {
        let host = FakeHost::new()
            .with_command("uname", "Linux\n")
            .with_command_result("lspci", Err(RunError::TimedOut))
            .with_file("/proc/cmdline", "quiet\n");
        let collectors = [
            Collector::command("hardware", "pci.txt", "lspci", &["-nnk"]),
            Collector::command("system", "kernel.txt", "uname", &["-a"]),
            Collector::command("hardware", "usb.txt", "lsusb", &[]),
            Collector::file("system", "kernel-cmdline.txt", "/proc/cmdline"),
        ];
        let (collected, missing) = run_collectors(&host, &collectors, &CollectOptions::default());
        let paths: Vec<&str> = collected.iter().map(|item| item.path.as_str()).collect();
        assert_eq!(paths, ["system/kernel.txt", "system/kernel-cmdline.txt"]);
        let reasons: Vec<(&str, MissingReason)> = missing
            .iter()
            .map(|entry| (entry.path.as_str(), entry.reason))
            .collect();
        assert_eq!(
            reasons,
            [
                ("hardware/pci.txt", MissingReason::TimedOut),
                ("hardware/usb.txt", MissingReason::CommandNotFound),
            ]
        );
    }

    #[test]
    fn the_whole_table_runs_on_a_host_with_nothing_installed() {
        let (collected, missing) =
            run_collectors(&FakeHost::new(), COLLECTORS, &CollectOptions::default());
        assert!(collected.is_empty());
        assert_eq!(missing.len(), COLLECTORS.len());
        for entry in &missing {
            assert!(
                matches!(
                    entry.reason,
                    MissingReason::CommandNotFound | MissingReason::FileNotFound
                ),
                "{}: {:?}",
                entry.path,
                entry.reason
            );
        }
    }
}
