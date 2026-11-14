#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""SEALED: the hidden scoring suite of the driver-synthesis experiment.

Runs a candidate Linux driver (a program following handoff/TASK.md's
command-line contract) against the emulated device, with values that
appear in no capture, and reports pass/fail per feature. Each feature is
judged from the device's side (the emulator's world socket: what it shows,
stores and counted), not only from what the candidate prints.

  score.py run --program PATH --name NAME --run-dir DIR --handoff DIR --out DIR [--user drvsynth]
  score.py validate --out DIR       the suite discriminates: the reference
                                    passes everything, the wrong drivers fail
                                    what they must

Runs as root (it talks to the world socket); every candidate run is an
unprivileged user's (--user), which cannot reach the socket.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import pwd
import random
import re
import string
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "bench"))
from world import World  # noqa: E402

RNG = random.SystemRandom()
NUMBER = re.compile(r"-?\d+(?:\.\d+)?")
TEXT_CHARS = string.ascii_letters + string.digits + " .,;:!?()'-"


class Scorer:
    def __init__(self, args: argparse.Namespace) -> None:
        self.program = pathlib.Path(args.program).resolve()
        self.name = args.name
        self.world = World(str(pathlib.Path(args.run_dir) / "world.sock"))
        entry = pwd.getpwnam(args.user)
        self.uid, self.gid, self.home = entry.pw_uid, entry.pw_gid, entry.pw_dir
        self.out = pathlib.Path(args.out)
        self.scratch = pathlib.Path(f"/tmp/drvsynth-score-{self.name}")
        self.scratch.mkdir(parents=True, exist_ok=True)
        os.chown(self.scratch, self.uid, self.gid)
        self.tokens = self.captured_tokens(pathlib.Path(args.handoff))
        self.features: list[dict] = []
        self.runs: list[dict] = []
        self.current: dict | None = None

    # ------------------------------------------------------------ fresh values
    @staticmethod
    def captured_tokens(handoff: pathlib.Path) -> set[str]:
        """Every number that appears in a capture's timeline: never reused."""
        tokens: set[str] = set()
        for path in sorted(handoff.glob("captures/*.timeline.json")):
            tokens.update(NUMBER.findall(path.read_text()))
        return tokens

    def fresh(self, make, fmt=str):
        for _ in range(1000):
            v = make()
            if fmt(v) not in self.tokens:
                self.tokens.add(fmt(v))
                return v
        raise RuntimeError("no fresh value")

    def fresh_text(self, lo: int, hi: int) -> str:
        n = RNG.randint(lo, hi)
        text = RNG.choice(string.ascii_letters) + "".join(RNG.choice(TEXT_CHARS) for _ in range(n - 1))
        return text.rstrip() + RNG.choice(string.ascii_letters) if text != text.rstrip() else text

    # ------------------------------------------------------------ running the candidate
    def argv(self, args: list[str]) -> list[str]:
        return ["setpriv", f"--reuid={self.uid}", f"--regid={self.gid}", "--init-groups", "--", "env", "-i",
                f"HOME={self.home}", "PATH=/usr/local/bin:/usr/bin:/bin", "LANG=C.UTF-8", "PYTHONDONTWRITEBYTECODE=1",
                "python3", str(self.program), *args]

    def record(self, args, rc, stdout, stderr, seconds) -> list[str]:
        lines = [l.rstrip() for l in stdout.splitlines()]
        while lines and not lines[-1]:
            lines.pop()
        self.runs.append({"feature": self.current["name"] if self.current else None, "args": args, "exit": rc,
                          "stdout": lines, "stderr": stderr.strip()[-600:], "seconds": round(seconds, 2)})
        return lines

    def run(self, args: list[str], timeout: float = 20) -> tuple[object, list[str]]:
        t = time.monotonic()
        try:
            p = subprocess.run(self.argv(args), capture_output=True, text=True, timeout=timeout,
                               cwd=str(self.program.parent))
            rc, so, se = p.returncode, p.stdout, p.stderr
        except subprocess.TimeoutExpired as exc:
            rc, so, se = "timeout", (exc.stdout or b"").decode(errors="replace") if isinstance(exc.stdout, bytes) \
                else (exc.stdout or ""), "killed after timeout"
        return rc, self.record(args, rc, so, se, time.monotonic() - t)

    # ------------------------------------------------------------ features
    def feature(self, name: str, title: str):
        scorer = self

        class Ctx:
            def __enter__(self_inner):
                scorer.current = {"name": name, "title": title, "checks": [],
                                  "counters_before": scorer.world.state()["counters"]}
                return self_inner

            def __exit__(self_inner, etype, exc, tb):
                f = scorer.current
                if exc is not None:
                    f["checks"].append({"check": "no-error", "ok": False, "detail": repr(exc)})
                f["counters_after"] = scorer.world.state()["counters"]
                f["result"] = "PASS" if f["checks"] and all(c["ok"] for c in f["checks"]) else "FAIL"
                scorer.features.append(f)
                print(f"[{scorer.name}] {f['result']} {name}", flush=True)
                for c in f["checks"]:
                    if not c["ok"]:
                        print(f"    FAIL {c['check']}: {c['detail'][:300]}", flush=True)
                scorer.current = None
                return True  # an exception fails the feature, not the suite

        return Ctx()

    def check(self, name: str, ok: bool, detail: str = "") -> bool:
        self.current["checks"].append({"check": name, "ok": bool(ok), "detail": detail})
        return ok

    def expect(self, name: str, args: list[str], want: list[str], want_rc: int = 0) -> list[str]:
        rc, out = self.run(args)
        self.check(name, rc == want_rc and out == want, f"{args}: exit {rc}, printed {out!r}; wanted {want!r}")
        return out

    def state_is(self, name: str, **want) -> None:
        st = self.world.state()
        bad = {k: (st.get(k), v) for k, v in want.items() if st.get(k) != v}
        self.check(name, not bad, f"device: {bad}" if bad else f"device has {want}")

    def f_info(self) -> None:
        with self.feature("info", "device information (firmware version and uptime)"):
            before = self.world.state()
            rc, out = self.run(["info"])
            after = self.world.state()
            ok = (rc == 0 and len(out) == 2 and out[0] == f"firmware {after['firmware']}" and
                  re.fullmatch(r"uptime \d+", out[1] or "") is not None and
                  before["uptime"] - 2 <= int(out[1].split()[1]) <= after["uptime"] + 2)
            self.check("info", ok, f"printed {out!r}; firmware {after['firmware']}, uptime {before['uptime']}..{after['uptime']}")

    def f_temperature(self) -> None:
        with self.feature("temperature", "read-temperature, including negative values"):
            fmt = lambda v: f"{v:.2f}"
            for make in (lambda: round(RNG.uniform(-39.99, -1.0), 2), lambda: round(RNG.uniform(-0.99, -0.01), 2),
                         lambda: round(RNG.uniform(0.01, 79.99), 2)):
                v = self.fresh(make, fmt)
                self.world.set(temperature=v)
                self.expect(f"temperature {fmt(v)}", ["read-temperature"], [fmt(v)])

    def f_humidity(self) -> None:
        with self.feature("humidity", "read-humidity"):
            fmt = lambda v: f"{v:.1f}"
            for _ in range(2):
                v = self.fresh(lambda: round(RNG.uniform(0.0, 100.0), 1), fmt)
                self.world.set(humidity=v)
                self.expect(f"humidity {fmt(v)}", ["read-humidity"], [fmt(v)])

    def f_light(self) -> None:
        with self.feature("light", "read-light, including values above 65535"):
            for make in (lambda: RNG.randint(65536, 1 << 24), lambda: RNG.randint(2, 65534)):
                v = self.fresh(make)
                self.world.set(light=v)
                self.expect(f"light {v}", ["read-light"], [str(v)])

    def f_sensors(self) -> None:
        with self.feature("sensors", "read-sensors"):
            t = self.fresh(lambda: round(RNG.uniform(-30.0, 45.0), 2), lambda v: f"{v:.2f}")
            h = self.fresh(lambda: round(RNG.uniform(0.0, 100.0), 1), lambda v: f"{v:.1f}")
            lux = self.fresh(lambda: RNG.randint(0, 3_000_000))
            self.world.set(temperature=t, humidity=h, light=lux)
            self.expect("read-sensors", ["read-sensors"], [f"temperature {t:.2f}", f"humidity {h:.1f}", f"light {lux}"])

    def fresh_level(self) -> int:
        return self.fresh(lambda: RNG.randint(1, 99))

    def f_brightness(self) -> None:
        with self.feature("brightness", "set-brightness and get-brightness in day mode"):
            self.world.reset()
            n = self.fresh_level()
            self.expect("set-brightness", ["set-brightness", str(n)], [f"brightness {n}"])
            self.state_is("device brightness", day_brightness=n, mode="day")
            m = self.fresh_level()
            self.world.set(day_brightness=m)
            self.expect("get-brightness", ["get-brightness"], [str(m)])

    def f_modes(self) -> None:
        with self.feature("modes", "set-mode and get-mode"):
            self.world.reset()
            self.expect("set-mode night", ["set-mode", "night"], ["mode night"])
            self.state_is("device in night mode", mode="night")
            self.expect("get-mode night", ["get-mode"], ["night"])
            self.expect("set-mode day", ["set-mode", "day"], ["mode day"])
            self.state_is("device in day mode", mode="day")
            self.expect("get-mode day", ["get-mode"], ["day"])

    def f_night(self) -> None:
        with self.feature("night-level", "night-light level, and the day brightness while in night mode"):
            self.world.reset()
            self.world.set(mode="night")
            n = self.fresh_level()
            self.expect("set-night-level (in night mode)", ["set-night-level", str(n)], [f"night-level {n}"])
            self.state_is("device night level", night_level=n, mode="night")
            m = self.fresh_level()
            self.world.set(night_level=m)
            self.expect("get-night-level", ["get-night-level"], [str(m)])
            b = self.fresh_level()
            self.expect("set-brightness (in night mode)", ["set-brightness", str(b)], [f"brightness {b}"])
            self.state_is("day brightness set, still night mode", day_brightness=b, night_level=m, mode="night")
            self.world.set(mode="day")
            k = self.fresh_level()
            self.expect("set-night-level (in day mode)", ["set-night-level", str(k)], [f"night-level {k}"])
            self.state_is("device night level, still day mode", night_level=k, mode="day")

    def f_alert(self) -> None:
        with self.feature("alert", "set-alert and get-alert, positive, negative and off"):
            self.world.reset()
            fmt = lambda v: f"{v:.1f}"
            t1 = self.fresh(lambda: round(RNG.uniform(10.0, 60.0), 1), fmt)
            self.expect("set-alert positive", ["set-alert", fmt(t1)], [f"alert {fmt(t1)}"])
            self.state_is("device alert", alert=t1, mode="day")
            t2 = self.fresh(lambda: round(RNG.uniform(-39.9, -0.1), 1), fmt)
            self.world.set(mode="night")
            self.expect("set-alert negative (in night mode)", ["set-alert", fmt(t2)], [f"alert {fmt(t2)}"])
            self.state_is("device alert, still night mode", alert=t2, mode="night")
            u = self.fresh(lambda: round(RNG.uniform(-20.0, 90.0), 1), fmt)
            self.world.set(alert=u)
            self.expect("get-alert", ["get-alert"], [fmt(u)])
            self.expect("set-alert off", ["set-alert", "off"], ["alert off"])
            self.state_is("device alert off", alert=None, mode="night")
            self.expect("get-alert off", ["get-alert"], ["off"])

    def f_image(self) -> None:
        with self.feature("image", "show-image and clear-display"):
            self.world.reset()
            for i in range(2):
                rows = ["".join(RNG.choice("0123456789abcdef") for _ in range(16)) for _ in range(8)]
                path = self.scratch / f"image-{i}.txt"
                path.write_text("\n".join(r.replace("0", ".") for r in rows) + "\n")
                os.chmod(path, 0o644)
                self.expect(f"show-image {i}", ["show-image", str(path)], ["image shown"])
                shown = self.world.state()["picture"]
                self.check(f"device shows image {i}", shown == rows, f"device {shown}, wanted {rows}")
            self.expect("clear-display", ["clear-display"], ["display cleared"])
            self.state_is("device display dark", picture=["0" * 16] * 8)

    def f_notes_write(self) -> None:
        with self.feature("notes-write", "write-note, short and long (several chunks)"):
            self.world.reset()
            a, b = RNG.sample(range(8), 2)
            long_text, short_text = self.fresh_text(81, 120), self.fresh_text(1, 9)
            self.expect("write-note long", ["write-note", str(a), long_text], [f"note {a} stored"])
            self.expect("write-note short", ["write-note", str(b), short_text], [f"note {b} stored"])
            self.state_is("device stores the notes", notes={str(s): t for s, t in sorted({a: long_text, b: short_text}.items())})

    def f_notes_read(self) -> None:
        with self.feature("notes-read", "read-note, long and short, and an empty slot"):
            self.world.reset()
            a, b, empty = RNG.sample(range(8), 3)
            long_text, short_text = self.fresh_text(41, 120), self.fresh_text(1, 39)
            self.world.note(a, long_text)
            self.world.note(b, short_text)
            self.expect("read-note long", ["read-note", str(a)], [long_text])
            self.expect("read-note short", ["read-note", str(b)], [short_text])
            self.expect("read-note empty", ["read-note", str(empty)], ["empty"], want_rc=4)

    def f_notes_list(self) -> None:
        with self.feature("notes-list-delete", "list-notes and delete-note"):
            self.world.reset()
            slots = sorted(RNG.sample(range(8), 3))
            texts = {s: self.fresh_text(1, 120) for s in slots}
            for s, t in texts.items():
                self.world.note(s, t)
            self.expect("list-notes", ["list-notes"], [f"{s} {len(texts[s])}" for s in slots])
            gone = RNG.choice(slots)
            self.expect("delete-note", ["delete-note", str(gone)], [f"note {gone} deleted"])
            left = {str(s): t for s, t in texts.items() if s != gone}
            self.state_is("device deleted the note", notes=left)
            self.expect("list-notes after delete", ["list-notes"], [f"{s} {len(texts[s])}" for s in slots if s != gone])

    def f_clear(self) -> None:
        with self.feature("clear-notes", "clear-notes (the device's confirmation quirk)"):
            self.world.reset()
            for s in RNG.sample(range(8), 3):
                self.world.note(s, self.fresh_text(1, 60))
            self.expect("clear-notes", ["clear-notes"], ["notes cleared"])
            self.state_is("device has no notes", notes={})
            self.expect("list-notes empty", ["list-notes"], [])

    def f_events(self) -> None:
        with self.feature("events", "wait-event: button presses and a temperature alert, nothing else"):
            self.world.reset()
            fmt1 = lambda v: f"{v:.1f}"
            a = self.fresh(lambda: round(RNG.uniform(15.0, 35.0), 1), fmt1)
            self.world.set(alert=a, temperature=round(a - RNG.uniform(1.0, 5.0), 2))
            b1, b2 = RNG.choice((1, 2)), RNG.choice((1, 2))
            d1 = self.fresh(lambda: round(RNG.uniform(0.05, 0.6), 2), lambda v: f"{v:.2f}")
            d2 = self.fresh(lambda: round(RNG.uniform(0.7, 4.0), 2), lambda v: f"{v:.2f}")
            hot = self.fresh(lambda: round(a + RNG.uniform(0.2, 6.0), 2), lambda v: f"{v:.2f}")
            auth0 = self.world.state()["counters"]["auth_ok"]
            args = ["wait-event", "8"]
            t = time.monotonic()
            proc = subprocess.Popen(self.argv(args), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                    cwd=str(self.program.parent))
            opened = None
            while time.monotonic() - t < 3.0:
                st = self.world.state()
                if st["session"] and st["counters"]["auth_ok"] > auth0:
                    opened = time.monotonic()
                    break
                time.sleep(0.05)
            self.check("session open within 2 s", opened is not None and opened - t <= 2.5,
                       f"{(opened - t) if opened else None}")
            if opened is not None:
                for delay, act in ((1.0, lambda: self.world.press(b1, d1)), (2.0, lambda: self.world.press(b2, d2)),
                                   (3.0, lambda: self.world.set(temperature=hot))):
                    time.sleep(max(0.0, opened + delay - time.monotonic()))
                    act()
            try:
                so, se = proc.communicate(timeout=20)
                rc = proc.returncode
            except subprocess.TimeoutExpired:
                proc.kill()
                so, se = proc.communicate()
                rc = "timeout"
            out = self.record(args, rc, so, se, time.monotonic() - t)
            want = [f"button {b1} {d1:.2f}", f"button {b2} {d2:.2f}", f"alert {hot:.2f}"]
            self.check("events printed", rc == 0 and out == want, f"exit {rc}, printed {out!r}; wanted {want!r}")

    # ------------------------------------------------------------ whole-run checks
    def whole_run(self, start: dict, end: dict) -> None:
        delta = {k: end[k] - start.get(k, 0) for k in end}
        quirk_features = [f for f in self.features if f["name"] in ("modes", "night-level", "alert")]
        dropped = sum(f["counters_after"]["dropped_busy"] - f["counters_before"]["dropped_busy"] for f in quirk_features)
        self.current = {"name": "mode-change-delay", "title": "the device's pause after a mode change is respected",
                        "checks": []}
        self.check("no frame sent while the device was busy after a mode change", dropped == 0,
                   f"{dropped} frames dropped by the device during its busy time in modes/night-level/alert")
        mode_changes_ok = all(f["result"] == "PASS" for f in quirk_features if f["name"] != "modes")
        self.check("settings that need a mode change worked", mode_changes_ok,
                   ", ".join(f"{f['name']} {f['result']}" for f in quirk_features))
        self.close_whole()
        self.current = {"name": "checksums", "title": "every frame's checksum, on payloads never captured",
                        "checks": []}
        self.check("no frame refused for its checksum or form", delta["bad_checksum"] == 0 and delta["bad_frame"] == 0,
                   f"bad checksum {delta['bad_checksum']}, malformed {delta['bad_frame']}")
        self.check("the device executed the candidate's commands", delta["executed"] >= 40,
                   f"{delta['executed']} commands executed")
        self.close_whole()
        runs = len(self.runs)
        self.current = {"name": "handshake", "title": "the nonce-dependent handshake, fresh in every run",
                        "checks": []}
        self.check("every run opened a session", delta["auth_ok"] >= runs and delta["auth_fail"] == 0,
                   f"{runs} runs, {delta['auth_ok']} sessions opened, {delta['auth_fail']} refused")
        self.close_whole()

    def close_whole(self) -> None:
        f = self.current
        f["result"] = "PASS" if all(c["ok"] for c in f["checks"]) else "FAIL"
        self.features.append(f)
        print(f"[{self.name}] {f['result']} {f['name']}", flush=True)
        self.current = None

    def score(self) -> dict:
        start = self.world.state()["counters"]
        for f in (self.f_info, self.f_temperature, self.f_humidity, self.f_light, self.f_sensors, self.f_brightness,
                  self.f_modes, self.f_night, self.f_alert, self.f_image, self.f_notes_write, self.f_notes_read,
                  self.f_notes_list, self.f_clear, self.f_events):
            f()
        end = self.world.state()["counters"]
        self.whole_run(start, end)
        self.world.reset()
        passed = sum(1 for f in self.features if f["result"] == "PASS")
        result = {"driver": self.name, "program": str(self.program), "passed": passed, "total": len(self.features),
                  "verdict": "PASS" if passed == len(self.features) else "FAIL",
                  "features": [{k: v for k, v in f.items() if not k.startswith("counters")} for f in self.features],
                  "runs": self.runs}
        self.out.mkdir(parents=True, exist_ok=True)
        (self.out / f"score-{self.name}.json").write_text(json.dumps(result, indent=2) + "\n")
        (self.out / f"score-{self.name}.md").write_text(markdown(result))
        return result


