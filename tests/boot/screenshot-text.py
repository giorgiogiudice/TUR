#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Read the text on the boot test's screenshots and fail if it names Fedora.

    tests/boot/screenshot-text.py OUT_DIR

desktop-experience.md, 12.5: "text recognised in the start-up, setup, login
and desktop screenshots must not contain 'Fedora'". Reads every
OUT_DIR/screenshot-*.png and OUT_DIR/splash/frame-*.png with Tesseract,
writes what it read to OUT_DIR/screenshot-text.json and fails if any of it
contains "fedora" in any case. Text recognition does not see a logo drawn
without letters: a person still looks at the pictures (TUR-008).
"""

from __future__ import annotations

import json
import pathlib
import re
import shutil
import subprocess
import sys


def main() -> int:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = pathlib.Path(sys.argv[1])
    if not shutil.which("tesseract"):
        sys.exit("tesseract is not installed")
    pictures = sorted(out.glob("screenshot-*.png")) + sorted((out / "splash").glob("frame-*.png"))
    if not pictures:
        sys.exit(f"no screenshots in {out}")
    found, read = [], {}
    for picture in pictures:
        result = subprocess.run(["tesseract", str(picture), "-", "--psm", "11"], capture_output=True, text=True)
        text = " ".join(result.stdout.split())
        read[str(picture.relative_to(out))] = text
        hits = re.findall(r"\S*fedora\S*", text, re.I)
        print(f"{'FEDORA' if hits else 'clean '}  {picture.relative_to(out)}: {text[:200]}")
        if hits:
            found.append({"picture": str(picture.relative_to(out)), "words": hits})
    (out / "screenshot-text.json").write_text(json.dumps({"found": found, "read": read}, indent=2) + "\n")
    print(f"\n{len(pictures)} pictures read; {len(found)} name Fedora.")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
