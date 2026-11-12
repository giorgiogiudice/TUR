#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""End-to-end proof of tur-apps with the real programs, on a CI runner.

    e2e.py flatpak --tur-apps BIN --tur-catalogue BIN --catalogue DIR --work DIR --evidence DIR
    e2e.py windows --tur-apps BIN --tur-catalogue BIN --tur-runtime BIN --catalogue DIR --work DIR --evidence DIR

flatpak: switch Flathub on for a scratch user; plan, install per user, start
under a virtual display, find the window, check for updates, update, remove
keeping the data, install again, remove with the data; check that nothing is
left. Also: the router's own refusal of an application with no route.

windows: plan Notepad++ by its catalogue identifier; refuse a download that is
not the pinned file before anything runs; download, check, install through
tur-runtime, start it and find its window; keep a document through removal;
check that nothing is left; refuse a file the user gives that is not the
pinned one.

Every check is recorded in results.json and summary.md under the evidence
directory, with the screenshots. The exit status is 1 when a check failed.
Standard library only. Run it under dbus-run-session: flatpak run and
flatpak uninstall --delete-data use the session bus.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import signal
import subprocess
import sys
import time

DISPLAY = ":99"
WINDOW_LINE = re.compile(r'^\s*(0x[0-9a-f]+) "(.*)": \("(.*?)" "(.*?)"\)\s+(\d+)x(\d+)')
NO_ROUTE_ID = "com.riotgames.valorant"
HOST_FACTS = {"arch": "x86_64", "flatpak": True, "wine": True}


def log(text: str) -> None:
    print(text, flush=True)


class Checks:
    """Every check made, passed or not."""

    def __init__(self, evidence: pathlib.Path) -> None:
        self.evidence = evidence
        self.items: list[dict] = []
        self.measurements: dict[str, object] = {}

    def check(self, name: str, passed: bool, detail: object = "") -> bool:
        self.items.append({"check": name, "passed": bool(passed), "detail": detail})
        log(f"[{'PASS' if passed else 'FAIL'}] {name}" + (f": {detail}" if detail != "" else ""))
        return bool(passed)

    def measure(self, name: str, value: object) -> None:
        self.measurements[name] = value
        log(f"[MEASURE] {name}: {value}")

    def write(self, title: str) -> int:
        failed = [item for item in self.items if not item["passed"]]
        (self.evidence / "results.json").write_text(json.dumps(
            {"checks": self.items, "measurements": self.measurements, "failed": len(failed)}, indent=2))
        lines = [f"### {title}", "", f"{len(self.items) - len(failed)} of {len(self.items)} checks passed.", "",
                 "| Check | Result | Detail |", "|---|---|---|"]
        for item in self.items:
            detail = str(item["detail"]).replace("|", "\\|").replace("\n", " ")[:300]
            lines.append(f"| {item['check']} | {'pass' if item['passed'] else '**FAIL**'} | {detail} |")
        lines += ["", "| Measurement | Value |", "|---|---|"]
        for name, value in self.measurements.items():
            lines.append(f"| {name} | {value} |")
        (self.evidence / "summary.md").write_text("\n".join(lines) + "\n")
        return 1 if failed else 0


