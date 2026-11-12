// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Preflight checks (specification §14.3): pure functions over a
//! [`HostFacts`] value.
//!
//! Nothing here looks at the machine. Whoever gathers the facts fills in a
//! [`HostFacts`]; a fact that could not be determined is `None`, and an
//! undetermined fact never passes. Each check returns a [`CheckResult`] with
//! a stable code, a user message and a technical detail (specification §6.3).
//!
//! The thresholds are data, in [`Requirements`]. They are assumptions until
//! UOS publishes measured minimums; see `docs/architecture/installer.md`.

use serde::{Deserialize, Serialize};

use crate::codes::InstallCode;
use crate::identity::Identity;
use crate::inventory::{ActiveUse, BlockDevice, DetectedOs};
use crate::plan::{LayoutOptions, PlanError, Scheme, describe_uses, minimum_disk_bytes};
use crate::units::{GIB, format_size};

/// The CPU architecture UOS supports (specification §4.2).
pub const REQUIRED_ARCHITECTURE: &str = "x86_64";

/// CPU feature flags, as `/proc/cpuinfo` names them, that the x86-64
/// baseline needs. Whether the UOS base image needs a higher microarchitecture
/// level has not been verified.
pub const REQUIRED_CPU_FEATURES: &[&str] = &[
    "cmov", "cx8", "fpu", "fxsr", "lm", "mmx", "sse", "sse2", "syscall",
];

/// Below this much memory the installer refuses to continue. Chosen so that
/// a machine sold with 4 GB passes.
pub const MIN_RAM_BYTES: u64 = 3 * GIB;

/// Below this much memory the installer warns. Chosen so that a machine sold
/// with 8 GB passes without a warning.
pub const RECOMMENDED_RAM_BYTES: u64 = 7 * GIB;

/// How the firmware started the installer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootMode {
    /// UEFI.
    Uefi,
    /// Legacy BIOS, or UEFI in compatibility mode.
    Bios,
}

/// The firmware's Secure Boot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecureBootState {
    /// Secure Boot is enforcing.
    Enabled,
    /// Secure Boot is switched off.
    Disabled,
    /// The firmware is in setup mode: no platform key is enrolled.
    SetupMode,
    /// The firmware does not offer Secure Boot.
    Unsupported,
}

/// Facts about the machine and the running installer environment.
///
/// `None` means the fact could not be determined.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentFacts {
    /// How the firmware started the installer.
    pub boot_mode: Option<BootMode>,
    /// The Secure Boot state.
    pub secure_boot: Option<SecureBootState>,
    /// The CPU architecture, as `uname -m` prints it.
    pub cpu_architecture: Option<String>,
    /// The CPU feature flags, as `/proc/cpuinfo` lists them.
    pub cpu_features: Option<Vec<String>>,
    /// Installed memory in bytes.
    pub ram_bytes: Option<u64>,
    /// True when the installer was started in safe graphics mode.
    pub safe_graphics: Option<bool>,
    /// True when a network connection is available.
    pub network_available: Option<bool>,
}

/// Facts about the chosen target disk, taken from the inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetFacts {
    /// The disk's identity.
    pub identity: Identity,
    /// The disk's capacity in bytes.
    pub capacity_bytes: u64,
    /// Mounted filesystems, active swap and open volumes on the disk.
    pub active_uses: Vec<ActiveUse>,
    /// True when the live environment runs from the disk.
    pub is_live_medium: bool,
    /// True when the disk is read-only.
    pub read_only: bool,
    /// Other operating systems detected on the disk.
    pub other_operating_systems: Vec<DetectedOs>,
}

impl TargetFacts {
    /// Derives the target facts from a device in an inventory.
    pub fn of(device: &BlockDevice) -> TargetFacts {
        TargetFacts {
            identity: Identity::of(device),
            capacity_bytes: device.size_bytes,
            active_uses: device.active_uses(),
            is_live_medium: device.is_live_medium(),
            read_only: device.read_only,
            other_operating_systems: device.detected_operating_systems(),
        }
    }
}

/// Everything the preflight checks look at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFacts {
    /// The machine and the installer environment.
    pub environment: EnvironmentFacts,
    /// The chosen target, once one is chosen.
    pub target: Option<TargetFacts>,
}

