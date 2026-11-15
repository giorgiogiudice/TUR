<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Experiment `layers`: layered Windows prefixes

A time-boxed research spike. Nothing here is part of TUR's services: it is a
prototype in Python that runs in CI only (`.github/workflows/exp-layers.yml`),
beside `services/tur-runtime`, to find out whether TUR should compose a
Windows application's environment from shared layers instead of giving every
application a whole Wine prefix. The report is
`docs/reports/exp-layers-1.md`.

**This file was written before anything was measured.** It registers the
hypotheses, how each is tested, what counts as passing, and what kills the
idea. It is not edited to fit results; corrections after the first run are
listed at the end with their date.

## The idea

| Layer | Holds | Written |
|---|---|---|
| Base | A freshly initialised Wine prefix for one Wine build: `system32`, the registry Wine creates | Once per Wine build, then never |
| Runtime (optional) | A redistributable component several applications need | Once, then never |
| Application | What the application's installer and the application itself wrote | By that application only |
| Data | The person's documents: the private home the profile's folders point into | By the person's applications |

Composed at launch by bubblewrap, in the user namespace it creates anyway,
with the kernel's overlayfs (`--overlay-src … --overlay …`; no root, no
helper). Drive C: is a second overlay mounted on the first one's `drive_c`,
so that a case-folded drive C: stays possible. Every sandbox sees the same
two paths (`/tur/prefix`, `/tur/home`), so a base holds no path that names an
application.

## Hypotheses

**H1. Speed and space.** With a shared base, creating an application's
environment takes under 2 seconds and under 10 MB before the application's
own files.

- Measured: wall-clock time and bytes (apparent and allocated) of (a) a plain
  prefix as `tur-runtime` makes one today (`wineboot --init`, updating
  switched off), twice; (b) an application layer (five times); (c) the first
  start of Wine in a new layer, and what the layer holds afterwards.
- On ext4 (as bootc makes TUR's root: no casefold feature), ext4 with a
  case-folded drive C:, and btrfs. On the runner's kernel and, in a virtual
  machine, on Fedora 45's kernel (the image's).
- Passes if (b) is under 2 s and the layer under 10 MB, and (c) leaves the
  layer under 10 MB, on the file system TUR uses (ext4). "Creating" is
  reported both ways: making the layer, and making it plus Wine's first
  start in it.
- Also answered, pass or fail: does overlayfs mount over a case-folded
  directory on kernel 7.3, without privilege, and in which arrangements
  (`fsmatrix.py`: thirteen arrangements, about thirty operations each)?

**H2. Correctness.** The five real applications TUR already tests (7-Zip,
Notepad++, PuTTY, VLC, Inno Setup; `tests/windows-runtime/apps.json`, all
eight installers) install, start, pass their functional checks and uninstall
on layers, and the base layer's bytes are unchanged afterwards.

- The checks are the ones of `tests/windows-runtime/apps.py`, imported, not
  rewritten. Installing and removing are done by this prototype with the
  commands `tur-runtime` documents (it cannot be asked to install into a
  prefix it did not create).
- Control: the same eight on plain prefixes, same Wine, same runner, same
  job. A step that fails both ways is the environment's, not the layers'.
- Passes if every step that passes on a plain prefix passes on layers, and
  the SHA-256 over the base's files, modes and link targets is the same
  before and after.

**H3. Registry rebase.** Wine rewrites a whole registry file on the first
change, which copies it into the application layer and would hide a new
base's registry behind it. A three-way rebase (what the layer changed
relative to its old base, replayed on the new base, conflicts detected)
makes a layer follow a new base.

- Proven on a real version change: the five applications are installed on
  layers over a base made by Fedora 43's Wine (11.0), then, in another
  container, moved onto a base of Fedora 45's Wine (11.20, the version TUR
  ships; whatever versions the two releases carry on the day are recorded),
  started, checked and uninstalled under the new Wine.
