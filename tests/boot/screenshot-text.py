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


# What may name Fedora on a screenshot: the names and descriptions of the
# base's own services, which systemd prints on the text console while no
# splash covers it ("Starting fedora-third-party-refresh.service - Refresh
# Fedora Third-party Repositories"). They describe Fedora's services
# truthfully (desktop-experience.md, 12.4, table C; TUR-008). The patterns
# allow for the recogniser reading "-" as "—" or "~".
ALLOWED = [
    r"fedora\W{1,2}third\W{1,2}party\S*",
    r"fedora\W{1,2}atomic\W{1,2}desktop\S*",
    r"fedora\W{1,2}kinoite\W{1,2}plasmalogin\S*",
    r"flatpak\W{1,2}add\W{1,2}fedora\W{1,2}repos\S*",
    r"add fedora flatpak",
]


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
        rest = text
        allowed = []
        for pattern in ALLOWED:
            allowed += re.findall(pattern, rest, re.I)
            rest = re.sub(pattern, " ", rest, flags=re.I)
        hits = re.findall(r"\S*fedora\S*", rest, re.I)
        verdict = "FEDORA" if hits else ("allowed" if allowed else "clean ")
        print(f"{verdict}  {picture.relative_to(out)}: {text[:200]}")
        if allowed:
            print(f"         allowed: {sorted(set(allowed))}")
        if hits:
            found.append({"picture": str(picture.relative_to(out)), "words": hits})
    (out / "screenshot-text.json").write_text(json.dumps({"found": found, "read": read}, indent=2) + "\n")
    print(f"\n{len(pictures)} pictures read; {len(found)} name Fedora.")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
