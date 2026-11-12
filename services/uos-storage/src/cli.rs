// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The `uos-storage` command line.
//!
//! Three read-only commands over captured data:
//!
//! * `volumes --from <lsblk.json>`: the inventory, with the decision and the
//!   runtime views for each volume.
//! * `decide --from <lsblk.json> --volume <id> [--access ro|rw]`: the decision
//!   for one volume.
//! * `capabilities --from-fs <fs> --to-fs <fs>`: what a copy between two
//!   filesystems would lose.
//!
//! Input comes from files named on the command line and from nowhere else:
//! the tool does not look at devices, does not run other programs and takes
//! no setting from the process surroundings. Exit status: 0 on success, 1
//! when the input is unusable or the volume is unknown, 2 for a usage error.
//! `decide` exits 0 whatever the decision is; the decision is the output.

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
use crate::probe::SnapshotProbe;
use crate::volume::Inventory;

/// The parsed command line.
#[derive(Debug, Parser)]
#[command(
    name = "uos-storage",
    version,
    about = "UOS storage policy core: lists volumes and previews mount decisions from captured \
             data. It never touches a device."
)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List the volumes in an lsblk snapshot, with the decision for each
    Volumes(VolumesArgs),
    /// Show the mount decision for one volume
    Decide(DecideArgs),
    /// List the metadata that a copy between two filesystems would lose
    Capabilities(CapabilitiesArgs),
}

/// Access requested on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AccessArg {
    /// Read-only
    Ro,
    /// Read and write
    Rw,
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
    #[arg(long, value_name = "FILE")]
    pub from: PathBuf,
    /// File holding `blkid --output export` style properties, including the
    /// UOS_NTFS_STATE and UOS_HFS_JOURNAL facts
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
    /// UOS volume identifier, as printed by `volumes`
    #[arg(long, value_name = "ID")]
    pub volume: String,
    /// Access to ask for
    #[arg(long, value_enum, value_name = "ACCESS", default_value = "rw")]
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
/// and failures go to `stderr`.
pub fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8
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
        Command::Volumes(args) => volumes(args, stdout),
        Command::Decide(args) => decide_one(args, stdout),
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

/// Reads the input files and plans the whole inventory for one request.
fn load_plan(input: &InputArgs, access: Access) -> Result<(MountPlan, PolicyOverrides), Failure> {
    let invalid = |path: &Path, error: &dyn fmt::Display| Failure::Coded {
        code: StorageCode::ProbeInputInvalid,
        detail: format!("{}: {error}", path.display()),
    };
    let mut probe = SnapshotProbe::from_lsblk_json(&read(&input.from)?)
        .map_err(|error| invalid(&input.from, &error))?;
    if let Some(path) = &input.props {
        probe = probe.with_properties(&read(path)?).map_err(|error| invalid(path, &error))?;
    }
    let inventory =
        Inventory::from_probe(&probe).map_err(|error| invalid(&input.from, &error))?;

    let user = match &input.user {
        Some(name) => Some(UserName::parse(name).ok_or_else(|| {
            Failure::Usage(format!("\"{}\" is not a usable login name", printable(name)))
        })?),
        None => None,
    };
    let context = NamespaceContext { user, ..NamespaceContext::default() };
    let policy = policy_from(input);
    let request = MountRequest { access, uid: input.uid, gid: input.gid };
    let plan = plan(&inventory, &request, &policy, &context);
    plan.verify().map_err(Failure::Internal)?;
    Ok((plan, policy))
}

fn emit(out: &mut dyn Write, text: &str) -> Result<(), Failure> {
    out.write_all(text.as_bytes()).map_err(|error| Failure::Output(error.to_string()))
}

fn emit_json<T: Serialize>(out: &mut dyn Write, report: &T) -> Result<(), Failure> {
    let mut json = serde_json::to_string_pretty(report)
        .map_err(|error| Failure::Internal(format!("cannot serialise the report: {error}")))?;
    json.push('\n');
    emit(out, &json)
}

