# Storage policy core

- **Component:** `services/uos-storage` (library `uos_storage`, binary `uos-storage`)
- **Specification sections:** §3.3, §6.3, §8.5, §11, §16, §19.4, §29.3; AI control
  layer §5 and §11; ADR-0007
- **Status:** policy core only. Nothing in this component mounts, unmounts,
  formats or writes to a device, and no D-Bus service exists yet.

Everything described here is tested in CI against hand-written fixtures. No
real filesystem, block device or hardware has been involved. Section 12 lists
what has not been done and what is recorded as NOT RUN.

## 1. What it is

The policy core answers two questions with plain data:

1. How may this volume be mounted? (`decide`)
2. Given one mount, where does each application runtime see it? (`plan`)

Both are pure functions. Facts about volumes come in through a `Probe` trait;
the only implementation so far answers from captured text. A later change adds
a live probe and a service that acts on the decisions through UDisks2.

```text
lsblk JSON + blkid-style properties
        │  Probe
        ▼
   Inventory of Volumes ──► decide(volume, request, overrides) ──► Decision
        │                                                             │
        └──────────────► plan(inventory, …) ──► one mount + runtime views
```

## 2. Volume model

A volume is one filesystem or container signature on a block device. The
inventory is built from:

- the device tree in the shape of `lsblk --json --bytes --output-all` (unknown
  columns are ignored; sizes must be byte counts, so output without `--bytes`
  is rejected rather than guessed);
- `blkid --output export` style properties per device. `UUID_SUB` is read from
  them. Two facts that neither tool reports are passed the same way:
  `UOS_NTFS_STATE` (`clean`, `dirty`, `hibernated`, `unknown`) and
  `UOS_HFS_JOURNAL` (`journalled`, `not-journalled`, `unknown`). A missing or
  unreadable fact is `unknown`, which is treated as unsafe.

Rules applied while building the inventory:

- Swap areas, partitioned disks, empty drives, loop devices without a
  signature and MBR extended-partition containers are not volumes.
- A signature on a whole disk that also has partitions (a hybrid ISO image on
  a USB stick) describes the same bytes as its first partition; only the
  partitions are volumes.
- A device listed under several parents (a logical volume on two physical
  volumes, an MD array under each member) is one device.
- **Removable** means the device or a device beneath it is flagged removable or
  hot-pluggable, or its transport is USB. **Read-only device** means the
  device or a device beneath it reports read-only, or it is an optical drive.
- **System storage** means the volume, or a volume stacked on it, is mounted
  at `/`, `/sysroot` (or below), `/boot`, `/boot/efi`, `/efi`, `/usr`, `/etc`,
  `/var`, `/var/home`, `/home`, `/root`, `/var/roothome`, or is active swap.
  A LUKS container whose mapping holds the root filesystem is therefore system
  storage too.

## 3. Filesystem policy table

One table in `src/policy.rs` (`POLICY_TABLE`) holds the policy. The decision
reads it and has no per-filesystem policy of its own.

| Filesystem | Default access | Implementation | Driver | Conditions |
|---|---|---|---|---|
| ext4 | Read/write | Kernel | `ext4` | — |
| Btrfs | Read/write | Kernel | `btrfs` | — |
| XFS | Read/write | Kernel | `xfs` | — |
| NTFS | Read/write | Kernel | `ntfs3` | Write only when the state is `Clean` |
| exFAT | Read/write | Kernel | `exfat` | — |
| FAT32/VFAT | Read/write | Kernel | `vfat` | — |
| HFS+ | Read-only | Kernel | `hfsplus` | Write only when not journalled **and** the HFS+ write override is on |
| APFS | Read-only | Userspace | `apfs-fuse` | Mounted only when the APFS override is on; never writable |
| ReFS | Unsupported | — | — | — |
| ZFS | Unsupported | — | — | "Optional" in §11.3; not included until the licence and kernel-compatibility review |
| ISO 9660 | Read-only | Kernel | `iso9660` | *Added row.* A read-only format |
| LUKS container | Refuse | — | — | *Added row.* Locked: needs unlock. Unlocked: the mapping is the volume |
| BitLocker volume | Unsupported | — | — | *Added row.* Unlocking is not implemented |
| LVM physical volume | Refuse | — | — | *Added row.* The logical volumes are the volumes |
| MD RAID member | Refuse | — | — | *Added row.* The array is the volume |
| unrecognised filesystem | Unsupported | — | — | No signature, or one not in this table |