- Two new bases are tried: the old base updated once by Wine itself
  (`wineboot --update`, the primary: it keeps the machine's identity values)
  and a fresh one.
- Control: the same applications freshly installed on layers under the new
  Wine (H2). A step that fails after a rebase and passes fresh is the
  rebase's fault.
- Wine's own noise (what it rewrites in a registry by merely starting) is
  measured in an empty layer and left to the new base.
- Passes if all five start and pass their checks after the rebase onto the
  updated base, Wine reports no malformed registry input, and every conflict
  is listed. Conflicts are reported with their number and kind whatever the
  outcome.
- The parser is held to Wine's own format: every base registry file must
  come back byte for byte from parse-and-write.

**H4. Groups.** Two applications installed in separate layers can be composed
so that one sees the other, and separated again, with the registry merged
correctly both ways.

- 7-Zip and Notepad++: from Notepad++'s sandbox, 7-Zip's program is absent
  and its registry key is absent; joined, the program runs and both
  applications' keys are there; a value written while joined survives the
  separation; separated, 7-Zip's key is gone again. 7-Zip's layer is hashed
  before and after: being seen must not change it.
- A runtime layer: a test program that imports a DLL it does not ship fails
  alone, runs with the runtime layer under it (DLL and registry value), and
  fails again without it.

**H5. Safety.** One application cannot write into another's layer or the
base, and the sandbox boundary TUR has today is kept.

- From inside a layered sandbox: the base, other layers and the store are
  not reachable by path; remounting, unmounting and mounting are refused;
  changes to base files through the view land in the application's own
  layer; the base and another layer hash the same afterwards.
- The bubblewrap command line is recorded, plain and layered, so the two
  can be compared option by option.

## Kill criteria

The idea fails **as a default** if either holds:

1. Overlay composition cannot be done without root inside TUR's sandbox on
   the image's kernel (Fedora 45's, in the virtual machine): bubblewrap's
   unprivileged overlay mount fails there, or needs `--disable-userns`
   dropped, a setuid helper, or any other weakening of today's sandbox.
2. The registry rebase cannot be made reliable on the five applications
   across the real Wine version change: an application that works freshly
   installed under the new Wine does not work after its layer is rebased.

A failed H1 number or a failed H4 does not kill the idea; it changes what is
recommended.

## Also measured

- Wine's own prefix update at a version change (`wineboot --update`): on the
  base (once per machine), on a plain prefix holding an application (today's
  migration, per prefix), and on a layer over the old base (the naive way:
  how much of the base Wine copies into the layer), against a rebase.
- Disk use for ten installed applications (the eight installers and second
  copies of two), layered and not; and how many bytes identical files across
  layers would save in a content-addressed store.
- Start-up: `wine cmd /c exit` alternating between a plain prefix and a
  layer; time to the first window of each application, plain and layered;
  the cost of a lookup through overlayfs.
- Snapshot and rollback of a layer; factory reset that keeps the person's
  documents; a trial layer over an application (the autopilot's throwaway
  copy) against the copy made today.

## What is not tested here

Stated now so that it is not mistaken for a result later: the TUR image
itself (SELinux, Plasma, a real disk); a GPU, sound, games, launchers, .NET
or any Microsoft redistributable; Proton; applications running while their
base changes; two sandboxes writing one layer at once beyond a single probe;
crash consistency of a rebase (power loss in the middle); performance on a
cold cache.

## Files

| File | What it is |
|---|---|
| `winereg.py` | Wine's registry file format, written from `server/registry.c` of Wine 11.20: read, write, diff, replay, three-way rebase. `--self-test` |
| `layerlib.py` | The layer store and the sandbox (bubblewrap command line) |
| `lrun.py` | Starts one program of an application in its sandbox |
| `experiment.py` | The phases; results as JSON |
| `fsmatrix.py` | overlayfs × file system × case folding, without privilege |
| `report.py` | Tables from the JSON |
| `fs-setup.sh`, `container.sh`, `vm.sh`, `vm-init.sh`, `vm-inner.sh` | The CI plumbing: file systems, the two Fedora containers, the virtual machine with Fedora 45's kernel |
| `rt/` | The two test programs of the runtime-layer test |
