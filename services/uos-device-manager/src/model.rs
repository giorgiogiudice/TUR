// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The inventory model: what `uos-device inventory --json` prints.
//!
//! The JSON form of these types is described by
//! `schemas/device-inventory.schema.json`. Every field is always present; a
//! value that could not be read is `null`. A test validates the output against
//! the schema, which forbids unknown fields and requires every known one, so
//! the two cannot drift apart unnoticed.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use std::fmt;

/// The version of the inventory and findings documents.
pub const SCHEMA_VERSION: u32 = 1;

/// A value that identifies a person's hardware: a serial number, a MAC
/// address, a hardware UUID.
///
/// The type marks the field as sensitive so that a caller can redact it
/// ([`crate::Inventory::redacted`]). In JSON it is an object,
/// `{"sensitive": true, "redacted": false, "value": "..."}`, so a consumer can
/// recognise a sensitive field without knowing the schema.
///
/// `Debug` does not print the value.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum Sensitive {
    /// The hardware gave no value, or it could not be read.
    #[default]
    Absent,
    /// The value.
    Present(String),
    /// A value existed and has been removed.
    Redacted,
}

impl Sensitive {
    /// Wrap an optional value.
    pub fn from_option(value: Option<String>) -> Self {
        value.map_or(Self::Absent, Self::Present)
    }

    /// The value, if it is present and not redacted.
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Present(value) => Some(value),
            Self::Absent | Self::Redacted => None,
        }
    }

    /// Remove the value, remembering that there was one.
    pub fn redact(&mut self) {
        if matches!(self, Self::Present(_)) {
            *self = Self::Redacted;
        }
    }
}

impl fmt::Debug for Sensitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Absent => "Sensitive(absent)",
            Self::Present(_) => "Sensitive(present)",
            Self::Redacted => "Sensitive(redacted)",
        })
    }
}

impl Serialize for Sensitive {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Sensitive", 3)?;
        state.serialize_field("sensitive", &true)?;
        state.serialize_field("redacted", &matches!(self, Self::Redacted))?;
        state.serialize_field("value", &self.value())?;
        state.end()
    }
}

/// What a device identifier is derived from, and so how well it survives
/// re-enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdBasis {
    /// An identifier the hardware carries (serial number, permanent MAC
    /// address, WWN, Thunderbolt UUID), hashed. The identifier follows the
    /// device to another port and survives a restart.
    HardwareIdentifier,
    /// The device's position: a PCI address or a USB port path. The identifier
    /// survives a restart while the hardware layout is unchanged, and is lost
    /// if the device moves to another port or slot.
    Topology,
    /// Only a kernel-assigned name (a virtual network interface, for example).
    /// Not stable.
    KernelName,
}

/// The kind of device, used in summaries and findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// A PCI or PCI Express function.
    Pci,
    /// A USB device.
    Usb,
    /// A network interface.
    Network,
    /// A sound card.
    Audio,
    /// A storage controller.
    StorageController,
    /// A whole block device.
    Block,
    /// An input device.
    Input,
    /// A Bluetooth adapter.
    Bluetooth,
    /// A V4L2 video device.
    Camera,
    /// A Thunderbolt or USB4 device.
    Thunderbolt,
}

/// A kernel module whose alias table matches a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleMatch {
    /// The module name, with `-` written as `_` as in `/sys/module`.
    pub name: String,
    /// Whether the module is loaded or built in (`/sys/module/<name>` exists).
    pub loaded: bool,
}