fn location(planned: &PlannedVolume) -> String {
    let volume = &planned.volume;
    let mut parts: Vec<&str> = vec![if volume.removable { "removable" } else { "internal" }];
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
    let devices: Vec<String> = volume.devices.iter().map(|device| printable(device)).collect();
    let signature = volume.signature.as_deref().map_or("none".to_owned(), printable);
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
        text.push_str(&format!("  Options:     {}\n", planned.decision.options.join(",")));
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

fn volumes(args: &VolumesArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let (plan, policy) = load_plan(&args.input, Access::ReadWrite)?;
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

fn decide_one(args: &DecideArgs, out: &mut dyn Write) -> Result<(), Failure> {
    let access = match args.access {
        AccessArg::Ro => Access::ReadOnly,
        AccessArg::Rw => Access::ReadWrite,
    };
    let (plan, policy) = load_plan(&args.input, access)?;
    let planned = plan.find(&args.volume).ok_or_else(|| Failure::Coded {
        code: StorageCode::NoSuchVolume,
        detail: format!("volume \"{}\" is not in {}", printable(&args.volume), args.input.from.display()),
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
        let known: Vec<&str> = Filesystem::all().iter().map(|filesystem| filesystem.as_str()).collect();
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
            text.push_str(&format!("UOS never writes to {to_name}.\n"));
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
        let mut full = vec!["uos-storage"];
        full.extend_from_slice(args);
        let status = run(full, &mut stdout, &mut stderr);
        (status, String::from_utf8(stdout).unwrap(), String::from_utf8(stderr).unwrap())
    }

    #[test]
    fn help_and_version_succeed_on_stdout() {
        let (status, stdout, stderr) = run_with(&["--help"]);
        assert_eq!(status, 0);
        assert!(stdout.contains("volumes") && stdout.contains("decide"), "{stdout}");
        assert!(stderr.is_empty());
        let (status, stdout, _) = run_with(&["--version"]);
        assert_eq!(status, 0);
        assert!(stdout.starts_with("uos-storage "), "{stdout}");
    }

    #[test]
    fn usage_errors_exit_with_status_two() {
        for args in [
            vec![],
            vec!["volumes"],
            vec!["decide", "--from", "x.json"],
            vec!["decide", "--from", "x.json", "--volume", "fs-1", "--access", "write"],
            vec!["capabilities", "--from-fs", "ntfs"],
            vec!["format", "--device", "/dev/sda"],
        ] {
            let (status, stdout, stderr) = run_with(&args);
            assert_eq!(status, 2, "{args:?}");
            assert!(stdout.is_empty(), "{args:?}");
            assert!(!stderr.is_empty(), "{args:?}");
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
            run_with(&["volumes", "--from", "/nonexistent/uos-storage/lsblk.json"]);
        assert_eq!(status, 1);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("UOS-STORAGE-PROBE-001\n"), "{stderr}");
        assert!(stderr.contains("/nonexistent/uos-storage/lsblk.json"));
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
        let (status, stdout, _) = run_with(&["capabilities", "--from-fs", "ext4", "--to-fs", "apfs"]);
        assert_eq!(status, 0);
        assert!(stdout.contains("Assessment: destination-not-writable\n"));
        assert!(stdout.contains("UOS never writes to APFS."));

        let (status, stdout, _) = run_with(&["capabilities", "--from-fs", "refs", "--to-fs", "ext4"]);
        assert_eq!(status, 0);
        assert!(stdout.contains("Assessment: no-data\n"));

        let (status, stdout, _) = run_with(&["capabilities", "--from-fs", "ext4", "--to-fs", "ext4"]);
        assert_eq!(status, 0);
        assert!(stdout.contains("Nothing recorded in the registry would be lost."));
    }

    #[test]
    fn capabilities_rejects_unknown_filesystem_names() {
        let (status, stdout, stderr) =
            run_with(&["capabilities", "--from-fs", "nonsense", "--to-fs", "ext4"]);
        assert_eq!(status, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("\"nonsense\" is not a known filesystem"), "{stderr}");
    }

    #[test]
    fn capabilities_json_has_the_documented_shape() {
        let (status, stdout, _) =
            run_with(&["capabilities", "--from-fs", "btrfs", "--to-fs", "ext4", "--json"]);
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
        assert!(text.starts_with("UOS-STORAGE-VOLUME-001\nNo volume with that identifier"));
        assert!(text.contains("List the volumes"));
        assert_eq!(failure.exit_status(), 1);
        assert_eq!(Failure::Usage("x".to_owned()).exit_status(), 2);
    }
}
