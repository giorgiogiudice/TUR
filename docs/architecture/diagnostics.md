# Diagnostics: `tur-diag`

Design note for the diagnostics collector (task TUR-004, first slice).
Specification sections: §5.5, §6.3, §6.4, §6.5, §29.1 to §29.3.

- Code: `services/tur-diagnostics/` (library `tur_diag`, binary `tur-diag`).
- Manifest schema: `schemas/diag-bundle-manifest.schema.json`.

## Purpose

`tur-diag` produces a support bundle: one local file that describes the state of a
machine well enough for someone else to diagnose a problem, with personal and
machine identifiers replaced. It runs only when a person asks.

It also prepares problem reports, and sends one, through the system's `curl`,
only when the person runs `tur-diag report send` and confirms it, and only to
the receiver configured for TUR, which is empty by default
([problem-reports.md](problem-reports.md), ADR-0024). Nothing else in
`tur-diag` uses the network, and nothing is sent in the background.

## Commands

```sh
tur-diag status [--json]
tur-diag collect --output FILE.tar.zst [--since-boot] [--profile default|technical]
tur-diag verify-bundle FILE
tur-diag report prepare|send|list|status ...     (problem-reports.md, section 2)
```

- `status` prints the TUR build (from `/usr/lib/tur/release`), source revision,
  deployment checksum (from `bootc status`), system name, kernel, boot ID and the
  number of failed systemd units. On a host that is not TUR it says so and still
  reports the rest. Values that cannot be read are shown as unknown, or `null` in
  JSON.
- `collect` writes a new file with mode 0600. It refuses to overwrite an existing
  file. `--since-boot` leaves out data from earlier boots (the previous boot's
  journal and the pstore listing).
- `verify-bundle` exits 0 if the bundle is intact and 1 otherwise, printing a
  stable error code.

Run `collect` as root for a complete bundle. As an ordinary user, the kernel ring
buffer, the system journal and some hardware details may be refused; each refusal
is recorded in the manifest and the rest is still collected.

## Structure

| Module | Responsibility |
|---|---|
| `host` | The only access to the machine: run a command (no shell, time limit, output limit) or read a file (size limit). `SystemHost` is real; `FakeHost` answers from a table in tests. |
| `collectors` | The table of what is collected, and the path allow-list. |
| `redact` | The redaction engine and `redaction-report.json`. |
| `manifest` | The types behind `manifest.json`. |
| `bundle` | Builds, writes and verifies the archive. |
| `identity` | Build identity and status; the names given to the redactor. |
| `error` | The `TUR-DIAG-*` registry. |
| `report` | Problem reports: `report.json`, preparing, checking, the configuration, the local list, sending through `curl` ([problem-reports.md](problem-reports.md)). |

`main.rs` only parses arguments and prints.

## What is collected

A collector is a row of data: bundle path, and either a command with arguments or
a file to read. Each has a time limit (20 seconds; 60 for the journal) and an
output limit (64 MiB).

| Bundle path | Source |
|---|---|
| `system/kernel.txt` | `uname -a` |
| `system/kernel-cmdline.txt` | `/proc/cmdline` |
| `system/os-release.txt` | `/etc/os-release` |
| `system/tur-release.txt` | `/usr/lib/tur/release` |
| `system/modules.txt` | `/proc/modules` |
| `system/deployment.json` | `bootc status --format=json` |
| `system/failed-units.txt` | `systemctl list-units --state=failed` |
| `system/systemd-analyze.txt` | `systemd-analyze critical-chain` |
| `boot/current-journal.jsonl` | `journalctl --boot=0 --output=json`, last 50 000 entries |
| `boot/previous-journal.jsonl` | `journalctl --boot=-1 --output=json`, last 20 000 entries |
| `boot/kernel-ring-buffer.txt` | `dmesg` |
| `boot/pstore/listing.txt` | `ls -lA /sys/fs/pstore` |
| `hardware/pci.txt` | `lspci -nnk` |
| `hardware/usb.txt` | `lsusb` |
| `hardware/block-devices.json` | `lsblk --json` with a fixed column list |
| `hardware/gpu-drm.txt` | `ls -l /sys/class/drm` |
| `hardware/gpu-vulkan.txt` | `vulkaninfo --summary` |
| `hardware/audio.txt` | `wpctl status` |
| `hardware/network.txt` | `nmcli general status` |
| `hardware/network-devices.txt` | `nmcli --fields DEVICE,TYPE,STATE device status` |
| `hardware/bluetooth.txt` | `bluetoothctl show`, without its `Name:` and `Alias:` lines |
| `hardware/firmware.txt` | `fwupdmgr get-devices` |
| `storage/filesystems.txt` | `blkid` limited to the `TYPE`, `UUID`, `PARTUUID` and `PTTYPE` tags |
| `storage/mounts.txt` | `/proc/mounts` |
| `services/tur-service-status.txt` | `systemctl list-units 'tur-*'` |

