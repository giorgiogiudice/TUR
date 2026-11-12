// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The dry-run command line behind the `uos-installer` binary.
//!
//! ```text
//! uos-installer inventory --from <lsblk.json> [--links <links.json>] [--json]
//! uos-installer plan --from <lsblk.json> --target <by-id path or identity>
//!                    [--links <links.json>] [--facts <facts.json>]
//!                    [--encrypt] [--data-partition]
//!                    [--acknowledge-weak-identity] [--json]
//! uos-installer session show <dir> [--json]
//! ```
//!
//! Every subcommand takes its input from files. None of them inspects or
//! changes a real disk, and none of them writes a file.
//!
//! Exit codes: `0` success; `1` a typed installer error, printed with its
//! code; `2` a usage error; `3` a plan was produced but a preflight check
//! blocks installation.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::codes::{Fault, FaultReport};
use crate::identity::{TargetSelector, WeakIdentityPolicy, select_target};
use crate::inventory::{BlockDevice, FileProbe, Inventory, read_json_file};
use crate::phase::{PhaseRecord, PhaseTable};
use crate::plan::{
    ERASE_DISK_SCHEME, InstallMode, LayoutOptions, Plan, PlanRequest, TargetRefusal, build_plan,
    describe_partition, target_refusal,
};
use crate::preflight::{
    CheckStatus, EnvironmentFacts, HostFacts, PreflightReport, Requirements, TargetFacts,
    run_preflight,
};
use crate::session::{Anomaly, OsFileSystem, ResumeDecision, SessionMeta, load};
use crate::units::{format_size, format_size_exact};

/// Exit code: success.
pub const EXIT_OK: u8 = 0;
/// Exit code: a typed installer error.
pub const EXIT_FAULT: u8 = 1;
/// Exit code: the command line could not be understood.
pub const EXIT_USAGE: u8 = 2;
/// Exit code: a plan was produced, but a preflight check blocks installation.
pub const EXIT_PREFLIGHT_BLOCKED: u8 = 3;

const DRY_RUN_NOTICE: &str = "UOS installer dry run. Nothing is written to any disk.";

/// Dry-run tools for the UOS installer core. Every subcommand reads files;
/// none touches a disk.
#[derive(Debug, Parser)]
#[command(name = "uos-installer", version)]
struct Cli {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// List the disks described by saved lsblk output.
    Inventory(InventoryArgs),
    /// Show the plan, preflight results and consequences of erasing a disk.
    Plan(PlanArgs),
    /// Inspect a session directory.
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
}

#[derive(Debug, Subcommand)]
enum SessionAction {
    /// Show a session's phases and whether it may be resumed.
    Show(ShowArgs),
}

