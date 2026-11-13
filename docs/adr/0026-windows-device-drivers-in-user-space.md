# ADR-0026: Windows device drivers: hand-off and HID first, a user-space driver host only as an experiment

- **Status:** Proposed
- **Date:** 2026-11-13
- **Specification sections:** rule 8 (§1), §10.1, §10.3, §10.4, §12.1; AI
  control layer §5. Evidence:
  [windows-drivers-1](../reports/windows-drivers-1.md)

## Context

Some devices have only Windows drivers, mostly USB devices: measuring
instruments, programmers, label printers, scanners, dongles, vendor
configuration tools for keyboards, mice and controllers. Rule 8 forbids
loading a Windows `.sys` driver into the Linux kernel and allows "Linux
drivers, userspace adapters, or explicit VM passthrough". The owner asked why
TUR should not try the user-space adapter path: Wine already has a driver host
(`winedevice.exe`, a user-space `ntoskrnl.exe`, `wineusb.sys` on libusb,
`winebus.sys`/`hidclass.sys` for HID).

The experiment in windows-drivers-1 measured that path on Fedora 45's stable
Wine (11.20, with the Staging patch set) against USB devices emulated through
USB/IP, and read Wine's source at 11.20, 11.21 and master. Its findings
decide this ADR; in short:

- A minimal WDM USB function driver, written for the experiment, installed
  from its INF, was loaded by Wine's PnP manager into `winedevice.exe` (a
  Linux process running as the user) and moved bulk and vendor-control data
  correctly, also inside a bubblewrap sandbox holding exactly one device.
  It avoided, on purpose, the many kernel functions Wine stubs.
- A KMDF driver does not load: `WDFLDR.SYS` is missing. KMDF and its loader
  are Microsoft binaries. UMDF has no host in Wine; `winusb.dll` is stubs.
- HID devices work through Linux hidraw and Wine's own HID stack, with no
  Windows driver at all.
- Wine's USB bus driver turns some failures into hangs (a refused transfer
  is never completed, `IoCancelIrp` does nothing), never claims interfaces,
  and handles eight URB functions; `pnputil` and `DiInstallDriverW` are stubs.
- Granting one device node and binding only it into the sandbox isolates the
  others; disabling the device's hub port withdrew it from a running
  application without hanging it.

## Options considered

1. **A TUR feature that hosts vendor USB drivers in Wine** for any device.
   Advantages: no Windows licence or VM. Costs: works only for the narrow class
   of plain WDM USB drivers that use the URBs `wineusb.sys` implements; KMDF
   drivers (the framework Microsoft steers new drivers to) cannot load at
   all, because the framework (`WDFLDR.SYS`, `Wdf01000.sys`) is a Microsoft
   binary TUR may not ship and Wine does not reimplement; a buggy driver
   hangs or crashes the device host that every Wine USB and HID device in the
   prefix shares.
2. **HID devices through Wine's existing path** (hidraw → `winebus.sys` →
   `hidclass.sys` → `hid.dll`), with a device grant. No Windows driver is
   involved; vendor tools that talk HID work.
3. **USB hand-off to the Windows guest** (ADR-0015, design §6.8, §10.4): the real
   Windows kernel runs the real driver, KMDF and UMDF included.
4. **Upstream fixes to Wine's USB stack** (listed in the report) so that more
   WDM drivers work, with TUR's catalogue recording which devices do.

## Decision

Proposed: (2) and (3) are the supported paths. A device grant for Wine
applications (one device, one application, revocable, shown to the person, a
class 3 action for P@x) is built on the hand-off state machine of §10.4: a
hidraw grant for HID devices first, a raw USB grant second. (1) stays an
experiment, not a feature: TUR does not promise that vendor kernel drivers run
in Wine; a vendor package may be tried in an application's prefix only behind
the static checks of the report (no KMDF/UMDF, no kernel-resource imports, no
filters) and is listed as working only per measured device. (4) is done as
ordinary upstream contribution, in the order the report gives, without a TUR
patch queue for Wine.

## Consequences

- No Windows driver runs in any kernel; nothing changes for rule 8.
- Devices with only a KMDF or UMDF driver go to the Windows guest. The
  catalogue says so per device.
- `tur-runtime` gains device grants: today it refuses device nodes in grants
  (runtime-manager.md). The grant mounts exactly one `/dev/bus/usb/BBB/DDD` or
  `/dev/hidrawN` into one application's sandbox, after the device manager has
  unbound the Linux driver if the person asked for raw USB access.
- Revocation ends the sandbox's access by disconnecting the device at its hub
  port (or ending the application); removing an ACL does not close an open
  file.

## Verification

- The windows-drivers workflow keeps measuring the Wine driver host on the
  Wine TUR ships, so that a change upstream is seen.
- Before a device grant is built: its design is checked against
  device-manager.md §8 and the windows-guest hand-off, and the revocation
  measurement of the report is repeated on real hardware.
- Each upstream fix is verified by the experiment before the catalogue claims a
  device works.
