<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# autopilot-1 — The compatibility autopilot, measured

```text
TUR DEVELOPMENT REPORT

Task ID:          autopilot-1
Subsystem:        Universal Runtime Manager (services/tur-runtime), with the
                  catalogue's record format (services/tur-catalogue)
Build ID:         not applicable: this task builds no image
Source revision:  854990c (feat/compat-autopilot, pull request 94), the commit
                  the measurements below come from. The commits after it add
                  this report and rustfmt's line breaks; they change no
                  behaviour, and CI ran again on them.

SUMMARY
- What changed: tur-runtime app autopilot <app-id> runs a Windows
  application unchanged in a throwaway copy of its prefix, reads a stable
  failure class from the evidence (TUR-WINDOWS-COMPAT-001..012), tries the
  remedies of a declarative, sourced catalogue chosen for that class, each in
  a fresh copy, singly and then in bounded pairs, and stops at the first that
  makes the application's check pass. It records every attempt, the remedy
  found, the time taken and the real prefix's fingerprints before and after
  (equal), and writes a compatibility record that tur-catalogue validates and
  routes as "works with these settings", never as a rating. --apply sets a
  found remedy in the real prefix after a backup; --revert restores it.
  P@x gets two read-only tools: what would be tried, and what was found.
- Why: this is done by hand on forums today.

FILES
- Added: services/tur-runtime/src/{failure,remedy,autopilot,autopilot_run}.rs,
  services/tur-runtime/remedies/catalogue.json,
  schemas/remedy-{catalogue,check,result,applied}.schema.json,
  tests/windows-runtime/autopilot.py,
  tests/windows-runtime/autopilot/{build.sh,winver.c,dllover.c,
  broken-d3dx9.c,mfc.c,mfc140u.def,casepath.c,codepage.c,codepage.rc,
  vdesktop.c,dpi.c},
  docs/architecture/compat-autopilot.md, docs/reports/autopilot-1.md
- Modified: services/tur-runtime/src/{codes,lib,main,pe_res}.rs,
  services/tur-runtime/agent-tools{,.adapter}.json,
  services/tur-runtime/tests/agent_tools.rs,
  services/tur-catalogue/src/{lib,model,report,route,validate}.rs,
  services/tur-catalogue/tests/{cli,router_space,seed}.rs,
  schemas/app-catalogue-entry.schema.json, catalogue/README.md,
  .github/workflows/windows-runtime.yml, docs/architecture/runtime-manager.md
- Removed: none

BUILD
- cargo clippy -D warnings, cargo test (workflow rust); cargo build --locked
  of tur-runtime and tur-catalogue, mingw-w64 for the test programs
  (workflow windows-runtime, job "Compatibility autopilot")
- Warnings: none from the compiler or clippy

TESTS
- Unit and integration tests, tur-runtime and tur-catalogue: pass (workflow
  rust, run 43379480977; the job failed only on rustfmt's line breaks, which
  the next commit applies). New: failure::tests (every Wine message, the
  earliest cause, Microsoft components, observed classes, hints),
  remedy::tests (the shipped catalogue, schema and types in step, refusals),
  autopilot::tests (search order, progress pairs, the bound, a Microsoft
  component, results and their digest, the record), autopilot_run::tests
  (window trees, fingerprints, collisions, copies, merge), pe_res (resource
  languages), tests/agent_tools.rs (the two tools run and change nothing),
  tur-catalogue validate, route and cli tests of tur-autopilot records.
- P@x invariants (services/tur-pax/tests/tools.rs, unchanged): pass; every
  offered tool is class 0.
- desktop_fixtures.rs: pass; the application centre's recorded router output
  is unchanged.
- Autopilot end to end: job "Compatibility autopilot", run 43379482258, job
  131733856021: 226 checks, 0 failed. Tables below.
- The other windows-runtime jobs, apps, pax, image, boot: pass on the same
  commit.
- Boot tests: run by the boot workflow and passed; nothing here touches boot.
- Hardware tests: NOT RUN. No GPU, no sound device, no ntsync device.
- On the TUR image: NOT RUN. Wine is not in the image.

DIAGNOSTICS
- Artifact "windows-runtime-autopilot" of run 43379482258: versions, file
  systems, the build's import lists, per case the autopilot's JSON, its
  result.json, record.toml and every attempt's log, the catalogue validation
  and route, the apply, the launch after it, the results list; per real
  application the same; autopilot.md and autopilot.json.
- Relevant error IDs: TUR-WINDOWS-COMPAT-001..012 (failure classes),
  TUR-WINDOWS-AUTOPILOT-001..009 (new).

RECOVERY
- Rollback: revert the merge. Nothing else reads the new files; an applied
  remedy is undone with --revert, or with prefix restore of the backup it
  names.
```

## What was run

