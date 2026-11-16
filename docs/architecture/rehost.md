<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Re-hosting: a Windows or macOS package on the Linux build of its own runtime

Design note for `tur-rehost` (specification §2.3, §7, §12.1, §19.2). Code:
`services/tur-rehost/`. Index: `services/tur-rehost/data/index.json`. Record
schema: `schemas/rehost-record.schema.json`. The measurements it rests on:
[exp-rehost-1](../reports/exp-rehost-1.md). The first report of the
component: [rehost-route-1](../reports/rehost-route-1.md).

> **Status: built, not run.** GitHub Actions was disabled for the owner's
> account while this was written: nothing in this note has been compiled
> or run. The measurements quoted are the experiment's, made with its
> Python prototype, not with this program.

## 1. Scope

Many Windows and macOS applications are a portable payload (Electron's
`app.asar`) in a thin platform shell. Re-hosting takes the payload out of
the person's own package, without running the installer, and runs it on the
Linux build of the same runtime at the same version: a Linux process, native
Wayland or X11 windows, the same accessibility tree as the publisher's own
Linux build (exp-rehost-1, section 6). The experiment measured 20 of 24
Electron applications reaching a working window from their Windows package
and 20 of 24 from their macOS package.

This slice re-hosts **Electron only**. NW.js, .NET with Avalonia, Java,
Godot, LÖVE, Ren'Py and frozen Python were measured too (exp-rehost-1,
section 5); the detector recognises them and the component refuses them
with `TUR-REHOST-DETECT-005` until each is built.

## 2. Where it lives, and why not inside `tur-runtime`

The report (section 9.2) suggested `tur-runtime`. It is a standalone program
instead, `tur-rehost`, for four reasons:

1. **`tur-runtime` promises not to fetch.** Its installer section says
   "Nothing is fetched. The file is one the user already has", and its
   inspection never decompresses anything. Re-hosting must open containers
   (NSIS, MSI, dmg, pkg) and must fetch a runtime. Putting that inside
   `tur-runtime` would break two invariants its design and tests rest on.
2. **Dependencies and lockfiles.** Each component is its own workspace
   (ADR-0005). `tur-rehost` needs `sha2` and the system's `curl`, as
   `tur-apps` does; `tur-runtime` needs neither.
3. **One route, one program.** The router's routes map onto programs:
   `wine` and `proton` to `tur-runtime`, `flatpak` to `flatpak`, `rehost` to
   `tur-rehost`. P@x gets the tools of each under its own service name
   (`org.tur.Rehost1`).
4. **Licence boundary.** A separate TUR program, talking to the others on
   the command line, is what AGENTS.md asks for.

What is shared is the **model**, not the code: one application, one
directory; the record outside what the application can see; the same
sandbox boundary; the same exit statuses. The bubblewrap builder is a second,
smaller copy of `tur-runtime`'s; moving both into a shared crate
(`crates/tur-sandbox`) is a follow-up.

## 3. What happens to a package

```
tur-rehost plan  <package>   open → detect → plan            nothing fetched
tur-rehost install <package> open → detect → plan → fetch → lay out → swap → record
tur-rehost launch <app-id>   check the payload's hash → sandbox → the program
tur-rehost uninstall <app-id>  delete everything of it → check nothing is left
```