class World:
    """A scratch user: a home of its own, and the programs under test."""

    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.work = pathlib.Path(args.work).resolve()
        self.evidence = pathlib.Path(args.evidence).resolve()
        self.home = self.work / "home"
        self.data = self.home / ".local/share"
        for directory in (self.home, self.data, self.evidence):
            directory.mkdir(parents=True, exist_ok=True)
        self.host = self.work / "host.json"
        self.host.write_text(json.dumps(HOST_FACTS))
        self.counter = 0

    def env(self) -> dict[str, str]:
        env = dict(os.environ)
        env.pop("WAYLAND_DISPLAY", None)
        env.update({
            "HOME": str(self.home),
            "XDG_DATA_HOME": str(self.data),
            "XDG_CACHE_HOME": str(self.home / ".cache"),
            "XDG_STATE_HOME": str(self.home / ".local/state"),
            "XDG_CONFIG_HOME": str(self.home / ".config"),
            "DISPLAY": DISPLAY,
        })
        env.pop("FLATPAK_USER_DIR", None)
        return env

    def programs(self) -> list[str]:
        programs = ["--catalogue", self.args.catalogue]
        if getattr(self.args, "tur_runtime", None):
            programs += ["--tur-runtime-program", str(pathlib.Path(self.args.tur_runtime).resolve())]
        return programs

    def apps(self, arguments: list[str], label: str, timeout: int = 1800,
             catalogue: str | None = None) -> tuple[int, str, str]:
        """Runs tur-apps and keeps what it printed as evidence."""
        self.counter += 1
        programs = self.programs()
        if catalogue:
            programs[1] = catalogue
        command = [str(pathlib.Path(self.args.tur_apps).resolve()), *programs, *arguments]
        started = time.monotonic()
        done = subprocess.run(command, env=self.env(), capture_output=True, text=True, timeout=timeout,
                              check=False)
        seconds = time.monotonic() - started
        name = f"{self.counter:02d}-{label}"
        (self.evidence / f"{name}.stdout").write_text(done.stdout)
        (self.evidence / f"{name}.stderr").write_text(done.stderr)
        log(f"$ tur-apps {' '.join(arguments)}  -> {done.returncode} in {seconds:.1f} s")
        if done.returncode != 0:
            log("  " + (done.stderr.strip() or done.stdout.strip())[-1500:].replace("\n", "\n  "))
        self.last_seconds = seconds
        return done.returncode, done.stdout, done.stderr

    def run(self, command: list[str], timeout: int = 120) -> subprocess.CompletedProcess:
        return subprocess.run(command, env=self.env(), capture_output=True, text=True, timeout=timeout,
                              check=False)


def json_of(text: str) -> dict:
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return {}


def events_of(text: str) -> list[dict]:
    events = []
    for line in text.splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            events.append({"event": "not-json", "line": line})
    return events


# ------------------------------------------------------------------ display


def start_display(world: World) -> list[subprocess.Popen]:
    server = subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", "1280x800x24", "-nolisten", "tcp", "-ac"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(100):
        if pathlib.Path(f"/tmp/.X11-unix/X{DISPLAY.lstrip(':')}").exists():
            break
        time.sleep(0.1)
    manager = subprocess.Popen(["openbox"], env=world.env(), stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL)
    for _ in range(100):
        if world.run(["wmctrl", "-m"], timeout=10).returncode == 0:
            break
        time.sleep(0.1)
    return [manager, server]


def windows(world: World) -> list[dict]:
    tree = world.run(["xwininfo", "-display", DISPLAY, "-root", "-tree"], timeout=20).stdout
    found = []
    for line in tree.splitlines():
        match = WINDOW_LINE.match(line)
        if match:
            identifier, title, instance, klass, width, height = match.groups()
            found.append({"id": identifier, "title": title, "class": [instance, klass],
                          "width": int(width), "height": int(height)})
    return found


def viewable(world: World, identifier: str) -> bool:
    info = world.run(["xwininfo", "-display", DISPLAY, "-id", identifier], timeout=20).stdout
    return "Map State: IsViewable" in info


def screenshot(world: World, name: str) -> bool:
    path = world.evidence / f"{name}.png"
    done = world.run(["import", "-display", DISPLAY, "-window", "root", str(path)], timeout=60)
    if done.returncode != 0 or not path.is_file():
        return False
    world.run(["convert", str(path), "-resize", "800x", "-colors", "128", "-strip",
               str(world.evidence / f"{name}-small.png")], timeout=60)
    return True


def launch_and_find(world: World, checks: Checks, app_id: str, title: str, shot: str,
                    wait: float = 90.0) -> None:
    """Starts the application through tur-apps, finds its window, takes a
    screenshot and closes it."""
    command = [str(pathlib.Path(world.args.tur_apps).resolve()), *world.programs(), "launch", app_id]
    output = (world.evidence / f"launch-{shot}.log").open("w")
    started = time.monotonic()
    process = subprocess.Popen(command, env=world.env(), stdout=output, stderr=subprocess.STDOUT,
                               start_new_session=True)
    pattern = re.compile(title)
    window = None
    while time.monotonic() - started < wait and window is None:
        for candidate in windows(world):
            if pattern.search(candidate["title"]) and candidate["width"] > 50 and viewable(world, candidate["id"]):
                window = candidate
                break
        if window is None:
            if process.poll() is not None:
                break
            time.sleep(0.25)
    seconds = time.monotonic() - started
    checks.check(f"{app_id}: its window appears", window is not None,
                 window or f"no window titled /{title}/ after {seconds:.1f} s; exit {process.poll()}; "
                           f"windows: {[w['title'] for w in windows(world)]}")
    if window is not None:
        checks.measure(f"{app_id}: seconds to its window", round(seconds, 1))
        time.sleep(2.0)
        checks.check(f"{app_id}: screenshot {shot}.png", screenshot(world, shot))
        world.run(["wmctrl", "-i", "-c", window["id"]], timeout=20)
    try:
        process.wait(timeout=30)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
    output.close()
    checks.check(f"{app_id}: no window is left", not any(pattern.search(w["title"]) for w in windows(world)))


