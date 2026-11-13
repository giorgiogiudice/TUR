<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Compatibility autopilot

Design note for the compatibility autopilot of `tur-runtime` (specification
§7.2, §12.2, §15.2; [runtime manager](runtime-manager.md); [AI control
layer](ai-control-layer.md) §§5, 16.3, 21–23; [catalogue and
routing](catalogue-and-routing.md)). Code: `services/tur-runtime/src/failure.rs`,
`remedy.rs`, `autopilot.rs`, `autopilot_run.rs`. Data:
`services/tur-runtime/remedies/catalogue.json`. Schemas:
`schemas/remedy-catalogue.schema.json`, `remedy-check.schema.json`,
`remedy-result.schema.json`, `remedy-applied.schema.json`, and the record
format of `schemas/app-catalogue-entry.schema.json`. Measurements:
[autopilot-1](../reports/autopilot-1.md).

## 1. What it is for

When a Windows application fails or misbehaves under Wine, people today search
forums for the setting that makes it work, try it in their only prefix, and
write down what they found, or do not. The autopilot does that work: it runs
the application unchanged in a **throwaway copy** of its prefix, reads what
went wrong from the evidence, tries the remedies known for that kind of
failure, each in a fresh copy, keeps the first that makes the application's
check pass, and writes down what it measured in a form that can be shared and
put into the catalogue.

It never changes the application's real prefix. Applying what it found is a
separate command, which the person runs; it takes a backup first, and a
second command restores that backup.

A result is a measurement: one application, one version, one runtime, one
machine, one check. It is never a rating and never a claim of compatibility
(AGENTS.md).

## 2. The flow

```
tur-runtime app autopilot <app-id> [--check FILE]

  1. read      the record, the prefix metadata, the main program, the check
  2. learn     what the program's files say: resource languages, DLLs beside
               it, names on drive C: that differ only in case
  3. probe     what the machine offers: X11 display, Wine's own DLLs, whether
               the file system can fold case, ntsync, font files
  4. copy      the prefix and private home, once, into the result's directory
               (copy-on-write where the file system can); Wine's crash dialog
               is switched off in this copy only
  5. baseline  attempt 0: a fresh copy, unchanged, run against the check
               - passes: done ("passed-without-remedy")
               - needs a Microsoft component: done ("unsuppliable"), the
                 Windows guest is suggested, nothing is tried
  6. classify  the failure class and hints, from the evidence
  7. search    each candidate remedy alone, each in a fresh copy; then bounded
               pairs; stop at the first attempt whose check holds
  8. record    result.json (every attempt, its log's digest, timings), the
               catalogue record record.toml, the fingerprints of the real
               prefix before and after, which must be equal
```

```
tur-runtime app autopilot --apply <result-id>     the person's decision
tur-runtime app autopilot <app-id> --revert       undo it
tur-runtime app autopilot [<app-id>] --plan [--failure CLASS]   what would be tried
tur-runtime app autopilot <app-id> --results      what was found
```

## 3. Failure detection

`failure::diagnose` reads a run's evidence into one stable class. The
evidence is the exit status, how the run ended (exited, stopped after its
window had settled, or stopped because its time was up), whether a window of
the program appeared, everything the program and Wine printed, and which parts
of the functional check did not hold. Wine's messages are matched by the text
its loader and debugger print, read in Wine 9.0's source
(`dlls/ntdll/loader.c`, `dlls/ntdll/signal_x86_64.c`, `dlls/kernelbase/debug.c`,
`programs/winedbg/info.c`).

