#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Capture sessions of the driver-synthesis test bench.

Runs as root inside the throw-away Fedora 45 container of the capture job,
after the emulated device has been plugged in on the runner (its "world"
socket and ready file are mounted at --run-dir). It

  1. puts the patched Wine modules over the container's Wine and installs
     the KMDF runtime and the device's Windows driver package into a Wine
     prefix, as the windows-drivers experiment does (kmdf_harness.py);
  2. runs the scripted sessions below: for each, usbmon records every USB
     transfer of the device (pcapng, plus a readable text form) while the
     Windows program gadget.exe is run once per user action, as the
     unprivileged user, and the "world" (room temperature, button presses)
     is changed between and during actions;
  3. writes, beside each capture, the timeline of what the user did and
     what the program showed, in user terms only;
  4. checks every action's output and the device's resulting state, which
     is the proof that the Windows driver works.

Output: --out (the captures and timelines for the hand-off bundle) and
--evidence (logs, results.json, results.md). Exit status non-zero when the
harness failed or any check failed: captures are only handed off when the
Windows side did everything their timelines say.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
import traceback
from dataclasses import dataclass, field

HERE = pathlib.Path(__file__).resolve().parent
TESTS = HERE.parents[2]
sys.path.insert(0, str(HERE))
import usbmon  # noqa: E402
from world import World  # noqa: E402

_spec = importlib.util.spec_from_file_location("kmdf_harness", TESTS / "windows-drivers" / "kmdf" / "kmdf_harness.py")
kh = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(kh)
twd = kh.twd

SPDX_MD = "<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->"
# The driver host's log: plug-and-play, loading, the drivers' own prints.
DEBUG_HOST = "+wdfldr,+plugplay,+loaddll,+seh,warn+debugstr,err+all,fixme+all"


# ---------------------------------------------------------------- pictures

def grid(rows: list[str]) -> list[str]:
    assert len(rows) == 8 and all(len(r) == 16 for r in rows), rows
    return rows


