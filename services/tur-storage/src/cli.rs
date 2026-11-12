// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `tur-storage` command line.
//!
//! Three read-only commands:
//!
//! * `volumes (--from <lsblk.json> | --live)`: the inventory, with the
//!   decision and the runtime views for each volume.
//! * `decide (--from <lsblk.json> | --live) --volume <id>
//!   [--access read-only|read-write]`: the decision for one volume.
//! * `capabilities --from-fs <fs> --to-fs <fs>`: what a copy between two
//!   filesystems would lose.
//!
//! The block devices come from one of two places, and the command line says
//! which. `--from` names a file captured earlier; then the tool looks at no
//! device and runs no other program. `--live` asks about this machine: the
//! tool runs `lsblk --json --bytes --output-all`, a program that lists block
//! devices and changes nothing, and reads what it prints
//! ([`crate::probe::LiveProbe`]). It runs nothing else, opens no device
//! itself and never mounts, unlocks or writes. Either way it takes no
//! setting from the process surroundings.
//!
//! Access is spelled `read-only` and `read-write`, as the assistant tool
//! manifest and the JSON output spell it. `ro` and `rw` are still accepted.
//!
//! Exit status: 0 on success, 1 when the input is unusable or the volume is
//! unknown, 2 for a usage error. `decide` exits 0 whatever the decision is;
//! the decision is the output.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::SCHEMA_VERSION;
use crate::capabilities::{CapabilityAssessment, Loss, copy_losses};
use crate::codes::StorageCode;
use crate::decision::{Access, MountRequest, printable};
use crate::filesystem::Filesystem;
use crate::namespace::{MountPlan, NamespaceContext, PlannedVolume, UserName, plan};
use crate::policy::{PolicyOverrides, UnsafeNtfsPolicy, policy_for};
use crate::probe::{CommandRunner, LiveProbe, Probe, SnapshotProbe, SystemRunner};
use crate::volume::Inventory;

/// The parsed command line.
#[derive(Debug, Parser)]
#[command(
    name = "tur-storage",
    version,
    about = "TUR storage policy core: lists volumes and previews mount decisions, from captured \
             data or for this machine. It never mounts, unlocks or writes."
)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List the volumes of an lsblk snapshot or of this machine, with the
    /// decision for each
    Volumes(VolumesArgs),
    /// Show the mount decision for one volume
    Decide(DecideArgs),
    /// List the metadata that a copy between two filesystems would lose
    Capabilities(CapabilitiesArgs),
}

/// Access requested on the command line.
///
/// Spelled as [`Access`] is spelled in the JSON output and in the assistant
/// tool manifest: `read-only`, `read-write`. The short forms `ro` and `rw`
/// are accepted as well and are not shown in the help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AccessArg {
    /// Read-only
    #[value(name = "read-only", alias = "ro")]
    ReadOnly,
    /// Read and write
    #[value(name = "read-write", alias = "rw")]
    ReadWrite,
}

/// Handling of NTFS volumes that are not known to be clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum UnsafeNtfsArg {
    /// Mount read-only
    ReadOnly,
    /// Do not mount
    Refuse,
}

