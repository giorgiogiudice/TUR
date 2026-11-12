// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Driver and firmware failures in the kernel log.
//!
//! sysfs shows that a device has no driver, but not why. The kernel log does:
//! it records a driver that refused a device and a firmware file that could
//! not be loaded. The log is read only if the caller is allowed to read the
//! system journal; otherwise the inventory says the evidence is unavailable.
//!
//! Only four message forms, all written by the kernel's driver core and
//! firmware loader, are recognised. Nothing else in the log is kept, so
//! unrelated lines (which may hold addresses or names) do not enter the
//! inventory.

use crate::model::{FirmwareLoadFailure, ProbeFailure};
use crate::text::{clean_to, kernel_name};

/// The longest log line considered, in bytes. Longer lines are cut.
pub const MAX_LINE_BYTES: usize = 1024;

/// The most failures of each kind kept.
pub const MAX_FAILURES: usize = 64;

/// The longest firmware file name kept, in characters.
pub const MAX_FIRMWARE_NAME_CHARS: usize = 128;

const DIRECT_LOAD: &str = "Direct firmware load for ";
const FAILED_WITH: &str = " failed with error ";
const PROBE_WITH_DRIVER: &str = "probe with driver ";
const PROBE_OF: &str = ": probe of ";
const FAILED_TO_LOAD: &str = "firmware: failed to load ";

/// Strip a leading `[   12.345678] ` timestamp, if there is one.
fn without_timestamp(line: &str) -> &str {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('[') {
        return trimmed;
    }
    match trimmed.split_once("] ") {
        Some((stamp, rest))
            if stamp
                .bytes()
                .skip(1)
                .all(|b| b.is_ascii_digit() || b == b'.' || b == b' ') =>
        {
            rest
        }
        _ => trimmed,
    }
}

/// Split the `dev_printk` prefix `"<driver> <device>"` into its parts. With a
/// single word, the word is the device.
fn subject(prefix: &str) -> (Option<String>, Option<String>) {
    let prefix = prefix.trim().trim_end_matches(':');
    let mut words = prefix.split_ascii_whitespace().rev();
    let device = words.next().and_then(kernel_name);
    let driver = words.next().and_then(kernel_name);
    (driver, device)
}

fn error_number(text: &str) -> Option<i32> {
    let token: String = text
        .trim()
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect();
    token.parse().ok()
}

fn firmware_name(text: &str) -> Option<String> {
    let name = clean_to(text.as_bytes(), MAX_FIRMWARE_NAME_CHARS);
    // Firmware names are paths below the firmware directory. Some include a
    // board name, so spaces and commas occur. A name cut by the length bound
    // ends in an ellipsis and is rejected here: it would not name a file.
    let plain = !name.is_empty()
        && name.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'.' | b'_' | b'/' | b'+' | b'-' | b':' | b',' | b'@' | b' '
                )
        });
    plain.then_some(name)
}

fn firmware_failure(line: &str) -> Option<FirmwareLoadFailure> {
    if let Some((prefix, rest)) = line.split_once(DIRECT_LOAD) {
        let (name, error) = rest.split_once(FAILED_WITH)?;
        let (driver, device) = subject(prefix);
        return Some(FirmwareLoadFailure {
            driver,
            device,
            firmware: firmware_name(name)?,
            error: error_number(error),
        });
    }
    let (prefix, rest) = line.split_once(FAILED_TO_LOAD)?;
    let (name, error) = rest.split_once(" (")?;
    let (driver, device) = subject(prefix);
    Some(FirmwareLoadFailure {
        driver,
        device,
        firmware: firmware_name(name)?,
        error: error_number(error),
    })
}

fn probe_failure(line: &str) -> Option<ProbeFailure> {
    // Linux 6.10 and later: "<bus> <device>: probe with driver <driver>
    // failed with error <n>".
    if let Some((prefix, rest)) = line.split_once(PROBE_WITH_DRIVER) {
        let (driver, error) = rest.split_once(FAILED_WITH)?;
        let (_, device) = subject(prefix);
        return Some(ProbeFailure {
            driver: kernel_name(driver),
            device: device?,
            error: error_number(error),
        });
    }
    // Earlier kernels: "<driver>: probe of <device> failed with error <n>".
    let (driver, rest) = line.split_once(PROBE_OF)?;
    let (device, error) = rest.split_once(FAILED_WITH)?;
    Some(ProbeFailure {
        driver: driver
            .split_ascii_whitespace()
            .next_back()
            .and_then(kernel_name),
        device: kernel_name(device)?,
        error: error_number(error),
    })
}

