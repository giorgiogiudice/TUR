// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Test material: bundles built directly, without `tur-diag`'s redaction,
//! and the corpus of planted personal data that must never reach a filed
//! summary, a log line or an answer.
//!
//! The bundles are real ones (they verify with `tur-diag`'s checks); they
//! are just not redacted by a client, so they test the receiver's own pass
//! alone. `examples/planted_bundle.rs` writes one for the `reports` workflow.

use tur_diag::Profile;
use tur_diag::bundle::{CHECKSUMS_PATH, MANIFEST_PATH, Member, pack, render_checksums, sha256_hex};
use tur_diag::manifest::{
    COLLECTOR_NAME, CollectorInfo, FileEntry, MANIFEST_SCHEMA_VERSION, Manifest, TurIdentity,
    format_utc,
};

/// The creation time of test bundles.
pub const CREATED: u64 = 1_793_000_000;

/// One planted value: what it is, the value itself, and the line it is
/// planted in. The value must not appear anywhere it could leave the
/// receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Planted {
    /// The category, as in the redaction table.
    pub category: &'static str,
    /// The personal value.
    pub value: &'static str,
    /// The text it is planted in.
    pub context: &'static str,
}

/// The corpus. Every category of the server's redaction table is here, in
/// the forms seen in logs, at least once.
pub const PLANTED: &[Planted] = &[
    Planted {
        category: "email",
        value: "alice.smith@example.org",
        context: "Please write to alice.smith@example.org about it.",
    },
    Planted {
        category: "email",
        value: "j.doe+tur@mail.example.net",
        context: "cc j.doe+tur@mail.example.net",
    },
    Planted {
        category: "home path",
        value: "jdoe-private",
        context: "could not open /home/jdoe-private/Documents/taxes-2026.pdf",
    },
    Planted {
        category: "home path",
        value: "ktaylor",
        context: "flatpak: /var/home/ktaylor/.var/app is full",
    },
    Planted {
        category: "home path",
        value: "anna-lena",
        context: "mounted /run/media/anna-lena/BACKUP",
    },
    Planted {
        category: "windows profile",
        value: "Giulia Rossi",
        context: r"wine: C:\Users\Giulia Rossi\AppData\Local\Temp",
    },
    Planted {
        category: "windows profile",
        value: "grossi",
        context: "prefix drive_c/users/grossi/Desktop missing",
    },
    Planted {
        category: "user name",
        value: "mweber",
        context: "pam_unix(login:session): session opened for user mweber(uid=1001) by (uid=0)",
    },
    Planted {
        category: "user name",
        value: "mweber",
        context: "USER=mweber LOGNAME=mweber",
    },
    Planted {
        category: "user name",
        value: "mweber",
        context: "and later mweber's files were gone",
    },
    Planted {
        category: "user name",
        value: "bwayne",
        context: "id: uid=1002(bwayne) gid=1002(bwayne)",
    },
    Planted {
        category: "user name",
        value: "ckent",
        context: "sshd: Accepted publickey for ckent from 81.2.69.160 port 50122",
    },
    Planted {
        category: "user name",
        value: "dprince",
        context: "prompt dprince@studio $ make",
    },
    Planted {
        category: "host name",
        value: "kitchen-pc-7731",
        context: "{\"_HOSTNAME\":\"kitchen-pc-7731\",\"MESSAGE\":\"kitchen-pc-7731 rebooted\",\"PRIORITY\":\"3\"}",
    },
    Planted {
        category: "host name",
        value: "studio-mac-2",
        context: "Static hostname: studio-mac-2",
    },
    Planted {
        category: "host name",
        value: "mybox.local",
        context: "printing to ipp://mybox.local:631/printers/x",
    },
    Planted {
        category: "host name",
        value: "nas-4471.lan",
        context: "smb: nas-4471.lan is not reachable",
    },
    Planted {
        category: "host name",
        value: "lenovo-ys",
        context: "Linux lenovo-ys 6.17.1-300.fc45.x86_64 #1 SMP PREEMPT_DYNAMIC",
    },
    Planted {
        category: "IPv4 address",
        value: "81.2.69.160",
        context: "connection from 81.2.69.160 refused",
    },
    Planted {
        category: "IPv4 address",
        value: "192.168.178.23",
        context: "DHCPACK: lease 192.168.178.23 for 86400 seconds",
    },
    Planted {
        category: "IPv6 address",
        value: "2a02:810d:4bc0:1a2b:3c4d:5e6f:7081:92a3",
        context: "addr 2a02:810d:4bc0:1a2b:3c4d:5e6f:7081:92a3/64 scope global",
    },
    Planted {
        category: "IPv6 address",
        value: "fe80::1c2d:3eff:fe4f:5a6b",
        context: "neighbour fe80::1c2d:3eff:fe4f:5a6b%wlp2s0 reachable",
    },
    Planted {
        category: "MAC address",
        value: "3c:52:82:aa:0b:1f",
        context: "wlp2s0: link/ether 3c:52:82:aa:0b:1f brd ff:ff:ff:ff:ff:ff",
    },
    Planted {
        category: "MAC address",
        value: "3C-52-82-AA-0B-2E",
        context: "bluetooth device 3C-52-82-AA-0B-2E connected",
    },
    Planted {
        category: "MAC address",
        value: "3c52.82aa.0b3f",
        context: "switch port learned 3c52.82aa.0b3f",
    },
    Planted {
        category: "MAC address",
        value: "3c5282aa0b4f",
        context: "hwaddr=3c5282aa0b4f",
    },
    Planted {
        category: "serial number",
        value: "PF2XK9QZ",
        context: "Serial Number: PF2XK9QZ",
    },
    Planted {
        category: "serial number",
        value: "S4EWNX0M987654",
        context: "ID_SERIAL_SHORT=S4EWNX0M987654",
    },
    Planted {
        category: "serial number",
        value: "5CD1234XYZ",
        context: "product_serial: 5CD1234XYZ",
    },
    Planted {
        category: "Wi-Fi name",
        value: "Schmidt Family WLAN",
        context: "wpa_supplicant: SSID=\"Schmidt Family WLAN\" authentication failed",
    },
    Planted {
        category: "Wi-Fi name",
        value: "Schmidt-5G",
        context: "NetworkManager: Activation: starting connection 'Schmidt-5G'",
    },
    Planted {
        category: "Wi-Fi name",
        value: "FRITZ!Box 7590 QX",
        context: "iwconfig: ESSID:\"FRITZ!Box 7590 QX\"",
    },
    Planted {
        category: "UUID",
        value: "6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10",
        context: "luks-6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10 unlocked",
    },
    Planted {
        category: "machine ID",
        value: "4c4c4544004e3510804ab4c04f4e3732",
        context: "_MACHINE_ID 4c4c4544004e3510804ab4c04f4e3732",
    },
    Planted {
        category: "token",
        value: "ghp_R8x2Lm4Pq7Ty9Wv1Zb3Nc5Df6Gh0Jk2Ls4Mq",
        context: "git: using token ghp_R8x2Lm4Pq7Ty9Wv1Zb3Nc5Df6Gh0Jk2Ls4Mq",
    },
    Planted {
        category: "token",
        value: "sk-ant-api03-Xy7Kp2Lm9Qr4St6Uv8Wz0AbCdEf",
        context: "P@x key sk-ant-api03-Xy7Kp2Lm9Qr4St6Uv8Wz0AbCdEf rejected",
    },
    Planted {
        category: "token",
        value: "AKIAQ7XEXAMPLEKEY123",
        context: "aws: AKIAQ7XEXAMPLEKEY123 expired",
    },
    Planted {
        category: "token",
        value: "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJtd2ViZXIifQ.Qm9ndXNTaWduYXR1cmU",
        context: "cookie eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJtd2ViZXIifQ.Qm9ndXNTaWduYXR1cmU",
    },
    Planted {
        category: "token",
        value: "xoxb-2048-Zx9QpLm3Rt",
        context: "slack token xoxb-2048-Zx9QpLm3Rt",
    },
    Planted {
        category: "secret",
        value: "hunter2-Correct",
        context: "mount.cifs: password=hunter2-Correct,vers=3.0",
    },
    Planted {
        category: "secret",
        value: "sesame-open-77",
        context: "wifi psk: sesame-open-77",
    },
    Planted {
        category: "secret",
        value: "Qw3rtyBearerT0ken9",
        context: "Authorization: Bearer Qw3rtyBearerT0ken9",
    },
    Planted {
        category: "secret",
        value: "s3cretPa55",
        context: "fstab: //admin:s3cretPa55@nas.example.net/share",
    },
    Planted {
        category: "private key",
        value: "MIIEowIBAAKCAQEAu1SU1LfVLPHCozMxH2Mo4lgOEePzNm0tRgeLezV6ffAt0gun",
        context: "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAu1SU1LfVLPHCozMxH2Mo4lgOEePzNm0tRgeLezV6ffAt0gun\n-----END RSA PRIVATE KEY-----",
    },
    Planted {
        category: "SSH key",
        value: "AAAAC3NzaC1lZDI1NTE5AAAAIPlantedKeyBody0",
        context: "authorized ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPlantedKeyBody0 mweber@studio",
    },
    Planted {
        category: "card number",
        value: "4111 1111 1111 1111",
        context: "paid with 4111 1111 1111 1111",
    },
    Planted {
        category: "random string",
        value: "Zm9vYmFyQmF6UXV4MTIzNDU2Nzg5MGFiY2RlZg",
        context: "blob Zm9vYmFyQmF6UXV4MTIzNDU2Nzg5MGFiY2RlZg== end",
    },
];

