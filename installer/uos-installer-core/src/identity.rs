// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Stable device identity, target selection and revalidation
//! (specification §5.2).
//!
//! A device path such as `/dev/sda` says where the kernel happened to put a
//! disk on this boot. It can name a different disk after a reboot, after a
//! USB device is plugged in, or between the moment a user confirms and the
//! moment the installer writes. So a device path is never accepted as a
//! target, and the installer never carries one from one step to the next.
//!
//! What is carried is an [`Identity`]: the properties the disk itself
//! reports. A target is chosen with a [`TargetSelector`], which is either a
//! `/dev/disk/by-id` path or a set of identity attributes, and the choice is
//! turned back into a device only by [`revalidate`], against a fresh
//! inventory, immediately before use.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::codes::{Fault, InstallCode};
use crate::inventory::{BlockDevice, BlockKind, Inventory, Transport};
use crate::units::format_size_exact;

/// Whether an identity rests on an identifier unique to the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityStrength {
    /// The disk reports a serial number or a WWN.
    Stable,
    /// The disk reports neither. It can be told apart only by model, size
    /// and the port it is plugged into.
    Weak,
}

/// What identifies a physical disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Model string.
    pub model: Option<String>,
    /// Serial number.
    pub serial: Option<String>,
    /// World Wide Name or equivalent.
    pub wwn: Option<String>,
    /// Capacity in bytes.
    pub size_bytes: u64,
    /// How the disk is connected.
    pub transport: Transport,
    /// Logical sector size in bytes.
    pub logical_sector_size: u32,
    /// Physical sector size in bytes.
    pub physical_sector_size: u32,
    /// The stable `/dev/disk/by-id` path, when the disk has one.
    pub by_id_path: Option<String>,
    /// The `/dev/disk/by-path` path of the port the disk is connected to.
    /// It identifies a port, not a disk, and is compared only for disks with
    /// a weak identity.
    pub by_path: Option<String>,
}

impl Identity {
    /// Captures the identity of a device in an inventory.
    pub fn of(device: &BlockDevice) -> Identity {
        Identity {
            model: device.model.clone(),
            serial: device.serial.clone(),
            wwn: device.wwn.clone(),
            size_bytes: device.size_bytes,
            transport: device.transport,
            logical_sector_size: device.logical_sector_size,
            physical_sector_size: device.physical_sector_size,
            by_id_path: device.preferred_by_id().map(str::to_owned),
            by_path: device.by_path.first().cloned(),
        }
    }

    /// True when the disk reports a serial number or a WWN.
    pub fn is_stable(&self) -> bool {
        self.serial.is_some() || self.wwn.is_some()
    }

    /// The strength of this identity.
    pub fn strength(&self) -> IdentityStrength {
        if self.is_stable() {
            IdentityStrength::Stable
        } else {
            IdentityStrength::Weak
        }
    }

    /// True when `device` has exactly this identity.
    ///
    /// Model, serial, WWN, size, transport and sector sizes must be equal.
    /// The recorded by-id path, if any, must still point at the device. For
    /// a weak identity the recorded port must also be unchanged.
    pub fn matches(&self, device: &BlockDevice) -> bool {
        self.serial == device.serial
            && self.wwn == device.wwn
            && self.model == device.model
            && self.size_bytes == device.size_bytes
            && self.transport == device.transport
            && self.logical_sector_size == device.logical_sector_size
            && self.physical_sector_size == device.physical_sector_size
            && self
                .by_id_path
                .as_deref()
                .is_none_or(|path| has_link(&device.by_id, path))
            && (self.is_stable()
                || self
                    .by_path
                    .as_deref()
                    .is_none_or(|path| has_link(&device.by_path, path)))
    }

    /// True when `device` carries one of the identifiers this identity is
    /// anchored on, even if other attributes differ.
    fn shares_anchor(&self, device: &BlockDevice) -> bool {
        let same_serial = self.serial.is_some() && self.serial == device.serial;
        let same_wwn = self.wwn.is_some() && self.wwn == device.wwn;
        let same_by_id = self
            .by_id_path
            .as_deref()
            .is_some_and(|path| has_link(&device.by_id, path));
        let same_port = !self.is_stable()
            && self
                .by_path
                .as_deref()
                .is_some_and(|path| has_link(&device.by_path, path));
        same_serial || same_wwn || same_by_id || same_port
    }

