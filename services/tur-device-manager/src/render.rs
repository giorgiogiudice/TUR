// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Plain-text output for `tur-device`.
//!
//! Every string printed here entered the model through [`crate::text`], so it
//! holds no control characters and is bounded in length. The functions add
//! only fixed text and line breaks.

use std::fmt::Write as _;

use serde::Serialize;

use crate::model::{Inventory, Sensitive};
use crate::problems::Finding;

/// The snake-case name of an enumeration value, as in the JSON output.
fn label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|json| json.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn or_dash(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "-",
    }
}

fn secret(value: &Sensitive) -> &str {
    match value {
        Sensitive::Absent => "-",
        Sensitive::Present(text) => text,
        Sensitive::Redacted => "<redacted>",
    }
}

fn id_pair(first: Option<&str>, second: Option<&str>) -> String {
    format!("{}:{}", first.unwrap_or("----"), second.unwrap_or("----"))
}

fn heading(out: &mut String, title: &str, count: usize) {
    let _ = writeln!(out, "\n{title} ({count})");
}

/// The inventory as text for a person to read.
pub fn inventory_text(inventory: &Inventory) -> String {
    let mut out = String::new();
    let platform = &inventory.platform;
    let _ = writeln!(
        out,
        "TUR hardware inventory{}",
        if inventory.redacted {
            " (redacted)"
        } else {
            ""
        }
    );
    let _ = writeln!(out, "\nPlatform");
    let _ = writeln!(
        out,
        "  System:      {} {} {}",
        or_dash(platform.sys_vendor.as_deref()),
        or_dash(platform.product_name.as_deref()),
        platform.product_version.as_deref().unwrap_or_default()
    );
    let _ = writeln!(
        out,
        "  Board:       {} {}",
        or_dash(platform.board_vendor.as_deref()),
        or_dash(platform.board_name.as_deref())
    );
    let _ = writeln!(
        out,
        "  Firmware:    {} {} ({})",
        or_dash(platform.bios_vendor.as_deref()),
        or_dash(platform.bios_version.as_deref()),
        or_dash(platform.bios_date.as_deref())
    );
    let _ = writeln!(out, "  Serial:      {}", secret(&platform.product_serial));
    let _ = writeln!(out, "  Boot mode:   {}", label(&platform.boot_mode));
    let _ = writeln!(out, "  Secure Boot: {}", label(&platform.secure_boot));

    heading(&mut out, "PCI devices", inventory.pci.len());
    for device in &inventory.pci {
        let _ = writeln!(
            out,
            "  {}  [{}]  class {}  {}  driver: {}",
            device.address,
            id_pair(device.vendor_id.as_deref(), device.device_id.as_deref()),
            or_dash(device.class.as_deref()),
            device
                .device_name
                .as_deref()
                .or(device.class_name.as_deref())
                .unwrap_or("-"),
            device.driver.as_deref().unwrap_or("none")
        );
        let _ = writeln!(out, "    id: {}", device.id);
        if let Some(gpu) = &device.gpu {
            for card in &gpu.drm_cards {
                for connector in &card.connectors {
                    let _ = writeln!(
                        out,
                        "    {} {}: {}",
                        card.name,
                        connector.name,
                        label(&connector.status)
                    );
                }
            }
        }
    }

    heading(&mut out, "USB devices", inventory.usb.len());
    for device in &inventory.usb {
        let _ = writeln!(
            out,
            "  {}  [{}]  {} {}  {} Mbit/s  {}",
            device.bus_port,
            id_pair(device.vendor_id.as_deref(), device.product_id.as_deref()),
            or_dash(device.manufacturer.as_deref()),
            or_dash(device.product.as_deref()),
            or_dash(device.speed_mbps.as_deref()),
            label(&device.removable)
        );
        let _ = writeln!(out, "    id: {}", device.id);
        let _ = writeln!(out, "    serial: {}", secret(&device.serial));
        for interface in &device.interfaces {
            let _ = writeln!(
                out,
                "    {}  class {}  {}  driver: {}",
                interface.name,
                or_dash(interface.class.as_deref()),
                or_dash(interface.class_name.as_deref()),
                interface.driver.as_deref().unwrap_or("none")
            );
        }
    }

    heading(&mut out, "Network interfaces", inventory.network.len());
    for interface in &inventory.network {
        let _ = writeln!(
            out,
            "  {}  {}  state: {}  driver: {}  address: {}",
            interface.name,
            label(&interface.kind),
            label(&interface.operstate),
            or_dash(interface.driver.as_deref()),
            secret(&interface.mac_address)
        );
        let _ = writeln!(out, "    id: {}", interface.id);
    }

    heading(&mut out, "Sound cards", inventory.audio.len());
    for card in &inventory.audio {
        let _ = writeln!(
            out,
            "  card{}  {}  {}  driver: {}",
            card.index,
            or_dash(card.card_id.as_deref()),
            or_dash(card.name.as_deref()),
            or_dash(card.driver.as_deref())
        );
        let _ = writeln!(out, "    id: {}", card.id);
    }

    heading(
        &mut out,
        "Storage controllers",
        inventory.storage_controllers.len(),
    );
    for controller in &inventory.storage_controllers {
        let _ = writeln!(
            out,
            "  {}  driver: {}",
            label(&controller.kind),
            controller.driver.as_deref().unwrap_or("none")
        );
        let _ = writeln!(out, "    id: {}", controller.id);
    }

    heading(&mut out, "Block devices", inventory.block.len());
    for device in &inventory.block {
        let _ = writeln!(
            out,
            "  {}  {}  {}  removable: {}  serial: {}",
            device.name,
            label(&device.transport),
            or_dash(device.model.as_deref()),
            yes_no(device.removable),
            secret(&device.serial)
        );
        let _ = writeln!(out, "    id: {}", device.id);
    }

    heading(&mut out, "Input devices", inventory.input.len());
    for device in &inventory.input {
        let kinds: Vec<String> = device.kinds.iter().map(label).collect();
        let _ = writeln!(
            out,
            "  {}  bus: {}  [{}]  {}",
            or_dash(device.name.as_deref()),
            label(&device.bus),
            id_pair(device.vendor_id.as_deref(), device.product_id.as_deref()),
            if kinds.is_empty() {
                "-".to_owned()
            } else {
                kinds.join(", ")
            }
        );
        let _ = writeln!(out, "    id: {}", device.id);
    }

    heading(&mut out, "Bluetooth adapters", inventory.bluetooth.len());
    for adapter in &inventory.bluetooth {
        let blocked = adapter
            .rfkill
            .as_ref()
            .map(|rfkill| rfkill.soft_blocked == Some(true) || rfkill.hard_blocked == Some(true));
        let _ = writeln!(
            out,
            "  {}  driver: {}  blocked: {}",
            adapter.name,
            or_dash(adapter.driver.as_deref()),
            yes_no(blocked)
        );
        let _ = writeln!(out, "    id: {}", adapter.id);
    }

    heading(&mut out, "Video devices", inventory.cameras.len());
    for camera in &inventory.cameras {
        let _ = writeln!(
            out,
            "  {}  {}  capture: {}  driver: {}",
            camera.node,
            or_dash(camera.name.as_deref()),
            yes_no(camera.capture),
            or_dash(camera.driver.as_deref())
        );
        let _ = writeln!(out, "    id: {}", camera.id);
    }

    heading(&mut out, "Thunderbolt devices", inventory.thunderbolt.len());
    for device in &inventory.thunderbolt {
        let _ = writeln!(
            out,
            "  {}  {} {}  {}{}",
            device.name,
            or_dash(device.vendor_name.as_deref()),
            or_dash(device.device_name.as_deref()),
            label(&device.authorised),
            if device.is_host { "  (host)" } else { "" }
        );
        let _ = writeln!(out, "    id: {}", device.id);
    }

    heading(&mut out, "Firmware", inventory.firmware.devices.len());
    let _ = writeln!(out, "  fwupd: {}", label(&inventory.firmware.fwupd));
    for device in &inventory.firmware.devices {
        let _ = writeln!(
            out,
            "  {}  version {}  updatable: {}",
            or_dash(device.name.as_deref()),
            or_dash(device.version.as_deref()),
            yes_no(Some(device.updatable))
        );
        for release in &device.pending_updates {
            let _ = writeln!(
                out,
                "    update available: {} (urgency {})",
                or_dash(release.version.as_deref()),
                or_dash(release.urgency.as_deref())
            );
        }
    }

    if !inventory.source_notes.is_empty() {
        let _ = writeln!(out, "\nSources not used");
        for note in &inventory.source_notes {
            let _ = writeln!(
                out,
                "  {} ({}): {}",
                note.source,
                label(&note.status),
                note.effect
            );
        }
    }
    out
}

