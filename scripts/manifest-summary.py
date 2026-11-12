#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Print a Markdown summary of a build manifest, for the CI job summary.

    scripts/manifest-summary.py build-manifest.json
"""

from __future__ import annotations

import json
import pathlib
import sys


def main() -> int:
    path = pathlib.Path(sys.argv[1])
    print("### TUR image")
    if not path.exists():
        print("No build manifest was produced.")
        return 0
    m = json.loads(path.read_text())
    rows = [
        ("Build ID", m["build_id"]),
        ("Source revision", m["source_revision"]),
        ("Base", f'{m["base"]["image"]}:{m["base"]["tag"]} ({m["base"]["version"]})'),
        ("Base digest", m["base"]["digest"]),
        ("Image ID", m["image"]["id"]),
        ("Kernel", m["kernel_release"]),
        ("Plasma", m["desktop"]["plasma_workspace"]),
        ("KWin", m["desktop"]["kwin"]),
        ("Display manager", m["desktop"]["display_manager"]),
        ("Bootloader packages", ", ".join(f"{k} {v}" for k, v in m["bootloader"].items() if v) or "none"),
        ("Kernel config", ", ".join(f"{k}={v}" for k, v in m["kernel_config"].items())),
        ("SELinux", m["security"]["selinux"]),
        ("RPM packages", m["rpm_count"]),
    ]
    print("| | |")
    print("|---|---|")
    for key, value in rows:
        print(f"| {key} | `{value}` |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