/// A PCI or PCI Express function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PciDevice {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The device's directory in sysfs.
    pub sysfs_path: String,
    /// Domain, bus, device and function, for example `0000:00:02.0`.
    pub address: String,
    /// Vendor ID, four hexadecimal digits.
    pub vendor_id: Option<String>,
    /// Device ID, four hexadecimal digits.
    pub device_id: Option<String>,
    /// Class, subclass and programming interface, six hexadecimal digits.
    pub class: Option<String>,
    /// The name of the base class.
    pub class_name: Option<String>,
    /// Subsystem vendor ID, four hexadecimal digits.
    pub subsystem_vendor_id: Option<String>,
    /// Subsystem device ID, four hexadecimal digits.
    pub subsystem_device_id: Option<String>,
    /// Revision, two hexadecimal digits.
    pub revision: Option<String>,
    /// The vendor's name from the udev hardware database.
    pub vendor_name: Option<String>,
    /// The device's name from the udev hardware database.
    pub device_name: Option<String>,
    /// The bound kernel driver, or `null` if none is bound.
    pub driver: Option<String>,
    /// The kernel's modalias string for the device.
    pub modalias: Option<String>,
    /// Kernel modules whose aliases match the modalias; `null` if the
    /// kernel's alias table could not be read.
    pub kernel_modules: Option<Vec<ModuleMatch>>,
    /// Display details, for display-class devices only.
    pub gpu: Option<Gpu>,
}

/// Display details of a GPU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gpu {
    /// Whether the firmware used this GPU for the boot display.
    pub boot_vga: Option<bool>,
    /// The DRM cards the driver registered. Empty if no driver is bound.
    pub drm_cards: Vec<DrmCard>,
}

/// A DRM card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DrmCard {
    /// The card's name, for example `card1`.
    pub name: String,
    /// The card's connectors.
    pub connectors: Vec<DrmConnector>,
}

/// A display connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DrmConnector {
    /// The connector's name without the card prefix, for example `eDP-1`.
    pub name: String,
    /// Whether a display is connected.
    pub status: ConnectorStatus,
    /// Whether the connector is driving a display.
    pub enabled: Option<bool>,
}

/// Whether a display is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorStatus {
    /// A display is connected.
    Connected,
    /// Nothing is connected.
    Disconnected,
    /// The kernel cannot tell, or the attribute could not be read.
    Unknown,
}

/// A USB device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsbDevice {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The device's directory in sysfs.
    pub sysfs_path: String,
    /// Bus number and port path, for example `1-2.3`; `usb1` for a root hub.
    pub bus_port: String,
    /// Vendor ID, four hexadecimal digits.
    pub vendor_id: Option<String>,
    /// Product ID, four hexadecimal digits.
    pub product_id: Option<String>,
    /// Device class, two hexadecimal digits. `00` means the interfaces say.
    pub class: Option<String>,
    /// The name of the device class.
    pub class_name: Option<String>,
    /// The manufacturer string supplied by the device.
    pub manufacturer: Option<String>,
    /// The product string supplied by the device.
    pub product: Option<String>,
    /// The serial number supplied by the device.
    pub serial: Sensitive,
    /// Negotiated speed in Mbit/s as the kernel prints it, for example `480`.
    pub speed_mbps: Option<String>,
    /// The USB specification version the device declares, for example `2.00`.
    pub usb_version: Option<String>,
    /// Whether the device can be unplugged by the user.
    pub removable: Removable,
    /// Whether this is the root hub of a host controller.
    pub is_root_hub: bool,
    /// The interfaces of the active configuration.
    pub interfaces: Vec<UsbInterface>,
}

/// One interface of a USB device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsbInterface {
    /// The interface's kernel name, for example `1-2.3:1.0`.
    pub name: String,
    /// Interface class, two hexadecimal digits.
    pub class: Option<String>,
    /// Interface subclass, two hexadecimal digits.
    pub subclass: Option<String>,
    /// Interface protocol, two hexadecimal digits.
    pub protocol: Option<String>,
    /// The name of the interface class.
    pub class_name: Option<String>,
    /// The kernel driver bound to this interface, or `null`.
    pub driver: Option<String>,
    /// The kernel's modalias string for the interface.
    pub modalias: Option<String>,
    /// Kernel modules whose aliases match the modalias; `null` if the
    /// kernel's alias table could not be read.
    pub kernel_modules: Option<Vec<ModuleMatch>>,
}

