#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Accept the fixtures the router printed in CI.

    desktop/apps/tur-centre/fixtures/update.py JOB_ID
    desktop/apps/tur-centre/fixtures/update.py --log FILE

The fixtures in this directory are the output of `tur-catalogue ... --json`
for the seed catalogue. Nobody can run that program on a development machine
(AGENTS.md), so CI makes them: the test
services/tur-catalogue/tests/desktop_fixtures.rs, in the `rust` workflow,
fails when a file here is not what the program prints, and prints what it
should be as lines that begin with TUR-FIXTURE.

This script reads those lines from the log of job JOB_ID (the job named
services/tur-catalogue of the rust workflow; through scripts/dev/ci.py) or
from a saved log, writes the files, and removes the ones the test reported
as stale. It does not commit.

LOOK AT `git diff` BEFORE COMMITTING. A fixture is a statement of what the
router says; the pictures of the application centre are made from it.

Needs only the Python standard library. It runs nothing but ci.py.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
NAME = re.compile(r"(?:[a-z0-9-]+/)?[A-Za-z0-9._-]+\.json")
CHUNK = re.compile(r"TUR-FIXTURE (\S+) (\d+) ([0-9a-f]+)\s*$")
END = re.compile(r"TUR-FIXTURE-END (\S+) (\d+)\s*$")
STALE = re.compile(r"TUR-FIXTURE-STALE (\S+)\s*$")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("job_id", nargs="?", type=int, help="the job of the rust workflow that ran the test")
    parser.add_argument("--log", type=pathlib.Path, help="read a saved job log instead")
    args = parser.parse_args()

    if args.log:
        text = args.log.read_text(encoding="utf-8", errors="replace")
    elif args.job_id:
        fetch = subprocess.run(
            [sys.executable, str(ROOT / "scripts" / "dev" / "ci.py"), "log", str(args.job_id), "--grep", "TUR-FIXTURE"],
            capture_output=True, text=True, check=False)
        if fetch.returncode != 0:
            print(fetch.stdout + fetch.stderr, file=sys.stderr)
            print(f"could not read the log of job {args.job_id}", file=sys.stderr)
            return 1
        text = fetch.stdout
    else:
        parser.error("give a job ID or --log FILE")

    chunks: dict[str, dict[int, bytes]] = {}
    sizes: dict[str, int] = {}
    stale: list[str] = []
    for line in text.splitlines():
        if match := CHUNK.search(line):
            chunks.setdefault(match.group(1), {})[int(match.group(2))] = bytes.fromhex(match.group(3))
        elif match := END.search(line):
            sizes[match.group(1)] = int(match.group(2))
        elif match := STALE.search(line):
            stale.append(match.group(1))

    if not chunks and not stale:
        print("the log holds no TUR-FIXTURE line: the fixtures are what the router prints, or this is the wrong job")
        return 1

    written = 0
    for name in sorted(chunks):
        if not NAME.fullmatch(name):
            print(f"refused: {name!r} is not the name of a fixture file", file=sys.stderr)
            return 1
        parts = chunks[name]
        if sorted(parts) != list(range(len(parts))):
            print(f"refused: {name} is incomplete in the log (lines are missing)", file=sys.stderr)
            return 1
        content = b"".join(parts[number] for number in range(len(parts)))
        if sizes.get(name) != len(content):
            print(f"refused: {name} is {len(content)} bytes in the log, and the test said {sizes.get(name)}",
                  file=sys.stderr)
            return 1
        target = HERE / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)
        print(f"  written  {name}  ({len(content)} bytes)")
        written += 1

    removed = 0
    for name in sorted(set(stale)):
        if not NAME.fullmatch(name):
            print(f"refused: {name!r} is not the name of a fixture file", file=sys.stderr)
            return 1
        target = HERE / name
        if target.exists():
            target.unlink()
            print(f"  removed  {name}  (stale: nothing produces it)")
            removed += 1

    print(f"{written} file(s) written, {removed} removed, in {HERE.relative_to(ROOT)}")
    print("Look at `git diff` before committing.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