Ubuntu 24.04.5 on a hosted runner (image ubuntu24 20261111.370.1, kernel
7.0.0-1017-azure), Wine 9.0 from Ubuntu (`9.0~repack-4build3`, 64- and 32-bit),
bubblewrap 0.9.0, coreutils `cp` 9.4, Xvfb 1280x800 with openbox, no GPU.
The data directory was on an ext4 file system made in the job with the
`casefold` feature (a loop device), so that the case-folding remedy could be
tried; two cases ran again with it on btrfs to measure copy-on-write copies.
Every attempt ran in the sandbox, in a throwaway copy, without network and
without granted directories.

## Programs broken on purpose

Built in the job from TUR's own C sources with mingw-w64. Each was installed
with `app install` (a single program with `--portable`, or a zip), then run
through the autopilot with a console check (exit status 0 and
`TUR-CHECK-PASS` in the output, 60 seconds). Then, in order: the real prefix
and private home were hashed by content before and after and had to be equal;
the record was appended to a scratch catalogue entry, `tur-catalogue validate`
had to accept it and `tur-catalogue route` had to keep the wine route's rating
`unknown` and show the remedies as its `settings`; `--apply` had to succeed
and leave a backup; `app launch` had to pass the program's own check; the
autopilot, run again, had to find nothing to do; `--revert` had to bring the
prefix back to its content hash before `--apply`; `--results` had to list both
runs and no applied remedy.

| Case | What is wrong | Failure class (code) | Remedy found | Attempts | Autopilot time | Copy | After `--apply` |
|---|---|---|---|---|---|---|---|
| winver | refuses any Windows older than 11 | exited-with-error (TUR-WINDOWS-COMPAT-009) | `windows-version-win11` | 1 | 13.2 s | copy, 0.6 s | passed unchanged |
| dllover | ships a d3dx9_43.dll that fails to initialise | dll-init-failed (TUR-WINDOWS-COMPAT-006) | `dll-builtin:d3dx9_43` | 1 | 23.5 s | copy, 0.6 s | passed unchanged |
| combo | both: the broken DLL, then the version check | dll-init-failed (TUR-WINDOWS-COMPAT-006) | `dll-builtin:d3dx9_43` + `windows-version-win11` | 2 | 18.3 s | copy, 0.6 s | passed unchanged |
| mfc | imports mfc140u.dll, a Microsoft component | microsoft-component-missing (TUR-WINDOWS-COMPAT-003) | none; Windows guest suggested, exit 10, `--apply` refused | 0 | 4.2 s | copy, 0.6 s | - |
| casepath | its archive spells a directory Plugins and plugins | exited-with-error (TUR-WINDOWS-COMPAT-009) | `drive-c-case-insensitive` | 1 | 9.2 s | copy, 0.6 s | passed unchanged |
| codepage | Japanese resources; crashes off code page 932 | crash (TUR-WINDOWS-COMPAT-008) | `locale-ja` | 1 | 7.5 s | copy, 0.6 s | passed unchanged |
| vdesktop | refuses a screen larger than 1024x768 | exited-with-error (TUR-WINDOWS-COMPAT-009) | `virtual-desktop-1024x768` | 1 | 25.2 s | copy, 0.6 s | passed unchanged |
| dpi | refuses fewer than 120 DPI | exited-with-error (TUR-WINDOWS-COMPAT-009) | `dpi-120` | 1 | 20.4 s | copy, 0.6 s | passed unchanged |

"Attempts" does not count the baseline. "Autopilot time" is the whole command,
measured by the driver. The time of each attempt (copy, remedy, run), baseline
first, in seconds: winver 1.2, 1.8; dllover 1.9, 18.4; combo 1.2, 3.1, 3.1;
mfc 1.2; casepath 1.3, 6.4; codepage 1.4, 4.5; vdesktop 2.5, 21.1; dpi 2.5,
16.4. Where an attempt took 16 to 21 seconds the program itself was quick
(1.8 to 2.9 s): the time went into setting the remedy, `reg import` in the
sandbox, which took 13.0 to 15.7 s in those attempts (dllover, vdesktop, dpi)
and under 2 s in others. Why was not established.

`combo` shows the progress rule: `dll-builtin:d3dx9_43` alone got the program
past its DLL and into its version check (failure class changed to
exited-with-error), and the autopilot then tried the version remedies on top
of it. Its record:

```toml
[[editions.records]]
route = "wine"
rating = "unknown"
reported_by = "tur-autopilot"
date = "2026-11-13"
remedies = ["dll-builtin:d3dx9_43", "windows-version-win11"]
runtime_version = "wine-9.0"
evidence_sha256 = "0869dda819dc19bb8a6a73626834b5dda77c87bc9d1b02133fb4d5150bf51c51"
notes = "tur-runtime compatibility autopilot, result tur.test.autopilot-combo@20261113T064040Z: failed as dll-init-failed (TUR-WINDOWS-COMPAT-006) unchanged; passed TUR's check (its exit status and output) in a throwaway copy of its prefix with these settings, after 2 attempts. wine-9.0 (Ubuntu 9.0~repack-4build3). Measured once, on one machine; not a rating."
```

