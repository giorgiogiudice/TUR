#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""On GitHub's macOS runner: build the ladder's open-source applications
from their pinned commits with xcodebuild, unsigned, for arm64 and x86_64.

    ci/build_ladder_macos.py --ladder ladder/ladder.json --out OUT [--rungs 3-10]

OUT/apps/<App>.app        the built application, ad-hoc signed so it runs
OUT/ladder-build.json     per rung: commit, command, seconds, result, slices
OUT/ladder-build/<n>.log  xcodebuild's output

Only source is fetched (a shallow fetch of the pinned commit); SwiftPM
dependencies are resolved by xcodebuild from the versions pinned in the
project's Package.resolved. Nothing built here is Apple software.
"""
import argparse
import glob
import json
import os
import shlex
import shutil
import subprocess
import time

PER_RUNG_LIMIT = 15 * 60


def run(cmd, cwd=None, log=None, timeout=PER_RUNG_LIMIT):
    if log:
        log.write(("$ %s\n" % " ".join(shlex.quote(c) for c in cmd)).encode())
        log.flush()
    return subprocess.run(cmd, cwd=cwd, stdout=log or subprocess.DEVNULL, stderr=subprocess.STDOUT,
                          timeout=timeout).returncode


def rungs(spec):
    out = set()
    for part in filter(None, spec.split(",")):
        a, _, b = part.partition("-")
        out.update(range(int(a), int(b or a) + 1))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ladder", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--rungs", default="")
    a = ap.parse_args()
    ladder = json.load(open(a.ladder))
    want = rungs(a.rungs)
    out = os.path.abspath(a.out)
    os.makedirs(os.path.join(out, "apps"), exist_ok=True)
    os.makedirs(os.path.join(out, "ladder-build"), exist_ok=True)
    src_root = os.path.join(out, "src")
    results = []
    for r in ladder["rungs"]:
        if r["source"] == "own" or (want and r["rung"] not in want):
            continue
        res = {"rung": r["rung"], "name": r["name"], "commit": r["commit"], "result": "not-run"}
        log = open(os.path.join(out, "ladder-build", "%02d-%s.log" % (r["rung"], r["name"])), "wb")
        t0 = time.monotonic()
        try:
            src = os.path.join(src_root, r["name"])
            os.makedirs(src, exist_ok=True)
            for cmd in (["git", "init", "-q"], ["git", "remote", "add", "origin", r["source"] + ".git"],
                        ["git", "fetch", "-q", "--depth", "1", "origin", r["commit"]],
                        ["git", "checkout", "-q", "FETCH_HEAD"],
                        ["git", "submodule", "update", "--init", "--depth", "1", "--recursive"]):
                if run(cmd, src, log, 600) != 0 and cmd[1] != "submodule":
                    raise RuntimeError("fetch failed: %s" % " ".join(cmd))
            b = r["build"]
            for prep in b.get("prepare", []):
                run(["/bin/sh", "-c", prep], src, log, 120)
            sym = os.path.join(src, "macfw-build")
            cmd = ["xcodebuild", "-project", b["project"], "-configuration", "Release",
                   "-derivedDataPath", os.path.join(src, "macfw-dd"),
                   "-skipPackagePluginValidation", "-skipMacroValidation"]
            cmd += ["-scheme", b["scheme"]] if "scheme" in b else ["-target", b["target"]]
            settings = {"ARCHS": "arm64 x86_64", "ONLY_ACTIVE_ARCH": "NO", "CODE_SIGNING_ALLOWED": "NO",
                        "CODE_SIGN_IDENTITY": "", "SYMROOT": sym, "ENABLE_HARDENED_RUNTIME": "NO"}
            settings.update(b.get("settings", {}))
            cmd += ["%s=%s" % kv for kv in settings.items()] + ["build"]
            res["command"] = " ".join(shlex.quote(c) for c in cmd)
            rc = run(cmd, src, log)
            res["xcodebuild_rc"] = rc
            found = [p for p in glob.glob(os.path.join(src, "**", r["app"]), recursive=True)
                     if "/Release/" in p and "/Index" not in p]
            if rc != 0 or not found:
                res["result"] = "build-failed"
                continue
            dest = os.path.join(out, "apps", r["app"])
            shutil.rmtree(dest, ignore_errors=True)
            shutil.copytree(found[0], dest, symlinks=True)
            run(["codesign", "--force", "--deep", "--sign", "-", dest], None, log, 300)
            # The artifact of a public repository can be downloaded by others:
            # keep each application's licence next to its build.
            for lic in glob.glob(os.path.join(src, "LICEN[CS]E*")) + glob.glob(os.path.join(src, "COPYING*")):
                shutil.copy(lic, os.path.join(out, "apps", "%s.%s" % (r["app"], os.path.basename(lic))))
            exe_dir = os.path.join(dest, "Contents", "MacOS")
            exe = os.path.join(exe_dir, sorted(os.listdir(exe_dir))[0])
            res["slices"] = subprocess.run(["lipo", "-archs", exe], capture_output=True, text=True).stdout.strip()
            res["result"] = "built"
        except (RuntimeError, subprocess.TimeoutExpired, OSError) as e:
            res["result"] = "error: %s" % e
        finally:
            res["seconds"] = round(time.monotonic() - t0)
            log.close()
            results.append(res)
            print("rung %2d %-12s %s %ss %s" % (r["rung"], r["name"], res["result"], res["seconds"], res.get("slices", "")))
    json.dump(results, open(os.path.join(out, "ladder-build.json"), "w"), indent=1)
    shutil.rmtree(src_root, ignore_errors=True)


if __name__ == "__main__":
    main()