/// Whether a USB device can be unplugged, as the firmware describes its port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Removable {
    /// On a user-accessible port.
    Removable,
    /// Built into the machine.
    Fixed,
    /// Not stated.
    Unknown,
}

/// A network interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetworkInterface {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The interface's directory in sysfs.
    pub sysfs_path: String,
    /// The interface name.
    pub name: String,
    /// The kind of interface.
    pub kind: NetKind,
    /// Whether it is a wireless LAN interface.
    pub wireless: bool,
    /// Whether it has no hardware behind it (loopback, bridge, tunnel).
    pub is_virtual: bool,
    /// The kernel's link-layer type number (`ARPHRD_*`).
    pub link_type: Option<u32>,
    /// The kernel driver of the underlying device, or `null`.
    pub driver: Option<String>,
    /// The operational state.
    pub operstate: OperState,
    /// The MAC address.
    pub mac_address: Sensitive,
    /// The identifier of the PCI or USB device that provides the interface.
    pub parent: Option<String>,
}

/// The kind of a network interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetKind {
    /// Wired Ethernet.
    Ethernet,
    /// Wireless LAN.
    Wireless,
    /// The loopback interface.
    Loopback,
    /// A software bridge.
    Bridge,
    /// A mobile broadband interface.
    Wwan,
    /// Another interface with no hardware behind it.
    Virtual,
    /// Anything else.
    Other,
}

/// The operational state of a network interface (RFC 2863).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperState {
    /// Able to pass traffic.
    Up,
    /// Not able to pass traffic.
    Down,
    /// Waiting for an external event.
    Dormant,
    /// A lower interface is down.
    LowerLayerDown,
    /// A component is missing.
    NotPresent,
    /// In a test mode.
    Testing,
    /// Not reported.
    Unknown,
}

/// A sound card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AudioCard {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The card's directory in sysfs, or under `/proc/asound` if sysfs has none.
    pub sysfs_path: String,
    /// The ALSA card number.
    pub index: u32,
    /// The ALSA card identifier, for example `PCH`.
    pub card_id: Option<String>,
    /// The card's short name.
    pub name: Option<String>,
    /// The card's long name.
    pub long_name: Option<String>,
    /// The ALSA driver name from `/proc/asound/cards`, for example `HDA-Intel`.
    pub alsa_driver: Option<String>,
    /// The kernel driver of the underlying device, or `null`.
    pub driver: Option<String>,
    /// The identifier of the PCI or USB device that provides the card.
    pub parent: Option<String>,
}

/// A storage controller. Each is also listed as the PCI device or USB
/// interface it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StorageController {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The controller's directory in sysfs.
    pub sysfs_path: String,
    /// The kind of controller.
    pub kind: StorageControllerKind,
    /// The bound kernel driver, or `null`.
    pub driver: Option<String>,
    /// The identifier of the PCI or USB device that is the controller.
    pub parent: String,
}

/// The kind of a storage controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageControllerKind {
    /// NVM Express.
    Nvme,
    /// Serial ATA (AHCI).
    Sata,
    /// Parallel ATA or IDE.
    Ata,
    /// SCSI.
    Scsi,
    /// Serial Attached SCSI.
    Sas,
    /// A RAID controller.
    Raid,
    /// Universal Flash Storage.
    Ufs,
    /// An SD or MMC host controller.
    SdHost,
    /// A USB mass-storage interface.
    UsbMassStorage,
    /// Another mass-storage controller.
    Other,
}