/// Input files and policy overrides shared by `volumes` and `decide`.
#[derive(Debug, Args)]
pub struct InputArgs {
    /// File holding the output of `lsblk --json --bytes --output-all`
    #[arg(
        long,
        value_name = "FILE",
        required_unless_present = "live",
        conflicts_with = "live"
    )]
    pub from: Option<PathBuf>,
    /// Ask about this machine: run `lsblk --json --bytes --output-all` and
    /// read what it prints. Nothing else is run and nothing is changed
    #[arg(long)]
    pub live: bool,
    /// File holding `blkid --output export` style properties, including the
    /// TUR_NTFS_STATE and TUR_HFS_JOURNAL facts
    #[arg(long, value_name = "FILE")]
    pub props: Option<PathBuf>,
    /// Login name for the user-facing paths under /run/media
    #[arg(long, value_name = "NAME")]
    pub user: Option<String>,
    /// User who owns files on filesystems without POSIX ownership
    #[arg(long, value_name = "UID", default_value_t = 1000)]
    pub uid: u32,
    /// Group for files on filesystems without POSIX ownership
    #[arg(long, value_name = "GID", default_value_t = 1000)]
    pub gid: u32,
    /// What to do with an NTFS volume that is not known to be clean
    #[arg(long, value_enum, value_name = "POLICY", default_value = "read-only")]
    pub ntfs_unsafe: UnsafeNtfsArg,
    /// Allow writing to HFS+ volumes that are known not to be journalled
    #[arg(long)]
    pub hfsplus_write: bool,
    /// Enable read-only APFS mounts
    #[arg(long)]
    pub apfs_read_only: bool,
    /// Add noexec to mounts of removable media
    #[arg(long)]
    pub noexec_removable: bool,
    /// Print JSON instead of text
    #[arg(long)]
    pub json: bool,
}

/// Arguments of `volumes`.
#[derive(Debug, Args)]
pub struct VolumesArgs {
    /// Input files and policy overrides.
    #[command(flatten)]
    pub input: InputArgs,
}

/// Arguments of `decide`.
#[derive(Debug, Args)]
pub struct DecideArgs {
    /// Input files and policy overrides.
    #[command(flatten)]
    pub input: InputArgs,
    /// TUR volume identifier, as printed by `volumes`
    #[arg(long, value_name = "ID")]
    pub volume: String,
    /// Access to ask for
    #[arg(
        long,
        value_enum,
        value_name = "ACCESS",
        default_value = "read-write"
    )]
    pub access: AccessArg,
}

/// Arguments of `capabilities`.
#[derive(Debug, Args)]
pub struct CapabilitiesArgs {
    /// Filesystem the files are copied from
    #[arg(long, value_name = "FS")]
    pub from_fs: String,
    /// Filesystem the files are copied to
    #[arg(long, value_name = "FS")]
    pub to_fs: String,
    /// Print JSON instead of text
    #[arg(long)]
    pub json: bool,
}

/// Why a command failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A condition with a registered error code.
    Coded {
        /// The code.
        code: StorageCode,
        /// What exactly went wrong.
        detail: String,
    },
    /// The command line asked for something that makes no sense.
    Usage(String),
    /// The plan failed its own invariant check. This is a bug.
    Internal(String),
    /// The result could not be written.
    Output(String),
}

impl Failure {
    /// The process exit status for this failure.
    pub fn exit_status(&self) -> u8 {
        match self {
            Failure::Usage(_) => 2,
            Failure::Coded { .. } | Failure::Internal(_) | Failure::Output(_) => 1,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Coded { code, detail } => {
                let entry = code.entry();
                write!(
                    f,
                    "{code}\n{}\n\nAction:\n{}\n\nTechnical detail:\n{detail}",
                    entry.user_message, entry.recovery_action
                )
            }
            Failure::Usage(message) => write!(f, "error: {message}"),
            Failure::Internal(message) => write!(f, "internal error: {message}"),
            Failure::Output(message) => write!(f, "error: cannot write output: {message}"),
        }
    }
}

#[derive(Serialize)]
struct InventoryReport<'a> {
    schema_version: u32,
    kind: &'static str,
    policy: PolicyOverrides,
    volumes: &'a [PlannedVolume],
}

#[derive(Serialize)]
struct DecisionReport<'a> {
    schema_version: u32,
    kind: &'static str,
    policy: PolicyOverrides,
    volume: &'a PlannedVolume,
}

#[derive(Serialize)]
struct CapabilityReport {
    schema_version: u32,
    kind: &'static str,
    source: Filesystem,
    destination: Filesystem,
    assessment: CapabilityAssessment,
    losses: Vec<Loss>,
}

