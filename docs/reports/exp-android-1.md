<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-android-1 — Android applications on TUR: translation first, how much needs no ARM translation, and whether the container can be tested in CI (experiment)

> **Status: first results; three of the five questions are still open.**
> One CI run has finished (43537684872, commit `4ccc96d`, 14 November
> 2026). It settles **B1** (92.8% of F-Droid's 4,987 applications need no
> ARM translation) and **D** (four of four web bundles tried show their
> first screen in a browser with no Android; such applications are about
> 2 to 3% of F-Droid). It did **not** produce a single ATL or Waydroid
> first screen: the ATL jobs and both Waydroid jobs stopped at defects of
> the test bed, each found and fixed in the commit that carries this
> version of the report (sections 3, 5, 6). **Hypotheses A, B2 and C are
> NOT RUN**; nothing in this report says ATL or Waydroid works or does not
> work on the sample. The hypotheses and kill criteria were committed
> before the first measurement (commit `2bfb491`).

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-android-1 (time-boxed research spike "android")
Subsystem:        Android runtime (addendum B, ADR-0006); no TUR service
Build ID:         not applicable: this task builds no image
Source revision:  exp/android (pull request 120); measured results are from
                  commit 4ccc96d (run 43537684872); the fixes named below
                  are in the commit that carries this report and have not
                  run

SUMMARY
- What changed: an experiment, not a feature. A workflow (exp-android) and
  harnesses under tests/experiments/android that (A) run sixty F-Droid
  applications under the Android Translation Layer in a headless Wayland
  compositor, (B) count the native ABIs of every F-Droid and IzzyOnDroid
  package and try ATL as a 64-bit ARM program under QEMU user-mode
  emulation, (C) start Waydroid on the runner's kernel and in a KVM guest
  on Fedora 45's kernel, and (D) classify what 600 random applications are
  made of and open web bundles in a browser with no Android.
- Why: ADR-0006 chose Waydroid without measurements of the alternatives;
  the owner wants Android applications as native windows on one desktop,
  fast, without VMs.

