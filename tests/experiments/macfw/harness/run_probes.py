#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Run every group of every macfw probe and collect what it printed.

The same probe binaries (universal Mach-O, built once on the macOS runner)
run on both sides:

  macOS (the oracle)       ./run_probes.py --side macos  --bin DIR --out results-macos-arm64.json
                           ./run_probes.py --side macos  --arch x86_64 ...   (under Rosetta, if present)
  Darling (the candidate)  ./run_probes.py --side darling --bin DIR --out results-darling-x86_64.json

Each group runs twice in a fresh process. A check whose value differs
between the two runs is marked unstable and the differ ignores it. A group's
status is one of: ok, crash (signal), exit:N, timeout, load-fail (dyld could
not bind a library or symbol; the message is kept), no-done (the process
ended without its last line).

Darling rules (darling-skills, darling-guest-testing): never pipe
`darling shell` output, always redirect to a file; always kill with
`timeout -s KILL`; give it /dev/null as input; run host files through
/Volumes/SystemRoot.
"""
import argparse
import json
import os
import platform
import re
import subprocess
import sys
import tempfile
import time

PROBES = ["fnd", "win", "view", "ctl", "tbl", "evt", "swak", "sui"]
LOAD_FAIL = re.compile(r"(dyld[^\n]*(Library not loaded|Symbol not found|missing symbol)[^\n]*(\n[^\n]*){0,3})")


def command(side, arch, path, args, display):
    if side == "macos":
        if arch and arch != platform.machine():
            return ["arch", "-" + arch, path, *args]
        return [path, *args]
    guest = "/Volumes/SystemRoot" + os.path.abspath(path)
    env = ["env", "DISPLAY=" + display, "MACFW=1"]
    if os.environ.get("MACFW_BACKEND"):
        env.append("DARLING_APPKIT_BACKEND=" + os.environ["MACFW_BACKEND"])
    return ["darling", "shell", *env, guest, *args]


def run_once(side, arch, path, args, timeout, display):
    with tempfile.TemporaryDirectory() as tmp:
        out_p, err_p = os.path.join(tmp, "out"), os.path.join(tmp, "err")
        cmd = command(side, arch, path, args, display)
        if side == "darling":
            cmd = ["timeout", "-s", "KILL", str(timeout), *cmd]
        start = time.monotonic()
        with open(out_p, "wb") as out, open(err_p, "wb") as err, open(os.devnull, "rb") as nul:
            try:
                rc = subprocess.run(cmd, stdin=nul, stdout=out, stderr=err, timeout=timeout + 15).returncode
            except subprocess.TimeoutExpired:
                rc = "timeout"
        took = round(time.monotonic() - start, 2)
        with open(out_p, "rb") as f:
            out = f.read().decode("utf-8", "replace")
        with open(err_p, "rb") as f:
            err = f.read().decode("utf-8", "replace")
    if side == "darling" and rc in (-9, 137):
        rc = "timeout"  # timeout -s KILL exits with 128 + 9
    return rc, out, err, took


def parse(out):
    checks, done = {}, False
    for line in out.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            obj = json.loads(line)
        except ValueError:
            continue
        if "done" in obj:
            done = True
        elif "id" in obj:
            checks[obj["id"]] = {"k": obj.get("k", "exact"), "v": obj.get("v")}
    return checks, done


def status_of(rc, err, done):
    m = LOAD_FAIL.search(err)
    if m and not done:
        return "load-fail", m.group(1).strip()[:600]
    if rc == "timeout":
        return "timeout", None
    if isinstance(rc, int) and rc < 0:
        return "crash", "signal %d" % -rc
    if isinstance(rc, int) and rc > 128 and not done:
        return "crash", "exit %d" % rc
    if not done:
        return "no-done", "exit %s" % rc
    if rc != 0:
        return "exit:%s" % rc, None
    return "ok", None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--side", choices=["macos", "darling"], required=True)
    ap.add_argument("--arch", default="", help="macOS: run this slice (arch -x86_64 under Rosetta)")
    ap.add_argument("--bin", required=True, help="directory with the probe binaries")
    ap.add_argument("--out", required=True)
    ap.add_argument("--timeout", type=int, default=60)
    ap.add_argument("--runs", type=int, default=2)
    ap.add_argument("--display", default=os.environ.get("DISPLAY", ":99"))
    ap.add_argument("--only", default="", help="comma-separated probes")
    a = ap.parse_args()

    probes = [p for p in PROBES if not a.only or p in a.only.split(",")]
    result = {"side": a.side, "arch": a.arch or platform.machine(), "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "groups": {}, "probes": {}}
    for prog in probes:
        path = os.path.join(a.bin, prog)
        if not os.path.exists(path):
            result["probes"][prog] = {"status": "not-built"}
            continue
        rc, out, err, took = run_once(a.side, a.arch, path, ["--list"], a.timeout, a.display)
        groups = [g.strip() for g in out.splitlines() if g.strip() and not g.startswith("{")]
        st, detail = status_of(rc, err, bool(groups))
        result["probes"][prog] = {"status": st if not groups else "ok", "detail": detail, "groups": groups,
                                  "seconds": took}
        if not groups:
            result["probes"][prog]["stderr_tail"] = err[-2000:]
            print("%s: --list failed: %s %s" % (prog, st, detail or ""), file=sys.stderr)
            continue
        for g in groups:
            runs = []
            for _ in range(a.runs):
                rc, out, err, took = run_once(a.side, a.arch, path, ["--run", g], a.timeout, a.display)
                checks, done = parse(out)
                st, detail = status_of(rc, err, done)
                runs.append({"status": st, "detail": detail, "checks": checks, "stderr_tail": err[-2000:], "seconds": took})
            first = runs[0]
            unstable = sorted(k for k in first["checks"]
                              if any(r["checks"].get(k, {}).get("v") != first["checks"][k]["v"] for r in runs[1:]))
            key = "%s.%s" % (prog, g)
            result["groups"][key] = {
                "status": first["status"], "detail": first["detail"],
                "statuses": [r["status"] for r in runs],
                "checks": first["checks"], "unstable": unstable,
                "stderr_tail": first["stderr_tail"], "seconds": [r["seconds"] for r in runs],
            }
            print("%-22s %-10s %4d checks %s" % (key, first["status"], len(first["checks"]),
                                                 ("unstable=%d" % len(unstable)) if unstable else ""))
    with open(a.out, "w") as f:
        json.dump(result, f, indent=1, sort_keys=True)


if __name__ == "__main__":
    main()
