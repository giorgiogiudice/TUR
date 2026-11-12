// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Building the inventory from a [`Probe`].
//!
//! [`collect`] never fails and never panics: a source that is missing,
//! unreadable or malformed leaves a `null`, an empty list or a
//! [`SourceNote`], and the rest of the inventory is still built. Everything
//! read is bounded (see [`crate::text`] and [`crate::probe`]).
//!
//! Sources, in the order of specification §10.2: sysfs attributes, the
//! `driver` symbolic link, `modalias` matched against the kernel's own alias
//! table, and udev properties. There is no device or driver table in this
//! crate beyond the names of the PCI and USB class codes.

use std::collections::{BTreeMap, BTreeSet};

use crate::fwupd;
use crate::ids::{Candidate, IdMaker, assign};
use crate::kernel_log;
use crate::modalias::ModuleAliases;
use crate::model::{
    AudioCard, BlockDevice, BlockTransport, BluetoothAdapter, BootMode, Camera, ConnectorStatus,
    DrmCard, DrmConnector, Firmware, FwupdStatus, Gpu, IdBasis, InputBus, InputDevice, InputKind,
    Inventory, KernelEvidence, ModuleMatch, NetKind, NetworkInterface, OperState, PciDevice,
    Platform, Removable, Rfkill, SCHEMA_VERSION, SecureBoot, Sensitive, SourceNote, SourceStatus,
    StorageController, StorageControllerKind, ThunderboltAuthorisation, ThunderboltDevice,
    UsbDevice, UsbInterface,
};
use crate::probe::{MAX_PATH_BYTES, Probe, ProbeCommand, ProbeError};
use crate::text::{
    MAX_ATTR_BYTES, MAX_MODALIAS_CHARS, MAX_PATH_CHARS, MAX_TEXT_CHARS, clean, clean_to, flag,
    hex_id, id_token, is_mac_address, kernel_name, natural_key,
};
use crate::udev::UdevDb;

/// The most bytes read from `/proc/asound/cards`.
const MAX_ASOUND_BYTES: usize = 64 * 1024;

/// The most bytes read from a kernel's `modules.alias`.
const MAX_ALIAS_BYTES: usize = 32 * 1024 * 1024;

/// The most sound cards listed.
const MAX_SOUND_CARDS: usize = 64;

/// How many directories above a class device are searched for its driver.
const MAX_DRIVER_LEVELS: usize = 6;

/// The EFI variable that holds the Secure Boot state.
const SECURE_BOOT_VARIABLE: &str =
    "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// Build the inventory. See the module documentation.
pub fn collect(probe: &dyn Probe) -> Inventory {
    let mut scanner = Scanner::new(probe);
    let platform = scanner.platform();
    // PCI and USB first: every later category refers to them as parents.
    let pci = scanner.pci();
    let usb = scanner.usb();
    let network = scanner.network();
    let audio = scanner.audio();
    let storage_controllers = storage_controllers(&pci, &usb);
    let block = scanner.block();
    let input = scanner.input();
    let bluetooth = scanner.bluetooth();
    let cameras = scanner.cameras();
    let thunderbolt = scanner.thunderbolt();
    let firmware = scanner.firmware();
    let kernel = scanner.kernel();
    Inventory {
        schema_version: SCHEMA_VERSION,
        redacted: false,
        platform,
        pci,
        usb,
        network,
        audio,
        storage_controllers,
        block,
        input,
        bluetooth,
        cameras,
        thunderbolt,
        firmware,
        kernel,
        source_notes: scanner.notes,
    }
}

trait Identified {
    fn set_identity(&mut self, id: String, basis: IdBasis);
}

macro_rules! identified {
    ($($device:ty),+ $(,)?) => {
        $(
            impl Identified for $device {
                fn set_identity(&mut self, id: String, basis: IdBasis) {
                    self.id = id;
                    self.id_basis = basis;
                }
            }
        )+
    };
}

identified!(
    PciDevice,
    UsbDevice,
    NetworkInterface,
    AudioCard,
    BlockDevice,
    InputDevice,
    BluetoothAdapter,
    Camera,
    ThunderboltDevice,
);

/// A device, the identifiers it could have, and its directory in sysfs.
type Found<T> = Vec<(T, Candidate, String)>;

struct Scanner<'a> {
    probe: &'a dyn Probe,
    udev: UdevDb,
    ids: IdMaker,
    aliases: Option<ModuleAliases>,
    /// The identifier of each PCI and USB device, by its sysfs directory.
    parents: BTreeMap<String, (String, IdBasis)>,
    /// The next index to give a device under one identifier stem.
    counters: BTreeMap<String, u32>,
    notes: Vec<SourceNote>,
}

fn status_of(error: &ProbeError) -> SourceStatus {
    match error {
        ProbeError::Denied => SourceStatus::Denied,
        ProbeError::TooLarge => SourceStatus::TooLarge,
        ProbeError::NotFound | ProbeError::TimedOut | ProbeError::Failed(_) => {
            SourceStatus::Unavailable
        }
    }
}

fn note(source: &str, status: SourceStatus, effect: &str) -> SourceNote {
    SourceNote {
        source: source.to_owned(),
        status,
        effect: effect.to_owned(),
    }
}

/// Whether a directory entry name can be used: bounded, a single path
/// component, and already clean text.
fn is_usable_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains('/')
        && clean(name.as_bytes()) == name
}

