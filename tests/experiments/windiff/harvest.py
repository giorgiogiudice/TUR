#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Helpers of ci/harvest.sh: where real applications touch what Wine lacks.

    harvest.py fetch APPS_JSON DEST      download the applications at their pinned hashes
    harvest.py wav FILE                  write two seconds of a 440 Hz tone
    harvest.py report LOGS DRIVE_C WINE_SPECS OUT
                                         count Wine's fixme/err lines by function, and set every
                                         function the applications import against Wine's .spec files
"""

from __future__ import annotations

import collections
import hashlib
import json
import math
import os
import pathlib
import re
import struct
import subprocess
import sys
import urllib.request
import wave

WANTED = ["7zip-exe", "notepadpp-zip", "putty-exe", "innosetup", "vlc-nsis"]


def fetch(apps_json: str, dest: str) -> int:
    apps = {a["id"]: a for a in json.loads(pathlib.Path(apps_json).read_text())["applications"]}
    for app_id in WANTED:
        app = apps.get(app_id)
        if not app:
            print(f"{app_id}: not in apps.json")
            continue
        suffix = ".zip" if app["url"].endswith(".zip") else ".exe"
        target = pathlib.Path(dest) / (app_id + suffix)
        try:
            request = urllib.request.Request(app["url"], headers={"User-Agent": "windiff-harvest"})
            with urllib.request.urlopen(request, timeout=300) as response:
                data = response.read()
        except OSError as error:
            print(f"{app_id}: download failed: {error}")
            continue
        digest = hashlib.sha256(data).hexdigest()
        if digest != app["sha256"]:
            print(f"{app_id}: hash mismatch ({digest}), not used")
            continue
        target.write_bytes(data)
        print(f"{app_id}: {app['name']} {app['version']}, {len(data)} bytes, sha256 ok")
    return 0


def wav(path: str) -> int:
    with wave.open(path, "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(22050)
        out.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / 22050)))
                                 for i in range(22050 * 2)))
    return 0


def parse_specs(root: pathlib.Path):
    """Wine's exports: {module name without extension: {function: 'impl'|'stub'}} and its API sets."""
    exports: dict[str, dict[str, str]] = {}
    apisets: dict[str, str] = {}
    for spec in sorted(root.glob("dlls/*/*.spec")):
        module = spec.parent.name.lower()
        module = module[:-4] if module.endswith(".dll") else module
        table = exports.setdefault(module, {})
        for line in spec.read_text(errors="replace").splitlines():
            line = line.split("#", 1)[0].strip()
            if not line:
                continue
            fields = line.split()
            if fields[0] == "apiset" and len(fields) >= 4:
                apisets[fields[1].lower()] = fields[3].lower()
                continue
            if len(fields) < 3:
                continue
            kind = fields[1]
            rest = [f for f in fields[2:] if not f.startswith("-")]
            if not rest:
                continue
            name = rest[0].split("(", 1)[0]
            table[name] = "stub" if kind == "stub" else "impl"
    return exports, apisets


def apiset_host(dll: str, apisets: dict[str, str]):
    name = dll[:-4] if dll.endswith(".dll") else dll
    if name in apisets:
        return apisets[name]
    # api-ms-win-core-file-l1-2-3 resolves like the newest version Wine knows of the same contract
    stem = re.sub(r"-\d+$", "", name)
    for known, host in apisets.items():
        if re.sub(r"-\d+$", "", known) == stem:
            return host
    stem2 = re.sub(r"-\d+-\d+$", "", name)
    for known, host in apisets.items():
        if re.sub(r"-\d+-\d+$", "", known) == stem2:
            return host
    return None


def imports_of(path: pathlib.Path):
    try:
        text = subprocess.run(["x86_64-w64-mingw32-objdump", "-p", str(path)], capture_output=True, text=True,
                              errors="replace", timeout=120).stdout
    except (OSError, subprocess.TimeoutExpired):
        return []
    found, dll = [], None
    for line in text.splitlines():
        match = re.match(r"^\s*DLL Name: (\S+)", line)
        if match:
            dll = match.group(1).lower()
            continue
        if line.startswith("The Export Tables") or line.startswith("The Function Table"):
            dll = None
        match = re.match(r"^\t[0-9a-f]+\t\s*\d+\s+([A-Za-z_?@$][\w?@$]*)\s*$", line)
        if match and dll:
            found.append((dll, match.group(1)))
    return found