/// Text that must stay readable after scrubbing: proof that the scrubber
/// does not simply delete everything.
pub const USEFUL: &[&str] = &[
    "Failed to start tur-update-stage.service",
    "TUR-BOOT-GATE-003",
    "nouveau 0000:01:00.0: firmware: failed to load nvidia/tu104/gr/fecs_bl.bin",
];

/// The whole corpus as one text, one planted line per line.
pub fn planted_text() -> String {
    let mut text = String::new();
    for planted in PLANTED {
        text.push_str(planted.context);
        text.push('\n');
    }
    for useful in USEFUL {
        text.push_str(useful);
        text.push('\n');
    }
    text
}

/// The corpus as journal lines of priority 3, `journalctl -o json` style.
pub fn planted_journal() -> String {
    let mut text = String::new();
    for line in planted_text().lines() {
        let entry = serde_json::json!({
            "MESSAGE": line,
            "PRIORITY": "3",
            "SYSLOG_IDENTIFIER": "planted",
            "_HOSTNAME": "kitchen-pc-7731",
        });
        text.push_str(&entry.to_string());
        text.push('\n');
    }
    text
}

/// A `report.json` with the corpus in its title and description.
pub fn planted_report() -> String {
    serde_json::json!({
        "schema_version": 1,
        "kind": "request",
        "title": "Sound stops for mweber on kitchen-pc-7731 (alice.smith@example.org)",
        "description": planted_text(),
        "error_code": "TUR-AUDIO-RESUME-001",
        "component": "audio",
        "affected_version": null,
        "prepared_by": "person",
        "boot_health": {
            "notice_kind": "fallback",
            "error_code": "TUR-BOOT-GATE-003",
            "text": "Restored after 3 tries on kitchen-pc-7731 for user mweber(uid=1001), SSID=\"Schmidt Family WLAN\"",
            "failed_check": "R4",
            "rejected_version": "0.2.0"
        }
    })
    .to_string()
}