/// The number after a prefix, as in `card0` or `input12`.
fn numbered(name: &str, prefix: &str) -> Option<u32> {
    let digits = name.strip_prefix(prefix)?;
    if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Lower-case hexadecimal digits, as sysfs writes addresses.
fn is_hex(text: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether a name is a PCI address: `dddd:bb:dd.f`.
fn is_pci_address(name: &str) -> bool {
    let mut parts = name.split(':');
    let (Some(domain), Some(bus), Some(slot), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let Some((device, function)) = slot.split_once('.') else {
        return false;
    };
    is_hex(domain, 4, 8) && is_hex(bus, 2, 2) && is_hex(device, 2, 2) && is_hex(function, 1, 1)
}

/// Resolve a symbolic link's target against the directory that holds the
/// link, without touching the filesystem.
fn resolve_link(link_path: &str, target: &str) -> Option<String> {
    if target.len() > MAX_PATH_BYTES {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        parts.extend(link_path.split('/').filter(|part| !part.is_empty()));
        parts.pop();
    }
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    let resolved = format!("/{}", parts.join("/"));
    (resolved.len() <= MAX_PATH_BYTES).then_some(resolved)
}

fn display_path(dir: &str) -> String {
    clean_to(dir.as_bytes(), MAX_PATH_CHARS)
}

/// The name of a PCI base class (PCI Code and ID Assignment Specification).
fn pci_class_name(class: &str) -> Option<&'static str> {
    Some(match class.get(..2)? {
        "00" => "Unclassified device",
        "01" => "Mass storage controller",
        "02" => "Network controller",
        "03" => "Display controller",
        "04" => "Multimedia controller",
        "05" => "Memory controller",
        "06" => "Bridge",
        "07" => "Communication controller",
        "08" => "Generic system peripheral",
        "09" => "Input device controller",
        "0a" => "Docking station",
        "0b" => "Processor",
        "0c" => "Serial bus controller",
        "0d" => "Wireless controller",
        "0e" => "Intelligent controller",
        "0f" => "Satellite communications controller",
        "10" => "Encryption controller",
        "11" => "Signal processing controller",
        "12" => "Processing accelerator",
        "13" => "Non-essential instrumentation",
        "40" => "Coprocessor",
        "ff" => "Unassigned class",
        _ => return None,
    })
}

/// The name of a USB class (USB-IF defined class codes).
fn usb_class_name(class: &str) -> Option<&'static str> {
    Some(match class {
        "00" => "Defined by the interfaces",
        "01" => "Audio",
        "02" => "Communications",
        "03" => "Human interface device",
        "05" => "Physical",
        "06" => "Imaging",
        "07" => "Printer",
        "08" => "Mass storage",
        "09" => "Hub",
        "0a" => "Communications data",
        "0b" => "Smart card",
        "0d" => "Content security",
        "0e" => "Video",
        "0f" => "Personal healthcare",
        "10" => "Audio and video",
        "11" => "Billboard",
        "12" => "USB Type-C bridge",
        "dc" => "Diagnostic",
        "e0" => "Wireless controller",
        "ef" => "Miscellaneous",
        "fe" => "Application specific",
        "ff" => "Vendor specific",
        _ => return None,
    })
}

fn storage_kind(class: &str) -> Option<StorageControllerKind> {
    if class.starts_with("0805") {
        return Some(StorageControllerKind::SdHost);
    }
    if !class.starts_with("01") {
        return None;
    }
    Some(match class.get(2..4) {
        Some("00") => StorageControllerKind::Scsi,
        Some("01" | "05") => StorageControllerKind::Ata,
        Some("04") => StorageControllerKind::Raid,
        Some("06") => StorageControllerKind::Sata,
        Some("07") => StorageControllerKind::Sas,
        Some("08") => StorageControllerKind::Nvme,
        Some("09") => StorageControllerKind::Ufs,
        _ => StorageControllerKind::Other,
    })
}

/// Storage controllers are a view of devices already listed: PCI functions of
/// the mass-storage class and USB mass-storage interfaces.
fn storage_controllers(pci: &[PciDevice], usb: &[UsbDevice]) -> Vec<StorageController> {
    let mut controllers = Vec::new();
    for device in pci {
        let Some(kind) = device.class.as_deref().and_then(storage_kind) else {
            continue;
        };
        controllers.push(StorageController {
            id: format!("storage-{}", device.id),
            id_basis: device.id_basis,
            sysfs_path: device.sysfs_path.clone(),
            kind,
            driver: device.driver.clone(),
            parent: device.id.clone(),
        });
    }
    for device in usb {
        for (index, interface) in device.interfaces.iter().enumerate() {
            if interface.class.as_deref() != Some("08") {
                continue;
            }
            controllers.push(StorageController {
                id: format!("storage-{}-if{index}", device.id),
                id_basis: device.id_basis,
                sysfs_path: display_path(&format!("{}/{}", device.sysfs_path, interface.name)),
                kind: StorageControllerKind::UsbMassStorage,
                driver: interface.driver.clone(),
                parent: device.id.clone(),
            });
        }
    }
    controllers
}

/// One card of `/proc/asound/cards`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProcCard {
    id: Option<String>,
    driver: Option<String>,
    name: Option<String>,
    long_name: Option<String>,
}