/// A whole block device. Partitions and volumes belong to the storage service
/// and are not listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockDevice {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The device's directory in sysfs.
    pub sysfs_path: String,
    /// The kernel name, for example `nvme0n1`.
    pub name: String,
    /// The model string.
    pub model: Option<String>,
    /// The vendor string.
    pub vendor: Option<String>,
    /// The serial number.
    pub serial: Sensitive,
    /// The world-wide identifier.
    pub wwid: Sensitive,
    /// How the device is attached.
    pub transport: BlockTransport,
    /// Whether the medium can be removed.
    pub removable: Option<bool>,
    /// Whether the device is a rotating disk.
    pub rotational: Option<bool>,
    /// The capacity in bytes.
    pub size_bytes: Option<u64>,
    /// The kernel driver of the underlying device, or `null`.
    pub driver: Option<String>,
    /// The identifier of the PCI or USB device the disk is attached through.
    pub parent: Option<String>,
}

/// How a block device is attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockTransport {
    /// NVM Express.
    Nvme,
    /// SCSI, SATA or SAS through the SCSI layer.
    Scsi,
    /// USB mass storage.
    Usb,
    /// SD or MMC.
    Mmc,
    /// A virtio disk in a virtual machine.
    Virtio,
    /// Anything else.
    Other,
}

/// An input device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputDevice {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The device's directory in sysfs.
    pub sysfs_path: String,
    /// The device's name, which a USB or Bluetooth device supplies itself.
    pub name: Option<String>,
    /// The bus the device is on.
    pub bus: InputBus,
    /// Vendor ID, four hexadecimal digits.
    pub vendor_id: Option<String>,
    /// Product ID, four hexadecimal digits.
    pub product_id: Option<String>,
    /// Version, four hexadecimal digits.
    pub version: Option<String>,
    /// What the device is, from udev's classification.
    pub kinds: Vec<InputKind>,
    /// The physical path. For a Bluetooth device this is the adapter's
    /// address, so the field is always treated as sensitive.
    pub phys: Sensitive,
    /// The unique identifier: a serial number, or a Bluetooth address.
    pub uniq: Sensitive,
    /// The identifier of the PCI or USB device that provides the input device.
    pub parent: Option<String>,
}

/// The bus of an input device (`BUS_*` in the kernel's `input.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputBus {
    /// PCI.
    Pci,
    /// USB.
    Usb,
    /// Bluetooth.
    Bluetooth,
    /// A virtual device.
    Virtual,
    /// The PC keyboard controller.
    I8042,
    /// I2C.
    I2c,
    /// The host itself (power button, lid switch).
    Host,
    /// SPI.
    Spi,
    /// Synaptics RMI.
    Rmi,
    /// Another bus.
    Other,
    /// Not reported.
    Unknown,
}

/// What an input device is, from udev's `ID_INPUT_*` properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// A keyboard.
    Keyboard,
    /// A device with keys or buttons that is not a full keyboard.
    Key,
    /// A mouse.
    Mouse,
    /// A touchpad.
    Touchpad,
    /// A touchscreen.
    Touchscreen,
    /// A graphics tablet.
    Tablet,
    /// A joystick or game controller.
    Joystick,
    /// A switch such as a lid switch.
    Switch,
    /// A pointing stick.
    PointingStick,
    /// An accelerometer.
    Accelerometer,
}

/// A Bluetooth adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BluetoothAdapter {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The adapter's directory in sysfs.
    pub sysfs_path: String,
    /// The kernel name, for example `hci0`.
    pub name: String,
    /// The kernel driver of the underlying device, or `null`.
    pub driver: Option<String>,
    /// The adapter's Bluetooth address, where sysfs shows it.
    pub address: Sensitive,
    /// The radio kill-switch state, where the adapter has one.
    pub rfkill: Option<Rfkill>,
    /// The identifier of the PCI or USB device that is the adapter.
    pub parent: Option<String>,
}

/// The state of a radio kill switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rfkill {
    /// Blocked by software.
    pub soft_blocked: Option<bool>,
    /// Blocked by a hardware switch.
    pub hard_blocked: Option<bool>,
}

