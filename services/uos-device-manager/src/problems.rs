// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Findings: what is wrong with the hardware, in words a person can act on.
//!
//! [`detect`] is a pure function of an [`Inventory`]. Each [`Finding`] has the
//! two layers specification §6.3 requires: a user message with a suggested
//! next step, and technical detail with a stable `UOS-DEVICE-*` code.
//!
//! The next step never involves a driver written for another operating
//! system. Linux cannot load one (specification §10.6), and a message that
//! suggested it would send the user to do something harmful.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Serialize, Serializer};
use uos_core::ErrorCode;

use crate::model::{
    Category, FirmwareLoadFailure, Inventory, ModuleMatch, PciDevice, SCHEMA_VERSION,
    ThunderboltAuthorisation, UsbDevice, UsbInterface,
};

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Worth knowing; the device may well be fine.
    Info,
    /// The device probably does not work.
    Warning,
    /// The kernel itself reported a failure for this device.
    Error,
}

/// The kinds of finding, one per error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProblemKind {
    /// No driver is bound, and no loadable module of this kernel claims the
    /// device (or that could not be checked).
    NoDriver,
    /// No driver is bound, although a module that claims the device exists.
    /// It is not loaded.
    DriverNotLoaded,
    /// A driver that claims the device is loaded but is not bound to it, or
    /// the kernel log records that it refused the device.
    DriverNotBound,
    /// The kernel log records a firmware file that could not be loaded.
    FirmwareMissing,
    /// A network controller has a driver but produced no network interface.
    DeviceInactive,
    /// A Thunderbolt device is connected but has not been authorised.
    ThunderboltNotAuthorised,
}

impl ProblemKind {
    /// Every kind, in code order.
    pub const ALL: [ProblemKind; 6] = [
        ProblemKind::NoDriver,
        ProblemKind::DriverNotLoaded,
        ProblemKind::DriverNotBound,
        ProblemKind::FirmwareMissing,
        ProblemKind::DeviceInactive,
        ProblemKind::ThunderboltNotAuthorised,
    ];

    /// The error code as text.
    pub const fn code_text(self) -> &'static str {
        match self {
            ProblemKind::NoDriver => "UOS-DEVICE-DRIVER-001",
            ProblemKind::DriverNotLoaded => "UOS-DEVICE-DRIVER-002",
            ProblemKind::DriverNotBound => "UOS-DEVICE-DRIVER-003",
            ProblemKind::FirmwareMissing => "UOS-DEVICE-FIRMWARE-001",
            ProblemKind::DeviceInactive => "UOS-DEVICE-INACTIVE-001",
            ProblemKind::ThunderboltNotAuthorised => "UOS-DEVICE-THUNDERBOLT-001",
        }
    }

    /// The validated error code. Every code above is valid, which a test
    /// checks, so this is `Some` for every kind.
    pub fn code(self) -> Option<ErrorCode> {
        self.code_text().parse().ok()
    }

    /// The registry title of the code.
    pub const fn title(self) -> &'static str {
        match self {
            ProblemKind::NoDriver => "No driver for the device",
            ProblemKind::DriverNotLoaded => "Driver available but not loaded",
            ProblemKind::DriverNotBound => "Driver did not attach to the device",
            ProblemKind::FirmwareMissing => "Firmware file could not be loaded",
            ProblemKind::DeviceInactive => "Driver attached but the device is not usable",
            ProblemKind::ThunderboltNotAuthorised => "Thunderbolt device not authorised",
        }
    }
}

fn code_as_text<S: Serializer>(code: &ErrorCode, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(code.as_str())
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// The stable error code, for example `UOS-DEVICE-DRIVER-001`.
    #[serde(serialize_with = "code_as_text")]
    pub code: ErrorCode,
    /// How much the finding matters.
    pub severity: Severity,
    /// The registry title of the code.
    pub title: String,
    /// The identifier of the device, if it is in the inventory.
    pub device_id: Option<String>,
    /// The category of the device, if it is in the inventory.
    pub category: Option<Category>,
    /// What happened, for the user.
    pub user_message: String,
    /// What the user can do next.
    pub next_step: String,
    /// Identifiers and evidence, for a developer.
    pub technical_detail: BTreeMap<String, String>,
}

/// The document `uos-device problems --json` prints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProblemReport {
    /// The version of this document.
    pub schema_version: u32,
    /// The findings, most severe first.
    pub findings: Vec<Finding>,
}

