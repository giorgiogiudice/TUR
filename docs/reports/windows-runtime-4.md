# windows-runtime-4 — Windows applications on the installed TUR system

```text
TUR DEVELOPMENT REPORT

Task ID:          windows-runtime-4 (issues #32, #98, #134: "Run on the TUR image")
Subsystem:        OS image (image/wine), Universal Runtime Manager
                  (services/tur-runtime), tur-apps, boot test (tests/boot)
Build ID:         @BUILD_ID@
Source revision:  @REVISION@ (windows-on-image, pull request 136)

SUMMARY
- What changed: the TUR image carries Wine. Fedora 45's own packages,
  Wine 11.21 for x86-64 in Fedora's "new WoW64" build, pinned by SHA-256
  (image/wine/wine.lock, written by image/wine/lock.py from Fedora's
  repository metadata), downloaded in a build stage and installed with rpm,
  without network, first in the final stage. tur-runtime installs an
  application's launcher entry where Plasma's launcher finds it and removes
  it with the application. The boot test gains a boot of the installed
  disk in which real Windows applications are installed, started, used
  and removed in a signed-in Plasma session with SELinux enforcing.
- Why: tur-runtime was in the image and Wine was not, so on an installed
  TUR no Windows program ran (issue #134, the owner's definition of done).
```

## 1. How Wine gets into the system, and why

Facts read on 14 and 15 November 2026 from Fedora 45's x86-64 repository
metadata (release tree, revision 1793218669; updates, revision
1794615633), the package files themselves (all 82 downloaded and read) and
Fedora's `wine.spec` on the f45 branch:

