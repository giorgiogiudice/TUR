<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Experiment macfw: real Macs as the oracle, Darling as the candidate

Plan, facts, registered hypotheses and kill criteria:
[docs/reports/exp-macfw-0.md](../../../docs/reports/exp-macfw-0.md).
**Nothing here has run yet.** The workflow,
[`.github/workflows/exp-macfw.yml`](../../../.github/workflows/exp-macfw.yml),
starts only by hand (`workflow_dispatch`).

| Path | What |
|---|---|
| `probes/` | TUR's probe programs: Objective-C (`mf.h` framework) and Swift (`MF.swift`). Each calls public Foundation, AppKit or SwiftUI API and prints one JSON line per observation. Built once on macOS, run unchanged on macOS and under Darling |
| `ladder/ladder.json` | The ladder: ten applications from a hello-world window to small real SwiftUI apps, pinned by commit, with each rung's usable-window test |
| `ladder/own/` | Rungs 1, 2, 6 and 8, written by TUR |
| `ci/oracle-macos.sh` | macOS runner: build probes and ladder (`build-probes-macos.sh`, `build_ladder_macos.py`), run them, record |
| `ci/darling-get.sh` | Linux runner: Darling's packages at the pinned commit (`darling.lock`), upstream or with TUR's queue |
| `ci/candidate-linux.sh` | Linux runner: the same binaries under Darling on Xvfb with xfwm4 |
| `harness/run_probes.py` | Runs every probe group twice on either side |
| `harness/diff.py` | Classes every check (same, different, unimplemented, missing, group not run to the end, metric) and, with `--before`, what TUR's queue closed and broke |
| `harness/ladder.py` | Starts each application, finds its window, screenshots, drives it, scores 0 to 4 |
| `harness/scoreboard.py` | One row per fix cycle in `scoreboard.json`; evaluates the registered kill criteria |
| `triage.tsv` | Differences grouped into causes |
| `darling-patches/` | TUR's patch queue to Darling, **under Darling's licences** (GPL-3.0-or-later, MIT for Cocotron, LGPL-2.1 for Foundation); see its README |

## One fix cycle

1. The lead dispatches `exp-macfw` (cycle `c00` first: oracle, upstream
   Darling only).
2. The `macfw-report` artifact has `differences.md` per candidate and the
   scoreboard row. The engineer (an agent) triages differences into
   `triage.tsv` and picks causes that a rung's failure points to.
3. For each cause: a probe that isolates it (if the existing check does not),
   a patch to Darling's source written from the documented contract and the
   oracle's observation, a Darling-style test, the header described in
   `darling-patches/README.md`. Nothing is compiled on the development
   machine.
4. Push; the lead dispatches with `queue: true` and `oracle_run` set to the
   run whose oracle is still valid (new probes need a new oracle run).
5. The score job says what the queue closed and broke at the same Darling
   commit; `scoreboard.py add` output is committed with the cycle.

## Rules

Apple is observed only from outside: through its public documentation and
the behaviour of programs TUR writes and builds. No Apple binary, SDK header
or source is committed, copied or redistributed; no Apple code is read,
disassembled or decompiled. Darling's AGENTS.md applies to every patch.
