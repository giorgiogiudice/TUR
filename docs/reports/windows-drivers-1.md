# windows-drivers-1 — Windows USB and HID drivers as user-space programs (experiment)

```text
TUR DEVELOPMENT REPORT

Task ID:          windows-drivers-1 (time-boxed experiment)
Subsystem:        Windows compatibility (Wine's driver host); device
                  manager and runtime sandbox (design only)
Build ID:         not applicable: this task builds no image
Source revision:  fed35166 (spike/userspace-windows-drivers, pull request
                  90), the commit the measurements come from (workflow run
                  43368061907). The commit that adds this report and the
                  ADR proposal follows it and changes no code.

SUMMARY
- What changed: an experiment, not a feature. tests/windows-drivers emulates
  three USB devices in user space and plugs them into the runner kernel's
  USB/IP virtual host controller, builds a minimal Windows WDM USB function
  driver, a KMDF binding probe,
  Microsoft's kmdf_fx2 sample (best effort) and test programs with
  mingw-w64, and runs them under Fedora 45's Wine in a Fedora 45 container;
  the workflow windows-drivers runs it. This report records what worked,
  what did not and why, and recommends what TUR should build.
- Why it changed: the owner asked why TUR should not try to make Windows
  drivers work. Rule 8 allows "userspace adapters"; Wine's driver host is
  the obvious one.

FILES
- Added: tests/windows-drivers/{build.sh,kmdf-fx2.sh,fedora-run.sh,
  harness.py}, tests/windows-drivers/virtual-usb/usbip_devices.py,
  tests/windows-drivers/driver/{twdusb.c,twdusb.h,twdusb.inf,kmdfprobe.c,
  kmdfprobe.inf,kmdfstub.c,wdfldr.def},
  tests/windows-drivers/app/{twdinst.c,twdtest.c,hidtest.c,winusbtest.c},
  .github/workflows/windows-drivers.yml, docs/reports/windows-drivers-1.md,
  docs/adr/0026-windows-device-drivers-in-user-space.md (Proposed)
- Modified: .github/workflows/gate.yml (windows-drivers added to the
  workflows the gate waits for)
- Removed: none

BUILD
- Command: tests/windows-drivers/build.sh and kmdf-fx2.sh with Fedora 45's
  mingw-w64, inside the workflow; nothing was built on a development machine
- Result: twdusb.sys, kmdfprobe.sys and the four programs built;
  Microsoft's kmdf_fx2 sample did not compile (section 3.6)
- Warnings: none for the drivers; five -Wmissing-field-initializers in the
  programs (structures initialised with their size field only)

TESTS
- Unit tests: none (no TUR code changed)
- Integration tests: workflow windows-drivers: runs 43361925857 (failed:
  the version listing did not find Fedora's Wine directory), 43363749643
  (failed: twdinst did not compile), 43365500575 (failed: the runner's
  kernel has no dummy_hcd), 43366132596 (pass, first full results),
  43367965703 (cancelled by the next push), 43368061907 (pass; the results
  below). Both passing runs gave the same results except for timings.
- Boot tests: NOT RUN. Nothing here can affect boot.
- Hardware tests: NOT RUN. Emulated USB devices only (USB/IP); no real
  device, no vendor driver.
- Performance tests: one loopback throughput figure, for orientation only.

DIAGNOSTICS
- Log paths: artifact "windows-drivers-evidence" of the run: results.json,
  results.md, scenarios/NN-name/ (Wine logs with +wineusb, +plugplay,
  +setupapi, +ntoskrnl, +usbd, +hid, +loaddll; excerpt.txt per scenario),
  build.log, imports.txt, versions.txt, kmdf-fx2/, kernel-probe.txt,
  devices.txt, virtual-usb/ (the emulator's log and the HID device's record
  of output reports), dmesg.txt
- Diagnostic bundle path: none
- Relevant error IDs: none (no TUR component)

RECOVERY
- Rollback procedure: revert the merge. Nothing outside tests/, the two
  workflows and docs/ changes.
- Recovery tested: not applicable

KNOWN LIMITATIONS
- See "Limits of this experiment" at the end.

NEXT SAFE ACTION
- Decide ADR-0026. If accepted: design the device grant on the hand-off
  state machine (device-manager.md section 8) and repeat the revocation and
  HID measurements on real hardware.
```

## The answer in short

- **Can a Windows USB driver run as an ordinary user-space program?** A
  plain WDM one, yes: our own minimal WDM USB function driver was installed
  from its INF, loaded by Wine's PnP manager into `winedevice.exe` (a Linux
  process running as the user), and moved data to and from a virtual USB
  device — bulk and vendor control transfers, byte-for-byte correct, about
  16 MiB/s — also inside a bubblewrap sandbox that held exactly one device.
  Never in a kernel. That is the whole positive result; it was our driver,
  written to avoid what Wine lacks, not a vendor's.
- **KMDF drivers — the model Microsoft steers new drivers to — cannot load
  at all**: they
  bind to `WDFLDR.SYS`/`Wdf01000.sys`, Microsoft binaries Wine does not
  reimplement and TUR may not ship. Measured with a probe driver. UMDF and
  WinUSB-based programs cannot run either (no host; `winusb.dll` is stubs,
  measured).
- **HID devices work today without any Windows driver**: Linux hidraw →
  Wine's `winebus.sys`/`hidclass.sys` → `hid.dll`; input and output reports
  measured end to end.
- **Wine's USB stack has defects that turn failures into hangs** (a refused
  transfer is never completed; `IoCancelIrp` does nothing; interfaces are
  never claimed) and **its installers are stubs** (`pnputil`,
  `DiInstallDriverW`). Each is a small upstream fix.
- **Granting one device to one application is safe to build**: an ACL on one
  device node plus a bind of that node into the sandbox; the device stays
  invisible to everything else; withdrawing it by disabling its hub port
  stopped the application's access at once without hanging it.
- **Recommendation**: build device grants for Wine applications (HID first,
  raw USB second) on the existing hand-off state machine; contribute the Wine
  fixes, above all a free WinUSB; do not promise vendor kernel drivers —
  KMDF, UMDF and everything else go to the Windows guest by USB hand-off.
  Section 8 has the effort.

## 1. What Wine provides for drivers, read from its source

