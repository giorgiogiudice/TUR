#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The macfw scoreboard: one row per fix cycle, and the registered kill
criteria evaluated on those rows (docs/reports/exp-macfw-0.md, section 4).

    scoreboard.py add --board scoreboard.json --cycle c03 --run RUN_ID
                      --diff DIR/diff.json [--diff-upstream DIR/diff.json]
                      --ladder ladder-darling.json [--ladder-upstream ladder-darling.json]
                      --darling COMMIT --patches N
    scoreboard.py render --board scoreboard.json > scoreboard.md

A cycle measures two candidates at the same pinned Darling commit: Darling
as upstream has it, and Darling with TUR's patch queue. Only what the queue
adds over upstream counts for the hypothesis; Darling's own progress is
recorded but is not the loop's.

`looked` (who looked at the contact sheet of a rung that scored 4, and
whether it is a usable window) is filled in by hand in scoreboard.json.
"""
import argparse
import json
import sys
import time

# Registered on 2026-11-16, before any measurement. Do not edit after the
# first cycle; a change is a new hypothesis with its own row in the report.
MAX_CYCLES = 12
MAX_WEEKS = 6
KILL_RUNG = 2
TARGET_RUNG = 4
MIN_FIX_YIELD = 0.25  # verified closures / differences the queue targeted


def load(p):
    try:
        with open(p) as f:
            return json.load(f)
    except FileNotFoundError:
        return {"registered": "2026-11-16", "cycles": []}


def ladder_scores(p):
    if not p:
        return {}
    with open(p) as f:
        return {str(r["rung"]): r["score"] for r in json.load(f)}


def cmd_add(a):
    board = load(a.board)
    with open(a.diff) as f:
        d = json.load(f)
    row = {
        "cycle": a.cycle, "date": time.strftime("%Y-%m-%d"), "run": a.run, "darling": a.darling,
        "patches": a.patches, "targeted": a.targeted,
        "probes": {"counted": d["counted"], "same": d["same"]},
        "closed_by_queue": len(d.get("closed_by_queue", [])), "broken_by_queue": len(d.get("broken_by_queue", [])),
        "ladder": ladder_scores(a.ladder), "looked": {},
    }
    if a.diff_upstream:
        with open(a.diff_upstream) as f:
            u = json.load(f)
        row["upstream_probes"] = {"counted": u["counted"], "same": u["same"]}
    if a.ladder_upstream:
        row["upstream_ladder"] = ladder_scores(a.ladder_upstream)
    board["cycles"] = [c for c in board["cycles"] if c["cycle"] != a.cycle] + [row]
    with open(a.board, "w") as f:
        json.dump(board, f, indent=1, sort_keys=True)


def reached(row, rung, key="ladder"):
    """A rung counts as reached when it scored 4 and a person confirmed it."""
    s = row.get(key, {}).get(str(rung))
    return s == 4 and (key != "ladder" or row.get("looked", {}).get(str(rung), {}).get("usable") is True)


def verdict(board):
    rows = [c for c in board["cycles"] if c["cycle"] != "c00"]
    base = next((c for c in board["cycles"] if c["cycle"] == "c00"), None)
    if base and reached(base, TARGET_RUNG, "upstream_ladder"):
        return "SUPERSEDED: upstream Darling already reaches rung %d without TUR's queue (cycle 0)" % TARGET_RUNG
    if not rows:
        return "NOT STARTED: no fix cycle measured yet"
    targeted = sum(c.get("targeted", 0) for c in rows)
    closed = sum(c.get("closed_by_queue", 0) for c in rows)
    best_queue = max((r for r in range(1, 11) if any(reached(c, r) and not reached(c, r, "upstream_ladder") for c in rows)),
                     default=0)
    weeks = (time.mktime(time.strptime(rows[-1]["date"], "%Y-%m-%d")) -
             time.mktime(time.strptime(board["registered"], "%Y-%m-%d"))) / (7 * 86400)
    finished = len(rows) >= MAX_CYCLES or weeks >= MAX_WEEKS
    yield_ = closed / targeted if targeted else 0.0
    if best_queue >= TARGET_RUNG and yield_ >= MIN_FIX_YIELD:
        return "SUPPORTED: rung %d reached because of TUR's queue; fix yield %.0f%%" % (best_queue, 100 * yield_)
    if not finished:
        return "RUNNING: %d of %d cycles, %.1f of %d weeks; best rung from the queue %d; fix yield %.0f%%" % (
            len(rows), MAX_CYCLES, weeks, MAX_WEEKS, best_queue, 100 * yield_)
    if best_queue < KILL_RUNG:
        return "KILLED (K1): after %d cycles rung %d does not show a usable window with TUR's queue" % (len(rows), KILL_RUNG)
    if yield_ < MIN_FIX_YIELD:
        return "KILLED (K2): fix yield %.0f%% < %.0f%%" % (100 * yield_, 100 * MIN_FIX_YIELD)
    return "PARTIAL: rung %d reached because of TUR's queue (target %d)" % (best_queue, TARGET_RUNG)


def cmd_render(a):
    board = load(a.board)
    out = ["# macfw scoreboard", "", "Verdict: **%s**" % verdict(board), "",
           "| Cycle | Date | Run | Darling | Patches | Probes same (queue) | Probes same (upstream) | Closed | Broken | Ladder with queue | Ladder upstream |",
           "|---|---|---|---|---|---|---|---|---|---|---|"]
    for c in board["cycles"]:
        p, u = c["probes"], c.get("upstream_probes")
        fmt = lambda s: " ".join("%s:%s" % (k, v) for k, v in sorted(s.items(), key=lambda x: int(x[0]))) or "-"
        out.append("| %s | %s | %s | `%s` | %s | %d/%d | %s | %d | %d | %s | %s |" % (
            c["cycle"], c["date"], c["run"], c["darling"][:9], c["patches"], p["same"], p["counted"],
            "%d/%d" % (u["same"], u["counted"]) if u else "-", c["closed_by_queue"], c["broken_by_queue"],
            fmt(c.get("ladder", {})), fmt(c.get("upstream_ladder", {}))))
    print("\n".join(out))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    ad = sub.add_parser("add")
    ad.add_argument("--board", required=True)
    ad.add_argument("--cycle", required=True)
    ad.add_argument("--run", required=True)
    ad.add_argument("--diff", required=True)
    ad.add_argument("--diff-upstream")
    ad.add_argument("--ladder")
    ad.add_argument("--ladder-upstream")
    ad.add_argument("--darling", required=True)
    ad.add_argument("--patches", type=int, default=0)
    ad.add_argument("--targeted", type=int, default=0, help="differences the queue's patches name as their target")
    rd = sub.add_parser("render")
    rd.add_argument("--board", required=True)
    a = ap.parse_args()
    {"add": cmd_add, "render": cmd_render}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