impl ProblemReport {
    /// Detect the problems of an inventory and wrap them as a document.
    pub fn of(inventory: &Inventory) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            findings: detect(inventory),
        }
    }
}

const STEP_UPDATE_THEN_REPORT: &str = "Install any pending UOS updates and restart: a newer kernel may include a driver for this device. If the device still does not work, run `uos-diag hardware` and report this finding.";
const STEP_NONE_UNLESS_BROKEN: &str = "No action is needed if the device works. If it does not, run `uos-diag hardware` and report this finding.";
const STEP_NOT_LOADED: &str = "Restart the computer. If the driver is still not loaded, check whether it has been disabled by a kernel module deny-list entry or a kernel command-line option, then run `uos-diag hardware` and report this finding.";
const STEP_NOT_BOUND: &str = "Restart the computer. If this happens again, run `uos-diag hardware` and report this finding; the kernel log records why the driver did not accept the device.";
const STEP_FIRMWARE: &str = "Install any pending UOS updates and restart: firmware files are delivered with the system. If the file is still missing, run `uos-diag hardware` and report this finding. Do not copy firmware files from other sources.";
const STEP_FIRMWARE_OPTIONAL: &str = "No action is needed if the device works: drivers often ask for optional files. If it does not, install any pending UOS updates, restart, and report this finding with `uos-diag hardware`.";
const STEP_INACTIVE: &str = "Restart the computer. If no network interface appears, install any pending UOS updates, then run `uos-diag hardware` and report this finding.";
const STEP_THUNDERBOLT: &str = "If you trust the device, authorise it in the system's Thunderbolt settings. If you do not recognise it, unplug it.";

/// Whether a PCI class is one whose devices do nothing useful without a
/// driver: storage, network, display, multimedia, communication, input,
/// serial bus and wireless controllers. Bridges, memory controllers and
/// system peripherals commonly have no driver on a healthy system.
fn pci_needs_driver(class: Option<&str>) -> bool {
    matches!(
        class.and_then(|code| code.get(..2)),
        Some("01" | "02" | "03" | "04" | "07" | "09" | "0c" | "0d")
    )
}

fn describe_pci(device: &PciDevice) -> String {
    let kind = device.class_name.as_deref().unwrap_or("PCI device");
    let model = match (&device.vendor_name, &device.device_name) {
        (Some(vendor), Some(name)) => format!(" ({vendor} {name})"),
        (None, Some(name)) => format!(" ({name})"),
        _ => String::new(),
    };
    format!("the {kind}{model} at PCI address {}", device.address)
}

fn describe_usb(device: &UsbDevice, interface: &UsbInterface) -> String {
    let function = interface.class_name.as_deref().unwrap_or("unknown");
    let product = device.product.as_deref().unwrap_or("USB device");
    format!(
        "the {function} function ({}) of {product} at USB port {}",
        interface.name, device.bus_port
    )
}

fn id_pair(first: Option<&str>, second: Option<&str>) -> String {
    format!(
        "{}:{}",
        first.unwrap_or("unknown"),
        second.unwrap_or("unknown")
    )
}