# ------------------------------------------------------------------ shared checks


def no_route_is_refused(world: World, checks: Checks) -> None:
    code, out, _ = world.apps(["--host", str(world.host), "plan", NO_ROUTE_ID, "--json"], "no-route")
    error = json_of(out).get("error", {})
    router = subprocess.run(
        [world.args.tur_catalogue, "--catalogue", world.args.catalogue, "route", NO_ROUTE_ID, "--json",
         "--host", str(world.host)], capture_output=True, text=True, check=False)
    (world.evidence / "no-route-router.json").write_text(router.stdout)
    options = json_of(router.stdout).get("options", [])
    reason = next((option["reason"] for option in options if option.get("recommended")), None)
    checks.check("no route: refused with exit status 3 and TUR-DESKTOP-APPS-003",
                 code == 3 and error.get("code") == "TUR-DESKTOP-APPS-003", error.get("code"))
    checks.check("no route: the message is the router's own reason, as tur-catalogue prints it",
                 bool(reason) and str(error.get("message", "")).startswith(reason), reason)
    checks.check("no route: the reason code is the router's", error.get("detail", {}).get("reason_code") == "no-route")


def nothing_under(path: pathlib.Path) -> list[str]:
    if not path.exists():
        return []
    return sorted(str(item.relative_to(path)) for item in path.rglob("*"))


# ------------------------------------------------------------------ flatpak


FLATPAK_ID = "net.sourceforge.klavaro"
FLATPAK_APP = "net.sourceforge.Klavaro"


def flatpak_user_refs(world: World) -> list[str]:
    done = world.run(["flatpak", "list", "--user", "--columns=ref"], timeout=60)
    return [line.strip() for line in done.stdout.splitlines() if line.strip()]


