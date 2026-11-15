#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Prove TUR's Android runtime on a booted TUR system (ADR-0006;
docs/architecture/android-runtime.md; docs/reports/android-1.md).

    tests/boot/android.py fetch CACHE_DIR
    tests/boot/android.py run --disk disk.raw --work WORK --out OUT --cache CACHE_DIR

`fetch` downloads the two Android image archives tur-android pins
(services/tur-android/data/images.json) and the test's APK, from their
publishers, each checked against its SHA-256. Nothing from Google is
fetched: the VANILLA image, and an open-source application from F-Droid.

`run` starts the boot test's prepared disk (run-vm.py --prepare-only) from a
copy-on-write overlay with opt/org.tur/android in fw_cfg naming a mirror:
CACHE_DIR, served over HTTP on the host and reached from the guest through
QEMU's user network (10.0.2.2). tur-android fetches the images from it and
checks their digests as it would from the publisher. The guest's
android-agent switches Android on, installs and starts the application,
switches Android off and reports; the host takes screenshots at its markers
and judges the facts. A person looks at the screenshots: the check that a
window "looks rendered" is coarse.

Writes OUT/android.json, OUT/summary.md and OUT/screenshot-*.png; exit
status 0 only if every required check passed. Run in CI only (ADR-0002).
"""

from __future__ import annotations

import argparse
import functools
import hashlib
import http.server
import io
import json
import pathlib
import socketserver
import subprocess
import sys
import tarfile
import threading
import time
import urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import vm  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
PINS = ROOT / "services/tur-android/data/images.json"
# Editor 1.98 by Bill Farmer, GPL-3.0, from F-Droid: 269 kB, no native code,
# Android 5 and newer. Pinned as in the Android experiment's lock file
# (tests/experiments/android/locks/sample.lock.tsv on branch exp/android).
APK = {
    "name": "org.billthefarmer.editor_198.apk",
    "sha256": "343d17e11d0914e1a7708df2781580a0413e52501c0a41d91f7ef725d9615fe9",
    "urls": [
        "https://f-droid.org/repo/org.billthefarmer.editor_198.apk",
        "https://f-droid.org/archive/org.billthefarmer.editor_198.apk",
    ],
}
PORT = 8765
PATTERNS = [
    ("kernel panic", r"Kernel panic - not syncing"),
    ("emergency mode", r"[Ee]mergency [Mm]ode"),
]


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def download(urls: list[str], target: pathlib.Path, expected: str) -> None:
    if target.exists() and sha256(target) == expected:
        print(f"{target.name}: cached")
        return
    for attempt in range(1, 4):
        for url in urls:
            try:
                print(f"{target.name}: {url} (attempt {attempt})", flush=True)
                request = urllib.request.Request(url, headers={"User-Agent": "tur-ci"})
                with urllib.request.urlopen(request, timeout=120) as response, target.open("wb") as out:
                    while chunk := response.read(1 << 20):
                        out.write(chunk)
                if sha256(target) == expected:
                    print(f"{target.name}: {target.stat().st_size} bytes, SHA-256 matches")
                    return
                print(f"{target.name}: SHA-256 does not match the pin")
            except OSError as error:
                print(f"{target.name}: {error}")
            time.sleep(10)
    raise SystemExit(f"{target.name} could not be fetched")


def fetch(cache: pathlib.Path) -> int:
    cache.mkdir(parents=True, exist_ok=True)
    pins = json.loads(PINS.read_text())
    for image in pins["images"]:
        download([image["url"]], cache / image["file"], image["sha256"])
    download(APK["urls"], cache / APK["name"], APK["sha256"])
    return 0


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, format, *args):  # noqa: A002 - the base class's name
        sys.stderr.write("http: " + (format % args) + "\n")


def serve(cache: pathlib.Path) -> socketserver.TCPServer:
    handler = functools.partial(Quiet, directory=str(cache))
    server = socketserver.ThreadingTCPServer(("127.0.0.1", PORT), handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def facts_of(path: pathlib.Path) -> dict[str, str]:
    found: dict[str, str] = {}
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            key, sep, value = line.partition("=")
            if sep:
                found[key.strip()] = value.strip()
    return found


def document(path: pathlib.Path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def drive(args, out: pathlib.Path, work: pathlib.Path) -> dict:
    overlay = work / "android.qcow2"
    overlay.unlink(missing_ok=True)
    subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-b", str(pathlib.Path(args.disk).resolve()),
                    "-F", "raw", str(overlay)], check=True)
    host = out / "host"
    host.mkdir(parents=True, exist_ok=True)
    serial_log, report_tar = out / "serial.log", work / "android-report.tar"
    for stale in (serial_log, report_tar):
        stale.unlink(missing_ok=True)
    mirror = f"http://10.0.2.2:{PORT}"
    machine = vm.Machine(disk=str(overlay), disk_format="qcow2", disk_cache="writeback", work=work,
                         serial_log=serial_log, report_tar=report_tar, qemu_log=host / "qemu.log",
                         name="tur-android", cpus=args.cpus, memory=args.memory, qemu=args.qemu,
                         fw_cfg={"android": mirror}, variables_from=str(work / "OVMF_VARS.prepared.fd"))
    machine.variables.unlink(missing_ok=True)
    started = time.monotonic()

    def elapsed() -> float:
        return round(time.monotonic() - started, 1)

    shots: dict[str, dict] = {}

    def shot(name: str) -> None:
        try:
            info = vm.screenshot(machine.qmp, work, out / f"screenshot-{name}.png")
            info["at_seconds"] = elapsed()
            shots[name] = info
            (out / f"screenshot-{name}.ppm.tmp").unlink(missing_ok=True)
            print(f"[{elapsed():7.1f}s] screenshot {name}: {info}", flush=True)
        except Exception as error:  # noqa: BLE001 - a missed picture is recorded, not fatal
            shots[name] = {"error": str(error)}

    print(f"$ {' '.join(machine.command())}", flush=True)
    watcher = vm.SerialWatcher(serial_log, elapsed, patterns=PATTERNS)
    outcome = "timeout"
    planned: list[tuple[float, str]] = []
    seen: set[str] = set()
    server = serve(pathlib.Path(args.cache).resolve())
    try:
        machine.start()
        if not machine.qmp.command("query-kvm").get("enabled"):
            raise RuntimeError("KVM is not enabled")
        while time.monotonic() < started + args.timeout:
            if not machine.alive():
                outcome = "qemu-exited"
                break
            time.sleep(0.5)
            watcher.pump()
            machine.collect()
            for line in watcher.agent_lines:
                marker = line["text"].split(" ", 1)[0]
                if marker in seen:
                    continue
                seen.add(marker)
                if marker == "ANDROID-DESKTOP":
                    planned.append((elapsed() + 2, "desktop-before"))
                elif marker == "ANDROID-APP-LAUNCHED":
                    planned += [(elapsed() + 1, "app-1"), (elapsed() + 15, "app-2"), (elapsed() + 30, "app-3")]
                elif marker == "ANDROID-DISABLED":
                    planned.append((elapsed() + 3, "after-disable"))
            for at, name in list(planned):
                if elapsed() >= at:
                    planned.remove((at, name))
                    shot(name)
            if "ANDROID-COMPLETE" in seen:
                outcome = "complete"
                break
    except Exception as error:  # the evidence below says what happened
        outcome = f"error: {error}"
    finally:
        server.shutdown()
    if machine.alive():
        shot("at-end")
        if not (outcome == "complete" and machine.powerdown(timeout=120)):
            machine.quit()
    watcher.pump()
    print(f"[{elapsed():7.1f}s] {outcome}", flush=True)
    if outcome != "complete" and serial_log.exists():
        lines = vm.clean(serial_log.read_bytes().decode("utf-8", errors="replace")).split("\n")
        print("\n".join([line for line in lines if line.strip()][-150:]))
    guest = out / "guest"
    guest.mkdir(parents=True, exist_ok=True)
    if report_tar.exists() and report_tar.stat().st_size:
        try:
            with tarfile.open(fileobj=io.BytesIO(report_tar.read_bytes()), mode="r:", ignore_zeros=True) as archive:
                archive.extractall(guest, filter="data")
        except (tarfile.TarError, OSError, EOFError) as error:
            print(f"the report could not be read: {error}")
    overlay.unlink(missing_ok=True)
    return {"outcome": outcome, "elapsed_seconds": elapsed(), "screenshots": shots,
            "agent_lines": watcher.agent_lines, "hits": watcher.hits}


def judge(result: dict, out: pathlib.Path) -> list[dict]:
    guest = out / "guest/android"
    facts = facts_of(guest / "facts.txt")
    docs = {name: document(guest / f"{name}.json") for name in
            ("enable", "enable-no-consent", "install", "launch", "remove", "disable", "list", "status-running",
             "status-before", "status-after", "explain")}
    shots = result["screenshots"]
    checks: list[dict] = []

    def check(name: str, ok, detail: str, required: bool = True) -> None:
        checks.append({"check": name, "ok": bool(ok), "required": required, "detail": detail})

    def code_of(name: str) -> str:
        return (docs.get(name) or {}).get("code") or "-"

    check("the agent reported", result["outcome"] == "complete", result["outcome"])
    check("SELinux was enforcing from start to end",
          facts.get("selinux_at_start") == "Enforcing" and facts.get("selinux_at_end") == "Enforcing",
          f"{facts.get('selinux_at_start')} -> {facts.get('selinux_at_end')}")
    check("the kernel offers binderfs", facts.get("binder_filesystem") == "1", facts.get("kernel", "?"))
    check("before switch-on no Waydroid process runs and the service is inactive",
          facts.get("waydroid_processes_before") == "0" and facts.get("service_before") != "active",
          f"processes {facts.get('waydroid_processes_before')}, service {facts.get('service_before')}")
    check("D-Bus activation does not start the service before switch-on",
          facts.get("service_after_dbus_call") != "active",
          f"busctl exit {facts.get('dbus_activation_before_rc')}, service {facts.get('service_after_dbus_call')}")
    check("the session has its sound server socket", facts.get("sound_server_socket") == "present",
          facts.get("sound_server_socket", "?"))
    check("switching on without consent is refused",
          code_of("enable-no-consent") == "TUR-ANDROID-CONSENT-001", code_of("enable-no-consent"))
    enable = docs.get("enable") or {}
    check("switching on succeeded", enable.get("ok") is True,
          f"{code_of('enable')}: {str(enable.get('message', ''))[:300]}")
    install = docs.get("install") or {}
    check("the APK installed", install.get("ok") is True,
          f"{code_of('install')}: {str(install.get('message', ''))[:300]}")
    entry = install.get("launcher_entry") or {}
    check("its launcher entry exists, with an icon", bool(entry) and bool(entry.get("icon")),
          json.dumps(entry)[:300], required=False)
    launch = docs.get("launch") or {}
    check("the application was started", launch.get("ok") is True,
          f"{code_of('launch')}: {str(launch.get('message', ''))[:300]}")
    before = shots.get("desktop-before", {})
    app = shots.get("app-2", {}) or shots.get("app-1", {})
    check("a picture of the desktop with the application was taken", "width" in app,
          json.dumps(app)[:200])
    check("the picture changed after the application was started (a person must still look)",
          "mean_level" in app and "mean_level" in before and app.get("mean_level") != before.get("mean_level"),
          f"mean level {before.get('mean_level')} -> {app.get('mean_level')}", required=False)
    remove = docs.get("remove") or {}
    check("the application was removed", remove.get("ok") is True, code_of("remove"), required=False)
    disable = docs.get("disable") or {}
    check("switching off succeeded", disable.get("ok") is True,
          f"{code_of('disable')}: {str(disable.get('message', ''))[:300]}")
    left = {key: facts.get(key) for key in ("left_processes", "left_container_processes", "left_mounts",
                                            "left_interfaces", "left_binder_links")}
    check("after switch-off no process, mount, interface or binder link of Android is left",
          all(value == "0" for value in left.values()), json.dumps(left))
    check("after switch-off the service and binderfs are inactive and D-Bus cannot start them",
          facts.get("service_after_disable") != "active" and facts.get("binderfs_after_disable") != "active"
          and facts.get("service_after_dbus_call_after_disable") != "active",
          f"service {facts.get('service_after_disable')}, binderfs {facts.get('binderfs_after_disable')}, "
          f"after a D-Bus call {facts.get('service_after_dbus_call_after_disable')}")
    check("no SELinux denial", facts.get("avc_denials") == "0", f"{facts.get('avc_denials')} denials, "
          f"{facts.get('avc_denials_waydroid')} naming Waydroid, containers or binder", required=False)
    return checks


def summarise(result: dict, checks: list[dict], out: pathlib.Path) -> None:
    guest = out / "guest/android"
    facts = facts_of(guest / "facts.txt")
    enable = document(guest / "enable.json") or {}
    install = document(guest / "install.json") or {}
    running = document(guest / "status-running.json") or {}
    measured = {
        "enable_seconds": enable.get("total_seconds"),
        "download_and_unpack_seconds": enable.get("download_and_unpack_seconds"),
        "setup_seconds": enable.get("setup_seconds"),
        "downloaded_bytes": enable.get("downloaded_bytes"),
        "disk_bytes": enable.get("disk_bytes"),
        "android_start_seconds": install.get("android_start_seconds"),
        "install_seconds": install.get("install_seconds"),
        "memory_used_before_mib": facts.get("memory_used_before_mib"),
        "memory_used_app_running_mib": facts.get("memory_used_app_running_mib"),
        "memory_used_after_disable_mib": facts.get("memory_used_after_disable_mib"),
        "container_memory_bytes": running.get("container_memory_bytes"),
        "disable_seconds": facts.get("disable_wall_seconds"),
    }
    result.update({"checks": checks, "measured": measured, "facts": facts})
    (out / "android.json").write_text(json.dumps(result, indent=2) + "\n")
    lines = ["### Android runtime on the booted TUR system", "",
             f"Outcome: **{result['outcome']}** after {result['elapsed_seconds']} s.", "",
             "| Check | Result | Detail |", "|---|---|---|"]
    for item in checks:
        mark = "pass" if item["ok"] else ("FAIL" if item["required"] else "not met")
        detail = item["detail"].replace("|", "/").replace("\n", " ")
        lines.append(f"| {item['check']} | {mark} | {detail} |")
    lines += ["", "| Measured | Value |", "|---|---|"]
    lines += [f"| {key} | {value} |" for key, value in measured.items()]
    lines += ["", "Screenshots: " + ", ".join(sorted(result["screenshots"])) + ". Look at them."]
    (out / "summary.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="what", required=True)
    f = sub.add_parser("fetch")
    f.add_argument("cache")
    r = sub.add_parser("run")
    r.add_argument("--disk", required=True)
    r.add_argument("--work", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--cache", required=True)
    r.add_argument("--timeout", type=int, default=3600)
    r.add_argument("--cpus", type=int, default=4)
    r.add_argument("--memory", type=int, default=10240)
    r.add_argument("--qemu", default="qemu-system-x86_64")
    args = ap.parse_args()
    if args.what == "fetch":
        return fetch(pathlib.Path(args.cache))
    out, work = pathlib.Path(args.out).resolve(), pathlib.Path(args.work).resolve()
    out.mkdir(parents=True, exist_ok=True)
    result = drive(args, out, work)
    checks = judge(result, out)
    summarise(result, checks, out)
    return 0 if all(item["ok"] for item in checks if item["required"]) else 1


if __name__ == "__main__":
    sys.exit(main())