Read at three points of Wine's tree: **wine-11.20**, what Fedora 45 ships as
stable (`wine-11.20-1.fc45`, stable since 31 October 2026 in Bodhi; Fedora
builds it with the Staging patch set) and what the experiment ran;
**wine-11.21** (`wine-11.21-1.fc45`, in updates-testing since 6 November);
and **master** cc5d1d86 (12 November 2026). From 11.20 to master nothing
changed in `dlls/wineusb.sys`, `dlls/winusb`, `dlls/hidclass.sys`,
`dlls/hid`, `dlls/usbd.sys`, `dlls/hal`, `dlls/newdev`, `programs/pnputil`
or `programs/winedevice`. 11.21 changed `SetupCopyOEMInf`'s error handling
and `winebus.sys`'s SDL signal handling; master adds device-file position
handling to `ntoskrnl.exe` and a device-interface path fix to `cfgmgr32`.
None of this bears on what follows. Wine Staging 11.20's patch sets touch none
of these files either; one, `winedevice-Default_Drivers`, adds stub
`win32k.sys`, `dxgkrnl.sys` and `dxgmms1.sys` that `winedevice.exe` loads.
Paths below are in Wine's tree.

### 1.1 The pieces

| Piece | What it is in Wine | What matters here |
|---|---|---|
| `winedevice.exe` | A Windows service program; one process per service group | All plug-and-play drivers are in the group `WinePlugPlay` (`LoadOrderGroup` in `wineusb.inf`, `winebus.inf`), and `ZwLoadDriver` loads a function driver into the process that asked for it. **A vendor driver runs in the same process as Wine's own USB and HID bus drivers**, as the user. |
| `ntoskrnl.exe` | A user-space reimplementation of the kernel's driver interface: IRPs, device objects, the I/O manager, synchronisation, a PnP manager | 1,720 exports in `ntoskrnl.exe.spec`, **726 of them `stub`** (calling one aborts the process). `hal.dll`: 92 exports, 71 stubs. `usbd.sys`: 35 exports, 24 stubs. |
| PnP manager (`ntoskrnl.exe/pnp.c`) | Enumerates bus relations, matches IDs against INFs, installs and loads function drivers | `install_device_driver()` runs `SetupDiBuildDriverInfoList` over `C:\windows\inf\*` and the class-installer steps; `load_function_driver()` calls `ZwLoadDriver` and `AddDevice`. A device with no driver is started raw if its bus says `RawDeviceOK` (wineusb does) and matched again at the next start. **Upper and lower filter drivers are never loaded** (no `UpperFilters`/`LowerFilters` handling). |
| `wineusb.sys` | A USB bus driver on **libusb** (Zebediah Figura, 2020) | A PDO for every USB device **the user can open** (`libusb_open`; others are skipped with `WARN "Failed to open device"`), one more per interface of a composite device. IDs `USB\VID_xxxx&PID_xxxx[&REV_xxxx][&MI_xx]`, `USB\Class_…`. One internal IOCTL, `IOCTL_INTERNAL_USB_SUBMIT_URB`, and eight URB functions: `SELECT_CONFIGURATION`, `BULK_OR_INTERRUPT_TRANSFER`, `GET_DESCRIPTOR_FROM_DEVICE`, `VENDOR_DEVICE`, `VENDOR_INTERFACE`, `VENDOR_ENDPOINT`, `ABORT_PIPE`, `SYNC_RESET_PIPE_AND_CLEAR_STALL` (`wineusb.c`, `unixlib.c`). It never claims an interface or detaches a Linux driver. |
| `usbd.sys` | URB helpers | `USBD_CreateConfigurationRequest[Ex]`, `USBD_Parse*` and three more are implemented. The pipe list of `SELECT_CONFIGURATION` is filled here, in the *caller*; `wineusb.sys` only turns the endpoint addresses it is handed into pipe handles, does not send `SET_CONFIGURATION` and looks at the first interface only. |
| `winusb.dll` | **Stubs**: 21 of 22 exports `stub`, `WinUsb_Free` a FIXME; no `winusb.sys` | Programs written for Microsoft's WinUSB cannot run. |
| `winebus.sys`, `winehid.sys`, `hidclass.sys`, `hid.dll` | Wine's HID stack: `winebus.sys` reads Linux **hidraw** (and evdev, and SDL) through udev; `winehid.sys` is a HID minidriver over it; `hidclass.sys` exports `HidRegisterMinidriver` | Programs use `hid.dll` (`HidD_*`, `HidP_*`), `ReadFile`/`WriteFile` and the report IOCTLs. hidraw is used for every device whose top-level usage page is not Generic Desktop, and never for mice, keyboards or touchscreens (`is_hidraw_enabled`, `winebus.sys/main.c`); a per-device `EnableHidraw` registry value overrides that. |
| Driver installation | `SetupCopyOEMInf` (copies the INF to `C:\windows\inf` under its own name, `oemN.inf` only on a clash, and the package to the driver store), `UpdateDriverForPlugAndPlayDevices` (newdev), class-installer steps, `AddService`, `CopyFiles` | Implemented. **`DiInstallDriverW` is a stub** that prints a FIXME and returns TRUE (`dlls/newdev/main.c`); **`pnputil.exe` is a stub program** that prints its arguments (`programs/pnputil/main.c`). Signatures are never checked. |
| KMDF | **Absent**: no `wdfldr.sys` or `wdf01000.sys` in the tree | A KMDF driver's stub calls `WdfVersionBind` in `WDFLDR.SYS`, which loads the framework `Wdf01000.sys` ([Microsoft: framework library versioning](https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/framework-library-versioning); prototypes in Windows-Driver-Frameworks' `fxldr.h`). Both are Microsoft binaries. Wine bug [56228](https://bugs.winehq.org/show_bug.cgi?id=56228) is this exact error with a real driver. |
| UMDF | **Absent**: no `WUDFHost.exe`, reflector `WUDFRd.sys` or `Wudfx02000.dll` | UMDF drivers are DLLs in Microsoft's host process, reached through a kernel reflector ([Microsoft: UMDF driver host process](https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/umdf-driver-host-process)). |
| Scanners | `sti.dll`, a minimal `wiaservc`, `twain_32`, `twaindsm`, and the TWAIN sources `sane.ds` and `gphoto2.ds`, which drive **Linux SANE and libgphoto2** | No `usbscan.sys`, through which WIA minidrivers and many TWAIN sources reach a USB scanner on Windows ([Microsoft: WIA architecture](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-architecture-overview)). |
| Printers | `winspool.drv` gives every printer Wine's own `wineps.drv` and prints through CUPS (`dlls/winspool.drv/info.c`) | A vendor's Windows printer driver is not used for rendering; the Linux CUPS driver is. |

### 1.2 Kernel interfaces that cannot mean in a process what they mean in a kernel

| Interface (11.21) | In Wine | Consequence |
|---|---|---|
| `IoConnectInterrupt`, `IoGetDmaAdapter`, `IoAssignResources`, `KeSynchronizeExecution`, `READ_REGISTER_ULONG`, `IoRegisterFsRegistrationChange` | `stub` | A driver that calls one aborts the driver host. |
| `MmMapIoSpace`, `MmAllocateContiguousMemorySpecifyCache` | return NULL with a FIXME | No device memory, no DMA buffers. |
| `MmGetPhysicalAddress` | "semi-stub" | Meaningless in a process. |
| `HalGetBusData[ByOffset]` | return 0 | No PCI configuration space. |
| `KeInsertQueueDpc` | **returns TRUE and never runs the DPC** | Added for the CED1401 USB driver; Wine bug [49394](https://bugs.winehq.org/show_bug.cgi?id=49394) was closed FIXED (8.0-rc2) by that stub. The driver loads and its deferred work silently never happens. |
| `KeGetCurrentIrql` (x86-64) | FIXME, returns 0 | IRQL is not modelled; `KeRaiseIrql*` in `hal.dll` are stubs. |
| `IoForwardIrpSynchronously`, `IoSetCompletionRoutineEx`, `IoAttachDeviceToDeviceStackSafe`, `ExAllocatePoolWithTagPriority` | `stub` | Common in current WDM drivers; each aborts `winedevice.exe` when called. The experiment's driver avoids all of them on purpose. |
| `ObRegisterCallbacks`, `PsSetCreateProcessNotifyRoutineEx`, `KeStackAttachProcess` | FIXME, report success, do nothing | What anti-cheat and security drivers rely on "succeeds" and does nothing. |
| `MmProbeAndLockPages` | sets a flag | Nothing is locked. |

## 2. The experiment

All of it runs in the `windows-drivers` workflow on a GitHub-hosted
`ubuntu-24.04` runner. Nothing was built or run on a development machine.

| Part | What | Where |
|---|---|---|
| Virtual USB devices | Three devices emulated in user space and plugged into the kernel's USB/IP virtual host controller (`vhci_hcd`, a stock module): **1209:0001** vendor class, bulk OUT/IN loopback, vendor control requests 0x5b/0x5c; **1209:0002** vendor class, Linux `f_sourcesink` semantics (patterned bulk IN, pattern-checked bulk OUT, 0x5b/0x5c); **1209:0003** HID, vendor usage page 0xFF00, 32-byte input and output reports on interrupt endpoints. The host kernel enumerates them as USB devices: usbfs nodes, sysfs, `usbhid` + `hidraw0` for the HID one. | `virtual-usb/usbip_devices.py` |
| Why USB/IP | The first plan, `dummy_hcd` + configfs gadgets, is impossible on these runners: their kernel (7.0.0-1017-azure) has `# CONFIG_USB_DUMMY_HCD is not set`, and `linux-modules-extra-7.0.0-1017-azure` does not exist (measured, run 1-3 logs). `vhci-hcd` and `usbip-core` are built (`CONFIG_USBIP_VHCI_HCD=m`). A VM would also have worked (`/dev/kvm` is present) but would have put Wine in a different kernel from the one in the probe; USB/IP keeps one kernel and no image. | `kernel-probe.txt` |
| Wine | Fedora 45's stable Wine, **wine-11.20 (Staging)**, `wine-core-11.20-1.fc45` (new WoW64 build, PE files in `/usr/lib64/wine-wow64/wine/x86_64-windows`), libusb1-1.0.30, bubblewrap 0.13.1, in the container `registry.fedoraproject.org/fedora:45` pinned by digest. wine-11.21 is in updates-testing (Bodhi). | `versions.txt` |
| Driver | **twdusb.sys**: a WDM USB function driver written for this experiment (about 500 lines of C), built with Fedora's mingw-w64 (gcc 16.2.1, headers 14.0.0) as a native-subsystem PE that imports only `ntoskrnl.exe` and `USBD.SYS`. AddDevice, START_DEVICE (device and configuration descriptors, `SELECT_CONFIGURATION`), a device interface, IOCTLs for one bulk/interrupt or vendor-control transfer with a time-out. | `driver/twdusb.c`, `.inf` |
| KMDF | **kmdfprobe.sys**: imports `WdfVersionBind` from `WDFLDR.SYS` and calls it from `DriverEntry`, as the WDK's stub does in every KMDF driver. **kmdf_fx2**: Microsoft's OSR USB FX2 KMDF sample (Windows-driver-samples, **MS-PL**, commit d49bb7d7) against the KMDF 1.33 headers of Windows-Driver-Frameworks (**MIT**, commit b6191d95), fetched in CI, never committed; WPP and ETW generated files replaced by empty stand-ins. | `driver/kmdfprobe.c`, `kmdf-fx2.sh`, `driver/kmdfstub.c` |
| Programs | `twdinst.exe` (installs with `SetupCopyOEMInf` / `UpdateDriverForPlugAndPlayDevices` / `DiInstallDriverW`, lists devices), `twdtest.exe` (finds the driver's device interface, transfers data), `hidtest.exe` (HID through `hid.dll`), `winusbtest.exe` (calls `WinUsb_Initialize`). | `app/` |
| Harness | Runs every scenario in its own Wine prefix (or says which it reuses) as an unprivileged user `tur`; grants device access with POSIX ACLs (what logind's `uaccess` does) as root; Wine debug channels `+wineusb,+plugplay,+setupapi,+usbd,+hid,+loaddll,+ntoskrnl,warn+debugstr`. | `harness.py` |

No Microsoft binary was downloaded or used. No winetricks verb ran. The only
kernel module loaded was `vhci-hcd` (and its dependency `usbip-core`).

## 3. What worked and what did not

Workflow run 43368061907 on commit fed35166 (artifact
`windows-drivers-evidence`). Every line is a measurement; "expected FAIL"
marks a negative result the experiment set out to show.

### 3.1 A WDM USB function driver in Wine's driver host: works

| Step | Result | Evidence |
|---|---|---|
| Install the package: `SetupCopyOEMInfW` | **Pass.** Wine keeps the INF's own name (`C:\windows\inf\twdusb.inf`, `oemN.inf` only on a clash) and stages the package in the driver store (`twdusb.inf_1`) | `02-install-no-grant` |
| Device appears without access | **No** (expected): `wineusb.sys` logs `Failed to open device: Access denied (insufficient permissions)`, Wine's PnP manager lists no 1209 device, `twdtest` finds nothing | `02-install-no-grant` |
| With an ACL on `/dev/bus/usb/001/002` | `wineusb.sys` creates `USB\VID_1209&PID_0001\…`; setupapi: `Found compatible driver: rank 0x1 … "TUR test USB device (loopback gadget)"`; the service `twdusb` is installed, `twdusb.sys` copied to `system32\drivers`, loaded into `winedevice.exe`; **DriverEntry, AddDevice, START_DEVICE all ran** | `03-wdm-loopback`, `twdinst-devices.out` |
| START_DEVICE: GET_DESCRIPTOR (device, configuration ×2), SELECT_CONFIGURATION | **Pass**: start step 6 of 6, status 0; pipes 0x01 and 0x81, bulk, 512 bytes | `twdtest-loopback.out` |
| Program opens the device interface (`SetupDiGetClassDevs` + `CreateFile`) and talks to the driver (`DeviceIoControl`) | **Pass** | |
| Bulk OUT and IN, 64, 500 and 4096 bytes, compared byte for byte | **Pass** | |
| 200 round trips of 4096 bytes | **Pass**, 200 of 200 intact; 0.474 ms per round trip, 16,879 KiB/s without debug output (with full tracing: 0.612 ms; the earlier passing run: 0.391 ms, 20,470 KiB/s). Through USB/IP on one machine: says the path adds little, not what real hardware does | `04-wdm-loopback-quiet` |
| Vendor control OUT (0x5b, 64 bytes) and IN (0x5c) on 1209:0002 | **Pass**, same 64 bytes back | `05-wdm-sourcesink` |
| Patterned bulk IN 4096 bytes / bulk OUT 4096 bytes checked by the device | **Pass** | |
| A bulk IN with nothing to read, 1 s time-out: `IoCancelIrp` | **Does not end it**: `wineusb.sys` sets no cancel routine | `timeout-ends` |
| … then `URB_FUNCTION_ABORT_PIPE` | **Ends it**: `USBD_STATUS_CANCELED` (0xC0010000); the pipe works afterwards | `bulk-echo-after-timeout` |

### 3.2 Inside a sandbox, with one device: works, and isolates

`07-sandbox-one-device`: the same prefix run inside bubblewrap with
`tur-runtime`'s namespaces and mounts (`bwrap_argv` in `harness.py`) plus
`--dev-bind /dev/bus/usb/001/002` only. The user had ACLs on **both** vendor
devices.

| | Result |
|---|---|
| USB device nodes inside the sandbox | exactly `/dev/bus/usb/001/002` |
| `twdusb.sys` on the granted device, every transfer of 3.1 | **Pass**, 0.583 ms per round trip (with full tracing) |
| The other device (allowed by ACL, not bound into the sandbox) | **Not found** (expected): `twdtest` exit 3 |
| `winebus.sys` and the HID device in the same sandbox | `Unable to open udev device "/dev/hidraw0": No such file or directory` — not granted, not seen |

`libusb` needed nothing more than the read-only `/sys` that `tur-runtime`
already mounts; no `/run/udev`.

### 3.3 Withdrawing the device while the application runs: works

`13-revoke-while-running`: a sandboxed soak (64-byte echoes in a loop), then,
from outside, the ACL removed and **the hub port disabled**
(`usb1-port1/disable`). The device left the bus, `wineusb.sys` removed its
PDO, and the next request failed with `STATUS_DELETE_PENDING` (0xC0000056)
after 10,062 round trips. The application did not hang: the sandbox exited
0.2 s after the revocation. Removing the ACL alone would not have done it:
an open file keeps its access.

### 3.4 A device that a Linux driver already has: fails, and hangs

`12-raw-usb-vs-linux-driver`: `twdusb.sys` bound to the HID device's USB
node while Linux's `usbhid` had its interface.

| | Result |
|---|---|
| Descriptors, SELECT_CONFIGURATION | Pass (control transfers to the device need no interface claim) |
| Interrupt IN transfer | **Fail.** usbfs refuses the implicit claim; `wineusb.sys` logs `err:wineusb:usb_submit_urb Failed to submit bulk transfer: Input/Output Error` and **leaves the IRP pending for ever**. `IoCancelIrp` and `ABORT_PIPE` cannot end it; the driver gave up after its time-outs (`STATUS_IO_TIMEOUT`, by design of the test driver) |
| After `echo 1-3:1.0 > /sys/bus/usb/drivers/usbhid/unbind` | **Pass**: the interrupt IN transfer returned the device's input report (`TURHID…`) |

The kernel logged `usbfs: process … (wine_sechost_se) did not claim
interface 0 before use` for every device Wine used: `wineusb.sys` never
claims interfaces.

### 3.5 HID through Wine's own stack: works, no Windows driver involved

`11-hid-winebus`: without an ACL on `/dev/hidraw0`, `hidtest` finds no device
(expected). With it: `HidD_GetAttributes` 1209:0003, `HidP_GetCaps` usage
page 0xFF00, input and output reports 33 bytes (report ID + 32), one input
report read (`TURHID…`), one output report written, and **the device
received it** (`TUROUT-from-wine` in `virtual-usb/hid-output.log`). Linux's
`usbhid` stayed bound throughout.

### 3.6 What does not work

| | Result | Evidence |
|---|---|---|
| A **KMDF** driver (`kmdfprobe.sys`) | **Does not load.** `err:module:import_dll Library WDFLDR.SYS (which is needed by L"C:\\windows\\system32\\drivers\\kmdfprobe.sys") not found`, `ZwLoadDriver failed … c0000142`, `load_function_driver Failed to load driver L"kmdfprobe", status 0xc0000142`. Its `DriverEntry` never ran. The package itself installed (service created). | `09-kmdf-probe` |
| Microsoft's **kmdf_fx2** sample with mingw-w64 | **Does not compile**: 20 errors in its five files, from four causes — the KMDF 1.33 headers need declarations mingw-w64's DDK lacks (`wdfdevice.h:4374: parameter 3 ('RequestType') has incomplete type`), mingw-w64's USB headers predate `USBD_CLIENT_CONTRACT_VERSION_602`, the WDK's IRQL annotations (`_IRQL_limited_to_`) are missing, and `size_t`/`SIZE_T` pointer mismatches. Porting it is possible, but it would then stop where `kmdfprobe.sys` stops; time-boxed out. | `kmdf-fx2/summary.txt`, `10-kmdf-fx2` |
| A **WinUSB** program | **Crashes**: `wine: Call from … to unimplemented function winusb.dll.WinUsb_Initialize, aborting` | `06-winusb` |
| **pnputil** `/add-driver … /install` | **Does nothing**: `fixme:pnputil:wmain stub` | `08-pnputil-diinstall` |
| **DiInstallDriverW** | **Does nothing, reports success**: `fixme:setupapi:DiInstallDriverW … stub!`; no package staged, the device got no driver | `08-pnputil-diinstall` |

## 4. Defects found in Wine's USB stack

Measured above or read in the source, with the line of evidence:

1. **A refused submit hangs the request.** `unixlib.c` returns
   `STATUS_PENDING` after `libusb_submit_transfer` fails, so the IRP is never
   completed (3.4).
2. **No cancel routine.** `IoCancelIrp` on a queued URB does nothing; only
   `URB_FUNCTION_ABORT_PIPE` ends a transfer (3.1). Frameworks and most WDM
   drivers time out with `IoCancelIrp`.
3. **No interface claim.** Relies on usbfs's implicit claim; a Linux driver
   in the way turns into defect 1 (3.4; kernel warnings in `dmesg.txt`).
4. **Eight URB functions.** No class requests, no generic control transfer,
   no `SELECT_INTERFACE`, no isochronous transfers, no port IOCTLs; anything
   else is `FIXME("Unhandled function")` and `STATUS_NOT_IMPLEMENTED`
   (`wineusb.c`, `unixlib.c`).
5. **`SELECT_CONFIGURATION` sends nothing** and handles only the first
   interface; pipe information comes from the caller's `usbd.sys` helper.
6. **`winusb.dll` is stubs**, and calling one aborts the program (3.6).
7. **`pnputil` and `DiInstallDriverW` are stubs** that report success (3.6).
8. **One shared driver host.** The PnP manager loads a function driver with
   `ZwLoadDriver` into the process that enumerated the device
   (`load_function_driver` in `pnp.c`): the `WinePlugPlay` `winedevice.exe`
   that also holds `wineusb.sys`, `winebus.sys` and `hidclass.sys`. A crash
   in a vendor driver stops every USB and HID device of the prefix. (Read in
   the source; the logs are consistent with it but name threads, not
   processes.)
9. **Kernel stubs that succeed**: `KeInsertQueueDpc` never runs the DPC;
   `ObRegisterCallbacks` and process-notify routines do nothing (section
   1.2).

## 5. Which Windows drivers can work this way

| Class | In Wine's driver host | Why |
|---|---|---|
| **WDM USB function driver** of a vendor device, plain IRP/URB code, importing only `ntoskrnl.exe`, `hal.dll`, `usbd.sys` | **Can work** — measured with our own driver (3.1–3.3) — if it uses only the eight URB functions `wineusb.sys` handles, none of the stubbed kernel calls, no `IoCancelIrp` on transfers, and the device has no Linux driver bound | Class requests, alternate settings, isochronous transfers, port reset/cycle, filters and `IoForwardIrpSynchronously` all fail (sections 1.2, 4). How many vendor drivers in the field fit is not known. Microsoft's guidance sends new drivers to UMDF or KMDF and to WDM only "in the rare case" a framework is not enough ([Choose a driver model](https://learn.microsoft.com/en-us/windows-hardware/drivers/gettingstarted/choosing-a-driver-model)), so the fitting ones are mostly older drivers. |
| **KMDF** driver (USB or other) | **No** | `WDFLDR.SYS` and `Wdf01000.sys` are Microsoft binaries that TUR may not ship; measured (3.6). |
| **UMDF** 1/2 driver | **No** | No `WUDFHost.exe`, reflector or framework DLL. |
| **WinUSB**-based vendor tool (no vendor driver; the program uses `winusb.dll`) | **No today; yes in principle** | `winusb.dll` is stubs (3.6). A Wine `winusb.dll` + `winusb.sys` over `wineusb.sys` needs no Microsoft file (section 9, fix 8). |
| **HID** device used by a vendor tool through `hid.dll` | **Yes**, no Windows driver involved | hidraw → `winebus.sys` → `hidclass.sys`; measured (3.5). Mice, keyboards and touchscreens are deliberately not given to Wine as hidraw. |
| **HID minidriver** (a vendor `.sys` calling `HidRegisterMinidriver`) | **Possibly**, WDM ones over a `wineusb.sys` PDO (Wine's own `winehid.sys` is one, over `winebus.sys`); **no** for KMDF HID minidrivers (Microsoft's `mshidkmdf.sys` + KMDF) | Not measured. |
| **Scanner**: TWAIN data source or WIA minidriver | **Mostly no** | User-mode code, but it reaches the scanner through `usbscan.sys` or a vendor kernel driver, neither of which Wine has. Linux SANE through Wine's `sane.ds` is the working path. |
| **Printer** driver | **No, and not needed** | Wine prints through CUPS with its own `wineps.drv`. |
| USB-serial adapters (FTDI, CH34x, CP210x, CDC-ACM) | **No Windows driver needed** | Linux drivers; Wine maps `COMn` to `/dev/ttyUSBn` / `ttyACMn`. |
| Anything needing interrupts, DMA, I/O ports, PCI resources, physical memory | **No** | Stubs or NULL (section 1.2); a process cannot have them. |
| Storage, file system, network (NDIS), graphics (WDDM), audio (PortCls), Bluetooth stacks | **No** | Whole kernel stacks; Linux owns that hardware (rule 8, §10). |
| Filter drivers (upper/lower device filters, file-system minifilters) | **No** | Wine's PnP manager loads no filters. |
| Anti-cheat, DRM, endpoint security | **No, and must not be tried** | Their kernel callbacks are FIXMEs that report success: the driver is lied to. (Fedora's Wine Staging adds stub `win32k.sys`, `dxgkrnl.sys`, `dxgmms1.sys` loaded by `winedevice.exe` — "Simulate a more realistic kernel environment" — which changes nothing here.) |

## 6. How a USB device reaches the Windows driver, and how TUR would grant one

### 6.1 The path

```text
USB device ── Linux xHCI/EHCI + usbcore ── /dev/bus/usb/BBB/DDD (usbfs)
        └─ (or) a Linux driver, e.g. usbhid ── /dev/hidrawN
                         │ libusb (wineusb.so)            │ libudev + read/write (winebus.so)
                         ▼                                ▼
          wineusb.sys (PDO per device/interface)   winebus.sys ── winehid.sys ── hidclass.sys
                         │ IRP_MJ_INTERNAL_DEVICE_CONTROL  │
                         ▼                                ▼
          vendor function driver (.sys)           hid.dll in the application
          all inside winedevice.exe, one Linux process, as the user
                         │ device interface: CreateFile/DeviceIoControl
                         ▼
          the vendor's application (another Wine process)
```

- **libusb access** is ordinary file access to `/dev/bus/usb/BBB/DDD`
  (usbfs). udev makes those nodes `root:root 0664` unless a rule or systemd's
  `uaccess` tag gives the seat user an ACL; `wineusb.sys` opens read-write,
  so without an ACL the device is simply not there for Wine (measured,
  section 3). On a normal desktop **Wine already exposes every USB device the
  user has an ACL on** (phones, cameras, security keys, game controllers):
  there is no per-application selection in Wine itself.
- **A Linux driver bound to an interface** (usbhid, cdc_acm, usb-storage)
  owns it: usbfs can claim an interface only when no driver has it.
  `wineusb.sys` never claims or detaches (it relies on usbfs's implicit claim
  at the first transfer), so the transfer fails and, because of the
  submit-error bug of section 4, the request hangs instead of failing
  (measured, section 3). To give a Windows USB driver such a device, the
  Linux driver must be unbound first (`/sys/bus/usb/drivers/<d>/unbind`),
  which needs root: a job for `tur-device-manager`, never for the
  application.
- **HID devices** need no unbinding: the Linux usbhid driver stays, and the
  application reaches the device through hidraw (`/dev/hidrawN`, also
  `root:root 0600` by default).

### 6.2 A device grant for a Wine application (design sketch)

The sandbox of `tur-runtime` (runtime-manager.md, "What the application
sees") already has the right shape: a minimal `/dev`, read-only `/sys/bus`,
`/sys/class`, `/sys/dev`, `/sys/devices`, no `/run`. It refuses device nodes
as grants today. The grant adds exactly one node:

1. **Request.** The person picks the device in the application's settings
   page ("Devices this application may use"), or accepts a prompt raised
   when the application's catalogue entry lists a device it needs (by
   VID/PID). P@x may *propose* a grant (it can read the device inventory,
   class 0) but **granting is class 3** (AI control layer §5): it is
   confirmed in a TUR dialog P@x cannot operate, showing the device's name,
   VID:PID, serial number and port, and the application.
2. **Hand-off.** `tur-device-manager` runs the state machine of §10.4
   (device-manager.md §8) with the application in the place of the guest:
   `available → handoff_requested → host_driver_unbound` (only for a raw USB
   grant of a device a Linux driver has; never for HID through hidraw, never
   for the keyboard or pointer in use, hubs, internal devices or storage with
   a mounted volume — the refusals of windows-guest.md §6.8 apply unchanged)
   `→ guest_attached` (an ACL for the user on that one node) `→ guest_owned`.
3. **Sandbox.** `tur-runtime` starts the application with `--dev-bind
   NODE NODE` for that node and no other; nothing else in `/dev/bus/usb` or
   `/dev/hidraw*` exists inside. Measured in section 3: a second device the
   user *had* an ACL for stayed invisible to Wine in the sandbox.
4. **Visible.** While granted, the device's entry in the device inventory
   and the application's window both say so; the grant is logged at each
   transition (§10.4).
5. **Revocation.** Removing the ACL does not close an open file. A revoke
   (by the person, at application exit, or when the device is unplugged)
   ends the application's access by **disconnecting the device at its hub
   port** (`usbN-portM/disable`; re-enabling it should bring the device back
   as a new device with a new node) or by ending the sandbox; then the Linux
   driver is rebound (`host_rebind_attempted → available`). The disable was
   measured (3.3) on the virtual hub; re-enabling and rebinding were not.
6. **Persistence.** A grant can be remembered per application and device
   (VID:PID plus serial number), never by bus/device number, which changes on
   every plug.

The same state machine already serves the Windows guest's USB hand-off
(windows-guest.md §6.8), so a device is never owned by a Wine application
and the guest at once.

## 7. Security: what a driver can do in this model

| | In the Windows kernel (or a VM's) | In Wine's driver host on TUR |
|---|---|---|
| Runs as | Ring 0, the whole machine | The user's uid, inside the application's sandbox |
| Memory it can touch | All of it | Its own process: `winedevice.exe` of that prefix — which also holds `wineusb.sys`, `winebus.sys` and every other driver loaded in that prefix |
| Hardware | Every device, I/O ports, DMA | The device nodes in the sandbox: with a grant, one USB device (any request usbfs allows, including control requests to the device and, if it supports it, rewriting its firmware) |
| Files | All | The prefix, the private home, granted directories (runtime-manager.md) |
| Network | All | Whatever the sandbox allows (shared by default) |
| Other processes | All | None outside the sandbox; inside, the Wine processes of that prefix |
| Persistence | Boot-start driver | The prefix only |
| A crash | A stop error | `winedevice.exe` dies: every Wine-driven USB and HID device of that prefix stops; the rest of the system is unaffected (from the process model; a crashing driver was not measured) |

What remains real in the user-space model:

- **The device itself.** A malicious driver with a raw USB grant can reflash
  a device's firmware (a USB stick, a keyboard), turning it into a hostile
  device for the next machine; that is the same risk as the Windows guest's
  hand-off (windows-guest.md, "Device returned from the guest is hostile").
- **The prefix.** Everything the application could do, the driver can do: it
  is just more code in the same sandbox.
- **Shared driver host.** All plug-and-play drivers of a prefix share one
  process; one prefix per vendor application (TUR's default) keeps one
  vendor's driver away from another's.
- **Lies.** Kernel-only functions that Wine fakes (`ObRegisterCallbacks`,
  process-notify routines, `KeInsertQueueDpc`) succeed and do nothing. A
  driver that depends on them believes it is protected or working when it is
  not.

What TUR must refuse, before installing a driver package into a prefix (from
the INF and the PE imports, both readable without running anything):

- KMDF and UMDF packages (`[*.Wdf]` sections, imports of `WDFLDR.SYS`,
  `UmdfService`): they cannot load; send the person to the Windows guest.
- Imports of the hardware-resource and kernel-hook functions of section 1.2
  (`IoConnectInterrupt`, `IoGetDmaAdapter`, `MmMapIoSpace`,
  `HalGetBusData*`, `ObRegisterCallbacks`, `PsSet*NotifyRoutine*`,
  `KeStackAttachProcess`, `FltRegisterFilter`, `NdisM*`): a driver that
  wants interrupts, DMA, another process's memory or a filter position is not
  a USB function driver.
- Boot- and system-start services, filter-driver registrations
  (`UpperFilters`/`LowerFilters`, `LoadOrderGroup` other than PnP), and any
  class other than the device's own (no `Class=Net`, `DiskDrive`, `Display`,
  `Volume`, `SCSIAdapter`).
- A raw USB grant of an input device in use, a hub, internal hardware or
  mounted storage (the hand-off refusals above).

None of this is a substitute for the sandbox: a driver that passes these
checks is still untrusted code, contained by the sandbox and the one-device
grant, not by the checks.

## 8. Recommendation

**Yes, try it — but as two narrow features, not as "Windows drivers work on
TUR".** The measurements show that Wine's driver host is real: a WDM USB
driver installed from its INF, was loaded by Wine's PnP manager, moved data
over bulk and control transfers at about 16 MiB/s on a virtual device, ran in a
sandbox with exactly one device, and stopped cleanly when the device was
withdrawn. They also show where it ends: no KMDF (the framework Microsoft
steers new drivers to), no UMDF, no WinUSB, no class requests, and failure
modes that hang instead of failing.

| Option | What | Verdict | Effort (one developer) |
|---|---|---|---|
| **A. Device grants for Wine applications** | The design of 6.2: one device, one application, through the hand-off state machine; hidraw grants for HID devices, raw USB grants (with the Linux driver unbound) for WDM drivers and for Windows programs that talk to a device through Wine | **Build it, HID first.** HID needs no Windows driver and works today (3.5); raw USB grants serve the WDM drivers that fit and every future Wine USB improvement | 4–6 weeks: privileged grant helper in `tur-device-manager` (ACL, unbind/rebind, port-disable revocation) 2 weeks; `tur-runtime` grant (schema, `--dev-bind`, record, refusal rules) 1 week; Settings page and prompt 1 week; CI with this USB/IP harness and the hand-off tests 1–2 weeks. Plus real-hardware validation when a hardware runner exists. |
| **B. Vendor kernel drivers in Wine** as a supported feature | Install the vendor's `.sys` package into the application's prefix | **Not as a feature.** Allow it only as an *experimental* path, behind the static checks of section 7, for packages the catalogue lists as tested; send everything else to the guest | Static package check (INF + PE imports) 1 week; catalogue fields for devices (VID:PID, path: hidraw / raw USB + driver / guest, package hash, Wine version, result, date) 1 week; then per-device testing, which needs real devices |
| **C. Upstream fixes to Wine** | Section 9 | **Do it**, in that order: fixes 1–3 and 7 (small, make failures fail instead of hang, make installers work), then **fix 8, a Wine WinUSB**, probably the change that would make the most vendor tools work with no vendor driver at all (an expectation; not measured) | 1–3 + 7: 2–3 weeks; 8: 4–8 weeks; 4–6: 3–5 weeks; review time upstream on top |
| **D. USB hand-off to the Windows guest** | ADR-0015, windows-guest.md 6.8 | **Keep it as the answer for everything else** (KMDF, UMDF, devices with filter drivers, anything that fails in A/B). Already designed; no new work from this report | In the guest plan (milestone W7) |

**P@x's role.** P@x reads the device inventory (class 0) and the catalogue,
and tells the person which path a device can take and why ("this scanner's
driver is KMDF: it needs the Windows guest"; "this keyboard's tool talks HID:
it can use the device directly"). It can *propose* a grant, a hand-off, or
installing a vendor package into a prefix; each is confirmed by the person in
a TUR dialog P@x cannot operate (a grant or hand-off is class 3). It can read
the experiment's kind of evidence (Wine's `+wineusb,+plugplay` log, the
static check's verdict) to explain a failure, and offer — never send — a
problem report.

**Catalogue of tested devices.** One record per device and path, with what
was measured and on which Wine version; "works" is written only from a
measurement on that device. The `windows-drivers` workflow keeps running on
Wine updates so that a regression in the paths above is seen.

The ADR proposal is
[ADR-0026](../adr/0026-windows-device-drivers-in-user-space.md) (Proposed).

## 9. Fixes worth contributing to Wine upstream

Each is a merge request to Wine (LGPL-2.1), in its tree, by its rules; TUR
carries no Wine patch queue. Ordered by what they unlock per unit of work.
The estimates are for one developer who knows Wine's driver code, and include
Wine's own tests (`dlls/ntoskrnl.exe/tests` has a test driver framework that
loads `.sys` files under Wine and Windows, which is how these would be
tested).

| # | Fix | Where | Why | Effort |
|---|---|---|---|---|
| 1 | Complete the IRP when `libusb_submit_transfer` fails, with a `USBD_STATUS` from the libusb error (`LIBUSB_ERROR_BUSY` → the interface is someone else's; `NO_DEVICE` → gone) | `wineusb.sys/unixlib.c`, the three `submit` paths | Today the IRP stays pending for ever: a driver that waits hangs its thread (measured, 3.4) | 1–2 days |
| 2 | A cancel routine for queued URB IRPs (`IoSetCancelRoutine` → `libusb_cancel_transfer`) | `wineusb.sys/wineusb.c` | `IoCancelIrp` is how WDM drivers and every framework time out a transfer; today only `URB_FUNCTION_ABORT_PIPE` ends one (measured) | 2–3 days |
| 3 | Claim interfaces explicitly (`libusb_claim_interface` at `SELECT_CONFIGURATION`, release at removal) and report a Linux driver in the way instead of relying on usbfs's implicit claim | `wineusb.sys` | A clear failure instead of a hang; also stops the kernel's "did not claim interface before use" warnings | 2–3 days |
| 4 | Class requests (`URB_FUNCTION_CLASS_DEVICE/INTERFACE/ENDPOINT`), `URB_FUNCTION_CONTROL_TRANSFER[_EX]`, `GET_DESCRIPTOR_FROM_INTERFACE/ENDPOINT`, `GET_STATUS_*`, `SET/CLEAR_FEATURE_*`, `GET_CONFIGURATION/INTERFACE` | `wineusb.sys` | All are plain control transfers libusb already does; class requests are needed by every CDC, DFU, printer-class and audio-class driver | 1–2 weeks with tests |
| 5 | `SELECT_INTERFACE` (alternate settings, `libusb_set_interface_alt_setting`), `SELECT_CONFIGURATION` for all interfaces and with `SET_CONFIGURATION` when the configuration differs | `wineusb.sys` | Devices with more than one alternate setting: Wine logs a FIXME and uses the first (`unixlib.c`) | 1 week |
| 6 | `IOCTL_INTERNAL_USB_GET_PORT_STATUS`, `RESET_PORT`, `CYCLE_PORT`, `GET_BUS_INFO`, `IRP_MN_QUERY_INTERFACE` for `USB_BUS_INTERFACE_USBDI_GUID` | `wineusb.sys` | Asked for by most drivers at start; `USBD_CreateHandle`/`USBD_QueryUsbCapability` (`usbdex.lib`) depend on the bus interface | 1–2 weeks |
| 7 | `pnputil /add-driver … /install` and `DiInstallDriverW` doing what `twdinst` does (`SetupCopyOEMInf` + `UpdateDriverForPlugAndPlayDevices`) | `programs/pnputil`, `dlls/newdev` | Vendor installers call these (or DIFx's `dpinst.exe`, bug [42424](https://bugs.winehq.org/show_bug.cgi?id=42424)); today they report success and install nothing (measured) | 3–5 days |
| 8 | A Wine `winusb.dll` with a Wine-written `winusb.sys` function driver over `wineusb.sys` (bound by INF `Include=winusb.inf`, and by Microsoft OS descriptors, as on Windows) | `dlls/winusb`, new `dlls/winusb.sys` | **Probably the largest win**: WinUSB is Windows' generic driver that lets a program use a USB device "without the need to write a custom driver" ([Introduction to WinUSB for developers](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/introduction-to-winusb-for-developers)); every vendor tool built on it needs no vendor driver, and with this it would run under Wine with no Microsoft file. How many tools that is was not measured | 4–8 weeks |
| 9 | Isochronous transfers | `wineusb.sys` | Audio/video-class and some instruments | 2–3 weeks |
| 10 | `KeInsertQueueDpc` that runs the DPC (on a worker thread), `IoForwardIrpSynchronously`, `IoSetCompletionRoutineEx`, `IoAttachDeviceToDeviceStackSafe` | `ntoskrnl.exe` | A stub that returns success while doing nothing is worse than a missing function | 1 week |
| 11 | One driver host process per function driver (or per vendor service group), not one shared `WinePlugPlay` process | `ntoskrnl.exe/pnp.c`, `programs/winedevice` | A vendor driver's crash then takes down only its own device, and its memory is not shared with Wine's bus drivers | 2–4 weeks; needs agreement upstream first |

Not proposed: a free KMDF (`WDFLDR.SYS` + `Wdf01000.sys`) for Wine. The
framework's source is public under MIT
([Windows-Driver-Frameworks](https://github.com/microsoft/Windows-Driver-Frameworks)),
but it is built with the WDK for the Windows kernel, its loader and stub are
not in it, and making it run on Wine's `ntoskrnl.exe` would mean
implementing most of the kernel interfaces of section 1.2 that cannot be
implemented in a process. That is not a fix; it is a second Windows kernel.

## 10. Limits of this experiment

- **Virtual devices, not real ones.** The devices are emulated in user space
  (`virtual-usb/usbip_devices.py`) and reach the runner's kernel through
  USB/IP's virtual host controller. The runner's kernel (linux-azure 7.0) is
  built without `dummy_hcd` (`# CONFIG_USB_DUMMY_HCD is not set`, and no
  extra-modules package exists for it), so the kernel's own gadget framework,
  the first plan, could not be used. To the host's USB core, usbfs, libusb,
  usbhid and hidraw, a vhci device is an ordinary USB device; timing, error
  behaviour and power management are not those of hardware.
- **Our driver, not a vendor's.** The WDM driver was written for this
  experiment and deliberately avoids the kernel functions Wine stubs. A real
  vendor driver uses more of the kernel, and of the USB stack, than it does;
  that it works says Wine *can* host such a driver, not that a given vendor
  driver will run. No proprietary driver was tried (the task forbids
  downloading one).
- **One Wine build.** Fedora 45's stable Wine, 11.20 with the Staging patch
  set, in a Fedora 45 container on an Ubuntu 24.04 runner kernel. The source
  was read at 11.20, 11.21 (in Fedora 45 updates-testing) and master; the
  driver-related code is the same in all three, except as noted in section 1.
- **KMDF**: the binding probe shows where every KMDF driver stops; the
  kmdf_fx2 sample build is best-effort with stand-ins for the WDK's
  generated files.
- **Not measured**: a HID minidriver (`HidRegisterMinidriver`) over a
  `wineusb.sys` PDO; isochronous devices; composite devices; hot-plug while
  an application runs (only removal was measured); suspend/resume; a 32-bit
  driver (Fedora's Wine is a WoW64 build; 32-bit Windows kernels and drivers
  are out of scope); real hardware of any kind.
- **The sandbox** is shaped like `tur-runtime`'s from its documentation
  (`harness.py`, `bwrap_argv`), not produced by `tur-runtime`, which has no
  device grant to produce it with.