def run_flatpak(world: World, checks: Checks) -> None:
    version = world.run(["flatpak", "--version"]).stdout.strip()
    checks.measure("flatpak", version)
    checks.check("a fresh user has nothing installed", flatpak_user_refs(world) == [])

    code, out, _ = world.apps(["plan", FLATPAK_ID, "--json"], "plan-source-off")
    checks.check("with Flathub off, the plan is refused (TUR-DESKTOP-APPS-008)",
                 code == 3 and json_of(out).get("error", {}).get("code") == "TUR-DESKTOP-APPS-008",
                 json_of(out).get("error", {}).get("message"))

    code, out, _ = world.apps(["sources", "enable", "flathub", "--json"], "enable-flathub")
    checks.check("Flathub is switched on for this user", code == 0 and json_of(out).get("state") == "on", out.strip())
    remotes = world.run(["flatpak", "remotes", "--user", "--columns=name,url,options"]).stdout
    (world.evidence / "remotes.txt").write_text(remotes)
    checks.check("flatpak lists the user's flathub remote", remotes.startswith("flathub\t"), remotes.strip())

    code, out, _ = world.apps(["plan", FLATPAK_ID, "--json"], "plan")
    plan = json_of(out)
    (world.evidence / "flatpak-plan.json").write_text(json.dumps(plan, indent=2))
    checks.check("the plan is made", code == 0 and plan.get("route") == "flatpak", plan.get("route"))
    downloads = plan.get("downloads", [])
    checks.check("the plan gives the application's download size in bytes",
                 bool(downloads) and isinstance(downloads[0].get("size"), int), downloads[:1])
    checks.check("the plan names the commit that would be installed",
                 bool(downloads) and len(str(downloads[0].get("commit") or "")) == 64)
    checks.check("the plan reads the permissions from the application's metadata",
                 plan.get("access_known") is True and any(a.get("kind") == "display" for a in plan.get("access", [])),
                 [a.get("permission") for a in plan.get("access", [])])
    code, text, _ = world.apps(["plan", FLATPAK_ID], "plan-text")
    (world.evidence / "flatpak-plan.txt").write_text(text)

    code, out, _ = world.apps(["install", FLATPAK_ID, "--yes", "--events"], "install")
    events = events_of(out)
    (world.evidence / "flatpak-install-events.jsonl").write_text(out)
    checks.measure("Flatpak install, seconds (Klavaro and its runtime)", round(world.last_seconds, 1))
    checks.check("installed per user from Flathub", code == 0 and events and events[-1].get("event") == "result",
                 events[-1] if events else None)
    checks.check("the event stream starts with the plan and every line is an event",
                 bool(events) and events[0].get("event") == "plan" and all(e.get("event") != "not-json" for e in events))
    checks.check("flatpak's activity reaches the events",
                 any(e.get("event") == "activity" and "installing app/" in str(e.get("text")) for e in events))
    refs = flatpak_user_refs(world)
    (world.evidence / "refs-installed.txt").write_text("\n".join(refs) + "\n")
    checks.check("flatpak lists the application per user", f"app/{FLATPAK_APP}/x86_64/stable" in refs, refs)
    checks.check("nothing was installed system-wide by the test",
                 FLATPAK_APP not in world.run(["flatpak", "list", "--system", "--columns=application"]).stdout)

    code, out, _ = world.apps(["list", "--json"], "list")
    listing = json_of(out).get("applications", [])
    (world.evidence / "flatpak-list.json").write_text(json.dumps(listing, indent=2))
    mine = next((a for a in listing if a.get("id") == FLATPAK_ID), {})
    checks.check("the list shows it, with its version, size and source",
                 mine.get("route") == "flatpak" and mine.get("version") and isinstance(mine.get("size"), int),
                 mine)

    launch_and_find(world, checks, FLATPAK_ID, r"(?i)klavaro", "flatpak-klavaro")

    code, out, _ = world.apps(["list", "--check-updates", "--json"], "check-updates")
    mine = next((a for a in json_of(out).get("applications", []) if a.get("id") == FLATPAK_ID), {})
    checks.check("the update check asks Flathub and gives an answer",
                 mine.get("update", {}).get("checked") is True and mine.get("update", {}).get("available") is not None,
                 mine.get("update"))
    code, out, _ = world.apps(["update", FLATPAK_ID, "--events"], "update")
    checks.check("update runs (nothing to update is a success)",
                 code == 0 and events_of(out)[-1].get("event") == "result", events_of(out)[-1:] if out else out)

    data = world.home / ".var/app" / FLATPAK_APP
    data.mkdir(parents=True, exist_ok=True)
    (data / "tur-e2e-keep.txt").write_text("the user's own file\n")
    checks.measure("files in the application's data after it ran", len(nothing_under(data)))
    code, out, _ = world.apps(["remove", FLATPAK_ID, "--events"], "remove-keep")
    result = events_of(out)[-1] if out else {}
    checks.check("removed, keeping its data by default",
                 code == 0 and result.get("result", {}).get("data") == "keep", result)
    checks.check("its data is still there", (data / "tur-e2e-keep.txt").is_file())
    refs = flatpak_user_refs(world)
    checks.check("no reference is left installed for this user (application, runtime, extensions)", refs == [], refs)
    for path in (world.data / "flatpak/app" / FLATPAK_APP,
                 world.data / "flatpak/exports/share/applications" / f"{FLATPAK_APP}.desktop",
                 world.data / "tur/apps/installed" / f"{FLATPAK_ID}.json"):
        checks.check(f"gone: {path.relative_to(world.home)}", not path.exists())

    code, out, _ = world.apps(["install", FLATPAK_ID, "--yes", "--events"], "install-again")
    checks.measure("Flatpak install again, seconds", round(world.last_seconds, 1))
    checks.check("installed again", code == 0)
    code, out, _ = world.apps(["remove", FLATPAK_ID, "--delete-data", "--events"], "remove-delete")
    result = events_of(out)[-1] if out else {}
    checks.check("removed with its data", code == 0 and result.get("result", {}).get("data") == "delete", result)
    checks.check("its data is gone", not data.exists())
    checks.check("no reference is left installed", flatpak_user_refs(world) == [])
    deployed = nothing_under(world.data / "flatpak/app") + nothing_under(world.data / "flatpak/runtime")
    checks.check("no deployment is left under ~/.local/share/flatpak/app or runtime", deployed == [], deployed[:20])
    exports = [p for p in nothing_under(world.data / "flatpak/exports") if not p.endswith(("share", "applications", "icons", "hicolor"))]
    checks.check("no exported launcher entry or icon is left", exports == [], exports[:20])
    repo = world.data / "flatpak/repo"
    repo_bytes = sum(p.stat().st_size for p in repo.rglob("*") if p.is_file()) if repo.exists() else 0
    checks.measure("bytes left in the user's flatpak repository (objects, remote config)", repo_bytes)
    code, out, _ = world.apps(["list", "--json"], "list-empty")
    checks.check("the list is empty", code == 0 and json_of(out).get("applications") == [], out.strip()[:300])

    no_route_is_refused(world, checks)


