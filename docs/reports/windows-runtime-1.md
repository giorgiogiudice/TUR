# windows-runtime-1 — Run a Windows program: prefixes, launch plans, sandbox

```text
UOS DEVELOPMENT REPORT

Task ID:          windows-runtime-1
Subsystem:        Universal Runtime Manager (services/uos-runtime)
Build ID:         not applicable: this task builds no image
Source revision:  8a33f1d3d4e8 (feat/windows-launch, pull request 26), which
                  is the commit the measurements below come from. The commit
                  that adds this report follows it and changes no code.

SUMMARY
- What changed: uos-runtime can run a Windows program. One prefix per
  application with schema-versioned metadata; every prefix operation and every
  launch is a plan first and is carried out through an Executor; launch plans
  for plain Wine and for Proton through umu-run; every runtime command runs in
  a bubblewrap sandbox that shows the application its own prefix and the
  directories the user granted and nothing else of the user's files; the text
  of a desktop entry; new command-line commands; a new CI workflow that runs
  real Windows programs under Wine through this code.
- Why it changed: the runtime manager could identify a Windows program and
  could not run one (specification 7.2, 12.2, 15.2, 19.2).

FILES
- Added: services/uos-runtime/src/{desktop,exec,grant,launch,launch_host,
  metadata,paths,plan,prefix,runtime_cmd,sandbox,timefmt}.rs,
  services/uos-runtime/tests/launch_cli.rs,
  schemas/prefix-metadata.schema.json,
  tests/windows-runtime/{console.c,gui.c,native.c,build.sh,
  sandbox-preflight.sh,e2e.py}, .github/workflows/windows-runtime.yml,
  docs/reports/windows-runtime-1.md
- Modified: services/uos-runtime/src/{lib,main,codes,limits,profile}.rs,
  services/uos-runtime/Cargo.toml (description only),
  services/uos-runtime/tests/cli.rs, docs/architecture/runtime-manager.md
- Removed: none

BUILD
- Command: cargo clippy --all-targets -- -D warnings; cargo test (workflow
  "rust"); cargo build --locked (workflow "windows-runtime")
- Result: see RUNS below
- Warnings: none from the compiler or clippy. Wine prints errors while it
  creates a prefix; see "What Wine printed".

TESTS
- Unit tests: 247 in services/uos-runtime (148 before this task), with a
  recording executor in place of the system. Pass.
- Integration tests: 19 tests of the binary (11 before). The new ones give it
  a scratch home and name the runtime on the command line; launches are dry
  runs; verify, backup, restore, remove, grant, revoke and pin are carried out
  for real. Pass.
- End to end: workflow "windows-runtime", 259 checks, on a hosted Ubuntu
  24.04 runner with Ubuntu's Wine 9.0. Pass. Detail below.
- Boot tests: NOT RUN. Nothing here can affect boot.
- Hardware tests: NOT RUN. No GPU, no audio device, no real display.
- Performance tests: NOT RUN.
- On the TUR image: NOT RUN. Wine is not in the image.
- Proton through umu-run: NOT RUN. Plan tests only.
- Real applications: NOT RUN. Two test programs.

DIAGNOSTICS
- Log paths: GitHub Actions runs below; artifact "windows-runtime-evidence"
  (versions, sandbox preflight, results.json, every launch log, operations
  logs, prefix metadata, the profiles used)
- Diagnostic bundle path: none; no bundle format exists yet
- Relevant error IDs: 22 new codes, UOS-WINDOWS-PREFIX-001 to -008,
  UOS-WINDOWS-RUNTIME-005 to -010, UOS-SECURITY-SANDBOX-001 to -006,
  UOS-DESKTOP-RUNTIME-005 and -006

RECOVERY
- Rollback procedure: revert the merge. No shared file is changed. Prefixes
  created by this code live under $XDG_DATA_HOME/uos/runtime and can be
  deleted.
- Recovery tested: backup, restore, reset and repair of a prefix ran in CI.
  A migration to a real second Wine version did not: one Wine was installed.

KNOWN LIMITATIONS
- See "What did not run" and "Known limitations" below.

NEXT SAFE ACTION
- Put Wine in the TUR image (a decision: Fedora package or own build) and run
  this same workflow against the image, under SELinux.
```

## Runs

