// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Tests for the redaction engine. Every identifier here is invented.

use super::*;

const MACHINE_ID: &str = "0123456789abcdef0123456789abcdef";
const BOOT_ID: &str = "0f0e0d0c-0b0a-4909-8807-060504030201";

fn known() -> KnownIdentifiers {
    KnownIdentifiers {
        usernames: vec!["alice".to_owned(), "Alice Example".to_owned()],
        hostnames: vec!["Workstation-7.lan.example.org".to_owned()],
        machine_ids: vec![MACHINE_ID.to_owned()],
        preserved_ids: vec![BOOT_ID.to_owned()],
    }
}

fn default_redactor() -> Redactor {
    Redactor::new(Profile::Default, &known())
}

fn technical_redactor() -> Redactor {
    Redactor::new(Profile::Technical, &known())
}

/// A redactor that knows nothing about the machine in advance.
fn plain() -> Redactor {
    Redactor::new(Profile::Default, &KnownIdentifiers::default())
}

#[test]
fn mac_addresses_get_stable_pseudonyms() {
    let mut redactor = plain();
    let out = redactor.redact(
        "link/ether 3C:52:82:AA:0b:1f brd ff:ff:ff:ff:ff:ff\n\
         wlan0: 3c-52-82-aa-0b-1f and 3c:52:82:aa:0b:20",
    );
    assert_eq!(
        out,
        "link/ether <mac-1> brd ff:ff:ff:ff:ff:ff\nwlan0: <mac-1> and <mac-2>"
    );
}

#[test]
fn longer_hardware_addresses_are_redacted_whole() {
    let mut redactor = plain();
    assert_eq!(
        redactor.redact("duid 00:01:00:01:2a:bb:cc:dd:3c:52:82:aa:0b:1f"),
        "duid <mac-1>"
    );
    assert_eq!(redactor.redact("aa:bb:cc:dd:ee:ff:1234"), "<mac-2>:1234");
}

#[test]
fn text_that_only_resembles_a_mac_address_is_left_alone() {
    for text in [
        "started at 12:34:56",
        "0000:00:1f.6 Ethernet controller",
        "aa:bb:cc:dd:ee",
        "checksum 00:00:00:00:00:00",
        "xaa:bb:cc:dd:ee:ff0",
        "2026-11-12T06:15:00",
        "ratio 16:9:4",
    ] {
        assert_eq!(plain().redact(text), text);
    }
}

#[test]
fn public_ipv4_addresses_are_redacted_and_private_ones_kept() {
    let mut redactor = plain();
    let out = redactor.redact(
        "inet 192.168.1.20/24 gw 192.168.1.1 dns 9.9.9.9 peer 93.184.216.34:443, \
         93.184.216.34 again; lo 127.0.0.1; cgnat 100.64.3.1; ll 169.254.10.2; \
         172.16.0.5 172.32.0.5 10.1.2.3",
    );
    assert_eq!(
        out,
        "inet 192.168.1.20/24 gw 192.168.1.1 dns <ipv4-1> peer <ipv4-2>:443, \
         <ipv4-2> again; lo 127.0.0.1; cgnat 100.64.3.1; ll 169.254.10.2; \
         172.16.0.5 <ipv4-3> 10.1.2.3"
    );
}

#[test]
fn ipv4_addresses_at_the_end_of_a_sentence_are_redacted() {
    let mut redactor = plain();
    assert_eq!(
        redactor.redact("Connection from 93.184.216.34. Closing (93.184.216.35)"),
        "Connection from <ipv4-1>. Closing (<ipv4-2>)"
    );
}

#[test]
fn ipv4_address_ranges_are_redacted_at_both_ends() {
    let mut redactor = plain();
    assert_eq!(
        redactor.redact("pool 93.184.216.1-93.184.216.254 lan 192.168.1.2-192.168.1.9"),
        "pool <ipv4-1>-<ipv4-2> lan 192.168.1.2-192.168.1.9"
    );
}

