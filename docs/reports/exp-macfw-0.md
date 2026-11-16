<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-macfw-0 — real Macs as the oracle for Darling's AppKit (experiment plan)

> **Status: planned, NOT RUN.** Branch `exp/macfw`. This document registers
> the hypotheses and kill criteria before any measurement, records the facts
> that set the odds (read 16 November 2026), and describes the harness that
> is in the branch. GitHub Actions is disabled for the owner's account; the
> workflow has only a `workflow_dispatch` trigger and nothing will run until
> the lead starts it. Every number that has to come from a run is marked
> **NOT RUN**.

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-macfw-0 (research spike `macfw`: plan and skeleton)
Subsystem:        macOS applications (priority 8: optional, experimental);
                  no TUR service, image or installer file is changed
Build ID:         not applicable: nothing is built
Source revision:  exp/macfw (this pull request)

SUMMARY
- What changed: a plan and a harness. Probe programs (Objective-C and
  Swift) that call public Foundation, AppKit and SwiftUI API and print what
  they observe; a ten-rung ladder of Mac-only applications built from
  source; scripts that build both on GitHub's macOS runner (the oracle),
  run the same binaries under Darling on Linux (the candidate), compare,
  score the ladder and keep a scoreboard; an empty patch queue to Darling
  under Darling's licences; a dispatch-only workflow.
- Why: to test whether an AI-driven differential loop, with real Macs as
  the black-box oracle, can extend Darling far enough for Mac-only GUI
  applications to show usable windows on Linux (owner's approval,
  16 November 2026).