/// Runs the command line and returns the process exit status.
///
/// `args` includes the program name. Results go to `stdout`; usage errors
/// and failures go to `stderr`. With `--live`, `lsblk` is run on this
/// machine.
pub fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run_with_runner(args, &SystemRunner, stdout, stderr)
}

/// As [`run`], with the runner the live probe uses given by the caller. A
/// test passes one that runs nothing.
pub fn run_with_runner<I, T>(
    args: I,
    runner: &dyn CommandRunner,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            // Help and version requests arrive here too; they are not errors.
            let text = error.to_string();
            return if error.use_stderr() {
                let _ = stderr.write_all(text.as_bytes());
                2
            } else {
                let _ = stdout.write_all(text.as_bytes());
                0
            };
        }
    };
    let result = match &cli.command {
        Command::Volumes(args) => volumes(args, runner, stdout),
        Command::Decide(args) => decide_one(args, runner, stdout),
        Command::Capabilities(args) => capabilities(args, stdout),
    };
    match result {
        Ok(()) => 0,
        Err(failure) => {
            let _ = writeln!(stderr, "{failure}");
            failure.exit_status()
        }
    }
}

fn read(path: &Path) -> Result<String, Failure> {
    fs::read_to_string(path).map_err(|error| Failure::Coded {
        code: StorageCode::ProbeInputInvalid,
        detail: format!("cannot read {}: {error}", path.display()),
    })
}

fn policy_from(input: &InputArgs) -> PolicyOverrides {
    PolicyOverrides {
        unsafe_ntfs: match input.ntfs_unsafe {
            UnsafeNtfsArg::ReadOnly => UnsafeNtfsPolicy::ReadOnly,
            UnsafeNtfsArg::Refuse => UnsafeNtfsPolicy::Refuse,
        },
        hfsplus_write: input.hfsplus_write,
        apfs_read_only: input.apfs_read_only,
        noexec_removable: input.noexec_removable,
    }
}

/// Where the block devices of a request come from, for messages.
fn source_name(input: &InputArgs) -> String {
    match &input.from {
        Some(path) => path.display().to_string(),
        None => "this machine".to_owned(),
    }
}

/// The probe a request asks for: a file captured earlier, or this machine.
fn probe_for(input: &InputArgs, runner: &dyn CommandRunner) -> Result<Box<dyn Probe>, Failure> {
    let invalid = |source: &str, error: &dyn fmt::Display| Failure::Coded {
        code: StorageCode::ProbeInputInvalid,
        detail: format!("{source}: {error}"),
    };
    let source = source_name(input);
    // The properties file is read after the block devices, as it always was.
    let properties = || -> Result<Option<(String, String)>, Failure> {
        match &input.props {
            Some(path) => Ok(Some((path.display().to_string(), read(path)?))),
            None => Ok(None),
        }
    };
    match &input.from {
        Some(path) => {
            let mut probe = SnapshotProbe::from_lsblk_json(&read(path)?)
                .map_err(|error| invalid(&source, &error))?;
            if let Some((name, text)) = properties()? {
                probe = probe
                    .with_properties(&text)
                    .map_err(|error| invalid(&name, &error))?;
            }
            Ok(Box::new(probe))
        }
        None => {
            // `--live`: the command line requires one of the two.
            let mut probe =
                LiveProbe::capture(runner).map_err(|error| invalid(&source, &error))?;
            if let Some((name, text)) = properties()? {
                probe = probe
                    .with_properties(&text)
                    .map_err(|error| invalid(&name, &error))?;
            }
            Ok(Box::new(probe))
        }
    }
}