    /// Says how `device`, which shares an anchor with this identity, differs.
    fn difference_from(&self, device: &BlockDevice) -> IdentityMismatch {
        if self.serial != device.serial {
            return IdentityMismatch::SerialChanged {
                expected: self.serial.clone(),
                found: device.serial.clone(),
            };
        }
        if self.size_bytes != device.size_bytes {
            return IdentityMismatch::SizeChanged {
                expected: self.size_bytes,
                found: device.size_bytes,
            };
        }
        let mut fields = Vec::new();
        if self.model != device.model {
            fields.push("model");
        }
        if self.wwn != device.wwn {
            fields.push("wwn");
        }
        if self.transport != device.transport {
            fields.push("transport");
        }
        if self.logical_sector_size != device.logical_sector_size {
            fields.push("logical_sector_size");
        }
        if self.physical_sector_size != device.physical_sector_size {
            fields.push("physical_sector_size");
        }
        if fields.is_empty() {
            fields.push("stable_links");
        }
        IdentityMismatch::AttributesChanged { fields }
    }

    /// The lines that show this identity to a person: model and capacity,
    /// serial, WWN, stable path, connection and sector sizes.
    pub fn display_lines(&self) -> Vec<String> {
        let model = self.model.as_deref().unwrap_or("Unknown model");
        let serial = self.serial.as_deref().unwrap_or("not reported by the drive");
        let wwn = self.wwn.as_deref().unwrap_or("not reported by the drive");
        let stable_path = self
            .by_id_path
            .as_deref()
            .unwrap_or("none: the drive has no stable identifier");
        let mut lines = vec![
            format!("Drive:        {model}"),
            format!("Capacity:     {}", format_size_exact(self.size_bytes)),
            format!("Serial:       {serial}"),
            format!("WWN:          {wwn}"),
            format!("Stable path:  {stable_path}"),
            format!("Connection:   {}", self.transport.label()),
            format!(
                "Sector size:  {} bytes logical, {} bytes physical",
                self.logical_sector_size, self.physical_sector_size
            ),
        ];
        if let Some(port) = &self.by_path {
            lines.push(format!("Port:         {port}"));
        }
        lines
    }
}

fn has_link(links: &[String], path: &str) -> bool {
    links.iter().any(|link| link.as_str() == path)
}

fn paths(devices: &[&BlockDevice]) -> Vec<String> {
    devices.iter().map(|device| device.path.clone()).collect()
}

/// Why the disk an identity describes cannot be found unambiguously.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityMismatch {
    /// No disk carries the expected identifiers.
    Missing,
    /// The disk with the expected identifiers has a different size.
    SizeChanged {
        /// The recorded size in bytes.
        expected: u64,
        /// The size found now.
        found: u64,
    },
    /// The disk now found at the recorded stable path, WWN or port reports a
    /// different serial number: it is another disk.
    SerialChanged {
        /// The recorded serial number.
        expected: Option<String>,
        /// The serial number found now.
        found: Option<String>,
    },
    /// Two or more disks match. The installer cannot tell which one was meant.
    Ambiguous {
        /// The device nodes of the matching disks, for display.
        devices: Vec<String>,
    },
    /// The disk is present but reports different attributes.
    AttributesChanged {
        /// The names of the attributes that differ.
        fields: Vec<&'static str>,
    },
}