#[test]
fn version_numbers_are_not_mistaken_for_ipv4_addresses() {
    for text in [
        "mesa v24.2.3.1",
        "kernel-6.17.1.300",
        "systemd 257.1.2.3",
        "build 1.2.3.4.5",
        "pkg 1.2.3.4-5.fc45",
        "firmware 1.2.3.4.fc45",
        "addr 01.02.03.04",
        "id_1.2.3.4",
        "mask 255.255.255.0",
        "net 0.0.0.0",
        "x1.2.3.4",
        "Linux 6.17.1-300.fc45.x86_64",
    ] {
        assert_eq!(plain().redact(text), text);
    }
}

#[test]
fn public_ipv6_addresses_are_redacted_in_every_spelling() {
    let mut redactor = plain();
    let out = redactor.redact(
        "inet6 2a02:8071:b585:1e40:0:0:0:1f/64 scope global\n\
         src=2A02:8071:B585:1E40::1F dst=[2606:4700:4700::1111]:443 via fe80::1%wlan0",
    );
    assert_eq!(
        out,
        "inet6 <ipv6-1>/64 scope global\n\
         src=<ipv6-1> dst=[<ipv6-2>]:443 via fe80::1%wlan0"
    );
}

#[test]
fn ipv6_addresses_next_to_punctuation_are_redacted() {
    let mut redactor = plain();
    assert_eq!(
        redactor.redact("connected to 2a02:8071::5. addr:2a02:8071::5 ::ffff:93.184.216.34"),
        "connected to <ipv6-1>. addr:<ipv6-1> <ipv6-2>"
    );
}

#[test]
fn local_ipv6_is_kept_unless_it_embeds_a_mac_address() {
    let text = "fe80::3e52:82ff:feaa:b1f fe80::1 fd12:3456:789a::10 \
                fd00::3e52:82ff:feaa:b1f ::1 ff02::fb";
    assert_eq!(
        default_redactor().redact(text),
        "<ipv6-1> fe80::1 fd12:3456:789a::10 <ipv6-2> ::1 ff02::fb"
    );
    // The technical profile keeps MAC addresses, so it keeps these too.
    assert_eq!(technical_redactor().redact(text), text);
}

#[test]
fn text_that_only_resembles_an_ipv6_address_is_left_alone() {
    for text in [
        "std::fmt::Display",
        "cafe::bad",
        "error: a::b",
        "12:34:56.789",
        "dead::beefy",
        "2001:db8::1 is documentation",
        "::ffff:192.168.0.1",
        "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    ] {
        assert_eq!(plain().redact(text), text);
    }
}

#[test]
fn uuids_are_redacted_but_boot_id_and_partition_types_stay() {
    let text = "root=UUID=6F1C2A9E-0d7b-4c55-9a1e-3b8f5d2c7e10 \
                parttype c12a7328-f81f-11d2-ba4b-00a0c93ec93b \
                boot 0f0e0d0c-0b0a-4909-8807-060504030201 \
                nil 00000000-0000-0000-0000-000000000000 \
                again 6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10";
    assert_eq!(
        default_redactor().redact(text),
        "root=UUID=<uuid-1> \
         parttype c12a7328-f81f-11d2-ba4b-00a0c93ec93b \
         boot 0f0e0d0c-0b0a-4909-8807-060504030201 \
         nil 00000000-0000-0000-0000-000000000000 \
         again <uuid-1>"
    );
    assert_eq!(technical_redactor().redact(text), text);
}

#[test]
fn uuids_escaped_in_systemd_unit_names_are_redacted() {
    let mut redactor = plain();
    let text = r"systemd-cryptsetup@luks\x2d6f1c2a9e\x2d0d7b\x2d4c55\x2d9a1e\x2d3b8f5d2c7e10.service /dev/disk/by-uuid/6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10";
    assert_eq!(
        redactor.redact(text),
        r"systemd-cryptsetup@luks\x2d<uuid-1>.service /dev/disk/by-uuid/<uuid-1>"
    );
}