def report(logs: str, drive_c: str, specs: str, out: str) -> int:
    out_dir = pathlib.Path(out)
    # 1. what Wine said while the applications ran
    counts: dict[tuple[str, str, str], collections.Counter] = collections.defaultdict(collections.Counter)
    aborts: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    for log in sorted(pathlib.Path(logs).glob("*.log")):
        app = log.stem.split("-")[0]
        text = log.read_text(errors="replace")
        for kind, channel, func in re.findall(r"\b(fixme|err):([a-z0-9_]+):([A-Za-z0-9_]+)", text):
            counts[(kind, channel, func)][app] += 1
        for func in re.findall(r"unimplemented function (\S+?)[,\s]", text):
            aborts[func][app] += 1
    rows = sorted(counts.items(), key=lambda item: (-len(item[1]), -sum(item[1].values())))
    with (out_dir / "fixmes.tsv").open("w") as handle:
        handle.write("kind\tchannel\tfunction\tapplications\tlines\n")
        for (kind, channel, func), apps in rows:
            handle.write(f"{kind}\t{channel}\t{func}\t{','.join(sorted(apps))}\t{sum(apps.values())}\n")
    fixmes = [(key, apps) for key, apps in rows if key[0] == "fixme"]
    errs = [(key, apps) for key, apps in rows if key[0] == "err"]
    print("## Harvest: what TUR's applications hit in Wine")
    print()
    print(f"- distinct `fixme:` sites: **{len(fixmes)}**; distinct `err:` sites: **{len(errs)}**; "
          f"unimplemented-function aborts: **{len(aborts)}**")
    by_channel = collections.Counter()
    for (kind, channel, _func), apps in fixmes:
        by_channel[channel] += 1
    print("- `fixme:` sites by channel: " + ", ".join(f"{c} {n}" for c, n in by_channel.most_common(25)))
    print()
    print("| Kind | Channel | Function | Applications | Lines |")
    print("|---|---|---|---|---|")
    for (kind, channel, func), apps in rows[:70]:
        print(f"| {kind} | {channel} | {func} | {', '.join(sorted(apps))} | {sum(apps.values())} |")
    print()
    for func, apps in sorted(aborts.items()):
        print(f"- abort: unimplemented `{func}` ({', '.join(sorted(apps))})")
    print()

    # 2. what the applications import
    exports, apisets = parse_specs(pathlib.Path(specs))
    root = pathlib.Path(drive_c)
    files = []
    for path in root.rglob("*"):
        rel = path.relative_to(root).as_posix().lower()
        if rel.startswith("windows/") or rel.startswith("users/") or rel.startswith("programdata/"):
            continue
        if path.suffix.lower() in (".exe", ".dll") and path.is_file():
            files.append(path)
    own = {path.name.lower() for path in files}
    per_app: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    lacking: dict[tuple[str, str, str], set] = collections.defaultdict(set)
    with (out_dir / "imports.tsv").open("w") as handle:
        handle.write("application\tfile\tdll\tfunction\tclass\n")
        for path in sorted(files):
            rel = path.relative_to(root).parts
            app = rel[1] if rel[0].lower().startswith("program files") and len(rel) > 1 else rel[0]
            per_app[app]["files"] += 1
            for dll, func in imports_of(path):
                if dll in own:
                    cls = "application's own"
                else:
                    module = dll[:-4] if dll.endswith(".dll") else dll
                    if module.startswith(("api-ms-", "ext-ms-")):
                        host = apiset_host(dll, apisets)
                        module = (host[:-4] if host and host.endswith(".dll") else host) if host else None
                    if module is None:
                        cls = "API set unknown to Wine"
                    elif module not in exports:
                        cls = "DLL absent in Wine"
                    elif func not in exports[module]:
                        cls = "function absent in Wine"
                    else:
                        cls = "stub in Wine" if exports[module][func] == "stub" else "implemented"
                per_app[app][cls] += 1
                handle.write(f"{app}\t{'/'.join(rel)}\t{dll}\t{func}\t{cls}\n")
                if cls not in ("implemented", "application's own"):
                    lacking[(cls, dll, func)].add(app)
    classes = ["implemented", "stub in Wine", "function absent in Wine", "DLL absent in Wine", "API set unknown to Wine",
               "application's own"]
    print("### Imports of the applications' programs and libraries, against Wine's .spec files")
    print()
    print("| Application | Files | " + " | ".join(classes) + " |")
    print("|---|---|" + "---|" * len(classes))
    for app in sorted(per_app):
        print(f"| {app} | {per_app[app]['files']} | " + " | ".join(str(per_app[app][c]) for c in classes) + " |")
    print()
    print(f"Distinct imported functions Wine lacks: **{len(lacking)}**")
    print()
    if lacking:
        print("| Class | DLL | Function | Applications |")
        print("|---|---|---|---|")
        for (cls, dll, func), apps in sorted(lacking.items())[:150]:
            print(f"| {cls} | {dll} | {func} | {', '.join(sorted(apps))} |")
    return 0


def main() -> int:
    command = sys.argv[1]
    if command == "fetch":
        return fetch(sys.argv[2], sys.argv[3])
    if command == "wav":
        return wav(sys.argv[2])
    if command == "report":
        return report(*sys.argv[2:6])
    return 2


if __name__ == "__main__":
    sys.exit(main())
