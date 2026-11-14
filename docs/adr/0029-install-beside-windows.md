# ADR-0029: Installing beside Windows on the same disk: the free space Windows made, its EFI system partition shared, Windows in TUR's boot menu

- **Status:** Proposed
- **Date:** 2026-11-13; owner's decisions 2026-11-14
- **Specification sections:** §5.2, §6.1 to §6.3, §13.3 (dual boot), §19.5,
  §28.1, §28.5; issue 64 (step 1), issue 76, issue 99; amends
  [ADR-0018](0018-first-installer-medium.md) and
  [ADR-0025](0025-live-session-and-graphical-installer.md)

## Context

The owner's test PC has Windows 10 on its only disk and asked to install TUR
beside it (issue 64). The medium of ADR-0018 erases one whole disk; its only
answer for keeping Windows is a second disk. Issue 64 plans two steps:

1. **Install into free space** that the person made with Windows' own
   "Shrink Volume", share Windows' EFI system partition (ESP) without
   touching Windows' files in it, and put a "Windows" entry in TUR's boot
   menu.
2. **Shrink Windows from the installer** with `ntfsresize`, only when it is
   safe (step 2, below; not decided here).

Constraints:

- AGENTS.md: never a destructive storage operation without a confirmation
  that shows the exact target and its stable identity; storage-critical work
  needs fault-injection and recovery tests; destructive tests only on
  disposable disk images. Priority 1 (boot and recover) over everything.
- The installed system must keep what ADR-0013 and ADR-0014 were proven on:
  Fedora's signed shim and GRUB installed by bootupd, GRUB's environment
  block on the ext4 root, TUR's counting fragment numbering the menu entries
  (`default=1` is the previous deployment).
- No Microsoft binary may be shipped or used in tests (AGENTS.md).
- Nothing is started outside CI. Real Windows cannot run in CI; what CI
  proves must be stated exactly.

### State of upstream, 13 November 2026

Read for this decision in the source of the versions Fedora 45 ships, not
recalled.