The first ten rows are specification §11.3. The specification's eleventh row,
"APFS experimental (linux-apfs-rw), disabled by default", is not a row of the
table: it is recorded in `EXCLUDED_IMPLEMENTATIONS` as an implementation this
component never selects. Rows marked *added* are needed to classify real
devices; none of them widens what §11.3 allows. ext2, ext3, UDF, F2FS and
every other signature are "unrecognised" until the specification says
otherwise.

### Policy overrides

`PolicyOverrides` is the whole set of knobs:

| Override | Default | Effect |
|---|---|---|
| `unsafe_ntfs` | `read-only` | `refuse` stops NTFS volumes that are not known to be clean from being mounted at all |
| `hfsplus_write` | off | Allows writing to HFS+ volumes known not to be journalled |
| `apfs_read_only` | off | Enables read-only APFS mounts. Off because ADR-0007 defers APFS |
| `noexec_removable` | off | Adds `noexec` on removable media. Off because Wine cannot start a program from a `noexec` mount |

There is no override, command-line flag or environment variable that makes
APFS writable, relaxes the NTFS state check or selects linux-apfs-rw.

## 4. The mount decision

`decide(volume, request, overrides)` returns one of five outcomes: mount
read/write, mount read-only, refuse, needs unlock, unsupported. Every decision
carries the driver, the exact mount options, a stable code, a user message, a
reason, a recovery action and technical detail, and renders in the two-layer
form of §6.3.

Rules, in order. The first that applies decides.

1. System storage is refused (`UOS-STORAGE-SYSTEM-001`). It is never offered
   for remounting.
2. A volume whose identity another device also claims is refused
   (`UOS-STORAGE-ID-001`).
3. A volume already mounted somewhere other than its canonical path is refused
   (`UOS-STORAGE-MOUNT-003`): UOS keeps one mount per filesystem.
4. Unsupported rows are unsupported (`UOS-STORAGE-FS-001`,
   `UOS-STORAGE-FS-002`, `UOS-STORAGE-BITLOCKER-001`). Container rows are
   refused (`UOS-STORAGE-CONTAINER-001`), except a locked LUKS container, which
   needs unlocking (`UOS-STORAGE-LUKS-001`).
5. A row's read conditions must hold, otherwise unsupported
   (`UOS-STORAGE-APFS-002`).
6. Writing is allowed only if every write condition of the row holds, the row
   is writable at all, and the device is not read-only. The first failure
   gives the code: `UOS-STORAGE-NTFS-004`, `UOS-STORAGE-NTFS-005`,
   `UOS-STORAGE-HFSPLUS-001`, `UOS-STORAGE-HFSPLUS-002`,
   `UOS-STORAGE-APFS-001`, `UOS-STORAGE-FS-003`, `UOS-STORAGE-MEDIA-001`.
   - If the failure is an NTFS state and `unsafe_ntfs` is `refuse`, the volume
     is refused whatever was requested.
   - Otherwise a read/write request is downgraded to read-only with that code.
7. A read/write request that passed rule 6 is granted
   (`UOS-STORAGE-MOUNT-001`). A read-only request is granted as asked
   (`UOS-STORAGE-MOUNT-002`).

A request is therefore never exceeded: the result is the request, less than
the request, or nothing.

### NTFS

The state is an input: `Clean`, `Dirty`, `Hibernated` or `Unknown`. Only
`Clean` can be written. `Hibernated` and `Unknown` give `UOS-STORAGE-NTFS-004`;
`Dirty` gives `UOS-STORAGE-NTFS-005`. The message for the hibernated case
follows the example in §6.3:

