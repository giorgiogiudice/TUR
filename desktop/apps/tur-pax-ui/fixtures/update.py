#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Take the fixture of org.tur.Agent1 from what the real service said in CI.

    desktop/apps/tur-pax-ui/fixtures/update.py RUN_ID

RUN_ID is a run of the desktop workflow. Its job `pax-ui-end-to-end` asks the
real `tur-pax serve` everything tur-pax-ui's tests answer from, records the
answers and the signals (desktop/tests/pax-ui/e2e.py, `record`), and fails
when they differ from the files here. This fetches that recording (the
artifact desktop-pax-ui-e2e, through scripts/dev/ci.py) and puts it in
fixtures/agent1 in place of what is there. Look at `git diff` before
committing: a change in what the service says is a change the window may
have to follow. Nobody writes these files by hand.
"""

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("run_id", type=int)
    arguments = parser.parse_args()
    with tempfile.TemporaryDirectory() as holder:
        fetched = Path(holder)
        subprocess.run([sys.executable, str(ROOT / "scripts" / "dev" / "ci.py"), "artifact", str(arguments.run_id),
                        "desktop-pax-ui-e2e", str(fetched)], check=True)
        recorded = fetched / "recorded" / "agent1"
        if not (recorded / "replies.json").exists():
            print(f"run {arguments.run_id} recorded nothing: no recorded/agent1/replies.json in its artifact", file=sys.stderr)
            return 1
        target = HERE / "agent1"
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(recorded, target)
    files = sorted(path.relative_to(HERE) for path in (HERE / "agent1").rglob("*.json"))
    print(f"{len(files)} files in {HERE.relative_to(ROOT)}/agent1:")
    for path in files:
        print(f"  {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