/// A V4L2 video device node. A webcam usually has two: one for capture and
/// one for metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Camera {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The node's directory in sysfs.
    pub sysfs_path: String,
    /// The kernel name, for example `video0`.
    pub node: String,
    /// The device's name, which a USB camera supplies itself.
    pub name: Option<String>,
    /// The node's index within its device; `0` is the main node.
    pub index: Option<u32>,
    /// Whether the node captures video, from udev; `null` if udev did not say.
    pub capture: Option<bool>,
    /// The kernel driver of the underlying device, or `null`.
    pub driver: Option<String>,
    /// The identifier of the PCI or USB device that is the camera.
    pub parent: Option<String>,
}

/// A Thunderbolt or USB4 device, including the host's own router.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThunderboltDevice {
    /// The stable identifier.
    pub id: String,
    /// What the identifier is derived from.
    pub id_basis: IdBasis,
    /// The device's directory in sysfs.
    pub sysfs_path: String,
    /// The kernel name, for example `0-1`.
    pub name: String,
    /// The vendor name supplied by the device.
    pub vendor_name: Option<String>,
    /// The device name supplied by the device.
    pub device_name: Option<String>,
    /// Vendor ID, four hexadecimal digits.
    pub vendor_id: Option<String>,
    /// Device ID, four hexadecimal digits.
    pub device_id: Option<String>,
    /// The device's unique identifier (a UUID).
    pub unique_id: Sensitive,
    /// Whether the device has been authorised to use PCIe tunnelling.
    pub authorised: ThunderboltAuthorisation,
    /// Whether this is the host's own router.
    pub is_host: bool,
    /// The security level of the device's domain, for example `user`.
    pub security_level: Option<String>,
}

/// The authorisation state of a Thunderbolt device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThunderboltAuthorisation {
    /// Connected but not authorised.
    NotAuthorised,
    /// Authorised by the user.
    Authorised,
    /// Authorised with a key challenge.
    AuthorisedSecure,
    /// The attribute could not be read.
    Unknown,
}

/// The machine itself: DMI identity and firmware mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Platform {
    /// The system manufacturer.
    pub sys_vendor: Option<String>,
    /// The product name.
    pub product_name: Option<String>,
    /// The product version.
    pub product_version: Option<String>,
    /// The product family.
    pub product_family: Option<String>,
    /// The chassis type number (SMBIOS).
    pub chassis_type: Option<String>,
    /// The board manufacturer.
    pub board_vendor: Option<String>,
    /// The board name.
    pub board_name: Option<String>,
    /// The firmware vendor.
    pub bios_vendor: Option<String>,
    /// The firmware version.
    pub bios_version: Option<String>,
    /// The firmware date.
    pub bios_date: Option<String>,
    /// The product serial number. Readable only by the administrator.
    pub product_serial: Sensitive,
    /// The product UUID. Readable only by the administrator.
    pub product_uuid: Sensitive,
    /// The board serial number. Readable only by the administrator.
    pub board_serial: Sensitive,
    /// The chassis serial number. Readable only by the administrator.
    pub chassis_serial: Sensitive,
    /// How the machine was booted.
    pub boot_mode: BootMode,
    /// The Secure Boot state.
    pub secure_boot: SecureBoot,
}

/// How the machine was booted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootMode {
    /// Through UEFI.
    Uefi,
    /// Through a legacy BIOS, or UEFI in compatibility mode.
    Bios,
    /// `/sys/firmware` could not be read.
    Unknown,
}

/// The Secure Boot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecureBoot {
    /// Secure Boot is enabled.
    Enabled,
    /// Secure Boot is disabled.
    Disabled,
    /// The machine uses UEFI but the state could not be read.
    Unknown,
    /// The machine was not booted through UEFI.
    NotApplicable,
}

/// Firmware devices and updates known to fwupd.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Firmware {
    /// Whether fwupd answered.
    pub fwupd: FwupdStatus,
    /// The devices fwupd reported.
    pub devices: Vec<FirmwareDevice>,
}