/// The thresholds the checks compare against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirements {
    /// The required CPU architecture.
    pub architecture: &'static str,
    /// The required CPU feature flags.
    pub cpu_features: &'static [&'static str],
    /// Memory below which installation is refused.
    pub min_ram_bytes: u64,
    /// Memory below which a warning is given.
    pub recommended_ram_bytes: u64,
    /// The smallest target disk the chosen layout fits on.
    pub min_target_bytes: u64,
}

impl Requirements {
    /// The requirements for installing with `scheme` and `options`.
    pub fn for_layout(scheme: &Scheme, options: LayoutOptions) -> Result<Requirements, PlanError> {
        Ok(Requirements {
            architecture: REQUIRED_ARCHITECTURE,
            cpu_features: REQUIRED_CPU_FEATURES,
            min_ram_bytes: MIN_RAM_BYTES,
            recommended_ram_bytes: RECOMMENDED_RAM_BYTES,
            min_target_bytes: minimum_disk_bytes(scheme, options)?,
        })
    }
}

/// The outcome of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// The check passed.
    Pass,
    /// The installation can continue, but the user should read the message.
    Warning,
    /// The installation must not continue.
    Blocking,
}

impl CheckStatus {
    /// The status as shown to the user.
    pub const fn label(self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Warning => "warning",
            CheckStatus::Blocking => "blocking",
        }
    }
}

/// The result of one preflight check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    /// The stable code that names the check.
    pub code: InstallCode,
    /// Whether the check passed, warned or blocked.
    pub status: CheckStatus,
    /// What the user needs to know and do.
    pub message: String,
    /// The values the check looked at.
    pub detail: String,
}

fn outcome(code: InstallCode, status: CheckStatus, message: &str, detail: String) -> CheckResult {
    CheckResult {
        code,
        status,
        message: message.to_owned(),
        detail,
    }
}

/// The results of every check that could be run.
///
/// Values are created only by [`run_preflight`], and a report cannot be read
/// from a file. So a report that names a target always holds the result of
/// every target check for that target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreflightReport {
    target: Option<Identity>,
    results: Vec<CheckResult>,
}

impl PreflightReport {
    /// The identity of the target the target checks were run for, if any.
    pub fn target(&self) -> Option<&Identity> {
        self.target.as_ref()
    }

    /// One result per check, in a fixed order.
    pub fn results(&self) -> &[CheckResult] {
        &self.results
    }

    /// The checks that block installation.
    pub fn blocking(&self) -> Vec<&CheckResult> {
        self.with_status(CheckStatus::Blocking)
    }

    /// The checks that warn.
    pub fn warnings(&self) -> Vec<&CheckResult> {
        self.with_status(CheckStatus::Warning)
    }

    /// True when at least one check blocks installation.
    pub fn has_blocking(&self) -> bool {
        !self.blocking().is_empty()
    }

    fn with_status(&self, status: CheckStatus) -> Vec<&CheckResult> {
        self.results
            .iter()
            .filter(|result| result.status == status)
            .collect()
    }
}

/// Runs every check. The target checks run only when a target is present.
pub fn run_preflight(facts: &HostFacts, requirements: &Requirements) -> PreflightReport {
    let environment = &facts.environment;
    let mut results = vec![
        check_boot_mode(environment),
        check_secure_boot(environment),
        check_cpu_architecture(environment, requirements),
        check_cpu_features(environment, requirements),
        check_memory(environment, requirements),
        check_safe_graphics(environment),
        check_network(environment),
    ];
    if let Some(target) = &facts.target {
        results.extend([
            check_target_capacity(target, requirements),
            check_target_in_use(target),
            check_target_live_medium(target),
            check_target_read_only(target),
            check_other_operating_systems(target),
            check_target_identity(target),
        ]);
    }
    PreflightReport {
        target: facts.target.as_ref().map(|target| target.identity.clone()),
        results,
    }
}

/// UEFI boot mode. UOS installs a UEFI boot path only, so legacy BIOS blocks.
pub fn check_boot_mode(environment: &EnvironmentFacts) -> CheckResult {
    let code = InstallCode::PreflightBootMode;
    match environment.boot_mode {
        Some(BootMode::Uefi) => outcome(
            code,
            CheckStatus::Pass,
            "The installer was started in UEFI mode.",
            "boot_mode: uefi".to_owned(),
        ),
        Some(BootMode::Bios) => outcome(
            code,
            CheckStatus::Blocking,
            "The installer was started in legacy BIOS mode. UOS needs UEFI. Restart, \
             choose the UEFI entry for the installation medium in the firmware boot menu, \
             and try again.",
            "boot_mode: bios".to_owned(),
        ),
        None => outcome(
            code,
            CheckStatus::Blocking,
            "The installer could not tell whether it was started in UEFI mode, so it \
             cannot install a bootable system. Export the diagnostics and report this.",
            "boot_mode: not determined".to_owned(),
        ),
    }
}

