<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# macos-darling-1 — the macOS route in Darling: packaging, runtime manager, router, tests

> **Status: built, NOT RUN.** Branch `feat/macos-darling`. Everything here was
> written without a compiler or a run: GitHub Actions is disabled for the
> owner's account (a support ticket is open) after the project drove too much
> CI load, so no workflow has run on this branch, and nothing is compiled on a
> development machine (AGENTS.md). Every number that has to come from a run is
> marked **NOT RUN** or **estimate**. The two new workflows start only by hand
> (`workflow_dispatch`); the existing `rust` workflow will compile and test the
> Rust changes on the pull request once Actions is back.

```text
TUR DEVELOPMENT REPORT

Task ID:          macos-darling-1 ("implement Darling as well and do whatever
                  is needed", owner, 16 November 2026)
Subsystem:        macOS applications (priority 8: optional, experimental):
                  tur-runtime (macOS route), tur-catalogue (router rule),
                  Darling's build (scripts/darling, third-party/darling)
Build ID:         none: nothing was built
Source revision:  feat/macos-darling (this pull request)

SUMMARY
- What changed:
  * Packaging: TUR builds Darling from source at a pinned commit
    (3fcd2e1a, 15 November 2026) with TUR's patch queue (empty), on the OS
    image's own pinned base, in CI, as one archive published to the
    repository's package registry by a dispatch-only workflow; a person's
    computer fetches it when it first installs a macOS application and
    checks it by SHA-256. Not in the image.
  * tur-runtime: `tur-runtime mac identify | route | install | list | show |
    launch | uninstall` and `mac runtime status | fetch | remove`. A Darling
    prefix per application in the same bubblewrap sandbox as Wine; install
    from a .app, .dmg (7-Zip in the sandbox, no mounting, no root), zip or
    .pkg (Darling's own installer); Mach-O load-command analysis that refuses
    arm64-only, SwiftUI, system-Swift, encrypted and non-macOS programs with
    plain reasons; launch through Darling's shell; removal with a check that
    nothing is left; JSON everywhere; 20 codes TUR-RUNTIME-MACOS-001..020;
    three class-0 P@x tools offered, install and uninstall declared class 2
    and not offered.
  * Router: Darling is offered only for catalogue entries measured to work
    in it (a darling record rated good or usable); otherwise it is listed
    blocked with the reason. The order for a macOS package is Linux edition,
    re-hosting, measured Darling, refusal (tur-runtime `mac route`; the
    catalogue router has no re-hosting route yet).
  * Tests: unit tests for Mach-O parsing on Mach-O files generated in the
    tests, package detection, the refusals, the runtime manifest and store,
    the sandbox boundary, the plans, the record and its schema, the router
    rule; CLI tests; a dispatch-only integration workflow that imports the
    built Darling and installs, starts and removes applications built from
    source by the macfw oracle, reusing that workflow's artifact.
- Why it changed: the owner's instruction of 16 November 2026, and the
  owner's decision that TUR's Darling fixes stay inside TUR.

FILES
- Added: services/tur-runtime/src/macos/{mod,loadcmd,analysis,package,
  darling,plan,record,route}.rs, services/tur-runtime/src/mac_cli.rs,
  services/tur-runtime/darling/runtime.json,
  services/tur-runtime/tests/mac_cli.rs, schemas/mac-app-record.schema.json,
  scripts/darling/{Containerfile,Containerfile.containerignore,
  build-darling.sh,check-tree.py,darling.lock},
  third-party/darling/{README.md,LICENSE,patches/series} (GPL-3.0-or-later),
  tests/macos-darling/{ci-run.sh,in-container.sh,integration.py},
  .github/workflows/darling-runtime.yml, .github/workflows/macos-darling.yml,
  docs/reports/macos-darling-1.md
- Modified: services/tur-runtime/src/{codes,sandbox,runtime_cmd,desktop,lib,
  main}.rs, services/tur-runtime/agent-tools*.json,
  services/tur-runtime/tests/agent_tools.rs,
  services/tur-catalogue/src/route.rs, services/tur-catalogue/tests/
  {router_space,seed}.rs, desktop/apps/tur-centre/fixtures/vocabulary.json,
  desktop/apps/tur-centre/qml/RouterWords.qml,
  docs/architecture/{runtime-manager,catalogue-and-routing}.md,
  docs/dependencies/upstream-projects.md,
  tests/experiments/macfw/ci/oracle-macos.sh (two disk images per app)
- Removed: none

BUILD
- Command: rust workflow on the pull request (tur-runtime, tur-catalogue);
  darling-runtime (dispatch) for Darling; nothing on a development machine
- Result: NOT RUN
- Warnings: NOT RUN. The Rust was written without a compiler: expect
  rustfmt fixes (scripts/dev/ci.py fixes) and possibly compile or clippy
  errors on the first run. Cargo.lock does not change: no new crate.

TESTS
- Unit tests: written, NOT RUN (listed in section 7)
- Integration tests: workflow macos-darling, written, NOT RUN
- Boot tests: not applicable (nothing in the image changes)
- Hardware tests: NOT RUN (none needed: Xvfb)
- Performance tests: NOT RUN (sizes and times below are estimates)
- Run on the development machine: python3 -m py_compile of the two Python
  programs, bash -n of every script, YAML parse of both workflows, JSON
  parse of every JSON file, sha256sum of the patch queue's series file.

DIAGNOSTICS
- Log paths: $XDG_STATE_HOME/tur/runtime/logs/<app-id>/ (one per launch,
  operations.log); .../logs/darling/operations.log (fetch, remove,
  uninstall); artifacts darling-runtime (archive and build-record.json) and
  macos-darling-results (results.json, summary.md, screenshots, logs)
- Diagnostic bundle path: none (the runtime manager has no bundle yet)
- Relevant error IDs: TUR-RUNTIME-MACOS-001..020 (section 4.6)

RECOVERY
- Rollback procedure: revert the pull request. Nothing in the image, boot
  or update path changes; a computer that fetched Darling deletes it with
  `tur-runtime mac runtime remove` and each application with
  `tur-runtime mac uninstall`. A failed installation rolls itself back.
- Recovery tested: NOT RUN

KNOWN LIMITATIONS
- Section 9.

NEXT SAFE ACTION
- When Actions is back: let the rust workflow judge the pull request and
  apply its rustfmt and lockfile fixes; then dispatch darling-runtime once
  (publish off); then exp-macfw once for c00 if it has not run (it makes
  the applications and, now, their disk images); then macos-darling with
  those two run ids. Publish (darling-runtime with publish on) only after
  the owner has answered section 8.
```

