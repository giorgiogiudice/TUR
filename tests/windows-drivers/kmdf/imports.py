#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Classify the kernel functions a Windows driver image needs against Wine.

Input: the symbols the driver's objects (or linked image) need from outside
(one per line, e.g. from `llvm-nm -u`), the mingw-w64 import libraries (to
know which Windows module exports each one) and one or more Wine source
trees. For every needed symbol and every Wine tree the result is one of

  implemented    Wine's .spec exports it with an implementation
  forwarded      exported as a forward to another Wine module (ntdll mostly)
  fixme-stub     exported, but the function body logs FIXME "stub" (it
                 returns without doing the work, or with a fixed value)
  stub           a `stub` in the .spec: calling it aborts the process
  missing        not exported at all: the image does not load

Symbols no Windows import library exports are reported separately: they
are the framework's own functions that no compiled file defines (parts of
the framework that are not in the published sources) or compiler runtime
helpers.

  imports.py --undefined FILE --mingw-lib DIR --out MD [--json JSON] WINE_TREE...
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys

MODULES = {  # Windows module -> (Wine directory, spec file)
    "ntoskrnl.exe": ("dlls/ntoskrnl.exe", "ntoskrnl.exe.spec"),
    "hal.dll": ("dlls/hal", "hal.spec"),
    "usbd.sys": ("dlls/usbd.sys", "usbd.sys.spec"),
    "wmilib.sys": ("dlls/wmilib.sys", "wmilib.sys.spec"),
    "ksecdd.sys": ("dlls/ksecdd.sys", "ksecdd.sys.spec"),
    "wdfldr.sys": ("dlls/wdfldr.sys", "wdfldr.sys.spec"),
}
MINGW_LIBS = {"ntoskrnl": "ntoskrnl.exe", "hal": "hal.dll", "usbd": "usbd.sys", "wmilib": "wmilib.sys",
              "ksecdd": "ksecdd.sys"}
SPEC = re.compile(r"^\s*(?:@|\d+)\s+(stdcall|cdecl|varargs|stub|extern|thiscall|fastcall)\s+((?:-\S+\s+)*)"
                  r"([A-Za-z_][\w@]*)\s*(\([^)]*\))?\s*(\S*)")


def nm_exports(libdir: str) -> dict[str, str]:
    """symbol -> Windows module, from the mingw-w64 import libraries."""
    found: dict[str, str] = {}
    nm = "llvm-nm" if subprocess.run(["sh", "-c", "command -v llvm-nm"], capture_output=True).returncode == 0 \
        else "x86_64-w64-mingw32-nm"
    for lib, module in MINGW_LIBS.items():
        path = os.path.join(libdir, f"lib{lib}.a")
        if not os.path.exists(path):
            continue
        out = subprocess.run([nm, "-g", path], capture_output=True, text=True).stdout
        for m in re.finditer(r"\b__imp_(\w+)", out):
            found.setdefault(m.group(1), module)
    return found


def spec_exports(tree: str, module: str) -> dict[str, tuple[str, str]]:
    d, spec = MODULES[module]
    path = os.path.join(tree, d, spec)
    result: dict[str, tuple[str, str]] = {}
    if not os.path.exists(path):
        return result
    for line in open(path, encoding="utf-8", errors="replace"):
        m = SPEC.match(line)
        if not m:
            continue
        kind, flags, name, _args, forward = m.groups()
        arch = re.search(r"-arch=(\S+)", flags or "")
        if arch:
            archs = arch.group(1)
            neg = archs.startswith("!")
            listed = set(archs.lstrip("!").split(","))
            on = ("x86_64" in listed or "win64" in listed or "64" in listed)
            if neg == on:
                continue
        if kind == "stub":
            result[name] = ("stub", "")
        elif forward and "." in forward and not forward.startswith("#"):
            result[name] = ("forwarded", forward)
        else:
            result[name] = ("implemented", "")
    return result


def function_body(tree: str, directory: str, name: str) -> str | None:
    pattern = re.compile(r"(?:^|\n)[^\n;{}]*\b" + re.escape(name) + r"\s*\([^;{]*\)\s*\{", re.S)
    d = os.path.join(tree, directory)
    if not os.path.isdir(d):
        return None
    for fn in sorted(os.listdir(d)):
        if not fn.endswith(".c"):
            continue
        text = open(os.path.join(d, fn), encoding="utf-8", errors="replace").read()
        m = pattern.search(text)
        if not m:
            continue
        i, depth = m.end() - 1, 0
        for j in range(i, len(text)):
            if text[j] == "{":
                depth += 1
            elif text[j] == "}":
                depth -= 1
                if depth == 0:
                    return text[i:j + 1]
    return None


