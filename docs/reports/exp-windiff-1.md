<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-windiff-1 — a machine that closes Windows-compatibility gaps by itself (experiment)

> **Status: first slice built, NOT YET MEASURED.** Pull request 124, branch
> `exp/windiff`. The harness, the check programs, the differ, the Wine build
> and the harvest job are written. The first workflow run (43537479583,
> commit `81fe016`, pushed 14 November 2026 00:56 UTC) had not been given a
> runner half an hour later: 61 runs were queued in the repository, the
> oldest for 150 minutes. The commit that adds this report starts the
> workflow again (it also changes the workflow), so the run to read is the
> one on the pull request's head. Every number that has to come from CI is
> marked **NOT RUN**. Sections 1, 2, 8, 9, 10 and 12 do not
> depend on the measurements.

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-windiff-1 (time-boxed research spike `windiff`)
Subsystem:        Windows compatibility (Wine); no TUR service is changed
Build ID:         not applicable: this task builds no image
Source revision:  exp/windiff (pull request 124); the commit a number comes
                  from is named next to it

SUMMARY
- What changed: an experiment, not a feature. Check programs in C, built
  once with mingw-w64, print what the documented Windows API did; a runner
  runs the same binaries on GitHub's Windows machine, under Fedora 45's
  Wine and under Wine master built in CI; a differ classes every check
  (same, different, crash, missing export, timeout); a fix queue of Wine
  patches is applied to master and measured again; a harvest job records
  what TUR's real applications hit in Wine.
- Why: to find out whether closing Wine's gaps can be a continuous,
  automated loop rather than something a person does by hand
  (windows-drivers-2 did it by hand for one area).