FILES
- Added: .github/workflows/exp-android.yml, tests/experiments/android/**
  (harnesses, probe application, sample lists, lock files),
  docs/reports/exp-android-1.md
- Modified: none
- Removed: none

BUILD
- Command: none on a development machine. In CI the probe application is
  built with the runner's Android SDK and NDK (probe/build.sh).
- Result: the probe built in run 43537684872 (build-tools 37.0.0, NDK
  30.0.16248370, javac 17.0.21): tur-probe.apk and tur-probe-arm64.apk
- Warnings: none

TESTS
- Unit tests: none (experiment)
- Integration tests: workflow exp-android, run 43537684872: fdroid job
  measured B1 and D; the two ATL jobs and the two Waydroid jobs were green
  but stopped before any application started (test-bed defects, sections
  3 and 6); the aarch64 job failed at "docker run" (section 5)
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (no GPU, no touchscreen, software rendering only)
- Performance tests: NOT RUN (start-up time, memory and the emulation
  benchmark need the ATL and Waydroid jobs)

DIAGNOSTICS
- Log paths: artifacts exp-android-fdroid, exp-android-atl-sample,
  exp-android-atl-random, exp-android-atl-arm64, exp-android-waydroid,
  exp-android-waydroid-f45 of run 43537684872
- Diagnostic bundle path: not applicable
- Relevant error IDs: none

RECOVERY
- Rollback procedure: close the pull request; nothing outside
  tests/experiments/android, the workflow and this report is touched
- Recovery tested: not applicable

KNOWN LIMITATIONS
- See section 11.

NEXT SAFE ACTION
- Let the run on the commit that carries this report finish; look at every
  contact sheet; settle A, B2 and C; then confirm or withdraw the
  provisional route order of section 9 and the decision text of section 14.
```

## Hypotheses and kill criteria (fixed before measuring)

| | Hypothesis | Kill criterion | Result |
|---|---|---|---|
| A | Translation first, as for Windows: on a sample of thirty real open-source applications, a useful share reaches a working first screen under the Android Translation Layer (ATL), which runs an APK as an ordinary Linux process | Fewer than 3 of the 30 hand-picked applications (10%) reach a usable first screen | **NOT RUN.** No application was started: the compositor could not be executed in the container (section 3) |
| B1 | Most open-source Android applications need no ARM translation on an x86-64 computer: at least 80% of F-Droid's applications carry x86-64 native code or no native code | Under 50%: the container route without translation would leave out most applications, and the design would have to say so | **Holds. 92.8%** (4,626 of 4,987): 42.1% no native code, 50.7% x86-64 code; 6.9% ARM only; 0.3% 32-bit x86 only. Run 43537684872 (section 2) |
| B2 | An open-source ARM-on-x86 route exists that TUR could ship legally, and it runs an application with native ARM code at a usable speed | No such route starts an application at all; or native ARM code runs more than 20 times slower than x86-64 code on the same work; or a first screen takes more than 60 seconds | **NOT RUN.** The job failed before ATL was installed (section 5). The desk finding stands: no open ARM-to-x86 native bridge exists (section 1.3) |
| C | Waydroid can be started in GitHub Actions, on the runner's kernel or in a KVM guest running Fedora 45's kernel, as far as one application's first screen in a screenshot | Android does not finish booting within ten minutes in either place; then the report says exactly what stops it | **NOT RUN to its end, not killed.** Binder, Waydroid's package, its initialisation and the container service all work in both places; the container did not start because the test session had no sound-server socket (section 6) |
| D | Some Android applications are a web view around bundled web assets, and such an application can be run as a web application with no Android at all: at least 3% of F-Droid's applications are of this kind, and one of them shows its first screen in a plain browser | None of up to four web bundles tried shows its first screen without the native bridge | **The share is under the bar; the re-hosting works.** 10 of 600 are certainly web-view applications (1.7%), 16 of 600 on inspection (2.7%, interval 1.6 to 4.3): under 3%. **4 of 4** tried show their first screen in a plain browser, 2 of them with nothing added (section 7) |

**"Usable first screen"**, decided before looking: the application's own
first screen (its main view, its introduction, or its own permission or
set-up dialog) is drawn legibly in the window, and the process is still
running when the screenshot is taken, several seconds after the picture
stopped changing. A blank window, a window with garbled or missing content,
a crash dialog, or a window whose process has died is not usable. The
verdict is made by looking at each screenshot, not by the harness.

## Method

- **Sample.** Thirty applications from F-Droid, hand-picked before
  measuring for spread across kinds (`tests/experiments/android/sample.txt`:
  notes, media, tools, games, messengers, mail, browsers, maps, and one
  each built with Flutter, React Native and a web view), and thirty drawn
  at random with a fixed seed from the whole index. Both are pinned by
  version and SHA-256 in lock files that the first run writes and that are
  then kept in `tests/experiments/android/locks/`. The hand-picked sample
  decides hypothesis A; the random one is reported beside it, because
  well-known applications are larger and more demanding than the average.
- **A.** Alpine edge packages ATL with its runtime (section 1). Each
  application is started in a headless Wayland compositor (sway, software
  rendering) in an Alpine container on GitHub's runner: time to the first
  window, time until the picture stops changing, memory (PSS of the
  process and its children), a screenshot, the accessibility tree, a
  resize, and the errors logged.
- **B1.** F-Droid's index records the native ABIs of every APK. Counted for
  the newest release of every application, taking the union over the
  per-ABI APKs of that release; by category, by year of last update and by
  year added. The same count for IzzyOnDroid, a repository in the same
  format whose APKs are the developers' own builds.
- **B2.** ATL and its whole runtime as 64-bit ARM programs under QEMU
  10.2.3's user-mode emulation on the x86-64 runner, with the window going
  to the same native compositor: the ARM builds of four applications, and a
  probe application that does the same integer and floating-point work in
  Java and in native code on both architectures.
- **C.** Waydroid with its VANILLA image (no Google software), software
  rendering, a headless compositor: on the runner's kernel (thirty
  applications), and in a KVM guest running Fedora 45 Cloud 45-1.4 with
  Fedora's own Waydroid package and SELinux enforcing (ten applications;
  from the second run on, with Fedora's current kernel, section 6).
- **D.** The ZIP directory of 600 random F-Droid APKs, read by HTTP range
  requests without downloading the APKs, says what each is built with. Up
  to four web bundles are unpacked and opened in a headless browser, with
  and without a stand-in for the framework's native bridge.
- **Integration facts.** A probe application built in CI
  (`tests/experiments/android/probe`) logs what happens to the clipboard,
  a notification, pointer and keyboard input, the back gesture, a resize,
  and requests to open, save, view and share; the harness watches the
  host's side (the Wayland clipboard, the session bus, the accessibility
  bus).
- **Test bed, stated so that it is not mistaken for a result.** No GPU:
  everything draws in software. A headless compositor has no input
  devices, so the harness holds a virtual keyboard and a virtual pointer
  on the seat for the whole session (`atl/vpointer.py`); without them an
  application never listens for input and clipboard and click checks would
  fail for a reason that has nothing to do with Android.

Nothing proprietary is fetched: no Google services, no Android image that
contains them, no ARM translation library. microG, the open-source
reimplementation of Play services, is fetched in CI for one check
(section 8) and is not shipped.

## 1. State of upstream, 14 November 2026

Read from the projects' own repositories and package indexes on
14 November 2026 (GitLab and GitHub APIs, pkgs.alpinelinux.org, Repology,
Fedora's mdapi and Bodhi, Flathub's API, ota.waydro.id).

### 1.1 Android Translation Layer

| Question | Finding |
|---|---|
| What it is | A program, `android-translation-layer`, that runs one APK as an ordinary Linux process: Android's own runtime (ART, as `art_standalone`) executes the application's bytecode against a reimplementation of the Android framework written for GTK 4; native libraries built for Android's C library are loaded through `bionic_translation`. No Android system, no container, no binder |
| Licence | `LICENSE.txt` is the GNU GPL version 3. Alpine records GPL-3.0-only, AUR and nixpkgs GPL-3.0-or-later; the files were not checked one by one. `art_standalone` is Apache-2.0 (AOSP); `bionic_translation` is recorded as Apache-2.0 by Alpine and MIT by AUR |
| Maintained? | Yes. Last commit on `master` 7 November 2026; last project activity 13 November 2026. 342 stars, 170 forks, 293 merged and 29 open merge requests, 110 issues of which 56 open. Two authors wrote most of it (812 and 344 commits of about 35 contributors) |
| Its own description | "If you are trying to launch a random app, chances are that we are missing implementations for some stuff that it needs" (README) |
| **Packaged by a distribution?** | **Yes, Alpine Linux edge (`testing`)**: `android-translation-layer` 0_git20261019-r1 with `art_standalone` 0_git20261018-r0 and `bionic_translation` 0_git20260927-r0, for x86_64, aarch64, armv7 and x86. Also AUR (`-git`) and nixpkgs. **Not in Fedora**; no COPR was found. So this experiment runs ATL in an Alpine container and builds nothing |
| Shipped to users anywhere? | **Yes: Flathub carries `net.newpipe.NewPipe`, "NewPipe (unofficial port)", packaged by the ATL project**: the Android application in a Flatpak with ATL. 76,528 installs in total, 2,021 in the last month (Flathub statistics, 14 November 2026) |
| How that port is built | Its Flathub manifest (`flathub/net.newpipe.NewPipe`) takes the upstream APK by SHA-256, sits on a Flatpak base application, **`io.gitlab.android_translation_layer.BaseApp`** (GNOME Platform 51), compiles the application's bytecode ahead of time with `dex2oat` at build time, and strips the native libraries of other architectures. Its permissions are the Flatpak's: network, Wayland, GPU, audio |
| Building it for Fedora | `doc/Build.md` lists Fedora build dependencies (GTK 4, libportal, WebKitGTK 6.0, OpenXR, libsecret, OpenJDK 17) and three things Fedora does not package: `art_standalone`, `bionic_translation`, and wolfSSL built with JNI |
| Web views | Implemented with WebKitGTK 6.0, off unless `ATL_UGLY_ENABLE_WEBVIEW` is set |
| Desktop integration it documents | Notifications and launcher entries through the desktop portal (libportal); Android intents between applications over D-Bus (`org.freedesktop.Application.ActivateAction`); application data under `~/.local/share/android_translation_layer/` |
| Sandboxing | None of its own. The README lists "explore using bubblewrap" on its roadmap; location and microphone are gated by environment variables named `ATL_UGLY_…` |
| Google services | None. A sister project, `gcm_service`, implements the push-message protocol (FCM) natively for ATL applications; it talks to Google's servers and contains no Google code |
| ARM applications on x86-64 | Not supported: ATL loads native libraries of the machine's own architecture. Issue 88, "libndk_translation/libhoudini integration", is open (May 2026) |

### 1.2 Waydroid

| Question | Finding |
|---|---|
| Licence, release | GPL-3.0. Release 1.6.3 of 28 May 2026; last commit 16 October 2026; 987 open issues and pull requests |
| Images | System `lineage-20.0-20260927-VANILLA-waydroid_x86_64` (Android 13; 847 MB archive) and vendor `lineage-20.0-20261022-MAINLINE-waydroid_x86_64` (190 MB), pinned here by the SHA-256 that ota.waydro.id publishes |
| Fedora 45 | `waydroid-1.6.3-2.fc45`. It requires `lxc`, `dnsmasq`, `nftables`, `gtk3`, `python3-gobject`, `dbus-python`, `gbinder-python` and, with SELinux, `waydroid-selinux` |
| Clipboard | Waydroid shares the clipboard only if the Python module `pyclip` can be imported (`tools/services/clipboard_manager.py`). **Fedora's package neither requires nor recommends `python3-pyclip`** (which Fedora 45 has, 0.7.0). Measured in section 4 |
| Upstream's package for Ubuntu | `repo.waydro.id` has a `noble` suite; used for the job on the runner's kernel |

### 1.3 ARM on x86-64 without a proprietary library

| Candidate | Finding |
|---|---|
| Berberis (AOSP `frameworks/libs/binary_translation`) | Its README: "Dynamic binary translator to run Android apps with riscv64 native code on x86_64 devices". No ARM guest. Last commit on the public `main` 27 March 2025 |
| `goffioul/ax86-nb-qemu` | An Android native bridge built on QEMU for Android-x86. Last push 16 May 2020; no licence file; written for Android 7 to 9. Dead |
| `SGNight/Arm-NativeBridge` and similar | Repackaged copies of Intel's `libhoudini`. Not open source, no licence: not usable (AGENTS.md) |
| QEMU user-mode emulation (`qemu-aarch64`, GPL-2.0) | Runs a whole 64-bit ARM Linux process on an x86-64 kernel. It is not an Android native bridge (which loads ARM libraries into an x86-64 process), and it cannot run a whole Android container, because it does not pass binder's ioctls through. **But ATL makes an Android application one ordinary Linux process**, so the whole of ATL, ARM build, can run under it and still draw to the native compositor. That is the route tried here (section 5). Nobody was found to have published this combination |

### 1.4 Google's services

See section 8.

## 2. Hypothesis B1: how much of F-Droid needs no ARM translation

**Measured**, run 43537684872, job `fdroid` (artifact `exp-android-fdroid`:
`abi.md`, `abi.json`, `abi-apps.json` with one row per application).
Index `https://f-droid.org/repo/index-v2.json`, SHA-256 `a504cce4…0b72`,
dated 11 November 2026: 4,987 applications. The newest non-beta release of
each is counted, with the union of the ABIs of all its APKs (F-Droid
publishes one APK per ABI for 545 applications).

| | Apps | No native code | x86-64 native code | **Needs no translation** | 32-bit x86 only | ARM only | Of those with native code: has x86-64 |
|---|---|---|---|---|---|---|---|
| **F-Droid, all** | 4,987 | 42.1% (2,098) | 50.7% (2,528) | **92.8% (4,626)** | 0.3% (16) | 6.9% (343) | 87.5% |
| F-Droid, updated in the last two years | 3,717 | 31.4% | 60.1% | 91.5% | 0.2% | 8.3% | 87.6% |
| IzzyOnDroid, all (see the caution below) | 1,406 | 41.2% | 39.6% | 80.8% | 0.2% | 19.0% | 67.4% |
| IzzyOnDroid, updated in the last two years | 1,019 | 31.0% | 46.9% | 77.9% | 0.0% | 22.1% | 68.0% |

The hypothesis (at least 80%) holds for F-Droid; the kill criterion (under
50%) is far away. Of the 343 ARM-only applications, 330 have 64-bit ARM
code and 13 only 32-bit. Counting every version still in the index instead
of the newest release moves the result by 0.3 points (ARM only 6.6%).

**What the number hides.**

- **F-Droid builds from source and usually builds every ABI; a developer's
  own build often does not.** IzzyOnDroid carries the developers' own
  APKs: there, of the applications with native code, 67.4% have x86-64
  code, against 87.5% in F-Droid. **Caution:** IzzyOnDroid's index holds
  exactly one APK per release for all 1,406 applications (measured), so
  where a developer publishes one APK per ABI the index shows only one of
  them. Its 19.0% "ARM only" is therefore an upper bound, and 80.8% a
  lower bound, for developer-built open-source applications.
- **It is getting worse, not better.** By year the application was added
  to F-Droid, ARM-only is between 0 and 5% for every year from 2013 to
  2022, then 6.6% for 2023, 5.4% for 2024, 10.7% for 2025 and 11.2% for
  2026. Newer
  applications have native code far more often (22% without native code
  among those added in 2026, 82% among those added in 2016), and a growing
  part of it is built for ARM alone.
- **Games are worse than the average.** Over F-Droid's game categories
  (463 application-category pairs, Game Helper left out) 86.4% need no
  translation; Platformer 36% (4 of 11), Sport 64%, Shooter 71%, Role-Playing
  81%, Action 83%, Puzzle 90.5%, Casual 94%.

By category (the twelve largest, then the lowest with at least 30
applications; all 115 categories are in the artifact):

| Category | Apps | Needs no translation | ARM only |
|---|---|---|---|
| System | 687 | 96.9% | 3.1% |
| Internet | 650 | 93.2% | 5.8% |
| Multimedia | 505 | 92.9% | 6.9% |
| Science & Education | 384 | 95.3% | 4.7% |
| Connectivity | 315 | 96.2% | 3.8% |
| Security | 285 | 95.1% | 4.2% |
| Sports & Health | 259 | 93.4% | 6.6% |
| Reading | 227 | 94.3% | 4.4% |
| Writing | 212 | 94.8% | 5.2% |
| Navigation | 191 | 94.8% | 4.7% |
| Keyboard & IME | 146 | 94.5% | 5.5% |
| Note | 143 | 88.8% | 11.2% |
| *Lowest:* Wallet | 42 | 76.2% | 23.8% |
| AI Chat | 36 | 77.8% | 22.2% |
| Voice & Video Chat | 32 | 78.1% | 21.9% |
| Ebook Reader | 47 | 80.9% | 12.8% |
| Social Network | 50 | 84.0% | 16.0% |
| Messaging | 84 | 84.5% | 15.5% |

By year of last update (F-Droid): 2026: 3,080 applications, 90.7%; 2025:
605, 95.7%; 2024: 302, 95.4%; 2023: 207, 97.6%; 2022 and before: 793,
between 90% and 100%.

**The Android version is not the obstacle.** 0.7% of F-Droid's
applications (33) require an Android newer than Waydroid's Android 13
image; 63.2% already *target* API level 35 or newer and still install on
13. The same 0.7% holds in IzzyOnDroid.

**The two samples, by ABI alone.** Of the thirty hand-picked applications
28 need no translation, one is ARM only (OpenSudoku 4.11.3) and one has
32-bit x86 code but no x86-64 (MuPDF viewer 1.28.5a). Of the thirty random
ones 29 need no translation and one is ARM only.

For applications outside F-Droid: section 9.3.

## 3. Hypothesis A: applications under ATL

**NOT RUN.** In run 43537684872 both ATL jobs installed ATL from Alpine
(`android-translation-layer-0_git20261019-r1`, `art_standalone-0_git20261018-r0`,
`bionic_translation-0_git20260927-r0`, GTK 4.24.1, WebKitGTK 2.54.2,
sway 1.12, Mesa 26.2.6; the full package list is in each artifact), then
stopped: `sway: Operation not permitted`. Alpine gives `/usr/bin/sway` the
file capability `cap_sys_nice`, and a container whose bounding set lacks
that capability refuses to execute such a file. No application was
started; the jobs were green because the harness exits cleanly, which is
the convention of experiment jobs here and is exactly why a green job
proves nothing.

Fixed in the commit that carries this report (a plain copy of the binary,
which has no file capability, and `--cap-add SYS_NICE`); not yet run. The
harness now also starts the probe application first and looks at whether
its window has a picture at all; if it has none with GTK's default
renderer it switches to GTK's software renderer and says so, so that a
blank window caused by the GPU-less runner is not counted against ATL.

What is known about ATL without the measurement is in section 1.1: it is
maintained, Alpine packages it, and one real application (NewPipe) is
distributed with it on Flathub and has 76,528 installs.

## 4. Integration facts (probe application)

**NOT RUN** for ATL and for Waydroid (no application started in either).
Two things about the test bed are measured (run 43537684872, both Waydroid
jobs, `sway-seats.json`, `logs/vpointer.log`):

- The probe application builds without Gradle from the runner's SDK and
  NDK, in two variants (x86-64 with ARM64 native code, and ARM64 only).
- The harness's own virtual pointer (`atl/vpointer.py`, the Wayland wire
  protocol written out by hand) and a sleeping `wtype` give the headless
  seat both capabilities: sway 1.9 (Ubuntu) and sway 1.12 (Fedora 45)
  report `capabilities: 3` with a `wlr_virtual_pointer_v1` and a
  `wlr_virtual_keyboard_v1` device.

One desk fact bears on the design already: Waydroid shares the clipboard
only when the Python module `pyclip` is importable, and Fedora 45's
package neither requires nor recommends `python3-pyclip` (section 1.2).
The harness measures the clipboard as packaged and again with the module.

## 5. Hypothesis B2: ARM applications through QEMU user-mode emulation

**NOT RUN.** Measured so far (run 43537684872, job `atl-arm64`):

- QEMU 10.2.3's `qemu-aarch64` registers with `binfmt_misc` on the runner
  (flags `POCF`).
- The reference point for the emulator's speed, native x86-64 on the
  runner (`openssl speed`, Alpine edge): SHA-256 at 2,177 MB/s on 16 kB
  blocks, RSA-2048 at 5,111 signatures per second.

The ARM half failed: `docker: cannot overwrite digest sha256:020dfcba…`.
The job pinned Alpine by the digest of its multi-architecture index and
then asked docker for two platforms of it; docker cannot hold one index
digest for two platforms. The job failed at its next step for the same
reason. Fixed in the commit that carries this report (each architecture is
pinned by the digest of its own manifest); not yet run.

The idea, for the record, because nobody was found to have published it:
ATL turns an Android application into one ordinary Linux process, so the
whole of ATL, as an ARM64 program, can run under user-mode emulation on an
x86-64 kernel and still be a native window. No proprietary library, no
native bridge, nothing in the kernel. Whether it starts, and how slow it
is, is what the job measures.

## 6. Hypothesis C: Waydroid in GitHub Actions

**NOT RUN to its end; not killed.** Run 43537684872 got as far as the
container's start in both places and stopped there for one and the same
reason, which belongs to the test bed. Measured:

| Step | Runner's own kernel (job `waydroid`) | KVM guest, Fedora 45 (job `waydroid-f45`) |
|---|---|---|
| Kernel | `7.0.0-1017-azure` (Ubuntu 24.04.5) | `7.2.11-300.fc45` (Cloud Base 45-1.4 as released) |
| Binder | `CONFIG_ANDROID_BINDER_IPC=m`, `CONFIG_ANDROID_BINDERFS=m`: the C driver as a module, **already in the runner's image** (`binder_linux.ko.zst`; no extra package). `modprobe` succeeds | **`CONFIG_ANDROID_BINDER_IPC=y`, `CONFIG_ANDROID_BINDERFS=y`: the C driver, built in.** No `CONFIG_ANDROID_BINDER_IPC_RUST` line. See the note below |
| Binder devices | Waydroid makes `anbox-binder`, `anbox-hwbinder`, `anbox-vndbinder` in binderfs through `binder-control` | Fedora's mount unit provides `binder`, `hwbinder`, `vndbinder` in `/dev/binderfs` |
| KVM guest | — | `/dev/kvm` usable on the runner; the guest (4 CPUs, 10.5 GB) answers on SSH **20 seconds** after QEMU starts |
| Waydroid | 1.6.3 from upstream's `noble` repository, LXC 5.0.3 | `waydroid-1.6.3-2.fc45` with `waydroid-selinux`, LXC 7.0.0; installed with sway and its dependencies in 30 seconds |
| `waydroid init` with the pinned VANILLA images (no network) | Status 0, 1 second | Status 0, 1 second |
| Container service | Active | Active |
| SELinux | not present (AppArmor on) | **Enforcing; 0 denials** up to this point |
| Headless compositor and input devices | sway 1.9, `wayland-1`, seat with pointer and keyboard | sway 1.12, the same |
| `waydroid session start` | **Fails: "container failed to start"** | **Fails: the same** |
| Cause | `lxc-start`: `Failed to mount "/run/user/1001/pulse/native"` | `Failed to mount "/run/user/1000/pulse/native"` |
| Job duration | 2.5 minutes | 8.8 minutes (6 of them downloading the Fedora image, the Android images and the applications; cached afterwards) |

Waydroid binds the session's PulseAudio socket into the container and the
mount is not optional: with no sound server in the session the container
does not start. A desktop session always has one; a CI session started by
hand has not. Fixed in the commit that carries this report (a sound server
with no sound card: PulseAudio on the runner, PipeWire in the guest, and
an empty file as the last resort); not yet run. So far nothing has been
found that prevents Waydroid from being tested in CI: binder is there in
both places, KVM works, the downloads cache, and the jobs are short.

**A correction to android-runtime.md, section 3.3.** That section measures
TUR's image (kernel `7.3.3-300.fc45`): C driver off, Rust driver built in;
and says that Fedora 43, 44 and 45 carry the same configuration, citing a
Fedora 44 kernel `7.2.9`. Measured here: Fedora 45's kernel **as released**
(`7.2.11-300.fc45`, `/lib/modules/…/config`) has the **C** driver built in
and no Rust binder option. The switch is between 7.2 and 7.3, not between
Fedora releases. Two consequences: the report cited there of Waydroid
working on `7.2.9` is evidence for the C driver, not the Rust one; and a
test of "Fedora 45's kernel" tests the Rust driver only if the guest is
updated first. The guest job now updates the kernel and restarts before
the test and records both configurations (`kernel-as-released.txt`,
`kernel-updated.txt`); not yet run.

## 7. Hypothesis D: what applications are made of, and re-hosting a web bundle

**Measured**, run 43537684872, job `fdroid` (artifact
`exp-android-fdroid`: `payload.md`, `payload-apps.json`, `rehost/`).

### 7.1 What 600 random F-Droid applications are built with

600 applications drawn with a fixed seed from the index of section 2. The
ZIP directory of each newest APK was read with HTTP range requests: 62 MB
fetched in all, where the APKs themselves are several gigabytes; none
failed. Shares with their 95% Wilson interval.

| Built with (first match) | Apps | Share (95% interval) | Can it run without Android? |
|---|---|---|---|
| Android's own toolkit, views | 304 | 50.7% (46.7–54.7) | No |
| Android's own toolkit, Jetpack Compose | 184 | 30.7% (27.1–34.5) | No |
| Flutter | 72 | 12.0% (9.6–14.8) | Not from the APK: the Dart code in it is compiled for Android. From source, yes: Flutter builds Linux desktop applications |
| React Native | 7 | 1.2% (0.6–2.4) | Not from the APK |
| A web view around bundled web assets (Cordova 5, Capacitor 5, other 10 by the automatic rule) | 20 | 3.3% (2.2–5.1) | **Yes** (7.2) |
| Python (Kivy and similar) | 6 | 1.0% (0.5–2.2) | From source |
| libGDX | 5 | 0.8% (0.4–1.9) | From source: libGDX has a desktop backend |
| Qt, SDL | 2 | 0.3% | From source |
| Unity, Godot | 0 | 0% (0–0.6) | — |

Kotlin is in 79.8% of the applications. Portable frameworks of every kind
are 18.7% (15.8–22.0), but only the web bundles are portable *as shipped*.

**The web-view share, read by hand.** The automatic rule (Cordova,
Capacitor, or an `index.html` with at least 20 kB of script anywhere in
the assets) found 20. Four of them are not web-view applications: a
browser's add-on page (Ceno), AnkiDroid's statistics pages, Kodi's and
BiglyBT's remote-control pages. That leaves **16 of 600, 2.7% (1.6–4.3)**;
the ten built with Cordova or Capacitor, **1.7% (0.9–3.0)**, are certain.
The hypothesis said at least 3%: **not met** on the point estimate, and
the interval reaches it only at its upper end. On F-Droid's 4,987
applications that is 80 to 215 applications, most likely about 130.

### 7.2 Four web bundles in a browser, with no Android

The four candidates with the largest share of web assets (two Cordova,
two Capacitor) were downloaded, verified by SHA-256, their web directory
unpacked and served on 127.0.0.1, and opened in headless Chrome 157 at
412 × 915. Each was tried as it is and with a 15-line stand-in for
`cordova.js` that announces "device ready" and fails plugin calls
(`payload.py`, `SHIM`). The screenshots were looked at.

| Application (version) | Framework | As it is | With the stand-in |
|---|---|---|---|
| Subz 1.3 (`com.flasskamp.subz`) | Capacitor | **First screen**: overview with help cards and working tab bar | The same |
| NicotinD 0.8.126 (`ar.kevinroberts.nicotind.tv`) | Capacitor | **First screen**: its sign-in page. It then asks its own server (`/api/…`, 8 requests answered 404 here): a client for a self-hosted service | The same |
| Deine Rechte 0.2.3 (`ch.gassenarbeit.bern.your.rights`) | Cordova | **Nothing**: no screenshot within 90 s, empty page | **First screen**: the brochure's first chapter, menu and search |
| TotaLog 0.1.2 (`de.dm1ri.totalog`) | Cordova | **Nothing**, as above | **First screen**: the log form with its tab bar |

**4 of 4 reach their first screen; 2 of 4 with nothing added.** The kill
criterion (none without the native bridge) is not met. Cordova's own
`cordova.js` for Android waits for the native side and the page never
starts; replacing that one file is enough for a first screen. What lies
behind the first screen was not tested: an application that uses a native
plug-in (files, camera, notifications) needs that plug-in answered, by a
web API or by TUR.

So re-hosting is real but small: about one open-source application in
forty, the simple ones. It costs no Android at all, starts like a web
page, and is sandboxed like one.

## 8. Google's services: facts and options

**Facts.**

- Google Mobile Services (Play services, Play Store) are proprietary and
  licensed by Google to device makers. TUR may not ship them or an image
  that contains them (AGENTS.md). Waydroid publishes a `GAPPS` image
  variant; this experiment never fetches it.
- F-Droid's inclusion policy excludes applications that depend on
  proprietary libraries, so the sample cannot show what a dependency on
  Play services costs: by construction none of the sixty applications
  needs them. The index's anti-feature counts (section 2) show how many
  F-Droid applications lean on a non-free network service or dependency.
- What applications outside F-Droid use Play services for: push messages
  (FCM), sign-in with a Google account, maps, location fusion, in-app
  purchases and licence checks, and Play Integrity attestation. The last
  cannot be satisfied by anything outside a certified device
  (android-runtime.md, section 3.4).

**Options.**

| Option | Licence | Under Waydroid | Under ATL | What it gives |
|---|---|---|---|---|
| **Run without** | — | The default (VANILLA image) | The only state today | Everything in F-Droid. Applications from elsewhere start if they treat Play services as optional; push, Google sign-in, Google maps and purchases are missing; some refuse to start |
| **microG** | Apache-2.0; release v0.3.18.252432 of 12 November 2026; shippable in law | Installable if the image lets microG present Google's signature. LineageOS allows this for microG alone (commits of September and November 2026 in its `frameworks_base`). **Whether Waydroid's image does is measured in section 6** (NOT RUN) | Not applicable: there is no Android system to install it into. ATL's own answer is piecemeal: `gcm_service` for push messages | Push messages, a maps API on open map data, network location, sign-in for many applications. By its users' reports it does not reach the Play Integrity device verdict (not verified) |
| **The person's own opt-in to Google's** | Proprietary; not shipped | The person chooses upstream's `GAPPS` image and registers the device with Google by hand, as android-runtime.md section 12.2 describes | Not possible | Play Store and Play services, without device certification: no Play Integrity device verdict, so banking and similar applications still fail |

## 9. Recommended route order for TUR's router

**Provisional.** Rungs 2 and 4 rest on hypotheses that are NOT RUN; they
are written as conditions, not as findings. Rungs 1, 3 and 5 rest on
measurements or on facts already in android-runtime.md.

| Rung | Route | For which applications | Share of F-Droid (measured) | Rests on |
|---|---|---|---|---|
| 1 | **Re-host as a web application**, no Android | Catalogue entries marked as a web bundle and checked once (Cordova, Capacitor, plain web view) | 1.7% certain, 2.7% likely | Section 7: 4 of 4 first screens. Catalogue only: an unknown APK is never re-hosted by guesswork |
| 2 | **ATL**, the application as an ordinary process with a native window | Catalogue entries that were measured to work under ATL, delivered as a Flatpak on ATL's base application; not arbitrary APKs | NOT RUN (hypothesis A decides whether this rung exists at all, and how wide it is) | Section 1.1: it ships NewPipe this way today |
| 3 | **Waydroid container** (ADR-0006), one window per Android task | Every other APK with x86-64 code or no native code | Up to 92.8%; 0.7% need a newer Android than the image | Section 2; section 6 for whether TUR can test it continuously (NOT RUN to its end) |
| 4 | **ATL as an ARM64 program under QEMU user-mode emulation** | ARM-only applications in the catalogue | 6.9% of F-Droid; at most 19% of developer-built open-source applications; more among games | NOT RUN (hypothesis B2). If it is killed, this rung is "not supported unless the person supplies a translation layer", as android-runtime.md says now |
| 5 | **Not supported, with the reason** | Applications that need a certified device (banking, DRM video, anti-cheat), phone hardware, or Google's services where neither microG nor the person's opt-in answers | Not in F-Droid by its policy | android-runtime.md, section 11 |

**How the routes fit the direction the owner has set** (TUR beside the
person's Windows, their Windows inside TUR, everything on one desktop):

- **Android cannot be delegated to the Windows side.** Microsoft ended
  the Windows Subsystem for Android in March 2025 (from Microsoft's
  announcement of 2024; not re-read for this report), so the person's
  Windows offers no Android route of its own. If Android applications are
  to be on the one desktop, TUR runs them.
- **Rungs 1, 2 and 4 are native windows in the plain sense**: one process
  per application, one Wayland window, its own identity in the task
  switcher, its own sandbox, nothing running when the application is
  closed, no root daemon. That is the shape the owner asks for, and it is
  the shape of TUR's Windows route too (Wine before the guest).
- **Rung 3 is windows from one shared Android system.** Each Android task
  is a window on the desktop (to be confirmed by the multi-window check of
  section 6), but behind them one container boots, holds memory while any
  Android application is open, and runs a root daemon (ADR-0006's
  recorded deviation). It is the rung that runs the most applications and
  the one that is furthest from "native".
- **Memory is shared with the Windows guest.** ADR-0007 indicates 16 GB
  with the guest, to be measured. A whole Android system beside a whole Windows is the
  expensive combination; an ATL process or a web application beside
  Windows is not. The numbers (memory of an ATL application, of Waydroid
  idle and per application) are NOT RUN; they decide how hard the router
  should prefer rungs 1 and 2 when the Windows guest is running.
- **Nothing here needs a VM.** Every rung runs on the host kernel.

### 9.3 Applications outside F-Droid: what can be known without downloading them

- **Not knowable from Google Play.** The store's listing does not state
  native ABIs, and Play delivers device-specific splits of an App Bundle:
  what an x86-64 device would receive cannot be seen without asking as
  that device.
- **Knowable from a package in hand.** The ZIP directory of an APK lists
  `lib/<abi>/`; TUR's inspector reads it before installing
  (android-runtime.md, section 6.2). For a remote APK two HTTP range
  requests are enough (section 7 did this for 600 packages, about
  106 kB each on average).
- **Knowable from other open indexes.** IzzyOnDroid publishes an index in
  F-Droid's format for developer-built APKs (section 2): at least 80.8%
  need no translation.
- **Inferable from the framework.** Flutter and React Native build
  x86-64 libraries by default; a Unity game is ARM-only unless its
  developer ticks x86-64. For the Play Store this is an inference, not a
  measurement, and the trend of section 2 (more ARM-only every year)
  argues against optimism.

## 10. What productising would take

The measured part is NOT RUN. From the upstream facts of section 1 alone,
ATL could reach a TUR computer in three ways, and the choice does not
depend on the measurements:

| Way | What TUR builds | Sandbox | Cost |
|---|---|---|---|
| **A Flatpak per application, on ATL's base application from Flathub** | Nothing of ATL. TUR's application centre already installs Flatpaks; a catalogue entry names the APK (by hash) and the base application | Flatpak's, per application: the gap ATL itself has | Each application is packaged once (by TUR's catalogue, by the ATL project, or by its author); an arbitrary APK the person brings is not covered unless TUR builds the Flatpak on the computer |
| ATL as packages in TUR's image | ATL, `art_standalone`, `bionic_translation`, wolfSSL with JNI: four packages Fedora does not have, tracked against an upstream without releases (snapshots only) | TUR's own (`tur-runtime`'s bubblewrap profiles, as for Wine) | A build and update pipeline for a second Android runtime; any APK can be tried |
| ATL in a container image fetched on demand (Alpine, as this experiment runs it) | A pinned container image | The container's | Simple to pin; a second C library and GTK stack on disk; desktop integration (portals, accessibility, theme) crosses a container boundary |

## 11. Limits

- **A, B2 and C are not measured.** No ATL window and no Android boot has
  been seen in this experiment yet. The route order is provisional.
- No GPU and no touchscreen in CI: software rendering only. Games and
  anything that needs hardware acceleration are not judged fairly by this
  test bed, in either route.
- First screens only. An application that draws its first screen may fail
  at the second; the re-hosted web bundles were not operated.
- ATL is measured as Alpine packages it (musl C library, snapshot of
  19 October 2026), not as a Fedora build, which does not exist.
- F-Droid is not the Play Store. Everything here is about open-source
  applications; sections 2 and 9.3 say what follows for the rest.
- IzzyOnDroid's index holds one APK per release, so its ARM-only share is
  an upper bound (section 2).
- The web-view share rests on file names in the APK and a reading of
  twenty entries by hand, not on running the applications.
- The fixes in the commit that carries this report have not run.

## 12. Licence and legal questions for the owner

1. **ATL is GPL-3.0.** TUR could ship it as a separate program, as it
   ships Waydroid (GPL-3.0); TUR's own code would drive it over its
   command line and D-Bus and never link to it. Fixes would go upstream
   under GPL-3.0. Is that acceptable for a second Android route?
2. **ATL is not in Fedora.** Shipping it means TUR builds and maintains
   three packages (ATL, `art_standalone`, `bionic_translation`), or ships
   it as a Flatpak runtime the way the ATL project does on Flathub. Which?
3. **QEMU user-mode emulation is GPL-2.0 and is in Fedora**
   (`qemu-user`). If section 5 shows the ARM route to be usable, shipping
   it raises no licence question that Fedora has not already answered; an
   ARM build of ATL and its runtime would have to be shipped beside the
   x86-64 one.
4. **microG** is Apache-2.0 and answers under Google's package name with
   Google's signature presented by the system. android-runtime.md already
   proposes it as an opt-in download. The legal review should say whether
   offering it is acceptable in the owner's jurisdiction.
5. **Re-hosting** (section 7) runs an application's own web assets
   outside the package its author shipped. For open-source applications
   the licence allows it; for others it is a question for the legal
   review.

## 13. Follow-ups

1. **Finish this experiment**: one more run settles A, B2 and C (the
   fixes are in place). Then confirm or withdraw rungs 2 and 4.
2. **Correct android-runtime.md, section 3.3** (binder on Fedora 45's
   released kernel is the C driver; section 6 here).
3. **`python3-pyclip` in TUR's image** if Waydroid is shipped: without it
   there is no shared clipboard (to be confirmed by the measurement).
4. **Catalogue fields**: `payload: web-bundle | flutter | …` and
   `native_abis`, both readable from an APK's ZIP directory without
   downloading it (`fdroid/payload.py` is the reference).
5. **A re-hosting runner** for rung 1: a sandboxed web view with a
   stand-in for `cordova.js` and Capacitor's bridge, answering the common
   plug-ins (storage, files through the portal, notifications).
6. **If A holds**: ATL in a Fedora 45 container or as TUR-built Flatpaks,
   tested on the catalogue's Android entries at every base update, the way
   Wine applications are.
7. **If B2 holds**: the compositor native and only ATL emulated (this
   experiment emulates the compositor too, which overstates the cost).
8. **Waydroid in CI as a permanent job** once C holds: the guest job costs
   about nine minutes with caches cold.

## 14. Proposed decision text (provisional; the lead assigns the number)

To be adopted only as far as the open hypotheses allow: paragraph 2 only
if hypothesis A survives, paragraph 4 only if B2 survives. As written it
amends ADR-0006 and ADR-0007's routing table; it does not replace
Waydroid.

> **Android applications: the lightest route that is known to work.**
>
> 1. The router treats "Android application" as a ladder, as it does
>    Windows applications. The first rung that the catalogue records as
>    working is used; the person can override it.
> 2. **Translation before the container.** Where the catalogue records
>    that an application runs under the Android Translation Layer, TUR
>    runs it that way: an ordinary process with a native window and a
>    per-application sandbox, delivered as a Flatpak on ATL's base
>    application. ATL is GPL-3.0 and stays a separate program. An
>    application not recorded as working under ATL is never sent there by
>    guesswork.
> 3. **The container is the general route.** Any other package with
>    x86-64 code or no native code goes to Waydroid (ADR-0006), unchanged.
> 4. **ARM-only applications** recorded as working go to ATL built for
>    ARM64 under QEMU's user-mode emulation. TUR ships no proprietary
>    translation library; otherwise the verdict of android-runtime.md
>    section 6.2 stands.
> 5. **An application that is a web view around its own bundled web
>    assets**, and is recorded as such, is run as a sandboxed web
>    application with no Android runtime.
> 6. The catalogue records, per Android application and version: native
>    ABIs, what it is built with, and the rungs measured to work. All
>    three are measured in CI, never claimed.
> 7. TUR does not rely on the person's Windows for Android applications.
