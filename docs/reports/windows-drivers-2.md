<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# windows-drivers-2 — KMDF drivers in Wine's user-space driver host, runtime built from Microsoft's MIT source (experiment, in progress)

> **Status: in progress.** This report is updated with every CI iteration of
> pull request 96 so that the findings are never only in a CI log. Sections
> marked *pending* have no measurement yet.

```text
TUR DEVELOPMENT REPORT

Task ID:          windows-drivers-2 (time-boxed experiment, about two days of
                  CI iterations)
Subsystem:        Windows compatibility (Wine's driver host); no TUR service
Build ID:         not applicable: this task builds no image
Source revision:  spike/kmdf-userspace (pull request 96); the iteration a
                  number comes from is named next to it

SUMMARY
- What changed: an experiment, not a feature. CI builds the Kernel-Mode
  Driver Framework runtime (Wdf01000.sys) from Microsoft's MIT-licensed
  Windows-Driver-Frameworks source with an open toolchain (clang, lld,
  mingw-w64 headers), builds Wine 11.20's ntoskrnl.exe, wineusb.sys and a
  new wdfldr.sys (the KMDF loader) with a sixteen-patch Wine patch set, and
  runs KMDF drivers against emulated USB devices in Fedora 45's Wine.
- Why: experiment 1 (windows-drivers-1) found that every KMDF driver fails
  to load in Wine because WDFLDR.SYS and Wdf01000.sys are Microsoft
  binaries. Microsoft publishes the framework's source under MIT.

FILES
- Added: third-party/wdf/** (MIT: Microsoft's LICENSE, patches, compat/
  stand-ins, sources.txt), third-party/wine-patches/** (LGPL-2.1-or-later:
  COPYING.LIB, sixteen patches, series), tests/windows-drivers/kmdf/**,
  docs/reports/windows-drivers-2.md
- Modified: .github/workflows/windows-drivers.yml (jobs kmdf-build,
  kmdf-run; path triggers), tests/windows-drivers/virtual-usb/
  usbip_devices.py (FX2-like device, --devices), docs/dependencies/
  upstream-projects.md (three rows, sources S86 and S87)
- Removed: none

BUILD
- Command: tests/windows-drivers/kmdf/ci-build.sh in a Fedora 45 container,
  in CI only; nothing was built on a development machine
- Result: Wine modules build; Wdf01000.sys (1,507,328 bytes), kmdftest.sys
  and Microsoft's osrusbfx2.sys build and load (sections 3 and 6)

TESTS
- Integration: workflow windows-drivers, all runs green (a failed build is a
  finding, not a failed job); the kmdf-run job runs the harness
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (emulated USB/IP devices only)

KNOWN LIMITATIONS
- See section 9.

NEXT SAFE ACTION
- Continue the run iterations past WdfDriverCreate; see section 8.
```

## The answer so far

