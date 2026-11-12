#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Validate a build manifest against the schema and reject template values.

    scripts/validate-manifest.py build-manifest.json
"""

from __future__ import annotations

import json
import pathlib
import sys

import jsonschema

ROOT = pathlib.Path(__file__).resolve().parent.parent
PLACEHOLDERS = ("build-value", "pinned-version", "pinned-commit", "ci-generated", "unknown", "local")


def strings(node, path=""):
    if isinstance(node, dict):
        for key, value in node.items():
            yield from strings(value, f"{path}/{key}")
    elif isinstance(node, list):
        for index, value in enumerate(node):
            yield from strings(value, f"{path}/{index}")
    elif isinstance(node, str):
        yield path, node


def main() -> int:
    manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
    schema = json.loads((ROOT / "schemas" / "build-manifest.schema.json").read_text())

    errors = [
        f"{'/'.join(str(p) for p in e.absolute_path) or '(root)'}: {e.message}"
        for e in jsonschema.Draft202012Validator(schema).iter_errors(manifest)
    ]
    for path, value in strings(manifest):
        if any(value == p or value.startswith(p + "-") for p in PLACEHOLDERS):
            errors.append(f"{path}: placeholder value {value!r}")

    # The programs the image holds are the ones image/programs.tsv names, each
    # where that file says: nothing was left out and nothing came from
    # elsewhere.
    wanted = {}
    for line in (ROOT / "image" / "programs.tsv").read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            component, program, where = line.split("\t")
            wanted[program] = (where, component)
    found = {}
    for program in manifest.get("programs") or []:
        if isinstance(program, dict):
            found[program.get("name")] = (program.get("path"), program.get("component"))
    for name in sorted(set(wanted) | set(found)):
        if wanted.get(name) != found.get(name):
            errors.append(
                f"programs: {name}: image/programs.tsv says {wanted.get(name)}, the image has {found.get(name)}"
            )
    hashes = [program.get("sha256") for program in manifest.get("programs") or [] if isinstance(program, dict)]
    if len(set(hashes)) != len(hashes):
        errors.append("programs: two programs have the same SHA-256")

    if errors:
        print("build manifest is invalid:")
        for error in errors:
            print(f"  - {error}")
        return 1
    print("build manifest is valid")
    return 0


if __name__ == "__main__":
    sys.exit(main())
