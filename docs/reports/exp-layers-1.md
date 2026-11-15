<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-layers-1 — Layered Windows prefixes: a shared base, thin application layers, a registry rebase (experiment)

> **Status: nothing is measured yet.** The experiment is written, its
> hypotheses and kill criteria were committed before any run
> (`tests/experiments/layers/README.md`, commit `10f6a64`), and its first CI
> run is waiting for a runner (pull request 133; about 120 runs were queued
> in the repository when it was pushed on 2026-11-14). Every number in this
> report is therefore **NOT RUN**. What is here is what could be established
> without running anything: what Wine, the kernel and bubblewrap do, read in
> their sources; the design that follows from it; the method; and the
> decisions the results will have to settle. Section 8 holds the tables the
> first run fills.

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-layers-1 (time-boxed research spike "layers")
Subsystem:        Windows compatibility (Universal Runtime Manager); no TUR
                  service is changed
Build ID:         not applicable: this task builds no image
Source revision:  exp/layers (pull request 133)

SUMMARY
- What changed: an experiment, not a feature. A prototype in Python under
  tests/experiments/layers composes a Windows application's Wine prefix at
  launch from a shared read-only base layer and a thin writable application
  layer, with overlayfs mounted by bubblewrap inside the sandbox's own user
  namespace; and rebases a layer's registry onto a new base (three-way).
  A workflow runs it under Fedora 43's and Fedora 45's Wine, and on Fedora
  45's kernel in a virtual machine.
- Why: a prefix per application costs tens of seconds and about 1.3 GB each,
  every Wine update must update every prefix, and applications that belong
  together cannot see each other.

