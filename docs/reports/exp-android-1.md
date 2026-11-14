<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-android-1 — Android applications on TUR: translation first, how much needs no ARM translation, and whether the container can be tested in CI (experiment)

> **Status: pre-registration.** This version was committed before anything
> was measured. It states the hypotheses, the kill criteria and the method.
> Results replace the "NOT RUN" entries as CI produces them.

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
  measuring for spread across kinds (`tests/experiments/android/sample.txt`),
  and thirty drawn at random with a fixed seed from the whole index, both
  pinned by version and SHA-256 in `tests/experiments/android/locks/`.
  The hand-picked sample decides hypothesis A; the random one is reported
  beside it.
- **A.** Alpine edge packages ATL with its runtime. Each application is
  started in a headless Wayland compositor (sway) in an Alpine container on
  GitHub's runner: time to the first window, time until the picture stops
  changing, memory (PSS), a screenshot, the accessibility tree, a resize.
- **B1.** F-Droid's index records the native ABIs of every APK. Counted for
  the newest release of every application, by category and by year.
- **B2.** ATL and its whole runtime as 64-bit ARM programs under QEMU's
  user-mode emulation on the x86-64 runner, running the ARM builds of four
  applications and a probe application that does the same integer and
  floating-point work in Java and in native code on both architectures.
- **C.** Waydroid with its VANILLA image (no Google software), software
  rendering, a headless compositor: on the runner's kernel, and in a KVM
  guest running Fedora 45 (Rust binder driver, SELinux enforcing, Fedora's
  Waydroid package). The same thirty applications.
- **D.** The ZIP directory of 600 random F-Droid APKs, read by HTTP range
  requests, says what each is built with. Up to four web bundles are
  unpacked and opened in a headless browser, with and without a stand-in
  for the framework's native bridge.
- **Integration facts.** A probe application built in CI
  (`tests/experiments/android/probe`) logs what happens to the clipboard,
  a notification, pointer and keyboard input, the back gesture, a resize,
  and requests to open, save, view and share.

Nothing proprietary is fetched: no Google services, no Android image that
contains them, no ARM translation library.