#[test]
fn short_filesystem_ids_are_redacted_by_label_and_then_everywhere() {
    let mut redactor = plain();
    let blkid = "/dev/sda1: UUID=\"A1B2-C3D4\" TYPE=\"vfat\" PARTUUID=\"9f8e7d6c-01\"\n";
    let journal = "mounting /dev/disk/by-uuid/A1B2-C3D4 on /boot/efi (a1b2-c3d4)\n";
    redactor.learn(blkid);
    redactor.learn(journal);
    assert_eq!(
        redactor.redact(blkid),
        "/dev/sda1: UUID=\"<uuid-1>\" TYPE=\"vfat\" PARTUUID=\"<uuid-2>\"\n"
    );
    assert_eq!(
        redactor.redact(journal),
        "mounting /dev/disk/by-uuid/<uuid-1> on /boot/efi (<uuid-1>)\n"
    );
}

#[test]
fn serial_numbers_are_redacted_by_label_and_then_everywhere() {
    let mut redactor = plain();
    let text = "Serial Number: PF2ABC9X\n\
                \"serial\": \"S4EWNX0M123456\",\n\
                ID_SERIAL_SHORT=WD-WX12A34567\n\
                wwn: 0x5002538e40a1b2c3\n\
                iSerial=20210315ABCD\n";
    redactor.learn(text);
    assert_eq!(
        redactor.redact(text),
        "Serial Number: <serial-1>\n\
         \"serial\": \"<serial-2>\",\n\
         ID_SERIAL_SHORT=<serial-3>\n\
         wwn: <serial-4>\n\
         iSerial=<serial-5>\n"
    );
    // The same serial number without its label, as another file would have it.
    assert_eq!(
        redactor.redact("ata1.00: ATA-11: Samsung SSD 870, S4EWNX0M123456, max UDMA/133"),
        "ata1.00: ATA-11: Samsung SSD 870, <serial-2>, max UDMA/133"
    );
}

#[test]
fn text_that_only_resembles_a_serial_number_is_left_alone() {
    for text in [
        "serial8250: ttyS0 at I/O 0x3f8 (irq = 4, base_baud = 115200) is a 16550A",
        "Serial: 8250/16550 driver, 4 ports, IRQ sharing enabled",
        "usb 1-2: New USB device strings: Mfr=1, Product=2, SerialNumber=3",
        "usb usb1: SerialNumber: 0000:00:14.0",
        "00:14.0 Serial bus controller [0c80]: Intel Corporation Device [8086:a0a4]",
        "Serial Number: To Be Filled By O.E.M.",
        "\"serial\": null, \"wwn\": null",
    ] {
        let mut redactor = plain();
        redactor.learn(text);
        assert_eq!(redactor.redact(text), text);
    }
}

#[test]
fn wifi_ssids_are_redacted_quoted_unquoted_and_out_of_context() {
    let mut redactor = plain();
    let text = "wlan0: Trying to associate with 3c:52:82:aa:0b:1f (SSID='Casa Rossi 5G' freq=5180 MHz)\n\
                \tSSID: Casa Rossi 5G\n\
                wpa: ssid=\"Casa Rossi 5G\" bssid=3c:52:82:aa:0b:1f\n\
                Activation: (wifi) connection 'Casa Rossi 5G' has security\n";
    redactor.learn(text);
    assert_eq!(
        redactor.redact(text),
        "wlan0: Trying to associate with <mac-1> (SSID='<ssid-1>' freq=5180 MHz)\n\
         \tSSID: <ssid-1>\n\
         wpa: ssid=\"<ssid-1>\" bssid=<mac-1>\n\
         Activation: (wifi) connection '<ssid-1>' has security\n"
    );
}

#[test]
fn an_unquoted_ssid_on_a_long_line_is_cut_at_the_first_word() {
    let mut redactor = plain();
    assert_eq!(
        redactor.redact("SSID: MyNet, signal -40 dBm, channel 36, security WPA2-PSK enabled"),
        "SSID: <ssid-1>, signal -40 dBm, channel 36, security WPA2-PSK enabled"
    );
}

