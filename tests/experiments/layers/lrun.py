#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Starts one program of an application of the layers experiment in its
sandbox and becomes it: what `tur-runtime launch` is for a plain prefix.

    lrun.py --store DIR --kind layer|plain --id ID --exe PATH-IN-SANDBOX
            [--display :N] [--print] [-- ARGUMENTS]

The process is replaced by bubblewrap, so stopping it stops the sandbox and
everything in it. --print writes the command line and starts nothing.
"""

from __future__ import annotations

import argparse
import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import layerlib  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--store", required=True, type=pathlib.Path)
    parser.add_argument("--kind", required=True, choices=("layer", "plain"))
    parser.add_argument("--id", required=True)
    parser.add_argument("--exe", required=True)
    parser.add_argument("--display")
    parser.add_argument("--print", action="store_true")
    parser.add_argument("arguments", nargs="*")
    options = parser.parse_args()

    store = layerlib.Store(options.store, display=options.display or os.environ.get("DISPLAY"))
    command = store.sandbox(options.kind, options.id, chdir=options.exe.rsplit("/", 1)[0])
    command += ["/bin/sh", "-c", layerlib.SUPERVISE, "tur-supervise", "wine", options.exe, *options.arguments]
    if options.print:
        print(" \\\n  ".join(command))
        return 0
    env = store.environment()
    os.execvpe(command[0], command, env)
    return 127


if __name__ == "__main__":
    sys.exit(main())
