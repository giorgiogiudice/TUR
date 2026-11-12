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
    size = m["image"].get("size_bytes")
    if size:
        rows.insert(5, ("Image size", f"{size} bytes ({size / 2**30:.2f} GiB)"))
    catalogue = m.get("catalogue") or {}
    crates = m.get("third_party_crates") or {}
    rows += [
        ("Application catalogue", f'{catalogue.get("entries")} entries in {catalogue.get("path")}'),
        ("Third-party crates in TUR programs", f'{crates.get("count")}, compiled with Rust {crates.get("rust")}'),
    ]
    print("| | |")
    print("|---|---|")
    for key, value in rows:
        print(f"| {key} | `{value}` |")

    programs = m.get("programs") or []
    if programs:
        print()
        print("### TUR programs in the image")
        print()
        print("| Program | Version | Path | Bytes | SHA-256 | Assistant tools |")
        print("|---|---|---|---:|---|---|")
        for program in programs:
            tools = (program.get("assistant_tools") or {}).get("directory") or "none"
            print(
                f'| `{program["name"]}` | {program["version"]} | `{program["path"]}` | {program["size_bytes"]} '
                f'| `{program["sha256"][:16]}…` | `{tools}` |'
            )
        total = sum(program["size_bytes"] for program in programs)
        print()
        print(f"{len(programs)} programs, {total} bytes ({total / 2**20:.1f} MiB).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
