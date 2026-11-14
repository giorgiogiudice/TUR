<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Experiment `dualrun`: the person's own Windows, started inside TUR

A time-boxed research spike. Report: `docs/reports/exp-dualrun-1.md`.
Workflow: `.github/workflows/exp-dualrun.yml`.

**This file is the registration.** The hypotheses and kill criteria below were
committed before anything was measured. Results never go here; they go to the
report, with the run they came from.

## The idea

On a computer that already has Windows installed, TUR is installed beside it
(pull request 112) and then starts **that same installation** as a virtual
machine from its real partitions, with the person's programs, files and
licence, instead of restarting into it. Windows stays startable by itself.
The owner's shape for it (14 November 2026): one boot, the person's Windows
kept but *protected, layered and controlled by P@x*; C: stays C:; one
identity on both sides; it must look as if TUR gave Windows superpowers.

## What "native hardware" means here

CI has no PC. Windows is installed unattended into a raw disk image under one
virtual machine configuration that stands in for the PC (profile `pc`: its own
firmware variables, SMBIOS strings, TPM, an emulated NVMe disk given as a
plain file) and is then started from the same disk under a different
configuration that stands in for TUR's guest (profile `guest`: the disk
given through the launcher's device-mapper view, other firmware variables,
other SMBIOS strings unless carried, other TPM, Hyper-V enlightenments). Both
are QEMU/KVM virtual machines. **Real firmware, a real TPM, real BitLocker
sealing, OEM activation, real graphics and a real Windows 10 installation are
NOT RUN until the owner's PC**, and nothing here is to be read as a
bare-metal result (AGENTS.md).

## A real Windows in CI, lawfully (written before use)

What was found on 14 November 2026:

- Microsoft's Evaluation Center offers **Windows 11 Enterprise, version 26H2,
  90-day evaluation** as an ISO. The download page lists direct links on
  Microsoft's own servers; the English x64 one is
  `https://go.microsoft.com/fwlink/?LinkId=2382600&clcid=0x409&culture=en-us&country=us`,
  which redirects to
  `software-static.download.prss.microsoft.com/dbazure/26300.9457.260913-1737.26h2_ge_release_svc_refresh_CLIENTENTERPRISEEVAL_OEMRET_x64FRE_en-us.iso`,
  8,225,329,152 bytes, range requests accepted. The page says "Register, then
  download and install the full-featured software for a 90-day evaluation",
  that no product key is needed, and that the software "is designed for IT
  professionals".
- **The evaluation licence terms are not published on that page.** They are
  inside the medium and shown by Setup. They could not be found on
  Microsoft's site, so they could not be read before a download.
- **Windows 10: no evaluation is offered any more.** The Windows 10 Enterprise
  evaluation page (`…/evalcenter/download-windows-10-enterprise`) answers 301
  to a 2024 blog post about the end of Windows 10 support. No other lawful
  automated source of Windows 10 was found. Windows 10 is NOT RUN in CI; the
  owner's own PC is the Windows 10 test.

The way used, in this order:

1. **Stage `terms`** (a short job): CI downloads the ISO from Microsoft's
   link, records its name, size and SHA-256, and prints the paragraphs of the
   licence text inside it that govern use (evaluation, testing, virtual
   machines, time limit). Nothing is installed. The engineer reads them and
   writes what they say into the report.
2. **Stage `experiment`** only if those terms allow installing the software
   for evaluation and testing. Setup's answer file accepts the terms; the
   run is dispatched by hand, never by a push, and the report names who
   dispatched it and what the terms said. If the terms do not allow it, the
   Windows part is NOT RUN and the experiment is what the synthetic job and
   the owner's-PC procedure can show.
3. The ISO and the installed disk live only on the runner's temporary disk
   and die with it. **Never committed, never cached, never an artifact.**
   Artifacts hold only what TUR's own scripts wrote: JSON, logs of TUR's
   programs, text Windows printed in answer to TUR's queries, and screenshots
   needed as evidence (H5). No file of Microsoft's is copied out: no event
   log file, registry hive, boot file or licence file.