| | |
|---|---|
| Wine in Fedora 45 | 11.0-3.fc44 in the release tree; 11.20-1.fc45 in updates on 14 November, **11.21-1.fc45 in updates on 15 November** (what a Fedora 45 system installs). Fedora follows Wine's development releases |
| How it is built | `%bcond new_wow64 1` for Fedora ≥ 43: x86-64 Wine is configured with `--enable-archs=i386,x86_64` and installs into `/usr/lib64/wine-wow64/wine/{x86_64-windows,i386-windows,x86_64-unix}`. 32-bit Windows programs run inside the 64-bit Wine through WoW64; no 32-bit Linux library is needed. The i686 `wine-core` (726 MB) still exists, for the old mode, and is not needed |
| What new WoW64 cannot do, upstream | 16-bit Windows programs (Win16, DOS) are not supported in WoW64 mode; 32-bit OpenGL is slower (buffers mapped through 64-bit memory). Neither was measured here |
| `wine-core` x86-64 | 260 MB to download, **1 538 MB installed** (both architectures' DLLs, 836 i386 and 758 x86-64 files) |
| Fedora's `wine` package | A meta package. It also brings `wine-desktop` (makes Wine the program that opens `.exe`/`.msi` files), `wine-systemd` (registers Wine with binfmt_misc, so a Windows program marked executable runs when started), `samba-winbind-clients` (`ntlm_auth`), `wine-mono` and both `wine-gecko`s |
| Wine Mono 11.3.0 | 82 MB / **326 MB installed**. GPL-2.0-or-later AND LGPL-2.1-only AND MIT AND BSD-4-Clause-UC AND MS-PL AND MPL-1.1, built by Fedora from source (no Microsoft binary) |
| Wine Gecko 2.47.4 | 52 MB (x86-64) + 56 MB (i386) installed; MPL-1.1 OR GPL-2.0-or-later OR LGPL-2.1-or-later. Both require `wine-common`, which requires Perl (60 packages) |
| What Wine's MinGW libraries pull in | `mingw64-*` libraries require `mingw64-filesystem`, which requires `mingw-filesystem-base`, `redhat-rpm-config`, 28 `*-srpm-macros`, `cmake-rpm-macros`, `ninja-build`, `mingw-binutils-generic` and `mingw64-crt` (143 MB of static libraries). Fedora's packaging; not avoidable without breaking the package database |
| ntsync | Fedora's kernel builds `ntsync` as a module (`CONFIG_NTSYNC=m`); `ntsync-autoload` (7 bytes) loads it at boot. Wine 11 uses `/dev/ntsync` when it can open it |
| Updates repository | Keeps only the newest build: 18 of the 82 pinned files were already gone from it a day later (HTTP 404), and were fetched from Koji's signed copy, which matched the pinned SHA-256 |

**Decision: in the image, as Fedora's packages, pinned.** Measured in
section 3: +1.92 GB uncompressed (+24 %), about 0.3 GB more to download.

- *In the image, not a system extension or an on-demand install.* Running
  Windows applications is one of the three things TUR is for, and "it just
  works after install" includes offline. An on-demand `rpm-ostree install`
  layers packages on the deployment (slower updates, a reboot, network at
  first use, and bootc's model does not support it). A system extension
  (sysext) must match the exact image version, so TUR would build and
  publish one per release next to the image, with its own SELinux labelling
  and signing: more machinery for the same bytes, and Windows would still
  need a download after installation. The cost of the image route is size
  (section 3), paid once per installation; an update that does not change
  the lock does not change Wine's files, and the layer is first in the final
  stage so that TUR's own changes do not rebuild it.
- *Pinned, with `rpm`, not `dnf install` in the build.* The image build must
  give the same bytes twice (the image workflow compares image IDs); `dnf`
  writes its history and caches with the time in them, and would take
  whatever the mirrors hold that hour. The lock is the same mechanism the
  branding and font packages use, plus a second source (Koji) because the
  updates repository drops old builds within days.
- *11.21 from updates, not 11.0 from the release tree.* It is what a
  Fedora 45 system has, and the base image takes Fedora's updates at every
  re-pin; 11.0 is ten months older. When the base is re-pinned, rerun
  `image/wine/lock.py` in the same pull request: rpm refuses a lock that no
  longer fits the base, so a stale one stops the build rather than the boot.
- *No `wine` meta package, no `wine-desktop`, no `wine-systemd`.* Either of
  the last two would start a Windows program outside tur-runtime's sandbox
  (double-click on an `.exe`, or `./setup.exe`). The smoke test checks they
  are absent.
- *No Mono, no Gecko* — for the owner to decide (section 7). The runtime
  design installs no dependency for an application, Mono and Gecko named
  (runtime-manager.md); every application measured so far runs without them.
- *No i686 Wine.* New WoW64 runs 32-bit programs; measured below with the
  32-bit PuTTY and with the 32-bit NSIS installers of Notepad++ and VLC.
- *`xwininfo` added* (56 kB): tur-runtime's autopilot finds windows with it,
  and the image did not have it.

## 2. What was built

| Where | What |
|---|---|
| `image/wine/lock.py` | Reads Fedora 45's repository metadata (release + updates, never updates-testing), resolves the dependencies of the ten packages TUR wants against the base's package list (from the image's SBOM), writes `wine.lock`: file, SHA-256, size, licence, Fedora path, Koji path, why |
| `image/wine/wine.lock` | 83 packages, 298 881 745 bytes to download (Wine 11.21-1.fc45) |
| `image/wine/wine.sh` | `fetch` (stage `wine`, the only step with network; Fedora's repository, its mirror redirector, then Koji; each checked) and `install` (final stage, no network: checksum and licence again, one `rpm -i --nosignature --notriggers` transaction at the build ID's date, then `wine --version`, the alternatives links, both DLL directories, and that nothing registered Wine for `.exe` files) |
| `image/Containerfile` | Stage `wine`; the install first in the final stage |
| `tests/image/smoke.sh` | Every package of the lock at its version; `wine --version` is 11.x; the links; both architectures' DLLs and WoW64's; the Direct3D DLLs `alternatives` manages; no i686 Wine, Mono, Gecko; no `wine`/`wine-desktop`/`wine-systemd`, binfmt entry or desktop entry for Wine; ntsync; xwininfo |
| `scripts/gen-manifest.py` | `runtime_versions.wine` from `wine-core` |
| `tests/image/marks-allowlist.tsv` | Comments naming Fedora in the RPM macros and MinGW files Wine's dependencies bring (found by reading all 82 packages before the first build) |
| `services/tur-runtime` | `app install` writes the launcher entry as `$XDG_DATA_HOME/applications/tur-wine-<app-id>.desktop` (record: `installed: true`); `app uninstall`, `prefix remove` and a rollback delete it and check it is gone. The executor's guard accepts in the applications directory only `tur-wine-<valid id>.desktop` |
| `services/tur-apps` | The plan names the entry; removal checks it is gone; the test's stand-in tur-runtime installs and removes one |
| `tests/boot/windows.py`, guest `windows-check`, two units | The Windows boot (section 4) |
| `tests/windows-runtime/apps.py`, `autopilot.py` | `--session` (the signed-in desktop's display and directories, host screenshots, KRunner), `--real-only`; Fedora's DLL directory; 32-bit PuTTY in `apps.json` |

## 3. Image size and build time

Measured by the `image` workflow (run 43755443830, commit 479ed7b, build
20261115.g479ed7b2c141) against main's last measured image (run 43545811883,
commit d424be7; main has changed only in TUR's own programs since):

| | main (d424be7) | with Wine (479ed7b) | Change |
|---|---|---|---|
| Image size (podman, uncompressed) | 7 902 362 717 B | 9 824 861 744 B | **+1 922 499 027 B (+24.3 %)** |
| What the Wine step added to the file system (`df` before and after, in the build) | | 1 929 723 904 B | |
| of which `/usr/lib64/wine-wow64` (Wine's DLLs and libraries) | | 1 553 443 042 B | |
| of which `/usr/share/wine` (fonts, NLS, winmd) | | 47 211 321 B | |
| RPM packages in the image | 1 743 | 1 826 | +83 |
| Packages to download for Wine (compressed RPMs; the image layer is of the same order) | | 298 881 745 B | |
| Image build, first build (`Build image` step) | 532 s and 1 402 s (two main runs; the runners vary) | 1 489 s | about +1 to +15 min |
| Image build, second build without cache (reproducibility) | 432 s and 1 296 s | 1 411 s | |
| Reproducible (two builds, same image ID) | yes | **yes** (`2eb3a887…`) | |

The 1.5 GB of Wine's DLLs is Fedora's build: its PE DLLs keep their
debugging information (`wine-core` x86-64 is 260 MB compressed and 1 538 MB
installed; the i686 build of the same version, one architecture, is 726 MB).
Stripping them in TUR's image would change files rpm owns; asking Fedora is a
follow-up. `wine --version` on the image says `wine-11.21 (Staging)`: Fedora
builds Wine with the Wine Staging patch set.

## 4. Measured on the installed, booted system

@BOOT_RESULTS@

## 5. Application centre and launcher

@CENTRE_RESULTS@

## 6. Runs

@RUNS@

## 7. Decisions for the owner

1. **Wine Mono and Gecko in the image?** +326 MB (Mono) and +108 MB plus
   Perl (Gecko) installed; +82 MB and +108 MB to download. With them, .NET
   Framework applications and those that embed Internet Explorer's engine
   can start; without them they fail, and the failure is classified
   (`dotnet-runtime-missing`). It also matters at the first launch after an
   update that moves Wine: Wine updates the prefix then, and with Mono and
   Gecko absent and `mscoree`/`mshtml` not disabled at launch, Wine offers
   to download them from WineHQ (follow-up below). Recommended: ship Mono
   (it is what makes the commonest class of small Windows tools run), not
   Gecko; measure a .NET application first.
2. **Updating Wine.** The lock pins 11.21. Fedora moves Wine every two
   weeks. Recommended: refresh the lock with every base re-pin (one command;
   rpm refuses a lock that does not fit), so Wine follows Fedora's updates
   like the rest of the image.
3. **ntsync is loaded at boot for everyone** (`ntsync-autoload`). Wine 11
   uses it when it can open `/dev/ntsync`. What it did on the image is in
   section 4.

## 8. Not run

- A graphics card: the guest has virtio-vga, everything drawn in software.
- Sound heard: VLC's check writes a file; nothing was played.
- Games, DXVK/VKD3D-Proton, Proton through `umu-run`: not run.
- 16-bit programs (not supported by new WoW64 upstream): not tried.
- A .NET application (no Mono in the image).
- Real hardware, a real disk.
- The application centre's "I have the installer file" path in the
  graphical centre: PR #128 (the desktop programs in the image) is not
  merged; the same route was measured through `tur-apps` on the command line.
- WinSCP (needs an SSH server on the machine; on Ubuntu it is measured).

## 9. Follow-ups

@FOLLOWUPS@