| Subject | What upstream does | Source |
|---|---|---|
| `bootc install to-filesystem` | Installs into a mounted, otherwise empty filesystem: the target must be a mount point; without `--replace` the root may hold only `lost+found` and directories that contain only mount points (an empty directory is refused). `root=UUID=` is taken from the mounted filesystem; the disk is the filesystem's backing device. The installation's last step is the boot loader, through bootupd. Unchanged between 1.17.1 and 1.17.4 (Fedora 45's current). | [S1] `install.rs`: `install_to_filesystem`, `require_empty_rootdir`, `require_dir_contains_only_mounts` |
| `--replace=alongside` | Calls `clean_boot_directories`, which mounts the ESP and **removes its whole `EFI` and `loader` directories** (`clean_esp_bootloader_dirs`, with an upstream TODO "keep other OSes' `EFI/<vendor>`"). On a Windows disk that deletes `EFI/Microsoft`. **Never to be used beside Windows.** `to-existing-root` is the same with the running system's root as target; not applicable to an installer. | [S1] `install.rs` |
| `--generic-image` | Installs every boot loader type (BIOS too: GRUB's i386-pc image into the disk's first sector and a BIOS boot partition) and skips the firmware. Without it bootc passes `--update-firmware --auto` to bootupd: EFI only when the machine was started by UEFI, and a firmware boot entry. | [S1] `bootloader.rs`, `install_via_bootupd`; [S2] `bootupd.rs`, `get_components_impl` |
| bootupd and an existing ESP | Fedora 45 ships bootupd 0.3.2. For the disk it is given it takes **the first partition whose type is the ESP GUID**, mounts it at a temporary place (or uses one mounted at `<root>/boot/efi`), and copies its EFI tree with `cp -rp`: `EFI/fedora/{shimx64,grubx64,mmx64}.efi`, `BOOTX64.CSV`, `grub.cfg`, `bootuuid.cfg`, and `EFI/BOOT/{BOOTX64.EFI,fbx64.efi}`. Nothing is removed. FAT ignores case, so **Windows' fallback copy of its boot manager, `EFI/Boot/bootx64.efi`, is overwritten with shim**. | [S2] `efi.rs` (`install`, `mount_esp_device`); [S3] `find_partition_of_esp`; [S4] |
| bootupd's firmware entry | `efibootmgr` deletes every entry whose label is the product name (from `/etc/system-release`: "TUR") and creates one with `--create`, which puts it **first** in `BootOrder`. Other entries ("Windows Boot Manager") are not touched. Skipped when efivars is read-only or empty. | [S2] `efi.rs`, `update_firmware`, `clear_efi_target`, `create_efi_boot_entry` |
| bootupd's GRUB configuration | Static: `grub-static-pre.cfg` (`timeout=1`, `timeout_style=menu`), then the drop-ins in name order: `01_users`, TUR's `08_tur_boot_count`, `10_blscfg` (the deployments), `14_menu_show_once`, `30_uefi-firmware`, `41_custom`, which **sources `$prefix/custom.cfg`** (`/boot/grub2/custom.cfg`) after every other entry. | [S2] `src/grub2/` |
| Fedora 45's GRUB under Secure Boot | The signed `grubx64.efi` has `chain`, `fat`, `part_gpt`, `search_fs_uuid` built in. `chainloader` reads the file with the shim_lock verifier (`GRUB_FILE_TYPE_EFI_CHAINLOADED_IMAGE` is checked), which asks shim to verify it against `db`/MOK; shim makes SBAT optional for an image verified through its protocol. Windows Boot Manager is signed with Microsoft's Windows keys, which a PC that starts Windows under Secure Boot has in `db`. | [S5] `grub.macros`; [S6] `chainloader.c`, `kern/efi/sb.c`; [S7] `pe.c`, `verify_sbat_section` |
| BitLocker and a boot manager started by GRUB | Starting Windows through shim and GRUB changes the TPM measurements BitLocker sealed its key to (PCR 7), so Windows asks for the recovery key. Fedora bug 2049849 ("needs to use BootNext instead of chainloader") is still NEW (last change 3 Nov 2026); Ubuntu's 2144897 (March 2026) is open. GRUB has no command that sets `BootNext`. | [S8], [S9] |
| Windows' disk layout | ESP, MSR (16 MB), Windows, recovery partition (`DE94BBA4-…`) "placed immediately after the Windows partition". So Shrink Volume leaves the free space **between Windows and the recovery partition**. Windows Setup makes a 100 MiB ESP on disks with 512-byte sectors (300 MiB on 4K-native ones); Microsoft's OEM guidance now asks 200 MB / 300 MB. | [S10], [S11] |

## Options considered

**How to put TUR into the free space.**

1. **`bootc install to-disk` on the whole disk** — erases Windows. Not this
   task.
2. **`to-filesystem --replace=alongside`** — deletes the ESP's `EFI`
   directory, Windows' boot manager with it. Rejected.
3. **Partition the free space with TUR's installer, make ext4 in it, then
   `bootc install to-filesystem` into it** (no `--replace`, no
   `--generic-image`). bootc and bootupd find the disk's ESP and add TUR's
   boot loader to it. The root is made as bootc makes it in `to-disk`
   (`mkfs.ext4 -L root -O verity`), so the installed root is the layout
   ADR-0013 and ADR-0014 were proven on, minus the BIOS boot partition and
   with Windows' ESP instead of TUR's own. **Chosen.**
4. **Anaconda** — rejected in ADR-0018 for reasons that still hold.

**The ESP.** Share Windows' (chosen; a second ESP on one disk is legal but
firmware and Windows handle it unevenly, and bootupd takes the first one
anyway). Refuse when the disk has not exactly one ESP, when it holds no
`EFI/Microsoft/Boot/bootmgfw.efi`, when it already holds `EFI/fedora`
(another Fedora-family system's, which bootupd would overwrite), or when its
free space is less than TUR's boot loader (measured from the files bootupd
copies, each rounded up to 4 KiB) plus the copy of Windows' fallback loader
plus 4 MiB.

**Windows' fallback loader, `EFI/Boot/bootx64.efi`.** (a) Leave it replaced
by shim, as Fedora's own installer does: if the firmware ever forgets its
entries, shim's fallback recreates TUR's entry. (b) **Put Windows' file back
after bootupd** (a copy beside it, then a rename): every file Windows had on
the ESP stays byte-identical. Chosen: the task is "without touching Windows'
files". The cost: TUR then depends on its firmware entry; if the firmware
loses it, Windows starts and TUR is reached from the firmware's boot menu.