# ------------------------------------------------------------------ windows


WINDOWS_ID = "org.notepad-plus-plus.notepad-plus-plus"


def downloads_left() -> list[str]:
    """What tur-apps left in its download root, /var/tmp."""
    return sorted(str(path) for path in pathlib.Path("/var/tmp").glob("tur-apps-*"))


def runtime_apps(world: World) -> list[str]:
    done = world.run([str(pathlib.Path(world.args.tur_runtime).resolve()), "app", "list", "--json"])
    return [a.get("app_id") for a in json_of(done.stdout).get("applications", [])]


def run_windows(world: World, checks: Checks) -> None:
    checks.measure("wine", world.run(["wine", "--version"]).stdout.strip())
    runtime = world.data / "tur/runtime"

    code, out, _ = world.apps(["plan", WINDOWS_ID, "--json"], "plan")
    plan = json_of(out)
    (world.evidence / "windows-plan.json").write_text(json.dumps(plan, indent=2))
    checks.check("the plan is made by catalogue identifier: Wine, the router's recommendation",
                 code == 0 and plan.get("route") == "wine" and plan.get("chosen_by") == "router", plan.get("route"))
    download = (plan.get("downloads") or [{}])[0]
    checks.check("the plan downloads the pinned installer with its size and SHA-256",
                 download.get("size") == 7048536
                 and download.get("sha256") == "ce3267f5c4e7e42f847e00f4b20234da6150647203033cdde34a69593efc3152",
                 download)
    code, text, _ = world.apps(["plan", WINDOWS_ID], "plan-text")
    (world.evidence / "windows-plan.txt").write_text(text)

    # A download that is not the pinned file: the same entry with one digit
    # of its digest changed.
    altered = world.work / "altered-catalogue"
    shutil.copytree(world.args.catalogue, altered)
    entry = altered / f"{WINDOWS_ID}.toml"
    entry.write_text(entry.read_text().replace(
        "ce3267f5c4e7e42f847e00f4b20234da6150647203033cdde34a69593efc3152",
        "de3267f5c4e7e42f847e00f4b20234da6150647203033cdde34a69593efc3152"))
    code, out, _ = world.apps(["install", WINDOWS_ID, "--yes", "--events"], "install-corrupted",
                              catalogue=str(altered))
    events = events_of(out)
    (world.evidence / "windows-corrupted-events.jsonl").write_text(out)
    error = events[-1].get("error", {}) if events else {}
    checks.check("a download that is not the pinned file is refused (exit 5, TUR-DESKTOP-APPS-006)",
                 code == 5 and error.get("code") == "TUR-DESKTOP-APPS-006", error.get("message"))
    checks.check("the file was really downloaded before it was refused",
                 any(e.get("event") == "step-finished" and e.get("step") == "download" for e in events)
                 or "downloaded from" in str(error.get("message")))
    checks.check("nothing ran: no install step started",
                 not any(e.get("event") == "step-started" and e.get("step") == "install" for e in events))
    checks.check("nothing ran: tur-runtime has no application and no prefix",
                 runtime_apps(world) == [] and nothing_under(runtime / "prefixes") == [])
    checks.check("nothing is kept of the refused download",
                 downloads_left() == [])

    code, out, _ = world.apps(["install", WINDOWS_ID, "--yes", "--events"], "install")
    events = events_of(out)
    (world.evidence / "windows-install-events.jsonl").write_text(out)
    checks.measure("Windows install, seconds (download, check, tur-runtime)", round(world.last_seconds, 1))
    result = events[-1].get("result", {}) if events else {}
    checks.check("installed through the whole chain", code == 0 and events[-1].get("event") == "result",
                 events[-1] if events else None)
    order = [f"{e.get('event')}:{e.get('step', '')}" for e in events]
    def at(kind: str) -> int:
        return order.index(kind) if kind in order else -1
    checks.check("downloaded, then checked, then installed",
                 0 <= at("step-finished:download") < at("step-finished:verify") < at("step-started:install"),
                 [k for k in order if k.startswith("step")])
    progress = [e for e in events if e.get("event") == "progress"]
    checks.check("download progress has a total", bool(progress) and progress[-1].get("total") == 7048536,
                 progress[-1:] if progress else None)
    checks.check("the record keeps the pinned digest",
                 result.get("source", {}).get("pinned") is True
                 and result.get("source", {}).get("sha256", "").startswith("ce3267f5"))
    checks.check("tur-runtime holds it under the catalogue's identifier", WINDOWS_ID in runtime_apps(world),
                 runtime_apps(world))
    checks.check("the downloaded installer was deleted", downloads_left() == [])

    code, out, _ = world.apps(["list", "--json", "--check-updates"], "list")
    mine = next((a for a in json_of(out).get("applications", []) if a.get("id") == WINDOWS_ID), {})
    (world.evidence / "windows-list.json").write_text(json.dumps(json_of(out), indent=2))
    checks.check("the list shows it with its version, size and source, and no update",
                 mine.get("route") == "wine" and isinstance(mine.get("size"), int)
                 and mine.get("update", {}).get("available") is False, mine)
    checks.measure("Notepad++ version as recorded", mine.get("version"))
    checks.measure("Notepad++ size on disk, bytes", mine.get("size"))

    launch_and_find(world, checks, WINDOWS_ID, r"Notepad\+\+", "windows-notepadpp")

    home = runtime / "prefixes" / WINDOWS_ID / "home"
    document = home / "Documents/tur-e2e-letter.txt"
    document.parent.mkdir(parents=True, exist_ok=True)
    document.write_text("A document the user saved.\n")
    code, out, _ = world.apps(["remove", WINDOWS_ID, "--events"], "remove-keep")
    events = events_of(out)
    (world.evidence / "windows-remove-events.jsonl").write_text(out)
    result = events[-1].get("result", {}) if events else {}
    checks.check("removed, keeping its documents by default",
                 code == 0 and result.get("data") == "keep" and result.get("left_behind") == [], result)
    kept = list((world.data / "tur/apps/kept" / WINDOWS_ID).glob("*/home/Documents/tur-e2e-letter.txt"))
    checks.check("the user's document was kept", len(kept) == 1 and kept[0].read_text() == "A document the user saved.\n",
                 [str(p) for p in kept])
    profiles = list((world.data / "tur/apps/kept" / WINDOWS_ID).glob("*/windows-profiles"))
    checks.check("its Windows settings were kept", len(profiles) == 1, [str(p) for p in profiles])
    checks.check("tur-runtime holds nothing", runtime_apps(world) == [])
    for path in (runtime / "prefixes" / WINDOWS_ID, runtime / "launchers" / WINDOWS_ID,
                 world.data / "tur/apps/installed" / f"{WINDOWS_ID}.json"):
        checks.check(f"gone: {path.relative_to(world.home)}", not path.exists())
    traces = [str(p) for p in (world.home / ".wine", world.home / ".local/share/applications",
                               world.home / "Desktop") if p.exists()]
    checks.check("nothing outside the sandbox in the scratch home", traces == [], traces)

    wrong = world.work / "npp.8.9.9.1.Installer.x64.exe"
    wrong.write_bytes(b"MZ" + os.urandom(4096))
    code, out, _ = world.apps(["install", WINDOWS_ID, "--installer", str(wrong), "--yes", "--events"],
                              "install-wrong-file")
    events = events_of(out)
    checks.check("a file you give that is not the pinned one is refused before anything runs",
                 code == 5 and events and events[-1].get("error", {}).get("code") == "TUR-DESKTOP-APPS-006"
                 and runtime_apps(world) == [], events[-1:] if events else None)

    no_route_is_refused(world, checks)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("route", choices=["flatpak", "windows"])
    parser.add_argument("--tur-apps", required=True)
    parser.add_argument("--tur-catalogue", required=True)
    parser.add_argument("--tur-runtime")
    parser.add_argument("--catalogue", required=True)
    parser.add_argument("--work", required=True)
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()
    args.catalogue = str(pathlib.Path(args.catalogue).resolve())
    args.tur_catalogue = str(pathlib.Path(args.tur_catalogue).resolve())
    world = World(args)
    checks = Checks(world.evidence)
    display = start_display(world)
    try:
        if args.route == "flatpak":
            run_flatpak(world, checks)
        else:
            run_windows(world, checks)
    except Exception as error:  # noqa: BLE001 - every failure is a recorded check
        checks.check("the test ran to the end", False, repr(error))
    finally:
        for process in display:
            process.terminate()
    return checks.write(f"tur-apps end to end: {args.route}")


if __name__ == "__main__":
    sys.exit(main())
