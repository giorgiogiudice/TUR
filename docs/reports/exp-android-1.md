<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-android-1 — Android applications on TUR: translation first, how much needs no ARM translation, and whether the container can be tested in CI (experiment)

> **Status: all five questions settled.** Measured in runs 43537684872,
> 43553105465 (14 November 2026) and 43747969168 (15 November 2026):
> **B1** holds (92.8% of F-Droid needs no ARM translation, twice, on two
> index dates); **D**: web bundles are 2 to 3% of F-Droid and 4 of 4 tried
> run with no Android; **A survives**: 12 of 60 applications reach a usable
> first screen under ATL (7 of the 30 hand-picked; 13 of 60 in the third
> run, once a font was added to the test container). **C is met**: on
> TUR's kernel with SELinux enforcing, Waydroid puts Android's applications
> on the desktop, **29 of 29 installable applications with a usable first
> screen** in Waydroid's full-screen window, and **one desktop window per
> application** in its multi-window mode when launched with `waydroid app
> launch`; pointer, clipboard both ways and notifications reach the host;
> the desktop cannot resize an application's window. **B2 is killed**: with
> ART's just-in-time compiler off, the emulated ARM route again drew one
> window (the first launch) and every later launch died. The hypotheses
> and kill criteria were committed before the first measurement (commit
> `2bfb491`). Section 6.4 is the summary for TUR's Android runtime
> (pull request 137).

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-android-1 (time-boxed research spike "android")
Subsystem:        Android runtime (addendum B, ADR-0006); no TUR service
Build ID:         not applicable: this task builds no image
Source revision:  exp/android (pull request 120); measured results are from
                  commits 4ccc96d (run 43537684872), 0f56629 (run
                  43553105465) and f7fc92d (run 43747969168)

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
- Integration tests: workflow exp-android. Run 43537684872: fdroid job
  measured B1 and D; the other jobs stopped at test-bed defects. Run
  43553105465: 60 applications under ATL (12 usable first screens), the
  probe's integration checks, Waydroid booted on the runner's kernel and
  on Fedora 45's 7.3.3 kernel (29 of 30 and 10 of 10 applications
  started, none visible on the desktop), ATL as ARM64 under QEMU (1 of 7
  launches). Run 43747969168: Waydroid's windows on the desktop (29 of 29
  usable first screens on the runner, 10 of 10 on Fedora 45's 7.3.3
  kernel; one window per application in multi-window mode), pointer,
  keys, clipboard, notifications and resize from the host, microG's
  signature; ATL's clipboard after a click; ATL as ARM64 with the
  just-in-time compiler off (1 of 7 launches again); A and B1 repeated
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (no GPU, no touchscreen, software rendering only)
- Performance tests: start-up time and memory of both routes and the
  probe's benchmark (sections 3 and 4); under emulation NOT RUN

DIAGNOSTICS
- Log paths: artifacts exp-android-fdroid, exp-android-atl-sample,
  exp-android-atl-random, exp-android-atl-arm64, exp-android-waydroid,
  exp-android-waydroid-f45 of runs 43537684872, 43553105465 and
  43747969168
- Diagnostic bundle path: not applicable
- Relevant error IDs: none

RECOVERY
- Rollback procedure: close the pull request; nothing outside
  tests/experiments/android, the workflow and this report is touched
- Recovery tested: not applicable

KNOWN LIMITATIONS
- See section 11.

NEXT SAFE ACTION
- The owner's decisions of section 12 and the decision text of section
  14. For the Android runtime (pull request 137): section 6.4, above all
  that the desktop cannot size Waydroid's per-application windows and
  that typed text from a real keyboard is still unmeasured.
