<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# pax-winbridge-1 — P@x operates Windows applications on the installed system

> **Status: built; measurement in progress.** Pull request 141, stacked on
> 136 (Wine in the image) and 128 (the desktop programs, for P@x's
> indicator). The design and what was built are below; the CI runs that
> measure it were queued when this was written, and every result in
> section 4 says **NOT YET MEASURED** until its run has finished. No number
> here comes from anything but a CI run named next to it.

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-winbridge-1 (the owner's decisions of exp-winsee-1,
                  section 11.1, made real)
Subsystem:        P@x (AI control layer, tier 2: acting through what an
                  application exposes); Windows compatibility (tur-runtime);
                  local AI (tur-ai-local); the image
Build ID:         the image of the boot run in section 4
Source revision:  pull request 141 (branch pax-winbridge)

SUMMARY
- What changed: the experiment's bridge became a component
  (services/tur-winbridge: the Win32 tier, and the MIT menu hook apart),
  built in the image build with Fedora's mingw-w64; tur-runtime offers it
  for every Wine launch on a socket of the session and its supervisor starts
  it inside the application's sandbox for each connection, with the
  connection as its pipe, the hook off for catalogue games; tur-pax-a11y
  speaks to it as a second source of windows with the same tools, node
  model, order and classes; the vision rule is a check with tests;
  tur-ai-local gains the capability "locate" and Qwen3-VL 4B Instruct.
- Why: Wine exposes nothing on the accessibility bus (pax-accessibility.md,
  limit 4b); the experiment proved the bridge (172 of 172 controls, 37 of 37
  actions, 46 of 46 policy checks) and the owner adopted it.

FILES
- Added: services/tur-winbridge/{tw_core.c,tw.h,build.sh},
  services/tur-winbridge/hook-mit/{tw_hook.c,LICENSE}, image/winbridge/
  winbridge.sh, services/tur-runtime/src/winbridge.rs,
  services/tur-pax-a11y/src/{wine.rs,vision.rs},
  services/tur-ai-local/catalogue/qwen3-vl-4b-instruct.toml,
  tests/pax/a11y/{winwindow.c,windows_phase.py,windows_boot.py}, this report
- Modified: image/{Containerfile,Containerfile.containerignore,programs.tsv,
  README.md}, tests/image/smoke.sh, services/tur-runtime/src/{exec.rs,
  launch.rs,launch_host.rs,plan.rs,main.rs,lib.rs},
  services/tur-runtime/tests/launch_cli.rs, services/tur-pax-a11y/src/
  {serve.rs,overlay.rs,main.rs,lib.rs}, services/tur-ai-local/src/
  {catalogue.rs,cli.rs}, the catalogue entries of Qwen3-VL 2B and Gemma 4
  E2B, schemas/local-model.schema.json, services/tur-pax/data/models.json,
  tests/pax/a11y/{harness.py,run.sh,Containerfile},
  tests/boot/windows.py and the guest's windows-check (136's files),
  .github/workflows/{image,pax,local-ai}.yml, docs/architecture/
  {pax-accessibility,runtime-manager,local-ai}.md, THIRD_PARTY_NOTICES.md
- Removed: none (tests/experiments/winsee is kept as it was)