/// Secure Boot state. Informational: the installer never changes it.
pub fn check_secure_boot(environment: &EnvironmentFacts) -> CheckResult {
    let code = InstallCode::PreflightSecureBoot;
    match environment.secure_boot {
        Some(SecureBootState::Enabled) => outcome(
            code,
            CheckStatus::Pass,
            "Secure Boot is enabled.",
            "secure_boot: enabled".to_owned(),
        ),
        Some(SecureBootState::Disabled) => outcome(
            code,
            CheckStatus::Warning,
            "Secure Boot is switched off in the firmware. The installer does not change \
             this setting. You can switch it on later in the firmware setup.",
            "secure_boot: disabled".to_owned(),
        ),
        Some(SecureBootState::SetupMode) => outcome(
            code,
            CheckStatus::Warning,
            "The firmware is in Secure Boot setup mode: no platform key is enrolled, so \
             Secure Boot is not protecting this machine. The installer does not enrol keys.",
            "secure_boot: setup_mode".to_owned(),
        ),
        Some(SecureBootState::Unsupported) => outcome(
            code,
            CheckStatus::Warning,
            "This firmware does not offer Secure Boot.",
            "secure_boot: unsupported".to_owned(),
        ),
        None => outcome(
            code,
            CheckStatus::Warning,
            "The Secure Boot state could not be determined.",
            "secure_boot: not determined".to_owned(),
        ),
    }
}

/// CPU architecture.
pub fn check_cpu_architecture(
    environment: &EnvironmentFacts,
    requirements: &Requirements,
) -> CheckResult {
    let code = InstallCode::PreflightCpuArchitecture;
    let required = requirements.architecture;
    match environment.cpu_architecture.as_deref() {
        Some(found) if found == required => outcome(
            code,
            CheckStatus::Pass,
            "The processor architecture is supported.",
            format!("cpu_architecture: {found}"),
        ),
        Some(found) => outcome(
            code,
            CheckStatus::Blocking,
            "This processor's architecture is not supported. UOS runs on 64-bit x86 \
             processors only.",
            format!("cpu_architecture: {found}; required: {required}"),
        ),
        None => outcome(
            code,
            CheckStatus::Blocking,
            "The processor architecture could not be determined, so the installer cannot \
             tell whether UOS will run. Export the diagnostics and report this.",
            format!("cpu_architecture: not determined; required: {required}"),
        ),
    }
}

/// Required CPU features.
pub fn check_cpu_features(
    environment: &EnvironmentFacts,
    requirements: &Requirements,
) -> CheckResult {
    let code = InstallCode::PreflightCpuFeatures;
    let Some(found) = &environment.cpu_features else {
        return outcome(
            code,
            CheckStatus::Blocking,
            "The processor's features could not be determined, so the installer cannot \
             tell whether UOS will run. Export the diagnostics and report this.",
            "cpu_features: not determined".to_owned(),
        );
    };
    let missing: Vec<&str> = requirements
        .cpu_features
        .iter()
        .copied()
        .filter(|required| !found.iter().any(|feature| feature.as_str() == *required))
        .collect();
    if missing.is_empty() {
        outcome(
            code,
            CheckStatus::Pass,
            "The processor has the features UOS needs.",
            format!("required: {}", requirements.cpu_features.join(", ")),
        )
    } else {
        outcome(
            code,
            CheckStatus::Blocking,
            "This processor lacks features UOS needs, so UOS cannot run on this machine.",
            format!("missing: {}", missing.join(", ")),
        )
    }
}

