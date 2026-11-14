#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Score every candidate against the Windows judge.

    score.py --truth DIR [--truth-other DIR] --candidates DIR --out DIR

DIR/<candidate>/shapes.json is what run.py wrote. The judge is the `truth`
run on the newest Windows runner; `--truth-other` is the same corpus on an
older Windows, scored against the judge like any candidate: how far Windows
is from Windows is the ceiling for everything else, and it shows which
checks are too strict. `ps51` (Windows PowerShell 5.1 on the judge) is a
second such floor.

Verdicts:

    PASS        same exit class (or exact code where marked) and same shape
    PARTIAL     same exit class, at least half of the shape
    ANSWER      not the same, but TUR said so: an explanation or a refusal
                (the `tur-winshell:` marker on standard error)
    FAIL        anything else
    NOT RUN     the candidate did not start, or was stopped
    INVALID     the check does not hold on Windows itself; not counted

Writes results.json, summary.md (the scorecard) and gaps.md.
"""

from __future__ import annotations

import argparse
import collections
import json
import pathlib
import re
import statistics
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import corpus  # noqa: E402
import shape  # noqa: E402

ORDER = ["windows-other", "ps51", "wine-cmd", "wine-pwsh", "pwsh-native", "pwsh-compat", "bash-shims", "router"]
LABEL = {
    "windows-other": "Windows, the other version (ceiling)",
    "ps51": "Windows PowerShell 5.1 on the judge (floor)",
    "wine-cmd": "(a) Wine cmd.exe and Wine's tools",
    "wine-pwsh": "(b) PowerShell 7 for Windows under Wine",
    "pwsh-native": "(c) PowerShell 7 for Linux, as shipped",
    "pwsh-compat": "(c+) PowerShell 7 for Linux + TurWinCompat",
    "bash-shims": "(e) pasted into bash, TUR names on PATH",
    "router": "(d) tur-winshell",
    "oracle": "best of (a), (b), (c), (c+) per command",
}
GROUPS = ["cmd", "tools", "ps"]
NOT_FOUND = re.compile(r"Can't recognize|is not recognized as|command not found|CommandNotFoundException|"
                       r"No such file or directory")


def load(path: pathlib.Path) -> dict:
    """A JSON file; also found one level up, where an artifact with a single
    candidate in it loses the candidate's directory."""
    for candidate in (path, path.parent.parent / path.name):
        try:
            return json.loads(candidate.read_text())
        except (OSError, ValueError):
            continue
    return {}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--truth", required=True)
    parser.add_argument("--truth-other", default="")
    parser.add_argument("--candidates", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    out_dir = pathlib.Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    truth = load(pathlib.Path(args.truth) / "truth" / "shapes.json")
    entries = corpus.CORPUS
    shapes: dict[str, dict] = {}
    raws: dict[str, dict] = {}
    metas: dict[str, dict] = {"truth": load(pathlib.Path(args.truth) / "truth" / "meta.json")}
    if args.truth_other:
        shapes["windows-other"] = load(pathlib.Path(args.truth_other) / "truth" / "shapes.json")
        metas["windows-other"] = load(pathlib.Path(args.truth_other) / "truth" / "meta.json")
    shapes["ps51"] = load(pathlib.Path(args.truth) / "ps51" / "shapes.json")
    for name in ORDER[2:]:
        base = pathlib.Path(args.candidates) / name
        shapes[name] = load(base / "shapes.json")
        raws[name] = load(base / "raw.json")
        metas[name] = load(base / "meta.json")

    invalid: dict[str, str] = {}
    for entry in entries:
        if not entry["truth"]:
            continue
        mine = truth.get(entry["id"])
        if mine is None:
            invalid[entry["id"]] = "not run on Windows"
        elif "notrun" in mine:
            invalid[entry["id"]] = mine["notrun"]
        else:
            problem = shape.truth_problem(entry, mine)
            if problem:
                invalid[entry["id"]] = problem

    results: dict[str, dict] = {}
    for entry in entries:
        ident = entry["id"]
        row: dict = {"group": entry["group"], "cmd": entry["cmd"], "rw": entry["rw"], "fresh": entry["fresh"],
                     "verdicts": {}}
        if ident in invalid:
            row["invalid"] = invalid[ident]
        for name in ORDER:
            got = shapes.get(name, {}).get(ident)
            if got is None:
                continue
            if "notrun" in got:
                row["verdicts"][name] = {"verdict": "NOT RUN", "why": [got["notrun"]]}
                continue
            if not entry["truth"]:
                verdict = {"verdict": "ANSWER" if got.get("answer") else "FAIL", "why": ["no ground truth"],
                           "fidelity": None}
            elif ident in invalid:
                verdict = {"verdict": "INVALID", "why": [invalid[ident]]}
            else:
                verdict = shape.compare(entry, truth[ident], got)
                if verdict["verdict"] != "PASS" and got.get("answer"):
                    verdict["verdict"] = "ANSWER"
                if verdict["verdict"] == "PASS" and truth[ident].get("exit") != got.get("exit"):
                    verdict["exit_differs"] = [truth[ident].get("exit"), got.get("exit")]
            raw = raws.get(name, {}).get(ident, {})
            if verdict["verdict"] in ("FAIL", "PARTIAL") and NOT_FOUND.search(raw.get("stderr", "") + raw.get("stdout", "")):
                verdict["not_found"] = True
            verdict["secs"] = got.get("secs")
            row["verdicts"][name] = verdict
        parts = [row["verdicts"].get(n, {}).get("verdict") for n in ("wine-cmd", "wine-pwsh", "pwsh-native", "pwsh-compat")]
        parts = [v for v in parts if v]
        if parts and ident not in invalid and entry["truth"]:
            row["verdicts"]["oracle"] = {"verdict": "PASS" if "PASS" in parts else "FAIL", "why": []}
        results[ident] = row
    (out_dir / "results.json").write_text(json.dumps(results, indent=1, sort_keys=True))

    # ------------------------------------------------------- scorecard --
    lines: list[str] = []
    held_out = {e["id"] for e in entries if e["fresh"]}
    entries = [e for e in entries if not e["fresh"]]
    fresh_results = {i: r for i, r in results.items() if i in held_out and r["verdicts"]}
    results = {i: r for i, r in results.items() if i not in held_out}
    invalid_main = {i: w for i, w in invalid.items() if i not in held_out}
    counts = collections.Counter(e["group"] for e in entries)
    changing = sum(1 for e in entries if e["rw"])
    lines += ["## Scorecard", "",
              f"Corpus: {len(entries)} commands ({counts['cmd']} cmd, {counts['tools']} tools, {counts['ps']} "
              f"PowerShell); {changing} state-changing, {len(entries) - changing} read-only. "
              f"{len(invalid_main)} checks do not hold on the judge itself and are not counted; "
              f"{sum(1 for e in entries if not e['truth'])} repair commands have no ground truth.", ""]
    for name in ["truth", "windows-other"] + ORDER[2:]:
        meta = metas.get(name) or {}
        if meta:
            keep = {k: v for k, v in meta.items() if k in ("powershell", "cmd", "seconds", "entries")}
            start = meta.get("bring_up", {})
            lines.append(f"- `{name}`: {json.dumps(keep)}; started: {start.get('ok')}")
    lines.append("")

    def tally(name: str, group: str | None, rows=None):
        tally_counter = collections.Counter()
        for row in (results if rows is None else rows).values():
            if group and row["group"] != group:
                continue
            verdict = row["verdicts"].get(name)
            if verdict:
                tally_counter[verdict["verdict"]] += 1
        return tally_counter

    header = "| Candidate | Group | PASS | PARTIAL | ANSWER | FAIL | NOT RUN | Scored | Pass rate |"
    lines += [header, "|---|---|---|---|---|---|---|---|---|"]
    rates: dict[str, dict[str, float]] = {}
    for name in ORDER + ["oracle"]:
        for group in GROUPS + [None]:
            tc = tally(name, group)
            scored = tc["PASS"] + tc["PARTIAL"] + tc["ANSWER"] + tc["FAIL"]
            if not scored and not tc["NOT RUN"]:
                continue
            rate = 100.0 * tc["PASS"] / scored if scored else 0.0
            rates.setdefault(name, {})[group or "all"] = rate
            lines.append(f"| {LABEL[name]} | {group or '**all it ran**'} | {tc['PASS']} | {tc['PARTIAL']} | "
                         f"{tc['ANSWER']} | {tc['FAIL']} | {tc['NOT RUN']} | {scored} | "
                         f"{rate:.1f} % |" if scored else
                         f"| {LABEL[name]} | {group or '**all it ran**'} | 0 | 0 | 0 | 0 | {tc['NOT RUN']} | 0 | NOT RUN |")
    lines.append("")

    if fresh_results:
        lines += ["### Held-out set", "",
                  f"{len(fresh_results)} commands written before any result was seen and run only now; nothing was "
                  "tuned on them.", "", "| Candidate | PASS | PARTIAL | ANSWER | FAIL | NOT RUN | Scored | Pass rate |",
                  "|---|---|---|---|---|---|---|---|"]
        for name in ORDER + ["oracle"]:
            tc = tally(name, None, fresh_results)
            scored = tc["PASS"] + tc["PARTIAL"] + tc["ANSWER"] + tc["FAIL"]
            if scored:
                lines.append(f"| {LABEL[name]} | {tc['PASS']} | {tc['PARTIAL']} | {tc['ANSWER']} | {tc['FAIL']} | "
                             f"{tc['NOT RUN']} | {scored} | {100.0 * tc['PASS'] / scored:.1f} % |")
        lines.append("")

    # The router: what is left unexplained.
    router_rows = [(i, r) for i, r in results.items() if "router" in r["verdicts"]]
    unexplained = [(i, r) for i, r in router_rows
                   if r["verdicts"]["router"]["verdict"] in ("FAIL", "PARTIAL")]
    silent = [(i, r) for i, r in unexplained if r["verdicts"]["router"].get("not_found")]
    lines += ["### tur-winshell: what it does not pass", "",
              f"Not passed and not answered: {len(unexplained)}; of those, left at a bare \"not found\": "
              f"{len(silent)}.", ""]

    # Timing.
    lines += ["### Seconds per command (median, 90th percentile)", "", "| Candidate | Median | p90 |", "|---|---|---|"]
    for name in ORDER:
        secs = sorted(v["secs"] for r in results.values() for n, v in r["verdicts"].items()
                      if n == name and isinstance(v.get("secs"), (int, float)))
        if secs:
            lines.append(f"| {LABEL[name]} | {statistics.median(secs):.2f} | {secs[int(len(secs) * 0.9)]:.2f} |")
    lines.append("")

    # Exit codes that agree only as zero/non-zero.
    loose = [(i, n, v["exit_differs"]) for i, r in results.items() for n, v in r["verdicts"].items()
             if v.get("exit_differs")]
    lines += [f"Passes whose exit code differs from Windows' in value but not in class: {len(loose)}.", ""]

    if invalid:
        lines += ["### Checks that do not hold on the judge (not counted)", "", "| Command | Why |", "|---|---|"]
        for ident, why in sorted(invalid.items()):
            lines.append(f"| `{ident}` | {why.replace('|', chr(92) + '|')} |")
        lines.append("")

    # Hypothesis.
    best = rates.get("router", {})
    if best:
        overall = best.get("all", 0.0)
        low = min(best.get(g, 0.0) for g in GROUPS)
        state = "SUPPORTED" if overall >= 80 and low >= 60 and not unexplained else \
            "KILLED" if overall < 50 else "PARTLY SUPPORTED"
        lines += ["### Hypothesis", "",
                  f"tur-winshell passes {overall:.1f} % of what it ran; its weakest group passes {low:.1f} %; "
                  f"{len(unexplained)} commands are neither passed nor answered. By the criteria in corpus.py: "
                  f"**{state}**.", ""]
    (out_dir / "summary.md").write_text("\n".join(lines) + "\n")

    # ------------------------------------------------------------ gaps --
    gap: list[str] = ["## Per command", "",
                      "| Command | " + " | ".join(n for n in ORDER + ["oracle"]) + " |",
                      "|---|" + "---|" * (len(ORDER) + 1)]
    short = {"PASS": "pass", "PARTIAL": "partial", "ANSWER": "answer", "FAIL": "FAIL", "NOT RUN": "not run",
             "INVALID": "invalid"}
    for ident, row in {**results, **fresh_results}.items():
        cells = [short.get(row["verdicts"].get(n, {}).get("verdict"), "") for n in ORDER + ["oracle"]]
        gap.append(f"| `{ident}` | " + " | ".join(cells) + " |")
    gap += ["", "## What each candidate got wrong", ""]
    for name in ORDER[2:]:
        gap += [f"### {LABEL[name]}", "", "| Command | Line | Verdict | What differed |", "|---|---|---|---|"]
        for ident, row in {**results, **fresh_results}.items():
            verdict = row["verdicts"].get(name)
            if not verdict or verdict["verdict"] in ("PASS", "INVALID"):
                continue
            why = ", ".join(str(w) for w in verdict.get("why", []))[:160].replace("|", chr(92) + "|")
            if verdict.get("not_found"):
                why += " (not found)"
            line = row["cmd"][:90].replace("|", chr(92) + "|").replace("`", "'")
            gap.append(f"| `{ident}` | `{line}` | {verdict['verdict']} | {why} |")
        gap.append("")
    (out_dir / "gaps.md").write_text("\n".join(gap) + "\n")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