```

## Hypotheses and kill criteria (fixed before measuring)

| | Hypothesis | Kill criterion | Result |
|---|---|---|---|
| A | Translation first, as for Windows: on a sample of thirty real open-source applications, a useful share reaches a working first screen under the Android Translation Layer (ATL), which runs an APK as an ordinary Linux process | Fewer than 3 of the 30 hand-picked applications (10%) reach a usable first screen | **Survives: 7 of 30** hand-picked applications (23%) reach a usable first screen under ATL, and 5 of 30 random ones (17%); 12 of 60 together. Run 43553105465 (section 3). Repeated in run 43747969168: the same 23 windows; 7 and 6, because one random application's Chinese text became legible once the test container had a CJK font |
| B1 | Most open-source Android applications need no ARM translation on an x86-64 computer: at least 80% of F-Droid's applications carry x86-64 native code or no native code | Under 50%: the container route without translation would leave out most applications, and the design would have to say so | **Holds. 92.8%** (4,626 of 4,987): 42.1% no native code, 50.7% x86-64 code; 6.9% ARM only; 0.3% 32-bit x86 only. Run 43537684872 (section 2); the index of 13 November gives the same 92.8% of 4,996 (run 43747969168) |
| B2 | An open-source ARM-on-x86 route exists that TUR could ship legally, and it runs an application with native ARM code at a usable speed | No such route starts an application at all; or native ARM code runs more than 20 times slower than x86-64 code on the same work; or a first screen takes more than 60 seconds | **Killed.** In each of two runs one launch of an ARM64 application under QEMU drew its window (the first, after 98 s and 49 s) and the six launches after it died inside the emulator within seconds, the second time with ART's just-in-time compiler off. No benchmark was obtained in either run. The emulator itself is 6 to 16 times slower on plain work (section 5) |
| C | Waydroid can be started in GitHub Actions, on the runner's kernel or in a KVM guest running Fedora 45's kernel, as far as one application's first screen in a screenshot | Android does not finish booting within ten minutes in either place; then the report says exactly what stops it | **Met.** Android 13 boots in CI in both places (44 s on the runner, 27 s on TUR's kernel with the Rust binder driver under SELinux enforcing, 0 denials). Run 43747969168: after `waydroid show-full-ui` every installable application of the sample shows a usable first screen in a screenshot of the desktop, **29 of 29** on the runner and **10 of 10** on TUR's kernel (the thirtieth is ARM only). With `persist.waydroid.multi_windows=true` and `waydroid app launch`, each application is **its own desktop window** (section 6) |
| D | Some Android applications are a web view around bundled web assets, and such an application can be run as a web application with no Android at all: at least 3% of F-Droid's applications are of this kind, and one of them shows its first screen in a plain browser | None of up to four web bundles tried shows its first screen without the native bridge | **The share is under the bar; the re-hosting works.** 10 of 600 are certainly web-view applications (1.7%), 16 of 600 on inspection (2.7%, interval 1.6 to 4.3): under 3%. **4 of 4** tried show their first screen in a plain browser, 2 of them with nothing added (section 7). A second sample drawn from a newer index in run 43747969168: 3 of the 3 bundles the harness opened correctly show their first screen with nothing added (section 7.3) |

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
| Clipboard | Waydroid's own clipboard service starts only if the Python module `pyclip` can be imported (`tools/services/clipboard_manager.py`). **Fedora's package neither requires nor recommends `python3-pyclip`** (which Fedora 45 has, 0.7.0). Measured: the clipboard works both ways without it (sections 4 and 6.1) |
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

**Repeated** in run 43747969168 on the index of 13 November 2026 (SHA-256
`70c807e8…0a0d`, 4,996 applications): 42.0% no native code, 50.8%
x86-64, **92.8%** need no translation, 6.9% ARM only (345, of which 332
have 64-bit ARM code); 91.5% of those updated in the last two years.

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

**Measured**, run 43553105465, jobs `atl (sample)` and `atl (random)`
(artifacts `exp-android-atl-sample` and `-random`: `results.json`,
`shots/`, `logs/`, contact sheets). ATL as Alpine edge packages it
(`android-translation-layer-0_git20261019-r1`, `art_standalone-0_git20261018-r0`,
`bionic_translation-0_git20260927-r0`, GTK 4.24.1, WebKitGTK 2.54.2),
x86-64, in a container on the runner, headless sway 1.12, software
rendering with GTK's default renderer (the probe's window was drawn, so no
fallback was needed). Every window's screenshot was looked at, the
doubtful ones at full size; the verdicts are mine, by the definition fixed
before measuring.

| | Hand-picked 30 | Random 30 | Together |
|---|---|---|---|
| **Usable first screen** | **7 (23%)** | **5 (17%)** | **12 of 60 (20%)** |
| Window drawn with visible defects (not counted) | 5 | 2 | 7 |
| Window blank, or stuck on a splash screen | 1 | 3 | 4 |
| No window | 17 | 20 | 37 |

**Repeated in run 43747969168** with the same packages: the same 13 and
10 applications opened a window and the rest stopped at the same first
blockers; I looked at the first-screen contact sheets of both samples. One
verdict moves: `net.toload.main.hd`, whose Chinese text showed as boxes,
is legible once the test container has Noto CJK (added for this run), so
the random sample reaches **6** usable first screens and the two samples
13 of 60. The hand-picked count, which decides A, is unchanged at 7.
`com.jellodog.chat` (blank before) now exits after its window appears.

**The kill criterion (fewer than 3 of 30) is not met: hypothesis A
survives.** A useful share, but one in five, not most: ATL is a route for
applications known to work, not one to try on an unknown APK.

Of the 23 windows, none crashed in the ten seconds or so it was watched,
and all 23 survived a resize to 760 × 620.

| Application | Version | Native code | ATL: first screen | What is seen, or the first blocker | Window (s) | Memory | Waydroid: started | Waydroid: memory |
|---|---|---|---|---|---|---|---|---|
| `org.billthefarmer.editor` | 1.98 | none | No window | missing method `setSubtitle` | – | – | yes, 456 ms | 76 MiB |
| `net.gsantner.markor` | 2026.10.0 | none | No window | missing method `setExitTransition` | – | – | yes, 617 ms | 157 MiB |
| `org.fossify.notes` | 1.7.0 | x86-64 | **Usable** | note editor with toolbar | 2.65 | 337 MiB | yes, 308 ms | 60 MiB |
| `com.orgzlyrevived` | 1.23.0 | x86-64 | Partial | its What's New dialog legible with raw HTML tags; the view behind it broken | 5.5 | 423 MiB | yes, 1113 ms | 140 MiB |
| `org.videolan.vlc` | 3.7.2 | x86-64 | No window | native library needs `__strlcpy_chk` | – | – | yes, 1025 ms | 113 MiB |
| `org.schabi.newpipe` | 0.29.1 | x86-64 | **Usable** | live list fetched from the network | 4.89 | 401 MiB | yes, 977 ms | 199 MiB |
| `org.fossify.gallery` | 1.14.2 | x86-64 | Partial | toolbar drawn, content area empty | 4.07 | 348 MiB | yes, 522 ms | 88 MiB |
| `org.oxycblt.auxio` | 4.1.5 | x86-64 | No window | `NoSuchMethodException` | – | – | yes, 1349 ms | 106 MiB |
| `de.danoeh.antennapod` | 3.12.3 | x86-64 | **Usable** | home view with bottom tabs | 4.3 | 400 MiB | yes, 1226 ms | 113 MiB |
| `org.fossify.filemanager` | 1.6.4 | x86-64 | Partial | toolbar and tabs drawn, list area empty | 3.27 | 350 MiB | yes, 626 ms | 86 MiB |
| `org.fossify.clock` | 1.6.0 | x86-64 | Partial | frame and tabs drawn, the clock itself missing | 2.86 | 344 MiB | yes, 601 ms | 67 MiB |
| `com.termux` | 0.118.3 | x86-64 | No window | class not found `com.termux.filepicker.TermuxDocumentsProvider` | – | – | yes, 294 ms | 98 MiB |
| `org.fdroid.fdroid` | 1.23.2 | x86-64 | No window | exits with nothing logged | – | – | yes, 540 ms | 101 MiB |
| `com.beemdevelopment.aegis` | 3.4.3 | x86-64 | **Usable** | main view, empty list with its hint | 2.85 | 372 MiB | yes, 716 ms | 63 MiB |
| `com.kunzisoft.keepass.libre` | 4.5.8 | x86-64 | Partial | drawn, but shapes overlap the buttons | 5.71 | 368 MiB | yes, 541 ms | 102 MiB |
| `org.isoron.uhabits` | 2.3.2 | none | **Usable** | introduction | 3.06 | 362 MiB | yes, 731 ms | 79 MiB |
| `com.artifex.mupdf.viewer.app` | 1.28.5a | 32-bit x86 | **Usable** | the system's file chooser, which is this application's first screen on Android too | 1.42 | 300 MiB | yes, 167 ms | 35 MiB |
| `net.sourceforge.opencamera` | 1.56.2 | none | No window | class not found `net.sourceforge.opencamera.MainActivity` | – | – | yes, 491 ms | 77 MiB |
| `org.wikipedia` | r/50616-r-2026 | x86-64 | No window | missing class `android.accounts.OnAccountsUpdateListener` | – | – | yes, 850 ms | 80 MiB |
| `app.organicmaps` | 2026.09.29-32- | x86-64 | Splash only | splash screen only, still there after the resize | 3.06 | 361 MiB | yes, 345 ms | 113 MiB |
| `com.shatteredpixel.shatteredpixeldungeon` | 4.0.2 | x86-64 | No window | no window within 60 s, still running | – | – | yes, 313 ms | 127 MiB |
| `org.moire.opensudoku` | 4.11.3 | ARM only | No window | `SQLiteException` | – | – | not installable: ARM only | – |
| `eu.siacs.conversations` | 2.21.0+free | x86-64 | No window | missing class `android.security.KeyChainAliasCallback` | – | – | yes, 470 ms | 93 MiB |
| `im.vector.app` | 1.6.62 | x86-64 | No window | missing class `android.security.KeyPairGeneratorSpec$Builder` | – | – | yes, 994 ms | 173 MiB |
| `com.fsck.k9` | 24.0 | x86-64 | **Usable** | welcome screen with Get started | 5.1 | 356 MiB | yes, 523 ms | 83 MiB |
| `de.baumann.browser` | 24 | none | No window | missing method `getTextCursorDrawable` | – | – | yes, 1066 ms | 189 MiB |
| `org.mozilla.fennec_fdroid` | 159.0.1 | x86-64 | No window | native library needs `__register_atfork` | – | – | yes, 365 ms | 236 MiB |
| `org.localsend.localsend_app` | 1.18.2 | x86-64 | No window | `libflutter.so` does not link | – | – | yes, 739 ms | 250 MiB |
| `org.jitsi.meet` | 26.0.0 | x86-64 | No window | missing method `getApplicationRestrictions` | – | – | yes, 482 ms | 317 MiB |
| `de.tutao.tutanota` | 366.261105.0 | x86-64 | No window | missing method `setSafeBrowsingEnabled` | – | – | yes, 465 ms | 197 MiB |

"Waydroid: started" is Android's own report in run 43553105465 (activity
resumed, process alive ten seconds later). In run 43747969168 every one of
these 29 showed a **usable first screen on the desktop** under Waydroid,
including the four built with Flutter, React Native, Gecko or a web view
and all 16 installable ones that open no window under ATL (section 6.2).

| Application | Version | Native code | ATL: first screen | What is seen, or the first blocker | Window (s) | Memory |
|---|---|---|---|---|---|---|
| `io.github.dorumrr.de1984` | 2.8.4 | none | No window | missing method `getUserProfiles` | – | – |
| `com.rascarlo.adaptive.brightness.tile` | 0.2 | none | No window | missing class `android.service.quicksettings.TileService` | – | – |
| `com.quaap.launchtime` | 0.8.6 | none | No window | missing class `android.appwidget.AppWidgetHost` | – | – |
| `de.bixilon.unithen` | 1.3.2 | x86-64 | No window | `BootstrapMethodError` | – | – |
| `com.werismoln.multibooter` | v0.1.4 | x86-64 | **Usable** | introduction | 1.44 | 308 MiB |
| `br.com.gualandi.dailypill` | 1.3.3 | none | **Usable** | main view | 2.88 | 328 MiB |
| `com.darkrockstudios.apps.fasttrack` | 5.1.1 | x86-64 | No window | `NoSuchMethodException` | – | – |
| `com.yahyaoui.prayermode` | 1.8.5 | none | **Usable** | settings view | 3.09 | 358 MiB |
| `com.bookorbit` | 0.7.1 | x86-64 | No window | no window within 60 s, still running | – | – |
| `app.auriel.nocean` | 0.2.0 | x86-64 | No window | missing method `setColor` | – | – |
| `net.toload.main.hd` | 7.6.0 | none | Partial (run 43553105465); **Usable** (run 43747969168) | drawn; its Chinese text showed as boxes for want of a CJK font in the test container; with Noto CJK, its set-up screen is legible | 2.27 | 334 MiB |
| `ro.radioromaniaactualitati.podcasts` | 2.5.7 | x86-64 | No window | missing class `android.media.session.MediaController$Callback` | – | – |
| `io.github.sykoram.sis` | 1.9.0 | none | Partial | text legible, layout broken | 3.7 | 523 MiB |
| `com.serwylo.lexica` | 3.13.1 | none | Blank | empty window | 3.93 | 340 MiB |
| `org.astonbitecode.rustkeylock` | 0.18.3 | ARM only | No window | missing method `startForegroundService` | – | – |
| `com.jellodog.chat` | 1.2.5 | none | Blank | empty window | 4.54 | 545 MiB |
| `com.mouzinho.pokebase` | 1.0.2 | x86-64 | No window | native library needs `__register_atfork` | – | – |
| `com.listen1.app` | 0.8.1 | x86-64 | No window | no window within 60 s, still running | – | – |
| `com.github.shadowsocks.plugin.ck_client` | 2.12.0 | x86-64 | No window | exits with nothing logged | – | – |
| `com.bmco.cratesiounofficial` | 1.5.3 | none | No window | `IllegalStateException` | – | – |
| `de.jepfa.bdt` | 1.6.1 | x86-64 | No window | `libflutter.so` does not link | – | – |
| `com.eneko.hexcolortimewallpaper` | 1.1 | none | No window | missing class `android.preference.PreferenceFragment` | – | – |
| `org.ligi.gobandroid_hd` | 2.5.9 | none | No window | native crash | – | – |
| `com.bosun.app` | 0.8.6 | x86-64 | No window | native library needs `__register_atfork` | – | – |
| `tribixbite.cleverkeys` | 1.5.0 | x86-64 | **Usable** | set-up view | 10.33 | 834 MiB |
| `com.agateau.catgenerator` | 0.2.1 | none | Blank | title bar only, content black | 3.09 | 355 MiB |
| `com.wagebit.app` | 0.1.0 | none | **Usable** | dashboard | 2.06 | 302 MiB |
| `io.gitlab.cryptographic_id` | 0.5.7 | x86-64 | No window | `libflutter.so` does not link | – | – |
| `fr.odrevet.kingdomino_score_count` | 5.0.2 | x86-64 | No window | `libflutter.so` does not link | – | – |
| `com.wangjinli.carvita` | 1.2.3 | x86-64 | No window | `libflutter.so` does not link | – | – |

**Why applications fail** (the first error logged, 37 applications
without a window):

| First blocker | Hand-picked | Random | Notes |
|---|---|---|---|
| A method or class of the Android framework that ATL does not have | 12 | 9 | Twenty-one different ones: `ActionBar.setSubtitle`, `android.security.KeyChain…`, `android.accounts…`, `WebSettings.setSafeBrowsingEnabled`, `android.appwidget…`, `TileService`, `PreferenceFragment`, … Each is the kind of stub the ATL README asks contributors for; what lies behind the first one is unknown |
| The application's native library does not load | 3 | 6 | **Three causes cover all nine**: `libflutter.so` does not link (all 5 Flutter applications in the two samples); `__register_atfork` is missing from the C-library shim (Firefox's `libmegazord.so`, React Native's `libreactnative.so`, one more); `__strlcpy_chk` is missing (VLC) |
| No window within 60 seconds, process still running | 1 | 2 | Includes the one game (libGDX); with no GPU in CI this may be the test bed |
| ARM-only APK | 1 | 1 | Expected on x86-64 |
| Other crash | 0 | 2 | |

By what the applications are built with: **Flutter 0 of 5, React Native
0 of 2, Gecko 0 of 1.** Jetpack Compose is not a barrier in itself (K-9
Mail's welcome screen, WageBit and CleverKeys are drawn). The usable ones
are plain Android-toolkit applications.

**Could TUR's contributing change the number?** For a specific
application, yes, and cheaply: the blockers are shallow and named, and
upstream takes such patches (293 merged merge requests). For the share as
a whole, three fixes in `bionic_translation` (the Flutter link failure and
two missing C-library symbols) would move the first blocker of 9 of the 60
applications, among them every Flutter application, which are 12% of
F-Droid; what those applications hit next is not known. The 21 framework
gaps are 21 separate pieces of work.

**Start-up and memory** (the 23 applications with a window):

| | ATL (per application) | Waydroid (run 43553105465) |
|---|---|---|
| Before the first application | nothing | Android boots: 44 to 45 s on the runner, 27 s in the guest (first boot; 20 to 24 s later); then stays up |
| Start of one application | Window after 1.4 to 5.7 s, median 3.3 s (hand-picked) and 3.1 s (random); picture still after a median of 4.1 s and 3.7 s. Each launch here is a first launch, bytecode compiled on the spot | `am start -W` total time 167 to 1,349 ms, median 541 ms (installed just before; software rendering) |
| Memory before the first application | nothing | **+956 MiB** on the runner, +936 MiB in the guest (host memory in use, idle Android, 95 processes); +903 and +857 MiB in run 43747969168 |
| Memory per application (PSS) | **300 to 423 MiB, median 361** (hand-picked); 302 to 834, median 347 (random). The probe, which does nothing, takes 294 MiB | 35 to 317 MiB, median 102 (Android's own figure); the probe 72 MiB |

So **ATL is not the lighter route once several Android applications are
open**: every ATL process carries its own runtime, toolkit and (as run
here) web engine, about 300 MiB before the application does anything.
One application costs about 0.35 GB under ATL and about 1.05 GB under
Waydroid; at four the two are level (about 1.4 GB); beyond that Waydroid
is cheaper. Caution: each ATL application was measured alone, and two
running together would share part of their libraries; `ATL_UGLY_ENABLE_WEBVIEW`
was set for all, which loads WebKitGTK.

## 4. Integration facts (probe application)

**Measured.** ATL: runs 43553105465 and 43747969168, job `atl (sample)`
(`results.json` → `probe`, `shots/probe-*.png`, `logs/probe-*`).
Waydroid: run 43747969168, jobs `waydroid` (runner's kernel) and
`waydroid-f45` (TUR's kernel), in Waydroid's full-screen window after
`waydroid show-full-ui` unless the row says multi-window (`facts.txt`,
`probe-*.txt`, `shots/`, `android-shots/`). The probe is
`tests/experiments/android/probe`, built in CI. Where the two Waydroid
jobs differ, the row says so.

| Question | ATL | Waydroid (run 43747969168) |
|---|---|---|
| **Is the application on the accessibility bus**, so that P@x could operate it? | **Yes.** The probe is an AT-SPI application with 28 nodes (frame, panels, 3 buttons, 4 labels), 26 of them with actions; **its own texts and button label are there** (`TUR probe: basic`, `tur-probe-tap`, the log text). Of the 23 real applications with a window, 21 are on the bus (1 to 2,455 nodes) and 14 expose their own text beyond the window title (for example Aegis's "No entries found", Loop Habit Tracker's introduction, WageBit's figures); 2 expose nothing | NOT RUN. By design Android's views are not on the host's bus; android-runtime.md section 9 plans a separate tier |
| Pointer | **Yes**: a click on the probe's button counted (`tap=1`), 8 touches seen | **Yes**, both places: six clicks from the host gave five touches in the application, one of them a tap on its button (`tap=1`) |
| Keyboard | **Yes**: typed letters arrive in the text field (`text=qqqqqqq`) | **Key events arrive, typed text not shown.** All seven key presses (six letters typed with `wtype` and one Escape) reached the application as `KEYCODE_ESCAPE` (111) followed by Back; no letter arrived, in both places. `wtype` uploads a keymap of its own with each call, in which the first key sits on Escape's scan code; Waydroid appears to read scan codes and ignore the client's keymap. A real keyboard uses the standard keymap, so this is probably the test bed, but **typed text into Waydroid is NOT measured** |
| Back | The **button ATL puts in the title bar** calls `onBackPressed`; the **Escape key does not** | **Escape is Back**: every Escape reached `onBackPressed` (7 of 7). Android's navigation bar (full-screen window) and the caption bar Android draws on each free-form window (multi-window) also carry Back |
| Resize | **Yes**: the window was resized to 760 × 620 and the application was laid out again at 760 × 573; all 23 real applications survived the same resize | **No, in multi-window mode.** The desktop's tile was 640 × 800 and the probe's window was then made floating and resized to 700 × 500 by the compositor; the application stayed laid out at 364 × 591 / 364 × 543, Android's own free-form size, and its picture stayed where Android placed it (section 6.2). The full-screen window was not resized |
| Notification | **Yes**: ATL calls `org.freedesktop.Notifications.Notify` directly (not the portal); the notification daemon showed "tur-probe-notification" (visible in the screenshots) | **Yes**, both places, with a notification server running before `waydroid session start`: Waydroid calls `org.freedesktop.Notifications.Notify` with the application's name (`TUR probe`, `F-Droid`) as the sender; the daemon showed the probe's notification and Android's own ("Configure physical keyboard") |
| Open a file (`ACTION_OPEN_DOCUMENT`) | **The desktop portal**: `org.freedesktop.portal.FileChooser.OpenFile`; the portal's "Open File" window appeared | Android's own picker (`com.android.documentsui`), inside Android, over Android's own storage; no portal |
| Save a file (`ACTION_CREATE_DOCUMENT`) | **The portal**: `FileChooser.SaveFile`, "Save File" window | NOT RUN |
| Open a web address (`ACTION_VIEW`) | **The portal**: `OpenURI.OpenURI`; the portal's "Open With…" window appeared (the container has no browser) | **Android's own browser** (LineageOS's Jelly) inside Android; the host's browser is not asked (Fedora: the page loaded; runner: still loading at the screenshot) |
| Share text (`ACTION_SEND`) | ATL's own small "Share" dialog: Cancel, Copy, Email | NOT RUN |
| WebView | **Renders** (WebKitGTK), when `ATL_UGLY_ENABLE_WEBVIEW` is set | **Renders** (Android's WebView), both places |
| Clipboard, set by the application with no input event | **Did not reach the host** and the application read nothing back. wlroots refuses a clipboard from a client that cannot name an input event, and the probe set it on a timer: this is the test, not ATL | **Android to host: yes**, both places. **Host to Android: yes**, both places: text put on the host's clipboard with `wl-copy` was read by the application (`clipboard_get=tur-host-clip-123`). Both **without `python3-pyclip`** (section 6.1) |
| **Clipboard after a click** (run 43747969168) | **Android to host: yes**: the text the application set in answer to the click was on the host's clipboard. **Host to Android: no**: the application read nothing, before the click and at it (`clipboard_at_tap=(empty)`) | **Host to Android: yes**, both places (`clipboard_at_tap=tur-host-clip-123`). **Android to host: yes on the runner, no on Fedora**: there the host's clipboard still held its own text after the application set its own at the click. One try each; the timer case before it worked in both places |
| What the application is told about the device | Android 5.0 (API 21), "HTC / unknown", ABIs `x86_64, x86` | Android 13 (API 33), "Waydroid / WayDroid x86_64 Device", ABIs `x86_64, x86` |
| What the application is told about Google Play services | **Present**, version 25.09.32, with Google's own certificate fingerprint (`38918a45…5788`): ATL answers with a stub (`loadGStub`) | Absent (VANILLA image). **With microG installed: present**, version 0.3.18.252432, **with Google's certificate fingerprint** (`38918a45…5788`), although microG is signed with its own key: the image presents Google's signature for microG (section 6.1) |
| A window the application marks secure (`FLAG_SECURE`: Aegis, KeePassDX) | Not measured | **Visible to the host.** Android's own screenshot of Aegis and KeePassDX is black where their content is; the host compositor's screenshot shows it. Any host program that can capture the screen sees what Android hides from its own capture |
| Same work, same checksum? | Yes, in Java and in native code (`c38f0c76db79b09e`, `56867.75`), as on Waydroid | Yes |

Benchmark of the probe (milliseconds; integer work, then floating point;
second pass). The three jobs ran on different runners, so only the
columns within a row compare:

| Where | Java, integer | Java, floating point | Native, integer | Native, floating point |
|---|---|---|---|---|
| ATL, x86-64 | 118 | 68 | 116 | 46 |
| Waydroid, runner's kernel | 269 | 363 | 168 | 108 |
| Waydroid, Fedora guest under KVM | 327 | 407 | 177 | 114 |
| Waydroid, runner's kernel (run 43747969168) | 273 | 372 | 168 | 108 |
| Waydroid, Fedora guest under KVM (run 43747969168) | 337 | 445 | 197 | 121 |

Under ATL the application's bytecode is compiled ahead of time on first
launch and runs as fast as the native library; under Waydroid's Android 13
the same Java code, in its first seconds, is 1.6 to 3.6 times slower than
the native library.

**What this means for the design.** An ATL application is an ordinary
desktop citizen: accessibility bus, portals for files and links, the
desktop's notifications, pointer, keyboard, resizing. P@x's existing tier
for native applications applies to it without anything Android-specific.
The gaps seen: Escape is not Back, the host's clipboard does not reach
the application, and the application is told it runs on Android 5.0.

A Waydroid application reaches the person through the desktop's pointer,
clipboard (both ways) and notifications, but it stays inside Android for
everything else: opening a file and opening a link go to Android's own
picker and browser, over Android's own storage (saving and sharing were
not tested), the desktop cannot size its window,
and it is not on the accessibility bus. Those are the gaps TUR's runtime
has to bridge (section 6.4).

## 5. Hypothesis B2: ARM applications through QEMU user-mode emulation

**Measured**, run 43553105465, job `atl-arm64`: Alpine edge for aarch64
with the same ATL packages, in a container under `qemu-aarch64` 10.2.3
(binfmt_misc) on the x86-64 runner. Everything in that container is
emulated, the compositor included. **Killed** (the second try, run
43747969168, is below the first table).

| Step | Result |
|---|---|
| The ARM64 userland under emulation (sway, GTK, Python, the harness) | Works |
| **First launch of the ARM64-only probe application** | **A window, drawn (881 colours), process alive.** The application reports `os_arch=aarch64`, `supported_abis=[arm64-v8a]`. Window after **98 seconds**, of which 61 s is ART compiling its boot image once (160 s of CPU), 8 s compiling ATL's own framework, and under 1 s the application |
| Six launches after it (the probe twice, four applications) | **All died within seconds**: `qemu-aarch64: QEMU internal SIGSEGV {code=ACCERR}` or a segmentation fault in the C library under GLib. No window was caught by the harness |
| Benchmark, Java against native ARM code | **NOT RUN**: the process died before it |
| The emulator on plain work, same machine (`openssl speed`) | SHA-256: 96 MB/s emulated against 1,541 MB/s native, **16 times slower** (the native side has the processor's SHA instructions); RSA-2048 signatures: 236 against 1,501 per second, **6.3 times slower** |

Against the kill criteria: an application did start, once; its first
screen took more than 60 seconds (98 s, a one-time cost on the first
launch ever); and the speed of native ARM code in an application could
not be measured. One success in seven launches is not a route.

**One more try: ART's just-in-time compiler off** (run 43747969168, the
same job with `-X -Xusejit:false` on every launch). The conjecture was
that the JIT writes machine code through one memory mapping and runs it
through another, which a user-mode emulator cannot follow. ATL accepted
the option (an unknown option stops every launch, and the first one drew
a window); the logs do not show ART echoing it.

| Step | Result, JIT off |
|---|---|
| First launch (the probe, empty compile cache) | **A window, drawn (873 colours), process alive**, after **49 s** (98 s in the first run, another runner) |
| Six launches after it (the probe's `basic` and `bench` phases, four applications) | **All died within seconds**, as before: `Fatal signal 11 (SIGSEGV), code 2 (SEGV_ACCERR)` in ART, or `qemu-aarch64: QEMU internal SIGSEGV {code=ACCERR}`. The four desktop screenshots are empty |
| Benchmark, Java against native ARM code | **NOT RUN again**: the `bench` phase died before it |
| The emulator on plain work, this runner (`openssl speed`, 16 KiB blocks) | SHA-256: 172 MB/s emulated against 2,132 MB/s native, **12 times slower**; RSA-2048 signatures: 389 against 4,964 per second, **13 times slower** |

The just-in-time compiler is not the cause, or not the only one: the
pattern "the first launch after an empty cache works, every later launch
dies" held for 6 of 6 launches in both runs. What differs between the
first launch and the later ones is that the later ones load the code
compiled ahead of time by the first (ART's boot image and ATL's framework,
under `~/.cache/art`); that loading is the next suspect, not tested.
**B2 is killed**: no shippable open-source route started more than one
application per run, no speed figure for native ARM code in an
application was obtained, and the one route found would cost an emulated
ATL, an ARM build of its whole runtime, and a first launch of about a
minute.

The idea stands on its design, and nobody was found to have published it:
ATL turns an Android application into one ordinary Linux process, so the
whole of ATL, as an ARM64 program, can run under user-mode emulation and
still be a native window, with no proprietary library and nothing in the
kernel. Section 3 limits what it could ever cover: an ARM-only application
must first be one that ATL runs at all.

## 6. Hypothesis C: Waydroid in GitHub Actions

**Met.** Android boots in CI in both places and its applications reach the
desktop. Run 43553105465 (commit `0f56629`) booted Android 13 on the
runner's kernel and in a KVM guest on TUR's own kernel (`7.3.3-300.fc45`,
Rust binder driver, SELinux enforcing) and started every application that
has x86 code, but drew nothing on the desktop, because the harness
started applications with Android's `am start`. Run 43747969168 (commit
`f7fc92d`) asked Waydroid for its display through Waydroid's own
interface, and **every installable application of the sample shows a
usable first screen in a screenshot of the host compositor: 29 of 29 on
the runner, 10 of 10 on TUR's kernel.** I looked at every desktop
screenshot and every Android screenshot of both jobs (section 6.2).

### 6.1 What Waydroid needed to start (for the runtime work under ADR-0006)

Measured in run 43553105465 unless a line says otherwise. The script is
`tests/experiments/android/waydroid/run.sh`; it runs unchanged on Ubuntu
and on Fedora 45.

| Need | What was measured |
|---|---|
| **Binder, on TUR's kernel** | Fedora 45 updated to `7.3.3-300.fc45` (the kernel of TUR's image): `# CONFIG_ANDROID_BINDER_IPC is not set`, `CONFIG_ANDROID_BINDER_IPC_RUST=y`, `CONFIG_ANDROID_BINDER_DEVICES="binder,hwbinder,vndbinder"`. Fedora's mount unit puts `binder`, `hwbinder`, `vndbinder` and `binder-control` in `/dev/binderfs`; Waydroid's configuration then reads `binder = binder`, `vndbinder = vndbinder`, `hwbinder = hwbinder`. **Nothing to load, build or configure.** Android booted on the Rust driver |
| Binder, kernel messages | On the Rust driver: `rust_binder: Failed to allocate buffer. len:96, is_oneway:true` four times in a six-minute session (twice in run 43747969168, `len:104` and `len:136`), with no visible effect; and the usual "undelivered transaction … process died" and "undelivered death notification" when applications were stopped |
| **Packages** | `waydroid-1.6.3-2.fc45` and `waydroid-selinux` (which it pulls in), LXC 7.0.0, `python3-gbinder` 1.3.0. Nothing from outside Fedora |
| **SELinux** | **Enforcing throughout; 0 denials** over the whole run (boot, 10 applications, a second boot), in the audit log and the journal; again 0 in run 43747969168, which also drew windows, ran microG and booted a second time in multi-window mode |
| **Images without the network** | `system.img` (1,827,717,120 bytes) and `vendor.img` (562,388,992 bytes) placed in `/usr/share/waydroid-extra/images` (label `usr_t` after `restorecon`). `waydroid init -f` then takes one to two seconds, asks no server, and records `system_ota = None`. This is the path for an image TUR pins and fetches itself. Archives: 847 MB and 190 MB |
| **Container service** | `systemctl restart waydroid-container` after `init`; active |
| **A sound server in the session** | **Required.** Waydroid binds `$XDG_RUNTIME_DIR/pulse/native` into the container and LXC refuses to start without it (`Failed to mount "/run/user/1000/pulse/native"`, run 43537684872). PipeWire's `pipewire-pulse` satisfies it. On a Plasma session it is there; a service that starts Android outside a full session must check for it first, or the error is "container failed to start" with no hint |
| **A notification server in the session** | Without one on the bus at session start Waydroid says `Skipping notification manager service because we could not connect to the notifications server` and Android notifications go nowhere (measured: the probe's notification never reached the bus). Plasma has one; the order matters: it must be there *before* `waydroid session start`. **With one (mako, run 43747969168) notifications are forwarded**, in both places, through `org.freedesktop.Notifications.Notify` with the application's name |
| **No GPU** (CI only) | `ro.hardware.gralloc=default` and `ro.hardware.egl=swiftshader` under `[properties]` in `/var/lib/waydroid/waydroid.cfg`, then `waydroid upgrade --offline`. On a computer with a Mesa driver these lines are absent |
| **Boot** | `waydroid session start` as the user, with `WAYLAND_DISPLAY`, the session bus and `XDG_RUNTIME_DIR`. `sys.boot_completed=1` after **27 s** in the guest (first boot, empty data) and 24 s on the second boot; 45 s and 20 s on the runner (44 s and 22 s in run 43747969168). Software rendering, 4 CPUs |
| **A window on the desktop** | **Only through Waydroid's own interface.** `waydroid session start` maps no window, and an activity started with Android's own `am start` runs invisibly (run 43553105465: 29 applications "resumed", the desktop empty). Run 43747969168: before `waydroid show-full-ui` the compositor had 0 windows; after it, **one window, app id `Waydroid`, title `Waydroid`, 1280 × 800** (the whole output), holding Android's whole interface: status bar, navigation bar, launcher, and whichever application is in front, including those started afterwards with `am start` (property `waydroid.active_apps=Waydroid`). In multi-window mode `waydroid app launch PACKAGE` gives **one window per application** (next row) |
| **Installing** | `pm install -r -g` through `waydroid shell` (which reads commands on standard input). An ARM-only APK is refused with `INSTALL_FAILED_NO_MATCHING_ABIS` (OpenSudoku 4.11.3 and the probe's ARM64 variant). The image's ABI list is `x86_64,x86`: an application with only 32-bit x86 code installed and started (MuPDF viewer). `ro.dalvik.vm.native.bridge=0` |
| **Applications** | Runner: 29 of 30 installed, started (`am start -W` status ok, 167 to 1,349 ms) and were alive with their own activity in front ten seconds later; the thirtieth is the ARM-only one; no crash logged. Guest, first ten: 10 of 10 |
| **Memory** | Host memory in use rose by **956 MiB** (runner) and **936 MiB** (guest) from before the session to a booted, idle Android with 95 processes (903 and 857 MiB in run 43747969168). Per application (Android's own PSS figure): 35 to 317 MiB, median 102 MiB; the probe 70 to 72 MiB |
| **Clipboard** | **Both ways, without `python3-pyclip`**, on both (run 43747969168): the application's text was read on the host with `wl-paste`, and text put there with `wl-copy` was read by the application, with the focus in Waydroid's window; again with pyclip installed, in multi-window mode. Waydroid 1.6.3's own clipboard service (`tools/services/clipboard_manager.py`) does not start when pyclip cannot be imported (it logs that at debug level only), so something else carries the clipboard in this image, most likely the Wayland client on Android's side (not traced). One failure: on Fedora, text the application set in answer to a click did not replace the host's (one try) |
| **One window per application** | With `persist.waydroid.multi_windows=true` (set with `waydroid prop set`, then the session restarted) and `waydroid app launch org.tur.probe` and `waydroid app launch com.android.settings`, the compositor had **two windows: app id `waydroid.org.tur.probe`, title `TUR probe`, and app id `waydroid.com.android.settings`, title `Settings`**, both places. Inside each, Android draws its own free-form caption bar (Back, minimise, maximise, close). **The desktop does not size them**: the compositor tiled each at 640 × 800, then floated the probe's and resized it to 700 × 500, and the application stayed laid out at Android's own 364 × 591 / 364 × 543, its picture where Android had placed it (section 6.2). Input in this mode was not tested |
| **microG** | Installs (`Success`). The image does not define `android.permission.FAKE_PACKAGE_SIGNATURE` (`pm list permissions`: 0 matches), **but an application asking for Play services' signature gets Google's** (run 43747969168, both places): the probe sees `com.google.android.gms` version 0.3.18.252432 with signer SHA-1 `38918a45…5788`, Google's certificate, while `dumpsys package` shows microG's own signing key (`9B:D0:67:27…`). The image spoofs the signature for microG alone, as LineageOS does since 2024, without the permission being defined. **This corrects run 43553105465's reading of the same image** (absent permission taken to mean no spoofing). Whether microG's services (push, location) then work was not tested |
| **Cost in CI** | Guest job 11.9 minutes in all: kernel update and restart 52 s, packages 42 s, images and applications copied in 31 s, the test itself 5.8 minutes |

### 6.2 Windows on the desktop (run 43747969168)

Two modes were measured, one after the other, in both places. Every step
is in `waydroid/run.sh`.

**Full-screen window** (`waydroid show-full-ui` once after the boot, then
each application installed and started with `am start`). One host window
holds Android's whole interface. The 29 applications of the sample with
x86 or no native code each drew a **usable first screen** by the
definition fixed before measuring: an introduction, a welcome or sign-in
screen, a main view, or the application's own permission or set-up
dialog (Organic Maps' map download, Markor's and Aegis's introductions,
Fossify's storage-permission dialogs, Termux's prompt with its extra
keys, Shattered Pixel Dungeon's title screen, LocalSend (Flutter), Jitsi
Meet (React Native), Fennec (Gecko), Tutanota (web view)). Open Camera
shows its own dialog over a black preview: there is no camera. The same
ten on TUR's kernel look the same. The desktop's picture and Android's
own screenshot match, except for Aegis and KeePassDX, which Android's own
capture blacks out (`FLAG_SECURE`) and the desktop's shows (section 4).

**Multi-window mode** (`persist.waydroid.multi_windows=true`, session
restarted, applications launched with `waydroid app launch PACKAGE`).
Each launched application is a separate toplevel with app id
`waydroid.PACKAGE` and the application's label as title, so a task bar
and a window switcher can tell them apart. But the windows are Android's,
not the desktop's: Android chooses their size (free-form, 364 pixels wide
for the probe at 180 dpi) and position, and draws its own caption bar
inside; the compositor's tiling (640 × 800) and an explicit resize to
700 × 500 left the application's layout unchanged, and the pictures were
drawn at Android's free-form positions, overlapping, rather than filling
the windows the compositor gave them. Measured under sway; KWin, TUR's
compositor, was not tried and may place them differently, but the
application's size did not follow the desktop's in either job.

So a Waydroid application can be its own desktop window, with its own
identity, and receives the desktop's pointer, clipboard and
notifications; the desktop cannot yet size or place it.

### 6.3 The earlier run, and a correction to android-runtime.md

Run 43537684872 stopped at the container's start in both places for want
of the sound-server socket (6.1). It also measured the kernel of Fedora
45's Cloud image **as released**, `7.2.11-300.fc45`:
`CONFIG_ANDROID_BINDER_IPC=y`, `CONFIG_ANDROID_BINDERFS=y`, no Rust binder
option: the **C** driver, built in. android-runtime.md section 3.3 says
Fedora 43, 44 and 45 carry the same configuration as TUR's `7.3.3` kernel
and cites Waydroid working on a Fedora 44 kernel `7.2.9`. The switch is
between 7.2 and 7.3, not between Fedora releases; that citation is
evidence for the C driver. The evidence for the Rust driver is this
experiment's run 43553105465.

The runner's own kernel (`7.0.0-1017-azure`) has the C driver as a module
in its stock image; Waydroid there creates `anbox-binder`,
`anbox-hwbinder` and `anbox-vndbinder` through `binder-control`.

### 6.4 For TUR's Android runtime (pull request 137, `feat/android-runtime`)

What the runtime built on the Waydroid route needs from this experiment,
all measured in run 43747969168 on Fedora 45's `7.3.3-300.fc45` kernel
with SELinux enforcing unless a line says otherwise.

**The launch command that maps windows.** One application per desktop
window:

```sh
waydroid prop set persist.waydroid.multi_windows true   # once; takes effect at the next session
waydroid session start                                  # as the user, in the graphical session
waydroid app launch PACKAGE                             # one host window: app id waydroid.PACKAGE, title = the label
```

The whole Android interface in one window instead: `waydroid show-full-ui`
(app id `Waydroid`). **Never Android's `am start`** (through `waydroid
shell`) to *show* something: it runs the activity invisibly unless a
Waydroid window is already up (run 43553105465). Once the full-screen
window is up, `am start` does bring an application into it, which is how
the harness drove 29 applications. PR 137's `waydroid app launch` and desktop entries
(`Exec=waydroid app launch …`) are the right call.

**Required before `waydroid session start`**, in the user's session:

| Need | Why | Measured |
|---|---|---|
| A PulseAudio socket at `$XDG_RUNTIME_DIR/pulse/native` (`pipewire-pulse`) | LXC refuses to start the container without it | Run 43537684872 failed without it; `pipewire-pulse` on Fedora satisfies it |
| A notification server on the session bus | Waydroid checks once, at session start; otherwise Android's notifications go nowhere | Without: none forwarded (run 43553105465). With: forwarded as `Notify` with the application's name (run 43747969168) |
| `waydroid-container.service` active, `waydroid init` done from local images | — | `/usr/share/waydroid-extra/images` (label `usr_t`) under SELinux; init in 1 to 2 s, no network |
| `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS` in the environment of `waydroid session start` and of every `waydroid app launch` | The windows belong to that session | As run by `run.sh` (`asuser`) |

**Not required, measured:** `python3-pyclip` (clipboard both ways worked
without it; keeping it, as PR 137 does on the owner's decision, does no
harm), `firewalld` (on the Fedora guest `waydroid-net.sh` printed
`firewall-cmd: command not found` and Android still loaded a web page),
any kernel module or binder configuration (Fedora's binderfs devices are
used as they are).

**What failed or is missing, for the runtime to handle:**

1. **The desktop cannot size or place a per-application window.** Android
   decides the size (364 × 591 for the probe) and the position, and draws
   its own caption bar inside the host window; a compositor resize to
   700 × 500 changed nothing in the application. Under sway; KWin NOT RUN.
   The runtime's window rules, and any "maximise" or snapping, need a
   measurement under KWin before they are promised.
2. **Typed text is unmeasured.** Key events arrive (every key reached the
   application, Escape is Back), but the harness's virtual keyboard
   (`wtype`) sends its own keymap and every key arrived as Escape. A test
   with a standard keymap (a real keyboard, or a virtual keyboard that
   uploads the system's keymap) is needed before typing is claimed.
3. **Clipboard from Android after a click failed once on Fedora** (one
   try; it worked on the runner and from a timer on both).
4. **Files and links stay inside Android**: `ACTION_OPEN_DOCUMENT` opens
   Android's own picker over Android's storage, `ACTION_VIEW` Android's
   own browser. Nothing goes through the desktop's portals.
5. **Secure windows are not secure on the host**: content Android hides
   from its own screenshots (`FLAG_SECURE`, Aegis, KeePassDX) is in the
   host's screenshots.
6. **Not on the accessibility bus** (by design); P@x needs the separate
   tier android-runtime.md section 9 plans.
7. **ARM-only APKs are refused** at install (`INSTALL_FAILED_NO_MATCHING_ABIS`);
   no route for them (section 5).
8. Android's idle cost: about 0.9 GB of host memory from session start
   (857 to 956 MiB measured), 20 to 45 s to boot with software rendering.

**Correction for android-runtime.md section 12.2:** microG *can* present
Google's signature in the image of 27 September 2026 (section 6.1, row
microG); run 43553105465's "no" was read from the absent permission, not
measured from an application.

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

### 7.3 A second sample (run 43747969168)

The `fdroid` job ran again on the index of 13 November 2026. The 600
applications are drawn with the same seed over the sorted package names of
the *current* index, so a newer index gives a different sample; the
verdict of 7.1 rests on the first one. In the second, the automatic rule
finds 27 of 600 (4.5%, interval 3.1 to 6.5): 13 Capacitor, 3 Cordova, 11
other web bundles. They were not read by hand; the list holds at least two
of the kinds removed by hand in 7.1 (Feeder's translation page, AndBible's
Bible view), so the hand-read share would be lower. The four largest
bundles were opened in the browser; I looked at the screenshots:

| Application | Framework | As it is | With the stand-in |
|---|---|---|---|
| Logseq 0.10.15 (`com.logseq.app`) | Capacitor | **First screen**: its demo graph with the day's journal | The same |
| How2Vote 1.9.6 (`au.how2vote.app`) | Capacitor | **First screen**: the comparison's start page | The same |
| NicotinD 0.8.127 (`ar.kevinroberts.nicotind`) | Capacitor | **First screen**: its sign-in page, as in 7.2 | The same |
| Conceal Wallet 6.0.9 (`com.concealnetwork.concealmobile`) | Cordova | Not judged: the harness took `www/404/index.html` for the entry page and showed the application's own "Page not found" | The same |

Three of the three opened correctly reach their first screen with nothing
added. The harness's choice of entry page needs a fix (prefer the
shallowest `index.html` under the web directory) before the fourth can be
judged.

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
| **microG** | Apache-2.0; release v0.3.18.252432 of 12 November 2026; shippable in law | **Installs, and an application sees it with Google's signature** (run 43747969168, both places, section 6.1): Waydroid's image spoofs it for microG alone, as LineageOS does. Whether its push and location services then work: NOT RUN | Not applicable: there is no Android system to install it into. ATL's own answer is piecemeal: `gcm_service` for push messages | Push messages, a maps API on open map data, network location, sign-in for many applications. By its users' reports it does not reach the Play Integrity device verdict (not verified) |
| **The person's own opt-in to Google's** | Proprietary; not shipped | The person chooses upstream's `GAPPS` image and registers the device with Google by hand, as android-runtime.md section 12.2 describes | Not possible | Play Store and Play services, without device certification: no Play Integrity device verdict, so banking and similar applications still fail |

## 9. Recommended route order for TUR's router

Every rung rests on measurements (runs 43537684872, 43553105465 and
43747969168).

| Rung | Route | For which applications | Measured | Rests on |
|---|---|---|---|---|
| 1 | **Re-host as a web application**, no Android | Catalogue entries marked as a web bundle and checked once (Cordova, Capacitor, plain web view) | 1.7% of F-Droid certain, 2.7% likely; 4 of 4 tried show a first screen, and 3 of 3 in a second sample | Section 7. Catalogue only |
| 2 | **ATL**: the application as an ordinary process with a native window, as a Flatpak on ATL's base application | **Only catalogue entries measured to work under ATL.** Never an unknown APK | 12 of 60 reach a usable first screen (20%; 13 of 60 in the repeat); none of the Flutter, React Native or Gecko applications. Every application that worked under ATL also worked under Waydroid | Section 3 |
| 3 | **Waydroid container** (ADR-0006), **the general route** | Every other APK with x86-64, 32-bit x86 or no native code | 92.8% of F-Droid by ABI; **29 of 29 installable applications of the sample show a usable first screen on the desktop** (10 of 10 on TUR's kernel); one desktop window per application with `waydroid app launch` in multi-window mode, sized by Android, not by the desktop | Sections 2 and 6 |
| 4 | ARM-only applications | 6.9% of F-Droid; at most 19% of developer-built open-source applications | Waydroid refuses them (`INSTALL_FAILED_NO_MATCHING_ABIS`); ATL as ARM64 under QEMU: 1 of 7 launches in each of two runs, also with the just-in-time compiler off: **killed** (section 5) | **Not supported unless the person supplies a translation layer**, as android-runtime.md says |
| 5 | **Not supported, with the reason** | Applications that need a certified device, phone hardware, or Google's services where neither microG nor the person's opt-in answers | Not in F-Droid by its policy | android-runtime.md, section 11 |

**"Translation first", as for Windows?** Not as a default. Wine is tried
first because it runs most of what is thrown at it; ATL runs one
application in five, Waydroid all 29 of the sample that it can install,
and a failed ATL try costs seconds and leaves nothing on screen. **ATL
adds no coverage**: every application that worked under ATL also worked
under Waydroid. What it adds is integration and idle cost: its window is
the desktop's (resized by the desktop, on the accessibility bus, files
and links through the portals), and nothing runs before the first
application. So the order holds per application, as a preference, where
the catalogue says an application works under ATL; everything else goes
straight to Waydroid. If the owner prefers one Android route to maintain,
rung 2 is the one to defer: nothing is lost in coverage, only
integration for about one application in five.

**How the routes fit the direction the owner has set** (TUR beside the
person's Windows, their Windows inside TUR, everything on one desktop):

- **Android cannot be delegated to the Windows side.** Microsoft ended
  the Windows Subsystem for Android in March 2025 (from Microsoft's
  announcement of 2024; not re-read for this report), so the person's
  Windows offers no Android route of its own. If Android applications are
  to be on the one desktop, TUR runs them.
- **Rungs 1 and 2 are native windows in the plain sense**: one process
  per application, one Wayland window with its own identity, on the
  accessibility bus, using the desktop's file chooser and notifications
  (measured for ATL, section 4), nothing running when the application is
  closed, no root daemon. That is the shape the owner asks for, and it is
  the shape of TUR's Windows route (Wine before the guest). Today it
  covers about a fifth of open-source applications.
- **Rung 3 is windows from one shared Android system**: it runs nearly
  everything that has x86 code, behind a container that boots in about
  half a minute, holds about 0.95 GB while any Android application is
  open, and runs a root daemon (ADR-0006's recorded deviation). In
  multi-window mode each application is its own desktop window with its
  own app id (`waydroid.PACKAGE`), and receives the desktop's pointer,
  clipboard and notifications; but Android, not the desktop, decides the
  window's size and position (measured under sway), and files and links
  stay inside Android.
- **Memory beside the Windows guest.** ADR-0007 indicates 16 GB with the
  guest, to be measured. Measured here: Waydroid idle about 0.95 GB plus
  about 0.1 GB per application; ATL about 0.35 GB per application and
  nothing idle. With the Windows guest running and one or two Android
  applications wanted, ATL is the cheaper company; with many, Waydroid.
- **Nothing here needs a VM.** Every rung runs on the host kernel.

## 10. What productising would take

**Waydroid** (the general route): sections 6.1 and 6.4 are the list.
Beyond ADR-0006's plan it adds: launch through Waydroid's own interface
(`waydroid app launch` in multi-window mode), never `am start`; a sound
server and a notification server in the session before the session
starts; making the desktop's window size reach Android (or accepting
Android-sized windows), measured under KWin first; bridges for files and
links to the desktop's portals; a decision about secure windows being
capturable on the host; the image already presents Google's signature to
microG, so microG as an opt-in is possible without another image; and a
CI job that boots it on TUR's kernel in a KVM guest in twelve minutes
(`waydroid/kvm-guest.sh`), which can become a permanent test.

**ATL** (the catalogue route) could reach a TUR computer in three ways:

| Way | What TUR builds | Sandbox | Cost |
|---|---|---|---|
| **A Flatpak per application, on ATL's base application from Flathub** | Nothing of ATL. TUR's application centre already installs Flatpaks; a catalogue entry names the APK (by hash) and the base application | Flatpak's, per application: the gap ATL itself has | Each application is packaged once (by TUR's catalogue, by the ATL project, or by its author); an arbitrary APK the person brings is not covered unless TUR builds the Flatpak on the computer |
| ATL as packages in TUR's image | ATL, `art_standalone`, `bionic_translation`, wolfSSL with JNI: four packages Fedora does not have, tracked against an upstream without releases (snapshots only) | TUR's own (`tur-runtime`'s bubblewrap profiles, as for Wine) | A build and update pipeline for a second Android runtime; any APK can be tried |
| ATL in a container image fetched on demand (Alpine, as this experiment runs it) | A pinned container image | The container's | Simple to pin; a second C library and GTK stack on disk; desktop integration (portals, accessibility, theme) crosses a container boundary |

For a catalogue route the first way fits: the catalogue already names
applications, a Flatpak gives each one a sandbox, and TUR builds nothing
of ATL. Its cost is one measurement per catalogue entry and release
(this experiment's harness does it in about twenty seconds an
application), and patches upstream for the applications TUR wants.

**Re-hosting** (rung 1): a sandboxed web view with a stand-in for
`cordova.js` and Capacitor's bridge that answers the common plug-ins;
`fdroid/payload.py` has the 15-line beginning.

## 11. Limits

- **Waydroid's windows were seen under sway, not KWin.** TUR's compositor
  may place and size the per-application windows differently; input in
  multi-window mode, typed text with a standard keymap, saving and sharing
  were not tested (section 6.4).
- **Each Waydroid integration check ran once per place.** The clipboard
  failure on Fedora after a click is one try.
- No GPU and no touchscreen in CI: software rendering only. Games and
  anything that needs hardware acceleration are not judged fairly by this
  test bed, in either route; the one game in the sample opened no window
  under ATL within a minute.
- **First screens only**, watched for about ten seconds. An application
  that draws its first screen may fail at the second; none of the
  applications was operated. "Usable" is a person's reading of a
  screenshot against a definition fixed beforehand; the seven "partial"
  windows are listed so that a reader can count differently.
- ATL is measured as Alpine packages it (musl C library, snapshot of
  19 October 2026) in a container, not as a Fedora build or as the
  Flatpak base application from Flathub.
- Sixty applications. With 12 successes the 95% interval for the share is
  about 12% to 32%.
- Memory is PSS of one application at a time; ATL applications running
  together would share libraries. Waydroid's per-application figure is
  Android's own.
- F-Droid is not the Play Store. Everything here is about open-source
  applications; sections 2 and 9.3 say what follows for the rest.
- IzzyOnDroid's index holds one APK per release, so its ARM-only share is
  an upper bound (section 2).
- The web-view share rests on file names in the APK and a reading of
  twenty entries by hand, not on running the applications.

## 12. Licence and legal questions for the owner

1. **ATL is GPL-3.0.** TUR could ship it as a separate program, as it
   ships Waydroid (GPL-3.0); TUR's own code would drive it over its
   command line and D-Bus and never link to it. Fixes would go upstream
   under GPL-3.0. Is that acceptable for a second Android route?
2. **ATL is not in Fedora.** Shipping it means TUR builds and maintains
   three packages (ATL, `art_standalone`, `bionic_translation`), or ships
   it as a Flatpak runtime the way the ATL project does on Flathub. Which?
3. **QEMU user-mode emulation is GPL-2.0 and is in Fedora**
   (`qemu-user`). The ARM route through it was killed (section 5), so
   the question does not arise now.
4. **microG** is Apache-2.0 and answers under Google's package name with
   Google's signature presented by the system. android-runtime.md already
   proposes it as an opt-in download. The legal review should say whether
   offering it is acceptable in the owner's jurisdiction.
5. **Re-hosting** (section 7) runs an application's own web assets
   outside the package its author shipped. For open-source applications
   the licence allows it; for others it is a question for the legal
   review.

## 13. Follow-ups

1. **For the Waydroid runtime (pull request 137)**: section 6.4. Then,
   measured on TUR's own desktop under KWin: per-application windows
   (size, placement, the double caption bar), typed text with a standard
   keymap, input in multi-window mode, the clipboard from Android after a
   click on Fedora, saving and sharing.
2. **Correct android-runtime.md**: section 3.3 (binder on Fedora 45's
   released kernel is the C driver; section 6.3 here); section 12.2
   (signature spoofing for microG: present in the image, measured from an
   application, section 6.1).
3. **Catalogue fields**: `payload: web-bundle | flutter | …`,
   `native_abis`, and per route a measured verdict with the run it came
   from. The first two are readable from an APK's ZIP directory without
   downloading it (`fdroid/payload.py`).
4. **Upstream, small and specific**: `__register_atfork` and
   `__strlcpy_chk` in `bionic_translation`; why `libflutter.so` does not
   link. Each is the first blocker of several applications (section 3).
   ATL's clipboard from the host (the application read nothing).
5. **ATL as shipped on Flathub**: the same sixty applications on the
   Flatpak base application in a Fedora 45 guest, which is how TUR would
   run it, if rung 2 is kept.
6. **A re-hosting runner** for rung 1; the harness's entry-page choice
   (section 7.3).
7. **Waydroid on TUR's kernel as a permanent CI job** (twelve minutes).
8. **The Rust binder driver's messages** (`Failed to allocate buffer`,
   two to four in a session): harmless here; worth a look under load.
9. **The ARM route, only if wanted later**: launches with an empty ART
   cache each time would test whether loading cached compiled code is what
   fails under emulation (section 5). Not proposed now.

## 14. Proposed decision text (the lead assigns the number)

Amends ADR-0006 and ADR-0007's routing table; it does not replace
Waydroid.

> **Android applications: the container by default, the lightest route
> where it is known to work.**
>
> 1. **Waydroid is the general route** for Android applications
>    (ADR-0006, unchanged). TUR shows Android applications only through
>    Waydroid's own interface: multi-window mode
>    (`persist.waydroid.multi_windows=true`) and `waydroid app launch
>    PACKAGE`, one desktop window per application, never Android's
>    `am start`. TUR starts the Android session only when a sound server
>    and a notification server are present in the user's session.
>    TUR does not promise that the desktop sizes or places Android
>    windows until that is measured under its own compositor.
> 2. **Translation before the container, per application, as a
>    preference.** Where the catalogue records that an application and
>    version was measured to reach a usable state under the Android Translation Layer, TUR runs it
>    that way: an ordinary process with a native window, as a Flatpak on
>    ATL's base application. ATL is GPL-3.0 and stays a separate program.
>    An application not so recorded is never sent to ATL.
> 3. **An application that is a web view around its own bundled web
>    assets**, and is recorded as such, is run as a sandboxed web
>    application with no Android runtime.
> 4. **ARM-only applications are not supported** unless the person
>    supplies a translation layer (android-runtime.md, section 6.2). TUR
>    ships none: the open-source route tried (ATL as a 64-bit ARM program
>    under QEMU user-mode emulation) started one application in seven, in
>    each of two runs.
> 5. The catalogue records, per Android application and version: native
>    ABIs, what it is built with, and for each route a verdict measured in
>    CI, with the run it came from. Nothing is claimed that was not
>    measured.
> 6. TUR does not rely on the person's Windows for Android applications.