BUILD
- Command: the image workflow (stage "winbridge"), the rust workflow, the
  pax accessibility job (bridge and test window with mingw-w64 in the
  test's Fedora container); in CI only
- Result: NOT YET MEASURED (section 4)

TESTS
- Unit tests: tur-runtime (winbridge, launch, exec, the supervisor's CLI),
  tur-pax-a11y (wine, vision), tur-ai-local (catalogue): NOT YET MEASURED
- Integration tests: pax accessibility job, Windows phase: NOT YET MEASURED
- Boot tests: the Windows boot of the boot workflow: NOT YET MEASURED
- Hardware tests: NOT RUN (no hardware runner)
- Performance tests: NOT RUN

DIAGNOSTICS
- Log paths: the launch log (logs/<app-id>/*.log) says whether the bridge
  was offered and why not; tur-pax-a11y logs TUR-A11Y lines (identifiers and
  positions only) on standard error
- Relevant error IDs: TUR-AI-INTERFACE-003/004/005, TUR-AI-POLICY-006

RECOVERY
- Rollback procedure: revert the pull request. Nothing is stored by the
  bridge; a socket left by a launcher that was killed is replaced at the
  next launch and ignored by tur-pax-a11y (nobody answers on it).
- Recovery tested: a stale socket is replaced (unit test); a bridge that
  is gone is left out (unit test)

KNOWN LIMITATIONS
- Section 5.

NEXT SAFE ACTION
- Section 6.
```

## 1. What was built

### 1.1 Where the code lives, and why

`services/tur-winbridge/` is a component of its own: a Windows program
(a PE file built with mingw-w64, not a Cargo workspace), started by
`tur-runtime` and spoken to by `tur-pax-a11y`, belonging to neither, and its
menu hook needs a directory of its own under another licence
(`services/tur-winbridge/hook-mit/`, MIT, with its LICENSE). The experiment
(`tests/experiments/winsee/`) is unchanged; the component is a copy of its
`bridge/tw_core.c` and `hook-mit/` with these differences:

| | Experiment | Component |
|---|---|---|
| Tiers | Win32, MSAA, UI Automation | Win32 only: Wine 11.20's UI Automation returns no children and its MSAA names 5 of 172 controls (exp-winsee-1, section 3) |
| Operations | `hello`, `windows`, `read`, `describe`, `act`, `at_point`, `click_point`, `close`, `foreground`, `com_*`, `quit` | `hello`, `windows`, `read`, `describe`, `act`, `at_point`, `quit`: no click at a position (the owner's decision 3), no closing of windows, no COM (not adopted) |
| Windows listing | the window's rectangle | also `content`: where the part the compositor shows begins (the window less the caption and frame the window manager draws, as Wine's X11 driver computes it), for P@x's indicator |
| Imports | `kernel32`, `user32`, `ole32`, `oleaut32`, `oleacc`, `advapi32`, the C runtime | `kernel32`, `user32`, the C runtime; the build fails on any other DLL |
| Build | `build.sh`, stubs for parts that do not compile | `build.sh`: no stubs, no PE time stamp, paths mapped, stripped |

### 1.2 How the bridge is started (decision text below)

The launcher binds `$XDG_RUNTIME_DIR/tur/winbridge/<app-id>.sock` (0600, in a
0700 directory) and makes the listening socket bubblewrap's standard input.
Inside the sandbox the supervisor (`tur-runtime supervise --bridge PATH
[--bridge-no-hook]`) gives the application an empty standard input and,
for each connection, starts `wine /usr/lib/tur/winbridge/tur-winbridge.exe`
with the connection as its standard input and output. This is neither of
the two ways the experiment proposed (start with every application, or
join the sandbox's namespaces): the supervisor is already inside, the
bridge costs nothing until P@x looks, no namespace is joined and no
descriptor is passed after the start.

## 2. Decisions for the lead (ADR text; the lead assigns numbers)

**A. The Windows bridge is started on demand by tur-runtime's supervisor.**

> tur-runtime offers P@x's bridge for every Wine launch on a socket of the
> user's runtime directory, `$XDG_RUNTIME_DIR/tur/winbridge/<app-id>.sock`,
> mode 0600 in a directory of mode 0700, whose listening end is the
> sandbox's standard input. The supervisor inside the sandbox starts
> `tur-winbridge.exe` under the application's Wine for each connection, with
> the connection as the bridge's standard input and output, one at a time,
> and stops it when the application ends; the launcher removes the socket.
> The bridge is a file of the image, read-only in the sandbox, never copied
> into a prefix. tur-pax-a11y trusts a socket only when it and its directory
> belong to the user and the directory is closed to others; the window's
> owner is then `user`, and the application's main identifier is the one
> tur-runtime named the socket after.

**B. The menu hook (the owner's decision 2) as built.** The ADR that
AGENTS.md asks for code under another licence: `services/tur-winbridge/
hook-mit/` holds `tw_hook.c` (`SPDX-License-Identifier: MIT`) and `LICENSE`;
it includes only the Windows headers and shares no code with the bridge;
the image installs it as `/usr/lib/tur/winbridge/tur-winhook.dll` with its
licence in `/usr/share/licenses/tur-winhook/`. It is off
(`TUR_WINBRIDGE_NO_HOOK=1`) for an application whose catalogue entry says
`category = "game"`, and for every Proton launch.

**C. Locating is a capability of its own in the model catalogue.**

> "Image input" does not mean a model can locate a control in a picture.
> The catalogue says `locate` only for a model measured doing it (a
> `[[tested]]` entry with `locate = "yes"`), with `locate_scale`, the
> enlargement it was measured at. Qwen3-VL 4B Instruct, from its
> publisher's GGUF, is the model P@x uses to locate controls when it needs
> vision. A point it proposes is never acted on by itself.

## 3. Upstream, checked on 15 November 2026

| What | Found |
|---|---|
| Qwen/Qwen3-VL-4B-Instruct-GGUF | revision `1cd86afb9a95c410a6038ab3b40d8b578c892266`; `Qwen3VL-4B-Instruct-Q4_K_M.gguf` 2 497 281 664 bytes, SHA-256 `66358cb1…730a0a`; `mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf` 453 974 304 bytes, `30ba2c7d…bd06d` (Hugging Face API, LFS digests; the same as the experiment pinned the day before); card `license: apache-2.0`, tag `license:apache-2.0`; base model Qwen/Qwen3-VL-4B-Instruct (revision `ebb281ec…`) `apache-2.0`; config: 36 layers, 8 KV heads, head size 128 (147 456 bytes of 16-bit cache a token) |
| Wine in the image | from pull request 136: Fedora 45's Wine (11.21 at its last commit) |
| mingw-w64 | Fedora 45's `mingw64-gcc`, `mingw64-crt`, `mingw64-headers`, `mingw64-binutils`, installed in the build stage on the day of the build (16.2.1 and 14.0.0 in the experiment's run) |

## 4. Measured

NOT YET MEASURED: the runs of pull request 141 were queued when this was
written. To be filled from: the rust workflow (unit tests), the image
workflow (the stage, the smoke test's four checks), the pax workflow's
accessibility job (the Windows phase, with screenshots under KWin and the
real overlay), the local-ai workflow (Qwen3-VL 4B through tur-ai-local:
download and digest, tool call, image input, peak memory), and the boot
workflow's Windows boot (7-Zip on the installed image, SELinux enforcing,
signed-in Plasma; screenshots by the host, looked at before anything is
claimed).

## 5. Not run, and limits

- **32-bit applications: NOT RUN.** A 32-bit application needs a 32-bit
  bridge and hook for list rows, tabs, toolbar buttons and menus; only the
  64-bit bridge is built.
- **Applications that draw their own controls** (Qt, Electron, Java,
  games): NOT RUN; the Win32 tier sees one window and nothing in it.
- **A graphics card, real hardware: NOT RUN.** Everything is in VMs and
  containers on hosted runners.
- **Proton**: no bridge is started under Proton (it runs in the Steam Linux
  Runtime's container, where the supervisor is not); only the hook decision
  is made and tested.
- **The accessibility tree's side of the vision rule** is not built: it
  cannot tell whether another window covers a point (KWin knows), so it
  would refuse every point.
- **No vision model is wired into P@x**; only the flag and the check.
- **Positions under Wine's Wayland driver** (the CI container's KWin has no
  Xwayland) follow the X11 driver's frame computation and may be off by the
  caption.
- **Anyone who is the user** can connect to the socket while the
  application runs, as they can reach its files.

## 6. Follow-ups

- A 32-bit build of the bridge and the hook.
- The bridge's JSON reader under a fuzzer (it reads a socket).
- The bridge under Proton, inside the Steam Linux Runtime.
- The accessibility tree's `at_point` with the overlay's stacking answer.
- The bridge and hook in the SBOM (`/usr/lib/tur/winbridge/build.tsv` holds
  what the SBOM needs).
- Toolbar buttons' names from their tool tips (Notepad++'s icons).
- With `TUR_WINBRIDGE_NO_HOOK=1` the bridge still allocates memory in the
  application to read list rows: for games that is still intrusive and
  could be switched off with the hook.