**The "Windows" entry.** (a) A drop-in in the image that searches every disk
for `bootmgfw.efi` at each start: an image change, a search over every disk
at every start, and an entry that comes and goes with USB disks. (b) os-prober:
bootupd's configuration is static; no. (c) **A file the installer writes,
`/boot/grub2/custom.cfg`, naming the ESP by its filesystem UUID**: read by
bootupd's own `41_custom.cfg` after all other entries, so the entry numbers
boot counting uses do not move. Chosen. It is GRUB script, so its template
is BSD-3-Clause in `installer/iso/third-party-licensed/grub/`. It has the key
`w`, and raises GRUB's timeout to 5 s so there is time to choose.

**The firmware boot order.** bootupd puts "TUR" first and leaves "Windows
Boot Manager" second (issue 76 asked whether TUR should leave the order as it
found it). **The person chooses** (owner's decision, below): both installers
ask which system starts by default, and the installer sets the firmware's
boot order with `tur-dualboot default tur|windows` (the chosen system first,
the other second, every other entry after them in its old order). GRUB's
default is not changed: it would move the entry numbers boot counting uses.
The same command changes it later on TUR.

**Starting Windows from TUR without the BitLocker prompt.** GRUB cannot set
`BootNext`, and a Windows started through shim and GRUB is measured
differently. **"Restart into Windows"** (owner's decision, below):
`tur-dualboot restart-windows` sets the firmware's `BootNext` to "Windows
Boot Manager" and restarts; the firmware starts Windows' own boot manager
once, as it does at power-on, and the boot after follows the boot order
again (UEFI: `BootNext` is deleted when it is used). The "Windows" entry of
TUR's menu stays, and on a disk that holds BitLocker its title says
"Windows (may ask for the BitLocker recovery key)".

**The order of writing, for a power cut at any moment.** Record (the GPT and
the SHA-256 of every file on the ESP), then one GPT entry for the free space
(`sfdisk --append`: GPT keeps two copies with checksums, so a cut leaves the
old table or the new one), then ext4 inside it, then bootc (whose last step
writes the ESP and the firmware entry), then Windows' fallback file back,
then the "Windows" entry, then the check that every file and every partition
entry Windows had is unchanged. Windows' partitions are never opened for
writing; the ESP is writable only inside bootupd and for the one rename.

## Decision

TUR's installer medium offers, for a disk that holds Windows (an ESP with
`EFI/Microsoft/Boot`) and has at least **25 GiB** of free space, **"Use the
free space beside Windows"**, and asks which system starts by default. With its own typed phrase, `BESIDE` and the end of
the disk's serial number, it makes one partition in the largest free
stretch, ext4 in it, runs

```sh
bootc install to-filesystem --source-imgref oci:/run/initramfs/live/payload:payload \
    --target-imgref <name> <the new partition, mounted>
```

shares Windows' ESP, puts Windows' `EFI/Boot/bootx64.efi` back, writes
"Windows" into TUR's boot menu, checks that every partition entry and every
ESP file Windows had is unchanged, and sets the default system. The image
carries `/usr/bin/tur-dualboot` (status, `default tur|windows`,
`restart-windows`), which the installer uses and the person uses later.
Changing the default needs an administrator's password (polkit action
`org.tur.dualboot`, `auth_admin_keep`). "Restart into Windows" does not: it
is no more than restarting, which the person at the computer may already do,
so its own action, `org.tur.dualboot.restart-windows`, naming a helper that
does nothing else (`/usr/libexec/tur/tur-restart-windows`), is `yes` for an
active local session and `auth_admin` otherwise (the lead's decision, 14
November 2026). Both installers do it, from
`engine.sh` (`beside_look`, `run_beside`). Nothing else changes: erasing a
whole disk still runs `to-disk` as before.

**Refusals**, each with its code (medium-only codes, not yet in the registry
crate):

| Code | When |
|---|---|
| `TUR-INSTALL-BESIDE-001` | The disk has no GPT (Windows starts the BIOS way) |
| `TUR-INSTALL-BESIDE-002` | Not exactly one ESP, or no Windows Boot Manager on it |
| `TUR-INSTALL-BESIDE-003` | The largest free stretch is smaller than 25 GiB; the message says how to shrink Windows |
| `TUR-INSTALL-BESIDE-004` | The ESP has too little free space for TUR's boot loader |
| `TUR-INSTALL-BESIDE-005` | The ESP already holds `EFI/fedora` |
| `TUR-INSTALL-BESIDE-006` | After writing, an entry or a file Windows had is different (the installation stops and says so) |

## Consequences

- A person can keep Windows on the same disk, if they first make room in
  Windows. The installer does not shrink Windows (step 2).
- The installed root is ADR-0013's layout without the BIOS boot partition:
  `--auto` installs the EFI boot loader only. BIOS boot from this disk is not
  possible (TUR is UEFI-only, §4.1).
