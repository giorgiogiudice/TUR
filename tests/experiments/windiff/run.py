#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Runs the windiff check programs and records what each check observed.

    run.py --bin DIR --root ROOT --out results.json --label NAME
           [--wine WINE [--wineserver WINESERVER]] [--repeat 2] [--timeout 300] [--only prog.group,...]

DIR holds the programs checks/build.sh made (manifest.txt names them). ROOT
is the directory that is drive C: for the programs: `C:\\` on Windows, the
prefix's drive_c under Wine. The programs are copied to ROOT/wdbin and each
group runs in a fresh ROOT/wd/<prog>-<group>, so that every path a check
sees is the same string on every machine.

With --wine the programs are started through that Wine (WINEPREFIX must be
set); without it they are started directly, which is what the Windows job
does. Every group is run --repeat times; a check whose value changes between
runs on one machine is marked unstable and is not compared.

Each program is started through wdlaunch.exe, which gives it a console
without a window and a job that is closed when the launcher is killed.

The output is one JSON document:
  {"label", "platform", "groups": {"prog.group": {"status", "rc", "seconds"}},
   "checks": {"prog.group.check": {"v": "...", "unstable": false}}}
status is ok, crash (the program ended without finishing the group; rc is
its exit code) or timeout. Standard error of every run is kept under
<out>.logs/.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import time


def remove_tree(path: pathlib.Path) -> None:
    if not path.exists():
        return
    target = "\\\\?\\" + str(path) if os.name == "nt" else str(path)

    def on_error(func, name, _exc):
        try:
            os.chmod(name, 0o700)
            func(name)
        except OSError:
            pass

    shutil.rmtree(target, onerror=on_error)
    if path.exists() and os.name == "nt":
        subprocess.run(["cmd", "/c", "rd", "/s", "/q", target], capture_output=True)
    elif path.exists():
        subprocess.run(["chmod", "-R", "u+rwx", str(path)], capture_output=True)
        shutil.rmtree(str(path), ignore_errors=True)


class Runner:
    def __init__(self, args):
        self.args = args
        self.root = pathlib.Path(args.root)
        self.bin = self.root / "wdbin"
        self.scratch = self.root / "wd"
        self.logs = pathlib.Path(str(args.out) + ".logs")
        self.logs.mkdir(parents=True, exist_ok=True)
        self.server = args.wineserver or ((args.wine or "") + "server")
        remove_tree(self.bin)
        shutil.copytree(args.bin, self.bin)
        remove_tree(self.scratch)
        self.scratch.mkdir(parents=True)

    def command(self, prog: str, *rest: str) -> list[str]:
        launcher, target = "C:\\wdbin\\wdlaunch.exe", "C:\\wdbin\\" + prog
        if self.args.wine:
            return [self.args.wine, launcher, target, *rest]
        return [launcher, target, *rest]

    def run(self, prog: str, rest: list[str], cwd: pathlib.Path, timeout: int):
        kwargs = {}
        if os.name == "nt":
            kwargs["creationflags"] = 0x00000008  # DETACHED_PROCESS: the launcher makes the console
        started = time.time()
        proc = subprocess.Popen(self.command(prog, *rest), cwd=str(cwd), stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs)
        try:
            out, err = proc.communicate(timeout=timeout)
            status = None
        except subprocess.TimeoutExpired:
            proc.kill()
            if self.args.wine:
                subprocess.run([self.server, "-k"], capture_output=True)
            try:
                out, err = proc.communicate(timeout=30)
            except subprocess.TimeoutExpired:
                out, err = b"", b""
            status = "timeout"
            if self.args.wine:
                subprocess.run([self.server, "-p"], capture_output=True)
        return status, proc.returncode, out, err, time.time() - started

    def groups(self, prog: str) -> list[str]:
        _status, _rc, out, _err, _secs = self.run(prog, ["--list"], self.scratch, 120)
        return [line.strip() for line in out.decode(errors="replace").splitlines() if line.strip()]

    def group(self, prog: str, group: str, rep: int):
        name = prog[:-4]
        cwd = self.scratch / f"{name}-{group}"
        remove_tree(cwd)
        cwd.mkdir(parents=True)
        status, rc, out, err, secs = self.run(prog, ["--run", group], cwd, self.args.timeout)
        (self.logs / f"{name}.{group}.{rep}.stderr.txt").write_bytes(err[-400000:])
        (self.logs / f"{name}.{group}.{rep}.stdout.txt").write_bytes(out[-2000000:])
        checks: dict[str, str] = {}
        done = False
        for line in out.decode("utf-8", errors="replace").splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if "done" in record:
                done = True
            elif "id" in record:
                key, n = record["id"], 2
                while key in checks:
                    key = f"{record['id']}#{n}"
                    n += 1
                checks[key] = record.get("v", "")
        if status is None:
            status = "ok" if done and rc == 0 else "crash"
        remove_tree(cwd)
        return {"status": status, "rc": rc, "seconds": round(secs, 2)}, checks


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--root", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--label", required=True)
    ap.add_argument("--wine")
    ap.add_argument("--wineserver")
    ap.add_argument("--repeat", type=int, default=2)
    ap.add_argument("--timeout", type=int, default=300)
    ap.add_argument("--only", default="")
    args = ap.parse_args()

    runner = Runner(args)
    only = {item for item in args.only.split(",") if item}
    programs = [line.strip() for line in (pathlib.Path(args.bin) / "manifest.txt").read_text().splitlines() if line.strip()]
    result = {"label": args.label,
              "platform": {"system": platform.system(), "release": platform.release(), "version": platform.version(),
                           "machine": platform.machine(), "python": platform.python_version(),
                           "wine": args.wine or ""},
              "groups": {}, "checks": {}}
    for prog in programs:
        names = runner.groups(prog)
        if not names:
            result["groups"][prog[:-4] + ".(list)"] = {"status": "crash", "rc": None, "seconds": 0}
            print(f"{prog}: could not list its groups", flush=True)
            continue
        for group in names:
            full = f"{prog[:-4]}.{group}"
            if only and full not in only and prog[:-4] not in only:
                continue
            runs = [runner.group(prog, group, rep) for rep in range(args.repeat)]
            info, checks = runs[0]
            info["statuses"] = [r[0]["status"] for r in runs]
            # the worst status of the repeats is the group's status
            for status in ("timeout", "crash"):
                if status in info["statuses"]:
                    info["status"] = status
            unstable = 0
            for key, value in checks.items():
                differs = any(other[1].get(key) != value for other in runs[1:])
                unstable += differs
                result["checks"][key] = {"v": value, "unstable": differs}
            for other in runs[1:]:
                for key, value in other[1].items():
                    if key not in checks:
                        result["checks"][key] = {"v": value, "unstable": True}
                        unstable += 1
            info["checks"] = len(checks)
            info["unstable"] = unstable
            result["groups"][full] = info
            print(f"{full}: {info['status']} rc={info['rc']} checks={len(checks)} unstable={unstable} "
                  f"{info['seconds']}s", flush=True)
    pathlib.Path(args.out).write_text(json.dumps(result, indent=1, sort_keys=True, ensure_ascii=False), encoding="utf-8")
    total = len(result["checks"])
    bad = sum(1 for g in result["groups"].values() if g["status"] != "ok")
    print(f"{args.label}: {total} checks in {len(result['groups'])} groups; {bad} groups did not finish")
    return 0


if __name__ == "__main__":
    sys.exit(main())
