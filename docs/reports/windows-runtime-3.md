# windows-runtime-3 — Case-insensitive Wine prefixes with ext4 case folding

```text
TUR DEVELOPMENT REPORT

Task ID:          windows-runtime-3 (issue #81)
Subsystem:        Universal Runtime Manager (services/tur-runtime)
Build ID:         not applicable: this task builds no image
Source revision:  5d289bb (feat/casefold-prefixes, pull request 82), the
                  commit the measurements below come from. The commit that
                  adds this report and the design note's decision follows
                  it; it changes no code, only the name of a Wine function
                  in a comment (setup_config_dir).

SUMMARY
- What changed: tur-runtime can create a prefix whose drive_c is
  case-insensitive in the kernel: drive_c is created empty and given ext4's
  case-folding attribute (chattr +F) before Wine writes into it. A file
  system that cannot fold case is reported, not a failure: the prefix is
  created as before and the reason is recorded (TUR-WINDOWS-PREFIX-009,
  -010). The application record says whether drive C: is case-insensitive
  and why. --casefold and --no-casefold on prefix create and app install;
  reset and restore keep what the replaced prefix had. The windows-runtime
  workflow has a third job that makes a casefold-capable ext4 file system
  and measures both kinds of prefix with a test program and with the eight
  real installers, twice each.
- Decision, from the measurements: off by default. Every application still
  passed, and none was faster; the test program showed case folding much
  faster for names asked for in another case, and slower for creating
  files, for names that are not there and for a search for one exact name,
  and it changed what some names resolve to.
- Why it changed: issue #81 (an idea from 13 November 2026): let the kernel
  do what Wine emulates, and measure whether that helps.

FILES
- Added: services/tur-runtime/src/casefold.rs,
  tests/windows-runtime/{casefold.c,casefold.py,compare.py},
  docs/reports/windows-runtime-3.md
- Modified: services/tur-runtime/src/{app_record,codes,exec,install,launch,
  launch_host,lib,main,plan,prefix}.rs,
  services/tur-runtime/tests/{app_cli,launch_cli}.rs,
  schemas/app-record.schema.json, tests/windows-runtime/apps.py,
  .github/workflows/windows-runtime.yml,
  docs/architecture/runtime-manager.md
- Removed: none

BUILD
- Command: cargo clippy --all-targets --all-features -- -D warnings; cargo
  test (workflow "rust"); cargo build --locked and mingw-w64 for the test
  program (workflow "windows-runtime")
- Result: see RUNS below
- Warnings: none from the compiler or clippy

TESTS
- Unit and integration tests of services/tur-runtime: pass (workflow rust,
  run 43334979482). New: casefold::tests (chattr messages classified, codes,
  records consistent or refused, lsattr parsed), exec::tests (the case-fold
  step with a chattr that succeeds, refuses, is missing; the plan goes on),
  prefix::tests (drive_c made before Wine; Proton left alone; reset and
  restore keep it), app_record::tests (schema and types in step; three
  contradictory records refused), launch_cli (the plan with --casefold,
  without, and both refused), app_cli (a record without the field is read).
- End to end, launch path: job "end-to-end", unchanged. Pass.
- End to end, real applications with the default: job "real-applications",
  unchanged. Pass, 32 of 32 steps.
- Case folding: job "case-folding", run 43334979526. Pass. Tables below.
- Boot tests: NOT RUN. Nothing here can affect boot.
- Hardware tests: NOT RUN. No GPU, no real disk: a loop device.
- On the TUR image: NOT RUN. Wine is not in the image.

DIAGNOSTICS
- Log paths: artifact "windows-runtime-casefold" of run 43334979526:
  versions, the file system made, program/casefold.json and .md, the four
  application passes (apps-N-*/results.json, compatibility.md, per
  application the record, the reports and logs), casefold-apps.md and .json
- Diagnostic bundle path: none; no bundle format exists yet
- Relevant error IDs: TUR-WINDOWS-PREFIX-009, TUR-WINDOWS-PREFIX-010 (new)

RECOVERY
- Rollback procedure: revert the merge. The default is off, so nothing
  changes for a prefix unless --casefold is given. A case-folded prefix
  stays usable by an older tur-runtime: the attribute is on the directory,
  and the record's new member is optional, but an older build refuses a
  record that has it (unknown fields are refused); `prefix remove` still
  works.
- Recovery tested: the fallback on a file system without the feature, for
  real (the runner's own disk); reset and restore as plans only.

KNOWN LIMITATIONS
- See "Limits" below.

NEXT SAFE ACTION
- When the installer makes /var/home ext4 with -O casefold and Wine is in
  the image, run the case-folding job's program and passes on the TUR image.
```