SMILEY = grid([
    "....ffffffff....",
    "..ff........ff..",
    ".f..ff....ff..f.",
    "f...ff....ff...f",
    "f..............f",
    "f..8........8..f",
    ".f..88888888..f.",
    "..ff........ff..",
])
DIAGONAL = grid(["".join("f" if c // 2 == r else "." for c in range(16)) for r in range(8)])
GRADIENT = grid(["".join("0123456789abcdef"[c] if c else "." for c in range(16)) for _ in range(8)])
CORNER = grid(["f" + "." * 15] + ["." * 16] * 7)
STEPS = grid(["".join("0123456789abcdef"[2 * r + (c % 2)] if 2 * r + (c % 2) else "." for c in range(16))
              for r in range(8)])
LETTERS = grid([
    "fff.f..f.fff....",
    ".f..f..f.f..f...",
    ".f..f..f.fff....",
    ".f..f..f.f.f....",
    ".f...ff..f..f...",
    "................",
    "5555555555555555",
    "................",
])
LONG_NOTE = ("The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs! "
             "0123456789")


def as_matrix(rows: list[str]) -> list[str]:
    """The picture as the device reports it (0 for off)."""
    return [r.replace(".", "0") for r in rows]


def sensors(t: float, h: float, lux: int) -> list[str]:
    return [f"temperature {t:.2f}", f"humidity {h:.1f}", f"light {lux}"]


# ---------------------------------------------------------------- sessions

@dataclass
class Step:
    say: str                      # what the user did, in user terms
    args: list[str]               # gadget.exe's arguments
    expect: list[str] | None = None   # the program's output; None: checked by `check`
    world: dict | None = None     # the room before the action (never shown)
    state: dict | None = None     # the device's state afterwards (user-level)
    events: list = field(default_factory=list)  # (seconds after listening, what, say)
    image: list[str] | None = None
    exit: int = 0
    check: object = None          # callable(lines, state) -> (ok, detail)


def info_check(firmware: str):
    def check(lines, state):
        ok = (len(lines) == 2 and lines[0] == f"firmware {firmware}" and lines[1].startswith("uptime ")
              and abs(int(lines[1].split()[1]) - state["uptime"]) <= 3)
        return ok, f"{lines} vs uptime {state['uptime']}"
    return check


SESSIONS = [
    ("First use", [
        Step("asked for the device information", ["info"], check=info_check("3.1.4")),
        Step("read all sensors", ["read-sensors"], sensors(21.37, 45.2, 312),
             world={"temperature": 21.37, "humidity": 45.2, "light": 312}),
        Step("asked which mode the gadget is in", ["get-mode"], ["day"]),
        Step("asked for the display brightness", ["get-brightness"], ["60"]),
        Step("asked for the night-light level", ["get-night-level"], ["10"]),
        Step("asked for the temperature alert", ["get-alert"], ["off"]),
        Step("listed the stored notes (there were none)", ["list-notes"], []),
    ]),
    ("Brightness", [
        Step("set the display brightness to 40", ["set-brightness", "40"], ["brightness 40"],
             state={"day_brightness": 40, "mode": "day"}),
        Step("asked for the display brightness", ["get-brightness"], ["40"]),
        Step("set the display brightness to 41", ["set-brightness", "41"], ["brightness 41"],
             state={"day_brightness": 41}),
        Step("set the display brightness to 75", ["set-brightness", "75"], ["brightness 75"],
             state={"day_brightness": 75}),
        Step("set the display brightness to 0", ["set-brightness", "0"], ["brightness 0"],
             state={"day_brightness": 0}),
        Step("set the display brightness to 100", ["set-brightness", "100"], ["brightness 100"],
             state={"day_brightness": 100}),
        Step("set the display brightness to 7", ["set-brightness", "7"], ["brightness 7"],
             state={"day_brightness": 7}),
        Step("asked for the display brightness", ["get-brightness"], ["7"]),
    ]),
    ("Night mode", [
        Step("switched to night mode", ["set-mode", "night"], ["mode night"], state={"mode": "night"}),
        Step("asked which mode the gadget is in", ["get-mode"], ["night"]),
        Step("set the night-light level to 10", ["set-night-level", "10"], ["night-level 10"],
             state={"night_level": 10, "mode": "night"}),
        Step("asked for the night-light level", ["get-night-level"], ["10"]),
        Step("set the night-light level to 3", ["set-night-level", "3"], ["night-level 3"],
             state={"night_level": 3, "mode": "night"}),
        Step("set the (daytime) display brightness to 55 while in night mode", ["set-brightness", "55"],
             ["brightness 55"], state={"day_brightness": 55, "night_level": 3, "mode": "night"}),
        Step("asked for the display brightness", ["get-brightness"], ["55"], state={"mode": "night"}),
        Step("asked for the night-light level", ["get-night-level"], ["3"]),
        Step("switched back to day mode", ["set-mode", "day"], ["mode day"], state={"mode": "day"}),
        Step("asked which mode the gadget is in", ["get-mode"], ["day"]),
    ]),
    ("Sensors through the day", [
        Step("read all sensors (early morning, lights off)", ["read-sensors"], sensors(18.05, 38.1, 0),
             world={"temperature": 18.05, "humidity": 38.1, "light": 0}),
        Step("read the temperature (office, afternoon)", ["read-temperature"], ["23.90"],
             world={"temperature": 23.90, "humidity": 52.7, "light": 815}),
        Step("read the humidity", ["read-humidity"], ["52.7"]),
        Step("read the light level", ["read-light"], ["815"]),
        Step("read all sensors (gadget out on the balcony in winter)", ["read-sensors"], sensors(-1.25, 81.0, 15),
             world={"temperature": -1.25, "humidity": 81.0, "light": 15}),
        Step("read all sensors (gadget in direct sunlight)", ["read-sensors"], sensors(30.42, 29.9, 70210),
             world={"temperature": 30.42, "humidity": 29.9, "light": 70210}),
        Step("read all sensors (by the window, cloudy)", ["read-sensors"], sensors(12.0, 60.0, 65535),
             world={"temperature": 12.00, "humidity": 60.0, "light": 65535}),
        Step("read all sensors (bathroom after a shower, under a lamp)", ["read-sensors"],
             sensors(0.07, 99.9, 131072), world={"temperature": 0.07, "humidity": 99.9, "light": 131072}),
        Step("read the temperature (gadget in the freezer)", ["read-temperature"], ["-12.34"],
             world={"temperature": -12.34, "humidity": 5.5, "light": 1}),
        Step("read the light level", ["read-light"], ["1"]),
    ]),
    ("Notes", [
        Step("listed the stored notes (none)", ["list-notes"], []),
        Step("stored the note 'hello' in slot 0", ["write-note", "0", "hello"], ["note 0 stored"],
             state={"notes": {"0": "hello"}}),
        Step("stored the note 'Buy milk and eggs' in slot 3", ["write-note", "3", "Buy milk and eggs"],
             ["note 3 stored"], state={"notes": {"0": "hello", "3": "Buy milk and eggs"}}),
        Step("listed the stored notes", ["list-notes"], ["0 5", "3 17"]),
        Step("read the note in slot 0", ["read-note", "0"], ["hello"]),
        Step("read the note in slot 3", ["read-note", "3"], ["Buy milk and eggs"]),
        Step(f"stored a long note in slot 7: '{LONG_NOTE}'", ["write-note", "7", LONG_NOTE], ["note 7 stored"]),
        Step("read the note in slot 7", ["read-note", "7"], [LONG_NOTE]),
        Step("tried to read slot 5, which is empty", ["read-note", "5"], ["empty"], exit=4),
        Step("deleted the note in slot 3", ["delete-note", "3"], ["note 3 deleted"],
             state={"notes": {"0": "hello", "7": LONG_NOTE}}),
        Step("listed the stored notes", ["list-notes"], ["0 5", f"7 {len(LONG_NOTE)}"]),
    ]),
    ("Pictures", [
        Step("showed a smiley on the display", ["show-image"], ["image shown"], image=SMILEY),
        Step("showed a diagonal line", ["show-image"], ["image shown"], image=DIAGONAL),
        Step("showed a left-to-right brightness ramp", ["show-image"], ["image shown"], image=GRADIENT),
        Step("lit only the top-left pixel, fully", ["show-image"], ["image shown"], image=CORNER),
        Step("showed a pattern of brightness steps", ["show-image"], ["image shown"], image=STEPS),
        Step("cleared the display", ["clear-display"], ["display cleared"],
             state={"picture": ["0" * 16] * 8}),
    ]),
    ("Alert and buttons", [
        Step("set the temperature alert to 26.5 degrees", ["set-alert", "26.5"], ["alert 26.5"],
             world={"temperature": 24.00}, state={"alert": 26.5, "mode": "day"}),
        Step("asked for the temperature alert", ["get-alert"], ["26.5"]),
        Step("watched the gadget for 10 seconds", ["wait-event", "10"], ["button 1 0.30", "alert 27.12",
                                                                          "button 2 1.62"],
             events=[(1.5, {"press": (1, 0.30)}, "briefly pressed button 1 (about 0.3 s)"),
                     (3.0, {"temperature": 25.80}, "the room warmed up a little"),
                     (4.5, {"temperature": 27.12}, "the room warmed up past the alert temperature"),
                     (6.0, {"press": (2, 1.62)}, "held button 2 down (about 1.6 s)")]),
        Step("switched the temperature alert off", ["set-alert", "off"], ["alert off"], state={"alert": None}),
        Step("asked for the temperature alert", ["get-alert"], ["off"]),
    ]),
    ("Clearing the notes", [
        Step("stored the note 'temp' in slot 1", ["write-note", "1", "temp"], ["note 1 stored"]),
        Step("stored the note 'call Ana at 5' in slot 2", ["write-note", "2", "call Ana at 5"], ["note 2 stored"]),
        Step("listed the stored notes", ["list-notes"], ["0 5", "1 4", "2 13", f"7 {len(LONG_NOTE)}"]),
        Step("cleared all notes", ["clear-notes"], ["notes cleared"], state={"notes": {}}),
        Step("listed the stored notes (none left)", ["list-notes"], []),
    ]),
    ("Evening", [
        Step("asked for the device information", ["info"], check=info_check("3.1.4")),
        Step("switched to night mode", ["set-mode", "night"], ["mode night"], state={"mode": "night"}),
        Step("set the (daytime) display brightness to 20", ["set-brightness", "20"], ["brightness 20"],
             state={"day_brightness": 20, "mode": "night"}),
        Step("set the night-light level to 1", ["set-night-level", "1"], ["night-level 1"],
             state={"night_level": 1, "mode": "night"}),
        Step("read all sensors", ["read-sensors"], sensors(19.5, 48.0, 2),
             world={"temperature": 19.50, "humidity": 48.0, "light": 2}),
        Step("watched the gadget for 6 seconds", ["wait-event", "6"], ["button 1 0.84", "button 1 0.12"],
             events=[(1.5, {"press": (1, 0.84)}, "pressed button 1 (about 0.8 s)"),
                     (3.0, {"press": (1, 0.12)}, "tapped button 1 very briefly")]),
        Step("asked which mode the gadget is in", ["get-mode"], ["night"]),
        Step("switched back to day mode", ["set-mode", "day"], ["mode day"], state={"mode": "day"}),
    ]),
    ("Frost alert and a last picture", [
        Step("set the temperature alert to -5.0 degrees", ["set-alert", "-5.0"], ["alert -5.0"],
             world={"temperature": -7.00}, state={"alert": -5.0}),
        Step("asked for the temperature alert", ["get-alert"], ["-5.0"]),
        Step("watched the gadget for 6 seconds", ["wait-event", "6"], ["alert -3.25"],
             events=[(2.0, {"temperature": -3.25}, "the gadget (outdoors) warmed up past the alert temperature")]),
        Step("set the temperature alert to 30 degrees", ["set-alert", "30"], ["alert 30.0"], state={"alert": 30.0}),
        Step("asked for the temperature alert", ["get-alert"], ["30.0"]),
        Step("switched the temperature alert off", ["set-alert", "off"], ["alert off"], state={"alert": None}),
        Step("showed the letters 'TUR' with a line under them", ["show-image"], ["image shown"], image=LETTERS),
        Step("asked for the display brightness", ["get-brightness"], ["20"]),
    ]),
]


# ---------------------------------------------------------------- running

class Bench:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.build = pathlib.Path(args.build).resolve()
        self.out = pathlib.Path(args.out)
        (self.out / "captures").mkdir(parents=True, exist_ok=True)
        self.h = twd.Harness(argparse.Namespace(build=args.build, evidence=args.evidence, user=args.user,
                                                hid_log=""))
        self.h.prefixes.mkdir(parents=True, exist_ok=True)
        os.chown(self.h.prefixes, self.h.uid, self.h.gid)
        self.k = kh.Kmdf(self.h, self.build)
        self.world = World(str(pathlib.Path(args.run_dir) / "world.sock"))
        self.device = json.loads((pathlib.Path(args.run_dir) / "ready").read_text())
        self.index: list[dict] = []
        self.counter = 0

    def overlay(self, sc) -> None:
        pe = kh.wine_pe_dir()
        for name, where in (("ntoskrnl.exe", pe), ("wdfldr.sys", pe), ("wineusb.sys", pe),
                            ("wineusb.so", pe.parent / "x86_64-unix")):
            shutil.copy2(self.build / name, where / name)
            sc.check(f"overlay-{name}", kh.sha256(where / name) == kh.sha256(self.build / name), str(where / name))

    def run_program(self, sc, name: str, args: list[str], step: Step, timeline: list, t0: float):
        gadget = str(self.build / "gadget.exe")
        if not step.events:
            rc = self.h.wine(sc, name, [gadget, *args], twd.DEBUG_QUIET, timeout=90)
            return rc
        proc = self.h.wine(sc, name, [gadget, *args], twd.DEBUG_QUIET, background=True)
        err = sc.dir / f"{name}.err"
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline and "listening for" not in twd.read(err):
            if proc.poll() is not None:
                break
            time.sleep(0.05)
        started = time.time()
        for delay, what, say in step.events:
            time.sleep(max(0.0, started + delay - time.time()))
            at = time.time()
            if "press" in what:
                delivered = self.world.press(*what["press"])
                sc.check(f"{name}:event-delivered", delivered, say)
            else:
                self.world.set(**what)
            timeline.append({"at": round(at - t0, 3), "did": say})
        try:
            return proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            return "timeout"

    def output(self, sc, name: str) -> list[str]:
        lines = twd.read(sc.dir / f"{name}.out").splitlines()
        return [l for l in lines[1:] if not l.startswith("(killed after")]

    def session(self, sc, number: int, title: str, steps: list[Step], boot: bool) -> None:
        stem = f"session-{number:02d}"
        pcap = self.out / "captures" / f"{stem}.pcapng"
        cap = usbmon.Capture(self.device["bus"], self.device["dev"], str(pcap)).start()
        t0 = cap.started_at
        timeline: list[dict] = []
        time.sleep(0.5)
        if boot:
            a = time.time()
            self.h.boot(sc, "boot", DEBUG_HOST)
            time.sleep(5)
            timeline.append({"from": round(a - t0, 3), "to": round(time.time() - t0, 3),
                             "did": "started the computer with the gadget plugged in (Windows loaded its driver)",
                             "shown": []})
        for i, step in enumerate(steps, 1):
            self.counter += 1
            name = f"{stem}-{i:02d}-{step.args[0]}"
            if step.world:
                self.world.set(**step.world)
            args = list(step.args)
            if step.image:
                path = sc.dir / f"{name}.image.txt"
                path.write_text("\n".join(step.image) + "\n")
                os.chmod(path, 0o644)
                args.append(self.h.winpath(path))
            a = time.time()
            entry = {"from": round(a - t0, 3), "did": step.say}
            if step.image:
                entry["picture"] = step.image
            timeline.append(entry)
            rc = self.run_program(sc, name, args, step, timeline, t0)
            entry["to"] = round(time.time() - t0, 3)
            lines = self.output(sc, name)
            entry["shown"] = lines
            state = self.world.state()
            if step.check:
                ok, detail = step.check(lines, state)
            else:
                ok, detail = lines == step.expect, f"{lines!r}" + ("" if lines == step.expect else f" expected {step.expect!r}")
            sc.check(f"{name}:output", ok and rc == step.exit, f"exit {rc}; {detail}")
            if step.state:
                bad = {k: (state.get(k), v) for k, v in step.state.items() if state.get(k) != v}
                sc.check(f"{name}:device-state", not bad, f"device {bad}" if bad else f"device has {step.state}")
            if step.image:
                shown = state.get("picture")
                sc.check(f"{name}:device-picture", shown == as_matrix(step.image), f"device shows {shown}")
            time.sleep(0.7)
        packets = cap.stop()
        sc.check(f"{stem}:captured", packets > 0 and not cap.error, f"{packets} packets{' ' + cap.error if cap.error else ''}")
        (self.out / "captures" / f"{stem}.txt").write_text(usbmon.text(str(pcap), t0, f"{stem}.pcapng"))
        self.write_timeline(stem, number, title, timeline, packets)

    def write_timeline(self, stem: str, number: int, title: str, timeline: list, packets: int) -> None:
        doc = {"session": number, "title": title, "capture": f"{stem}.pcapng", "text": f"{stem}.txt",
               "clock": "seconds since the capture started", "entries": timeline}
        (self.out / "captures" / f"{stem}.timeline.json").write_text(json.dumps(doc, indent=2) + "\n")
        md = [SPDX_MD, f"# Session {number:02d}: {title}", "",
              f"Capture: `{stem}.pcapng` (readable form: `{stem}.txt`), {packets} USB events.",
              "Times are seconds since the capture started, on the same clock as the capture.",
              "Each \"did\" entry is one run of the gadget's Windows program; \"saw\" is what it printed.",
              "Entries with a single time are things that happened while the program was running.", ""]
        for e in timeline:
            if "at" in e:
                md.append(f"- **{e['at']:.2f} s**: {e['did']}")
                continue
            md.append(f"- **{e['from']:.2f} - {e['to']:.2f} s**: did: {e['did']}")
            if e.get("picture"):
                md.append("  - the picture (16 x 8 pixels; `.` is off, `1`-`f` are brightness levels, `f` brightest):")
                md.append("")
                md.append("    ```text")
                md += [f"    {row}" for row in e["picture"]]
                md.append("    ```")
                md.append("")
            if e.get("shown"):
                md.append("  - saw:")
                md.append("")
                md.append("    ```text")
                md += [f"    {line}" for line in e["shown"]]
                md.append("    ```")
                md.append("")
            elif "shown" in e:
                md.append("  - saw: nothing (no output)")
        (self.out / "captures" / f"{stem}.timeline.md").write_text("\n".join(md) + "\n")
        size = (self.out / "captures" / f"{stem}.pcapng").stat().st_size
        self.index.append({"session": number, "title": title, "events": packets, "pcapng_bytes": size,
                           "actions": sum(1 for e in timeline if "from" in e)})

    def run(self) -> None:
        sc = self.h.scenario("drvsynth-capture", "The invented device's Windows driver under Wine; capture sessions",
                             prefix="drvsynth")
        self.overlay(sc)
        self.h.grant(self.device["node"])
        self.h.boot(sc, "boot-init", init=True)
        self.k.install_runtime(sc)
        self.k.install_package(sc, "qfdesk.inf")
        try:
            for number, (title, steps) in enumerate(SESSIONS, 1):
                print(f"===== session {number}: {title}", flush=True)
                self.session(sc, number, title, steps, boot=(number == 1))
        finally:
            logs = sc.logs()
            for step in ("qfdesk: DriverEntry", "qfdesk: EvtDevicePrepareHardware", "qfdesk: EvtDeviceD0Entry",
                         "WdfUsbTargetPipeConfigContinuousReader returned 0x00000000"):
                sc.check(f"driver-step:{step.split(': ')[-1].split()[0]}", step in logs, step)
            crash = next((l for l in logs.splitlines() if "Unhandled exception" in l or "unimplemented function" in l
                          or "KeBugCheck" in l), "")
            sc.check("no-driver-host-crash", not crash, crash[:300] or "none logged")
            self.h.kill(sc)
            self.h.revoke(self.device["node"])
            sc.finish()
        self.verify_with_tshark(sc)
        lines = [SPDX_MD, "# Captures", "", "| Session | Title | User actions | USB events | pcapng bytes |", "|---|---|---|---|---|"]
        for s in self.index:
            lines.append(f"| [{s['session']:02d}](session-{s['session']:02d}.timeline.md) | {s['title']} | "
                         f"{s['actions']} | {s['events']} | {s['pcapng_bytes']} |")
        (self.out / "captures" / "index.md").write_text("\n".join(lines) + "\n")
        (pathlib.Path(self.args.evidence) / "captures.json").write_text(json.dumps(self.index, indent=2) + "\n")

    def verify_with_tshark(self, sc) -> None:
        if not shutil.which("tshark"):
            sc.note("tshark not installed; pcapng files not cross-checked")
            return
        for s in self.index:
            path = self.out / "captures" / f"session-{s['session']:02d}.pcapng"
            r = subprocess.run(["tshark", "-r", str(path), "-T", "fields", "-e", "frame.number"],
                               capture_output=True, text=True)
            n = len(r.stdout.split())
            sc.check(f"tshark-reads-session-{s['session']:02d}", r.returncode == 0 and n == s["events"],
                     f"tshark: {n} frames, ours {s['events']}; {r.stderr.strip()[:200]}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--build", required=True)
    ap.add_argument("--evidence", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--run-dir", required=True, help="the emulator's ready file and world socket")
    ap.add_argument("--user", default="tur")
    args = ap.parse_args()
    if os.geteuid() != 0:
        print("run as root", file=sys.stderr)
        return 2
    bench = Bench(args)
    failed = False
    try:
        bench.run()
    except Exception:
        traceback.print_exc()
        failed = True
    h = bench.h
    h.revoke_all()
    twd.write(h.evidence / "results.json", json.dumps({"label": "drvsynth capture", "scenarios": h.results},
                                                     indent=2) + "\n")
    twd.write(h.evidence / "results.md", twd.markdown(h, "drvsynth: the Windows driver of the invented device, captured"))
    print(twd.read(h.evidence / "results.md"))
    # The captures are only worth handing off if the Windows side did
    # everything the timelines say: any failed check fails the job.
    bad = [c["name"] for r in h.results for c in r["checks"] if c["result"] != "PASS"]
    if bad:
        print(f"{len(bad)} checks failed: {', '.join(bad[:20])}", file=sys.stderr)
    return 1 if failed or bad else 0


if __name__ == "__main__":
    sys.exit(main())
