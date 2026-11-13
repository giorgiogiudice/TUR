#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Copies the recorded runs of tur-apps out of a CI run into the fixtures.

    desktop/apps/tur-centre/fixtures/apps/update.py RUN_ID

RUN_ID is a run of the desktop workflow whose job centre-e2e recorded them
(desktop/ci/centre-e2e/record.py). Its artifact desktop-centre-e2e holds
fixtures/<scenario>/runs.json; each replaces the file of the same scenario
here. Nothing is edited on the way: the files are what tur-apps printed on
the runner. Look at `git diff` before committing them.
"""

from __future__ import annotations

import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[4]


def main() -> int:
    if len(sys.argv) != 2 or not sys.argv[1].isdigit():
        print(__doc__)
        return 2
    with tempfile.TemporaryDirectory() as scratch:
        subprocess.run([sys.executable, str(ROOT / "scripts/dev/ci.py"), "artifact", sys.argv[1],
                        "desktop-centre-e2e", scratch], check=True)
        found = sorted(pathlib.Path(scratch).glob("fixtures/*/runs.json"))
        if not found:
            print("the artifact holds no fixtures/*/runs.json")
            return 1
        for source in found:
            document = json.loads(source.read_text())
            if not document.get("invocations"):
                print(f"{source}: no invocations; not copied")
                continue
            target = HERE / source.parent.name / "runs.json"
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
            print(f"{target.relative_to(ROOT)}: {len(document['invocations'])} runs")
    return 0


if __name__ == "__main__":
    sys.exit(main())