4. No product key is entered and no activation or evaluation timer is reset,
   extended or bypassed (`slmgr /rearm` is never run). Setup's hardware
   checks are met with a virtual TPM 2.0 and Secure Boot firmware, not
   bypassed. The guest has no route to the Internet (QEMU's user network
   with `restrict=on`), so online activation of the evaluation is NOT RUN.
5. A test agent of TUR's own (`guest/agent.ps1`) is installed in that
   Windows by Setup's first-logon command. It is test instrumentation, as a
   person's own program would be. Nothing else is added: no driver, no
   change to boot files.

## Hypotheses (registered before measuring)

**Foundations. Nothing else matters if these fail.**

- **H1 Round trip.** A Windows installed natively (UEFI, GPT, its own EFI boot
  files) starts unmodified as a KVM guest from the same disk when the disk is
  presented on a controller Windows already has a boot driver for, and
  afterwards starts natively again, repeatedly, with no repair.
  *Measured:* at least five native→guest→native round trips; each start's
  time; Windows' own disk check after the last; boot-start drivers and boot
  files compared before and after.
  *Sub-question H1x:* a Windows installed on NVMe, started on emulated AHCI.
  **Kill:** Windows needs repair or reinstallation of boot files, or a
  recovery environment, to start in either direction in the matching
  controller case.
- **H2 Safety.** TUR can refuse reliably and in plain words when: the Windows
  volume is hibernated or was left by Fast Startup; a Windows partition is
  mounted or held by something else; the volume is BitLocker-encrypted and no
  key is available; the volume is dirty or its state cannot be read; memory
  is short. The guest gets the Windows partitions and never TUR's own. Ending
  the guest or the host process at any moment leaves Windows no worse than a
  power cut on real hardware.
  *Measured:* each refusal on synthetic volumes (no Microsoft software) and
  on volumes left by real Windows (hybrid shutdown, hibernation, kill);
  TUR's stand-in partition byte-identical after every session; QEMU killed
  during start, idle, heavy writing and shutdown, the launcher killed while
  the guest runs; then Windows' own disk check in the following start.
  **Kill (unattended use dead):** a state that must be refused is accepted
  (a real hibernated or Fast Startup volume passes the check), or a byte of
  a partition not given to the guest changes, or a kill leaves damage
  Windows' own check cannot repair by itself.
- **H3 Identity.** SMBIOS UUID, serial and model strings, the disk serial and
  the network address can be carried into the guest; with them carried,
  Windows reports fewer changes than without. *Measured:* what Windows
  reports in each configuration (licence status, product identifiers,
  device re-detection counts, hardware-change events). The firmware's
  embedded key table (ACPI MSDM) is passed through by the launcher only on
  request and is NOT RUN in CI (an evaluation medium has none and none is
  fabricated). Nothing is built to defeat licensing; the licence question
  is stated for the owner's lawyer.
  **Kill:** none of its own; a negative result (Windows treats the guest as
  another computer whatever is carried) is reported and decides only how
  the product explains activation.

**Use.**

- **H4 Integration without changing Windows.** With only what Windows has
  built in: memory returned to the host through Hyper-V's dynamic-memory
  protocol (`hv-balloon`), a USB storage device handed to the running
  guest, a clean shutdown when the session is closed, start-up time.
  Clipboard and a shared folder need Remote Desktop or added drivers;
  measured through Remote Desktop where the edition has it.
  **Kill for "no added drivers":** memory cannot be reclaimed and the
  guest cannot be shut down cleanly from outside with built-in drivers.