#[test]
fn user_names_are_redacted_in_paths_and_as_whole_words() {
    let mut redactor = default_redactor();
    let text = "ExecStart=/home/alice/bin/sync --log /var/home/alice/.cache/x.log\n\
                session opened for user alice(uid=1000) by (uid=0)\n\
                malice alice_backup alice@example.org Alice Example\n\
                /run/media/alice/USB-STICK mounted, /home/bob/notes.txt read, /root/.bashrc";
    redactor.learn(text);
    assert_eq!(
        redactor.redact(text),
        "ExecStart=/home/<user-1>/bin/sync --log /var/home/<user-1>/.cache/x.log\n\
         session opened for user <user-1>(uid=1000) by (uid=0)\n\
         malice alice_backup <user-1>@example.org <user-2>\n\
         /run/media/<user-1>/USB-STICK mounted, /home/<user-3>/notes.txt read, /root/.bashrc"
    );
}

#[test]
fn user_names_are_learnt_from_login_records() {
    let mut redactor = plain();
    redactor.learn("pam_unix(sddm:session): session opened for user carol.m(uid=1001) by (uid=0)");
    assert_eq!(
        redactor.redact("carol.m started plasma; root(uid=0) and gdm(uid=42) did not"),
        "<user-1> started plasma; root(uid=0) and gdm(uid=42) did not"
    );
}

#[test]
fn host_names_are_redacted_whatever_their_case() {
    let mut redactor = default_redactor();
    let text = "Linux workstation-7 6.17.1-300.fc45.x86_64 #1 SMP\n\
                WORKSTATION-7.lan.example.org login: \n\
                workstation-70 and myworkstation-7 are other machines; localhost is not one";
    assert_eq!(
        redactor.redact(text),
        "Linux <host-1> 6.17.1-300.fc45.x86_64 #1 SMP\n\
         <host-1> login: \n\
         workstation-70 and myworkstation-7 are other machines; localhost is not one"
    );
}

#[test]
fn journal_lines_are_redacted_field_by_field() {
    let mut redactor = default_redactor();
    let journal = concat!(
        r#"{"_HOSTNAME":"workstation-7","MESSAGE":"Accepted key for alice from 93.184.216.34\nalice logged in","_MACHINE_ID":"0123456789abcdef0123456789abcdef","_PID":"812"}"#,
        "\n",
        r#"{"MESSAGE":[47,104,111,109,101,47,97,108,105,99,101,47,27,91,48,109],"_HOSTNAME":"workstation-7"}"#,
        "\n",
        "not json alice\n",
    );
    redactor.learn_document(Format::JsonLines, journal);
    let out = redactor.redact_document(Format::JsonLines, journal);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3);

    let first: Value = serde_json::from_str(lines[0]).expect("line stays valid JSON");
    assert_eq!(first["_HOSTNAME"], "<host-1>");
    // "\nalice" in the JSON source is a line break followed by the name, not
    // the letter n glued to it.
    assert_eq!(
        first["MESSAGE"],
        "Accepted key for <user-1> from <ipv4-1>\n<user-1> logged in"
    );
    assert_eq!(first["_MACHINE_ID"], "<machine-id-1>");
    assert_eq!(first["_PID"], "812");

    // journalctl writes a message with control characters as a byte array.
    let second: Value = serde_json::from_str(lines[1]).expect("line stays valid JSON");
    assert_eq!(second["MESSAGE"], "/home/<user-1>/\u{1b}[0m");
    assert_eq!(second["_HOSTNAME"], "<host-1>");

    assert_eq!(lines[2], "not json <user-1>");
    for original in ["alice", "workstation", "93.184", MACHINE_ID] {
        assert!(!out.contains(original), "{original} survived in {out}");
    }
}

