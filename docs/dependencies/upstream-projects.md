# Upstream projects and their licences

This is the dependency inventory required by task UOS-001 and by specification
§17.2. It lists every upstream project UOS integrates or proposes to integrate,
the licence each one states for itself, how UOS consumes it, and what
[ADR-0003](../adr/0003-licence.md) implies for UOS code that sits next to it.

Companion documents:

- [licence-review.md](licence-review.md) — the licences in the built image, the
  obligations that distributing it would create, and the questions for legal
  review.
- [image-licences.csv](image-licences.csv) — every package in the image with its
  declared licence.

This inventory is input to a lawyer's review, which the specification requires
before public distribution (§17.2). It is not legal advice.

## How to read the tables

| Column | Meaning |
|---|---|
| Licence | SPDX identifiers, as stated by the project's own repository or site on the date in [Sources](#sources). "unverified" means no source was checked; it is not a guess. |
| Route | How UOS consumes the project. **Base image**: already in the Fedora Kinoite base. **Fedora package**: to be added from Fedora, the intended route under ADR-0001; whether Fedora 45 carries the package was not checked. **Built by UOS**: Fedora does not carry it, or UOS needs its own build. **Fetched by user**: downloaded at run time on the user's machine, at the user's request; UOS does not redistribute it. **Not shipped**: evaluation or research only. |
| In image | Whether the package is in the image built by CI run 43147702795 (build `20261112.gdf32bacdf174`, 1,750 RPM packages), checked against its SBOM. Versions are the RPM versions in that SBOM. |
| Boundary | What ADR-0003's licence boundary means for this project. "No constraint" means UOS code that reaches the project only through D-Bus, a socket or the command line stays UOS's own separate work and may be PolyForm-Noncommercial-1.0.0. |

Two rules apply to every row and are not repeated in each one:

- A patch to an upstream project carries that project's licence, never the UOS
  licence.
- The project itself keeps its licence inside the UOS image and stays usable
  commercially under it. UOS cannot restrict it.

## Projects on the specification shortlist (§17)