A missing command, a non-zero exit, a timeout, a missing or unreadable file never
fails the bundle. The item is left out and recorded in the manifest's `missing`
list with one of these reasons: `command_not_found`, `non_zero_exit`, `timed_out`,
`file_not_found`, `permission_denied`, `io_error`, `excluded_by_option`,
`forbidden_path`. Output over the size limit is cut at the last complete line,
kept, and recorded with the reason `truncated`.

## What is never collected

- Nothing under a home directory, `/root`, `/run/user`, `/run/media`, `/media` or
  `/mnt`. File collectors may read only below `/proc/`, `/sys/` and `/usr/lib/`,
  plus `/etc/os-release`; an absolute path given to a command must be below the
  same locations. A test checks every row of the table against this allow-list,
  and the same check runs again before a collector executes, so a row that
  violated it would be recorded as `forbidden_path` and not run.
- No user documents, browser data, application databases, keyrings, private keys
  or shell history.
- `/etc/fstab`, because it can hold credentials for network shares. The mount
  table carries what is mounted and how.
- NetworkManager connection profiles and connection names.
- Filesystem and partition labels: `lsblk` and `blkid` are asked for named columns
  and tags that exclude them.
- `/etc/passwd`, the host name and the machine ID are read to tell the redactor
  what to look for. They are not copied into the bundle.

Commands are chosen so that they print little personal data in the first place.
Redaction is the second line of defence.

## Redaction

Every collected file is text and passes through the redactor before it is stored.
The text recorded for a failed collector is redacted too. Each distinct value is
replaced by a pseudonym, `<category-N>`, that is the same everywhere in one bundle,
so that a disk seen in `lsblk` can still be matched to the same disk in the
journal. Pseudonyms are not stable between bundles.

Redaction makes two passes over the whole bundle. The first learns values that are
recognisable only in context (a user name in a home path, a serial number after
its label). The second replaces those values wherever they stand as a whole word,
together with everything recognisable by shape.

Journal files are JSON lines. Each line is parsed, every string value is redacted
after decoding, and the line is written back, so JSON escapes can neither hide an
identifier nor be broken by a pseudonym. Messages that `journalctl` encodes as byte
arrays are decoded to text first. A side effect is that keys come out in sorted
order.

| Category | Pseudonym | What is recognised | `default` | `technical` |
|---|---|---|---|---|
| User names | `<user-N>` | Names from `/etc/passwd` (UID 1000 to 59999: login name, real name, home directory name) and from `USER`, `LOGNAME`, `SUDO_USER`; the path component after `/home/` (which covers `/var/home/`) and `/run/media/`; `name(uid=N)` for the same UID range. Then every whole-word occurrence. `root` and `nobody` are left. | redacted | redacted |
| Host names | `<host-N>` | The kernel host name and `/etc/hostname`; the `_HOSTNAME` journal field. Then every whole-word occurrence, ignoring case. The short and the fully qualified name share one pseudonym. `localhost` and `fedora` are left. | redacted | redacted |
| Serial numbers | `<serial-N>` | The value after a `serial` label (`Serial Number:`, `"serial":`, `ID_SERIAL_SHORT=`, `iSerial=`, `SerialNumber:`) or a `wwn` label, if it is 4 to 64 letters, digits, `.`, `_` or `-` with at least one digit. Values of 8 characters or more are then replaced everywhere. | redacted | **kept** |
| MAC addresses | `<mac-N>` | Six or more pairs of hexadecimal digits joined by `:` or by `-`, which includes Bluetooth addresses and longer hardware addresses. All-zero and broadcast addresses are left. | redacted | **kept** |
| IPv4 addresses | `<ipv4-N>` | Dotted quads that are globally routable. Loopback, private, link-local, shared (100.64/10), documentation, benchmarking, multicast and reserved ranges are left readable. | redacted | redacted |
| IPv6 addresses | `<ipv6-N>` | Any spelling of a global address; equal addresses share a pseudonym. Loopback, unspecified, multicast and documentation addresses are left. Link-local and unique-local addresses are left unless their interface identifier is a MAC address in EUI-64 form. | redacted | redacted, except the EUI-64 local addresses, which are **kept** |
| UUIDs | `<uuid-N>` | Any 8-4-4-4-12 UUID, including the `\x2d`-escaped form in systemd unit names; shorter filesystem identifiers after a `UUID` or `PARTUUID` label or after `by-uuid/`. The current boot ID (the manifest records it), GPT partition-type GUIDs and the nil UUID are left. | redacted | **kept** |
| Wi-Fi SSIDs | `<ssid-N>` | The value after an `SSID` label, quoted or to the end of the line. Values of 4 characters or more are then replaced everywhere. | redacted | redacted |
| Machine ID | `<machine-id-N>` | `/etc/machine-id` in plain and dashed spelling; the `_MACHINE_ID` journal field; a 32-digit value after a `machine_id` label. | redacted | redacted |
| Labelled secrets | `<secret-N>` | The value after `password`, `passwd`, `passphrase`, `psk`, `token`, `secret`, `api_key`, `apikey`; the rest of the line after `authorization`. | redacted | redacted |