#[derive(Debug, Args)]
struct InventoryArgs {
    /// Saved output of `lsblk --json --bytes --output-all`.
    #[arg(long = "from", value_name = "LSBLK_JSON")]
    lsblk: PathBuf,
    /// JSON file mapping /dev/disk/by-id and by-path link names to devices.
    #[arg(long, value_name = "LINKS_JSON")]
    links: Option<PathBuf>,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct PlanArgs {
    /// Saved output of `lsblk --json --bytes --output-all`.
    #[arg(long = "from", value_name = "LSBLK_JSON")]
    lsblk: PathBuf,
    /// JSON file mapping /dev/disk/by-id and by-path link names to devices.
    #[arg(long, value_name = "LINKS_JSON")]
    links: Option<PathBuf>,
    /// The disk to plan for: a /dev/disk/by-id path, or identity attributes
    /// such as serial=<serial>. A device path such as /dev/sda is refused.
    #[arg(long, value_name = "TARGET")]
    target: String,
    /// JSON file of host facts (boot mode, Secure Boot, CPU, memory). Facts
    /// that are not given count as not determined.
    #[arg(long, value_name = "FACTS_JSON")]
    facts: Option<PathBuf>,
    /// Encrypt the system and user data with LUKS2.
    #[arg(long)]
    encrypt: bool,
    /// Create a separate user-data partition.
    #[arg(long)]
    data_partition: bool,
    /// Accept a disk that reports no serial number or WWN.
    #[arg(long)]
    acknowledge_weak_identity: bool,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ShowArgs {
    /// The session directory.
    #[arg(value_name = "DIR")]
    directory: PathBuf,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

/// What a subcommand produced.
struct Output {
    stdout: String,
    stderr: String,
    code: u8,
}

impl Output {
    fn text(lines: &[String], code: u8) -> Output {
        Output {
            stdout: format!("{}\n", lines.join("\n")),
            stderr: String::new(),
            code,
        }
    }

    fn json<T: Serialize>(value: &T, code: u8) -> Output {
        match serde_json::to_string_pretty(value) {
            Ok(text) => Output {
                stdout: format!("{text}\n"),
                stderr: String::new(),
                code,
            },
            Err(error) => Output {
                stdout: String::new(),
                stderr: format!("error: the output could not be serialised: {error}\n"),
                code: EXIT_FAULT,
            },
        }
    }

    fn fault(fault: &dyn Fault) -> Output {
        Output {
            stdout: String::new(),
            stderr: render_fault(&FaultReport::of(fault)),
            code: EXIT_FAULT,
        }
    }
}

fn render_fault(report: &FaultReport) -> String {
    format!(
        "error: {}\n{}\n\nTechnical detail:\n{}\n",
        report.code, report.message, report.technical_detail
    )
}

/// Runs the command line and returns the process exit code.
///
/// `arguments` includes the program name, as `std::env::args_os` yields it.
pub fn run<I, T>(arguments: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let output = match Cli::try_parse_from(arguments) {
        Ok(cli) => match &cli.action {
            Action::Inventory(arguments) => inventory(arguments),
            Action::Plan(arguments) => plan(arguments),
            Action::Session {
                action: SessionAction::Show(arguments),
            } => session_show(arguments),
        },
        Err(error) => {
            let text = error.to_string();
            if error.use_stderr() {
                Output {
                    stdout: String::new(),
                    stderr: text,
                    code: EXIT_USAGE,
                }
            } else {
                Output {
                    stdout: text,
                    stderr: String::new(),
                    code: EXIT_OK,
                }
            }
        }
    };
    let written = stdout
        .write_all(output.stdout.as_bytes())
        .and_then(|()| stderr.write_all(output.stderr.as_bytes()));
    match written {
        Ok(()) => output.code,
        Err(_) => EXIT_FAULT,
    }
}

fn inventory(arguments: &InventoryArgs) -> Output {
    let probe = FileProbe::new(&arguments.lsblk, arguments.links.as_deref());
    let inventory = match Inventory::probe(&probe) {
        Ok(inventory) => inventory,
        Err(error) => return Output::fault(&error),
    };
    if arguments.json {
        Output::json(&inventory, EXIT_OK)
    } else {
        Output::text(&render_inventory(&inventory), EXIT_OK)
    }
}

/// The JSON printed by `plan --json`.
#[derive(Serialize)]
struct PlanOutput<'a> {
    dry_run: bool,
    preflight: Option<&'a PreflightReport>,
    plan: Option<&'a Plan>,
    refusal: Option<FaultReport>,
}

fn plan(arguments: &PlanArgs) -> Output {
    let probe = FileProbe::new(&arguments.lsblk, arguments.links.as_deref());
    let inventory = match Inventory::probe(&probe) {
        Ok(inventory) => inventory,
        Err(error) => return Output::fault(&error),
    };
    let environment = match &arguments.facts {
        Some(path) => match read_json_file::<EnvironmentFacts>(path) {
            Ok(facts) => facts,
            Err(error) => return Output::fault(&error),
        },
        None => EnvironmentFacts::default(),
    };
    let options = LayoutOptions {
        encrypt: arguments.encrypt,
        data_partition: arguments.data_partition,
    };
    let policy = if arguments.acknowledge_weak_identity {
        WeakIdentityPolicy::Acknowledged
    } else {
        WeakIdentityPolicy::Refuse
    };

    let selected = arguments
        .target
        .parse::<TargetSelector>()
        .and_then(|selector| select_target(&inventory, &selector, policy));
    let target = match selected {
        Ok(target) => target,
        Err(error) => return plan_refused(arguments.json, None, &error),
    };
    let requirements = match Requirements::for_layout(&ERASE_DISK_SCHEME, options) {
        Ok(requirements) => requirements,
        Err(error) => return plan_refused(arguments.json, None, &error),
    };
    let facts = HostFacts {
        environment,
        target: Some(TargetFacts::of(target.device())),
    };
    let preflight = run_preflight(&facts, &requirements);
    let request = PlanRequest {
        mode: InstallMode::EraseDisk,
        options,
    };
    let plan = match build_plan(&target, &request, &ERASE_DISK_SCHEME) {
        Ok(plan) => plan,
        Err(error) => return plan_refused(arguments.json, Some(&preflight), &error),
    };
    let code = if preflight.has_blocking() {
        EXIT_PREFLIGHT_BLOCKED
    } else {
        EXIT_OK
    };
    if arguments.json {
        let output = PlanOutput {
            dry_run: true,
            preflight: Some(&preflight),
            plan: Some(&plan),
            refusal: None,
        };
        Output::json(&output, code)
    } else {
        let mut lines = vec![DRY_RUN_NOTICE.to_owned(), String::new()];
        lines.extend(render_preflight(&preflight));
        lines.push(String::new());
        lines.extend(render_plan(&plan));
        Output::text(&lines, code)
    }
}

/// The output of `plan` when no plan could be made.
fn plan_refused(json: bool, preflight: Option<&PreflightReport>, fault: &dyn Fault) -> Output {
    let report = FaultReport::of(fault);
    let stderr = render_fault(&report);
    let mut output = if json {
        let value = PlanOutput {
            dry_run: true,
            preflight,
            plan: None,
            refusal: Some(report),
        };
        Output::json(&value, EXIT_FAULT)
    } else {
        let mut lines = vec![DRY_RUN_NOTICE.to_owned(), String::new()];
        if let Some(preflight) = preflight {
            lines.extend(render_preflight(preflight));
            lines.push(String::new());
        }
        lines.push("No plan was made. See the error below.".to_owned());
        Output::text(&lines, EXIT_FAULT)
    };
    output.stderr.push_str(&stderr);
    output.code = EXIT_FAULT;
    output
}

/// The JSON printed by `session show --json`.
#[derive(Serialize)]
struct SessionOutput<'a> {
    session: &'a SessionMeta,
    phases: &'a PhaseTable,
    decision: &'a ResumeDecision,
    anomalies: &'a [Anomaly],
    plan_readable: bool,
}

fn session_show(arguments: &ShowArgs) -> Output {
    let loaded = match load(&OsFileSystem, &arguments.directory) {
        Ok(loaded) => loaded,
        Err(error) => return Output::fault(&error),
    };
    if arguments.json {
        let output = SessionOutput {
            session: loaded.meta(),
            phases: loaded.phases(),
            decision: loaded.decision(),
            anomalies: loaded.anomalies(),
            plan_readable: loaded.plan().is_some(),
        };
        return Output::json(&output, EXIT_OK);
    }

    let meta = loaded.meta();
    let mut lines = vec![
        format!("Session {}", meta.session_id),
        format!("  Directory:          {}", loaded.directory().display()),
        format!("  Created:            {}", meta.created_at),
        format!("  Last written:       {}", meta.updated_at),
        format!("  Installer version:  {}", meta.installer_version),
    ];
    match &meta.mode {
        Some(mode) => lines.push(format!("  Mode:               {}", mode.label())),
        None => lines.push("  Mode:               not chosen yet".to_owned()),
    }
    lines.push(format!(
        "  Target written:     {}",
        if meta.target_writes_started {
            "possibly: a phase that writes to the target started"
        } else {
            "no"
        }
    ));
    lines.push(String::new());
    match &meta.target {
        Some(identity) => {
            lines.push("Target".to_owned());
            lines.extend(indent(&identity.display_lines()));
        }
        None => lines.push("Target: not chosen yet".to_owned()),
    }
    lines.push(String::new());
    lines.push("Phases".to_owned());
    lines.extend(loaded.phases().records().iter().map(render_phase));
    if !loaded.anomalies().is_empty() {
        lines.push(String::new());
        lines.push("Anomalies".to_owned());
        lines.extend(loaded.anomalies().iter().map(render_anomaly));
    }
    lines.push(String::new());
    lines.push("Decision".to_owned());
    lines.push(format!("  {}", loaded.decision().summary()));
    Output::text(&lines, EXIT_OK)
}

fn indent(lines: &[String]) -> Vec<String> {
    lines.iter().map(|line| format!("  {line}")).collect()
}

fn render_phase(record: &PhaseRecord) -> String {
    let phase = record.phase_id;
    let destructive = if phase.destructive() {
        "writes to target"
    } else {
        "-"
    };
    let code = match &record.error_code {
        Some(code) => format!(" [{code}]"),
        None => String::new(),
    };
    format!(
        "  {:<26}{:<11}{:<16}{:<18}{}{code}",
        phase.as_str(),
        record.state.as_str(),
        phase.class().as_str(),
        destructive,
        record.summary
    )
}

fn render_anomaly(anomaly: &Anomaly) -> String {
    match anomaly {
        Anomaly::UnreadableRecord { line, at_end: true } => format!(
            "  phases.jsonl line {line} is incomplete: the installer stopped while \
             writing it, and the record was never acknowledged"
        ),
        Anomaly::UnreadableRecord {
            line,
            at_end: false,
        } => format!("  phases.jsonl line {line} cannot be read"),
        Anomaly::SequenceGap {
            line,
            expected,
            found,
        } => format!(
            "  phases.jsonl line {line} has sequence number {found}, expected {expected}: \
             records are missing or out of order"
        ),
        Anomaly::LogBehindSnapshot {
            snapshot_records,
            log_records,
        } => format!(
            "  session.json reflects {snapshot_records} records but phases.jsonl holds \
             {log_records}: the end of the log is missing"
        ),
        Anomaly::UnreadablePlan { reason } => format!("  the plan cannot be used: {reason}"),
    }
}

fn render_preflight(report: &PreflightReport) -> Vec<String> {
    let mut lines = vec!["Preflight checks".to_owned()];
    for result in &report.results {
        let status = match result.status {
            CheckStatus::Pass => "[pass]    ",
            CheckStatus::Warning => "[warning] ",
            CheckStatus::Blocking => "[BLOCKING]",
        };
        lines.push(format!("  {status} {}  {}", result.code, result.message));
        if result.status != CheckStatus::Pass {
            lines.push(format!("             detail: {}", result.detail));
        }
    }
    let blocking = report.blocking().len();
    let warnings = report.warnings().len();
    let verdict = if blocking == 0 {
        "No check blocks installation."
    } else {
        "Installation would not be allowed to proceed."
    };
    lines.push(format!(
        "  Result: {blocking} blocking, {warnings} warnings. {verdict}"
    ));
    lines
}

fn render_plan(plan: &Plan) -> Vec<String> {
    let mut lines = vec!["Target".to_owned()];
    lines.extend(indent(&plan.target.identity.display_lines()));
    lines.push(format!(
        "  Kernel name:  {} when planned (not stable; never used to find the disk)",
        plan.target.device_path_when_planned
    ));
    if plan.target.weaker_identity_acknowledged {
        lines.push("  Identity:     weak; acknowledged explicitly".to_owned());
    }
    lines.push(String::new());

    lines.push(format!("Plan: {}", plan.mode.label()));
    let verified = if plan.scheme_verified {
        "verified"
    } else {
        "an unverified assumption; see docs/architecture/installer.md"
    };
    lines.push(format!("  Scheme:           {} ({verified})", plan.scheme));
    lines.push(format!(
        "  Partition table:  GPT, partitions aligned to {}",
        format_size(plan.alignment_bytes)
    ));
    lines.push("  New partitions:".to_owned());
    for partition in &plan.partitions {
        lines.push(format!("    {}", describe_partition(partition)));
    }
    lines.push(String::new());

    let consequences = &plan.consequences;
    lines.push("Consequences".to_owned());
    lines.push(format!("  {}", consequences.summary));
    lines.push(format!(
        "  Existing partition table: {}",
        consequences
            .existing_partition_table
            .as_deref()
            .unwrap_or("none recognised")
    ));
    if consequences.destroyed.is_empty() {
        lines.push("  Destroyed: no partitions or recognised filesystems".to_owned());
    } else {
        lines.push("  Destroyed:".to_owned());
        for item in &consequences.destroyed {
            lines.push(format!("    {}", item.description));
        }
    }
    if consequences.other_operating_systems.is_empty() {
        lines.push("  Other operating systems detected: none".to_owned());
    } else {
        for system in &consequences.other_operating_systems {
            lines.push(format!(
                "  Other operating system detected: {}",
                system.headline()
            ));
            for evidence in &system.evidence {
                lines.push(format!("    {evidence}"));
            }
        }
    }
    lines.push(format!(
        "  User data preserved: {}",
        if consequences.user_data_preserved {
            "yes"
        } else {
            "no"
        }
    ));
    lines.push(String::new());

    let confirmation = &plan.confirmation;
    lines.push(format!("Confirmation screen: {}", confirmation.heading));
    lines.push(format!("  {}", confirmation.encryption_notice));
    lines.push(format!("  {}", confirmation.privacy_notice));
    lines.push(format!("  {}", confirmation.phrase_instruction));
    lines
}

fn erase_status(device: &BlockDevice) -> &'static str {
    if !device.has_medium() {
        return "cannot be chosen: the drive holds no medium";
    }
    match target_refusal(device) {
        Some(TargetRefusal::LiveMedium) => "refused: this is the installation medium",
        Some(TargetRefusal::ReadOnly) => "refused: the disk is read-only",
        Some(TargetRefusal::InUse { .. }) => "refused: the disk is in use",
        None if device.serial.is_some() || device.wwn.is_some() => "can be chosen",
        None => "can be chosen only with an explicit weaker-identity acknowledgement",
    }
}