FILES
- Added: tests/experiments/macfw/** (probes/, ladder/, harness/, ci/,
  darling-patches/ under GPL-3.0-or-later, census/, triage.tsv,
  scoreboard.json, README.md), .github/workflows/exp-macfw.yml, docs/reports/exp-macfw-0.md
- Modified: docs/frontier.md (one row, F9, planned)
- Removed: none

BUILD
- Command: workflow exp-macfw (dispatch only); nothing was compiled on a
  development machine
- Result: NOT RUN
- Warnings: NOT RUN

TESTS
- Unit tests: diff.py and scoreboard.py on hand-made results, py_compile of
  every Python file, `bash -n` of every script, JSON files parsed, workflow
  YAML parsed, the census script run on the cask metadata (Python and jq
  only, on the development machine)
- Integration tests: NOT RUN (the workflow has never run)
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (not applicable)
- Performance tests: NOT RUN

DIAGNOSTICS
- Log paths: artifacts macfw-oracle, macfw-darling-*, macfw-candidate-*,
  macfw-report of a dispatched run (none yet)
- Diagnostic bundle path: not applicable
- Relevant error IDs: none

RECOVERY
- Rollback procedure: delete tests/experiments/macfw, the workflow, this
  report and the frontier row; nothing else refers to them
- Recovery tested: not applicable

KNOWN LIMITATIONS
- Section 10.

NEXT SAFE ACTION
- The lead dispatches cycle c00 once (section 9): about 30 macOS runner
  minutes and an hour of Linux. Then the owner decides whether to run fix
  cycles, using the c00 numbers and section 9.
```

## The answer so far

**Worth one baseline run; whether it is worth the full loop depends on that
run.** Three facts found today change the picture the task was written
with:

1. **Darling is moving very fast, with AI help, and its AppKit is the centre
   of it.** In the first half of November 2026 the main repository received
   1,427 commits and 371 merged pull requests, and darling-cocotron (its
   AppKit, CoreGraphics and Onyx2D) 1,723 commits; in all of January to
   October 2026 those numbers were 88 and 17. Native arm64 Mach-O on arm64
   Linux landed on 6 November. Apple's TextEdit sample now builds and runs
   inside Darling (#2066, 13 November), and the lead maintainer writes AppKit
   tests "from black-box observations through the public API on macOS
   27.0.1 (arm64)" (#1883). Darling's own CI has no Mac; that observation is
   done by hand.
2. **Darling accepts AI-assisted contributions, unlike Wine,** under an
   `AGENTS.md` (added 4 November 2026) that requires a human to own and
   explain every line and to answer reviewers personally. Its preferred
   evidence for undocumented behaviour is exactly what TUR's oracle
   produces: "Black-box observation of the original through its public API,
   captured as a reproducible behavioral test (inputs, outputs, error codes,
   OS version)."
3. **Swift and SwiftUI applications built with today's Xcode do not run in
   Darling,** by the lead maintainer's own account (#2059, 13 November):
   macOS 26 exports about 19,600 SwiftUI Swift symbols and Darling none;
   about 5,500 AppKit Swift symbols against Darling's 241; his plan puts
   "SwiftUI last". No SwiftUI reimplementation aims at Apple's binary
   interface.

So the hypothesis is plausible for Objective-C AppKit applications, long
odds for Swift-AppKit ones, and close to nil for SwiftUI within any budget
TUR should spend. And the loop's distinctive contribution is probably not
its patches (Darling is producing patches at a rate TUR cannot match) but
its **oracle**: an automated, repeatable macOS observation that Darling's
process already accepts as evidence and does not have. The kill criteria
below are written so the measurement separates TUR's contribution from
Darling's own progress (same pinned commit, with and without TUR's queue).

My estimate, before any measurement (section 9.2): rung 2 reached by the
end of the budget, from any cause, about 70%; reached *because of* TUR's
queue, about 35%. Rung 4: about 30% from any cause, about 15% because of
TUR's queue. SwiftUI rungs: under 5%.

## 1. Hypotheses and kill criteria (registered 16 November 2026, before any measurement)

Definitions used below:

- A **cycle** is one round of the loop: read the differences, write patches
  and probes, one dispatch with `queue: true`, read the result. At most one
  cycle per day.
- A rung is **reached** when its application scores 4 in
  `harness/ladder.py` (section 5.4) on x86-64 Linux, X11, in the cycle's
  run, *and* a person has looked at the contact sheet beside macOS's
  screenshot and recorded in `scoreboard.json` that it is a usable window.
- A rung is reached **because of TUR's queue** when it is reached with the
  queue and not reached by upstream Darling at the same pinned commit in the
  same run.
- **Fix yield** is the number of probe differences that a queue patch names
  as its target and that are closed and verified (the check equals macOS
  afterwards; no other check goes from same to different), divided by the
  number of targeted differences.

| # | Hypothesis | Measure | Supported if | Killed if |
|---|---|---|---|---|
| H1 | **The loop extends Darling's AppKit far enough for real Mac-only applications to show usable windows.** | Ladder (section 4) with and without TUR's queue; scoreboard | Rung 4 reached because of TUR's queue, with a fix yield of at least 25%, within the budget | **K1:** after 12 cycles or 6 weeks from the first cycle, whichever comes first, rung 2 is not reached with TUR's queue. **K2:** fix yield under 25% at the end of the budget (the loop is not what moves the rungs, even if they move) |
| H1-pre | (Pre-emption, not a hypothesis of the loop.) | Cycle c00 | | **K0:** upstream Darling at c00 already reaches rung 4: the hypothesis is moot ("superseded"); the experiment continues only as H4 |
| H2 | **SwiftUI.** The same loop gets TUR's own SwiftUI application (rung 8) to a usable window. | Rung 8 | Rung 8 reached because of TUR's queue | Expected negative. Not pursued (no patch targets SwiftUI) unless rung 6 is reached by cycle 6. Killed at the end of the budget if not reached |
| H3 | **arm64.** The arm64 slices on arm64 Linux reach the same rung as x86-64. | `arm64: true` runs | Same highest rung on both | Rung 1 not reached on arm64 at the end: arm64 deferred (no effect on H1) |
| H4 | **The oracle is worth giving to Darling** whatever happens to H1. | Causes in `triage.tsv` | At least 10 causes that are new (no Darling issue, pull request or test records them) and relevant (a ladder application or a common API pattern hits them) | Fewer than one new, relevant cause in ten differences (windiff's threshold) |

Budget, registered with the criteria: 12 cycles or 6 weeks; **at most 400
macOS runner minutes in total** (section 6); one person-equivalent of agent
work per cycle. The constants are in `harness/scoreboard.py`, which
evaluates K1, K2 and K0 from `scoreboard.json`; changing them after c00
means registering a new hypothesis here, not editing the old one.

What would make me doubt a result: a rung that scores 4 only on one of two
runs (count it as not reached); a probe whose oracle value changes between
macOS runner images (the image version is in `env.txt`; the differ ignores
checks that differ between the oracle's two architectures, and a check that
changes between oracle runs is re-triaged as `artifact`); a fix that closes
a probe but no ladder rung moves (that counts for K2's yield, not for H1).

## 2. Darling today (read 16 November 2026, from its own repositories)

### 2.1 What it is and how it runs a Mac program

| Fact | Source |
|---|---|
| Licence GPL-3.0-or-later (file headers: "either version 3 of the License, or (at your option) any later version"); "Individual submodules may be licensed differently" | `darlinghq/darling` LICENSE, README, `src/startup/mldr/mldr.c` |
| A Mach-O loader (`mldr`) and a userspace kernel server (`darlingserver`) that "implements macOS's Mach IPC, POSIX, and Darwin syscall interfaces on top of Linux". The old kernel module is "now-defunct" | README; darlingserver docs |
| Low layers (XNU parts, libSystem, Security) are Apple's open-source releases; higher frameworks are reimplementations: "Foundation, AppKit (based on Cocotron), CoreAudio, and CoreFoundation" | README |
| AppKit, CoreGraphics, Onyx2D: `darling-cocotron`, MIT, a fork of Cocotron (cjwl/cocotron) | GitHub API, file headers |
| Foundation: `darling-foundation`, LGPL-2.1; its README says the GNUstep base was "replaced with an entirely different base project (Appportable Cocotron, and some others)" | darling-foundation README |
| CoreFoundation: `darling-corefoundation`, a fork of GNUstep's libs-corebase | GitHub API |
| Prefixes like Wine's (`~/.darling`, overlayfs); `darling shell`; `.pkg` and `.dmg` installation | README |
| X11 and Wayland backends for AppKit (`DARLING_APPKIT_BACKEND`); "an initial Metal backend powered by Vulkan translation" | darling-skills `darling-guest-testing`; README |
| **arm64:** "On an aarch64 host, `darling shell` now starts and runs native arm64 Mach-O binaries" (bash, coreutils, Perl, Ruby, Python); merged 6 November 2026. GUI on arm64 is not described anywhere | PR #1818; also #1903, #2094, #2106 |
| x86_64 and i386 Mach-O on x86-64 hosts; universal binaries are sliced by mldr (#2069) | PRs |
| Swift: Darling ships the "unmodified binaries from `usr/lib/swift/macosx`" of swift.org's 6.3.3 macOS release (the runtime), plus overlays built from source. "Swift apps built with a current Xcode don't run on Darling. The core runtime is ready, but these apps use Swift APIs that, on current macOS, live inside the system frameworks themselves" | darling-swift README; issue #2059 |
| SwiftUI: none. #2059's table: SwiftUI "about 19,600 / none", Combine "about 2,100 / none", AppKit "about 5,500 / 241"; suggested order ends "6. SwiftUI last". An earlier stub PR (#1139) was closed unmerged in 2022 | issue #2059 |

### 2.2 Activity

| Repository | Commits 2025 | Jan–Oct 2026 | 1–16 Nov 2026 |
|---|---|---|---|
| darling (main) | 77 | 88 | 1,427 (371 merged pull requests) |
| darling-cocotron (AppKit) | 64 | 17 | 1,723 |
| darling-foundation | 22 | 5 | 431 |
| darling-corefoundation | 2 | 7 | 204 |
| darling-swift | 0 | 0 | 197 |
| darling-metal | 5 | 7 | 92 |

(Commit search and the commits API, default branches.) Most November
commits are by the lead maintainer, Luboš Doležel, with a few other
contributors; the pull-request descriptions carry testing and provenance
sections in the form Darling's AGENTS.md prescribes. Darling's CI builds
Debian packages for amd64 and arm64 on GitHub's Ubuntu runners (about 15
minutes with a warm ccache) and keeps them 90 days; it has no macOS job.

### 2.3 What its GUI does today

- **TextEdit** (Apple's sample code) is built into Darling and was tested on
  Xvfb (#2066, merged 13 November). Not used by this experiment: it is Apple
  source (section 4).
- **VLC 3.0.24**: its GUI plugin fails to load, with missing AppKit and
  CoreServices symbols (#2055, open, 12 November).
- **AppDB** (`darlinghq/darling-appdb-database`) has two entries: Angry Birds
  4.0.0 rated *gold* (15 November; needs `vm.mmap_min_addr=4096`), and
  pearCalc rated *garbage* ("Doesn't start").
- Darling's documentation pages on GUI applications are stale (2022 to
  January 2024): "experimental support for graphical applications written
  using Cocoa"; one known-working GUI app, EdenMath.
- Its test tree has AppKit window and view tests that encode macOS 27.0.1
  behaviour observed by hand (`tests/src/appkit_view_window_macos_defaults.m`,
  37 checks, #1883), and Swift type-check tests of AppKit's Swift names.

Inference: Objective-C AppKit applications that use mainstream controls are
at or near the edge of what Darling runs this month; nobody has published a
systematic measurement of how near.

### 2.4 Does Darling accept AI-assisted contributions? Yes, with a human owner

From `AGENTS.md` (added 4 November 2026 in #1811, answering issue #1806,
"Please document a clear and official AI policy"; last changed 14 November):

> "This document tells an AI agent how to behave *if* you use one. It is not
> permission to submit code you do not understand. You are responsible for
> every line in a PR you open, AI-assisted or not: you must be able to
> explain what it does, why it is correct, and why it follows the rules
> below, in your own words."

> "When a maintainer asks a question on a PR or issue, we expect a reply
> from you, the human contributor, not from your AI assistant pasted
> verbatim, and not from the AI replying on your behalf."

Acceptable evidence for undocumented behaviour, strongest first, includes
"Black-box observation of the original through its public API, captured as
a reproducible behavioral test (inputs, outputs, error codes, OS version)".
Never acceptable: "Disassembly, decompilation, or instruction-level
inspection of Apple binaries, and leaked or confidential Apple material."
Undocumented behaviour is implemented as a marked `QUIRK:` with its evidence
and a test; application-specific hacks are forbidden; every pull request
carries a provenance note. The lead maintainer, in #1811: "in my
experience, AI-generated code quality is much higher with frontier models
than what average people contribute." Darling also publishes skills for AI
agents (`darlinghq/darling-skills`: building, guest testing on Xvfb and
headless sway, debugging, compatibility scans).

Consequences for TUR:

- Unlike Wine (exp-windiff-1, section 8.2), the upstream route for fixes is
  **open**, but only through a named TUR person who understands each patch
  and answers for it. Without that person the queue is evidence, not
  contributions. That is a decision for the owner.
- TUR's rules and Darling's agree: both forbid Apple code of any kind and
  both rely on black-box observation. The patch-queue README makes Darling's
  AGENTS.md binding for every patch.

### 2.5 The other routes

| Project | State (16 November 2026) | Use for running Mac binaries |
|---|---|---|
| GNUstep (libs-base 1.31.1, libs-gui/back 0.32.0, all February 2025; 711 and 1,459 commits in 2026) | Source compatibility with Cocoa, LGPL libraries; Wayland backend alpha; Swift interop not supported (libobjc2 #306) | None: no Mach-O loader. Useful only if an application is rebuilt from source |
| OpenSwiftUI (MIT, 0.25.0, 1 November 2026) | "A SwiftUI source compatibility framework", module `OpenSwiftUI`; on Linux only a stdout renderer ("Linux UI framework integration is not available yet"). An open PR mentions "Darling-specific changes" held back in a fork | No binary compatibility with Apple's `SwiftUI` symbols |
| Tokamak | Archived; last release November 2022 | None |
| SwiftCrossUI (MIT, 0.10.0, September 2026) | Real windows on Linux through GTK; "does not attempt to replicate SwiftUI's API perfectly" | Source only |
| Adwaita for Swift, SkipUI | SwiftUI-like APIs for GNOME, and for Android through Compose | Source only |
| Swift on Linux (6.4.2, 30 October 2026) | swift-corelibs-foundation "re-exports" swift-foundation's modules and "builds for non-Darwin platforms only"; it is for "platforms where there is no Objective-C runtime" and "the compatibility is best-effort only" | Source only; no Objective-C interop |

So for an unmodified Mac binary, Darling is the only route, and for a
SwiftUI binary there is none.

### 2.6 Darling on Fedora

Darling publishes only Debian packages (a zip of `.deb` files per release;
latest release v0.1.20260608, 9 June 2026; daily packages as CI artifacts).
The repository's `rpm/` directory builds an RPM in Docker, documented as
defaulting to Fedora 30 and "isn't quite ready for official releasing"; the
build instructions (updated 15 November 2026) list dependencies for
"Fedora 44, RHEL 9". No COPR, no Fedora package. It no longer needs a kernel
module. For TUR (Fedora 45 Kinoite, image-based) Darling would have to be
packaged by TUR and installed as a layer or a container; the experiment runs
on Ubuntu runners with Darling's own packages, because what is measured is
Darling, not the distribution (section 10).

## 3. Census: x86-64 slices, arm64-only applications and Apple's timeline

### 3.1 Homebrew Cask metadata

Source: `https://formulae.brew.sh/api/cask.json`, downloaded 16 November 2026
(7,831 casks; SHA-256 `574257f5…d089d`). No application was downloaded.
Deprecated and disabled casks are excluded; "application" means a cask that
installs an `.app` (3,495 casks; the rest are fonts, drivers, command-line
tools and `.pkg` installers). Classification: `tests/experiments/macfw/census/cask-arch.jq`:

| Class | How it is recognised | Applications | Share |
|---|---|---|---|
| Separate downloads per architecture | the Intel variations' URL differs from the default (arm64) URL | 976 | 27.9% |
| One download for both | same URL for both, no architecture requirement, no Rosetta caveat | 2,082 | 59.6% |
| x86-64 only | the `requires_rosetta` caveat, or `depends_on arch: :intel` | 280 | 8.0% |
| arm64 only | `depends_on arch: :arm64` | 157 | 4.5% |

All casks (6,900 including non-applications): 1,216 separate, 5,192 one
download, 321 x86-64 only, 171 arm64 only.

Reading: at least 36% of Mac applications in Homebrew publish an x86-64
build for certain (separate download or Intel-only), and most of the 60%
with one download are universal binaries (inference: Homebrew marks
arm64-only casks with `depends_on arch: :arm64` because Intel Macs would
otherwise install them, so an unmarked single download carries an x86-64
slice; checking would mean downloading the binaries, which this task does
not do). Only about 4.5% are marked arm64-only, typically young projects
(Homebrew's list starts with names such as `airbuild`, `arcbox`, `battery`).
Every application on this experiment's ladder that is in Homebrew
(Itsycal, Hex Fiend, Hidden Bar, TomatoBar, Table Tool, Maccy) ships one
download, without an architecture requirement.

### 3.2 Apple's and the ecosystem's timeline

| Date | Event | Source |
|---|---|---|
| June 2025 (WWDC25) | macOS 26 Tahoe is the last macOS for Intel Macs; full Rosetta 2 through macOS 27; from macOS 28 only a limited Rosetta for older games | Apple's Platforms State of the Union, as reported by [Tom's Hardware](https://www.tomshardware.com/software/macos/macos-tahoe-is-the-last-release-for-intel-macs-apple-silicon-exclusivity-will-mark-end-of-hackintoshes), [MacRumors](https://www.macrumors.com/2025/06/10/apple-to-phase-out-rosetta-2/), [AppleInsider](https://appleinsider.com/articles/25/06/10/macos-27-will-be-the-last-operating-system-to-fully-support-rosetta-2) |
| February 2026 | macOS 26.4 warns about apps that will not work after Rosetta | [MacRumors](https://www.macrumors.com/2026/02/16/macos-tahoe-26-4-rosetta-2-warnings) |
| September 2026 | Homebrew 7.0.0 moves Intel macOS to Tier 3 and stops new Intel bottles; end of Intel support 1 September 2027 (the 5.0.0 release notes set this schedule) | [Homebrew 5.0.0](https://brew.sh/2025/11/12/homebrew-5.0.0), [Support Tiers](https://docs.brew.sh/Support-Tiers), secondary coverage of 7.0.0 |
| November 2026 | GitHub's macOS runners: `macos-26` (arm64, macOS 26.7.2, Xcode 26.6 default) and `macos-26-intel` (x86-64); macOS 14 images retired 2 November; an `xcode-27` preview image | [actions/runner-images README](https://github.com/actions/runner-images) |
| Fall 2027 | GitHub: x86-64 macOS runners end when their image retires | [GitHub changelog, 19 September 2025](https://github.blog/changelog/2025-09-19-github-actions-macos-13-runner-image-is-closing-down/) |

### 3.3 What this means for the arm64 question

- **For this experiment, nothing urgent:** the ladder is built from source
  by TUR, as universal binaries, so the x86-64 slice exists whatever its
  publishers do; Xcode 26 still builds x86-64.
- **For TUR's users, the x86-64 window is closing.** Most TUR machines are
  x86-64 PCs. Darling there runs x86-64 slices only; there is no maintained
  arm64-to-x86-64 translator in Darling's plan. Universal binaries will
  persist while developers support Intel Macs (macOS 26 gets security
  updates for about three years, so to roughly 2028), but from macOS 28
  (2027) Rosetta's general version is gone and the incentive to ship x86-64
  with it. Expect the arm64-only share (4.5% today) to grow from 2027.
  On arm64 Linux machines Darling now runs arm64 Mach-O natively, which is
  why H3 exists.

## 4. The ladder

Applications in order of expected difficulty for Darling. Every one is
built from source on the macOS runner from the pinned commit; the binaries
the experiment runs are those builds, never downloaded Apple or vendor
binaries. Rungs 1, 2, 6 and 8 are TUR's own small programs
(`ladder/own/`), because no open-source Mac-only application is that small
and that isolating. Full detail, including each rung's usable-window test,
is in `tests/experiments/macfw/ladder/ladder.json`.

| Rung | Application | Licence | Size | Interface | Framework surface | What "usable" means |
|---|---|---|---|---|---|---|
| 1 | TUR `rung1-hello` | TUR | 100 lines Objective-C | AppKit, code only | NSApplication and delegate, main menu, NSWindow, NSStackView, NSTextField, NSButton | Window shown and key; typing then Return prints `macfw-click 1 text=abc` |
| 2 | TUR `rung2-form` | TUR | 195 lines Objective-C | AppKit, code only | NSSplitView, NSTableView with data source in NSScrollView, NSTextView with undo, NSPopUpButton, NSSlider, NSToolbar, NSAlert sheet, Auto Layout | Window with 8 rows; a click selects a row; typing into the text view changes it |
| 3 | [jakob/TableTool](https://github.com/jakob/TableTool) `066e66d8` | MIT | 12 `.m`, 5 XIB | AppKit + XIB | NSDocument (CSV), nib loading, NSTableView, toolbar, popovers | Opens a generated CSV; editing a cell changes the window |
| 4 | [sfsam/Itsycal](https://github.com/sfsam/Itsycal) tag 0.15.15, `95395138` | MIT | 36 `.m` | AppKit, code-built views + MainMenu.xib | NSStatusItem, popover panel, custom NSView drawing, EventKit; vendored MASShortcut and Sparkle frameworks (not Apple) | Clicking the status item shows the month grid |
| 5 | [HexFiend/HexFiend](https://github.com/HexFiend/HexFiend) `32cb2ef7` | BSD-2-Clause | 112 `.m`, 18 XIB, 6 `.swift` | AppKit + XIB, own framework | NSDocument, custom text and hex views, NSOutlineView, Tcl templates; Sparkle and swift-argument-parser through SwiftPM | Opens a generated binary file; Page Down scrolls |
| 6 | TUR `rung6-swift` | TUR | 66 lines Swift | AppKit, code only | Rung 1 through AppKit's and Foundation's Swift API (#2059) | As rung 1 |
| 7 | [dwarvesf/hidden](https://github.com/dwarvesf/hidden) `a5eab837` | MIT | about 75 Swift files | AppKit + storyboard | NSStatusItem, storyboard loading, preferences window; HotKey, Sparkle | First-launch preferences window shown |
| 8 | TUR `rung8-swiftui` | TUR | 59 lines Swift | SwiftUI `App` | WindowGroup, Text, TextField, Button, List with selection, @State | As rung 1 |
| 9 | [ivoronin/TomatoBar](https://github.com/ivoronin/TomatoBar) `90a77d66` | MIT | 7 Swift files | SwiftUI in NSPopover | NSStatusItem, NSHostingController; KeyboardShortcuts, LaunchAtLogin, SwiftState | Clicking the status item shows the popover |
| 10 | [jordansinger/SwiftUI-Kit](https://github.com/jordansinger/SwiftUI-Kit) `1211aa6b` | MIT | 18 Swift files | SwiftUI `App` | NavigationView sidebar, List, every standard control, SF Symbols, MapKit `Map` | Selecting a sidebar entry changes the page |

None of these projects publishes a Linux or Windows build. Rejected:
Apple's TextEdit sample (Apple source; Darling already carries it, and it
would make a convenient positive control, but this experiment never copies
or builds Apple source), CotEditor (645 Swift files, about 30 SwiftPM
packages: a stretch rung, not a step), Maccy (swift-syntax macros,
SwiftData), Latest (ships Apple-private framework headers), Vienna (private
WebKit header), KeyCastr (needs Input Monitoring), and window managers,
system monitors and system extensions (Rectangle, AltTab, Ice, Stats,
MonitorControl, LuLu), which test permissions and private API rather than
AppKit.

The kill criterion's rung 2 is TUR's own Objective-C form. That is
deliberate: it tests whether the loop can carry Darling's AppKit through the
controls almost every application's first window uses, without depending on
Swift (#2059) or on nib decoding (rung 3).

## 5. The harness (in the branch; not yet run)

### 5.1 Probes

Small programs written by TUR from Apple's public documentation. Each is
one Mach-O with several groups; `probe --run GROUP` prints one JSON line per
observation, `{"id":"win.show.is_key","k":"exact","v":"YES"}`, then a
`done` line. Values are written so that two systems that behave the same
print the same bytes (one decimal place, public class names, YES/NO, orders
as lists; never addresses, times or process ids). Each observation has a
kind: `exact`; `num:T` (within T); `metric` (depends on fonts, theme or
screen: reported, compared within 10%, never counted as a difference);
`env` (describes the machine). An Objective-C exception inside a check is
caught and becomes that check's value, so a missing method costs one
observation. Most checks are font-independent by construction (fixed
frames, constraints, colours the probe fills itself).

| Probe | Groups | About | Covers |
|---|---|---|---|
| `fnd` (Foundation) | string, number, date, plist, url, files, defaults, bundle | 70 | formatting with fixed locales and time zones, property lists and JSON byte for byte, URL parsing, file-manager errors, user defaults precedence |
| `win` (NSApplication, NSWindow) | app, styles, show, two_windows, screen | 150 | defaults for five style masks, frame/content arithmetic, the order of delegate calls when a window is shown, resized, moved, closed; key and main window hand-over |
| `view` (NSView) | geometry, hierarchy, autoresize, layout, stack, draw | 60 | coordinate conversion, bounds scaling, hit testing, the view-hierarchy callbacks, autoresizing masks, Auto Layout with priorities and multipliers, NSStackView distributions, pixels drawn into a bitmap (flipped and not) |
| `ctl` (controls, menus) | button, popup, textfield, misc, menu | 50 | button state machines, target/action, pop-up duplicates and selection, text field value conversion and formatters, slider ticks, segmented controls, menu validation and key equivalents |
| `tbl` (tables, text) | table, outline, text | 40 | row geometry with explicit sizes, selection and its notifications, outline expansion and levels, NSTextStorage edits and attribute runs, NSTextView insertion, undo |
| `evt` (events) | chain, keys, mouse, actions, queue | 25 | first-responder negotiation, the responder chain, where synthesized key and mouse events land, `sendAction:to:from:` falling through to the application delegate, posted events and timers |
| `swak` (Swift AppKit) | load, window, observation | 15 | whether a Swift program built by today's Xcode loads at all (#2059), Swift key-path KVO, notifications |
| `sui` (SwiftUI) | layout, render, state | 15 | font-free layout arithmetic, ImageRenderer pixels, ObservableObject updates |

About 430 observations in 38 groups (the exact number is what the first
run reports: **NOT RUN**). Probes are built once, on the macOS runner, as
universal binaries (`ci/build-probes-macos.sh`); the identical bytes run on
both sides. They could also be compiled on Linux against Darling's own SDK
(Darling's `guest-cc.sh`), but Darling's own tests note that "the SDK's
AppKit headers do not build with this cross compiler", and only binaries
from Apple's toolchain test what real applications are.

### 5.2 Oracle and candidate

| | Oracle | Candidate |
|---|---|---|
| Machine | GitHub's `macos-26` runner (Apple silicon; macOS 26.7.2, Xcode 26.6 on the 2 November image) | GitHub's `ubuntu-24.04` (x86-64), optionally `ubuntu-24.04-arm` |
| Programs | builds probes, TUR's rungs and the ladder (`xcodebuild`, unsigned, `ARCHS="arm64 x86_64"`, then ad-hoc signed); uploads them | downloads the oracle's artifact; runs the same files |
| Runs | probes natively (arm64) and, if the runner has Rosetta, the x86-64 slice under Rosetta; the ladder, with window lists from `mfwin` (bounds only, no permission) and screenshots where Screen Recording is permitted | Darling at the pinned commit, upstream and with TUR's queue, on Xvfb `:99` with xfwm4 (as Darling's own GUI tests run); probes through `darling shell`; ladder with `wmctrl`, ImageMagick and `xdotool` |
| Compared against | — | the oracle's x86-64 results when present (same slice), else arm64; checks that differ between the oracle's two architectures are set aside |

The oracle does not inject input on macOS (that needs the Accessibility
permission, which a hosted runner is not known to grant); for TUR's own
rungs it confirms the window and the `macfw-ready` line, for the others the
window and, where permitted, a screenshot for the contact sheet.

### 5.3 The differ

`harness/diff.py` gives every oracle check one class: same, different,
unimplemented (the candidate raised where macOS did not), missing, group not
run to the end (crash, timeout, load failure with dyld's message, no `done`
line), metric, ignored. With `--before` (the same Darling without TUR's
queue, same commit, same run) it lists the checks the queue closed and those
it broke: the per-patch evidence for K2. `triage.tsv` groups differences
into causes with a status (open, fixed, known to Darling, irrelevant,
artifact), as windiff does; H4 counts causes, not checks.

### 5.4 Ladder scoring

`harness/ladder.py`, 0 to 4: 0 does not start (dyld or a crash before a
window); 1 runs, no window of the rung's size in 45 s; 2 a blank window
(fewer than 16 colours); 3 content but the interaction fails or the process
dies before 60 s; 4 usable (content, interaction as specified in
`ladder.json`, alive at 60 s). A 4 counts only after a person has looked at
the contact sheet; the scoreboard has a field for that.

### 5.5 The fix loop

An agent (TUR's research engineer) reads `macfw-report`, triages, and for
the causes behind the lowest failing rung writes, per cause: a probe that
isolates it, a patch to the Darling component where macOS has the behaviour
(AppKit in darling-cocotron, Foundation in darling-foundation, and so on),
and a test in Darling's style. Patches go into
`tests/experiments/macfw/darling-patches/` (under the licences of the files
they change, with a header naming the reason, the probe evidence and the
provenance) and `series`. Nothing is compiled on the development machine;
the lead dispatches the next cycle with `queue: true` and `oracle_run` set
to the last valid oracle (new probes need a new oracle run). The score job
appends the cycle to the scoreboard.

The loop follows Darling's AGENTS.md: general fixes only, never keyed on an
application; `QUIRK:` markers with the oracle's evidence; no Apple code of
any kind. A ladder application only discovers a gap; the fix has its own
probe.

### 5.6 The workflow

`.github/workflows/exp-macfw.yml`: `workflow_dispatch` only, no other
trigger; `concurrency: exp-macfw` with `cancel-in-progress: false` (one run
at a time across all branches; a second dispatch waits); exactly one macOS
job (`oracle`, 50-minute limit), skipped when `oracle_run` reuses an earlier
oracle; Linux jobs `darling` (90 minutes), `candidate` (40), `score` (10),
at most two in parallel. Inputs: `cycle`, `oracle_run`, `queue`, `rungs`
(default 1-5), `arm64`.

## 6. Cost

GitHub-hosted standard runners cost nothing for a public repository; the
cost that matters is load, which is why GitHub Actions is off today. The
estimates below are from step counts, not from runs (**NOT RUN**).

| Job | Each run | Notes |
|---|---|---|
| oracle, rungs 1-5 | about 30 macOS minutes | probes 8 programs and 4 own rungs about 5 min; TableTool, Itsycal, Hex Fiend about 10 min; probes twice on two architectures about 6 min; ladder about 6 min; set-up 3 min |
| oracle, all ten rungs | about 45 macOS minutes | adds Hidden Bar, TomatoBar, SwiftUI-Kit (SwiftPM resolution) and five more ladder runs; hard limit 50 |
| darling, upstream | 3 minutes (Darling's CI packages) to about 90 (a cold build) | Darling's CI builds warm in about 15 minutes |
| darling, queue | 20 minutes warm; the first cold build may hit the 90-minute limit and need a second dispatch (ccache is saved even when the job fails) | ccache 2 GB per architecture in the repository's cache, which was 9.2 of 10 GB full on 14 November |
| candidate | about 25 Linux minutes per variant | probes twice, ten rungs at about 80 s each |
| score | 2 Linux minutes | |

Plan for the whole budget: one oracle at c00, a new one only when probes
change (expected every third cycle) and one full-ladder oracle at the end:
**about 6 oracle runs, 200 macOS minutes expected, 400 the registered
ceiling.** Linux: about 75 to 200 minutes per cycle, so 1,000 to 2,400 for
12 cycles (double where arm64 is switched on). Per cycle, the macOS share
is zero unless probes changed: the design reuses the oracle across cycles
on purpose.

## 7. Legal notes (for the owner's lawyer; not legal advice)

1. **Clean room.** Apple is observed only through its public documentation
   and the behaviour of programs TUR writes and builds. Nothing in the
   branch contains, copies or derives from Apple binaries, SDK headers or
   Apple source; no Apple code is read, disassembled or decompiled. The
   ladder excludes Apple sample code. Darling's AGENTS.md imposes the same
   rules on every patch, and its evidence rules accept this kind of
   observation.
2. **The Xcode and Apple SDKs Agreement** (EA2002, dated 06/08/2026, the
   version at apple.com/legal/sla/docs/xcode.pdf on 16 November 2026):
   - §2.2.A grants installation "on Apple-branded computers that are owned
     or controlled by You" and, for the macOS SDK, use "to test and develop
     application and other software". The narrower purpose, Applications
     "specifically for use on Apple-branded products", is stated for the
     *other* SDKs (§2.2.A(iii)) and Apple Services (§2.2.A(iv)), not for the
     macOS SDK. The distribution restriction that follows also excludes the
     macOS SDK.
   - §2.7 forbids installing, using or running "the Apple Software" on
     non-Apple hardware. Xcode and the SDK run only on GitHub's Macs here;
     what leaves the runner is TUR's compiled programs and open-source
     applications, which are not Apple Software.
   - §2.7 forbids decompiling or disassembling the Apple Software; nothing
     here does.
   - **Questions for the lawyer:** (a) whether a GitHub-hosted macOS runner,
     provided by GitHub with Xcode preinstalled, counts as a computer
     "controlled by You" (GitHub's arrangement with Apple; common practice,
     but not TUR's to assert); (b) whether running TUR-built binaries outside
     macOS, when they were built with the macOS SDK, conflicts with anything
     in the agreement's definitions (our reading of §2.2.A(ii): no); (c)
     whether any statically linked toolchain code in those binaries is
     Apple Software rather than open source (we expect only LLVM's
     compiler-rt and Swift's compatibility libraries, both Apache-2.0 from
     swift.org and llvm.org; `build-manifest.txt` records each binary's
     linkage so this can be checked on the first run); (d) compiled nibs and
     asset catalogs are outputs of Apple's tools from the applications' own
     sources; system images (SF Symbols) are referenced by name, never
     embedded (§2.10 restricts System-Provided Images).
3. **Built binaries leaving the runner.** They leave as workflow artifacts
   (30 days). For a public repository, artifacts can be downloaded by any
   signed-in GitHub user, so this is distribution of TUR's probes and of the
   ladder applications' builds. The ladder's licences (MIT, BSD-2-Clause)
   permit it with the licence notice; `build_ladder_macos.py` puts each
   project's licence file beside its build. Vendored third-party binaries
   (Sparkle, MASShortcut, both open source) travel inside those builds.
   Retention can be shortened to 7 days if the lawyer prefers.
4. **Licences in the branch.** The harness and probes are TUR's own,
   standalone programs that drive Darling from the command line; they do not
   link Darling (AGENTS.md licence rules). The patch queue is under
   Darling's licences in a marked directory; if it is ever kept beyond the
   spike, AGENTS.md asks for an ADR. The probes are under TUR's licence; to
   give them to Darling as tests, the owner would have to license them under
   GPL-3.0-or-later (H4).
5. **Upstream.** Darling's AGENTS.md requires a human who answers for each
   pull request. No patch goes upstream from this experiment without a
   named TUR person; that is the owner's decision.

## 8. Risks and unknowns

- **Darling outruns the loop.** At about a hundred commits a day, any
  difference TUR finds may be fixed upstream within days. The same-commit
  control keeps the measurement honest; it also means a K2 kill is a real
  possibility even if the ladder moves.
- **Darling in a GitHub runner.** Darling's own CI builds packages but does
  not run them there. Whether `darling shell` starts in a hosted Ubuntu
  runner (overlayfs prefix, setuid launcher, namespaces) is unknown until
  c00. If it does not, the candidate needs a VM (TUR's existing QEMU pattern)
  and the cost section changes.
- **Status items under X11** (rungs 4, 7, 9): how Darling presents an
  NSStatusItem without a macOS menu bar is undocumented; `ladder.py` looks
  for a small new window. If Darling has no visible status item, those
  rungs need a different usable-window definition, registered before the
  first measurement that uses it.
- **GitHub's macOS runner permissions:** Screen Recording for screenshots
  and Rosetta for the x86-64 oracle are not documented for the image; both
  are recorded in `env.txt` and the harness works without them (with a
  weaker oracle).
- **NSApplication opening a file named on the command line** (rungs 3 and
  5) is assumed; if Darling or macOS does not, the fixture is opened through
  the interaction steps instead.
- **The macOS version Darling reports** is fixed at build time (#1959,
  open). Objective-C probes and rungs 1 and 2 target macOS 11; the Swift
  probes and rung 6 target macOS 14 (for `MainActor.assumeIsolated`), rung 8
  macOS 14 (SwiftUI's `onChange` form). If Darling reports an older version
  and something checks the binary's minimum, that shows up as a load
  failure with its own message, not as an AppKit difference.
- **Probe correctness:** the probes were written without a compiler; the
  first oracle run will surface compile errors in `build.log`, to be fixed
  before c00 counts.

## 9. Decision

### 9.1 Recommendation

Run **cycle c00 only** now (`cycle: c00`, `queue: false`, `rungs: 1-5`):
one oracle and upstream Darling, about 30 macOS minutes and an hour of
Linux. It answers, for the first time and with numbers, how far Darling's
AppKit is from macOS on a fixed set of public-API behaviours and five
rungs. Then:

- if K0 fires (upstream already reaches rung 4), stop H1 and offer the
  oracle and probes to Darling (H4): that is the most useful thing TUR can
  give a project that is already doing the fixing;
- if upstream does not start rung 1, look at why before any cycle (it may be
  the runner, not Darling);
- otherwise run cycles, at most 12, within the registered budget, and
  decide early who at TUR would own patches upstream.

Given TUR's priorities (macOS is item 8, "not a main effort"), the
experiment should not grow beyond that budget whatever the first results.

### 9.2 Odds, before any measurement

| Outcome | My estimate | Why |
|---|---|---|
| Rung 1 at c00, upstream alone | 60% | Darling's November AppKit work covers windows, views, text fields, buttons and stack views, tested on Xvfb |
| Rung 2 reached by the end, any cause | 70% | the controls are mainstream and Darling is fixing them now |
| Rung 2 reached because of TUR's queue | 35% | upstream may reach it first; the same-commit control would then not credit the loop |
| Rung 4 (Itsycal) reached, any cause | 30% | status item, popover, EventKit, two vendored frameworks, custom drawing |
| Rung 4 because of TUR's queue | 15% | as above, and the queue's patches must be ones Darling has not made |
| K0 (superseded at c00) | 10% | |
| Rung 6 (Swift AppKit) by the end | 15% | needs the AppKit Swift API inside AppKit (#2059 step 3) |
| Rung 8 or above (SwiftUI) | under 5% | about 19,600 SwiftUI symbols, none implemented, no binary-compatible reimplementation |
| H4 supported (10 new, relevant causes) | 60% | the probes cover behaviour Darling's own tests do not; but Darling's velocity shortens the window in which a cause stays new |

## 10. What was not done

- **Nothing was run.** No oracle, no candidate, no ladder. Every measured
  quantity above is NOT RUN.
- The probes and TUR's rungs have not been compiled; they were written
  against Apple's public documentation without a compiler.
- Darling was not built or run anywhere; its state comes from its
  repositories, issues and pull requests.
- The census did not open any binary; the share of universal binaries among
  single-download casks is inferred (section 3.1).
- Wayland (headless sway) as the candidate's display is designed into the
  harness (`MACFW_BACKEND`) but not wired into the workflow; X11 first.
- Fedora: the candidate runs on Ubuntu with Darling's Debian packages.
  Packaging Darling for TUR's image is out of scope until H1 is supported.
- No numbered ADR (the patch queue is a spike, under its upstream's licence
  in a marked directory).

## Sources

All read on 16 November 2026 unless dated otherwise.

- Darling: [repository](https://github.com/darlinghq/darling) (master
  `41130df8`), [AGENTS.md](https://github.com/darlinghq/darling/blob/master/AGENTS.md),
  issues [#1806](https://github.com/darlinghq/darling/issues/1806),
  [#2059](https://github.com/darlinghq/darling/issues/2059),
  [#2055](https://github.com/darlinghq/darling/issues/2055); pull requests
  [#1811](https://github.com/darlinghq/darling/pull/1811),
  [#1818](https://github.com/darlinghq/darling/pull/1818),
  [#1883](https://github.com/darlinghq/darling/pull/1883),
  [#1903](https://github.com/darlinghq/darling/pull/1903),
  [#2066](https://github.com/darlinghq/darling/pull/2066),
  [#2069](https://github.com/darlinghq/darling/pull/2069);
  [darling-cocotron](https://github.com/darlinghq/darling-cocotron),
  [darling-foundation](https://github.com/darlinghq/darling-foundation),
  [darling-swift](https://github.com/darlinghq/darling-swift),
  [darling-skills](https://github.com/darlinghq/darling-skills),
  [darling-appdb-database](https://github.com/darlinghq/darling-appdb-database),
  [docs](https://docs.darlinghq.org/); CI run 43762728027 (job timings).
- GNUstep: [libs-base](https://github.com/gnustep/libs-base),
  [libs-gui](https://github.com/gnustep/libs-gui),
  [libs-back](https://github.com/gnustep/libs-back),
  [libobjc2 #306](https://github.com/gnustep/libobjc2/issues/306),
  [apps-gorm POLICY_AI.md](https://github.com/gnustep/apps-gorm).
- SwiftUI and Swift: [OpenSwiftUI](https://github.com/OpenSwiftUIProject/OpenSwiftUI),
  [Tokamak](https://github.com/TokamakUI/Tokamak),
  [SwiftCrossUI](https://github.com/moreSwift/swift-cross-ui),
  [SkipUI](https://github.com/skiptools/skip-ui),
  [swift-corelibs-foundation](https://github.com/swiftlang/swift-corelibs-foundation),
  [swift-foundation](https://github.com/swiftlang/swift-foundation),
  [Swift releases](https://github.com/swiftlang/swift/releases).
- Census: [Homebrew cask API](https://formulae.brew.sh/api/cask.json);
  timeline sources in section 3.2.
- Runners: [actions/runner-images](https://github.com/actions/runner-images)
  and its macOS 26 arm64 image readme (image 20261102.0473.1).
- Apple: [Xcode and Apple SDKs Agreement](https://www.apple.com/legal/sla/docs/xcode.pdf)
  (EA2002, 06/08/2026).
- Ladder: the repositories linked in section 4, at the commits named there.
- TUR: [exp-rehost-1](exp-rehost-1.md); exp-windiff-1 (branch `exp/windiff`,
  pull request 124); [frontier](../frontier.md).