```text
UOS-STORAGE-NTFS-004
The Windows volume was not mounted for writing.

Reason:
The volume appears to be hibernated or in an unsafe state.

Action:
Boot Windows, disable Fast Startup/hibernation, shut down fully, then retry. UOS mounts the volume read-only to protect your data.

Technical detail:
Device: /dev/disk/by-uuid/01D9A7C2E4F6B801
Driver: ntfs3
Mount policy: read-only
Mount options: ro,nosuid,nodev,uid=1000,gid=1000,dmask=0077,fmask=0177,iocharset=utf8
Evidence: filesystem=ntfs; ntfs_state=hibernated; … (facts from the probe; no journal reference at this layer)
```

Two deliberate differences from the example: the tense ("mounts", because a
decision is made before anything is mounted), and the device, which is shown
in full here; redaction belongs to the diagnostics collector (§6.5).

### Mount options

Every option was checked against the kernel documentation or source named in
section 8 and its references.

| Applies to | Options |
|---|---|
| Every mount | `ro` or `rw`, `nosuid`, `nodev` |
| Removable media, with `noexec_removable` | `noexec` |
| NTFS (`ntfs3`) | `uid=`, `gid=`, `dmask=0077`, `fmask=0177`, `iocharset=utf8`; `windows_names` when writable |
| exFAT | `uid=`, `gid=`, `dmask=0077`, `fmask=0177`, `iocharset=utf8`, `errors=remount-ro` |
| FAT | `uid=`, `gid=`, `dmask=0077`, `fmask=0177`, `shortname=mixed`, `utf8=1`, `errors=remount-ro`; `flush` when writable and removable |
| HFS+ | `uid=`, `gid=` |
| ext4, Btrfs, XFS, ISO 9660, APFS | none beyond the common ones |

The masks keep other local users out of filesystems that have no ownership of
their own, because the canonical mount point is not inside a per-user
directory. `windows_names` stops UOS creating names Windows cannot use.

No decision contains `force`, and no message suggests it. The `hfsplus`
driver has a `force` option for journalled volumes and `ntfs3` has one for
dirty volumes; neither is ever emitted. `remove_hiberfile` and `recover` are
not `ntfs3` options at all.

A known limit: `ro` makes the mount read-only, which is not the same as never
writing to the device. ext4 and XFS may replay their journal on a read-only
mount. For NTFS, see section 8.

### Safety argument

| Invariant | Why it holds | Test |
|---|---|---|
| Only a clean NTFS volume is writable | The NTFS row has the write condition `NtfsClean`; a missing state is `Unknown` | `only_a_clean_ntfs_volume_is_writable`, `missing_ntfs_state_is_treated_as_unknown` |
| No decision produces or recommends a forced mount | Options come from one function with a fixed vocabulary; registry text avoids the wording | `no_decision_produces_or_recommends_an_unsafe_mount` enumerates every decision (all facts × both requests × all 16 overrides, more than 60 000) and checks options against an allow-list and all text against a deny-list |
| APFS is never writable | The row has `writable: false`; no override mentions APFS writing; the write driver is not in the table | `apfs_is_never_writable_over_the_whole_option_space`; `apfs_is_never_writable_through_the_command_line` runs the binary with every flag combination |
| A request is never exceeded | Rule 6 only downgrades | `a_request_is_never_exceeded`, `writing_is_granted_only_when_every_rule_allows_it` |
| System storage is never offered | Rule 1 comes first | `system_storage_is_never_offered`, `dual_boot_system_storage_is_never_offered` |
| Every mount outside system storage has `nosuid` and `nodev` | They are added before anything else | `every_mount_has_safe_defaults_and_a_driver` |
| No filesystem is planned to be mounted twice | One canonical mount per volume; clones are refused; views are binds or links | `no_filesystem_is_planned_to_be_mounted_twice` over every fixture, request and override |
| Nothing is read from the environment, no program is started, no file is written | The sources contain no such call | `the_sources_read_no_environment_start_no_program_and_write_no_file`, `the_environment_does_not_change_any_decision` |

## 5. Identifiers and paths

The volume identifier is the first of these that is available:

