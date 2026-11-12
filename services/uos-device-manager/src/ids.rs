// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Stable device identifiers.
//!
//! Where the hardware carries an identifier of its own (a USB serial number, a
//! permanent MAC address, a disk WWN, a Thunderbolt UUID), the device
//! identifier is derived from it, so the same device is recognised after it is
//! unplugged, moved to another port or re-enumerated in a different order.
//!
//! The hardware identifier itself is personal data, so it never appears in the
//! device identifier. It is hashed with SHA-256 together with the machine
//! identifier (`/etc/machine-id`) and a domain label, and the first 64 bits are
//! used. The result is stable on one installation, differs between
//! installations, and cannot be turned back into the serial number without the
//! machine identifier. If the machine identifier cannot be read, no hash is
//! made and the identifier falls back to the device's position.
//!
//! Where the hardware gives nothing, the identifier is the device's position
//! (PCI address, USB port path), which is stable only while the device stays
//! where it is and the bus layout does not change.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::model::IdBasis;

/// Makes the hashed part of identifiers.
#[derive(Debug, Clone)]
pub struct IdMaker {
    salt: Option<String>,
}

impl IdMaker {
    /// Create a maker from the content of `/etc/machine-id`. Anything other
    /// than 32 hexadecimal digits is treated as no machine identifier.
    pub fn new(machine_id: Option<&str>) -> Self {
        let salt = machine_id
            .map(str::trim)
            .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_ascii_lowercase);
        Self { salt }
    }

    /// Whether hardware identifiers can be hashed.
    pub fn can_hash(&self) -> bool {
        self.salt.is_some()
    }

    /// Sixteen hexadecimal digits that stand for `value` within `domain` on
    /// this machine, or `None` without a machine identifier.
    pub fn digest(&self, domain: &str, value: &str) -> Option<String> {
        let salt = self.salt.as_deref()?;
        let mut hasher = Sha256::new();
        hasher.update(salt.as_bytes());
        hasher.update([0_u8]);
        hasher.update(domain.as_bytes());
        hasher.update([0_u8]);
        hasher.update(value.as_bytes());
        let digest = hasher.finalize();
        let mut out = String::with_capacity(16);
        for byte in digest.iter().take(8) {
            let _ = write!(out, "{byte:02x}");
        }
        Some(out)
    }
}

/// The identifiers a device could have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// An identifier derived from a hardware identifier, if the device has one.
    pub hardware: Option<String>,
    /// The identifier to use otherwise.
    pub fallback: String,
    /// What the fallback is derived from.
    pub fallback_basis: IdBasis,
}

/// Choose an identifier for each device of one category so that every
/// identifier is unique.
///
/// A hardware-derived identifier is used only if no other device in the list
/// has the same one: cheap devices often share one serial number, and two of
/// them must not collapse into one identifier. Any identifier still repeated
/// after that gets a `~2`, `~3`, ... suffix.
pub fn assign(candidates: &[Candidate]) -> Vec<(String, IdBasis)> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for candidate in candidates {
        if let Some(hardware) = &candidate.hardware {
            *counts.entry(hardware.as_str()).or_insert(0) += 1;
        }
    }
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut assigned = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let (base, basis) = match &candidate.hardware {
            Some(hardware) if counts.get(hardware.as_str()).copied() == Some(1) => {
                (hardware.clone(), IdBasis::HardwareIdentifier)
            }
            _ => (candidate.fallback.clone(), candidate.fallback_basis),
        };
        let mut id = base.clone();
        let mut suffix = 2_u32;
        while used.contains(&id) {
            id = format!("{base}~{suffix}");
            suffix = suffix.saturating_add(1);
        }
        used.insert(id.clone());
        assigned.push((id, basis));
    }
    assigned
}

#[cfg(test)]
mod tests {
    use super::*;

    const MACHINE_ID: &str = "0123456789abcdef0123456789abcdef\n";