/// The findings as text, each in the two layers of specification §6.3.
pub fn problems_text(findings: &[Finding]) -> String {
    let mut out = String::new();
    if findings.is_empty() {
        out.push_str("No hardware problems found.\n");
        return out;
    }
    for (index, finding) in findings.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "{}  [{}]  {}",
            finding.code,
            label(&finding.severity),
            finding.title
        );
        let _ = writeln!(out, "{}", finding.user_message);
        let _ = writeln!(out, "\nAction:\n{}", finding.next_step);
        let _ = writeln!(out, "\nTechnical detail:");
        if let Some(id) = &finding.device_id {
            let _ = writeln!(out, "  device_id: {id}");
        }
        for (key, value) in &finding.technical_detail {
            let _ = writeln!(out, "  {key}: {value}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect;
    use crate::probe::FakeProbe;
    use crate::problems::detect;

    #[test]
    fn an_empty_inventory_still_renders() {
        let inventory = collect(&FakeProbe::new());
        let text = inventory_text(&inventory);
        assert!(text.starts_with("TUR hardware inventory\n"));
        assert!(text.contains("\nPCI devices (0)\n"));
        assert!(text.contains("  Boot mode:   unknown\n"));
        assert!(text.contains("\nSources not used\n"));
        assert!(text.contains("  fwupd (unavailable): "));
        assert_eq!(
            problems_text(&detect(&inventory)),
            "No hardware problems found.\n"
        );
    }

    #[test]
    fn a_finding_renders_in_two_layers() {
        let dir = "/sys/devices/pci0000:00/0000:00:05.0";
        let probe = FakeProbe::new()
            .link(
                "/sys/bus/pci/devices/0000:00:05.0",
                "../../../devices/pci0000:00/0000:00:05.0",
            )
            .file(&format!("{dir}/vendor"), "0x1234\n")
            .file(&format!("{dir}/device"), "0x5678\n")
            .file(&format!("{dir}/class"), "0x078000\n");
        let inventory = collect(&probe);
        let text = problems_text(&detect(&inventory));
        let expected_start = "TUR-DEVICE-DRIVER-001  [warning]  No driver for the device\n\
            No driver is attached to the Communication controller at PCI address 0000:00:05.0.";
        assert!(text.starts_with(expected_start), "{text}");
        assert!(text.contains("\nAction:\nInstall any pending TUR updates"));
        assert!(text.contains("\nTechnical detail:\n  device_id: pci-0000:00:05.0-1234:5678\n"));
        assert!(text.contains("  ids: 1234:5678\n"));

        let listing = inventory_text(&inventory);
        assert!(listing.contains(
            "  0000:00:05.0  [1234:5678]  class 078000  Communication controller  driver: none\n"
        ));
        assert!(listing.contains("    id: pci-0000:00:05.0-1234:5678\n"));
    }

    #[test]
    fn a_redacted_inventory_says_so() {
        let probe = FakeProbe::new()
            .link("/sys/class/dmi/id", "../../devices/virtual/dmi/id")
            .file("/sys/devices/virtual/dmi/id/product_serial", "PF2ABCDE\n");
        let inventory = collect(&probe);
        assert!(inventory_text(&inventory).contains("  Serial:      PF2ABCDE\n"));
        let redacted = inventory_text(&inventory.redacted());
        assert!(redacted.starts_with("TUR hardware inventory (redacted)\n"));
        assert!(redacted.contains("  Serial:      <redacted>\n"));
        assert!(!redacted.contains("PF2ABCDE"));
    }
}