1. **Open, never run** (`src/unpack.rs`). The container is recognised by its
   content. Each layer is opened by a Fedora archive tool in the *tool
   sandbox* (no network, nothing writable but the output directory, the
   input read-only): `bsdtar` for zip and tar, 7-Zip (`7zz`) for 7z, NSIS
   (and electron-builder's `$PLUGINSDIR/app-64.7z` inside it) and disk
   images, `innoextract`, `msiextract`, and for a macOS `.pkg` `bsdtar` plus
   `xz` (the `pbzx` stream is split into its xz chunks here). Squirrel's
   `Setup.exe` has its zip cut out here. A directory or `.app` bundle is
   read where it is.
2. **Detect** (`src/detect.rs`, types in `src/finding.rs`): the runtime
   family, its version (Electron's user-agent string in the publisher's
   binary, the framework's `Info.plist`), the platform, the publisher's
   Electron fuses, and every native Node module with the npm package that
   owns it. Nothing is executed. Electron is read in full; the other
   families are recognised.
3. **Plan** (`src/install.rs`, `prepare`). Every refusal happens here,
   before anything is fetched: native code (`-DETECT-004`, with the route
   that may run it), a family not built (`-005`), no version (`-006`), a
   vendor build of Electron such as castlabs' (`-007`), a Linux package
   (`-008`), a version TUR has not pinned (`-INDEX-002`), a native module
   with no Linux build TUR can supply (`-NATIVE-001`), a build the person
   has not allowed (`-NATIVE-002`). The plan lists every download with its
   size, origin, pinned SHA-256 and licence, and what the person is told.
4. **Install** (`install`), staged in `staging/<app-id>/`:
   - the runtime, fetched (section 4) and unpacked; its `electron` program
     renamed after the publisher's (`Joplin`), `default_app.asar` removed;
   - the payload copied into `resources/`, without `.lproj`, `.icns` and
     `app-update.yml` (the application's own updater is off: it would
     fetch a Windows or macOS update); on Windows the application's own
     files beside its program are copied too, Electron's and the
     installer's are not;
   - the **fuses** of the publisher's binary set on the Linux binary, so it
     is no more permissive than the one the publisher shipped;
     `EnableEmbeddedAsarIntegrityValidation` is turned off (its expected
     hash lives in the publisher's binary, which is not used) and TUR's own
     SHA-256 of the payload is recorded and checked before every start;
     `OnlyLoadAppFromAsar` is off for a directory payload;
   - native modules and helper packages swapped (section 6);
   - `ffmpeg` and `ffprobe` helper programs replaced by links to Fedora's
     (`ffmpeg-free`) when the system has them;
   - the launcher entry staged (`launchers/<app-id>/tur-rehost-<app-id>.desktop`,
     `Exec=tur-rehost launch <app-id>`, `X-TUR-Runtime=rehost`);
   - the record written last; then the directory is renamed into
     `apps/<app-id>/`. A failure removes the staging directory and the
     launcher entry (`TUR-REHOST-APP-003`).
5. **Launch**: the payload's SHA-256 against the record (`-APP-006` when it
   changed), then the program in the application sandbox (section 7), with
   `--force-renderer-accessibility` when P@x or a screen reader is on
   (`--accessibility`), because Fedora 45's accessibility bus has no
   `ScreenReaderEnabled` property through which Chromium could be told
   (exp-rehost-1, 6.1). `--no-sandbox` is never passed.
6. **Uninstall**: the application's directory (runtime, payload, private
   home, record), staging, launcher entry and logs are deleted, then each
   is checked to be gone (`-APP-007`, exit 8, if not). Everything the
   application could write was in its own directory.

## 4. The pinned index

`data/index.json` is the only list of what `tur-rehost` may fetch. It is
compiled into the program (`--index FILE` replaces it for a test). It holds:

| Part | Entries | From | Digest taken from |
|---|---|---|---|
| Electron's Linux x86-64 release, per version | 19 versions, 110 to 127 MB each | `https://github.com/electron/electron/releases/download/v<version>/electron-v<version>-linux-x64.zip` | the release's `SHASUMS256.txt`; for 18 of the 19 GitHub's asset digest agreed |
| Electron's open-codecs media library, per version | 19, about 1.4 MB | `.../ffmpeg-v<version>-linux-x64.zip`, same release | `SHASUMS256.txt` |
| Electron's headers, per version (for builds) | 19, about 340 KB | `https://artifacts.electronjs.org/headers/dist/v<version>/node-v<version>-headers.tar.gz` | that directory's `SHASUMS256.txt` |
| Linux siblings of per-platform npm packages | 8 | `https://registry.npmjs.org/` | computed when the index was written; npm's own SHA-512 integrity matched and is kept |
| Linux prebuilds in modules' npm tarballs | 4 | the npm registry | as above |
| Release prebuilds | 3 (keytar, sqlite3, kerberos) | the modules' GitHub releases | computed when the index was written: those releases publish no digest |
| Source builds | 26 modules, with their build-time packages (`node-addon-api` at the pinned version) | the npm registry | as above |

The versions are exactly those the experiment's 40 working variants need.
`data/pin-index.py` writes the file from `data/wanted.json` (a maintainer
runs it; it downloads into a scratch directory, hashes, never unpacks to
disk or runs anything). A unit test holds every URL to the three hosts above
and every module licence to an open-source one.

Nothing is fetched that is not in the index: no "latest", no version range.
A newer Electron in a new version of an application is a refusal until the
index is updated by a reviewed change.

## 5. The runtime and its codecs

Electron's Linux release carries `libffmpeg.so` built with Chromium's
proprietary codecs (H.264, AAC). TUR may not ship codecs from outside
Fedora. So:

- **What is fetched, from where**: the file above, unchanged, by the
  person's computer, the first time an application needs that version, and
  only when the person installs that application. Never in the image: the
  image does not contain Electron (nor, yet, `tur-rehost`).
- **Verified how**: `curl` reads no configuration, speaks HTTPS only, also
  on redirects, and stops at the pinned size; the file's SHA-256 and size
  must equal the index's pin, or it is deleted (`-FETCH-002`) and nothing in
  it is used. A file already in `~/.cache/tur/rehost/downloads/` is checked
  again before use.
- **What the person is told** before anything is fetched: the plan's
  `runtime.notice` ("TUR downloads Electron's own Linux build from the
  Electron project's releases on GitHub, unchanged, and checks it against
  the SHA-256 TUR pinned. It is not part of TUR. It contains media codecs
  (H.264 and AAC) that TUR does not ship; the download is made by this
  computer, at your request, as with Proton."), the list of downloads with
  sizes and licences, and the router's caveats `rehost.runtime-fetched` and
  `rehost.not-publisher-supported` in the application centre. The record
  keeps what was fetched, from where, with its digest and licence.
- **The other option is built**: `--codecs open-only` replaces
  `libffmpeg.so` with Electron's own open-codecs build of the same version
  (also pinned). H.264 and AAC then do not play inside that application;
  VP8, VP9 and Opus do. The default is `upstream`, as the owner decided for
  this slice; question 10 of the experiment's legal list is still open and
  could make `open-only` the default.

## 6. Native modules

The experiment found 138 foreign native modules in 49 Electron payloads: 55
with a published Linux build, 53 compiled from their npm source (27 distinct,
1 to 43 s each), 10 for Windows or macOS only, 20 dead ends (section 4).
`src/natives.rs` resolves each module, in this order:

| Resolution | When |
|---|---|
| `not-for-linux` | the package's `os` excludes Linux (`fsevents`): nothing to do |
| `carried` | the package already holds a Linux x86-64 build: copied over the foreign one where loaders look (`build/`, `bin/`) |
| `npm-package` | a per-platform package (`@scope/x-win32-x64-msvc`) whose `linux-x64` sibling the index pins: laid beside it |
| `npm-prebuild`, `release-prebuild` | the index pins a Linux build: laid where the loader looks (node-pre-gyp's `module_path` with Linux's values, or over the foreign file) |
| `source-build` | the index pins the npm source, and the person allowed builds: built on this computer (below) |
| `needs-build` | as above, builds not allowed: refused (`-NATIVE-002`, exit 4) unless accepted |
| `unresolved` | no `package.json` above the file (bundled, or the application's own module), or nothing pinned: refused (`-NATIVE-001`, exit 3) unless accepted |

"Accepted" is `--allow-unresolved name@version` (or `file:<name>.node` for a
module that cannot be identified): the person runs the application without
that module's Linux build, and the application fails if it loads it.
`tur-apps` passes exactly the modules the catalogue record measured the
application starting without, and only for the package TUR measured.

### Where builds happen: on the person's computer, offline

The report proposed a build service in TUR's CI that compiles, signs and
serves binaries. This slice builds **on the person's computer** instead,
because it is feasible and safe offline, and it keeps TUR from distributing
binaries built from third-party source (and from running a signing and
serving infrastructure that does not exist):

1. **Only with permission**: `--allow-builds`, which the router asks for as
   the prerequisite `allow-rehost-builds` and `tur-apps` passes on.
2. **Fedora's toolchain**: a container image built once from Fedora 45's
   base image, pinned by digest, with Fedora's `gcc-c++`, `make`,
   `python3`, `nodejs`, `nodejs-npm` (which carries node-gyp) and the
   X11, xkbfile, libsecret and fontconfig headers the experiment's modules
   needed. This is the only network use of a build, and it is Fedora's
   registry and repositories with Fedora's signatures. Its size was not
   measured (a few hundred megabytes is the expectation).
3. **Pinned inputs**: the module's npm tarball, the build-time packages its
   `binding.gyp` requires (`node-addon-api`, at the version the module
   declares, pinned), and Electron's headers, each checked against the
   index on the host and unpacked in the tool sandbox.
4. **No network during the build**: `podman run --network=none --read-only
   --cap-drop=all --security-opt=no-new-privileges --userns=keep-id`, the
   source directory the only writable mount. A module's `binding.gyp` can
   run commands; they run there with nothing to reach.
5. **Only Linux results**: only ELF x86-64 `.node` files from
   `build/Release` are taken; the directory is deleted.

Not built yet: modules whose build is Rust (napi-rs) or CMake; a module the
index does not pin; a cache of built modules between applications (each
re-host builds again, seconds per module).

## 7. Layout, record and sandbox

```
$XDG_DATA_HOME/tur/rehost/
    apps/<app-id>/record.json   written by tur-rehost only; not shown to the application
    apps/<app-id>/app/          the runtime with the payload; read-only in the sandbox
    apps/<app-id>/home/         the application's private HOME; the only place it writes
    staging/<app-id>/           an installation in progress
    launchers/<app-id>/         the staged launcher entry
$XDG_CACHE_HOME/tur/rehost/downloads/<sha256>   verified downloads
$XDG_STATE_HOME/tur/rehost/logs/<app-id>/       one log per launch
```

The record (`schemas/rehost-record.schema.json`) names the package (file,
size, SHA-256, platform, layers), the runtime (version, URL, SHA-256,
codecs), the program, the payload and its SHA-256, the publisher's fuses and
what was set, each module's resolution and where its files went, the system
helpers linked, everything fetched with its licence, `updater: off`, the
time and the launcher entry. It is read back with a 256 KiB cap and must
name the application of its directory.

The application sandbox is the Windows prefixes' boundary
(runtime-manager.md, "The sandbox") with three differences: user namespaces
stay available inside (Chromium's own sandbox needs them; Proton's sandbox
does the same), `/dev/shm` is a private in-memory file system, and the
accessibility bus socket is shown when `AT_SPI_BUS_ADDRESS` names one in the
runtime directory. Shown: `/usr`, `/etc` read-only; the application's `app/`
read-only; its `home/` read-write; the Wayland, PipeWire and PulseAudio
sockets one by one; a local X display and its authority file; the resolver
directory; the GPU device nodes. Not shown: the person's home, the session
bus (so no desktop portal yet: see Not built), the keyring, other
applications, TUR's records. The network is shared: these applications need
it.

## 8. The router and the application centre

The catalogue gains the route `rehost` (catalogue-and-routing.md). A
Windows or macOS edition is offered the route **only when a catalogue record
says TUR measured it working**, with its evidence: the package's file name
and SHA-256, the runtime and version, the run, the modules built from source
and the modules the application started without. The route sits below any
Linux edition and above Wine. The 40 working variants of the experiment are
the seed records. A macOS edition with such a record has a route without
Darling; one without it falls back to Darling (when enabled) or a refusal
with the reason, unchanged.

`tur-apps` installs by the route from the person's own file
(app-install-backend.md). The application centre's "I have the installer
file" path is a follow-up: the desktop programs pull request (#128) is not
merged.

## 9. Error codes

| Code | Condition | Exit |
|---|---|---|
| `TUR-REHOST-DETECT-001` | The file could not be read | 1 |
| `TUR-REHOST-DETECT-002` | The package's container could not be opened | 3 |
| `TUR-REHOST-DETECT-003` | Nothing in the package was recognised | 3 |
| `TUR-REHOST-DETECT-004` | Native code, or a Windows-only framework: not re-hostable | 3 |
| `TUR-REHOST-DETECT-005` | A family recognised and not built yet (all but Electron) | 3 |
| `TUR-REHOST-DETECT-006` | The runtime's version could not be read | 3 |
| `TUR-REHOST-DETECT-007` | A vendor build of Electron | 3 |
| `TUR-REHOST-DETECT-008` | The package is already for Linux | 3 |
| `TUR-REHOST-INDEX-001` | TUR's index is unreadable or not valid | 1 |
| `TUR-REHOST-INDEX-002` | The runtime version is not pinned | 3 |
| `TUR-REHOST-FETCH-001` | A download did not complete | 7 |
| `TUR-REHOST-FETCH-002` | A download is not the pinned file; deleted | 7 |
| `TUR-REHOST-NATIVE-001` | A native module has no Linux build TUR can supply | 3 |
| `TUR-REHOST-NATIVE-002` | A module must be built and builds are not allowed | 4 |
| `TUR-REHOST-NATIVE-003` | A build failed | 7 |
| `TUR-REHOST-HOST-001` | A program the operation needs is not installed | 4 |
| `TUR-REHOST-HOST-002` | bubblewrap is not installed | 4 |
| `TUR-REHOST-HOST-003` | Run as root: refused | 6 |
| `TUR-REHOST-APP-001` | A value given is not allowed | 1 |
| `TUR-REHOST-APP-002` | Already re-hosted | 6 |
| `TUR-REHOST-APP-003` | A step failed; rolled back | 7 |
| `TUR-REHOST-APP-004` | Not re-hosted | 6 |
| `TUR-REHOST-APP-005` | The record is not valid | 1 |
| `TUR-REHOST-APP-006` | The payload changed since it was set up: not started | 6 |
| `TUR-REHOST-APP-007` | Something was left after removal | 8 |

`tur-core` has a `REHOST` family for these (specification §29.3). They are
not yet registry entries in the sense of §29.3.

## 10. Command line and P@x

```
tur-rehost detect <package> [--json]
tur-rehost plan <package> [--app-id ID] [--codecs upstream|open-only]
        [--allow-builds] [--allow-unresolved MODULE]... [--json]
tur-rehost install <package> [same options] [--name NAME] [--json]
tur-rehost list|index [--json]
tur-rehost show|uninstall <app-id> [--json]
tur-rehost launch <app-id> [--accessibility] [-- ARGUMENTS]
global: --index FILE --data-dir DIR --sandbox-program --curl-program --podman-program --launcher
```

P@x tools (`agent-tools.json`, service `org.tur.Rehost1`), all class 0
(observe): `rehost_detect` and `rehost_plan` (a package the assistant was
given access to: it is opened in the sandbox, nothing is fetched or
installed), `rehost_list`, `rehost_show`. Installing downloads and changes
the computer: class 2, and not a tool, as with `tur-apps`.

## 11. Tests

- Unit tests in every module, on synthetic data only: detection of
  synthetic Electron layouts, asar reading and writing, fuses, PE, ELF and
  Mach-O heads, zip tails; the index's validity, hosts and licences; native
  module resolution for every tier against the real index; the two
  sandboxes read back as boundaries; curl's arguments and a mismatching
  download being deleted; pbzx splitting; record schema in step; removal
  leaving nothing.
- Command-line tests (`tests/cli.rs`) on synthetic directories: detect,
  plan with the pinned runtime, the build permission, an unpinned version,
  an unidentifiable module accepted or not, a Qt negative control, list,
  show and uninstall refusals. Nothing is fetched.
- P@x tool tests (`tests/agent_tools.rs`) with P@x's own loader.
- Integration (`.github/workflows/rehost.yml`, started by hand only): five
  of the experiment's open-source packages (a Windows zip, a macOS zip, an
  NSIS installer, a release prebuild, and with `builds` a source build),
  each detected, planned, installed with the real runtime download,
  started on a headless X server until a window maps, and removed.

## 12. Not built

- The other runtime families (section 1).
- Updates: the application's own updater is off; watching the publisher's
  feed and re-hosting each new version, keeping the previous re-host until
  the new one has started (exp-rehost-1, 9.4), is not built. A new version
  is a remove and an install.
- The desktop portal (file chooser, opener) inside the sandbox: needs a
  filtered session bus (`xdg-dbus-proxy`).
- Installing the launcher entry where the desktop looks (the desktop
  bridge, as for `tur-runtime`).
- `tur-rehost` in the image (`image/programs.sh`): until it is, the router's
  host fact `rehost` is false on TUR and the route asks for it as a
  prerequisite.
- Source builds of Rust and CMake modules; a cache of built modules.
- ARM machines: x86-64 only, as measured.
- A seccomp filter and resource limits, as for Windows applications.

## 13. Verified upstream

Read on 16 November 2026.

| Subject | What | Source |
|---|---|---|
| Electron releases | `electron-v<v>-linux-x64.zip` and `ffmpeg-v<v>-linux-x64.zip` per release, with `SHASUMS256.txt`; the 19 digests pinned match it, and GitHub's asset digest where published | `https://github.com/electron/electron/releases/download/v<v>/SHASUMS256.txt` |
| Electron headers | `node-v<v>-headers.tar.gz` and `SHASUMS256.txt`; the version index gives each release's ABI (`modules`) and Chromium | `https://artifacts.electronjs.org/headers/dist/` |
| Fedora 45 packages | `7zip` (26.03), `bsdtar`, `innoextract`, `msitools`, `squashfs-tools`, `bubblewrap` (0.12.0), `podman` (6.1.4), `gcc-c++`, `make`, `python3`, `nodejs` (22.23.4), `nodejs-npm` (10.9.9, which carries `node_modules/npm/node_modules/node-gyp/bin/node-gyp.js`), `libX11-devel`, `libxkbfile-devel`, `libsecret-devel`, `fontconfig-devel`; Fedora 45 has no `p7zip` and no separate `node-gyp` package | `https://mdapi.fedoraproject.org/f45/` |
| npm | each pinned tarball's SHA-512 `dist.integrity` matched the downloaded file | `https://registry.npmjs.org/` |

Not verified: how much the build environment weighs; that node-gyp from
Fedora's npm (rather than the experiment's node-gyp 11.2.0 from npm) builds
every one of the 26 modules; whether every NSIS installer of the corpus
opens with Fedora's `7zz` (the experiment used `7z` from `7zip`).
