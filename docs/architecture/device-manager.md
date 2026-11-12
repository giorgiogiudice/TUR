# Device manager: hardware inventory

Design note for `services/tur-device-manager` (task TUR-007). Baseline:
specification §3.1, §6.3, §6.5, §10, §16 and §31;
[ADR-0005](../adr/0005-languages-and-workspace.md);
[ADR-0007](../adr/0007-app-centric-routing.md), which reduces the device manager
to "a device inventory and guest hand-off"; AI control layer §5 and §11.

**Status.** The inventory core, the problem findings, the redacted view and the
`tur-device` command-line tool are built and tested in CI **against fixtures
only**. No test has run on real hardware. The D-Bus interface is defined but not
implemented. The hand-off state machine exists as types only. See
[What is not done](#what-is-not-done).

## 1. Scope

Linux owns the hardware (§3.1, §10.1). This component describes it and changes
nothing. Every fact is read through one trait, `Probe`, which can read a file,
list a directory, read a symbolic link and run one of three fixed read-only
commands. The trait has no operation that writes, so no code built on it can
change device ownership, bind or unbind a driver, or write to sysfs.

| Part | Where |
|---|---|
| Library, all logic | `services/tur-device-manager/src/lib.rs` and modules |
| Command-line tool `tur-device` | `src/main.rs` |
| Schema of the JSON output | `schemas/device-inventory.schema.json` |
| D-Bus interface definition | `services/tur-device-manager/dbus/org.tur.Device1.xml` |
| Assistant tool manifest | `services/tur-device-manager/agent-tools.json` |
| Fixture: a hand-written tree, not a capture | `services/tur-device-manager/tests/fixtures/laptop/`, written by `generate-laptop.py` beside it |

## 2. Sources

No driver or device database is kept here (§10.2). The only tables in the code
are the names of the PCI and USB class codes.

| Source | Used for | If unavailable |
|---|---|---|
| sysfs attributes under `/sys/bus`, `/sys/class`, `/sys/devices` | Every device and its identity | The category is empty |
| The `driver` symbolic link | The bound driver, or none | `driver` is `null` |
| `modalias`, matched against `/usr/lib/modules/<release>/modules.alias` | Whether this kernel has a module for the device; `/sys/module/<name>` says whether it is loaded | `kernel_modules` is `null`; a missing driver cannot be told from an unloaded one |
| `udevadm info --export-db` | Hardware-database names, input device kinds, V4L2 capabilities, the serial number of SCSI disks | Those fields are `null` or empty |
| `/proc/asound/cards` | Sound card names | Cards are listed from sysfs without names |
| `/sys/class/dmi/id`, `/sys/firmware/efi` | Platform identity, UEFI or BIOS, Secure Boot | `null` or `unknown` |
| `fwupdmgr get-devices --json` | Firmware devices and releases newer than the installed one | `firmware.fwupd` is `unavailable` or `invalid` |
| `journalctl --dmesg --boot --output=cat --no-pager --quiet --lines=20000` | Drivers that refused a device; firmware files that could not be loaded | `kernel.available` is false; findings rest on sysfs alone |
| `/etc/machine-id` | Salt for hashed identifiers (section 4) | Identifiers are positions only |

The commands are run by absolute path, with fixed arguments, an empty
environment, no standard input, a bound on output size and a time limit. The
set is closed: a probe cannot be asked to run anything else. A source that
could not be used is recorded in `source_notes` with what the inventory lacks
as a result, so an absent value is never mistaken for a negative finding. An
empty kernel log is treated as unreadable, because `journalctl` gives an empty
answer, not an error, to a user who may not read the journal.

The module alias table lists loadable modules only. "No module matches" means
no loadable module claims the device, not that no built-in driver exists.

## 3. Model

`Inventory` has one list per category, the platform, firmware status, kernel
evidence and source notes. Every field is always present in JSON; `null` means
the value could not be read or does not exist. Each device carries `id`,
`id_basis` and `sysfs_path`.

| Category | Fields beyond the common three |
|---|---|
| `pci` | address, vendor, device, class, subsystem and revision IDs, class and hardware-database names, driver, modalias, matching kernel modules; for display-class devices `gpu`: boot VGA flag, DRM cards and each connector's status |
| `usb` | bus-port path, vendor and product IDs, class, manufacturer and product strings, serial, speed, declared USB version, removable or fixed, root-hub flag; per interface: class, subclass, protocol, driver, modalias, matching modules |
| `network` | name, kind (ethernet, wireless, loopback, bridge, wwan, virtual, other), wireless flag, virtual flag, link type, driver, operational state, MAC address, parent device |
| `audio` | card number and identifier, short and long name, ALSA driver name, kernel driver, parent device |
| `storage_controllers` | kind (nvme, sata, ata, scsi, sas, raid, ufs, sd_host, usb_mass_storage, other), driver, parent device. A view of PCI functions and USB interfaces already listed |
| `block` | name, model, vendor, serial, WWID, transport, removable, rotational, size, driver, parent device. Whole physical disks only: partitions, loop, device-mapper and RAID devices belong to the storage service |
| `input` | name, bus, vendor, product and version IDs, kinds from udev, `phys`, `uniq`, parent device |
| `bluetooth` | name, driver, address where sysfs shows one, kill-switch state, parent device |
| `cameras` | V4L2 node, name, index, whether it captures, driver, parent device |
| `thunderbolt` | name, vendor and device names and IDs, unique ID, authorisation state, host flag, domain security level |
| `platform` | DMI vendor, product, version, family, chassis type, board, firmware vendor, version and date, the four DMI serial and UUID values, boot mode, Secure Boot state |
| `firmware` | per fwupd device: name, vendor, plugin, version, serial, flags, updatable, update state, releases flagged as upgrades |
| `kernel` | firmware files that failed to load and drivers that refused a device, each reduced to driver, kernel device name, file name and error number |

Printers, scanners, fingerprint readers and USB serial ports (§10.3) appear as
the USB or PCI devices they are; they have no category of their own yet.

## 4. Identifier stability

A device identifier never contains a serial number or address. Where it is
derived from one, the value is hashed: the first 64 bits of SHA-256 over the
machine identifier, a NUL, a domain label, a NUL and the value. The result is
stable on one installation, differs between installations, and cannot be turned
back into the value without the machine identifier.

| `id_basis` | Derived from | Survives |
|---|---|---|
| `hardware_identifier` | USB serial number with vendor and product ID; permanent MAC address; disk WWID, or serial with model; Thunderbolt unique ID. Devices that hang off such a device (its camera nodes, its sound card) inherit the basis | A restart, re-enumeration in a different order, a move to another port |
| `topology` | PCI address with vendor and device ID; USB bus-port path; for class devices, the parent's identifier and an index among siblings | A restart, while the hardware layout is unchanged |
| `kernel_name` | A kernel-assigned name (virtual network interfaces, sound cards known only to procfs) | Nothing is promised |

When an identifier cannot be stable:

- **No hardware identifier.** Every PCI device, and any USB device without a
  serial number, is known only by position. Moving it to another slot or port
  gives it a new identifier. PCI bus numbers can also shift when a card or a
  Thunderbolt dock is added.
- **Shared serial numbers.** If two devices in one inventory have the same
  hardware-derived identifier (cheap devices often share one serial number),
  both fall back to position, so they stay distinct. An identifier can
  therefore change basis when a twin is plugged in.
- **No machine identifier.** If `/etc/machine-id` cannot be read, nothing is
  hashed and every identifier is a position. A source note says so.
- **Reinstallation.** A new machine identifier changes every hashed identifier.
- **Random MAC addresses.** Only an address the kernel reports as permanent
  (`addr_assign_type` 0) is used.
- **A root hub's `serial`** is the address of its controller. It is not treated
  as a serial number and is not reported.

Identifiers are the same in the plain and the redacted view.

## 5. Privacy

Serial numbers, MAC and Bluetooth addresses, hardware UUIDs, disk WWIDs and the
`phys` and `uniq` strings of input devices have the type `Sensitive`. In JSON
such a field is always an object, `{"sensitive": true, "redacted": false,
"value": "..."}`, so a consumer can recognise it without the schema.

`Inventory::redacted()` (`--redact`) empties every such field and then removes
the same values wherever they recur in other text. udev names a USB network
adapter `enx` followed by its MAC address, so without the second step the
address would survive in the interface name and its sysfs path. Values shorter
than six characters are removed from their own field only.

Each model type lists every one of its fields in `src/redact.rs`, as walked or
skipped; a field added to the model without being listed does not compile.

Limits: a hashed identifier is a pseudonym. Anyone who holds the machine
identifier can test a guessed serial number against it. Product and
manufacturer strings are not treated as sensitive, although a device could put
anything in them.

## 6. Untrusted input

sysfs content is partly chosen by devices (USB descriptor strings, input device
names) and command output is parsed text. All of it is treated as untrusted:

- A read is cut at 4096 bytes for an attribute; command output over its limit is
  an error, not a truncation. Directory listings, udev records, alias lines,
  log lines, fwupd devices and failure lists are all bounded.
- Text is decoded leniently (invalid UTF-8 becomes U+FFFD), control characters
  and white space collapse to one space, invisible formatting characters
  (zero-width, bidirectional overrides) are removed, and the result is cut to
  256 characters with an ellipsis.
- Identifiers, driver names and codes are accepted only in their expected form;
  anything else is `null`.
- Code that is not a test may not use `unwrap`, `expect`, `panic!` or unchecked
  indexing (denied by lint). Pattern matching against `modules.alias` is
  iterative and bounded.

`--from` reads a developer's fixture directory. A symbolic link in it that
points outside the directory is followed by the operating system; a fixture is
trusted input.

## 7. Problem findings

`detect(inventory)` is a pure function. Each finding has a code, a severity, a
title, the device identifier, a user message, a next step and technical detail
(§6.3). No next step involves a driver written for another operating system
(§10.6); a test checks the wording.

| Code | Title | Detected when | Severity | Next step |
|---|---|---|---|---|
| `TUR-DEVICE-DRIVER-001` | No driver for the device | No `driver` link, and no module alias matches (or the alias table could not be read) | Warning for storage, network, display, multimedia, communication, input, serial-bus and wireless controllers; information for other PCI classes and for USB interfaces, which are often driven from user space | Install updates and restart; report with `tur-diag hardware` |
| `TUR-DEVICE-DRIVER-002` | Driver available but not loaded | No `driver` link; a module alias matches; no matching module is loaded | As above | Restart; check for a deny-list entry or kernel option; report |
| `TUR-DEVICE-DRIVER-003` | Driver did not attach to the device | The kernel log records a failed probe (error); or no `driver` link although a matching module is loaded (as above); or the log records a failed probe of a device outside the inventory (warning) | See left | Restart; report |
| `TUR-DEVICE-FIRMWARE-001` | Firmware file could not be loaded | The kernel log records a failed firmware load | Warning if the device also has no driver or is inactive; otherwise information, because drivers often ask for optional files | Install updates; report; do not copy firmware from other sources |
| `TUR-DEVICE-INACTIVE-001` | Driver attached but the device is not usable | A PCI network controller (class 0200 or 0280) has a driver, at least one interface exists on the system, and none belongs to it | Warning | Restart; install updates; report |
| `TUR-DEVICE-THUNDERBOLT-001` | Thunderbolt device not authorised | `authorized` is 0 on a device that is not the host router | Information | Authorise it in settings if trusted; otherwise unplug it |
| `TUR-DEVICE-INPUT-001` | Fixture directory cannot be read | `tur-device --from` names something that is not a directory | Command fails, exit status 1 | Correct the path |
| `TUR-DEVICE-OUTPUT-001` | Output cannot be written | Encoding or writing the result failed | Command fails, exit status 1 | None |

What the findings can and cannot see: a driver that failed to bind is reported
with certainty only when the kernel log is readable. Without it, the failure is
inferred from a loaded module that is not bound. Missing firmware is visible
only in the kernel log; `TUR-DEVICE-INACTIVE-001` is the sysfs-only hint. Each
finding states its evidence.

Tests for each code are in `src/problems.rs` and `tests/inventory.rs`.

## 8. Hand-off states

`src/handoff.rs` defines the states of §10.4, the events between them and
`transition(state, event)`, a pure function. **No code performs a transition on
hardware.** Hand-off is a class 3 action (AI control layer §5) and will be a
separate interface.

```text
discovered -> identified -> driver_bound -> permission_assigned -> available

available -> handoff_requested -> host_driver_unbound -> guest_attached
          -> guest_owned -> guest_detached -> host_rebind_attempted -> available
```

Added to the two chains, each to honour the text of §10.4:

- `unavailable`, with a reason. After `guest_detach_failed` the only ways out
  are confirmation that the guest has released the device, or removal. A host
  rebind is refused in that state, so conflicting ownership cannot be attempted.
  After `host_rebind_failed` the rebind may be retried.
- Failure and cancellation edges: a request cancelled or an unbind that failed
  returns to `available`; a failed guest attach goes to `host_rebind_attempted`.
- `removed`, reachable from every state and terminal.

Tests enumerate every pair of state and event against the table of legal
transitions, and check that no single transition moves ownership directly
between host and guest.

## 9. Command line

```text
tur-device inventory [--from <dir>] [--json] [--redact]
tur-device problems  [--from <dir>] [--json]
```

With `--from`, a directory laid out like the root of a small system is read
instead of the live system, and no command is run: `sys/`, `proc/`,
`etc/machine-id`, `usr/lib/modules/<release>/modules.alias`, and recorded
command output in `commands/` (`udevadm-info-export-db.txt`,
`fwupdmgr-get-devices.json`, `journalctl-kernel.txt`).

Exit status: 0 on success, whether or not problems were found; 1 if the input
or output failed, with `error: TUR-DEVICE-...` on standard error; 2 for a usage
error.

## 10. D-Bus interface

`org.tur.Device1` on the system bus, object `/org/tur/Device1`.
**Proposed, not implemented and not frozen.** JSON results are documents of
`schemas/device-inventory.schema.json`.

| Member | Returns | Authorisation |
|---|---|---|
| `ListDevices(a{sv} options) -> a(ssss)` | id, category, name, driver for each device (`device_summary`). Option `categories` (`as`) filters | `org.tur.device.read`: allowed for any local user. The same facts are world-readable in sysfs |
| `GetDevice(s id, a{sv} options) -> s` | JSON `device_document`, redacted unless `include-sensitive` is true | `org.tur.device.read`; with `include-sensitive`, `org.tur.device.read-sensitive`: administrator authentication in an active local session, refused otherwise |
| `GetInventory(a{sv} options) -> s` | JSON inventory, redacted unless `include-sensitive` is true | As `GetDevice` |
| `ListProblems(a{sv} options) -> a(sssssssa{ss})` | code, severity, device identifier, category, title, user message, next step, technical detail for each finding | `org.tur.device.read` |
| Signal `DeviceAdded(s id, s category)` | A device appeared | Broadcast; carries no sensitive value |
| Signal `DeviceRemoved(s id, s category)` | A device went away | Broadcast |
| Property `SchemaVersion` (`u`, constant) | The `schema_version` of the JSON documents | Readable by any local user |

Contract common to every method:

- **Errors.** `org.tur.Device1.Error.InvalidArgument` (malformed identifier,
  unknown option, option of the wrong type); `org.tur.Device1.Error.NoSuchDevice`
  (`GetDevice` only); `org.tur.Device1.Error.NotAuthorized`;
  `org.tur.Device1.Error.NotReady` (no scan has completed);
  `org.tur.Device1.Error.Failed` (internal error, with a `TUR-DEVICE-*` code in
  the message).
- **Timeout.** A method answers from the last completed scan and never scans
  inside a call, so it answers within one second. If no scan has completed since
  the service started, the call waits up to 20 seconds and then fails with
  `NotReady`. A scan is bounded by the command time limits of section 2.
- **Cancellation.** Every method only reads and holds no state for the caller.
  A caller that disconnects, or whose call times out, leaves nothing to undo.
  No method takes a cancellation handle.
- **Unknown options** are an error, not ignored, so that a later option is
  never silently dropped by an older service.

The service runs with the privileges needed to read DMI serial numbers and the
kernel log, which an ordinary user lacks. That is why the unredacted view needs
its own authorisation.

## 11. Assistant tools

`agent-tools.json` follows AI control layer §11. It declares four tools, all
class 0 (observe): `device_list`, `device_get`, `device_get_inventory`,
`device_list_problems`. No tool has an argument that asks for sensitive values;
the assistant is always given the redacted view (class 0 is "allowed, subject to
privacy exclusions"). There is no tool for hand-off: that is class 3 and is not
part of this interface.

## What is not done

- **Any change of ownership.** No driver is bound or unbound, no device is
  attached to a guest, nothing is written to sysfs. The hand-off state machine
  is types and a pure function.
- **The D-Bus server.** No process owns `org.tur.Device1`. There is no polkit
  policy file, no systemd unit and no subscription to udev events; the signals
  are defined but nothing emits them.
- **The Device Centre user interface.**
- **Hardware validation.** Recorded as **NOT RUN**: every case of the §31 matrix
  (USB HID, storage, serial, audio, webcam; AMD, Intel and NVIDIA GPUs; internal
  and USB audio; Ethernet and Wi-Fi; Bluetooth; fwupd; Thunderbolt). No hardware
  runner exists (ADR-0002).
- **The live path.** `tur-device` without `--from` (reading the running system
  and running `udevadm`, `fwupdmgr` and `journalctl`) is compiled but has not
  been run by any test. The bounding of command output and time is tested with
  `echo` and `sleep`.
- **Real command output.** The fixture's udev database, fwupd JSON and kernel
  log were written by hand from the documented formats. They are not captures
  from a real machine, so agreement with what those tools really print is not
  shown.
- **The image.** The binary is not installed in the OS image.

Known limits of what is built:

- The schema test checks that every field the code emits is declared and every
  declared field is emitted, for the fixture and for an empty system. It does
  not prove that every enumeration value in the schema can be produced.
- `modules.alias` does not list built-in drivers (section 2).
- Devices on buses other than PCI and USB (platform, I2C, ACPI) are listed only
  through their class devices; a failed probe of one is reported from the
  kernel log without a device identifier.
- §10.4 has no path for a USB device that needs no kernel driver (a WinUSB or
  libusb device), which is the usual candidate for hand-off. The state machine
  follows the specification and has no such edge; it needs a decision.