impl fmt::Display for IdentityMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            IdentityMismatch::Missing => {
                "The disk chosen for installation is no longer connected. \
                 Nothing was written. Reconnect it or choose a disk again."
            }
            IdentityMismatch::SizeChanged { .. } => {
                "The disk chosen for installation now reports a different size. \
                 Nothing was written. Choose the disk again."
            }
            IdentityMismatch::SerialChanged { .. } => {
                "A different disk is now where the chosen disk was. \
                 Nothing was written. Choose the disk again."
            }
            IdentityMismatch::Ambiguous { .. } => {
                "More than one connected disk matches the chosen disk, so the installer \
                 cannot tell them apart. Nothing was written. Disconnect the disk you do \
                 not want to erase, then choose again."
            }
            IdentityMismatch::AttributesChanged { .. } => {
                "The disk chosen for installation now reports different properties. \
                 Nothing was written. Choose the disk again."
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for IdentityMismatch {}

impl Fault for IdentityMismatch {
    fn code(&self) -> InstallCode {
        match self {
            IdentityMismatch::Missing => InstallCode::IdentityMissing,
            IdentityMismatch::SizeChanged { .. } => InstallCode::IdentitySizeChanged,
            IdentityMismatch::SerialChanged { .. } => InstallCode::IdentitySerialChanged,
            IdentityMismatch::Ambiguous { .. } => InstallCode::IdentityAmbiguous,
            IdentityMismatch::AttributesChanged { .. } => InstallCode::IdentityAttributesChanged,
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            IdentityMismatch::Missing => {
                "no whole disk carries the recorded serial, WWN, by-id link or port".to_owned()
            }
            IdentityMismatch::SizeChanged { expected, found } => {
                format!("size_bytes: expected {expected}, found {found}")
            }
            IdentityMismatch::SerialChanged { expected, found } => format!(
                "serial: expected {}, found {}",
                expected.as_deref().unwrap_or("none"),
                found.as_deref().unwrap_or("none")
            ),
            IdentityMismatch::Ambiguous { devices } => {
                format!("matching devices in this inventory: {}", devices.join(", "))
            }
            IdentityMismatch::AttributesChanged { fields } => {
                format!("changed: {}", fields.join(", "))
            }
        }
    }
}

/// Finds the disk `expected` describes in `current`, a fresh inventory.
///
/// Returns the device only when exactly one whole disk has that identity.
/// Call this immediately before any use of the device; never keep the
/// returned reference across a new probe.
pub fn revalidate<'a>(
    expected: &Identity,
    current: &'a Inventory,
) -> Result<&'a BlockDevice, IdentityMismatch> {
    let disks = current.disks();
    let matches: Vec<&BlockDevice> = disks
        .iter()
        .copied()
        .filter(|disk| expected.matches(disk))
        .collect();
    if matches.len() == 1 {
        return Ok(matches[0]);
    }
    if matches.len() > 1 {
        return Err(IdentityMismatch::Ambiguous {
            devices: paths(&matches),
        });
    }
    let anchored: Vec<&BlockDevice> = disks
        .iter()
        .copied()
        .filter(|disk| expected.shares_anchor(disk))
        .collect();
    match anchored.as_slice() {
        [] => Err(IdentityMismatch::Missing),
        [device] => Err(expected.difference_from(device)),
        many => Err(IdentityMismatch::Ambiguous {
            devices: paths(many),
        }),
    }
}

/// Identity attributes used to choose a disk when no by-id path is given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributeSelector {
    /// Required model string.
    pub model: Option<String>,
    /// Required serial number.
    pub serial: Option<String>,
    /// Required WWN.
    pub wwn: Option<String>,
    /// Required size in bytes.
    pub size_bytes: Option<u64>,
    /// Required `/dev/disk/by-path` path of the port.
    pub by_path: Option<String>,
}

impl AttributeSelector {
    fn matches(&self, device: &BlockDevice) -> bool {
        self.model.as_ref().is_none_or(|model| device.model.as_ref() == Some(model))
            && self.serial.as_ref().is_none_or(|serial| device.serial.as_ref() == Some(serial))
            && self.wwn.as_ref().is_none_or(|wwn| device.wwn.as_ref() == Some(wwn))
            && self.size_bytes.is_none_or(|size| device.size_bytes == size)
            && self.by_path.as_deref().is_none_or(|path| has_link(&device.by_path, path))
    }
}

/// How the user names the disk to install on.
///
/// Parsed from text with [`FromStr`]:
///
/// * `/dev/disk/by-id/<name>` selects by stable link.
/// * `key=value[,key=value...]` selects by identity attributes. The keys are
///   `model`, `serial`, `wwn`, `size` (bytes) and `by-path`.
///
/// Any other `/dev/...` path, and any bare kernel name, is rejected.
///
/// ```
/// use uos_installer_core::identity::{SelectError, TargetSelector};
///
/// assert!("/dev/disk/by-id/ata-Example_SSD_S123".parse::<TargetSelector>().is_ok());
/// assert!("serial=S123".parse::<TargetSelector>().is_ok());
/// assert!(matches!(
///     "/dev/sda".parse::<TargetSelector>(),
///     Err(SelectError::DevicePathRejected { .. })
/// ));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetSelector {
    /// A `/dev/disk/by-id/...` path.
    ById(String),
    /// Identity attributes that must all match.
    Attributes(AttributeSelector),
}

impl fmt::Display for TargetSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetSelector::ById(path) => f.write_str(path),
            TargetSelector::Attributes(attributes) => {
                let mut parts = Vec::new();
                if let Some(model) = &attributes.model {
                    parts.push(format!("model={model}"));
                }
                if let Some(serial) = &attributes.serial {
                    parts.push(format!("serial={serial}"));
                }
                if let Some(wwn) = &attributes.wwn {
                    parts.push(format!("wwn={wwn}"));
                }
                if let Some(size) = attributes.size_bytes {
                    parts.push(format!("size={size}"));
                }
                if let Some(by_path) = &attributes.by_path {
                    parts.push(format!("by-path={by_path}"));
                }
                f.write_str(&parts.join(","))
            }
        }
    }
}