FILES
- Added: tests/experiments/layers/** (README.md with the hypotheses,
  winereg.py, layerlib.py, lrun.py, experiment.py, fsmatrix.py, report.py,
  fs-setup.sh, container.sh, vm.sh, vm-init.sh, vm-inner.sh, rt/),
  .github/workflows/exp-layers.yml, docs/reports/exp-layers-1.md
- Modified: none
- Removed: none

BUILD
- Command: none on a development machine. CI builds two small test programs
  with mingw-w64 (tests/experiments/layers/rt/build.sh)
- Result: NOT RUN (the workflow has not started)
- Warnings: none known

TESTS
- Unit tests: winereg.py --self-test (24 checks of the registry format, the
  diff, the replay and the three-way rebase) is the first step of each CI
  stage. On the development machine only the Python modules' own logic was
  exercised, on scratch files, with Wine and bubblewrap replaced by
  stand-ins: that finds mistakes in the harness and measures nothing
- Integration tests: workflow exp-layers on pull request 133: NOT RUN (its
  run is queued)
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (a hosted runner; a virtual machine for the kernel)
- Performance tests: NOT RUN (they are part of the workflow)

DIAGNOSTICS
- Log paths: CI artifacts exp-layers-evidence and exp-layers-kernel-evidence
  (results.json, fsmatrix*.json, one log per sandbox run, screenshots)
- Diagnostic bundle path: not applicable
- Relevant error IDs: none (no TUR error code is produced by a prototype)

RECOVERY
- Rollback procedure: delete tests/experiments/layers, the workflow and this
  report; nothing else refers to them
- Recovery tested: not applicable

KNOWN LIMITATIONS
- See section 11. The first of them: no result yet.

NEXT SAFE ACTION
- Let the workflow's run on the branch's head finish, read its two
  artifacts, fill section 8, and settle the kill criteria. Until then
  nothing in this report is a reason to change tur-runtime.
```

## The answer

**None yet.** Two things are already known, from reading, that the
measurements cannot overturn and that shaped the design:

1. **Letting Wine update a layered prefix would copy the whole base into the
   layer.** `wineboot --update` opens every Wine-owned DLL in `system32` for
   reading and writing, truncates it and writes it again, changed or not
   (`dlls/setupapi/fakedll.c`, `create_dest_file` and `install_fake_dll`,
   Wine 11.20). On overlayfs, opening a lower file for writing copies it up.
   So with layers a Wine update cannot be "run `wineboot --update` in every
   prefix", which is what `tur-runtime`'s `migrate` does today. The base is
   replaced, and each layer has to be moved onto it: that is the registry
   rebase (H3), and without it the idea has no update story at all.
2. **A case-folded drive C: cannot live inside one overlay with the rest of
   the prefix.** On the kernel TUR ships (7.3), overlayfs accepts case-folded
   layers only when every directory of every layer agrees: the mount is
   refused when the layer roots differ ("case-insensitive directory on … is
   inconsistent", `fs/overlayfs/params.c`), and a lookup that meets a
   directory of the other kind fails (`ESTALE` "parent wrong casefold",
   `EREMOTE` "child wrong casefold", `fs/overlayfs/namei.c`). A prefix has an
   ordinary root (the registry files, `dosdevices`) and, with `--casefold`,
   a folded `drive_c`. The prototype therefore mounts **two** overlays: one
   for the prefix's root, one for drive C: on top of it. Whether that works
   without privilege on 7.3 is measured, not assumed (section 8.1).

## 1. The idea, and what was built

| Layer | Holds | Written |
|---|---|---|
| Base | A freshly initialised Wine prefix for one Wine build | Once per Wine build |
| Runtime (optional) | A redistributable component several applications need | Once |
| Application | What its installer and the application wrote | By that application |
| Data | The person's documents: the private home that the profile's folders link into | By the person's applications |

Composed at launch, inside the sandbox:

```
/tur/prefix            overlay: lower = base/root [, runtime/root, …]   upper = layer/root
/tur/prefix/drive_c    overlay: lower = base/c    [, runtime/c, …]      upper = layer/c
/tur/home              bind:    layer/home (the data)
```

Three choices that are not obvious:

- **Two overlays**, as above, so that a case-folded drive C: stays possible
  and so that one layout serves both.
- **The same two paths in every sandbox** (`/tur/prefix`, `/tur/home`),
  whatever lies under them. Today `tur-runtime` mounts a prefix at its own
  host path, so the path names the application. A base must be the same
  bytes for every application: nothing in it may name one. (What a prefix
  holds of its own path: `dosdevices/c:` is `../drive_c` and `z:` is `/`,
  both independent of it; the profile's Documents, Desktop and so on are
  links into `$HOME` when those directories exist. With one `$HOME` path
  for all, those links are valid in every layer, and they are what puts
  the person's documents outside the application layer.)
- **The registry is kept as a record of changes**, beside the layer, not
  only as the three files Wine writes (section 4).

The prototype (`tests/experiments/layers/`) is about 3 500 lines of Python.
It does not use `tur-runtime`: the service creates a fresh prefix for every
installation and refuses an existing one (`TUR-WINDOWS-PREFIX-002`), so it
cannot be asked to install into a composed prefix. The sandbox is written out
again from `services/tur-runtime/src/sandbox.rs`; installing and removing use
the commands `runtime-manager.md` documents; the application checks are the
functions of `tests/windows-runtime/apps.py`, imported unchanged.

## 2. What Wine does (read in Wine 11.20's source)

From `server/registry.c`, `server/unicode.c`, `programs/wineboot/wineboot.c`
and `dlls/setupapi/fakedll.c` at tag `wine-11.20` (the GitHub mirror).

**The registry is three text files, each rewritten whole.**

- `system.reg` is `HKEY_LOCAL_MACHINE`, `user.reg` is `HKEY_CURRENT_USER`,
  `userdef.reg` is `HKEY_USERS\.Default` (`init_registry`).
- A branch is saved only when something in it changed (`save_branch` returns
  at once unless the branch has `KEY_DIRTY`), every 30 seconds
  (`save_period`) and when the server exits (`flush_registry`).
- It is saved into a temporary file `reg<pid><n>.tmp` in the prefix, which
  is then renamed over the old file. (When the old file has more than one
  link or is not a regular file, it is truncated and written in place.) On
  overlayfs this is a new file in the upper layer that replaces a lower one:
  no copy-up of the old file, but from then on **the layer holds a complete
  registry and the base's is hidden**. That is the freeze the task
  describes.
- Format: `WINE REGISTRY Version 2`, a comment naming the branch,
  `#arch=win64`, then per key `[Path\\Escaped] <seconds since 1970>`,
  `#time=<hex, 100 ns since 1601>`, optionally `#class="…"` and `#link`,
  then one line per value: `"name"=` or `@=`, followed by `"string"`,
  `str(2):"…"`, `str(7):"…"`, `dword:%08x`, `hex:…` or `hex(type):…`, hex
  wrapped with a trailing backslash (`save_subkeys`, `dump_value`).
- A key is written only when it has values, or no subkeys, or a class, or is
  a symbolic link; a key that only has subkeys exists through them. Volatile
  keys are never written (`save_subkeys`).
- Loading (`load_keys`): the seconds on the bracket line set the key's time
  unless `#time=` already did (`update_key_time` only sets a time on a key
  that has none); unknown `#` options are ignored; a malformed line is
  reported on standard error ("Malformed value", "Unrecognized input") and
  skipped, it does not stop the load. The prototype looks for those messages
  after every rebase.
- Names are compared without regard to case, 16-bit unit by unit, the
  shorter first when one is a prefix of the other (`find_subkey`,
  `find_value`, `memicmp_strW`). Subkeys and values are kept sorted that
  way, so that is the order of the file. Strings are written in ASCII with
  escapes (`dump_strW`): a backslash before a backslash and before the two
  delimiters, `\x%04x` or `\x%x` above 127, C or octal escapes below 32.

**Starting Wine writes to the prefix even when the program does nothing.**

- `wineboot --init`, which Wine runs at the start of a session (the caller
  is in ntdll and was not read here), opens `<prefix>/.update-timestamp`
  **for reading and writing** before it reads it (`update_timestamp`:
  `_wopen(file, O_RDWR)`). On overlayfs that copies the (tiny) file into
  the layer. It holds `disable` in every prefix `tur-runtime` makes, so
  nothing more happens.
- `wineboot` then writes hardware, computer-name and environment keys
  (`create_hardware_registry_keys`, `create_dynamic_registry_keys`,
  `create_computer_name_keys`, …). Whatever of that is not volatile makes
  `HKEY_LOCAL_MACHINE` dirty, so `system.reg` is written again. This is the
  "noise" the rebase has to tell from an application's changes; how much
  it is was not read but is measured, in an empty layer (section 8.3).

**An update rewrites every DLL.** See "The answer", point 1. The same
function keeps a file that is not Wine's own ("is not a fake dll, not
overwriting it"): a native DLL an installer put into `system32` survives a
Wine update. The rebase's rule for files is the same (section 4.4).

## 3. What the kernel and bubblewrap do (read)

**overlayfs without privilege** (`fs/overlayfs/` and
`Documentation/filesystems/overlayfs.rst` at tag `v7.3`):

- A mount inside a user namespace must use `userxattr`: overlayfs then keeps
  its marks in `user.overlay.*` instead of `trusted.overlay.*`.
- `userxattr` switches `redirect_dir` and `metacopy` off (`params.c`:
  "Resolve userxattr -> !redirect && !metacopy dependency"). Without
  `redirect_dir`, renaming a directory that exists in a lower layer fails
  with `EXDEV` (documented under "renaming directories": "return EXDEV
  error: this error is returned by rename(2) when trying to move a file or
  directory across filesystem boundaries … This is the default behavior").
  Directories an application created itself are in its own layer and rename
  normally. Without `metacopy`, a `chmod` or a mere open-for-writing of a
  lower file copies its whole content.
- Case folding: section "The answer", point 2; in addition the layers must
  have the same encoding and strict encoding is refused (`super.c`).
- Whiteouts (a deleted lower file) are character devices 0:0 in the upper
  layer. Groups and trial layers rely on overlayfs honouring them, and
  `user.overlay.opaque`, in a layer that is later used as a lower layer;
  the groups test exercises that.

**bubblewrap** (`bubblewrap.c` at `v0.11.0`; Fedora 45 has 0.12.0):

- `--overlay-src SRC` (repeatable, bottom layer first) followed by
  `--overlay RWSRC WORKDIR DEST` mounts
  `upperdir=RWSRC,workdir=WORKDIR,lowerdir=<top>:…:<bottom>,userxattr` on
  DEST, after it has entered the new user and mount namespaces. The layer
  directories are named through the old root (`/oldroot`), which is gone
  when the program starts, so they should have no path inside the sandbox;
  the safety test looks.
- The options are refused in setuid mode ("not permitted in setuid mode").
  Fedora's bubblewrap is not setuid.
- Nothing else about the sandbox changes: the same namespaces, the same
  `--disable-userns`. That the program is left without a capability to
  mount with is checked by the safety test.

## 4. The registry rebase

`tests/experiments/layers/winereg.py`. Values are kept as the text Wine
wrote and compared as text (every file was written by the same
`dump_value`); key and value names are decoded with Wine's escape rules and
compared without case.

### 4.1 The record of changes

After an application ran, the layer's registry files are compared with the
registry of everything below the layer, and the difference is stored beside
the layer (`regdelta/<file>.json`), with what was there before:

| Operation | Recorded |
|---|---|
| A key was created | Its path, time, class, link flag, values |
| Values were set | The new text and the old text (or "there was none") |
| Values were deleted | The name and the old text |
| A key was deleted with everything under it | The path, and a digest of every key that was under it |

Times alone are not a change. A key that Wine no longer writes because it
now only has subkeys is not "deleted": existence is "written, or an ancestor
of something written".

### 4.2 The replay

`rebased = replay(new base's registry, record)`:

| The record says | The new base has | Result |
|---|---|---|
| set value V to x (was y) | y | x |
| set value V to x (was y) | x | nothing to do |
| set value V to x (was y) | z, another value | **conflict** `change-vs-change` |
| set value V to x (was y) | no V | **conflict** `change-vs-delete` |
| set value V to x (was absent) | another value | **conflict** `add-vs-add` |
| delete value V (was y) | y | deleted |
| delete value V (was y) | z | **conflict** `delete-vs-change` |
| delete key K | K as it was | deleted with its subtree |
| delete key K | K, with something under it changed | **conflict** `delete-vs-change` |
| change inside key K | K no longer exists | **conflict** `change-vs-delete`; K is made again |

A conflict is settled for the application by default (its value is what it
will look for) and always listed; `prefer="base"` is the other policy. The
result is written through a temporary file and a rename, in Wine's order,
leaving out the keys Wine itself would leave out.

### 4.3 Noise

What Wine rewrites in a registry by merely starting is measured once per
base: an empty layer, `wine cmd /c exit` twice, the record of that layer.
Every (key, value) pair in it is left to the new base during a rebase
instead of being replayed: Wine will write it again anyway, and replaying an
old Wine's value over a new Wine's is exactly the wrong thing.

### 4.4 Files

A file in the layer that also exists in the old base is compared with it. If
it is byte for byte the old base's file it is only there because something
opened or touched it (the `.update-timestamp` above, for one): it would hide
the new base's file, so it is dropped. If it differs, the application put it
there on purpose (a native DLL): it is kept, as Wine's own update keeps it,
and reported; if the new base's file changed too, that is reported as
"both changed". A file of the layer's own that the new base now also has is
reported as hiding it. Whiteouts and opaque directories are counted.

### 4.5 One mechanism for three things

"What lies below the layer" is the base today. With a group it is the base
plus the other application's recorded changes. The same replay then serves
a Wine update (the base changed), joining a group (the registry below gains
another application's keys) and leaving it (it loses them). That is why the
record is kept, rather than recomputed from files at the moment of a
rebase: the old base may be gone by then.

## 5. The sandbox, side by side

The prototype's command lines are written to the evidence
(`sandbox-plain.txt`, `sandbox-layered.txt`) by the run; until it has run,
this is what the code builds. Everything but the last block is
`tur-runtime`'s, option for option:

```
bwrap --unshare-user --unshare-ipc --unshare-pid --unshare-uts --unshare-cgroup-try
      --disable-userns --die-with-parent --new-session
      --ro-bind /usr /usr --ro-bind /etc /etc
      --symlink usr/bin /bin --symlink usr/lib /lib --symlink usr/lib64 /lib64 …
      --proc /proc --dev /dev [--dev-bind /dev/ntsync /dev/ntsync]
      --ro-bind-try /sys/block /sys/block … --tmpfs /tmp
      --ro-bind /tmp/.X11-unix/X<n> /tmp/.X11-unix/X<n>

  today:    --bind PREFIX /tur/prefix
            --bind HOME   /tur/home

  layered:  --overlay-src BASE/root  [--overlay-src LOWER/root …]
            --overlay     LAYER/root LAYER/work-root  /tur/prefix
            --overlay-src BASE/c     [--overlay-src LOWER/c …]
            --overlay     LAYER/c    LAYER/work-c     /tur/prefix/drive_c
            --bind        LAYER/home /tur/home

      [--ro-bind FILE FILE] [--bind GRANT GRANT …]
      --remount-ro / --chdir DIR -- <supervisor> wine PROGRAM …
```

What the boundary rests on, to be confirmed by the run (section 8.6): the
base and other applications' layers have no path in the sandbox; the
program has no capability with which to mount, remount or unmount; a write
to a base file through the view lands in the application's own layer.

## 6. Method

| Hypothesis | Phase of `experiment.py` | What is recorded |
|---|---|---|
| H1 speed and space | `h1`, on ext4, ext4 with a folded drive C:, btrfs; in the containers and in the virtual machine | Time and bytes of a plain prefix (twice) and of a layer (five times); Wine's first start in a layer and what the layer holds after; `wine cmd /c exit` six times each, alternating; a copy of a plain prefix |
| H1, case folding | `fsmatrix.py`, as root only to read the kernel log | Thirteen arrangements of layers × about thirty operations, mounted in a user namespace and again through bubblewrap |
| H2 correctness | `layered`, `plain`, `remove-layered` | The eight installers: install, first window, functional check, uninstall, on layers and on plain prefixes; the base's SHA-256 before and after |
| H3 rebase | `old-base` (Fedora 43's Wine), `update`, `rebase` (Fedora 45's) | Byte-for-byte round trip of every base registry file; the five applications before and after; conflicts; Wine's complaints; the noise |
| H4 groups | `groups` | 7-Zip seen from Notepad++: absent, present, absent; registry both ways; a runtime layer |
| H5 safety | `safety`, in the containers and on the 7.3 kernel | Seventeen attempts from inside; hashes of the base and of another layer |
| Also | `update`, `disk`, `extras` | `wineboot --update` three ways; ten applications; identical files; snapshot, rollback, reset, trial layers |

Environment. **Not the TUR image.** Job `layers`: a hosted Ubuntu 24.04
runner; Fedora 43's and Fedora 45's containers with their own Wine; the
runner's kernel. Job `kernel`: Fedora 45's kernel packages unpacked and
booted in QEMU with KVM, root file system a copy of the Fedora 45 container,
no network. No GPU, no sound, no SELinux, Xvfb with Openbox.

## 7. Kill criteria

From the README, unchanged:

1. Overlay composition cannot be done without root inside TUR's sandbox on
   the image's kernel. **NOT RUN.**
2. The registry rebase cannot be made reliable on the five applications
   across a real Wine version change. **NOT RUN.**

## 8. Results

**NOT RUN.** The workflow's run had not been given a runner when this was
written. The tables below are the ones `report.py` prints from the run's
`results.json`; they are listed so that it is clear what will and will not
be claimed.

### 8.1 overlayfs without privilege, by file system and case folding

NOT RUN (runner's kernel). NOT RUN (kernel 7.3).

### 8.2 H1: prefix against layer

NOT RUN: plain prefix (time, bytes), layer (time, bytes), Wine's first start
in a layer, start-up plain against layered, on ext4, ext4 with a folded
drive C:, btrfs; on both kernels.

### 8.3 H3: registry rebase across a Wine version change

NOT RUN: round trip of the registry files; noise; per application: its
changes, what was replayed, conflicts, stale files dropped, launch and check
after the rebase onto the updated base and onto a fresh base; `wineboot
--update` on the base, on a plain prefix, and naively on a layer.

### 8.4 H2: the applications, layered and plain

NOT RUN.

### 8.5 H4: groups and a runtime layer

NOT RUN.

### 8.6 H5: safety

NOT RUN.

### 8.7 Ten applications, identical files, snapshots, reset, trial layers

NOT RUN.

## 9. What tur-runtime and the autopilot would need

This is a description, not a change; `services/` is untouched. It assumes
the kill criteria do not fire.

**tur-runtime**

| Area | Today | With layers |
|---|---|---|
| Layout (`paths.rs`) | `prefixes/<app-id>/prefix`, `home` | `bases/<base-id>/{root,c}`, `layers/<id>/{root,c,work-root,work-c,home,regdelta}`; a layer's record names what lies below it |
| Sandbox (`sandbox.rs`) | `MountKind::ReadWrite` for the prefix, at its host path | A mount kind for an overlay (`--overlay-src … --overlay …`), twice; fixed paths inside the sandbox for the prefix and the home; bubblewrap 0.11 or later, not setuid, checked by the host probe. The boundary tests that read the arguments back gain: no lower layer is ever mounted writable, and the only upper layer is the application's own |
| `prefix create` | `wineboot --init`, tens of seconds | Make four directories and a record. The base is made once per Wine build (`wineboot --init` as today, then sealed and hashed), on first need or ahead of it |
| `prefix verify`, `repair` | Checks; `wineboot --update` | The base's hash against its record; a repair is "write the registry again from the record, drop stale files". A damaged base is rebuilt without touching any application |
| `back up`, `restore`, `reset` | `tar` of 1.3 GB and more | The layer only. A reset empties the layer and keeps the home |
| `migrate` (`prefix.rs`) | Backup, then `wineboot --update` in the prefix | Snapshot of the layer, then the rebase; refused, with the list, when conflicts exceed what policy allows. `wineboot --update` must never run in a layer (section "The answer") |
| Registry (`winreg.rs`) | Reads uninstall entries | Also: write, diff, replay. The record is written when the sandbox ends, by the supervisor's parent, outside the sandbox |
| Launch plan | Version mismatch is refused (`TUR-WINDOWS-PREFIX-006`) | A layer on an older base is rebased first (a plan step, visible in `--dry-run`), then started |
| Metadata schema | `runtime_version` | Plus the base's identifier and digest, what lies below, the state of the record |
| Concurrency | Not addressed: two launches of one application are two sandboxes and two Wine servers on one prefix | Must be addressed: one sandbox per layer at a time; a second program of the same application has to be started inside the first sandbox. Section 8.6 records what overlayfs does when it is not |
| Application record | Host paths into the prefix | Paths relative to drive C:, resolved through the layers |

**The autopilot** (`autopilot_run.rs`, `CopyTrials`): a trial is today a
copy of the prefix and home (`cp -a --reflink=always`, falling back to a
full copy on ext4, which is what TUR's root is). With layers a trial is an
empty layer over the base and the application's layer, which is then a
read-only lower layer: nothing is copied on any file system, and "the real
prefix is byte for byte what it was" holds by construction instead of by
fingerprint. The `drive-c-case-folding` remedy, which rebuilds drive C:,
becomes a different question (a folded base is another base).

**The installer and the image**: nothing, for layers as such. A base could
later be shipped in the image, built when the image is built (section 10,
decision 5).

## 10. Proposed decision text, and what the owner has to decide

No numbered ADR is written (house rule). If the results support it, this is
the text proposed for one:

> **Windows applications run on layers.** A Windows application's Wine
> prefix is composed at launch from a read-only base layer, one per Wine
> build, zero or more read-only shared layers, and the application's own
> writable layer, with overlayfs mounted by bubblewrap inside the sandbox's
> user namespace. Drive C: is composed separately from the rest of the
> prefix. Every sandbox sees its prefix and its home at the same two paths.
> The sandbox is otherwise unchanged; no privilege and no helper is added.
> A layer records what it changed in the registry. When the base changes,
> or what lies below a layer changes, the layer's registry is rebuilt from
> that record on the new registry (a three-way rebase); conflicts are
> listed, settled for the application, and block the operation above a
> threshold. Wine's own prefix update is run on the base only, never in a
> layer. Applications see each other only when the person, or the
> catalogue with the person's consent, puts them in one group; a group is a
> stacking order, not a shared writable prefix. One prefix per application
> (specification §12.2) is kept in meaning: one writable layer per
> application.

Decisions that are the owner's, whatever the numbers:

1. **Default or option.** Layers for every new Windows application, or
   behind a flag until they have been used on the image.
2. **The new base: updated or fresh.** Updating the old base once with
   Wine keeps the machine's identity values (an application that bound a
   licence to them keeps working) and keeps whatever old Wine versions left
   behind. A fresh base is clean and loses them. Both are measured (8.3).
3. **Conflict policy.** The application wins and it is listed (the
   prototype's default), or the launch stops and asks.
4. **Groups.** Who decides that two applications belong together: the
   person, the catalogue, or an installer that looks for another
   application; and whether "everything installed is simply there" should
   ever mean one group for everything, which gives up the isolation between
   applications that one-prefix-per-application exists for.
5. **A base in the image.** Building the base when the image is built and
   shipping it under `/usr` would make the first Windows application start
   as fast as the tenth and tie the base to the image's rollback. It needs
   an answer to prefix ownership and the per-user profile directory, and
   was not tried.
6. **Case folding.** If overlayfs over folded directories works on 7.3
   only in the two-overlay arrangement, `--casefold` stays possible; it is
   still off by default for the reasons of `windows-runtime-3`.

Order of adoption proposed, each step useful alone and each reversible:

1. Fixed paths inside the sandbox for the prefix and the home (no change in
   behaviour; makes a prefix relocatable).
2. Trial layers for the autopilot (the largest saving on ext4, where a
   trial is a full copy today; the real prefix can stay a plain one, used
   as the lower layer).
3. Base and application layers for new installations, behind a flag;
   existing prefixes untouched.
4. The registry record and the rebase; migration by rebase.
5. Groups and runtime layers.
6. A content-addressed store, only if 8.7 shows it would save enough to
   matter.

## 11. Limits

- **No result yet.** Everything in sections 8 is NOT RUN.
- Not the TUR image: no SELinux (overlayfs in a user namespace under
  Fedora's policy is untested), no Plasma, no real disk, no fscrypt.
- The kernel of the virtual machine is Fedora 45's package of the day; the
  image's is 7.3.3-300.fc45. The exact version is in the evidence.
- The old Wine is Fedora 43's (11.0 on the day), the new one Fedora 45's.
  One version change, one direction. A rebase back (an image rollback) is
  exercised only in the registry module's own test.
- Five applications, eight installers, one window and one function each, no
  .NET, no Microsoft redistributable, no game, no launcher, no GPU.
- Applications are not running while their base changes; a rebase is not
  interrupted on purpose (no crash-consistency test); two sandboxes on one
  layer are probed once, not explored.
- Value comparison is textual. A value Wine would write differently in
  another version (it has not changed its format in years) would read as
  changed. Case-insensitive comparison of names uses Python's lower-casing,
  not Wine's table; they differ for a few characters outside ASCII.
- Start-up is measured warm. Lookups through overlayfs are measured with
  2 000 files, not with a real application's tree on a cold cache.
- `Z:` is still the sandbox's root, as today.