- The computer starts the system the person chose. From TUR, "Restart into
  Windows" (`pkexec tur-dualboot restart-windows`) starts Windows through
  the firmware, without the BitLocker prompt and without a password; "Windows" in TUR's menu starts
  it through GRUB, and **with BitLocker Windows may then ask for its
  recovery key** (upstream bug above): the entry's title says so. A page in
  TUR's Settings for both is a follow-up (TUR Settings is another
  component); until then the command line.
- bootupd records shim as `EFI/BOOT/BOOTX64.EFI` in `/boot/bootupd-state.json`.
  The base image's `bootloader-update.service` runs `bootupctl update` at
  start (TUR only keeps it from running during a trial boot), and bootupd
  applies the difference between its record and the new files to the ESP:
  **a later update that changes shim writes shim over Windows'
  `EFI/Boot/bootx64.efi` again**. Windows' own entry, which starts
  `EFI/Microsoft/Boot/bootmgfw.efi`, is not affected. Recorded as a
  follow-up: put Windows' copy back after such an update.
- `sfdisk` writes the protective MBR again with the GPT; on a disk Windows
  partitioned its bytes may change (it holds no Windows data). Measured and
  reported, not judged.
- Windows grows its recovery partition in place by shrinking the Windows
  partition when an update needs it; TUR's partition sits between them. The
  documented manual procedure (shrink C:, recreate the recovery partition
  right after it) still works; Windows' automatic resize may not. Not
  measured.
- Leftovers of a power cut: an unfinished "TUR" partition in the free space,
  which the installer then shows as a partition, not as free space. It is
  removed in Windows' Disk Management. The installer does not yet offer to
  reuse it.
