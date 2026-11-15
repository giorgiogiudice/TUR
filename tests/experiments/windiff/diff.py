#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Compares what the windiff checks observed on Windows with what they
observed under each Wine.

    diff.py --ref windows=windows.json --target fedora=a.json --target master=b.json
            [--target patched=c.json] [--base master --fixed patched]
            [--triage triage.tsv] --out DIR

Every check of the reference is put in one class per target:

  same             the same value
  different        another value
  missing-export   the check found the function missing in the target
  crash            the target's program ended before it reached the check
  timeout          the target's program was stopped before it reached the check
  not-emitted      the group finished but did not report the check
  unstable         the value changed between two runs on one machine: not compared
  after-divergence a later chunk of a generated sequence that already differs
                   (the state is different from there on): not counted

A check the reference does not have (the Windows run crashed, or the check
is new) is listed as no-reference and not counted.

The triage file (tab-separated: id pattern, cause, status, note) says what a
difference is: several checks often have one cause. Status is one of
  known       Wine's own tests or bug tracker already record it
  new         not recorded there as far as this spike could find
  irrelevant  real, but nothing an application depends on
  artifact    produced by the test set-up, not by Wine
Patterns are shell-style and the first match wins; a difference no pattern
matches is untriaged.

Writes DIR/diff.json, DIR/summary.md and DIR/differences.md.
"""

from __future__ import annotations

import argparse
import collections
import fnmatch
import json
import pathlib
import re
import sys

CLASSES = ["same", "different", "missing-export", "crash", "timeout", "not-emitted", "unstable", "after-divergence"]
FOUND = ("different", "missing-export", "crash", "timeout", "not-emitted")
SEQ = re.compile(r"^(fuzz\.seq-\d+)\.(steps-\d+|listing-at-end)$")


def load(spec: str):
    label, _, path = spec.partition("=")
    return label, json.loads(pathlib.Path(path).read_text(encoding="utf-8"))


def group_of(check_id: str, groups) -> str:
    parts = check_id.split(".")
    return ".".join(parts[:2]) if len(parts) >= 2 else check_id


def classify(ref, target):
    out = {}
    diverged = set()
    for check_id in sorted(ref["checks"]):
        r = ref["checks"][check_id]
        t = target["checks"].get(check_id)
        seq = SEQ.match(check_id)
        if r.get("unstable") or (t and t.get("unstable")):
            cls = "unstable"
        elif t is None:
            status = target["groups"].get(group_of(check_id, target["groups"]), {}).get("status", "missing")
            cls = {"crash": "crash", "timeout": "timeout"}.get(status, "not-emitted")
        elif t["v"] == r["v"]:
            cls = "same"
        elif t["v"] == "missing-export":
            cls = "missing-export"
        else:
            cls = "different"
        if seq:
            if seq.group(1) in diverged:
                cls = "after-divergence"
            elif cls != "same" and cls != "unstable":
                diverged.add(seq.group(1))
        out[check_id] = cls
    return out


def load_triage(path):
    rules = []
    if not path or not pathlib.Path(path).exists():
        return rules
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        fields = [f.strip() for f in line.split("\t")]
        if len(fields) < 3:
            continue
        rules.append({"pattern": fields[0], "cause": fields[1], "status": fields[2],
                      "note": fields[3] if len(fields) > 3 else ""})
    return rules


def triage_of(check_id, rules):
    for rule in rules:
        if fnmatch.fnmatchcase(check_id, rule["pattern"]):
            return rule
    return {"pattern": "", "cause": "untriaged: " + ".".join(check_id.split(".")[:2]), "status": "untriaged", "note": ""}


def cell(text: str, limit: int = 110) -> str:
    text = text.replace("|", "\\|").replace("\n", " ").replace("`", "'")
    return text if len(text) <= limit else text[:limit] + "…"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", required=True)
    ap.add_argument("--target", action="append", default=[])
    ap.add_argument("--base")
    ap.add_argument("--fixed")
    ap.add_argument("--triage")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    ref_label, ref = load(args.ref)
    targets = [load(spec) for spec in args.target]
    rules = load_triage(args.triage)
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    classes = {label: classify(ref, data) for label, data in targets}
    lines = ["## windiff: differences from Windows", ""]
    lines += ["| Machine | Checks reported | Groups | Groups not finished |", "|---|---|---|---|"]
    for label, data in [(ref_label, ref)] + targets:
        bad = [g for g, info in data["groups"].items() if info["status"] != "ok"]
        lines.append(f"| {label} | {len(data['checks'])} | {len(data['groups'])} | {', '.join(sorted(bad)) or 'none'} |")
    lines.append("")

    ref_unstable = sum(1 for c in ref["checks"].values() if c.get("unstable"))
    lines.append(f"Reference: {len(ref['checks'])} checks, {ref_unstable} unstable on the reference itself.")
    lines.append("")
    lines += ["| Target | " + " | ".join(CLASSES) + " | compared | differing |", "|---|" + "---|" * (len(CLASSES) + 2)]
    totals = {}
    for label, _data in targets:
        count = collections.Counter(classes[label].values())
        compared = sum(count[c] for c in ("same",) + FOUND)
        differing = sum(count[c] for c in FOUND)
        totals[label] = {"counts": dict(count), "compared": compared, "differing": differing}
        lines.append(f"| {label} | " + " | ".join(str(count.get(c, 0)) for c in CLASSES) + f" | {compared} | {differing} |")
    lines.append("")

    # per group
    groups = sorted({group_of(c, None) for c in ref["checks"]})
    lines += ["### By group", "", "| Group | Checks | " + " | ".join(f"{label}: differing" for label, _ in targets) + " |",
              "|---|---|" + "---|" * len(targets)]
    for group in groups:
        ids = [c for c in ref["checks"] if group_of(c, None) == group]
        row = [group, str(len(ids))]
        for label, _ in targets:
            row.append(str(sum(1 for c in ids if classes[label][c] in FOUND)))
        lines.append("| " + " | ".join(row) + " |")
    lines.append("")

    # triage: by cause, for the differences of each target
    triage_table = {}
    for label, _ in targets:
        by_status = collections.defaultdict(lambda: {"checks": 0, "causes": set()})
        for check_id, cls in classes[label].items():
            if cls not in FOUND:
                continue
            rule = triage_of(check_id, rules)
            by_status[rule["status"]]["checks"] += 1
            by_status[rule["status"]]["causes"].add(rule["cause"])
        triage_table[label] = {s: {"checks": v["checks"], "causes": sorted(v["causes"])} for s, v in by_status.items()}
    statuses = ["new", "known", "irrelevant", "artifact", "untriaged"]
    lines += ["### Triage (differing checks / distinct causes)", "",
              "| Target | " + " | ".join(statuses) + " |", "|---|" + "---|" * len(statuses)]
    for label, _ in targets:
        row = []
        for status in statuses:
            entry = triage_table[label].get(status, {"checks": 0, "causes": []})
            row.append(f"{entry['checks']} / {len(entry['causes'])}")
        lines.append(f"| {label} | " + " | ".join(row) + " |")
    lines.append("")

    # what the patch set changed
    fixed, regressed = [], []
    if args.base and args.fixed and args.base in classes and args.fixed in classes:
        for check_id in sorted(ref["checks"]):
            before, after = classes[args.base][check_id], classes[args.fixed][check_id]
            if before in FOUND and after == "same":
                fixed.append(check_id)
            elif before == "same" and after in FOUND:
                regressed.append(check_id)
        by_cause = collections.defaultdict(list)
        for check_id in fixed:
            by_cause[triage_of(check_id, rules)["cause"]].append(check_id)
        lines += [f"### Patch set: {args.base} to {args.fixed}", "",
                  f"- checks that differed on {args.base} and match Windows on {args.fixed}: **{len(fixed)}** "
                  f"({len(by_cause)} causes)",
                  f"- checks that matched on {args.base} and differ on {args.fixed} (regressions): **{len(regressed)}**", ""]
        if by_cause:
            lines += ["| Cause | Checks now matching |", "|---|---|"]
            for cause in sorted(by_cause):
                lines.append(f"| {cell(cause)} | {len(by_cause[cause])} |")
            lines.append("")
        for check_id in regressed:
            lines.append(f"- regression: `{check_id}`")
        if regressed:
            lines.append("")

    (out / "summary.md").write_text("\n".join(lines) + "\n", encoding="utf-8")

    # the full list
    detail = ["## windiff: every difference", ""]
    records = []
    for check_id in sorted(ref["checks"]):
        row_classes = {label: classes[label][check_id] for label, _ in targets}
        if not any(cls in FOUND or cls == "after-divergence" for cls in row_classes.values()):
            continue
        rule = triage_of(check_id, rules)
        record = {"id": check_id, "reference": ref["checks"][check_id]["v"], "status": rule["status"],
                  "cause": rule["cause"], "note": rule["note"], "targets": {}}
        for label, data in targets:
            value = data["checks"].get(check_id, {}).get("v")
            record["targets"][label] = {"class": row_classes[label], "v": value}
        records.append(record)
    by_cause = collections.defaultdict(list)
    for record in records:
        by_cause[(record["status"], record["cause"])].append(record)
    for (status, cause) in sorted(by_cause):
        items = by_cause[(status, cause)]
        detail += [f"### [{status}] {cause} ({len(items)} checks)", ""]
        if items[0]["note"]:
            detail += [items[0]["note"], ""]
        detail += ["| Check | " + ref_label + " | " + " | ".join(label for label, _ in targets) + " |",
                   "|---|---|" + "---|" * len(targets)]
        for record in items[:60]:
            row = [f"`{record['id']}`", cell(record["reference"])]
            for label, _ in targets:
                t = record["targets"][label]
                if t["class"] == "same":
                    row.append("same")
                elif t["v"] is None:
                    row.append(f"({t['class']})")
                else:
                    row.append(("" if t["class"] == "different" else f"({t['class']}) ") + cell(t["v"]))
            detail.append("| " + " | ".join(row) + " |")
        if len(items) > 60:
            detail.append(f"| … {len(items) - 60} more, in diff.json | | " + " | " * len(targets))
        detail.append("")
    no_ref = {label: sorted(set(data["checks"]) - set(ref["checks"])) for label, data in targets}
    if any(no_ref.values()):
        detail += ["### Checks with no reference value", ""]
        for label, ids in no_ref.items():
            detail.append(f"- {label}: {len(ids)}" + (": " + ", ".join(f"`{i}`" for i in ids[:20]) if ids else ""))
        detail.append("")
    (out / "differences.md").write_text("\n".join(detail) + "\n", encoding="utf-8")

    (out / "diff.json").write_text(json.dumps({
        "reference": {"label": ref_label, "platform": ref.get("platform"), "checks": len(ref["checks"]),
                      "groups": ref["groups"]},
        "targets": {label: {"platform": data.get("platform"), "groups": data["groups"], **totals[label],
                            "triage": triage_table[label]} for label, data in targets},
        "fixed": fixed, "regressed": regressed, "differences": records,
    }, indent=1, ensure_ascii=False), encoding="utf-8")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
