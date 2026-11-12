#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Describe a disk the installer medium installed, in the form
tests/boot/report.py reads (host/disk-info.env, which tests/boot/build-disk.sh
writes for a disk it installs itself).

    tests/iso/disk-info.py RESULT.json MANIFEST.json DISK TARGET.env

RESULT.json is what tests/iso/install.py wrote for the installation,
MANIFEST.json the medium's manifest, DISK the installed disk image.
"""

from __future__ import annotations

import json
import os
import pathlib
import sys


def main() -> int:
    if len(sys.argv) != 5:
        print(__doc__, file=sys.stderr)
        return 2
    result = json.loads(pathlib.Path(sys.argv[1]).read_text())
    manifest = json.loads(pathlib.Path(sys.argv[2]).read_text())
    disk, target = pathlib.Path(sys.argv[3]), pathlib.Path(sys.argv[4])
    facts, payload = result.get("facts") or {}, manifest.get("payload") or {}
    stat = os.stat(disk)
    lines = {
        # The name the installed system records as its image: what bootc
        # status reports as booted.
        "test_image": payload.get("recorded_origin"),
        "test_image_id": payload.get("image_id"),
        "installed_from": f"installer medium {(manifest.get('medium') or {}).get('file')}, "
        f"sha256 {(manifest.get('medium') or {}).get('sha256')}",
        "install_command": facts.get("install_command"),
        "install_seconds": facts.get("install_seconds"),
        "root_fs": payload.get("root_filesystem"),
        "root_fs_source": payload.get("root_filesystem_source"),
        "kargs": "from /usr/lib/bootc/kargs.d in the image (tests/iso/guest)",
        "disk_apparent_bytes": stat.st_size,
        "disk_allocated_bytes": stat.st_blocks * 512,
    }
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text("".join(f"{key}={value}\n" for key, value in lines.items() if value is not None))
    print(target.read_text())
    return 0


if __name__ == "__main__":
    sys.exit(main())
