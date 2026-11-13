// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The detection checks, one function per kind of [`Check`].
//!
//! A check only reads. It answers whether the fault is present, what it is
//! about (the units or programs concerned) and a few lines of evidence for
//! the person. Evidence never holds a name server, an address or file
//! content: it is shown to P@x and kept in a file anyone on the computer may
//! read. A check that cannot run says so; it never counts as a fault.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::catalogue::Check;
use super::machine::Machine;
use crate::host::RunError;

/// What one check found.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Finding {
    /// Whether the fault is present.
    pub present: bool,
    /// What the fault is about: the failed units, the crashed programs, the
    /// directory that is short of space.
    pub subjects: Vec<String>,
    /// What was seen, for the person.
    pub evidence: Vec<String>,
    /// Why the check could not be made, when it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
}

impl Finding {
    fn absent(evidence: impl Into<String>) -> Self {
        Finding {
            present: false,
            subjects: Vec::new(),
            evidence: vec![evidence.into()],
            unavailable: None,
        }
    }

    fn unavailable(reason: impl Into<String>) -> Self {
        Finding {
            present: false,
            subjects: Vec::new(),
            evidence: Vec::new(),
            unavailable: Some(reason.into()),
        }
    }

    fn present(subjects: Vec<String>, evidence: Vec<String>) -> Self {
        Finding {
            present: true,
            subjects,
            evidence,
            unavailable: None,
        }
    }
}

fn describe(error: &RunError) -> String {
    match error {
        RunError::NotFound => "the program is not installed".to_owned(),
        RunError::PermissionDenied => "the program may not be run".to_owned(),
        RunError::TimedOut => "the program did not finish in time".to_owned(),
        RunError::Io(detail) => detail.clone(),
    }
}

/// Runs `check`.
pub fn run_check(machine: &dyn Machine, check: &Check) -> Finding {
    match check {
        Check::UnitFailed { units } => units_failed(machine, units),
        Check::NetworkTimeOff => network_time_off(machine),
        Check::ResolverNotManaged {
            service,
            path,
            managed_targets,
            stub_address,
            ..
        } => resolver_not_managed(machine, service, path, managed_targets, stub_address),
        Check::FreeSpaceLow {
            path,
            min_free_bytes,
            min_free_percent,
        } => free_space_low(machine, path, *min_free_bytes, *min_free_percent),
        Check::TurProgramCrashed { prefixes, .. } => tur_program_crashed(machine, prefixes),
    }
}