| Project | Role in UOS | Source | Licence | Route | In image | Boundary |
|---|---|---|---|---|---|---|
| Linux kernel | Host kernel | https://github.com/torvalds/linux | `GPL-2.0-only WITH Linux-syscall-note`; individual files under other GPL-2.0-compatible licences [S1] | Base image, unmodified and signed by Fedora | Yes, 7.3.3-300.fc45 | User space reaches the kernel through system calls, which the syscall note places outside the kernel's copyleft: no constraint on UOS services. A kernel patch or kernel module must be GPL-2.0-compatible and can never be noncommercial; it also needs an ADR (AGENTS.md, ADR-0001). |
| systemd | Init, services, logging, boot assessment | https://github.com/systemd/systemd | `LGPL-2.1-or-later`; udev programs `GPL-2.0-or-later`; small parts `MIT`, `MIT-0`, `CC0-1.0`, `BSD-3-Clause` [S2] | Base image | Yes, 262.1 | Units, D-Bus and `sd_notify`: no constraint. Linking `libsystemd` is allowed on LGPL terms. Do not copy udev source into UOS code. |
| bootc | Image-based install and update | https://github.com/bootc-dev/bootc | `MIT OR Apache-2.0` [S3] | Base image | Yes, 1.17.1 | Command line: no constraint. Permissive if used as a library; keep notices. |
| OSTree | Atomic deployments and rollback | https://github.com/ostreedev/ostree | `LGPL-2.0-or-later` [S4] | Base image | Yes, 2026.4 (with rpm-ostree 2026.3) | Command line and D-Bus: no constraint. Linking `libostree` is allowed on LGPL terms. |
| Wine | Windows compatibility | https://github.com/wine-mirror/wine | `LGPL-2.1-or-later` [S5] | Fedora package, or built by UOS if patches are needed (not decided) | No | Launching Wine and managing prefixes from a separate process: no constraint. UOS patches are LGPL-2.1-or-later and their source must be offered with any binary. A helper built against Wine's libraries follows LGPL terms. |
| Proton | Gaming-focused Wine distribution | https://github.com/ValveSoftware/Proton | Top level `BSD-3-Clause`; a bundle of components each under its own licence (Wine, DXVK, VKD3D-Proton, FFmpeg, GStreamer and others); `lsteamclient` carries Valve's proprietary Steamworks SDK licence [S6] | Fetched by user (through umu-launcher or Steam) | No | Not uniformly open source. UOS must not redistribute a Proton build until the Steamworks SDK terms and every bundled component have been reviewed. Starting it as a separate process: no constraint. |
| DXVK | Direct3D 8 to 11 on Vulkan | https://github.com/doitsujin/dxvk | `Zlib` [S7] | Inside Proton (fetched by user); Fedora package for plain Wine | No | Its DLLs load into the Windows application, not into UOS code: no constraint. Altered versions must be marked as altered. |
| VKD3D-Proton | Direct3D 12 on Vulkan | https://github.com/HansKristian-Work/vkd3d-proton | `LGPL-2.1-or-later` [S8] | Inside Proton (fetched by user) | No | As DXVK: no constraint on UOS code. Patches are LGPL-2.1-or-later. |
| Darling | macOS compatibility (research track) | https://github.com/darlinghq/darling | Darling's own code `GPL-3.0-or-later`; 151 submodules under their own licences, many of them Apple open-source releases: submodule licences unverified [S9] | Built by UOS (Fedora does not carry it, ADR-0001) | No | UOS integration must be a separate process. Research patches are GPL-3.0-or-later or the submodule's licence, kept in a marked directory. Every submodule needs its own licence check before a binary is shipped. No Apple proprietary framework may be added. |
| Mesa | Open graphics stack | https://gitlab.freedesktop.org/mesa/mesa | `MIT` for the core and most code; individual files under other licences [S10] | Base image | Yes, 26.2.6 | Used through OpenGL and Vulkan: no constraint. |
| KDE Plasma (plasma-workspace, plasma-desktop) | Desktop | https://invent.kde.org/plasma | Mixed, stated per file. Programs such as `plasmashell`, System Settings and the screen locker: `GPL-2.0-or-later`. Library headers such as libplasma's: `LGPL-2.0-or-later`. Also `GPL-2.0-only`, `GPL-3.0-only`, `LGPL-2.1-only`, `LGPL-3.0-only`, `BSD-2-Clause`, `BSD-3-Clause`, `MIT`, `CC0-1.0` [S11] | Base image | Yes, 6.8.2 | **Plugins run inside GPL programs.** An applet, look-and-feel package, lock-screen or splash QML, System Settings module or KRunner plugin written by UOS must be GPL-compatible and cannot be noncommercial. Anything copied from Breeze or Plasma keeps its upstream licence. A standalone UOS program that only links LGPL libraries may stay noncommercial. See [licence-review.md](licence-review.md#4-the-noncommercial-licence-and-the-components-uos-ships-with). |
| KWin | Wayland compositor | https://invent.kde.org/plasma/kwin | `GPL-2.0-or-later` for the compositor and its scripting engine; some files `LGPL`, `BSD`, `MIT` [S12] | Base image | Yes, 6.8.2 | **Effects, scripts (JavaScript or QML), decorations and plugins run inside KWin**: GPL-compatible only. A Wayland client or a D-Bus caller is a separate program: no constraint. Avoid a fork (specification §17). |
| UDisks2 | Disk management | https://github.com/storaged-project/udisks | Daemon and tools `GPL-2.0-or-later`; `libudisks2` `LGPL-2.0-or-later` [S13] | Base image | Yes, 2.11.2 | D-Bus: no constraint. A UDisks daemon module would be GPL. |
| libblockdev | Block device operations | https://github.com/storaged-project/libblockdev | `LGPL-2.1-or-later` [S14] | Base image | Yes, 3.5.0 | Linking is allowed on LGPL terms; reaching it through UDisks: no constraint. |
| PipeWire | Audio and video graph | https://gitlab.freedesktop.org/pipewire/pipewire | `MIT`; except the ALSA SPA plugin (LGPL) and `libjackserver` (GPL-2.0) [S15] | Base image | Yes, 1.6.11 | No constraint. |
| NetworkManager | Networking | https://gitlab.freedesktop.org/NetworkManager/NetworkManager | Daemon `GPL-2.0-or-later`; `libnm` `LGPL-2.1-or-later` [S16] | Base image | Yes, 1.58.1 | D-Bus or `libnm`: no constraint beyond LGPL terms. A daemon plugin would be GPL. |
| BlueZ | Bluetooth | https://github.com/bluez/bluez | `GPL-2.0-or-later`, including the `libbluetooth` headers; a few files `LGPL-2.1-or-later` [S17] | Base image | Yes, 5.88 | D-Bus: no constraint. Do not link `libbluetooth` from noncommercial code: it is GPL, not LGPL. |
| fwupd | Firmware updates | https://github.com/fwupd/fwupd | `LGPL-2.1-or-later` [S18] | Base image | Yes, 2.1.9 | D-Bus or `libfwupd`: no constraint beyond LGPL terms. |
| Flatpak | Application packaging and sandboxing | https://github.com/flatpak/flatpak | `LGPL-2.1-or-later` [S19] | Base image | Yes, 1.18.4 (RPM version `1.19.0^really1.18.4`) | Command line, D-Bus or `libflatpak`: no constraint beyond LGPL terms. Applications from a remote such as Flathub are fetched by the user under each application's licence; some are proprietary. UOS ships none of them. |
| Bubblewrap | Process sandboxing | https://github.com/containers/bubblewrap | `LGPL-2.0-or-later` in the source; the repository's `COPYING` is the LGPL-2.1 text and Fedora tags the package `LGPL-2.1-or-later` [S20] | Base image | Yes, 0.13.1 | Executed as a separate program: no constraint. |
| XDG Desktop Portals | Desktop permission and portal interfaces | https://github.com/flatpak/xdg-desktop-portal | `LGPL-2.1-or-later`; some files `GPL-2.0-or-later`, `MIT`, `CC0-1.0` [S21] | Base image | Yes, 1.22.1 (KDE backend 6.8.2) | D-Bus: no constraint. A portal backend is its own D-Bus service, so a UOS backend may be noncommercial. |
| Calamares | Possible installer foundation | https://codeberg.org/Calamares/calamares (the GitHub repository in §17 is retired) | `GPL-3.0-or-later` for the program and `libcalamares`; some files `LGPL-2.1-only`, `LGPL-3.0-or-later`, `BSD-2-Clause`, `MIT`, `CC0-1.0`, `CC-BY-4.0` [S22] | Not shipped (evaluation) | No | **Modules load into a GPL-3.0 program.** A C++ or Python module, view step or QML branding written by UOS must be GPL-3.0-compatible. Noncommercial installer logic must be a separate program that a thin GPL module starts. |
| QEMU | Windows guest back end | https://github.com/qemu/qemu | `GPL-2.0-only` as a whole; parts `GPL-2.0-or-later`, `BSD`, `MIT`; bundled firmware under separate licences [S23] | Fedora package | Partly: only `qemu-guest-agent` and `qemu-user-static-aarch64` 11.1.3; no system emulator | Driven over QMP, the command line or libvirt: no constraint. |
| libvirt | Virtual machine management | https://github.com/libvirt/libvirt | C API `LGPL-2.1-or-later`; some other parts `GPL-2.0-or-later` [S24] | Fedora package | No | Socket API or linking the LGPL library: no constraint beyond LGPL terms. |
| Looking Glass | Guest display (expert option) | https://github.com/gnif/LookingGlass | `GPL-2.0-or-later`; includes the `kvmfr` kernel module (`MODULE_LICENSE("GPL v2")`) [S25] | Not shipped (evaluation); built by UOS if adopted | No | Separate program: no constraint. `kvmfr` is an out-of-tree kernel module, so it needs an ADR and reopens Secure Boot signing (ADR-0001). The Windows-side host program is a GPL binary that would go into the user's guest. |
| apfs-fuse | Read-only APFS access | https://github.com/sgan81/apfs-fuse | `GPL-2.0-or-later`; bundles Apple's lzfse as a submodule: lzfse licence unverified [S26] | Not shipped (optional); built by UOS if adopted | No | A FUSE driver is a separate process: no constraint. |
| linux-apfs-rw | APFS write research | https://github.com/linux-apfs/linux-apfs-rw | `GPL-2.0-only` (`MODULE_LICENSE("GPL")`) [S27] | Not shipped (research branch only) | No | Out-of-tree kernel module: GPL only, never noncommercial, never enabled by default (AGENTS.md). |