## 1. The answer in short

**Darling is delivered as a separate, digest-checked archive that TUR builds
from source in CI and a person's computer fetches when it first installs a
macOS application.** It runs unprivileged (no set-user-ID, no kernel module)
in Darling's `nonroot` mode inside the same bubblewrap sandbox as Wine, one
prefix per application. A macOS package goes to Darling only when its program
can run there (x86-64 code, no SwiftUI, not Swift on macOS's runtime, not
encrypted) and, for the router, only when the application is measured to work
in Darling; everything else is refused with a plain reason. Nothing of this
has run.

## 2. Darling today (read 16 November 2026, from its repositories)

In addition to exp-macfw-0, section 2 (which this report does not repeat):

| Fact | Source |
|---|---|
| Master moves about a hundred commits a day; Darling's CI for master is superseded before it finishes most of the time (the 15 runs before 09:04 UTC on 16 November were all cancelled). The last master commit whose CI built packages for both architectures is `3fcd2e1a` (15 November 21:16 UTC, run 43762728027) | Actions API, `darlinghq/darling` |
| Darling's CI: "Build DEBs" 15 min 24 s on amd64 and 13 min 31 s on arm64, with a warm ccache, for every component including i386, Python, Ruby, Perl and JavaScriptCore | the jobs of run 43762728027 |
| Artifacts of that run: `debs` (amd64) 147 MB, `debs-arm64` 114 MB, `source` 535 MB; release v0.1.20260608: `debs_20260608.zip` 118 MB, `darling-source.tar.xz` 506 MB | Actions and releases API |
| Build needs "up to 16 GB of disk space", the source "up to 5 GB", "a minimum of 4 GB of RAM"; "the Darling installation itself then takes up to 1 GB"; JavaScriptCore alone adds "30 minutes to 1 hour" | `darling-docs/src/build-instructions.md` (changed 15 November 2026) |
| Components can be chosen (`-DCOMPONENTS=`): `cli`, `gui`, `gui_frameworks`, `gui_stubs`, `python`, `jsc`, …; `stock` is the default; `-DTARGET_i386=OFF` drops 32-bit; ccache is used when found; `ENABLE_METAL` needs Vulkan and LLVM; `ADHOC_CODESIGN` is on | `CMakeLists.txt` at `3fcd2e1a` |
| The launcher is installed set-user-ID root. Without root it runs only when asked: `DARLING_ROOTLESS=1` ("a user namespace of your own") or `DARLING_NONROOT=1` ("without namespaces"); an unprivileged launcher takes its install prefix from its own path and passes `DARLING_LIBEXEC_PATH` to the server | `src/startup/darling.c`, `launcher-mode.[ch]` |
| `nonroot`: no namespaces, the prefix is a copy of the system ("copy the system into the prefix; works anywhere, but uses more disk space"), `/proc` a link to the host's. `rootless`: user, mount and PID namespaces of its own, an overlay, and `/proc` mounted in its PID namespace | `darlingserver/src/darlingserver.cpp`, `prefix-setup.cpp` |
| The copy is a plain read/write copy, no reflink; a file whose modification time equals the original's is only re-attributed, a newer one kept | `prefix-setup.cpp`, `copyEntry` |
| A prefix that is not empty must hold `var`, `private/etc`, `Volumes`, `usr`, `var/run` and the three account files, or the launcher refuses it; the launcher makes `<prefix>.workdir` beside the prefix; `darling exec <path>` runs a host path through `/Volumes/SystemRoot`, `darling shell a b` runs `sh -c "'a' 'b'"` | `darling.c`, `prefix-state.h` |
| Disk images: Darling's `hdiutil` uses `darling-dmg`, a FUSE module for HFS+ only (GPL-3) | `darlinghq/darling-dmg` README |
| Swift: `darling-swift` holds "unmodified binaries from `usr/lib/swift/macosx`" of swift.org's 6.3.3 macOS package, plus overlays from Swift 5.2.2, also unmodified; overlays built from source only "when the build finds a Swift toolchain" | `darling-swift` README |
| Debian packaging builds everything, then installs per component; the RPM directory is "not quite ready"; no Fedora package, no COPR | `debian/rules`, `rpm/`, exp-macfw-0 |