/// `systemctl is-failed` prints one state for each unit named, in order.
fn units_failed(machine: &dyn Machine, units: &[String]) -> Finding {
    let mut args = vec!["is-failed", "--no-pager"];
    args.extend(units.iter().map(String::as_str));
    let output = match machine.run("systemctl", &args) {
        Ok(output) => output,
        Err(error) => return Finding::unavailable(format!("systemctl: {}", describe(&error))),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let states: Vec<&str> = text.lines().map(str::trim).collect();
    if states.len() != units.len() {
        return Finding::unavailable(format!(
            "systemctl is-failed printed {} states for {} units",
            states.len(),
            units.len()
        ));
    }
    let failed: Vec<String> = units
        .iter()
        .zip(&states)
        .filter(|(_, state)| **state == "failed")
        .map(|(unit, _)| unit.clone())
        .collect();
    if failed.is_empty() {
        Finding::absent("none of the watched services has failed")
    } else {
        let evidence = failed
            .iter()
            .map(|unit| format!("{unit} is in the failed state"))
            .collect();
        Finding::present(failed, evidence)
    }
}

/// `timedatectl show` prints properties as `NAME=value`.
fn network_time_off(machine: &dyn Machine) -> Finding {
    let output = match machine.run("timedatectl", &["show", "--property=NTP", "--property=CanNTP"])
    {
        Ok(output) if output.success() => output,
        Ok(output) => {
            return Finding::unavailable(format!(
                "timedatectl show exited with {:?}",
                output.exit_code
            ));
        }
        Err(error) => return Finding::unavailable(format!("timedatectl: {}", describe(&error))),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let value = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            .map(str::trim)
            .map(str::to_owned)
    };
    let (ntp, can) = (value("NTP"), value("CanNTP"));
    match (ntp.as_deref(), can.as_deref()) {
        (Some("no"), Some("yes")) => Finding::present(
            vec!["network time".to_owned()],
            vec!["timedatectl: NTP=no, CanNTP=yes".to_owned()],
        ),
        (Some(_), Some(_)) => Finding::absent(format!(
            "timedatectl: NTP={}, CanNTP={}",
            ntp.as_deref().unwrap_or_default(),
            can.as_deref().unwrap_or_default()
        )),
        _ => Finding::unavailable("timedatectl did not report NTP and CanNTP"),
    }
}

fn resolver_not_managed(
    machine: &dyn Machine,
    service: &str,
    path: &str,
    managed: &[String],
    stub: &str,
) -> Finding {
    let active = match machine.run("systemctl", &["is-active", service]) {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        Err(error) => return Finding::unavailable(format!("systemctl: {}", describe(&error))),
    };
    if active != "active" {
        return Finding::absent(format!(
            "{service} is {active}: {path} is not its to manage"
        ));
    }
    match machine.read_link(path) {
        Ok(target) if managed.contains(&target) => {
            Finding::absent(format!("{path} points to {target}"))
        }
        Ok(target) => Finding::present(
            vec![path.to_owned()],
            vec![format!(
                "{path} points to {target}, not to a file {service} keeps"
            )],
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Finding::present(
            vec![path.to_owned()],
            vec![format!("{path} is missing")],
        ),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            let content = match machine.read_file(path) {
                Ok(content) => content,
                Err(error) => {
                    return Finding::unavailable(format!("{path} cannot be read: {error:?}"));
                }
            };
            let text = String::from_utf8_lossy(&content);
            let servers: Vec<&str> = text
                .lines()
                .filter_map(|line| line.trim().strip_prefix("nameserver"))
                .map(str::trim)
                .collect();
            if !servers.is_empty() && servers.iter().all(|server| *server == stub) {
                Finding::absent(format!("{path} is a file that names only {stub}"))
            } else {
                // How many, never which: the evidence is readable by anyone.
                Finding::present(
                    vec![path.to_owned()],
                    vec![format!(
                        "{path} is a file of its own naming {} name server(s), not {stub}",
                        servers.len()
                    )],
                )
            }
        }
        Err(error) => Finding::unavailable(format!("{path}: {error}")),
    }
}

fn mebibytes(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

/// `df --output=avail,size -B1 PATH` prints a heading and one line of two
/// numbers.
fn free_space_low(machine: &dyn Machine, path: &str, min_bytes: u64, min_percent: u64) -> Finding {
    let target = machine.command_path(path);
    let output = match machine.run("df", &["--output=avail,size", "-B1", &target]) {
        Ok(output) if output.success() => output,
        Ok(output) => {
            return Finding::unavailable(format!("df exited with {:?}", output.exit_code));
        }
        Err(error) => return Finding::unavailable(format!("df: {}", describe(&error))),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let numbers: Vec<u64> = text
        .lines()
        .nth(1)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    let &[available, size] = numbers.as_slice() else {
        return Finding::unavailable("df did not print the free space and the size");
    };
    let evidence = format!(
        "{} MiB free of {} MiB on the file system of {path}",
        mebibytes(available),
        mebibytes(size)
    );
    let low = available < min_bytes
        || u128::from(available) * 100 < u128::from(size) * u128::from(min_percent);
    if low {
        Finding::present(vec![path.to_owned()], vec![evidence])
    } else {
        Finding::absent(evidence)
    }
}

/// The start of this boot, in seconds since the epoch: `btime` in
/// `/proc/stat`.
pub fn boot_epoch(machine: &dyn Machine) -> Option<u64> {
    let content = machine.read_file("/proc/stat").ok()?;
    String::from_utf8_lossy(&content)
        .lines()
        .find_map(|line| line.strip_prefix("btime "))
        .and_then(|value| value.trim().parse().ok())
}

/// `coredumpctl list --json=short` prints an array of objects with the
/// list's columns (`time`, `pid`, `uid`, `gid`, `sig`, `corefile`, `exe`,
/// `size`); when nothing matches it prints nothing and exits non-zero.
fn tur_program_crashed(machine: &dyn Machine, prefixes: &[String]) -> Finding {
    let Some(boot) = boot_epoch(machine) else {
        return Finding::unavailable("the start of this boot is not known (/proc/stat)");
    };
    let since = format!("--since=@{boot}");
    let output = match machine.run(
        "coredumpctl",
        &["list", "--json=short", "--no-pager", "--quiet", &since],
    ) {
        Ok(output) => output,
        Err(error) => return Finding::unavailable(format!("coredumpctl: {}", describe(&error))),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    if text.trim().is_empty() {
        return Finding::absent("no program crashed during this boot");
    }
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&text) else {
        return Finding::unavailable("coredumpctl did not print a list");
    };
    let mut programs = BTreeSet::new();
    let mut evidence = Vec::new();
    for entry in &entries {
        let Some(exe) = entry.get("exe").and_then(Value::as_str) else {
            continue;
        };
        if !prefixes.iter().any(|prefix| exe.starts_with(prefix.as_str())) {
            continue;
        }
        let program = exe.rsplit('/').next().unwrap_or(exe).to_owned();
        let signal = match entry.get("sig") {
            Some(Value::Number(number)) => number.to_string(),
            Some(Value::String(text)) => text.clone(),
            _ => "unknown".to_owned(),
        };
        if evidence.len() < 10 {
            evidence.push(format!("{exe} stopped with signal {signal}"));
        }
        programs.insert(program);
    }
    if programs.is_empty() {
        Finding::absent("no program of TUR crashed during this boot")
    } else {
        Finding::present(programs.into_iter().collect(), evidence)
    }
}