/// Extract firmware-load and driver-probe failures from kernel messages, one
/// message per line, with or without timestamps. Repeated messages are
/// reported once.
pub fn parse(raw: &[u8]) -> (Vec<FirmwareLoadFailure>, Vec<ProbeFailure>) {
    let mut firmware: Vec<FirmwareLoadFailure> = Vec::new();
    let mut probes: Vec<ProbeFailure> = Vec::new();
    for line in raw.split(|byte| *byte == b'\n') {
        let line = line.get(..MAX_LINE_BYTES).unwrap_or(line);
        let line = String::from_utf8_lossy(line);
        let line = without_timestamp(&line);
        if let Some(failure) = firmware_failure(line) {
            if firmware.len() < MAX_FAILURES && !firmware.contains(&failure) {
                firmware.push(failure);
            }
        } else if let Some(failure) = probe_failure(line)
            && probes.len() < MAX_FAILURES
            && !probes.contains(&failure)
        {
            probes.push(failure);
        }
    }
    (firmware, probes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_a_direct_firmware_load_failure() {
        let (firmware, probes) = parse(
            b"rtw_8822ce 0000:03:00.0: Direct firmware load for rtw88/rtw8822c_fw.bin failed with error -2\n",
        );
        assert_eq!(
            firmware,
            [FirmwareLoadFailure {
                driver: Some("rtw_8822ce".to_owned()),
                device: Some("0000:03:00.0".to_owned()),
                firmware: "rtw88/rtw8822c_fw.bin".to_owned(),
                error: Some(-2),
            }]
        );
        assert!(probes.is_empty());
    }

    #[test]
    fn recognises_timestamps_and_single_word_subjects() {
        let (firmware, _) = parse(
            b"[   12.345678] bluetooth hci0: Direct firmware load for intel/ibt-19-0-4.sfi failed with error -2\n\
              [   13.000001] hci0: firmware: failed to load intel/ibt-19-0-4.ddc (-2)\n",
        );
        assert_eq!(firmware.len(), 2);
        assert_eq!(firmware[0].driver.as_deref(), Some("bluetooth"));
        assert_eq!(firmware[0].device.as_deref(), Some("hci0"));
        assert_eq!(firmware[1].driver, None);
        assert_eq!(firmware[1].device.as_deref(), Some("hci0"));
        assert_eq!(firmware[1].firmware, "intel/ibt-19-0-4.ddc");
        assert_eq!(firmware[1].error, Some(-2));
    }

    #[test]
    fn recognises_both_probe_failure_forms() {
        let (_, probes) = parse(
            b"pci 0000:04:00.0: probe with driver nouveau failed with error -12\n\
              iwlwifi: probe of 0000:00:14.3 failed with error -110\n",
        );
        assert_eq!(
            probes,
            [
                ProbeFailure {
                    driver: Some("nouveau".to_owned()),
                    device: "0000:04:00.0".to_owned(),
                    error: Some(-12),
                },
                ProbeFailure {
                    driver: Some("iwlwifi".to_owned()),
                    device: "0000:00:14.3".to_owned(),
                    error: Some(-110),
                },
            ]
        );
    }

    #[test]
    fn reports_repeats_once_and_bounds_the_lists() {
        let line = b"iwlwifi 0000:00:14.3: Direct firmware load for a.ucode failed with error -2\n";
        let (firmware, _) = parse(&line.repeat(50));
        assert_eq!(firmware.len(), 1);

        let mut log = String::new();
        for index in 0..500 {
            log.push_str("drv 0000:00:01.0: Direct firmware load for file");
            log.push_str(&index.to_string());
            log.push_str(".bin failed with error -2\n");
            log.push_str("drv: probe of dev");
            log.push_str(&index.to_string());
            log.push_str(" failed with error -5\n");
        }
        let (firmware, probes) = parse(log.as_bytes());
        assert_eq!(firmware.len(), MAX_FAILURES);
        assert_eq!(probes.len(), MAX_FAILURES);
    }

    #[test]
    fn ignores_everything_else() {
        let (firmware, probes) = parse(
            b"usb 1-2: new high-speed USB device number 3 using xhci_hcd\n\
              usb 1-2: SerialNumber: 4C530001230915112345\n\
              e1000e 0000:00:1f.6 enp0s31f6: NIC Link is Up 1000 Mbps\n\
              \n",
        );
        assert!(firmware.is_empty());
        assert!(probes.is_empty());
    }

    #[test]
    fn survives_hostile_lines() {
        let mut log = Vec::new();
        log.extend_from_slice(b"\xff\xfe Direct firmware load for \x1b[31m failed with error x\n");
        log.extend_from_slice(b"Direct firmware load for  failed with error -2\n");
        log.extend_from_slice(b"Direct firmware load for name\n");
        log.extend_from_slice(b"a b c d: probe with driver ../../x failed with error -1\n");
        log.extend_from_slice(b": probe of  failed with error \n");
        log.extend_from_slice(b"[ not a stamp] x: probe of y failed with error -99999999999999\n");
        log.extend_from_slice(&b"A".repeat(100_000));
        log.extend_from_slice(b" Direct firmware load for late.bin failed with error -2\n");
        log.extend_from_slice(b"d 0: Direct firmware load for ");
        log.extend_from_slice(&b"f".repeat(900));
        log.extend_from_slice(b" failed with error -2\n");
        log.extend_from_slice(b"d 0: Direct firmware load for ");
        log.extend_from_slice(&b"g".repeat(100));
        log.extend_from_slice(b" failed with error -2\n");
        let (firmware, probes) = parse(&log);

        // An escape sequence is not a firmware name; an empty name is not one;
        // a message pushed beyond the line bound is not seen; a name longer
        // than the bound is not a file name. Only the last line is kept.
        assert_eq!(firmware.len(), 1);
        assert_eq!(firmware[0].firmware, "g".repeat(100));
        assert_eq!(firmware[0].device.as_deref(), Some("0"));

        // A driver name with a path in it is dropped, the device kept.
        assert_eq!(probes.len(), 2);
        assert_eq!(probes[0].driver, None);
        assert_eq!(probes[0].device, "d");
        // An out-of-range error number is dropped, the failure kept.
        assert_eq!(probes[1].device, "y");
        assert_eq!(probes[1].error, None);
    }
}
