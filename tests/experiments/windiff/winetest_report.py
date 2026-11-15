#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Reads the output of Wine conformance test programs and says what failed,
and whether a failure is in a line the fix queue added.

    winetest_report.py DIR LABEL

DIR holds added.diff (git diff -U0 of the test sources the queue changed),
tests.txt (lines "PROGRAM TEST SOURCE") and one PROGRAM.TEST.LABEL.txt per
test: what the program printed when run on the machine LABEL names. Prints
a Markdown table and writes DIR/report-LABEL.json.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys


def added_ranges(diff_text: str) -> dict[str, list[tuple[int, int]]]:
    ranges: dict[str, list[tuple[int, int]]] = {}
    current = None
    for line in diff_text.splitlines():
        if line.startswith("+++ b/"):
            current = line[6:].strip()
            ranges.setdefault(current, [])
        match = re.match(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", line)
        if match and current:
            start, count = int(match.group(1)), int(match.group(2) or "1")
            if count:
                ranges[current].append((start, start + count - 1))
    return ranges


def main() -> int:
    folder, label = pathlib.Path(sys.argv[1]), sys.argv[2]
    ranges = added_ranges((folder / "added.diff").read_text(errors="replace")) if (folder / "added.diff").exists() else {}
    report = []
    print(f"### Wine conformance tests on {label}")
    print()
    print("| Test | Added lines | Summary | Failures | of them in added lines | Todo successes in added lines |")
    print("|---|---|---|---|---|---|")
    tests = (folder / "tests.txt").read_text().splitlines() if (folder / "tests.txt").exists() else []
    for entry in tests:
        if not entry.strip():
            continue
        exe, name, source = entry.split()
        path = folder / f"{exe}.{name}.{label}.txt"
        mine = ranges.get(source, [])
        added = sum(end - start + 1 for start, end in mine)
        if not path.exists():
            print(f"| {exe} {name} | {added} | NOT RUN | | | |")
            report.append({"test": f"{exe} {name}", "ran": False})
            continue
        text = path.read_text(errors="replace")
        basename = pathlib.Path(source).name
        failures, in_added, todo_ok = [], [], []
        for line in text.splitlines():
            match = re.match(rf"^(?:[0-9a-f]+:)?{re.escape(basename)}:(\d+): (Test failed|Test succeeded inside todo block)", line)
            if not match:
                continue
            number = int(match.group(1))
            inside = any(start <= number <= end for start, end in mine)
            if match.group(2) == "Test failed":
                failures.append(line.strip())
                if inside:
                    in_added.append(line.strip())
            elif inside:
                todo_ok.append(line.strip())
        summary = re.findall(r"\d+ tests executed \(\d+ marked as todo, \d+ failures?\), \d+ skipped\.", text)
        print(f"| {exe} {name} | {added} | {summary[-1] if summary else 'no summary line (crash or timeout)'} | "
              f"{len(failures)} | {len(in_added)} | {len(todo_ok)} |")
        report.append({"test": f"{exe} {name}", "ran": True, "summary": summary[-1] if summary else None,
                       "failures": failures[:200], "failures_in_added_lines": in_added, "todo_successes_in_added_lines": todo_ok,
                       "added_lines": added})
    print()
    for item in report:
        for line in item.get("failures_in_added_lines", [])[:20]:
            print(f"- {item['test']}: `{line[:300]}`")
    (folder / f"report-{label}.json").write_text(json.dumps(report, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