impl FromStr for TargetSelector {
    type Err = SelectError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        let invalid = |reason: &str| SelectError::InvalidSelector {
            text: text.to_owned(),
            reason: reason.to_owned(),
        };
        if let Some(name) = text.strip_prefix("/dev/disk/by-id/") {
            if name.is_empty() || name.contains('/') {
                return Err(invalid("the by-id path does not name a link"));
            }
            return Ok(TargetSelector::ById(text.to_owned()));
        }
        if text.starts_with("/dev/") {
            return Err(SelectError::DevicePathRejected {
                path: text.to_owned(),
            });
        }
        if !text.contains('=') {
            return Err(invalid(
                "expected a /dev/disk/by-id path or key=value identity attributes; \
                 kernel names such as sda are never accepted",
            ));
        }
        let mut attributes = AttributeSelector::default();
        for pair in text.split(',') {
            let Some((key, value)) = pair.split_once('=') else {
                return Err(invalid("each attribute must be written key=value"));
            };
            let value = value.trim();
            if value.is_empty() {
                return Err(invalid("attribute values must not be empty"));
            }
            match key.trim() {
                "model" => attributes.model = Some(value.to_owned()),
                "serial" => attributes.serial = Some(value.to_owned()),
                "wwn" => attributes.wwn = Some(value.to_owned()),
                "size" => {
                    let size = value
                        .parse()
                        .map_err(|_| invalid("size must be a whole number of bytes"))?;
                    attributes.size_bytes = Some(size);
                }
                "by-path" => {
                    if !value.starts_with("/dev/disk/by-path/") {
                        return Err(invalid("by-path must be a /dev/disk/by-path path"));
                    }
                    attributes.by_path = Some(value.to_owned());
                }
                _ => {
                    return Err(invalid(
                        "unknown attribute; use model, serial, wwn, size or by-path",
                    ));
                }
            }
        }
        Ok(TargetSelector::Attributes(attributes))
    }
}

/// Whether the user has acknowledged that the chosen disk has no stable
/// identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeakIdentityPolicy {
    /// Refuse a disk without a serial number or WWN.
    Refuse,
    /// The user explicitly accepted the weaker identity.
    Acknowledged,
}

/// A disk chosen as the installation target, with the identity captured at
/// the moment of choice.
///
/// Values are created only by [`select_target`].
#[derive(Debug, Clone)]
pub struct SelectedTarget<'a> {
    device: &'a BlockDevice,
    identity: Identity,
    weaker_identity_acknowledged: bool,
}

impl<'a> SelectedTarget<'a> {
    /// The chosen device in the inventory it was selected from.
    pub fn device(&self) -> &'a BlockDevice {
        self.device
    }

    /// The identity captured when the device was chosen.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// True when the disk has only a weak identity and the user accepted that.
    pub fn weaker_identity_acknowledged(&self) -> bool {
        self.weaker_identity_acknowledged
    }
}

/// Chooses the installation target in `inventory`.
///
/// The selector must resolve to exactly one whole disk that holds a medium,
/// and no other disk in the inventory may share its identity. A disk without
/// a serial number or WWN is accepted only under
/// [`WeakIdentityPolicy::Acknowledged`].
pub fn select_target<'a>(
    inventory: &'a Inventory,
    selector: &TargetSelector,
    weak_identity: WeakIdentityPolicy,
) -> Result<SelectedTarget<'a>, SelectError> {
    let disks = inventory.disks();
    let candidates: Vec<&BlockDevice> = match selector {
        TargetSelector::ById(path) => {
            let holders: Vec<&BlockDevice> = inventory
                .all_nodes()
                .into_iter()
                .filter(|node| has_link(&node.by_id, path))
                .collect();
            for node in &holders {
                if !inventory.is_whole_disk(node) {
                    return Err(SelectError::NotWholeDisk {
                        selector: path.clone(),
                        kind: node.kind,
                    });
                }
            }
            holders
        }
        TargetSelector::Attributes(attributes) => disks
            .iter()
            .copied()
            .filter(|disk| attributes.matches(disk))
            .collect(),
    };
    let device = match candidates.as_slice() {
        [] => {
            return Err(SelectError::NotFound {
                selector: selector.to_string(),
            });
        }
        [device] => *device,
        many => {
            return Err(SelectError::Ambiguous {
                selector: selector.to_string(),
                devices: paths(many),
            });
        }
    };
    if !device.has_medium() {
        return Err(SelectError::NoMedium {
            selector: selector.to_string(),
        });
    }
    let identity = Identity::of(device);
    let twins: Vec<&BlockDevice> = disks
        .iter()
        .copied()
        .filter(|disk| identity.matches(disk))
        .collect();
    if twins.len() != 1 {
        return Err(SelectError::Ambiguous {
            selector: selector.to_string(),
            devices: paths(&twins),
        });
    }
    let weak = !identity.is_stable();
    if weak && weak_identity == WeakIdentityPolicy::Refuse {
        return Err(SelectError::WeakIdentityNotAcknowledged {
            selector: selector.to_string(),
        });
    }
    Ok(SelectedTarget {
        device,
        identity,
        weaker_identity_acknowledged: weak,
    })
}

