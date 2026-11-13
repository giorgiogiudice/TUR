# ADR-0018: The first installer medium — a small live environment that runs the image's own installer

- **Status:** Proposed
- **Date:** 2026-11-12
- **Specification sections:** §4.1, §4.3, §5.1, §5.2, §6.1 to §6.3, §14, §19.5,
  §28; tasks TUR-006 and D-23
- **Amended:** 2026-11-13 by [ADR-0025](0025-live-session-and-graphical-installer.md);
  2026-11-13 by [ADR-0028](0028-install-beside-windows.md) (installing beside
  Windows on the same disk, into free space Windows made; erasing a whole disk
  is unchanged)

> **Amendment, 13 November 2026.** The medium described here now starts, by
> default, a live TUR desktop that opens a graphical installer;
> the text installer of this ADR is the boot menu's second entry, "Install
> TUR (text mode)". Both run the same `bootc install to-disk` command from one
> library, `installer/iso/rootfs/usr/lib/tur-installer/engine.sh`. The
> installer's log is now kept on the installed disk. The rest of this ADR
> (the medium's shape, the payload as an OCI layout, the boot chain, the
> safety rules, the error codes) holds unchanged. See ADR-0025 and
> [TUR-009](../reports/TUR-009.md).

## Context

TUR can be built, installed to a virtual disk with `bootc install to-disk`
and started to a graphical login, all in CI
([TUR-003](../reports/TUR-003.md), [TUR-005](../reports/TUR-005.md)). Boot
health and automatic rollback are in the image
([ADR-0014](0014-boot-health-and-rollback.md)). Nothing can yet put TUR on a
real computer. A test PC is waiting for a USB stick.

Two pieces of TUR's own installer exist and neither installs anything: a
planning library that by design writes to no disk
([installer.md](../architecture/installer.md)), and the design of a graphical
installer (task D-23). Waiting for them means no hardware result for a long
time, and priority 3 of `AGENTS.md` (hardware works well) cannot start
without one.

Constraints:

- The destructive-operation rules of `AGENTS.md` and specification §5.2 and
  §28.5 apply to any installer, however small.
- The installed disk must be what the boot test installs today: the layout
  ADR-0014's boot counting was proven on. GRUB's environment block must be
  on ext4 on a plain partition, and bootupd must write TUR's counting
  fragment into `grub.cfg`.
- The Secure Boot chain stays Fedora's unmodified signed shim, GRUB and
  kernel ([ADR-0001](0001-base-distribution.md), [ADR-0013](0013-bootloader.md)).
- Installation works offline (§6.1, §14.1).
- Nothing is built or started outside CI ([ADR-0002](0002-ci-and-build-infrastructure.md)),
  so every design is judged by what CI can prove about it.
- The image may not be distributed while it contains Fedora's marks (issue
  11). An installer environment that adds more of them makes that worse.

### State of upstream, 12 November 2026

Fetched for this decision, not recalled.