| Code | Class | Read from |
|---|---|---|
| `TUR-WINDOWS-COMPAT-001` | `prefix-unusable` | `wine: could not load kernel32.dll`, `wine: cannot find`, `is not owned by you`, ... |
| `TUR-WINDOWS-COMPAT-002` | `dll-missing` | `Library X (which is needed by ...) not found` (also what a disabled DLL gives) |
| `TUR-WINDOWS-COMPAT-003` | `microsoft-component-missing` | `dll-missing` where X belongs to a Microsoft redistributable (below) and this Wine has no implementation of that name |
| `TUR-WINDOWS-COMPAT-004` | `dotnet-runtime-missing` | `mscoree.dll not found, IL-only binary ...`, or a missing `mscoree`/`mscorwks` |
| `TUR-WINDOWS-COMPAT-005` | `dll-load-failed` | `Loading library X (which is needed by ...) failed` |
| `TUR-WINDOWS-COMPAT-006` | `dll-init-failed` | `"X" failed to initialize, aborting` |
| `TUR-WINDOWS-COMPAT-007` | `unimplemented-function` | `unimplemented function X.Y` (the loader's stub, or winedbg) |
| `TUR-WINDOWS-COMPAT-008` | `crash` | `Unhandled exception`, `wine: Unhandled ...`, `starting debugger` |
| `TUR-WINDOWS-COMPAT-009` | `exited-with-error` | a non-zero exit status and none of the above |
| `TUR-WINDOWS-COMPAT-010` | `no-window` | a window was expected and none of the program's appeared |
| `TUR-WINDOWS-COMPAT-011` | `hung` | no window expected, and the program did not finish in time |
| `TUR-WINDOWS-COMPAT-012` | `check-failed` | it ran, and the functional check did not hold |

When several lines name a failure, the earliest cause wins: a prefix Wine
cannot start in, then .NET, then a missing DLL, a DLL that would not load, one
that would not initialise, an unimplemented function, a crash.

**Windows.** A window is "of the program" when an X11 window whose instance or
class is the program's file name in lower case (what Wine sets,
[runtime-manager](runtime-manager.md#desktop-entries)) appears, is viewable
(`xwininfo -id`), and was not there before the attempt. `xwininfo` from the X
utilities is run outside the sandbox and only lists windows. Inside a virtual
desktop the program's windows are inside the desktop's own, so an attempt with
a virtual-desktop remedy also accepts the desktop window (class `explorer.exe`).

**Hints** order remedies; they never decide a class:

| Hint | From |
|---|---|
| `windows-version`, `display-size`, `dpi`, `audio`, `direct3d`, `fonts` | Words in the program's own output: lines that are not Wine's debug channels and that come before any crash report. "requires Windows", "resolution", "DPI", ... (`OUTPUT_HINTS` in `failure.rs`). Wine's own `err:d3d`, `err:wgl`, `err:vulkan` lines also give `direct3d` |
| `locale-ja`, `locale-zh-cn`, `locale-zh-tw`, `locale-ko` | The language identifiers in the main program's resource tree (`pe_res::resource_languages`) |
| `case-collision` | Names in one directory on drive C: that differ only in case (a bounded walk that follows no link) |
| `dll-beside-program` | The DLL the evidence names lies in the program's own directory |

## 4. The remedy catalogue

`services/tur-runtime/remedies/catalogue.json`, format
`schemas/remedy-catalogue.schema.json`, compiled into the program. A remedy is
data, not code: one of four kinds of change, each reversible.

| Kind | What changes | How it is set in a copy | How it is applied | How it is undone |
|---|---|---|---|---|
| `registry` | Values under `HKEY_CURRENT_USER` of the prefix (and nothing else: the validator refuses another hive) | A `.reg` file (REGEDIT4) imported with Wine's own `reg import`, in the sandbox, with only that file shown read-only | The same, in the real prefix, after a backup | The backup; the result also shows the `.reg` text that deletes the values |
| `environment` | Variables of the application's launches (`LC_ALL`, ...), held to the launch rules: no variable the runtime manager sets itself, nothing starting with `LD_` | Passed to the attempt's launch | Written to `prefixes/<app-id>/remedies.json`; `app launch` adds them (a variable given with `--env` wins) | `--revert` deletes the file |
| `drive-c-case-folding` | Drive C: rebuilt as a directory with the ext4 case-folding attribute, the contents copied in | The copy's `drive_c` is created empty, given `chattr +F`, and the prefix copied into it | A new case-folded directory beside `drive_c`, the contents copied in, the two swapped | The backup, restored with an ordinary drive C: |
| `hide-ntsync` | `/dev/ntsync` left out of the sandbox, so that Wine 11 synchronises through the Wine server | The attempt's sandbox does not get the device | `remedies.json`; `app launch` then does not pass the device | `--revert` |

No remedy downloads, installs or runs anything: the kinds cannot express it,
and the validator also refuses a remedy whose text mentions winetricks,
vcredist, dotnetfx, corefonts, "download", "install " or msiexec, or that
addresses a missing Microsoft component.

Registry remedies that Wine supports per program are written under
`HKCU\Software\Wine\AppDefaults\<program>` (`{exe}` in the catalogue): they
apply to the application's own program and not to Wine's helpers in the same
prefix. Those Wine reads only globally (the virtual desktop, DPI, audio
driver, font replacements) are written globally; one application has one
prefix (§12.2), so "global" is still that application's.

### 4.1 The remedies

Every source was read on the date given in the catalogue (13 November 2026,
the ntsync note 12 November). Wine's sources were read at the tag
`wine-9.0` from the GitHub mirror `wine-mirror/wine` (winehq.org refuses
automated reading); what they say is described in TUR's words, nothing was
copied. The Proton README (BSD-style licence) is described, not copied.

| Remedy | Kind | Group | Tried for | Brought forward by | Needs | Source |
|---|---|---|---|---|---|---|
| `windows-version-win11` | registry: `AppDefaults\{exe}` `Version=win11` | windows-version | exited-with-error, check-failed, no-window, crash | windows-version | | `dlls/ntdll/version.c` (`version_init`, name table: win11 is 10.0 build 22000; win10, build 19043, is the default) |
| `windows-version-win10` | `Version=win10` | windows-version | same | windows-version | | same |
| `windows-version-win7` | `Version=win7` | windows-version | same | windows-version | | same |
| `windows-version-winxp64` | `Version=winxp64` | windows-version | same, only with the hint | windows-version | | same |
| `dll-builtin` | `AppDefaults\{exe}\DllOverrides` `{dll}=builtin` | dll-load-order | dll-missing, dll-load-failed, dll-init-failed, crash | | Wine has `{dll}` | `dlls/ntdll/unix/loadorder.c` (order: `WINEDLLOVERRIDES`, `AppDefaults\<program>\DllOverrides`, `DllOverrides`; `n`, `b`, empty = disabled); `dlls/ntdll/unix/loader.c` and the `-Wb,--prefer-native` DLLs |
| `dll-native-builtin` | `{dll}=native,builtin` | dll-load-order | unimplemented-function, dll-missing, crash, only with the hint | dll-beside-program | `{dll}` beside the program | `loadorder.c` |
| `virtual-desktop-1024x768` | `Explorer` `Desktop=Default`, `Explorer\Desktops` `Default=1024x768` | virtual-desktop | exited-with-error, check-failed, no-window, crash | display-size | X11 display | `programs/explorer/desktop.c`; `programs/winecfg/x11drvdlg.c` writes the same values |
| `virtual-desktop-800x600` | as above, 800x600 | virtual-desktop | same, only with the hint | display-size | X11 display | same |
| `dpi-120` | `HKCU\Control Panel\Desktop` `LogPixels=120` (dword) | dpi | exited-with-error, check-failed, only with the hint | dpi | | `dlls/win32u/sysparams.c` (`LOGPIXELS` entry); `x11drvdlg.c` |
| `dpi-144` | `LogPixels=144` | dpi | same | dpi | | same |
| `renderer-gl` | `AppDefaults\{exe}\Direct3D` `renderer=gl` | renderer | crash, no-window, exited-with-error, only with the hint | direct3d | | `dlls/wined3d/wined3d_main.c` (`renderer`: vulkan, gl, gdi or no3d; global and per program) |
| `renderer-vulkan` | `renderer=vulkan` | renderer | same | direct3d | | same |
| `renderer-no3d` | `renderer=no3d` | renderer | crash, no-window, only with the hint | direct3d | | same |
| `audio-off` | `Drivers` `Audio=""` | audio | crash, hung, no-window, exited-with-error, only with the hint | audio | | `dlls/mmdevapi/main.c` (`init_driver`: empty means no driver; default list pulse,alsa,oss,coreaudio) |
| `audio-alsa` | `Drivers` `Audio=alsa` | audio | same | audio | | same |
| `font-arial-liberation-sans` | `Fonts\Replacements` `Arial=Liberation Sans` | font-arial | check-failed, exited-with-error, crash, only with the hint | fonts | `LiberationSans-Regular.ttf` installed | `dlls/win32u/font.c` (`load_gdi_font_replacements`) |
| `locale-ja` | environment `LC_ALL=ja_JP.UTF-8` | locale | crash, exited-with-error, check-failed, no-window, only with the hint | locale-ja | | `dlls/ntdll/unix/env.c` (`init_locale`, `unix_to_win_locale`; `LC_ALL` read directly when setlocale gives C) |
| `locale-zh-cn`, `locale-zh-tw`, `locale-ko` | `LC_ALL=zh_CN.UTF-8`, `zh_TW.UTF-8`, `ko_KR.UTF-8` | locale | same | the matching locale hint | | same |
| `drive-c-case-insensitive` | drive-c-case-folding | case-folding | check-failed, exited-with-error, crash, no-window, only with the hint | case-collision | a file system that folds case | TUR's own measurement ([windows-runtime-3](../reports/windows-runtime-3.md)); `dlls/ntdll/unix/file.c` |
| `ntsync-off` | hide-ntsync | synchronisation | crash, hung, no-window | | `/dev/ntsync` opens and Wine ≥ 11.0 | Wine 11.0 release notes, as recorded in [runtime-manager](runtime-manager.md#verified-upstream) |
| `proton-no-esync`, `proton-no-fsync` | environment `PROTON_NO_ESYNC=1`, `PROTON_NO_FSYNC=1` | proton-synchronisation | crash, hung; never tried automatically | | a Proton application | Proton `proton_10.0` README, runtime-configuration table |

The esync/fsync remedies exist only for Proton: upstream Wine has neither
([runtime-manager](runtime-manager.md#the-launch-environment)). The autopilot
of this build runs Wine applications only, so they are data, not yet tried.
`WINE_LARGE_ADDRESS_AWARE` is not in the catalogue: it is not read anywhere in
Wine 9.0's `dlls/ntdll` (it belongs to Wine Staging and Proton's own Wine);
Proton's `PROTON_FORCE_LARGE_ADDRESS_AWARE` is "enabled by default" there.

### 4.2 What TUR never supplies

`microsoft_components` in the catalogue lists the Microsoft redistributables
by DLL-name prefix: MFC (`mfc`), ATL (`atl`), the Visual C++ runtime (`msvcp`,
`msvcr`, `vcruntime`, `vcomp`, `concrt`, `ucrtbase`), the Visual Basic runtime
(`msvbvm`), the DirectX redistributable (`d3dx`, `d3dcompiler`, `xinput`,
`xaudio`, `x3daudio`, `xapofx`), MSXML (`msxml`) and .NET (`mscoree`,
`mscorwks`). A DLL belongs to one when its name is a prefix followed by nothing
or a digit (`mfc140u`, not `mfcustom`). A missing DLL of one of these families
is `microsoft-component-missing` **only when this Wine has no implementation
of that name** on the machine (`/usr/lib*/wine/{x86_64,i386}-windows/`): Wine
implements most of the Visual C++ runtime and the DirectX DLLs itself, and for
those the remedy is to use Wine's.

For `microsoft-component-missing` and `dotnet-runtime-missing` nothing is
tried. The result is `unsuppliable`, names the component and suggests the
route `windows-guest` (ADR-0007), and the command exits with 10 and
`TUR-WINDOWS-AUTOPILOT-002`.

## 5. Choosing and bounding

`autopilot::candidates` takes every remedy that addresses the class, runs on
the application's runtime and is tried automatically. A remedy marked
`needs_hint` is tried only when one of its hints is present; a remedy that
takes the DLL needs the evidence to name one; a requirement the machine does
not meet passes the remedy over. Every remedy that addresses the class and is
not tried is listed with the reason (`passed_over`). The order: remedies a
hint brought forward first, then by `order` in the catalogue.

`autopilot::search`:

1. The baseline, unchanged.
2. Each candidate alone, in order, each in a fresh copy.
3. **Progress.** A remedy whose attempt fails with a *different* class has
   got the application past its first failure (a broken DLL fixed, and now a
   version check refuses). The remedies for the new class are tried together
   with it, at most six of them.
4. Pairs of the first four single remedies from different groups.
5. Stop at the first attempt whose check holds.

At most `--max-attempts` attempts (default 12, at most 64; the baseline is
not counted), at most two remedies together (`MAX_COMBINATION`), never two of
one group. `--no-combinations` stops after step 2.

## 6. The copies

```
$XDG_DATA_HOME/tur/runtime/autopilot/<app-id>/<UTC time>/
    base/prefix, base/home     the first copy; deleted at the end unless --keep-copies
    trial/                     one attempt's copy; deleted after the attempt
    attempt-N.log              what attempt N printed, with a header
    result.json, record.toml   the result and the catalogue record
```

- **Copy-on-write where it can.** The first copy is made with
  `cp -a --reflink=always`; where the file system cannot share blocks (ext4)
  it is made again with `--reflink=never`, and the result says which
  (`snapshot.method`), how long it took, and how many files and bytes it
  holds. Every attempt's copy is made from the first copy the same way. `cp -a`
  copies links as links and never follows them.
- **Case folding is kept.** When the real drive C: is case-insensitive the
  copies' are made so too, and the case-folding remedy makes one so.
- **What an attempt sees.** The same sandbox as a launch
  ([runtime-manager](runtime-manager.md#the-sandbox)) around the copy's
  prefix and private home, with **no granted directory** and **no network**
  (`--network` shares it), so that an attempt can neither change the
  person's files nor reach out while it is tried over and over. The program's
  standard input is `/dev/null`.
- **Crashes end.** Wine's crash dialog waits for someone to close it. The
  first copy, and so every attempt, gets `HKCU\Software\Wine\WineDbg`
  `ShowCrashDialog=0` (`programs/winedbg/intvar.h`, `crashdlg.c`); the real
  prefix never does.
- **Stopping.** An attempt ends when the program exits, when its window has
  been up for the check's settle time, or when its time is up; the sandbox is
  then killed, which takes everything in it down (`--die-with-parent`).
- Remedies already applied to the application (`remedies.json`) are part of
  every attempt, as they are part of every launch.

## 7. The check

`schemas/remedy-check.schema.json`, given with `--check`. Without one: a
window of the program within 60 seconds, then 3 seconds up or an exit with
status 0. A run also fails when Wine reports that a process was ended (a
crash, a DLL that failed to initialise, imports that could not be resolved, an
unimplemented function, a prefix Wine cannot start in); a DLL reported missing
on its own does not count, because Wine says so for optional imports of its
own DLLs too.

```json
{ "schema_version": 1, "window": false, "exit": "zero", "timeout_seconds": 60,
  "settle_seconds": 0, "output_contains": ["TUR-CHECK-PASS"] }
```

`exit` is `zero`, `zero-or-running` or `any`; `window_class` overrides the
class looked for; `output_contains`, `output_lacks` and `arguments` complete
it.

## 8. The result and the shareable record

`result.json` (`schemas/remedy-result.schema.json`) holds the application and
the file it was installed from (SHA-256), the runtime version and build, the
check, how the copies were made, what was known about the program, the failure
of the unchanged application, the plan (what was chosen and what was passed
over, with reasons), every attempt (remedies, outcome, failure, exit status,
window, timings for copying, setting the remedies and running, the log's name
and SHA-256), what was found, the real prefix's fingerprints before and after,
`evidence_sha256` and the total time. `evidence_sha256` is the SHA-256 of the
attempts serialised as compact JSON, every time in it written as whole
milliseconds (a time read back from JSON can differ in its last digit); a
result whose attempts do not match it
is refused by `--apply` and `--results`.

The fingerprint of the real prefix and private home is the SHA-256 of every
entry's path, kind, size, modification time to the nanosecond and mode, in a
fixed order, links not followed. It is taken before the first copy and after
the last attempt. When the two differ the command fails with
`TUR-WINDOWS-RUNTIME-010`: the autopilot does not write to the prefix, so
something else did (the application running at the same time, for example).

When a remedy was found, `record.toml` is a compatibility record in the
catalogue's own format, to be appended to the Windows edition of the
application's entry:

```toml
[[editions.records]]
route = "wine"
rating = "unknown"
reported_by = "tur-autopilot"
version_tested = "1.0"
date = "2026-11-13"
remedies = ["dll-builtin:d3dx9_43", "windows-version-win11"]
runtime_version = "wine-9.0"
evidence_sha256 = "..."
notes = "tur-runtime compatibility autopilot, result ...: failed as dll-init-failed (TUR-WINDOWS-COMPAT-006) unchanged; passed TUR's check ... Measured once, on one machine; not a rating."
```

`tur-catalogue validate` checks it (catalogue/README.md, "Records from TUR's
compatibility autopilot"): the rating must be `unknown`, the route `wine` or
`proton`, 1 to 16 remedy names, a runtime version and a 64-digit digest; the
three fields are refused on any other kind of record. The router never rates a
route from such a record. It carries the most recent one's remedies, runtime
version, date and digest as the route option's `settings`
(`tur-catalogue route --json`), and the text output says "Works with these
settings (TUR's autopilot ...; not a rating)". Routes without such a record
print exactly what they printed before (the field is left out), which the
application centre's recorded fixtures check.

Nothing is uploaded. The record is a file; the person, or later the
problem-report flow ([problem-reports](problem-reports.md)), can share it.

## 9. Applying and undoing

`tur-runtime app autopilot --apply <result-id>` is the person's decision. It
refuses a result that found nothing (`TUR-WINDOWS-AUTOPILOT-005`), one
measured with another runtime version than the one installed and the prefix
is on (`-007`), one whose remedies name another program than the
application's (`-004`), and an application that already has remedies applied
(`-008`). Then:

1. `prefix backup` with the reason `autopilot`;
2. the registry remedies, with `reg import` in the real prefix, in the
   sandbox, without network or granted directories;
3. for the case-folding remedy, a new case-folded directory beside drive C:,
   the contents copied in, the two swapped, the old one deleted; when the
   file system will not fold case, nothing is swapped;
4. `prefixes/<app-id>/remedies.json` (`schemas/remedy-applied.schema.json`):
   the result, the backup, the remedies, the environment, whether ntsync is
   hidden, whether drive C: was case-insensitive before.

Any failure after the backup restores it. `app launch` reads `remedies.json`
and adds its variables and its ntsync choice to every launch.
`tur-runtime app autopilot <app-id> --revert` restores the backup taken at
`--apply`, with drive C: as it was before, and deletes `remedies.json`.
`app uninstall` deletes `remedies.json` with the application; results are
kept, like backups.

## 10. Safety properties, and the tests that hold them

| Property | Held by |
|---|---|
| The real prefix is never written by a search | Every attempt runs on a copy under `autopilot/` (`CopyTrials`); the fingerprints before and after are in every result and the command fails when they differ; CI hashes the real prefix and private home by content before and after every case (`autopilot.py`, "the real prefix is byte for byte what it was") |
| A copy never reaches back into the original | `cp -a` copies links as links (`autopilot_run` test `a_copy_never_reaches_back_into_the_original`); fingerprints do not follow links (`a_fingerprint_sees_every_change_and_follows_no_link`) |
| Nothing outside the runtime manager's directories is deleted | Copies are deleted through `SystemExecutor`'s guard (same test, last assertions; `exec` tests) |
| An attempt cannot change the person's files or reach the network | No granted directory and no network in an attempt's sandbox (`CopyTrials::attempt`); the sandbox boundary tests of [runtime-manager](runtime-manager.md#the-sandbox) apply unchanged |
| No remedy installs or fetches anything, or touches another hive | The four kinds; `remedy` tests `the_shipped_catalogue_is_valid` and `a_remedy_that_would_install_or_reach_outside_the_user_is_refused` |
| A Microsoft component is never "remedied" | `failure` test `a_microsoft_redistributable_this_wine_lacks_is_named_and_not_remedied`; `autopilot` test `a_missing_microsoft_component_is_not_tried_at_all`; CI case `mfc` |
| A hostile parameter cannot write another registry key | `remedy` test `instances_fill_in_the_program_and_the_dll` (a program name with `]` and a newline is refused) |
| Attempts are bounded | `autopilot` test `the_attempt_limit_is_kept` |
| Applying is the person's act, after a backup, and is undone exactly | No assistant tool applies (`tests/agent_tools.rs`); CI checks the backup, that the application passes after `--apply`, and that after `--revert` the prefix hashes as it did before `--apply` |
| A tampered or foreign result is refused | `autopilot` test `a_result_survives_a_round_trip_and_tampering_is_seen` |
| A record never becomes a rating | `tur-catalogue` tests `an_autopilot_record_states_settings_and_never_a_rating`, `an_autopilot_record_gives_settings_and_never_a_rating`, `an_autopilot_record_is_validated_and_routed_as_settings` |
| Existing catalogue output is unchanged | `desktop_fixtures.rs` (the recorded router output for the seed catalogue) |

## 11. P@x

The design (ai-control-layer §5, §16.3) puts trying a fix in a copy that
never touches the person's prefix at class 1 at most, and applying a fix the
person's decision. Two tools are offered, both class 0, both read-only:

| Tool | Command | What it returns |
|---|---|---|
| `runtime_autopilot_plan` | `tur-runtime app autopilot --plan --json [--failure=CLASS] -- [APP_ID]` | For a failure class (or the class of the latest result, or every class): the remedies that would be tried, in order, with reasons, those passed over with reasons, and the components never supplied |
| `runtime_autopilot_results` | `tur-runtime app autopilot --results --json -- APP_ID` | Every result kept for the application, and the remedies applied now |

No adapter uses a word that changes anything (`tur-pax/tests/tools.rs`,
`CHANGING_WORDS`), and the invariant test "every offered tool only observes"
stays green.

**`runtime_autopilot_run` is not declared.** It would be class 1 by the
design, and the policy engine as built does not allow it yet: every tool with
an adapter must be class 0 (`every_tool_has_a_class_and_every_offered_tool_only_observes`),
and P@x's read-only sandbox has no display, no network, no writable directory
and no view of the user's data directory, so the program could not run there
anyway. What the policy would need, described and not done here (`tur-pax*`
is not this change's to edit):

1. a class-1 tool kind whose adapter runs outside the read-only sandbox, as
   the user, with the runtime manager's own sandbox around the application
   instead, limited to commands whose only writes are under
   `$XDG_DATA_HOME/tur/runtime/autopilot/`;
2. a per-tool declaration of that write location, checked by the loader;
3. the same for reading: today the results tool, run in P@x's sandbox, sees an
   empty data directory and answers "no results"; it is useful from the
   desktop and the command line now, and to P@x once item 1 or a read grant
   for `~/.local/share/tur/runtime/autopilot` exists.

`--apply` and `--revert` stay out of P@x's reach whatever the class model
becomes: they change the real prefix.

## 12. Command line and codes

```
tur-runtime app autopilot <app-id> [--check FILE] [--max-attempts N] [--no-combinations]
        [--network] [--keep-copies] [--json] [--runtime-program PATH]
        [--runtime-version VERSION] [--sandbox-program PATH]
tur-runtime app autopilot [<app-id>] --plan [--failure CLASS] [--json]
tur-runtime app autopilot <app-id> --results [--json]
tur-runtime app autopilot --apply <result-id> [--dry-run] [--json]
tur-runtime app autopilot <app-id> --revert [--dry-run] [--json]
```

`<app-id> --dry-run` is `--plan`. Exit status: 0 when a remedy was found or
none was needed; 10 when nothing that was tried passed or the failure needs
something TUR cannot supply; the others as for every command
([runtime-manager](runtime-manager.md#command-line)).

| Code | Condition |
|---|---|
| `TUR-WINDOWS-AUTOPILOT-001` | No remedy that was tried made the application pass its check |
| `TUR-WINDOWS-AUTOPILOT-002` | The application needs a Microsoft component TUR cannot supply; the Windows guest is the route |
| `TUR-WINDOWS-AUTOPILOT-003` | No result has this identifier |
| `TUR-WINDOWS-AUTOPILOT-004` | A result is unreadable, not valid, does not match its evidence digest, or names another application or program |
| `TUR-WINDOWS-AUTOPILOT-005` | The result found no remedy to apply |
| `TUR-WINDOWS-AUTOPILOT-006` | The check file is unreadable or not valid |
| `TUR-WINDOWS-AUTOPILOT-007` | The autopilot cannot run here: not a Wine application, no X11 display or `xwininfo` for a window check, or a result measured with another runtime version |
| `TUR-WINDOWS-AUTOPILOT-008` | Remedies are already applied; revert them first |
| `TUR-WINDOWS-AUTOPILOT-009` | No remedies are applied, so there is nothing to revert |

## 13. Decisions the owner may want to reverse

| Decision | Why | The other way |
|---|---|---|
| The catalogue lives in `services/tur-runtime/remedies/` and is compiled in | It is the runtime manager's own knowledge, versioned with the code that interprets it; no file to go missing | `catalogue/remedies/`, beside the application catalogue, read at run time |
| Copies get no network and no granted directory | An attempt is run many times by a program, not by the person | `--network`, which exists; or the application's own grants read-only |
| Results are kept after `app uninstall` | They are measurements to share, like backups | Delete them with the application |
| A second `--apply` is refused until `--revert` | One backup, one undo, no stacking | Merge remedies, one backup per apply |
| Remedies are written per program (`AppDefaults`) where Wine allows it | Wine's helpers in the prefix are left alone | Global keys |
| `Reporter::ALL` keeps three reporters; `Reporter::EVERY` has `tur-autopilot` | The application centre's recorded vocabulary (`desktop/apps/tur-centre/fixtures/vocabulary.json`) lists `ALL`, and this change may not edit `desktop/` | Add it to `ALL` and regenerate the fixtures with the desktop's owner |
| The router shows settings and never a rating | The honesty rule (catalogue/README.md) | A rating once the application centre says whose it is |
| The schema of catalogue entries (`schemas/app-catalogue-entry.schema.json`) gained three optional fields and a reporter | The record format the task asks for lives there; the change is additive | |

## 14. Not done

- **Proton.** The autopilot runs Wine applications only; the Proton remedies
  are data.
- **The TUR image.** Everything measured ran on a hosted Ubuntu runner with
  Ubuntu's Wine 9.0 ([autopilot-1](../reports/autopilot-1.md)); nothing on
  Fedora, Plasma, Wayland or a GPU. `renderer-*`, `audio-*`, the font remedy
  and `ntsync-off` have never been tried by CI: no GPU, no sound device worth
  testing, no ntsync device, no Liberation fonts on the runner.
- **Wayland-only windows.** Windows are found through X11; a program on Wine's
  Wayland driver would be `no-window`.
- **Programs that need their own grants** fail in a copy that has none; give
  the check a program run that does not need them, or use `--network`.
- **More remedies**: per-application environment beyond the locale, Wine's
  `WINEDEBUG` (it would remove the evidence the classifier reads), `WINEARCH`
  (cannot change after creation), and anything that fetches.
- **Sharing**: no upload; a later problem-report flow can carry `record.toml`
  and the logs.
- **D-Bus**: none; the command line is the interface.