/// Reads the input and plans the whole inventory for one request.
fn load_plan(
    input: &InputArgs,
    access: Access,
    runner: &dyn CommandRunner,
) -> Result<(MountPlan, PolicyOverrides), Failure> {
    let probe = probe_for(input, runner)?;
    let inventory = Inventory::from_probe(probe.as_ref()).map_err(|error| Failure::Coded {
        code: StorageCode::ProbeInputInvalid,
        detail: format!("{}: {error}", source_name(input)),
    })?;

    let user = match &input.user {
        Some(name) => Some(UserName::parse(name).ok_or_else(|| {
            Failure::Usage(format!(
                "\"{}\" is not a usable login name",
                printable(name)
            ))
        })?),
        None => None,
    };
    let context = NamespaceContext {
        user,
        ..NamespaceContext::default()
    };
    let policy = policy_from(input);
    let request = MountRequest {
        access,
        uid: input.uid,
        gid: input.gid,
    };
    let plan = plan(&inventory, &request, &policy, &context);
    plan.verify().map_err(Failure::Internal)?;
    Ok((plan, policy))
}

fn emit(out: &mut dyn Write, text: &str) -> Result<(), Failure> {
    out.write_all(text.as_bytes())
        .map_err(|error| Failure::Output(error.to_string()))
}

fn emit_json<T: Serialize>(out: &mut dyn Write, report: &T) -> Result<(), Failure> {
    let mut json = serde_json::to_string_pretty(report)
        .map_err(|error| Failure::Internal(format!("cannot serialise the report: {error}")))?;
    json.push('\n');
    emit(out, &json)
}

fn location(planned: &PlannedVolume) -> String {
    let volume = &planned.volume;
    let mut parts: Vec<&str> = vec![if volume.removable {
        "removable"
    } else {
        "internal"
    }];
    if volume.optical {
        parts.push("optical");
    }
    if volume.device_read_only {
        parts.push("write-protected");
    }
    if volume.system {
        parts.push("system");
    }
    if volume.duplicate_identity {
        parts.push("duplicate identity");
    }
    parts.join(", ")
}

fn describe(planned: &PlannedVolume) -> String {
    let volume = &planned.volume;
    let devices: Vec<String> = volume
        .devices
        .iter()
        .map(|device| printable(device))
        .collect();
    let signature = volume
        .signature
        .as_deref()
        .map_or("none".to_owned(), printable);
    let mut text = format!(
        "{id}\n  Name:        {name}\n  Filesystem:  {filesystem} (signature: {signature})\n  \
         Devices:     {devices}\n  Size:        {size} bytes\n  Location:    {location}\n  \
         Encryption:  {encryption}\n  Decision:    {outcome} ({code})\n  Reason:      {reason}\n",
        id = volume.id,
        name = planned.display_name,
        filesystem = volume.filesystem,
        devices = devices.join(", "),
        size = volume.size_bytes,
        location = location(planned),
        encryption = volume.encryption,
        outcome = planned.decision.outcome,
        code = planned.decision.code,
        reason = planned.decision.reason,
    );
    if planned.decision.is_mount() {
        text.push_str(&format!("  Mount path:  {}\n", planned.canonical_path));
        text.push_str(&format!(
            "  Options:     {}\n",
            planned.decision.options.join(",")
        ));
    }
    for view in &planned.views {
        let mut notes: Vec<&str> = vec![view.kind.as_str()];
        if view.read_only {
            notes.push("read-only");
        }
        if view.requires_grant {
            notes.push("needs a grant");
        }
        text.push_str(&format!(
            "  View:        {} {} ({})\n",
            view.runtime,
            view.path,
            notes.join(", ")
        ));
    }
    text
}

fn volumes(
    args: &VolumesArgs,
    runner: &dyn CommandRunner,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let (plan, policy) = load_plan(&args.input, Access::ReadWrite, runner)?;
    if args.input.json {
        return emit_json(
            out,
            &InventoryReport {
                schema_version: SCHEMA_VERSION,
                kind: "inventory",
                policy,
                volumes: &plan.volumes,
            },
        );
    }
    if plan.volumes.is_empty() {
        return emit(out, "No volumes found.\n");
    }
    let blocks: Vec<String> = plan.volumes.iter().map(describe).collect();
    emit(out, &blocks.join("\n"))
}

