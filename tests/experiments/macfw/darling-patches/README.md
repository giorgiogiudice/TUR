<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# macfw fix queue: patches to Darling

**Not under TUR's licence.** Every file in this directory except this README
and [`series`](series) is a patch to Darling and carries the licence of the
Darling file it changes:

| Darling component | Where | Licence |
|---|---|---|
| Darling itself (mldr, darlingserver glue, framework stubs, `src/frameworks`) | `darlinghq/darling` | GPL-3.0-or-later |
| AppKit, CoreGraphics, Onyx2D (Cocotron-derived) | `darlinghq/darling-cocotron`, `src/external/cocotron` | MIT |
| Foundation | `darlinghq/darling-foundation`, `src/external/foundation` | LGPL-2.1 |
| CoreFoundation | `darlinghq/darling-corefoundation` | as its files say (mixed: GNUstep corebase LGPL and Apple's APSL CF-Lite) |

This README and `series` are GPL-3.0-or-later so that the whole directory can
be offered to Darling as it is.

| | |
|---|---|
| Against | the Darling commit in [`../ci/darling.lock`](../ci/darling.lock); each line of `series` names the submodule path the patch applies to |
| Applied and built by | [`../ci/darling-get.sh`](../ci/darling-get.sh) `queue`, in CI only (Darling's own `tools/debian/make-deb`, with ccache) |
| Verified by | the probe difference it names: equal to macOS afterwards, at the same Darling commit, with no other probe check changing from same to different (`diff.py --before`), and the ladder not losing a rung |
| Shipped | never; this queue measures the loop. A patch worth keeping goes to Darling as a pull request by a person (below) |

## Rules for every patch

Darling's [AGENTS.md](https://github.com/darlinghq/darling/blob/master/AGENTS.md)
(read 2026-11-16) is binding here, because the point of the queue is to be
acceptable upstream:

- Implement the documented contract, never a special case for one
  application (section 1). A ladder application only *finds* a gap; the fix
  is general and has its own probe.
- Fix at the layer where macOS has the behaviour (section 2).
- Undocumented behaviour is a `QUIRK:` with its evidence: the probe check id,
  the oracle run, the macOS version and architecture. TUR's probes are
  black-box observations through the public API, Darling's evidence tier 2
  (section 3).
- No disassembly or decompilation of Apple binaries, no leaked or
  source-available-only Apple code, no Apple SDK header text (sections 4, 5,
  7). Nothing in this experiment reads Apple code of any kind.
- No debugging leftovers (section 6). A test in Darling's `tests/src` style
  (PASS/FAIL lines) for every behaviour (section 8).
- Each patch file starts with a header: `Subject`, `TUR-Reason` (why, in one
  sentence), `Evidence` (probe check ids and oracle run), `Provenance`, and
  `Upstream` (not offered / offered as PR #n / merged / superseded by
  upstream commit).

## Upstream

Darling accepts AI-assisted contributions, on conditions: the human who opens
the pull request is "responsible for every line", must be able to explain it
"in your own words", and answers maintainers' questions personally, "not from
your AI assistant pasted verbatim" (AGENTS.md, note to contributors). A patch
from this queue therefore reaches Darling only through a named TUR person who
has read and understood it and will answer for it. Until then it stays here.

## Format

`series`: one line per patch, `<submodule path or .> <patch file>`, applied in
order with `git apply --index` in that directory; a line starting with `#` is a
comment. Patch files are `git format-patch` output for the submodule's own
repository.