## Runs

| Commit | Workflow | Run | Result |
|---|---|---|---|
| `b6eac47`, `b6a8efd` | none | | No workflow ran: they run for pull requests, and the pull request was opened at `b6a8efd` |
| `b6a8efd` | rust | 43329671972 | Failed for `services/tur-runtime`: one test (`tests/app_cli.rs`) built an application record without the new field, and rustfmt drift. Everything else compiled |
| `b6a8efd` | windows-runtime and the others | 43329672081 … | Cancelled or superseded by the next push; no result used |
| `5d289bb` | rust | 43334979482 | Passed, every component |
| `5d289bb` | windows-runtime | 43334979526 | Passed: end-to-end; real applications 32 of 32; case-folding (test program, four passes of eight installers, comparison). **This report is written from this run** |
| `5d289bb` | apps, boot, image, pax, gate | 43334979519, 43334979507, 43334979653, 43334979458, 43334979532 | Passed |

The case-folding job ran once to the end for this report. Its numbers are
one run on one runner, not a distribution. The commit that adds the report
runs the workflow again; that run is named in the pull request, and its
numbers are not in this report.

## Where this ran

```text
runner image: ubuntu24 20261111.370.1
os: Ubuntu 24.04.5 LTS
kernel: 7.0.0-1017-azure
kernel CONFIG_UNICODE: CONFIG_UNICODE=y
kernel ext4 casefold support (/sys/fs/ext4/features/casefold): supported
wine: wine-9.0 (Ubuntu 9.0~repack-4build3)
bubblewrap: bubblewrap 0.9.0
e2fsprogs: mke2fs 1.47.0 (5-Feb-2023)
mingw x86_64: x86_64-w64-mingw32-gcc (GCC) 13-win32
runner's own disk under RUNNER_TEMP: /dev/nvme0n1p1 ext4
features of the runner's root file system (/dev/nvme0n1p1):
  has_journal ext_attr resize_inode dir_index filetype needs_recovery extent
  64bit flex_bg sparse_super large_file huge_file dir_nlink extra_isize
  metadata_csum
```

The file system with the feature, made by the job:

```text
image: /home/runner/work/_temp/casefold.img (24G apparent)
Filesystem features: has_journal ext_attr resize_inode dir_index filetype
  needs_recovery extent 64bit flex_bg casefold sparse_super large_file
  huge_file dir_nlink extra_isize metadata_csum
Character encoding:       utf8-12.1
/srv/tur-casefold /dev/loop0 ext4 rw,relatime
--------------e--F---- /srv/tur-casefold/probe
```

`mkfs.ext4 -O casefold -E encoding=utf8` on a sparse image file, mounted on
a loop device. Not strict. The runner's root file system has no casefold
feature, which is what the fallback test uses.

**This is Ubuntu's Wine 9.0 on a GitHub-hosted runner, not the TUR image**,
and a loop device on the runner's disk, not a real disk. There is no GPU:
Xvfb, Openbox, software rendering. Nothing was installed for any
application.

## Research

