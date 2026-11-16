<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# rehost-route-1 — re-hosting becomes a route: `tur-rehost`, the router's `rehost`, `tur-apps`

> **Status: written, not run.** GitHub Actions is disabled for the owner's
> account (support ticket open). Nothing in this change has been compiled,
> tested or run anywhere: no Rust compiler exists on the development machine
> (AGENTS.md), and no workflow can start. Every "test" below is a test that
> will run when CI is back. The only measurements quoted are the
> experiment's ([exp-rehost-1](exp-rehost-1.md)), made with its Python
> prototype, not with this program.

```text
TUR DEVELOPMENT REPORT

Task ID:          rehost-route-1
Subsystem:        application compatibility: a new route between native
                  Linux and Wine (services/tur-rehost), the catalogue and
                  router (services/tur-catalogue), the installer back end
                  (services/tur-apps), the application centre's vocabulary
Build ID:         none: nothing was built (CI unavailable)
Source revision:  feat/rehost-route (pull request 146)

SUMMARY
- What changed: the experiment's prototype became a TUR component,
  tur-rehost (Rust): it opens a Windows or macOS package in a sandbox
  without running it, detects the runtime, plans every download from a
  pinned index and refuses before fetching when the re-host cannot work,
  fetches Electron's Linux build at the payload's exact version (verified
  by SHA-256), lays the payload under it, swaps native Node modules
  (fetched prebuilds, or built on the person's computer without network
  when allowed), launches it in a bubblewrap sandbox as a native window,
  and removes it without leftovers. The catalogue gained the route
  `rehost`, offered only for an edition a TUR record measured (at least
  starting and showing its first window);
  the experiment's 40 working variants are the seed records (20 new
  entries). tur-apps installs by the route from the person's own file.
- Why: the experiment passed its kill criterion by a wide margin (20 of 24
  Electron applications from each platform's package) and asked for the
  route (exp-rehost-1, sections 9 and 12).

FILES
- Added: services/tur-rehost/ (Cargo.toml, src/*.rs, tests/*.rs,
  agent-tools.json, agent-tools.adapter.json, data/index.json,
  data/wanted.json, data/pin-index.py); schemas/rehost-record.schema.json;
  docs/architecture/rehost.md; docs/reports/rehost-route-1.md;
  catalogue/<20 entries>.toml; tests/rehost/{corpus.json,run.sh,
  integration.py}; .github/workflows/rehost.yml
- Added (image): image/archive-tools/{lock.py,tools.lock,tools.sh}
- Modified: image/{programs.tsv,Containerfile,Containerfile.containerignore,
  README.md}; docs/dependencies/image-licences.csv (the eleven added
  packages); .github/workflows/{image,boot}.yml (triggers, shellcheck);
  tests/image/smoke.sh; tests/boot/{report.py,guest/Containerfile,
  guest/rootfs/usr/libexec/tur-boot-test/component-checks};
  docs/architecture/pax-orchestrator.md (tool table); crates/tur-core (REHOST family); services/tur-catalogue
  (model, validator, router, host facts, report, tests); services/tur-apps
  (src/{plan,ops,records,paths,cli,main,lib,exec,codes}.rs, new
  src/rehost.rs, tests/{cli,interfaces}.rs, tests/fakes/tur-rehost,
  dbus/org.tur.Apps1.xml comments, Cargo.toml); schemas/{app-catalogue-entry,app-install-plan,
  app-installed}.schema.json; desktop/apps/tur-centre (vocabulary and validate
  fixtures, Words.qml, RouterWords.qml, answers.cpp, three QML tests);
  docs/architecture/{catalogue-and-routing,runtime-manager,
  app-install-backend,desktop-experience,specification}.md;
  catalogue/README.md
- Removed: none

BUILD
- Command: none locally (AGENTS.md); in CI the rust workflow builds every
  component, including the new one
- Result: NOT RUN
- Warnings: the new component has no Cargo.lock and was formatted by hand:
  the first CI run will produce CI's lockfile and rustfmt patch
  (scripts/dev/ci.py fixes), as AGENTS.md describes

TESTS
- Unit tests: written (tur-rehost: every module; tur-catalogue: router,
  validator, seed; tur-apps; tur-core); NOT RUN
- Integration tests: tests/rehost (five open-source packages of the
  experiment's corpus, by hand only: workflow rehost); tur-rehost
  tests/cli.rs on synthetic packages; NOT RUN
- Boot tests: written (tur-rehost runs on the installed system, detect on
  a synthetic fixture); NOT RUN
- Hardware tests: NOT RUN (no hardware runner)
- Performance tests: NOT RUN

DIAGNOSTICS
- Log paths: $XDG_STATE_HOME/tur/rehost/logs/<app-id>/ (one log per
  launch); the record $XDG_DATA_HOME/tur/rehost/apps/<app-id>/record.json
- Diagnostic bundle path: not collected (not built)
- Relevant error IDs: TUR-REHOST-* (docs/architecture/rehost.md, section 9)

RECOVERY
- Rollback procedure: revert the pull request (the image loses
  /usr/bin/tur-rehost; nothing else in the boot path changes); a re-hosted application is removed with `tur-rehost uninstall`
  (or by deleting ~/.local/share/tur/rehost/apps/<app-id>)
- Recovery tested: no

KNOWN LIMITATIONS
- Section 6.

NEXT SAFE ACTION
- When Actions is back: run the rust workflow on the branch, apply CI's
  lockfile and rustfmt fixes, fix what clippy and the tests find; then run
  the rehost workflow by hand; then the owner's decisions in section 8 and
  the lawyer's answers in section 9.
```

