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
  mingw-w64 headers), builds Wine 11.20's ntoskrnl.exe and a new wdfldr.sys
  (the KMDF loader) with a two-patch Wine patch set, and runs KMDF drivers
  against emulated USB devices in Fedora 45's Wine.
- Why: experiment 1 (windows-drivers-1) found that every KMDF driver fails
  to load in Wine because WDFLDR.SYS and Wdf01000.sys are Microsoft
  binaries. Microsoft publishes the framework's source under MIT.

FILES
- Added: third-party/wdf/** (MIT: Microsoft's LICENSE, patches, compat/
  stand-ins, sources.txt), third-party/wine-patches/** (LGPL-2.1-or-later:
  COPYING.LIB, two patches, series), tests/windows-drivers/kmdf/**,
  docs/reports/windows-drivers-2.md
- Modified: .github/workflows/windows-drivers.yml (jobs kmdf-build,
  kmdf-run; path triggers), tests/windows-drivers/virtual-usb/
  usbip_devices.py (FX2-like device, --devices), docs/dependencies/
  upstream-projects.md (three rows, sources S86 and S87)
- Removed: none

BUILD
- Command: tests/windows-drivers/kmdf/ci-build.sh in a Fedora 45 container,
  in CI only; nothing was built on a development machine
- Result: Wine modules build; Wdf01000.sys does not compile yet (section 3)

TESTS
- Integration: workflow windows-drivers, all runs green (a failed build is a
  finding, not a failed job); the kmdf-run job runs the harness
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (emulated USB/IP devices only)

KNOWN LIMITATIONS
- See section 9.

NEXT SAFE ACTION
- Continue the compile iterations; see section 8.
```

## The answer so far

- **The published framework source is not buildable as it stands.** It is
  "source for debugging": the WDK's private and generated headers, the
  loader (WdfLdr.sys) and the driver stub (WdfDriverEntry.lib) are not in
  it, several files have lines blanked where code was, and some functions
  live only in WDK libraries. Every gap is listed in section 2; each one is
  filled in this branch by a stand-in or a small patch, written from the
  framework's own use of it.
- **Compile status (latest finished iteration): 149 of 180 files compile,
  89 errors.** The error count went 106,913 → 529 → 46,584 → 811 → 89 (section 3):
  each round removes a layer and exposes the next. `Wdf01000.sys` has not
  been linked yet.
- **The Wine side builds**: Wine 11.20's `ntoskrnl.exe` with real DPCs and a
  new `wdfldr.sys` (KMDF loader), two patches that also apply to Wine
  master (section 4). They are installed over Fedora 45's Wine in the CI
  container; no KMDF driver has exercised them yet.
- **No KMDF driver has run yet.** The client side (a minimal KMDF driver
  `kmdftest.sys` and Microsoft's `kmdf_fx2` sample) and an FX2-like
  emulated device are ready and waiting for the runtime (section 5).

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
| 6 | *pending* | *pending* | |

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

### 5.2 Authoritative import table

*Pending*: written by `imports.py` from the compiled objects' undefined
symbols, classified implemented / forwarded / FIXME-stub / stub / missing
against Wine 11.20, master and the patched tree.

## 6. Running KMDF drivers

| Scenario | Status |
|---|---|
| Overlay of the patched `ntoskrnl.exe` and new `wdfldr.sys` over Fedora 45's Wine | PASS (run 43382377329) |
| `kmdftest.sys` (minimal KMDF USB driver) on the loopback device 1209:0001 | *pending* (not built) |
| Microsoft's `kmdf_fx2` sample and its `testapp.c` on the FX2-like device 0547:1002 | *pending* (not built) |

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

1. Finish compiling and link `Wdf01000.sys` (each iteration so far exposed
   one more layer).
2. The import table (5.2), then Wine patches for the real imports, starting
   with `WmiQueryTraceInformation`.
3. Run `kmdftest.sys` to `EvtDriverDeviceAdd` and `EvtDevicePrepareHardware`;
   then `kmdf_fx2`.

## 9. Limits

- Emulated USB devices (USB/IP), not hardware.
- Wine 11.20 with the patched modules only inside a CI container.
- The stand-ins are written from the framework's use of what they replace;
  where Microsoft's real definition differs (values of private constants,
  layouts only class extensions see), they are this branch's own and are
  marked so in each file.