Fedora 45 (mdapi, 16 November 2026): `7zip` 26.03 (`/usr/bin/7z`; DMG with
LZFSE since 7-Zip 18.01, XZ in DMG since 24.01, APFS since 22.00, per 7-Zip's
history file; LGPL-2.1-or-later with BSD parts), `dmg2img` 1.6.7 (a 2020
snapshot, HFS+ only), `apfs-fuse` (a 2020 snapshot, FUSE). No `p7zip`.

## 3. Packaging decision

### 3.1 The options

| | A. Separate archive, fetched on first use (**chosen**) | B. In the image | C. Flatpak | D. OCI container |
|---|---|---|---|---|
| Cost to everyone who never runs a Mac application | None | About 0.5 to 1 GB in every image (estimate) | None | None |
| Links against | The image's own libraries: built on the pinned base, like the local AI engine | The same | A Flatpak runtime's (another few hundred MB) | Its own Fedora userland (about 1.5 GB with Darling, estimate) |
| Privilege | None: `nonroot` mode, no set-user-ID | Darling's packaging installs the launcher set-user-ID root, on every computer | Flatpak's sandbox forbids nested user namespaces and would replace tur-runtime's | podman per launch, nested inside or instead of tur-runtime's sandbox |
| Update cadence | Its own: a new archive when TUR moves the pin; the OS update is not touched | Every Darling change is an OS image change, at a hundred commits a day upstream | Its own | Its own |
| Image reproducibility rules | Kept: pinned base digest, pinned commit, network only in the fetch step, compile with `--network=none`, deterministic tar | Kept, but adds an hour of build to every image build | Flathub has no Darling; a manifest of 151 submodules | Base pinned; more to pin |
| Fits "one application centre, P@x tools, one sandbox model" | Yes | Yes | No | No |
| How the computer gets it | `curl` by digest from the repository's package registry, checked against the SHA-256 compiled into tur-runtime | With the image | Flathub or a TUR remote | A registry pull |