fn decide_one(
    args: &DecideArgs,
    runner: &dyn CommandRunner,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let access = match args.access {
        AccessArg::ReadOnly => Access::ReadOnly,
        AccessArg::ReadWrite => Access::ReadWrite,
    };
    let (plan, policy) = load_plan(&args.input, access, runner)?;
    let planned = plan.find(&args.volume).ok_or_else(|| Failure::Coded {
        code: StorageCode::NoSuchVolume,
        detail: format!(
            "volume \"{}\" is not in {}",
            printable(&args.volume),
            source_name(&args.input)
        ),
    })?;
    if args.input.json {
        return emit_json(
            out,
            &DecisionReport {
                schema_version: SCHEMA_VERSION,
                kind: "decision",
                policy,
                volume: planned,
            },
        );
    }
    let mut text = planned.decision.render();
    text.push_str(&format!("\nVolume: {}\n", planned.volume.id));
    if planned.decision.is_mount() {
        text.push_str(&format!("Mount path: {}\n", planned.canonical_path));
    }
    emit(out, &text)
}

fn parse_filesystem(name: &str) -> Result<Filesystem, Failure> {
    Filesystem::from_name(name).ok_or_else(|| {
        let known: Vec<&str> = Filesystem::all()
            .iter()
            .map(|filesystem| filesystem.as_str())
            .collect();
        Failure::Usage(format!(
            "\"{}\" is not a known filesystem; known: {}",
            printable(name),
            known.join(", ")
        ))
    })
}