    fn hex<'a>(bytes: impl Iterator<Item = &'a u8>) -> String {
        let mut out = String::new();
        for byte in bytes {
            write!(out, "{byte:02x}").unwrap();
        }
        out
    }

    fn candidate(hardware: Option<&str>, fallback: &str) -> Candidate {
        Candidate {
            hardware: hardware.map(str::to_owned),
            fallback: fallback.to_owned(),
            fallback_basis: IdBasis::Topology,
        }
    }

    #[test]
    fn digest_is_deterministic_and_bounded() {
        let maker = IdMaker::new(Some(MACHINE_ID));
        let first = maker.digest("usb", "046d:c52b:SERIAL").unwrap();
        let second = maker.digest("usb", "046d:c52b:SERIAL").unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 16);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn digest_matches_sha256_of_the_documented_input() {
        // Pins the construction: SHA-256 over the machine identifier, NUL, the
        // domain, NUL and the value, cut to the first eight bytes.
        let maker = IdMaker::new(Some(MACHINE_ID));
        let mut hasher = Sha256::new();
        hasher.update(b"0123456789abcdef0123456789abcdef\0usb\0value");
        let expected = hex(hasher.finalize().iter().take(8));
        assert_eq!(maker.digest("usb", "value").unwrap(), expected);
    }

    #[test]
    fn sha256_implementation_matches_a_published_vector() {
        // FIPS 180-2, appendix B.1: SHA-256("abc").
        let digest = hex(Sha256::digest(b"abc").iter());
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn digest_depends_on_machine_domain_and_value() {
        let maker = IdMaker::new(Some(MACHINE_ID));
        let other = IdMaker::new(Some("fedcba9876543210fedcba9876543210"));
        let base = maker.digest("usb", "A").unwrap();
        assert_ne!(base, other.digest("usb", "A").unwrap());
        assert_ne!(base, maker.digest("net", "A").unwrap());
        assert_ne!(base, maker.digest("usb", "B").unwrap());
        // The separator keeps ("ab", "c") apart from ("a", "bc").
        assert_ne!(maker.digest("ab", "c"), maker.digest("a", "bc"));
    }

    #[test]
    fn digest_does_not_contain_the_value() {
        let maker = IdMaker::new(Some(MACHINE_ID));
        let digest = maker.digest("usb", "deadbeefdeadbeef").unwrap();
        assert!(!digest.contains("deadbeef"));
    }

    #[test]
    fn no_machine_id_means_no_hash() {
        for bad in [None, Some(""), Some("short"), Some("zz23456789abcdef0123456789abcdef")] {
            let maker = IdMaker::new(bad);
            assert!(!maker.can_hash());
            assert_eq!(maker.digest("usb", "A"), None);
        }
        assert!(IdMaker::new(Some("0123456789ABCDEF0123456789ABCDEF")).can_hash());
    }

    #[test]
    fn assign_prefers_a_unique_hardware_identifier() {
        let assigned = assign(&[
            candidate(Some("usb-s1"), "usb-1-1"),
            candidate(None, "usb-1-2"),
        ]);
        assert_eq!(
            assigned,
            [
                ("usb-s1".to_owned(), IdBasis::HardwareIdentifier),
                ("usb-1-2".to_owned(), IdBasis::Topology),
            ]
        );
    }

    #[test]
    fn assign_falls_back_when_two_devices_share_a_serial() {
        let assigned = assign(&[
            candidate(Some("usb-same"), "usb-1-1"),
            candidate(Some("usb-same"), "usb-1-2"),
            candidate(Some("usb-other"), "usb-1-3"),
        ]);
        assert_eq!(
            assigned,
            [
                ("usb-1-1".to_owned(), IdBasis::Topology),
                ("usb-1-2".to_owned(), IdBasis::Topology),
                ("usb-other".to_owned(), IdBasis::HardwareIdentifier),
            ]
        );
    }

    #[test]
    fn assign_never_repeats_an_identifier() {
        let assigned = assign(&[
            candidate(None, "x"),
            candidate(None, "x"),
            candidate(None, "x"),
            candidate(Some("x~2"), "y"),
        ]);
        let ids: BTreeSet<&str> = assigned.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids.len(), 4);
        assert_eq!(assigned[0].0, "x");
        assert_eq!(assigned[1].0, "x~2");
        assert_eq!(assigned[2].0, "x~3");
    }
}