/// Why a target could not be chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectError {
    /// A device path such as `/dev/sda` was given. It is not an identity.
    DevicePathRejected {
        /// The path that was given.
        path: String,
    },
    /// The selector text is not in an accepted form.
    InvalidSelector {
        /// The text that was given.
        text: String,
        /// What is wrong with it.
        reason: String,
    },
    /// No disk matches.
    NotFound {
        /// The selector as text.
        selector: String,
    },
    /// Several disks match, or another disk shares the chosen disk's identity.
    Ambiguous {
        /// The selector as text.
        selector: String,
        /// The device nodes of the matching disks, for display.
        devices: Vec<String>,
    },
    /// The selector names a partition or a mapping, not a whole disk.
    NotWholeDisk {
        /// The selector as text.
        selector: String,
        /// What the selector names.
        kind: BlockKind,
    },
    /// The drive holds no medium.
    NoMedium {
        /// The selector as text.
        selector: String,
    },
    /// The disk has no serial number or WWN and that was not acknowledged.
    WeakIdentityNotAcknowledged {
        /// The selector as text.
        selector: String,
    },
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            SelectError::DevicePathRejected { .. } => {
                "A device path is not accepted as an installation target, because it can \
                 name a different disk on the next boot. Choose the disk by its \
                 /dev/disk/by-id path, or by serial number or WWN."
            }
            SelectError::InvalidSelector { .. } => {
                "The installation target was not understood. Give a /dev/disk/by-id path, \
                 or identity attributes such as serial=<serial number>."
            }
            SelectError::NotFound { .. } => {
                "No connected disk matches the chosen installation target. Check that the \
                 disk is connected and list the disks again."
            }
            SelectError::Ambiguous { .. } => {
                "More than one connected disk matches the chosen installation target, so \
                 the installer cannot tell them apart. Disconnect the disk you do not \
                 want to erase, or choose by a more specific identifier."
            }
            SelectError::NotWholeDisk { .. } => {
                "The chosen installation target is a partition or a volume, not a whole \
                 disk. Choose the disk that contains it."
            }
            SelectError::NoMedium { .. } => {
                "The chosen drive holds no medium. Insert one, or choose another disk."
            }
            SelectError::WeakIdentityNotAcknowledged { .. } => {
                "The chosen disk reports no serial number or WWN, so the installer can \
                 recognise it only by model, size and port. Choose another disk, or \
                 explicitly acknowledge the weaker identity."
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for SelectError {}

impl Fault for SelectError {
    fn code(&self) -> InstallCode {
        match self {
            SelectError::DevicePathRejected { .. } => InstallCode::TargetDevicePath,
            SelectError::InvalidSelector { .. } => InstallCode::TargetSelectorInvalid,
            SelectError::NotFound { .. } => InstallCode::TargetNotFound,
            SelectError::Ambiguous { .. } => InstallCode::TargetAmbiguous,
            SelectError::NotWholeDisk { .. } => InstallCode::TargetNotWholeDisk,
            SelectError::NoMedium { .. } => InstallCode::TargetNoMedium,
            SelectError::WeakIdentityNotAcknowledged { .. } => InstallCode::TargetWeakIdentity,
        }
    }

    fn technical_detail(&self) -> String {
        match self {
            SelectError::DevicePathRejected { path } => format!("rejected target: {path}"),
            SelectError::InvalidSelector { text, reason } => {
                format!("selector \"{text}\": {reason}")
            }
            SelectError::NotFound { selector } => format!("selector: {selector}"),
            SelectError::Ambiguous { selector, devices } => {
                format!("selector: {selector}; matching devices: {}", devices.join(", "))
            }
            SelectError::NotWholeDisk { selector, kind } => {
                format!("selector: {selector}; it names a {}", kind.label())
            }
            SelectError::NoMedium { selector } => {
                format!("selector: {selector}; the drive reports a size of 0 bytes")
            }
            SelectError::WeakIdentityNotAcknowledged { selector } => {
                format!("selector: {selector}; the disk reports neither serial nor WWN")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{oddities, workstation};

    const NVME_BY_ID: &str = "/dev/disk/by-id/nvme-Samsung_SSD_980_PRO_1TB_S5GXNX0T123456A";

    fn selector(text: &str) -> TargetSelector {
        text.parse()
            .unwrap_or_else(|error| panic!("{text} did not parse: {error}"))
    }

    fn select<'a>(
        inventory: &'a Inventory,
        text: &str,
    ) -> Result<SelectedTarget<'a>, SelectError> {
        select_target(inventory, &selector(text), WeakIdentityPolicy::Refuse)
    }

    fn disk_mut<'a>(inventory: &'a mut Inventory, name: &str) -> &'a mut BlockDevice {
        inventory
            .devices
            .iter_mut()
            .find(|device| device.name == name)
            .unwrap_or_else(|| panic!("fixture has no device {name}"))
    }

    #[test]
    fn device_paths_are_never_accepted() {
        for text in [
            "/dev/sda",
            "/dev/nvme0n1",
            "/dev/nvme0n1p3",
            "/dev/mapper/luks-root",
            "/dev/disk/by-uuid/0a1b2c3d",
            "/dev/disk/by-label/Windows",
            "/dev/disk/by-path/pci-0000:01:00.0-nvme-1",
            "/dev/disk/by-partuuid/1234",
            "  /dev/sda  ",
        ] {
            let error = text.parse::<TargetSelector>().unwrap_err();
            assert_eq!(error.code(), InstallCode::TargetDevicePath, "{text}");
            assert!(error.technical_detail().contains(text.trim()));
        }
    }

    #[test]
    fn kernel_names_and_malformed_selectors_are_rejected() {
        for text in [
            "",
            "sda",
            "nvme0n1",
            "/dev/disk/by-id/",
            "/dev/disk/by-id/a/b",
            "serial=",
            "colour=blue",
            "size=big",
            "serial=S1,sda",
            "by-path=/dev/sda",
        ] {
            let error = text.parse::<TargetSelector>().unwrap_err();
            assert_eq!(error.code(), InstallCode::TargetSelectorInvalid, "{text}");
        }
    }

    #[test]
    fn selectors_display_in_the_form_they_parse_from() {
        for text in [
            NVME_BY_ID,
            "serial=S5GXNX0T123456A",
            "model=Samsung SSD 980 PRO 1TB,serial=S5GXNX0T123456A,wwn=eui.002538b111b2c3d4,size=1000204886016",
            "by-path=/dev/disk/by-path/pci-0000:00:14.0-usb-0:3:1.0-scsi-0:0:0:0",
        ] {
            assert_eq!(selector(text).to_string(), text);
        }
    }

    #[test]
    fn selects_a_disk_by_its_by_id_path() {
        let inventory = workstation();
        let target = select(&inventory, NVME_BY_ID).unwrap();
        assert_eq!(target.device().name, "nvme0n1");
        assert!(!target.weaker_identity_acknowledged());
        let identity = target.identity();
        assert_eq!(identity.model.as_deref(), Some("Samsung SSD 980 PRO 1TB"));
        assert_eq!(identity.serial.as_deref(), Some("S5GXNX0T123456A"));
        assert_eq!(identity.wwn.as_deref(), Some("eui.002538b111b2c3d4"));
        assert_eq!(identity.size_bytes, 1_000_204_886_016);
        assert_eq!(identity.transport, Transport::Nvme);
        assert_eq!(identity.logical_sector_size, 512);
        assert_eq!(identity.by_id_path.as_deref(), Some(NVME_BY_ID));
        assert_eq!(identity.strength(), IdentityStrength::Stable);
    }

    #[test]
    fn selects_a_disk_by_identity_attributes() {
        let inventory = workstation();
        for text in [
            "serial=S5GXNX0T123456A",
            "wwn=eui.002538b111b2c3d4",
            "model=Samsung SSD 980 PRO 1TB",
            "model=Samsung SSD 980 PRO 1TB,size=1000204886016",
        ] {
            let target = select(&inventory, text).unwrap();
            assert_eq!(target.device().name, "nvme0n1", "{text}");
        }
        let sata = select(&inventory, "serial=S4EWNX0M700001B").unwrap();
        assert_eq!(sata.device().name, "sda");
    }

    #[test]
    fn selection_errors_are_typed() {
        let inventory = workstation();
        let missing = select(&inventory, "serial=NOSUCHSERIAL").unwrap_err();
        assert_eq!(missing.code(), InstallCode::TargetNotFound);

        let unknown_link = select(&inventory, "/dev/disk/by-id/ata-Not_Here").unwrap_err();
        assert_eq!(unknown_link.code(), InstallCode::TargetNotFound);

        let partition = select(&inventory, &format!("{NVME_BY_ID}-part3")).unwrap_err();
        assert_eq!(partition.code(), InstallCode::TargetNotWholeDisk);
        assert!(partition.technical_detail().contains("partition"));

        let reader = select(&inventory, "model=Card Reader").unwrap_err();
        assert_eq!(reader.code(), InstallCode::TargetNoMedium);
    }

    #[test]
    fn a_disk_without_stable_identifiers_needs_an_acknowledgement() {
        let inventory = oddities();
        let text = "model=Generic Mass Storage,size=64023257088";
        let refused = select(&inventory, text).unwrap_err();
        assert_eq!(refused.code(), InstallCode::TargetWeakIdentity);

        let accepted =
            select_target(&inventory, &selector(text), WeakIdentityPolicy::Acknowledged).unwrap();
        assert!(accepted.weaker_identity_acknowledged());
        assert_eq!(accepted.identity().strength(), IdentityStrength::Weak);
        assert_eq!(accepted.identity().by_id_path, None);
        assert!(accepted.identity().by_path.is_some());
    }

    #[test]
    fn acknowledging_a_weak_identity_is_not_recorded_for_a_stable_disk() {
        let inventory = workstation();
        let target = select_target(
            &inventory,
            &selector(NVME_BY_ID),
            WeakIdentityPolicy::Acknowledged,
        )
        .unwrap();
        assert!(!target.weaker_identity_acknowledged());
    }

    #[test]
    fn duplicate_serials_are_ambiguous_however_they_are_selected() {
        let inventory = oddities();
        for text in [
            "serial=0123456789ABCDEF",
            "/dev/disk/by-id/usb-JMicron_Tech_0123456789ABCDEF-0:0",
            "by-path=/dev/disk/by-path/pci-0000:00:14.0-usb-0:1:1.0-scsi-0:0:0:0",
        ] {
            let error = select(&inventory, text).unwrap_err();
            assert_eq!(error.code(), InstallCode::TargetAmbiguous, "{text}");
            let SelectError::Ambiguous { devices, .. } = error else {
                panic!("expected an ambiguity for {text}");
            };
            assert_eq!(devices, ["/dev/sdb", "/dev/sdc"], "{text}");
        }
    }

    #[test]
    fn multipath_paths_are_ambiguous() {
        let inventory = oddities();
        let error = select(&inventory, "wwn=0x5000c500a1b2c3d4").unwrap_err();
        assert_eq!(error.code(), InstallCode::TargetAmbiguous);
        let by_link = select(&inventory, "/dev/disk/by-id/wwn-0x5000c500a1b2c3d4").unwrap_err();
        assert_eq!(by_link.code(), InstallCode::TargetAmbiguous);
    }

    #[test]
    fn revalidation_finds_the_same_disk_under_a_new_kernel_name() {
        let inventory = workstation();
        let identity = select(&inventory, NVME_BY_ID).unwrap().identity().clone();

        let mut later = workstation();
        later.devices.reverse();
        {
            let nvme = disk_mut(&mut later, "nvme0n1");
            nvme.name = "nvme1n1".to_owned();
            nvme.kernel_name = "nvme1n1".to_owned();
            nvme.path = "/dev/nvme1n1".to_owned();
        }
        let found = revalidate(&identity, &later).unwrap();
        assert_eq!(found.path, "/dev/nvme1n1");
        assert_eq!(found.serial.as_deref(), Some("S5GXNX0T123456A"));
    }

    #[test]
    fn revalidation_reports_a_missing_disk() {
        let inventory = workstation();
        let identity = select(&inventory, NVME_BY_ID).unwrap().identity().clone();
        let mut later = workstation();
        later.devices.retain(|device| device.name != "nvme0n1");
        let mismatch = revalidate(&identity, &later).unwrap_err();
        assert_eq!(mismatch, IdentityMismatch::Missing);
        assert_eq!(mismatch.code(), InstallCode::IdentityMissing);
    }

    #[test]
    fn revalidation_reports_a_changed_size() {
        let inventory = workstation();
        let identity = select(&inventory, NVME_BY_ID).unwrap().identity().clone();
        let mut later = workstation();
        disk_mut(&mut later, "nvme0n1").size_bytes = 500_107_862_016;
        assert_eq!(
            revalidate(&identity, &later).unwrap_err(),
            IdentityMismatch::SizeChanged {
                expected: 1_000_204_886_016,
                found: 500_107_862_016
            }
        );
    }

    #[test]
    fn revalidation_reports_a_different_disk_in_the_same_place() {
        let inventory = workstation();
        let identity = select(&inventory, NVME_BY_ID).unwrap().identity().clone();
        let mut later = workstation();
        // Another disk now answers at the recorded by-id path and WWN.
        disk_mut(&mut later, "nvme0n1").serial = Some("S5GXNX0T999999Z".to_owned());
        let mismatch = revalidate(&identity, &later).unwrap_err();
        assert_eq!(
            mismatch,
            IdentityMismatch::SerialChanged {
                expected: Some("S5GXNX0T123456A".to_owned()),
                found: Some("S5GXNX0T999999Z".to_owned())
            }
        );
        assert_eq!(mismatch.code(), InstallCode::IdentitySerialChanged);
    }

    #[test]
    fn revalidation_reports_ambiguity_when_a_twin_appears() {
        let inventory = workstation();
        let identity = select(&inventory, NVME_BY_ID).unwrap().identity().clone();
        let mut later = workstation();
        let mut twin = disk_mut(&mut later, "nvme0n1").clone();
        twin.name = "nvme7n1".to_owned();
        twin.kernel_name = "nvme7n1".to_owned();
        twin.path = "/dev/nvme7n1".to_owned();
        later.devices.push(twin);
        let mismatch = revalidate(&identity, &later).unwrap_err();
        assert_eq!(
            mismatch,
            IdentityMismatch::Ambiguous {
                devices: vec!["/dev/nvme0n1".to_owned(), "/dev/nvme7n1".to_owned()]
            }
        );
        assert_eq!(mismatch.code(), InstallCode::IdentityAmbiguous);
    }

    #[test]
    fn revalidation_reports_other_changed_attributes() {
        let inventory = workstation();
        let identity = select(&inventory, NVME_BY_ID).unwrap().identity().clone();
        let mut later = workstation();
        {
            let nvme = disk_mut(&mut later, "nvme0n1");
            nvme.model = Some("Samsung SSD 980 PRO 2TB".to_owned());
            nvme.logical_sector_size = 4096;
        }
        assert_eq!(
            revalidate(&identity, &later).unwrap_err(),
            IdentityMismatch::AttributesChanged {
                fields: vec!["model", "logical_sector_size"]
            }
        );

        let mut unlinked = workstation();
        disk_mut(&mut unlinked, "nvme0n1").by_id.clear();
        assert_eq!(
            revalidate(&identity, &unlinked).unwrap_err(),
            IdentityMismatch::AttributesChanged {
                fields: vec!["stable_links"]
            }
        );
    }

    #[test]
    fn a_weak_identity_does_not_follow_a_disk_to_another_port() {
        let inventory = oddities();
        let text = "model=Generic Mass Storage,size=64023257088";
        let identity =
            select_target(&inventory, &selector(text), WeakIdentityPolicy::Acknowledged)
                .unwrap()
                .identity()
                .clone();
        assert!(revalidate(&identity, &inventory).is_ok());

        let mut moved = oddities();
        disk_mut(&mut moved, "sda").by_path =
            vec!["/dev/disk/by-path/pci-0000:00:14.0-usb-0:9:1.0-scsi-0:0:0:0".to_owned()];
        assert_eq!(
            revalidate(&identity, &moved).unwrap_err(),
            IdentityMismatch::Missing
        );

        let mut swapped = oddities();
        disk_mut(&mut swapped, "sda").size_bytes = 32_000_000_000;
        assert_eq!(
            revalidate(&identity, &swapped).unwrap_err(),
            IdentityMismatch::SizeChanged {
                expected: 64_023_257_088,
                found: 32_000_000_000
            }
        );
    }

    #[test]
    fn a_stable_identity_follows_a_disk_to_another_port() {
        let inventory = workstation();
        let identity = select(&inventory, "serial=S4EWNX0M700001B")
            .unwrap()
            .identity()
            .clone();
        let mut moved = workstation();
        disk_mut(&mut moved, "sda").by_path =
            vec!["/dev/disk/by-path/pci-0000:00:17.0-ata-6".to_owned()];
        assert_eq!(revalidate(&identity, &moved).unwrap().name, "sda");
    }

    #[test]
    fn identity_lines_show_what_the_specification_requires() {
        let inventory = workstation();
        let lines = select(&inventory, NVME_BY_ID)
            .unwrap()
            .identity()
            .display_lines();
        let text = lines.join("\n");
        for needle in [
            "Samsung SSD 980 PRO 1TB",
            "931.5 GiB (1000204886016 bytes)",
            "S5GXNX0T123456A",
            "eui.002538b111b2c3d4",
            NVME_BY_ID,
            "NVMe",
            "512 bytes logical",
        ] {
            assert!(text.contains(needle), "missing {needle} in:\n{text}");
        }
    }
}