/// Whether fwupd answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FwupdStatus {
    /// fwupd answered and its answer was understood.
    Available,
    /// `fwupdmgr` is missing, failed or timed out.
    Unavailable,
    /// fwupd answered with something that is not the expected JSON.
    Invalid,
}

/// A device whose firmware fwupd knows about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirmwareDevice {
    /// fwupd's own identifier for the device.
    pub fwupd_device_id: Option<String>,
    /// The device's name.
    pub name: Option<String>,
    /// The vendor.
    pub vendor: Option<String>,
    /// The fwupd plugin that handles the device.
    pub plugin: Option<String>,
    /// The installed firmware version.
    pub version: Option<String>,
    /// The device's serial number, where fwupd reports one.
    pub serial: Sensitive,
    /// fwupd's flags for the device, for example `updatable`.
    pub flags: Vec<String>,
    /// Whether fwupd can update the device.
    pub updatable: bool,
    /// The state of the last update, for example `pending` or `needs-reboot`.
    pub update_state: Option<String>,
    /// Releases newer than the installed version.
    pub pending_updates: Vec<FirmwareRelease>,
}

/// A firmware release newer than the installed version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirmwareRelease {
    /// The release's version.
    pub version: Option<String>,
    /// How urgent the vendor says the release is.
    pub urgency: Option<String>,
    /// The fwupd remote that offers the release, for example `lvfs`.
    pub remote: Option<String>,
}

/// What the kernel log says about drivers and firmware.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KernelEvidence {
    /// Whether the kernel log could be read. If not, the lists are empty and
    /// prove nothing.
    pub available: bool,
    /// Firmware files the kernel could not load.
    pub firmware_failures: Vec<FirmwareLoadFailure>,
    /// Drivers that refused a device.
    pub probe_failures: Vec<ProbeFailure>,
}

/// A firmware file the kernel could not load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirmwareLoadFailure {
    /// The driver that asked for the file.
    pub driver: Option<String>,
    /// The kernel name of the device, for example `0000:00:14.3`.
    pub device: Option<String>,
    /// The file name, relative to the firmware directory.
    pub firmware: String,
    /// The kernel's error number, for example `-2`.
    pub error: Option<i32>,
}

/// A driver that refused a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeFailure {
    /// The driver.
    pub driver: Option<String>,
    /// The kernel name of the device.
    pub device: String,
    /// The kernel's error number.
    pub error: Option<i32>,
}

/// A source that could not be used, and what the inventory lacks without it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceNote {
    /// The source, for example `udev` or `fwupd`.
    pub source: String,
    /// What went wrong.
    pub status: SourceStatus,
    /// What the inventory lacks as a result.
    pub effect: String,
}

/// Why a source could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    /// Missing, failed or timed out.
    Unavailable,
    /// Not permitted.
    Denied,
    /// Present but not in the expected form.
    Invalid,
    /// Larger than the limit.
    TooLarge,
}

/// The hardware inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inventory {
    /// The version of this document: [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Whether sensitive values have been removed.
    pub redacted: bool,
    /// The machine itself.
    pub platform: Platform,
    /// PCI functions.
    pub pci: Vec<PciDevice>,
    /// USB devices.
    pub usb: Vec<UsbDevice>,
    /// Network interfaces.
    pub network: Vec<NetworkInterface>,
    /// Sound cards.
    pub audio: Vec<AudioCard>,
    /// Storage controllers.
    pub storage_controllers: Vec<StorageController>,
    /// Whole block devices.
    pub block: Vec<BlockDevice>,
    /// Input devices.
    pub input: Vec<InputDevice>,
    /// Bluetooth adapters.
    pub bluetooth: Vec<BluetoothAdapter>,
    /// V4L2 video devices.
    pub cameras: Vec<Camera>,
    /// Thunderbolt and USB4 devices.
    pub thunderbolt: Vec<ThunderboltDevice>,
    /// Firmware devices and updates.
    pub firmware: Firmware,
    /// Driver and firmware failures from the kernel log.
    pub kernel: KernelEvidence,
    /// Sources that could not be used.
    pub source_notes: Vec<SourceNote>,
}