| Subject | What upstream says | Source |
|---|---|---|
| `bootc install` | "The installation commands MUST be run from the container image that will be installed." `to-disk` creates a GPT with an EFI system partition and a root filesystem and is "mainly a demo" for `to-filesystem`, which external installers use. | [S1] |
| `--source-imgref` | With it "`bootc` no longer assumes that it runs inside podman"; the image is fetched from the given reference. "`bootc install` still has to be run inside a chroot created from the container image." The install configuration and ostree's `prepare-root.conf` "are read from the root `bootc` runs in, also with `--source-imgref`". | [S1], and the source of bootc 1.17.1 [S2] |
| Kernel arguments in the image | Read from the image being deployed (`/usr/lib/bootc/kargs.d` in the pulled commit), not from the root bootc runs in. | bootc 1.17.1 source [S2] |
| image-builder ISO types for bootc images | `generic-iso` is the recommended type: it turns a bootable container into an ISO whose root filesystem is that container, as `LiveOS/squashfs.img`. The container must bring a kernel, an initramfs with dracut's `dmsquash-live` module (package `dracut-live`), shim and GRUB. `--bootc-installer-payload-ref` copies a second image into the ISO's container storage for offline installation. `bootc-installer` is "historical"; `anaconda-iso` is "not available in image-builder". | [S3] |
| What runs the installation on such an ISO | Upstream's example puts Anaconda in the container, with a kickstart that runs `bootc --source-imgref containers-storage:...`. The page warns that a bootc system installed through Anaconda fails to start `systemd-remount-fs.service`. | [S3] |
| bootc-image-builder | Its sources "are now located in the osbuild/image-builder repository"; the two projects "share a lot of code" and are expected to merge. | [S4] |
| Anaconda's `bootc` kickstart command | Anaconda prepares the storage, then runs the equivalent of `bootc install to-filesystem`. Documented through kickstart only; layouts spanning several disks, arbitrary mount points and authenticated registries are not supported; Anaconda "treats any image as a Fedora installation". The older `ostreecontainer` command deploys with ostree and has Anaconda run `bootupctl` itself. | [S5] |
| Fedora 45 Atomic Desktop ISOs | Built by Image Builder since Fedora 45; they use the Anaconda WebUI; they are still `boot.iso` images "with an embedded `ostree`-commit". Building them from bootable containers is future work. | [S6], [S7], [S8] |
| Others | Universal Blue's newer ISOs are described as live images whose installer wraps `bootc install`. LWN's article on bootc for workstations recommends starting "a live system, such as a Fedora Workstation live image" to run `bootc install` for the first time. | [S9], [S10] |

## Options considered

1. **An Anaconda ISO**, built by image-builder (`generic-iso` with Anaconda in
   the container) or from Fedora's own `boot.iso` with a kickstart.
   A mature installer with every feature the specification lists.
   - It offers what TUR has not tested: manual partitioning, encryption,
     Btrfs, installing beside another system. An installer must not offer an
     untested layout, least of all for the partition that holds GRUB's
     environment block. Hiding those screens means maintaining an Anaconda
     configuration; running it unattended from a kickstart means erasing a
     disk with no confirmation.
   - Its default layout on Fedora is not the one ADR-0014 was proven on, and
     upstream documents a defect of systems installed this way [S3].
   - It installs through a path the boot test does not use (Anaconda's
     storage code, then `to-filesystem`), so today's proof would not carry
     over.
   - It brings Anaconda's own Fedora branding on top of the image's.
   - Nothing of it is reused by TUR's installer.
2. **image-builder `generic-iso` with TUR's installer instead of Anaconda.**
   The upstream tool for exactly this shape of medium. Not chosen *now*: it
   could only be evaluated through CI runs, its command-line interface for
   bootc ISOs is new and still moving (two of its three ISO types are
   already historical), and what it does to the root filesystem (labels,
   ownership, where the payload goes) would have had to be discovered
   rather than read. The medium built here deliberately has the same shape
   (shim, GRUB, `LiveOS/squashfs.img`, `dmsquash-live`), so moving to the
   tool later changes the build script, not the medium.
3. **A small live environment that runs `bootc install to-disk` itself,
   assembled by a script in this repository.** About 500 lines of shell
   around `skopeo`, `xorriso`, `mksquashfs` and `mtools`, and a text
   installer of about 800 lines of shell. The
   installation is the command the boot test already proves, on the layout
   ADR-0014 needs, with nothing else on offer. Every input is visible and
   recorded. Costs: TUR owns the assembly; a text screen, in English, with a
   US keyboard; none of the specification's other installation modes.
4. **A raw disk image for a USB stick, made by `bootc install to-disk`**, so
   that the stick is itself an installed TUR that starts an installer. It
   reuses the proven boot path entirely. Rejected: the stick is writable and
   changes each time it is used; it needs a 16 GB stick and writes all of it;
   and bootupd's layout runs shim's fallback program, which adds a boot
   entry for the stick to the computer's firmware.
5. **Wait for TUR's own installer** (D-23 on `tur-installer-core`). No
   hardware result until an executor, a session model and a user interface
   exist and are proven.

Within option 3, two questions were decided by what bootc documents:

- **What root does `bootc` run in?** The image being installed, because
  bootc takes the bootloader files, GRUB's configuration fragments and its
  installation settings from that root. So the installer environment is
  built `FROM` the payload image and adds only `dracut-live` (which brings
  `dracut-network` and `fuse` with it), an initramfs for the medium and the
  installer. A smaller, different environment would
  have installed the wrong bootloader configuration silently. The build
  stops if any file `bootc install` reads differs between the two.
- **Where does it read the image from?** An OCI layout directory on the
  medium, with `--source-imgref`. The alternative is the boot test's exact
  invocation (`podman run` from container storage). In a live environment
  that needs a second, read-only container store, overlay mounts on top of
  an overlay root, and room in memory for each layer as it is copied. The
  OCI layout needs none of that and is plain files whose names are their
  SHA-256, which the installer can check before it erases anything. The
  price is size: the image is on the medium twice, once as the environment's
  root filesystem and once as the layout.

## Decision

The first installer medium is option 3: an ISO 9660 image with a GPT and an
appended EFI system partition, started under UEFI through Fedora's unmodified
signed shim and GRUB, whose root filesystem is the TUR image plus Fedora's
`dracut-live` and a text installer on the first console. The installer erases
one whole disk, after a typed confirmation that names the disk, by running

```sh
bootc install to-disk --source-imgref oci:/run/initramfs/live/payload:payload \
    --target-imgref <name> --generic-image --wipe --filesystem ext4 <disk>
```

It is defined in `installer/iso/`, built and proven by the `iso` workflow, and
described for its user in
[install-on-a-test-pc.md](../guides/install-on-a-test-pc.md).

**What the medium does, in order**

1. Checks every file of the image on the medium against its SHA-256, and the
   image's index against a hash fixed inside the installer environment.
2. Checks the computer: started by UEFI, x86-64, at least 3 GiB of memory
   (the rule in `tur-installer-core`). Reports the Secure Boot state.
3. Lists whole disks, each with its size, model, connection, the end of its
   serial number and what it holds in plain words: Windows (an EFI system
   partition holding `EFI/Microsoft/Boot`), BitLocker (detected by blkid, or
   the signature `-FVE-FS-` at byte 3 of a volume), NTFS, Linux
   filesystems. Finding this out only reads: signatures through `lsblk`, a
   few bytes with `head`, and an EFI system partition mounted read-only in
   a private mount namespace to look for that directory. A disk can be
   chosen only if it is not the medium, not in use, not read-only, has a
   `/dev/disk/by-id` link that carries its serial number or WWN, and is at
   least 20 GiB (the size the boot test installs to). A disk that cannot be
   chosen is shown with the reason and its error code.
4. Shows the chosen disk's model, capacity, connection, serial number, stable
   path and every partition on it, and what will be written. If the disk
   holds Windows or BitLocker, a separate warning: "This disk holds Windows.
   Erasing it removes Windows and everything on it." Requires the phrase
   `ERASE` followed by the last eight letters and digits of the serial
   number: the rule of `tur-installer-core`.
5. Looks at the disk again immediately before writing: same stable path,
   serial number, size and content, still not in use.
6. Runs the command above, showing its output. Checks that the disk then has
   an EFI system partition and a root partition.
7. Powers off when asked.

**What it does not do**, stated on its first screen and in the guide:
installation on the same disk as another operating system (shrinking it),
encryption, manual partitioning, keeping any data on the chosen disk, any
use of a network, languages other than English, keyboard layouts other than
US, BIOS boot.

**Beside Windows, on a second disk.** The supported way to keep Windows is to
install TUR on another disk (internal, or an external USB SSD). What the
medium does to the rest of the computer:

- **Other disks.** It writes only to the chosen disk. `bootc install
  to-disk` partitions and formats the device it is given, and bootupd
  installs the boot loader into the EFI system partition it finds on that
  device. Proven in CI by hashing the whole Windows-like disk before and
  after the installation and after TUR's first start.