## Projects proposed by ADR-0007 (app-centric routing)

ADR-0007 is a proposal in an open pull request, not an accepted decision. Every
row here is **proposed**.

| Project | Role in UOS (proposed) | Source | Licence | Route | In image | Boundary |
|---|---|---|---|---|---|---|
| umu-launcher | Runs Proton outside Steam | https://github.com/Open-Wine-Components/umu-launcher | `GPL-3.0-only`: the licence file is the GPL-3.0 text and the package metadata says GPLv3; no "or later" statement found [S28] | Fedora package, or built by UOS | No | Started from the command line: no constraint. At run time it downloads the Steam Linux Runtime and a Proton build into the user's home directory; those are fetched by the user and not shipped by UOS. |
| Waydroid | Android runtime | https://github.com/waydroid/waydroid | `GPL-3.0-or-later` [S29] | Fedora package, or built by UOS | No | Driven through its command line: no constraint. Waydroid contains no Android: `waydroid init` downloads LineageOS-based system and vendor images (default type `VANILLA`). See "Android system images" below. |
| FreeRDP | Displays guest applications as windows (RDP RemoteApp) | https://github.com/FreeRDP/FreeRDP | `Apache-2.0` [S30] | Base image | Yes, 3.33.1 (`freerdp`, `freerdp-libs`) | Permissive. UOS may start it or link its libraries; keep the notices. |
| WinApps | Candidate for the seamless Windows guest | https://github.com/winapps-org/winapps | Most files `AGPL-3.0-only` ("or later" not stated). **Some files remain from the original project, which has no licence and is all rights reserved**, by the project's own statement [S31] | Not shipped (evaluation) | No | **Flagged.** Code adapted from it would be AGPL and could not be noncommercial, and the unlicensed remainder makes any redistribution uncertain. Use it only as an unmodified tool the user installs, or not at all. |
| WinBoat | Candidate for the seamless Windows guest | https://github.com/winboat-org/winboat | `MIT` [S32] | Not shipped (evaluation) | No | Permissive: may be adapted with notices kept. It is an Electron application in beta that depends on a container engine, dockur/windows and FreeRDP. |
| Flatpak | First route for Linux applications | See the shortlist table | | | | |
| Distrobox | Containers of other distributions | https://github.com/89luca89/distrobox | `GPL-3.0-only`: the licence file is the GPL-3.0 text; no "or later" statement found [S33] | Fedora package | No | Started from the command line: no constraint. |
| Toolbx and Podman | The same job as Distrobox, already present | https://github.com/containers/toolbox, https://github.com/containers/podman | `Apache-2.0` [S34] | Base image | Yes, toolbox 0.3 and podman 6.1.3 | Permissive; no constraint. Container images of other distributions are fetched by the user. |