fn modules_text(modules: Option<&[ModuleMatch]>) -> String {
    match modules {
        None => "unknown: the kernel's module alias table could not be read".to_owned(),
        Some([]) => "none".to_owned(),
        Some(modules) => modules
            .iter()
            .map(|module| {
                let state = if module.loaded {
                    "loaded"
                } else {
                    "not loaded"
                };
                format!("{} ({state})", module.name)
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn module_names(modules: &[ModuleMatch]) -> String {
    modules
        .iter()
        .map(|module| module.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn error_text(error: Option<i32>) -> String {
    error.map_or_else(|| "not stated".to_owned(), |number| number.to_string())
}

/// Everything `finding` needs to know about a device without a driver.
struct Unbound<'a> {
    subject: String,
    device: (&'a str, Category),
    /// The kernel name the log would use for this device.
    kernel_name: &'a str,
    modules: Option<&'a [ModuleMatch]>,
    /// Whether devices of this kind need a driver to be of any use.
    needs_driver: bool,
    /// Whether a function of this kind is often driven from user space.
    user_space_common: bool,
    detail: Vec<(&'static str, String)>,
}

fn finding(
    kind: ProblemKind,
    severity: Severity,
    device: Option<(&str, Category)>,
    user_message: String,
    next_step: &str,
    detail: Vec<(&'static str, String)>,
) -> Option<Finding> {
    Some(Finding {
        code: kind.code()?,
        severity,
        title: kind.title().to_owned(),
        device_id: device.map(|(id, _)| id.to_owned()),
        category: device.map(|(_, category)| category),
        user_message,
        next_step: next_step.to_owned(),
        technical_detail: detail
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    })
}

/// The finding for one device (or USB interface) that has no driver bound.
fn unbound_finding(inventory: &Inventory, unbound: Unbound<'_>) -> Option<Finding> {
    let Unbound {
        subject,
        device,
        kernel_name,
        modules,
        needs_driver,
        user_space_common,
        mut detail,
    } = unbound;
    detail.push(("matching_modules", modules_text(modules)));
    let weight = if needs_driver {
        Severity::Warning
    } else {
        Severity::Info
    };

    // The strongest evidence first: the kernel said a driver refused it.
    let refused = inventory
        .kernel
        .probe_failures
        .iter()
        .find(|failure| failure.device == kernel_name);
    if let Some(failure) = refused {
        let driver = failure.driver.as_deref().unwrap_or("a driver");
        detail.push((
            "evidence",
            format!(
                "kernel log: probe of {kernel_name} with driver {driver} failed with error {}",
                error_text(failure.error)
            ),
        ));
        return finding(
            ProblemKind::DriverNotBound,
            Severity::Error,
            Some(device),
            format!(
                "The driver {driver} did not accept {subject} (kernel error {}), so the device will not work.",
                error_text(failure.error)
            ),
            STEP_NOT_BOUND,
            detail,
        );
    }

    let matches = modules.unwrap_or_default();
    if matches.iter().any(|module| module.loaded) {
        detail.push((
            "evidence",
            "sysfs: no driver link, although a matching module is loaded".to_owned(),
        ));
        return finding(
            ProblemKind::DriverNotBound,
            weight,
            Some(device),
            format!(
                "A driver for {subject} is loaded ({}) but is not attached to the device, so the device will not work.",
                module_names(matches)
            ),
            STEP_NOT_BOUND,
            detail,
        );
    }
    if !matches.is_empty() {
        detail.push((
            "evidence",
            "sysfs: no driver link, and no matching module is loaded".to_owned(),
        ));
        return finding(
            ProblemKind::DriverNotLoaded,
            weight,
            Some(device),
            format!(
                "This kernel has a driver for {subject} ({}) but it is not loaded, so the device will not work.",
                module_names(matches)
            ),
            STEP_NOT_LOADED,
            detail,
        );
    }

    detail.push(("evidence", "sysfs: no driver link".to_owned()));
    let (message, step) = if user_space_common {
        (
            format!(
                "No kernel driver is attached to {subject}. Many USB functions are used directly by applications and need none."
            ),
            STEP_NONE_UNLESS_BROKEN,
        )
    } else if !needs_driver {
        (
            format!(
                "No driver is attached to {subject}. Devices of this kind often work without one."
            ),
            STEP_NONE_UNLESS_BROKEN,
        )
    } else if modules.is_some() {
        (
            format!(
                "No driver is attached to {subject}, and this kernel has no loadable driver that claims it. The device will not work."
            ),
            STEP_UPDATE_THEN_REPORT,
        )
    } else {
        (
            format!(
                "No driver is attached to {subject}. Whether this kernel has a driver for it could not be checked. The device will not work."
            ),
            STEP_UPDATE_THEN_REPORT,
        )
    };
    finding(
        ProblemKind::NoDriver,
        weight,
        Some(device),
        message,
        step,
        detail,
    )
}

fn pci_detail(device: &PciDevice) -> Vec<(&'static str, String)> {
    vec![
        ("bus", "pci".to_owned()),
        ("address", device.address.clone()),
        (
            "ids",
            id_pair(device.vendor_id.as_deref(), device.device_id.as_deref()),
        ),
        (
            "subsystem_ids",
            id_pair(
                device.subsystem_vendor_id.as_deref(),
                device.subsystem_device_id.as_deref(),
            ),
        ),
        (
            "class",
            device.class.clone().unwrap_or_else(|| "unknown".to_owned()),
        ),
        (
            "modalias",
            device
                .modalias
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
        ),
        ("sysfs_path", device.sysfs_path.clone()),
    ]
}

/// Where a kernel device name leads in the inventory.
struct KnownDevice<'a> {
    id: &'a str,
    category: Category,
    subject: String,
    /// Whether the device currently lacks a driver.
    unbound: bool,
}

fn known_devices(inventory: &Inventory) -> BTreeMap<&str, KnownDevice<'_>> {
    let mut known = BTreeMap::new();
    for device in &inventory.pci {
        known.insert(
            device.address.as_str(),
            KnownDevice {
                id: &device.id,
                category: Category::Pci,
                subject: describe_pci(device),
                unbound: device.driver.is_none(),
            },
        );
    }
    for device in &inventory.usb {
        let product = device.product.as_deref().unwrap_or("USB device");
        known.insert(
            device.bus_port.as_str(),
            KnownDevice {
                id: &device.id,
                category: Category::Usb,
                subject: format!("{product} at USB port {}", device.bus_port),
                unbound: false,
            },
        );
        for interface in &device.interfaces {
            known.insert(
                interface.name.as_str(),
                KnownDevice {
                    id: &device.id,
                    category: Category::Usb,
                    subject: describe_usb(device, interface),
                    unbound: interface.driver.is_none(),
                },
            );
        }
    }
    for adapter in &inventory.bluetooth {
        known.insert(
            adapter.name.as_str(),
            KnownDevice {
                id: &adapter.id,
                category: Category::Bluetooth,
                subject: format!("the Bluetooth adapter {}", adapter.name),
                unbound: false,
            },
        );
    }
    known
}

/// PCI network controllers that have a driver but no network interface.
fn inactive_network_controllers(inventory: &Inventory) -> BTreeSet<&str> {
    // Without any interface at all the network class was probably not
    // readable, and nothing can be concluded.
    if inventory.network.is_empty() {
        return BTreeSet::new();
    }
    inventory
        .pci
        .iter()
        .filter(|device| {
            device.driver.is_some()
                && device
                    .class
                    .as_deref()
                    .is_some_and(|class| class.starts_with("0200") || class.starts_with("0280"))
                && !inventory
                    .network
                    .iter()
                    .any(|interface| interface.parent.as_deref() == Some(device.id.as_str()))
        })
        .map(|device| device.id.as_str())
        .collect()
}

fn firmware_finding(
    failure: &FirmwareLoadFailure,
    known: &BTreeMap<&str, KnownDevice<'_>>,
    inactive: &BTreeSet<&str>,
) -> Option<Finding> {
    let device = failure.device.as_deref().and_then(|name| known.get(name));
    let unusable = device.is_some_and(|device| device.unbound || inactive.contains(device.id));
    let subject = match (device, failure.device.as_deref()) {
        (Some(device), _) => device.subject.clone(),
        (None, Some(name)) => format!("the device {name}"),
        (None, None) => "a device".to_owned(),
    };
    let detail = vec![
        ("firmware_file", failure.firmware.clone()),
        (
            "kernel_device",
            failure
                .device
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
        ),
        (
            "driver",
            failure
                .driver
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
        ),
        ("kernel_error", error_text(failure.error)),
        (
            "evidence",
            format!(
                "kernel log: firmware load for {} failed with error {}",
                failure.firmware,
                error_text(failure.error)
            ),
        ),
    ];
    let (severity, consequence, step) = if unusable {
        (
            Severity::Warning,
            "The device does not work without it.",
            STEP_FIRMWARE,
        )
    } else {
        (
            Severity::Info,
            "Drivers often ask for optional files, so the device may still work.",
            STEP_FIRMWARE_OPTIONAL,
        )
    };
    finding(
        ProblemKind::FirmwareMissing,
        severity,
        device.map(|device| (device.id, device.category)),
        format!(
            "The kernel could not load the firmware file {} for {subject}. {consequence}",
            failure.firmware
        ),
        step,
        detail,
    )
}

/// Find the problems an inventory shows, most severe first.
///
/// Findings rest on what the inventory holds. Without the kernel log
/// (`inventory.kernel.available` is false) a driver that failed to bind is
/// inferred from sysfs alone, and missing firmware cannot be seen at all.
pub fn detect(inventory: &Inventory) -> Vec<Finding> {
    let mut findings: Vec<Finding> = Vec::new();
    let known = known_devices(inventory);
    let inactive = inactive_network_controllers(inventory);

    for device in &inventory.pci {
        if device.driver.is_some() {
            continue;
        }
        findings.extend(unbound_finding(
            inventory,
            Unbound {
                subject: describe_pci(device),
                device: (device.id.as_str(), Category::Pci),
                kernel_name: &device.address,
                modules: device.kernel_modules.as_deref(),
                needs_driver: pci_needs_driver(device.class.as_deref()),
                user_space_common: false,
                detail: pci_detail(device),
            },
        ));
    }

    for device in &inventory.usb {
        for interface in &device.interfaces {
            if interface.driver.is_some() {
                continue;
            }
            let detail = vec![
                ("bus", "usb".to_owned()),
                ("port", device.bus_port.clone()),
                ("interface", interface.name.clone()),
                (
                    "ids",
                    id_pair(device.vendor_id.as_deref(), device.product_id.as_deref()),
                ),
                (
                    "interface_class",
                    interface
                        .class
                        .clone()
                        .unwrap_or_else(|| "unknown".to_owned()),
                ),
                (
                    "modalias",
                    interface
                        .modalias
                        .clone()
                        .unwrap_or_else(|| "unknown".to_owned()),
                ),
                ("sysfs_path", device.sysfs_path.clone()),
            ];
            findings.extend(unbound_finding(
                inventory,
                Unbound {
                    subject: describe_usb(device, interface),
                    device: (device.id.as_str(), Category::Usb),
                    kernel_name: &interface.name,
                    modules: interface.kernel_modules.as_deref(),
                    needs_driver: false,
                    user_space_common: true,
                    detail,
                },
            ));
        }
    }

    // Drivers the kernel says refused a device that is not in the inventory
    // (a platform or I2C device, for example).
    for failure in &inventory.kernel.probe_failures {
        if known.contains_key(failure.device.as_str()) {
            continue;
        }
        let driver = failure.driver.as_deref().unwrap_or("a driver");
        findings.extend(finding(
            ProblemKind::DriverNotBound,
            Severity::Warning,
            None,
            format!(
                "The driver {driver} did not accept the device {} (kernel error {}). Part of the hardware may not work.",
                failure.device,
                error_text(failure.error)
            ),
            STEP_NOT_BOUND,
            vec![
                ("kernel_device", failure.device.clone()),
                ("driver", driver.to_owned()),
                ("kernel_error", error_text(failure.error)),
                (
                    "evidence",
                    format!(
                        "kernel log: probe of {} with driver {driver} failed with error {}",
                        failure.device,
                        error_text(failure.error)
                    ),
                ),
            ],
        ));
    }

    for failure in &inventory.kernel.firmware_failures {
        findings.extend(firmware_finding(failure, &known, &inactive));
    }

    for device in &inventory.pci {
        if !inactive.contains(device.id.as_str()) {
            continue;
        }
        let driver = device.driver.as_deref().unwrap_or("unknown");
        let mut detail = pci_detail(device);
        detail.push(("driver", driver.to_owned()));
        detail.push((
            "evidence",
            "sysfs: a driver is bound, but no network interface has this device as its parent"
                .to_owned(),
        ));
        findings.extend(finding(
            ProblemKind::DeviceInactive,
            Severity::Warning,
            Some((device.id.as_str(), Category::Pci)),
            format!(
                "{} has a driver ({driver}) but no network interface appeared. A common cause is firmware that is missing or did not start.",
                capitalised(&describe_pci(device))
            ),
            STEP_INACTIVE,
            detail,
        ));
    }

    for device in &inventory.thunderbolt {
        if device.is_host || device.authorised != ThunderboltAuthorisation::NotAuthorised {
            continue;
        }
        let name = device.device_name.as_deref().unwrap_or("A device");
        findings.extend(finding(
            ProblemKind::ThunderboltNotAuthorised,
            Severity::Info,
            Some((device.id.as_str(), Category::Thunderbolt)),
            format!(
                "{name} is connected through Thunderbolt but has not been authorised, so it cannot be used yet."
            ),
            STEP_THUNDERBOLT,
            vec![
                ("bus", "thunderbolt".to_owned()),
                ("kernel_device", device.name.clone()),
                (
                    "security_level",
                    device
                        .security_level
                        .clone()
                        .unwrap_or_else(|| "unknown".to_owned()),
                ),
                ("sysfs_path", device.sysfs_path.clone()),
                ("evidence", "sysfs: authorized is 0".to_owned()),
            ],
        ));
    }

    // Most severe first; the sort is stable, so within one severity and code
    // the findings stay in inventory order.
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.code.cmp(&b.code))
    });
    findings
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect;
    use crate::probe::{FakeProbe, ProbeCommand};
    use uos_core::Family;

    #[test]
    fn every_code_is_a_valid_device_code() {
        let mut seen = BTreeSet::new();
        for kind in ProblemKind::ALL {
            let code = kind.code().expect("the code is valid");
            assert_eq!(code.family(), Family::Device, "{kind:?}");
            assert_eq!(code.as_str(), kind.code_text());
            assert!(seen.insert(code), "{kind:?} repeats a code");
            assert!(!kind.title().is_empty());
        }
    }

    /// A PCI device with no driver, of the given class and modalias.
    fn pci_without_driver(class: &str, modalias: &str) -> FakeProbe {
        let dir = "/sys/devices/pci0000:00/0000:00:05.0";
        FakeProbe::new()
            .link(
                "/sys/bus/pci/devices/0000:00:05.0",
                "../../../devices/pci0000:00/0000:00:05.0",
            )
            .file(&format!("{dir}/vendor"), "0x10de\n")
            .file(&format!("{dir}/device"), "0x25a2\n")
            .file(&format!("{dir}/class"), class)
            .file(&format!("{dir}/modalias"), modalias)
    }

    fn with_aliases(probe: FakeProbe, table: &str) -> FakeProbe {
        probe
            .file("/proc/sys/kernel/osrelease", "6.17.0-test\n")
            .file("/usr/lib/modules/6.17.0-test/modules.alias", table)
    }

    const GPU_MODALIAS: &str = "pci:v000010DEd000025A2sv000017AAsd000022D5bc03sc02i00\n";
    const NOUVEAU: &str = "alias pci:v000010DEd*sv*sd*bc03sc*i* nouveau\n";

    fn only(findings: &[Finding]) -> &Finding {
        assert_eq!(findings.len(), 1, "{findings:#?}");
        &findings[0]
    }

    #[test]
    fn a_device_no_module_claims_has_no_driver() {
        let probe = with_aliases(pci_without_driver("0x030200\n", GPU_MODALIAS), "");
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-DRIVER-001");
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(
            finding.device_id.as_deref(),
            Some("pci-0000:00:05.0-10de:25a2")
        );
        assert_eq!(finding.category, Some(Category::Pci));
        assert!(finding.user_message.contains("0000:00:05.0"));
        assert!(finding.user_message.contains("no loadable driver"));
        assert_eq!(finding.technical_detail["matching_modules"], "none");
        assert_eq!(finding.technical_detail["ids"], "10de:25a2");
        assert_eq!(finding.technical_detail["class"], "030200");
    }

    #[test]
    fn an_unchecked_device_says_it_could_not_be_checked() {
        let probe = pci_without_driver("0x030200\n", GPU_MODALIAS);
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-DRIVER-001");
        assert!(finding.user_message.contains("could not be checked"));
        assert!(finding.technical_detail["matching_modules"].starts_with("unknown"));
    }

    #[test]
    fn a_bridge_without_a_driver_is_only_information() {
        let probe = with_aliases(pci_without_driver("0x060000\n", "pci:x\n"), "");
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-DRIVER-001");
        assert_eq!(finding.severity, Severity::Info);
        assert!(finding.user_message.contains("often work without one"));
    }

    #[test]
    fn a_matching_module_that_is_not_loaded_is_reported() {
        let probe = with_aliases(pci_without_driver("0x030200\n", GPU_MODALIAS), NOUVEAU);
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-DRIVER-002");
        assert_eq!(finding.severity, Severity::Warning);
        assert!(finding.user_message.contains("nouveau"));
        assert_eq!(
            finding.technical_detail["matching_modules"],
            "nouveau (not loaded)"
        );
    }

    #[test]
    fn a_loaded_module_that_is_not_bound_failed_to_bind() {
        let probe = with_aliases(pci_without_driver("0x030200\n", GPU_MODALIAS), NOUVEAU)
            .file("/sys/module/nouveau/initstate", "live\n");
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-DRIVER-003");
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(
            finding.technical_detail["matching_modules"],
            "nouveau (loaded)"
        );
        assert!(finding.technical_detail["evidence"].starts_with("sysfs:"));
    }

    #[test]
    fn a_probe_failure_in_the_kernel_log_is_an_error() {
        let probe = with_aliases(pci_without_driver("0x030200\n", GPU_MODALIAS), NOUVEAU).command(
            ProbeCommand::KernelLog,
            "pci 0000:00:05.0: probe with driver nouveau failed with error -12\n",
        );
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-DRIVER-003");
        assert_eq!(finding.severity, Severity::Error);
        assert!(finding.user_message.contains("nouveau"));
        assert!(finding.user_message.contains("-12"));
        assert!(finding.technical_detail["evidence"].starts_with("kernel log:"));
    }

    #[test]
    fn a_probe_failure_of_an_unlisted_device_is_still_reported() {
        let probe = FakeProbe::new().command(
            ProbeCommand::KernelLog,
            "i2c_hid_acpi i2c-ELAN0000:00: probe with driver i2c_hid_acpi failed with error -110\n",
        );
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-DRIVER-003");
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(finding.device_id, None);
        assert_eq!(finding.category, None);
        assert_eq!(finding.technical_detail["kernel_device"], "i2c-ELAN0000:00");
    }

    #[test]
    fn a_probe_failure_of_a_device_that_now_has_a_driver_is_not_a_problem() {
        let probe = pci_without_driver("0x030200\n", GPU_MODALIAS)
            .link(
                "/sys/devices/pci0000:00/0000:00:05.0/driver",
                "../../../bus/pci/drivers/nouveau",
            )
            .command(
                ProbeCommand::KernelLog,
                "pci 0000:00:05.0: probe with driver nvidiafb failed with error -19\n",
            );
        assert!(detect(&collect(&probe)).is_empty());
    }

    #[test]
    fn missing_firmware_is_a_warning_only_if_the_device_is_unusable() {
        let log = "nouveau 0000:00:05.0: Direct firmware load for nvidia/tu117/gsp.bin failed with error -2\n";
        // Driver bound: the file may be optional.
        let bound = pci_without_driver("0x030200\n", GPU_MODALIAS)
            .link(
                "/sys/devices/pci0000:00/0000:00:05.0/driver",
                "../../../bus/pci/drivers/nouveau",
            )
            .command(ProbeCommand::KernelLog, log);
        let findings = detect(&collect(&bound));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-FIRMWARE-001");
        assert_eq!(finding.severity, Severity::Info);
        assert_eq!(
            finding.device_id.as_deref(),
            Some("pci-0000:00:05.0-10de:25a2")
        );
        assert_eq!(
            finding.technical_detail["firmware_file"],
            "nvidia/tu117/gsp.bin"
        );
        assert!(finding.user_message.contains("may still work"));

        // No driver bound: the device does not work.
        let unbound =
            pci_without_driver("0x030200\n", GPU_MODALIAS).command(ProbeCommand::KernelLog, log);
        let findings = detect(&collect(&unbound));
        assert_eq!(findings.len(), 2);
        let firmware = findings
            .iter()
            .find(|finding| finding.code.as_str() == "UOS-DEVICE-FIRMWARE-001")
            .unwrap();
        assert_eq!(firmware.severity, Severity::Warning);
        assert!(firmware.user_message.contains("does not work without it"));
    }

    #[test]
    fn a_network_controller_without_an_interface_is_inactive() {
        let dir = "/sys/devices/pci0000:00/0000:03:00.0";
        let probe = FakeProbe::new()
            .link(
                "/sys/bus/pci/devices/0000:03:00.0",
                "../../../devices/pci0000:00/0000:03:00.0",
            )
            .file(&format!("{dir}/vendor"), "0x10ec\n")
            .file(&format!("{dir}/device"), "0xc822\n")
            .file(&format!("{dir}/class"), "0x028000\n")
            .link(
                &format!("{dir}/driver"),
                "../../../bus/pci/drivers/rtw_8822ce",
            )
            // Some interface must exist, or nothing can be concluded.
            .file("/sys/devices/virtual/net/lo/type", "772\n")
            .link("/sys/class/net/lo", "../../devices/virtual/net/lo");
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-INACTIVE-001");
        assert_eq!(finding.severity, Severity::Warning);
        assert!(finding.user_message.starts_with("The Network controller"));
        assert_eq!(finding.technical_detail["driver"], "rtw_8822ce");

        // With its interface present there is nothing to report.
        let healthy = probe.file(&format!("{dir}/net/wlan0/type"), "1\n").link(
            "/sys/class/net/wlan0",
            "../../devices/pci0000:00/0000:03:00.0/net/wlan0",
        );
        assert!(detect(&collect(&healthy)).is_empty());

        // With no interface readable at all, nothing is concluded.
        let unreadable = healthy.remove("/sys/class/net");
        assert!(detect(&collect(&unreadable)).is_empty());
    }

    #[test]
    fn a_usb_function_without_a_driver_is_information() {
        let dir = "/sys/devices/pci0000:00/0000:00:14.0/usb1/1-3";
        let probe = FakeProbe::new()
            .link(
                "/sys/bus/usb/devices/1-3",
                "../../../devices/pci0000:00/0000:00:14.0/usb1/1-3",
            )
            .file(&format!("{dir}/idVendor"), "06cb\n")
            .file(&format!("{dir}/idProduct"), "00bd\n")
            .file(&format!("{dir}/product"), "Fingerprint Reader\n")
            .file(&format!("{dir}/1-3:1.0/bInterfaceClass"), "ff\n");
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-DRIVER-001");
        assert_eq!(finding.severity, Severity::Info);
        assert_eq!(finding.category, Some(Category::Usb));
        assert!(finding.user_message.contains("Fingerprint Reader"));
        assert!(
            finding
                .user_message
                .contains("used directly by applications")
        );
        assert_eq!(finding.technical_detail["interface"], "1-3:1.0");
    }

    #[test]
    fn an_unauthorised_thunderbolt_device_is_reported() {
        let dir = "/sys/devices/pci0000:00/0000:00:0d.2/domain0/0-0/0-1";
        let probe = FakeProbe::new()
            .link(
                "/sys/bus/thunderbolt/devices/0-1",
                "../../../devices/pci0000:00/0000:00:0d.2/domain0/0-0/0-1",
            )
            .file(&format!("{dir}/authorized"), "0\n")
            .file(&format!("{dir}/device_name"), "TS3 Plus\n");
        let findings = detect(&collect(&probe));
        let finding = only(&findings);
        assert_eq!(finding.code.as_str(), "UOS-DEVICE-THUNDERBOLT-001");
        assert_eq!(finding.severity, Severity::Info);
        assert!(finding.user_message.starts_with("TS3 Plus is connected"));

        let authorised = probe.file(&format!("{dir}/authorized"), "1\n");
        assert!(detect(&collect(&authorised)).is_empty());
    }

    #[test]
    fn findings_are_ordered_most_severe_first() {
        let probe = with_aliases(pci_without_driver("0x060000\n", "pci:x\n"), "").command(
            ProbeCommand::KernelLog,
            "a b: Direct firmware load for x.bin failed with error -2\n\
             drv 0000:00:05.0: probe with driver drv failed with error -5\n\
             other: probe of platform-dev failed with error -22\n",
        );
        let findings = detect(&collect(&probe));
        let order: Vec<(Severity, &str)> = findings
            .iter()
            .map(|finding| (finding.severity, finding.code.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                (Severity::Error, "UOS-DEVICE-DRIVER-003"),
                (Severity::Warning, "UOS-DEVICE-DRIVER-003"),
                (Severity::Info, "UOS-DEVICE-FIRMWARE-001"),
            ]
        );
    }

    #[test]
    fn no_finding_suggests_a_driver_from_another_operating_system() {
        let steps = [
            STEP_UPDATE_THEN_REPORT,
            STEP_NONE_UNLESS_BROKEN,
            STEP_NOT_LOADED,
            STEP_NOT_BOUND,
            STEP_FIRMWARE,
            STEP_FIRMWARE_OPTIONAL,
            STEP_INACTIVE,
            STEP_THUNDERBOLT,
        ];
        for step in steps {
            let lower = step.to_lowercase();
            for word in [
                "windows",
                "macos",
                "mac os",
                ".sys",
                ".kext",
                ".inf",
                "ndiswrapper",
                "vendor's",
                "download",
            ] {
                assert!(!lower.contains(word), "{step}");
            }
        }
    }

    #[test]
    fn a_finding_serialises_with_its_code_as_text() {
        let probe = pci_without_driver("0x030200\n", GPU_MODALIAS);
        let report = ProblemReport::of(&collect(&probe));
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["findings"][0]["code"], "UOS-DEVICE-DRIVER-001");
        assert_eq!(json["findings"][0]["severity"], "warning");
        assert_eq!(json["findings"][0]["category"], "pci");
    }
}
