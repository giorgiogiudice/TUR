# Universal Runtime Manager: inspection and recommendation

Design note for the first slice of `uos-runtime` (specification §2.3, §7, §12.2,
§13.1, §19.2, §19.3). Code: `services/uos-runtime/`. Profile schema:
`schemas/app-profile.schema.json`.

## Scope

This slice answers two questions about a file the user points at: what is it, and
which runtime is the route for it. It also defines the application profile.

It does not run anything. The specification forbids executing a file because of
its type (§7.2); `uos-runtime inspect` only reads, and there is no launch command.
See [Not done yet](#not-done-yet).

A recommendation names a route. It is never a statement that a particular
application works: that needs a recorded test (§2.3).

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

The limits are constants in `services/uos-runtime/src/limits.rs`.

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

`uos-core` has no family for the runtime manager itself. Ecosystem-specific
conditions use `WINDOWS`, `DARWIN` and `ANDROID`; the rest use `DESKTOP` with a
`RUNTIME` or `PROFILE` topic until a family is agreed.

| Code | Condition |
|---|---|
| `UOS-DESKTOP-RUNTIME-001` | The path could not be read |
| `UOS-DESKTOP-RUNTIME-002` | An ELF, zip, OLE or script file is truncated or malformed |
| `UOS-DESKTOP-RUNTIME-003` | The file exceeds an inspection limit |
| `UOS-DESKTOP-RUNTIME-004` | No supported runtime exists for the file |
| `UOS-WINDOWS-INSPECT-001` | A PE file is truncated or malformed |
| `UOS-WINDOWS-RUNTIME-001` | Wine is not installed |
| `UOS-WINDOWS-RUNTIME-002` | The file requires a Windows kernel |
| `UOS-WINDOWS-RUNTIME-003` | The Windows VM backend is not installed |
| `UOS-WINDOWS-RUNTIME-004` | A Windows file no runtime handles |
| `UOS-DARWIN-INSPECT-001` | A Mach-O file or bundle is truncated or malformed |
| `UOS-DARWIN-RUNTIME-001` | Darling is not installed |
| `UOS-DARWIN-RUNTIME-002` | A macOS kernel extension |
| `UOS-DARWIN-RUNTIME-003` | A macOS file no runtime handles |
| `UOS-ANDROID-RUNTIME-001` | The Android runtime is not installed |
| `UOS-ANDROID-RUNTIME-002` | ARM translation is not installed |
| `UOS-ANDROID-RUNTIME-003` | An Android package no runtime handles |
| `UOS-DESKTOP-PROFILE-001` | The profile could not be read |
| `UOS-DESKTOP-PROFILE-002` | The profile is not JSON or does not match the schema |
| `UOS-DESKTOP-PROFILE-003` | A profile field holds a value that is not allowed |
| `UOS-DESKTOP-PROFILE-004` | The profile appears to hold a credential |

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

## Command line

```
uos-runtime inspect <path> [--json] [--host-arch ARCH] [--available LIST]
uos-runtime profile validate <file>
```

| Exit status | Meaning |
|---|---|
| 0 | The file has a route and everything it needs is installed; or the profile is valid |
| 1 | The input could not be read, or is truncated, malformed or over a limit |
| 2 | The command line is wrong |
| 3 | No supported runtime exists for the file |
| 4 | The file has a route, but something it needs is not installed |
| 5 | The profile is not valid |

Errors are written to standard error as `error[CODE]: message`. With `--json`,
`inspect` writes its report, or its error, as JSON on standard output.

## Not done yet

- **Launching.** No launch, stop, repair, reset, backup or rollback operation.
- **Prefix management.** No Wine prefix or Darling environment is created; a
  profile only names a prefix.
- **Application database.** No known-application lookup, so the recommendation
  uses the file's format and architecture and the host's runtimes only. User
  preferences and security policy (§7.4) are not inputs yet.
- **D-Bus.** No service and no interface. The library is linked directly by the
  command-line tool.
- **Deeper inspection.** No PE import tables (so no detection of a user-mode
  program that needs a kernel driver, anti-cheat or DRM), no Mach-O load
  commands, no Android manifest decoding, no look inside nested packages, no
  binary property lists, no installer-framework detection for `.exe` installers.
- **Other formats.** DMG, PKG, Flatpak and Snap bundles, Java archives and
  Windows shortcuts are reported as `unknown` or as a plain zip.
- **Desktop entries, file associations, diagnostics.** Not started.
- **Schema validation in CI.** A unit test keeps the schema's enumerations,
  required fields and property names in step with the Rust types. The schema
  itself is not run through a JSON Schema validator in CI.
