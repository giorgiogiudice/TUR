#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""List the packages in a TUR image SBOM with their declared licences.

    scripts/sbom-licences.py SBOM.spdx.json > docs/dependencies/image-licences.csv

Reads the SPDX 2.3 JSON document written by scripts/sbom.sh and writes CSV to
standard output with the columns: package, version, licence.

The licence column is the expression the package itself declares (the RPM
License tag). Two things the SBOM tool does to that tag are undone so that the
column matches `rpm -qa --qf '%{LICENSE}'` and licences.txt:

- An expression the tool could not parse as SPDX is replaced in the SBOM by a
  generated "LicenseRef-..." identifier. The original text is kept in the
  document's hasExtractedLicensingInfos table and is restored from there.
- A compound expression is wrapped in one extra pair of parentheses, which is
  removed.

Nothing else is normalised: legacy and non-SPDX tags appear as the package
declares them, because they are findings in their own right.

One difference from licences.txt remains and cannot be undone from the SBOM:
where a whole tag is a single identifier the SBOM tool recognises, the tool
writes its canonical SPDX spelling ("zlib" becomes "Zlib", the deprecated
"LGPL-2.1" becomes "LGPL-2.1-only"). The CSV therefore has a few fewer
distinct expressions than licences.txt; the package count is the same.
"""

from __future__ import annotations

import csv
import json
import sys


def strip_outer_parentheses(expression: str) -> str:
    """Remove one pair of parentheses that encloses the whole expression."""
    if not (expression.startswith("(") and expression.endswith(")")):
        return expression
    depth = 0
    for index, char in enumerate(expression):
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0 and index != len(expression) - 1:
                return expression  # the first "(" closes before the end
    return expression[1:-1].strip() if depth == 0 else expression


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    with open(argv[1], encoding="utf-8") as handle:
        document = json.load(handle)

    # Generated identifier -> the licence text the package really declares.
    original = {
        entry["licenseId"]: entry["name"]
        for entry in document.get("hasExtractedLicensingInfos", [])
        if entry.get("licenseId") and entry.get("name")
    }

    rows = []
    for package in document.get("packages", []):
        if package.get("SPDXID", "").startswith("SPDXRef-DocumentRoot"):
            continue  # the scanned directory itself, not a package
        declared = package.get("licenseDeclared") or "NOASSERTION"
        licence = original.get(declared) or strip_outer_parentheses(declared)
        rows.append((package.get("name", ""), package.get("versionInfo", ""), licence))

    rows.sort(key=lambda row: (row[0].lower(), row[0], row[1]))
    writer = csv.writer(sys.stdout, lineterminator="\n")
    writer.writerow(("package", "version", "licence"))
    writer.writerows(rows)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