/// Installed memory.
pub fn check_memory(environment: &EnvironmentFacts, requirements: &Requirements) -> CheckResult {
    let code = InstallCode::PreflightMemory;
    let Some(ram) = environment.ram_bytes else {
        return outcome(
            code,
            CheckStatus::Blocking,
            "The amount of memory could not be determined, so the installer cannot tell \
             whether UOS will run. Export the diagnostics and report this.",
            "ram_bytes: not determined".to_owned(),
        );
    };
    let detail = format!(
        "ram_bytes: {ram}; minimum: {}; recommended: {}",
        requirements.min_ram_bytes, requirements.recommended_ram_bytes
    );
    if ram < requirements.min_ram_bytes {
        outcome(
            code,
            CheckStatus::Blocking,
            "This machine does not have enough memory to run UOS.",
            detail,
        )
    } else if ram < requirements.recommended_ram_bytes {
        outcome(
            code,
            CheckStatus::Warning,
            "This machine has less memory than recommended. UOS will install, but it may \
             be slow.",
            detail,
        )
    } else {
        outcome(
            code,
            CheckStatus::Pass,
            "This machine has enough memory.",
            detail,
        )
    }
}

/// Whether the installer itself was started in safe graphics mode.
pub fn check_safe_graphics(environment: &EnvironmentFacts) -> CheckResult {
    let code = InstallCode::PreflightSafeGraphics;
    match environment.safe_graphics {
        Some(false) => outcome(
            code,
            CheckStatus::Pass,
            "The installer is running with normal graphics.",
            "safe_graphics: false".to_owned(),
        ),
        Some(true) => outcome(
            code,
            CheckStatus::Warning,
            "The installer was started in safe graphics mode. Installation is not \
             affected, but the installed system may need the same mode on its first start.",
            "safe_graphics: true".to_owned(),
        ),
        None => outcome(
            code,
            CheckStatus::Warning,
            "The installer could not tell whether it is running in safe graphics mode.",
            "safe_graphics: not determined".to_owned(),
        ),
    }
}

/// Network availability. Informational: installation works offline.
pub fn check_network(environment: &EnvironmentFacts) -> CheckResult {
    let code = InstallCode::PreflightNetwork;
    match environment.network_available {
        Some(true) => outcome(
            code,
            CheckStatus::Pass,
            "A network connection is available.",
            "network_available: true".to_owned(),
        ),
        Some(false) => outcome(
            code,
            CheckStatus::Pass,
            "No network connection. None is needed: UOS installs from the installation \
             medium.",
            "network_available: false".to_owned(),
        ),
        None => outcome(
            code,
            CheckStatus::Warning,
            "The network state could not be determined. None is needed: UOS installs from \
             the installation medium.",
            "network_available: not determined".to_owned(),
        ),
    }
}

/// Target disk capacity.
pub fn check_target_capacity(target: &TargetFacts, requirements: &Requirements) -> CheckResult {
    let code = InstallCode::PreflightTargetCapacity;
    let detail = format!(
        "capacity_bytes: {}; required_bytes: {}",
        target.capacity_bytes, requirements.min_target_bytes
    );
    if target.capacity_bytes == 0 {
        outcome(
            code,
            CheckStatus::Blocking,
            "The chosen drive holds no medium. Insert one, or choose another disk.",
            detail,
        )
    } else if target.capacity_bytes < requirements.min_target_bytes {
        let message = format!(
            "The chosen disk is too small. It holds {}; at least {} is needed for the \
             chosen options. Choose a larger disk.",
            format_size(target.capacity_bytes),
            format_size(requirements.min_target_bytes)
        );
        outcome(code, CheckStatus::Blocking, &message, detail)
    } else {
        outcome(
            code,
            CheckStatus::Pass,
            "The chosen disk is large enough.",
            detail,
        )
    }
}

/// Whether anything on the target is mounted or held open.
pub fn check_target_in_use(target: &TargetFacts) -> CheckResult {
    let code = InstallCode::PreflightTargetInUse;
    if target.active_uses.is_empty() {
        outcome(
            code,
            CheckStatus::Pass,
            "Nothing on the chosen disk is in use.",
            "no mounted filesystems, active swap or open volumes".to_owned(),
        )
    } else {
        outcome(
            code,
            CheckStatus::Blocking,
            "The chosen disk is in use: a filesystem on it is mounted, or an encrypted or \
             logical volume on it is open. Unmount and close everything on the disk, or \
             choose another disk.",
            describe_uses(&target.active_uses),
        )
    }
}

/// Whether the target is the medium the installer runs from.
pub fn check_target_live_medium(target: &TargetFacts) -> CheckResult {
    let code = InstallCode::PreflightTargetLiveMedium;
    if target.is_live_medium {
        outcome(
            code,
            CheckStatus::Blocking,
            "The chosen disk is the medium the installer is running from, so it cannot be \
             erased. Choose another disk.",
            "a filesystem on the disk is mounted at a live-medium mount point".to_owned(),
        )
    } else {
        outcome(
            code,
            CheckStatus::Pass,
            "The chosen disk is not the installation medium.",
            "no live-medium mount point on the disk".to_owned(),
        )
    }
}