fn render_contents(device: &BlockDevice, depth: usize, lines: &mut Vec<String>) {
    for child in &device.children {
        let mounted = if child.mountpoints.is_empty() {
            String::new()
        } else {
            format!("  [in use: {}]", child.mountpoints.join(", "))
        };
        lines.push(format!(
            "{}{}  {}  {}{mounted}",
            "  ".repeat(depth),
            child.name,
            format_size(child.size_bytes),
            child.describe_contents()
        ));
        render_contents(child, depth + 1, lines);
    }
}

fn render_inventory(inventory: &Inventory) -> Vec<String> {
    let disks = inventory.disks();
    let mut lines = vec![format!(
        "Storage inventory: {} whole disks, {} other top-level devices",
        disks.len(),
        inventory.devices.len() - disks.len()
    )];
    for disk in disks {
        lines.push(String::new());
        lines.push(format!("Disk: {}", disk.display_model()));
        lines.push(format!("  Capacity:         {}", format_size_exact(disk.size_bytes)));
        lines.push(format!(
            "  Serial:           {}",
            disk.serial.as_deref().unwrap_or("not reported")
        ));
        lines.push(format!(
            "  WWN:              {}",
            disk.wwn.as_deref().unwrap_or("not reported")
        ));
        lines.push(format!("  Connection:       {}", disk.transport.label()));
        lines.push(format!(
            "  Stable path:      {}",
            disk.preferred_by_id().unwrap_or("none")
        ));
        lines.push(format!(
            "  Port:             {}",
            disk.by_path.first().map(String::as_str).unwrap_or("unknown")
        ));
        lines.push(format!(
            "  Sector size:      {} bytes logical, {} bytes physical",
            disk.logical_sector_size, disk.physical_sector_size
        ));
        lines.push(format!(
            "  Partition table:  {}",
            disk.partition_table.as_deref().unwrap_or("none recognised")
        ));
        lines.push(format!(
            "  Kernel name:      {} (not stable; never use it to choose a disk)",
            disk.kernel_name
        ));
        lines.push(format!("  Erase target:     {}", erase_status(disk)));
        for system in disk.detected_operating_systems() {
            lines.push(format!("  Detected system:  {}", system.headline()));
        }
        if disk.children.is_empty() {
            lines.push("  Contents:         no partitions".to_owned());
        } else {
            lines.push("  Contents:".to_owned());
            render_contents(disk, 2, &mut lines);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(arguments: &[&str]) -> (u8, String, String) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut full = vec!["uos-installer"];
        full.extend_from_slice(arguments);
        let code = run(full, &mut stdout, &mut stderr);
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    fn fixture(name: &str) -> String {
        format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn help_and_version_exit_zero_on_stdout() {
        let (code, stdout, stderr) = run_with(&["--help"]);
        assert_eq!(code, EXIT_OK);
        assert!(stdout.contains("inventory"));
        assert!(stdout.contains("plan"));
        assert!(stdout.contains("session"));
        assert!(stderr.is_empty());

        let (code, stdout, _) = run_with(&["--version"]);
        assert_eq!(code, EXIT_OK);
        assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn usage_errors_exit_two_on_stderr() {
        for arguments in [
            &[][..],
            &["format"][..],
            &["inventory"][..],
            &["plan", "--from", "x.json"][..],
            &["session"][..],
            &["session", "show"][..],
        ] {
            let (code, stdout, stderr) = run_with(arguments);
            assert_eq!(code, EXIT_USAGE, "{arguments:?}");
            assert!(stdout.is_empty(), "{arguments:?}");
            assert!(!stderr.is_empty(), "{arguments:?}");
        }
    }

    #[test]
    fn inventory_text_lists_identity_and_status() {
        let (code, stdout, stderr) = run_with(&[
            "inventory",
            "--from",
            &fixture("lsblk-workstation.json"),
            "--links",
            &fixture("links-workstation.json"),
        ]);
        assert_eq!(code, EXIT_OK, "{stderr}");
        for needle in [
            "Storage inventory: 4 whole disks, 2 other top-level devices",
            "Disk: Samsung SSD 980 PRO 1TB",
            "931.5 GiB (1000204886016 bytes)",
            "S5GXNX0T123456A",
            "/dev/disk/by-id/nvme-Samsung_SSD_980_PRO_1TB_S5GXNX0T123456A",
            "Detected system:  Windows (likely)",
            "refused: this is the installation medium",
            "cannot be chosen: the drive holds no medium",
            "not stable; never use it to choose a disk",
        ] {
            assert!(stdout.contains(needle), "missing \"{needle}\" in:\n{stdout}");
        }
    }

    #[test]
    fn a_missing_input_file_is_a_typed_error() {
        let (code, stdout, stderr) = run_with(&["inventory", "--from", "/nonexistent/lsblk.json"]);
        assert_eq!(code, EXIT_FAULT);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error: UOS-INSTALL-INVENTORY-001\n"), "{stderr}");
        assert!(stderr.contains("Technical detail:"));
    }

    #[test]
    fn a_directory_or_device_node_is_never_read_as_input() {
        for path in ["/", "/dev/null"] {
            let (code, _, stderr) = run_with(&["inventory", "--from", path]);
            assert_eq!(code, EXIT_FAULT, "{path}");
            assert!(stderr.contains("not a regular file"), "{path}: {stderr}");
        }
    }
}
