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

    if errors:
        print("build manifest is invalid:")
        for error in errors:
            print(f"  - {error}")
        return 1
    print("build manifest is valid")
    return 0


if __name__ == "__main__":
    sys.exit(main())