## Projects the shortlist does not name but the inventory depends on

| Project | Role in UOS | Source | Licence | Route | In image | Boundary |
|---|---|---|---|---|---|---|
| Fedora Linux 45 Kinoite | Base image (ADR-0001) | https://quay.io/repository/fedora/fedora-kinoite | A collection: each of the 1,750 packages has its own licence ([image-licences.csv](image-licences.csv)). `fedora-release-*` is `MIT`. `fedora-logos` is `LicenseRef-Fedora-Logos` [S35] | Base image | Yes | **`fedora-logos` may not be distributed with anything but unmodified Fedora packages**, and Fedora's trademark guidelines require `fedora-logos` and `fedora-release` to be removed from a derived distribution. Both are in the image today. They must be replaced before any image is published. |
| Qt 6 | Toolkit for UOS's own user interface (specification §9.1) | https://www.qt.io | Most modules `LGPL-3.0-only OR GPL-3.0-only` (or commercial). Some modules are `GPL-3.0-only` and not available under the LGPL: Qt Graphs, Qt HTTP Server, Qt Lottie Animation, Qt Network Authorization, Qt Quick 3D, Qt Quick 3D Physics, Qt Quick Timeline, Qt Virtual Keyboard, Qt Wayland Compositor and others [S36] | Base image | Yes, 6.11.2 | A UOS program may link the LGPL modules and stay noncommercial on LGPL-3.0 terms (dynamic linking, the user can replace the library, notices). **It must not use a GPL-only Qt module.** Check the module, not the RPM tag. |
| KDE Frameworks 6 | Libraries under Plasma; likely used by UOS's own user interface | https://invent.kde.org/frameworks | Library code `LGPL-2.1-only OR LGPL-3.0-only OR LicenseRef-KDE-Accepted-LGPL`, or `LGPL-2.1-or-later`, `BSD`, `MIT` by KDE policy [S37]; individual frameworks carry other files, see the CSV | Base image | Yes, 6.31.0 | Linking is allowed on LGPL terms. Check each framework before relying on this: several `kf6-*` packages also declare GPL parts. |
| GRUB 2 | Bootloader in the base image | https://www.gnu.org/software/grub/ | `GPL-3.0-or-later` [S38] | Base image, signed by Fedora | Yes, 2.12 | UOS supplies configuration only: no constraint. |
| shim | First-stage UEFI loader | https://github.com/rhboot/shim | `BSD-2-Clause` by the upstream file; Fedora tags the package `BSD-3-Clause` [S39] | Base image, signed | Yes, 16.1 | No constraint. |
| Steam Linux Runtime | Container that Proton runs in under umu-launcher (proposed) | https://gitlab.steamos.cloud/steamrt/steam-runtime-tools | Tools: `LGPL-2.1-or-later` overall, with `MIT`, `Apache-2.0` and `Zlib` files. Licence of the runtime's library content: unverified [S40] | Fetched by user | No | Not shipped by UOS. |
| Android system images for Waydroid | Android userspace (proposed) | The `ota.waydro.id` channels set in Waydroid's configuration | LineageOS-based; licence of the images as distributed: unverified. The `GAPPS` image type adds Google's applications, which are licensed only by agreement with Google [S29] [S41] | Fetched by user | No | UOS ships no Android image. It must never ship, preselect or automate the `GAPPS` type. |
| dockur/windows | Installs Windows in a container; used by WinApps and WinBoat (proposed) | https://github.com/dockur/windows | `MIT`. It downloads a Windows installation image from Microsoft at run time and states that the user is responsible for holding a valid Windows licence [S42] | Not shipped (evaluation) | No | The scripts are permissive. What they automate, obtaining and installing Windows, is governed by Microsoft's terms: an open question for legal review. |
| virtio-win guest drivers | Storage, network and display drivers inside a Windows guest | https://github.com/virtio-win/kvm-guest-drivers-windows | Source `BSD-3-Clause`. Terms for redistributing the prebuilt, signed driver image: unverified [S43] | Fetched by user, pending review | No | Not shipped until the redistribution terms of the signed binaries are confirmed. |
| Microsoft Windows | Guest operating system for the seamless tier (proposed) and dual boot | Microsoft | Proprietary. Licence terms unverified | Supplied by the user | No | Never shipped (AGENTS.md). Whether a user's licence permits use in a guest is to be confirmed (ADR-0007, Verification). |
| Apple macOS and frameworks | None | Apple | Proprietary. Licence terms unverified here; ADR-0007 records that macOS guests are allowed only on Apple hardware | Not shipped, not used | No | Never shipped (AGENTS.md). |