- **Iteration 11 (run 43395168698, commit `97d5434`): the goal is met for
  the minimal driver and largely for Microsoft's sample.** No crash, no
  unimplemented call in either scenario.
  - `kmdftest.sys`: every check PASS (28 of 28 in the scenario): load,
    bind, `DriverEntry` → `WdfDriverCreate` → `EvtDriverDeviceAdd` →
    `WdfDeviceCreate` → `EvtDevicePrepareHardware` (USB target, descriptor,
    configuration; pipes bulk IN 0x81, bulk OUT 0x01) → `EvtDeviceD0Entry`;
    IOCTLs answered; **`WDFDPC`, `WDFTIMER` (fired after 100 ms) and
    `WDFWORKITEM` ran**; **bulk echo of 64 and 512 bytes** through the
    loopback device and a **vendor control transfer** (32 bytes) worked.
  - Microsoft's unmodified `kmdf_fx2` sample on the FX2-like device:
    `DriverEntry`, `EvtDriverDeviceAdd`, **`EvtDevicePrepareHardware`**
    (pipes selected: interrupt, bulk out, bulk in), **`EvtDeviceD0Entry`**
    ("coming from WdfPowerDeviceD3Final"), its continuous reader on the
    interrupt pipe received the switch changes ("SwitchState 2"), and **its
    test program read and wrote**: `testapp -u` dumped the descriptors,
    `testapp -r 64 -w 64 -c 10` wrote and read back 64-byte buffers
    (exit 0), and the bar-graph request reached the device ("vendor OUT
    set-bargraph 0xff"). In the interactive menu run the test program then
    stopped: its first `DeviceIoControl` returned `ERROR_IO_PENDING`
    because it opens the device for overlapped I/O and calls without an
    `OVERLAPPED`, which Windows waits for and Wine's `kernelbase` did not
    (patch 0016), so the switch and 7-segment requests were never sent.
  - 17 distinct FIXMEs, all non-fatal (callback objects, WMI registration,
    bug-check reason callbacks, optional interrupt functions, power state
    notification, `IRP_MN_QUERY_INTERFACE` and an ID type in `wineusb`).
- **Iteration 10 (run 43393835772, commit `7ec6962`): a KMDF driver
  answered I/O in Wine.** With `compat/wdm.h` the queue loop is gone.
  `kmdftest.sys` started as before and its test program got real answers:
  `IOCTL_INFO` returned 44 bytes (events 0x7: device added, hardware
  prepared, D0 entered; USB target created, device 1209:0001, one
  interface, two pipes), and the self-test ran a **`WDFDPC`** and a
  **`WDFWORKITEM`** (each ran once). No crash, no unimplemented call.
  Failed: `WdfTimerCreate` (`STATUS_INSUFFICIENT_RESOURCES`: the framework
  creates every timer with `ExAllocateTimer`, which patch 0010 had made
  fail; patch 0014 implements it), and the bulk echo and vendor control
  transfer (error 22): the pipes were still endpoint 0. Patches 0012 and
  0013 change `wineusb.sys`'s Unix half, `wineusb.so`, and the overlay only
  replaced the PE half, so they had no effect yet; from iteration 11 the
  overlay includes `wineusb.so`. Microsoft's `kmdf_fx2` sample reached its
  `DriverEntry` and `EvtDriverDeviceAdd`, then the framework bugchecked
  (`KeBugCheckEx 0x10D` `WDF_VIOLATION`, `WDF_INVALID_HANDLE`, handle 0, type
  memory): Wine's `IoGetDeviceProperty` returned setupapi's Win32 error 13
  for the missing friendly name, which `NT_SUCCESS` takes for success, and
  the sample used the NULL memory object (patch 0015).
- **Iteration 9 (run 43390973700, commit `be3cd7e`): a KMDF driver started
  in Wine.** `kmdftest.sys`, on the KMDF runtime built from Microsoft's MIT
  source, logged in order: `DriverEntry`, `WdfDriverCreate returned
  0x00000000`, `DriverEntry returned 0x00000000`, `EvtDriverDeviceAdd`,
  `WdfDeviceCreate returned 0x00000000`, `WdfIoQueueCreate` and
  `WdfDeviceCreateDeviceInterface` 0, `EvtDevicePrepareHardware`,
  `WdfUsbTargetDeviceCreateWithParameters` 0, the device descriptor read
  over USB (1209:0001, bcdUSB 0200), `WdfUsbTargetDeviceSelectConfig` 0,
  **`EvtDeviceD0Entry from D4`**; the framework's PnP and power state
  machines ran through their start states, and the test program opened the
  device interface (check `device-interface-found` PASS). Then the I/O
  queue looped: its DPC re-queued itself forever (section 2.7, a header
  problem, not Wine's), the test program's first IOCTL never completed, the
  trace grew to 14 GB and the job was killed before the FX2 scenario.
  Iteration 10 fixes the cause (`compat/wdm.h`), caps logs, and fills in
  pipe information in `wineusb.sys` (patch 0012; the pipes came back as
  endpoint 0, packet size 0) and adds the generic control transfer the
  framework sends for every driver control request (patch 0013).
- **Iteration 8 (run 43388872919, commit `abbbb69`) was the first in which
  KMDF code ran in Wine.** Measured in the `kmdftest` scenario:
  - `Wdf01000.sys` built (1,507,328 bytes) and loaded native in Wine;
  - `WdfRegisterLibrary` 1.33.0 succeeded;
  - the client stub bound KMDF 1.33 with 458 functions;
  - **`WdfVersionBind` returned 0**;
  - **kmdftest's `DriverEntry` ran**.

  It then aborted: "Call from 00006FFFFF36EA1C to unimplemented function
  ntoskrnl.exe.MmIsDriverVerifying" (9 distinct FIXMEs in `fixmes.txt`), so
  `WdfDriverCreate` never returned. The FX2 scenario stopped at the same
  call, so the sample's test program found no device interface (error
  0x2e). Patch 0011 implements `MmIsDriverVerifying` (FALSE, as Windows
  without Driver Verifier) and every other aborting stub the framework
  imports.
- **The published framework source is not buildable as it stands.** It is
  "source for debugging": the WDK's private and generated headers, the
  loader (WdfLdr.sys) and the driver stub (WdfDriverEntry.lib) are not in
  it, several files have lines blanked where code was, and some functions
  live only in WDK libraries. Every gap is listed in section 2; each one is
  filled in this branch by a stand-in or a small patch, written from the
  framework's own use of it.
- **All 180 files of the runtime compile (iteration 6) and link (iteration
  8).** The compile error count went 106,913 → 529 → 46,584 → 811 → 89 → 0
  over six iterations (section 3); the link needed two more.
- **The client drivers build**: `kmdftest.sys` and Microsoft's unmodified
  `kmdf_fx2` sample (`osrusbfx2.sys`) and its test program, with the same
  toolchain and a stand-in KMDF stub (iteration 7).
- **The framework imports at least 182 kernel functions and data** (section
  5.2). Of the 162 that mingw-w64's import libraries know, Wine 11.20 lacks or
  stubs 48: 7 not exported at all, 19 aborting stubs, 22 that log FIXME and
  do nothing; Wine master is identical for these. Another 20 (power
  framework, high-resolution timers, activity IDs, verifier queries, ...)
  are in neither Wine nor mingw-w64's libraries and were found at the link.
  The TUR patches add all of the missing ones (0007, 0009, 0010) and
  replace every aborting stub (0001, 0004, 0011).
- **The Wine side**: sixteen patches to Wine 11.20 (`ntoskrnl.exe`, a new
  `wdfldr.sys`, `wineusb.sys`), all of which also apply to Wine master
  (section 4). They are installed over Fedora 45's Wine in the CI container.

## 1. What KMDF consists of, and what the published source holds

| Piece | On Windows | In Windows-Driver-Frameworks (commit `b6191d95`, MIT) |
|---|---|---|
| `Wdf01000.sys` (the runtime, KMDF 1.33, 458 DDIs) | A kernel export driver; registers itself with the loader from its DriverEntry | Source of 178 kernel-mode files (about 160,000 lines of C++ under `src/framework/kmdf` and `src/framework/shared`), no build files |
| `WdfLdr.sys` (the loader, `WdfVersionBind`) | Loads the runtime for each client driver | **Not included**; its interface is in `fxldr.h`, but `wdfldr.h`, which `fxldr.h` includes, is not |
| `WdfDriverEntry.lib` (the stub in every KMDF driver) | `FxDriverEntry` calls `WdfVersionBind`, then the driver's `DriverEntry` | **Not included** |
| Public headers | WDK | Included (`src/publicinc/wdf/kmdf/1.15`–`1.33`) |

Build environment the source expects: the WDK's MSVC (`/permissive`
behaviour, implicit int, MS token pasting, forward-declared enums), WPP's
trace preprocessor, the message compiler, and WDK-only static libraries.