/// The members of a bundle with `report_json` (if given) and `files`, with
/// a manifest and checksums that verify.
pub fn members(report_json: Option<&str>, files: &[(&str, &str)], profile: Profile) -> Vec<Member> {
    members_at(report_json, files, profile, CREATED, Some("0.2.0"))
}

/// [`members`] with a creation time and a TUR version.
pub fn members_at(
    report_json: Option<&str>,
    files: &[(&str, &str)],
    profile: Profile,
    created: u64,
    version: Option<&str>,
) -> Vec<Member> {
    let mut listed: Vec<Member> = vec![
        (
            "README.txt".to_owned(),
            b"TUR support bundle (test)\n".to_vec(),
        ),
        (
            "redaction-report.json".to_owned(),
            br#"{"schema_version":1,"profile":"default","categories":{}}"#.to_vec(),
        ),
    ];
    if let Some(report) = report_json {
        listed.push(("report.json".to_owned(), format!("{report}\n").into_bytes()));
    }
    for (path, text) in files {
        listed.push(((*path).to_owned(), text.as_bytes().to_vec()));
    }
    let mut entries: Vec<FileEntry> = listed
        .iter()
        .map(|(path, data)| FileEntry {
            path: path.clone(),
            sha256: sha256_hex(data),
            size: u64::try_from(data.len()).unwrap_or(u64::MAX),
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest = Manifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        collector: CollectorInfo {
            name: COLLECTOR_NAME.to_owned(),
            version: "0.1.0".to_owned(),
        },
        created_utc: format_utc(created),
        tur: TurIdentity {
            build_id: Some("20261112.g0123456789ab".to_owned()),
            version: version.map(str::to_owned),
            source_revision: None,
            deployment_checksum: None,
            image_digest: None,
        },
        boot_id: None,
        architecture: "x86_64".to_owned(),
        kernel_release: Some("6.17.1-300.fc45.x86_64".to_owned()),
        installer_session_id: None,
        redaction_profile: profile,
        since_boot: false,
        files: entries,
        missing: Vec::new(),
    };
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap_or_default();
    manifest_bytes.push(b'\n');
    let mut members: Vec<Member> = vec![(MANIFEST_PATH.to_owned(), manifest_bytes)];
    members.extend(listed);
    let checksums = render_checksums(&members);
    members.push((CHECKSUMS_PATH.to_owned(), checksums.into_bytes()));
    members
}

/// The manifest among `members`.
pub fn manifest(members: &[Member]) -> Manifest {
    let data = &members
        .iter()
        .find(|(path, _)| path == MANIFEST_PATH)
        .unwrap_or_else(|| panic!("no {MANIFEST_PATH}"))
        .1;
    serde_json::from_slice(data).unwrap_or_else(|error| panic!("{MANIFEST_PATH}: {error}"))
}

/// A packed bundle.
pub fn bundle(report_json: Option<&str>, files: &[(&str, &str)], profile: Profile) -> Vec<u8> {
    pack_members(&members(report_json, files, profile))
}

/// Packs members into a bundle.
pub fn pack_members(members: &[Member]) -> Vec<u8> {
    pack(members, CREATED).unwrap_or_else(|error| panic!("packing: {error}"))
}

/// The bundle with the whole corpus planted, unredacted, in `report.json`,
/// the journal and the failed units.
pub fn planted_bundle() -> Vec<u8> {
    let report = planted_report();
    let journal = planted_journal();
    let units = planted_text();
    bundle(
        Some(&report),
        &[
            ("boot/current-journal.jsonl", journal.as_str()),
            ("boot/previous-journal.jsonl", journal.as_str()),
            ("system/failed-units.txt", units.as_str()),
            (
                "system/kernel.txt",
                "Linux lenovo-ys 6.17.1-300.fc45.x86_64 #1 SMP\n",
            ),
        ],
        Profile::Default,
    )
}

/// Every planted value that occurs in `text`, with its category.
pub fn leaks(text: &str) -> Vec<(&'static str, &'static str)> {
    let lower = text.to_lowercase();
    PLANTED
        .iter()
        .filter(|planted| lower.contains(&planted.value.to_lowercase()))
        .map(|planted| (planted.category, planted.value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundles_verify_with_tur_diag() {
        let archive = planted_bundle();
        let (verified, members) =
            tur_diag::verify_members(&archive, tur_diag::report::MAX_UNPACKED_BYTES)
                .expect("verifies");
        assert_eq!(verified.manifest.redaction_profile, Profile::Default);
        let report = tur_diag::report::read_report(&members).expect("a report");
        assert_eq!(report.component, "audio");
        // The corpus really is in it.
        let all: String = members
            .iter()
            .map(|(_, data)| String::from_utf8_lossy(data).into_owned())
            .collect();
        assert_eq!(leaks(&all).len(), PLANTED.len());
    }
}
