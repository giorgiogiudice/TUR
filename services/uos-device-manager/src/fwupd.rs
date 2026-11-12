// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Parser for the output of `fwupdmgr get-devices --json`.
//!
//! fwupd is the host's firmware update service (specification §10.5). This
//! module only reads what it reports: the devices whose firmware it knows and
//! the releases it could install. It never asks fwupd to do anything.
//!
//! The document is read as loosely typed JSON, because fwupd has changed the
//! type of some fields between versions (`UpdateState` has been both a number
//! and a string). A field of an unexpected type is treated as absent.

use serde_json::Value;

use crate::model::{FirmwareDevice, FirmwareRelease, Sensitive};
use crate::text::{MAX_NAME_CHARS, clean};

/// The most devices kept.
pub const MAX_DEVICES: usize = 512;

/// The most flags kept for one device, and the most releases examined.
pub const MAX_ITEMS: usize = 64;

/// The release flag fwupd sets on a release newer than the installed version.
const IS_UPGRADE: &str = "is-upgrade";

fn text(object: &Value, key: &str) -> Option<String> {
    let cleaned = clean(object.get(key)?.as_str()?.as_bytes());
    (!cleaned.is_empty()).then_some(cleaned)
}

/// A short lower-case word such as a flag name; anything else is `None`.
fn token(value: &Value) -> Option<String> {
    let word = value.as_str()?;
    let valid = !word.is_empty()
        && word.len() <= MAX_NAME_CHARS
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    valid.then(|| word.to_owned())
}

fn tokens(object: &Value, key: &str) -> Vec<String> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().take(MAX_ITEMS).filter_map(token).collect())
        .unwrap_or_default()
}

fn update_state(object: &Value) -> Option<String> {
    let value = object.get("UpdateState")?;
    if let Some(number) = value.as_u64() {
        // FwupdUpdateState in libfwupd.
        let name = match number {
            0 => "unknown",
            1 => "pending",
            2 => "success",
            3 => "failed",
            4 => "needs-reboot",
            5 => "failed-transient",
            _ => return None,
        };
        return Some(name.to_owned());
    }
    token(value)
}

fn release(value: &Value) -> Option<FirmwareRelease> {
    if !value.is_object() || !tokens(value, "Flags").iter().any(|flag| flag == IS_UPGRADE) {
        return None;
    }
    Some(FirmwareRelease {
        version: text(value, "Version"),
        urgency: text(value, "Urgency"),
        remote: text(value, "RemoteId"),
    })
}

fn device(value: &Value) -> Option<FirmwareDevice> {
    if !value.is_object() {
        return None;
    }
    let flags = tokens(value, "Flags");
    let pending_updates = value
        .get("Releases")
        .and_then(Value::as_array)
        .map(|releases| {
            releases
                .iter()
                .take(MAX_ITEMS)
                .filter_map(release)
                .collect()
        })
        .unwrap_or_default();
    Some(FirmwareDevice {
        fwupd_device_id: value.get("DeviceId").and_then(token),
        name: text(value, "Name"),
        vendor: text(value, "Vendor"),
        plugin: text(value, "Plugin"),
        version: text(value, "Version"),
        serial: Sensitive::from_option(text(value, "Serial")),
        updatable: flags
            .iter()
            .any(|flag| flag == "updatable" || flag == "updatable-hidden"),
        flags,
        update_state: update_state(value),
        pending_updates,
    })
}

/// A label for a JSON error that does not quote the input.
fn error_label(error: &serde_json::Error) -> &'static str {
    match error.classify() {
        serde_json::error::Category::Io => "read error",
        serde_json::error::Category::Syntax => "syntax error",
        serde_json::error::Category::Data => "unexpected data",
        serde_json::error::Category::Eof => "unexpected end",
    }
}

