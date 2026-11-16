#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Checks the image's list of fixed codes before it ships.

    scripts/check-fixes.py [image/rootfs/usr/share/tur/heal/fixes.json]

A pull request that fixes a reported problem adds the problem's code to this
file (docs/guides/fix-a-reported-problem.md). After the update, a TUR
computer tells the person who reported it that it is fixed, so a wrong entry
tells someone something untrue. Checked here, by the rules of
schemas/heal-fixes.schema.json and two more:

  * every code has the form of a TUR code and is listed once;
  * every code is one TUR knows: it appears somewhere in the source tree
    other than this file (the healing catalogue, an error registry, a test).

Prints each problem and exits 1 if there is one. Reads files only.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT = ROOT / "image/rootfs/usr/share/tur/heal/fixes.json"
CODE = re.compile(r"^TUR-[A-Z]+(-[A-Z0-9]+)*-[0-9]{3}$")
TOP = {"schema_version", "about", "fixes"}
ENTRY = {"code", "summary", "change"}


def known_elsewhere(code, path):
    """Whether `code` appears in a tracked file other than `path`."""
    found = subprocess.run(["git", "-C", str(ROOT), "grep", "-l", "-F", code, "--", "."],
                           capture_output=True, text=True)
    files = {line.strip() for line in found.stdout.splitlines() if line.strip()}
    relative = str(path.resolve().relative_to(ROOT)) if path.resolve().is_relative_to(ROOT) else None
    files.discard(relative)
    return bool(files)


def main():
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT
    problems = []
    try:
        document = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        print(f"{path}: {error}")
        return 1
    if not isinstance(document, dict) or set(document) - TOP or not {"schema_version", "fixes"} <= set(document):
        problems.append(f"the fields must be {sorted(TOP)}, with schema_version and fixes")
    elif document["schema_version"] != 1:
        problems.append("schema_version must be 1")
    fixes = document.get("fixes") if isinstance(document, dict) else None
    if not isinstance(fixes, list):
        problems.append("fixes must be a list")
        fixes = []
    seen = set()
    for index, fix in enumerate(fixes):
        where = f"fixes[{index}]"
        if not isinstance(fix, dict) or set(fix) - ENTRY or not {"code", "summary"} <= set(fix):
            problems.append(f"{where}: the fields must be code, summary and optionally change")
            continue
        code = fix["code"]
        if not isinstance(code, str) or not CODE.match(code):
            problems.append(f"{where}: {code!r} is not a TUR code")
            continue
        if code in seen:
            problems.append(f"{where}: {code} is listed twice")
        seen.add(code)
        summary = fix["summary"]
        if not isinstance(summary, str) or not 1 <= len(summary) <= 300:
            problems.append(f"{where}: summary must be 1 to 300 characters")
        change = fix.get("change")
        if change is not None and (not isinstance(change, str) or len(change) > 200):
            problems.append(f"{where}: change must be at most 200 characters")
        if not known_elsewhere(code, path):
            problems.append(f"{where}: {code} appears nowhere else in TUR; is it a real code?")
    for problem in problems:
        print(f"{path}: {problem}")
    if not problems:
        print(f"{path}: {len(fixes)} fixed codes, all well formed and known")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
