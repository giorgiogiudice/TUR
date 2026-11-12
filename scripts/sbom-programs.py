#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Add TUR's own programs, and the crates compiled into them, to the image SBOM.

    scripts/sbom-programs.py IMAGE SBOM.spdx.json [--csv image-crates.csv]

The SBOM tool reads the image's package database, which knows nothing of the
programs the image build compiles (image/README.md, "Programs built into the
image"). This adds to the SPDX document, from what the built image holds:

- one package for each program in /usr/lib/tur/programs.tsv, with its version
  and the SHA-256 of the installed file;
- one package for each third-party crate in
  /usr/share/licenses/tur-third-party/crates.tsv, with the licence the crate
  declares and the SHA-256 of the crate file, as Cargo.lock pins it, and one
  for the Rust standard library;
- a STATIC_LINK relationship from each program to each crate in it.

It also holds every crate's licence against an allow-list and exits 1 if one
is not on it. TUR's programs carry a noncommercial licence and link their
crates statically, so a crate under the GPL or the LGPL may not be among them
(docs/dependencies/licence-review.md, section 5, item 13). A crate under a
licence that is not listed here stops the build until somebody has read the
licence and added it.

PODMAN may name the container tool, for example PODMAN="sudo podman".
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import pathlib
import re
import shlex
import subprocess
import sys

PODMAN = shlex.split(os.environ.get("PODMAN", "podman"))
TUR_LICENCE = "LicenseRef-TUR-Noncommercial-1.0.0"
PROGRAMS = "/usr/lib/tur/programs.tsv"
CRATES = "/usr/share/licenses/tur-third-party/crates.tsv"

# Licences a crate may have when it is linked statically into a program under
# the TUR licence: permissive ones, whose conditions are met by shipping the
# licence text and the copyright notice, which the image does under
# /usr/share/licenses/tur-third-party/.
ALLOWED = {
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "CDLA-Permissive-2.0",
    "ISC",
    "MIT",
    "MIT-0",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Unlicense",
    "Zlib",
}
ALLOWED_EXCEPTIONS = {"LLVM-exception"}

PROBE = r"""
grep -v '^#' /usr/lib/tur/programs.tsv | while IFS=$'\t' read -r program path component tools; do
    said=$("$path" --version 2>/dev/null </dev/null | head -n1)
    printf '%s\t%s\t%s\t%s\t%s\n' "$program" "$path" "$component" "${said##* }" "$(sha256sum "$path" | cut -d' ' -f1)"
done
"""


def in_image(image: str, *command: str) -> str:
    return subprocess.run(
        PODMAN + ["run", "--rm", "--network", "none", image, *command], check=True, capture_output=True, text=True
    ).stdout


def rows(text: str) -> list[list[str]]:
    return [line.split("\t") for line in text.splitlines() if line.strip() and not line.startswith("#")]


def normalise(expression: str) -> str:
    """A crate's licence field as an SPDX expression: the old "A/B" means "A OR B"."""
    return re.sub(r"\s*/\s*", " OR ", expression.strip())


def acceptable(expression: str) -> bool:
    """Whether a licence expression can be met with licences of the allow-list.

    AND needs every part; OR needs one, because the recipient may choose.
    """
    tokens = re.findall(r"\(|\)|[^\s()]+", normalise(expression))
    position = 0

    def peek() -> str | None:
        return tokens[position] if position < len(tokens) else None

    def take() -> str:
        nonlocal position
        position += 1
        return tokens[position - 1]

    def atom() -> bool:
        token = take()
        if token == "(":
            value = either()
            if take() != ")":
                raise ValueError("unbalanced parentheses")
            return value
        value = token in ALLOWED
        if peek() == "WITH":
            take()
            value = (take() in ALLOWED_EXCEPTIONS) and value
        return value

    def both() -> bool:
        value = atom()
        while peek() == "AND":
            take()
            value = atom() and value
        return value

    def either() -> bool:
        value = both()
        while peek() == "OR":
            take()
            value = both() or value
        return value

    try:
        value = either()
    except (IndexError, ValueError):
        return False
    return value and position == len(tokens)


def spdx_id(*parts: str) -> str:
    return "SPDXRef-Package-" + re.sub(r"[^A-Za-z0-9.]+", "-", "-".join(parts)).strip("-")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("image")
    ap.add_argument("sbom")
    ap.add_argument("--csv", help="also write the crates as CSV: crate, version, licence, programs")
    args = ap.parse_args()

    programs = rows(in_image(args.image, "/bin/bash", "-c", PROBE))
    crates = rows(in_image(args.image, "cat", CRATES))
    if not programs or not crates:
        print(f"the image has no {PROGRAMS} or no {CRATES}", file=sys.stderr)
        return 1

    path = pathlib.Path(args.sbom)
    document = json.loads(path.read_text())
    packages = document.setdefault("packages", [])
    relationships = document.setdefault("relationships", [])
    root = next(
        (package["SPDXID"] for package in packages if package.get("SPDXID", "").startswith("SPDXRef-DocumentRoot")),
        "SPDXRef-DOCUMENT",
    )
    extracted = document.setdefault("hasExtractedLicensingInfos", [])
    if not any(entry.get("licenseId") == TUR_LICENCE for entry in extracted):
        extracted.append(
            {
                "licenseId": TUR_LICENCE,
                "name": TUR_LICENCE,
                "extractedText": "TUR Noncommercial License 1.0.0. The text is in the image at "
                "/usr/share/licenses/tur/LICENSE.md and at https://github.com/giorgiogiudice/TUR/blob/main/LICENSE.md",
            }
        )

    # crate -> (licence, checksum, licence files, programs)
    linked: dict[tuple[str, str], dict] = {}
    for program, crate, version, licence, checksum, files in crates:
        entry = linked.setdefault(
            (crate, version), {"licence": licence, "checksum": checksum, "files": int(files), "programs": []}
        )
        entry["programs"].append(program)

    program_ids = {}
    for name, where, component, version, sha256 in programs:
        program_ids[name] = spdx_id("tur-program", name)
        packages.append(
            {
                "SPDXID": program_ids[name],
                "name": name,
                "versionInfo": version,
                "supplier": "Organization: TUR project",
                "downloadLocation": "NOASSERTION",
                "filesAnalyzed": False,
                "checksums": [{"algorithm": "SHA256", "checksumValue": sha256}],
                "licenseConcluded": TUR_LICENCE,
                "licenseDeclared": TUR_LICENCE,
                "copyrightText": "Copyright (c) 2026 TUR project authors (https://github.com/giorgiogiudice/TUR)",
                "sourceInfo": f"compiled in the image build from {component}; installed at {where}; "
                "the checksum is that of the installed file",
            }
        )
        relationships.append(
            {"spdxElementId": root, "relatedSpdxElementId": program_ids[name], "relationshipType": "CONTAINS"}
        )

    refused = []
    for (crate, version), entry in sorted(linked.items()):
        ident = spdx_id("tur-crate", crate, version)
        declared = normalise(entry["licence"])
        if not acceptable(entry["licence"]):
            refused.append(f"{crate} {version}: {entry['licence']} (in {', '.join(sorted(entry['programs']))})")
        package = {
            "SPDXID": ident,
            "name": crate,
            "versionInfo": version,
            "supplier": "NOASSERTION",
            "filesAnalyzed": False,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": declared if declared and declared != "NOASSERTION" else "NOASSERTION",
            "copyrightText": "NOASSERTION",
        }
        if crate == "rust-std":
            package["downloadLocation"] = "https://static.rust-lang.org/dist/"
            package["sourceInfo"] = "the Rust standard library of the pinned toolchain, linked into every TUR program"
        else:
            package["downloadLocation"] = f"https://crates.io/api/v1/crates/{crate}/{version}/download"
            package["checksums"] = [{"algorithm": "SHA256", "checksumValue": entry["checksum"]}]
            package["externalRefs"] = [
                {
                    "referenceCategory": "PACKAGE-MANAGER",
                    "referenceType": "purl",
                    "referenceLocator": f"pkg:cargo/{crate}@{version}",
                }
            ]
            package["sourceInfo"] = (
                "a crate compiled into TUR programs; the checksum is that of the crate file, as Cargo.lock pins it; "
                f"{entry['files']} licence files in /usr/share/licenses/tur-third-party/{crate}-{version}/"
            )
        packages.append(package)
        for program in sorted(set(entry["programs"])):
            relationships.append(
                {
                    "spdxElementId": program_ids.get(program, root),
                    "relatedSpdxElementId": ident,
                    "relationshipType": "STATIC_LINK",
                }
            )

    path.write_text(json.dumps(document, indent=1) + "\n")
    if args.csv:
        with open(args.csv, "w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle, lineterminator="\n")
            writer.writerow(("crate", "version", "licence", "programs"))
            for (crate, version), entry in sorted(linked.items(), key=lambda item: (item[0][0].lower(), item[0][1])):
                writer.writerow((crate, version, entry["licence"], " ".join(sorted(set(entry["programs"])))))

    without = sorted(f"{crate} {version}" for (crate, version), entry in linked.items() if entry["files"] == 0)
    print(f"TUR programs added to the SBOM: {len(programs)}")
    print(f"crates added to the SBOM: {len(linked)} (the Rust standard library included)")
    print(f"crates that ship no licence file of their own: {', '.join(without) or 'none'}")
    if refused:
        print("crates whose licence is not on the allow-list of scripts/sbom-programs.py:", file=sys.stderr)
        for line in refused:
            print(f"  - {line}", file=sys.stderr)
        return 1
    print("every crate's licence is on the allow-list")
    return 0


if __name__ == "__main__":
    sys.exit(main())
