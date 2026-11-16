<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# macos-darling-1 — the macOS route in Darling: in the image, kept updated; runtime manager, router, tests

> **Status: built, NOT RUN.** Branch `feat/macos-darling` (pull request 147).
> Everything here was written without a compiler or a run: GitHub Actions is
> disabled for the owner's account (a support ticket is open) after the
> project drove too much CI load, so no workflow has run on this branch, and
> nothing is compiled on a development machine (AGENTS.md). Every number that
> has to come from a run is marked **NOT RUN** or **estimate**. The three new
> workflows start only by hand (`workflow_dispatch`, two of them also by
> `workflow_call` from the third); the existing `rust` and image workflows will
> judge the Rust and image changes on the pull request once Actions is back.

```text
TUR DEVELOPMENT REPORT

Task ID:          macos-darling-1 ("implement Darling as well and do whatever
                  is needed", owner, 16 November 2026; "Darling should be
                  installed by default and then kept updated", owner, 16
                  November 2026)
Subsystem:        macOS applications (priority 8: optional, experimental):
                  the OS image (Darling stage), tur-runtime (macOS route),
                  tur-catalogue (router rule), Darling's build
                  (scripts/darling, third-party/darling)
Build ID:         none: nothing was built
Source revision:  feat/macos-darling (pull request 147)

SUMMARY
- What changed:
  * Delivery: Darling is installed by default, in the OS image, read-only
    at /usr/lib/tur/darling, and kept updated through TUR's signed image
    updates (it rolls back with the image). The image build never compiles
    Darling: it takes TUR's prebuilt archive by the SHA-256 in
    image/darling.lock. The archive is built from source in CI at a pinned
    commit (3fcd2e1a, 15 November 2026) with TUR's patch queue (empty), on
    the image's own pinned base, and published with its corresponding
    source to the repository's package registry by a dispatch-only workflow
    (darling-runtime). darling-update (dispatch; weekly proposed) builds a
    newer commit, runs the integration tests strictly, publishes, and opens
    a pull request moving the pins. On a system without Darling in its
    image, tur-runtime fetches the same archive by digest on first use (the
    fallback).
  * Per-application prefixes are layered: the application's own layer
    (empty copies of Darling's directories, its account files, its own
    data) under an overlay that bubblewrap mounts over Darling's read-only
    system. Estimated 10 to 80 MB per application instead of a 0.3 to 0.7
    GB copy.
  * tur-runtime: `mac identify | route | install | list | show | launch |
    uninstall` and `mac runtime status | fetch | remove`; one Darling prefix
    per application in the same bubblewrap sandbox as Wine; Darling
    unprivileged in its nonroot mode; install from .app, .dmg and zip
    (7-Zip's 7zz, already in the base image, in the sandbox; nothing
    mounted) or .pkg (Darling's installer); Mach-O load-command analysis
    that refuses arm64-only, SwiftUI, system-Swift, encrypted and non-macOS
    programs with plain reasons; codes TUR-RUNTIME-MACOS-001..020; three
    class-0 P@x tools, install and uninstall declared class 2, not offered.
  * Router: Darling is offered only for applications TUR's own tests
    measured to work in it; outside reports (Darling's AppDB) are shown and
    never enable it; otherwise Darling is listed blocked with the reason.
    Order for a Mac package: Linux edition, re-hosting, measured Darling,
    refusal.
  * Licences and source: every licence file of Darling and its submodules
    printed for review (licences.md); publishing refuses any text a person
    has not read (scripts/darling/licences-reviewed.txt); the corresponding
    source archive published beside each build and pinned by digest.
  * Tests: unit tests on Mach-O files generated in the tests, CLI tests,
    router invariants; the integration workflow installs the build where
    the image puts it and installs, starts and removes ladder apps built by
    the macfw oracle (artifact reused, no new macOS job).
- Why it changed: the owner's instructions and decisions of 16 November
  2026 (section 8).

FILES
- Added: image/darling.lock, image/darling/darling.sh,
  services/tur-runtime/src/macos/{mod,loadcmd,analysis,package,darling,
  plan,record,route}.rs, services/tur-runtime/src/mac_cli.rs,
  services/tur-runtime/darling/runtime.json,
  services/tur-runtime/tests/mac_cli.rs, schemas/mac-app-record.schema.json,
  scripts/darling/{Containerfile,Containerfile.containerignore,
  build-darling.sh,check-tree.py,licence-summary.py,publish.sh,
  licences-reviewed.txt,darling.lock},
  third-party/darling/{README.md,LICENSE,patches/series} (GPL-3.0-or-later),
  tests/macos-darling/{ci-run.sh,in-container.sh,integration.py,
  expected.json}, .github/workflows/{darling-runtime,darling-update,
  macos-darling}.yml, docs/reports/macos-darling-1.md
- Modified: image/Containerfile (a Darling stage that unpacks the pinned
  archive), image/README.md, services/tur-runtime/src/{codes,sandbox,exec,
  plan,prefix,runtime_cmd,desktop,lib,main}.rs (new codes; an overlay and
  two read-only/alias mounts in the sandbox; a mirror-directories plan
  step), services/tur-runtime/agent-tools*.json,
  services/tur-runtime/tests/agent_tools.rs,
  services/tur-catalogue/src/route.rs, services/tur-catalogue/tests/
  {router_space,seed}.rs, desktop/apps/tur-centre/fixtures/vocabulary.json,
  desktop/apps/tur-centre/qml/RouterWords.qml,
  docs/architecture/{runtime-manager,catalogue-and-routing}.md,
  docs/dependencies/upstream-projects.md,
  tests/experiments/macfw/ci/oracle-macos.sh (two disk images per app)
- Removed: none

BUILD
- Command: rust and image workflows on the pull request; darling-runtime
  (dispatch) for Darling; nothing on a development machine
- Result: NOT RUN
- Warnings: NOT RUN. The Rust was written without a compiler and reviewed
  once by reading: expect rustfmt fixes (scripts/dev/ci.py fixes) and
  possibly compile or clippy errors on the first run. Cargo.lock does not
  change: no new crate. The image build is unchanged in effect until
  image/darling.lock pins a published build (the Darling stage is empty).

TESTS
- Unit tests: written, NOT RUN (section 7)
- Integration tests: workflow macos-darling, written, NOT RUN
- Boot tests: NOT RUN. The image change adds files under /usr only and
  nothing that runs at boot; it is inert until a build is pinned.
- Hardware tests: NOT RUN (none needed: Xvfb)
- Performance tests: NOT RUN (sizes and times below are estimates)
- Run on the development machine: python3 -m py_compile of the Python
  programs, bash -n / sh -n of every script, YAML parse of the workflows,
  JSON parse of every JSON file, sha256sum of the patch queue's series file.

DIAGNOSTICS
- Log paths: $XDG_STATE_HOME/tur/runtime/logs/<app-id>/ (one per launch,
  operations.log); .../logs/darling/operations.log (fetch, remove,
  uninstall); artifacts darling-runtime (archives, licences.md,
  build-record.json), darling-published (manifest-entry.json),
  macos-darling-results (results.json, summary.md, screenshots, logs)
- Diagnostic bundle path: none (the runtime manager has no bundle yet)
- Relevant error IDs: TUR-RUNTIME-MACOS-001..020 (section 4.6)

RECOVERY
- Rollback procedure: Darling in the image rolls back with the image (bootc
  rollback to the previous deployment). The change itself: revert the pull
  request; while image/darling.lock pins nothing, no image differs. On a
  computer: `tur-runtime mac uninstall` per application; `mac runtime
  remove` deletes a fetched fallback build. A failed installation rolls
  itself back.
- Recovery tested: NOT RUN

KNOWN LIMITATIONS
- Section 9.

NEXT SAFE ACTION
- When Actions is back: the rust workflow judges the pull request (apply
  its rustfmt and lockfile fixes); dispatch darling-runtime (publish off);
  a person reads licences.md, summarises it in section 8.3 and lists the
  texts in scripts/darling/licences-reviewed.txt; dispatch exp-macfw once
  (it makes the applications and their disk images); dispatch macos-darling
  with both run ids; then darling-runtime with publish on; the owner makes
  the package public; then a pull request pins it in image/darling.lock and
  runtime.json (section 8.4).
```