## 2. What is missing from the published source, and what this branch puts in its place

All stand-ins are in `third-party/wdf/compat/` (written for this build,
offered under MIT — see section 7, open decision); patches to Microsoft's
files are in `third-party/wdf/patches/` (MIT).

### 2.1 Private headers not published

| Header | Used for | Stand-in |
|---|---|---|
| `wdfldr.h` | loader types (`WDF_COMPONENT_GLOBALS`, class bind info, `WDF_INTERFACE_HEADER`) | written from `fxldr.h` and the sources |
| `wdfcx.h`, `wdfcxbase.h` | class-extension DDIs and callback structures (13 DDI types, ~60 callback types) | written from `fxcxpnppowercallbacks.hpp`, `pnpcallbacks.cpp`, `fxdynamics.h` |
| `wdfdevicepri.h`, `wdfiotargetpri.h` | self I/O target DDIs | from `fxdynamics.h` prototypes |
| `wdftriage.h` | crash-dump triage tables | from `fxlibrarycommon.cpp` |
| `cobbled.hpp`, `device_common.h`, `WudfRdNonPnP.hpp` | included unconditionally | empty so far |
| `FxCompanionLibrary.hpp`, `FxCompanionTarget.hpp` | UMDF companions | a library that never needs a companion |
| `MxWnf.h`, `SleepStudyHelper.h` | sleep-study reporting | every call fails ("disabled") |
| `FxDsf.h` | Device Simulation Framework | an interface nobody answers |
| `FeatureStagingSupport.h`, `FeatureStaging-WDF.h`, `wil/staging.h` | feature staging | empty (no feature is queried) |

### 2.2 Generated files

| File | Generator | Stand-in |
|---|---|---|
| 205 `*.tmh` | WPP | empty; trace calls go to `DoTraceLevelMessage` (`tur-wdf-missing.cpp`), which translates WPP's `%!NAME!` conversions and prints through `DbgPrintEx` |
| `DriverFrameworks-KernelMode-KmEvents.h` | message compiler (ETW) | events off |
| `WdfVersionLog.h` | message compiler (event log) | message IDs |

### 2.3 Lines blanked in published files

| File | What is blank | Patch |
|---|---|---|
| `shared/inc/private/common/fxirp.hpp` | 65 lines: end of the header comment, include guard, the head of `class FxIrp` (the rest of the file is inside an unterminated comment) | 0002, reconstructed from the members' use |
| `shared/inc/private/common/fxglobals.h` | `StateSeparationDetection` member and its enumeration | 0005 |
| `shared/inc/private/common/fxregkey.hpp` | `VerifyStateSeparationRegistryPolicy` (called, declared nowhere) | 0004 |
| `shared/core/fxrequestapi.cpp` | `WDF_REQUEST_REUSE_MUST_COMPLETE` (defined only in `fxrequest.cpp`) | 0006 |
| `shared/inc/private/common/fxforward.hpp` | `enum FxWmiInstanceAction` forward-declared, defined nowhere | 0010 |

### 2.4 MSVC-only code

| What | Patch |
|---|---|
| `wdfdevice.h` uses `WDF_REQUEST_TYPE`, a forward-declared enum, by value | 0001 (GCC/clang get the definition early) |
| six member functions without a return type (implicit int) | 0003 |
| aligned array of a 1-byte type with 16-byte alignment | 0007 |
| `DebugLevel`/`DebugFlag` declared C++, defined C | 0008 |
| members newer than mingw-w64's structures (`WakeFromInterrupt`, interrupt policy) | 0009, through compat layouts |
| `-fms-compatibility` hides `__GNUC__` from mingw-w64's headers | build flag `-fgnuc-version=4.2.1` |
| `#include "km\..."` backslashes, case-insensitive names | build script: separator normalisation, `incmap.py` links |
| SAL annotations mingw-w64 does not define | `salgen.py`, 133 empty definitions |

### 2.5 Functions from WDK libraries