**A** is the established pattern for large optional runtimes in TUR (the
Android images, the local AI models: never in the image, pinned, fetched when a
person asks, checked by digest). B charges every computer for a priority-8
feature and would put a set-user-ID root binary in the image. C and D replace
the sandbox model the rest of tur-runtime has.

### 3.2 What was built

| Piece | Where |
|---|---|
| The pin | `scripts/darling/darling.lock`: commit `3fcd2e1aa85079789d19d82415695bdecc512241` (the latest master commit Darling's own CI built), components `cli,gui_stubs`, `-DTARGET_i386=OFF -DENABLE_METAL=OFF`, install prefix `/opt/tur-darling`, ORAS 1.3.6 by SHA-256 |
| The build | `scripts/darling/Containerfile` on `image/base.lock`'s base: `build-darling.sh fetch` (with network: Fedora's build tools, Darling and its submodules at the commit) then `build-darling.sh build` (`--network=none`: TUR's patch queue, CMake and Ninja, install, removals, `check-tree.py`, deterministic `tar` with `xz`); only `darling-runtime.tar.xz` and `build-record.json` leave it |
| What is removed | Apple's Swift runtime (`libexec/darling/usr/lib/swift`), headers, module maps, Swift interfaces, `.tbd` stubs, static libraries, `Developer/` and `*.sdk` directories, the guest's `/Applications` (Darling builds Apple's TextEdit sample there); set-user-ID bits |
| What refuses the archive | `check-tree.py`: any Mach-O whose code signature carries an Apple certificate; any header or SDK left; the Swift runtime left; any Mach-O that links `/usr/lib/swift/*` without the weak flag; any ELF that needs a host library whose package was not in the base before the build tools were added (unless the build is dispatched to finish anyway, for a look; such a build is never published) |
| Licences | Every `LICENSE*`, `COPYING*`, `APPLE_LICENSE*`, `NOTICE*` of Darling and each submodule into `share/licenses/darling/`, with `SOURCE.txt` (repository, commit, every submodule commit, the queue) and `PATCHES.txt` |
| Publishing | `darling-runtime.yml`, dispatch only; `publish` defaults to off. ORAS pushes `ghcr.io/giorgiogiudice/tur-darling-runtime:<commit12>-q<queue8>-fc45-x86_64` with the archive as a layer of media type `application/vnd.tur.darling.runtime.v1.tar+xz`; the job checks that the registry holds it under its own SHA-256 and prints the manifest entry |
| The manifest | `services/tur-runtime/darling/runtime.json`, compiled into tur-runtime: id `darling-3fcd2e1aa850-qc89ea8ce-fc45-x86_64`, `sha256` and `size` **null until published** |
| The patch queue | `third-party/darling/` (GPL-3.0-or-later): README with Darling's licences and the owner's decision, Darling's `LICENSE`, `patches/series` (empty; SHA-256 `c89ea8ce…`) |

### 3.3 Size and time (estimates; NOT RUN)

| Quantity | Estimate | From |
|---|---|---|
| Archive (download) | 90 to 150 MB | Darling's stock amd64 packages are 147 MB with i386, Python, Ruby, Perl and JavaScriptCore; TUR builds fewer components, no i386, and removes the Swift runtime |
| Unpacked build | 0.4 to 0.8 GB | "up to 1 GB" for a stock installation |
| Per installed application (`nonroot`) | about the guest system's size, 0.3 to 0.7 GB, on any file system | Darling copies its system into each prefix with plain reads and writes |
| Cold build | 60 to 150 minutes on a hosted runner (timeout 300) | 15 minutes warm for more components in Darling's CI; the build directory up to 16 GB, so podman's storage goes to `/mnt` |
| Warm build (ccache restored) | 15 to 30 minutes | Darling's CI |
| First installation on a computer | download time plus 1 to 3 minutes to unpack and to create the prefix | estimate |

`build-record.json` reports the real archive size, unpacked size and build
time; the integration test reports the per-application prefix size.

## 4. The macOS route in tur-runtime

The design is in [runtime-manager.md, "Running a macOS
application"](../architecture/runtime-manager.md#running-a-macos-application);
the decisions are summarised here.

### 4.1 Sandbox and mode

Darling runs in tur-runtime's own sandbox (system read-only, prefix and private
home read-write, nothing else of the person's files, no session bus), with two
additions in `sandbox.rs`: the build read-only at `/opt/tur-darling` (the path
it was built for), and the prefix a second time at `/run/tur-runtime/prefix`,
because Darling's Unix sockets live in the prefix and a socket path holds at
most 107 bytes. `DPREFIX` is `/run/tur-runtime/prefix/darling`, so Darling's
`<prefix>.workdir` is inside the writable mount. Darling's
`/Volumes/SystemRoot` is the sandbox's root, not the computer's.

The default mode is `nonroot`: it needs no namespaces, so the sandbox keeps
`--disable-userns`. `rootless` is offered (`--darling-mode rootless`, nested
user namespaces allowed) so that it can be measured; I expect its `/proc` mount
to be refused by the kernel, because bubblewrap's `/proc` has read-only mounts
over `sys`, `sysrq-trigger`, `irq` and `bus`, and a new procfs mount in a child
user namespace is allowed only when the existing one is fully visible
(inference from the kernel's rules; NOT RUN).

### 4.2 Installing without mounting

| Package | How | Why |
|---|---|---|
| `.dmg` (UDIF; raw HFS+ or APFS) and zip | `7z x` in the sandbox, into the prefix's `unpacked/`; a partition image 7-Zip leaves is unpacked in turn | No FUSE, no loop device, no root; 7-Zip reads HFS+, APFS, zlib, bzip2, LZFSE and LZMA DMGs; it is a Fedora package; every one of its parsers runs where only the prefix and the private home can be written |
| `.pkg` | Darling's `installer -pkg … -target /`, in Darling and the sandbox | Packages carry pre- and post-install scripts that only a macOS-like system can run |
| `.app` | `cp --archive --no-dereference --reflink=auto` in the sandbox | Links inside the bundle (`Versions/Current`) stay links and are never followed |

7-Zip is not in the image (Kinoite does not carry it); tur-runtime reports
`TUR-RUNTIME-MACOS-010` without it. Adding Fedora's `7zip` to the image is a
follow-up for the image's owner.

### 4.3 What the analysis refuses

From the Mach-O header and the load commands of the slice Darling would run,
read with the component's bounded reader (every count and size checked, 4 MiB of
load commands at most, names bounded and printable):

| Program | Answer |
|---|---|
| Apple silicon only (arm64) on an x86-64 PC | Refused: "It is built only for Apple silicon (arm64). Darling runs only programs built for the computer's own processor, this computer is x86-64, and nothing translates arm64 code for it. A version of the application for Intel Macs, or a universal one, would run." (`-002`) |
| Universal | The x86-64 slice is used |
| SwiftUI (required) | Refused (`-004`); weakly linked: a warning |
| Swift on macOS's runtime (`/usr/lib/swift/libswift*`) | Refused (`-005`): TUR's build does not ship Apple's Swift runtime, and Swift applications from current Xcode do not run in Darling (#2059) |
| Swift with its own runtime (`@rpath/libswiftCore.dylib`) | Refused unless `--allow-swift` |
| Encrypted (App Store) | Refused (`-006`) |
| iOS or another platform; Mac Catalyst | Refused (`-017`); Catalyst unless allowed |
| 32-bit Intel, PowerPC, not a program | Refused (`-003`) |
| Private frameworks | Warning |

### 4.4 Launch, removal, record

Launch: `darling shell /Applications/<bundle>/Contents/MacOS/<program>` with
the build the application was installed with, so that the bundle is where
macOS would have it; one log per launch. Removal: the application's directory,
staging, launcher entry and logs, each checked to be gone; only a directory
with a macOS record is ever removed. Record: `mac-application.json`
(`schemas/mac-app-record.schema.json`), written last; a record edited to name
another application, a bundle outside the prefix or a program outside the
bundle is refused.

### 4.5 Router

The catalogue router lists Darling (when the person has enabled it) only as
blocked, with `experimental.darling-not-measured`, unless the macOS edition has
a `darling` record rated `good` or `usable`; with one it is listed as before,
doubtful and experimental. Final Cut Pro, the seed's macOS-only entry, now has
no route even with Darling enabled. `tur-runtime mac route` applies the full
order (Linux edition, re-hosting with the re-hosting component's verdict as an
input, measured Darling, refusal `-020`) to a file the person has. The
re-hosting route itself is being productised separately and is not in the
router; when it lands it sits before Darling.

### 4.6 Codes

`TUR-RUNTIME-MACOS-001` to `-020`, listed in runtime-manager.md, "Codes of the
macOS route". The older `TUR-DARWIN-*` codes of inspection did not move.

## 5. Licences

| What | Licence | Where it lives |
|---|---|---|
| tur-runtime's macOS route, the build and test scripts, the workflows | TUR's licence | `services/`, `scripts/darling/`, `tests/macos-darling/` |
| TUR's patches to Darling (none yet), the queue's README and series | GPL-3.0-or-later, or the licence of the changed file | `third-party/darling/` (marked) |
| Darling, as built | GPL-3.0-or-later; components MIT (Cocotron), LGPL-2.1 (Foundation), APSL-2.0 (Apple's open-source releases), BSD, Apache-2.0 and others | the published archive only, with every licence file found |
| 7-Zip, used on the computer | LGPL-2.1-or-later with BSD parts | Fedora's package, a separate program |
| ORAS, used in CI | Apache-2.0 | the CI runner only |

tur-runtime starts Darling as a separate program and never links it (AGENTS.md
licence boundary).

## 6. Owner's decisions (recorded)

1. 16 November 2026: "implement Darling as well and do whatever is needed".
2. TUR's Darling fixes stay inside TUR: a patch queue on a pinned Darling
   commit, applied when TUR builds Darling; not a fork, not upstreamed;
   GPL-licensed in a marked directory (`third-party/darling/`).

## 7. Tests (written; NOT RUN)

| Test | What it holds |
|---|---|
| `macos::loadcmd` (8) | libraries of every link kind, rpaths, build version and old version commands, Swift sections in 32- and 64-bit segments, encryption, a second platform, malformed sizes, commands past the block, section counts that do not fit, hostile names, limits through the bounded reader |
| `macos::analysis` (10) | runnable Objective-C AppKit program; universal; arm64-only refused with its sentence; i386, dylib, kext, script; SwiftUI required and weak; Swift with the system runtime whatever is allowed; embedded Swift refused unless allowed; encrypted, Catalyst, iOS, private frameworks; a bundle through its main program |
| `macos::package` (3) | every kind by content (a disk image called `notes.txt`), short files; the search (outer bundles only, `__MACOSX` skipped, links not followed, partitions, packages) and the choice |
| `macos::darling` (10) | the compiled manifest; every manifest field's rule; registry addresses; tokens; the token never on a command line; digests of published and unpublished builds; status from what is on disk; the record written last; the system release |
| `macos::plan` (9) | Darling's command inside the sandbox boundary (read-only tree, `--disable-userns` in `nonroot`, environment), variables refused, root refused, 7-Zip without Darling and with only the package shown, the copy, the package installer's path, launch through the shell, removal checked, the desktop entry |
| `macos::record` (2), `macos::route` (3), `sandbox` (1 new), `codes` (1 new) | schema and type in step, records only for their own directory; the route order; the new mounts; the codes |
| `tests/mac_cli.rs` (5) | identify, route, install refusals before anything is made, the runtime's digest rules, list/show/launch refusal/uninstall (and a Windows application's directory left alone) |
| `tests/agent_tools.rs` | the three new class-0 tools, the two class-2 tools declared and not offered, the tools run on a scratch home |
| tur-catalogue | the router rule in a unit test, the 36 864 cases of `macos_edition_space`, the seed's Final Cut Pro |
| `macos-darling` workflow | import the built Darling; identify every oracle app, with the expected refusals (rung 8 SwiftUI `-004`, rung 6 Swift `-005`, an arm64 slice cut from rung 1 `-002`); install rung 1 and Table Tool from the bundle, a zip and the oracle's HFS+ and APFS disk images, in `nonroot` and `rootless`; start each under Xvfb, wait 90 s for a window, screenshot, type into rung 1 and look for its line in the log; remove and check nothing is left |

The integration test needs a run of `darling-runtime` (no publishing needed)
and a run of `exp-macfw` that made the oracle artifact with this branch's
`oracle-macos.sh` (for the disk images; without them those cases are skipped).
No macOS job is added.

## 8. Questions for the owner (before publishing)

1. **APSL object code.** The archive contains code compiled from Apple's
   open-source releases (XNU parts, libSystem, Security; APSL-2.0), as every
   Darling build does. AGENTS.md says "never ship Apple binaries". My reading:
   the rule is about Apple's own proprietary binaries; Apple-built binaries
   (the swift.org Swift runtime) and SDK headers are removed and checked
   for; code built by TUR from published APSL source is redistributable
   under APSL-2.0. Please confirm, or the route cannot be published.
2. **Corresponding source.** GPL-3.0 needs the source of what TUR publishes.
   `SOURCE.txt` points to Darling's repository at the pinned commit and its
   submodule commits, and to TUR's queue (GPL-3 section 6(d) allows a third
   party's server with clear directions, but TUR stays responsible for it
   remaining available). The alternative, publishing Darling's source next to
   the archive, means redistributing Apple's APSL source. Which?
3. **Submodule licences.** `docs/dependencies/upstream-projects.md` requires
   every submodule's licence to be checked before a binary is shipped. The
   build collects the files; nobody has read them yet.
4. **Package visibility.** The registry package must be public for computers
   to fetch it without signing in (a one-time setting after the first push).
5. **"Measured to work".** I accepted any admissible record rated good or
   usable (TUR's own test, or an external report such as Darling's AppDB).
   TUR's own tests only would be stricter.

## 9. Known limitations and follow-ups

- **Nothing has run.** No compile, no Darling build, no installation, no
  window. Every statement about behaviour is from Darling's source and docs.
- **Rust written without a compiler.** Expect rustfmt changes and some compile
  or clippy fixes on the first CI run.
- **Host libraries.** Whether everything Darling links is in the Kinoite base
  is unknown until the build's check runs. Likely gaps: FFmpeg's libraries
  (`libavcodec-free` and friends), `giflib`, `libbsd`, `libxkbfile`. If so:
  add the runtime packages to the image (small), or ship them in the archive
  with an rpath (a change to the build), or drop the components that need
  them. The build refuses to publish meanwhile.
- **Disk per application.** `nonroot` copies Darling's system into every
  prefix. A shared, initialised template prefix copied with
  `cp --reflink=auto` would make that nearly free on Btrfs (Darling skips
  files whose times match), but a template's paths and account files would
  have to be made per application; not built.
- **MIT-SHM.** The sandbox has its own SysV IPC namespace; Darling says Mesa
  passes SysV shared memory to the X server. Wine works this way; whether
  Darling's AppKit and Mesa fall back cleanly is NOT RUN.
- **`rootless` mode** is expected to fail inside bubblewrap (4.1).
- **Wayland.** X11 (Xwayland) is what the test uses; `DARLING_APPKIT_BACKEND`
  may be set by a launch; Wayland is not tested.
- **arm64 computers.** Only an x86-64 build is planned; on arm64 the route
  reports `-008`. Darling runs arm64 programs natively on arm64 Linux, so an
  arm64 build is a later addition of one matrix entry.
- **7-Zip** is not in the image (`-010` until it is).
- **`.pkg` installation** depends on Darling's `installer`; untested here.
- **Inspection** (`tur-runtime inspect`) still detects Darling as a `darling`
  program in `PATH` and does not know TUR's own build.
- **Desktop integration.** The launcher entry is staged, not installed, as
  for Windows applications; no icon is taken from the bundle (`.icns` is not
  read).
- **Reproducibility** of the archive (two builds, same bytes) is designed for
  (pinned inputs, no network while compiling, sorted tar with a fixed time)
  and not verified.
- **Darling outruns the pin.** At a hundred commits a day the pin will be old
  quickly; moving it is a change of `darling.lock` and, after a build, of
  `runtime.json`, with the integration test run against it.

## 10. Proposed decision record (the lead assigns the number)

```text
ADR-XXXX: Darling as TUR's macOS route for AppKit applications

Status: proposed (macos-darling-1, 16 November 2026)

Context
  TUR runs macOS applications as an optional, experimental feature
  (priority 8). Three routes exist or are planned: a native Linux edition;
  re-hosting a package's portable payload on the native runtime
  (exp-rehost-1: 38 of 47 macOS packages); and, for native Mac code, Darling,
  the only project that runs unmodified Mach-O programs on Linux. Darling
  runs x86-64 Mach-O programs on x86-64 PCs, has AppKit through Cocotron, no
  SwiftUI, and takes the Swift runtime from Apple's binaries. Fedora does not
  package it; Darling publishes Debian packages and sources only. The owner
  asked to implement Darling (16 November 2026).

Decision
  1. Darling is TUR's route for macOS applications whose program is native
     Mac code that it can run: x86-64 code, not SwiftUI, not Swift on macOS's
     runtime, not encrypted. The order for a macOS package is: native Linux
     edition, re-hosting, Darling for applications measured to work in it
     (a catalogue record rated good or usable), otherwise a refusal with the
     reason. Darling is never offered unmeasured.
  2. Delivery: TUR builds Darling from source in CI at a pinned commit with
     its patch queue, on the OS image's pinned base, and publishes one
     archive to the repository's package registry when the lead dispatches
     it. The archive is not in the image. tur-runtime fetches it when a
     person first installs a macOS application, by digest, and refuses
     anything whose size and SHA-256 differ from the manifest compiled into
     it. The archive contains no Apple-built binary, no SDK header and no
     Apple source; the build refuses it otherwise.
  3. Darling runs unprivileged (no set-user-ID, no kernel module), in its
     nonroot mode, inside tur-runtime's bubblewrap sandbox, one prefix per
     application, like Wine.
  4. Patch-queue policy: TUR's fixes to Darling are a patch queue in
     third-party/darling (GPL-3.0-or-later, or the changed file's licence),
     applied to the pinned commit at build time; not a fork; kept in TUR and
     not offered upstream (owner's decision, 16 November 2026). Each patch
     has one stated reason, evidence and provenance; is written clean-room
     (public documentation and observed behaviour only); is general, never
     keyed on one application; and is rebased or dropped whenever the pin
     moves. Offering a patch upstream would need the owner's decision and a
     named person who answers for it.

Consequences
  + No cost for computers that never run a Mac application; Darling's pace
    does not touch the OS image.
  + The same sandbox, plans, logs and removal guarantees as Windows
    applications; P@x can identify and list.
  - Each installed application holds a copy of Darling's system (nonroot).
  - TUR publishes GPL-3.0 and APSL-2.0 object code and must keep its
    corresponding source available (pending the owner's choice).
  - Swift and SwiftUI applications and Apple-silicon-only applications are
    refused; the share of the latter will grow from 2027.
  - The archive must be rebuilt whenever the image's base moves to another
    Fedora release; tur-runtime refuses a build made for another release.
```

## Sources

Read on 16 November 2026 unless dated otherwise.

- Darling: repository at `3fcd2e1a` and master `754fe00b`
  (`src/startup/darling.c`, `launcher-mode.c`, `launcher-mode.h`,
  `prefix-state.h`, `CMakeLists.txt`, `debian/control`, `debian/rules`,
  `.github/workflows/actions.yaml`, `README.md`, `LICENSE`);
  `darlingserver` (`src/darlingserver.cpp`, `src/run-mode.cpp`,
  `src/prefix-setup.cpp`); `darling-docs` (`build-instructions.md`,
  `installing-software.md`, `darling-prefix.md`); `darling-swift` README;
  `darling-dmg` README; Actions runs 43762728027 and 43826864895 and the
  master run list; releases v0.1.20260608 and v0.1.20260222.
- 7-Zip: https://www.7-zip.org/history.txt, https://www.7-zip.org/license.txt.
- Fedora 45 packages: https://mdapi.fedoraproject.org/f45/ (`7zip`,
  `dmg2img`, `apfs-fuse`).
- ORAS: release v1.3.6 (24 October 2026), its asset digests and checksums file.
- TUR: [exp-macfw-0](exp-macfw-0.md), [exp-rehost-1](exp-rehost-1.md),
  [runtime-manager](../architecture/runtime-manager.md),
  [catalogue-and-routing](../architecture/catalogue-and-routing.md),
  [android-runtime](../architecture/android-runtime.md) section 8 (the
  fetched-image pattern), [local-ai](../architecture/local-ai.md).
