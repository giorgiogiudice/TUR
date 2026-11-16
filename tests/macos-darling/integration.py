#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The macOS route's integration test, as the user, in the base-image container.

Steps, each recorded in results.json whatever happens:

 1. import TUR's build of Darling from the darling-runtime artifact
    (`tur-runtime mac runtime fetch --from-file --sha256`), and check its status;
 2. `mac identify` every bundle the oracle built, and check the refusals TUR
    promises: SwiftUI (rung8-swiftui) TUR-RUNTIME-MACOS-004, Swift with the
    system runtime (rung6-swift) TUR-RUNTIME-MACOS-005; an arm64-only program
    made from the oracle's universal one is TUR-RUNTIME-MACOS-002;
 3. for each Darling mode and each chosen application, from its bundle, from
    a zip of it and from the oracle's disk images (HFS+ and APFS): install,
    start under Xvfb, wait for a window, screenshot, drive TUR's own rung 1
    (type, Return, look for its line in the launch log), stop, remove, and
    check that nothing is left;
 4. write results.json and summary.md.

Green means the harness ran. What worked is the finding.
"""

import argparse
import glob
import json
import os
import shutil
import signal
import struct
import subprocess
import sys
import time

WINDOW_SECONDS = 90
SETTLE_SECONDS = 5
DISPLAY = ":99"


def run(command, timeout=1800, env=None):
    started = time.monotonic()
    try:
        done = subprocess.run(command, capture_output=True, text=True, timeout=timeout, env=env)
        status, out, err = done.returncode, done.stdout, done.stderr
    except subprocess.TimeoutExpired as expired:
        status, out, err = "timeout", expired.stdout or "", expired.stderr or ""
        if isinstance(out, bytes):
            out = out.decode("utf-8", "replace")
        if isinstance(err, bytes):
            err = err.decode("utf-8", "replace")
    return {
        "command": command,
        "status": status,
        "seconds": round(time.monotonic() - started, 1),
        "stdout": out[-6000:],
        "stderr": err[-6000:],
    }


def as_json(result):
    try:
        return json.loads(result["stdout"])
    except (ValueError, TypeError):
        return None


def error_code(result):
    document = as_json(result) or {}
    error = document.get("error") or {}
    return error.get("code")


def environment():
    env = dict(os.environ)
    env["DISPLAY"] = DISPLAY
    return env


def windows():
    listed = run(["xdotool", "search", "--onlyvisible", "--name", "."], timeout=20, env=environment())
    return set(listed["stdout"].split()) if listed["status"] == 0 else set()


def screenshot(path):
    return run(["import", "-display", DISPLAY, "-window", "root", path], timeout=60)["status"] == 0


def du(path):
    result = run(["du", "-sb", path], timeout=300)
    try:
        return int(result["stdout"].split()[0])
    except (IndexError, ValueError):
        return None


def arm64_only(universal, dest):
    """The arm64 image of a universal Mach-O file, as a thin file of its own."""
    with open(universal, "rb") as handle:
        data = handle.read()
    if struct.unpack_from(">I", data, 0)[0] != 0xCAFEBABE:
        return False
    count = struct.unpack_from(">I", data, 4)[0]
    for index in range(min(count, 8)):
        cpu, _, offset, size, _ = struct.unpack_from(">iiIII", data, 8 + index * 20)
        if cpu == 0x0100000C:
            with open(dest, "wb") as out:
                out.write(data[offset:offset + size])
            return True
    return False


class Harness:
    def __init__(self, args):
        self.tur = args.tur_runtime
        self.darling = args.darling
        self.oracle = args.oracle
        self.out = args.results
        self.results = {"steps": [], "apps": [], "refusals": [], "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
        self.home = os.environ["HOME"]
        self.state = os.path.join(self.home, ".local", "state", "tur", "runtime")
        self.data = os.path.join(self.home, ".local", "share", "tur", "runtime")

    def tur_runtime(self, *arguments, timeout=1800):
        return run([self.tur, *arguments], timeout=timeout, env=environment())

    def step(self, name, result, ok):
        self.results["steps"].append({"name": name, "ok": ok, **result})
        print(f"{'ok  ' if ok else 'FAIL'} {name} ({result['seconds']} s, status {result['status']})", flush=True)
        return ok

    def import_runtime(self):
        record = json.load(open(os.path.join(self.darling, "build-record.json")))
        self.results["darling_build"] = {key: record.get(key) for key in (
            "id", "darling_commit", "queue_sha256", "sha256", "size", "unpacked_size", "build_seconds")}
        # in-container.sh put the build where the TUR image has it, with the
        # image's own script: tur-runtime must find it there and fetch nothing.
        status = self.tur_runtime("mac", "runtime", "status", "--json")
        document = as_json(status) or {}
        ok = self.step("Darling is part of the system", status, document.get("state") == "system")
        self.results["darling_system_bytes"] = du("/usr/lib/tur/darling")
        self.results["fetched_anything"] = os.path.exists(os.path.join(self.data, "darling"))
        return ok

    def identify_all(self):
        expected = {
            "rung8-swiftui.app": "TUR-RUNTIME-MACOS-004",
            "rung6-swift.app": "TUR-RUNTIME-MACOS-005",
        }
        for bundle in sorted(glob.glob(os.path.join(self.oracle, "apps", "*.app"))):
            name = os.path.basename(bundle)
            result = self.tur_runtime("mac", "identify", "--json", bundle)
            document = as_json(result) or {}
            application = document.get("application") or {}
            refusal = next((f for f in application.get("findings", [])
                            if f.get("severity") in ("refusal", "refused-unless-allowed")), None)
            entry = {"bundle": name, "status": result["status"], "verdict": document.get("verdict"),
                     "refusal": refusal and refusal.get("code"),
                     "frameworks": (application.get("program") or {}).get("frameworks")}
            if name in expected:
                entry["expected"] = expected[name]
                entry["as_expected"] = entry["refusal"] == expected[name] and result["status"] == 3
            self.results["refusals"].append(entry)
            print(f"identify {name}: {entry['verdict']} {entry['refusal'] or ''}", flush=True)
        # An arm64-only program, cut from the oracle's universal rung 1.
        universal = os.path.join(self.oracle, "apps", "rung1-hello.app", "Contents", "MacOS", "rung1-hello")
        thin = os.path.join(self.out, "rung1-hello-arm64")
        if os.path.isfile(universal) and arm64_only(universal, thin):
            result = self.tur_runtime("mac", "identify", "--json", thin)
            document = as_json(result) or {}
            program = document.get("program") or {}
            refusal = next((f for f in program.get("findings", []) if f.get("severity") == "refusal"), {})
            self.results["refusals"].append({
                "bundle": "rung1-hello (arm64 slice only)", "status": result["status"],
                "refusal": refusal.get("code"), "expected": "TUR-RUNTIME-MACOS-002",
                "as_expected": refusal.get("code") == "TUR-RUNTIME-MACOS-002" and result["status"] == 3})

    def packages_of(self, bundle):
        """The bundle itself, a zip of it, and the oracle's disk images of it."""
        name = os.path.basename(bundle)
        stem = name[:-4].replace(" ", "-")
        packages = [("bundle", bundle)]
        archive = os.path.join(self.out, "packages", stem + ".zip")
        os.makedirs(os.path.dirname(archive), exist_ok=True)
        if not os.path.exists(archive):
            # -y keeps the bundle's symbolic links (Versions/Current) as links.
            subprocess.run(["zip", "-qry", archive, name], cwd=os.path.dirname(bundle),
                           capture_output=True, text=True, timeout=300)
        if os.path.isfile(archive):
            packages.append(("zip", archive))
        for kind in ("hfs", "apfs"):
            image = os.path.join(self.oracle, "dmg", f"{stem}-{kind}.dmg")
            if os.path.isfile(image):
                packages.append((f"dmg-{kind}", image))
        return packages

    def exercise(self, bundle, kind, package, mode):
        name = os.path.basename(bundle)
        slug = "".join(c if c.isalnum() else "-" for c in name[:-4].lower()).strip("-")
        app_id = f"org.tur.test.{slug}.{kind}.{mode}"
        entry = {"bundle": name, "package": kind, "mode": mode, "app_id": app_id}
        self.results["apps"].append(entry)
        install = self.tur_runtime("mac", "install", package, "--app-id", app_id,
                                   "--darling-mode", mode, "--json", timeout=3600)
        entry["install_status"] = install["status"]
        entry["install_seconds"] = install["seconds"]
        entry["install_error"] = error_code(install)
        self.step(f"install {name} from {kind} ({mode})", install, install["status"] == 0)
        if install["status"] != 0:
            entry["install_stderr"] = install["stderr"][-2000:]
            entry["install_stdout"] = install["stdout"][-4000:]
            return
        entry["prefix_bytes"] = du(os.path.join(self.data, "prefixes", app_id))

        before = windows()
        log_dir = os.path.join(self.state, "logs", app_id)
        launched = subprocess.Popen([self.tur, "mac", "launch", app_id, "--json"],
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    env=environment(), start_new_session=True)
        started = time.monotonic()
        window = None
        while time.monotonic() - started < WINDOW_SECONDS and launched.poll() is None:
            new = windows() - before
            if new:
                window = sorted(new)[0]
                break
            time.sleep(1)
        entry["window"] = window is not None
        entry["seconds_to_window"] = round(time.monotonic() - started, 1) if window else None
        entry["exited_early"] = launched.poll()
        if window:
            time.sleep(SETTLE_SECONDS)
            entry["alive_after_settle"] = launched.poll() is None
            shot = os.path.join(self.out, f"{app_id}.png")
            entry["screenshot"] = os.path.basename(shot) if screenshot(shot) else None
            if name == "rung1-hello.app":
                env = environment()
                run(["xdotool", "windowfocus", "--sync", window], timeout=20, env=env)
                run(["xdotool", "type", "--delay", "50", "abc"], timeout=20, env=env)
                run(["xdotool", "key", "Return"], timeout=20, env=env)
                time.sleep(3)
        if launched.poll() is None:
            os.killpg(launched.pid, signal.SIGTERM)
            try:
                launched.wait(timeout=20)
            except subprocess.TimeoutExpired:
                os.killpg(launched.pid, signal.SIGKILL)
                launched.wait()
        stdout, stderr = launched.communicate()
        entry["launch_stderr"] = stderr.decode("utf-8", "replace")[-2000:]
        logs = sorted(glob.glob(os.path.join(log_dir, "*.log")))
        text = ""
        for log in logs:
            if os.path.basename(log) == "operations.log":
                continue
            with open(log, "rb") as handle:
                text += handle.read().decode("utf-8", "replace")
        entry["log_tail"] = text[-4000:]
        if name == "rung1-hello.app":
            entry["ready_line"] = "macfw-ready" in text
            entry["interaction"] = "macfw-click 1 text=abc" in text
        if os.path.isdir(log_dir):
            shutil.copytree(log_dir, os.path.join(self.out, "logs", app_id), dirs_exist_ok=True)

        removed = self.tur_runtime("mac", "uninstall", app_id, "--json")
        entry["uninstall_status"] = removed["status"]
        left = [path for path in (
            os.path.join(self.data, "prefixes", app_id),
            os.path.join(self.data, "launchers", app_id),
            os.path.join(self.data, "staging", app_id),
            log_dir) if os.path.lexists(path)]
        entry["left_behind"] = left
        self.step(f"remove {app_id}", removed, removed["status"] == 0 and not left)

    def summary(self):
        lines = ["## The macOS route in Darling", ""]
        build = self.results.get("darling_build") or {}
        lines.append(f"Darling build `{build.get('id')}`: archive {build.get('size')} bytes, "
                     f"unpacked {build.get('unpacked_size')} bytes.")
        lines.append("")
        lines.append("| Application | Package | Mode | Installed | Window | Seconds | Interaction | Removed cleanly |")
        lines.append("|---|---|---|---|---|---|---|---|")
        for app in self.results["apps"]:
            lines.append("| {} | {} | {} | {} | {} | {} | {} | {} |".format(
                app["bundle"], app["package"], app["mode"],
                "yes" if app.get("install_status") == 0 else f"no ({app.get('install_error')})",
                "yes" if app.get("window") else "no",
                app.get("seconds_to_window") or "",
                {True: "yes", False: "no"}.get(app.get("interaction"), "n/a"),
                "yes" if app.get("uninstall_status") == 0 and not app.get("left_behind") else
                ("n/a" if app.get("install_status") != 0 else "no")))
        lines.append("")
        lines.append("| Refusal check | Expected | Got | As expected |")
        lines.append("|---|---|---|---|")
        for entry in self.results["refusals"]:
            if "expected" in entry:
                lines.append(f"| {entry['bundle']} | {entry['expected']} | {entry.get('refusal')} | "
                             f"{'yes' if entry.get('as_expected') else 'no'} |")
        return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--tur-runtime", required=True)
    parser.add_argument("--darling", required=True)
    parser.add_argument("--oracle", required=True)
    parser.add_argument("--results", required=True)
    parser.add_argument("--expected", help="expected.json: what must keep working")
    parser.add_argument("--strict", action="store_true",
                        help="exit 1 when an expectation is not met (darling-update)")
    args = parser.parse_args()
    os.makedirs(args.results, exist_ok=True)

    xvfb = subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", "1280x800x24", "-nolisten", "tcp"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(2)
    harness = Harness(args)
    try:
        if harness.import_runtime():
            harness.identify_all()
            apps = [name.strip() for name in os.environ.get("APPS", "").split(",") if name.strip()]
            modes = [name.strip() for name in os.environ.get("MODES", "nonroot").split(",") if name.strip()]
            for mode in modes:
                for name in apps:
                    bundle = os.path.join(args.oracle, "apps", name)
                    if not os.path.isdir(bundle):
                        harness.results["apps"].append({"bundle": name, "mode": mode, "package": "-",
                                                        "install_error": "not built by the oracle"})
                        continue
                    packages = harness.packages_of(bundle)
                    # Every package in the first mode; the bundle only in the others.
                    if mode != modes[0]:
                        packages = packages[:1]
                    for kind, package in packages:
                        harness.exercise(bundle, kind, package, mode)
    finally:
        xvfb.terminate()
        harness.results["finished"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        harness.results["unmet"] = unmet(harness.results, args.expected)
        with open(os.path.join(args.results, "results.json"), "w") as handle:
            json.dump(harness.results, handle, indent=1)
        with open(os.path.join(args.results, "summary.md"), "w") as handle:
            handle.write(harness.summary())
            for line in harness.results["unmet"]:
                handle.write(f"- **Not met:** {line}\n")
    if args.strict and harness.results["unmet"]:
        print("\n".join(harness.results["unmet"]), file=sys.stderr)
        return 1
    return 0


def unmet(results, expected_path):
    """What a build must do and this one did not: Darling found in the system,
    every promised refusal, every removal clean, and every application that
    expected.json says works (once measured) still showing its window."""
    problems = []
    if not any(step["name"] == "Darling is part of the system" and step["ok"] for step in results["steps"]):
        problems.append("tur-runtime did not find Darling where the image puts it")
    for entry in results["refusals"]:
        if "expected" in entry and not entry.get("as_expected"):
            problems.append(f"{entry['bundle']}: expected {entry['expected']}, got {entry.get('refusal')}")
    for app in results["apps"]:
        if app.get("install_status") == 0 and (app.get("uninstall_status") != 0 or app.get("left_behind")):
            problems.append(f"{app['app_id']}: not removed cleanly")
    expected = json.load(open(expected_path)) if expected_path and os.path.exists(expected_path) else {}
    for wanted in expected.get("windows", []):
        shown = any(app["bundle"] == wanted["bundle"] and app["package"] == wanted["package"]
                    and app["mode"] == wanted["mode"] and app.get("window") for app in results["apps"])
        if not shown:
            problems.append(f"{wanted['bundle']} from {wanted['package']} ({wanted['mode']}) no longer shows a window")
    return problems


if __name__ == "__main__":
    sys.exit(main())