def classify(tree: str, module: str, name: str, cache: dict) -> tuple[str, str]:
    key = (tree, module)
    if key not in cache:
        cache[key] = spec_exports(tree, module)
    kind, forward = cache[key].get(name, ("missing", ""))
    if kind == "implemented":
        body = function_body(tree, MODULES[module][0], name)
        if body and re.search(r'FIXME\s*\(\s*"[^"]*stub', body, re.I):
            return "fixme-stub", ""
    if kind == "forwarded":
        target = forward.split(".")[0]
        body = function_body(tree, f"dlls/{target}", forward.split(".", 1)[1])
        if body and re.search(r'FIXME\s*\(\s*"[^"]*stub', body, re.I):
            return "fixme-stub", forward
    return kind, forward


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--undefined", required=True)
    ap.add_argument("--mingw-lib", required=True)
    ap.add_argument("--sources", default="")
    ap.add_argument("--out", required=True)
    ap.add_argument("--json", default="")
    ap.add_argument("--extra-module", action="append", default=[],
                    help="SYMBOL=MODULE for symbols no mingw-w64 import library lists (e.g. wdfldr.sys)")
    ap.add_argument("trees", nargs="+")
    args = ap.parse_args()

    wanted = sorted({l.strip() for l in open(args.undefined) if l.strip()})
    owner = nm_exports(args.mingw_lib)
    for item in args.extra_module:
        sym, module = item.split("=", 1)
        owner[sym] = module
    labels = [os.path.basename(os.path.normpath(t)) for t in args.trees]
    cache: dict = {}
    rows, internal = [], []
    for sym in wanted:
        module = owner.get(sym)
        if module is None:
            internal.append(sym)
            continue
        rows.append({"symbol": sym, "module": module,
                     "wine": {label: classify(tree, module, sym, cache) for label, tree in zip(labels, args.trees)}})

    counts = {label: {} for label in labels}
    for r in rows:
        for label in labels:
            k = r["wine"][label][0]
            counts[label][k] = counts[label].get(k, 0) + 1
    order = ["implemented", "forwarded", "fixme-stub", "stub", "missing"]
    out = ["## Imports of the framework, classified against Wine", ""]
    out.append(f"{len(rows)} functions and data imported from Windows kernel modules "
               f"({', '.join(sorted({r['module'] for r in rows}))}).")
    out.append("")
    out.append("| Wine | " + " | ".join(order) + " |")
    out.append("|---|" + "---|" * len(order))
    for label in labels:
        out.append(f"| {label} | " + " | ".join(str(counts[label].get(k, 0)) for k in order) + " |")
    out.append("")
    out.append("Every import that is not plainly implemented in at least one tree:")
    out.append("")
    out.append("| symbol | module | " + " | ".join(labels) + " |")
    out.append("|---|---|" + "---|" * len(labels))
    for r in rows:
        if all(r["wine"][l][0] in ("implemented", "forwarded") for l in labels):
            continue
        out.append(f"| `{r['symbol']}` | {r['module']} | " +
                   " | ".join(r["wine"][l][0] for l in labels) + " |")
    out.append("")
    out.append(f"<details><summary>All {len(rows)} imports</summary>")
    out.append("")
    out.append("| symbol | module | " + " | ".join(labels) + " |")
    out.append("|---|---|" + "---|" * len(labels))
    for r in rows:
        out.append(f"| `{r['symbol']}` | {r['module']} | " +
                   " | ".join(r["wine"][l][0] + (f" ({r['wine'][l][1]})" if r["wine"][l][1] else "")
                              for l in labels) + " |")
    out.append("")
    out.append("</details>")
    out.append("")
    out.append(f"{len(internal)} needed symbols that no Windows import library exports "
               "(the framework's own functions no compiled file defines, or compiler helpers):")
    out.append("")
    out.append("```")
    out.extend(internal)
    out.append("```")
    with open(args.out, "w", encoding="utf-8") as f:
        f.write("\n".join(out) + "\n")
    if args.json:
        with open(args.json, "w", encoding="utf-8") as f:
            json.dump({"imports": rows, "internal": internal, "counts": counts}, f, indent=1)
    print("\n".join(out[:12]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
