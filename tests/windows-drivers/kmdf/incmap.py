#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Case-insensitive #include resolution for building Windows sources on Linux.

Windows file names are case-insensitive and Microsoft's sources rely on it
(wdf.h includes "WdfQueryInterface.h"; the file is wdfqueryinterface.h).
For every #include spelling found under the source trees that names no file
with exactly that spelling, this finds the file the WDK's compiler would open
on Windows -- the first directory of the include path holding a file of that
name, ignoring case -- and makes a symbolic link to it under LINKDIR with the
spelling the source uses. LINKDIR goes on the compiler's include path.

  incmap.py LINKDIR SOURCE_TREE... -- INCLUDE_DIR...
"""

from __future__ import annotations

import os
import re
import sys

INCLUDE = re.compile(r'^\s*#\s*include\s*[<"]([^>"]+)[>"]', re.M)


def main(argv: list[str]) -> int:
    if "--" not in argv:
        print(__doc__, file=sys.stderr)
        return 2
    sep = argv.index("--")
    linkdir, trees, incdirs = argv[0], argv[1:sep], argv[sep + 1:]
    os.makedirs(linkdir, exist_ok=True)

    wanted: set[str] = set()
    for tree in trees:
        for root, _dirs, files in os.walk(tree):
            for name in files:
                if not name.lower().endswith((".h", ".hpp", ".c", ".cpp", ".inl", ".tmh")):
                    continue
                with open(os.path.join(root, name), encoding="latin-1") as f:
                    wanted.update(m.replace("\\", "/") for m in INCLUDE.findall(f.read()))

    # Index every directory of the include path: lower-cased relative path
    # (up to two levels, for "wil/staging.h") -> real path.
    def index(d: str) -> dict[str, str]:
        found: dict[str, str] = {}
        for root, dirs, files in os.walk(d):
            depth = os.path.relpath(root, d).count(os.sep) + (0 if root == d else 1)
            if depth >= 2:
                dirs[:] = []
            for name in files:
                rel = os.path.relpath(os.path.join(root, name), d)
                found.setdefault(rel.lower(), os.path.join(root, name))
        return found

    indexes = [(d, index(d)) for d in incdirs if os.path.isdir(d)]
    made = missing = 0
    for spelled in sorted(wanted):
        exact = next((os.path.join(d, spelled) for d, _ in indexes if os.path.isfile(os.path.join(d, spelled))), None)
        loose = next((idx[spelled.lower()] for _, idx in indexes if spelled.lower() in idx), None)
        if loose is None:
            missing += 1
            print(f"missing {spelled}")
            continue
        if exact is not None:
            # The compiler finds the exact spelling by itself. (This also
            # keeps a stand-in that wraps a system header with #include_next
            # from being linked under the system header's name.)
            continue
        target = os.path.join(linkdir, spelled)
        if os.path.lexists(target):
            continue
        os.makedirs(os.path.dirname(target), exist_ok=True)
        os.symlink(loose, target)
        made += 1
        print(f"link {spelled} -> {loose}")
    print(f"{made} links, {missing} names found in no include directory")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