## 1. The answer in short

**Darling is part of every TUR image, from a prebuilt, digest-pinned archive,
and is updated with the image.** TUR builds it from source in CI, at a pinned
commit with its own patch queue, on the image's own base; the image build only
fetches, checks and unpacks it. It runs unprivileged (no set-user-ID, no kernel
module) in Darling's `nonroot` mode inside the same bubblewrap sandbox as Wine,
one prefix per application, each prefix a thin layer over Darling's read-only
system. A macOS package goes to Darling only when its program can run there
(x86-64 code, no SwiftUI, not Swift on macOS's runtime, not encrypted) and, for
the router, only when TUR's own tests measured the application to work in
Darling; everything else is refused with a plain reason. Nothing of this has
run.

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
| The copy is a plain read/write copy, no reflink; a file whose modification time equals the original's and whose mode is the same is left alone (the non-root copier changes nothing then), a newer one kept | `prefix-setup.cpp`, `copyEntry`, `updateFileAttributes` |
| The launcher takes a non-empty prefix as initialised when `var`, `private`, `private/etc`, `Volumes`, `usr`, `var/run` are directories and `private/etc/passwd`, `master.passwd`, `group` regular files; it then skips its set-up (which writes those three files with a root line and one line for the user) and only adds missing service accounts | `prefix-state.h`, `darling.c` (`setupPrefix`, `ensureSystemAccounts`) |
| The launcher makes `<prefix>.workdir` beside the prefix; `darling shell a b` runs `sh -c "'a' 'b'"` | `darling.c` |
| Disk images: Darling's `hdiutil` uses `darling-dmg`, a FUSE module for HFS+ only (GPL-3) | `darlinghq/darling-dmg` README |
| Swift: `darling-swift` holds "unmodified binaries from `usr/lib/swift/macosx`" of swift.org's 6.3.3 macOS package, plus overlays from Swift 5.2.2, also unmodified; overlays built from source only "when the build finds a Swift toolchain" | `darling-swift` README |
| Debian packaging builds everything, then installs per component; the RPM directory is "not quite ready"; no Fedora package, no COPR | `debian/rules`, `rpm/`, exp-macfw-0 |

Fedora 45: `7zip` 26.04 is in the base image (`7zz`;
docs/dependencies/image-licences.csv); 7-Zip reads DMG with LZFSE since 18.01,
XZ in DMG since 24.01, APFS since 22.00 (7-Zip's history file).
`bubblewrap` 0.12.0 (overlay mounts since 0.11). `dmg2img` and `apfs-fuse` are
2020 snapshots (HFS+ only; FUSE).

## 3. Delivery

### 3.1 The decision: in the image, prebuilt, kept updated (owner, 16 November 2026)

The owner decided: "Darling should be installed by default and then kept
updated." Earlier in the day this branch had chosen a separate archive fetched
on first use (Darling outside the image); that is now the fallback only.

| | What |
|---|---|
| Built | From source, by `darling-runtime` (dispatch only), at the commit in `scripts/darling/darling.lock`, with `third-party/darling/patches`, on `image/base.lock`'s base (so it links against the image's libraries), network only in the fetch step, compiled with `--network=none`, deterministic tar |
| Published | With its corresponding source, to `ghcr.io/giorgiogiudice/tur-darling-runtime:<commit12>-q<queue8>-fc45-x86_64`, by `scripts/darling/publish.sh`, only when the lead sets `publish`, and only when every licence text has been read |
| Into the image | `image/Containerfile`'s stage `darling` (`image/darling/darling.sh`) fetches the archive by the SHA-256 and size in `image/darling.lock`, checks both, unpacks it read-only into `/usr/lib/tur/darling`, writes `/usr/lib/tur/darling.json`, copies the licences to `/usr/share/licenses/tur-darling`. The image build never compiles Darling. While the lock pins nothing, the stage is empty |
| Kept updated | Through TUR's own signed image updates: a new Darling arrives with a new image and rolls back with it. `darling-update` (dispatch; proposed weekly schedule `17 4 * * 1`) picks the newest Darling commit whose own CI built it, builds it, runs `macos-darling` strictly on it (no lost refusal, clean removals, every application of `tests/macos-darling/expected.json` still shows its window), publishes it (refused while a licence text is unread) and opens a pull request moving `scripts/darling/darling.lock`, `image/darling.lock` and `runtime.json`; review and CI as usual. Opening the pull request needs the repository setting that allows Actions to create pull requests |
| Fallback | For a system without Darling in its image (an image built before the first publish, plain Fedora, test containers), tur-runtime fetches the same archive by digest from `runtime.json` when the first macOS application is installed. **Kept, as the fallback only**: when the image carries Darling, nothing is fetched (`mac runtime fetch` says so), and every application runs on the image's build |
| What is never shipped | Apple-built binaries (the swift.org Swift runtime Darling vendors), SDK headers and stubs, Darling's TextEdit sample (Apple sample code), anything Apple-signed: removed or refused by the build. APSL-2.0 code from Apple's open-source releases ships, with its notices and source (owner's decision 8.1) |

### 3.2 Image reproducibility

The image's rules hold: the Darling stage pins its input by SHA-256 and size,
fetches nothing else, compiles nothing, and checks what it unpacks. The archive
itself is built from pinned inputs without network; two builds giving the same
bytes is designed for and not verified (section 9).

### 3.3 Size and time (estimates; NOT RUN)

| Quantity | Estimate | From |
|---|---|---|
| Archive (registry, and the image layer compressed) | 90 to 150 MB | Darling's stock amd64 packages are 147 MB with i386, Python, Ruby, Perl and JavaScriptCore; TUR builds fewer components, no i386, and removes the Swift runtime |
| Image growth, unpacked | 0.4 to 0.8 GB | "up to 1 GB" for a stock installation |
| Image update when Darling moves | the changed files of the tree (an OSTree delta), up to the archive's size | |
| Per installed application (layered prefix) | 10 to 80 MB beside the application: the empty directory copies (one per directory of Darling's system; their number is NOT RUN), the account files, and what the application writes | section 4.1; the integration test records `prefix_bytes` |
| Per application without layering (`--prefix-layout darling`) | 0.3 to 0.7 GB | Darling's own copy |
| Corresponding source archive | 0.5 to 0.6 GB in the registry, not in the image | Darling's own `darling-source.tar.xz`: 506 MB |
| Cold build | 60 to 150 minutes on a hosted runner (timeout 300) | 15 minutes warm for more components in Darling's CI |
| Warm build (ccache) | 15 to 30 minutes | |
| Image build | unchanged but for one download of the archive | |

## 4. The macOS route in tur-runtime

The design is in [runtime-manager.md, "Running a macOS
application"](../architecture/runtime-manager.md#running-a-macos-application);
the decisions are summarised here.

### 4.1 Sandbox, mode and layered prefixes

Darling runs in tur-runtime's own sandbox (system read-only, prefix and private
home read-write, nothing else of the person's files, no session bus), with three
additions in `sandbox.rs`: Darling's tree read-only at `/opt/tur-darling` (the
path it was built for; the image keeps it at `/usr/lib/tur/darling`); the prefix
a second time at `/run/tur-runtime/prefix`, because Darling's Unix sockets live
in the prefix and a socket path holds at most 107 bytes; and, for a layered
prefix, an overlay that bubblewrap mounts at `/run/tur-runtime/prefix/darling`
(`--overlay-src <Darling's libexec/darling> --overlay <layer> <layer>.work
<target>`; the layer must be inside the application's prefix). Darling's
`/Volumes/SystemRoot` is the sandbox's root, not the computer's.

The mode is `nonroot`: no namespaces of Darling's own, so the sandbox keeps
`--disable-userns`. Left alone, `nonroot` copies Darling's whole system into each
prefix. With Darling's system read-only in the image, tur-runtime layers the
prefix instead, using two behaviours read in Darling's source (section 2): the
launcher's test of an initialised prefix, and the copy pass leaving alone a file
with the original's time and mode. At creation tur-runtime writes the
application's layer: an empty, user-owned copy of every directory of Darling's
system (a new plan step, `mirror-directories`, repeated at every launch for
directories a newer Darling brings), the directories Darling's own set-up makes,
and the three account files with the lines Darling writes for the user. Darling
then finds the prefix initialised and every system file in place (they are the
originals, through the overlay), and copies nothing. Writes land in the layer.
Because the system shows through, an image update's newer Darling serves
existing applications at once.

`rootless` (Darling's own namespaces and overlay) stays available for
measurement with the `darling` layout; I expect its `/proc` mount to be refused
under bubblewrap (bubblewrap's `/proc` has read-only mounts over parts of it).
`--prefix-layout darling` also gives `nonroot` its own full copy, in case the
layered prefix fails where the test cannot see.

### 4.2 Installing without mounting

| Package | How | Why |
|---|---|---|
| `.dmg` (UDIF; raw HFS+ or APFS) and zip | 7-Zip (`7zz`, in the base image; `7z` as a fallback) in the sandbox, into the prefix's `unpacked/`; a partition image 7-Zip leaves is unpacked in turn | No FUSE, no loop device, no root; every one of 7-Zip's parsers runs where only the prefix and the private home can be written |
| `.pkg` | Darling's `installer -pkg … -target /`, in Darling and the sandbox | Packages carry scripts only a macOS-like system can run |
| `.app` | `cp --archive --no-dereference --reflink=auto` into the layer's `Applications`, in the sandbox | Links inside the bundle stay links and are never followed |

### 4.3 What the analysis refuses

From the Mach-O header and the load commands of the slice Darling would run,
read with the component's bounded reader:

| Program | Answer |
|---|---|
| Apple silicon only (arm64) on an x86-64 PC | Refused: "It is built only for Apple silicon (arm64). Darling runs only programs built for the computer's own processor, this computer is x86-64, and nothing translates arm64 code for it. A version of the application for Intel Macs, or a universal one, would run." (`-002`) |
| Universal | The x86-64 slice is used |
| SwiftUI (required) | Refused (`-004`); weakly linked: a warning |
| Swift on macOS's runtime (`/usr/lib/swift/libswift*`) | Refused (`-005`): TUR's build does not ship Apple's Swift runtime, and Swift applications from current Xcode do not run in Darling (#2059) |
| Swift with its own runtime | Refused unless `--allow-swift` |
| Encrypted (App Store) | Refused (`-006`) |
| iOS or another platform; Mac Catalyst | Refused (`-017`); Catalyst unless allowed |
| 32-bit Intel, PowerPC, not a program | Refused (`-003`) |
| Private frameworks | Warning |

### 4.4 Launch, removal, record

Launch: `darling shell /Applications/<bundle>/Contents/MacOS/<program>` with
the image's Darling (or, without it, the fetched build the application was
installed with). Removal: the application's directory (layer, prefix, private
home, record), staging, launcher entry and logs, each checked to be gone; only
a directory with a macOS record is ever removed. Record:
`mac-application.json` (`schemas/mac-app-record.schema.json`, now with the
prefix layout), written last.

### 4.5 Router

Darling is offered only for a macOS edition with a `darling` record of TUR's own
tests (`reported_by = "tur-test"`) rated `good` or `usable`; outside reports are
shown with the option and never enable it (owner's decision 8.5); otherwise
Darling is listed blocked with `experimental.darling-not-measured`. Final Cut
Pro has no route even with Darling enabled. `tur-runtime mac route` applies the
full order (Linux edition, re-hosting with the re-hosting component's verdict
as input, measured Darling, refusal `-020`).

### 4.6 Codes

`TUR-RUNTIME-MACOS-001` to `-020`, listed in runtime-manager.md.

## 5. Licences

| What | Licence | Where it lives |
|---|---|---|
| tur-runtime's macOS route, the build, publish and test scripts, the workflows, the image's Darling stage | TUR's licence | `services/`, `scripts/darling/`, `image/darling/`, `tests/macos-darling/` |
| TUR's patches to Darling (none yet), the queue's README and series | GPL-3.0-or-later, or the licence of the changed file | `third-party/darling/` (marked) |
| Darling, as built | GPL-3.0-or-later; components MIT (Cocotron), LGPL-2.1 (Foundation), APSL-2.0 (Apple's open-source releases), BSD, Apache-2.0 and others | the image (`/usr/lib/tur/darling`) and the registry, with every licence file found and `SOURCE.txt` |
| Darling's corresponding source | as above | the registry, beside each archive, by digest |
| 7-Zip | LGPL-2.1-or-later with BSD parts | Fedora's package in the base image, a separate program |
| ORAS, used in CI | Apache-2.0 | the CI runner only |

tur-runtime starts Darling as a separate program and never links it.

## 6. Tests (written; NOT RUN)

| Test | What it holds |
|---|---|
| `macos::loadcmd` (8) | libraries of every link kind, rpaths, build and old version commands, Swift sections, encryption, a second platform, malformed sizes, commands past the block, section counts that do not fit, hostile names, limits through the bounded reader |
| `macos::analysis` (10) | runnable Objective-C AppKit program; universal; arm64-only refused with its sentence; i386, dylib, kext, script; SwiftUI required and weak; Swift with the system runtime whatever is allowed; embedded Swift refused unless allowed; encrypted, Catalyst, iOS, private frameworks; a bundle through its main program |
| `macos::package` (3) | every kind by content, short files; the bundle search and the choice |
| `macos::darling` (11) | the compiled manifest; every manifest field's rule, the source pinned with a published build; registry and source addresses; tokens; the token never on a command line; digests; status from disk; the image's build wins and nothing is fetched; the record written last; the system release |
| `macos::plan` (10) | Darling's command inside the sandbox boundary with the overlay (system read-only, layer inside the prefix), variables refused, root refused, the layered prefix's seed (directory mirror, skeleton, account files) and the rootless/layout rule, 7-Zip without Darling, the copy into the layer, the package installer, launch through the shell with the directory mirror first, removal checked, the desktop entry |
| `macos::record` (2), `macos::route` (3), `sandbox` (1 new), `codes` (1 new) | schema and type in step; route order; the new mounts; the codes |
| `tests/mac_cli.rs` (5) | identify, route, install refusals before anything is made, the fallback's digest rules, list/show/launch refusal/uninstall |
| `tests/agent_tools.rs` | the three class-0 tools, the two class-2 tools declared and not offered |
| tur-catalogue | the router rule (outside reports never enable Darling, TUR's test does), `macos_edition_space` (36 864 cases), Final Cut Pro |
| `macos-darling` workflow | installs the build at `/usr/lib/tur/darling` with the image's own script; checks tur-runtime finds it and fetches nothing; identifies every oracle app with the expected refusals (SwiftUI `-004`, Swift `-005`, an arm64 slice `-002`); installs rung 1 and Table Tool from the bundle, a zip and HFS+ and APFS disk images; starts each under Xvfb, waits for a window, screenshots, types into rung 1 and looks for its line; records each prefix's size; removes and checks nothing is left; `--strict` for darling-update |

## 7. Answers to the review of the first version

The lead's corrections of the same day are applied: 7-Zip is in the base image
(`7zz`, looked for first); "measured" is TUR's own measurement only; the
packaging moved into the image (section 3).

## 8. The owner's decisions (16 November 2026) and the owner's checklist

### 8.1 Decisions

1. **"Implement Darling as well and do whatever is needed."**
2. **TUR's Darling fixes stay inside TUR**: a patch queue on a pinned Darling
   commit (`third-party/darling/patches`, GPL-3.0-or-later), not a fork, not
   upstreamed.
3. **"Darling should be installed by default and then kept updated."** In the
   image, from TUR's prebuilt archive pinned by digest (`image/darling.lock`);
   the image build never compiles it; updated through TUR's signed image
   updates, proposed by `darling-update`.
4. **APSL-2.0 code may ship.** Apple's open-source code under APSL-2.0 inside
   Darling builds is not what "never ship Apple binaries" forbids (that rule is
   about Apple's proprietary software); it ships with APSL's notices and
   source availability. **On the lawyer's list.**
5. **TUR publishes the corresponding source itself**: a source archive of the
   exact pinned commit with submodules and TUR's patch queue, beside each
   build in the same registry package, its digest pinned in the manifest
   (`source_sha256`) and the image lock.
6. **The first publish is blocked until the licence files have been read** and
   summarised here (8.3); the build prints them (`licences.md`), and
   `publish.sh` refuses any text not listed in
   `scripts/darling/licences-reviewed.txt`.
7. **Package visibility** is an owner action after the first publish.
8. **"Measured to work" means TUR's own measurement only**: records of TUR's
   tests; outside reports such as Darling's AppDB are shown as information and
   never enable the Darling route.

### 8.2 Owner's checklist, in order

- [ ] Actions back; the `rust` workflow green on pull request 147 (apply CI's
      rustfmt and lockfile fixes).
- [ ] `darling-runtime` dispatched, publish off; its build record shows no
      violation and no missing host library.
- [ ] Licences read (8.3), summarised below, and every text listed in
      `scripts/darling/licences-reviewed.txt`, in a pull request of its own.
- [ ] `macos-darling` run on that build (with an `exp-macfw` run's apps); the
      results looked at.
- [ ] First publish: `darling-runtime` with publish on.
- [ ] **The package `tur-darling-runtime` made public** in its settings on
      GitHub (owner action).
- [ ] A pull request that pins the published build in `image/darling.lock` and
      `services/tur-runtime/darling/runtime.json` (the values `publish.sh`
      printed); the image built and booted with it. From then on Darling is on
      every new TUR image.
- [ ] Repository setting: allow GitHub Actions to create pull requests (for
      `darling-update`), and the weekly schedule switched on when CI load
      allows.
- [ ] Lawyer: APSL-2.0 object code in the image and the registry (8.1, item 4);
      the corresponding source published by TUR (item 5).

### 8.3 Licence review (to be filled before the first publish)

| Check | Done |
|---|---|
| `licences.md` of the build read in full (every distinct text) | not yet |
| Repositories with **no licence file** listed, and their source headers read | not yet |
| Every licence found is one TUR may redistribute in binary and source form (GPL-2.0/3.0, LGPL, APSL-2.0, MIT, BSD, Apache-2.0, ISC, Zlib, public domain…); anything else named here with a decision | not yet |
| GPL-2.0-only parts, if any, checked for combination with GPL-3.0 parts in one program | not yet |
| APSL-2.0 notices present in `share/licenses/darling` | not yet |
| Summary written here: repositories, licences, numbers | not yet |
| `scripts/darling/licences-reviewed.txt` lists every text read | not yet |

### 8.4 After the first publish

`image/darling.lock` gets `DARLING_ID`, `DARLING_TAG`, `DARLING_SHA256`,
`DARLING_SIZE`, `DARLING_SOURCE_SHA256`, `DARLING_SOURCE_SIZE`; `runtime.json`
gets the same entry. Every image built from then on carries Darling.

## 9. Known limitations and follow-ups

- **Nothing has run.** No compile, no Darling build, no installation, no
  window. Every statement about behaviour is from Darling's source and docs.
- **Rust written without a compiler.** Expect rustfmt changes and some compile
  or clippy fixes on the first CI run.
- **Host libraries.** Whether everything Darling links is in the Kinoite base
  is unknown until the build's check runs. Likely gaps: FFmpeg's libraries
  (`libavcodec-free` and friends), `giflib`, `libbsd`, `libxkbfile`. If so:
  add the runtime packages to the image (small), or drop the components that
  need them. The build refuses to publish meanwhile.
- **Layered prefixes are new territory** (NOT RUN):
  - the overlay's lower layer is on the image's composefs root, itself an
    overlay; the kernel allows two levels of stacking, so this is at the
    limit;
  - writing to a system file through the overlay needs a copy-up of a file
    owned by root in the image, which may fail in the sandbox's user
    namespace (applications normally write only to their home and
    `Library`); the `darling` layout is the way out;
  - overlayfs's work directory may hold entries the user cannot list, which
    could stop `mac uninstall`'s deletion (it would report it, `-015`).
- **MIT-SHM.** The sandbox has its own SysV IPC namespace; whether Darling's
  AppKit and Mesa fall back cleanly is NOT RUN.
- **`rootless` mode** is expected to fail inside bubblewrap.
- **Wayland** is not tested (X11 under Xvfb).
- **arm64 computers.** Only an x86-64 build is planned; on arm64 the fallback
  reports `-008` and the image carries nothing.
- **`.pkg` installation** depends on Darling's `installer`; untested here.
- **Inspection** (`tur-runtime inspect`) still detects Darling as a `darling`
  program in `PATH` and does not know `/usr/lib/tur/darling`.
- **Desktop integration.** The launcher entry is staged, not installed; no icon
  from the bundle (`.icns` is not read).
- **SBOM.** The image's SBOM scripts do not yet list Darling's components;
  `/usr/lib/tur/darling.json` and `share/licenses` are where it would read
  them.
- **Reproducibility** of the archive is designed for and not verified.
- **Darling outruns the pin.** `darling-update` exists for that; until Actions
  allows a schedule, each update is a dispatch.

## 10. Proposed decision record (the lead assigns the number)

```text
ADR-XXXX: Darling as TUR's macOS route for AppKit applications, in the image

Status: proposed (macos-darling-1, 16 November 2026)

Context
  TUR runs macOS applications as an optional, experimental feature
  (priority 8). Three routes exist or are planned: a native Linux edition;
  re-hosting a package's portable payload on the native runtime
  (exp-rehost-1: 38 of 47 macOS packages); and, for native Mac code,
  Darling, the only project that runs unmodified Mach-O programs on Linux.
  Darling runs x86-64 Mach-O programs on x86-64 PCs, has AppKit through
  Cocotron, no SwiftUI, and vendors Apple's Swift runtime binaries. Fedora
  does not package it. The owner decided on 16 November 2026 to implement
  it, to have it installed by default and kept updated, and to keep TUR's
  fixes inside TUR.

Decision
  1. Route. Darling is TUR's route for macOS applications whose program is
     native Mac code it can run: x86-64 code, not SwiftUI, not Swift on
     macOS's runtime, not encrypted. The order for a macOS package is:
     native Linux edition, re-hosting, Darling for applications TUR's own
     tests measured to work in it, otherwise a refusal with the reason.
     Outside reports never enable Darling.
  2. Delivery. Darling is part of the OS image, read-only in
     /usr/lib/tur/darling, from a prebuilt archive pinned by SHA-256 in
     image/darling.lock. The image build never compiles Darling. TUR builds
     the archive from source in CI at a pinned commit with its patch queue,
     on the image's pinned base, and publishes it with its corresponding
     source to the repository's package registry when the lead dispatches
     it, after every licence text has been read. The archive contains no
     Apple-built binary, no SDK header, no Apple sample application;
     APSL-2.0 code from Apple's open-source releases ships with its
     notices and source.
  3. Updates. Darling is updated only through TUR's signed image updates and
     rolls back with the image. A newer Darling is proposed by a workflow
     that builds it, runs the integration tests strictly, publishes it and
     opens a pull request moving the pins. Systems without Darling in their
     image fetch the same archive by digest on first use (fallback only).
  4. Running. Darling runs unprivileged (no set-user-ID, no kernel module),
     in its nonroot mode, inside tur-runtime's bubblewrap sandbox, one
     prefix per application; each prefix is the application's own layer
     over Darling's read-only system, an overlay mounted by bubblewrap.
  5. Patch-queue policy. TUR's fixes to Darling are a patch queue in
     third-party/darling (GPL-3.0-or-later, or the changed file's licence),
     applied to the pinned commit at build time; not a fork; kept in TUR and
     not offered upstream. Each patch has one stated reason, evidence and
     provenance; is written clean-room (public documentation and observed
     behaviour only); is general, never keyed on one application; and is
     rebased or dropped whenever the pin moves. Offering a patch upstream
     would need the owner's decision and a named person who answers for it.

Consequences
  + Darling is on every TUR computer from installation, updated and rolled
    back with the system; image builds stay fast (no compilation).
  + Per-application prefixes cost tens of megabytes, not a copy of Darling.
  + The same sandbox, plans, logs and removal guarantees as Windows
    applications; P@x can identify and list.
  - The image grows by about 0.4 to 0.8 GB for a priority-8 feature.
  - TUR publishes GPL-3.0 and APSL-2.0 object code and its source, and must
    keep both available.
  - Swift, SwiftUI and Apple-silicon-only applications are refused; the
    share of the latter will grow from 2027.
  - The archive must be rebuilt whenever the image's base moves to another
    Fedora release; until it is, a base-pin change and a Darling-pin change
    go together.
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
- 7-Zip: https://www.7-zip.org/history.txt, https://www.7-zip.org/license.txt;
  docs/dependencies/image-licences.csv (`7zip` 26.04 in the base).
- Fedora 45 packages: https://mdapi.fedoraproject.org/f45/ (`bubblewrap`,
  `dmg2img`, `apfs-fuse`).
- ORAS: release v1.3.6 (24 October 2026), its asset digests and checksums file.
- TUR: [exp-macfw-0](exp-macfw-0.md), [exp-rehost-1](exp-rehost-1.md),
  [runtime-manager](../architecture/runtime-manager.md),
  [catalogue-and-routing](../architecture/catalogue-and-routing.md),
  [android-runtime](../architecture/android-runtime.md) section 8,
  [local-ai](../architecture/local-ai.md), image/README.md.
