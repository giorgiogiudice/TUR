<!-- SPDX-License-Identifier: MIT -->
# Windows Driver Frameworks (KMDF) for Wine — TUR build files

**Not under TUR's licence.** Everything in this directory is under the
**MIT licence** of Microsoft's Windows-Driver-Frameworks ([LICENSE](LICENSE),
Microsoft's file, verbatim). Files written for TUR here are offered under the
same MIT terms (copyright TUR contributors) so that the runtime built from
them is MIT throughout.

This directory belongs to the experiment in
[docs/reports/windows-drivers-2.md](../../docs/reports/windows-drivers-2.md):
build the KMDF runtime (`Wdf01000.sys`) from Microsoft's published source with
an open toolchain and run KMDF drivers as ordinary user-space programs in
Wine's driver host. Nothing here is loaded into a Linux kernel, and nothing is
shipped in a TUR image.

## Source

| | |
|---|---|
| Repository | https://github.com/microsoft/Windows-Driver-Frameworks |
| Commit | `b6191d9543441329154da32f7ab9bdd97228dd3c` (10 September 2024, "Fix GetIfr.ps1 when Parameters key is not present"; framework sources from "Windows Release win-10.0.22621.1") |
| Licence | MIT ([LICENSE](LICENSE); compared with the fetched copy at every build) |
| Framework version built | KMDF 1.33 (458 DDIs) |

The sources are **not committed**: `tests/windows-drivers/kmdf/build-wdf.sh`
fetches them at the pinned commit in CI. No Microsoft binary (WDK, MSVC,
`Wdf01000.sys`, `WdfLdr.sys`, redistributables) is downloaded or used.

## What is here

| Path | What |
|---|---|
| `LICENSE` | Microsoft's MIT licence for Windows-Driver-Frameworks |
| `sources.txt` | The files that make up `Wdf01000.sys`: the 178 kernel-mode files of the source tree minus one (`dbgtrace.cpp`), plus two of `compat/` |
| `compat/` | Headers and two source files written for this build. They stand in for what the WDK supplies and the published source lacks: private headers (`wdfldr.h`, `wdfcx.h`, `wdfdevicepri.h`, `wdfiotargetpri.h`, `FxCompanion*.hpp`, `MxWnf.h`, `FxDsf.h`, …), generated files (the message compiler's `*Events.h`, `WdfVersionLog.h`; WPP's `.tmh` files are generated empty by the build script), SDK headers mingw-w64 does not ship (`traceloggingprovider.h`, `winmeta.h`, `aux_klib.h`, `ntwmi.h`, …), kernel declarations mingw-w64's DDK lacks (`tur-ddk-additions.h`, through a wrapping `ntddk.h`), a wrapping `wdm.h` that makes `KeGetCurrentThread`/`PsGetCurrentThread` call ntoskrnl instead of reading a kernel's processor block through `gs`, and functions from WDK libraries (`tur-wdf-missing.cpp`, `tur-wdf-entry.c`). Each file says what it stands in for and how it was written. |
| `patches/` | Changes to Microsoft's files, as `git apply` patches (MIT). One mechanical change is made by the build script instead of a patch: backslashes in `#include` lines become slashes. |

## How it is built

`tests/windows-drivers/kmdf/build-wdf.sh` (TUR's licence) in a Fedora 45
container: clang targeting `x86_64-w64-mingw32` with `-fms-extensions
-fms-compatibility`, mingw-w64's SDK and DDK headers, an include map that
resolves `#include` names case-insensitively as Windows does
(`incmap.py`), and empty definitions for every source annotation mingw-w64
does not define (`salgen.py`). `link-wdf.sh` links with lld against import
libraries made from mingw-w64's and Wine's export lists.
