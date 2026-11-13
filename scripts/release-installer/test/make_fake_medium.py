#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Make a small stand-in for the `iso` workflow's medium artifact and test record.

    make_fake_medium.py DIR REVISION [MIB] [--not-redistributable]

Writes DIR/medium/{tur-installer.iso, .manifest.json, .SHA256SUMS} as the
`iso` workflow's medium artifact holds them, and DIR/record/medium/ with the
manifest and checksums as its test record holds them. The "medium" is MIB
(default 24) MiB of pseudo-random bytes, so that an upload of it is split into
several parts. Test use only.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import random
import shutil
import sys


def make(folder: pathlib.Path, revision: str, mib: int = 24, redistributable: bool = True) -> None:
    medium = folder / "medium"
    record = folder / "record" / "medium"
    medium.mkdir(parents=True, exist_ok=True)
    record.mkdir(parents=True, exist_ok=True)
    iso = medium / "tur-installer.iso"
    rng = random.Random(revision)
    with iso.open("wb") as handle:
        for _ in range(mib):
            handle.write(rng.randbytes(1 << 20))
    data_sha = hashlib.sha256(iso.read_bytes()).hexdigest()
    manifest = {
        "schema": "tur-installer-medium-manifest",
        "schema_version": 1,
        "medium": {"file": iso.name, "bytes": iso.stat().st_size, "sha256": data_sha},
        "source": {"revision": revision, "tur_version": "0.1.0-dev", "build_id": f"20261112.g{revision[:12]}",
                   "commit_time": 1794000000},
        "payload": {
            "image_id": "f" * 64,
            "base_image": "quay.io/fedora/fedora-kinoite",
            "base_digest": "sha256:" + "e" * 64,
            "base_version": "45.20261112.0",
            "oci_layout": {"config_digest": "sha256:" + "f" * 64, "index_sha256": "d" * 64},
            "smallest_disk_accepted_bytes": 20 * 1024**3,
        },
        "installer_environment": {"kernel": "6.17.7-300.fc45.x86_64"},
        "distribution": "Not for redistribution. Test stand-in." if not redistributable
        else "A pre-alpha preview for testers (test stand-in).",
    }
    path = medium / "tur-installer.manifest.json"
    path.write_text(json.dumps(manifest, indent=2) + "\n")
    manifest_sha = hashlib.sha256(path.read_bytes()).hexdigest()
    sums = medium / "tur-installer.SHA256SUMS"
    sums.write_text(f"{data_sha}  {iso.name}\n{manifest_sha}  {path.name}\n")
    shutil.copyfile(sums, record / sums.name)
    shutil.copyfile(path, record / path.name)
    (medium / "DO-NOT-REDISTRIBUTE.txt").write_text("test stand-in\n")


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    make(pathlib.Path(args[0]), args[1], int(args[2]) if len(args) == 3 else 24,
         "--not-redistributable" not in sys.argv)
    return 0


if __name__ == "__main__":
    sys.exit(main())