| Commit | Workflow | Run | Result |
|---|---|---|---|
| `3b3c087` (first push) | rust | 43160284849 | Failed for `services/uos-runtime`: rustfmt drift and two clippy findings. Every other component passed |
| `3b3c087` | windows-runtime | 43160285006 | Failed: 252 of 255 checks. A Windows program ran through the sandbox on this first run; the three failures were the repair test (see Known limitations) |
| `a9ca6ed` | rust | 43161217098 | Passed |
| `a9ca6ed` | windows-runtime | 43161217128 | Passed |
| `8a33f1d` (after merging `main`) | rust | 43178138268 | Passed, all ten jobs |
| `8a33f1d` | windows-runtime | 43178138193 | Passed, 259 of 259 checks. **This report is written from this run** |

The commit that adds this report is later than `8a33f1d`; its runs are named in
the pull request. Three iterations were needed to reach green.

## What ran end to end

The `windows-runtime` workflow, on the runner, in this order:

1. Installed `wine`, `wine64`, `bubblewrap`, `gcc-mingw-w64-x86-64`,
   `gcc-mingw-w64-i686`, `xvfb` and `x11-utils` from Ubuntu's archive and
   printed their versions.
2. Checked whether bubblewrap could build the sandbox (see "The sandbox on the
   hosted runner").
3. Compiled, from C sources in `tests/windows-runtime/`: a 64-bit Windows
   console program, a 64-bit Windows GUI-subsystem program, a 32-bit Windows
   console program and a 64-bit DLL with mingw-w64; and a native program six
   ways with gcc. Nothing built is committed.
4. Built `uos-runtime` with `cargo build --locked`.
5. Ran `tests/windows-runtime/e2e.py`, which records every check as PASS or
   FAIL and fails the job if any check fails.

Versions, from the run's own output:

```text
runner image: ubuntu24 20261101.360.1
os: Ubuntu 24.04.5 LTS
kernel: 7.0.0-1017-azure
wine: wine-9.0 (Ubuntu 9.0~repack-4build3)
bubblewrap: bubblewrap 0.9.0
mingw x86_64: x86_64-w64-mingw32-gcc (GCC) 13-win32
mingw i686: i686-w64-mingw32-gcc (GCC) 13-win32
gcc: gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0
binutils: GNU readelf (GNU Binutils for Ubuntu) 2.42
python: Python 3.12.3
packages:
  bubblewrap 0.9.0-1ubuntu0.3
  gcc-mingw-w64-i686-win32 13.2.0-6ubuntu1+26.1
  gcc-mingw-w64-x86-64-win32 13.2.0-6ubuntu1+26.1
  libwine 9.0~repack-4build3
  mingw-w64-common 11.0.1-3build1
  wine 9.0~repack-4build3
  wine64 9.0~repack-4build3
  xvfb 2:21.1.12-1ubuntu1.9
ntsync device: ls: cannot access '/dev/ntsync': No such file or directory
```

Nothing is pinned: these are the versions Ubuntu 24.04's archive carried on the
day. A later run can get a different Wine. Rust is the toolchain pinned in
`rust-toolchain.toml` (1.99.1).

### Inspection against real binaries (issue #12, first item)

`uos-runtime inspect --json` was run on every file built in step 3 and compared
field by field with what binutils says about the same file: `readelf -h` and
`readelf -l` for ELF, `x86_64-w64-mingw32-objdump -p` for PE.

| File | Built with | Inspected as | Agrees with |
|---|---|---|---|
| `native-pie` | `gcc` | ELF64, x86-64, PIE, dynamic, interpreter `/lib64/ld-linux-x86-64.so.2` | readelf |
| `native-nopie` | `gcc -no-pie` | ELF64, fixed-address executable, dynamic | readelf |
| `native-static` | `gcc -static` | ELF64, executable, static, no interpreter, GNU OS ABI | readelf |
| `native-static-pie` | `gcc -static-pie` | ELF64, PIE, static, no interpreter | readelf |
| `libnative.so` | `gcc -shared` | ELF64, shared object: not a program | readelf |
| `native.o` | `gcc -c` | ELF64, relocatable: not a program | readelf |
| `console64.exe` | `x86_64-w64-mingw32-gcc` | PE32+, x86-64, console subsystem | objdump |
| `gui64.exe` | `... -mwindows` | PE32+, x86-64, GUI subsystem | objdump |
| `console32.exe` | `i686-w64-mingw32-gcc` | PE32, i386, console subsystem | objdump |
| `library64.dll` | `... -shared` | PE32+, DLL: not a program | objdump |
| APK-shaped zip | Python `zipfile`, deflate | Android package, dex present, ABIs `arm64-v8a` and `x86_64`, 5 members | the writer |
| zip with a comment | Python `zipfile`, stored | zip, 2 members | the writer |
| zip with 70 000 members | Python `zipfile` | zip, 70 000 members (zip64 end records) | the writer |

**The hand-written parsers agreed with the real files in every field checked.
No parser error was found and no parser was changed.** What this covers: the
ELF header fields, the program-header walk for the interpreter and the dynamic
section, the `DF_1_PIE` flag, the PE machine, optional-header magic, subsystem
and DLL flag, and the zip end-of-central-directory search including zip64. What
it does not cover: 32-bit and big-endian ELF, ARM64 PE, .NET assemblies, a
kernel driver, Mach-O, OLE and MSI files, a real APK built by the Android
tools. No toolchain for those was installed.

### A Windows program under Wine, through `uos-runtime launch`

The console program prints a known line, its arguments, two environment
variables, and what happened when it wrote into a granted directory, wrote into
a read-only granted directory, and tried to read four files it was not granted.
It exits with the status it is given as its first argument.

What was run and asserted, in order:

| Step | Asserted |
|---|---|
| `prefix create local.uos-test --grant-rw DIR --grant DIR` | Exit 0. `system.reg` and `drive_c/windows` exist; `.update-timestamp` holds `disable`; the metadata records the application, `wine`, `wine-9.0`, the build string `wine-9.0 (Ubuntu 9.0~repack-4build3)`, `win64`, both grants, and a launch count of 0. `prefix verify` passes. A second `prefix create` is refused (`UOS-WINDOWS-PREFIX-002`) |
| `launch profile.json --dry-run --json` | Exit 0; the plan reuses the prefix; nothing ran (the launch count is still 0) |
| `launch profile.json -- 42 "two words" "C:\path with\back slashes"` | The program printed `uos-windows-test: hello from a Windows process` and `pointer-bits=64`; the three arguments arrived intact; the variable given with `--env` arrived; **the exit status of `uos-runtime` was 42**, the program's own |
| The record after it | Launch count 1, a last-run time, creation time and runtime version unchanged, the `ntsync` probe result (`absent`), the selection (`server`) and the reason |
| The launch log | One file; it holds the program's output and `# exit status: 42`; it does not hold the value of any environment variable |
| `launch` again | Exit 0. The program counts its runs in a file inside the prefix and printed `prefix-runs=2`: the same prefix. Launch count 2, creation time unchanged, last-run time moved on, a second log file |
| `launch` with a profile pinned to `wine-0.0.1` | **Refused**, exit 6, `UOS-WINDOWS-RUNTIME-005`; the program did not run |
| `launch` with a profile pinned to `wine-9.0` | Ran (`prefix-runs=3`) |
| `launch --runtime-version wine-99.0` (the installed Wine made to look newer than the prefix) | **Refused**, exit 6, `UOS-WINDOWS-PREFIX-006`; the program did not run; the record is unchanged |
| The same with `--migrate` | Ran (`prefix-runs=4`). A backup archive exists and holds the registry, the private home and the metadata; the record names `wine-99.0` and the move from `wine-9.0`. `prefix migrate` then moved it back; both moves are recorded |
| `prefix pin`, then `launch --migrate` with the faked version | Refused, `UOS-WINDOWS-PREFIX-007` |
| `prefix backup`; delete `user.reg` and the run counter; `prefix verify`; `prefix restore` | Verify exits 8 with `UOS-WINDOWS-PREFIX-004`; after the restore both files are back and the next launch prints `prefix-runs=5` |
| Delete `system32/d3d11.dll`; `prefix repair` | Exit 0; Wine restored the file; user files kept; the next launch prints `prefix-runs=6` |
| GUI program, first launch, on an Xvfb display, no prefix beforehand | The launch created the prefix. A window titled "UOS test window" appeared in `xwininfo -root -tree` on the display. Exit status 7, the program's own. Launched again with a granted directory, the program wrote `window-created visible=1 screen=1280x800` |
| `prefix reset` | A fresh prefix (the run counter is gone), the grants kept, the old prefix archived first |
| `prefix remove` | The application's directory and logs are gone; its backups, the other application's prefix, the granted directories and the canary in the home directory are untouched |

The migration rows used a faked version because one Wine was installed. What
they exercise is the plan: the refusal, the backup, `wineboot --update` run by
the real Wine, and the record. They do not show a prefix surviving a real Wine
upgrade.

### The sandbox as a security boundary

**Control.** Before anything else, the same program was started by `wine`
directly, with no sandbox. It read every one of the files below and saw the
caller's environment. So the reads that fail under `uos-runtime` fail because of
the sandbox, not because the test looks in the wrong place.

**Under `uos-runtime launch`**, as reported by the Windows program itself and
checked again on the host:

| Attempt | Result |
|---|---|
| Write a file into the read-write grant | Succeeded; the file is on the host with the expected content |
| Read a file from the read-only grant | Succeeded |
| Write a file into the read-only grant | Denied (`errno` 13); nothing appeared on the host |
| Read a file in a directory that was not granted | Denied |
| Read `~/.ssh/uos-test-canary`, in the runner user's real home | Denied |
| Read a file in another application's prefix | Denied |
| Read its own `metadata.json` | Denied |
| Read the ungranted file through a symbolic link planted inside the read-write grant | Denied |
| See `UOS_TEST_HOST_CANARY`, set in the caller's environment | Not set |

Also run for real: a grant of `/`, of the home directory, of `~/.ssh`, of a path
with `..`, of `/proc/self/root`, of the runtime manager's data directory and of
another application's directory were each refused with
`UOS-SECURITY-SANDBOX-003`; metadata edited by hand to grant `~/.ssh` made the
next launch refuse with `UOS-SECURITY-SANDBOX-004`; and `sudo uos-runtime
launch` and `sudo uos-runtime prefix create` were refused with
`UOS-SECURITY-SANDBOX-001` and created nothing.

What this proves: on this runner, with this Wine, a Windows process started
through `uos-runtime` could not read the files it was not granted, could not
write to a read-only grant, and did not receive the caller's environment.

What it does not prove: that nothing else is reachable. The test tries specific
files. The stronger statement comes from the unit tests, which enumerate every
mount of every sandbox built from over a hundred grant requests and assert that
none is, contains or lies inside a protected path; and from the design, in
which the home directory is simply not mounted. Neither is a proof against
kernel bugs, and there is no seccomp filter. The threat model and its limits are
in the [design note](../architecture/runtime-manager.md#the-sandbox).

### The sandbox on the hosted runner

bubblewrap could not create namespaces on the runner as delivered:

```text
kernel: 7.0.0-1017-azure
bubblewrap: bubblewrap 0.9.0
setuid bit on bwrap: -rwxr-xr-x
sysctl kernel.apparmor_restrict_unprivileged_userns = 1
sysctl kernel.unprivileged_userns_clone = 1
sysctl user.max_user_namespaces = 58121
AppArmor profiles naming bwrap:
first attempt failed: bwrap: setting up uid map: Permission denied
remedy 1: an AppArmor profile that allows /usr/bin/bwrap to create user namespaces
result: bubblewrap builds the sandbox after remedy 1 (AppArmor profile for bwrap only)
```

Ubuntu 24.04 restricts unprivileged user namespaces through AppArmor
(`kernel.apparmor_restrict_unprivileged_userns = 1`): a program may create one
only if an AppArmor profile allows it, and the runner has none for `bwrap`. The
workflow installs a profile that allows `/usr/bin/bwrap`, and only it, to create
user namespaces, which is the mechanism Ubuntu uses for the applications it
ships profiles for. That was enough. The machine-wide switch
(`kernel.apparmor_restrict_unprivileged_userns=0`) is the script's second
remedy and was not needed. With neither, the script fails the job: the sandbox
is never dropped from the test, and `uos-runtime` has no way to launch without
it.

This is a property of Ubuntu, not of TUR. Fedora does not carry this
restriction; SELinux is the thing to test there.

### What Wine printed

Wine is not quiet on this runner, with or without the sandbox. Creating a
prefix printed 21 `err:` lines when Wine was started directly and 23 inside the
sandbox. Both lists are the same in kind:

- `it looks like wine32 is missing` and `failed to open
  C:\windows\syswow64\rundll32.exe`: Ubuntu's 32-bit Wine is not installed.
- `nodrv_CreateWindow ... no driver could be loaded` and `Could not create tray
  window`: there is no display while the prefix is created.
- `err:ole:... Failed to create ifstub` and `Failed to open RpcSs service`:
  the RPC service, during `wineboot`.

The two extra lines inside the sandbox are `Error running cmd
winemenubuilder.exe -a -r (126)`, which is the override that disables it doing
its job, and one more `Failed to start RpcSs service`. Neither stopped the
prefix from being created or the programs from running. Whether the RpcSs
failure matters to an application that uses COM across processes was not
tested. The full output of both is in the artifact (`kept/control-run-stderr.txt`
and the operations logs).

## What did not run

| Not run | Why |
|---|---|
| Anything on the TUR image | Wine is not in the image. Everything above is Ubuntu 24.04 with Ubuntu's Wine 9.0, AppArmor, no SELinux, no Plasma |
| Proton through `umu-run` | Downloading Proton in CI is out of scope. The command line, environment and sandbox for it are unit-tested as plans; nothing has executed |
| A real application | Two test programs prove the path: start, arguments, environment, exit status, file access, a window. They prove nothing about compatibility |
| `ntsync` in use | The runner has no `/dev/ntsync` and its Wine (9.0) predates `ntsync` support. The fallback ran and was recorded; the positive path is unit-tested only |
| A migration between two real Wine versions | One Wine was installed. The migration steps ran for real (backup, `wineboot --update`, new record) with the version faked on the command line, and are labelled so |
| DXVK, VKD3D-Proton, any Direct3D or Vulkan | No GPU on the runner, and installing the DLLs is not built |
| A 32-bit Windows program | Built and inspected; not run. Ubuntu's `wine32` needs the i386 architecture, which the workflow does not enable |
| Wayland, audio | The runner has neither. The window test used X11 (Xvfb) |
| A second user, SELinux, a read-only `/usr` as on an image-based system | Not available on the runner |

## Known limitations

- **X11 gives an application the keyboard and the screen.** Any X11 client can
  read both. The sandbox does not change that.
- **No seccomp filter, no Landlock, no resource limits.**
- **Interrupting `uos-runtime` kills the sandbox at once**, without letting
  the Wine server write the registry. There is no stop command.
- **Repair is only what `wineboot --update` can do.** In the first CI run the
  test deleted `kernel32.dll` from a prefix: `wineboot --update` then failed
  and the next launch exited with status 53, because Wine could not start in
  that prefix at all. Such a prefix needs a restore or a reset. The repair test
  now deletes `d3d11.dll`, which Wine restores.
- **`launch` exits with the application's status**, which can coincide with
  `uos-runtime`'s own refusal statuses (6, 7, 8). `--json` disambiguates.
- **A Proton application would download its own copy of the runtime** into its
  private home. A shared, read-only runtime store is future work.
- **`isolation_profile` is not interpreted.** There is one sandbox.
- **Wine's own automatic prefix update is switched off** by writing `disable`
  to `.update-timestamp`. That is read from Wine's source, not from its
  documentation, and a future Wine could change it. The version comparison in
  the launch plan does not depend on it.
- **Error codes** use the `WINDOWS`, `SECURITY` and `DESKTOP` families; the
  `RUNTIME` family asked for in issue #12 is a change to `crates/uos-core`,
  which this task does not own.

## Dependencies

No crate was added. `services/uos-runtime/Cargo.lock` is unchanged. The crate
still depends on `clap` (MIT OR Apache-2.0), `serde` and `serde_json` (MIT OR
Apache-2.0) and `uos-core` (this project). `unsafe` is still forbidden: the
effective user comes from `/proc/self/status`, and the `ntsync` probe is an
ordinary `open`.

Programs started at run time, none of them linked or shipped by this change:

| Program | Licence | Used for | On the TUR image |
|---|---|---|---|
| bubblewrap (`bwrap`) | LGPL-2.0-or-later | The sandbox | Yes, 0.13.1 |
| Wine | LGPL-2.1-or-later | The runtime | No |
| umu-launcher (`umu-run`) | GPL-3.0 | Starting Proton; plan only | No |
| Proton | Mixed; not redistributable by this project | Fetched by `umu-run` on the user's machine; never shipped | No |
| GNU tar, gzip | GPL-3.0-or-later | Backups | Yes |

CI only: mingw-w64 (the compiler and its runtime, used to build the test
programs on the runner; nothing built is committed or shipped), Xvfb.

Starting a program as a separate process puts no licence constraint on
`uos-runtime` (AGENTS.md, licence boundary rules).

## Changes needed in shared files

None was made. Three are worth making by their owners:

- `crates/uos-core`: a `RUNTIME` error-code family (issue #12, second item).
  The codes added here would move to it before any release.
- `docs/dependencies/upstream-projects.md`: bubblewrap's role is now concrete
  (the sandbox for Windows programs); umu-launcher's README states GPLv3 and
  the file already says so. No licence finding changed.
- `README.md`: the status table, if it lists what the runtime manager can do.

Issue #12: the first item is done by this change. The others are untouched.
