<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-wincli-1 — TUR's terminal behaves like a Windows PC's (experiment)

> **Status: built and registered; NOT YET MEASURED.** The hypothesis, the
> kill criterion, the corpus, the judge, five candidates, the prototype and
> the safety tests are on pull request 125 (commit `602849c` and later).
> No CI run has produced a result yet: the first (43537757872) failed on
> Windows at checkout and never got a Linux runner, the second
> (43538182154) never started (section 12); the repository's queue held 62
> runs. **Every number in this report is NOT RUN until a run is named
> next to it.** Sections 1,
> 2, 6, 8, 9 and 10 (method, design, proposed rules, licence facts) do not
> depend on the numbers; sections 3, 4, 5 and 7 are placeholders.

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-wincli-1 (time-boxed research spike)
Subsystem:        Windows compatibility (command line); no TUR service
Build ID:         not applicable: this task builds no image
Source revision:  exp/wincli (pull request 125)

SUMMARY
- What changed: an experiment, not a feature. A corpus of 416 everyday
  Windows command lines (plus 54 held out), a harness that runs it on real
  Windows (GitHub's Windows runners) and reduces every result to a shape,
  five Linux candidates scored against that shape, a prototype router
  (tur-winshell) with TUR-aware commands and a PowerShell compatibility
  module, integration tests and proposed safety rules with tests.
- Why: the owner wants TUR to act like a Windows PC at the command line
  too. Nobody had measured how close a Linux system gets, with real
  Windows as the judge.

FILES
- Added: tests/experiments/wincli/** (corpus, harness, scoring, the
  prototype, three small Windows programs in C), .github/workflows/
  exp-wincli.yml, docs/reports/exp-wincli-1.md
- Modified: none
- Removed: none

BUILD
- Command: none on a development machine. In CI: mingw-w64 builds
  turrun.exe, turshim.exe and hello.exe in a Fedora 45 container.
- Result: NOT RUN (no runner yet)

TESTS
- Unit tests: the pure-Python parts (shape reduction, scoring, path
  translation, the safety verdicts for all 43 cases) were exercised on the
  development machine with the Python interpreter only; nothing was
  compiled, and no Wine, PowerShell or container was run there.
- Integration tests: NOT RUN (workflow exp-wincli; no run has got past
  checkout yet, see section 12)
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (not applicable)
- Performance tests: NOT RUN (seconds per command are recorded by the run)

DIAGNOSTICS
- Log paths: CI artifacts wincli-truth-windows-2025, wincli-truth-
  windows-2022, wincli-candidates, wincli-score
- Diagnostic bundle path: not applicable
- Relevant error IDs: none

RECOVERY
- Rollback procedure: close the pull request; nothing outside
  tests/experiments/wincli/ and one workflow file is touched
- Recovery tested: not applicable

KNOWN LIMITATIONS
- See section 11.

NEXT SAFE ACTION
- Let the workflow run; fill sections 3, 4, 5 and 7 from its artifacts;
  then the owner's decisions in section 10.
```

## The answer

NOT RUN. This section will state, from the scorecard: the pass rate of each
candidate per group against real Windows, which architecture wins for which
group, whether the hypothesis is supported, partly supported or killed, and
what the prototype adds over the parts it is built from.

What can be said before measuring, from upstream sources read on
14 November 2026:

- **Of the 42 tool names in the corpus, Wine 11.20 has 31 as programs of
  its own** (`arp`, `attrib`, `certutil`, `chcp`, `dism`, `fc`, `find`,
  `findstr`, `fsutil`, `hostname`, `icacls`, `ipconfig`, `net`,
  `netstat`, `ping`, `powershell`, `reg`, `robocopy`, `sc`, `schtasks`,
  `setx`, `shutdown`, `sort`, `subst`, `systeminfo`, `taskkill`,
  `tasklist`, `where`, `whoami`, `wmic`, `xcopy`; from the `programs/`
  directory of the `wine-11.20` tag) and `more` built into its cmd. It
  has no `tracert`, `nslookup`, `tar`, `curl`, `tree`, `getmac`, `route`,
  `winget`, `sfc` or `chkdsk`. Its `powershell.exe` is a stub. How well
  the 31 behave is what the run measures.
- **Windows itself moved**: `wmic` is no longer installed on current
  Windows, and Windows PowerShell 5.1 and PowerShell 7 differ; the run
  measures both distances so that the candidates are not held to a
  standard Windows does not meet.

## 1. Hypothesis and kill criterion (registered before the first run)

The first commit of the pull request (`c2192cf`) carries this text, before
any measurement:

**Hypothesis.** With the right combination of existing parts, a large
majority of everyday Windows commands can run on TUR with the same meaning
as on Windows, and the rest can be answered honestly ("this is what that
command does on Windows; on TUR the equivalent is …") instead of "command
not found".

- **Supported** if the best architecture passes at least 80 % of the corpus
  and at least 60 % of each of its three groups, where a pass is the same
  exit class and the same output shape as real Windows, and every command
  it does not pass gets an explanation.
- **Killed** if the best architecture passes less than 50 % overall, or no
  candidate passes half of some group.
- **Partly supported** otherwise; the report then says where.

## 2. Method

### 2.1 Corpus

`tests/experiments/wincli/corpus.py`: 416 command lines as people type
them, each run with `C:\turfix` (a throwaway folder of 13 small files,
rebuilt before every command) as the current directory.

| Group | Commands | State-changing | What is in it |
|---|---|---|---|
| cmd | 125 | 25 | `dir`, `cd`, `echo`, `%VAR%` and its substring forms, `set`, `set /a`, `if`, `for` in all its forms, pipes, redirection, `&&` and `\|\|`, `copy`, `move`, `ren`, `del`, `mkdir`, `rmdir`, `mklink`, batch files by name, exit codes, `/?` |
| tools | 112 | 29 | `ipconfig`, `ping`, `tracert`, `nslookup`, `tasklist`, `taskkill`, `systeminfo`, `where`, `findstr`, `find`, `sort`, `fc`, `tree`, `xcopy`, `robocopy`, `attrib`, `sc`, `net`, `reg`, `certutil`, `tar`, `curl`, `schtasks`, `shutdown /?`, `winget`, `getmac`, `arp`, `netstat`, `route`, `wmic`, `setx`, `icacls`, `fsutil`, `chcp`, `subst`, `powershell` called from cmd, and three repair commands |
| PowerShell | 179 | 22 | the file-system cmdlets and aliases, pipelines with `Where-Object`/`Select-Object`/`Sort-Object`/`Group-Object`/`Measure-Object`, CSV and JSON, the environment provider, the registry provider, `Get-CimInstance` on nine `Win32_` classes, `Get-Service`, `Get-Process`, `Start-Process`, `Invoke-WebRequest`, `Invoke-RestMethod`, the Windows-only network cmdlets, identity (`$IsWindows`, `$PSVersionTable`), tools called from PowerShell, scripts by name |

State-changing commands touch only the fixture folder, the registry key
`HKCU\Software\TurWincliTest`, the user variable `TURTEST` and the
scheduled task `TurWincliTest`, on a throwaway runner or in a throwaway
Windows environment. Commands that would change anything else are not in
the corpus; the dangerous ones are in `safety_cases.py` and are never run
on the judge. Three repair commands (`sfc /scannow`, `chkdsk C: /f`,
`DISM /Online /Cleanup-Image /RestoreHealth`) are in the corpus without
ground truth (minutes long, and they change the machine): they are scored
only for whether TUR answers them.

A further **54 held-out commands** were written on 14 November 2026 before
any result existed (commit named in section 12). They run only when
`WINCLI_FRESH=1`, in the last run of the experiment, so that nothing can
have been tuned on them; they have their own table.

### 2.2 The judge, and what leaves it

The corpus runs on GitHub's `windows-2025` runner (Windows Server 2025,
build 26100, image 20261111.331.1; `cmd.exe` and the runner's PowerShell
7.6.10). GitHub's Windows runners are real Windows and are allowed for
this by the amendment of 12 November 2026 to ADR-0015.

What is recorded is the **shape** of each result (`shape.py`), not
Microsoft's text:

| Comparator | What is compared | Used for |
|---|---|---|
| exit | zero against non-zero; the exact number where it carries meaning (`robocopy`, `fc`, `sc query` of a missing service, `exit /b 7`) | every command |
| files | the fixture folder afterwards: every path, the first 64 bits of each file's SHA-256, read-only flags, links | every command |
| text | the output lines, normalised (line endings, trailing space, blank lines, `C:\turfix` → `<FIX>`; per command: dates masked, case folded, lines sorted) | commands whose output comes from our own fixture |
| probes | a list of patterns; which of them matched | output that depends on the machine (`ipconfig`, `tasklist`, `systeminfo`, …) |
| columns | the header cells of the first table | `tasklist`, `Get-Process`, `Get-Service`, `Get-ChildItem` |
| keys | the set of `Name : value` names | `Format-List` output |
| json | the parsed structure and values | `ConvertTo-Json` |

Help text is never kept: for `/?` and `Get-Help` only the probe results
leave the runner. The raw output of the other commands is uploaded as a
separate artifact that expires after one day, for debugging the checks;
nothing of it is committed.

Verdicts: **PASS** (same exit class or code, every comparator equal),
**PARTIAL** (same exit class, at least half of the shape), **ANSWER** (not
the same, but TUR said so: an explanation or a refusal behind the marker
`tur-winshell:` on standard error), **FAIL**, **NOT RUN**, and **INVALID**
(the check does not hold on the judge itself; not counted, and listed).

Two controls bound what a pass can mean:

- **Windows against Windows.** The same corpus on `windows-2022`, scored
  against the judge like a candidate. Its pass rate is the ceiling: a
  command whose shape differs between two Windows versions is either
  version-dependent or checked too strictly.
- **Windows PowerShell 5.1 against PowerShell 7**, on the judge: the
  distance between the shell most tutorials were written for and the one
  the candidates use.

### 2.3 Candidates

All on Linux, in a Fedora 45 container (image by digest) with Fedora 45's
own Wine, on a GitHub Ubuntu runner whose network and service manager the
container sees. Each has its own copy of a fresh Wine prefix.

| | Candidate | Groups | How a line reaches it |
|---|---|---|---|
| (a) | Wine's `cmd.exe` and Wine's own tools | cmd, tools | `cmd.exe /d /s /c "<line>"`, started through `turrun.exe` so that cmd receives exactly the characters a Windows console would give it |
| (b) | PowerShell 7.6.10 for Windows under Wine | PowerShell | `pwsh.exe -EncodedCommand`; registry, services and CIM hit Wine |
| (c) | PowerShell 7.6.10 for Linux, as shipped | PowerShell | `pwsh -EncodedCommand` |
| (c+) | (c) with the `TurWinCompat` module | PowerShell | the same, after `Import-Module` |
| (d) | `tur-winshell`, the router | all | `tur-winshell --shell cmd\|ps -c "<line>"` |
| (e) | the line pasted into bash, with TUR's command names on `PATH` | cmd, tools | `bash -c "<line>"`; only names Linux does not already have get a shim |

An **oracle** column takes, per command, the best of (a), (b), (c) and
(c+): what a perfect router over the existing parts could reach without
any TUR-aware command.

### 2.4 The prototype

`tur-winshell` (`winshell/turwinshell.py`, Python for the spike):

1. applies the safety rules (section 6); a refused line is not run;
2. sends a cmd line to Wine's `cmd.exe` **with a directory of TUR's own
   commands at the front of its `PATH`**. Each is a copy of
   `turshim.exe`, a 150-line Windows program that starts the Linux helper
   with its own name and arguments, relays the helper's output and takes
   its exit code from a trailer (Wine can start a Linux program from a
   Windows one but hands back no process to wait for). The TUR commands
   are therefore ordinary commands to cmd: usable in a pipe, in `for /f`,
   in a batch file;
3. sends a PowerShell line to PowerShell for Linux with `TurWinCompat`
   (or, with `TUR_WINSHELL_PS=wine`, to PowerShell for Windows under
   Wine);
4. explains a command nobody has instead of leaving it at "not
   recognized", from a small table of what Windows commands are.

The TUR-aware commands (`winshell/shims.py`):

| Command | What it does on TUR |
|---|---|
| `ipconfig`, `ipconfig /all`, `getmac`, `route print` | the computer's real network state (`ip -j`, `/etc/resolv.conf`), in the Windows shape |
| `tasklist`, `taskkill` | processes of every ecosystem from `/proc`: Linux programs by name, Windows programs by image name; filters, CSV and list forms. `taskkill` by name leaves the Windows programs of other Windows environments alone |
| `systeminfo` | real hardware and system facts in the Windows field names |
| `tracert`, `nslookup` | the Linux tools' answers in the Windows shape |
| `tar`, `curl` | the Linux programs, with Windows paths in arguments translated (`C:\…`, `NUL`) |
| `tree` | its own implementation |
| `powershell`, `pwsh` (from cmd) | the script goes to TUR's PowerShell |
| `winget install X` | finds X in TUR's catalogue and shows TUR's install plan (`tur-apps plan <id>`); never installs without the application centre's confirmation |
| `sfc`, `chkdsk`, `dism` | say what the command does on Windows, why TUR's read-only verified image has nothing for it to repair, and point at TUR Diagnostics and rollback |
| `shutdown` | prints its options for `/?`; otherwise explains and does nothing |
| `format`, `diskpart`, `bcdedit`, `bcdboot`, `bootrec`, `bootsect` | refuse, with the reason (also behind the lexical rule, so that a batch file cannot reach past it) |

`TurWinCompat` (`compat/TurWinCompat/TurWinCompat.psm1`), for PowerShell
for Linux: the Windows environment variables; drive letters (C: is the
Windows environment's `drive_c`); the aliases Linux PowerShell drops
(`ls`, `cat`, `cp`, `rm`, `ps`, `kill`, `sleep`, `sort`, …);
`Get-CimInstance` for `Win32_OperatingSystem`, `ComputerSystem`,
`Processor`, `BIOS`, `LogicalDisk`, `NetworkAdapterConfiguration`,
`Process`, `Service` and `VideoController` from `/proc`, `/sys`, `ip` and
`df`, with WQL filters; `Get-Service` from systemd and from the Windows
environment's services; `HKLM:`/`HKCU:` paths in the item cmdlets, served
by the Windows environment's registry through Wine's `reg.exe`;
`Get-NetIPAddress`, `Get-NetAdapter`, `Test-NetConnection`,
`Resolve-DnsName`, `Get-ComputerInfo`, `Get-LocalUser`,
`Set-`/`Get-ExecutionPolicy`; the classic tools as functions that call
`tur-winshell`; refusals for the disk, partition and power cmdlets.

The CIM answers are honest about what the computer is: `Caption` is the
real operating system's name, not "Microsoft Windows". A script that
tests for that string gets a truthful no.

## 3. Scorecard

NOT RUN. To be filled from `wincli-score/summary.md`: per candidate and
group, PASS / PARTIAL / ANSWER / FAIL / NOT RUN and the pass rate; the two
Windows controls; the oracle; seconds per command; the held-out set.

## 4. Which architecture wins where

NOT RUN. To be answered per group from the per-command table
(`wincli-score/gaps.md`): Wine's cmd against the router for cmd and tools;
PowerShell for Windows under Wine against PowerShell for Linux with the
module; which TUR-aware commands Wine's own tool already matches (those
shims should go); whether the lines survive being pasted into bash.

## 5. Gap list

NOT RUN.

## 6. Safety: proposed rules

A Windows command pasted into TUR must not damage the real computer by
being taken literally. Proposed (`winshell/rules.py`):

| Rule | Text |
|---|---|
| R1 | Disk, partition, boot and firmware commands (`format`, `diskpart`, `bcdedit`, `bootrec`, `bcdboot`, `bootsect`, `cipher /w`, `manage-bde`, `mountvol`, `convert`, `vssadmin`, `wbadmin`, `reagentc`; `Format-Volume`, `Clear-Disk`, `Initialize-Disk`, the `-Partition` cmdlets, `Set-SecureBootUEFI`, the BitLocker cmdlets) are **refused, always**, with what they do on Windows and where TUR does that work. Nothing typed in a terminal reaches disks or the boot loader; that stays with TUR Storage and its confirmation path, which the assistant cannot operate either (AGENTS.md). |
| R2 | Power, repair and update commands (`shutdown`, `Stop-Computer`, `Restart-Computer`, `sfc`, `chkdsk`, `dism`, `winget`) are **not passed on**. They are answered with the TUR action that has that meaning. |
| R3 | A recursive delete or a mirror (`del /s`, `rd /s`, `robocopy /mir`/`/purge`/`/move`, `Remove-Item -Recurse`) whose target, reached through a drive letter, lies **outside the Windows environment's own drive** (a mapped folder such as Documents, or another drive letter) is refused unless the person confirms; so is one that names the root of a drive, and one whose target cannot be worked out before it runs (a variable). The same verbs inside the environment run. |
| R4 | Registry writes and service and account commands run only inside the Windows environment. That holds by construction: `reg`, `sc`, `net` and the registry paths of PowerShell reach Wine's registry and services, never the computer's. |
| R5 | A line that starts another shell (`cmd /c`, `call`, `start`, `powershell -Command`, an encoded command) is judged by what that shell would run. |
| R6 | No command gets privileges: nothing here calls `sudo`, `pkexec` or polkit. |

The rules are lexical: a seat belt. A batch file, a variable or a program
can do what a line does not show. Two things stand behind them: the R1
commands are also **programs on the Windows `PATH` that refuse**, so a
batch file that calls `format` meets the refusal too (tested); and the
real wall is the runtime's sandbox (`tur-runtime`, bubblewrap), which only
shows a Windows program the folders it was granted. The experiment's
`safety-hazard` rows measure what happens to a mapped folder with plain
Wine and no rule, to show what R3 is for.

A piped web script (`irm … | iex`) gets **no special handling**: it is
ordinary PowerShell and can touch only what any command in that shell can
touch. Nothing here automates, documents or eases licence activation;
`slmgr` is answered with one sentence (TUR's Windows environment is not a
Windows installation and has no licence to manage).

Tests (`safety_cases.py`, `integration.py`): 43 cases (32 to refuse, 11 to
allow) judged without being run; 15 of them then run for real in a throwaway Windows environment whose
Documents folder is a mapped folder holding canary files, through the
router and, for R3, through plain Wine.

- Verdicts on the development machine (pure Python, no command run): 43 of
  43 as expected.
- In CI, with Wine: NOT RUN.

## 7. Integration pieces

NOT RUN. Built and waiting for a runner (`integration.py`):

| Piece | Test |
|---|---|
| binfmt_misc | handlers by magic (`MZ`) and by extension (`.bat`, `.cmd`, `.ps1`) that hand the file to `tur-winshell --run-file`; `./hello.exe one "two words"`, `./scripts/hello.bat`, `./scripts/args.cmd one "two three"` (exit code 3), `./scripts/hello.ps1 -Name World` from bash; and what happens without the execute bit, as a download arrives |
| Path translation | `winpath.py` against Wine's own `winepath`, both directions, case-insensitive spelling, mapped folders (a file in a mapped folder gets the `C:\users\…\Documents\…` path a Windows program would use, where `winepath` answers `Z:\…`), paths inside arguments (`--output=C:\…`, `/out:C:\…`, `NUL`), and through a real Linux tool |
| Scripts by name | in the corpus: `scripts\hello.bat`, `hello` with the folder current, `.\scripts\hello.ps1`, a batch file from PowerShell |
| The bridge | a TUR command in a pipe, in `for /f`, in a batch file; order in a sequence; its exit code and `%errorlevel%` |
| A wilder idea | under Wine, `Get-CimInstance` needs Windows' `mi.dll`, which Wine does not have, but the older .NET route to WMI goes through COM, which Wine implements (`wbemprox`). Six probes say whether `[wmisearcher]`, the .NET registry API and the .NET service API work in PowerShell for Windows under Wine; if they do, a small module gives candidate (b) its CIM back with Wine's own data |

## 8. Product shape (proposal; to be confirmed by the numbers)

The design the prototype argues for, to be kept or changed by section 4:

1. **A Windows mode in TUR's terminal**, as two Konsole profiles:
   "Command Prompt" is Wine's `cmd.exe` in the person's default Windows
   environment with TUR's commands on its `PATH`; "PowerShell" is
   PowerShell with `TurWinCompat` (or PowerShell for Windows under Wine,
   whichever section 4 favours). In this mode a tutorial's lines mean
   what they mean on Windows: `%VAR%`, `C:\…`, `for %i`, `$env:…`.
2. **The same commands everywhere, where they cannot collide.** In bash,
   a Windows command name that means nothing on Linux (`ipconfig`,
   `tasklist`, `systeminfo`, `winget`, `sfc`, …) runs TUR's command or
   its explanation; names Linux already has (`ping`, `tar`, `find`,
   `sort`, `echo`, `set`, …) keep their Linux meaning. Candidate (e)
   measures how far that goes: bash rewrites `C:\Users\me` and `$env:X`
   before any shim sees them, so pasted lines with paths or variables
   need Windows mode. bash's command-not-found hook is where the offer
   "this looks like a Windows command; run it in Windows mode?" belongs.
3. **Files by name**: `./setup.exe`, `./install.bat`, `./script.ps1` go
   through `tur-winshell` (binfmt_misc), which hands programs to
   `tur-runtime`. In the product an `.exe` started this way must take the
   same route as one started from the desktop: inspection, the sandbox,
   the per-application environment (ADR-0007). The spike calls Wine
   directly.
4. **When a command has no equivalent, P@x**: `tur-winshell` prints what
   the command does on Windows and what TUR offers, from a table that
   ships with TUR and needs no model. With P@x enabled, the same hook
   hands P@x the line and the explanation as context; P@x may propose
   the TUR action as a typed tool call, which goes through the policy
   engine like any other (ADR-0012). P@x never gets to run a refused
   command: R1 refusals are final for it too.

## 9. What productising would take

- **`tur-winshell` as a TUR program** (Rust, its own workspace under
  `services/`, command-line and D-Bus), replacing the Python prototype:
  the rules, the router, the explanation table, exit codes. The
  TUR-aware commands move with it; `winget` calls `tur-apps` over D-Bus
  and `sfc`/`chkdsk` call `tur-diagnostics`.
- **The bridge** (`turshim.exe`) built in TUR's image build with
  mingw-w64, installed into every Windows environment `tur-runtime`
  creates; a real exit-code channel instead of the output trailer, and
  streaming instead of buffering.
- **The PowerShell side** as compiled code: a real registry provider
  (so that `cd HKLM:\`, tab completion and `Get-PSDrive` work) and real
  CIM classes, instead of plain functions over the item cmdlets. The
  prototype's functions lose `$?` for a failing wrapped cmdlet and do not
  stream pipeline input; that is acceptable to measure with, not to ship.
- **Routes through `tur-runtime`**: every Windows program started from the
  terminal goes through the sandbox and the per-application environment;
  which environment a terminal's Windows mode uses (a default one per
  person) needs a design decision.
- **Konsole profiles, the bash/zsh/fish hooks**, and a setting to turn
  the whole thing off.
- **Tests**: the corpus becomes a regression suite; the judge's shapes are
  refreshed from the Windows runners when the corpus changes.
- Wine fixes upstream for whatever section 5 finds in Wine's cmd and
  tools (TUR integrates upstream, it does not fork).

## 10. Licence and legal questions for the owner

### 10.1 PowerShell: the facts (read on 14 November 2026)

| Fact | Source |
|---|---|
| PowerShell is MIT-licensed: `LICENSE.txt`, "Copyright (c) Microsoft Corporation. MIT License" | https://github.com/PowerShell/PowerShell at tag `v7.6.10` |
| The project is alive: 7.6.10 and 7.7.0 published on 10 November 2026; 7.6 is the long-term-support line (LTS packages are published for it; built with .NET SDK 10.0.404); 7.7 is built on .NET 11 | GitHub releases API |
| The release packages contain more than PowerShell's own code: the .NET runtime (MIT) and third-party components; `ThirdPartyNotices.txt` (206 kB) names MIT for nearly all of them, BSD-2-Clause (Markdig, PSReadLine) and one Apache-2.0 | `ThirdPartyNotices.txt` at `v7.6.10` |
| **PowerShell sends usage telemetry to Microsoft unless `POWERSHELL_TELEMETRY_OPTOUT` is set**, and checks for updates unless `POWERSHELL_UPDATECHECK=Off` | `src/System.Management.Automation/utils/Telemetry.cs` at `v7.6.10`; README, "Telemetry" |
| Fedora does not package PowerShell (no `powershell` source package); Fedora does package .NET 10 (`dotnet10.0`) | https://src.fedoraproject.org |
| Sizes: Linux archive 73.4 MB, Windows archive 109.6 MB; unpacked sizes NOT RUN | release assets, pinned by SHA-256 in the workflow |
| The Windows-only modules tutorials use (`NetTCPIP`, `DnsClient`, `Storage`, `LocalAccounts`' data, the CIM format files) are part of Windows, not of PowerShell's MIT package; they are Microsoft binaries TUR cannot ship | to be confirmed by section 3 (candidate (b)) |

The experiment downloads the two archives from the project's GitHub
release in CI, checks their hashes, uses them and throws them away.
Nothing is committed or redistributed.

### 10.2 The choice

The project rule says "never bundle Microsoft or Apple binaries". The
PowerShell release archives are binaries built and published by Microsoft
under MIT. The ways forward:

| | Option | Rule | Cost |
|---|---|---|---|
| A | Ship Microsoft's release archive in the image | Breaks the rule as written; needs the owner to read it as "proprietary Microsoft binaries" and say so in an ADR. MIT allows it | None to build; telemetry must be switched off system-wide; the name and logo are Microsoft's trade marks |
| B | Build PowerShell from its MIT source in TUR's CI with Fedora's source-built .NET | Keeps the rule in the way windows-drivers-2 did for KMDF (a TUR-built binary of MIT source). But PowerShell's build restores NuGet packages that are themselves binaries published by Microsoft (`Microsoft.Management.Infrastructure`, `Microsoft.PowerShell.Native`, Roslyn, …); a build with none of those was NOT attempted | A new build pipeline; unknown effort; Fedora itself has not done it |
| C | The person's computer fetches it: the first use of PowerShell mode downloads the official archive by pinned hash, as AGENTS.md already prescribes for Proton builds | Keeps the rule: TUR redistributes nothing | Needs a network on first use; a pinned version to maintain; telemetry off by default in TUR's wrapper |
| D | No PowerShell: cmd and the tools only | Keeps the rule | The PowerShell group (179 of 416 commands) is answered only by explanation |

Section 4 will say how much the PowerShell group is worth. The spike's
own view is C, with the module from this experiment and telemetry off:
it honours the rule, costs no legal review beyond MIT, and matches the
existing Proton precedent.

### 10.3 Other questions

1. **Windows command names and output shapes.** TUR's commands print the
   field names Windows prints ("IPv4 Address", "Image Name", "OS Name")
   because scripts and people read them. They are short functional
   labels, written out by TUR's own code from this computer's data; no
   Microsoft text was copied, and help output is TUR's own words. A
   lawyer should confirm that reproducing such labels and command names
   is not an issue (Wine and ReactOS do the same).
2. **Identity.** `ver`, `systeminfo` and the CIM answers: Wine's `cmd`
   prints "Microsoft Windows [Version 10.0.…]" because programs test for
   it. TUR's own commands say what the computer is. The owner may want a
   single rule for what TUR says when asked "are you Windows?".
3. **GitHub's Windows runners as judge**: allowed by the ADR-0015
   amendment for the guest helper; this experiment relies on the same
   permission for measurement. Only shapes leave the runner. If the owner
   reads the amendment more narrowly, the judge has to be a Windows
   machine of the owner's.
4. **The corpus' shapes as a test asset**: the recorded shapes (exit
   codes, field names, which patterns matched) are facts about Windows'
   behaviour, kept as CI artifacts, not committed. Committing them would
   make the regression suite independent of the Windows runners; that is
   a question for the same review.

## 11. Known limitations and threats to validity

- **Nothing is measured yet.** Every claim about behaviour above is a
  design statement until a run is named.
- **The judge is Windows Server**, not Windows 11 Home or Pro. Known
  consequences: PowerShell's default execution policy allows local
  scripts on Server and forbids them on client Windows; `winget` may be
  absent; some services differ. A Windows 11 client judge (GitHub offers
  `windows-11-arm`, a different architecture) was NOT RUN.
- **The corpus is one author's idea of "everyday"**, written before the
  measurement and not drawn from a sample of real tutorials. The
  held-out set is by the same author. A corpus mined from popular
  support pages is a follow-up.
- **The router is tuned on the corpus it is scored on** once the first
  run is in; the held-out set and the oracle column bound the optimism.
- **Shape is not meaning.** A probe that matches "IPv4 Address … : an
  address" does not check that it is the right address. The comparators
  were chosen so that a pass needs the right structure and plausible
  values; they cannot prove equivalence.
- **The container is not TUR**: Fedora 45 with Fedora's Wine, run as
  root, on Ubuntu's kernel, with the runner's systemd reached through a
  socket; no desktop, no `tur-runtime` sandbox, no TUR image.
- **Linux exit statuses keep eight bits**; Windows exit codes have 32.
  The harness reads the full code from a file; a shell pipeline on Linux
  cannot.
- **The PowerShell module is a prototype** (section 9); its registry
  cmdlets start one Wine process per call.
- **`tasklist` shows Linux process identifiers**, also for Windows
  programs, whose Windows process identifiers differ; `taskkill /pid`
  agrees with `tasklist`, but a Windows program that asks Windows for
  that PID will not find it.

## 12. Runs

| Run | Commit | What | Result |
|---|---|---|---|
| 43537757872 | `c2192cf` | first push: registration, corpus, harness, candidates | Both Windows jobs got a runner within three minutes and **failed at checkout**: the repository contains a path Windows cannot create (`services/tur-device-manager/tests/fixtures/laptop/sys/bus/pci/devices/0000:00:00.0`). The Linux job was cancelled in the queue. Nothing measured |
| 43538182154 | `602849c` | telemetry off; WMI-through-COM probes | never started: it waited behind the first run, whose scorecard job (`if: always()`) sat in the queue for an Ubuntu runner. Superseded |
| (next) | this commit | Windows jobs check out only `tests/experiments/wincli`; the scorecard no longer runs for a superseded run; the held-out set, behind a switch | NOT RUN when this version was written |

A finding outside the experiment: **the repository cannot be checked out
on Windows** (the device-manager fixture's `0000:00:00.0` directories).
Any future job on a Windows runner needs a sparse checkout, as this
workflow now does.

Upstream versions pinned or observed: Fedora 45 container image
`sha256:fa773d10…3770`; Wine 11.20 (Fedora 45, per the task; the run
prints `wine --version`); PowerShell 7.6.10
(`powershell-7.6.10-linux-x64.tar.gz` sha256 `e261cabc…e10d`,
`PowerShell-7.6.10-win-x64.zip` sha256 `7d871531…e166`); the Windows
runner's PowerShell 7.6.10.

## 13. Proposed decision text (for the lead to number)

To be completed with the numbers. Draft:

> **Windows command lines on TUR.** TUR offers a Windows mode in its
> terminal: Wine's `cmd.exe`, and PowerShell, in the person's Windows
> environment, with TUR's own commands on the path for the commands whose
> meaning on TUR is a TUR action (`winget`, `ipconfig`, `tasklist`,
> `sfc`, `chkdsk`, `shutdown`). Windows command names that do not collide
> with Linux ones also work in the ordinary shell. Disk, partition and
> boot commands are refused; recursive deletes that would leave the
> Windows environment need confirmation; power, repair and update
> commands are answered with the TUR action. A command with no equivalent
> is explained, never left at "not found". PowerShell is [fetched by the
> person's computer on first use / built by TUR / shipped / not offered]
> (owner's decision), with telemetry off. TUR does not claim that every
> Windows command works: the measured pass rates are published with each
> release.

## 14. Follow-ups

1. Run the workflow; fill sections 3, 4, 5 and 7.
2. Drop every TUR-aware command that Wine's own tool already matches.
3. The held-out set, once, at the end (`WINCLI_FRESH=1`).
4. A Windows 11 client judge.
5. A corpus mined from real tutorials and support pages.
6. If the COM route works under Wine: a module that gives PowerShell for
   Windows its CIM cmdlets back there, and a second look at (b).
7. ReactOS's command-line tools (GPL, not Microsoft's) under Wine for the
   tools Wine lacks (`tracert`, `nslookup`, `tree`): NOT RUN, one cheap
   test.
8. A real registry provider for PowerShell on Linux, compiled, backed by
   the Windows environment's registry.
9. Wine bug reports and patches for what section 5 finds.