- **H5 One seamless desktop.** Each Windows application as its own window
  among TUR's windows.
  (a) Editions with Remote Desktop: FreeRDP RemoteApp gives one host window
  per application window, with title and icon; clipboard and a redirected
  folder work.
  (b) Every edition, nothing asked of Windows' licensing components: a TUR
  agent in the guest lists top-level windows (rectangle, title, class,
  order) and captures each window's own content with public Windows APIs;
  the host shows two overlapping guest windows as two host windows,
  routes a click back, and closes one from outside.
  **Kill (a):** RemoteApp against Windows 11 does not yield separate
  windows. **Kill (b):** the content of an occluded window cannot be
  obtained without Remote Desktop; then only a shaped view of the one
  guest screen is possible and per-window seamlessness on Home is dead.
  Windows' Remote Desktop service is never patched or wrapped.
- **H6 Protected.** A session on a copy-on-write layer over the real
  partitions, opened read-only: (1) a destructive change inside Windows,
  then discard: every byte of the disk is as before and Windows starts
  natively as before; (2) a kept session, committed: a native start sees
  the change and the disk check is clean; (3) a commit interrupted by a
  kill, run again: the same result as (2); (4) an undo record taken at
  commit rolls Windows back to before the session; (5) a damaged boot
  configuration is restored from outside from TUR's own copy of the
  person's file.
  **Kill:** (1) a discarded session changes a byte of a Windows partition;
  (2) or (3) a committed session needs repair natively; then layers can
  only ever be thrown away, not kept.
- **H7 One boot.** The guest's part of the start fits a budget a person
  accepts. *Registered budget (targets, not results):* guest ready in
  45 s from a cold start on the owner's class of PC, 10 s from a saved
  state. *Measured in CI:* cold start times (nested virtualisation, so
  slower than a PC), and whether a guest saved to a file by QEMU can be
  resumed with the storage controller in use.
  **Kill for "near-instant":** a saved guest cannot be resumed with any
  controller Windows starts from.
- **H8 Volumes keep their identity.** In the guest, every Windows volume has
  the letter and label it has natively, and TUR's own partition has no
  letter and is not offered for formatting. *Measured:* Windows' volume
  and partition lists in both configurations.
  **Kill:** a letter or label differs, or TUR's partition gets a letter.
- **H9 The same files both ways.** A file written by TUR to the Windows
  volume while the guest is down is there when the guest starts; a file
  written through the guest (TUR's agent, and Windows' own file sharing)
  is on the volume after shutdown; the volume is clean both times.
  **Kill:** either write is missing or the volume is left dirty or
  damaged.
- **H10 Windows alone is untouched.** After guest sessions, a native start
  is not slower, has the same boot-start drivers, the same boot files and
  the same firmware entry as before the first session.
  **Kill:** a guest session leaves a boot-critical change that a native
  start depends on.

**Design only (not hypotheses; stated in the report):** the layer vocabulary
shared with the `layers` spike; what P@x may do to the guest and in which
action class; the choice of which desktop is the face; one sign-in; the
shared identity's source of truth; uninstalling TUR; what the installer of
pull request 112 must change.

## Layout

| Path | What |
|---|---|
| `tur-dualrun` | The prototype launcher: `inspect`, `check`, `attach`, `detach`, `discard`, `commit`, `undo`, `esp-backup`, `vars`, `identity`, `run`, `recover` |
| `dualrunlib/` | Read-only parsers (GPT, NTFS, FAT, registry hive for the boot configuration), firmware variables, the check-list, the device-mapper plan, sessions and layers |
| `guest/` | The answer file template and TUR's test agent for Windows |
| `ci/` | Host preparation, the download and terms stage, the synthetic tests (no Microsoft software), the experiment, the report tables |

## What runs where

- `lint` (on a pull request that touches these paths): syntax, SPDX lines,
  and the synthetic tests: volumes made by `mkfs.ntfs` and `mkfs.vfat` in
  every state the check-list knows, the device-mapper view, the firmware
  variable writer. A few minutes, no KVM, no Microsoft software.
- `terms` and `experiment`: by `workflow_dispatch` only, one runner each.
- Nothing runs on a development machine.
