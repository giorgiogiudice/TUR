#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The third-party crates compiled into tur-report-receiver, and their licences.

Reads `cargo metadata` for x86_64 Linux, follows the receiver's normal
(not development, not build-time) dependencies, and writes one line per crate:
name, version and declared licence. Every licence must be permissive (the
list below; an "OR" needs one acceptable choice, an "AND" needs all). The
result must equal the committed docs/dependencies/receiver-crates.csv.

    crates.py MANIFEST_DIR COMMITTED_CSV OUTPUT_CSV
"""

import csv
import io
import json
import re
import subprocess
import sys
from pathlib import Path

PERMISSIVE = {
    "0BSD", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "BSL-1.0",
    "CC0-1.0", "CDLA-Permissive-2.0", "ISC", "MIT", "MIT-0", "Unicode-3.0", "Unicode-DFS-2016", "Unlicense",
    "Zlib",
}


def acceptable(expression):
    """Whether an SPDX expression (or the old a/b form) allows use under a permissive licence."""
    text = expression.replace("(", " ").replace(")", " ")
    alternatives = re.split(r"\s+OR\s+|/", text)
    for alternative in alternatives:
        parts = [part.strip() for part in re.split(r"\s+AND\s+", alternative) if part.strip()]
        if parts and all(part in PERMISSIVE for part in parts):
            return True
    return False


def main():
    manifest_dir, committed, output = (Path(argument) for argument in sys.argv[1:4])
    metadata = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", "x86_64-unknown-linux-gnu"],
        cwd=manifest_dir, check=True, capture_output=True, text=True).stdout)
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    seen, stack = set(), [root]
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        for dependency in nodes[current]["deps"]:
            if any(kind["kind"] is None for kind in dependency["dep_kinds"]):
                stack.append(dependency["pkg"])
    rows, problems = [], []
    for package_id in seen:
        package = packages[package_id]
        if package["source"] is None:
            continue  # TUR's own crates, by path
        licence = package.get("license") or "(none declared)"
        rows.append((package["name"], package["version"], licence))
        if not acceptable(licence):
            problems.append(f"{package['name']} {package['version']}: {licence}")
    rows.sort()
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(["crate", "version", "licence"])
    writer.writerows(rows)
    text = buffer.getvalue()
    output.write_text(text)
    print(f"{len(rows)} third-party crates in tur-report-receiver")
    status = 0
    if problems:
        print("Not permissive:", *problems, sep="\n  ")
        status = 1
    if not committed.exists() or committed.read_text() != text:
        print(f"{committed} differs from what cargo resolves; the computed list is {output}")
        status = 1
    return status


if __name__ == "__main__":
    sys.exit(main())