/// Whether the target is writable.
pub fn check_target_read_only(target: &TargetFacts) -> CheckResult {
    let code = InstallCode::PreflightTargetReadOnly;
    if target.read_only {
        outcome(
            code,
            CheckStatus::Blocking,
            "The chosen disk is read-only. Remove the write protection or choose another \
             disk.",
            "read_only: true".to_owned(),
        )
    } else {
        outcome(
            code,
            CheckStatus::Pass,
            "The chosen disk is writable.",
            "read_only: false".to_owned(),
        )
    }
}

/// Other operating systems on the target. A warning: erasing destroys them.
pub fn check_other_operating_systems(target: &TargetFacts) -> CheckResult {
    let code = InstallCode::PreflightOtherOs;
    if target.other_operating_systems.is_empty() {
        return outcome(
            code,
            CheckStatus::Pass,
            "No other operating system was detected on the chosen disk.",
            "no known operating-system partitions or filesystems".to_owned(),
        );
    }
    let names: Vec<String> = target
        .other_operating_systems
        .iter()
        .map(DetectedOs::headline)
        .collect();
    let evidence: Vec<&str> = target
        .other_operating_systems
        .iter()
        .flat_map(|system| system.evidence.iter().map(String::as_str))
        .collect();
    let message = format!(
        "Another operating system was detected on the chosen disk: {}. Erasing the disk \
         destroys it and its data. Back up anything you need first.",
        names.join(", ")
    );
    outcome(code, CheckStatus::Warning, &message, evidence.join("; "))
}