| Function | Library | Stand-in |
|---|---|---|
| `WdmlibIoCreateDeviceSecure` (`IoCreateDeviceSecure`), `SDDL_DEVOBJ_SYS_ALL_ADM_ALL` | `wdmsec.lib` | `tur-wdmsec.c`: `IoCreateDevice`, no security descriptor (Wine has none on device objects) |
| `FxPnpDevice{D0Entry,D0Exit,PrepareHardware,ReleaseHardware}::InvokeCompanionCallback` | not published | succeed (no companion target ever exists) |
| `WdfBindClientHelper`, `WdfLdrDiagnosticsValueByNameAsULONG` | WDF build internals | `tur-wdf-missing.cpp` |
| `USBD_CreateHandle`, `USBD_UrbAllocate`, `USBD_QueryUsbCapability`, … | `usbdex.lib` | `tur-usbdex.c`, from Microsoft's documentation |
| C++ start-up (static constructors) | MSVC CRT | `tur-wdf-entry.c` |
| `imp_WdfCompanionTarget*` (2 of 458 DDIs) | not published | fail with `STATUS_NOT_SUPPORTED` |
| `FxVerifierQueryStateSeparationDetection` | not published | "none" |

### 2.6 Kernel declarations mingw-w64 14.0.0 lacks

`ExAllocatePool2` and pool flags, `ExInitializeDriverRuntime`,
`KeQuerySystemTimePrecise`, `IoOpenDriverRegistryKey`, power framework v3
(`PO_FX_DEVICE_V3`, directed power callbacks), `D3COLD_SUPPORT_INTERFACE`,
DMA version 3 (the members appended to `DMA_OPERATIONS` and
`DEVICE_DESCRIPTION` in Windows 8), the USB client contract (`USBD_HANDLE`),
`IO_REPORT_INTERRUPT_ACTIVE_STATE_PARAMETERS`, `RtlUnicodeStringPrintf`,
`RtlUnicodeStringCopy`, `RtlSIZETAdd`, activity-ID IRP functions,
`IoQueryFullDriverPath`, `IoGetInitiatorProcess`. All in
`compat/tur-ddk-additions.h` (through a wrapping `ntddk.h`), `usbdlib.h`,
`usbdrivr.h`, `tur-wdf-prelude.h`.

### 2.7 Inline kernel accessors that read a kernel's processor block

Found at run time (iteration 9), not at compile time: on x86-64 mingw-w64's
`wdm.h` defines `KeGetCurrentThread` (and `PsGetCurrentThread` through it)
as an inline read of `gs:[0x188]`, the current thread in a real kernel's
processor control region. In Wine's driver host `gs` is the thread's TEB,
where that offset is zero in every thread. Every thread therefore looked
like the same NULL thread, `FxCallbackLock::IsOwner` answered "owner" for an
unowned lock, and an I/O queue deferred its dispatch to a DPC that deferred
again, forever. `compat/wdm.h` wraps mingw-w64's `wdm.h` and makes both
names call ntoskrnl's exports (iteration 10). A driver built with the WDK's
own headers calls the import and never had this problem.

## 3. Compiling Wdf01000.sys: the error trend

Fedora 45 container, clang 23.1.5 (`--target=x86_64-w64-mingw32`,
`-fms-extensions -fms-compatibility -fdelayed-template-parsing`),
mingw64-headers 14.0.0, 180 files (178 of Microsoft's minus `dbgtrace.cpp`,
plus three of `compat/`).

| Iteration (commit, run) | Files compiled | Errors | What dominated |
|---|---|---|---|
| 1 (`bb6dc6f`, 43378979394) | 1 | 106,913 | mingw-w64 headers broken under `-fms-compatibility` (`__GNUC__` not defined) |
| 2 (`2ef0349`, 43379819341) | 3 | 529 | `cobbled.hpp`, `wdftriage.h` missing; `WDF_REQUEST_TYPE`; an `MmMapLockedPagesSpecifyCache` overload |
| 3 (`6faf383`, 43380816664) | 4 | 46,584 | `FxIrp` incomplete (the blanked class head) cascading through every class |
| 4 (`80a5c2c`, 43382377329) | 0 | 811 | `FxIrp::m_Irp` private; my `DMA_COMPLETION_STATUS`; `device_common.h`; token pasting |
| 5 (`265e616`, 43383650067) | 149 | 89 | Windows 8–10 kernel types mingw-w64 lacks (`STACK_DEVICE_CAPABILITIES`, power framework v1/v3 functions, `WakeFromInterrupt`, interrupt policy fields), one more undefined enumeration (`FxWmiInstanceAction`), a linkage mismatch |
| 6 (`ffc7579`, 43386297642) | **180 of 180** | **0** | compiled; the link stopped at the first 20 duplicate definitions of mingw-w64's `ntstrsafe.h` functions (C99 `extern inline` in the C files) before reporting undefined symbols |
| 7 (`1f91af3`, 43387715486) | 180 of 180 | 0 | 74 duplicates, all `ntstrsafe.h`'s, linked past; the link then reported undefined symbols: more kernel imports Wine lacks (power framework, high-resolution timers, activity IDs, verifier queries, `IoQueryFullDriverPath`, `IoGetInitiatorProcess`), `wdmsec.lib`'s `IoCreateDeviceSecure` and SDDL constant, four companion callbacks, one C-linkage mismatch of mine, and `__CTOR_LIST__`. `kmdftest.sys` (90,112 bytes) and Microsoft's `osrusbfx2.sys` (221,184 bytes) linked |
| 8 (`abbbb69`, 43388872919) | 180 of 180 | 0 | **linked**: `Wdf01000.sys`, 1,507,328 bytes, after Wine patches 0009–0010, `tur-wdmsec.c`, the companion callbacks and the linkage fix; loaded in Wine (section 6) |