fn capabilities(args: &CapabilitiesArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let source = parse_filesystem(&args.from_fs)?;
    let destination = parse_filesystem(&args.to_fs)?;
    let (assessment, losses) = match copy_losses(source, destination) {
        Ok(losses) => (CapabilityAssessment::Complete, losses),
        Err(assessment) => (assessment, Vec::new()),
    };
    if args.json {
        return emit_json(
            out,
            &CapabilityReport {
                schema_version: SCHEMA_VERSION,
                kind: "capabilities",
                source,
                destination,
                assessment,
                losses,
            },
        );
    }
    let from_name = policy_for(source).spec_name;
    let to_name = policy_for(destination).spec_name;
    let mut text = format!("Copy from {from_name} to {to_name}\nAssessment: {assessment}\n");
    match assessment {
        CapabilityAssessment::Complete if losses.is_empty() => {
            text.push_str("Nothing recorded in the registry would be lost.\n");
        }
        CapabilityAssessment::Complete => {
            text.push_str("Not preserved in full:\n");
            for loss in &losses {
                text.push_str(&format!(
                    "  {}: {} (source {}, destination {})\n",
                    loss.feature, loss.kind, loss.source, loss.destination
                ));
            }
        }
        CapabilityAssessment::DestinationNotWritable => {
            text.push_str(&format!("TUR never writes to {to_name}.\n"));
        }
        CapabilityAssessment::NoData => {
            text.push_str(
                "The registry has no data for one of these filesystems, so what would be lost \
                 is not known.\n",
            );
        }
    }
    emit(out, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(args: &[&str]) -> (u8, String, String) {
        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let mut full = vec!["tur-storage"];
        full.extend_from_slice(args);
        let status = run(full, &mut stdout, &mut stderr);
        (
            status,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn help_and_version_succeed_on_stdout() {
        let (status, stdout, stderr) = run_with(&["--help"]);
        assert_eq!(status, 0);
        assert!(
            stdout.contains("volumes") && stdout.contains("decide"),
            "{stdout}"
        );
        assert!(stderr.is_empty());
        let (status, stdout, _) = run_with(&["--version"]);
        assert_eq!(status, 0);
        assert!(stdout.starts_with("tur-storage "), "{stdout}");
    }

    #[test]
    fn usage_errors_exit_with_status_two() {
        for args in [
            vec![],
            vec!["volumes"],
            vec!["decide", "--from", "x.json"],
            vec![
                "decide", "--from", "x.json", "--volume", "fs-1", "--access", "write",
            ],
            vec!["capabilities", "--from-fs", "ntfs"],
            vec!["format", "--device", "/dev/sda"],
        ] {
            let (status, stdout, stderr) = run_with(&args);
            assert_eq!(status, 2, "{args:?}");
            assert!(stdout.is_empty(), "{args:?}");
            assert!(!stderr.is_empty(), "{args:?}");
        }
    }

    /// A runner that runs nothing: it records what it was asked to run and
    /// answers as `lsblk` would for one disk with one ext4 partition.
    struct FakeLsblk {
        asked: std::cell::RefCell<Vec<String>>,
        status: Option<i32>,
    }

    impl FakeLsblk {
        fn new() -> FakeLsblk {
            FakeLsblk {
                asked: std::cell::RefCell::new(Vec::new()),
                status: Some(0),
            }
        }
    }

    const LIVE_DOCUMENT: &str = r#"{"blockdevices":[
        {"name":"sdb","path":"/dev/sdb","type":"disk","size":8000000000,"rm":true,
         "tran":"usb",
         "children":[{"name":"sdb1","path":"/dev/sdb1","type":"part","size":7999000000,
                      "fstype":"ext4","label":"Backup",
                      "uuid":"0a0a0a0a-1111-4bbb-8ccc-000000000b01","rm":true}]}
    ]}"#;

    impl CommandRunner for FakeLsblk {
        fn run(
            &self,
            program: &str,
            arguments: &[&str],
        ) -> Result<crate::probe::CommandOutput, String> {
            self.asked
                .borrow_mut()
                .push(format!("{program} {}", arguments.join(" ")));
            Ok(crate::probe::CommandOutput {
                status: self.status,
                stdout: LIVE_DOCUMENT.as_bytes().to_vec(),
                stderr: b"lsblk: failed".to_vec(),
            })
        }
    }

    fn run_live(runner: &FakeLsblk, args: &[&str]) -> (u8, String, String) {
        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let mut full = vec!["tur-storage"];
        full.extend_from_slice(args);
        let status = run_with_runner(full, runner, &mut stdout, &mut stderr);
        (
            status,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn live_mode_lists_the_volumes_of_this_machine_by_running_lsblk_only() {
        let runner = FakeLsblk::new();
        let (status, stdout, stderr) = run_live(&runner, &["volumes", "--live", "--json"]);
        assert_eq!(status, 0, "{stderr}");
        // Exactly one program was run, the one that lists block devices.
        assert_eq!(
            *runner.asked.borrow(),
            ["/usr/bin/lsblk --json --bytes --output-all"]
        );
        let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(value["kind"], "inventory");
        let volumes = value["volumes"].as_array().unwrap();
        let volume = volumes
            .iter()
            .find(|volume| volume["filesystem"] == "ext4")
            .unwrap_or_else(|| panic!("the partition is not listed: {stdout}"));
        let id = volume["id"].as_str().unwrap().to_owned();

        // The same volume, previewed: one more run of the same program.
        let (status, stdout, stderr) = run_live(
            &runner,
            &[
                "decide",
                "--live",
                "--json",
                "--access=read-only",
                &format!("--volume={id}"),
            ],
        );
        assert_eq!(status, 0, "{stderr}");
        let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(value["kind"], "decision");
        assert_eq!(value["volume"]["id"], id.as_str());
        assert_eq!(value["volume"]["decision"]["requested_access"], "read-only");
        assert_eq!(
            *runner.asked.borrow(),
            [
                "/usr/bin/lsblk --json --bytes --output-all",
                "/usr/bin/lsblk --json --bytes --output-all"
            ]
        );

        // A volume this machine does not have is said to be missing from
        // this machine, not from a file.
        let (status, _, stderr) =
            run_live(&runner, &["decide", "--live", "--volume", "fs-nowhere"]);
        assert_eq!(status, 1);
        assert!(stderr.starts_with("TUR-STORAGE-VOLUME-001\n"), "{stderr}");
        assert!(stderr.contains("is not in this machine"), "{stderr}");
    }

    #[test]
    fn a_file_and_this_machine_are_two_sources_and_one_is_required() {
        // With a file, nothing is run.
        let runner = FakeLsblk::new();
        let (status, _, _) = run_live(&runner, &["volumes", "--from", "/nonexistent.json"]);
        assert_eq!(status, 1);
        assert!(runner.asked.borrow().is_empty());
        // Both, or neither, is a usage error, and nothing is run.
        for args in [
            vec!["volumes", "--live", "--from", "x.json"],
            vec!["volumes"],
            vec!["decide", "--volume", "fs-1"],
            vec!["volumes", "--live=yes"],
        ] {
            let (status, stdout, stderr) = run_live(&runner, &args);
            assert_eq!(status, 2, "{args:?}");
            assert!(stdout.is_empty() && !stderr.is_empty(), "{args:?}");
            assert!(runner.asked.borrow().is_empty(), "{args:?}");
        }
    }

    #[test]
    fn a_live_probe_that_fails_is_reported_with_a_code_and_nothing_is_listed() {
        let runner = FakeLsblk {
            status: Some(32),
            ..FakeLsblk::new()
        };
        let (status, stdout, stderr) = run_live(&runner, &["volumes", "--live", "--json"]);
        assert_eq!(status, 1);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("TUR-STORAGE-PROBE-001\n"), "{stderr}");
        assert!(stderr.contains("this machine: live probe: /usr/bin/lsblk ended with status 32"));
    }

    #[test]
    fn access_has_one_spelling_and_the_short_forms_are_still_read() {
        let runner = FakeLsblk::new();
        let (_, listed, _) = run_live(&runner, &["volumes", "--live", "--json"]);
        let listed: serde_json::Value = serde_json::from_str(&listed).unwrap();
        let id = listed["volumes"][0]["id"].as_str().unwrap().to_owned();
        let requested = |access: &[&str]| {
            let mut args = vec!["decide", "--live", "--json", "--volume", id.as_str()];
            args.extend_from_slice(access);
            let (status, stdout, stderr) = run_live(&runner, &args);
            assert_eq!(status, 0, "{access:?}: {stderr}");
            let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
            value["volume"]["decision"]["requested_access"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        // The spelling of the manifest and of the JSON output.
        assert_eq!(requested(&["--access", "read-only"]), "read-only");
        assert_eq!(requested(&["--access=read-write"]), "read-write");
        // The former spelling means the same.
        assert_eq!(requested(&["--access", "ro"]), "read-only");
        assert_eq!(requested(&["--access", "rw"]), "read-write");
        // Without the option, read and write is asked for, as before.
        assert_eq!(requested(&[]), "read-write");
        // The help names the one spelling.
        let (status, help, _) = run_with(&["decide", "--help"]);
        assert_eq!(status, 0);
        assert!(help.contains("read-only") && help.contains("read-write"), "{help}");
        assert!(help.contains("--live"), "{help}");
        // Anything else is refused.
        for access in ["write", "readonly", "read_only", "RO", ""] {
            let (status, _, _) = run_with(&[
                "decide", "--from", "x.json", "--volume", "fs-1", "--access", access,
            ]);
            assert_eq!(status, 2, "{access:?}");
        }
    }

    #[test]
    fn there_is_no_option_that_overrides_a_safety_check() {
        for flag in [
            "--force",
            "--apfs-write",
            "--apfs-read-write",
            "--enable-apfs-write",
            "--experimental",
            "--remove-hiberfile",
            "--recover",
            "--ntfs-unsafe=read-write",
            "--ntfs-unsafe=rw",
        ] {
            let (status, _, stderr) =
                run_with(&["decide", "--from", "x.json", "--volume", "fs-1", flag]);
            assert_eq!(status, 2, "{flag} was accepted");
            assert!(!stderr.is_empty());
        }
    }

    #[test]
    fn a_missing_input_file_is_reported_with_a_code() {
        let (status, stdout, stderr) =
            run_with(&["volumes", "--from", "/nonexistent/tur-storage/lsblk.json"]);
        assert_eq!(status, 1);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("TUR-STORAGE-PROBE-001\n"), "{stderr}");
        assert!(stderr.contains("/nonexistent/tur-storage/lsblk.json"));
    }

    #[test]
    fn capabilities_lists_losses_as_text() {
        let (status, stdout, stderr) =
            run_with(&["capabilities", "--from-fs", "ntfs", "--to-fs", "fat32"]);
        assert_eq!(status, 0, "{stderr}");
        assert!(stdout.starts_with("Copy from NTFS to FAT32/VFAT\nAssessment: complete\n"));
        assert!(stdout.contains("  alternate-data-streams: lost (source yes, destination no)\n"));
        assert!(stdout.contains("  windows-security-descriptors: lost"));
    }

    #[test]
    fn capabilities_says_when_it_cannot_assess() {
        let (status, stdout, _) =
            run_with(&["capabilities", "--from-fs", "ext4", "--to-fs", "apfs"]);
        assert_eq!(status, 0);
        assert!(stdout.contains("Assessment: destination-not-writable\n"));
        assert!(stdout.contains("TUR never writes to APFS."));

        let (status, stdout, _) =
            run_with(&["capabilities", "--from-fs", "refs", "--to-fs", "ext4"]);
        assert_eq!(status, 0);
        assert!(stdout.contains("Assessment: no-data\n"));

        let (status, stdout, _) =
            run_with(&["capabilities", "--from-fs", "ext4", "--to-fs", "ext4"]);
        assert_eq!(status, 0);
        assert!(stdout.contains("Nothing recorded in the registry would be lost."));
    }

    #[test]
    fn capabilities_rejects_unknown_filesystem_names() {
        let (status, stdout, stderr) =
            run_with(&["capabilities", "--from-fs", "nonsense", "--to-fs", "ext4"]);
        assert_eq!(status, 2);
        assert!(stdout.is_empty());
        assert!(
            stderr.contains("\"nonsense\" is not a known filesystem"),
            "{stderr}"
        );
    }

    #[test]
    fn capabilities_json_has_the_documented_shape() {
        let (status, stdout, _) = run_with(&[
            "capabilities",
            "--from-fs",
            "btrfs",
            "--to-fs",
            "ext4",
            "--json",
        ]);
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["kind"], "capabilities");
        assert_eq!(value["source"], "btrfs");
        assert_eq!(value["destination"], "ext4");
        assert_eq!(value["assessment"], "complete");
        assert_eq!(value["losses"].as_array().unwrap().len(), 3);
        assert_eq!(value["losses"][0]["feature"], "compression");
        assert_eq!(value["losses"][0]["kind"], "lost");
    }

    #[test]
    fn failures_render_with_their_registry_text() {
        let failure = Failure::Coded {
            code: StorageCode::NoSuchVolume,
            detail: "volume \"fs-x\" is not in a.json".to_owned(),
        };
        let text = failure.to_string();
        assert!(text.starts_with("TUR-STORAGE-VOLUME-001\nNo volume with that identifier"));
        assert!(text.contains("List the volumes"));
        assert_eq!(failure.exit_status(), 1);
        assert_eq!(Failure::Usage("x".to_owned()).exit_status(), 2);
    }
}