- **Case-insensitive folders for Windows applications (issue 99, issue 81).**
  Installing into a partition TUR makes itself opens the way the whole-disk
  installation lacked: the installer chooses the filesystems. Proposed,
  **not implemented here**, because it changes the layout boot counting was
  proven on and needs those proofs again:
  - a separate `/boot`, ext4 without `casefold`, 1 GiB (GRUB reads only it;
    the environment block moves there; `tur-boot-health` already remounts
    `/boot` read-write in a private namespace to write it, and bootc's
    `to-filesystem` supports a separate `/boot` mount);
  - the root made with `mkfs.ext4 -O verity,casefold -E encoding=utf8`. The
    feature only allows case-insensitive directories; a Wine prefix is made
    case-insensitive with `chattr +F` on its empty directory.

  Simpler alternatives, in increasing distance from today's layout: an ext4
  image file with `casefold` under `/var` mounted by the Windows runtime (no
  installer change, ADR-0025's option 3); a separate partition for
  `/var/home` (bootc writes no fstab line for it: TUR would). Whichever is
  chosen applies to both ways of installing, needs its own ADR, and the boot
  test, the rollback scenarios and this ADR's tests on the new layout.

## Verification

Shown by the `iso` workflow (jobs `beside` and `beside-faults`; results in
[TUR-011](../reports/TUR-011.md)), on a disposable disk image laid out as
Windows leaves a disk after Shrink Volume
(`tests/iso/make-windows-shared-disk.sh`): 100 MiB ESP holding 26 MiB of
files under Windows' names and a stand-in for `bootmgfw.efi` and
`EFI/Boot/bootx64.efi` (TUR's own test program, `tests/iso/standin`, not a
Microsoft binary), MSR, NTFS made by ntfs-3g with known files, 30 GiB free,
recovery partition at the end:

- refusals: no free space, a too-full ESP: listed with the reason, not
  offered, the disks byte-identical;
- the default system: TUR chosen in the text installer (TUR's menu at the
  first start), Windows chosen in the graphical one (the firmware starts the
  stand-in at the first start, the boot order is Windows Boot Manager, TUR);
- `tur-dualboot default windows`, `default tur` and `restart-windows`, run
  as root from the medium's command line after an installation: BootNext
  set, the stand-in started once by the firmware, then the next start is TUR
  again, BootNext gone;
- the text installer and the graphical one install beside Windows; the erase
  phrase does not confirm it;
- `tests/iso/windows-disk.py` compares the disk with what it held before,
  after the installation, after TUR's first start and after "Windows" was
  chosen in TUR's menu: every partition entry, the bytes of every partition
  but the ESP, every file on the ESP;
- TUR starts from the disk; choosing "Windows" (key `w`) starts the
  stand-in, which reports the file it was started from and the partition's
  UUID, compared with the ESP's;
- a power cut (QEMU stopped at once) at each step: after each, the same
  comparison, the firmware's "Windows Boot Manager" entry, and what the disk
  then starts.

**Not shown**, recorded as not run:

- Real BitLocker: whether Windows asks for its key when started from TUR's
  menu, and that it does not after "Restart into Windows". **NOT RUN.**
- `tur-dualboot` run on an installed TUR through pkexec (it was run as root
  from the medium, whose system is the same image).
- Real Windows: that Windows Boot Manager started from TUR's menu goes on to
  start Windows; that Windows accepts the shared ESP and the new partition;
  BitLocker's prompt; Fast Startup or hibernation; Windows' own updates of
  its recovery partition and its ESP afterwards.
- Secure Boot with the "Windows" entry: the stand-in is not signed, so it
  can only be started without Secure Boot; that GRUB starts a
  Microsoft-signed boot manager under Secure Boot is read in the source, not
  run.
- A power cut that tears a sector: QEMU's process ends between writes; a
  real disk can lose part of one. A real power cut while the ESP is being
  written can damage the FAT, which this order makes short but cannot
  remove.
- Real firmware: whether it keeps the entry bootupd adds, and how it shows
  the boot order.

## Owner's decisions, 14 November 2026

1. **Boot order: the person decides.** Both installers ask which system
   starts by default, TUR or Windows; the installer sets the firmware's boot
   order; `tur-dualboot default` changes it later. A Settings page is a
   follow-up.
2. **BitLocker: "Restart into Windows"** through the firmware's `BootNext`
   (`tur-dualboot restart-windows`). The GRUB entry stays, titled to say it
   may ask for the BitLocker recovery key when the disk holds BitLocker.
3. **Shrinking Windows from the installer is to be offered** (step 2, pull
   request 116): ntfsresize on the medium, the choice in both installers,
   every refusal in plain words with what to do, a size picker that keeps
   the minimum, a typed confirmation, and a strong recommendation to back up
   first. Tested on volumes made by mkfs.ntfs, not on a real Windows volume.
4. **Minimum free space for TUR: 25 GiB.**

## Step 2: shrinking Windows from the installer

Offered (owner's decision 3); built in pull request 116 and recorded in
TUR-012: `ntfsresize` (ntfs-3g), only
after `ntfsresize --info` and `--no-action` succeed; refusing a BitLocker
volume (`-FVE-FS-`) and a hibernated or dirty volume (Fast Startup); keeping
a margin over the reported minimum; resizing the filesystem first, then the
partition entry, so that a power cut between the two leaves a smaller
filesystem in an unchanged partition, which Windows accepts. Real-Windows
safety cannot be claimed from CI without a real Windows image.

## Sources

| | |
|---|---|
| S1 | `crates/lib/src/install.rs`, `crates/lib/src/bootloader.rs` at tags v1.17.1 and v1.17.4 of https://github.com/bootc-dev/bootc |
| S2 | `src/efi.rs`, `src/bootupd.rs`, `src/grub2/` at tag v0.3.2 of https://github.com/coreos/bootupd; Fedora 45: rust-bootupd-0.3.2-1.fc45 (Bodhi, stable 14 Sep 2026) |
| S3 | `crates/blockdev/src/blockdev.rs` at tag v1.17.1 of https://github.com/bootc-dev/bootc (`bootc-internal-blockdev`, which bootupd uses) |
| S4 | https://src.fedoraproject.org/rpms/rust-bootupd (branch f45: Version 0.3.2) |
| S5 | `grub.macros`, branch f45 of https://src.fedoraproject.org/rpms/grub2 (`platform_modules` for x86_64: `chain`; `grub_modules`: `fat`, `part_gpt`, `search_fs_uuid`) |
| S6 | `grub-core/loader/efi/chainloader.c`, `grub-core/kern/efi/sb.c`, branch fedora-45 of https://github.com/rhboot/grub2 |
| S7 | `pe.c` of https://github.com/rhboot/shim (main) |
| S8 | https://bugzilla.redhat.com/show_bug.cgi?id=2049849 |
| S9 | https://lists.snapcraft.io/archives/foundations-bugs/2026-March/546735.html (Launchpad bug 2144897) |
| S10 | https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/configure-uefigpt-based-hard-drive-partitions |
| S11 | https://wiki.archlinux.org/title/Dual_boot_with_Windows |