#[test]
fn a_value_named_in_one_field_is_redacted_in_every_other() {
    let mut redactor = plain();
    let lsblk = "{\n   \"blockdevices\": [\n      {\"name\": \"nvme0n1\", \"serial\": \
                 \"S4EWNX0M123456\", \"model\": \"Samsung SSD 980\"}\n   ]\n}\n";
    let journal = concat!(
        r#"{"MESSAGE":"nvme nvme0: S4EWNX0M123456 firmware 2B4QFXO7","_HOSTNAME":"den-pc"}"#,
        "\n",
        r#"{"MESSAGE":"den-pc kernel: /home/dana/.config read by dana","_HOSTNAME":"den-pc"}"#,
        "\n",
    );
    redactor.learn_document(Format::Text, lsblk);
    redactor.learn_document(Format::JsonLines, journal);
    let lsblk_out = redactor.redact_document(Format::Text, lsblk);
    let journal_out = redactor.redact_document(Format::JsonLines, journal);

    assert!(
        lsblk_out.contains("\"serial\": \"<serial-1>\""),
        "{lsblk_out}"
    );
    assert!(
        journal_out.contains("nvme nvme0: <serial-1> firmware 2B4QFXO7"),
        "{journal_out}"
    );
    assert!(
        journal_out.contains("<host-1> kernel: /home/<user-1>/.config read by <user-1>"),
        "{journal_out}"
    );
    for original in ["S4EWNX0M123456", "den-pc", "dana"] {
        assert!(!lsblk_out.contains(original), "{original} in {lsblk_out}");
        assert!(
            !journal_out.contains(original),
            "{original} in {journal_out}"
        );
    }
}

#[test]
fn machine_id_is_redacted_in_both_profiles_and_both_spellings() {
    for profile in Profile::ALL {
        let mut redactor = Redactor::new(profile, &known());
        assert_eq!(redactor.profile(), profile);
        assert_eq!(
            redactor.redact(
                "machine 0123456789ABCDEF0123456789abcdef \
                 dbus 01234567-89ab-cdef-0123-456789abcdef"
            ),
            "machine <machine-id-1> dbus <machine-id-1>"
        );
    }
}

#[test]
fn labelled_secrets_are_redacted() {
    let mut redactor = plain();
    let out = redactor.redact(
        "mount -o username=svc,password=hunter2 //nas/share\n\
         psk=\"correct horse\" token: abc.def-123\n\
         Authorization: Bearer eyJhbGciOi.payload.sig\n\
         Password: \n\
         password required for tokens: 5",
    );
    assert_eq!(
        out,
        "mount -o username=svc,password=<secret-1> //nas/share\n\
         psk=\"<secret-2>\" token: <secret-3>\n\
         Authorization: <secret-4>\n\
         Password: \n\
         password required for tokens: 5"
    );
}

#[test]
fn technical_profile_keeps_exactly_serials_macs_and_uuids() {
    let text = "alice@workstation-7 3c:52:82:aa:0b:1f Serial Number: PF2ABC9X \
                UUID=6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10 from 93.184.216.34 \
                SSID='Casa Rossi' /home/alice";

    let mut technical = technical_redactor();
    technical.learn(text);
    assert_eq!(
        technical.redact(text),
        "<user-1>@<host-1> 3c:52:82:aa:0b:1f Serial Number: PF2ABC9X \
         UUID=6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10 from <ipv4-1> \
         SSID='<ssid-1>' /home/<user-1>"
    );

    let mut default = default_redactor();
    default.learn(text);
    assert_eq!(
        default.redact(text),
        "<user-1>@<host-1> <mac-1> Serial Number: <serial-1> \
         UUID=<uuid-1> from <ipv4-1> \
         SSID='<ssid-1>' /home/<user-1>"
    );

    for category in Category::ALL {
        let kept = matches!(category, Category::Serial | Category::Mac | Category::Uuid);
        assert_eq!(
            Profile::Technical.redacts(category),
            !kept,
            "{}",
            category.name()
        );
        assert!(Profile::Default.redacts(category), "{}", category.name());
    }
}

#[test]
fn ordinary_log_lines_are_left_alone() {
    for text in [
        "[    1.234567] pci 0000:00:1f.6: [8086:15fa] type 00 class 0x020000",
        "usb 3-1.2: new high-speed USB device number 4 using xhci_hcd",
        "audit: type=1400 audit(1731391234.567:89): apparmor",
        "Bluetooth: hci0: BCM: chip id 150 build 0759",
        "systemd[1]: Started cups.service - CUPS Scheduler.",
        "NetworkManager[812]: <info>  [1731391234.5678] dhcp4 (wlan0): address=192.168.1.20",
        "kernel: x86/fpu: xstate_offset[2]:  576, xstate_sizes[2]:  256",
        "resolution 1920x1080@60.00Hz, version 2.40.1, time 23:59:59",
        "/usr/lib/systemd/system/tur-diag.service: loaded",
        "/root/.cache and /var/lib/home-assistant are not home directories",
    ] {
        let mut redactor = default_redactor();
        redactor.learn(text);
        assert_eq!(redactor.redact(text), text);
    }
}