- **Firmware boot entries, during the installation: none written.** bootc
  1.17.1 passes `--update-firmware` to `bootupctl backend install` only
  without `--generic-image` (`crates/lib/src/bootloader.rs`: `bootupd_opts =
  (!configopts.generic_image).then_some(["--update-firmware", "--auto"])`),
  and its documentation for `--generic-image` says "Changes to the system
  firmware will be skipped". bootupd runs `efibootmgr` only in its
  `update_firmware` path (`src/efi.rs`). The medium always passes
  `--generic-image`. Checked in CI by reading the firmware's variable file
  after the installation.
- **Firmware boot entries, at TUR's first start: shim's fallback adds one.**
  The installed disk is first started through the removable-media path
  (`EFI/BOOT/BOOTX64.EFI`, which is shim), whose fallback program creates a
  boot entry for TUR (named "Fedora", ADR-0013) and puts it first in the
  boot order. It does not change or delete other entries. TUR-007 records
  what CI measured. So after the first start the computer starts TUR by
  default, until the owner changes the order in the firmware setup.
- **TUR's boot menu does not list Windows.** The firmware's one-time boot
  menu chooses the system.

**Not implemented from the specification**, and not claimed: the phase model
and session directory (§6.2, §28.3), the diagnostics drawer and bundle export
(§6.1, §28.4), resume after interruption (§28.1), copying the installer's
log to the installed system. The installer's log exists only in memory until
power-off. These belong to TUR's installer.

**Error codes.** A condition that already has a code in
`installer/tur-installer-core/src/codes.rs` uses it (`TUR-INSTALL-PREFLIGHT-001`,
`-003`, `-005`, `-006`, `-007`, `-008`, `-012`; `TUR-INSTALL-TARGET-006`,
`-007`; `TUR-INSTALL-CONFIRM-001`, `-004`; `TUR-INSTALL-IDENTITY-001` to
`-003`). Three conditions exist only here:

| Code | Meaning |
|---|---|
| `TUR-INSTALL-MEDIUM-001` | The image on the medium is missing, incomplete, damaged or not the one the medium was built with |
| `TUR-INSTALL-MEDIUM-002` | `bootc install` failed, or reported success without leaving the expected partitions |
| `TUR-INSTALL-MEDIUM-003` | No disk on this computer can be installed to |

They are not yet in the registry crate.

## Consequences

- TUR can be installed on a UEFI PC from a USB stick, in one way only.
- The installed disk is produced by the same `bootc install to-disk` as the
  boot test's, so ADR-0013 and ADR-0014 apply to it as they are. The `iso`
  workflow compares the two disks on every run.
- TUR owns a medium-assembly script and a second initramfs (for the medium
  only; the installed system's initramfs is the image's, untouched).
- **The installer environment is the whole image**, 3.3 GB as a
  zstd-compressed SquashFS, and the image is on the medium a second time as
  the payload (3.2 GB of zstd-compressed layers). The medium is 6.7 GB,
  about twice the size it needs to be, and needs an 8 GB stick. Storing the payload so that
  the two share their files is possible and is deferred (see Verification).
- The installer environment starts with SELinux disabled (`selinux=0`): its
  root filesystem carries no labels. bootc then labels the installed system
  from the image's policy, which is what it does in the boot test on every
  run, because the CI host has no SELinux either. The installed system runs
  enforcing.
- The medium is **not bit-for-bit reproducible**: `dracut-live` comes from
  Fedora's repositories as they are on the day of the build. Every input is
  recorded by hash in a manifest beside the medium.
- **Licences.** The installer and its units are TUR programs under the TUR
  licence. The boot menu is interpreted by GRUB and is kept, like TUR's
  other GRUB fragment, in a marked directory under BSD-3-Clause. Everything
  else on the medium is unmodified Fedora packages.
- **Trademarks and distribution.** The medium carries what the image
  carries (issue 11) plus Fedora's signed boot chain on its own EFI system
  partition, and nothing more: no Anaconda, no additional Fedora artwork.
  It must not be published. The workflow uploads it only when started by
  hand with `upload_medium`, for three days.
- **Path to TUR's installer.** D-23 replaces the text program and the single
  command with the graphical installer driving `tur-installer-core` and
  `bootc install to-filesystem`. What carries over: the medium's shape
  (which is the shape the library already recognises as a live medium,
  `/run/initramfs/live`), the build and its manifest, the virtual-machine
  harness, the reference comparison of installed disks, and the refusal
  tests. What does not: the text screens.
