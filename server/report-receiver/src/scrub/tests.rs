// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! One test per rule: what it removes and what it must leave readable.

use super::*;

fn scrub(text: &str) -> String {
    let mut scrubber = Scrubber::new();
    scrubber.learn(text);
    scrubber.scrub(text)
}

/// `text` is scrubbed to exactly `expected`.
fn check(text: &str, expected: &str) {
    assert_eq!(scrub(text), expected, "input: {text}");
}

/// Nothing in `text` changes.
fn kept(text: &str) {
    check(text, text);
}

#[test]
fn every_rule_compiles() {
    for rule in all_rules() {
        assert!(!rule.as_str().is_empty());
    }
}

#[test]
fn private_keys_and_ssh_keys() {
    check(
        "key:\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----\nafter",
        "key:\n<private-key>\nafter",
    );
    check(
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEow cut short",
        "<private-key>",
    );
    check(
        "authorized: ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGx0aW1lbGVzcw alice@laptop done",
        "authorized: <ssh-key> done",
    );
}

#[test]
fn credentials_tokens_and_secrets() {
    check(
        "mount https://admin:s3cretPa55@nas.example.net/share failed",
        "mount https://<credentials>@nas.example.net/share failed",
    );
    check(
        "token ghp_abcdefghijklmnopqrstuvwxyz0123456789 leaked",
        "token <token> leaked",
    );
    check(
        "key sk-ant-api03-AbCdEf0123456789AbCd end",
        "key <token> end",
    );
    check("aws AKIAIOSFODNN7EXAMPLE end", "aws <token> end");
    check(
        "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N end",
        "jwt <token> end",
    );
    check(
        "Authorization: Bearer abc.def.ghi\nnext line",
        "Authorization: <secret>\nnext line",
    );
    check("curl -H Bearer abcDEF123456ghi", "curl -H Bearer <secret>");
    check("password=hunter2-Correct next", "password=<secret> next");
    check(
        r#"{"wifi_psk": "verysecret", "x": 1}"#,
        r#"{"wifi_psk": "<secret>", "x": 1}"#,
    );
    check("status_token=0123 done", "status_token=<secret> done");
    // A systemd unit is not a secret.
    kept("gnome-keyring-secrets.service: Started");
    // A client pseudonym stays.
    kept("password=<secret-1>");
}

#[test]
fn email_addresses_but_not_unit_names() {
    check(
        "contact alice.smith@example.org please",
        "contact <email> please",
    );
    kept("Started user@1000.service - User Manager for UID 1000.");
    kept("getty@tty1.service: Deactivated successfully.");
    check("prompt alice@laptop $", "prompt <user>@<host> $");
    kept("Unit getty@tty1 is fine");
}

#[test]
fn home_and_profile_paths() {
    check(
        "open /home/jdoe-private/Documents/taxes.pdf and /var/home/jdoe-private/x",
        "open /home/<user>/Documents/taxes.pdf and /var/home/<user>/x",
    );
    check(
        "mounted /run/media/anna/USB",
        "mounted /run/media/<user>/USB",
    );
    check(
        r"wine: C:\Users\Giulia Rossi\AppData\Local",
        r"wine: C:\Users\<user>\AppData\Local",
    );
    check(
        "prefix drive_c/users/grossi/Desktop",
        "prefix drive_c/users/<user>/Desktop",
    );
    kept(r"C:\Users\Public\Desktop");
    kept("/home/<user-1>/x");
    // A name learnt from a path is removed everywhere.
    check(
        "/home/jdoe-private/x then jdoe-private logged in",
        "/home/<user>/x then <user> logged in",
    );
}

