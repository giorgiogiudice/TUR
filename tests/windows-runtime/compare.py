#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Puts passes of apps.py side by side: the real applications installed with
and without case-insensitive prefixes (issue #81).

    compare.py --pass LABEL=EVIDENCE_DIR [--pass ...] --out DIR

Each EVIDENCE_DIR is the --evidence directory of one apps.py pass; its
results.json is read. LABEL names the mode, and passes with the same label
are taken as repeats of that mode. Writes DIR/casefold-apps.md and
DIR/casefold-apps.json. It only reads and tabulates; nothing is judged here
beyond what apps.py already recorded.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import statistics
import sys

STAGES = ("install", "launch", "check", "uninstall")


def number(result: dict, stage: str) -> float | None:
    entry = result.get(stage, {})
    value = entry.get("seconds_to_window") if stage == "launch" else entry.get("seconds")
    return float(value) if isinstance(value, (int, float)) else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pass", dest="passes", action="append", required=True)
    parser.add_argument("--out", required=True, type=pathlib.Path)
    arguments = parser.parse_args()

    passes = []
    for text in arguments.passes:
        label, _, directory = text.partition("=")
        path = pathlib.Path(directory) / "results.json"
        document = json.loads(path.read_text()) if path.is_file() else {"results": [], "missing": True}
        passes.append({"label": label, "directory": directory, "document": document})
    labels = list(dict.fromkeys(entry["label"] for entry in passes))

    applications: list[str] = []
    names: dict[str, str] = {}
    for entry in passes:
        for result in entry["document"].get("results", []):
            if result["id"] not in applications:
                applications.append(result["id"])
                names[result["id"]] = f"{result.get('name')} {result.get('version')} ({result.get('installer_kind')})"

    def results_for(label: str, app: str) -> list[dict]:
        return [result for entry in passes if entry["label"] == label
                for result in entry["document"].get("results", []) if result["id"] == app]

    lines = ["### Real applications with and without case-insensitive prefixes", "",
             "Every pass ran on the same loop-mounted ext4 file system made with the casefold feature, on one "
             "runner, one after the other in the order " + ", ".join(entry["label"] for entry in passes) + ". "
             "Times are wall-clock seconds, one value per pass in that order.", "",
             "| Application | Mode | drive_c case-insensitive | Install (s) | First window (s) | Check | "
             "Uninstall (s) | Left behind |", "|---|---|---|---|---|---|---|---|"]
    summary: dict = {}
    table = []
    for app in applications:
        for label in labels:
            found = results_for(label, app)
            if not found:
                continue
            row = {"application": app, "mode": label, "passes": []}
            for result in found:
                row["passes"].append({stage: {"status": result.get(stage, {}).get("status"),
                                              "seconds": number(result, stage)} for stage in STAGES}
                                     | {"case_folding": result.get("case_folding"),
                                        "left": result.get("uninstall", {}).get("left", [])})
            table.append(row)

            def cell(stage: str, rows: list[dict] = row["passes"]) -> str:
                return " / ".join(
                    (f"{entry[stage]['seconds']:.1f}" if entry[stage]["seconds"] is not None else "-")
                    + ("" if entry[stage]["status"] == "pass" else f" {entry[stage]['status']}")
                    for entry in rows)

            folded = " / ".join(str((entry["case_folding"] or {}).get("lsattr_casefolded")) for entry in row["passes"])
            checks = " / ".join(str(entry["check"]["status"]) for entry in row["passes"])
            left = " / ".join(str(len(entry["left"])) for entry in row["passes"])
            lines.append(f"| {names[app]} | {label} | {folded} | {cell('install')} | {cell('launch')} | {checks} | "
                         f"{cell('uninstall')} | {left} |")
            for stage in ("install", "launch", "uninstall"):
                for entry in row["passes"]:
                    if entry[stage]["seconds"] is not None:
                        summary.setdefault(label, {}).setdefault(stage, []).append(entry[stage]["seconds"])
            for entry in row["passes"]:
                summary.setdefault(label, {}).setdefault("failed", 0)
                summary[label]["failed"] += sum(1 for stage in STAGES if entry[stage]["status"] != "pass")

    lines += ["", "| Mode | Steps not passed | Install, sum of all passes (s) | Install, median (s) | "
                  "First window, median (s) | Uninstall, median (s) |", "|---|---|---|---|---|---|"]
    for label in labels:
        values = summary.get(label, {})
        runs = sum(1 for entry in passes if entry["label"] == label)

        def median(stage: str) -> str:
            return f"{statistics.median(values[stage]):.1f}" if values.get(stage) else "-"

        total = sum(values.get("install", []))
        lines.append(f"| {label} ({runs} passes) | {values.get('failed', 0)} | {total:.1f} | {median('install')} | "
                     f"{median('launch')} | {median('uninstall')} |")
    lines += ["", "Left behind: how many things apps.py found after the removal (none is 0). A step that did not "
                  "pass is marked after its time.", ""]
    for entry in passes:
        differences = entry["document"].get("differences") or []
        if entry["document"].get("missing"):
            lines.append(f"- {entry['label']} ({entry['directory']}): no results.json")
        elif differences:
            lines.append(f"- {entry['label']} ({entry['directory']}): " + "; ".join(differences))
    arguments.out.mkdir(parents=True, exist_ok=True)
    (arguments.out / "casefold-apps.md").write_text("\n".join(lines) + "\n")
    (arguments.out / "casefold-apps.json").write_text(json.dumps(
        {"passes": [{"label": entry["label"], "directory": entry["directory"]} for entry in passes],
         "rows": table, "summary": summary}, indent=2))
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