def markdown(r: dict) -> str:
    lines = [f"### Scoring: {r['driver']} — {r['verdict']} ({r['passed']} of {r['total']} features)", "",
             "| Feature | Result | Failed checks |", "|---|---|---|"]
    for f in r["features"]:
        bad = "; ".join(f"{c['check']}: {c['detail']}" for c in f["checks"] if not c["ok"])
        lines.append(f"| {f['name']} | {f['result']} | {bad.replace('|', '/')[:400]} |")
    return "\n".join(lines) + "\n"


# The wrong drivers' expected outcome (sealed/wrong/*): the suite is valid
# only if it fails what each gets wrong and, for "quirks", passes what it
# gets right.
EXPECT = {
    "reference": {"pass": "all"},
    "wrong-replay": {"fail": "all"},
    "wrong-quirks": {
        "fail": ["temperature", "sensors", "night-level", "alert", "clear-notes", "events", "mode-change-delay"],
        "pass": ["info", "humidity", "light", "brightness", "image", "notes-write", "notes-read",
                 "notes-list-delete", "checksums", "handshake"],
    },
}


def validate(out: pathlib.Path) -> int:
    lines, ok = ["### Is the scoring suite valid?", ""], True
    for name, want in EXPECT.items():
        path = out / f"score-{name}.json"
        if not path.exists():
            lines.append(f"- {name}: **not scored**")
            ok = False
            continue
        r = json.loads(path.read_text())
        results = {f["name"]: f["result"] for f in r["features"]}
        problems = []
        if want.get("pass") == "all":
            problems += [n for n, v in results.items() if v != "PASS"]
        elif want.get("pass"):
            problems += [n for n in want["pass"] if results.get(n) != "PASS"]
        if want.get("fail") == "all":
            problems += [n for n, v in results.items() if v != "FAIL"]
        elif want.get("fail"):
            problems += [n for n in want["fail"] if results.get(n) != "FAIL"]
        ok = ok and not problems
        lines.append(f"- {name}: {r['passed']} of {r['total']} features pass; "
                     + ("as expected" if not problems else f"**unexpected: {', '.join(problems)}**"))
    lines += ["", f"Suite valid: **{'yes' if ok else 'no'}**", ""]
    (out / "validation.md").write_text("\n".join(lines))
    print("\n".join(lines))
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)
    r = sub.add_parser("run")
    r.add_argument("--program", required=True)
    r.add_argument("--name", required=True)
    r.add_argument("--run-dir", required=True)
    r.add_argument("--handoff", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--user", default="drvsynth")
    v = sub.add_parser("validate")
    v.add_argument("--out", required=True)
    args = ap.parse_args()
    if args.command == "validate":
        return validate(pathlib.Path(args.out))
    if os.geteuid() != 0:
        print("run as root", file=sys.stderr)
        return 2
    result = Scorer(args).score()
    print(markdown(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