#[test]
fn user_names_after_their_labels() {
    check("USER=mweber HOME=/root", "USER=<user> HOME=/root");
    check(
        "pam_unix(login:session): session opened for user mweber(uid=1001) by (uid=0)",
        "pam_unix(login:session): session opened for user <user>(uid=1001) by (uid=0)",
    );
    check(
        "id: uid=1001(mweber) gid=1001",
        "id: uid=1001(<user>) gid=1001",
    );
    check(
        "Accepted publickey for mweber from",
        "Accepted publickey for <user> from",
    );
    check(
        "Failed password for invalid user bob42 from",
        "Failed password for invalid user <user> from",
    );
    check(r#""USER":"mweber""#, r#""USER":"<user>""#);
    kept("session opened for user root(uid=0)");
}

#[test]
fn host_names() {
    check(
        r#"{"_HOSTNAME":"kitchen-pc-7731","MESSAGE":"kitchen-pc-7731 is up"}"#,
        r#"{"_HOSTNAME":"<host>","MESSAGE":"<host> is up"}"#,
    );
    check(
        "Static hostname: kitchen-pc-7731",
        "Static hostname: <host>",
    );
    check(
        "Linux kitchen-pc-7731 6.17.1-300.fc45.x86_64 #1 SMP",
        "Linux <host> 6.17.1-300.fc45.x86_64 #1 SMP",
    );
    check(
        "printer at mybox.local and nas-4471.lan",
        "printer at <host> and <host>",
    );
    // The first line of the kernel log is not a host name.
    kept("Linux version 6.17.1-300.fc45.x86_64 (gcc 15.1.1)");
    // pam's service:type pair is not a user name.
    kept("pam_unix(login:session): session closed");
    kept("hostname=localhost");
    kept("files in /home/<user-1>/.local/share");
}

#[test]
fn wifi_names() {
    check(
        r#"wpa: SSID="Schmidt Family WLAN" connected; Schmidt Family WLAN ok"#,
        r#"wpa: SSID=<wifi> connected; <wifi> ok"#,
    );
    check(
        "Trying to associate with SSID 'Home Net 5'",
        "Trying to associate with SSID <wifi>",
    );
    check(
        "Activation: starting connection 'Schmidt-5G' (uuid)",
        "Activation: starting connection '<wifi>' (uuid)",
    );
    check(r#"ESSID:"FRITZ!Box 7590 QX""#, "ESSID:<wifi>");
    kept("ssid=<ssid-1>");
}

#[test]
fn serial_numbers() {
    check("Serial Number: PF2XK9QZ", "Serial Number: <serial>");
    check(
        "ID_SERIAL_SHORT=S4EWNX0M987654 and again S4EWNX0M987654",
        "ID_SERIAL_SHORT=<serial> and again <serial>",
    );
    check("product_serial: 5CD1234XYZ", "product_serial: <serial>");
    kept("Serial Number: Not Specified");
    kept("serial8250: ttyS0 at I/O 0x3f8");
}

#[test]
fn hardware_addresses() {
    check("ether 3c:52:82:aa:0b:1f brd", "ether <mac> brd");
    check("BT 3C-52-82-AA-0B-2E up", "BT <mac> up");
    check("cisco 3c52.82aa.0b3f", "cisco <mac>");
    check("mac=3c5282aa0b4f", "mac=<mac>");
    kept("link/ether 00:00:00:00:00:00 brd ff:ff:ff:ff:ff:ff");
}

#[test]
fn ip_addresses() {
    check("from 81.2.69.160 port 22", "from <ipv4> port 22");
    check("lease 192.168.178.23/24", "lease <ipv4>/24");
    kept("listening on 127.0.0.1:631 and 0.0.0.0");
    kept("version 1.2.3.4.5 and v1.2.3.4");
    check(
        "addr 2a02:810d:4bc0:1a2b:3c4d:5e6f:7081:92a3 and fe80::1c2d:3eff:fe4f:5a6b%wlp2s0",
        "addr <ipv6> and <ipv6>",
    );
    kept("loopback ::1 any ::");
    kept("at 10:23:45 and 12:00");
    kept("std::io::Error in std::fs");
}

#[test]
fn identifiers_cards_and_random_strings() {
    check(
        "disk 6f1c2a9e-0d7b-4c55-9a1e-3b8f5d2c7e10 ok",
        "disk <uuid> ok",
    );
    check(
        r"unit dev-disk-by\x2duuid-6f1c2a9e\x2d0d7b\x2d4c55\x2d9a1e\x2d3b8f5d2c7e10.device",
        r"unit dev-disk-by\x2duuid-<uuid>.device",
    );
    check(
        "machine 4c4c4544004e3510804ab4c04f4e3732 boot",
        "machine <id> boot",
    );
    check("card 4111 1111 1111 1111 exp", "card <card> exp");
    kept("serial count 1234567890123");
    check(
        "blob Zm9vYmFyQmF6UXV4MTIzNDU2Nzg5MGFiY2RlZg== end",
        "blob <token> end",
    );
    kept("digest sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    kept("/org/freedesktop/NetworkManager/ActiveConnection/12345678901");
}

#[test]
fn useful_text_stays_readable() {
    let text = "Failed to start tur-update-stage.service - Stage the TUR update.\n\
                kernel: nouveau 0000:01:00.0: firmware: failed to load nvidia/tu104/gr/fecs_bl.bin\n\
                TUR-BOOT-GATE-003 required check R4 failed after 93 s\n\
                pipewire[1234]: spa.alsa: hw:0: snd_pcm_avail after recover: Broken pipe";
    let out = scrub(text);
    assert_eq!(out, text);
}

#[test]
fn replacements_are_counted() {
    let mut scrubber = Scrubber::new();
    let text = "USER=mweber and mweber again, mail mweber@example.org";
    scrubber.learn(text);
    assert_eq!(scrubber.learnt(), 1);
    let out = scrubber.scrub(text);
    assert_eq!(out, "USER=<user> and <user> again, mail <email>");
    assert_eq!(scrubber.replacements(), 3);
}