## Corrections the specification needs

These are observations for whoever next edits §17; this change does not edit the
specification.

| Where | Observation |
|---|---|
| §17, Calamares | The GitHub repository says development has moved to https://codeberg.org/Calamares/calamares. |
| §17, Proton | "Integrate as optional runtime channel" should say that UOS does not redistribute Proton; the user's machine fetches it. |
| §17, table | FreeRDP, umu-launcher, Waydroid, Qt, GRUB and shim are relied on but not listed. |

## Sources

Every licence above was read from the project's own repository or site on
**12 November 2026**, at the default branch as it stood that day. Where the table
quotes a package version or a Fedora licence tag, the source is the image SBOM
(CI run 43147702795, artifact `uos-image-metadata`).

| Key | Project | What was checked | URL |
|---|---|---|---|
| S1 | Linux kernel | `COPYING`; licensing rules | https://github.com/torvalds/linux/blob/master/COPYING ; https://github.com/torvalds/linux/blob/master/Documentation/process/license-rules.rst |
| S2 | systemd | `LICENSES/README.md` | https://github.com/systemd/systemd/blob/main/LICENSES/README.md |
| S3 | bootc | `crates/lib/Cargo.toml` licence field; `LICENSE-APACHE`, `LICENSE-MIT` | https://github.com/bootc-dev/bootc/blob/main/crates/lib/Cargo.toml |
| S4 | OSTree | `README.md` (Licensing); `COPYING` | https://github.com/ostreedev/ostree/blob/main/README.md |
| S5 | Wine | `LICENSE`; `COPYING.LIB` | https://github.com/wine-mirror/wine/blob/master/LICENSE |
| S6 | Proton | `LICENSE`, `LICENSE.proton`, `dist.LICENSE`, `.gitmodules`, `README.md`; `lsteamclient/LICENSE` | https://github.com/ValveSoftware/Proton/blob/HEAD/LICENSE ; https://github.com/ValveSoftware/Proton/blob/HEAD/lsteamclient/LICENSE |
| S7 | DXVK | `LICENSE` | https://github.com/doitsujin/dxvk/blob/master/LICENSE |
| S8 | VKD3D-Proton | `COPYING`; `LICENSE` | https://github.com/HansKristian-Work/vkd3d-proton/blob/master/COPYING |
| S9 | Darling | `README.md`; `LICENSE`; header of `src/startup/darling.c`; `.gitmodules` | https://github.com/darlinghq/darling/blob/master/README.md ; https://github.com/darlinghq/darling/blob/master/src/startup/darling.c |
| S10 | Mesa | `docs/license.rst` | https://gitlab.freedesktop.org/mesa/mesa/-/blob/main/docs/license.rst |
| S11 | KDE Plasma | `LICENSES/` of plasma-workspace, plasma-desktop and libplasma; headers of `shell/main.cpp`, systemsettings `app/main.cpp`, kscreenlocker `greeter/main.cpp`, libplasma `src/plasma/applet.h` | https://invent.kde.org/plasma/plasma-workspace/-/tree/master/LICENSES ; https://invent.kde.org/plasma/plasma-workspace/-/blob/master/shell/main.cpp ; https://invent.kde.org/plasma/libplasma/-/blob/master/src/plasma/applet.h |
| S12 | KWin | `LICENSES/`; headers of `src/main_wayland.cpp` and `src/scripting/scripting.cpp` | https://invent.kde.org/plasma/kwin/-/tree/master/LICENSES ; https://invent.kde.org/plasma/kwin/-/blob/master/src/scripting/scripting.cpp |
| S13 | UDisks2 | `COPYING`; `README.md` | https://github.com/storaged-project/udisks/blob/master/COPYING |
| S14 | libblockdev | `README.md` (License); `LICENSE` | https://github.com/storaged-project/libblockdev/blob/master/README.md |
| S15 | PipeWire | `LICENSE`; `COPYING` | https://gitlab.freedesktop.org/pipewire/pipewire/-/blob/master/LICENSE |
| S16 | NetworkManager | `CONTRIBUTING.md` (licensing); `COPYING`; `COPYING.LGPL` | https://gitlab.freedesktop.org/NetworkManager/NetworkManager/-/blob/main/CONTRIBUTING.md |
| S17 | BlueZ | `COPYING`; `COPYING.LIB`; headers of `src/main.c` and `lib/bluetooth/bluetooth.h` | https://github.com/bluez/bluez/blob/master/lib/bluetooth/bluetooth.h |
| S18 | fwupd | `COPYING`; header of `src/fu-main.c` | https://github.com/fwupd/fwupd/blob/main/src/fu-main.c |
| S19 | Flatpak | `COPYING`; header of `app/flatpak-main.c` | https://github.com/flatpak/flatpak/blob/main/app/flatpak-main.c |
| S20 | Bubblewrap | `COPYING`; header of `bubblewrap.c` | https://github.com/containers/bubblewrap/blob/main/bubblewrap.c |
| S21 | XDG Desktop Portals | `LICENSES/`; header of `desktop-portal/xdp-main.c` | https://github.com/flatpak/xdg-desktop-portal/tree/main/LICENSES |
| S22 | Calamares | GitHub `README.md` (migration notice); Codeberg `LICENSES/`; headers of `src/calamares/main.cpp` and `src/libcalamares/Job.h` | https://github.com/calamares/calamares/blob/calamares/README.md ; https://codeberg.org/Calamares/calamares/src/branch/calamares/LICENSES |
| S23 | QEMU | `LICENSE` | https://github.com/qemu/qemu/blob/master/LICENSE |
| S24 | libvirt | `README.rst` (License); `COPYING`; `COPYING.LESSER` | https://github.com/libvirt/libvirt/blob/master/README.rst |
| S25 | Looking Glass | `LICENSE`; headers of `client/src/main.c` and `module/kvmfr.c` | https://github.com/gnif/LookingGlass/blob/master/module/kvmfr.c |
| S26 | apfs-fuse | `LICENSE`; header of `apfsfuse/ApfsFuse.cpp`; `README.md` | https://github.com/sgan81/apfs-fuse/blob/master/apfsfuse/ApfsFuse.cpp |
| S27 | linux-apfs-rw | `LICENSE`; `super.c` | https://github.com/linux-apfs/linux-apfs-rw/blob/master/super.c |
| S28 | umu-launcher | `LICENSE`; `pyproject.toml` classifiers; `README.md` | https://github.com/Open-Wine-Components/umu-launcher/blob/main/pyproject.toml |
| S29 | Waydroid | `LICENSE`; headers of `waydroid.py` and `tools/actions/initializer.py`; `tools/config/__init__.py` (image channels and default type); `README.md` | https://github.com/waydroid/waydroid/blob/main/tools/config/__init__.py ; https://docs.waydro.id/faq/google-play-certification |
| S30 | FreeRDP | `LICENSE` | https://github.com/FreeRDP/FreeRDP/blob/master/LICENSE |
| S31 | WinApps | `LICENSE.md` (preamble and text); `COPYRIGHT.md` | https://github.com/winapps-org/winapps/blob/main/LICENSE.md |
| S32 | WinBoat | `LICENSE`; `README.md` | https://github.com/winboat-org/winboat/blob/main/LICENSE |
| S33 | Distrobox | `COPYING.md` | https://github.com/89luca89/distrobox/blob/main/COPYING.md |
| S34 | Toolbx, Podman | `COPYING`; `LICENSE` | https://github.com/containers/toolbox/blob/main/COPYING ; https://github.com/containers/podman/blob/main/LICENSE |
| S35 | Fedora | Trademark guidelines; `fedora-logos`, `generic-logos` and `generic-release` package descriptions; licence approval criteria (firmware) | https://docs.fedoraproject.org/en-US/legal/trademarks/ ; https://src.fedoraproject.org/rpms/fedora-logos/blob/rawhide/f/fedora-logos.spec ; https://src.fedoraproject.org/rpms/generic-release/blob/rawhide/f/generic-release.spec ; https://docs.fedoraproject.org/en-US/legal/license-approval/ |
| S36 | Qt 6 | "Qt Licensing" | https://doc.qt.io/qt-6/licensing.html |
| S37 | KDE Frameworks | KDE licensing policy | https://community.kde.org/Policies/Licensing_Policy |
| S38 | GRUB 2 | `COPYING` in Fedora's source tree for the package | https://github.com/rhboot/grub2/blob/HEAD/COPYING |
| S39 | shim | `COPYRIGHT` | https://github.com/rhboot/shim/blob/main/COPYRIGHT |
| S40 | Steam Linux Runtime | `COPYING` of steam-runtime-tools | https://gitlab.steamos.cloud/steamrt/steam-runtime-tools/-/blob/main/COPYING |
| S41 | Google Mobile Services | "GMS is only available through a license with Google" | https://www.android.com/gms/ |
| S42 | dockur/windows | `license.md`; `readme.md` ("Is this project legal?") | https://github.com/dockur/windows/blob/master/readme.md |
| S43 | virtio-win | `LICENSE` of the driver source; packaging `README.md` | https://github.com/virtio-win/kvm-guest-drivers-windows/blob/master/LICENSE ; https://github.com/virtio-win/virtio-win-pkg-scripts/blob/master/README.md |

### Not verified

| Item | Why |
|---|---|
| Licences of Darling's 151 submodules | Too many to check here; each needs its own review before Darling is built for distribution. |
| lzfse (inside apfs-fuse) | Submodule not opened. |
| Library content of the Steam Linux Runtime | Only the tools' licence file was read. |
| Android system images served by Waydroid | No licence statement for the images was found on the pages checked. |
| Redistribution terms of the signed virtio-win driver image | The packaging page describes how the binaries are built and signed, not their licence. |
| Microsoft Windows and Apple macOS licence terms | Not read. ADR-0007 states Apple's restriction; it is repeated here on that authority only. |
| Whether Fedora 45 packages Wine, QEMU's system emulator, libvirt, umu-launcher, Waydroid and Distrobox | Not checked; "Fedora package" in the Route column is the intended route, not a finding. |
