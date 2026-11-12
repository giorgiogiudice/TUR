// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The redacted view of an inventory (specification §6.5).
//!
//! [`Inventory::redacted`] removes the value of every [`Sensitive`] field:
//! serial numbers, MAC and Bluetooth addresses, hardware UUIDs, world-wide
//! identifiers and the physical paths of input devices. It then removes the
//! same values wherever they recur inside other text. That matters in
//! practice: udev names a USB network adapter `enx` followed by its MAC
//! address, so the address would otherwise survive in the interface name and
//! in its sysfs path.
//!
//! Device identifiers are not altered. They are either positions or hashes
//! made with the machine identifier (see [`crate::ids`]), so they carry no
//! serial number, and they must stay the same in both views so that a device
//! found in a redacted listing can be asked for by its identifier.
//!
//! Each model type lists every one of its fields here, either as walked or as
//! skipped. A field added to the model without being listed is a compile
//! error, so a new sensitive field cannot be forgotten silently.

use crate::model::{
    AudioCard, BlockDevice, BluetoothAdapter, Camera, Firmware, FirmwareDevice,
    FirmwareLoadFailure, FirmwareRelease, InputDevice, Inventory, KernelEvidence, NetworkInterface,
    PciDevice, Platform, ProbeFailure, Sensitive, StorageController, ThunderboltDevice, UsbDevice,
    UsbInterface,
};

/// What replaces a removed value where it occurred inside other text.
pub const REDACTION_MARK: &str = "<redacted>";

/// The shortest value that is searched for inside other text. A shorter value
/// (a serial number of `1`, say) is removed from its own field only: searching
/// for it everywhere would damage unrelated text and protect nothing.
pub const MIN_SCRUB_CHARS: usize = 6;

