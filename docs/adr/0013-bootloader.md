# ADR-0013: Bootloader — GRUB 2 through shim, installed by bootupd

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §4.1, §4.3, §5.1, §5.3, §5.4, §23 (question 1), §27.1

## Context

The specification left the bootloader open (question 1: systemd-boot or GRUB) and
wrote its update and rollback design around systemd-boot boot counting (§5.3).
ADR-0001 chose Fedora Kinoite on bootc and required this question to be answered
with evidence, not assumed.

The evidence now exists. UOS-003 installed the image to a disk with the installer
the image carries and booted it under UEFI in CI (run 43153380759, report
[UOS-003](../reports/UOS-003.md)). What was measured:

- `bootc install to-disk` (bootc 1.17.1) reported `Bootloader: grub` and
  `Installing bootloader via bootupd`. Nothing was passed to choose a bootloader.
- The EFI system partition holds Fedora's shim and GRUB
  (`EFI/BOOT/BOOTX64.EFI`, `EFI/fedora/shimx64.efi`, `EFI/fedora/grubx64.efi`) and
  a small `grub.cfg` that chains to `/boot/grub2/grub.cfg` on the root
  filesystem. bootupd reports `grub2-1:2.12-83.fc45, shim-16.1-7`. The boot
  test records whether each binary has a signature attached; no firmware has
  verified one yet (see Verification).
- The kernel is started from a Boot Loader Specification entry that OSTree wrote
  (`/boot/loader/entries/ostree-1.conf`), read by GRUB's `blscfg`.
- The first boot goes through the removable-media path (`EFI/BOOT/BOOTX64.EFI`,
  which is shim). Shim's fallback then creates a firmware entry named "Fedora"
  for `\EFI\fedora\shimx64.efi` and puts it first in the boot order; the second
  boot went through that entry.
- In the booted system `bootctl` names the running loader as `GRUB 2.12` and
  lists its features: **boot counting is not among them**.
- The image contains no systemd-boot (UOS-002).
- The GRUB configuration bootupd installs has no boot counter. GRUB's environment
  block exists and GRUB can read and write it.

What upstream documents for this version:

- bootc installs GRUB through bootupd, or systemd-boot through `bootctl`.
  systemd-boot "is only supported for Composefs Backend and not for Ostree".
  This image is deployed with the OSTree backend.
- For the OSTree backend, bootc relies on OSTree's own failure detection
  (`ostree-finalize-staged.service`, `ostree-boot-complete.service`), which
  detects a deployment that failed to be finalised. It does not detect a
  deployment that boots badly. `bootc rollback` swaps the boot order when
  something asks it to.
- greenboot (the Fedora package, since 0.16 a Rust implementation "designed for
  bootc based systems") provides health checks before `boot-complete.target`, a
  GRUB snippet installed through bootupd that counts boot attempts in the GRUB
  environment block and falls back to the previous deployment when the counter
  runs out, and the rollback itself. It is not in the Kinoite base.
- systemd's automatic boot assessment needs a loader that implements boot
  counting, which here means systemd-boot.

## Options considered

1. **GRUB 2 loaded by shim, installed by bootupd.** What the installer does
   today, with no UOS code. These are the binaries Fedora ships for Secure
   Boot, so its chain should keep working as long as UOS ships them unmodified
   (ADR-0001); that is still to be shown. BIOS boot is available from the same disk. Cost: no systemd-boot
   boot counting, so rollback on a bad boot must be built on GRUB's environment
   block; and the ESP directory and firmware entry are named "fedora"/"Fedora"
   by the signed binaries themselves.
2. **systemd-boot with the current OSTree backend.** Would give the boot
   counting the specification assumed. Not supported by bootc 1.17, and not in
   the image.
3. **systemd-boot with bootc's composefs backend and unified kernel images.**
   The direction upstream is moving in, and the one that fits "signed UKI where
   practical" (§4.1). It changes the storage backend, needs UOS to own a signing
   chain (Fedora does not sign these for UOS), and the composefs backend does
   not configure boot counting yet either. Too much to take on before UOS has
   update, rollback and recovery working at all.

## Decision

UOS boots through Fedora's shim and GRUB 2, installed and updated by bootupd, as
`bootc install` sets it up for this base. systemd-boot is not adopted now.

Boot health checking and automatic rollback (UOS-005, §5.4) are designed around
this stack: a boot-attempt counter in GRUB's environment block, cleared by the
health gate when the boot is good. greenboot is evaluated first as the existing
mechanism to wrap, before UOS writes its own.

## Consequences

- Specification question 1 is answered for the current base: GRUB. The
  systemd-boot boot counting in §5.3 does not apply; the "attempt counter" of
  stage BOOT-010 (§27.1) comes from the GRUB environment block.
- UOS writes no bootloader code. Kernel arguments are machine-local state set at
  install time (`bootc install --karg`) or image state in
  `/usr/lib/bootc/kargs.d`.
- The installed layout is the installer's default: a GPT with a 1 MiB BIOS boot
  partition, a 512 MiB FAT EFI system partition and one root partition typed as
  "Linux root (x86-64)". There is no separate `/boot`; one would appear with
  LUKS. The UOS installer (UOS-006) will drive `bootc install to-filesystem` and
  decide the layout itself, including whether to keep BIOS boot at all (§4.1
  targets UEFI).
- The base image sets no default root filesystem. The installer must choose one;
  the boot test passes `--filesystem ext4` only so that it has something to
  boot.
- **Branding.** The firmware boot menu will say "Fedora" and the ESP will carry
  `EFI/fedora`, because those names are part of Fedora's signed shim and GRUB
  packages. Changing them means building and signing UOS's own shim and GRUB.
  This bears on the rule that no image may be published before Fedora's marks
  are replaced, and needs a decision of its own before a public image.
- bootc does not update the bootloader after installation; bootupd does, when
  asked. The update design (§5.3, §15.1) must say when it is asked.
- This decision is tied to the OSTree backend. A move to the composefs backend
  with unified kernel images reopens it, by a new ADR.

## Verification

- Shown in CI on every change that can affect boot: the `boot` workflow fails
  unless the bootloader is identified from the firmware variables, the ESP
  content and the loader entries.
- **Not yet shown:** Secure Boot. The test firmware has Secure Boot unavailable
  (`disabled (unsupported)`). A boot with Secure Boot enforced and Microsoft's
  keys enrolled must pass before this ADR's claim about Fedora's signed chain is
  relied on.
- **Not yet shown:** anything about rollback. UOS-005 must demonstrate, in a VM,
  a deployment that fails its health gate being replaced by the previous one
  within a bounded number of attempts, and must confirm at that point which
  greenboot version Fedora 45 ships and that it works with this image.
- **Not yet shown:** real firmware. Shim's fallback and the boot order behave
  differently across vendors; the hardware runner (ADR-0002) has to cover it.
- Re-check on each rebase to a new Fedora release that bootc still selects GRUB
  for this base, and what the composefs backend then supports.