FILES
- Added: tests/experiments/windiff/** (checks/*.c, wd.h, build.sh; run.py,
  diff.py, harvest.py, winetest_report.py; ci/*.sh; triage.tsv;
  wine-patches/ under LGPL-2.1-or-later), .github/workflows/exp-windiff.yml,
  docs/reports/exp-windiff-1.md
- Modified: none
- Removed: none

BUILD
- Command: tests/experiments/windiff/checks/build.sh (mingw-w64) and
  ci/wine-build.sh (Wine master), in Fedora 45 containers in CI only;
  nothing was built on a development machine
- Result: NOT RUN (the run is queued)
- Warnings: NOT RUN

TESTS
- Unit tests: diff.py on hand-made results and harvest.py's parser of
  Wine's .spec files against a checkout of Wine master, on the development
  machine (Python only, nothing compiled)
- Integration tests: workflow exp-windiff: NOT RUN (queued)
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (not applicable)
- Performance tests: NOT RUN

DIAGNOSTICS
- Log paths: artifacts of workflow exp-windiff: windiff-bin (build.log),
  windiff-windows, windiff-wine-fedora, windiff-wine-master (results-*.json,
  per-group standard error, Wine's fixme lines), windiff-harvest,
  windiff-report (summary.md, differences.md, diff.json)
- Diagnostic bundle path: not applicable
- Relevant error IDs: none

RECOVERY
- Rollback procedure: delete tests/experiments/windiff, the workflow and
  this report; nothing else refers to them
- Recovery tested: not applicable

KNOWN LIMITATIONS
- See section 11.

NEXT SAFE ACTION
- Let the workflow run on the pull request's head finish (or start it
  again with workflow_dispatch), read windiff-report, fill sections 4 to
  7, then the fix loop.
- The owner's decision in section 8.3: Wine does not take code a language
  model wrote, which changes what "every fix goes upstream" can mean.
```

## The answer

**Not known yet.** Nothing has been measured: the first run was queued behind
other work. What can be said now:

- The engine exists and is small: about 4,200 lines of C in seven programs and a header,
  800 lines of Python and 420 lines of shell. It is designed to report about
  4,000 observations per machine in 39 groups (the exact number is whatever
  the first run reports).
- **Wine's maintainers do not accept code written by a language model**
  (section 8.2: the maintainer's own words on the mailing list in April
  2026, repeated by reviewers in May 2026). The loop this spike was asked to
  test ends with "queue for upstream". For fixes written by P@x or any other
  model that step is closed today. What stays open is narrower and still
  useful: the *finding* (a black-box observation of Windows, a minimal test
  program, a bug report) can go upstream, and a person can write the fix.
  Section 8.3 puts the decision to the owner.
- The harvest idea in the task (read `fixme:` lines from the Windows
  runtime's CI runs) does not work as stated: `tur-runtime` runs every
  application with `WINEDEBUG=fixme-all`, so the applications' logs in runs
  43435958928 and 43458998606 hold no `fixme:` line from an application at
  all (all 218 are from the driver experiment and from prefix creation).
  The spike therefore runs the five applications itself with Wine's default
  diagnostics and reads their import tables (section 3).

## 1. Hypothesis and kill criterion

Stated before anything was measured (commit `81fe016`, before the first
run).

**Hypothesis.** A differential engine works: (1) generate many small test
programs for an API area, (2) run the same .exe on real Windows and on Wine,
(3) diff the observable behaviour, (4) write a fix to Wine, (5) verify on
both, (6) queue for upstream. It finds real, previously unrecorded
differences and fixes a useful share of them at low cost per fix.

**Kill criterion.** If nearly everything the engine finds is already
recorded in Wine's own test suite as a known difference (`todo_wine`) or is
irrelevant to real applications, generic generation is not worth it; then
only the application-driven version remains (differences a real application
in TUR's catalogue actually hits).

**What counts.** A *check* is one observation with a stable name. A
*difference* is a check whose value under Wine is not the value on Windows.
Several differences often have one *cause*; findings are counted in causes,
and the tables give both. A cause is *known* when Wine's tests (`todo_wine`
or a `broken()` that describes it) or bug tracker record it, *irrelevant*
when it is real but nothing an application could depend on, an *artifact*
when the test set-up produced it, *new* otherwise. "Nearly everything" is
taken to mean: fewer than one new, relevant cause in ten.

## 2. The harness

| Piece | What it does |
|---|---|
| `checks/wd.h` | The framework each program includes: `prog --list`, `prog --run GROUP`, one JSON line per check (`{"id":"prog.group.check","v":"..."}`), a last line when the group finished. Values are what the API returned (error codes, counts, attribute bits, names), never handles, addresses, times of day or process identifiers |
| `checks/fsbasic.c` | Sharing modes (a 24 by 24 matrix), deletion (delete-on-close, open files, read-only, through a handle), creation dispositions against six kinds of existing name, attributes, times, hard links and file identity, renaming, copying, `ReplaceFile`, reading, writing, seeking, locks, directory enumeration and wildcards |
| `checks/fsname.c` | 87 special names (reserved devices, trailing dots and spaces, forbidden characters), case (28 pairs of names that differ by a case mapping), long paths, alternate data streams, 100 paths through `GetFullPathNameW`, three `ntdll` path functions and eleven `PathCch` functions, temporary names, volumes, the current directory |
| `checks/proc.c`, `checks/wdchild.c` | 66 command lines as the C runtime and `CommandLineToArgvW` split them, 70 ways of naming the program to `CreateProcessW`, the environment (API, expansion, the C runtime's copy, ten environment blocks given to a child), 38 ways for a process to end and the exit code its parent sees, what a child is started with (flags, standard handles, `STARTUPINFO`) |
| `checks/con.c` | In a pseudoconsole of 80 by 25 cells: modes, code pages, the screen buffer (writing, wrapping, scrolling, attributes, sizes, a second buffer, the title), virtual-terminal sequences, input (cooked, raw, records), console handles as files, control events, leaving the console |
| `checks/fuzz.c` | Section 7: generated sequences |
| `checks/wdlaunch.c` | Starts each program with a console without a window and a job that ends everything when the runner stops it. Without it the starting point differs: a program started from a script has a console on Windows and none under Wine |
| `run.py` | Copies the programs to `C:\wdbin`, gives each group an empty `C:\wd\<prog>-<group>` (the same string on every machine, so path lengths are equal), runs every group twice, keeps standard error, marks a check whose value changed between the two runs as unstable |
| `diff.py` | Classes every check per target: same, different, missing-export, crash, timeout, not-emitted, unstable; counts differences and causes with `triage.tsv`; says which differences a patched Wine no longer has and which it gained |
| `ci/wine-build.sh`, `ci/wine-master.sh` (jobs `wine-tree`, `wine-master`) | Build Wine master (64-bit, no display, no optional libraries) in a Fedora 45 container; the build tree and a compiler cache are kept between runs, so a patch rebuilds only what it touches. Run the checks, apply the queue, rebuild, run again, build and run the Wine conformance tests the queue adds to |
| `winetest_report.py` | Reads the output of Wine's test programs (run under the patched Wine and on Windows) and says whether a failure is in a line the queue added |

Machines:

| Machine | What | Version |
|---|---|---|
| Windows | GitHub's `windows-latest` hosted runner | NOT RUN (the job records product, build, file system of C:, 8.3 names, long-path setting and code pages) |
| Fedora 45 Wine | `dnf install wine` in `fedora:45@sha256:fa773d10…` | wine-11.20 per the earlier experiment; this run: NOT RUN |
| Wine master | built in CI from `9f8c4ae4d35c7ff4153267aba9618c026f5f5583` | `git ls-remote` on 14 November 2026: `master` and the tag `wine-11.22` are this commit |

Choices that matter for reading the results:

- **One oracle.** Windows is whatever `windows-latest` is on the day. Some
  behaviour differs between Windows versions (reserved names changed in
  Windows 11); a difference from this one Windows is not always a
  difference from all of them. Wine's own tests mark those with
  `broken()`; the triage treats a behaviour Wine's tests accept for some
  Windows version as known.
- **Wine runs as an ordinary user, not root**, because root may write to
  read-only files.
- **64-bit programs only.**
- **The jobs are green when they produced evidence.** A difference is a
  finding.

## 3. Choosing areas by evidence

### 3.1 The runtime's own CI logs

Run 43435958928 (windows-runtime, `main`, 13 November 2026: 7-Zip,
Notepad++, PuTTY, VLC, Inno Setup) and run 43458998606 (windows-drivers),
all artifacts, searched for `fixme:` and `err:` lines:

| Artifact | Files | `fixme:` lines | From |
|---|---|---|---|
| windows-runtime-applications | 118 | 0 | the applications run with `WINEDEBUG=fixme-all` (`services/tur-runtime/src/runtime_cmd.rs`) |
| windows-runtime-autopilot | 259 | 0 | `WINEDEBUG=-all` |
| windows-runtime-casefold | 471 | 0 | as above |
| windows-runtime-evidence | 23 | 0 | as above |
| windows-drivers-evidence | 154 | 78 | prefix creation (`winediag:loader_init` 46, `actctx:parse_depend_manifests` 16) and the driver host |
| kmdf-run-evidence | 56 | 140 | the driver host: `MmGetSystemRoutineAddress` 24, `wineusb:pdo_pnp` 16, the kernel stubs windows-drivers-2 lists |

So the runtime's logs cannot say where applications touch Wine's gaps. That
is a finding for the runtime too: the compatibility autopilot classifies
failures from Wine's messages, and with `fixme-all` it never sees the
`fixme:` line that often precedes a failure. Section 9 proposes keeping
them in a file.

### 3.2 The spike's own harvest

The `harvest` job (`ci/harvest.sh`) fetches the five applications at the
hashes pinned in `tests/windows-runtime/apps.json`, installs or unpacks
them in Fedora 45's Wine on a virtual display, uses each briefly with
Wine's default diagnostics, and counts (`harvest.py`):

- `fixme:` and `err:` sites and unimplemented-function aborts, by channel
  and function, with the applications that hit them;
- every function the applications' programs and libraries import
  (`objdump -p`), set against Wine master's `.spec` files: implemented,
  stub, absent, or in a DLL or API set Wine does not have.

Result: **NOT RUN** (queued).

The driver experiment's stubs (windows-drivers-2, section 5) are kernel
functions. They cannot be put through this engine: a hosted Windows runner
does not load an unsigned test driver, so there is no oracle for them here.
NOT RUN, and not runnable on hosted runners.

### 3.3 The two areas named in the task

File-system semantics and process and console behaviour are in the first
slice as asked (section 2). They were written before the harvest ran; the
harvest decides the next area.

## 4. What was measured

**NOT RUN.** The tables below are what `diff.py` writes; they are filled
from the `windiff-report` artifact.

| Target | same | different | missing-export | crash | timeout | not-emitted | unstable | compared | differing |
|---|---|---|---|---|---|---|---|---|---|
| Fedora 45 Wine 11.20 | NOT RUN | | | | | | | | |
| Wine master 9f8c4ae4 | NOT RUN | | | | | | | | |

## 5. Found, already known, new

**NOT RUN.** Method, fixed in advance: for every cause, search Wine
master's `dlls/*/tests` for the function and the behaviour (a `todo_wine`,
a `broken()`, or a test that asserts what Wine does), and Wine's bug
tracker by function name where one search is enough; the evidence (file and
line, or bug number) goes in `triage.tsv` beside the status. For scale:
Wine master's tests hold 8,666 `todo_wine` lines; `kernel32/tests/file.c`
has 60, `ntdll/tests/file.c` 94, `kernel32/tests/console.c` 20,
`kernel32/tests/process.c` 5, `kernel32/tests/path.c` 7,
`ntdll/tests/path.c` none.

Three differences are predicted from reading Wine's source, and none of the
three is in Wine's tests. They are predictions, not findings, until the
Windows run says what Windows does:

| Check | Wine master, from its source | In Wine's tests |
|---|---|---|
| `fsbasic.rename.MoveFile-null-target` | `MoveFileWithProgressW` with no target and without `MOVEFILE_DELAY_UNTIL_REBOOT` calls `DeleteFileW(source)` (`dlls/kernelbase/file.c`): the file is deleted | no test passes a null target |
| `con.modes.input.set-*`, `output.set-*` | `conhost` stores any mode value (`IOCTL_CONDRV_SET_MODE` in `programs/conhost/conhost.c`), no combination is refused | invalid modes are not tested |
| `fsname.volume.GetTempFileName.unique-0x10000` | `GetTempFileNameW` masks the number to 16 bits first, so 0x10000 is treated as 0: a file is created | not tested with more than 16 bits |

## 6. The fix loop

**NOT RUN.** The queue (`tests/experiments/windiff/wine-patches/series`) is
empty. The loop is built: `ci/wine-master.sh` applies each patch to master
(and reports whether it also applies to wine-11.20), rebuilds, runs every
check again, and `diff.py` lists the checks that now match Windows and any
that stopped matching. A patch that adds to Wine's conformance tests has
its test program run under the patched Wine and, by the `winetests-windows`
job, on real Windows, which is how Wine itself validates a test.

Effort per fix: NOT RUN.

## 7. The wild variant: generated sequences

`checks/fuzz.c` is a property-based test with Windows as the oracle. From a
seed, a pseudo-random walk of 400 steps over 20 kinds of file operation
(open with random access, sharing, disposition and flags; close; read;
write; delete; rename with and without replace; set and get attributes;
make and remove directories; truncate; lock and unlock; copy; size;
disposition through a handle; hard link) on seven names and six handle
slots. The walk is the same for the same seed on every machine; each step
records its error code; twenty steps are one check. The first chunk in
which Wine's record differs from Windows's names the step at which
behaviour diverged; later chunks of that sequence are not counted, because
the state is different from there on. Twelve seeds, 4,800 steps.

What it is for: states no hand-written check thought of, such as a file
that is delete-pending, locked, read-only and hard-linked at once.

Result: **NOT RUN.** The question to answer from the first run: does a
sequence diverge at a step whose cause no hand-written check shows?

Coverage-guided generation (instrumenting Wine and steering the walk toward
unexecuted code) was not attempted: NOT RUN.

## 8. Clean-room discipline, and what Wine says about contributions

### 8.1 What this spike did

Windows was observed only as a black box: programs written here call
documented functions and print what came back; the Windows job uploads
that text and nothing else. No Microsoft source code, leaked code,
disassembly or decompilation of a Microsoft binary was read, and no
Microsoft binary was downloaded, copied off the runner or committed. The
harvest reads the import tables of the five third-party applications, not
of Windows. The checks use `GetProcAddress` for functions that newer
headers declare, never a table read from a Microsoft DLL.

One thing to weigh (section 12, question 3): the author of the checks and
of any patch in the queue is a language model. What it was trained on is
not known to this project. Wine's maintainers name exactly that as a
reason (below).

### 8.2 What Wine's project says

Fetched on 14 November 2026. Wine's wiki (`gitlab.winehq.org/wine/wine/-/wikis`,
where `wiki.winehq.org` redirects) and `testbot.winehq.org` answered
automated requests with a bot-protection page, and this spike did not go
around it; **the wiki's current wording has to be read by a person in a
browser.** What follows is marked as read directly or as quoted by others.

| Question | What is said | Source | Read |
|---|---|---|---|
| Sign-off | "signoffs are no longer required on merge requests. This is replaced by the requirement to explicitly approve your own merge request if it contains commits authored by someone else" | Alexandre Julliard, wine-devel, "Merge requests documentation", 11 July 2022, list.winehq.org thread `PQ7VNGOKOFB4KQTVGVOSLJOD3VK6MHRD` | directly |
| Real name | "the real name requirement on commits now extends to Gitlab as well. Please make sure to use your real name in your Gitlab profile so that it matches your commits." | same message | directly |
| How patches are sent | "Mailing patches to wine-devel is now officially discouraged" (merge requests on gitlab.winehq.org) | same message | directly |
| Code from a language model | "Clean room is not even the main issue. In order to ship any code, we need a proper authorship record, and an LGPL-compatible license. The legal copyright/licensing status of LLM output is currently not well defined, so we can't accept such code." | Alexandre Julliard, wine-devel, 9 April 2026, message `PKPXWOWINTTYWEE5CKLDXGNDBDFCAET4` | directly |
| The same, in review | "Was AI used to generate any of this code? ... our clean room guidlines ... prohibit LLM generated code." (spelling as in the original) | Connor McAdams, merge request 10813, 4 May 2026, wine-gitlab message `TOHK6X7DFYNLD6BBD5WI3Z2OSRDSZMLN` | directly |
| The same, a contributor withdrawing | "I realized that LLM-generated code is not allowed and this MR contains some LLM-generated code" | Daniel Lu, merge request 10985, 27 May 2026, wine-gitlab message `53OD52NESPF5IXVCCOUOOTHHC4R3GJ3Z` | directly |
| Clean room, wiki | "Don't use an LLM tool to generate code. There's no guarantee that the training material of that LLM respects our Clean Room Guidelines, or that its output is compatible with the LGPL." | Clean Room Guidelines, as quoted in github.com/chaoss/wg-ai-alignment issue 108 (16 September 2026) | quoted by others; **not verified at the wiki** |
| Who may not contribute, wiki | "Some people cannot contribute to Wine because of potential copyright violation. This would be anyone who has seen Microsoft Windows source code (stolen, under an NDA, disassembled, or otherwise)." | Developer FAQ, from a search index of a mirror | **not verified at the wiki** |
| Clean room, wiki | "Don't look at any Microsoft source code, even if it's made "public" under some license" | Clean Room Guidelines, same kind of source | **not verified at the wiki** |
| Tests on real Windows | Merge requests run `test-win10-21h2-32` and `test-win10-21h2-64` on runners tagged `win10-21h2`, executing `winetest.exe` | `tools/gitlab/test.yml` in Wine's source tree | directly |

Not found: any statement about help from a model that stops short of
writing code (finding a difference, writing a bug report, reviewing).
Related projects, as context only: Mesa allows it with mandatory disclosure
(`docs.mesa3d.org/submittingpatches.html`: "Disclosure is always required
when "AI" was involved in the creative process"); Proton's maintainer
points to Wine's rule for anything that can go upstream (ValveSoftware/Proton
issue 8072, 3 October 2026). ReactOS, DXVK, Wine Staging: no stated policy
found in one search each.

Note on the earlier report: windows-drivers-2, section 10, drafts a
proposal to wine-devel for sixteen patches. Those patches were also
written by a model. The same rule applies to them; that draft should not
be sent as it stands.

### 8.3 What that means for the loop

| Step | Can it go upstream? |
|---|---|
| A difference observed on Windows by a black-box program | Yes: it is a fact about Windows. A bug report in Wine's tracker with the program's output is the ordinary way |
| The small test program that shows it | Open. It is code a model wrote. As an attachment to a bug report it is evidence, not a contribution to Wine's tree; whether Wine's people want even that is their call, and it should be said plainly in the report that a model wrote it |
| A test in Wine's test suite, a fix in Wine | No, as Wine's maintainers put it today, when a model wrote them |
| The same fix written by a person from the bug report | Yes, under that person's real name, if they have not read the model's patch. Someone who has read it is in the position Wine's clean-room rule describes for other people's code: that is for Wine to judge, not for TUR |

So the honest shape of the loop today is: the machine finds, minimises and
reports; people fix upstream; TUR may carry its own queue in the meantime,
for its own image, under the LGPL, saying what it is. Carrying a queue
upstream will not take is a cost that grows with every Wine release
(AGENTS.md: "Integrate upstream projects rather than forking them. Patches
live in a small queue, each with a stated reason").

## 9. As a standing TUR service

A design, not built. Names are proposals.

```text
   inputs                          engine (CI, nightly)                     outputs
   ------                          --------------------                     -------
   check corpus (this spike)  -->  build once (mingw-w64)                   dashboard: "differences from
   generated sequences        -->  run: Windows runner (oracle)               Windows: N open, M fixed this
   autopilot results          -->       Wine TUR ships                        week", per area, per cause
     (failure class, function)          Wine master                         bug reports upstream (a person
   consented problem reports  -->       TUR's queue on top                    sends them)
     (component "windows")         diff, triage, bisect                     TUR's queue, each patch with its
   harvest of catalogue apps  -->  regressions: a check that matched         check; dropped when upstream
     (fixme sites, imports)          and no longer does                       fixes the cause
                                                                            image update: the Wine TUR ships
                                                                              plus the queue
```

**Nightly.** One workflow on a schedule: the corpus against the pinned
Windows image, the Wine in TUR's image, Wine master at that night's commit
(recorded), and TUR's queue on master. About 25 runner-minutes when Wine's
tree is cached (NOT RUN: an estimate until the first run gives times).
Results are appended to a small history (one JSON line per check per night
is too much; one line per *change* of a check's class is enough).

**Dashboard.** A static page made by the workflow from the history:
differences open, by area and by cause; closed this week and by what
(upstream commit, or TUR's queue); new this week (a regression in Wine
master is news upstream wants the same day); the oldest open causes. Each
cause links to its check's source, the Windows value, the Wine value, and
the bug report if one was filed. Numbers are counts of causes, never a
percentage of "compatibility" (AGENTS.md: never claim compatibility that
was not measured).

**From one person's failure to a test.**

1. An application fails under Wine on someone's computer. The autopilot
   (`docs/architecture/compat-autopilot.md`) already classes the failure
   and writes `result.json`. Two additions make it an input here: keep
   Wine's `fixme:` and `err:` lines of the failing run in the result (today
   `fixme-all` hides them), and for the class `unimplemented-function`
   (`TUR-WINDOWS-COMPAT-007`) the function's name is already there.
2. With consent, the person sends a problem report
   (`docs/architecture/problem-reports.md`): component `windows`, the
   autopilot's failure class, the functions named, the application's
   identity (name, version, installer hash), nothing from the person's
   files. The receiver's signature already groups duplicates by error code,
   component and version; adding the function name to the signature for
   this component would group by cause.
3. The engine turns a signature into a *target*: the functions and the
   failure class say which area to generate checks for. A maintainer (or
   P@x, proposing) writes or generates the checks; they join the corpus
   only when they are deterministic on Windows (two runs agree) and fail on
   the Wine the report came from.
4. The nightly run shows the difference. A fix lands in TUR's queue or
   upstream; the check turns to "same"; the report's issue is marked
   `fixed` with the image version (`mark-fixed`), and the person's
   `tur-diag report status` says so.
5. Everyone gets it with the next image update. The autopilot's catalogue
   record for that application stops needing its workaround when the check
   passes without it.

**What it does not do.** It never sends anything from a user's machine by
itself (ADR-0024). It never runs a user's application on the Windows
runner: only check programs written for the corpus, so no third-party or
Microsoft binary is uploaded anywhere. It does not rate applications.

**Regressions upstream.** The same corpus on Wine master every night is a
regression detector for Wine: a check that matched Windows last night and
does not tonight, bisected over that day's commits (Wine merges about 30 a
day; five builds with the cached tree). That is a contribution Wine's rules
do not stand in the way of: a bug report naming the commit.

## 10. What productising would take

| Item | Work | Depends on |
|---|---|---|
| A place for the corpus and engine outside `tests/experiments` | move to `tests/windiff` (or its own repository), a schema for results in `schemas/`, the workflow on a schedule and added to `gate.yml`'s list | owner's go |
| History and dashboard | a JSON-lines history kept as a release asset or a branch, a static page | the above |
| A pinned oracle | `windows-latest` moves; pin `windows-2025` and record its image version every night, re-baseline deliberately when it changes; a second oracle (Windows 11 client) needs a self-hosted machine and a licence | legal review of ADR-0015's amendment for this use |
| Triage that scales | `triage.tsv` by hand works for hundreds of causes; beyond that, an index of Wine's tests (function, `todo_wine` lines) built nightly, and the tracker's search, to pre-fill "known" | none |
| The queue in the image | TUR's image would have to build Wine with the queue instead of installing Fedora's package: its own package, rebuilt for every Fedora Wine update. This is the largest cost and is an ADR ("never track an unpinned upstream branch", "patches live in a small queue") | owner's decision in 8.3 |
| Autopilot and problem-report inputs | keep `fixme:`/`err:` lines in `result.json`; a report field for the functions named; signature by function for component `windows` | their owners |
| More areas | each area is a C file of checks; registry, time zones and locale, `msvcrt`, sockets, GDI text metrics, COM marshalling are candidates the harvest should rank | harvest results |
| 32-bit programs, a display | the checks build for i686 unchanged; windows and messages need Wine built with a display driver and a virtual display | none |

## 11. Limits

- Nothing was measured yet.
- One Windows version is the oracle (section 2).
- Console checks run in a pseudoconsole; a console in a window is not
  covered.
- Wine master is built without a display driver, fonts or optional
  libraries; nothing here exercises windows, graphics, sound or the
  network.
- The checks print what the API returned. Timing, performance and anything
  that needs two machines are out of reach.
- Table-driven checks are many observations of few functions: the number
  of checks says how much was observed, not how much of Windows was
  covered.
- The workflow is not in `gate.yml`'s list (spikes may not edit it), so the
  `gate` status of a commit that starts it stays pending until another
  listed workflow finishes; that is the documented behaviour for an
  unlisted workflow.

## 12. Licence and legal questions for the owner

1. **Upstream.** Wine does not accept code a language model wrote. Does TUR
   (a) send findings only (bug reports with the black-box observation, said
   to come from an automated differential test), (b) also ask Wine's
   maintainers whether they want the test programs attached, or (c) carry
   its own queue and not go upstream? This report recommends (a) now, (b)
   as a question on wine-devel signed by the owner, and (c) only for fixes
   an application in the catalogue needs.
2. **The sixteen driver patches** (windows-drivers-2, section 10) have the
   same author. The draft proposal to wine-devel there should be withdrawn
   or rewritten as a report of findings.
3. **Clean room.** The model's training material is unknown to TUR. If the
   owner wants TUR's own queue to be defensible as clean, the rule would be:
   a patch is written only from the check's output and Wine's source, and
   its commit message says so. That is what this spike did; it cannot prove
   more.
4. **The Windows runner.** ADR-0015's amendment allows GitHub's hosted
   Windows machines "to build and test the guest-side helper". The task
   text extends that to this spike. For a standing nightly service the
   amendment should say so in words, and the lawyer's review of Microsoft's
   terms (named in ADR-0015) should cover "running our own test programs on
   hosted Windows to learn how the documented API behaves, and publishing
   the results".
5. **Publishing results.** The dashboard would publish, per check, what
   Windows returned. These are facts observed through the public API, the
   same kind Wine's own test suite records; a lawyer should confirm before
   it is public.
6. **Licences.** The engine is TUR's code (noncommercial licence); the
   queue is LGPL-2.1-or-later and kept in a directory marked so. If the
   test corpus is ever to be given to Wine or others, TUR's licence does
   not allow that use by commercial parties; the corpus would need a
   second, permissive licence, which is the owner's to grant.

## 13. Proposed decision text

For the lead to number and place; not an ADR.

> **Differential testing against Windows.** TUR keeps a corpus of small
> test programs that record what the documented Windows API does, and runs
> it every night on a hosted Windows machine and on the Wine it ships and
> on Wine's development branch. The differences are counted by cause and
> shown; nothing is called compatible that was not measured. Windows is
> observed only through its public interface: no Microsoft source,
> disassembly or binary is read, copied or kept. Findings go to Wine as bug
> reports sent by a person. TUR does not submit model-written code to
> Wine, in keeping with Wine's stated rule. TUR may carry a small queue of
> its own fixes for applications in its catalogue, each tied to the check
> that shows it and dropped when upstream closes the cause.

## 14. Follow-ups

1. Let the first run finish; fill sections 4 to 7; triage.
2. The fix loop on the first new, relevant causes; effort per fix.
3. The owner's answers to section 12, question 1 first.
4. `tur-runtime`: keep Wine's `fixme:` and `err:` lines of a failing run
   (section 3.1).
5. Rank the next areas from the harvest.
