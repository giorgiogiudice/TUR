#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Keep Microsoft's files out of the experiment's artifacts.

    artifact-guard.py OUT_DIR

Run just before the artifact is uploaded. Deletes from OUT_DIR anything that
is not something TUR's own scripts wrote: files with the signature of an
executable, a registry hive, an event log, a disk or firmware image, an
archive or an RTF document, files with such an extension, and any file over
8 MiB. What it removed is listed in OUT_DIR/artifact-guard.json, so a
removal is visible and never silent.
"""

from __future__ import annotations

import json
import pathlib
import sys

LIMIT = 8 * 1024 * 1024
EXTENSIONS = {
    ".iso", ".wim", ".esd", ".img", ".raw", ".qcow2", ".fd", ".efi", ".exe", ".dll", ".sys",
    ".evtx", ".etl", ".rtf", ".hiv", ".dat", ".log1", ".log2", ".cab", ".msi", ".mui", ".bin",
}
SIGNATURES = [
    (b"MZ", "executable"), (b"regf", "registry hive"), (b"ElfFile\x00", "event log"),
    (b"MSWIM\x00\x00\x00", "install archive"), (b"{\\rtf", "RTF document"), (b"QFI\xfb", "qcow2 image"),
    (b"MSCF", "cabinet"), (b"\xd0\xcf\x11\xe0", "compound document"),
]


def main() -> int:
    out = pathlib.Path(sys.argv[1])
    removed = []
    for path in sorted(p for p in out.rglob("*") if p.is_file() or p.is_symlink()):
        reason = None
        if path.is_symlink():
            reason = "symbolic link"
        else:
            size = path.stat().st_size
            with path.open("rb") as handle:
                head = handle.read(16)
            if path.suffix.lower() in EXTENSIONS:
                reason = f"extension {path.suffix}"
            elif size > LIMIT:
                reason = f"{size} bytes, over the limit"
            else:
                for magic, what in SIGNATURES:
                    if head.startswith(magic):
                        reason = what
                        break
        if reason:
            path.unlink()
            removed.append({"path": str(path.relative_to(out)), "reason": reason})
    (out / "artifact-guard.json").write_text(json.dumps({"removed": removed}, indent=2) + "\n")
    print(f"artifact guard: {len(removed)} file(s) removed")
    for item in removed:
        print(f"  {item['path']}: {item['reason']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