## 4. The Wine patch set

`third-party/wine-patches`, LGPL-2.1-or-later, against wine-11.20
(`7298a818`, what Fedora 45 ships; Staging touches none of these files);
both also apply to master `cc5d1d86` (checked in every build).

| Patch | What | Why |
|---|---|---|
| 0001 `ntoskrnl.exe: Run DPCs queued with KeInsertQueueDpc.` | queued DPCs run in order on one driver-host thread; `KeRemoveQueueDpc` implemented (was `stub`); `KeFlushQueuedDpcs` waits; `KeInitializeDpc` no longer a semi-stub | KMDF's `WDFDPC`, and any driver's DPC, never ran (the stub that closed bug 49394 returns TRUE and does nothing) |
| 0002 `wdfldr.sys: Add a Kernel-Mode Driver Framework loader.` | new module: `WdfVersionBind` loads the `Wdf01000` service with `ZwLoadDriver`, accepts `WdfRegisterLibrary`, registers clients; `WdfLdrQueryInterface`; class extensions return `STATUS_NOT_SUPPORTED` | every KMDF driver imports it (bug 56228) |
| 0003 `ntoskrnl.exe: Implement driver object extensions.` | `IoAllocateDriverObjectExtension` (returned `STATUS_NOT_IMPLEMENTED`) and `IoGetDriverObjectExtension` (returned NULL): a list per driver object | `WdfDriverCreate` keeps the framework's driver object in one; found reading Wine's source |
| 0004 `ntoskrnl.exe: Add a WmiQueryTraceInformation that fails.` | was a `stub` (aborts); now returns `STATUS_NOT_SUPPORTED` | called while the framework starts (`globals.cpp`); the framework falls back when it fails |
| 0005 `ntoskrnl.exe: Add KeQuerySystemTimePrecise.` | not exported before | imported by the framework (Mx layer) |
| 0006 `ntoskrnl.exe: Add IoOpenDriverRegistryKey.` | not exported before; opens or creates `Parameters` (or `State`) under the service key | imported by the framework (`fxglobals.h`) |
| 0007 `ntoskrnl.exe: Add functions the KMDF runtime imports, and implement PoRequestPowerIrp.` | the six missing imports (failing or minimal); `PoRequestPowerIrp` builds the power IRP, sends it to the top of the stack and calls the completion callback | the import table (5.2); the power-policy owner requests D0 at start (upstream this would be two patches) |
| 0008 `wineusb.sys: Handle power IRPs.` | set/query power succeed on PDOs, the bus FDO passes them down | without it every KMDF USB driver's start fails at D0 |
| 0009 `ntoskrnl.exe: Add power framework functions that report it unavailable.` | `PoFxRegisterDevice` fails with `STATUS_NOT_SUPPORTED`; the ten functions that take its handle do nothing | imported by the framework (`poxinterfacekm.cpp`); without them the image does not load |
| 0010 `ntoskrnl.exe: Add more functions the KMDF runtime imports.` | `ExAllocateTimer` fails (callers use `KeSetTimer` timers), `ExSetTimer`/`ExCancelTimer`/`ExDeleteTimer`, `IoGetActivityIdIrp`/`IoSetActivityIdIrp`, `IoGetInitiatorProcess`, `IoQueryFullDriverPath` (from the loader entry), `VfIsRuleClassEnabled` (FALSE), `VfCheckNxPoolType` | the link of iteration 7 |
| 0012 `wineusb.sys: Fill in interface and pipe information on SELECT_CONFIGURATION.` | fills each interface's class/subclass/protocol and each pipe's endpoint, packet size, interval and type from the active configuration descriptor, sets configuration and interface handles, walks every interface in the request | iteration 9: the framework builds the request itself, and its pipes came back as endpoint 0, packet size 0, control; Wine had relied on usbd.sys's `USBD_CreateConfigurationRequestEx` prefilling them |
| 0013 `wineusb.sys: Handle URB_FUNCTION_CONTROL_TRANSFER and URB_FUNCTION_GET_STATUS_FROM_DEVICE.` | submits the generic control transfer (the caller's setup packet; direction flag and buffer length take precedence, as on Windows) and the device get-status request on the default control pipe | the framework sends every driver control transfer (`WdfUsbTargetDeviceSendControlTransferSynchronously`, `WdfUsbTargetDeviceFormatRequestForControlTransfer`, which the FX2 sample uses for its bar graph, switches and 7-segment display) as `URB_FUNCTION_CONTROL_TRANSFER`, which wineusb rejected; get-status was the FIXME of iteration 9; found reading the framework's source |
| 0014 `ntoskrnl.exe: Implement ExAllocateTimer, ExSetTimer, ExCancelTimer and ExDeleteTimer.` | the Windows 8.1 timer interface on top of `KeSetTimerEx` and a DPC that calls the callback (replacing 0010's failing versions) | iteration 10: the framework creates every timer of a 1.13+ client with `ExAllocateTimer`; `WdfTimerCreate` failed |
| 0015 `ntoskrnl.exe: Return NTSTATUS codes from IoGetDeviceProperty.` | maps setupapi's Win32 errors (`ERROR_INSUFFICIENT_BUFFER` → `STATUS_BUFFER_TOO_SMALL`, a missing property → `STATUS_OBJECT_NAME_NOT_FOUND`, ...) | iteration 10: a missing friendly name came back as 13, "success"; the FX2 sample then used a NULL memory object and the framework bugchecked. A Wine bug for any driver |
| 0016 `kernelbase: Wait for a pending DeviceIoControl called without an OVERLAPPED.` | as `ReadFile`/`WriteFile` already do in Wine, and as Windows does | iteration 11: the FX2 test program opens its device overlapped and calls `DeviceIoControl` without an `OVERLAPPED`; the framework pends every request, so the call failed with `ERROR_IO_PENDING`. Installed for the FX2 scenario only (Fedora ships Staging's `kernelbase`; this is plain 11.20 plus the patch) |
| 0011 `ntoskrnl.exe: Implement the remaining stubs the KMDF runtime imports.` | `MmIsDriverVerifying` returns FALSE (no Driver Verifier, as on Windows without it); `MmSizeOfMdl`, `IoBuildPartialMdl`, `IoSetCompletionRoutineEx`, `KeGetRecommendedSharedDataAlignment`, `MmLockPagable{Data,Image}Section`, the error-log and WMI-event functions (no event log, no WMI), `WmiTraceMessageVa`, `IoInvalidateDeviceState`, `IoRequestDeviceEject`, `IoRegisterLastChanceShutdownNotification`, `IoGetDmaAdapter` (no adapters), `KeSynchronizeExecution` and the interrupt spin locks (interrupts are never connected), the `KdDebuggerNotPresent` variable | iteration 8: `WdfDriverCreate` called `MmIsDriverVerifying`, a `stub`, and the driver host aborted; the rest were the other `stub` imports in 5.2, each an abort waiting to happen |

A correction to an earlier iteration of this report: patch 0009 was first
written, then dropped because the import table did not list the power
framework. The table had missed it (its symbols are in neither mingw-w64's
import libraries nor Wine, and the table only counted names one of them
knows); the link of iteration 7 showed them, and the patch is back.

Iteration 5: all six applied (and apply to master), but `ntoskrnl.exe` did
not link: patch 0006 called `ZwOpenKey`/`ZwCreateKey`/`ZwClose`, which
ntoskrnl itself only forwards to ntdll; fixed to `Nt*` in iteration 6.
Built in CI (`build-wine.sh`: Wine's tools, `ntoskrnl.exe`, `wdfldr.sys`
only) and put over Fedora 45's Wine in the throw-away container
(`/usr/lib64/wine-wow64/wine/x86_64-windows/`, measured in run
43382377329). Run-time effect: *pending*.

## 5. Kernel functions the framework uses that Wine lacks

### 5.1 Static pre-scan (source, before linking)

The 156 kernel functions the kernel-mode sources call by name, checked
against Wine's `ntoskrnl.exe.spec` and `hal.spec`. Some are found at run
time with `MmGetSystemRoutineAddress` rather than imported; the link step
(5.2) decides.

| | Wine 11.20 and master |
|---|---|
| Not exported at all | `EtwActivityIdControl`, `ExAllocateTimer`, `ExCancelTimer`, `ExDeleteTimer`, `ExSetTimer`, `IoOpenDriverRegistryKey`, `IoUnregisterPlugPlayNotificationEx`, `IoWMIDeviceObjectToProviderId`, `KeQuerySystemTimePrecise`, `KeSetCoalescableTimer`, `PoFxRegisterDevice`, `PoGetSystemWake`, `PoSetSystemWake` |
| `stub` (calling it aborts the driver host) | `IoBuildPartialMdl`, `IoConnectInterrupt`, `IoGetDmaAdapter`, `IoInvalidateDeviceState`, `IoRegisterLastChanceShutdownNotification`, `IoRequestDeviceEject`, `IoSetCompletionRoutineEx`, `IoWMIWriteEvent`, `IoWriteErrorLogEntry`, `KeAcquireInterruptSpinLock`, `KeGetRecommendedSharedDataAlignment`, `KeReleaseInterruptSpinLock`, `KeSynchronizeExecution`, `MmIsDriverVerifying`, `MmSizeOfMdl`, `WmiQueryTraceInformation`, `WmiTraceMessageVa`; `KeRemoveQueueDpc` (fixed by patch 0001) |

`WmiQueryTraceInformation` is called while the framework starts
(`globals.cpp`), so as it stands it would abort the driver host before any
driver binds.

### 5.2 Authoritative import table (iteration 6, from the compiled objects)

`imports.py` over the undefined symbols of all 180 objects (run
43386297642, artifact `kmdf-build-evidence`, `wdf/imports.md`). 162 imports,
all from `ntoskrnl.exe` except `WdfRegisterLibrary` (`wdfldr.sys`); none
from `hal.dll` or `usbd.sys` (the framework's USB target builds its URBs
itself; the client-contract functions are in `tur-usbdex.c`). Functions the
framework looks up at run time with `MmGetSystemRoutineAddress` (the power
framework, `IoConnectInterruptEx`, ...) are not imports and are not counted.

| Wine | implemented | FIXME stub (does nothing) | `stub` (aborts) | missing (no load) |
|---|---|---|---|---|
| 11.20 | 114 | 22 | 19 | 7 |
| master `cc5d1d86` | 114 | 22 | 19 | 7 |
| 11.20 + TUR patches 0001–0006 | 122 | 17 | 17 | 6 |
| 11.20 + TUR patches 0001–0008 | 128 (expected; iteration 7 measures) | 17 | 17 | 0 |

Every import not plainly implemented (11.20 = master; "+TUR" after patches
0001–0006):

| Function | 11.20 / master | +TUR | Effect for a KMDF USB driver |
|---|---|---|---|
| `EtwActivityIdControl`, `IoUnregisterPlugPlayNotificationEx`, `IoWMIDeviceObjectToProviderId`, `KeSetCoalescableTimer`, `PoGetSystemWake`, `PoSetSystemWake` | missing | missing → added by 0007 | the runtime would not load |
| `WdfRegisterLibrary` | missing | implemented (0002) | |
| `KeInsertQueueDpc`, `KeInitializeDpc`, `KeFlushQueuedDpcs` | FIXME stub | implemented (0001) | `WDFDPC` never ran |
| `KeRemoveQueueDpc` | stub | implemented (0001) | |
| `IoAllocateDriverObjectExtension`, `IoGetDriverObjectExtension` | FIXME stub | implemented (0003) | `WdfDriverCreate` fails |
| `WmiQueryTraceInformation` | stub | implemented (0004) | aborted the host at start |
| `PoRequestPowerIrp` | FIXME stub (`STATUS_NOT_IMPLEMENTED`) | implemented in 0007 | the device never reaches D0 |
| `IoBuildPartialMdl`, `IoGetDmaAdapter`, `IoInvalidateDeviceState`, `IoRegisterLastChanceShutdownNotification`, `IoRequestDeviceEject`, `IoSetCompletionRoutineEx`, `IoWMIWriteEvent`, `IoWriteErrorLogEntry`, `KdDebuggerNotPresent` (data), `KeAcquireInterruptSpinLock`, `KeReleaseInterruptSpinLock`, `KeGetRecommendedSharedDataAlignment`, `KeSynchronizeExecution`, `MmIsDriverVerifying`, `MmLockPagableDataSection`, `MmSizeOfMdl`, `WmiTraceMessageVa` | stub | implemented or failing cleanly (0011) | abort if called: `MmIsDriverVerifying` aborted `WdfDriverCreate` in iteration 8 |
| `ExCreateCallback`, `ExRegisterCallback`, `ExUnregisterCallback`, `IoAllocateErrorLogEntry`, `IoGetRelatedDeviceObject`, `IoRegisterPlugPlayNotification`, `IoRegisterShutdownNotification`, `IoUnregisterShutdownNotification`, `IoWMIRegistrationControl`, `KeDeregisterBugCheckReasonCallback`, `KeRegisterBugCheckReasonCallback`, `KeGetCurrentProcessorNumberEx`, `MmLockPagableSectionByHandle`, `MmUnlockPagableImageSection`, `PoSetPowerState`, `PoStartNextPowerIrp` | FIXME stub | FIXME stub | do nothing or report success; mostly harmless in user space |

Found by reading Wine, not by the table: `wineusb.sys` has no `IRP_MJ_POWER`
handler, so a power IRP to a USB device fails with
`STATUS_INVALID_DEVICE_REQUEST` (patch 0008).

## 6. Running KMDF drivers

| Scenario | Status |
|---|---|
| Overlay of the patched `ntoskrnl.exe`, `wineusb.sys` and new `wdfldr.sys` over Fedora 45's Wine | PASS (run 43382377329 onwards) |
| `kmdftest.sys` (minimal KMDF USB driver) on the loopback device 1209:0001 | iteration 9: started; iteration 10: IOCTLs, `WDFDPC`, `WDFWORKITEM`; **iteration 11: all PASS**, including `WDFTIMER`, bulk echo 64/512 bytes and a vendor control transfer |
| Microsoft's `kmdf_fx2` sample and its `testapp.c` on the FX2-like device 0547:1002 | iteration 10: `WDF_VIOLATION` (Wine `IoGetDeviceProperty` bug, patch 0015); **iteration 11: `EvtDevicePrepareHardware`, `EvtDeviceD0Entry`, interrupt reads, test program's descriptor dump and bulk write/read PASS, bar graph set**; menu run stopped at `ERROR_IO_PENDING` (patch 0016) |

Iteration 10, `kmdftest.exe` checks: `device-interface-found`, `ioctl-info`
(44 bytes), `prepare-hardware-ran` (events 0x7), `d0-entry-ran` (1),
`usb-target-and-config` (status 0, 1209:0001), `ioctl-selftest`,
`wdfdpc-ran` (1 run), `wdfworkitem-ran` (1 run) PASS; `pipes` (bulk in/out
endpoint 00), `wdftimer-fired` (`WdfTimerCreate` 0xc000009a),
`bulk-echo-64`, `bulk-echo-512`, `vendor-control` (error 22) FAIL. All
driver-host checks PASS (bind, `DriverEntry` returned 0, no crash). New:
"MxSubscribeWnfStateChange failed, disabling Sleep Study" (no WNF in Wine;
the framework carries on).

Iteration 9, step by step (`kmdftest`, from the driver host's log):

| Step | Result |
|---|---|
| load, bind, `DriverEntry` (as iteration 8) | PASS |
| `WdfDriverCreate` | PASS, returned 0 ("WDF[4]: FxIFR logging started") |
| `EvtDriverDeviceAdd` → `WdfDeviceCreate`, `WdfIoQueueCreate`, `WdfDeviceCreateDeviceInterface` | PASS, all 0; PnP state 0x100 → 0x105 |
| `IRP_MN_START_DEVICE` → PnP states 0x106, 0x108 → `EvtDevicePrepareHardware` | PASS |
| `WdfUsbTargetDeviceCreateWithParameters` (four `IOCTL_INTERNAL_USB_SUBMIT_URB` to `wineusb.sys`) | PASS; device descriptor 1209:0001 bcdUSB 0200; `GET_STATUS_FROM_DEVICE` unhandled by wineusb (FIXME, tolerated) |
| `WdfUsbTargetDeviceSelectConfig` | returned 0, but both pipes reported endpoint 0x00, packet size 0 (patch 0012) |
| power policy 0x500 → 0x501, power 0x300 → 0x316 → 0x30f → `EvtDeviceD0Entry from D4` → 0x310, 0x311, 0x367, 0x312, "Power resume all queues" | PASS |
| `kmdftest.exe`: device interface found and opened | PASS |
| first IOCTL (`INFO`) | hung: the queue's DPC re-queued itself ("Presentation lock … already held, deferring to dpc or workitem") until the 240 s timeout; 14 GB of `+ntoskrnl` trace; the harness was killed reading it (exit 137) |

New FIXMEs in iteration 9, none fatal: `ExCreateCallback`/`ExRegisterCallback`
(stubs; the framework's power-state callback), `wineusb` `pdo_pnp`
"Unhandled minor function 0x8" (`IRP_MN_QUERY_INTERFACE`; the framework
does without the USBDI interface), `usb_submit_urb` "Unhandled function
0x13" (`GET_STATUS_FROM_DEVICE`), `PoSetPowerState`, `IoWMIRegistrationControl`.

Iteration 8, step by step (`kmdftest`, from the driver host's log):

| Step | Result |
|---|---|
| `ZwLoadDriver` of the client, `kmdftest.sys` loaded native, its import `WDFLDR.SYS` loaded builtin | PASS |
| stub: "binding to KMDF 1.33, 458 functions"; `WdfVersionBind` → `ZwLoadDriver` of the `Wdf01000` service | PASS |
| `Wdf01000.sys` loaded native; its `DriverEntry` created `\Device\KMDF0` and called `WdfRegisterLibrary` (1.33.0) | PASS |
| `LibraryRegisterClient` returned 0; the stub: `WdfVersionBind returned 0x00000000` | PASS |
| kmdftest's `DriverEntry` ran ("kmdftest: DriverEntry") | PASS |
| `WdfDriverCreate` | aborted: unimplemented `ntoskrnl.exe.MmIsDriverVerifying` (patch 0011) |
| `EvtDriverDeviceAdd`, `WdfDeviceCreate`, `EvtDevicePrepareHardware`, `EvtDeviceD0Entry` | not reached |

The 9 distinct FIXMEs of iteration 8, none fatal: `KeRegisterBugCheckReasonCallback`
(stub, twice: the framework's triage data), `MmGetSystemRoutineAddress` not
finding `IoConnectInterruptEx`, `IoDisconnectInterruptEx`,
`IoReportInterruptActive`, `IoReportInterruptInactive` (optional; the
framework looks them up and does without), `WmiQueryTraceInformation` (twice;
patch 0004 makes it fail cleanly), and `wineusb`'s "Unhandled ID query type"
(an ID type it does not report).

The harness's `wdfldr-loaded` check failed in iteration 8 although the bind
succeeded: it looked for `wdfldr.sys` in lower case and Wine's loader logs
the import as `WDFLDR.SYS`; fixed in iteration 9. The FX2 scenario's
`sample-step:DriverEntry` PASS was a false positive (it matched `wineusb`'s
own `DriverEntry`); from iteration 9 the sample's `driver.c` is built with
`DBG=1` so its own `TraceEvents` lines are in the log, and the steps match
those.

The FX2-like device (`usbip_devices.py --devices loopback,fx2`): interrupt
IN 0x81 (switch state, changes every 2 s), bulk OUT 0x06 looped back on IN
0x88, the board's vendor requests 0xD4–0xDB, every request logged; written
from the sample's `osrusbfx2.h`/`ioctl.c`. Enumerated by the runner's kernel
in run 43382377329.

## 7. Licences and open decisions

- Microsoft's framework source: MIT, fetched at a pinned commit, never
  committed; `third-party/wdf/LICENSE` is Microsoft's text and is compared
  with the fetched copy at every build.
- The sample `kmdf_fx2`: MS-PL, fetched in CI, never committed.
- Wine patches: LGPL-2.1-or-later, with Wine's COPYING.LIB.
- **Open decision for the owner:** the stand-ins and patches in
  `third-party/wdf` were written for TUR and are offered on this branch
  under MIT (so the runtime is MIT throughout and could go upstream).
  Releasing TUR-written code under MIT needs the owner's sign-off.
- No Microsoft binary was downloaded or used; nothing ran in a kernel.

## 8. Effort and next steps (as of this iteration)

1. Iteration 12: the FX2 test program's menu run (bar graph, switches,
   7-segment display, switch interrupt message) with patch 0016.
2. Each run iteration so far exposed one layer; the report records them.

## 9. Limits

- Emulated USB devices (USB/IP), not hardware.
- Wine 11.20 with the patched modules only inside a CI container.
- The stand-ins are written from the framework's use of what they replace;
  where Microsoft's real definition differs (values of private constants,
  layouts only class extensions see), they are this branch's own and are
  marked so in each file.