/// One line of a device listing: what `ListDevices` returns for each device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceSummary {
    /// The stable identifier.
    pub id: String,
    /// The kind of device.
    pub category: Category,
    /// A name for display.
    pub name: String,
    /// The bound kernel driver, or `null`.
    pub driver: Option<String>,
}

/// One device of any category, borrowed from an inventory: what `GetDevice`
/// returns. In JSON it is `{"category": "...", "device": {...}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "category", content = "device", rename_all = "snake_case")]
pub enum DeviceRef<'a> {
    /// A PCI function.
    Pci(&'a PciDevice),
    /// A USB device.
    Usb(&'a UsbDevice),
    /// A network interface.
    Network(&'a NetworkInterface),
    /// A sound card.
    Audio(&'a AudioCard),
    /// A storage controller.
    StorageController(&'a StorageController),
    /// A whole block device.
    Block(&'a BlockDevice),
    /// An input device.
    Input(&'a InputDevice),
    /// A Bluetooth adapter.
    Bluetooth(&'a BluetoothAdapter),
    /// A V4L2 video device.
    Camera(&'a Camera),
    /// A Thunderbolt or USB4 device.
    Thunderbolt(&'a ThunderboltDevice),
}

fn first_of(candidates: &[Option<&str>], fallback: &str) -> String {
    candidates
        .iter()
        .flatten()
        .next()
        .map_or_else(|| fallback.to_owned(), |name| (*name).to_owned())
}

impl DeviceRef<'_> {
    /// The device's category.
    pub fn category(&self) -> Category {
        match self {
            Self::Pci(_) => Category::Pci,
            Self::Usb(_) => Category::Usb,
            Self::Network(_) => Category::Network,
            Self::Audio(_) => Category::Audio,
            Self::StorageController(_) => Category::StorageController,
            Self::Block(_) => Category::Block,
            Self::Input(_) => Category::Input,
            Self::Bluetooth(_) => Category::Bluetooth,
            Self::Camera(_) => Category::Camera,
            Self::Thunderbolt(_) => Category::Thunderbolt,
        }
    }

    /// The device's identifier.
    pub fn id(&self) -> &str {
        match self {
            Self::Pci(device) => &device.id,
            Self::Usb(device) => &device.id,
            Self::Network(device) => &device.id,
            Self::Audio(device) => &device.id,
            Self::StorageController(device) => &device.id,
            Self::Block(device) => &device.id,
            Self::Input(device) => &device.id,
            Self::Bluetooth(device) => &device.id,
            Self::Camera(device) => &device.id,
            Self::Thunderbolt(device) => &device.id,
        }
    }

    /// The one-line summary of the device.
    pub fn summary(&self) -> DeviceSummary {
        let (name, driver) = match self {
            Self::Pci(device) => (
                first_of(
                    &[
                        device.device_name.as_deref(),
                        device.class_name.as_deref(),
                    ],
                    &device.address,
                ),
                device.driver.clone(),
            ),
            Self::Usb(device) => (
                first_of(
                    &[device.product.as_deref(), device.class_name.as_deref()],
                    &device.bus_port,
                ),
                None,
            ),
            Self::Network(device) => (device.name.clone(), device.driver.clone()),
            Self::Audio(device) => (
                first_of(
                    &[device.name.as_deref(), device.card_id.as_deref()],
                    "sound card",
                ),
                device.driver.clone(),
            ),
            Self::StorageController(device) => {
                ("storage controller".to_owned(), device.driver.clone())
            }
            Self::Block(device) => (
                first_of(&[device.model.as_deref()], &device.name),
                device.driver.clone(),
            ),
            Self::Input(device) => (
                first_of(&[device.name.as_deref()], "input device"),
                None,
            ),
            Self::Bluetooth(device) => (device.name.clone(), device.driver.clone()),
            Self::Camera(device) => (
                first_of(&[device.name.as_deref()], &device.node),
                device.driver.clone(),
            ),
            Self::Thunderbolt(device) => (
                first_of(&[device.device_name.as_deref()], &device.name),
                None,
            ),
        };
        DeviceSummary {
            id: self.id().to_owned(),
            category: self.category(),
            name,
            driver,
        }
    }
}

impl Inventory {
    /// Every device of every category, in a fixed order.
    pub fn devices(&self) -> Vec<DeviceRef<'_>> {
        let mut all = Vec::new();
        all.extend(self.pci.iter().map(DeviceRef::Pci));
        all.extend(self.usb.iter().map(DeviceRef::Usb));
        all.extend(self.network.iter().map(DeviceRef::Network));
        all.extend(self.audio.iter().map(DeviceRef::Audio));
        all.extend(
            self.storage_controllers
                .iter()
                .map(DeviceRef::StorageController),
        );
        all.extend(self.block.iter().map(DeviceRef::Block));
        all.extend(self.input.iter().map(DeviceRef::Input));
        all.extend(self.bluetooth.iter().map(DeviceRef::Bluetooth));
        all.extend(self.cameras.iter().map(DeviceRef::Camera));
        all.extend(self.thunderbolt.iter().map(DeviceRef::Thunderbolt));
        all
    }

    /// The one-line summaries of every device: what `ListDevices` returns.
    pub fn summaries(&self) -> Vec<DeviceSummary> {
        self.devices().iter().map(DeviceRef::summary).collect()
    }

    /// The device with this identifier: what `GetDevice` returns.
    pub fn find(&self, id: &str) -> Option<DeviceRef<'_>> {
        self.devices().into_iter().find(|device| device.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_serialises_as_a_marked_object() {
        let present = Sensitive::Present("SN123".to_owned());
        assert_eq!(
            serde_json::to_string(&present).unwrap(),
            r#"{"sensitive":true,"redacted":false,"value":"SN123"}"#
        );
        assert_eq!(
            serde_json::to_string(&Sensitive::Absent).unwrap(),
            r#"{"sensitive":true,"redacted":false,"value":null}"#
        );
        assert_eq!(
            serde_json::to_string(&Sensitive::Redacted).unwrap(),
            r#"{"sensitive":true,"redacted":true,"value":null}"#
        );
    }

    #[test]
    fn sensitive_redacts_only_what_exists() {
        let mut present = Sensitive::Present("SN123".to_owned());
        present.redact();
        assert_eq!(present, Sensitive::Redacted);
        assert_eq!(present.value(), None);

        let mut absent = Sensitive::Absent;
        absent.redact();
        assert_eq!(absent, Sensitive::Absent);
    }

    #[test]
    fn sensitive_debug_does_not_print_the_value() {
        let text = format!("{:?}", Sensitive::Present("SN123".to_owned()));
        assert!(!text.contains("SN123"));
        assert_eq!(text, "Sensitive(present)");
    }

    #[test]
    fn enums_serialise_in_snake_case() {
        let json = |value: serde_json::Value| value.as_str().unwrap().to_owned();
        assert_eq!(
            json(serde_json::to_value(IdBasis::HardwareIdentifier).unwrap()),
            "hardware_identifier"
        );
        assert_eq!(
            json(serde_json::to_value(OperState::LowerLayerDown).unwrap()),
            "lower_layer_down"
        );
        assert_eq!(json(serde_json::to_value(InputBus::I8042).unwrap()), "i8042");
        assert_eq!(json(serde_json::to_value(InputBus::I2c).unwrap()), "i2c");
        assert_eq!(
            json(serde_json::to_value(StorageControllerKind::UsbMassStorage).unwrap()),
            "usb_mass_storage"
        );
        assert_eq!(
            json(serde_json::to_value(Category::StorageController).unwrap()),
            "storage_controller"
        );
    }
}
