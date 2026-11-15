#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What Wine's own tests already record about a function.

    todo_index.py WINE_TREE FUNCTION [FUNCTION...]     lines of dlls/*/tests that call the function,
                                                       and whether a todo_wine or broken() is near
    todo_index.py WINE_TREE --summary                  todo_wine lines per test file

Used when a difference is triaged: a difference is "known" when a test of
Wine's already records it. A `todo_wine` marks behaviour Wine's developers
know is wrong; a `broken()` marks a value some Windows version returns and
Wine does not imitate. This only finds candidates; a person (or the model
doing the triage) reads the lines.
"""

from __future__ import annotations

import collections
import pathlib
import re
import sys


def main() -> int:
    tree = pathlib.Path(sys.argv[1])
    files = sorted(tree.glob("dlls/*/tests/*.c"))
    if sys.argv[2] == "--summary":
        counts = collections.Counter()
        for path in files:
            counts[str(path.relative_to(tree))] = len(re.findall(r"\btodo_wine", path.read_text(errors="replace")))
        print(f"todo_wine lines in {len(files)} test files: {sum(counts.values())}")
        for name, count in counts.most_common(40):
            print(f"{count:6d}  {name}")
        return 0
    for function in sys.argv[2:]:
        pattern = re.compile(r"\b" + re.escape(function) + r"[AW]?\s*\(")
        total = near = 0
        print(f"== {function}")
        for path in files:
            lines = path.read_text(errors="replace").splitlines()
            for number, line in enumerate(lines):
                if not pattern.search(line):
                    continue
                total += 1
                window = lines[max(0, number - 1):number + 6]
                marks = sorted({m for m in ("todo_wine", "broken(") if any(m in w for w in window)})
                if marks:
                    near += 1
                    print(f"  {path.relative_to(tree)}:{number + 1}: [{', '.join(marks)}] {line.strip()[:140]}")
        print(f"  {total} calls in Wine's tests, {near} with a todo_wine or broken() within six lines")
    return 0


if __name__ == "__main__":
    sys.exit(main())