The `technical` profile therefore keeps exactly three things: hardware serial
numbers and world-wide names, MAC addresses (and local IPv6 addresses derived from
them), and disk, partition and other UUIDs. They identify the computer and its
devices. The profile is recorded in the manifest, stated in `README.txt`, and
`collect` prints a warning when it is used.

`redaction-report.json` holds, for each category, whether the profile redacts it,
the number of distinct values and the number of replacements. It never holds a
value.

When a rule has to guess, it guesses towards redacting. Known cases where
harmless text is replaced:

- A four-part version number that is also a valid public IPv4 address and stands
  alone (`24.2.3.1`). Version numbers attached to a name, a `v`, or a suffix
  (`pkg-1.2.3.4`, `v1.2.3.4`, `1.2.3.4-5`, `1.2.3.4.5`) are recognised and left.
- Certificate fingerprints and similar colon-separated byte strings count as
  hardware addresses.
- Bluetooth service UUIDs are replaced like any other UUID.
- A word that equals a user name, host name or SSID is replaced wherever it stands
  alone.

## Bundle layout

```text
<name>.tar.zst
  manifest.json
  README.txt
  redaction-report.json
  system/ boot/ hardware/ storage/ services/    (the files listed above, sorted by path)
  checksums.sha256
```

The archive is a tar stream of regular files (mode 0644, owner 0, modification
time set to the creation time) compressed with zstd, with a zstd content checksum.
Members are written in the order shown, so the same input produces the same
bytes.

`manifest.json` (schema version 1) records: the collector name and version, the
UTC creation time, the TUR build ID, version, source revision, deployment checksum
and image digest where available, boot ID, architecture, kernel release, an
installer session ID (always `null` for now), the redaction profile, whether
`--since-boot` was used, every file with its SHA-256 and size as stored, and the
`missing` list. A test compares the Rust types with the JSON schema.

`checksums.sha256` lists every other member, including the manifest, in
`sha256sum --check` format.

A problem report adds one member, `report.json`, listed in the manifest and the
checksum list like every other file and redacted with the rest
(`bundle::build_bundle_with`). `README.txt` then names it.

`verify-bundle` checks that the archive decompresses and holds only regular files
with safe, unique paths; that the manifest and the checksum list are present, well
formed and of a supported schema version; that every member matches its recorded
SHA-256 (and its size, for files listed in the manifest); and that the archive,
the manifest and the checksum list name exactly the same files. It guards against
damage and incomplete copies. It is not a signature: someone who rewrites a file
and both lists consistently is not detected.

The same checks are available with a smaller limit on the unpacked size
(`bundle::verify_members`, `bundle::unpack_within`), which return the verified
members too. Problem reports use 64 MiB, and the receiver links this library to
check every upload with exactly these rules.

### Differences from §29.1

- Files hold the output of the tool named above, so several that §29.1 lists as
  `.json` are `.txt` here (`pci.txt`, `usb.txt`, `audio.txt`, `network.txt`).
- `boot/pstore/` holds a listing only, not the stored records.
- Categories without a collector yet are absent: `desktop/`, `runtimes/`,
  `installer/`, and `system/bootloader.txt`, `system/build-manifest.json`,
  `boot/boot-stages.jsonl`, `storage/udisks-status.json`.
- The file is named by `--output`; the `tur-support-<build-id>-<boot-id>` name is
  not generated.

## Error codes

Every failure is reported in two layers (§6.3): the code and a message for the
user with an action, then the technical detail. The registry is in
`src/error.rs`; a test checks that every code parses as a `tur_core::ErrorCode` in
the `DIAG` family, is unique, and has not changed.

