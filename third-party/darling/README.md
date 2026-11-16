<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Darling: TUR's patch queue

**Not under TUR's licence.** This directory holds TUR's changes to
[Darling](https://github.com/darlinghq/darling), the translation layer that
runs macOS programs on Linux. Every patch here is a change to Darling and
carries the licence of the Darling file it changes. This README, `series`
and [`LICENSE`](LICENSE) (Darling's own licence file, the GNU General Public
License version 3, verbatim from the pinned commit) are under
GPL-3.0-or-later, so that the directory can be given to Darling as it is.

## The owner's decisions (16 November 2026)

1. > "Implement Darling as well and do whatever is needed."
2. **TUR's Darling fixes stay inside TUR.** They are a patch queue on a
   pinned Darling commit, applied when TUR builds Darling. They are not a
   fork, and they are not offered upstream. If that changes, a patch goes to
   Darling only through a named person at TUR who has read it, can explain
   it, and answers Darling's reviewers personally, as Darling's
   [AGENTS.md](https://github.com/darlinghq/darling/blob/master/AGENTS.md)
   requires of every contribution, AI-assisted or not.
3. > "Darling should be installed by default and then kept updated."

   Darling is in the OS image, from TUR's prebuilt archive taken by digest
   (`image/darling.lock`; the image build never compiles it), and arrives
   with TUR's own signed image updates, so it rolls back with the image.
4. **Apple's open-source code under APSL-2.0 inside Darling builds may
   ship.** "Never ship Apple binaries" is about Apple's proprietary
   software; APSL-2.0 code built by TUR from published source ships with
   APSL's notices and with its source available. Also on the list for the
   owner's lawyer.
5. **TUR publishes the corresponding source itself**, beside each archive,
   in the same registry package and pinned by digest: Darling at the exact
   commit with every submodule, and this patch queue. Not only a pointer to
   Darling's repository.
6. **Nothing is published before the licences are read.** The first publish
   waits until a person has read every licence file of Darling and its
   submodules (the build prints them, `licences.md`) and summarised them in
   docs/reports/macos-darling-1.md; `scripts/darling/licences-reviewed.txt`
   lists the texts read, and publishing refuses any other.
7. **The registry package is made public by the owner**, once, after the
   first publish.
8. **"Measured to work" means TUR's own measurement.** The router offers
   Darling only for an application with a record of TUR's own tests;
   outside reports such as Darling's AppDB are shown and never enable it.

## What is here

| | |
|---|---|
| [`patches/series`](patches/series) | The queue, in order: one line per patch, `<submodule path, or . for the main repository> <patch file>`. **Empty**: TUR's first build is Darling as upstream has it |
| `patches/*.patch` | `git format-patch` output for the submodule's own repository, each with the header below |
| [`LICENSE`](LICENSE) | Darling's licence file |

| | |
|---|---|
| Against | the commit in [`scripts/darling/darling.lock`](../../scripts/darling/darling.lock) (`3fcd2e1aa85079789d19d82415695bdecc512241`, 15 November 2026); its submodules at the commits that tree records |
| Applied and built by | [`scripts/darling/build-darling.sh`](../../scripts/darling/build-darling.sh), in CI only (workflow `darling-runtime`), with `git apply --index` in that directory, in the order of `series`; a patch that does not apply stops the build |
| Shipped | in TUR's Darling archive, published to the repository's package registry when the lead dispatches the workflow, with the corresponding source (Darling at the commit with its submodules, and this directory as `tur-patch-queue/`); the OS image carries the archive the pin in `image/darling.lock` names. Each build's record names the queue's SHA-256 (the `series` file, then every patch it names, in order), and so does the build's id |

## Rules for every patch

1. **One reason.** The header says why, in one sentence, and what shows it
   (a test, a probe of the macfw experiment, a ladder application).
2. **General, not for one application.** A patch implements documented
   behaviour, never a special case keyed on an application.
3. **Clean room.** Written from Apple's public documentation and from the
   observed behaviour of programs TUR writes and builds (the macfw
   experiment's probes, docs/reports/exp-macfw-0.md). Never from Apple
   source, SDK headers, disassembly or decompilation of Apple binaries, or
   leaked material. Darling's own AGENTS.md has the same rule.
4. **Small, and rebased.** When the pinned commit moves, every patch is
   rebased or dropped; a patch whose fix Darling has made is dropped, and the
   commit that superseded it is named in the change that drops it.
5. **Tested.** A test in Darling's `tests/` style for the behaviour, and the
   integration workflow `macos-darling` run with the queue.

Header of every patch file, above `git format-patch`'s own:

```text
TUR-Reason: <why, in one sentence>
TUR-Evidence: <test, probe check ids, oracle run>
TUR-Provenance: <how it was written: from which documentation and observations>
TUR-Upstream: kept in TUR (owner's decision of 16 November 2026)
```

## Darling's licences

Darling is "licensed under GNU GPL 3.0. Individual submodules may be licensed
differently, as indicated within each of them" (Darling's README). From its
README and repositories, read on 16 November 2026:

| Part | Licence |
|---|---|
| Darling itself: the Mach-O loader `mldr`, the launcher, `darlingserver`, framework glue | GPL-3.0-or-later |
| AppKit, CoreGraphics, Onyx2D (`darling-cocotron`, from Cocotron) | MIT |
| Foundation (`darling-foundation`) | LGPL-2.1 |
| CoreFoundation (`darling-corefoundation`, from GNUstep corebase and Apple's CF-Lite) | mixed, as its files say (LGPL, APSL-2.0) |
| The low layers: XNU parts, libSystem, Security and others | Apple's open-source releases, APSL-2.0 and others ("based on Apple's open-source releases", Darling's README) |
| The Swift runtime (`darling-swift`) | Apache-2.0, unmodified binaries from swift.org: **removed from TUR's build** (below) |

Every licence file of Darling and of each submodule is copied into the
archive, under `share/licenses/darling/`, with `SOURCE.txt` (where the
corresponding source is: Darling's repository at the pinned commit, the
submodule commits, and this queue) and `PATCHES.txt`.

## What TUR's build leaves out

TUR never ships Apple binaries, SDK headers or Apple source (AGENTS.md).
Darling's build contains some, and TUR's build removes them before the
archive is made (`scripts/darling/build-darling.sh`), then refuses the archive
if any is left (`scripts/darling/check-tree.py`):

- **The Swift runtime** (`libexec/darling/usr/lib/swift`): Darling ships it as
  "unmodified binaries from `usr/lib/swift/macosx`" of Apple's swift.org
  package. Without it, Swift applications that use macOS's Swift runtime do
  not start; tur-runtime refuses them with a plain reason
  (`TUR-RUNTIME-MACOS-005`).
- **Headers, Swift interfaces, text stubs, static libraries, SDK
  directories**: not needed to run programs.
- **Applications in the guest's `/Applications`**: Darling builds Apple's
  TextEdit sample code there.
- **Anything signed with an Apple certificate** stops the build.

What remains still contains code compiled from Apple's open-source releases
under APSL-2.0, as every build of Darling does. By the owner's decision (4,
above) it ships, with APSL's notices (in `share/licenses/darling/`) and with
its source published beside the archive (5).
