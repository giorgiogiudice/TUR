#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Run the ladder's applications and score each rung 0 to 4.

    ladder.py --side macos   --apps DIR --ladder ladder/ladder.json --out DIR [--rungs 1-10]
    ladder.py --side darling --apps DIR --ladder ladder/ladder.json --out DIR [--rungs 1-10]

DIR holds the .app bundles the oracle job built (ci/oracle-macos.sh). The
same bundles run on both sides.

Score (report section 5.3; registered before any run):
  0  the program does not start: dyld refuses it, or it crashes before a
     window is mapped
  1  it runs, but no window of the rung's minimum size is mapped in 45 s
  2  a window is mapped but is blank: the window's area in the screenshot
     has fewer than 16 distinct colours
  3  a window with content, but the rung's interaction fails or the
     program dies before the end (60 s)
  4  usable: the window has content, the interaction produced what the rung
     expects (a line on standard output for TUR's own rungs, a visible change
     in the window for the others), and the program is still alive at 60 s

A score of 4 still needs a person to look at the contact sheet
(sheet-<rung>.png beside the oracle's screenshot) before the scoreboard
counts the rung as reached; the scoreboard has a column for that.

macOS side: windows are listed with probes/mfwin (bounds only; no
permission needed). Screenshots use `screencapture -l` and need the Screen
Recording permission, which a hosted runner may not grant: the result says
so. No input is injected on macOS (that needs the Accessibility permission);
the oracle shows that the app reaches its window, and for TUR's own rungs
that it prints macfw-ready.

Darling side: a private Xvfb display with xfwm4 (the harness starts
neither; ci/candidate-linux.sh does). Windows come from wmctrl, screenshots
from ImageMagick's import, input from xdotool.
"""
import argparse
import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
import time

WAIT_WINDOW = 45
ALIVE_FOR = 60


def sh(cmd, timeout=30):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except (subprocess.TimeoutExpired, FileNotFoundError) as e:
        return -1, "", str(e)


def rung_range(spec, n):
    out = set()
    for part in spec.split(","):
        if "-" in part:
            a, b = part.split("-")
            out.update(range(int(a), int(b) + 1))
        elif part:
            out.add(int(part))
    return out or set(range(1, n + 1))


def executable(app):
    with open(os.path.join(app, "Contents", "Info.plist"), "rb") as f:
        info = plistlib.load(f)
    return os.path.join(app, "Contents", "MacOS", info["CFBundleExecutable"]), info.get("CFBundleIdentifier", "")


def fixture(kind, work):
    os.makedirs(work, exist_ok=True)
    if kind == "gen:csv":
        p = os.path.join(work, "table.csv")
        with open(p, "w") as f:
            f.write("name,count,price\n" + "".join("item%d,%d,%d.%02d\n" % (i, i * 3, i, i * 7 % 100) for i in range(1, 41)))
        return p
    if kind == "gen:bytes":
        p = os.path.join(work, "bytes.bin")
        with open(p, "wb") as f:
            f.write(bytes(range(256)) * 64)
        return p
    return None


class Side:
    def __init__(self, a):
        self.a = a

    # Each side implements: start, windows, shot, act, crop_colours, changed


class MacSide(Side):
    def start(self, exe, args, env, log):
        return subprocess.Popen([exe, *args], stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                env={**os.environ, **env})

    def windows(self, proc):
        rc, out, _ = sh([self.a.mfwin, str(proc.pid)])
        wins = []
        for line in out.splitlines():
            try:
                w = json.loads(line)
            except ValueError:
                continue
            x, y, wd, ht = (int(float(v)) for v in w["bounds"].split(","))
            if w["layer"] == 0:
                wins.append({"id": str(w["wid"]), "x": x, "y": y, "w": wd, "h": ht})
            elif ht <= 40:
                wins.append({"id": str(w["wid"]), "x": x, "y": y, "w": wd, "h": ht, "status_item": True})
        return wins

    def shot(self, win, path):
        rc, _, err = sh(["screencapture", "-x", "-o", "-l", win["id"], path])
        return rc == 0 and os.path.exists(path) and os.path.getsize(path) > 0

    def act(self, step, win):
        return "not-run-on-macos"


class DarlingSide(Side):
    def start(self, exe, args, env, log):
        guest = "/Volumes/SystemRoot" + os.path.abspath(exe)
        envs = ["%s=%s" % kv for kv in env.items()]
        cmd = ["timeout", "-s", "KILL", str(ALIVE_FOR + 30), "darling", "shell", "env", "DISPLAY=" + self.a.display, *envs, guest, *args]
        return subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def windows(self, proc):
        rc, out, _ = sh(["wmctrl", "-lG"], 10)
        wins = []
        for line in out.splitlines():
            parts = line.split(None, 7)
            if len(parts) < 7:
                continue
            wid, _, x, y, w, h = parts[:6]
            if wid in self.a.preexisting:
                continue
            wins.append({"id": wid, "x": int(x), "y": int(y), "w": int(w), "h": int(h), "status_item": int(h) <= 40})
        return wins

    def shot(self, win, path):
        rc, _, _ = sh(["import", "-window", "root", "-crop", "%dx%d+%d+%d" % (win["w"], win["h"], win["x"], win["y"]),
                       "+repage", path], 30)
        return rc == 0 and os.path.exists(path)

    def act(self, step, win):
        verb = step[0]
        sh(["xdotool", "windowactivate", "--sync", win["id"]], 5)
        if verb == "type":
            return sh(["xdotool", "type", "--delay", "60", step[1]])[0]
        if verb == "key":
            return sh(["xdotool", "key", step[1]])[0]
        if verb == "click_rel":
            x, y = int(win["w"] * step[1]), int(win["h"] * step[2])
            return sh(["xdotool", "mousemove", "--window", win["id"], str(x), str(y), "click", "1"])[0]
        if verb == "click_status_item":
            return sh(["xdotool", "mousemove", "--window", win["id"], str(win["w"] // 2), str(win["h"] // 2), "click", "1"])[0]
        return "unknown-step"


def colours(path):
    rc, out, _ = sh(["magick", "identify", "-format", "%k", path]) if shutil.which("magick") else sh(["identify", "-format", "%k", path])
    try:
        return int(out.strip())
    except ValueError:
        return -1


def changed(a, b):
    """Share of pixels that differ between two screenshots of the same size."""
    tool = ["magick", "compare"] if shutil.which("magick") else ["compare"]
    rc, out, err = sh([*tool, "-metric", "AE", "-fuzz", "2%", a, b, "null:"])
    try:
        diff = float((err or out).split()[0])
    except (ValueError, IndexError):
        return None
    rc, out, _ = sh(["identify", "-format", "%w %h", a])
    try:
        w, h = (int(v) for v in out.split())
    except ValueError:
        return None
    return diff / float(w * h)


def run_rung(side, r, a):
    work = os.path.join(a.out, "rung%02d" % r["rung"])
    os.makedirs(work, exist_ok=True)
    res = {"rung": r["rung"], "name": r["name"], "side": a.side, "score": 0, "notes": []}
    app_name = r.get("app", r["name"] + ".app")
    app = os.path.join(a.apps, app_name)
    if not os.path.isdir(app):
        res["notes"].append("not built: %s" % app_name)
        res["score"] = None
        return res
    exe, bundle_id = executable(app)
    for dom, kv in r.get("defaults", {}).items():
        for k, v in kv.items():
            cmd = ["defaults", "write", dom, k, "-bool", "true" if v else "false"]
            if a.side == "darling":
                cmd = ["timeout", "-s", "KILL", "30", "darling", "shell", *cmd]
            sh(cmd, 40)
    args = []
    acc = r.get("accept", {})
    if r.get("fixture"):
        fx = fixture(r["fixture"], work)
        args.append(("/Volumes/SystemRoot" + fx) if a.side == "darling" else fx)
    env = {"MACFW_QUIT_AFTER": str(ALIVE_FOR + 20)}
    log_path = os.path.join(work, "output.log")
    log = open(log_path, "wb")
    t0 = time.monotonic()
    proc = side.start(exe, args, env, log)
    win = None
    min_w, min_h = acc.get("window_min", [200, 120])
    while time.monotonic() - t0 < WAIT_WINDOW and proc.poll() is None:
        wins = side.windows(proc)
        big = [w for w in wins if w["w"] >= min_w and w["h"] >= min_h and not w.get("status_item")]
        small = [w for w in wins if w.get("status_item")]
        if big:
            win = max(big, key=lambda w: w["w"] * w["h"])
            break
        if small and acc.get("steps") and acc["steps"][0][0] == "click_status_item" and a.side == "darling":
            side.act(["click_status_item"], small[0])
            res["notes"].append("clicked the status item")
        time.sleep(1)
    res["window_seconds"] = round(time.monotonic() - t0, 1) if win else None
    if proc.poll() is not None and win is None:
        log.close()
        out = open(log_path, errors="replace").read()
        m = re.search(r"(dyld[^\n]*|Symbol not found[^\n]*|Library not loaded[^\n]*|unrecognized selector[^\n]*)", out)
        res["notes"].append("exited %s before a window: %s" % (proc.returncode, m.group(1) if m else "see output.log"))
        return res
    res["score"] = 1
    if win is None:
        res["notes"].append("no window of %dx%d in %ds" % (min_w, min_h, WAIT_WINDOW))
    else:
        res["window"] = win
        time.sleep(3)
        before = os.path.join(work, "before.png")
        if side.shot(win, before):
            n = colours(before)
            res["colours"] = n
            res["score"] = 2 if n < 16 else 3
        else:
            res["notes"].append("no screenshot (on macOS: Screen Recording permission)")
            res["score"] = 3 if a.side == "macos" else 2
        ok = True
        if a.side == "darling" and res["score"] >= 3:
            for step in acc.get("steps", []):
                if step[0] == "click_status_item":
                    continue
                side.act(step, win)
                time.sleep(0.7)
            time.sleep(2)
            after = os.path.join(work, "after.png")
            exp = acc.get("expect")
            if acc.get("expect_all") or (exp and exp.startswith("^")):
                log.flush()
                out = open(log_path, errors="replace").read()
                pats = acc.get("expect_all") or [exp]
                ok = all(re.search(p, out, re.M) for p in pats)
                res["interaction"] = "stdout %s" % ("matched" if ok else "did not match")
            elif exp == "screen-change" and side.shot(win, after):
                frac = changed(before, after)
                res["changed"] = frac
                ok = frac is not None and frac >= 0.005
                res["interaction"] = "screen changed %.3f" % (frac or 0)
            elif exp == "window":
                ok = True
                res["interaction"] = "window only"
        ready = acc.get("ready")
        if ready:
            log.flush()
            if not re.search(ready, open(log_path, errors="replace").read(), re.M):
                ok = False
                res["notes"].append("no ready line")
        while time.monotonic() - t0 < ALIVE_FOR and proc.poll() is None:
            time.sleep(1)
        alive = proc.poll() is None
        res["alive_at_%ds" % ALIVE_FOR] = alive
        if res["score"] >= 3 and ok and alive and a.side == "darling":
            res["score"] = 4
        if a.side == "macos" and res["score"] >= 3 and ok and alive:
            res["score"] = 4  # the oracle is not asked to pass the interaction
    if proc.poll() is None:
        proc.kill()
    log.close()
    if a.side == "darling":
        sh(["timeout", "-s", "KILL", "20", "darling", "shell", "killall", os.path.basename(exe)], 25)
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--side", choices=["macos", "darling"], required=True)
    ap.add_argument("--apps", required=True)
    ap.add_argument("--ladder", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--rungs", default="")
    ap.add_argument("--mfwin", default="")
    ap.add_argument("--display", default=os.environ.get("DISPLAY", ":99"))
    a = ap.parse_args()
    with open(a.ladder) as f:
        ladder = json.load(f)
    os.makedirs(a.out, exist_ok=True)
    a.preexisting = set()
    if a.side == "darling":
        rc, out, _ = sh(["wmctrl", "-lG"], 10)
        a.preexisting = {l.split()[0] for l in out.splitlines() if l.strip()}
    side = MacSide(a) if a.side == "macos" else DarlingSide(a)
    want = rung_range(a.rungs, len(ladder["rungs"]))
    results = []
    for r in ladder["rungs"]:
        if r["rung"] not in want:
            continue
        res = run_rung(side, r, a)
        results.append(res)
        print("rung %2d %-14s score %s %s" % (r["rung"], r["name"], res["score"], "; ".join(res["notes"])))
        sys.stdout.flush()
    with open(os.path.join(a.out, "ladder-%s.json" % a.side), "w") as f:
        json.dump(results, f, indent=1)


if __name__ == "__main__":
    main()
