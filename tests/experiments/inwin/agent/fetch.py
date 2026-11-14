#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Fetch the applications of apps.json, each refused unless its size and
SHA-256 are the pinned ones. One that cannot be fetched is left out and
its scenario says NOT RUN.

    fetch.py --dir DIR
"""

import argparse
import hashlib
import json
import pathlib
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dir", required=True)
    arguments = parser.parse_args()
    directory = pathlib.Path(arguments.dir)
    directory.mkdir(parents=True, exist_ok=True)
    listed = json.loads((pathlib.Path(__file__).resolve().parent / "apps.json").read_text(encoding="utf-8"))["applications"]
    for app in listed:
        path = directory / app["url"].rsplit("/", 1)[1]
        try:
            request = urllib.request.Request(app["url"], headers={"User-Agent": "tur-experiment"})
            with urllib.request.urlopen(request, timeout=120) as response:
                data = response.read()
        except OSError as error:
            print(f"::warning::{app['id']}: not fetched ({error}); its scenario will say NOT RUN")
            continue
        if len(data) != app["size"] or hashlib.sha256(data).hexdigest() != app["sha256"]:
            print(f"::warning::{app['id']}: what came is not the pinned file; its scenario will say NOT RUN")
            continue
        path.write_bytes(data)
        print(f"{app['id']}: {path.name}, {app['name']} {app['version']} ({app['licence']})")


if __name__ == "__main__":
    main()