/// Whether the target has a stable identifier.
pub fn check_target_identity(target: &TargetFacts) -> CheckResult {
    let code = InstallCode::PreflightTargetIdentity;
    if target.identity.is_stable() {
        outcome(
            code,
            CheckStatus::Pass,
            "The chosen disk reports a stable identifier.",
            format!(
                "serial: {}; wwn: {}",
                target.identity.serial.as_deref().unwrap_or("none"),
                target.identity.wwn.as_deref().unwrap_or("none")
            ),
        )
    } else {
        outcome(
            code,
            CheckStatus::Warning,
            "The chosen disk reports no serial number or WWN. The installer can recognise \
             it only by model, size and port. Disconnect other disks of the same model to \
             avoid erasing the wrong one.",
            "serial: none; wwn: none".to_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::ERASE_DISK_SCHEME;
    use crate::testing::{good_environment, oddities, workstation};
    use crate::units::MIB;

    fn requirements() -> Requirements {
        Requirements::for_layout(&ERASE_DISK_SCHEME, LayoutOptions::default()).unwrap()
    }

    fn target_facts(inventory: &crate::inventory::Inventory, name: &str) -> TargetFacts {
        let device = inventory
            .devices
            .iter()
            .find(|device| device.name == name)
            .unwrap_or_else(|| panic!("fixture has no device {name}"));
        TargetFacts::of(device)
    }

    fn status_of(report: &PreflightReport, code: InstallCode) -> CheckStatus {
        report
            .results
            .iter()
            .find(|result| result.code == code)
            .unwrap_or_else(|| panic!("no result for {code}"))
            .status
    }

    #[test]
    fn a_good_machine_and_an_idle_disk_have_no_blocking_checks() {
        let inventory = workstation();
        let facts = HostFacts {
            environment: good_environment(),
            target: Some(target_facts(&inventory, "sda")),
        };
        let report = run_preflight(&facts, &requirements());
        assert!(!report.has_blocking(), "{:?}", report.blocking());
        assert_eq!(report.results.len(), 13);
        assert_eq!(report.target, facts.target.map(|target| target.identity));
        // The disk holds a Linux installation: a warning, not a blocker.
        let warnings: Vec<InstallCode> = report.warnings().iter().map(|r| r.code).collect();
        assert_eq!(warnings, [InstallCode::PreflightOtherOs]);
    }

    #[test]
    fn every_result_has_both_layers_and_a_distinct_code() {
        let inventory = workstation();
        let facts = HostFacts {
            environment: good_environment(),
            target: Some(target_facts(&inventory, "nvme0n1")),
        };
        let report = run_preflight(&facts, &requirements());
        let mut codes: Vec<InstallCode> = report.results.iter().map(|r| r.code).collect();
        for result in &report.results {
            assert!(!result.message.is_empty());
            assert!(!result.detail.is_empty());
            assert!(result.code.as_str().starts_with("UOS-INSTALL-PREFLIGHT-"));
        }
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), report.results.len());
    }

    #[test]
    fn target_checks_are_absent_without_a_target() {
        let facts = HostFacts {
            environment: good_environment(),
            target: None,
        };
        let report = run_preflight(&facts, &requirements());
        assert_eq!(report.results.len(), 7);
        assert_eq!(report.target, None);
        assert!(!report.has_blocking());
    }

    #[test]
    fn undetermined_facts_never_pass() {
        let facts = HostFacts {
            environment: EnvironmentFacts::default(),
            target: None,
        };
        let report = run_preflight(&facts, &requirements());
        for result in &report.results {
            assert_ne!(result.status, CheckStatus::Pass, "{}", result.code);
            assert!(result.detail.contains("not determined"), "{}", result.code);
        }
        for code in [
            InstallCode::PreflightBootMode,
            InstallCode::PreflightCpuArchitecture,
            InstallCode::PreflightCpuFeatures,
            InstallCode::PreflightMemory,
        ] {
            assert_eq!(status_of(&report, code), CheckStatus::Blocking, "{code}");
        }
        for code in [
            InstallCode::PreflightSecureBoot,
            InstallCode::PreflightSafeGraphics,
            InstallCode::PreflightNetwork,
        ] {
            assert_eq!(status_of(&report, code), CheckStatus::Warning, "{code}");
        }
    }

    #[test]
    fn legacy_bios_blocks() {
        let mut environment = good_environment();
        environment.boot_mode = Some(BootMode::Bios);
        let result = check_boot_mode(&environment);
        assert_eq!(result.status, CheckStatus::Blocking);
        assert_eq!(result.code.as_str(), "UOS-INSTALL-PREFLIGHT-001");
        assert!(result.message.contains("UEFI"));
    }

    #[test]
    fn secure_boot_never_blocks() {
        let mut environment = good_environment();
        for (state, expected) in [
            (Some(SecureBootState::Enabled), CheckStatus::Pass),
            (Some(SecureBootState::Disabled), CheckStatus::Warning),
            (Some(SecureBootState::SetupMode), CheckStatus::Warning),
            (Some(SecureBootState::Unsupported), CheckStatus::Warning),
            (None, CheckStatus::Warning),
        ] {
            environment.secure_boot = state;
            assert_eq!(
                check_secure_boot(&environment).status,
                expected,
                "{state:?}"
            );
        }
    }

    #[test]
    fn other_architectures_and_missing_features_block() {
        let requirements = requirements();
        let mut environment = good_environment();
        environment.cpu_architecture = Some("aarch64".to_owned());
        let result = check_cpu_architecture(&environment, &requirements);
        assert_eq!(result.status, CheckStatus::Blocking);
        assert!(result.detail.contains("aarch64"));

        let mut environment = good_environment();
        environment.cpu_features = Some(vec!["fpu".to_owned(), "sse".to_owned()]);
        let result = check_cpu_features(&environment, &requirements);
        assert_eq!(result.status, CheckStatus::Blocking);
        assert_eq!(
            result.detail,
            "missing: cmov, cx8, fxsr, lm, mmx, sse2, syscall"
        );
    }

    #[test]
    fn memory_thresholds() {
        let requirements = requirements();
        let mut environment = good_environment();
        for (ram, expected) in [
            (0, CheckStatus::Blocking),
            (MIN_RAM_BYTES - 1, CheckStatus::Blocking),
            (MIN_RAM_BYTES, CheckStatus::Warning),
            (RECOMMENDED_RAM_BYTES - 1, CheckStatus::Warning),
            (RECOMMENDED_RAM_BYTES, CheckStatus::Pass),
            (u64::MAX, CheckStatus::Pass),
        ] {
            environment.ram_bytes = Some(ram);
            assert_eq!(
                check_memory(&environment, &requirements).status,
                expected,
                "{ram}"
            );
        }
    }

    #[test]
    fn safe_graphics_and_network_are_informational() {
        let mut environment = good_environment();
        environment.safe_graphics = Some(true);
        assert_eq!(
            check_safe_graphics(&environment).status,
            CheckStatus::Warning
        );
        environment.network_available = Some(false);
        let network = check_network(&environment);
        assert_eq!(network.status, CheckStatus::Pass);
        assert!(network.message.contains("None is needed"));
    }

    #[test]
    fn the_live_medium_blocks_on_two_checks() {
        let inventory = workstation();
        let facts = HostFacts {
            environment: good_environment(),
            target: Some(target_facts(&inventory, "sdb")),
        };
        let report = run_preflight(&facts, &requirements());
        assert_eq!(
            status_of(&report, InstallCode::PreflightTargetLiveMedium),
            CheckStatus::Blocking
        );
        assert_eq!(
            status_of(&report, InstallCode::PreflightTargetInUse),
            CheckStatus::Blocking
        );
    }

    #[test]
    fn mounted_filesystems_and_open_volumes_block() {
        let inventory = oddities();
        let target = target_facts(&inventory, "nvme1n1");
        let result = check_target_in_use(&target);
        assert_eq!(result.status, CheckStatus::Blocking);
        assert!(result.detail.contains("active encrypted volume"));

        let mut idle = target_facts(&workstation(), "sda");
        assert_eq!(check_target_in_use(&idle).status, CheckStatus::Pass);
        idle.active_uses.push(ActiveUse {
            path: "/dev/sda2".to_owned(),
            reason: "mounted at /mnt".to_owned(),
        });
        let result = check_target_in_use(&idle);
        assert_eq!(result.status, CheckStatus::Blocking);
        assert_eq!(result.detail, "/dev/sda2: mounted at /mnt");
    }

    #[test]
    fn capacity_thresholds() {
        let requirements = requirements();
        let mut target = target_facts(&workstation(), "sda");
        for (capacity, expected) in [
            (0, CheckStatus::Blocking),
            (requirements.min_target_bytes - MIB, CheckStatus::Blocking),
            (requirements.min_target_bytes, CheckStatus::Pass),
            (u64::MAX, CheckStatus::Pass),
        ] {
            target.capacity_bytes = capacity;
            assert_eq!(
                check_target_capacity(&target, &requirements).status,
                expected,
                "{capacity}"
            );
        }
        let empty = target_facts(&workstation(), "sdc");
        let result = check_target_capacity(&empty, &requirements);
        assert_eq!(result.status, CheckStatus::Blocking);
        assert!(result.message.contains("no medium"));
    }

    #[test]
    fn read_only_blocks_and_other_systems_warn() {
        let mut target = target_facts(&workstation(), "nvme0n1");
        assert_eq!(check_target_read_only(&target).status, CheckStatus::Pass);
        target.read_only = true;
        assert_eq!(
            check_target_read_only(&target).status,
            CheckStatus::Blocking
        );

        let result = check_other_operating_systems(&target);
        assert_eq!(result.status, CheckStatus::Warning);
        assert!(result.message.contains("Windows (likely)"));
        assert!(result.detail.contains("NTFS volume /dev/nvme0n1p3"));

        target.other_operating_systems.clear();
        assert_eq!(
            check_other_operating_systems(&target).status,
            CheckStatus::Pass
        );
    }

    #[test]
    fn a_weak_identity_warns() {
        let target = target_facts(&oddities(), "sda");
        let result = check_target_identity(&target);
        assert_eq!(result.status, CheckStatus::Warning);
        assert_eq!(result.code, InstallCode::PreflightTargetIdentity);
        let stable = target_facts(&workstation(), "sda");
        assert_eq!(check_target_identity(&stable).status, CheckStatus::Pass);
    }

    #[test]
    fn environment_facts_parse_from_json_and_reject_unknown_keys() {
        let facts: EnvironmentFacts = serde_json::from_str(
            r#"{"boot_mode": "uefi", "secure_boot": "setup_mode", "cpu_architecture": "x86_64",
                "ram_bytes": 16000000000}"#,
        )
        .unwrap();
        assert_eq!(facts.boot_mode, Some(BootMode::Uefi));
        assert_eq!(facts.secure_boot, Some(SecureBootState::SetupMode));
        assert_eq!(facts.cpu_features, None);
        assert_eq!(facts.network_available, None);
        assert!(serde_json::from_str::<EnvironmentFacts>(r#"{"bootmode": "uefi"}"#).is_err());
        assert!(serde_json::from_str::<EnvironmentFacts>(r#"{"boot_mode": "efi"}"#).is_err());
    }
}