| Form | Source | Stable |
|---|---|---|
| `fs-<uuid>` | Filesystem UUID, lower-cased | Until the filesystem is re-created |
| `part-<partuuid>` | Partition UUID | Until the partition table is rewritten |
| `dev-<hash>` | Disk WWN or serial number, partition offset, size and signature | Across reconnects and device renaming |
| `tmp-<hash>` | Device path | No. Flagged as `unstable` |

A UUID is used only if it is 4 to 64 characters of ASCII letters, digits and
hyphens and is not all zeros; otherwise the next form is used. An identifier
consists of lower-case ASCII letters, digits and hyphens, so it is always safe
as one path component. The hash is 64-bit FNV-1a; it is not a security
boundary.

**Duplicates.** Two devices with the same identifier are one volume only if
they are members of one multi-device set (Btrfs, MD RAID, ZFS) and each has
its own `UUID_SUB`. Anything else is a clone or cannot be told apart from
one: both devices are flagged, both are refused, and each falls back to the
next identifier form so that it can still be named.

**Paths.**

- Canonical: `/run/uos/volumes/<volume-id>`. The label is not part of it.
- User-facing: `/run/media/<user>/<name>`.

`<name>` is the sanitised label: control and invisible characters removed,
`/`, `\` and `:` replaced by `_`, white space collapsed, leading dots removed,
cut to 48 characters. With no usable label the name is the volume identifier.
Equal names (compared without case) get ` (2)`, ` (3)`, … in identifier order.
A caller can pass names that are already in use, and they are kept, so that a
path does not change while a volume is mounted.

## 6. Runtime mappings

Each mountable volume has one real mount, at its canonical path. Every runtime
sees a view of that mount; no view mounts the filesystem again.

| Runtime | View | Mechanism |
|---|---|---|
| Linux | `/run/media/<user>/<name>` | Bind of the canonical mount |
| Windows (Wine) | `dosdevices/<letter>:` in the prefix | Symbolic link to the canonical mount |
| macOS (Darling) | `/Volumes/<name>` inside the runtime | Bind in the runtime's namespace |
| Android | `/storage/emulated/0/Volumes/<name>` inside Android | Bind applied only after the user grants it |

- Drive letters are `D` to `Y` in identifier order. `A`, `B`, `C` and `Z` are
  never used. A caller can pin letters already given, because Windows
  applications store paths that contain them. With more than 22 volumes the
  rest get no letter.
- A view of a read-only mount is read-only.
- The Android mapping follows the Android runtime addendum (§7.2): nothing is
  visible by default, and a grant is a bind of the canonical mount. That
  addendum is not merged yet. The path inside Android and the ownership
  mapping (bindfs or an ID-mapped mount) are proposals.
- `MountPlan::verify` checks the invariants of a plan and the command line
  runs it before printing.

## 7. Metadata capability registry

`src/capabilities.rs` records, for each mountable filesystem, how well it
represents each feature listed in §11.5 **as mounted by the driver UOS uses**:
`yes`, `partial`, `no` or `unverified`.

| | perm | ACL | xattr | ADS | sec. desc. | res. fork | case-sens. | symlink | hard link | sparse | compr. | snapshot | CoW |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ext4 | yes | yes | yes | no | no | no | yes | yes | yes | yes | no | no | no |
| Btrfs | yes | yes | yes | no | no | no | yes | yes | yes | yes | yes | yes | yes |
| XFS | yes | yes | yes | no | no | no | yes | yes | yes | yes | no | no | partial |
| NTFS | partial | partial | partial | yes | yes | no | partial | partial | yes | yes | yes | no | no |
| exFAT | no | no | no | no | no | no | no | no | no | no | no | no | no |
| FAT | no | no | no | no | no | no | no | no | no | no | no | no | no |
| HFS+ | yes | unverified | partial | no | no | unverified | partial | yes | unverified | no | unverified | no | no |
| APFS | yes | unverified | unverified | no | no | unverified | partial | yes | unverified | unverified | partial | unverified | unverified |
| ISO 9660 | partial | no | no | no | no | no | partial | partial | no | no | no | no | no |

`copy_losses(source, destination)` lists what a copy would not preserve:
`lost` (the destination cannot represent it), `degraded` (only in part) or
`unverified` (not established, so the caller must warn). It refuses to answer
when the destination is never written or a filesystem has no entry, so that
missing data is never read as "nothing is lost". Copying and sidecar formats
are not implemented.

The entries come from kernel documentation and general knowledge of the
formats. They have not been checked against real filesystems.

## 8. NTFS state: what `ntfs3` does, and how the state will be detected

### Verified

From the kernel documentation (https://docs.kernel.org/filesystems/ntfs3.html,
read on 12 November 2026; identical to `Documentation/filesystems/ntfs3.rst`
on the kernel's master branch that day):

- The documented mount options are `iocharset`, `uid`, `gid`, `umask`,
  `dmask`, `fmask`, `nohidden`, `sys_immutable`, `hide_dot_files`,
  `windows_names`, `discard`, `force`, `sparse`, `showmeta`, `prealloc`,
  `acl`, `ads`, `nocase` and `delalloc`.
- `force` "forces the driver to mount partitions even if volume is marked
  dirty. Not recommended for use."
- The driver "supports native journal replaying".
- The page says nothing about hibernation, and nothing about how a dirty
  volume is reported.

From reading `fs/ntfs3` on the kernel's master branch the same day (`super.c`,
`fsntfs.c`, `fslog.c`):

- At mount the driver reads the volume flags from `$Volume`. If the dirty flag
  (0x0001) is set it logs "It is recommended to use chkdsk." and remembers
  the volume as really dirty.
- A read/write mount of a dirty volume without `force` fails with `EINVAL`
  and the kernel log line `volume is dirty and "force" flag is not set!`. A
  read-only mount succeeds.
- The journal (`$LogFile`) is examined before that check. On a read-only
  mount it is not replayed; the driver records that a replay is needed and
  later refuses a remount to read/write. On a read/write mount it is
  replayed. So **a read/write mount attempt can write to the volume before
  the dirty check rejects it**: trying read/write and seeing whether the
  driver objects is not a safe probe.
- While mounted read-only the driver does not change the volume state.
- Once mounted, `/proc/fs/ntfs3/<device>/volinfo` reports the real and the
  current dirty state as its last two lines.
- **No hibernation check was found.** The word does not occur in any of the
  eighteen driver source files read (everything in `fs/ntfs3` except the
  compression code and the upper-case table). A volume left by Windows
  hibernation or Fast Startup is, to `ntfs3`, an ordinary volume, and if its
  dirty flag is clear the driver will mount it read/write and replay its
  journal.

From the ntfs-3g source (`libntfs-3g/volume.c`, not used by UOS, read for the
detection rule): a volume is treated as hibernated when `/hiberfil.sys`
exists and its first four bytes are `hibr` or `HIBR`.

### Not verified

- Behaviour on the kernel Fedora 45 ships. The reading above is of the master
  branch, not of a pinned release.
- That a read-only `ntfs3` mount writes nothing in any code path. The paths
  read (journal replay, state change, boot-sector update) are all guarded;
  the driver was not audited in full.
- That the `hiberfil.sys` rule is right for current Windows releases.
- Anything on a real volume. No NTFS image was mounted.

### Consequence for this component

`ntfs3` protects a dirty volume and does nothing for a hibernated one. UOS
must therefore establish the state itself, before any read/write mount, and
must treat "could not establish" as unsafe. That is why the state is an input
with an `Unknown` value that is never writable.

### Proposal for the detection change

Read-only first, using the kernel's own driver:

1. Mount the volume **read-only** at its canonical path with the options in
   section 4. This is the mount an unsafe volume would get anyway.
2. Read `/proc/fs/ntfs3/<device>/volinfo`. If either dirty line says `dirty`,
   the state is `Dirty`.
3. Read the first four bytes of `hiberfil.sys` in the volume's root. If they
   are `hibr` or `HIBR`, the state is `Hibernated`.
4. If both checks pass and read/write was requested, ask the kernel to
   remount read/write, without `force`. If the kernel refuses (journal not
   replayed, or dirty), the state is `Dirty` and the volume stays read-only.
   If it accepts, the state is `Clean`.
5. If any step fails or gives an answer that is not understood, the state is
   `Unknown` and the volume stays read-only.
6. Record the kernel log lines for the device as the evidence reference in
   the decision's technical detail.

Two alternatives that avoid mounting, to be evaluated in the same change:
reading the two on-disk facts directly from the block device opened
read-only (a small NTFS reader, or an existing read-only parser crate after a
licence and maintenance review), and `ntfs-3g.probe`, whose manual documents
exit status 14 for a hibernated volume and 15 for one not cleanly unmounted.
Whether that tool can be trusted not to write has not been checked.

Tests that change needs, none of which exist yet: NTFS images in the clean,
hibernated (Windows 10 and 11 with Fast Startup), dirty and
journal-needs-replay states, mounted in a CI virtual machine, with the image
checksum compared before and after every read-only step.

The same change should detect the HFS+ journal bit in the volume header
attributes (`HFSPLUS_VOL_JOURNALED` in the kernel source). The Linux
`hfsplus` driver already keeps a journalled volume read-only unless `force`
is given, and mounts a volume that was not cleanly unmounted read-only
(`fs/hfsplus/super.c`, read the same day).

## 9. Error codes

The registry is `StorageCode::entry` in `src/codes.rs`: an exhaustive match,
so a code without an entry does not compile. Each entry has a title, severity,
detection criteria, user message, technical explanation and recovery action.

| Code | Severity | Meaning |
|---|---|---|
| `UOS-STORAGE-MOUNT-001` | info | A read/write mount is permitted |
| `UOS-STORAGE-MOUNT-002` | info | A read-only mount, as requested |
| `UOS-STORAGE-MOUNT-003` | warning | Already mounted outside the UOS namespace; not mounted again |
| `UOS-STORAGE-SYSTEM-001` | notice | Part of the running system; not offered |
| `UOS-STORAGE-ID-001` | error | Two devices claim the same filesystem identity |
| `UOS-STORAGE-NTFS-004` | warning | NTFS hibernated or in an unsafe (undetermined) state; not mounted for writing |
| `UOS-STORAGE-NTFS-005` | warning | NTFS dirty flag set; not mounted for writing |
| `UOS-STORAGE-HFSPLUS-001` | warning | HFS+ journalled or journal state unknown; not mounted for writing |
| `UOS-STORAGE-HFSPLUS-002` | notice | HFS+ writing switched off by policy |
| `UOS-STORAGE-APFS-001` | notice | APFS is read-only |
| `UOS-STORAGE-APFS-002` | notice | APFS support not enabled |
| `UOS-STORAGE-FS-001` | notice | Filesystem known and not supported |
| `UOS-STORAGE-FS-002` | notice | No recognised filesystem |
| `UOS-STORAGE-FS-003` | info | Read-only filesystem format |
| `UOS-STORAGE-MEDIA-001` | notice | Device or medium is write-protected |
| `UOS-STORAGE-LUKS-001` | info | Encrypted volume is locked |
| `UOS-STORAGE-CONTAINER-001` | info | A container for other volumes, not a filesystem |
| `UOS-STORAGE-BITLOCKER-001` | notice | BitLocker volume; not supported |
| `UOS-STORAGE-PROBE-001` | error | Probe input could not be read or parsed |
| `UOS-STORAGE-VOLUME-001` | error | Unknown volume identifier |

`UOS-STORAGE-NTFS-004` has the meaning of the specification's example. The
dirty case has its own code because the recovery action differs (let Windows
check the disk). `UOS-STORAGE-NTFS-001` to `003` are not assigned.

Tests: every code parses as a `uos-core` storage code and is unique; the set
of codes produced by the full enumeration of decisions equals the set of
decision codes in the registry; the two input codes are produced by the
command-line tests.

## 10. D-Bus contract (proposed, not implemented)

`services/uos-storage/dbus/org.uos.Storage1.xml`. System bus, name
`org.uos.Storage1`, object `/org/uos/Storage1`. The interface is **not
frozen**; it should be reviewed and frozen before other components depend on
it.

The service will orchestrate UDisks2 and libblockdev (§11.7) and will not
replace them: block-device operations are requested from UDisks2, and
`uos-storage` adds the policy decision, the canonical namespace and the
runtime views. It will be the only component that asks for mounts (§3.3).

JSON results are documents of `schemas/storage-volume.schema.json`. A string
of JSON is used instead of nested D-Bus dictionaries so that the D-Bus
interface, the command line and the diagnostics bundle share one versioned
schema (§16).

| Method | Authorisation (polkit action) | Returns | Timeout | Cancellation |
|---|---|---|---|---|
| `ListVolumes` | `org.uos.storage.read`: any local session | `s`: inventory document | 5 s | Not cancellable; read-only |
| `PreviewMount` | `org.uos.storage.read` | `s`: decision document | 5 s | Not cancellable; read-only |
| `Mount` | `org.uos.storage.mount-removable`: active local session. `org.uos.storage.mount-internal`: administrator authentication, kept for the session | `s`: decision document for what was done | 30 s; 60 s for a userspace driver | `Cancel` before the mount call is issued abandons it. After that the mount completes and is undone before the method returns `Cancelled` |
| `Unmount` | `org.uos.storage.unmount`: the session that mounted it. `org.uos.storage.unmount-others`: administrator authentication | nothing | 120 s | `Cancel` while waiting for users of the volume or for data to be written leaves it mounted. Not cancellable once the unmount has happened |
| `Unlock` | `org.uos.storage.unlock`: active local session for removable media, administrator authentication for internal; then the passphrase, asked by the system's authentication agent | `s`: identifier of the volume inside | 180 s, which includes the person typing | `Cancel` dismisses the prompt; nothing is unlocked |
| `Cancel` | The caller that started the operation | nothing | 5 s | — |

Rules common to all methods:

- **Errors.** `org.uos.Storage1.Error.NoSuchVolume`, `.InvalidArgument`,
  `.NotAuthorized`, `.Refused`, `.NeedsUnlock`, `.Unsupported`, `.Busy`,
  `.Cancelled`, `.TimedOut`, `.Failed`. The message starts with the
  `UOS-STORAGE-*` code, then the user message. `Mount` returns `.Refused`,
  `.NeedsUnlock` or `.Unsupported` when the decision is not a mount; a
  read/write request that is downgraded is a success whose result says so.
- **Timeouts** are enforced by the service. On expiry the method returns
  `.TimedOut` and the service puts the volume back in a known state. An
  unmount that times out leaves the volume mounted: there is no lazy or
  forced unmount.
- **Cancellation.** The caller chooses an `operation-id` and passes it in
  `options`. `Cancel` accepts it only from the same bus connection. A caller
  that leaves the bus cancels its operations that have not passed the point
  of no return.
- **The decision is made again at call time.** A preview is not a promise.
- **No secret is an argument.** The passphrase never crosses this interface.
- **There is no method for formatting, partitioning or encrypting.**

Signals: `VolumeAdded`, `VolumeRemoved`, `VolumeChanged`, each with the volume
identifier. Property: `SchemaVersion`.

## 11. Assistant tools, command line and schema

`services/uos-storage/agent-tools.json` follows the manifest example in the AI
control layer §11.

| Tool | Class | Method |
|---|---|---|
| `storage_list_volumes` | 0 | `ListVolumes` |
| `storage_preview_mount` | 0 | `PreviewMount` |
| `storage_mount` | 1 | `Mount` |
| `storage_unmount` | 1 | `Unmount` |
| `storage_unlock` | 1 | `Unlock` |

Formatting, partitioning and encryption are class 3 and are not offered by
this service as tools or as D-Bus methods. Enabling APFS writing is class 4
and cannot be expressed. The assistant passes a volume identifier and an
access level, never a device path, an option or a secret.

Command line (input from files only; see `uos-storage --help`):

```sh
uos-storage volumes --from lsblk.json [--props blkid.export] [--user NAME] [--json]
uos-storage decide --from lsblk.json --volume ID [--access ro|rw] [--json]
uos-storage capabilities --from-fs ntfs --to-fs exfat [--json]
```

Exit status 0 on success (whatever the decision), 1 for unusable input or an
unknown volume, 2 for a usage error. JSON output conforms to
`schemas/storage-volume.schema.json`; the tests validate every document the
binary prints for the fixtures against it.

## 12. Deliberately not done

| Item | State |
|---|---|
| Any real mount, unmount or bind | Not implemented |
| The D-Bus service, polkit actions, UDisks2 integration | Not implemented; contract proposed above |
| A live probe (udev, libblkid, UDisks2) | Not implemented; only the snapshot probe exists |
| Detection of NTFS state and HFS+ journal | Not implemented; proposal in section 8 |
| LUKS unlocking, BitLocker | Not implemented |
| APFS beyond the policy entry (no `apfs-fuse` packaging or invocation) | Not implemented; deferred by ADR-0007 |
| Copying with metadata preservation or sidecars | Not implemented |
| Validation on real filesystems (§19.4: clean NTFS mounts read/write, hibernated and dirty NTFS protected, exFAT/ext4/Btrfs/XFS mount, clean unmount) | **NOT RUN** |
| Validation on real hardware (removable media, optical drives, card readers) | **NOT RUN** |
| Validation of the mount options against the shipped kernel | **NOT RUN** |
| Fault-injection and recovery tests (AGENTS.md, storage-critical work) | **NOT RUN**; nothing here performs an operation that can fail half-way |
| Validation of fixtures against real `lsblk` output | **NOT RUN**; the fixtures are hand-written |

## 13. Open questions

1. **Canonical path and UDisks2.** §11.4 wants the one real mount at
   `/run/uos/volumes/<id>`; §11.7 wants UDisks2 to do the mounting, and
   UDisks2 chooses its own mount point under `/run/media/<user>`. Either
   UDisks2's mount becomes the canonical one, or `uos-storage` mounts through
   libblockdev at the canonical path and tells UDisks2. Until that is decided,
   a volume that something else has mounted is refused rather than mounted a
   second time.
2. **Read-only that never writes.** For a volume that must not change at all,
   `ro` is not enough on journalling Linux filesystems. A stricter mode needs
   its own decision.
3. **`noexec`.** Safer on removable media, and it breaks starting Windows
   programs from them. The default is off.
4. **HFS+ and ZFS.** §11.3 says "policy-dependent" and "optional" without
   saying what the policy is. This component chose read-only with an opt-in,
   and unsupported.
5. **Filesystems outside §11.3** (ext2, ext3, UDF, F2FS, …) are unsupported
   here. UDF in particular matters for optical media.
6. **An ADR.** AGENTS.md asks for one when a change affects an interface or
   storage behaviour. The choices above should be recorded in one when the
   interface is frozen.

## 14. Code that should move to a shared crate

The installer also reads `lsblk` output. Nothing is shared in this change.
Candidates for a shared crate once both exist: the `lsblk` JSON model and its
tolerant number and flag parsing; the `blkid` export parser; the flattening of
the device tree with de-duplication; the system-storage and removable rules;
the stable device identity (`VolumeId` and its fallbacks); and safe printing
of device-supplied text. `uos-core`'s `ErrorCode` would be easier to use with
optional `serde` support.

## References

- NTFS3: https://docs.kernel.org/filesystems/ntfs3.html
- VFAT: https://docs.kernel.org/filesystems/vfat.html
- HFS+: https://docs.kernel.org/filesystems/hfsplus.html
- Kernel source read: `fs/ntfs3/super.c`, `fs/ntfs3/fsntfs.c`,
  `fs/ntfs3/fslog.c`, `fs/exfat/super.c`, `fs/hfsplus/super.c` (master branch,
  12 November 2026)
- ntfs-3g: `libntfs-3g/volume.c`, `src/ntfs-3g.probe.8.in`
  (https://github.com/tuxera/ntfs-3g)