/// One field met while walking the model.
enum Field<'a> {
    /// A sensitive value.
    Secret(&'a mut Sensitive),
    /// Ordinary text, which might contain a sensitive value.
    Text(&'a mut String),
}

trait Walk {
    fn walk(&mut self, visit: &mut dyn FnMut(Field<'_>));
}

impl Walk for String {
    fn walk(&mut self, visit: &mut dyn FnMut(Field<'_>)) {
        visit(Field::Text(self));
    }
}

impl Walk for Sensitive {
    fn walk(&mut self, visit: &mut dyn FnMut(Field<'_>)) {
        visit(Field::Secret(self));
    }
}

impl<T: Walk> Walk for Option<T> {
    fn walk(&mut self, visit: &mut dyn FnMut(Field<'_>)) {
        if let Some(inner) = self {
            inner.walk(visit);
        }
    }
}

impl<T: Walk> Walk for Vec<T> {
    fn walk(&mut self, visit: &mut dyn FnMut(Field<'_>)) {
        for item in self {
            item.walk(visit);
        }
    }
}

/// Implement [`Walk`] for a model type, naming every field. Fields under
/// `skip` are identifiers, numbers, flags, enumerations and fixed-format codes
/// that cannot hold a sensitive value.
macro_rules! walk {
    ($type:ty { walk: [$($walked:ident),+ $(,)?], skip: [$($skipped:ident),* $(,)?] }) => {
        impl Walk for $type {
            fn walk(&mut self, visit: &mut dyn FnMut(Field<'_>)) {
                let Self { $($walked,)+ $($skipped: _,)* } = self;
                $($walked.walk(visit);)+
            }
        }
    };
}

walk!(Inventory {
    walk: [
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
    ],
    // `source_notes` holds only text written by this crate.
    skip: [schema_version, redacted, source_notes]
});

walk!(Platform {
    walk: [
        sys_vendor,
        product_name,
        product_version,
        product_family,
        board_vendor,
        board_name,
        bios_vendor,
        bios_version,
        bios_date,
        product_serial,
        product_uuid,
        board_serial,
        chassis_serial,
    ],
    skip: [chassis_type, boot_mode, secure_boot]
});

walk!(PciDevice {
    walk: [sysfs_path, vendor_name, device_name, modalias],
    skip: [
        id,
        id_basis,
        address,
        vendor_id,
        device_id,
        class,
        class_name,
        subsystem_vendor_id,
        subsystem_device_id,
        revision,
        driver,
        kernel_modules,
        gpu,
    ]
});

walk!(UsbDevice {
    walk: [sysfs_path, manufacturer, product, serial, interfaces],
    skip: [
        id,
        id_basis,
        bus_port,
        vendor_id,
        product_id,
        class,
        class_name,
        speed_mbps,
        usb_version,
        removable,
        is_root_hub,
    ]
});

walk!(UsbInterface {
    walk: [modalias],
    skip: [
        name,
        class,
        subclass,
        protocol,
        class_name,
        driver,
        kernel_modules
    ]
});

walk!(NetworkInterface {
    walk: [sysfs_path, name, mac_address],
    skip: [
        id, id_basis, kind, wireless, is_virtual, link_type, driver, operstate, parent
    ]
});

walk!(AudioCard {
    walk: [sysfs_path, card_id, name, long_name],
    skip: [id, id_basis, index, alsa_driver, driver, parent]
});

walk!(StorageController {
    walk: [sysfs_path],
    skip: [id, id_basis, kind, driver, parent]
});

walk!(BlockDevice {
    walk: [sysfs_path, name, model, vendor, serial, wwid],
    skip: [
        id, id_basis, transport, removable, rotational, size_bytes, driver, parent
    ]
});

walk!(InputDevice {
    walk: [sysfs_path, name, phys, uniq],
    skip: [
        id, id_basis, bus, vendor_id, product_id, version, kinds, parent
    ]
});

walk!(BluetoothAdapter {
    walk: [sysfs_path, name, address],
    skip: [id, id_basis, driver, rfkill, parent]
});

walk!(Camera {
    walk: [sysfs_path, node, name],
    skip: [id, id_basis, index, capture, driver, parent]
});

walk!(ThunderboltDevice {
    walk: [sysfs_path, name, vendor_name, device_name, unique_id],
    skip: [
        id,
        id_basis,
        vendor_id,
        device_id,
        authorised,
        is_host,
        security_level
    ]
});

walk!(Firmware {
    walk: [devices],
    skip: [fwupd]
});

walk!(FirmwareDevice {
    walk: [name, vendor, version, serial, pending_updates],
    skip: [fwupd_device_id, plugin, flags, updatable, update_state]
});

walk!(FirmwareRelease {
    walk: [version],
    skip: [urgency, remote]
});

walk!(KernelEvidence {
    walk: [firmware_failures, probe_failures],
    skip: [available]
});

walk!(FirmwareLoadFailure {
    walk: [device, firmware],
    skip: [driver, error]
});

walk!(ProbeFailure {
    walk: [device],
    skip: [driver, error]
});

/// Finds sensitive values inside other text.
#[derive(Debug, Default)]
struct Scrubber {
    /// Lower-cased values to search for, longest first.
    needles: Vec<String>,
}

impl Scrubber {
    /// Remember a sensitive value, and the form of it without `:` and `-`
    /// (a MAC address or UUID is often written without its separators).
    fn add(&mut self, value: &str) {
        let lower = value.to_ascii_lowercase();
        let compact: String = lower
            .chars()
            .filter(|ch| !matches!(ch, ':' | '-'))
            .collect();
        for needle in [compact, lower] {
            if needle.chars().count() >= MIN_SCRUB_CHARS && !self.needles.contains(&needle) {
                self.needles.push(needle);
            }
        }
    }

    fn finish(&mut self) {
        self.needles
            .sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    }

    /// Replace every occurrence of a remembered value, in any letter case.
    fn scrub(&self, text: &mut String) {
        for needle in &self.needles {
            let mut from = 0_usize;
            loop {
                // ASCII lower-casing keeps every byte offset, so offsets found
                // in the lower-cased copy are valid in the original.
                let lower = text.to_ascii_lowercase();
                let Some(found) = lower
                    .get(from..)
                    .and_then(|rest| rest.find(needle.as_str()))
                else {
                    break;
                };
                let start = from + found;
                let end = start + needle.len();
                if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                    break;
                }
                text.replace_range(start..end, REDACTION_MARK);
                from = start + REDACTION_MARK.len();
            }
        }
    }
}

impl Inventory {
    /// A copy with every sensitive value removed, from its own field and from
    /// any other text it occurs in. See the module documentation.
    pub fn redacted(&self) -> Inventory {
        let mut copy = self.clone();
        let mut scrubber = Scrubber::default();
        copy.walk(&mut |field: Field<'_>| {
            if let Field::Secret(secret) = field
                && let Some(value) = secret.value()
            {
                scrubber.add(value);
            }
        });
        scrubber.finish();
        copy.walk(&mut |field: Field<'_>| match field {
            Field::Secret(secret) => secret.redact(),
            Field::Text(text) => scrubber.scrub(text),
        });
        copy.redacted = true;
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrubbed(secrets: &[&str], text: &str) -> String {
        let mut scrubber = Scrubber::default();
        for secret in secrets {
            scrubber.add(secret);
        }
        scrubber.finish();
        let mut text = text.to_owned();
        scrubber.scrub(&mut text);
        text
    }

    #[test]
    fn scrub_removes_a_value_in_any_case_and_without_separators() {
        let mac = ["00:E0:4c:68:00:01"];
        assert_eq!(
            scrubbed(&mac, "/sys/devices/x/net/enx00e04c680001"),
            "/sys/devices/x/net/enx<redacted>"
        );
        assert_eq!(
            scrubbed(&mac, "addr 00:e0:4C:68:00:01!"),
            "addr <redacted>!"
        );
        assert_eq!(scrubbed(&mac, "unrelated"), "unrelated");
        assert_eq!(
            scrubbed(&["SERIAL99"], "serial99 and SERIAL99 and Serial99"),
            "<redacted> and <redacted> and <redacted>"
        );
    }

    #[test]
    fn scrub_leaves_short_values_alone() {
        assert_eq!(scrubbed(&["0001", "1"], "usb1 at 0001"), "usb1 at 0001");
    }

    #[test]
    fn scrub_ends_when_a_value_looks_like_the_mark() {
        // A hostile serial number chosen to match the replacement text.
        assert_eq!(scrubbed(&["redacted"], "a redacted b"), "a <redacted> b");
        assert_eq!(scrubbed(&["<redacted>"], "x<redacted>y"), "x<redacted>y");
        // Overlapping values: the search still ends.
        assert_eq!(
            scrubbed(&["dacted", "redact"], "redacted"),
            "re<<redacted>ed>"
        );
    }

    #[test]
    fn scrub_handles_non_ascii_text() {
        assert_eq!(
            scrubbed(
                &["s\u{00e9}rie-123"],
                "caf\u{00e9} S\u{00e9}RIE-123 \u{2026}"
            ),
            "caf\u{00e9} <redacted> \u{2026}"
        );
        assert_eq!(
            scrubbed(&["abcdef"], "\u{00e9}abcdef\u{00e9}"),
            "\u{00e9}<redacted>\u{00e9}"
        );
    }
}
