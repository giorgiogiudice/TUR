#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The integration test of tur-rehost on a few real, open-source packages
(tests/rehost/corpus.json), run by tests/rehost/run.sh in Fedora 45.

For each package, with the program the workflow built:

  1. download the publisher's own release asset and check its pinned SHA-256
     (this is the test fetching its input, not tur-rehost);
  2. `detect --json`: Electron, at the version the experiment read;
  3. `plan --json`: ready (or, for a package that needs a native module
     built, refused with TUR-REHOST-NATIVE-002 until builds are allowed);
  4. `install --json`: the runtime fetched and verified against TUR's pinned
     index, the payload laid under it, a record written;
  5. `launch -- --disable-gpu` on a headless X server: a window must be
     mapped within 45 seconds (the X server has no GPU; the experiment
     started every Chromium program with the same flag);
  6. `uninstall --json`: nothing may be left behind.

Results: out/results.json and out/results.md. Exit status 1 when any
package fails a step. Nothing here runs a package's installer.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
import urllib.request


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(4 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fetch(variant, downloads):
    os.makedirs(downloads, exist_ok=True)
    path = os.path.join(downloads, variant["url"].rsplit("/", 1)[1])
    if not (os.path.exists(path) and sha256(path) == variant["sha256"]):
        request = urllib.request.Request(variant["url"], headers={"User-Agent": "tur-rehost-integration/1"})
        with urllib.request.urlopen(request, timeout=600) as response, open(path + ".part", "wb") as out:
            while True:
                chunk = response.read(4 << 20)
                if not chunk:
                    break
                out.write(chunk)
        os.replace(path + ".part", path)
    actual = sha256(path)
    if actual != variant["sha256"]:
        raise RuntimeError(f"{variant['url']}: SHA-256 {actual}, pinned {variant['sha256']}")
    return path


def run(program, data, *arguments, timeout=1800):
    command = [program, "--data-dir", data, *arguments]
    started = time.time()
    done = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    try:
        document = json.loads(done.stdout) if done.stdout.strip() else None
    except ValueError:
        document = None
    return done.returncode, document, done.stderr[-4000:], round(time.time() - started, 1)


def windows():
    done = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "."], capture_output=True, text=True)
    return [line for line in done.stdout.split() if line.strip()]


def launch(program, data, app_id, out, seconds=45):
    before = set(windows())
    log = open(os.path.join(out, f"launch-{app_id}.log"), "w")
    process = subprocess.Popen(
        [program, "--data-dir", data, "launch", app_id, "--", "--disable-gpu"],
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    started = time.time()
    first = None
    while time.time() - started < seconds:
        if process.poll() is not None:
            break
        if set(windows()) - before:
            first = round(time.time() - started, 1)
            break
        time.sleep(0.5)
    alive = process.poll() is None
    if alive:
        time.sleep(3)  # let it draw before it is stopped
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    log.close()
    return {"window": first is not None, "seconds_to_window": first, "exited_early": not alive and first is None, "exit_status": process.returncode}


def check(variant, program, data, downloads, out, builds):
    result = {"key": variant["key"], "app_id": variant["app_id"], "platform": variant["platform"], "steps": {}}
    steps = result["steps"]
    package = fetch(variant, downloads)
    status, document, error, seconds = run(program, data, "detect", "--json", package)
    finding = (document or {}).get("findings", [{}])[0] if document else {}
    steps["detect"] = {"status": status, "seconds": seconds, "family": finding.get("family"), "version": finding.get("version"), "layers": (document or {}).get("layers")}
    if status != 0 or finding.get("family") != "electron" or finding.get("version") != variant["electron"]:
        steps["detect"]["error"] = error
        return result, False
    options = ["--app-id", variant["app_id"]]
    if variant["needs_builds"]:
        status, document, error, seconds = run(program, data, "plan", "--json", *options, package)
        refused = (document or {}).get("blockers", [{}])[0].get("code") if document else None
        steps["plan-without-builds"] = {"status": status, "blocker": refused}
        if status != 4 or refused != "TUR-REHOST-NATIVE-002":
            return result, False
        if not builds:
            steps["install"] = {"skipped": "native builds were not asked for (workflow input 'builds')"}
            return result, True
        options.append("--allow-builds")
    status, document, error, seconds = run(program, data, "plan", "--json", *options, package)
    steps["plan"] = {"status": status, "seconds": seconds, "ready": (document or {}).get("ready"), "downloads": len((document or {}).get("downloads", []))}
    if status != 0:
        steps["plan"]["error"] = error or document
        return result, False
    status, document, error, seconds = run(program, data, "install", "--json", *options, package, timeout=3600)
    record = (document or {}).get("record", {})
    steps["install"] = {
        "status": status,
        "seconds": seconds,
        "runtime": record.get("runtime"),
        "modules": record.get("modules"),
        "fuses_set": record.get("fuses_set"),
        "payload_sha256": record.get("payload_sha256"),
    }
    if status != 0:
        steps["install"]["error"] = (document or {}).get("error") or error
        return result, False
    steps["launch"] = launch(program, data, variant["app_id"], out)
    status, document, error, seconds = run(program, data, "uninstall", "--json", variant["app_id"])
    steps["uninstall"] = {"status": status, "left_behind": (document or {}).get("left_behind")}
    app_dir = os.path.join(data, "data", "apps", variant["app_id"])
    steps["uninstall"]["directory_gone"] = not os.path.exists(app_dir)
    ok = steps["launch"]["window"] and status == 0 and steps["uninstall"]["directory_gone"]
    return result, ok


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--program", required=True)
    parser.add_argument("--corpus", required=True)
    parser.add_argument("--data", required=True)
    parser.add_argument("--downloads", required=True)
    parser.add_argument("--out", required=True)
    options = parser.parse_args()
    corpus = json.load(open(options.corpus))
    only = {item for item in os.environ.get("REHOST_ONLY", "").split(",") if item}
    builds = os.environ.get("REHOST_BUILDS") == "true"
    results, failed = [], []
    for variant in corpus["variants"]:
        if only and variant["key"] not in only:
            continue
        print(f"== {variant['key']} ({variant['platform']})", flush=True)
        try:
            result, ok = check(variant, options.program, options.data, options.downloads, options.out, builds)
        except Exception as problem:  # a failed step is a finding, reported, not a crash
            result, ok = {"key": variant["key"], "error": repr(problem)}, False
        result["ok"] = ok
        results.append(result)
        print(json.dumps(result, indent=1), flush=True)
        if not ok:
            failed.append(variant["key"])
    with open(os.path.join(options.out, "results.json"), "w") as handle:
        json.dump(results, handle, indent=1)
    with open(os.path.join(options.out, "results.md"), "w") as handle:
        handle.write("| Package | Platform | Detect | Plan | Install | Window | Removed |\n|---|---|---|---|---|---|---|\n")
        for result in results:
            steps = result.get("steps", {})
            window = steps.get("launch", {})
            handle.write(
                f"| {result['key']} | {result.get('platform', '')} | {steps.get('detect', {}).get('version', '—')} "
                f"| {steps.get('plan', {}).get('status', '—')} | {steps.get('install', {}).get('status', steps.get('install', {}).get('skipped', '—'))} "
                f"| {window.get('seconds_to_window', '—') if window.get('window') else 'no'} | {steps.get('uninstall', {}).get('directory_gone', '—')} |\n"
            )
    if failed:
        print("failed: " + ", ".join(failed), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