- The installed system records `localhost/tur:<build id>` as its image
  origin. There is nothing to update from; a newer build is installed by
  installing again.
- **Beside Windows**, TUR becomes the firmware's first boot entry at its
  first start (shim's fallback). Removing that behaviour means changing what
  bootupd puts on the ESP, which ADR-0013 left as installed; it is recorded
  as a decision the owner may want to reverse, not changed here.

## Verification

Shown by the `iso` workflow on every change (results in
[TUR-007](../reports/TUR-007.md)):

- the medium starts under UEFI with no network device, as a USB stick and as
  an optical drive;
- an installation driven through the keyboard, screen by screen, onto a blank
  disk and onto a disk that holds data;
- a disk that holds data is bit-for-bit unchanged when the confirmation is
  wrong or not given, and a disk that is too small is refused with a message;
- beside a Windows-like disk (no Microsoft file in it) and a "Windows Boot
  Manager" firmware entry: the disk is described as Windows, choosing it
  shows the warning, TUR is installed on the second disk, the Windows-like
  disk is byte-identical afterwards and its firmware entry is unchanged;
- the installed disk has the layout of the boot test's disk, compared
  item by item with a reference installation made in the same run;
- the boot test's own checks, and a boot-health scenario in which a failing
  update is undone, pass on a disk installed from a medium.

**Not shown**, and to be recorded as not run until done:

- Real firmware, real disks, a real USB stick. Whether the medium starts on a
  given computer, and whether that computer's firmware then finds the
  installed disk, is exactly what the test PC is for.
- NVMe, SATA and USB target disks: the virtual disk is virtio.
- Secure Boot on real firmware. In OVMF with Secure Boot enforced and
  Microsoft's keys enrolled, the medium started, installed, and the
  installed system started with its kernel reporting Secure Boot enabled
  (TUR-007). That run is informational in the workflow: it does not fail
  the job.
- The failure screen after a failed `bootc install`: no failure was induced.
- A disk without a serial number, a read-only disk, a disk in use: the
  refusals exist and were not exercised.
- Real Windows, real BitLocker, a real "Windows Boot Manager", and how a
  vendor's firmware orders and shows boot entries.

**To revisit:**

- image-builder's `generic-iso`, at the next Fedora rebase or when Fedora's
  Atomic Desktops are built from bootable containers with it, whichever
  comes first.
- The size of the medium: store the payload as a read-only container store
  inside the environment's filesystem, where identical files are stored
  once.
- A media check at start, as Fedora's ISOs have (`rd.live.check`).
- An entry in the boot menu for computers whose graphics driver blanks the
  console.

## Sources

| | |
|---|---|
| S1 | https://bootc.dev/bootc/bootc-installation.7.html and `docs/src/bootc-installation.7.md` at tag v1.17.1 of https://github.com/bootc-dev/bootc |
| S2 | `crates/lib/src/install.rs` at tag v1.17.1 of https://github.com/bootc-dev/bootc |
| S3 | https://osbuild.org/docs/developer-guide/projects/image-builder/advanced/bootc/isos/ |
| S4 | https://github.com/osbuild/bootc-image-builder (README) and https://github.com/osbuild/image-builder-cli (README, FAQ) |
| S5 | https://fedoramagazine.org/introducing-the-new-bootc-kickstart-command-in-anaconda/ (31 December 2025) |
| S6 | https://fedoramagazine.org/whats-new-for-fedora-atomic-desktops-in-fedora-linux-45/ (3 November 2026) |
| S7 | https://fedoraproject.org/wiki/Changes/BuildAtomicDesktopsWithImageBuilder |
| S8 | https://fedoraproject.org/wiki/Changes/Anaconda_WebUI_Fedora_Atomic |
| S9 | https://universal-blue.discourse.group/t/introducing-bluefins-new-installation-experience/7991 (as summarised by a search result; the page itself was not read) |
| S10 | https://lwn.net/Articles/1042708/ (7 November 2025) |