/// Parse the JSON document. An error is a short reason, for the source note.
pub fn parse(raw: &[u8]) -> Result<Vec<FirmwareDevice>, String> {
    let document: Value = serde_json::from_slice(raw)
        .map_err(|error| format!("not JSON: {}", error_label(&error)))?;
    let devices = document
        .get("Devices")
        .and_then(Value::as_array)
        .ok_or_else(|| "no \"Devices\" array".to_owned())?;
    Ok(devices
        .iter()
        .take(MAX_DEVICES)
        .filter_map(device)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "Devices": [
        {
          "Name": "System Firmware",
          "DeviceId": "a45df35ac0e948ee180fe216a5f703f32dda163f",
          "Plugin": "uefi_capsule",
          "Flags": ["internal", "updatable", "require-ac", "supported", "needs-reboot"],
          "Vendor": "Example Computer",
          "Version": "1.20",
          "UpdateState": 1,
          "Releases": [
            { "Version": "1.24", "Urgency": "high", "RemoteId": "lvfs", "Flags": ["trusted-metadata", "is-upgrade"] },
            { "Version": "1.20", "RemoteId": "lvfs", "Flags": ["trusted-metadata"] },
            { "Version": "1.10", "RemoteId": "lvfs", "Flags": ["is-downgrade"] }
          ]
        },
        {
          "Name": "SSD 980",
          "DeviceId": "71b677ca0f1bc2c5b804fa1d59e52064ce589293",
          "Serial": "S64ANX0R123456",
          "Plugin": "nvme",
          "Flags": ["internal", "updatable", "supported"],
          "Version": "2B4QFXO7",
          "UpdateState": "needs-reboot"
        },
        { "Name": "Webcam", "Flags": ["registered"] }
      ]
    }"#;

    #[test]
    fn parses_devices_and_pending_updates() {
        let devices = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(devices.len(), 3);

        let system = &devices[0];
        assert_eq!(system.name.as_deref(), Some("System Firmware"));
        assert_eq!(
            system.fwupd_device_id.as_deref(),
            Some("a45df35ac0e948ee180fe216a5f703f32dda163f")
        );
        assert!(system.updatable);
        assert_eq!(system.update_state.as_deref(), Some("pending"));
        assert_eq!(
            system.pending_updates,
            [FirmwareRelease {
                version: Some("1.24".to_owned()),
                urgency: Some("high".to_owned()),
                remote: Some("lvfs".to_owned()),
            }]
        );

        let disk = &devices[1];
        assert_eq!(disk.serial.value(), Some("S64ANX0R123456"));
        assert_eq!(disk.update_state.as_deref(), Some("needs-reboot"));
        assert!(disk.pending_updates.is_empty());

        let webcam = &devices[2];
        assert!(!webcam.updatable);
        assert_eq!(webcam.fwupd_device_id, None);
        assert_eq!(webcam.serial, Sensitive::Absent);
    }

    #[test]
    fn rejects_what_is_not_the_expected_document() {
        assert_eq!(parse(b""), Err("not JSON: unexpected end".to_owned()));
        assert_eq!(parse(b"{ nope"), Err("not JSON: syntax error".to_owned()));
        assert_eq!(parse(b"[]"), Err("no \"Devices\" array".to_owned()));
        assert_eq!(parse(br#"{"Devices": 3}"#), Err("no \"Devices\" array".to_owned()));
        assert_eq!(parse(br#"{"Devices": []}"#), Ok(Vec::new()));
        // Deep nesting is refused by the JSON parser, not by the stack.
        let deep = "[".repeat(100_000);
        assert!(parse(deep.as_bytes()).is_err());
    }

    #[test]
    fn tolerates_wrong_types_and_hostile_strings() {
        let devices = parse(
            br#"{"Devices": [
                42,
                "text",
                null,
                {
                  "Name": "Evil\u001b[31m\u0000Dock‮",
                  "DeviceId": "../../etc/passwd",
                  "Vendor": 7,
                  "Version": ["1"],
                  "Flags": ["updatable", "Not A Flag", 9, "ok-flag"],
                  "UpdateState": 99,
                  "Releases": [ 1, {"Flags": "is-upgrade"}, {"Flags": ["is-upgrade"], "Version": {}} ]
                }
            ]}"#,
        )
        .unwrap();
        assert_eq!(devices.len(), 1);
        let dock = &devices[0];
        assert_eq!(dock.name.as_deref(), Some("Evil [31m Dock"));
        assert_eq!(dock.fwupd_device_id, None);
        assert_eq!(dock.vendor, None);
        assert_eq!(dock.version, None);
        assert_eq!(dock.flags, ["updatable", "ok-flag"]);
        assert_eq!(dock.update_state, None);
        assert_eq!(
            dock.pending_updates,
            [FirmwareRelease {
                version: None,
                urgency: None,
                remote: None,
            }]
        );
    }

    #[test]
    fn bounds_devices_and_flags() {
        let many: Vec<Value> = (0..(MAX_DEVICES + 100))
            .map(|index| serde_json::json!({ "Name": format!("d{index}") }))
            .collect();
        let flags: Vec<Value> = (0..1000).map(|index| Value::from(format!("f{index}"))).collect();
        let mut devices = many;
        devices.insert(0, serde_json::json!({ "Flags": flags }));
        let document = serde_json::json!({ "Devices": devices });
        let parsed = parse(document.to_string().as_bytes()).unwrap();
        assert_eq!(parsed.len(), MAX_DEVICES);
        assert_eq!(parsed[0].flags.len(), MAX_ITEMS);
    }
}
