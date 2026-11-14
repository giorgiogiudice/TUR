<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-android-1 — Android applications on TUR: translation first, how much needs no ARM translation, and whether the container can be tested in CI (experiment)

> **Status: interim. Nothing has been measured yet.** The hypotheses, kill
> criteria and method below were committed before the first measurement
> (commit `2bfb491`). The harness is written and pushed (pull request 120,
> head `551cedd`); its first CI run was still waiting for a runner when
> this version was written, behind about sixty queued runs of the
> repository. Every result is therefore **NOT RUN**. What is in this
> version and is checked: the state of the upstream projects on
> 14 November 2026, read from their own repositories and package indexes
> (section 1), and the facts and options about Google's services
> (section 8).

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-android-1 (time-boxed research spike "android")
Subsystem:        Android runtime (addendum B, ADR-0006); no TUR service
Build ID:         not applicable: this task builds no image
Source revision:  exp/android (pull request 120), head 551cedd

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
  the owner wants Android applications native and fast, without VMs.

FILES
- Added: .github/workflows/exp-android.yml, tests/experiments/android/**,
  docs/reports/exp-android-1.md
- Modified: none
- Removed: none

BUILD
- Command: none on a development machine. In CI the probe application is
  built with the runner's Android SDK and NDK (probe/build.sh).
- Result: NOT RUN (queued)
- Warnings: none yet

TESTS
- Unit tests: none (experiment)
- Integration tests: workflow exp-android, five jobs: NOT RUN (queued
  since 00:46 UTC on 14 November 2026)
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (no GPU, no touchscreen, software rendering only)
- Performance tests: NOT RUN (start-up time, memory and the emulation
  benchmark are part of the queued jobs)

DIAGNOSTICS
- Log paths: artifacts exp-android-fdroid, exp-android-atl-sample,
  exp-android-atl-random, exp-android-atl-arm64, exp-android-waydroid,
  exp-android-waydroid-f45 of the workflow run
- Diagnostic bundle path: not applicable
- Relevant error IDs: none

RECOVERY
- Rollback procedure: close the pull request; nothing outside
  tests/experiments/android, the workflow and this report is touched
- Recovery tested: not applicable

KNOWN LIMITATIONS
- See section 11.

NEXT SAFE ACTION
- Let the queued run finish, read its artifacts (the screenshots decide
  hypothesis A), commit the lock files it produces, and fill in sections
  2 to 7.
```

## Hypotheses and kill criteria (fixed before measuring)

| | Hypothesis | Kill criterion | Result |
|---|---|---|---|
| A | Translation first, as for Windows: on a sample of thirty real open-source applications, a useful share reaches a working first screen under the Android Translation Layer (ATL), which runs an APK as an ordinary Linux process | Fewer than 3 of the 30 hand-picked applications (10%) reach a usable first screen | NOT RUN |
| B1 | Most open-source Android applications need no ARM translation on an x86-64 computer: at least 80% of F-Droid's applications carry x86-64 native code or no native code | Under 50%: the container route without translation would leave out most applications, and the design would have to say so | NOT RUN |
| B2 | An open-source ARM-on-x86 route exists that TUR could ship legally, and it runs an application with native ARM code at a usable speed | No such route starts an application at all; or native ARM code runs more than 20 times slower than x86-64 code on the same work; or a first screen takes more than 60 seconds | NOT RUN |
| C | Waydroid can be started in GitHub Actions, on the runner's kernel or in a KVM guest running Fedora 45's kernel, as far as one application's first screen in a screenshot | Android does not finish booting within ten minutes in either place; then the report says exactly what stops it | NOT RUN |
| D | Some Android applications are a web view around bundled web assets, and such an application can be run as a web application with no Android at all: at least 3% of F-Droid's applications are of this kind, and one of them shows its first screen in a plain browser | None of up to four web bundles tried shows its first screen without the native bridge | NOT RUN |

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
  Fedora's own Waydroid package and SELinux enforcing (ten applications).
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

NOT RUN (job `fdroid`, queued).

For applications outside F-Droid: what can and cannot be known without
downloading them is in section 9.3.

## 3. Hypothesis A: applications under ATL

NOT RUN (jobs `atl (sample)` and `atl (random)`, queued).

## 4. Integration facts (probe application)

NOT RUN.

## 5. Hypothesis B2: ARM applications through QEMU user-mode emulation

NOT RUN (job `atl-arm64`, queued).

## 6. Hypothesis C: Waydroid in GitHub Actions

NOT RUN (jobs `waydroid` and `waydroid-f45`, queued).

## 7. Hypothesis D: what applications are made of, and re-hosting a web bundle

NOT RUN (job `fdroid`, queued).

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

NOT RUN: the recommendation depends on sections 2 to 7. The questions the
numbers must answer, fixed now:

1. Is ATL good enough to be the **first** route for a defined class of
   applications (the way Wine is tried before a Windows guest), with the
   container as the second route? The measure is hypothesis A on the
   hand-picked sample, and the random sample beside it.
2. If it is not, is it good enough as a **per-application** route: a
   catalogue entry that says "this application, this version, runs under
   ATL", the way Flathub's NewPipe port does?
3. Which route takes ARM-only applications, if any does (section 5)?
4. Which applications need no Android at all (section 7)?

### 9.3 Applications outside F-Droid: what can be known without downloading them

- **Not knowable from Google Play.** The store's listing does not state
  native ABIs, and Play delivers device-specific splits of an App Bundle:
  what an x86-64 device would receive cannot be seen without asking as
  that device.
- **Knowable from a package in hand.** The ZIP directory of an APK lists
  `lib/<abi>/`; TUR's inspector reads it before installing
  (android-runtime.md, section 6.2). For a remote APK two HTTP range
  requests are enough (section 7 does this for 600 packages).
- **Knowable from other open indexes.** IzzyOnDroid publishes an index in
  F-Droid's format for developer-built APKs (section 2).
- **Inferable from the framework.** Flutter and React Native build
  x86-64 libraries by default; a Unity game is ARM-only unless its
  developer ticks x86-64. Section 7's classification gives the share of
  each framework in F-Droid; for the Play Store it is an inference, not a
  measurement.

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

- Nothing was measured yet (see the status at the top).
- No GPU and no touchscreen in CI: software rendering only. Games and
  anything that needs hardware acceleration are not judged fairly by this
  test bed, in either route.
- First screens only. An application that draws its first screen may fail
  at the second.
- ATL is measured as Alpine packages it (musl C library, snapshot of
  19 October 2026), not as a Fedora build, which does not exist.
- F-Droid is not the Play Store. Everything here is about open-source
  applications; section 9.3 says what follows for the rest.

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

Written once results exist.