| Code | Meaning |
|---|---|
| `TUR-DIAG-COLLECT-001` | The bundle could not be written to the output path. |
| `TUR-DIAG-COLLECT-002` | The output path already exists; nothing was overwritten. |
| `TUR-DIAG-BUNDLE-001` | The bundle file could not be read. |
| `TUR-DIAG-BUNDLE-002` | The archive is damaged: zstd or tar decoding failed. |
| `TUR-DIAG-BUNDLE-003` | The archive has a member `tur-diag` never writes (not a regular file, unsafe path, duplicate). |
| `TUR-DIAG-BUNDLE-004` | `manifest.json` or `checksums.sha256` is missing. |
| `TUR-DIAG-BUNDLE-005` | The manifest or the checksum list cannot be parsed. |
| `TUR-DIAG-BUNDLE-006` | The manifest's schema version is not supported. |
| `TUR-DIAG-BUNDLE-007` | A file does not match its recorded SHA-256 or size. |
| `TUR-DIAG-BUNDLE-008` | Files were added or removed: the lists and the archive disagree. |
| `TUR-DIAG-BUNDLE-009` | The archive expands beyond the 2 GiB verification limit. |
| `TUR-DIAG-INTERNAL-001` | A defect in `tur-diag`. |
| `TUR-DIAG-REPORT-001` | A field of a problem report is missing, too long, or not an accepted value. |
| `TUR-DIAG-REPORT-002` | No receiver is configured; nothing was sent. |
| `TUR-DIAG-REPORT-003` | `--to` is not the configured receiver; nothing was sent. |
| `TUR-DIAG-REPORT-004` | Sending was not confirmed (`--yes`, or `yes` typed on a terminal). |
| `TUR-DIAG-REPORT-005` | The file is not a prepared report a receiver accepts. |
| `TUR-DIAG-REPORT-006` | The receiver could not be reached, or its answer was not a receipt; the prepared file is kept. |
| `TUR-DIAG-REPORT-007` | The receiver refused the report; the detail carries its `TUR-DIAG-RECEIVER-*` code. |
| `TUR-DIAG-REPORT-008` | The problem-report configuration file is not usable. |
| `TUR-DIAG-REPORT-009` | Boot health has no notice to make a report from. |
| `TUR-DIAG-REPORT-010` | The directory of prepared and sent reports cannot be used. |
| `TUR-DIAG-REPORT-011` | No sent report has that identifier. |
| `TUR-DIAG-REPORT-012` | The prepared report is over 8 MiB, or expands beyond 64 MiB. |

Command-line usage errors are reported by the argument parser with exit code 2 and
have no TUR code.

## Known gaps

- **Redaction is rule-based and incomplete.** It does not recognise: e-mail
  addresses and personal names that are not account names; secrets without one of
  the labels above; MAC addresses written without separators or in dotted form;
  a public IPv4 address joined to a word by `-` or `_` (`host-203.0.113.9`),
  which is taken for a version number;
  short filesystem identifiers inside systemd-escaped unit names; an unquoted SSID
  containing spaces on a line longer than 32 bytes (only its first word is
  replaced); Bluetooth device names in `wpctl status`; mount points named after a
  volume label. System logs can also describe what a person was doing. A bundle
  should be read before it is shared, and `README.txt` says so.
- **No preview before export.** §6.5 asks for one. `collect` prints counts after
  writing; an interactive preview belongs with the GUI entry point.
- **Not run on a real TUR image.** The code is tested with a fake host only. The
  exact output and exit codes of the collected tools on Fedora 45, and the field
  names read from `bootc status --format=json`, have not been checked against a
  running system. A tool that behaves differently is recorded as missing data
  rather than failing the bundle, but its file would be absent.
- **Performance is unmeasured.** The bundle is built in memory, and each journal
  field is scanned once per learnt value. Time and memory on a large journal have
  not been measured.
- **Not signed.** See the note on `verify-bundle` above.
- The registry holds title, severity, detection, user message, explanation and
  recovery for each code. §29.3 also asks for tests per code; the paths that
  produce `COLLECT-001`, `COLLECT-002`, `BUNDLE-001` to `BUNDLE-009` and the
  `REPORT-*` codes other than `-011` are tested (`BUNDLE-009` through the
  smaller limit of a report); `INTERNAL-001` and `REPORT-011` are not.

## Out of scope for this slice

- The GUI entry point and a D-Bus service.
- `collect --installer-session` and installer logs.
- The other §6.4 subcommands: `boot-report`, `hardware`, `storage`, `drivers`,
  `runtimes`.
- Upload other than a problem report the person sends
  ([problem-reports.md](problem-reports.md)).
- journald persistence configuration in the image (§5.5), and installing
  `tur-diag` into the image.
- Test of export to a removable disk image (TUR-004).