## Copies: copy and copy-on-write

| Data directory | Case | Copy method | First copy | Size copied |
|---|---|---|---|---|
| ext4 (casefold), loop device | every case | copy | 0.56 to 0.62 s | 1 309 270 654 to 1 309 360 638 bytes, 1 755 to 1 757 files |
| btrfs, loop device | winver | reflink | 0.052 s | 1 309 271 379 bytes, 1 755 files |
| btrfs, loop device | dllover | reflink | 0.068 s | 1 309 359 688 bytes, 1 756 files |

Copy-on-write made the first copy about ten times faster, and the whole
autopilot run of the same programs 2.8 s and 4.1 s instead of 13.2 s and
23.5 s. The ordinary copy was fast because the runner's page cache held the
prefix; on a real disk it would be slower. A Wine 9.0 prefix on this runner is
about 1.3 GB in 1 755 files. `--apply` took 57 to 67 seconds, nearly all of it
the `tar --gzip` backup of that prefix.

## Real applications with a fault injected

The free applications of `tests/windows-runtime/apps.json`, downloaded from
their publishers with the pinned sizes and SHA-256 digests, installed with
`app install`, and run through the autopilot with the default check (a window
of the program's own within 60 seconds, then 3 seconds up). Unchanged, each
had to pass. Then one DLL its main program imports (read from its import
table) was disabled for that program in the prefix, as a stale override
leaves it (`HKCU\Software\Wine\AppDefaults\<program>\DllOverrides`, value
empty), with Wine's `reg add` outside the sandbox: this is the test's fault,
not something the autopilot does. The autopilot had to name the class and the
remedy; after `--apply` it had to pass the application unchanged.

| Application | Unchanged | Fault | Failure class (code) | Remedy found | Attempts | Autopilot time | Copy | After `--apply` |
|---|---|---|---|---|---|---|---|---|
| 7-Zip 26.06 | passed, 13.5 s | comctl32 disabled for 7zFM.exe | dll-missing (TUR-WINDOWS-COMPAT-002) | `dll-builtin:comctl32` | 1 | 24.5 s | copy, 0.60 s | passed unchanged |
| Notepad++ 8.9.9.1 (portable) | passed, 16.1 s | comctl32 disabled for notepad++.exe | dll-missing (TUR-WINDOWS-COMPAT-002) | `dll-builtin:comctl32` | 1 | 25.1 s | copy, 0.63 s | passed unchanged |
| PuTTY 0.85 (single program) | passed, 14.7 s | comdlg32 disabled for putty.exe | dll-missing (TUR-WINDOWS-COMPAT-002) | `dll-builtin:comdlg32` | 1 | 22.3 s | copy, 0.60 s | passed unchanged |
| Inno Setup 7.1.1 | passed, 8.0 s | comctl32 disabled for ISIDE.exe | dll-missing (TUR-WINDOWS-COMPAT-002) | `dll-builtin:comctl32` | 1 | 19.5 s | copy, 0.71 s | passed unchanged |
| VLC 3.0.24 | passed, 13.5 s | shell32 disabled for vlc.exe | dll-missing (TUR-WINDOWS-COMPAT-002) | `dll-builtin:shell32` | 1 | 17.7 s | copy, 0.74 s | passed unchanged |

Installation took 11.9 to 28.2 s, removal 0.1 to 3.3 s. The whole job,
including installing packages and building, took 29 minutes 13 seconds; the
driver 27 minutes 4 seconds.

## What the earlier CI runs found

The first two runs of the job (runs 43369129407 on e0bb9e8 and 43376390364 on
3760771) found
four problems, fixed before these measurements:

1. `cp -a` refuses to copy `plugins` into a case-folded directory that has
   just received `Plugins` ("File exists"). Copies into a case-folded drive
   C: now go through `merge_copy`, which merges them.
2. A store to address `0x81` did not fault under Wine 9.0, so the code-page
   program exited instead of crashing; it raises the access violation
   explicitly now.
3. `GetSystemMetrics(SM_CXSCREEN)` asked before anything else in a console
   program reported the X screen (1280x800) even with a virtual desktop
   configured; asking for the desktop window first gives 1024x768.
4. `--apply` refused some valid results as "evidence does not match its
   digest": a time written to JSON and read back can differ in its last
   digit. The digest now takes times in whole milliseconds.

## What this does not show

- Nothing ran on the TUR image, on Fedora, under SELinux, on Wayland, with a
  GPU, a sound device or `/dev/ntsync`. The renderer, audio, font and ntsync
  remedies, and the 800x600 desktop, 144 DPI, Windows 10/7/XP, native-DLL and
  Chinese and Korean locale remedies, were never chosen by these cases and are
  unmeasured.
- The broken programs are TUR's own and broken in one known way each; the
  real applications' faults were injected. No proprietary application was
  tried; no application whose real-world problem was unknown in advance.
- One run of each, on one machine. Times vary with the runner.
- Proton was not run.
