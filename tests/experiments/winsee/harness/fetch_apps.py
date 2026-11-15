#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Fetch the real Windows applications the winsee experiment reads and
operates (docs/reports/exp-winsee-1.md): the ones TUR's Windows tests
already use, from tests/windows-runtime/apps.json, each from its
publisher's location and refused unless its size and SHA-256 are the ones
pinned there. Nothing fetched is committed or redistributed.

    fetch_apps.py --apps tests/windows-runtime/apps.json --dir DIR ID...
"""

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--apps", required=True)
    parser.add_argument("--dir", required=True)
    parser.add_argument("ids", nargs="+")
    arguments = parser.parse_args()
    listed = {app["id"]: app for app in json.loads(pathlib.Path(arguments.apps).read_text())["applications"]}
    directory = pathlib.Path(arguments.dir)
    directory.mkdir(parents=True, exist_ok=True)
    status = 0
    for identifier in arguments.ids:
        app = listed[identifier]
        path = directory / app["url"].rsplit("/", 1)[1]
        if path.exists() and path.stat().st_size == app["size"] and sha256_of(path) == app["sha256"]:
            print(f"{identifier}: {path.name} present and verified")
            continue
        done = subprocess.run(["curl", "-fL", "--retry", "3", "--silent", "--show-error", "-o", str(path), app["url"]], check=False)
        if done.returncode != 0 or path.stat().st_size != app["size"] or sha256_of(path) != app["sha256"]:
            print(f"::warning::{identifier}: {app['url']} could not be fetched as pinned; its scenario will say NOT RUN")
            path.unlink(missing_ok=True)
            status = 0
            continue
        print(f"{identifier}: {path.name} {app['size']} bytes, {app['sha256']} ({app['name']} {app['version']}, {app['licence'][:60]})")
    return status


if __name__ == "__main__":
    sys.exit(main())