fn non_empty(text: &str) -> Option<String> {
    let cleaned = clean(text.as_bytes());
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Parse `/proc/asound/cards`:
///
/// ```text
///  0 [PCH            ]: HDA-Intel - HDA Intel PCH
///                       HDA Intel PCH at 0xf7f10000 irq 30
/// ```
fn parse_asound_cards(raw: &[u8]) -> BTreeMap<u32, ProcCard> {
    let text = String::from_utf8_lossy(raw);
    let mut cards = BTreeMap::new();
    let mut last: Option<u32> = None;
    for line in text.lines() {
        let header = line.split_once('[').and_then(|(number, rest)| {
            let index = numbered(number.trim(), "")?;
            let (id, description) = rest.split_once("]:")?;
            Some((index, id, description))
        });
        match header {
            Some((index, id, description)) => {
                let (driver, name) = match description.split_once(" - ") {
                    Some((driver, name)) => (non_empty(driver), non_empty(name)),
                    None => (None, non_empty(description)),
                };
                if cards.len() < MAX_SOUND_CARDS {
                    cards.insert(
                        index,
                        ProcCard {
                            id: non_empty(id),
                            driver,
                            name,
                            long_name: None,
                        },
                    );
                    last = Some(index);
                } else {
                    last = None;
                }
            }
            None => {
                // The line after a card is its long name.
                if let Some(index) = last.take()
                    && let Some(card) = cards.get_mut(&index)
                {
                    card.long_name = non_empty(line);
                }
            }
        }
    }
    cards
}

impl<'a> Scanner<'a> {
    fn new(probe: &'a dyn Probe) -> Self {
        let mut notes = Vec::new();

        let machine_id = probe.read("/etc/machine-id", 128);
        let ids = IdMaker::new(
            machine_id
                .as_ref()
                .ok()
                .map(|raw| String::from_utf8_lossy(raw).into_owned())
                .as_deref(),
        );
        if !ids.can_hash() {
            let status = match &machine_id {
                Ok(_) => SourceStatus::Invalid,
                Err(error) => status_of(error),
            };
            notes.push(note(
                "machine-id",
                status,
                "Device identifiers are derived from position only; none is derived from a serial number or address.",
            ));
        }

        let udev = match probe.run(ProbeCommand::UdevExportDb) {
            Ok(raw) => {
                let db = UdevDb::parse(&raw);
                if db.is_empty() {
                    notes.push(note(
                        "udev",
                        SourceStatus::Invalid,
                        "No udev properties: hardware database names, input device kinds and camera capabilities are missing.",
                    ));
                }
                db
            }
            Err(error) => {
                notes.push(note(
                    "udev",
                    status_of(&error),
                    "No udev properties: hardware database names, input device kinds and camera capabilities are missing.",
                ));
                UdevDb::default()
            }
        };

        let aliases = Self::load_aliases(probe);
        if aliases.is_none() {
            notes.push(note(
                "kernel-module-aliases",
                SourceStatus::Unavailable,
                "Devices cannot be matched to kernel modules, so a missing driver cannot be told from one that is not loaded.",
            ));
        }

        Self {
            probe,
            udev,
            ids,
            aliases,
            parents: BTreeMap::new(),
            counters: BTreeMap::new(),
            notes,
        }
    }

    fn load_aliases(probe: &dyn Probe) -> Option<ModuleAliases> {
        let release = probe.read("/proc/sys/kernel/osrelease", 256).ok()?;
        let release = kernel_name(&String::from_utf8_lossy(&release))?;
        let raw = probe
            .read(
                &format!("/usr/lib/modules/{release}/modules.alias"),
                MAX_ALIAS_BYTES,
            )
            .or_else(|_| {
                probe.read(
                    &format!("/lib/modules/{release}/modules.alias"),
                    MAX_ALIAS_BYTES,
                )
            })
            .ok()?;
        Some(ModuleAliases::parse(&raw))
    }

    fn text_limited(&self, path: &str, max_chars: usize) -> Option<String> {
        let raw = self.probe.read(path, MAX_ATTR_BYTES).ok()?;
        let cleaned = clean_to(&raw, max_chars);
        (!cleaned.is_empty()).then_some(cleaned)
    }

    fn attr(&self, dir: &str, name: &str) -> Option<String> {
        self.text_limited(&format!("{dir}/{name}"), MAX_TEXT_CHARS)
    }

    fn hex(&self, dir: &str, name: &str, digits: usize) -> Option<String> {
        self.attr(dir, name)
            .and_then(|value| hex_id(&value, digits))
    }

    fn flag(&self, dir: &str, name: &str) -> Option<bool> {
        self.attr(dir, name).and_then(|value| flag(&value))
    }

    fn sensitive(&self, dir: &str, name: &str) -> Sensitive {
        Sensitive::from_option(self.attr(dir, name))
    }

    fn is_dir(&self, path: &str) -> bool {
        self.probe.list(path).is_ok()
    }

    /// The usable names in a directory, in natural order.
    fn entries(&self, dir: &str) -> Vec<String> {
        let mut names = self.probe.list(dir).unwrap_or_default();
        names.retain(|name| is_usable_name(name));
        names.sort_by_cached_key(|name| natural_key(name));
        names
    }

    /// The directory a class or bus entry points to. Entries under
    /// `/sys/class` and `/sys/bus` are symbolic links into `/sys/devices`.
    fn canonical(&self, entry: &str) -> String {
        match self.probe.read_link(entry) {
            Ok(target) => resolve_link(entry, &target).unwrap_or_else(|| entry.to_owned()),
            Err(_) => entry.to_owned(),
        }
    }

    /// The name of the driver bound to the device in `dir`, from its `driver`
    /// symbolic link.
    fn driver(&self, dir: &str) -> Option<String> {
        let target = self.probe.read_link(&format!("{dir}/driver")).ok()?;
        kernel_name(target.rsplit('/').next()?)
    }

    /// The driver of the nearest device above a class device. A network
    /// interface, sound card or disk has no `driver` link of its own; the
    /// device it belongs to, a few directories up, has.
    fn nearest_driver(&self, dir: &str) -> Option<String> {
        let mut current = dir;
        for _ in 0..MAX_DRIVER_LEVELS {
            let (parent, _) = current.rsplit_once('/')?;
            if parent.is_empty() || parent == "/sys" || parent == "/sys/devices" {
                return None;
            }
            if let Some(driver) = self.driver(parent) {
                return Some(driver);
            }
            current = parent;
        }
        None
    }

    fn uevent(&self, dir: &str, key: &str) -> Option<String> {
        let raw = self
            .probe
            .read(&format!("{dir}/uevent"), MAX_ATTR_BYTES)
            .ok()?;
        let text = String::from_utf8_lossy(&raw);
        text.lines()
            .filter_map(|line| line.split_once('='))
            .find(|(name, _)| *name == key)
            .and_then(|(_, value)| non_empty(value))
    }

    fn modalias(&self, dir: &str) -> Option<String> {
        self.text_limited(&format!("{dir}/modalias"), MAX_MODALIAS_CHARS)
            .filter(|alias| alias.bytes().all(|b| b.is_ascii_graphic() || b == b' '))
    }

    fn modules(&self, modalias: Option<&str>) -> Option<Vec<ModuleMatch>> {
        let aliases = self.aliases.as_ref()?;
        let Some(modalias) = modalias else {
            return Some(Vec::new());
        };
        Some(
            aliases
                .matching(modalias)
                .into_iter()
                .map(|name| {
                    let loaded = self.is_dir(&format!("/sys/module/{name}"));
                    ModuleMatch { name, loaded }
                })
                .collect(),
        )
    }

    fn udev_text(&self, dir: &str, key: &str) -> Option<String> {
        self.udev.property(dir, key).map(str::to_owned)
    }

    /// The nearest PCI or USB device above a sysfs directory.
    fn parent_of(&self, dir: &str) -> Option<(String, IdBasis)> {
        let mut current = dir;
        while let Some((parent, _)) = current.rsplit_once('/') {
            if parent.is_empty() {
                break;
            }
            if let Some(found) = self.parents.get(parent) {
                return Some(found.clone());
            }
            current = parent;
        }
        None
    }

    fn next_index(&mut self, stem: &str) -> u32 {
        let counter = self.counters.entry(stem.to_owned()).or_insert(0);
        let index = *counter;
        *counter = counter.saturating_add(1);
        index
    }

    /// The fallback identifier of a device that hangs off a PCI or USB
    /// device: the parent's identifier and an index among its siblings.
    /// Without such a parent, the kernel name.
    fn child_candidate(
        &mut self,
        category: &str,
        dir: &str,
        fallback_name: &str,
    ) -> (Candidate, Option<String>) {
        match self.parent_of(dir) {
            Some((parent, basis)) => {
                let stem = format!("{category}-{parent}");
                let index = self.next_index(&stem);
                (
                    Candidate {
                        hardware: None,
                        fallback: format!("{stem}-{index}"),
                        fallback_basis: basis,
                    },
                    Some(parent),
                )
            }
            None => (
                Candidate {
                    hardware: None,
                    fallback: format!("{category}-name-{}", id_token(fallback_name)),
                    fallback_basis: IdBasis::KernelName,
                },
                None,
            ),
        }
    }

    /// Give every found device its identifier. With `register`, record the
    /// devices as possible parents of later ones.
    fn finish<T: Identified>(&mut self, found: Found<T>, register: bool) -> Vec<T> {
        let candidates: Vec<Candidate> = found
            .iter()
            .map(|(_, candidate, _)| candidate.clone())
            .collect();
        let assigned = assign(&candidates);
        let mut devices = Vec::with_capacity(found.len());
        for ((mut device, _, dir), (id, basis)) in found.into_iter().zip(assigned) {
            if register {
                self.parents.insert(dir, (id.clone(), basis));
            }
            device.set_identity(id, basis);
            devices.push(device);
        }
        devices
    }

    fn note_error(&mut self, source: &str, error: &ProbeError, effect: &str) {
        self.notes.push(note(source, status_of(error), effect));
    }

    fn platform(&self) -> Platform {
        let dmi = "/sys/class/dmi/id";
        let boot_mode = if self.is_dir("/sys/firmware/efi") {
            BootMode::Uefi
        } else if self.is_dir("/sys/firmware") {
            BootMode::Bios
        } else {
            BootMode::Unknown
        };
        let secure_boot = match boot_mode {
            BootMode::Uefi => self.secure_boot(),
            BootMode::Bios => SecureBoot::NotApplicable,
            BootMode::Unknown => SecureBoot::Unknown,
        };
        Platform {
            sys_vendor: self.attr(dmi, "sys_vendor"),
            product_name: self.attr(dmi, "product_name"),
            product_version: self.attr(dmi, "product_version"),
            product_family: self.attr(dmi, "product_family"),
            chassis_type: self.attr(dmi, "chassis_type"),
            board_vendor: self.attr(dmi, "board_vendor"),
            board_name: self.attr(dmi, "board_name"),
            bios_vendor: self.attr(dmi, "bios_vendor"),
            bios_version: self.attr(dmi, "bios_version"),
            bios_date: self.attr(dmi, "bios_date"),
            product_serial: self.sensitive(dmi, "product_serial"),
            product_uuid: self.sensitive(dmi, "product_uuid"),
            board_serial: self.sensitive(dmi, "board_serial"),
            chassis_serial: self.sensitive(dmi, "chassis_serial"),
            boot_mode,
            secure_boot,
        }
    }

    /// The `SecureBoot` EFI variable is four attribute bytes and one data
    /// byte: 1 for enabled, 0 for disabled.
    fn secure_boot(&self) -> SecureBoot {
        let Ok(raw) = self.probe.read(SECURE_BOOT_VARIABLE, 16) else {
            return SecureBoot::Unknown;
        };
        match (raw.len(), raw.get(4)) {
            (5, Some(1)) => SecureBoot::Enabled,
            (5, Some(0)) => SecureBoot::Disabled,
            _ => SecureBoot::Unknown,
        }
    }

    fn pci(&mut self) -> Vec<PciDevice> {
        let mut found: Found<PciDevice> = Vec::new();
        for name in self.entries("/sys/bus/pci/devices") {
            if !is_pci_address(&name) {
                continue;
            }
            let dir = self.canonical(&format!("/sys/bus/pci/devices/{name}"));
            let vendor_id = self.hex(&dir, "vendor", 4);
            let device_id = self.hex(&dir, "device", 4);
            let class = self.hex(&dir, "class", 6);
            let modalias = self.modalias(&dir);
            let fallback = match (&vendor_id, &device_id) {
                (Some(vendor), Some(device)) => format!("pci-{name}-{vendor}:{device}"),
                _ => format!("pci-{name}"),
            };
            let is_display = class.as_deref().is_some_and(|code| code.starts_with("03"));
            let class_name = self
                .udev_text(&dir, "ID_PCI_CLASS_FROM_DATABASE")
                .or_else(|| class.as_deref().and_then(pci_class_name).map(str::to_owned));
            let device = PciDevice {
                id: String::new(),
                id_basis: IdBasis::Topology,
                sysfs_path: display_path(&dir),
                address: name,
                vendor_id,
                device_id,
                class,
                class_name,
                subsystem_vendor_id: self.hex(&dir, "subsystem_vendor", 4),
                subsystem_device_id: self.hex(&dir, "subsystem_device", 4),
                revision: self.hex(&dir, "revision", 2),
                vendor_name: self.udev_text(&dir, "ID_VENDOR_FROM_DATABASE"),
                device_name: self.udev_text(&dir, "ID_MODEL_FROM_DATABASE"),
                driver: self.driver(&dir),
                kernel_modules: self.modules(modalias.as_deref()),
                modalias,
                gpu: is_display.then(|| self.gpu(&dir)),
            };
            let candidate = Candidate {
                hardware: None,
                fallback,
                fallback_basis: IdBasis::Topology,
            };
            found.push((device, candidate, dir));
        }
        self.finish(found, true)
    }

    fn gpu(&self, dir: &str) -> Gpu {
        let drm_dir = format!("{dir}/drm");
        let mut drm_cards = Vec::new();
        for card in self.entries(&drm_dir) {
            if numbered(&card, "card").is_none() {
                continue;
            }
            let card_dir = format!("{drm_dir}/{card}");
            let prefix = format!("{card}-");
            let mut connectors = Vec::new();
            for entry in self.entries(&card_dir) {
                let Some(connector) = entry.strip_prefix(&prefix) else {
                    continue;
                };
                let connector_dir = format!("{card_dir}/{entry}");
                let status = match self.attr(&connector_dir, "status").as_deref() {
                    Some("connected") => ConnectorStatus::Connected,
                    Some("disconnected") => ConnectorStatus::Disconnected,
                    _ => ConnectorStatus::Unknown,
                };
                let enabled = match self.attr(&connector_dir, "enabled").as_deref() {
                    Some("enabled") => Some(true),
                    Some("disabled") => Some(false),
                    _ => None,
                };
                connectors.push(DrmConnector {
                    name: connector.to_owned(),
                    status,
                    enabled,
                });
            }
            drm_cards.push(DrmCard {
                name: card,
                connectors,
            });
        }
        Gpu {
            boot_vga: self.flag(dir, "boot_vga"),
            drm_cards,
        }
    }

    fn usb(&mut self) -> Vec<UsbDevice> {
        let mut found: Found<UsbDevice> = Vec::new();
        for name in self.entries("/sys/bus/usb/devices") {
            // Entries with a colon are interfaces; they are read below, as
            // children of their device.
            let is_root_hub = numbered(&name, "usb").is_some();
            let is_device = name.bytes().next().is_some_and(|b| b.is_ascii_digit());
            if name.contains(':') || !(is_root_hub || is_device) {
                continue;
            }
            let dir = self.canonical(&format!("/sys/bus/usb/devices/{name}"));
            if !self.is_dir(&dir) {
                continue;
            }
            let vendor_id = self.hex(&dir, "idVendor", 4);
            let product_id = self.hex(&dir, "idProduct", 4);
            let class = self.hex(&dir, "bDeviceClass", 2);
            // A root hub's "serial" is the PCI address of its controller, not
            // an identifier of its own, so it is not reported as one.
            let serial = if is_root_hub {
                None
            } else {
                self.attr(&dir, "serial")
            };

            let mut interfaces = Vec::new();
            for child in self.entries(&dir) {
                if !child.contains(':') {
                    continue;
                }
                let child_dir = format!("{dir}/{child}");
                let Some(interface_class) = self.hex(&child_dir, "bInterfaceClass", 2) else {
                    continue;
                };
                let modalias = self.modalias(&child_dir);
                interfaces.push(UsbInterface {
                    name: child,
                    class_name: usb_class_name(&interface_class).map(str::to_owned),
                    class: Some(interface_class),
                    subclass: self.hex(&child_dir, "bInterfaceSubClass", 2),
                    protocol: self.hex(&child_dir, "bInterfaceProtocol", 2),
                    driver: self.driver(&child_dir),
                    kernel_modules: self.modules(modalias.as_deref()),
                    modalias,
                });
            }

            let hardware = match (&vendor_id, &product_id, &serial) {
                (Some(vendor), Some(product), Some(serial)) => self
                    .ids
                    .digest("usb", &format!("{vendor}:{product}:{serial}"))
                    .map(|digest| format!("usb-{vendor}:{product}-s{digest}")),
                _ => None,
            };
            let fallback = match (&vendor_id, &product_id) {
                (Some(vendor), Some(product)) => format!("usb-{name}-{vendor}:{product}"),
                _ => format!("usb-{name}"),
            };
            let removable = match self.attr(&dir, "removable").as_deref() {
                Some("removable") => Removable::Removable,
                Some("fixed") => Removable::Fixed,
                _ => Removable::Unknown,
            };
            let speed_mbps = self.attr(&dir, "speed").filter(|speed| {
                speed.len() <= 8 && speed.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            });
            let device = UsbDevice {
                id: String::new(),
                id_basis: IdBasis::Topology,
                sysfs_path: display_path(&dir),
                bus_port: name,
                vendor_id,
                product_id,
                class_name: class.as_deref().and_then(usb_class_name).map(str::to_owned),
                class,
                manufacturer: self.attr(&dir, "manufacturer"),
                product: self.attr(&dir, "product"),
                serial: Sensitive::from_option(serial),
                speed_mbps,
                usb_version: self.attr(&dir, "version"),
                removable,
                is_root_hub,
                interfaces,
            };
            let candidate = Candidate {
                hardware,
                fallback,
                fallback_basis: IdBasis::Topology,
            };
            found.push((device, candidate, dir));
        }
        self.finish(found, true)
    }

    fn network(&mut self) -> Vec<NetworkInterface> {
        let mut found: Found<NetworkInterface> = Vec::new();
        for name in self.entries("/sys/class/net") {
            let dir = self.canonical(&format!("/sys/class/net/{name}"));
            // `/sys/class/net` also holds plain files such as `bonding_masters`.
            if !self.is_dir(&dir) {
                continue;
            }
            let is_virtual = dir.starts_with("/sys/devices/virtual/");
            let link_type = self
                .attr(&dir, "type")
                .and_then(|value| value.parse::<u32>().ok());
            let device_type = self.uevent(&dir, "DEVTYPE");
            let device_type = device_type.as_deref();
            let wireless = device_type == Some("wlan")
                || self.is_dir(&format!("{dir}/wireless"))
                || self.probe.read_link(&format!("{dir}/phy80211")).is_ok();
            let kind = if wireless {
                NetKind::Wireless
            } else if link_type == Some(772) {
                NetKind::Loopback
            } else if device_type == Some("bridge") {
                NetKind::Bridge
            } else if device_type == Some("wwan") {
                NetKind::Wwan
            } else if is_virtual {
                NetKind::Virtual
            } else if link_type == Some(1) {
                NetKind::Ethernet
            } else {
                NetKind::Other
            };
            let operstate = match self.attr(&dir, "operstate").as_deref() {
                Some("up") => OperState::Up,
                Some("down") => OperState::Down,
                Some("dormant") => OperState::Dormant,
                Some("lowerlayerdown") => OperState::LowerLayerDown,
                Some("notpresent") => OperState::NotPresent,
                Some("testing") => OperState::Testing,
                _ => OperState::Unknown,
            };
            let mac = self
                .attr(&dir, "address")
                .map(|address| address.to_ascii_lowercase())
                .filter(|address| is_mac_address(address));
            // 0 means the address is the permanent one burnt into the device;
            // a random or assigned address would not identify the hardware.
            let permanent = self.attr(&dir, "addr_assign_type").as_deref() == Some("0");
            let (mut candidate, parent) = self.child_candidate("net", &dir, &name);
            if !is_virtual
                && permanent
                && let Some(mac) = &mac
            {
                candidate.hardware = self
                    .ids
                    .digest("net", mac)
                    .map(|digest| format!("net-m{digest}"));
            }
            let device = NetworkInterface {
                id: String::new(),
                id_basis: IdBasis::Topology,
                sysfs_path: display_path(&dir),
                name,
                kind,
                wireless,
                is_virtual,
                link_type,
                driver: self.nearest_driver(&dir),
                operstate,
                mac_address: Sensitive::from_option(mac),
                parent,
            };
            found.push((device, candidate, dir));
        }
        self.finish(found, false)
    }

    fn audio(&mut self) -> Vec<AudioCard> {
        let listed = self
            .probe
            .read("/proc/asound/cards", MAX_ASOUND_BYTES)
            .map(|raw| parse_asound_cards(&raw))
            .unwrap_or_default();
        let mut indices: BTreeSet<u32> = listed.keys().copied().collect();
        for name in self.entries("/sys/class/sound") {
            if let Some(index) = numbered(&name, "card") {
                indices.insert(index);
            }
        }
        let mut found: Found<AudioCard> = Vec::new();
        for index in indices.into_iter().take(MAX_SOUND_CARDS) {
            let class_path = format!("/sys/class/sound/card{index}");
            let dir = if self.is_dir(&class_path) {
                self.canonical(&class_path)
            } else {
                format!("/proc/asound/card{index}")
            };
            let from_proc = listed.get(&index).cloned().unwrap_or_default();
            let card_id = self.attr(&dir, "id").or(from_proc.id);
            let fallback_name = card_id.clone().unwrap_or_else(|| format!("card{index}"));
            let (candidate, parent) = self.child_candidate("audio", &dir, &fallback_name);
            let device = AudioCard {
                id: String::new(),
                id_basis: IdBasis::Topology,
                sysfs_path: display_path(&dir),
                index,
                card_id,
                name: from_proc.name,
                long_name: from_proc.long_name,
                alsa_driver: from_proc.driver,
                driver: self.nearest_driver(&dir),
                parent,
            };
            found.push((device, candidate, dir));
        }
        self.finish(found, false)
    }

    fn block(&mut self) -> Vec<BlockDevice> {
        let mut found: Found<BlockDevice> = Vec::new();
        for name in self.entries("/sys/class/block") {
            let dir = self.canonical(&format!("/sys/class/block/{name}"));
            // Whole physical disks only: partitions and virtual devices
            // (loop, device-mapper, RAID, zram) belong to the storage service.
            if !self.is_dir(&dir)
                || dir.starts_with("/sys/devices/virtual/")
                || self.attr(&dir, "partition").is_some()
            {
                continue;
            }
            let model = self.attr(&dir, "device/model");
            let serial = self
                .attr(&dir, "device/serial")
                .or_else(|| self.udev_text(&dir, "ID_SERIAL_SHORT"));
            let wwid = self
                .attr(&dir, "wwid")
                .or_else(|| self.attr(&dir, "device/wwid"))
                .or_else(|| self.udev_text(&dir, "ID_WWN"));
            let (mut candidate, parent) = self.child_candidate("block", &dir, &name);
            candidate.hardware = match (&wwid, &serial) {
                (Some(wwid), _) => self
                    .ids
                    .digest("block-wwid", wwid)
                    .map(|digest| format!("block-w{digest}")),
                (None, Some(serial)) => self
                    .ids
                    .digest(
                        "block-serial",
                        &format!("{}:{serial}", model.as_deref().unwrap_or_default()),
                    )
                    .map(|digest| format!("block-s{digest}")),
                (None, None) => None,
            };
            let transport = if name.starts_with("nvme") {
                BlockTransport::Nvme
            } else if name.starts_with("mmcblk") {
                BlockTransport::Mmc
            } else if name.starts_with("vd") {
                BlockTransport::Virtio
            } else if parent.as_deref().is_some_and(|id| id.starts_with("usb-")) {
                BlockTransport::Usb
            } else if name.starts_with("sd") || name.starts_with("sr") {
                BlockTransport::Scsi
            } else {
                BlockTransport::Other
            };
            let size_bytes = self
                .attr(&dir, "size")
                .and_then(|sectors| sectors.parse::<u64>().ok())
                .and_then(|sectors| sectors.checked_mul(512));
            let device = BlockDevice {
                id: String::new(),
                id_basis: IdBasis::Topology,
                sysfs_path: display_path(&dir),
                name,
                model,
                vendor: self.attr(&dir, "device/vendor"),
                serial: Sensitive::from_option(serial),
                wwid: Sensitive::from_option(wwid),
                transport,
                removable: self.flag(&dir, "removable"),
                rotational: self.flag(&dir, "queue/rotational"),
                size_bytes,
                driver: self.nearest_driver(&dir),
                parent,
            };
            found.push((device, candidate, dir));
        }
        self.finish(found, false)
    }

    fn input_kinds(&self, dir: &str) -> Vec<InputKind> {
        const KINDS: [(&str, InputKind); 10] = [
            ("ID_INPUT_KEYBOARD", InputKind::Keyboard),
            ("ID_INPUT_KEY", InputKind::Key),
            ("ID_INPUT_MOUSE", InputKind::Mouse),
            ("ID_INPUT_TOUCHPAD", InputKind::Touchpad),
            ("ID_INPUT_TOUCHSCREEN", InputKind::Touchscreen),
            ("ID_INPUT_TABLET", InputKind::Tablet),
            ("ID_INPUT_JOYSTICK", InputKind::Joystick),
            ("ID_INPUT_SWITCH", InputKind::Switch),
            ("ID_INPUT_POINTINGSTICK", InputKind::PointingStick),
            ("ID_INPUT_ACCELEROMETER", InputKind::Accelerometer),
        ];
        KINDS
            .iter()
            .filter(|(key, _)| self.udev.property(dir, key) == Some("1"))
            .map(|(_, kind)| *kind)
            .collect()
    }

    fn input(&mut self) -> Vec<InputDevice> {
        let mut found: Found<InputDevice> = Vec::new();
        for name in self.entries("/sys/class/input") {
            // `eventN`, `mouseN` and `mice` are handlers of these devices.
            if numbered(&name, "input").is_none() {
                continue;
            }
            let dir = self.canonical(&format!("/sys/class/input/{name}"));
            if !self.is_dir(&dir) {
                continue;
            }
            let bus_type = self.hex(&dir, "id/bustype", 4);
            let vendor_id = self.hex(&dir, "id/vendor", 4);
            let product_id = self.hex(&dir, "id/product", 4);
            let bus = match bus_type.as_deref() {
                Some("0001") => InputBus::Pci,
                Some("0003") => InputBus::Usb,
                Some("0005") => InputBus::Bluetooth,
                Some("0006") => InputBus::Virtual,
                Some("0011") => InputBus::I8042,
                Some("0018") => InputBus::I2c,
                Some("0019") => InputBus::Host,
                Some("001c") => InputBus::Spi,
                Some("001d") => InputBus::Rmi,
                Some(_) => InputBus::Other,
                None => InputBus::Unknown,
            };
            let (candidate, parent) = if self.parent_of(&dir).is_some() {
                self.child_candidate("input", &dir, &name)
            } else {
                // No PCI or USB device above it (a power button, the built-in
                // keyboard): the bus and IDs, and an index among equals.
                let stem = format!(
                    "input-b{}-{}:{}",
                    bus_type.as_deref().unwrap_or("0000"),
                    vendor_id.as_deref().unwrap_or("0000"),
                    product_id.as_deref().unwrap_or("0000"),
                );
                let index = self.next_index(&stem);
                (
                    Candidate {
                        hardware: None,
                        fallback: format!("{stem}-{index}"),
                        fallback_basis: IdBasis::Topology,
                    },
                    None,
                )
            };
            let device = InputDevice {
                id: String::new(),
                id_basis: IdBasis::Topology,
                sysfs_path: display_path(&dir),
                name: self.attr(&dir, "name"),
                bus,
                vendor_id,
                product_id,
                version: self.hex(&dir, "id/version", 4),
                kinds: self.input_kinds(&dir),
                phys: self.sensitive(&dir, "phys"),
                uniq: self.sensitive(&dir, "uniq"),
                parent,
            };
            found.push((device, candidate, dir));
        }
        self.finish(found, false)
    }

    fn bluetooth(&mut self) -> Vec<BluetoothAdapter> {
        let mut found: Found<BluetoothAdapter> = Vec::new();
        for name in self.entries("/sys/class/bluetooth") {
            // `hci0:256` and the like are connections, not adapters.
            if numbered(&name, "hci").is_none() {
                continue;
            }
            let dir = self.canonical(&format!("/sys/class/bluetooth/{name}"));
            if !self.is_dir(&dir) {
                continue;
            }
            let rfkill = self
                .entries(&dir)
                .into_iter()
                .find(|child| numbered(child, "rfkill").is_some())
                .map(|child| {
                    let rfkill_dir = format!("{dir}/{child}");
                    Rfkill {
                        soft_blocked: self.flag(&rfkill_dir, "soft"),
                        hard_blocked: self.flag(&rfkill_dir, "hard"),
                    }
                });
            let address = self
                .attr(&dir, "address")
                .map(|address| address.to_ascii_lowercase())
                .filter(|address| is_mac_address(address));
            let (mut candidate, parent) = self.child_candidate("bluetooth", &dir, &name);
            if let Some(address) = &address {
                candidate.hardware = self
                    .ids
                    .digest("bluetooth", address)
                    .map(|digest| format!("bluetooth-a{digest}"));
            }
            let device = BluetoothAdapter {
                id: String::new(),
                id_basis: IdBasis::Topology,
                sysfs_path: display_path(&dir),
                name,
                driver: self.nearest_driver(&dir),
                address: Sensitive::from_option(address),
                rfkill,
                parent,
            };
            found.push((device, candidate, dir));
        }
        self.finish(found, false)
    }

    fn cameras(&mut self) -> Vec<Camera> {
        let mut found: Found<Camera> = Vec::new();
        for name in self.entries("/sys/class/video4linux") {
            if numbered(&name, "video").is_none() {
                continue;
            }
            let dir = self.canonical(&format!("/sys/class/video4linux/{name}"));
            if !self.is_dir(&dir) {
                continue;
            }
            let capture = self
                .udev
                .property(&dir, "ID_V4L_CAPABILITIES")
                .map(|capabilities| capabilities.contains(":capture:"));
            let (candidate, parent) = self.child_candidate("camera", &dir, &name);
            let device = Camera {
                id: String::new(),
                id_basis: IdBasis::Topology,
                sysfs_path: display_path(&dir),
                node: name,
                name: self.attr(&dir, "name"),
                index: self
                    .attr(&dir, "index")
                    .and_then(|value| value.parse::<u32>().ok()),
                capture,
                driver: self.nearest_driver(&dir),
                parent,
            };
            found.push((device, candidate, dir));
        }
        self.finish(found, false)
    }

    fn thunderbolt(&mut self) -> Vec<ThunderboltDevice> {
        let bus = "/sys/bus/thunderbolt/devices";
        let mut found: Found<ThunderboltDevice> = Vec::new();
        for name in self.entries(bus) {
            // `domainN` is the bus itself; names with a colon are retimers
            // and services of a device.
            let Some((domain, route)) = name.split_once('-') else {
                continue;
            };
            if name.contains(':') || numbered(domain, "").is_none() || route.is_empty() {
                continue;
            }
            let dir = self.canonical(&format!("{bus}/{name}"));
            if !self.is_dir(&dir) {
                continue;
            }
            let unique_id = self.attr(&dir, "unique_id");
            let authorised = match self.attr(&dir, "authorized").as_deref() {
                Some("0") => ThunderboltAuthorisation::NotAuthorised,
                Some("1") => ThunderboltAuthorisation::Authorised,
                Some("2") => ThunderboltAuthorisation::AuthorisedSecure,
                _ => ThunderboltAuthorisation::Unknown,
            };
            let security_level = self
                .attr(&format!("{bus}/domain{domain}"), "security")
                .and_then(|level| kernel_name(&level));
            let candidate = Candidate {
                hardware: unique_id.as_deref().and_then(|unique| {
                    self.ids
                        .digest("thunderbolt", unique)
                        .map(|digest| format!("thunderbolt-u{digest}"))
                }),
                fallback: format!("thunderbolt-{}", id_token(&name)),
                fallback_basis: IdBasis::Topology,
            };
            let device = ThunderboltDevice {
                id: String::new(),
                id_basis: IdBasis::Topology,
                sysfs_path: display_path(&dir),
                is_host: route == "0",
                name,
                vendor_name: self.attr(&dir, "vendor_name"),
                device_name: self.attr(&dir, "device_name"),
                vendor_id: self.hex(&dir, "vendor", 4),
                device_id: self.hex(&dir, "device", 4),
                unique_id: Sensitive::from_option(unique_id),
                authorised,
                security_level,
            };
            found.push((device, candidate, dir));
        }
        self.finish(found, false)
    }

    fn firmware(&mut self) -> Firmware {
        let effect = "Firmware devices and pending firmware updates are not listed.";
        let raw = match self.probe.run(ProbeCommand::FwupdGetDevices) {
            Ok(raw) => raw,
            Err(error) => {
                self.note_error("fwupd", &error, effect);
                return Firmware {
                    fwupd: FwupdStatus::Unavailable,
                    devices: Vec::new(),
                };
            }
        };
        match fwupd::parse(&raw) {
            Ok(devices) => Firmware {
                fwupd: FwupdStatus::Available,
                devices,
            },
            Err(_) => {
                self.notes
                    .push(note("fwupd", SourceStatus::Invalid, effect));
                Firmware {
                    fwupd: FwupdStatus::Invalid,
                    devices: Vec::new(),
                }
            }
        }
    }

    fn kernel(&mut self) -> KernelEvidence {
        let effect = "Driver and firmware failures recorded by the kernel are not known; findings rest on sysfs alone.";
        let unavailable = KernelEvidence {
            available: false,
            firmware_failures: Vec::new(),
            probe_failures: Vec::new(),
        };
        match self.probe.run(ProbeCommand::KernelLog) {
            // A user who may not read the journal gets an empty answer, not an
            // error. The kernel log of a running system is never empty.
            Ok(raw) if raw.iter().all(u8::is_ascii_whitespace) => {
                self.notes
                    .push(note("kernel-log", SourceStatus::Denied, effect));
                unavailable
            }
            Ok(raw) => {
                let (firmware_failures, probe_failures) = kernel_log::parse(&raw);
                KernelEvidence {
                    available: true,
                    firmware_failures,
                    probe_failures,
                }
            }
            Err(error) => {
                self.note_error("kernel-log", &error, effect);
                unavailable
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::FakeProbe;
    use crate::text::ELLIPSIS;

    const MACHINE_ID: &str = "0123456789abcdef0123456789abcdef\n";

    /// A USB device at a port, with whatever descriptor strings are given.
    fn usb_device(probe: FakeProbe, port: &str, strings: &[(&str, &[u8])]) -> FakeProbe {
        let dir = format!("/sys/devices/pci0000:00/0000:00:14.0/usb1/{port}");
        let mut probe = probe
            .link(
                &format!("/sys/bus/usb/devices/{port}"),
                &format!("../../../devices/pci0000:00/0000:00:14.0/usb1/{port}"),
            )
            .file(&format!("{dir}/idVendor"), "0781\n")
            .file(&format!("{dir}/idProduct"), "5581\n")
            .file(&format!("{dir}/bDeviceClass"), "00\n");
        for (name, value) in strings {
            probe = probe.file(&format!("{dir}/{name}"), value);
        }
        probe
    }

    #[test]
    fn an_empty_system_gives_an_empty_inventory() {
        let inventory = collect(&FakeProbe::new());
        assert_eq!(inventory.schema_version, SCHEMA_VERSION);
        assert!(inventory.devices().is_empty());
        assert_eq!(inventory.platform.boot_mode, BootMode::Unknown);
        assert_eq!(inventory.platform.secure_boot, SecureBoot::Unknown);
        assert_eq!(inventory.firmware.fwupd, FwupdStatus::Unavailable);
        assert!(!inventory.kernel.available);
        let sources: Vec<&str> = inventory
            .source_notes
            .iter()
            .map(|note| note.source.as_str())
            .collect();
        assert_eq!(
            sources,
            [
                "machine-id",
                "udev",
                "kernel-module-aliases",
                "fwupd",
                "kernel-log"
            ]
        );
    }

    #[test]
    fn hostile_usb_descriptor_strings_are_cleaned_and_bounded() {
        let long = b"A".repeat(4000);
        let probe = usb_device(
            FakeProbe::new().file("/etc/machine-id", MACHINE_ID),
            "1-1",
            &[
                (
                    "manufacturer",
                    b"Evil\x1b]0;owned\x07 Corp\r\n\x00".as_slice(),
                ),
                ("product", long.as_slice()),
                ("serial", b"\xff\xfe\xfd\x80SER\xc3\x28IAL".as_slice()),
                ("speed", b"480; rm -rf /\n".as_slice()),
                ("version", b" 2.00\n".as_slice()),
                ("removable", b"\x00\x01\x02".as_slice()),
            ],
        );
        let inventory = collect(&probe);
        assert_eq!(inventory.usb.len(), 1);
        let device = &inventory.usb[0];

        assert_eq!(device.manufacturer.as_deref(), Some("Evil ]0;owned Corp"));
        let product = device.product.as_deref().unwrap();
        assert_eq!(product.chars().count(), MAX_TEXT_CHARS);
        assert!(product.ends_with(ELLIPSIS));
        let serial = device.serial.value().unwrap();
        assert_eq!(serial, "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}SER\u{FFFD}(IAL");
        assert_eq!(device.speed_mbps, None);
        assert_eq!(device.usb_version.as_deref(), Some("2.00"));
        assert_eq!(device.removable, Removable::Unknown);

        // Nothing in the whole document carries a control character.
        let json = serde_json::to_string(&inventory).unwrap();
        assert!(!json.chars().any(|ch| ch.is_control()));
        assert!(!json.contains("\\u00"));
        // The identifier is made of safe characters only.
        assert!(
            device
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'.' | b'_' | b'-' | b'~'))
        );
    }

    #[test]
    fn a_string_longer_than_the_read_limit_is_not_read_whole() {
        let huge = b"x".repeat(10 * 1024 * 1024);
        let probe = usb_device(FakeProbe::new(), "1-1", &[("product", huge.as_slice())]);
        let inventory = collect(&probe);
        let product = inventory.usb[0].product.as_deref().unwrap();
        assert_eq!(product.chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn missing_and_malformed_attributes_leave_nulls() {
        let dir = "/sys/devices/pci0000:00/0000:00:1f.0";
        let probe = FakeProbe::new()
            .link(
                "/sys/bus/pci/devices/0000:00:1f.0",
                "../../../devices/pci0000:00/0000:00:1f.0",
            )
            .file(&format!("{dir}/vendor"), "not hex\n")
            .file(&format!("{dir}/device"), "0x12345678\n")
            .file(&format!("{dir}/class"), "")
            .file(&format!("{dir}/modalias"), "pci:\x01bad\n")
            .link(
                &format!("{dir}/driver"),
                "../../../bus/pci/drivers/bad name",
            )
            // Entries that are not PCI addresses are ignored.
            .dir("/sys/bus/pci/devices/not-an-address")
            .dir("/sys/bus/pci/devices/0000:00:zz.0");
        let inventory = collect(&probe);
        assert_eq!(inventory.pci.len(), 1);
        let device = &inventory.pci[0];
        assert_eq!(device.address, "0000:00:1f.0");
        assert_eq!(device.id, "pci-0000:00:1f.0");
        assert_eq!(device.vendor_id, None);
        assert_eq!(device.device_id, None);
        assert_eq!(device.class, None);
        assert_eq!(device.class_name, None);
        assert_eq!(device.driver, None);
        assert_eq!(device.modalias.as_deref(), Some("pci: bad"));
        assert_eq!(device.kernel_modules, None);
        assert_eq!(device.gpu, None);
    }

    #[test]
    fn a_usb_identifier_follows_the_device_to_another_port() {
        let base = || FakeProbe::new().file("/etc/machine-id", MACHINE_ID);
        let at_port_one = collect(&usb_device(
            base(),
            "1-1",
            &[("serial", b"4C53000123".as_slice())],
        ));
        let at_port_two = collect(&usb_device(
            base(),
            "1-4.2",
            &[("serial", b"4C53000123".as_slice())],
        ));
        assert_eq!(at_port_one.usb[0].id_basis, IdBasis::HardwareIdentifier);
        assert_eq!(at_port_one.usb[0].id, at_port_two.usb[0].id);
        assert_ne!(at_port_one.usb[0].bus_port, at_port_two.usb[0].bus_port);
        // The serial number is not in the identifier.
        assert!(!at_port_one.usb[0].id.contains("4C53000123"));

        // Without a serial number the identifier is the port.
        let no_serial_one = collect(&usb_device(base(), "1-1", &[]));
        let no_serial_two = collect(&usb_device(base(), "1-4.2", &[]));
        assert_eq!(no_serial_one.usb[0].id_basis, IdBasis::Topology);
        assert_eq!(no_serial_one.usb[0].id, "usb-1-1-0781:5581");
        assert_ne!(no_serial_one.usb[0].id, no_serial_two.usb[0].id);
    }

    #[test]
    fn two_devices_with_one_serial_number_stay_distinct() {
        let probe = FakeProbe::new().file("/etc/machine-id", MACHINE_ID);
        let probe = usb_device(probe, "1-1", &[("serial", b"0123456789ABCDEF".as_slice())]);
        let probe = usb_device(probe, "1-2", &[("serial", b"0123456789ABCDEF".as_slice())]);
        let inventory = collect(&probe);
        assert_eq!(inventory.usb.len(), 2);
        assert_eq!(inventory.usb[0].id, "usb-1-1-0781:5581");
        assert_eq!(inventory.usb[1].id, "usb-1-2-0781:5581");
        assert_eq!(inventory.usb[0].id_basis, IdBasis::Topology);
    }

    #[test]
    fn without_a_machine_identifier_no_serial_number_is_hashed() {
        let probe = usb_device(
            FakeProbe::new(),
            "1-1",
            &[("serial", b"4C53000123".as_slice())],
        );
        let inventory = collect(&probe);
        assert_eq!(inventory.usb[0].id, "usb-1-1-0781:5581");
        assert_eq!(inventory.usb[0].id_basis, IdBasis::Topology);
        assert_eq!(inventory.usb[0].serial.value(), Some("4C53000123"));
        assert_eq!(inventory.source_notes[0].source, "machine-id");
        assert_eq!(inventory.source_notes[0].status, SourceStatus::Unavailable);

        let probe = usb_device(
            FakeProbe::new().file("/etc/machine-id", "uninitialized\n"),
            "1-1",
            &[("serial", b"4C53000123".as_slice())],
        );
        let inventory = collect(&probe);
        assert_eq!(inventory.usb[0].id_basis, IdBasis::Topology);
        assert_eq!(inventory.source_notes[0].status, SourceStatus::Invalid);
    }

    #[test]
    fn sound_cards_come_from_proc_when_sysfs_has_none() {
        let probe = FakeProbe::new().file(
            "/proc/asound/cards",
            " 0 [PCH            ]: HDA-Intel - HDA Intel PCH\n                      HDA Intel PCH at 0xf7f10000 irq 30\n 1 [Device         ]: USB-Audio - USB Audio Device\n                      Generic USB Audio Device at usb-0000:00:14.0-2, full speed\n",
        );
        let inventory = collect(&probe);
        assert_eq!(inventory.audio.len(), 2);
        let card = &inventory.audio[0];
        assert_eq!(card.index, 0);
        assert_eq!(card.card_id.as_deref(), Some("PCH"));
        assert_eq!(card.alsa_driver.as_deref(), Some("HDA-Intel"));
        assert_eq!(card.name.as_deref(), Some("HDA Intel PCH"));
        assert_eq!(
            card.long_name.as_deref(),
            Some("HDA Intel PCH at 0xf7f10000 irq 30")
        );
        assert_eq!(card.driver, None);
        assert_eq!(card.id, "audio-name-PCH");
        assert_eq!(card.id_basis, IdBasis::KernelName);
        assert_eq!(card.sysfs_path, "/proc/asound/card0");
        assert_eq!(inventory.audio[1].name.as_deref(), Some("USB Audio Device"));
    }

    #[test]
    fn asound_cards_parser_tolerates_garbage() {
        assert!(parse_asound_cards(b"--- no soundcards ---\n").is_empty());
        assert!(parse_asound_cards(b"\xff\xfe[[[]:]:\n x [a]: b - c\n").is_empty());
        let cards = parse_asound_cards(b"7 [\x1b[0m]: only a description\n");
        assert_eq!(cards.len(), 1);
        let card = &cards[&7];
        assert_eq!(card.id.as_deref(), Some("[0m"));
        assert_eq!(card.driver, None);
        assert_eq!(card.name.as_deref(), Some("only a description"));
    }

    #[test]
    fn secure_boot_is_read_from_the_efi_variable() {
        let with = |value: &[u8]| {
            collect(&FakeProbe::new().file(SECURE_BOOT_VARIABLE, value))
                .platform
                .secure_boot
        };
        assert_eq!(with(&[6, 0, 0, 0, 1]), SecureBoot::Enabled);
        assert_eq!(with(&[6, 0, 0, 0, 0]), SecureBoot::Disabled);
        assert_eq!(with(&[6, 0, 0, 0, 7]), SecureBoot::Unknown);
        assert_eq!(with(&[1]), SecureBoot::Unknown);
        assert_eq!(with(&[6, 0, 0, 0, 1, 1]), SecureBoot::Unknown);

        let uefi_without_variable = collect(&FakeProbe::new().dir("/sys/firmware/efi"));
        assert_eq!(uefi_without_variable.platform.boot_mode, BootMode::Uefi);
        assert_eq!(
            uefi_without_variable.platform.secure_boot,
            SecureBoot::Unknown
        );

        let bios = collect(&FakeProbe::new().dir("/sys/firmware/acpi"));
        assert_eq!(bios.platform.boot_mode, BootMode::Bios);
        assert_eq!(bios.platform.secure_boot, SecureBoot::NotApplicable);
    }

    #[test]
    fn serial_numbers_the_user_may_not_read_are_absent() {
        let dmi = "/sys/devices/virtual/dmi/id";
        let probe = FakeProbe::new()
            .link("/sys/class/dmi/id", "../../devices/virtual/dmi/id")
            .file(&format!("{dmi}/sys_vendor"), "Example Computer\n")
            .file(&format!("{dmi}/product_serial"), "PF2ABCDE\n")
            .deny(&format!("{dmi}/product_serial"));
        let inventory = collect(&probe);
        assert_eq!(
            inventory.platform.sys_vendor.as_deref(),
            Some("Example Computer")
        );
        assert_eq!(inventory.platform.product_serial, Sensitive::Absent);
    }

    #[test]
    fn an_unreadable_journal_is_not_taken_for_a_clean_one() {
        let probe = FakeProbe::new().command(ProbeCommand::KernelLog, "\n");
        let inventory = collect(&probe);
        assert!(!inventory.kernel.available);
        let last = inventory.source_notes.last().unwrap();
        assert_eq!(last.source, "kernel-log");
        assert_eq!(last.status, SourceStatus::Denied);
    }

    #[test]
    fn invalid_fwupd_output_is_reported_as_invalid() {
        let probe = FakeProbe::new().command(ProbeCommand::FwupdGetDevices, "<html>");
        let inventory = collect(&probe);
        assert_eq!(inventory.firmware.fwupd, FwupdStatus::Invalid);
        assert!(inventory.firmware.devices.is_empty());
    }

    #[test]
    fn link_targets_are_resolved_without_escaping() {
        assert_eq!(
            resolve_link(
                "/sys/bus/pci/devices/0000:00:02.0",
                "../../../devices/pci0000:00/0000:00:02.0"
            )
            .as_deref(),
            Some("/sys/devices/pci0000:00/0000:00:02.0")
        );
        assert_eq!(
            resolve_link("/sys/class/net/eth0", "/sys/devices/x").as_deref(),
            Some("/sys/devices/x")
        );
        // More `..` than there are directories stops at the root.
        assert_eq!(
            resolve_link("/sys/a", "../../../../etc").as_deref(),
            Some("/etc")
        );
        assert_eq!(resolve_link("/sys/a", ".."), None);
        assert_eq!(resolve_link("/sys/a", &"x/".repeat(MAX_PATH_BYTES)), None);
    }

    #[test]
    fn pci_addresses_are_recognised() {
        assert!(is_pci_address("0000:00:02.0"));
        assert!(is_pci_address("10000:e1:00.7"));
        assert!(!is_pci_address("0000:00:02"));
        assert!(!is_pci_address("0000:00:02.10"));
        assert!(!is_pci_address("000:00:02.0"));
        assert!(!is_pci_address("0000:00:02.0:1"));
        assert!(!is_pci_address("usb1"));
    }

    #[test]
    fn hostile_directory_names_are_skipped() {
        let probe = FakeProbe::new()
            .file("/sys/class/net/evil\u{202e}name/operstate", "up\n")
            .file("/sys/class/net/ok0/operstate", "up\n")
            .file(
                &format!("/sys/class/net/{}/operstate", "n".repeat(300)),
                "up\n",
            );
        let inventory = collect(&probe);
        let names: Vec<&str> = inventory
            .network
            .iter()
            .map(|interface| interface.name.as_str())
            .collect();
        assert_eq!(names, ["ok0"]);
        assert_eq!(inventory.network[0].id, "net-name-ok0");
        assert_eq!(inventory.network[0].id_basis, IdBasis::KernelName);
    }
}
