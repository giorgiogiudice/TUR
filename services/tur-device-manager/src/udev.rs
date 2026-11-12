// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Parser for the output of `udevadm info --export-db`.
//!
//! The udev database supplies what sysfs alone does not: names from the
//! hardware database, the classification of input devices, and V4L2
//! capabilities. It is optional. If it cannot be read the inventory is built
//! from sysfs alone and says so.
//!
//! Only `P:` (device path) and `E:` (property) lines are used. The database
//! is bounded in records, properties per record and line length.

use std::collections::BTreeMap;

use crate::text::{MAX_PATH_CHARS, clean, clean_to};

/// The most device records kept.
pub const MAX_RECORDS: usize = 20_000;

/// The most properties kept for one device.
pub const MAX_PROPERTIES: usize = 256;

/// The longest line considered, in bytes. Longer lines are ignored.
pub const MAX_LINE_BYTES: usize = 8192;

/// The udev database: properties by device path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UdevDb {
    records: BTreeMap<String, BTreeMap<String, String>>,
}

fn is_property_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

impl UdevDb {
    /// Parse the database. Lines that are not understood are skipped; nothing
    /// in the input is an error.
    pub fn parse(raw: &[u8]) -> Self {
        let mut records: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut current: Option<String> = None;
        for line in raw.split(|byte| *byte == b'\n') {
            if line.len() > MAX_LINE_BYTES {
                continue;
            }
            if line.is_empty() {
                current = None;
            } else if let Some(path) = line.strip_prefix(b"P: ") {
                let path = clean_to(path, MAX_PATH_CHARS);
                current = None;
                if path.starts_with('/')
                    && (records.len() < MAX_RECORDS || records.contains_key(&path))
                {
                    records.entry(path.clone()).or_default();
                    current = Some(path);
                }
            } else if let Some(property) = line.strip_prefix(b"E: ") {
                let Some(path) = current.as_ref() else {
                    continue;
                };
                let Some(record) = records.get_mut(path) else {
                    continue;
                };
                let Some(split) = property.iter().position(|byte| *byte == b'=') else {
                    continue;
                };
                let (key, value) = property.split_at(split);
                let key = String::from_utf8_lossy(key);
                if !is_property_key(&key) {
                    continue;
                }
                if record.len() >= MAX_PROPERTIES && !record.contains_key(&*key) {
                    continue;
                }
                // `value` starts with the `=`.
                let value = clean(value.get(1..).unwrap_or_default());
                record.insert(key.into_owned(), value);
            }
        }
        Self { records }
    }

    /// The number of device records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the database has no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// A property of the device at a sysfs path such as
    /// `/sys/devices/pci0000:00/0000:00:02.0`. Empty values are `None`.
    pub fn property(&self, sysfs_path: &str, key: &str) -> Option<&str> {
        let device_path = sysfs_path.strip_prefix("/sys").unwrap_or(sysfs_path);
        self.records
            .get(device_path)
            .and_then(|record| record.get(key))
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    const SAMPLE: &str = "\
P: /devices/pci0000:00/0000:00:03.0
M: 0000:00:03.0
R: 0
U: pci
V: virtio-pci
E: DEVPATH=/devices/pci0000:00/0000:00:03.0
E: SUBSYSTEM=pci
E: ID_VENDOR_FROM_DATABASE=Red Hat, Inc.
E: ID_MODEL_FROM_DATABASE=Virtio network device
E: EMPTY=

P: /devices/pci0000:00/0000:00:03.0/virtio1
E: SUBSYSTEM=virtio
E: EQUALS=a=b=c
";

    #[test]
    fn parses_records_and_properties() {
        let db = UdevDb::parse(SAMPLE.as_bytes());
        assert_eq!(db.len(), 2);
        assert_eq!(
            db.property(
                "/sys/devices/pci0000:00/0000:00:03.0",
                "ID_VENDOR_FROM_DATABASE"
            ),
            Some("Red Hat, Inc.")
        );
        assert_eq!(
            db.property("/sys/devices/pci0000:00/0000:00:03.0/virtio1", "SUBSYSTEM"),
            Some("virtio")
        );
        assert_eq!(
            db.property("/sys/devices/pci0000:00/0000:00:03.0/virtio1", "EQUALS"),
            Some("a=b=c")
        );
        // A property belongs to its own record only.
        assert_eq!(
            db.property(
                "/sys/devices/pci0000:00/0000:00:03.0/virtio1",
                "ID_VENDOR_FROM_DATABASE"
            ),
            None
        );
        assert_eq!(
            db.property("/sys/devices/pci0000:00/0000:00:03.0", "EMPTY"),
            None
        );
        assert_eq!(db.property("/sys/devices/none", "SUBSYSTEM"), None);
    }

    #[test]
    fn tolerates_garbage() {
        assert!(UdevDb::parse(b"").is_empty());
        assert!(
            UdevDb::parse(b"\xff\xfe\x00\n\n\nE: ORPHAN=1\nP: relative/path\nE: X=1\n").is_empty()
        );

        let db = UdevDb::parse(b"P: /devices/a\nE: no-equals\nE: bad key=1\nE: lower=1\nE: =1\nE: OK=\x1b[2Jfine\xff\n");
        assert_eq!(db.len(), 1);
        assert_eq!(db.property("/sys/devices/a", "OK"), Some("[2Jfine\u{FFFD}"));
        assert_eq!(db.property("/sys/devices/a", "lower"), None);
    }

    #[test]
    fn bounds_records_properties_and_lines() {
        let mut input = String::new();
        for index in 0..(MAX_RECORDS + 50) {
            write!(input, "P: /devices/d{index}\nE: N=1\n\n").unwrap();
        }
        let db = UdevDb::parse(input.as_bytes());
        assert_eq!(db.len(), MAX_RECORDS);
        assert_eq!(db.property("/sys/devices/d0", "N"), Some("1"));
        assert_eq!(
            db.property(&format!("/sys/devices/d{}", MAX_RECORDS + 10), "N"),
            None
        );

        let mut input = String::from("P: /devices/many\n");
        for index in 0..(MAX_PROPERTIES + 50) {
            writeln!(input, "E: K{index}=v").unwrap();
        }
        writeln!(input, "E: LONG={}", "x".repeat(MAX_LINE_BYTES)).unwrap();
        writeln!(input, "E: K0={}", "y".repeat(1000)).unwrap();
        let db = UdevDb::parse(input.as_bytes());
        assert_eq!(db.property("/sys/devices/many", "K1"), Some("v"));
        assert_eq!(
            db.property("/sys/devices/many", &format!("K{}", MAX_PROPERTIES + 10)),
            None
        );
        assert_eq!(db.property("/sys/devices/many", "LONG"), None);
        // An existing key can still be updated, and its value is bounded.
        let updated = db.property("/sys/devices/many", "K0").unwrap();
        assert_eq!(updated.chars().count(), crate::text::MAX_TEXT_CHARS);
    }
}