The findings, with their sources, are in the design note,
[Case-insensitive prefixes](../architecture/runtime-manager.md#case-insensitive-prefixes).
In short:

- ext4 folds case per directory (`+F`) on a file system made with the
  `casefold` feature, since Linux 5.2; the attribute can only be set on an
  empty directory, by its owner, and is inherited by new directories. The
  comparison is Unicode NFD plus full case folding.
- Wine (since 4.11) reads the attribute and, on a case-folded directory,
  lets the kernel find a name in any case; it still reads the whole
  directory for a missing 8.3 name and for a search for one exact name.
- Proton (since proton-4.11-1) and Bottles (since 51.12) set the attribute
  on the `drive_c` they create; Wine itself does not.
- Fedora 45: kernel `CONFIG_UNICODE=y`, e2fsprogs 1.47.4, whose
  `mke2fs.conf` does not turn on `casefold`; bootc adds only `-O verity` for
  ext4; anaconda's Kinoite default is btrfs, which has no case folding.
- A user cannot add the feature: `tune2fs -O casefold` needs root and an
  unmounted file system.

## What was built

| Built | Proved by |
|---|---|
| A `case-fold` plan step: create the directory, run `chattr +F`, record the outcome; only creating the directory can fail it | `exec::tests` with a stand-in `chattr` that succeeds, refuses and is missing; the real `chattr` in the case-folding job |
| `prefix create` and `app install` with `--casefold`: `drive_c` made and marked before Wine starts | `prefix::tests`, `launch_cli`; the job: `lsattr` shows `F` on every case-folded `drive_c` (2 test prefixes, 16 application installations) and on none of the others (3 test prefixes, 16 installations) |
| The fallback: a file system without the feature gives a case-sensitive prefix, exit status 0, the reason and `TUR-WINDOWS-PREFIX-009` | `prefix::tests`; the job, on the runner's own disk: `no-casefold-feature`, "/usr/bin/chattr: Operation not supported while setting flags on …/drive_c" |
| `case_folding` in the application record and its schema | `app_record::tests`; the job compares the record with `lsattr` for every installation |
| Reset and restore keep the attribute | `prefix::tests` (plans). Not carried out for real |
| `--no-casefold` | The job's ordinary passes |

## The measurements

### Real applications, case-folded and not

Four passes on the same case-folding file system, one after the other:
case-folded, not, not, case-folded. Each pass installed, started, used and
removed all eight installers as the second report describes. Times are
wall-clock seconds, one per pass of that mode.

| Application | Installer kind | Mode | drive_c case-insensitive | Install (s) | First window (s) | Check | Uninstall (s) | Left behind |
|---|---|---|---|---|---|---|---|---|
| 7-Zip 26.06 | seven-zip-installer | case-folded | yes / yes | 13.9 / 15.5 | 1.0 / 1.0 | pass / pass | 1.9 / 2.0 | nothing / nothing |
| 7-Zip 26.06 | seven-zip-installer | not | no / no | 15.6 / 14.4 | 1.0 / 1.0 | pass / pass | 1.9 / 1.9 | nothing / nothing |
| 7-Zip 26.06 | msi | case-folded | yes / yes | 14.7 / 14.6 | 1.0 / 1.0 | pass / pass | 2.0 / 2.1 | nothing / nothing |
| 7-Zip 26.06 | msi | not | no / no | 12.0 / 11.0 | 1.0 / 1.0 | pass / pass | 1.9 / 2.0 | nothing / nothing |
| Notepad++ 8.9.9.1 | nsis | case-folded | yes / yes | 13.0 / 15.3 | 4.6 / 2.5 | pass / pass | 2.4 / 2.4 | nothing / nothing |
| Notepad++ 8.9.9.1 | nsis | not | no / no | 16.2 / 12.8 | 2.5 / 4.6 | pass / pass | 2.3 / 2.2 | nothing / nothing |
| Notepad++ 8.9.9.1 (portable) | portable-zip | case-folded | yes / yes | 13.1 / 10.5 | 4.6 / 2.5 | pass / pass | 0.1 / 0.1 | nothing / nothing |
| Notepad++ 8.9.9.1 (portable) | portable-zip | not | no / no | 13.9 / 12.3 | 2.5 / 2.5 | pass / pass | 0.1 / 0.1 | nothing / nothing |
| PuTTY 0.85 | msi | case-folded | yes / yes | 14.9 / 14.5 | 0.8 / 0.8 | pass / pass | 1.9 / 2.0 | nothing / nothing |
| PuTTY 0.85 | msi | not | no / no | 12.9 / 14.2 | 0.8 / 0.8 | pass / pass | 1.9 / 1.9 | nothing / nothing |
| PuTTY 0.85 (single program) | plain-exe | case-folded | yes / yes | 10.3 / 10.6 | 0.8 / 0.8 | pass / pass | 0.1 / 0.1 | nothing / nothing |
| PuTTY 0.85 (single program) | plain-exe | not | no / no | 11.8 / 11.7 | 0.8 / 0.8 | pass / pass | 0.1 / 0.1 | nothing / nothing |
| Inno Setup 7.1.1 | inno-setup | case-folded | yes / yes | 15.7 / 14.7 | 2.0 / 2.0 | pass / pass | 2.8 / 2.9 | nothing / nothing |
| Inno Setup 7.1.1 | inno-setup | not | no / no | 16.2 / 14.9 | 2.0 / 2.0 | pass / pass | 2.8 / 2.9 | nothing / nothing |
| VLC media player 3.0.24 | nsis | case-folded | yes / yes | 22.7 / 23.6 | 1.0 / 1.0 | pass / pass | 3.2 / 3.1 | nothing / nothing |
| VLC media player 3.0.24 | nsis | not | no / no | 19.9 / 22.8 | 1.0 / 1.0 | pass / pass | 2.9 / 3.1 | nothing / nothing |

| Mode | Steps not passed | Install, sum of both passes (s) | Install, median (s) | First window, median (s) | Uninstall, median (s) |
|---|---|---|---|---|---|
| case-folded | 0 of 64 | 237.6 | 14.6 | 1.0 | 2.0 |
| not case-folded | 0 of 64 | 232.6 | 14.1 | 1.0 | 1.9 |

"Install" is the whole `app install`; tur-runtime's own breakdown splits it
into creating the prefix and running the installer. The installer part was
the same in both modes, within 0.3 s, for every application (7-Zip
1.9 s, Notepad++ 2.5 to 2.6 s, Inno Setup 3.8 to 3.9 s, VLC 8.0 to 8.3 s);
creating the prefix varied from 8.8 to 13.2 s in either mode, and that is
where the differences above come from. "Left behind" is what apps.py found
after the removal; nothing in all 32 removals. Nothing appeared outside the
sandbox in any pass. The record agreed with `lsattr` for all 32
installations.

**Reading:** no measurable difference. The 5 s between the sums is less
than the spread between two passes of one mode, and the time that varies is
prefix creation, which does the same work in both.

### The test program

`tests/windows-runtime/casefold.c`, installed with `--portable` into three
prefixes: case-folded and not on the case-folding file system, and one with
`--casefold` on the runner's own disk.

| Prefix | Asked | drive_c has F (`lsattr`) | Recorded reason | Warning |
|---|---|---|---|---|
| case-folded | `--casefold` | yes | `casefolded` | none |
| not | `--no-casefold` | no | `not-requested` | none |
| runner's disk | `--casefold` | no | `no-casefold-feature`: "/usr/bin/chattr: Operation not supported while setting flags on …/local.casefold-plain/prefix/drive_c" | `TUR-WINDOWS-PREFIX-009` |

**What names resolve to.** Each test created the first name and asked for
the second. The runner's-disk prefix gave the same results as "not" in
every row.

| Test | Created, asked for | Case-folded | Not |
|---|---|---|---|
| open in other case | `Readme.Txt`, `README.TXT` | found, same file | found, same file |
| create a second file, CREATE_NEW | `Readme.Txt`, `readme.txt` | refused, error 80 (exists) | refused, error 80 |
| `FindFirstFile` name | `README.TXT` | `Readme.Txt` | `Readme.Txt` |
| `GetLongPathName` | `C:\tur-casefold\README.TXT` | `…\Readme.Txt` | `…\Readme.Txt` |
| `GetFinalPathNameByHandle` | opened as `README.TXT` | **`\\?\C:\tur-casefold\README.TXT`** | `\\?\C:\tur-casefold\Readme.Txt` |
| rename to other case (`MoveFile`) | `Readme.Txt` to `README.TXT` | succeeds; name stays `Readme.Txt` | succeeds; name stays `Readme.Txt` |
| directory, then a file through it | `Sub`, `SUB\inner.txt` | created; directory `Sub` | created; directory `Sub` |
| Latin letter | `Ärger.txt`, `äRGER.TXT` | found | found |
| Greek letters | `ΟΔΟΣ.txt`, `οδοσ.txt` | found | found |
| Cyrillic letters | `Файл.txt`, `фАЙЛ.TXT` | found | found |
| precomposed and decomposed | `café.txt` (U+00E9), `cafe` + U+0301 | **found, same file** | not found |
| sharp s | `straße.txt`, `STRASSE.TXT` | **found, same file** | not found |
| Kelvin sign | `key.txt`, U+212A `ey.txt` | **found, same file** | not found |
| dotless i | `fix.txt`, `fıx.txt` | not found | not found |
| decomposed beside precomposed, CREATE_NEW | `café-new.txt`, decomposed | **refused, error 80** | created: two files |

Where the columns differ, the case-folded drive behaves as the kernel's
Unicode folding says, and the ordinary one as Wine's own comparison does.
For `GetFinalPathNameByHandle` Windows returns the name as it is on disk,
which is the ordinary drive's answer. The others are cases Windows' simple
upper-casing does not join, so the ordinary drive is again the closer one;
that is from how NTFS compares names, not from a test on Windows. A rename
to another case did not change the case in either mode with Wine 9.0.

**An archive that spells one directory two ways.** A portable application
whose zip holds `App/Plugins/one.txt` and `App/plugins/two.txt`, installed
(`unzip`, inside the sandbox) and then asked for its files:

| Asked for | Case-folded | Not |
|---|---|---|
| `Plugins\one.txt` | found | found |
| `plugins\two.txt` | found | found |
| `PLUGINS\one.txt` | found | found |
| `PLUGINS\two.txt` | found | not found (error 2) |
| `Plugins\two.txt` | found | not found (error 2) |
| `plugins\one.txt` | found | not found (error 2) |

On the case-folded drive `unzip` made one directory, `Plugins`, holding
both files; on the ordinary one it made two, and Wine resolved every other
spelling to the first.

**Lookups, timed.** For 100, 1 000 and 5 000 files in one directory, 2 000
operations each (creation: one per file), three rounds alternating which
mode ran first. Microseconds per operation, median of the three rounds, with
the lowest and highest; "ratio" is the ordinary drive's time divided by the
case-folded one's: above 1, case folding was faster. "Long" names are not
8.3 names (`longname-00001.data`), "short" ones are (`f00001.dat`).

| Operation | Files | Case-folded (µs) | Not (µs) | Ratio |
|---|---|---|---|---|
| open, same case, long | 100 | 67.0 (65.8–69.7) | 64.6 (64.1–65.8) | 0.96 |
| open, same case, long | 1 000 | 68.7 (67.1–72.7) | 67.1 (63.1–68.6) | 0.98 |
| open, same case, long | 5 000 | 70.6 (69.7–71.1) | 65.1 (63.1–66.4) | 0.92 |
| open, other case, long | 100 | 75.8 (71.3–77.8) | 135.4 (135.0–137.3) | 1.79 |
| open, other case, long | 1 000 | 72.1 (70.9–77.3) | 459.6 (454.2–462.3) | 6.37 |
| open, other case, long | 5 000 | 75.7 (72.6–78.3) | 1 677.2 (1 661.0–1 681.2) | 22.15 |
| open, other case, short | 100 | 68.7 (68.5–73.7) | 139.9 (139.6–140.8) | 2.04 |
| open, other case, short | 1 000 | 68.8 (68.3–72.8) | 539.7 (535.7–544.2) | 7.84 |
| open, other case, short | 5 000 | 70.7 (69.2–75.3) | 2 035.3 (2 035.3–2 039.3) | 28.78 |
| attributes, other case, long | 100 | 15.7 (15.5–15.9) | 74.5 (74.2–75.3) | 4.75 |
| attributes, other case, long | 1 000 | 16.0 (15.8–16.1) | 391.9 (389.2–394.5) | 24.57 |
| attributes, other case, long | 5 000 | 16.2 (16.1–16.2) | 1 594.5 (1 585.2–1 596.9) | 98.66 |
| open, missing, long | 100 | 132.0 (132.0–132.2) | 70.5 (70.0–70.6) | 0.53 |
| open, missing, long | 1 000 | 763.7 (759.2–764.9) | 524.1 (524.1–534.3) | 0.69 |
| open, missing, long | 5 000 | 3 604.9 (3 542.6–3 607.4) | 2 756.7 (2 744.4–2 759.8) | 0.76 |
| open, missing, short | 100 | 227.5 (226.8–228.2) | 87.0 (86.7–87.6) | 0.38 |
| open, missing, short | 1 000 | 1 889.5 (1 888.3–1 895.2) | 702.5 (702.0–706.5) | 0.37 |
| open, missing, short | 5 000 | 9 374.5 (9 338.0–9 376.0) | 3 610.0 (3 603.2–3 619.0) | 0.39 |
| create, long | 100 | 184.1 (184.1–185.0) | 112.5 (111.3–120.0) | 0.61 |
| create, long | 1 000 | 742.0 (742.0–748.6) | 245.0 (244.0–247.6) | 0.33 |
| create, long | 5 000 | 2 556.4 (2 552.3–2 557.1) | 837.4 (836.4–843.2) | 0.33 |
| create, short | 100 | 310.9 (310.7–317.7) | 155.4 (154.0–155.5) | 0.50 |
| create, short | 1 000 | 1 826.4 (1 820.1–1 827.1) | 630.4 (628.5–631.6) | 0.35 |
| create, short | 5 000 | 8 356.6 (8 342.1–8 388.4) | 2 855.6 (2 845.0–2 857.4) | 0.34 |
| `FindFirstFile`, exact name | 100 | 287.4 (286.2–288.8) | 121.2 (116.3–125.2) | 0.42 |
| `FindFirstFile`, exact name | 1 000 | 1 717.7 (1 714.3–1 726.1) | 120.9 (119.1–132.1) | 0.07 |
| `FindFirstFile`, exact name | 5 000 | 8 133.2 (8 131.4–8 139.6) | 122.1 (117.1–129.8) | 0.02 |
| `FindFirstFile`, other case | 100 | 284.2 (284.0–289.0) | 203.7 (201.6–211.6) | 0.72 |
| `FindFirstFile`, other case | 1 000 | 1 721.9 (1 721.8–1 725.0) | 886.8 (883.8–900.0) | 0.51 |
| `FindFirstFile`, other case | 5 000 | 8 139.4 (8 115.0–8 159.5) | 4 020.2 (4 010.9–4 024.8) | 0.49 |

Every operation found what it should in every round (the "missing" rows
found nothing, as intended). The three rounds of one measurement were within 10 per
cent of one another.

What the numbers say, and what explains them:

1. **A name in another case** is found by the kernel on a case-folded
   directory, in constant time; Wine reads the directory on an ordinary
   one, in time that grows with it. Up to 29 times faster at 5 000 files
   (98 times for `GetFileAttributes`). This is the case case folding is
   for, and Wine's source predicts it (`find_file_in_dir`).
2. **A search for one exact name** (`FindFirstFile` without wildcards) is a
   single `stat` on an ordinary directory and a full read of a case-folded
   one, by Wine's design (`read_directory_data_stat`): 66 times slower at
   5 000 files.
3. **Reading a whole case-folded directory cost about twice as much** as an
   ordinary one (`FindFirstFile` in other case, missing 8.3 names: both
   read the directory in both modes). Not investigated further; ext4 hashes
   every name in its indexed directory read, and on a case-folded directory
   that hash is taken of the folded name.
4. **A missing long name and creating a file were slower** on the
   case-folded directory, and the cost grew with the directory. Wine's
   source says the directory scan is skipped for a missing name that is not
   an 8.3 name; something else costs the time, and **where it goes was not
   established**. The kernel does not cache negative lookups in a
   case-folded directory (`ext4_lookup`), which explains part at most.
   This is an open question, not an explanation.

## Decision

**Off by default**, with `--casefold` to ask for it. The criterion was: on
only if every application still passes and it is not slower. Every
application passed (64 of 64 steps in each mode). For the real applications
it was neither faster nor slower by any amount these runs can resolve; for
the operations measured one by one it was much faster at one thing and
slower at four, and it changes what five kinds of name resolve to, one of
them (`GetFinalPathNameByHandle`) away from what Windows does. Issue #81
asked for it to be switched on "only if faster". It is not, for anything
measured but the one case. The reasoning is recorded in the design note,
[The default](../architecture/runtime-manager.md#the-default).

It helps one kind of application clearly: one whose files spell a
directory two ways (the archive test). That is what `--casefold` is for.

## What the installer side must guarantee

For `--casefold` to do anything on an installed TUR system:

1. The file system holding `/var/home` is ext4 made with `-O casefold`
   (UTF-8; strict mode not needed, not tested). Neither Fedora's
   `mke2fs.conf` nor bootc's ext4 defaults turn it on, and btrfs has no case
   folding at all.
2. `CONFIG_UNICODE=y` in the kernel (Fedora 45's has it).
3. It is decided at `mkfs` time: a user cannot add it later (`tune2fs`
   needs root and an unmounted file system).
4. With fscrypt home directories: a v2 policy, and the key loaded. Not
   tested.

Nothing else on the system gets the attribute; `tur-runtime` asks for it on
one directory per prefix.

## What was not tested

| Not tested | Why |
|---|---|
| The TUR image, Fedora's kernel 7.2/7.3, Fedora's Wine | Wine is not in the image |
| A real disk | The case-folding file system is a loop device on an image file on the runner's disk. Absolute times would differ; both modes shared it |
| Strict encoding, names that are not UTF-8 | Not made; Wine writes UTF-8 |
| fscrypt with case folding | Not available on the runner as set up |
| Proton prefixes | Proton sets the attribute itself; Proton never ran here |
| Reset and restore of a case-folded prefix, for real | Plans only (unit tests) |
| Games, large installers, applications with tens of thousands of files | Not in the pinned set |
| Why missing names and new files were slower on a case-folded directory | See point 4 above |

## Limits

- **Off by default.** Nothing changes for a prefix unless `--casefold` is
  given.
- **Only the application record keeps the outcome.** The prefix metadata's
  schema was not changed, so a prefix from `prefix create --casefold` keeps
  it only as its directory's attribute, and `prefix verify` does not check
  it.
- **`chattr` and `lsattr` are started by `tur-runtime`**, outside the
  sandbox, on its own directory, and their messages are classified from the
  C locale's text. A different `chattr` wording would be recorded as
  `failed`, with the message, not misread.
- **A restore with no prefix to replace** gives a case-sensitive prefix:
  the archive does not hold the attribute.
- **A restore of an archive from an ordinary prefix into a case-folded
  one** would merge names that differ only in case. Wine does not create
  such names; a Linux tool could.
- **Still Ubuntu's Wine 9.0 on a hosted runner**, no GPU, as in the earlier
  reports.

## Dependencies

No crate was added; `Cargo.lock` is unchanged. Programs started at run
time, none linked or shipped by this change:

| Program | Licence | Used for | On the TUR image |
|---|---|---|---|
| `chattr`, `lsattr` (e2fsprogs) | GPL-2.0 | Setting and reading the case-folding attribute, outside the sandbox | Yes, e2fsprogs 1.47.4 in Fedora 45 |

CI only: `mkfs.ext4` and `dumpe2fs` (e2fsprogs) to make and describe the
file system, a loop mount with `sudo`, and mingw-w64 for the test program.

## Changes needed in shared files

None was made. Worth making by their owners:

- `installer/`: make the root file system's ext4 with `-O casefold`, if TUR
  wants `--casefold` to work on an installed system (the other agent's
  work).
- `schemas/prefix-metadata.schema.json`: a `case_folding` member, so that a
  prefix made without an installation keeps the outcome and `prefix verify`
  can check it.
- `docs/dependencies/upstream-projects.md`: `chattr` and `lsattr` are now
  started by `tur-runtime`.
- The catalogue: a per-application wish for a case-insensitive drive C:
  (`--casefold`), for applications such as the archive test describes.

## Issue #81

- "Verify: does the TUR root filesystem as installed have the casefold
  feature": not from this side. From the sources: not by default (bootc
  adds only `-O verity` for ext4; Fedora's `mke2fs.conf` has no `casefold`);
  the installer has to ask for it. The mkfs option is `-O casefold` (with
  `-E encoding=utf8`, the default); it affects only directories given `+F`.
- "Verify how Wine behaves on a casefolded drive_c": done, from Wine's
  source and measured (tables above).
- "Measure with the real-application jobs … switch it on per prefix only if
  faster and every application still passes": done. Every application
  passed; it was not faster; it stays off by default and is available per
  prefix with `--casefold`.
