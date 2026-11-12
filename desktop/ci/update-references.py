#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Accept the pictures CI rendered as the new reference images.

    desktop/ci/update-references.py RUN_ID [--only GLOB] [--keep DIR]

Nobody can render the pictures on a development machine, so a reference
image is always made by CI: the `screenshots` job of the desktop workflow
uploads, in the artifact `desktop-screenshots`, every picture that is new or
differs from its reference (proposed/), next to the reference it differs from
and a picture of the difference (diff/).

This script downloads that artifact from run RUN_ID (through
scripts/dev/ci.py), copies proposed/*.png into desktop/tests/screenshots, and
removes references the run reported as stale. It does not commit.

LOOK AT EVERY PICTURE IT LISTS BEFORE COMMITTING. A reference is a statement
that the picture is what the design intends; the comparison can only say that
nothing changed since. With --keep DIR the whole artifact is left in DIR so
that the pictures, the old references and the differences can be opened.

Needs only the Python standard library. It runs nothing but ci.py.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
REFERENCES = ROOT / "desktop" / "tests" / "screenshots"
ARTIFACT = "desktop-screenshots"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("run_id", type=int, help="the run of the desktop workflow to take the pictures from")
    parser.add_argument("--only", default="*", help="accept only the pictures whose name matches this glob")
    parser.add_argument("--keep", type=pathlib.Path, help="leave the downloaded artifact in this directory")
    args = parser.parse_args()

    holder = None
    if args.keep:
        directory = args.keep
        directory.mkdir(parents=True, exist_ok=True)
    else:
        holder = tempfile.TemporaryDirectory(prefix="tur-screenshots-")
        directory = pathlib.Path(holder.name)

    fetch = subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "dev" / "ci.py"), "artifact", str(args.run_id), ARTIFACT, str(directory)],
        capture_output=True, text=True, check=False)
    if fetch.returncode != 0:
        print(fetch.stdout + fetch.stderr, file=sys.stderr)
        print(f"could not download the artifact {ARTIFACT} of run {args.run_id}", file=sys.stderr)
        return 1

    report = json.loads((directory / "report.json").read_text(encoding="utf-8"))
    status = {row["name"]: (row["status"], row["detail"]) for row in report["rows"]}

    refused = [name for name, (state, _) in status.items() if state == "not repeatable"]
    if refused:
        print("These pictures differ between two runs of the same program and cannot be references:")
        for name in refused:
            print(f"  {name}: {status[name][1]}")

    accepted = 0
    REFERENCES.mkdir(parents=True, exist_ok=True)
    for picture in sorted((directory / "proposed").glob("*.png")):
        name = picture.name
        if not fnmatch.fnmatch(name, args.only) or name in refused:
            continue
        shutil.copyfile(picture, REFERENCES / name)
        state, detail = status.get(name, ("", ""))
        print(f"  accepted  {name}  ({state}: {detail})")
        accepted += 1

    removed = 0
    for name in report.get("stale", []):
        if fnmatch.fnmatch(name, args.only) and (REFERENCES / name).exists():
            (REFERENCES / name).unlink()
            print(f"  removed   {name}  (stale: nothing renders it any more)")
            removed += 1

    print(f"{accepted} reference(s) written, {removed} removed, in {REFERENCES.relative_to(ROOT)}")
    if accepted or removed:
        print("Look at each picture before committing it. `git status` shows what changed.")
    if args.keep:
        print(f"The artifact is in {directory}: actual/, reference/, diff/, proposed/, contact-*.png")
    if holder:
        holder.cleanup()
    return 0


if __name__ == "__main__":
    sys.exit(main())
