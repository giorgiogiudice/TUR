# Universal Runtime Manager

Design note for `tur-runtime` (specification §2.3, §7, §8.2, §12.1, §12.2, §13.1,
§15.2, §19.2, §19.3). Code: `services/tur-runtime/`. Schemas:
`schemas/app-profile.schema.json`, `schemas/prefix-metadata.schema.json`,
`schemas/app-record.schema.json`.

## Scope

The first slice answers two questions about a file the user points at: what is
it, and which runtime is the route for it. It also defines the application
profile. That part only reads: the specification forbids executing a file because
of its type (§7.2), and `tur-runtime inspect` never runs anything.

The second slice runs a Windows program the user asks to run: a prefix per
application, a launch plan, and a sandbox around every runtime command. It is
described from [Running a Windows program](#running-a-windows-program) on.

The third slice installs a Windows application from its installer: it tells
what kind of installer a file is, runs it unattended in a fresh prefix inside
the sandbox, finds out what it installed, records that, generates a launcher
entry, and removes the application again. It is described from
[Installing a Windows application](#installing-a-windows-application) on.

The fourth is the compatibility autopilot: when an installed application
fails, it tries known remedies in throwaway copies of the prefix and records
what made it pass. It has a design note of its own,
[compat-autopilot](compat-autopilot.md); see also
[The compatibility autopilot](#the-compatibility-autopilot) below.

A recommendation names a route. It is never a statement that a particular
application works: that needs a recorded test (§2.3). The same holds for
launching and installing: an application that installs and opens a window has
been measured to do that, in one version, on one machine. The measurements so
far are in the [second report](../reports/windows-runtime-2.md); case-insensitive
prefixes were measured in the [third](../reports/windows-runtime-3.md).
See [Not done yet](#not-done-yet).

## Inspection

`inspect(path)` identifies the input from its content. The file name and its
extension are never consulted. A directory is examined as a macOS application
bundle. The result is an `Inspection`, or a typed error when a recognised format
is truncated, malformed or over a limit. An unrecognised file is reported as
`unknown`, which is a statement of ignorance and not a verdict on the file.

| Format | Recognised by | What is read | What is not read |
|---|---|---|---|
| ELF | `\x7fELF` | Class, byte order, machine, type, OS ABI; program headers for the interpreter path and the dynamic section; `DT_FLAGS_1` for the PIE flag; AppImage magic at offset 8 | Sections, symbols, needed libraries |
| PE/COFF | `MZ`, then `PE\0\0` at `e_lfanew` | Machine, PE32 or PE32+, subsystem, DLL flag, CLR data directory (.NET) | Sections, imports, resources, signatures |
| Other `MZ` files | `MZ` without a PE signature | Whether the newer header is NE, LE, LX, absent or outside the file | Anything else |
| Mach-O, thin | `feedface`, `feedfacf`, either byte order | Processor type, file type | Load commands, linked libraries, code signature |
| Mach-O, universal | `cafebabe`, `cafebabf` | Every architecture entry, and the header of each image | As above |
| macOS `.app` bundle | A directory with `Contents/Info.plist` or `Contents/MacOS` | Top-level strings of an XML `Info.plist`; the main program, which is then inspected as a file | Binary property lists; frameworks, resources, entitlements |
| Android package | A zip with `AndroidManifest.xml` (APK), `base/manifest/AndroidManifest.xml` (AAB), or APK members plus `manifest.json` (XAPK) or `toc.pb` (APKS) | Member names only; native ABIs from `lib/<abi>/` | The binary manifest; the packages inside an XAPK or APKS |
| Windows Installer | OLE compound-file magic and the root storage class identifier | Whether the file is an installer package, patch or transform | The installer tables, including the target architecture |
| Script | `#!` and an absolute interpreter path | The interpreter line | The script |
| Zip | `PK\x03\x04` | The number of members | Member contents |

Points worth knowing:

- **ELF type.** `ET_DYN` is reported as a position-independent executable when
  the file names an interpreter or sets `DF_1_PIE`, and as a shared object
  otherwise. A static PIE without `DF_1_PIE` would be reported as a shared
  object.
- **Windows kernel.** An image with the native subsystem (drivers, boot-time
  system programs) is reported as requiring a Windows kernel. Detection is by
  subsystem only. A driver package is usually an installer that carries a `.sys`
  file; the installer itself is an ordinary program and is not flagged.
- **Universal files and Java.** Java class files share the universal Mach-O
  magic number and store their version where the architecture count lives. A
  count of 45 or more that looks like a class-file version is reported as a Java
  class file (`unknown`); any other count above 44 is refused.
- **Bundles.** The main program is named by `CFBundleExecutable`; failing that,
  by convention (`Contents/MacOS/<bundle name>`), or as the only file in
  `Contents/MacOS`. The report says which. A binary `Info.plist` is reported as
  not parsed. A program name that is not a plain file name is ignored, and
  symbolic links inside a bundle are not followed, so nothing outside the bundle
  is read.
- **APK ABIs.** An APK whose only native libraries are ARM needs instruction
  translation on an x86-64 host. An APK with no `lib/` entries has no native code
  and runs on any processor.

### Hostile input

Inputs are untrusted downloads. The rules the code follows:

- Every read goes through one bounded reader. A range that is not wholly inside
  the file is refused before any I/O.
- Arithmetic on values taken from the file is checked. Nothing indexes a buffer
  with an unchecked value.
- No field can cause a large allocation: a buffer is only allocated after its
  range has been checked against the file size and a cap.
- Nothing is decompressed, so there is no decompression bomb to defend against.
- Strings from the file that reach a report are refused if they hold control
  characters, so a file cannot inject terminal escape sequences.
- A device, pipe or socket is refused before it is opened.

| Limit | Value |
|---|---|
| Bytes read from one file | 32 MiB in total, 16 MiB per read |
| ELF program headers examined | 512 |
| ELF interpreter path | 4096 bytes |
| Mach-O universal architectures | 44 |
| Zip entries | 100 000 |
| Zip central directory | 16 MiB |
| `Info.plist` | 1 MiB |
| Application profile | 1 MiB |

The limits are constants in `services/tur-runtime/src/limits.rs`.

## Recommendation

`recommend(&Inspection, &HostFacts)` returns a `Recommendation`: a runtime, a list
of machine-readable reasons, the components that are missing, a stable error code
when the outcome is not plain success, and a plain-language explanation.
`HostFacts` holds the host architecture and which components are installed: Wine,
Darling, the Android runtime, ARM translation, the Windows VM backend.

| Input | Recommendation | Reason code |
|---|---|---|
| ELF program for the host architecture | Native Linux | `elf-matches-host` |
| ELF program for another architecture | Unsupported | `elf-foreign-architecture` |
| 32-bit x86 ELF program on an x86-64 host | Unsupported: the 32-bit libraries are not provided | `elf-needs-multilib` |
| ELF program for another operating system | Unsupported | `elf-foreign-os` |
| ELF shared object, object file or core dump | Unsupported: not a program | `elf-not-a-program` |
| Script with an interpreter line | Native Linux | `script-interpreter` |
| PE, graphical or console, x86-64 or i386 | Wine | `pe-windows-application` (and `pe-dotnet`) |
| PE, native subsystem | Offer a full Windows VM: requires a Windows kernel | `pe-requires-windows-kernel` |
| PE for ARM64 or another architecture | Unsupported | `pe-foreign-architecture` |
| PE, UEFI application, driver or ROM | Unsupported | `pe-efi-image` |
| PE DLL | Unsupported: not a program | `pe-library` |
| PE, any other subsystem | Unsupported | `pe-unsupported-subsystem` |
| MS-DOS, NE, LE or LX program | Unsupported | `legacy-executable` |
| Windows Installer package | Wine; the target architecture is not read | `msi-installer` |
| Mach-O program with x86-64 code, thin or universal | Darling, experimental | `mach-o-runnable-slice`, `darling-experimental` |
| Mach-O program for ARM64 only | Unsupported (§23, question 7) | `mach-o-apple-silicon-only` |
| Mach-O program for another architecture | Unsupported | `mach-o-no-runnable-slice` |
| Mach-O kernel extension | Unsupported: macOS kernel extensions cannot be loaded | `mach-o-kernel-extension` |
| Mach-O library, plug-in or object file | Unsupported: not a program | `mach-o-not-a-program` |
| `.app` bundle | As for its main program | `mac-app-bundle` plus the above |
| APK with code for the host, or with no native code | Android runtime | `android-native-code-matches-host`, `android-no-native-code` |
| APK with ARM-only native code on x86-64 | Android runtime; needs ARM translation | `android-needs-arm-translation` |
| XAPK, APKS | Android runtime; the packages inside were not opened | `android-nested-packages-not-inspected` |
| AAB | Unsupported: a publishing format, not installable | `android-app-bundle-not-installable` |
| Plain zip, other OLE file, unknown | Unsupported | `archive-not-an-application`, `ole-not-an-application`, `unrecognised-format` |

When the recommended runtime, or a component it needs, is not installed, the
recommendation keeps the runtime, lists what is missing and adds
`runtime-not-installed`. Wine being installed does not change the answer for a
kernel driver: only the VM backend counts.

Host detection in the command-line tool looks for a launcher in `PATH` (`wine` or
`wine64`, `darling`, `waydroid`, `qemu-system-x86_64` together with `/dev/kvm`)
and executes nothing. The Android probe is provisional until the Android runtime
design is settled. ARM translation is never detected. `--host-arch` and
`--available` replace detection.

### Error codes

Conditions that are the runtime manager's own use its family, `RUNTIME`, with a
`MANAGER` or `PROFILE` topic. Ecosystem-specific conditions use `WINDOWS`,
`DARWIN` and `ANDROID`; refusals that keep an application inside its sandbox
use `SECURITY`. A test checks that every code parses, that no two conditions
share one, and which family each group is in.

Until `tur-core` had a `RUNTIME` family, the first group was parked under
`DESKTOP`. `TUR-DESKTOP-RUNTIME-00N` is now `TUR-RUNTIME-MANAGER-00N` and
`TUR-DESKTOP-PROFILE-00N` is now `TUR-RUNTIME-PROFILE-00N`; the numbers are
unchanged. Those ten strings were used by nothing outside this component and
its tests. The `WINDOWS` and `SECURITY` codes did not move: they are in the
families they belong to, and the Windows end-to-end test
(`tests/windows-runtime/e2e.py`) reads eight of them.

| Code | Condition |
|---|---|
| `TUR-RUNTIME-MANAGER-001` | The path could not be read |
| `TUR-RUNTIME-MANAGER-002` | An ELF, zip, OLE or script file is truncated or malformed |
| `TUR-RUNTIME-MANAGER-003` | The file exceeds an inspection limit |
| `TUR-RUNTIME-MANAGER-004` | No supported runtime exists for the file |
| `TUR-RUNTIME-MANAGER-005` | The runtime manager's data or state directory cannot be found or used |
| `TUR-RUNTIME-MANAGER-006` | A program the operation needs (`tar`, `unzip`, `install`) is not installed |
| `TUR-WINDOWS-INSPECT-001` | A PE file is truncated or malformed |
| `TUR-WINDOWS-RUNTIME-001` | Wine is not installed |
| `TUR-WINDOWS-RUNTIME-002` | The file requires a Windows kernel |
| `TUR-WINDOWS-RUNTIME-003` | The Windows VM backend is not installed |
| `TUR-WINDOWS-RUNTIME-004` | A Windows file no runtime handles |
| `TUR-WINDOWS-RUNTIME-005` | The profile pins a runtime version and another one is installed |
| `TUR-WINDOWS-RUNTIME-006` | The runtime's version could not be determined |
| `TUR-WINDOWS-RUNTIME-007` | `umu-run` is not installed |
| `TUR-WINDOWS-RUNTIME-008` | A Proton profile does not pin a Proton version |
| `TUR-WINDOWS-RUNTIME-009` | The profile is not one this build launches |
| `TUR-WINDOWS-RUNTIME-010` | A step of an operation failed while it was carried out |
| `TUR-WINDOWS-PREFIX-001` | The application has no prefix |
| `TUR-WINDOWS-PREFIX-002` | The application already has a prefix |
| `TUR-WINDOWS-PREFIX-003` | The prefix metadata is unreadable or not valid |
| `TUR-WINDOWS-PREFIX-004` | Verification found problems in the prefix |
| `TUR-WINDOWS-PREFIX-005` | The prefix belongs to another application, prefix name or runtime |
| `TUR-WINDOWS-PREFIX-006` | The prefix is on another runtime version than the one installed |
| `TUR-WINDOWS-PREFIX-007` | The prefix is pinned and a migration was asked for |
| `TUR-WINDOWS-PREFIX-008` | The named backup does not exist |
| `TUR-WINDOWS-PREFIX-009` | Case-insensitive file names were asked for and the file system holding the prefix cannot provide them (reported with the result; the prefix is created case-sensitive) |
| `TUR-WINDOWS-PREFIX-010` | Case-insensitive file names were asked for and drive C: could not be given them for another reason: not permitted, not empty, read-only, `chattr` missing (reported with the result; the prefix is created case-sensitive) |
| `TUR-WINDOWS-APP-001` | The file is not a Windows installer, program or archive of programs |
| `TUR-WINDOWS-APP-002` | The file's kind has no known unattended mode, so it is not run |
| `TUR-WINDOWS-APP-003` | The installer exited with a failure status |
| `TUR-WINDOWS-APP-004` | The installer did not finish in the time it was given and was stopped |
| `TUR-WINDOWS-APP-005` | The installer finished and no new program was found in the prefix |
| `TUR-WINDOWS-APP-006` | No program could be chosen as the application's main program |
| `TUR-WINDOWS-APP-007` | No application with this identifier is installed |
| `TUR-WINDOWS-APP-008` | The application record is unreadable, not valid, or another application's |
| `TUR-WINDOWS-APP-009` | Something of the application was still there after it was removed |
| `TUR-WINDOWS-APP-010` | A value given for an installation (name, time limit) is not allowed |
| `TUR-WINDOWS-APP-011` | The application's own uninstaller failed or ran out of time (reported with the result; the application is removed all the same) |
| `TUR-WINDOWS-AUTOPILOT-001` to `-009` | The compatibility autopilot's conditions: no remedy found, a Microsoft component TUR cannot supply, a result missing or not valid, nothing to apply, a check file not valid, the autopilot unavailable here, remedies already applied or none to revert ([compat-autopilot](compat-autopilot.md#12-command-line-and-codes)) |
| `TUR-WINDOWS-COMPAT-001` to `-012` | Failure classes, not errors of this program: what the autopilot read from a run of an application, from `prefix-unusable` to `check-failed` ([compat-autopilot](compat-autopilot.md#3-failure-detection)) |
| `TUR-SECURITY-SANDBOX-001` | The effective user is root |
| `TUR-SECURITY-SANDBOX-002` | bubblewrap is not installed |
| `TUR-SECURITY-SANDBOX-003` | A directory cannot be granted to an application |
| `TUR-SECURITY-SANDBOX-004` | A recorded grant no longer resolves to itself, or no longer passes the rules |
| `TUR-SECURITY-SANDBOX-005` | An extra environment variable is not allowed |
| `TUR-SECURITY-SANDBOX-006` | The profile's program file cannot be used |
| `TUR-DARWIN-INSPECT-001` | A Mach-O file or bundle is truncated or malformed |
| `TUR-DARWIN-RUNTIME-001` | Darling is not installed |
| `TUR-DARWIN-RUNTIME-002` | A macOS kernel extension |
| `TUR-DARWIN-RUNTIME-003` | A macOS file no runtime handles |
| `TUR-ANDROID-RUNTIME-001` | The Android runtime is not installed |
| `TUR-ANDROID-RUNTIME-002` | ARM translation is not installed |
| `TUR-ANDROID-RUNTIME-003` | An Android package no runtime handles |
| `TUR-RUNTIME-PROFILE-001` | The profile could not be read |
| `TUR-RUNTIME-PROFILE-002` | The profile is not JSON or does not match the schema |
| `TUR-RUNTIME-PROFILE-003` | A profile field holds a value that is not allowed |
| `TUR-RUNTIME-PROFILE-004` | The profile appears to hold a credential |

The application codes have the topic `APP` in the `WINDOWS` family, not
`INSTALL`: `TUR-INSTALL-*` is the family of the operating system's own
installer, and a code about installing 7-Zip should not read like one about
installing TUR.

These are not yet registry entries in the sense of §29.3 (severity, detection
criteria, recovery action); that registry does not exist yet.

## Application profile

Schema version 1, `schemas/app-profile.schema.json` (JSON Schema 2020-12).
Unknown fields are refused.

| Field | Required | Values |
|---|---|---|
| `schema_version` | yes | `1` |
| `app_id` | yes | Lower-case, dot-separated, at least two segments |
| `display_name` | yes | 1 to 200 characters, no control characters |
| `binary` | yes | Absolute path |
| `format` | yes | `elf`, `script`, `pe`, `msi`, `mach-o`, `app-bundle`, `apk`, `apk-set` |
| `architecture` | yes | `x86_64`, `i386`, `aarch64`, `arm`, `universal`, `any` |
| `runtime` | yes | `native`, `wine`, `proton`, `darling`, `android`, `windows-vm` |
| `runtime_channel` | no | `stable`, `testing`, `experimental` |
| `runtime_version` | no | Version token |
| `prefix_id` | for `wine` and `proton` | 1 to 64 lower-case letters, digits, hyphens |
| `graphics_backend` | no | `auto`, `dxvk`, `vkd3d-proton`, `wined3d`, `native` |
| `sync_backend` | no | `auto`, `ntsync`, `esync`, `fsync` |
| `isolation_profile` | yes | As `prefix_id` |
| `known_requirements` | no | Up to 64 unique short tokens |
| `fallback` | yes | `none`, `windows-vm-offer` |
| `test_status` | yes | `untested`, `works`, `partial`, `broken` |

Rules across fields, in the schema and in the validator:

- The runtime must be able to host the format: `elf` and `script` with `native`;
  `pe` and `msi` with `wine`, `proton` or `windows-vm`; `mach-o` and `app-bundle`
  with `darling`; `apk` and `apk-set` with `android`.
- `wine` and `proton` need a `prefix_id`: each application has its own prefix
  (§12.2).
- `windows-vm-offer` is only allowed for Windows formats.

Rules in the validator only:

- **No secrets.** A key that looks like the name of a credential (`password`,
  `token`, `api_key`, `secret` and similar, in any case and punctuation) is
  refused wherever it appears, and so is a value that looks like a private key
  or a URL with a password in it. The error names the key or field and never
  repeats the value.
- `null` is refused; an unused field is left out.

`draft_profile` builds a profile from an inspection and its recommendation. A
draft is marked `untested`, its runtime version is not pinned, and its identifier
comes from the bundle identifier or, failing that, from the file name under
`local.`. No draft is produced for a file with no supported runtime.

Proton is a value in the schema and is never recommended: the specification
reserves it for tested gaming profiles, which need the application database.

## Running a Windows program

Everything from here on is about the `wine` and `proton` runtimes. Nothing is
run because of a file's type: a launch happens when the user asks for one, from
a profile.

The application catalogue ([catalogue and routing](catalogue-and-routing.md))
chooses a route for a known application. Its `wine` and `proton` routes are the
two runtime kinds here, with the same names; its `windows-guest` route is not
handled by this code. Nothing connects the two yet: a launch starts from a
profile, and no code turns a catalogue decision into one.

### Plans

Every operation is a **plan** before it is anything else: a list of steps that
can be printed (`--dry-run`), serialised (`--json`), compared and tested without
touching the system. A plan is carried out by `run_plan` through an `Executor`.
Unit tests use an executor that records steps; `SystemExecutor` is the only code
that creates, writes, deletes or starts anything.

| Step | What the executor does |
|---|---|
| `create-dir` | Creates a directory and its parents, mode 0700 |
| `case-fold` | Creates a directory as `create-dir` does, then runs `chattr +F` on it, outside the sandbox. What `chattr` says is the outcome, recorded in the report; a file system that cannot fold case is not a failure. See [Case-insensitive prefixes](#case-insensitive-prefixes) |
| `write-file` | Writes a file through a temporary file and a rename, mode 0600 |
| `write-bytes` | The same for a file that is not text (an icon). A printed plan shows its size, not its bytes |
| `remove-tree` | Deletes a tree. Refused unless the path lies inside an application's own directory under the runtime manager's roots. A symbolic link is removed, never followed |
| `rename` | Renames; both paths are held to the same rule as `remove-tree` |
| `run` | Runs a command to completion with exactly the planned environment. Any exit status but 0 stops the plan |
| `run-bounded` | As `run`, with a time limit. When the time is up the command is killed, which takes the sandbox and everything in it down, and the plan stops. At most 1 MiB of its output is kept |
| `launch` | Starts the application, copies its output to the log, and returns its exit status as the plan's result |
| `check` | A read-only test. A failed check is a finding; the plan continues |
| `require` | A read-only test the rest of the plan depends on. A failed requirement stops the plan |

The tests a `check` or `require` can make: a path is a directory, is a file,
is a file that contains a text, belongs to the user, or is absent. None follows
a symbolic link.

Planning is a function of the profile, the prefix metadata, the facts about the
host (`LaunchHost`) and the time. It reads the file system to resolve granted
paths; it starts nothing. The one program started outside a plan is `wine
--version`, to learn the installed version; `--runtime-version` replaces that
too. `--dry-run` therefore runs no step and changes nothing.

### Prefix layout

```
$XDG_DATA_HOME/tur/runtime/
    prefixes/<app-id>/metadata.json     written by tur-runtime only
    prefixes/<app-id>/prefix/           the Wine prefix (WINEPREFIX)
    prefixes/<app-id>/home/             the application's private HOME
    prefixes/<app-id>/application.json  the record of an installed application
    prefixes/<app-id>/profile.json      the profile it is launched from
    backups/<app-id>/<time>-<reason>.tar.gz
    staging/<app-id>/                   scratch space while restoring
    launchers/<app-id>/                 the staged launcher entry and its icon
$XDG_STATE_HOME/tur/runtime/
    logs/<app-id>/<time>-<pid>.log      one file per launch
    logs/<app-id>/operations.log        output of prefix operations
```

One application, one prefix (§12.2). The directory is named by the application
identifier, which is validated before it becomes a path. A profile's `prefix_id`
is recorded in the metadata and must match at launch; so must the application
identifier and the runtime. A prefix is never reused for another application.

Only `prefix/` and `home/` are shown to the application. The metadata, the
application record and the profile sit beside them, outside the sandbox, so an
application cannot change its own grants, what is recorded about it, or which
program is started in its name; backups, logs and launcher entries are in
other trees for the same reason.

### Prefix metadata

`schemas/prefix-metadata.schema.json`, schema version 1. Unknown fields are
refused. A unit test keeps the schema and the Rust types in step.

| Field | Meaning |
|---|---|
| `schema_version` | `1` |
| `app_id`, `prefix_id` | The application the prefix belongs to, and the name its profile uses for it |
| `runtime` | `wine` or `proton` |
| `runtime_version` | The version the prefix is on, as a token: `wine-9.0`, `GE-Proton10-20` |
| `runtime_build` | Everything the runtime reported, for example `wine-9.0 (Ubuntu 9.0~repack-4build3)` |
| `pinned` | A pinned prefix is never migrated |
| `architecture` | `win64` |
| `created_at`, `last_run_at`, `launch_count` | Times in UTC to the second; `last_run_at` is the start of the latest launch |
| `graphics_backend` | As in the profile |
| `sync` | What the profile asked for, what was selected, what the `ntsync` probe found, when, and why |
| `network` | Whether the host's network is shared |
| `grants` | Host directories shown to the application, each `read-only` or `read-write` |
| `migrations` | Earlier runtime versions, each with the name of the backup taken first |

Only 64-bit prefixes are created. Wine 11.0 deprecates prefixes created with
`WINEARCH=win32`; a 64-bit prefix hosts 32-bit programs where the Wine build
can. Whether a given 32-bit program runs was not tested.

The metadata is read with the same discipline as an inspected file: at most
256 KiB, at most 64 grants and 32 remembered migrations, every path an absolute
path without `..` or control characters, every version a version token. A
launch takes at most 1024 arguments and 64 extra variables.

### Prefix operations

| Operation | Plan |
|---|---|
| create | Create the directories; with `--casefold` (Wine only), create `drive_c` empty and give it the case-folding attribute; run `wine wineboot --init` (or `umu-run ""`) in the sandbox; require `system.reg`; switch off Wine's own prefix updating; write the metadata. The metadata is written last, so a failed creation leaves no valid prefix behind |
| verify | Checks only: the directories exist and are not links, the prefix belongs to the user, `system.reg` and `user.reg` exist, `drive_c/windows` exists, the registry's `#arch` line matches, Wine's own updating is still off. Exit status 8 when a check fails |
| repair | `wine wineboot --update` with the runtime the prefix is on. Refused when another version is installed: that would be a migration. It restores system files Wine can start without; when Wine cannot start in the prefix at all (a missing `kernel32.dll`, measured in CI), only a restore or a reset helps |
| back up | `tar --create --gzip` of `metadata.json`, `prefix/` and `home/` into `backups/<app-id>/`; and of `application.json` and `profile.json` when the application was installed, so that a restore brings back the application and not only its prefix |
| restore | Unpack into `staging/`; require that the archive's metadata names this application and that it holds a prefix; only then move the current prefix aside and the unpacked one into place. An archive does not keep the case-folding attribute: when the prefix being replaced has a case-insensitive drive C:, the unpacked `drive_c` is created and given the attribute before the archive is unpacked into it |
| reset | Back up (unless `--no-backup`), delete `prefix/` and `home/`, create again with the same settings and grants, and with a case-insensitive drive C: when the old one was |
| remove | Delete the application's directory, staging, logs and staged launcher entry. Backups are kept |
| migrate | See below |
| grant, revoke, pin, unpin | Rewrite the metadata |

### Runtime versions (§15.2)

A runtime upgrade must not change a prefix on its own. Three things make that
true:

1. **Wine's own updating is switched off.** Wine does not record its version in
   a prefix. It stores the modification time of its `wine.inf` in
   `<prefix>/.update-timestamp` and, when a newer Wine finds a different time,
   updates the prefix by itself at the next start. `wineboot` skips that when
   the file holds `disable`; every prefix created here holds it. An explicit
   `wineboot --update` still works.
2. **The launch plan compares versions.** The metadata records the version the
   prefix is on. When the installed runtime reports another, the launch is
   refused (`TUR-WINDOWS-PREFIX-006`) and nothing is started.
3. **Migration is explicit and backed up.** `launch --migrate` or `prefix
   migrate` plans: a backup, then `wineboot --update` with the installed
   runtime, then the new record with the move appended to `migrations`. A
   pinned prefix refuses it (`TUR-WINDOWS-PREFIX-007`).

A profile's `runtime_version` pins the runtime itself: when it is set and
another version is installed, the launch is refused (`TUR-WINDOWS-RUNTIME-005`),
with or without `--migrate`. There is one system Wine; keeping several versions
side by side is not built.

For Proton, `umu-run` fetches the build named by `PROTONPATH`, and without one
it follows the latest build. A Proton profile must therefore pin a version
(`TUR-WINDOWS-RUNTIME-008`), and that version is what the prefix records.

Versions are compared as tokens (`wine-9.0`). A distribution rebuild of the same
upstream version is not a mismatch; the full build string is recorded for the
record.

### The launch command

`plan_launch` produces the program, its arguments, its whole environment and
its working directory, the sandbox they run in, and the steps.

| Runtime | Command inside the sandbox |
|---|---|
| Wine | `wine <program> <arguments>` |
| Proton | `umu-run <program> <arguments>`, with `PROTON_VERB=waitforexitandrun` |

The working directory is the program's directory. Only Windows programs (`pe`)
are launched; an installer package (`msi`) is refused.

For Wine, the first process in the sandbox is not Wine but `tur-runtime
supervise`, mounted read-only into the sandbox: it starts Wine, waits for it,
then runs `wineserver --wait`. The Wine server outlives the program by a few
seconds and writes the registry when it exits, and the sandbox is torn down when
its first process exits; without the supervisor a short-lived program would
lose its registry changes, and a freshly created prefix could be left without a
registry at all. Proton's `waitforexitandrun` verb waits by itself.

### The launch environment

Nothing is inherited. The environment is built from:

| Variables | Source |
|---|---|
| `PATH=/usr/bin:/bin`, `HOME` (the private home), `USER`, `LOGNAME` | Fixed, or the user's login name |
| `LANG`, `LANGUAGE`, `LC_ALL`, `LC_COLLATE`, `LC_CTYPE`, `LC_MESSAGES`, `LC_MONETARY`, `LC_NUMERIC`, `LC_TIME`, `TZ`, `XDG_CURRENT_DESKTOP`, `XDG_SESSION_TYPE`, `DRI_PRIME`, `__NV_PRIME_RENDER_OFFLOAD`, `__GLX_VENDOR_LIBRARY_NAME`, `__VK_LAYER_NV_optimus` | Copied from the caller when set and free of control characters |
| `DISPLAY`, `XAUTHORITY`, `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR` | Rebuilt from the session endpoints that were found and checked (see the sandbox) |
| `WINEPREFIX` | The application's prefix |
| `WINEDEBUG` | `fixme-all` unless `--wine-debug` says otherwise |
| `WINEDLLOVERRIDES` | See below (Wine only) |
| `WINEARCH=win64` | Only while a prefix is created (Wine only) |
| `GAMEID=umu-default`, `STORE=none`, `PROTONPATH=<pinned version>`, `PROTON_VERB=waitforexitandrun` | Proton only. `umu-default` applies no fixes: there is no application database to supply a real identifier |
| `--env NAME=VALUE` | Added by the user to one launch. A name this table sets, and any name starting with `LD_`, is refused |
| Applied remedies | `app launch` only: the variables of remedies the person applied with the autopilot (`prefixes/<app-id>/remedies.json`), held to the same rules; a variable given with `--env` wins. A remedy may also leave `/dev/ntsync` out of the sandbox ([compat-autopilot](compat-autopilot.md#9-applying-and-undoing)) |

The session bus address, the SSH and GnuPG agent variables, tokens and everything
else in the caller's environment are not passed on. Launch logs record the names
of the variables, not their values.

**DLL overrides and graphics (Wine).**

| `graphics_backend` | `WINEDLLOVERRIDES` |
|---|---|
| `auto`, `wined3d`, `native` | `winemenubuilder.exe=` |
| `dxvk` | `d3d8,d3d9,d3d10core,d3d11,dxgi=n,b;winemenubuilder.exe=` |
| `vkd3d-proton` | `d3d12,d3d12core=n,b;winemenubuilder.exe=` |

DXVK's instructions for a plain prefix are to copy its DLLs into the prefix and
set native overrides for `d3d8`, `d3d9`, `d3d10core`, `d3d11` and `dxgi`.
Installing the DLLs is not built, so the override is `n,b`: the translation
layer when it is there, Wine's own implementation when it is not. The same form
is used for VKD3D-Proton's `d3d12.dll` and `d3d12core.dll`; its README names
the two DLLs but gives no prefix instructions, so that override is by analogy
and was not verified. `winemenubuilder.exe` is disabled because desktop entries
come from `tur-runtime`. While a prefix is created, `mscoree` and `mshtml` are
disabled too, so that Wine does not offer to download Mono and Gecko.

**Graphics (Proton).** Proton uses DXVK and VKD3D-Proton by default; nothing is
overridden. `wined3d` sets `PROTON_USE_WINED3D=1`.

**Synchronisation (§8.2).** The device is probed at every launch: `/dev/ntsync`
has to be a character device and has to open. The kernel version is never
consulted. The result, the choice and the reason are written to the metadata
before the application starts.

| Runtime | Asked for | Device opens | Result |
|---|---|---|---|
| Wine 11.0 or later | `auto`, `ntsync` | yes | `ntsync`: the device is passed into the sandbox; Wine uses it when it is available, no variable is needed |
| Wine before 11.0 | `auto`, `ntsync` | yes | `server`: this Wine cannot use it |
| Wine | `auto`, `ntsync` | no | `server`, with the reason. The launch does not fail |
| Wine | `esync`, `fsync` | any | `server`: neither is part of upstream Wine |
| Proton | `auto` | any | `runtime-default`: left to the Proton build |
| Proton | `ntsync` | yes | `ntsync`, with `PROTON_USE_NTSYNC=1` |
| Proton | `ntsync` | no | `runtime-default`, with the reason |
| Proton | `esync` | any | `esync`, with `PROTON_NO_FSYNC=1` |
| Proton | `fsync` | any | `fsync`: Proton's default, nothing set |

`server` is synchronisation through the Wine server, which every Wine has.

### The sandbox

Every runtime command, including prefix creation, runs inside bubblewrap. There
is no option to start an application without it; when `bwrap` is missing the
operation is refused (`TUR-SECURITY-SANDBOX-002`). Root is refused before
anything else is looked at (`TUR-SECURITY-SANDBOX-001`, §12.1).

What the application sees:

| Path | How |
|---|---|
| `/usr`, `/etc` | Read-only |
| `/bin`, `/lib`, `/lib64`, ... | Recreated as the links they are on a merged-`/usr` system, or mounted read-only where they are directories |
| `/proc` | A new one, showing the sandbox's own processes |
| `/dev` | A minimal one, plus `/dev/dri` and the NVIDIA device nodes when they exist, plus `/dev/ntsync` when the probe opened it |
| `/sys/block`, `/sys/bus`, `/sys/class`, `/sys/dev`, `/sys/devices` | Read-only, when they exist |
| `/tmp` | A private, empty in-memory file system |
| `XDG_RUNTIME_DIR` | A private, empty directory with these sockets mounted read-only, each on its own: the Wayland display, `pipewire-0`, `pulse/native` |
| `/tmp/.X11-unix/X<n>` | The socket of a local X display, read-only |
| The X authority file | Read-only, at a fixed path, only when it is `$HOME/.Xauthority` or a session file directly in the runtime directory |
| `/run/systemd/resolve` | Read-only, when the network is shared |
| The prefix and the private home | Read-write |
| The program file, when it is outside the prefix; an installer file while it is installed | That one file, read-only. Never its directory |
| Granted directories | Read-only or read-write, as granted |
| `/` | An in-memory file system, sealed read-only once everything is mounted |

What it does not see: the home directory, the session bus, the keyring, the SSH
and GnuPG agents and directories, browser and mail profiles, any other
application's prefix, its own metadata, backups and logs, `/var`, `/run`.

Namespaces: user, IPC, PID, UTS and, where possible, cgroup. The network
namespace is shared unless the prefix was created with `--no-network`. For Wine,
the sandbox may not create further user namespaces (`--disable-userns`); for
Proton it may, because `umu-run` starts Proton in the Steam Linux Runtime
container. The sandbox dies with `tur-runtime` (`--die-with-parent`) and has no
controlling terminal (`--new-session`); the application's standard input is
`/dev/null`.

**Grants.** The user grants a directory with `prefix create --grant[-rw]` or
`prefix grant`. The rules are applied to the path with every symbolic link
resolved, because that is what is mounted, and the resolved path is what is
recorded:

- `/`, the home directory, and anything that contains the home directory are
  refused.
- A protected location, anything inside one and anything that contains one are
  refused: `.ssh`, `.gnupg`, `.pki`, the password store, the keyring and KDE
  wallet directories, browser and mail profile directories, Flatpak and snap
  application data, cloud and registry credentials, the runtime manager's own
  data and state (every application's prefix), the user's runtime directory,
  and the SSH agent socket.
- Every hidden entry of the home directory is refused, named above or not.
  Credentials and application profiles live there by convention and no list is
  complete.
- System directories and kernel file systems are refused, and so are `/tmp` and
  `/var/tmp` as a whole.
- A path with a `.` or `..` component, a relative path, a path that does not
  exist and a path that is not a directory are refused.

Every recorded grant is checked again when a launch is planned, against the file
system as it then is, and must still resolve to itself. An application with
write access to one granted directory therefore cannot turn a second grant
nested in it into a link to somewhere else: the next launch is refused
(`TUR-SECURITY-SANDBOX-004`). Metadata edited by hand to name a protected
directory is refused the same way. A symbolic link *inside* a granted directory
needs no rule: its target is resolved inside the sandbox, where the rest of the
home directory does not exist.

Session values are treated the same way. `WAYLAND_DISPLAY` is accepted only as a
plain file name, although the protocol also allows a path; `DISPLAY` only as a
local display; `XAUTHORITY` only in the two places a session puts it. The
environment cannot name an arbitrary file to be mounted.

**Threat model.** A Windows program under Wine is a Linux process: it can call
Linux directly and can read and write whatever its user can. The sandbox is what
stands between a malicious or compromised Windows program and the user's files.
It is designed to stop that program from reading, encrypting or deleting
anything outside its own prefix, its private home and the directories the user
granted, and from reaching the user's credentials.

It does not protect against:

- **What the user granted.** A read-write grant can be encrypted or emptied.
  Grants are the user's decision; they are narrow by rule and read-only unless
  asked otherwise.
- **X11.** Any X11 client can read the keyboard and the contents of other
  windows and can send input to them. An application given the X socket has that
  power. A Wayland-only path would not; evaluating Wine's Wayland driver is not
  done.
- **The network.** Shared by default: an application can reach the local
  network and the internet. `--no-network` removes it.
- **Kernel vulnerabilities and the GPU driver.** There is no seccomp filter:
  the whole system-call surface is reachable, and so are the GPU device nodes.
  A seccomp filter and Landlock (§12.1) are not built.
- **Resource exhaustion.** No limits on memory, processes or disk are set.
- **The user's own account being hostile.** Another process of the same user,
  outside the sandbox, can change the prefix or the metadata.
- **Proton.** `umu-run` and the Steam Linux Runtime run inside the sandbox with
  nested user namespaces allowed; that combination has only been planned, not
  run.

For untrusted Windows software a guest is the stronger boundary (ADR-0007).

The sandbox is tested as a boundary, not as a list of options. Unit tests build
a scratch home with keys, browser profiles, a keyring and another application's
prefix; put over a hundred grant requests to it, among them `/`, the home
directory, paths with `..`, and links that lead to protected places; and for
every sandbox that is built read the bubblewrap arguments back and assert that
no mounted host path is, contains or lies inside a secret, that nothing from the
runtime directory is mounted but the session's own sockets, and that the only
writable paths are the prefix, the private home and read-write grants. The same
requests are also written straight into metadata, bypassing admission. The
`windows-runtime` workflow then runs a real Windows program inside the real
sandbox and checks what it could and could not read.

### Logs

One file per launch, `logs/<app-id>/<UTC time>-<pid>.log`, mode 0600: a header
with the command and the names of the environment variables, the application's
standard output and standard error as they came, and the exit status. A launch
log is capped at 64 MiB; later output is dropped and the log says so. In text
mode the application's output is also `tur-runtime`'s own output; with `--json`
it goes to the log only and standard output carries the report.

### Desktop entries

`tur-runtime desktop-entry <profile>` prints the text of a `.desktop` file and
installs nothing:

```
[Desktop Entry]
Version=1.5
Type=Application
Name=Example Application
Comment=Windows application (Wine)
Exec=tur-runtime launch "/path/to/profile.json"
Icon=application-x-ms-dos-executable
Terminal=false
StartupNotify=true
X-TUR-App-Id=com.example.app
X-TUR-Runtime=wine
X-TUR-Prefix-Id=example-app
```

`X-TUR-Runtime` and `X-TUR-App-Id` tell the desktop which runtime an entry
belongs to (§3.4); the file is named `tur-<runtime>-<app-id>.desktop`. The entry
goes through `tur-runtime launch`, so a click gets the same sandbox and the same
refusals as the command line. Values are escaped by the Desktop Entry
Specification's rules: the name cannot add a line or a key, and the profile path
is always one quoted argument.

For an installed application the entry also carries `Icon=<file>`, the
absolute path of a PNG taken from the program, and `StartupWMClass=<name>`, the
program's file name in lower case, which is what Wine's X11 driver sets as the
class of the application's windows. See [the launcher
entry](#the-launcher-entry). Categories and file associations are not built.

## Installing a Windows application

`tur-runtime app install <file>` puts an application into a prefix of its own
and makes it startable; `app uninstall <app-id>` removes it. Like everything
else here, each part is a plan first and runs inside the sandbox.

Nothing is fetched. The file is one the user already has. No dependency is
installed for an application: no Microsoft component, no redistributable
runtime, no winetricks verb, no Mono, no Gecko. An application that needs one
fails to install or to start, and that is what is reported.

### What kind of installer a file is

`tur-runtime installer identify <file>` and the first step of `app install`
decide the kind from the file's content. The name and extension are not looked
at: a file called `notes.txt` that is an NSIS installer is an NSIS installer.

| Kind | Recognised by | Unattended mode | Where that is documented |
|---|---|---|---|
| `msi`, a Windows Installer package | The OLE compound-file magic and the root storage class of a package | `msiexec /i <package> /qn` | Microsoft's `msiexec` reference; Wine's `msiexec.c` for what Wine implements |
| `nsis` | The NSIS first header, `0xDEADBEEF` then `NullsoftInst`, in the first 64 KiB of the data appended to the program | `/S` | NSIS manual, section 3.2 |
| `inno-setup` | The Inno Setup loader's offset table, `rDlPtS`, inside the program image | `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-` | Inno Setup help, "Setup Command Line Parameters" |
| `seven-zip-installer`, the installer 7-Zip ships for itself | A 7z archive appended to a program whose version resource calls it `7-Zip Installer` | `/S` | 7-Zip FAQ |
| `portable-zip` | A zip archive with at least one member named `*.exe` | None: it is unpacked, not run | |
| `plain-exe` | A Windows program for x86-64 or i386 with none of the above | None | |

"Appended data" is what follows the last section of a PE image: the layout is
read from the section table, so a signature in the program's own code or
resources is not mistaken for a payload. A 7z archive appended to any other
program (a self-extracting archive) is a `plain-exe` with a note: no unattended
mode is documented for those in general.

Not installable (`TUR-WINDOWS-APP-001`): a Windows Installer patch or
transform, a DLL, a kernel-mode image, a program for another processor
architecture, MS-DOS and 16-bit programs, a zip archive without a program, and
everything that is not Windows software.

The identification also says which architecture the installer program itself
is. Most installers are 32-bit programs whatever they install: Notepad++ and
VLC ship 64-bit applications in 32-bit NSIS installers. Wine must have its
32-bit half to run those.

A `plain-exe` is not run as an installer: nothing is known about what it would
ask. `--portable` is the user's statement that the file is the application
itself; it is then copied into the prefix. Without it the installation is
refused (`TUR-WINDOWS-APP-002`). The statement is only taken for a file that
could be one: `--portable` with a file whose content says it is an installer
or an archive is refused (`TUR-WINDOWS-APP-010`). Running an installer of an
unknown kind with its windows shown, for the user to click through, is not
built.

### The three plans

An installation is three plans carried out in order. The third cannot be made
before the second has run, because what an installer installs is only known
afterwards. `--dry-run` therefore prints the first two and says so.

1. **Create a fresh prefix** for the application, exactly as `prefix create`
   does. An application that already has a prefix is refused
   (`TUR-WINDOWS-PREFIX-002`): nothing is installed over anything.
2. **Put the application into the prefix**, as one `run-bounded` step inside
   the sandbox:

   | Kind | Command inside the sandbox |
   |---|---|
   | `msi` | `wine msiexec /i Z:\<path of the package> /qn` |
   | `nsis`, `seven-zip-installer` | `wine <installer> /S` |
   | `inno-setup` | `wine <installer> /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-` |
   | `portable-zip` | `unzip -q -o <archive> -d <prefix>/drive_c/Program Files/<prefix-id>` |
   | `plain-exe` with `--portable` | `install -D -m 0644 -- <file> <prefix>/drive_c/Program Files/<prefix-id>/<name>.exe` |

   The sandbox is the one every launch gets, with no granted directory and
   one addition: the installer file itself, read-only, at its own path. Its
   directory is not shown. The installer reaches that file through the
   prefix's `Z:` drive, which Wine maps to the root of what the sandbox
   shows; so does every program started from outside its prefix. A prefix
   without that drive, which is a hardening step some recommend, would need
   the file handed over another way. `unzip` and `install` run inside the sandbox too,
   without Wine: an archive is untrusted, and whatever a hostile one tries
   (paths that climb out, links that point elsewhere) lands in a file system
   where only the prefix is writable. Nothing in a portable archive or a
   copied program is run during installation.

   The time limit is 15 minutes unless `--timeout` says otherwise, at most a
   day. An unattended installer that stops to ask something would otherwise
   wait for ever on a display nobody is watching. A non-zero exit status is
   `TUR-WINDOWS-APP-003`, running out of time `TUR-WINDOWS-APP-004`.
3. **Record what was installed**: the profile, the icon, the launcher entry
   and, last, the application record. An application has a record only when
   everything else is in place.

A failed installation is rolled back: the prefix and anything staged are
deleted, and the log in `logs/<app-id>/operations.log` is kept, because it says
what went wrong. `--keep-failed` keeps the prefix for a look; `prefix remove`
then deletes it.

### Finding what was installed

The installer is not asked what it did. The prefix is looked at before and
after it runs, and three things are compared:

- **Programs**: every `*.exe` under the prefix's `drive_c`.
- **Shortcuts**: every `*.lnk` there. With Wine's own menu builder switched off
  (see the DLL overrides), an installer's Start-menu and desktop shortcuts
  still exist as shortcut files inside the prefix. Each new one is parsed (the
  Shell Link format: target path, arguments, description) and its target is
  matched, without regard to case, against the new programs.
- **Uninstall entries**: the keys under
  `Software\Microsoft\Windows\CurrentVersion\Uninstall`, and the same under
  `Wow6432Node`, in `system.reg` and `user.reg`: name, version, publisher,
  uninstall command, install location, icon.

The walk follows no symbolic link and lists none, so nothing outside the
prefix is read whatever the installer left there; a `drive_c` that is itself a
link is not walked at all. It stops at 200 000 entries or 24 levels. Registry
files are read up to 64 MiB, shortcut files up to 64 KiB. Every string that is
kept is stripped of control characters and bounded.

### Choosing the main program

The launcher entry starts one program. Programs under `windows/`, programs in
a `Temp` directory and uninstallers (`unins*`, `*uninstall*`, `Au_.exe`) are
never candidates. Among the rest, the first rule that singles out one program
decides, and the record says which rule it was:

1. The program the user named with `--main-exe`, by file name or by the end of
   its path. It must name exactly one new program.
2. The one program that Start-menu and desktop shortcuts point to.
3. Among several such programs: the one that is also the icon of an uninstall
   entry; failing that, the one whose shortcut is named after the application
   as its uninstall entry names it ("PuTTY" for "PuTTY release 0.85", and not
   "PuTTYgen").
4. With no shortcut to a program: the icon of an uninstall entry.
5. The only new program.
6. The only new program nearest the top of the drive (an unpacked archive's
   program, not the updater in its subdirectory).

When no rule does, the installation fails with `TUR-WINDOWS-APP-006` and lists
the candidates; the user names one. Nothing is guessed. An installer that
installs no program at all is `TUR-WINDOWS-APP-005`. The chosen file must be a
regular file that resolves to itself inside the prefix and a Windows program
for x86-64 or i386.

### The application record

`prefixes/<app-id>/application.json`, schema version 1,
`schemas/app-record.schema.json`. Unknown fields are refused, and a unit test
keeps the schema and the Rust types in step. It is read back with a 256 KiB
cap and must name the application, the prefix and the profile of the
directory it is in: a record cannot start something else in an application's
name.

| Field | Meaning |
|---|---|
| `schema_version` | `1` |
| `app_id` | The application identifier: `--app-id`, or `local.<name of the file>` |
| `name`, `version`, `publisher` | In this order of preference: what the user gave (`--name`); what the installer registered in its uninstall entry; what the main program's version resource says; the shortcut's name; the program's file name. Free text, as the application states it |
| `main_executable` | The program the launcher entry starts, as a path on this system, inside the prefix |
| `main_executable_windows` | The same program as a Windows path |
| `main_executable_reason` | The rule that chose it |
| `prefix`, `prefix_id` | The prefix and the name the profile uses for it |
| `runtime`, `runtime_version` | What the application was installed with |
| `installed_at` | When, in UTC |
| `case_folding` | Whether drive C: is case-insensitive: `requested`, `casefolded`, a `reason` (`casefolded`, `not-requested`, `no-casefold-feature`, `no-attributes`, `not-permitted`, `not-empty`, `read-only`, `tool-missing`, `failed`) and, when `chattr` refused, its message as `detail`. Optional: records written before it existed do not have it |
| `install` | The installer's `kind`; the `method` (`unattended-installer`, `unpacked`, `copied`); the `arguments` the installer was given; and the `source`: the file's name, size and SHA-256 |
| `uninstall` | The `method` (`uninstaller`, `msi-product`, `remove-prefix`), the `program` (a Windows path on `C:`, or a product code), its `arguments`, and the `registry_key` they came from |
| `launcher` | The staged `desktop_entry` and `icon` files, and `installed: false` |
| `profile` | The application profile the launcher entry launches from |
| `executables`, `shortcuts` | What the installer added: programs as Windows paths, and Start-menu and desktop shortcuts with their targets. At most 64 of each |

Where the installer file came from is not recorded: `tur-runtime` is given a
file, not a URL. The digest is what lets a record be matched to a publisher's
checksum.

The profile written beside the record is an ordinary application profile:
format `pe`, runtime `wine`, the prefix name, `test_status: untested`. Its
runtime version is not pinned: the prefix records the version it is on and a
launch refuses another one until the prefix is migrated.

### The launcher entry

Staged in this slice, not installed:

```
$XDG_DATA_HOME/tur/runtime/launchers/<app-id>/tur-wine-<app-id>.desktop
$XDG_DATA_HOME/tur/runtime/launchers/<app-id>/tur-wine-<app-id>.png
```

On a system whose desktop this is integrated with, the entry would be
`$XDG_DATA_HOME/applications/tur-wine-<app-id>.desktop` and the icon
`$XDG_DATA_HOME/icons/hicolor/<size>x<size>/apps/tur-wine-<app-id>.png`, named
in the entry as `Icon=tur-wine-<app-id>`. That step is not taken here because
it belongs to the desktop bridge (§9.3), which also has to refresh the
desktop's own caches and to remove the entry reliably; `app install` prints
both destinations and the record says `installed: false`.

The icon is taken from the main program: the first icon group of its
resources, and of that the largest image and, among equals, the deepest. An
image that is already a PNG is copied as it is. Any other is a
device-independent bitmap with a transparency mask (1, 4, 8, 24 or 32 bits per
pixel) and is written out as a PNG with eight bits per channel and alpha. The
PNG stores its pixels uncompressed: an icon is at most 256 by 256 pixels, and
no compressor is a dependency this way. A program without an icon that can be
read gets the icon theme's generic one. The program file is untrusted input:
sizes are capped (2 MiB per image), every index is checked, and a PNG taken
from a program is copied, never decoded.

That last point has a cost. An icon that is a PNG inside the program reaches
the desktop's image decoder exactly as the application supplied it, and the
desktop is outside the sandbox. Every packaging format that lets an
application bring its own icon has this exposure. Closing it means decoding
the PNG here and writing the pixels out again, which needs a PNG decoder in
this component; that is not built. The Windows guest design
([windows-guest.md](windows-guest.md), 6.7) says of its own launcher entries
that "icons are decoded and re-encoded on the host"; this component does that
for bitmaps and not yet for PNGs, and the two should end up the same.

Two more things that design and the desktop design ask of a launcher entry
are not in this one: the "About this application" action that opens the
application's page in the application centre
([desktop-experience.md](desktop-experience.md), 7.3), and a `%F` field code
with the file types the application handles. Both belong with installing the
entry.

An extracted icon is the publisher's work. It is written beside the launcher
entry on the user's machine and nowhere else: the test in CI keeps its
dimensions and digest, not the file.

`tur-runtime app launch <app-id>` is `tur-runtime launch` with the
application's profile.

### Removing an application

`tur-runtime app uninstall <app-id>` is three plans:

1. **The application's own uninstaller**, inside the sandbox, with the same
   time limit as an installer:

   | How it was installed | Command inside the sandbox |
   |---|---|
   | Windows Installer product (the uninstall entry is flagged as one, or its command is `MsiExec.exe`, and its key is a product code) | `wine msiexec /x {product code} /qn` |
   | NSIS, 7-Zip's installer | `wine <UninstallString's program> <its arguments> /S` |
   | Inno Setup | `wine <UninstallString's program> <its arguments> /VERYSILENT /SUPPRESSMSGBOXES /NORESTART` |
   | Unpacked or copied | Nothing to run |

   The program must be on the prefix's `C:` drive, without `..`; a record
   edited to run something else is refused (`TUR-WINDOWS-APP-008`).
2. **Delete everything kept for the application**: its directory (prefix,
   private home, metadata, record, profile), staging, logs and launcher
   entry.
3. **Check that each of those paths is gone.** Anything still there is a
   finding and exit status 8 (`TUR-WINDOWS-APP-009`).

The uninstaller is a courtesy, not the mechanism. An application lives
entirely in its own directory, because the sandbox gave it nowhere else to
write; deleting that directory is what removes it. So a failing uninstaller
does not stop the removal: it is reported (`TUR-WINDOWS-APP-011`, with its exit
status and what it left in the prefix) and the removal goes on.
`--skip-uninstaller` leaves it out.

Two things the owner may see differently:

- **Documents go with the application.** Whatever the application saved in its
  private home (its "My Documents") is inside its directory and is deleted
  with it. Files in granted directories are the user's own and are untouched.
- **Backups are kept**, as `prefix remove` keeps them, and the result names
  them. `--remove-backups` deletes them too.

### Decisions where the specification is silent

The specification names what the runtime manager must do (§7.2: "detect
bundle/package/install formats", "register application desktop entries", "track
runtime version and app version") and not how. Decided here:

| Question | Decision |
|---|---|
| Which installer kinds, and how identified | Six, above, by content only |
| What "unattended" means | Each kind's own documented switch. No kind is run with its windows |
| A program of no known installer kind | Refused unless the user says it is the application itself |
| Where the record lives, and its name | `application.json` beside the prefix, outside the sandbox |
| The application identifier | Given, or `local.<file name>`; it names the prefix directory before anything is installed, so it cannot come from what the installer registers |
| Name, version, publisher | The installer's own registration first, then the program's version resource |
| The main program | Six rules; no guess when they do not decide |
| Error codes | `TUR-WINDOWS-APP-*`, not `TUR-INSTALL-*` |
| The launcher entry | Staged; destinations stated |
| The icon | PNG, from the main program's first icon group |
| A failed installation | Rolled back; the log is kept |
| Removal when the uninstaller fails | Goes on; the failure is reported |
| Backups at removal | Kept |
| An installer's exit status | Only 0 is success. `msiexec` returns 3010 when a restart is wanted; that is treated as a failure, and was not met |

## Case-insensitive prefixes

Windows finds `File.txt` when a program asks for `FILE.TXT`; a Linux file
system compares names byte for byte. Wine bridges the two in user space, and
ext4 can do it in the kernel. This section says how `tur-runtime` uses the
kernel's way, what that changes, and what was measured
([third report](../reports/windows-runtime-3.md), issue #81).

### What Wine does, and what changes on a case-folded directory

Read in Wine's source (`dlls/ntdll/unix/file.c`, Wine 9.0 and master, the
GitHub mirror):

- **Lookup** (`find_file_in_dir`). Wine first asks the kernel for the name
  exactly as given (`stat`, `fstatat` in master); if that finds it, nothing
  else happens. Otherwise it reads the whole directory and compares every
  entry with the name, without regard to case (`wcsnicmp`). On a directory
  of 5 000 files that is 5 000 comparisons for every name asked for in
  another case, and for every name that is not there.
- **Detection** (`get_dir_case_sensitivity_stat`). On Linux, Wine opens the
  directory and asks for its inode flags (`EXT2_IOC_GETFLAGS`); when
  `EXT4_CASEFOLD_FL` is set it treats the directory as case-insensitive. The
  answer is not cached: every check is an open, an ioctl and a close. Added
  in Wine 4.11 (commit 0639d182e54c, "ntdll: Add support for EXT4 per
  directory case folding", 2019).
- **On a case-folded directory**: the first `stat` already finds the name in
  any case, so the directory scan does not happen. A name that is not there
  skips the scan only when it is not an 8.3 name (`if (!is_name_8_dot_3 &&
  !get_dir_case_sensitivity(...)) goto not_found;`): `README.TXT` still
  causes a scan when it is missing, because it could be the short name of
  another file. Measured, a missing long name was nevertheless slower on a
  case-folded directory than on an ordinary one, and its cost grew with the
  size of the directory; where that time goes was not established (see the
  report).
- **One thing gets slower**: a directory query for one name without
  wildcards (`FindFirstFile("C:\\dir\\name.txt")`) is answered with one `stat`
  on an ordinary directory, and with a full read of the directory on a
  case-folded one (`read_directory_data_stat`: "if the file system is not
  case sensitive we can't find the actual name through stat()").

### What the kernel does

From the kernel's own source and documentation (torvalds/linux master, which
was 7.4-rc2 on 13 November 2026):

- The feature is per file system (`casefold`, `EXT4_FEATURE_INCOMPAT_CASEFOLD`,
  made with `mkfs.ext4 -O casefold`, encoding UTF-8 12.1, optionally
  `-E encoding_flags=strict`) **and** per directory (the `F` attribute,
  `EXT4_CASEFOLD_FL`, `chattr +F`). Since Linux 5.2. A file system with the
  feature cannot be mounted by a kernel without `CONFIG_UNICODE`.
- `ext4_ioctl_setflags` refuses the attribute with `EOPNOTSUPP` on a file
  system without the feature, `ENOTDIR` on a file and `ENOTEMPTY` on a
  directory that is not empty (setting and clearing alike). The owner of the
  directory may set it; no capability is needed (`vfs_fileattr_set`:
  `inode_owner_or_capable`).
- New directories inherit it (`EXT4_FL_INHERITED`, applied when the inode is
  created). A directory *moved* into a case-folded one keeps its own flags.
- Comparison is Unicode NFD plus full case folding, ignoring
  default-ignorable code points (`fs/unicode/mkutf8data.c`: "Apply unicode
  normalization form NFD … Apply a full casefold (C + F)"). Names are kept on
  disk as written. Without strict mode, a name that is not valid UTF-8 is
  compared byte for byte.
- Negative lookups are not cached in a case-folded directory (`ext4_lookup`):
  a name that is not there is looked for on disk every time.
- A rename to the same name in another case is a no-op in the kernel
  (`vfs_rename`: `if (source == target) return 0;`): the name keeps its old
  case.
- With encryption (fscrypt): supported together since Linux 5.13, with a v2
  policy; setting the attribute on an encrypted directory needs its key.
- Bind mounts keep the attribute: it is a property of the inode, so
  bubblewrap's `--bind` shows the prefix as it is. overlayfs is not used
  here; it refused case-folding layers until 6.17 and has had bugs with them
  since (fixed in 7.4-rc1).
- Neither `/sys/fs/ext4/features/casefold` (it says what the kernel
  supports) nor `statx` tells an unprivileged process whether a mounted file
  system has the feature. Trying the attribute on an empty directory does.

Other file systems: btrfs has no case folding; f2fs and tmpfs (since 6.13,
mount option `casefold`) have the same per-directory attribute; xfs has
only a whole-file-system ASCII mode.

### What other projects do

- **Proton** sets the attribute on the `drive_c` it creates for a new
  prefix (`set_dir_casefold_bit` in Valve's `proton` script, since
  proton-4.11-1) and ignores a refusal.
- **Bottles** does the same: "Pre-create drive_c directory and set the
  case-fold flag" (`bottles/backend/managers/manager.py`), since 51.12
  ("Improved performance by using case-fold for drive_c when available").
- Lutris, Heroic and umu-launcher have no code of their own for it; umu runs
  Proton's script.
- SteamOS is reported to format SD cards with `mkfs.ext4 -O casefold`; that
  was read second-hand, not in an official release note.
- Wine itself does not set the attribute: its prefix creation makes
  `drive_c` with a plain `mkdir`.

### What tur-runtime does

- With `--casefold` (it is off by default, see below), creating a Wine prefix
  creates `prefix/drive_c` empty and runs `chattr +F` on it **before** Wine
  is started in the prefix. Everything Wine and the application then create
  on drive C: inherits the attribute. The rest of the prefix (registry files,
  `dosdevices` with its `c:` and `z:` links) stays an ordinary directory, as
  Wine expects; Z: (the sandbox's root) is not affected. Wine keeps a
  `drive_c` that is already there (`setup_config_dir` in
  `dlls/ntdll/unix/server.c`).
- Why `drive_c` and not the whole prefix: drive C: is the only part of a
  prefix Windows programs look up names in; the registry files and
  `dosdevices` are read by Wine under the names it wrote. That is also where
  Proton and Bottles put it.
- `chattr` and `lsattr` (e2fsprogs) set and read the attribute, outside the
  sandbox, on the runtime manager's own directory, with `LC_ALL=C`; their
  messages are classified into the reasons the record keeps. This crate
  forbids `unsafe`, and the standard library has no safe form of the ioctl.
- A file system that cannot fold case is not a failure. The prefix is
  created as before, case-sensitive, and the report and the application
  record say why (`no-casefold-feature` for `EOPNOTSUPP`, and so on), with
  `TUR-WINDOWS-PREFIX-009` (the file system cannot) or `-010` (another
  reason) as a warning on standard error and in the JSON report.
- `tur-runtime` never changes a file system's features. Whether the file
  system holding `$XDG_DATA_HOME` has `casefold` is decided when it is made,
  by the installer.
- A Proton prefix is left to Proton, which sets the attribute itself.
- `--no-casefold` leaves drive C: an ordinary directory: for comparison, and
  as the way out if an application misbehaves on a case-folded drive.
- A reset keeps drive C: case-insensitive, or not, as it was; so does a
  restore over an existing prefix (the archive does not hold the attribute,
  so the unpacked `drive_c` is given it before the archive is unpacked into
  it).

### The default

**Off.** `DEFAULT_CASE_FOLDING` in `services/tur-runtime/src/casefold.rs` is
`CaseFolding::Off`; `--casefold` asks for it per prefix. Decided on 13
November 2026 from the measurements in the
[third report](../reports/windows-runtime-3.md) (workflow run 43334979526):

- **Every application still passes.** The eight installers of the five
  real applications installed, opened their window, did their job and were
  removed with nothing left, in two case-folded and two ordinary passes on
  the same ext4 file system: 64 of 64 steps in each mode.
- **It is not faster for them.** Install times summed over both passes were
  237.6 s case-folded and 232.6 s not; the medians 14.6 s and 14.1 s; time to
  the first window and to remove were the same. The difference is smaller
  than the spread between two passes of the same mode (creating a prefix
  alone varied from 8.8 to 13.2 s). Issue #81 asks for case folding to be
  switched on "only if faster".
- **It is slower for some things Windows programs do often.** In the test
  program, case folding made a lookup of an existing name in another case up
  to 29 times faster (5 000 files), which is the case it exists for. But
  creating a file was 1.6 to 3 times slower, looking for a name that is
  not there 1.3 to 2.7 times slower, and `FindFirstFile` of one exact name 66
  times slower at 5 000 files: Wine answers that with one `stat` on an
  ordinary directory and with a full read of a case-folded one.
- **It changes what programs see.** `GetFinalPathNameByHandle` returns the
  name in the case it was asked for, not the case on disk; precomposed and
  decomposed accents, `ß` and `ss`, and the Kelvin sign and `K` become the
  same name; a second file whose name differs only by composition is
  refused. Wine's own emulation does none of these, and Windows does not
  either for the first.

It is the right choice for one kind of application: one whose files spell
a directory in two ways, as archives put together on case-sensitive systems
sometimes do (game modifications, plug-in collections). In the test, an
archive with `App/Plugins/one.txt` and `App/plugins/two.txt` gave one
directory on a case-folded drive C:, where all six spellings found their
file, and two directories on an ordinary one, where three of six did not.
That is what `--casefold` is for, and what a catalogue entry or the
application centre could ask for per application.

To be looked at again when: the measurement is repeated on the TUR image
(Fedora's kernel and Wine, a real disk); Wine stops reading the whole
directory for an exact name on a case-folded directory; or an application
is found that is measurably faster with it.

### What the installer must guarantee

For case folding to happen at all on an installed TUR system:

1. The file system that holds `/var/home` (and so every user's
   `$XDG_DATA_HOME/tur/runtime`) is ext4 made with `-O casefold` (encoding
   UTF-8; strict mode is not needed and was not tested). Fedora's
   `/etc/mke2fs.conf` (e2fsprogs 1.47.4) does not turn it on, bootc adds only
   `-O verity` for ext4, and the Kinoite default layout is btrfs, which has
   no case folding at all.
2. The kernel has `CONFIG_UNICODE=y` (Fedora 45's does).
3. The feature cannot be added later by a user: `tune2fs -O casefold` works
   only on an unmounted file system (e2fsprogs 1.45.7 and later) and needs
   root; `fuse2fs` refuses case-folding file systems.
4. Encrypted home directories with fscrypt would need a v2 policy and the
   key loaded when the prefix is created. Not tested.

Nothing else is needed from the installer: `tur-runtime` asks for the
attribute per prefix, and the rest of the system is unaffected because no
other directory gets it.

### What it does not change

- Windows and ext4 do not fold case the same way. NTFS compares names after
  a simple upper-casing table, without Unicode normalisation; ext4 normalises
  (NFD) and applies full case folding. On a case-folded drive C: `café`
  written precomposed and decomposed are one name, `ß` matches `ss`, and the
  Kelvin sign matches `K`; Wine's own comparison on an ordinary directory
  does not do these. What was measured is in the report.
- Renaming a file to the same name in another case does not change its
  case on a case-folded drive (the kernel treats it as a rename to itself).
  Measured, Wine 9.0 did not change it on an ordinary drive either.
- A handle's final path (`GetFinalPathNameByHandle`) carries the case the
  file was opened with on a case-folded drive, and the case on disk on an
  ordinary one; Windows gives the case on disk.
- Granted host directories and the private home are not case-folded; Wine
  emulates case-insensitivity there as before.

### Sources

Read on 13 November 2026. Kernel files are torvalds/linux master (7.4-rc2):
`fs/ext4/ioctl.c` (`ext4_ioctl_setflags`), `fs/ext4/ext4.h`
(`EXT4_CASEFOLD_FL`, `EXT4_FL_INHERITED`), `fs/ext4/ialloc.c`,
`fs/ext4/namei.c` (`ext4_lookup`), `fs/ext4/super.c`, `fs/file_attr.c`
(`vfs_fileattr_set`), `fs/namei.c` (`vfs_rename`), `fs/unicode/mkutf8data.c`,
`fs/crypto/hooks.c`, `Documentation/admin-guide/ext4.rst`,
`Documentation/filesystems/tmpfs.rst`, `fs/overlayfs/params.c`; release
containment of commits b886ee3e778e (5.2), 471fbbea7ff7 (5.13), 083957f9614a
(6.17) and 1d27da7de3f7 (7.4-rc1) checked against the tags. e2fsprogs:
`misc/mke2fs.8.in`, `misc/tune2fs.c`, `misc/fuse2fs.c` (v1.47.4) and the
release notes of 1.45.7, 1.46.0 and 1.47.0. Wine: `dlls/ntdll/unix/file.c`
and `dlls/ntdll/unix/server.c` at `wine-9.0` and master, from the GitHub
mirror `wine-mirror/wine` (winehq.org refuses automated reading). Proton:
`proton` at HEAD of `ValveSoftware/Proton`, and the tags `proton-4.2-9` and
`proton-4.11-1`. Bottles: `bottles/backend/utils/file.py`,
`bottles/backend/managers/manager.py` and its 51.12 release notes. Fedora:
`kernel-x86_64-fedora.config` and `e2fsprogs` on src.fedoraproject.org
(branch f45); anaconda's `data/profile.d/fedora.conf` and
`fedora-kinoite.conf`; bootc's `crates/lib/src/install/baseline.rs`. Ubuntu:
the noble kernel annotations and Launchpad's e2fsprogs; the runner's own
`/boot/config` and `mke2fs -V`, printed by the job.

Not read from a primary source: that SteamOS formats SD cards with
`-O casefold` (a third party's copy of its script); the configuration of the
Ubuntu Azure kernel flavour, except as the job printed it
(`CONFIG_UNICODE=y`); Wine bug 47099, and whether Wine ever accepted
case-only renames on a case-folded directory (bugs.winehq.org and
gitlab.winehq.org refuse automated reading).

## Verified upstream

Read on 12 November 2026. Where a primary source could not be read, that is
said.

| Subject | What was verified | Source |
|---|---|---|
| Wine environment | `WINEPREFIX` (default `$HOME/.wine`), `WINESERVER`, `WINEDEBUG` and its syntax (`fixme-all` is one of the man page's examples), `WINEDLLPATH`, `WINEDLLOVERRIDES`: `n` and `b`, an empty order disables a DLL, `;` between entries. `WINEARCH` takes `win32`, `win64` or `wow64`; it "is set at prefix creation time and cannot be changed afterwards", and "Wine will refuse to start if WINEARCH doesn't match the prefix architecture" | `wine(1)` source, https://raw.githubusercontent.com/wine-mirror/wine/master/tools/wine/wine.man.in |
| `WINEARCH` status | Wine 11.0: the new WoW64 mode is "fully supported"; `WINEARCH=wow64` forces it in a 64-bit prefix; "Pure 32-bit prefixes created with `WINEARCH=win32` are deprecated"; the `wine64` loader is removed in favour of one `wine` | Wine 11.0 release notes, https://raw.githubusercontent.com/wine-mirror/wine/wine-11.0/ANNOUNCE.md |
| `wineboot` | Options `--init`, `--update`, `--kill`, `--restart`, `--shutdown`, `--end-session`, `--force`. `--init` updates the prefix only when the stored time differs; `--update` forces it | `wineboot.c`, https://raw.githubusercontent.com/wine-mirror/wine/master/programs/wineboot/wineboot.c |
| How a prefix records its Wine version | It does not. `.update-timestamp` holds the modification time of `wine.inf`; the text `disable` there makes `wineboot` skip the automatic update | Same file, `update_timestamp()` |
| `ntsync` in Wine | "The NTSync Linux kernel module is used when available"; the module ships with the kernel from 6.14 | Wine 11.0 release notes, as above |
| `ntsync` device | "The ntsync driver creates a single char device /dev/ntsync"; each open of it is an independent instance | https://docs.kernel.org/userspace-api/ntsync.html |
| Wayland driver | Wine 11.0 calls it "the experimental Wayland driver"; clipboard and input methods are implemented in it | Wine 11.0 release notes, as above |
| `umu-launcher` | `umu-run [FILE [ARG...]]`; `WINEPREFIX` (default `$HOME/Games/umu/$GAMEID`), `GAMEID` (default `umu-default`, "does not apply any fixes"), `PROTONPATH` (a directory, a version name, or `GE-Proton`; default UMU-Proton), `STORE`, `PROTON_VERB` (default `waitforexitandrun`), `UMU_RUNTIME_UPDATE`. `umu-run ""` creates a prefix. It downloads the Steam Linux Runtime into `$HOME/.local/share/umu`. Licence: GPLv3 | https://raw.githubusercontent.com/Open-Wine-Components/umu-launcher/main/README.md and `docs/umu.1.scd` in the same repository |
| Proton options | `PROTON_USE_WINED3D`, `PROTON_NO_ESYNC`, `PROTON_NO_FSYNC`, `PROTON_LOG`, `PROTON_LOG_DIR` | https://raw.githubusercontent.com/ValveSoftware/Proton/proton_10.0/README.md |
| DXVK in a plain prefix | Copy the DLLs into `system32` and `syswow64`; add native overrides for `d3d8`, `d3d9`, `d3d10core`, `d3d11` and `dxgi`; `wineboot -u` restores Wine's own files. Launchers such as Steam Play set DXVK up themselves | https://raw.githubusercontent.com/doitsujin/dxvk/master/README.md |
| VKD3D-Proton | Used "as native Win32 DLLs (d3d12.dll and d3d12core.dll)"; needs Vulkan 1.3 | https://raw.githubusercontent.com/HansKristian-Work/vkd3d-proton/master/README.md |
| bubblewrap | Every option used: `--unshare-user`, `--unshare-ipc`, `--unshare-pid`, `--unshare-uts`, `--unshare-cgroup-try`, `--unshare-net`, `--disable-userns` ("requires --unshare-user"), `--die-with-parent`, `--new-session`, `--ro-bind[-try]`, `--bind`, `--dev-bind[-try]`, `--proc`, `--dev`, `--tmpfs`, `--perms`, `--symlink`, `--remount-ro`, `--chdir`; file-system options "are applied in the order they are given" | `bwrap(1)` source, https://raw.githubusercontent.com/containers/bubblewrap/main/bwrap.xml |
| Desktop entries | String escapes `\s`, `\n`, `\t`, `\r`, `\\`; `Exec` arguments quoted with double quotes, with `"`, `` ` ``, `$` and `\` escaped; a literal backslash is four in the file; `%` is `%%` | https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html and `value-types.html` |
| Ubuntu and user namespaces | Ubuntu 23.10 and 24.04 restrict unprivileged user namespaces through AppArmor; what bubblewrap needed on the CI runner is in the [report](../reports/windows-runtime-1.md) | https://discourse.ubuntu.com/t/understanding-apparmor-user-namespace-restriction/58007 |
| NSIS installers | "/S runs the installer or uninstaller silently"; `/D` sets the directory and must be last; the examples `installer.exe /S`, `uninstaller.exe /S _?=...` | NSIS manual, section 3.2, https://nsis.sourceforge.io/Docs/Chapter3.html |
| Inno Setup installers | `/SP-`, `/SILENT`, `/VERYSILENT` ("this installation progress window is not displayed"), `/SUPPRESSMSGBOXES` ("only has an effect when combined with /SILENT or /VERYSILENT"), `/NORESTART`; the uninstaller takes `/SILENT`, `/VERYSILENT`, `/SUPPRESSMSGBOXES`, `/NORESTART` | https://jrsoftware.org/ishelp/topic_setupcmdline.htm and `topic_uninstcmdline.htm` |
| 7-Zip's installer | "For exe installer: Use the "/S" parameter to do a silent installation"; "For msi installer: Use the /q INSTALLDIR=... parameters"; the options are case-sensitive | https://www.7-zip.org/faq.html |
| `msiexec` in Wine | `/i` and `/package`, `/x` and `/uninstall` (the package or product code follows, attached or as the next argument), `/q`, `/qn`, `/quiet` set the user interface level to none; an argument that is not an option becomes a property, so nothing else is passed | Wine 9.0, `programs/msiexec/msiexec.c`, https://raw.githubusercontent.com/wine-mirror/wine/wine-9.0/programs/msiexec/msiexec.c |
| Shortcuts written by Wine | The header with the Unicode flag; the item list when there is one; the link information with the local base path in the ANSI code page at the offset in its header; then description, relative path, working directory, arguments and icon location, each as a 16-bit count that includes the terminator followed by UTF-16 | Wine 9.0, `dlls/shell32/shelllink.c`, `IPersistStream_fnSave`, `Stream_WriteLocationInfo`, `Stream_WriteString` |
| Wine's registry files | Key lines are `[path] time` with the path escaped for `[` and `]`; string values are `"name"="value"`, with `str(2):` before an expandable string; `dword:%08x`; escapes are `\\`, `\"`, C escapes for control characters, `\x%04x` or `\x%x` for a UTF-16 unit above 127 and up to three octal digits below 32 | Wine 9.0, `server/registry.c` (`dump_value`, `save_subkeys`) and `server/unicode.c` (`dump_strW`) |
| The class of a Wine window | `res_name` and `res_class` are both the process name, which is the program's file name passed through `RtlDowncaseUnicodeChar` | Wine 9.0, `dlls/winex11.drv/window.c` (`set_initial_wm_hints`) and `x11drv_main.c` (`setup_options`) |
| What identifies each installer kind | Checked on the real files of the test matrix on 12 November 2026 by reading their bytes (nothing was run): the NSIS first header four bytes into the appended data of the Notepad++ 8.9.9.1 and VLC 3.0.24 installers; `rDlPtS` in resource 11111 of the Inno Setup 7.1.1 installer, inside the image; a 7z signature at the start of the appended data of 7-Zip 26.06's installer, whose version resource says `7-Zip Installer`; the package class in both `.msi` files | The files named in `tests/windows-runtime/apps.json` |

Not verified, or verified only at second hand:

- **winehq.org and gitlab.winehq.org** refused automated reading (HTTP 403 and
  a bot check), so the Wine wiki was not consulted and Wine's sources were read
  from the GitHub mirror `wine-mirror/wine`.
- **Which display driver Wine prefers** when both an X11 and a Wayland display
  are offered was not established from a primary source. Both sockets are passed
  and the choice is left to Wine.
- **`PROTON_USE_NTSYNC`** is not in Valve's README. It comes from coverage of
  GE-Proton 10-9, which introduced it, and of 10-10, which made `ntsync` the
  default where the kernel has it
  (https://www.gamingonlinux.com/2025/07/ge-proton-10-9-released-with-ntsync-and-fsr4-upgrade-support).
  The variable is only set when a profile asks for `ntsync` explicitly.
- **VKD3D-Proton in a plain prefix**: the README gives no prefix instructions;
  the override used is by analogy with DXVK's.
- **How Proton records a prefix's version**, and where `umu-run` stores the
  Proton builds it downloads, were not read from a primary source. Nothing here
  depends on either.
- **esync and fsync** are treated as absent from upstream Wine. That is from
  knowledge of the project, not from a document read for this work.
- **Microsoft's `msiexec` reference**
  (https://learn.microsoft.com/windows-server/administration/windows-commands/msiexec)
  is named as the documentation of `/qn` and was not read for this work. What
  Wine's `msiexec` does with the option was read from its source, above.
- **The file formats read during installation** (the PE section table and
  resource tree, version resources, icon groups and their bitmaps, the Shell
  Link format, PNG, zlib's stored blocks, SHA-256) are implemented from
  knowledge of the formats, not from specifications read for this work. They
  are checked three ways: unit tests against files built by hand and against
  the published check values of SHA-256, CRC-32 and Adler-32; the real
  installers and installed programs in the `windows-runtime` workflow; and,
  for the icons written, an independent PNG reader in the test (Python's
  `zlib`).
- **Old Inno Setup installers** (before 5.1.5) keep their offset table
  elsewhere and are not recognised; they are reported as `plain-exe`. None was
  tried.

## Command line

```
tur-runtime inspect <path> [--json] [--host-arch ARCH] [--available LIST]
tur-runtime profile validate <file>

tur-runtime prefix create <app-id> [--runtime wine|proton] [--prefix-id NAME]
        [--graphics SETTING] [--pin] [--no-network] [--casefold | --no-casefold]
        [--grant DIR]... [--grant-rw DIR]...
tur-runtime prefix verify|repair|backup|remove|migrate|pin|unpin <app-id>
tur-runtime prefix reset <app-id> [--no-backup]
tur-runtime prefix restore <app-id> <backup-name>
tur-runtime prefix grant <app-id> <dir> [--read-write]
tur-runtime prefix revoke <app-id> <dir>

tur-runtime launch <profile> [--migrate] [--env NAME=VALUE]... [--wine-debug CHANNELS]
        [-- arguments for the application]
tur-runtime desktop-entry <profile> [--launcher PROGRAM]

tur-runtime installer identify <file> [--json]
tur-runtime app install <installer> [--app-id ID] [--name NAME] [--main-exe PROGRAM]
        [--portable] [--no-network] [--casefold | --no-casefold] [--timeout SECONDS]
        [--keep-failed] [--launcher PROGRAM]
tur-runtime app list [--json]
tur-runtime app show <app-id> [--json]
tur-runtime app launch <app-id> [--migrate] [--env NAME=VALUE]... [--wine-debug CHANNELS]
        [-- arguments for the application]
tur-runtime app uninstall <app-id> [--skip-uninstaller] [--remove-backups]
        [--timeout SECONDS]
tur-runtime app autopilot <app-id> [--check FILE] [--max-attempts N] [--no-combinations]
        [--network] [--keep-copies]
tur-runtime app autopilot [<app-id>] --plan [--failure CLASS]
tur-runtime app autopilot <app-id> --results
tur-runtime app autopilot --apply <result-id>
tur-runtime app autopilot <app-id> --revert
```

Every `prefix` and `launch` command, and `app install`, `app launch` and
`app uninstall`, also takes:

| Option | Meaning |
|---|---|
| `--dry-run` | Print the plan and carry none of it out |
| `--json` | Print JSON instead of text: the plan, and after a real run the report |
| `--runtime-program PATH` | Use this `wine` (or `umu-run`) instead of the one found |
| `--runtime-version VERSION` | Assume this version instead of asking the runtime; for Proton, the build to use |
| `--sandbox-program PATH` | Use this `bwrap` instead of the one found |

Programs are looked for in `/usr/bin`, `/usr/local/bin` and `/bin`, not in the
caller's `PATH`: what is found there is also visible inside the sandbox.

| Exit status | Meaning |
|---|---|
| 0 | Success. For `inspect`: the file has a route and everything it needs is installed |
| 1 | The input could not be read, or is truncated, malformed or over a limit |
| 2 | The command line is wrong |
| 3 | No supported runtime exists for the file. For `installer identify` and `app install`: the file is not something that can be installed |
| 4 | The file has a route, but something it needs is not installed |
| 5 | The profile is not valid |
| 6 | The operation was refused before anything was changed or started |
| 7 | A step failed while the operation was carried out. For `app install`: the installer failed or ran out of time, or what it installed could not be made into an application; the installation was rolled back |
| 8 | Verification found problems in the prefix. For `app uninstall`: something was still there after the removal |
| 10 | `app autopilot`: no remedy that was tried made the application pass, or it needs a Microsoft component TUR cannot supply |

Once the application has run, `launch` exits with the application's own exit
status, which can be any of the numbers above; the JSON report says which it
was.

Errors are written to standard error as `error[CODE]: message`. With `--json`
the report, or the error, is JSON on standard output.

## The compatibility autopilot

`tur-runtime app autopilot <app-id>` copies the application's prefix and
private home (copy-on-write where the file system can), runs the application
unchanged in a copy against a check, reads the failure class from the
evidence, and tries the remedies of a declarative catalogue
(`services/tur-runtime/remedies/catalogue.json`) chosen for that class, each in
a fresh copy, singly and then in bounded pairs. Every attempt runs in this
document's sandbox, without network and without granted directories. The
real prefix is only read, and its fingerprints before and after are part of
the result. `--apply` sets what was found in the real prefix after a
`prefix backup`; `--revert` restores that backup. No remedy installs or
fetches anything; a failure that needs a Microsoft component is answered with
the Windows-guest route. Everything else is in
[compat-autopilot](compat-autopilot.md); the measurements in
[autopilot-1](../reports/autopilot-1.md).

## Not done yet

- **Case-insensitive prefixes beyond the first measurement.** Off by
  default (see [The default](#the-default)). Not tried: the TUR image and
  Fedora's kernel, a real disk instead of a loop device, strict encoding,
  fscrypt, Proton (which sets the attribute itself). The prefix metadata
  does not record it (only the application record does), so `prefix
  verify` cannot check it and a prefix made with `prefix create` keeps it
  only in its directory's attribute. Nothing asks for it per application
  yet: a catalogue entry or the application centre would pass `--casefold`.
- **Running on the TUR image.** Everything that has run, ran on a hosted Ubuntu
  runner with Ubuntu's Wine (reports [1](../reports/windows-runtime-1.md) and
  [2](../reports/windows-runtime-2.md)). Wine is not in the image yet, and
  nothing here has run on Fedora, under SELinux, on Plasma or with a GPU.
- **Real applications beyond a first few.** Eight installers of five free
  applications have been installed, started, used once and removed in CI; the
  measured results are in the [second report](../reports/windows-runtime-2.md).
  That is a first table, not a compatibility database: one window and one
  function per application, one Wine version, software rendering. No
  proprietary application, no game, no application that needs a Microsoft
  component has been tried.
- **Installers that ask questions.** An installer is run only in its
  documented unattended mode. Running one with its windows shown, for a kind
  that has no such mode or is not recognised, is not built; nor is choosing
  components, an install directory or a language. Other installer kinds (WiX
  Burn bundles, InstallShield, Wise, self-extracting archives, Squirrel) are
  reported as plain programs.
- **Dependencies.** Nothing is installed for an application: no Microsoft
  component, no redistributable runtime, no Mono, no Gecko, no winetricks
  recipe. An application that needs one fails, and that is the result.
- **Installing the launcher entry.** The entry and its icon are staged under
  the runtime manager's own directory. Putting them where the desktop looks,
  refreshing the desktop's caches, categories, file associations and "open
  with" are the desktop bridge's work (§9.3) and are not built.
- **Updating an installed application.** There is install and uninstall. A
  newer installer over an existing prefix is refused; so is a second
  application in the same prefix.
- **Application data at removal.** Removing an application deletes its private
  home with it. Offering to keep or export the user's files first is not
  built.
- **Proton.** The `umu-run` path is planned and unit-tested only. It has never
  been run: not the download of a Proton build, not the Steam Linux Runtime
  inside the sandbox, not a game. Each application would fetch its own copy of
  the runtime into its private home; a shared, read-only runtime store is not
  built.
- **DXVK and VKD3D-Proton packaging.** The overrides are set; the DLLs are not
  installed into a prefix, and nothing checks that a prefix has them.
- **Several runtime versions side by side.** A pin is honoured by refusing; no
  second Wine can be selected.
- **Wayland-native driver evaluation.** Both display sockets are passed to
  Wine and the choice is left to it.
- **seccomp, Landlock, resource limits.** The sandbox is namespaces and mounts
  only. The `isolation_profile` field of a profile is not interpreted: there is
  one isolation profile, the one described above.
- **Stop.** There is no way to stop a running application other than ending
  `tur-runtime`, which kills the sandbox without letting the Wine server write
  the registry. Rollback of a failed migration is a manual `prefix restore`.
- **32-bit applications.** Two 32-bit installer programs ran (the NSIS
  installers of Notepad++ and VLC), in a 64-bit prefix, on a runner with
  Ubuntu's `wine32:i386` installed. Every application they installed is
  64-bit; no 32-bit application has been started. The launch-path job still
  runs without Wine's 32-bit half.
- **A portal-like grant flow.** Grants are made on the command line.
- **Darling.** No environment is created; the macOS route is still
  recommendation only.
- **Application database.** No known-application lookup, so the recommendation
  uses the file's format and architecture and the host's runtimes only. User
  preferences and security policy (§7.4) are not inputs yet. Proton is never
  recommended, and `GAMEID` is always `umu-default`.
- **D-Bus.** No service and no interface. The library is linked directly by the
  command-line tool.
- **Deeper inspection.** No PE import tables (so no detection of a user-mode
  program that needs a kernel driver, anti-cheat or DRM), no Mach-O load
  commands, no Android manifest decoding, no look inside nested packages, no
  binary property lists. `inspect` does not say what kind of installer a
  program is; `installer identify` does, for the kinds listed above.
- **Other formats.** DMG, PKG, Flatpak and Snap bundles, Java archives and
  Windows shortcuts are reported as `unknown` or as a plain zip.
- **Diagnostics.** Launch logs and an operations log are kept per application;
  nothing collects them into a diagnostic bundle (§6.4).
- **Schema validation in CI.** A unit test for each schema keeps its
  enumerations, required fields and property names in step with the Rust types.
  Neither schema is run through a JSON Schema validator in CI.
- **Error-code registry.** The codes above are not yet registry entries in the
  sense of §29.3.
- **Assistant tools for applications.** P@x can inspect a file
  (`runtime_inspect_file`) and ask what the autopilot would try and what it
  found (`runtime_autopilot_plan`, `runtime_autopilot_results`, both class 0).
  It has no tool to identify an installer, to list installed applications or
  to run the autopilot; why the last is not offered yet is in
  [compat-autopilot](compat-autopilot.md#11-px).
