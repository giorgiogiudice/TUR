#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Compare a candidate's probe results (Darling) with the oracle's (macOS).

    diff.py --oracle results-macos-x86_64.json [--oracle-other results-macos-arm64.json]
            --candidate results-darling-x86_64.json
            [--before results-darling-x86_64-upstream.json]
            [--triage triage.tsv] --out DIR

Every oracle check gets one class:

  same           equal (exact), or within tolerance (num:T)
  different      both sides ran the check and the values differ
  unimplemented  the candidate raised where the oracle did not (very often a
                 missing method: "!exception NSInvalidArgumentException")
  missing        the candidate's group ran to the end without this check
  group-<st>     the candidate's group did not run to the end: crash,
                 timeout, load-fail, no-done, exit:N
  metric         a font-, theme- or screen-dependent value more than 10% off:
                 listed, never counted as a difference
  ignored        env checks, and checks unstable on either side, or whose
                 value differs between the oracle's two architectures
                 (when --oracle-other is given; listed separately)

With --before (the same candidate without TUR's patch queue, at the same
Darling commit) it also says which checks the queue closed and which it
broke: the fix loop's evidence.

triage.tsv groups differences into causes: one line per cause,
`cause<TAB>status<TAB>regex over check ids<TAB>note`, status one of open,
fixed, known (Darling already tracks it), irrelevant, artifact.
"""
import argparse
import json
import os
import re
from collections import Counter, defaultdict

FAIL_CLASSES = ("different", "unimplemented", "missing")


def load(p):
    with open(p) as f:
        return json.load(f)


def num_close(a, b, tol):
    try:
        xs = [float(x) for x in re.split(r"[ ,]+", a.strip())]
        ys = [float(y) for y in re.split(r"[ ,]+", b.strip())]
    except ValueError:
        return a == b
    return len(xs) == len(ys) and all(abs(x - y) <= tol for x, y in zip(xs, ys))


def metric_close(a, b):
    try:
        xs = [float(x) for x in re.split(r"[ ,]+", a.strip())]
        ys = [float(y) for y in re.split(r"[ ,]+", b.strip())]
    except ValueError:
        return a == b
    return len(xs) == len(ys) and all(abs(x - y) <= max(1.0, 0.1 * abs(x)) for x, y in zip(xs, ys))


def classify(oracle, cand, other=None):
    out = {}
    for gkey, og in oracle["groups"].items():
        if og["status"] != "ok":
            for cid in og["checks"]:
                out[cid] = ("ignored", "oracle group " + og["status"], None, None)
            continue
        cg = cand["groups"].get(gkey)
        unstable = set(og.get("unstable", [])) | set((cg or {}).get("unstable", []))
        for cid, ocheck in og["checks"].items():
            kind, ov = ocheck["k"], ocheck["v"]
            if kind == "env" or cid in unstable:
                out[cid] = ("ignored", "env" if kind == "env" else "unstable", ov, None)
                continue
            if other is not None:
                oo = other["groups"].get(gkey, {}).get("checks", {}).get(cid)
                if oo is not None and oo["v"] != ov and kind == "exact":
                    out[cid] = ("ignored", "differs between oracle architectures", ov, oo["v"])
                    continue
            if cg is None:
                out[cid] = ("group-not-run", None, ov, None)
                continue
            cc = cg["checks"].get(cid)
            if cc is None:
                cls = "missing" if cg["status"] == "ok" else "group-" + cg["status"]
                out[cid] = (cls, cg.get("detail"), ov, None)
                continue
            cv = cc["v"]
            if kind == "exact":
                same = cv == ov
            elif kind.startswith("num:"):
                same = num_close(ov, cv, float(kind[4:]))
            elif kind == "metric":
                out[cid] = ("same" if metric_close(ov, cv) else "metric", None, ov, cv)
                continue
            else:
                same = cv == ov
            if same:
                out[cid] = ("same", None, ov, cv)
            elif str(cv).startswith("!exception") and not str(ov).startswith("!exception"):
                out[cid] = ("unimplemented", None, ov, cv)
            else:
                out[cid] = ("different", None, ov, cv)
    return out


def read_triage(p):
    causes = []
    if p and os.path.exists(p):
        with open(p) as f:
            for line in f:
                if line.startswith("#") or not line.strip():
                    continue
                parts = line.rstrip("\n").split("\t")
                if len(parts) >= 3:
                    causes.append((parts[0], parts[1], re.compile(parts[2]), parts[3] if len(parts) > 3 else ""))
    return causes


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--oracle", required=True)
    ap.add_argument("--oracle-other")
    ap.add_argument("--candidate", required=True)
    ap.add_argument("--before")
    ap.add_argument("--triage")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()

    oracle, cand = load(a.oracle), load(a.candidate)
    other = load(a.oracle_other) if a.oracle_other else None
    cls = classify(oracle, cand, other)
    before = classify(oracle, load(a.before), other) if a.before else None

    counts = Counter(c[0] for c in cls.values())
    per_group = defaultdict(Counter)
    for cid, c in cls.items():
        per_group[".".join(cid.split(".")[:2])][c[0]] += 1
    counted = sum(n for k, n in counts.items() if k not in ("ignored", "metric"))
    passed = counts.get("same", 0)

    causes = read_triage(a.triage)
    cause_of = {}
    for cid, c in cls.items():
        if c[0] in FAIL_CLASSES or c[0].startswith("group-"):
            for name, status, rx, _ in causes:
                if rx.search(cid):
                    cause_of[cid] = (name, status)
                    break

    closed, broken = [], []
    if before:
        for cid, c in cls.items():
            b = before.get(cid)
            if not b:
                continue
            if b[0] != "same" and c[0] == "same":
                closed.append(cid)
            elif b[0] == "same" and c[0] != "same" and c[0] not in ("ignored", "metric"):
                broken.append(cid)

    os.makedirs(a.out, exist_ok=True)
    with open(os.path.join(a.out, "diff.json"), "w") as f:
        json.dump({
            "oracle": {"side": oracle["side"], "arch": oracle["arch"]},
            "candidate": {"side": cand["side"], "arch": cand["arch"]},
            "counts": counts, "counted": counted, "same": passed,
            "checks": {cid: {"class": c[0], "detail": c[1], "oracle": c[2], "candidate": c[3],
                             "cause": cause_of.get(cid, (None, None))[0]} for cid, c in sorted(cls.items())},
            "closed_by_queue": sorted(closed), "broken_by_queue": sorted(broken),
        }, f, indent=1, sort_keys=True)

    lines = ["# macfw probe differences: %s %s against %s %s" % (cand["side"], cand["arch"], oracle["side"], oracle["arch"]), "",
             "Counted checks: **%d**, same: **%d** (%.1f%%)." % (counted, passed, 100.0 * passed / counted if counted else 0), "",
             "| Class | Checks |", "|---|---|"]
    lines += ["| %s | %d |" % (k, n) for k, n in sorted(counts.items(), key=lambda x: -x[1])]
    lines += ["", "| Group | same | different | unimplemented | missing | not run to the end | metric |", "|---|---|---|---|---|---|---|"]
    for g, c in sorted(per_group.items()):
        notrun = sum(n for k, n in c.items() if k.startswith("group-"))
        lines.append("| %s | %d | %d | %d | %d | %d | %d |" % (g, c["same"], c["different"], c["unimplemented"],
                                                              c["missing"], notrun, c["metric"]))
    if before:
        lines += ["", "Against the same Darling without TUR's queue: **%d** checks closed, **%d** broken." % (len(closed), len(broken))]
        lines += ["", "Broken:"] + ["- `%s`" % b for b in broken[:200]] if broken else []
    open_causes = Counter(v[0] for v in cause_of.values())
    untriaged = [cid for cid, c in cls.items() if (c[0] in FAIL_CLASSES or c[0].startswith("group-")) and cid not in cause_of]
    lines += ["", "Causes (triage.tsv): %d named; %d failing checks not yet triaged." % (len(open_causes), len(untriaged))]
    lines += ["", "## Differences", "", "| Check | Class | macOS | Darling |", "|---|---|---|---|"]
    for cid, c in sorted(cls.items()):
        if c[0] in FAIL_CLASSES or c[0] == "metric":
            lines.append("| `%s` | %s | `%s` | `%s` |" % (cid, c[0], str(c[2])[:120].replace("|", "\\|"),
                                                        str(c[3])[:120].replace("|", "\\|")))
    with open(os.path.join(a.out, "differences.md"), "w") as f:
        f.write("\n".join(lines) + "\n")
    print("\n".join(lines[:12 + len(counts)]))


if __name__ == "__main__":
    main()