## 1. What was asked, and what was decided

| Asked | Decided | Why |
|---|---|---|
| The component: `services/tur-rehost`, or a route inside `tur-runtime` | **A separate program, `services/tur-rehost`** | `tur-runtime` promises never to fetch and never to decompress (runtime-manager.md); re-hosting must do both. Separate lockfile and dependency surface (`sha2`, the system's `curl`, as `tur-apps`); one route, one program, one P@x service. The model is shared, not the code (rehost.md, section 2) |
| Codecs and runtimes | **Electron's Linux release, unchanged, fetched by the person's computer on first use, never in the image; `--codecs open-only` built as the alternative** | The owner's instruction for this slice (as Proton is fetched). Exactly what is fetched, how it is verified and what the person is told: section 3 |
| Native modules that must be built (53 of 138 in the experiment) | **On the person's computer, in a container of Fedora's own toolchain, without network, only when the person allows it** | Feasible and offline-safe (section 4). It differs from the experiment's proposal (a TUR build service): TUR would otherwise distribute binaries compiled from third-party source and run a signing and serving infrastructure that does not exist |
| The router | **`rehost` offered only for an edition whose TUR record measured it working**, between Linux editions and Wine; the 40 working variants seeded with their evidence | exp-rehost-1, 9.1; the honesty rule of the catalogue |
| The application centre | **tur-apps wired; the centre's "I have the installer file" path is a follow-up** | The desktop programs pull request (#128) is open, not merged. The centre's vocabulary and words were extended only as far as its tests require, so that it keeps building |

## 2. The component

`services/tur-rehost` (design: [rehost.md](../architecture/rehost.md)):

```
tur-rehost detect <package> [--json]
tur-rehost plan <package> [--app-id ID] [--codecs upstream|open-only]
        [--allow-builds] [--allow-unresolved MODULE]... [--json]
tur-rehost install <package> [same] [--name NAME] [--json]
tur-rehost list | show <id> | launch <id> [--accessibility] | uninstall <id> | index
```

- **Detect** (ported from the prototype's `detect.py`, `binfmt.py`): the
  Electron payload in full (Windows and macOS layouts, universal macOS
  builds, `node_modules.asar` inner archives, fuses, the version from the
  user-agent string or the framework's property list, every `.node` file
  with its owning package and its `os` field); NW.js, .NET, Java, Godot,
  LÖVE, Ren'Py and PyInstaller recognised; native toolkits refused.
- **Open** (from `unpack.py`): zip, 7z, NSIS with electron-builder's
  `app-64.7z`, Inno Setup, MSI, Squirrel, dmg and its partitions, `.pkg`
  with its `pbzx` payload: Fedora's archive tools in a bubblewrap sandbox
  with no network and one writable directory. Installers are never run.
- **Plan**: every refusal before any download, with a code; every download
  with its size, origin, pinned SHA-256 and licence.
- **Install**: staged, rolled back on failure; the record written last.
- **Launch**: the payload's SHA-256 checked against the record (Electron's
  own integrity fuse must be off, because its expected hash lives in the
  publisher's binary), then the application sandbox: its runtime and payload
  read-only, its private home read-write, the display and sound sockets, the
  accessibility bus, the network; never `--no-sandbox`.
- **Uninstall**: everything of it deleted, then checked gone.
- **P@x**: `rehost_detect`, `rehost_plan`, `rehost_list`, `rehost_show`, all
  class 0. Installing is class 2 and is not a tool.
- **Error codes**: `TUR-REHOST-DETECT-001` to `-008`, `-INDEX-001`/`002`,
  `-FETCH-001`/`002`, `-NATIVE-001` to `003`, `-HOST-001` to `003`,
  `-APP-001` to `007`; a new `REHOST` family in `tur-core` and in the
  specification's §29.3.

## 3. Codecs and runtimes: what is fetched, from where, verified how, what the person is told

| | |
|---|---|
| **What** | Electron's Linux x86-64 release archive, `electron-v<version>-linux-x64.zip`, 110 to 127 MB, at exactly the version the payload names. With `--codecs open-only`, also `ffmpeg-v<version>-linux-x64.zip` (about 1.4 MB), Electron's media library built with open codecs only. For a native build, `node-v<version>-headers.tar.gz` (about 340 KB) |
| **From where** | The Electron project's GitHub releases (`https://github.com/electron/electron/releases/download/v<version>/`) and, for headers, `https://artifacts.electronjs.org/headers/dist/v<version>/`. A unit test holds every URL of the index to these hosts and the npm registry |
| **When, by whom** | By the person's computer, the first time an application needs that version, when the person installs it. Never in the image (the image contains `tur-rehost`, never Electron) |
| **Verified how** | Its SHA-256 and size against TUR's pinned index (`services/tur-rehost/data/index.json`), whose digests were taken from each release's `SHASUMS256.txt` (and agreed with GitHub's published asset digest for 18 of the 19 versions). `curl` reads no configuration, speaks HTTPS only, also on redirects, and stops at the pinned size. A file that differs is deleted; nothing in it is used (`TUR-REHOST-FETCH-002`). A cached file is checked again before each use. A version not in the index is refused (`TUR-REHOST-INDEX-002`), never fetched |
| **What the person is told** | Before anything is fetched, the plan says: "TUR downloads Electron's own Linux build from the Electron project's releases on GitHub, unchanged, and checks it against the SHA-256 TUR pinned. It is not part of TUR. It contains media codecs (H.264 and AAC) that TUR does not ship; the download is made by this computer, at your request, as with Proton." The plan lists each download with size and licence. The router's caveats `rehost.runtime-fetched` and `rehost.not-publisher-supported` say the same in the application centre. The record keeps what was fetched, from where, its digest and licence |

The index pins 19 Electron versions (those the 40 working variants need), 8
Linux siblings of per-platform npm packages, 4 npm prebuilds, 3 release
prebuilds and 26 source builds with their build-time packages. It was
written by `data/pin-index.py` on 16 November 2026 from the projects' own
pages (that script ran on the development machine: it downloads and hashes,
it builds and runs nothing); the 19 Electron digests are the ones the
experiment recorded.

## 4. Native modules: where builds happen

The decision: **on the person's computer, offline, with Fedora's
toolchain, only with permission.** Implemented in `src/build.rs` and
`src/natives.rs`:

1. Permission: `--allow-builds`; the router asks for it as the
   prerequisite `allow-rehost-builds`; tur-apps passes it on.
2. The environment, once: a podman image from Fedora 45's base image
   pinned by digest, with Fedora's `gcc-c++`, `make`, `python3`, `nodejs`,
   `nodejs-npm` (which carries node-gyp: verified in Fedora 45's package
   files) and the X11, xkbfile, libsecret and fontconfig headers. Fedora's
   registry and repositories, Fedora's signatures: the one network use.
3. Inputs pinned in the index and checked on the host: the module's npm
   tarball, the packages its `binding.gyp` requires (`node-addon-api` at
   the version the module declares), Electron's headers.
4. The build: `podman run --network=none --read-only --cap-drop=all
   --security-opt=no-new-privileges --userns=keep-id`, the source the only
   writable mount; only ELF x86-64 `.node` files are taken.

"Offline-safe" here means: the build itself has no network, every input is
pinned and verified before it starts, and a computer without network gets a
clear refusal before anything changes. It does not mean the first use needs
no network: the toolchain and inputs are downloaded once.

Modules with no Linux build TUR can supply (no identity, the application's
own module, a module not in the index) are refused unless the person
accepts running without them; `tur-apps` accepts exactly those the
catalogue record measured the application starting without, and only for
the measured package.

## 5. The router and tur-apps

- `Route::Rehost` (`rehost`) for Windows and macOS editions; a record of the
  route carries a `rehost` table: the package's file name, size and SHA-256,
  the runtime and version, the evidence run, the modules built from source
  and those the application started without. Only `tur-test` records may
  rate it.
- The ladder: Linux editions, the primary web application, another
  distribution's container, **re-hosted**, Proton or Wine (and the guest),
  Android, the web, Darling. A macOS edition with a measured re-host has a
  route without Darling. No record: no option, so Wine, Darling (when
  enabled) or a refusal with the reason, unchanged.
- Caveats: `rehost.not-publisher-supported`, `rehost.runtime-fetched`,
  `rehost.unresolved-modules`, and one new requirement sentence
  (`requirement.kernel-anti-cheat.rehost`). Prerequisites `install-rehost`
  and `allow-rehost-builds`. Host facts `rehost` and `rehost_builds`.
- Seed: 20 entries (Bruno, draw.io, balenaEtcher, Freelens, Heroic Games
  Launcher, Jitsi Meet, Joplin, Logseq, LosslessCut, MarkText, Mattermost,
  Min, MQTTX, Podman Desktop, Rocket.Chat, Standard Notes, Tabby, Trilium
  Notes, VSCodium, Zulip), each with its Linux edition (no record: nothing
  is claimed about it here), its Windows edition and its macOS edition, each
  of the latter with one record from TUR's test, evidence run and report,
  rated `starts`: a new rating, "starts and shows its first window, measured
  by TUR; nothing beyond that was tested" (owner's decision 4). `starts` is
  only TUR's own (an external scale never maps to it); it offers the
  re-host, is not adverse, but does not outweigh a known obstacle (the
  option is then doubtful) and never moves a route ahead of a Linux
  edition. The centre shows it as "Starts: TUR saw it start and show its
  first window. Nothing beyond that was tested."
- tur-apps: `plan` and `install` by the route from the person's own file
  (`--installer FILE`; without it the existing "supply the installer"
  refusal), `--allow-builds`; when the file is the package TUR measured
  (size and SHA-256 equal the record's) the record's `unresolved` modules are
  passed as accepted, otherwise the plan says plainly it was not measured;
  the exact downloads come from `tur-rehost plan --json` (not with
  `--offline`); `list`, `launch` (with `--accessibility`), `remove` (data
  kept by default, copied aside first) through tur-rehost; `update` refused
  as not built. Not done in tur-apps: `--codecs open-only` is not exposed,
  its end-to-end script (`ci/e2e.py`) does not run the real tur-rehost, and
  its P@x tool descriptions do not mention the route yet.
- The application centre: its fixtures (vocabulary, statistics, a new
  statistic `applications_tested_by_tur`, 20) and words extended so that it
  keeps building; its Home view now says "TUR has tested 20 applications
  itself. Everything else shows who tested it, or says that nobody has
  yet.", with the number from the catalogue (owner's decision 6), and the
  old sentence when the number is 0. The new rating has its own glyph,
  which has not been through the contrast check in either theme.
- The image: `tur-rehost` is a program of the image (`image/programs.tsv`,
  the Containerfile, its ignore file, the image and boot workflows'
  triggers), with its P@x tools installed (owner's decision 3). The boot
  test runs it on the installed system: `--version`, `index`, `list`, the
  archive tools present, and `detect` on a synthetic Qt program (must be
  refused as native code). The archive tools: 7-Zip is in the base image;
  `innoextract` and `msitools` are added, pinned by SHA-256 in
  `image/archive-tools/tools.lock` and installed without network (owner's
  decision 7, section 8); `bsdtar` is not in the image, so there
  `tur-rehost` opens zip with `unzip`, tar with GNU `tar`, and xar and cpio
  with 7-Zip.

## 6. Limits and what is NOT RUN

**NOT RUN, all of it**: no compiler has seen any of the Rust; no test, unit or
integration, has run; no package has been re-hosted by this program; no
window has been opened by it. In particular:

- the rust workflow (tur-rehost, tur-catalogue, tur-apps, tur-core and every
  component that links tur-core) and the desktop workflow (the centre's
  QML tests and screenshots, whose Home view changes);
- the image and boot workflows: the image build needs `tur-rehost`'s
  `Cargo.lock`, which CI writes on its first run, so the image cannot build
  before the lockfile fix is applied;
- the integration workflow `rehost` (five packages, started by hand only);
- a native build in the build environment, and whether Fedora's node-gyp
  builds all 26 pinned modules as the experiment's node-gyp 11.2.0 did;
- the application sandbox around a real Electron program: Chromium's own
  sandbox inside bubblewrap, Wayland, PipeWire, the accessibility bus;
- which archive tools the installed TUR has (the boot test records it).

Limits of what is built:

- Electron only; the other measured families are refused with a code.
- No update path: a new version is a remove and an install (exp-rehost-1,
  9.4, describes the update design).
- No desktop portal in the sandbox (needs a filtered session bus); the
  launcher entry is staged, not installed (as for Windows applications).
- Source builds: node-gyp projects only; no cache between applications.
- x86-64 only.
- The measurements behind the seed records are the experiment's: the
  first window and typed text, in a headless sway session, not Plasma.

## 7. Proposed decision-record text (not an ADR)

> **Re-hosting portable payloads (route `rehost`).** TUR runs the Windows
> or macOS edition of an application on the Linux build of the runtime it
> is built on, from the person's own package, without Wine and without a
> virtual machine, through a separate program, `tur-rehost`, that never
> runs the installer. The router offers the route only for an edition whose
> re-host TUR measured (at least starting and showing its first window,
> rated `starts`), recorded in the catalogue with its package
> digest and evidence; it is listed after any Linux edition and before
> Wine, and it is never presented as the publisher's Linux version. The
> runtime is fetched by the person's computer at the payload's exact
> version from the project's own release, only if TUR's reviewed index
> pins its SHA-256; it is never part of the image. Electron's Linux build
> is fetched unchanged, including its proprietary codecs, and the person is
> told so before the download; an open-codecs alternative is available per
> application. Native modules without a published Linux build are built on
> the person's computer, only with their permission, in a container of
> Fedora's toolchain without network, from inputs TUR pinned; TUR does not
> build or distribute such binaries. A module with no Linux build is a
> refusal unless the person accepts running without it. The application
> runs confined (its own directory read-only, a private home, the
> session's display and sound sockets, Chromium's sandbox on), with the
> publisher's Electron fuses copied, TUR's own payload hash checked at each
> start, and its own updater off. Only Electron is re-hosted until each
> other runtime is built and measured. Applications written for the Mac
> alone are never offered.

Not proposed: the "pretend" variant; a TUR build service; any claim for a
proprietary application before it is measured.

## 8. The owner's decisions (16 November 2026)

1. **Codecs**: `upstream` stays the default. The person's computer fetches
   Electron unchanged from its official release on first use, like Proton;
   TUR ships none of it; the plan says so plainly before downloading.
   `--codecs open-only` stays available. The question stays on the
   lawyer's list (question 10).
2. **Native modules are built on the person's computer**, as built here.
   Confirmed.
3. **`tur-rehost` is in the image now**, so the route needs no
   prerequisite on TUR; the boot test checks that it runs (`--version`,
   `index`, `list`, and `detect` on a synthetic fixture). Applied.
4. **Seed rating**: first-window-only results are not called `usable`. The
   catalogue had no rating for "starts and shows its first window, measured
   by TUR", so one was added, `starts`, with a plain description in the
   schema and the centre's words, and the 40 records use it. Applied.
5. **Unmeasured packages**: keep installing them with the plain "not
   measured" note. Kept.
6. **The centre's Home sentence**: "TUR has tested N applications itself.
   Everything else shows who tested it, or says that nobody has yet.", N
   computed from the catalogue (20 today). Applied.

7. **Archive tools in the image**: TUR's image carries Fedora's 7-Zip,
   innoextract and msitools, pinned, so that re-hosting can open NSIS, Inno
   Setup, MSI, 7z and disk-image packages on TUR. Applied:
   - 7-Zip (`7zip`, 26.04) turned out to be **in the base image already**
     (docs/dependencies/image-licences.csv); nothing is added for it.
   - `innoextract` and `msitools`, with what they need that the base lacks
     (eleven packages, about 1.5 MB), are pinned by the SHA-256 of the signed
     package in `image/archive-tools/tools.lock`, written by
     `image/archive-tools/lock.py`, downloaded and checked in a stage of
     their own and installed with `rpm` without network
     (`image/archive-tools/tools.sh`). Pull request 136 (Wine in the image)
     had not merged, so this builds on its pattern (`image/wine/lock.py`,
     `wine.sh`) instead of extending its lock; the two can share one script
     once it merges.
   - Licences, as each package states it and as the install step checks
     again: innoextract `Zlib`; msitools `LGPL-2.1-or-later AND
     GPL-2.0-or-later AND MS-RL`; libmsi1 `LGPL-2.1-or-later AND blessing`;
     libgcab1 `LGPL-2.1-or-later`; libgsf `LGPL-2.1-only`; six Boost
     libraries `BSL-1.0 AND MIT AND Python-2.0.1`. All are open-source
     licences Fedora ships. MS-RL (the Microsoft Reciprocal License,
     OSI-approved) covers part of msitools' own source; it is not a
     Microsoft binary, and nothing of Microsoft's is bundled. TUR's programs
     do not link any of them (they are separate programs `tur-rehost`
     starts), so the licence boundary rules are kept.
   - `bsdtar` is not in the base and not added: `tur-rehost` opens zip with
     `unzip`, tar with GNU `tar`, and xar and cpio with 7-Zip there.
   - The smoke test checks every package of the lock is installed at its
     version and that `7zz`, `innoextract` and `msiextract` run.

### Proposed decision-record text for the archive tools (not an ADR)

> **Archive tools for re-hosting in the image.** TUR's image carries
> Fedora's 7zip, innoextract and msitools, pinned, for opening application
> packages without running them. 7-Zip comes with the base image;
> innoextract and msitools, and only what they need that the base lacks,
> are added from Fedora 45's release and updates repositories, each pinned
> by the SHA-256 of the signed package in `image/archive-tools/tools.lock`,
> downloaded and checked in a separate build stage and installed with rpm
> without network, their licence tags checked against the lock. They are
> run only by `tur-rehost`, inside its sandbox without network, never
> registered to open files by themselves, and never linked into TUR's own
> programs. This is an exception to the rule that the image adds no
> package to its pinned base, made for the same reason and in the same way
> as Wine's (pull request 136); the lock is renewed when the base pin moves.

## 9. Questions for the owner's lawyer (carried from exp-rehost-1, section 11)

Unchanged, and still unanswered; numbers as in the experiment:

1. Extraction without execution: installing by extraction, on an operating
   system the licence does not name.
2. Platform clauses ("on a device running Windows", "Apple-branded
   products").
3. Adaptation for interoperability (Directive 2009/24/EC art. 5(1);
   17 U.S.C. § 117(a)(1)): replacing the runtime and native modules,
   turning off the asar integrity fuse.
4. The integrity fuse as a technological measure (17 U.S.C. § 1201;
   Directive 2001/29/EC art. 6); does TUR's own hash check change the
   answer?
5. Who performs the act: the person with TUR as a tool, or TUR inducing a
   breach?
6. Naming applications and showing their icons ("not supported by the
   publisher").
7. Online services' terms for a Linux client (the "pretend" variant is not
   built).
8. Games bought on a store, against Proton as Valve provides it.
9. Fetching the runtimes on the person's behalf, and the notices TUR must
   show.
10. **Electron's bundled codecs** (H.264, AAC): fetched unchanged by the
    person's computer at their request (this slice's default), or always
    replaced by the open-codecs build? Does the patent position change
    because TUR's program does the fetching?
11. Native modules: **changed by this slice's decision**: TUR no longer
    proposes to build and serve them; the person's computer builds them
    from source TUR pins. Is publishing the pins (URLs and digests of
    third-party open-source code) any form of distribution with notice
    duties?
12. Swapped NuGet and Maven assets: not used in this slice (Electron only).

New with this slice:

13. **Building third-party source on the person's computer at TUR's
    direction**: the module's build description runs (in a container
    without network). Any liability for TUR if a pinned module's build
    misbehaves?
14. **The catalogue's seed names 20 open-source applications** with the
    statement that their Windows and macOS packages run re-hosted
    ("tested by TUR, not supported by the publisher"): is the wording
    enough under question 6, for open-source projects?
