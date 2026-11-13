# Installer core

**Status:** non-destructive core only. Nothing in this library writes to a
disk; the installer medium's engine does (section 0).
**Component:** `installer/tur-installer-core` (library and the `tur-installer` binary)
**Specification sections:** §5.2, §6.1 to §6.3, §14, §19.5, §28; task TUR-006
**Related decisions:** [ADR-0001](../adr/0001-base-distribution.md), [ADR-0005](../adr/0005-languages-and-workspace.md)

This note describes the part of the installer that decides which disk is meant,
whether it is safe to erase, what would be written and what would be lost, and
that records what the installer did. The part that carries a plan out does not
exist yet. Statements here describe code that CI compiles and tests; anything
not yet verified is listed under [Assumptions](#assumptions-to-verify).

## 0. What installs TUR today: the medium's engine and its two front ends

This library still writes to no disk. What does install TUR is the
installer medium ([ADR-0018](../adr/0018-first-installer-medium.md),
[ADR-0025](../adr/0025-live-session-and-graphical-installer.md)), in three
pieces:

| Piece | Where | Runs as | Does |
|---|---|---|---|
| The engine | `installer/iso/rootfs/usr/lib/tur-installer/engine.sh` | root | Checks the image on the medium against its SHA-256 sums; checks the computer; lists disks with their stable identity and, in plain words, what is on them (read-only); derives the confirmation phrase; looks at the chosen disk again just before writing; runs the one `bootc install to-disk` command; checks the result; keeps the log on the installed disk |
| The text installer | `installer/iso/rootfs/usr/libexec/tur-installer/tur-install` | root, on the first console | Sources the engine. The boot menu's "Install TUR (text mode)" |
| The graphical installer | `desktop/apps/tur-installer` (`tur-installer-wizard`) | the live session's user | The screens of desktop-experience §11.5. Reaches the engine only through `tur-install-engine`, started with pkexec for each step |

`tur-install-engine` is the engine's command line for the graphical
installer: `check`, `disks`, `install --by-id … --serial … --wwn … --size …
--fingerprint … --phrase …`, `sticks`, `save-log --to …`. It writes one JSON
object per line (the format is described at the top of the program and in
`desktop/apps/tur-installer/fixtures/README.md`), reads `cancel` on its
standard input until it starts writing, and logs every line it sends. It
trusts nothing it is told: the disk must still be the one that was listed
(stable path, serial number, WWN, size and a fingerprint of its partitions
and filesystems), and the phrase must be the one it derives itself from the
disk it finds. polkit allows it to the live user only, on the medium only
(`org.tur.installer.engine`).

The rules the engine follows are this library's: the phrase of
`confirmation_phrase` (plan.rs), the minimum memory of preflight.rs, the
error codes of codes.rs where one exists. The engine has five codes of its
own, `TUR-INSTALL-MEDIUM-001` to `-005` (ADR-0018 and ADR-0025), not yet in
the registry crate.

**Beside Windows on the same disk** ([ADR-0028](../adr/0028-install-beside-windows.md)).
For a disk that holds Windows and has at least 20 GiB of free space (made
with Windows' Shrink Volume), the engine offers a second way: `disks` lists,
per Windows disk, `"beside": {possible, code, why, free_start, free_size,
before, after, esp, esp_size, esp_free, esp_needed, phrase}`; `install
--mode beside --region-start B --region-size B ... --phrase "BESIDE ..."`
checks the disk and the free stretch again, then (`run_beside`) records the
GPT and the SHA-256 of every file on the ESP, adds one GPT entry ("TUR",
type Linux root x86-64) for the free stretch, makes ext4 in it, runs `bootc
install to-filesystem` into it (bootupd adds TUR's boot loader to Windows'
ESP and a "TUR" firmware entry first in the boot order), puts Windows'
`EFI/Boot/bootx64.efi` back, writes "Windows" into
`/boot/grub2/custom.cfg`, and checks that every partition entry and every
ESP file Windows had is unchanged. Its state lines start `TUR-INSTALLER:
beside `. Never `--replace=alongside`: it deletes the ESP's `EFI` directory.

The installation log (the engine's log, bootc's output, the graphical
installer's state lines) is copied to `/var/log/tur-installer/<session>/`
on the installed disk (§28.3), with the SELinux label of `/var/log`. It is
not yet the session directory of section 8.

**ext4 `casefold` on the root (issue 81) is not set.** The root filesystem
holds `/boot`, which GRUB reads at every start, and Fedora 45's GRUB refuses
an ext4 filesystem with the `casefold` feature: measured in CI, a root
given it with `tune2fs` stops at GRUB's prompt ("no such device" for the
root's UUID; TUR-009). See ADR-0025 for the ways to issue 81.

The way to the design of this document: the engine's steps become the
phases of section 7, carried out by a `CommandRunner` that this library
records, and the graphical installer talks to that instead of the shell
library. Until then the shell library is the one place that touches a disk.

## 1. What is deliberately not implemented

- **All writing to disk.** No partitioning, formatting, encryption, image
  deployment or bootloader step exists. The crate defines the record of an
  executed command and the `CommandRunner` trait, and has no implementation of
  that trait. A test (`tests/no_destructive_tools.rs`) fails if any line of code
  under `src/` names a tool that changes a disk, or if any file other than
  `system_probe.rs` starts a process.
- **The graphical installer** and the **D-Bus service**.
- **The other installation modes**: "install alongside", "manual" and
  "reinstall preserving data" are defined as `InstallMode` variants and return
  `TUR-INSTALL-PLAN-002`.
- **Continuing an interrupted installation.** Reconciliation examines the disk
  and reports; it never permits carrying on.
- Hardware inventory, GPU and storage-controller preflight checks, swap, a
  recovery partition, the diagnostic bundle, and copying the session to the
  target.

## 2. Storage inventory

The inventory is built from data, not from the machine: the JSON printed by
`lsblk --json --bytes --output-all`, and a map of the links in
`/dev/disk/by-id` and `/dev/disk/by-path`. Both arrive through the `Probe`
trait. Tests use fixture files. `SystemProbe` runs `lsblk` and reads the two
link directories; it is the only code that starts a process, it is read-only,
and no test runs it.

The parser accepts numbers and booleans in the forms different lsblk versions
print them, and refuses an entry without a name or a size in bytes. Stable
paths come from the link map and from lsblk's own `id-link` column, so an
inventory without a link map still has by-id paths.

A disk counts as **in use** when anything on it is mounted, is active swap, or
is held open by an unlocked encrypted volume, an active logical volume, a
multipath map or an assembled array. A disk is the **live medium** when a
filesystem on it is mounted at one of `/run/initramfs/live`,
`/run/initramfs/isoscan` or `/run/install/repo`.

Other operating systems are detected from partition types and filesystem
signatures alone. No volume is mounted.

## 3. Identity: why a device path is never enough

`/dev/sda` says where the kernel put a disk on this boot. The same name can
belong to another disk after a reboot, after a USB device is plugged in, or
between the moment a user confirms and the moment the installer writes. An
installer that remembers a device path can erase the wrong disk without any
component misbehaving.

So the installer never accepts a device path as a target and never carries one
from one step to the next. What it carries is an `Identity`:

| Field | Source |
|---|---|
| `model`, `serial`, `wwn` | reported by the disk |
| `size_bytes` | reported by the disk |
| `transport` | NVMe, SATA, USB and so on |
| `logical_sector_size`, `physical_sector_size` | reported by the disk |
| `by_id_path` | the `/dev/disk/by-id` link, preferring one that carries model and serial |
| `by_path` | the port; compared only for a weak identity |

A target is chosen with a selector: a `/dev/disk/by-id/...` path, or identity
attributes (`serial=...`, `wwn=...`, `model=...`, `size=...`, `by-path=...`).
Any other `/dev/...` path is `TUR-INSTALL-TARGET-001`; a bare kernel name is
`TUR-INSTALL-TARGET-002`. The selector must resolve to exactly one whole disk
that holds a medium, and no other disk may share its identity. Two enclosures
that report the same serial number, or two paths to one multipath disk, are
therefore refused as ambiguous however they are selected.

An identity is **stable** when the disk reports a serial number or a WWN, and
**weak** otherwise. A weak disk can be told apart only by model, size and port.
It is selectable only with an explicit acknowledgement; the plan records that
it was given, and the confirmation screen says so. A weak identity does not
follow a disk to another port. A stable one does.

`revalidate(expected, current_inventory)` returns the one disk with that
identity, or a typed mismatch:

| Mismatch | Meaning |
|---|---|
| missing | no disk carries the recorded serial, WWN, by-id link or port |
| size changed | the disk with those identifiers has another size |
| serial changed | the disk now at the recorded by-id link, WWN or port has another serial: it is a different disk |
| ambiguous | two or more disks match |
| attributes changed | model, WWN, transport, sector size or links differ |

## 4. Preflight checks

Each check is a pure function over a `HostFacts` value and returns pass, warning
or blocking, with a code, a user message and a technical detail. A fact that
could not be determined never passes: it blocks where the fact decides whether
the installed system can work, and warns where it is informational.

| Code | Check | Blocks when |
|---|---|---|
| `TUR-INSTALL-PREFLIGHT-001` | boot mode | not UEFI, or not determined |
| `TUR-INSTALL-PREFLIGHT-002` | Secure Boot state | never (warning unless enabled) |
| `TUR-INSTALL-PREFLIGHT-003` | CPU architecture | not `x86_64`, or not determined |
| `TUR-INSTALL-PREFLIGHT-004` | CPU features | a required feature is missing, or not determined |
| `TUR-INSTALL-PREFLIGHT-005` | memory | below the minimum, or not determined (warning below the recommendation) |
| `TUR-INSTALL-PREFLIGHT-006` | target capacity | no medium, or smaller than the layout needs |
| `TUR-INSTALL-PREFLIGHT-007` | target not in use | anything on the disk is mounted or held open |
| `TUR-INSTALL-PREFLIGHT-008` | target is not the live medium | it is |
| `TUR-INSTALL-PREFLIGHT-009` | other operating systems | never (warning when one is detected) |
| `TUR-INSTALL-PREFLIGHT-010` | safe graphics mode | never (warning when active or not determined) |
| `TUR-INSTALL-PREFLIGHT-011` | network | never (informational) |
| `TUR-INSTALL-PREFLIGHT-012` | target writable | the disk is read-only |
| `TUR-INSTALL-PREFLIGHT-013` | target identity | never (warning for a weak identity) |

The live medium, a disk in use and a read-only disk are refused twice over: by
these checks, and by plan building itself (`TUR-INSTALL-PLAN-005`, `-006`,
`-007`), so no plan value can exist for such a disk.

## 5. The plan and its consequences

A `Plan` is a value, not a list of commands. For the "erase target disk" mode
it holds:

- the target's identity, and whether a weak identity was acknowledged;
- the partition table type and the partitions to create, each with number,
  role, label, type GUID, offset, size, filesystem, mount point and encryption;
- the **consequences**: the existing partition table, every existing partition,
  volume and filesystem that would be destroyed (including volumes inside
  encrypted and LVM containers, and a filesystem written straight to the disk),
  any other operating system detected, and whether user data is preserved
  (never, in this mode);
- the **confirmation screen** text of §28.5, including the encryption choice
  with its key-recovery explanation, a privacy notice for the serial number,
  and the exact phrase the user must type.

The phrase is `ERASE` followed by the last eight letters and digits of the
serial number (of the WWN when there is no serial number; the size in bytes
when there is neither). It differs from disk to disk, so the phrase typed for
one disk does not confirm another. The installer recomputes it from the
identity when checking; it does not trust the copy in a plan file.

All layout arithmetic is checked. Every offset and size is a multiple of the
alignment. A disk that is too small is `TUR-INSTALL-PLAN-001`, with the
required and available sizes.

### The layout is an unverified assumption

The scheme that `bootc install` expects on the Fedora Kinoite base has not been
verified. The boot-test work will do that. Until then:

- the whole scheme is one constant, `ERASE_DISK_SCHEME`, in `src/plan.rs`; no
  size, type GUID, filesystem or mount point appears anywhere else;
- the scheme is marked `verified: false`, every plan carries
  `scheme_verified: false`, and the dry-run output says so.

What the draft scheme assumes:

| Item | Draft value | Why it is only an assumption |
|---|---|---|
| Partition table | GPT | §4.1; not in doubt |
| Alignment | 1 MiB | conventional; suits 512-byte and 4096-byte sectors |
| Reserved at start and end | 1 MiB each | room for the primary and backup GPT |
| EFI System Partition | 1 GiB, FAT32, `/boot/efi` | size and mount point depend on the bootloader, which ADR-0001 leaves to be verified |
| Boot partition | 1 GiB, ext4, `/boot`, always created | bootc may need it only with encryption, or not at all with a UKI in the ESP |
| Root | rest of the disk, Btrfs, at least 20 GiB | bootc's default root filesystem and Kinoite's differ; the minimum is a guess |
| Root with a data partition | 64 GiB when the disk allows, never below the minimum | a guess |
| Data partition | rest of the disk, Btrfs, `/var/home`, at least 8 GiB | the mount point follows Fedora Atomic's `/home` link; unverified for TUR |
| Encryption | LUKS2 on root and data; ESP and boot stay plain | how bootc sets up LUKS on this base is unverified |
| Type GUIDs | Discoverable Partitions Specification (ESP, XBOOTLDR, root x86-64, home) | whether the base image relies on them is unverified |

## 6. The chain that guards the disk

The types make the steps of §5.2 hard to skip. Each value can be obtained only
from the one before it.

1. `select_target` is the only source of a `SelectedTarget`. It never accepts a
   device path.
2. `build_plan` takes a `SelectedTarget`. It refuses the live medium, a disk in
   use and a read-only disk.
3. `Plan::confirm` is the only source of a `ConfirmedPlan`. It needs the exact
   typed phrase and a preflight report, for the same disk, with no blocking
   check. Only `run_preflight` can produce a report, so one that names a disk
   holds every target check for it. `confirm` refuses a plan whose displayed
   phrase does not belong to its target identity. A `ConfirmedPlan` cannot be
   serialised, so a confirmation never survives in a file.
4. `ConfirmedPlan::revalidate` is the only source of a `ValidatedTarget`. It
   needs a fresh inventory and fails if the identity no longer matches exactly
   one disk, if the disk became the live medium, read-only or in use, or if
   what is on the disk is no longer what the user was told would be destroyed.
   A `ValidatedTarget` borrows that inventory and cannot outlive it.
5. `Session::begin_destructive` needs a `ValidatedTarget`, from the session's
   own plan, to record the start of any phase that writes to the target, and
   consumes it: every such phase needs its own revalidation immediately before
   it starts. `Session::begin` refuses such phases.

## 7. Phases

Sixteen phases run in order (§6.2). Each has a record with `phase_id`,
`phase_name`, `state`, `attempt`, `started_at`, `finished_at`, `duration_ms`,
`progress`, `error_code`, `summary`, `technical_details`, `log_reference` and
`recovery_action`. Progress is either a count out of a known total or a named
activity; there is no percentage without a reliable total.

"Destructive" here means the phase writes to the target disk or to the
firmware's boot configuration.

| # | Phase | Class | Destructive | What it is for |
|---|---|---|---|---|
| 1 | `START` | `idempotent` | no | the session directory exists and is durable |
| 2 | `ENVIRONMENT_CHECK` | `idempotent` | no | boot mode, Secure Boot, CPU, memory |
| 3 | `HARDWARE_INVENTORY` | `idempotent` | no | hardware detection |
| 4 | `STORAGE_DISCOVERY` | `idempotent` | no | block devices and stable identifiers |
| 5 | `TARGET_SELECTION` | `idempotent` | no | choice of the target disk |
| 6 | `PARTITION_PLAN` | `idempotent` | no | the plan and its consequences |
| 7 | `USER_CONFIRMATION` | `idempotent` | no | the typed confirmation |
| 8 | `PARTITIONING` | `non_repeatable` | yes | writing the partition table |
| 9 | `ENCRYPTION_SETUP` | `non_repeatable` | yes | creating encrypted containers; optional |
| 10 | `FILESYSTEM_CREATION` | `non_repeatable` | yes | creating filesystems |
| 11 | `IMAGE_DEPLOYMENT` | `reconcilable` | yes | deploying the image |
| 12 | `BOOTLOADER_INSTALL` | `reconcilable` | yes | bootloader and boot entries |
| 13 | `INITRAMFS_CONFIGURATION` | `reconcilable` | yes | initramfs of the installed system |
| 14 | `FIRST_BOOT_CONFIGURATION` | `reconcilable` | yes | first-boot configuration |
| 15 | `INSTALL_VALIDATION` | `idempotent` | no | checking the installed system |
| 16 | `COMPLETE` | `idempotent` | no | done |

`USER_CONFIRMATION` is idempotent in the sense that asking again is always
safe. A confirmation that was given is never reused after a restart, because it
exists only in memory.

Legal transitions:

- `pending -> running`, once every earlier phase has succeeded, finished with a
  warning or been skipped;
- `pending -> skipped`, for the optional phase only, under the same ordering;
- `running -> succeeded | warning | failed | cancelled`;
- `failed | cancelled -> running` (a retry), except for non-repeatable phases;
- a phase and all later ones back to `pending` (the user goes back), only while
  no destructive phase has ever started and none of them is running.

Everything else is a typed error (`TUR-INSTALL-PHASE-*`).

## 8. Session directory and crash safety

```text
<session directory>/
  session.json            identity of the session, target, snapshot of the phase table
  phases.jsonl            append-only, one record per transition; the authority
  storage-inventory.json  the inventory the plan was made from
  partition-plan.json     the plan
```

Files and the directory are readable by their owner only, because they hold
serial numbers. The formats of `session.json`, the lines of `phases.jsonl` and
`partition-plan.json` are given by `schemas/installer-session.schema.json` and
`schemas/installer-plan.schema.json`; tests validate what the crate writes
against them.

The crash-safety argument:

- **Whole files** are replaced by writing `<name>.tmp` in the same directory,
  flushing it, renaming it over the destination and flushing the directory. A
  rename within one directory is atomic, so a crash leaves the old file or the
  new one, never a mixture.
- **The log** is only appended to. Each append is flushed before the transition
  is reported as done. A crash can therefore damage only the final line, and
  that line was never acknowledged: whatever it announced had not begun.
  Loading tolerates an unreadable final line. If the session continues, the
  damaged line is closed with a line terminator and stays in the log; sequence
  numbers show that no acknowledged record is missing.
- **Order of writes for a transition:** append and flush the log, then rewrite
  the snapshot. The log can be one record ahead of the snapshot, never behind.
- **Before the first destructive record:** `session.json` is rewritten with
  `target_writes_started: true` *before* the record is appended. The flag is
  never cleared. If the log is later found shorter than it should be, the
  session still says the target may have been written.
- **After any failed write** the in-memory session refuses further use. The
  caller must load the directory again, which applies the resume rules.

File-system access and time go through the `FileSystem` and `Clock` traits. The
tests replay a whole recorded installation with the in-memory file system
failing at each operation in turn, with torn appends, and truncate
`phases.jsonl` at every byte offset.

## 9. Resume rules

Loading never trusts the session's files further than this.

1. **Any evidence that a destructive phase started** means the decision is
   *needs reconciliation*. Evidence is the `target_writes_started` flag, any
   log record of a destructive phase that is not `pending`, or the same in the
   snapshot. The one exception is a log that is intact and shows every phase
   finished: that is *completed*. The decision is never *resume*.
2. **Reconciliation** requires an inventory taken now. The target's identity
   is revalidated against it; a missing, changed or ambiguous disk is an error
   and nothing more may be done. Otherwise the result reports whether the disk
   still holds what the plan said would be destroyed, carries the planned
   partition table, or neither. It never permits continuing: every route to a
   phase that writes needs a new typed confirmation, and continuing in place is
   not implemented.
3. **No evidence of a write** and a consistent log: the decision is *resume*,
   from storage discovery or earlier, never from a destructive phase. A phase
   that was running is recorded as failed with `TUR-INSTALL-SESSION-005`.
   Discovery, selection, planning and confirmation run again, and the session
   forgets its plan: the saved inventory is stale, and the confirmation existed
   only in memory.
4. **An inconsistent log** (missing or reordered records, or a log shorter than
   the snapshot) with no evidence of a write: the decision is *restart* with a
   new session.

This is the specification's rule "never resume a destructive operation blindly
based only on a stale state file" (§28.1), applied so that a stale or damaged
file can only make the installer more cautious.

## 10. Command record

`CommandRecord` holds the phase, program, sanitised arguments and environment,
start and end time, exit code and a reference to the captured output (§14.4).
A secret enters a `CommandSpec` as a `Secret`, which cannot be serialised and
prints as `<redacted>`. Sanitising also redacts the value of any option or
environment variable whose name suggests a secret, and any occurrence of a
declared secret inside another argument.

## 11. Dry-run command line

```text
tur-installer inventory --from <lsblk.json> [--links <links.json>] [--json]
tur-installer plan --from <lsblk.json> --target <by-id path or identity>
                   [--links <links.json>] [--facts <facts.json>]
                   [--encrypt] [--data-partition]
                   [--acknowledge-weak-identity] [--json]
tur-installer session show <dir> [--json]
```

Every subcommand reads files and writes to standard output. Input paths must be
regular files of at most 16 MiB; a device node or a directory is refused.
`plan` without `--facts` reports the environment checks as not determined.

Exit codes: `0` success; `1` a typed error, printed with its code; `2` a usage
error; `3` a plan was produced but a preflight check blocks installation.

## 12. Error codes

A code's meaning never changes once released (§29.3). A preflight code names a
check; the result's status says whether it passed. Errors have two layers
(§6.3): a user message that says what happened and what to do, and a technical
detail with the exact values.

| Code | Severity | Meaning |
|---|---|---|
| `TUR-INSTALL-INVENTORY-001` | error | Installer input data could not be read |
| `TUR-INSTALL-INVENTORY-002` | error | Installer input data is not in the expected format |
| `TUR-INSTALL-TARGET-001` | error | A device path was given as the installation target |
| `TUR-INSTALL-TARGET-002` | error | The target selector is not valid |
| `TUR-INSTALL-TARGET-003` | error | No disk matches the target selector |
| `TUR-INSTALL-TARGET-004` | error | More than one disk matches the target |
| `TUR-INSTALL-TARGET-005` | error | The target is not a whole disk |
| `TUR-INSTALL-TARGET-006` | error | The target drive holds no medium |
| `TUR-INSTALL-TARGET-007` | error | The target has no stable identifier and this was not acknowledged |
| `TUR-INSTALL-IDENTITY-001` | error | The target disk is no longer present |
| `TUR-INSTALL-IDENTITY-002` | error | The target disk changed size |
| `TUR-INSTALL-IDENTITY-003` | error | A different disk is now in the target's place |
| `TUR-INSTALL-IDENTITY-004` | error | Several disks now match the target's identity |
| `TUR-INSTALL-IDENTITY-005` | error | The target disk's reported attributes changed |
| `TUR-INSTALL-PREFLIGHT-001` | error | Preflight check: firmware boot mode is UEFI |
| `TUR-INSTALL-PREFLIGHT-002` | warning | Preflight check: Secure Boot state |
| `TUR-INSTALL-PREFLIGHT-003` | error | Preflight check: CPU architecture |
| `TUR-INSTALL-PREFLIGHT-004` | error | Preflight check: required CPU features |
| `TUR-INSTALL-PREFLIGHT-005` | error | Preflight check: installed memory |
| `TUR-INSTALL-PREFLIGHT-006` | error | Preflight check: target disk capacity |
| `TUR-INSTALL-PREFLIGHT-007` | error | Preflight check: target disk is not in use |
| `TUR-INSTALL-PREFLIGHT-008` | error | Preflight check: target disk is not the live medium |
| `TUR-INSTALL-PREFLIGHT-009` | warning | Preflight check: other operating systems on the target |
| `TUR-INSTALL-PREFLIGHT-010` | warning | Preflight check: installer graphics mode |
| `TUR-INSTALL-PREFLIGHT-011` | info | Preflight check: network availability |
| `TUR-INSTALL-PREFLIGHT-012` | error | Preflight check: target disk is writable |
| `TUR-INSTALL-PREFLIGHT-013` | warning | Preflight check: target disk has a stable identifier |
| `TUR-INSTALL-PLAN-001` | error | The target disk is too small |
| `TUR-INSTALL-PLAN-002` | error | The installation mode is not implemented yet |
| `TUR-INSTALL-PLAN-003` | error | The target disk's sector size is not supported |
| `TUR-INSTALL-PLAN-004` | error | A partition size calculation overflowed |
| `TUR-INSTALL-PLAN-005` | error | The target is the medium the installer runs from |
| `TUR-INSTALL-PLAN-006` | error | The target disk is in use |
| `TUR-INSTALL-PLAN-007` | error | The target disk is read-only |
| `TUR-INSTALL-PLAN-008` | error | The partition plan is not consistent |
| `TUR-INSTALL-CONFIRM-001` | error | The confirmation phrase was not typed exactly |
| `TUR-INSTALL-CONFIRM-002` | error | A blocking preflight check prevents confirmation |
| `TUR-INSTALL-CONFIRM-003` | error | The preflight report is not for the planned target |
| `TUR-INSTALL-CONFIRM-004` | error | The target's contents changed after confirmation |
| `TUR-INSTALL-CONFIRM-005` | error | The plan's weaker identity was not acknowledged |
| `TUR-INSTALL-PHASE-001` | error | Illegal phase transition |
| `TUR-INSTALL-PHASE-002` | error | A phase was started before an earlier phase finished |
| `TUR-INSTALL-PHASE-003` | error | A non-repeatable phase cannot be retried |
| `TUR-INSTALL-PHASE-004` | error | Phases cannot be reopened after the target was written |
| `TUR-INSTALL-PHASE-005` | error | A mandatory phase cannot be skipped |
| `TUR-INSTALL-SESSION-001` | error | The session directory could not be read or written |
| `TUR-INSTALL-SESSION-002` | error | A session file is corrupt |
| `TUR-INSTALL-SESSION-003` | error | The session was written by an incompatible installer |
| `TUR-INSTALL-SESSION-004` | error | The session wrote to the target and must be reconciled |
| `TUR-INSTALL-SESSION-005` | warning | The installer stopped while a phase was running |
| `TUR-INSTALL-SESSION-006` | error | The session directory already holds a session |
| `TUR-INSTALL-SESSION-007` | error | A phase that writes to the target needs a revalidated target |
| `TUR-INSTALL-SESSION-008` | error | The session has no partition plan |
| `TUR-INSTALL-SESSION-009` | error | No session was found in the directory |
| `TUR-INSTALL-SESSION-010` | error | The session cannot be resumed |
| `TUR-INSTALL-SESSION-011` | error | The session identifier is not valid |
| `TUR-INSTALL-SESSION-012` | error | The session failed to persist and must be reloaded |
| `TUR-INSTALL-SESSION-013` | error | The plan cannot change after the target was written |

The registry in code (`src/codes.rs`) holds the code, title and severity. The
detection criteria, user message, technical explanation and recovery action
that §29.3 asks for are in each error type's implementation and its tests, not
yet in one registry file.

## Assumptions to verify

Each of these is a design assumption. None has been checked against the TUR
image or a real machine.

1. **The partition scheme**, as tabled in section 5.
2. **Live-medium mount points.** The three paths in section 2 are those of a
   dracut live boot. The TUR live image does not exist yet.
3. **lsblk output.** The fixtures are hand-written to the documented shape of
   lsblk's JSON; they were not captured from the lsblk the TUR image ships.
   `SystemProbe` has never been run.
4. **Memory thresholds.** Blocking below 3 GiB and warning below 7 GiB are
   placeholders chosen so that machines sold with 4 GB and 8 GB pass. The
   specification gives development recommendations, not minimums (§4.1).
5. **CPU baseline.** The required feature list is the x86-64 baseline. Whether
   the base image needs a higher microarchitecture level is unverified.
6. **Directory flush.** Crash safety relies on a flushed rename being durable
   on the live environment's filesystem. This holds for the common Linux
   filesystems; it has not been tested with fault injection below the
   file-system interface.