#[test]
fn report_counts_values_and_never_contains_them() {
    let mut redactor = default_redactor();
    let out =
        redactor.redact("alice alice bob 3c:52:82:aa:0b:1f 3c:52:82:aa:0b:20 3c:52:82:aa:0b:1f");
    assert_eq!(out, "<user-1> <user-1> bob <mac-1> <mac-2> <mac-1>");

    let report = redactor.report();
    assert_eq!(report.schema_version, REDACTION_REPORT_SCHEMA_VERSION);
    assert_eq!(report.profile, Profile::Default);
    assert_eq!(report.categories.len(), Category::ALL.len());
    let users = &report.categories["username"];
    assert_eq!((users.distinct_values, users.replacements), (1, 2));
    assert_eq!(users.pseudonym_prefix, "user");
    let macs = &report.categories["mac_address"];
    assert_eq!((macs.distinct_values, macs.replacements), (2, 3));
    let uuids = &report.categories["uuid"];
    assert_eq!((uuids.distinct_values, uuids.replacements), (0, 0));
    assert!(uuids.redacted);

    let json = serde_json::to_string(&report).expect("report serialises");
    for original in ["alice", "3c:52", "workstation", MACHINE_ID] {
        assert!(!json.contains(original), "{original} is in the report");
    }

    let technical = technical_redactor().report();
    assert_eq!(technical.profile, Profile::Technical);
    assert!(!technical.categories["mac_address"].redacted);
    assert!(!technical.categories["serial_number"].redacted);
    assert!(!technical.categories["uuid"].redacted);
    assert!(technical.categories["username"].redacted);
}

#[test]
fn overlapping_candidates_never_leave_a_remainder() {
    let spans = vec![
        Span::new(0, 10, Category::Ssid, "0123456789"),
        Span::new(5, 15, Category::Username, "56789abcde"),
        Span::new(2, 4, Category::Mac, "23"),
        Span::new(20, 25, Category::Uuid, "xxxxx"),
        Span::new(20, 25, Category::MachineId, "xxxxx"),
    ];
    let ranges: Vec<(usize, usize, Category)> = resolve(spans)
        .iter()
        .map(|span| (span.start, span.end, span.category))
        .collect();
    assert_eq!(
        ranges,
        vec![
            (0, 10, Category::Ssid),
            (10, 15, Category::Username),
            (20, 25, Category::MachineId),
        ]
    );
}

#[test]
fn non_ascii_text_is_redacted_without_splitting_characters() {
    let known = KnownIdentifiers {
        usernames: vec!["jörg".to_owned()],
        hostnames: vec!["büro-pc".to_owned()],
        ..KnownIdentifiers::default()
    };
    let mut redactor = Redactor::new(Profile::Default, &known);
    let text = "Größe: 5 — /home/jörg/Übung\n\tSSID: Café Ünïcode\n\
                büro-pc ✓ 日本語 3c:52:82:aa:0b:1f é jörg";
    redactor.learn(text);
    let out = redactor.redact(text);
    assert_eq!(
        out,
        "Größe: 5 — /home/<user-1>/Übung\n\tSSID: <ssid-1>\n\
         <host-1> ✓ 日本語 <mac-1> é <user-1>"
    );
}

#[test]
fn generic_names_are_not_identifiers() {
    let known = KnownIdentifiers {
        usernames: vec!["root".to_owned(), "x".to_owned()],
        hostnames: vec!["localhost".to_owned(), "fedora".to_owned()],
        machine_ids: vec!["not-a-machine-id".to_owned()],
        preserved_ids: Vec::new(),
    };
    let mut redactor = Redactor::new(Profile::Default, &known);
    let text = "root@localhost fedora x not-a-machine-id /root /home/x/file";
    assert_eq!(
        redactor.redact(text),
        "root@localhost fedora x not-a-machine-id /root /home/<user-1>/file"
    );
}
