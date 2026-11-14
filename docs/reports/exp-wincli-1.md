<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-wincli-1 — TUR's terminal behaves like a Windows PC's (experiment)

> **Status: registered, not yet measured.** This first version states the
> hypothesis, the kill criterion and the method before any run. Every
> result below is NOT RUN until a CI run is named next to it.

## Hypothesis and kill criterion (written before the first measurement)

**Hypothesis.** With the right combination of existing parts, a large
majority of everyday Windows commands can run on TUR with the same meaning
as on Windows, and the rest can be answered honestly ("this is what that
command does on Windows; on TUR the equivalent is …") instead of "command
not found".

- **Supported** if the best architecture passes at least 80 % of the corpus
  and at least 60 % of each of its three groups, where a pass is the same
  exit class and the same output shape as real Windows, and every command
  it does not pass gets an explanation.
- **Killed** if the best architecture passes less than 50 % overall, or no
  candidate passes half of some group.
- **Partly supported** otherwise; the report then says where.

## Method

- **Corpus** (`tests/experiments/wincli/corpus.py`): everyday commands as
  people type them, in three groups: cmd built-ins and batch constructs;
  the classic tools; PowerShell cmdlets and idioms. Each is marked
  read-only or state-changing; state-changing ones touch only a throwaway
  folder (`C:\turfix`), one throwaway registry key, one user variable and
  one scheduled task.
- **Judge**: the corpus on GitHub's Windows runners (real Windows; ADR-0015
  amendment). Recorded: exit codes and the shape of the output (normalised
  text of commands whose output comes from our own fixture, field names,
  table columns, which patterns matched, the state of the fixture folder),
  not Microsoft's text. Two Windows versions, so that the distance between
  Windows and Windows is known and the checks that are too strict show up.
- **Candidates** on Linux, Fedora 45 with Fedora's Wine, each scored against
  the judge: (a) Wine's cmd.exe and tools; (b) PowerShell 7 for Windows
  under Wine; (c) PowerShell 7 for Linux, alone and with a prototype
  compatibility module; (d) `tur-winshell`, a router with TUR-aware
  commands; (e) the lines pasted into bash with TUR's command names on
  PATH.
- **Integration pieces**: binfmt_misc, path translation both ways, scripts
  run by name. **Safety**: rules proposed in
  `tests/experiments/wincli/winshell/rules.py`, each tested.

## Results

NOT RUN.
