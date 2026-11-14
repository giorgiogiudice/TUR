#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Run Android applications under the Android Translation Layer and record what happens.

    harness.py --apks DIR --out DIR [--lock FILE ...] [--probe APK] [--scale N] [--limit N]

Runs inside a Wayland session (sway, headless) in an Alpine container; see
run-in-alpine.sh. For each pinned application: start it, wait for a window,
wait for the picture to stop changing, take a screenshot, read memory, dump
the accessibility tree, resize the window, take a second screenshot, stop
it. The probe application (tests/experiments/android/probe) is then run
through its phases for the integration facts.

The harness never judges an application "usable": it records what can be
measured (a window, a live process, how many colours the picture has, the
errors logged). The verdict is made by a person looking at the screenshots
and is recorded in the report.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import signal
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "fdroid"))
import fdroid_index  # noqa: E402

ATL = "android-translation-layer"
ERROR_RE = re.compile(
    r"NoSuchMethodError|NoSuchFieldError|ClassNotFoundException|NoClassDefFoundError|UnsatisfiedLinkError|"
    r"AbstractMethodError|VerifyError|Fatal signal|SIGSEGV|SIGABRT|SIGILL|SIGBUS|Segmentation fault|Aborted|"
    r"FATAL EXCEPTION|Exception in thread|java\.lang\.\w*(Exception|Error)|android\.\w+(\.\w+)*Exception|"
    r"cannot locate symbol|dlopen failed|not implemented|STUB|stub")


def run(cmd: list[str], timeout: float = 15, **kw) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, **kw)
    except subprocess.TimeoutExpired as e:
        return subprocess.CompletedProcess(cmd, 124, e.stdout or "", "timeout")
    except FileNotFoundError as e:
        return subprocess.CompletedProcess(cmd, 127, "", str(e))


def windows() -> list[dict]:
    r = run(["swaymsg", "-t", "get_tree"])
    if r.returncode != 0 or not r.stdout:
        return []
    out = []

    def walk(node: dict) -> None:
        if node.get("pid") and node.get("type") in ("con", "floating_con") and not node.get("nodes"):
            out.append({"pid": node["pid"], "app_id": node.get("app_id"), "name": node.get("name"),
                        "rect": node.get("rect"), "id": node.get("id")})
        for key in ("nodes", "floating_nodes"):
            for child in node.get(key, []):
                walk(child)

    try:
        walk(json.loads(r.stdout))
    except json.JSONDecodeError:
        return []
    return out


def session_pids(sid: int) -> list[int]:
    pids = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            stat = pathlib.Path(f"/proc/{entry}/stat").read_text()
            fields = stat.rsplit(")", 1)[1].split()
            if int(fields[3]) == sid:
                pids.append(int(entry))
        except (OSError, IndexError, ValueError):
            continue
    return pids


def memory_kib(pids: list[int]) -> dict:
    pss = rss = 0
    names = []
    for pid in pids:
        try:
            for line in pathlib.Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
                if line.startswith("Pss:"):
                    pss += int(line.split()[1])
                elif line.startswith("Rss:"):
                    rss += int(line.split()[1])
            names.append(pathlib.Path(f"/proc/{pid}/comm").read_text().strip())
        except OSError:
            continue
    return {"pss_kib": pss, "rss_kib": rss, "processes": names}


def cpu_seconds(pids: list[int]) -> float:
    ticks = 0
    for pid in pids:
        try:
            fields = pathlib.Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            ticks += int(fields[11]) + int(fields[12])
        except (OSError, IndexError, ValueError):
            continue
    return ticks / os.sysconf("SC_CLK_TCK")


def frame_hash(rect: dict | None) -> str:
    cmd = ["grim", "-t", "ppm"]
    if rect:
        cmd += ["-g", f"{rect['x']},{rect['y']} {rect['width']}x{rect['height']}"]
    cmd.append("-")
    try:
        r = subprocess.run(cmd, capture_output=True, timeout=20)
    except subprocess.TimeoutExpired:
        return ""
    return hashlib.sha256(r.stdout).hexdigest() if r.returncode == 0 else ""


def screenshot(path: pathlib.Path, rect: dict | None = None) -> bool:
    cmd = ["grim"]
    if rect and rect.get("width", 0) > 0:
        cmd += ["-g", f"{rect['x']},{rect['y']} {rect['width']}x{rect['height']}"]
    cmd.append(str(path))
    return run(cmd, timeout=30).returncode == 0


def colours(path: pathlib.Path) -> int:
    r = run(["magick", "identify", "-format", "%k", str(path)], timeout=60)
    if r.returncode != 0:
        r = run(["identify", "-format", "%k", str(path)], timeout=60)
    try:
        return int(r.stdout.strip())
    except ValueError:
        return -1


def log_findings(log: pathlib.Path) -> dict:
    counts: dict[str, int] = {}
    first: list[str] = []
    try:
        with open(log, errors="replace") as f:
            for line in f:
                m = ERROR_RE.search(line)
                if not m:
                    continue
                key = m.group(0)
                if key.lower() in ("stub", "not implemented"):
                    key = "stub or not implemented"
                counts[key] = counts.get(key, 0) + 1
                if key != "stub or not implemented" and len(first) < 12:
                    line = line.strip()[:300]
                    if line not in first:
                        first.append(line)
    except OSError:
        pass
    return {"counts": counts, "first_lines": first}


def cap_log(log: pathlib.Path, keep: int = 1500) -> None:
    """Keep the head and the tail of a log; an application in a crash loop writes gigabytes."""
    try:
        if log.stat().st_size < 600_000:
            return
        with open(log, errors="replace") as f:
            lines = f.readlines()
        if len(lines) > 2 * keep:
            lines = lines[:keep] + [f"... {len(lines) - 2 * keep} lines removed ...\n"] + lines[-keep:]
        with open(log, "w") as f:
            f.writelines(line[:2000] + ("\n" if len(line) > 2000 else "") for line in lines)
    except OSError:
        pass


class App:
    """One launch of an application under ATL."""

    def __init__(self, apk: pathlib.Path, label: str, out: pathlib.Path, extras: list[str] | None = None, scale: float = 1.0):
        self.apk, self.label, self.out, self.scale = apk, label, out, scale
        self.log = out / "logs" / f"{label}.log"
        self.log.parent.mkdir(parents=True, exist_ok=True)
        data = out / "data" / label
        shutil.rmtree(data, ignore_errors=True)
        data.mkdir(parents=True)
        self.data = data
        env = dict(os.environ)
        env.update({
            "ANDROID_APP_DATA_DIR": str(data),
            "GDK_BACKEND": "wayland",
            # WebView is off by default in ATL; a browser or a web-view
            # application is nothing without it.
            "ATL_UGLY_ENABLE_WEBVIEW": "1",
            # WebKitGTK's own sandbox needs user namespaces, which the CI
            # container does not have; this is a property of the test bed.
            "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS": "1",
            "WEBKIT_DISABLE_COMPOSITING_MODE": "1",
        })
        cmd = [ATL, str(apk), "--window-width=540", "--window-height=960"] + (extras or [])
        self.cmd = cmd
        self.logf = open(self.log, "w")
        self.t0 = time.monotonic()
        self.proc = subprocess.Popen(cmd, stdout=self.logf, stderr=subprocess.STDOUT, env=env,
                                     start_new_session=True, cwd=str(data))
        self.sid = self.proc.pid
        self.window: dict | None = None

    def pids(self) -> list[int]:
        return session_pids(self.sid)

    def alive(self) -> bool:
        return self.proc.poll() is None

    def find_window(self) -> dict | None:
        mine = set(self.pids())
        for w in windows():
            if w["pid"] in mine:
                return w
        return None

    def wait_window(self, timeout: float) -> float | None:
        deadline = self.t0 + timeout * self.scale
        while time.monotonic() < deadline:
            w = self.find_window()
            if w:
                self.window = w
                return time.monotonic() - self.t0
            if not self.alive():
                return None
            time.sleep(0.2)
        return None

    def wait_stable(self, timeout: float) -> float | None:
        """Seconds from launch until the window's picture stopped changing (two equal frames a second apart)."""
        deadline = time.monotonic() + timeout * self.scale
        prev = None
        prev_t = None
        while time.monotonic() < deadline:
            w = self.find_window()
            if not w:
                return None
            self.window = w
            t = time.monotonic()
            h = frame_hash(w["rect"])
            if h and h == prev:
                return prev_t - self.t0
            prev, prev_t = h, t
            time.sleep(1.0)
        return None

    def text(self) -> str:
        self.logf.flush()
        try:
            return self.log.read_text(errors="replace")
        except OSError:
            return ""

    def probe_lines(self) -> list[str]:
        lines = []
        for line in self.text().splitlines():
            m = re.search(r"TURPROBE\W*(?:\d+\W+)?(\w+=.*)$", line)
            if m and m.group(1) not in lines:
                lines.append(m.group(1).strip())
        for f in self.data.rglob("turprobe.log"):
            for line in f.read_text(errors="replace").splitlines():
                if line.strip() and line.strip() not in lines:
                    lines.append(line.strip())
        return lines

    def stop(self) -> int | None:
        for sig in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.killpg(self.sid, sig)
            except ProcessLookupError:
                break
            try:
                self.proc.wait(timeout=4)
                break
            except subprocess.TimeoutExpired:
                continue
        for pid in self.pids():
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        self.logf.close()
        cap_log(self.log)
        return self.proc.returncode


def a11y(pids: list[int]) -> dict:
    r = run([sys.executable, str(HERE / "a11y_dump.py")] + [str(p) for p in pids], timeout=45)
    try:
        return json.loads(r.stdout.strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        return {"error": (r.stderr or r.stdout)[-400:]}


def run_sample_app(row: dict, apks: pathlib.Path, out: pathlib.Path, scale: float) -> dict:
    pkg = row["package"]
    res = {"package": pkg, "group": row["group"], "name": row["name"], "version_name": row["version_name"],
           "version_code": row["version_code"], "sha256": row["sha256"], "nativecode": row["nativecode"],
           "abi_class": row["abi_class"], "size": int(row["size"])}
    src = apks / f"{row['sha256']}.apk"
    if not src.exists():
        res["outcome"] = "not run: APK not downloaded"
        return res
    stage = out / "stage"
    stage.mkdir(exist_ok=True)
    apk = stage / f"{pkg}.apk"
    if apk.exists():
        apk.unlink()
    try:
        os.link(src, apk)
    except OSError:
        shutil.copy(src, apk)
    shots = out / "shots"
    shots.mkdir(exist_ok=True)
    app = App(apk, pkg, out, scale=scale)
    try:
        t_window = app.wait_window(60)
        res["window_seconds"] = round(t_window, 2) if t_window is not None else None
        if t_window is None:
            res["outcome"] = "no window: " + ("still running after the time limit" if app.alive()
                                              else f"exited with status {app.proc.returncode}")
            screenshot(shots / f"{pkg}-full.png")
            return res
        t_stable = app.wait_stable(20)
        res["stable_seconds"] = round(t_stable, 2) if t_stable is not None else None
        time.sleep(1.0 * scale)
        w = app.find_window()
        res["alive_at_screenshot"] = app.alive()
        if w:
            res["window"] = {"app_id": w["app_id"], "title": w["name"], "width": w["rect"]["width"], "height": w["rect"]["height"]}
            screenshot(shots / f"{pkg}-1.png", w["rect"])
            res["colours"] = colours(shots / f"{pkg}-1.png")
        screenshot(shots / f"{pkg}-full.png")
        pids = app.pids()
        res["memory"] = memory_kib(pids)
        res["cpu_seconds"] = round(cpu_seconds(pids), 2)
        res["windows_of_app"] = sum(1 for x in windows() if x["pid"] in set(pids))
        acc = a11y(pids)
        res["a11y"] = {k: acc.get(k) for k in ("found", "nodes", "roles", "actions", "error")}
        res["a11y"]["names"] = (acc.get("names") or [])[:25]
        res["a11y"]["texts"] = (acc.get("texts") or [])[:25]
        # Resize: does the application survive and follow?
        if w and app.alive():
            run(["swaymsg", f"[pid={w['pid']}]", "resize", "set", "width", "760", "px", "height", "620", "px"])
            time.sleep(2.5 * scale)
            w2 = app.find_window()
            res["resize"] = {"alive": app.alive(),
                             "size": f"{w2['rect']['width']}x{w2['rect']['height']}" if w2 else None}
            if w2:
                screenshot(shots / f"{pkg}-2.png", w2["rect"])
        res["outcome"] = "window, process alive" if app.alive() else f"window, then exited with status {app.proc.returncode}"
        return res
    finally:
        res["exit_status_on_stop"] = app.stop()
        res["log"] = log_findings(app.log)
        shutil.rmtree(app.data, ignore_errors=True)
        try:
            apk.unlink()
        except OSError:
            pass


def click(x: int, y: int) -> None:
    # The session's virtual pointer (vpointer.py) if it is there; sway's own
    # cursor commands otherwise.
    cmd_file = pathlib.Path(os.environ.get("XDG_RUNTIME_DIR", "/nonexistent")) / "vpointer.cmd"
    if cmd_file.exists():
        with open(cmd_file, "a") as f:
            f.write(f"click {x} {y}\n")
        time.sleep(0.5)
        return
    run(["swaymsg", "seat", "seat0", "cursor", "set", str(x), str(y)])
    run(["swaymsg", "seat", "seat0", "cursor", "press", "button1"])
    time.sleep(0.08)
    run(["swaymsg", "seat", "seat0", "cursor", "release", "button1"])


def run_probe(probe: pathlib.Path, out: pathlib.Path, scale: float, phases: set[str]) -> dict:
    """The probe application through its phases; every check is recorded with what was seen."""
    res: dict = {"apk": probe.name, "phases": {}}
    shots = out / "shots"
    shots.mkdir(exist_ok=True)
    stage = out / "stage"
    stage.mkdir(exist_ok=True)
    apk = stage / "org.tur.probe.apk"
    shutil.copy(probe, apk)

    def launch(phase: str) -> App:
        return App(apk, f"probe-{phase}", out, extras=[f"--extra-string-key=phase={phase}"], scale=scale)

    # -- basic: clipboard, notification, input, back, resize, accessibility
    monitor_log = out / "logs" / "probe-dbus-monitor.log"
    monitor_log.parent.mkdir(parents=True, exist_ok=True)
    mon_f = open(monitor_log, "w")
    try:
        mon = subprocess.Popen(["dbus-monitor", "--session"], stdout=mon_f, stderr=subprocess.DEVNULL)
    except FileNotFoundError:
        mon = None
    basic: dict = {}
    app = launch("basic")
    try:
        t = app.wait_window(120)
        basic["window_seconds"] = round(t, 2) if t is not None else None
        if t is None:
            basic["outcome"] = "no window"
        else:
            time.sleep(4 * scale)
            r = run(["wl-paste", "-n"], timeout=5)
            basic["clipboard_android_to_host"] = {"host_clipboard": r.stdout[:100], "ok": r.stdout.strip() == "tur-probe-clip-from-android"}
            subprocess.Popen(["wl-copy", "tur-host-clip-123"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            w = app.find_window()
            screenshot(shots / "probe-basic-1.png", w["rect"] if w else None)
            time.sleep(12 * scale)
            lines = app.probe_lines()
            got = [x for x in lines if x.startswith("clipboard_get=")]
            basic["clipboard_host_to_android"] = {"seen_by_app": got, "ok": any("tur-host-clip-123" in x for x in got)}
            basic["notify_logged"] = [x for x in lines if x.startswith("notify")]
            # Pointer and keyboard: click down the window in steps, typing a
            # letter after each click; the tap button and the edit box are
            # somewhere under the title.
            if w:
                rect = w["rect"]
                for dy in range(70, 330, 26):
                    click(rect["x"] + rect["width"] // 2, rect["y"] + dy)
                    time.sleep(0.3)
                    run(["wtype", "q"], timeout=5)
                    time.sleep(0.3)
                time.sleep(1.0 * scale)
                lines = app.probe_lines()
                basic["pointer"] = {"taps": [x for x in lines if x.startswith("tap=")][-1:],
                                    "touches": len([x for x in lines if x.startswith("touch=")])}
                basic["keyboard"] = {"text": [x for x in lines if x.startswith("text=")][-1:],
                                     "key_down": [x for x in lines if x.startswith("key_down=")][:5]}
                # Back: the Escape key, then the button ATL puts in the title bar.
                before = len([x for x in app.probe_lines() if x.startswith("back_pressed=")])
                run(["wtype", "-k", "Escape"], timeout=5)
                time.sleep(1.0 * scale)
                after_key = len([x for x in app.probe_lines() if x.startswith("back_pressed=")])
                click(rect["x"] + 26, rect["y"] + 24)
                time.sleep(1.0 * scale)
                after_button = len([x for x in app.probe_lines() if x.startswith("back_pressed=")])
                basic["back"] = {"escape_key": after_key > before, "title_bar_button": after_button > after_key}
                screenshot(shots / "probe-basic-2.png", rect)
                screenshot(shots / "probe-basic-full.png")
                # Resize.
                n_layout = len([x for x in app.probe_lines() if x.startswith("layout=")])
                run(["swaymsg", f"[pid={w['pid']}]", "resize", "set", "width", "760", "px", "height", "620", "px"])
                time.sleep(2.5 * scale)
                layouts = [x for x in app.probe_lines() if x.startswith("layout=")]
                w2 = app.find_window()
                basic["resize"] = {"window": f"{w2['rect']['width']}x{w2['rect']['height']}" if w2 else None,
                                   "layouts_seen_by_app": layouts, "app_followed": len(layouts) > n_layout}
                if w2:
                    screenshot(shots / "probe-basic-3.png", w2["rect"])
            pids = app.pids()
            basic["memory"] = memory_kib(pids)
            acc = a11y(pids)
            words = " ".join((acc.get("names") or []) + (acc.get("texts") or []))
            basic["a11y"] = {"found": acc.get("found"), "nodes": acc.get("nodes"), "roles": acc.get("roles"),
                             "actions": acc.get("actions"), "names": (acc.get("names") or [])[:40],
                             "texts": (acc.get("texts") or [])[:40],
                             "application_text_visible": "tur-probe" in words,
                             "applications_on_bus": acc.get("applications_on_bus"), "error": acc.get("error")}
            basic["alive"] = app.alive()
            basic["outcome"] = "ran"
        basic["probe_lines"] = app.probe_lines()
    finally:
        app.stop()
        basic["log"] = log_findings(app.log)
    if mon:
        time.sleep(0.5)
        mon.terminate()
    mon_f.close()
    try:
        mtxt = monitor_log.read_text(errors="replace")
    except OSError:
        mtxt = ""
    basic["notification_on_bus"] = {
        "portal_AddNotification": "AddNotification" in mtxt,
        "fdo_Notify": bool(re.search(r"interface=org\.freedesktop\.Notifications; member=Notify", mtxt)),
        "title_seen": "tur-probe-notification" in mtxt,
    }
    res["phases"]["basic"] = basic

    # -- the phases that ask the system for something
    for phase, wait in (("open", 8), ("save", 8), ("view", 6), ("share", 6), ("webview", 8)):
        if phase not in phases:
            continue
        info: dict = {}
        mon_log = out / "logs" / f"probe-{phase}-dbus-monitor.log"
        mf = open(mon_log, "w")
        try:
            mon = subprocess.Popen(["dbus-monitor", "--session"], stdout=mf, stderr=subprocess.DEVNULL)
        except FileNotFoundError:
            mon = None
        app = launch(phase)
        try:
            t = app.wait_window(120)
            info["window_seconds"] = round(t, 2) if t is not None else None
            time.sleep(wait * scale)
            info["alive"] = app.alive()
            info["windows_on_screen"] = [{"app_id": x["app_id"], "title": x["name"]} for x in windows()]
            screenshot(shots / f"probe-{phase}.png")
            info["probe_lines"] = [x for x in app.probe_lines()
                                   if x.split("=")[0] not in ("sdk_int", "release", "device", "supported_abis", "os_arch", "vm", "display",
                                                              "files_dir", "native_library_dir", "external_files_dir", "google_play_services")]
        finally:
            app.stop()
            info["log"] = log_findings(app.log)
            if mon:
                mon.terminate()
            mf.close()
        try:
            mtxt = mon_log.read_text(errors="replace")
        except OSError:
            mtxt = ""
        info["portal_calls"] = sorted(set(re.findall(r"interface=(org\.freedesktop\.portal\.\w+); member=(\w+)", mtxt)))[:20]
        info["portal_calls"] = [f"{a}.{b}" for a, b in info["portal_calls"]]
        res["phases"][phase] = info

    # -- bench
    if "bench" not in phases:
        return res
    bench: dict = {}
    app = launch("bench")
    try:
        t = app.wait_window(120)
        bench["window_seconds"] = round(t, 2) if t is not None else None
        deadline = time.monotonic() + 240 * max(1.0, scale / 2)
        while time.monotonic() < deadline and app.alive():
            if any(x.startswith("bench_done") for x in app.probe_lines()):
                break
            time.sleep(2)
        bench["probe_lines"] = app.probe_lines()
        bench["alive"] = app.alive()
        bench["memory"] = memory_kib(app.pids())
    finally:
        app.stop()
        bench["log"] = log_findings(app.log)
    res["phases"]["bench"] = bench
    return res


def md_table(header: list[str], rows: list[list]) -> str:
    return fdroid_index.table(header, rows)


def write_summary(out: pathlib.Path, results: list[dict], probe: dict | None, meta: dict) -> None:
    md = [f"## Android Translation Layer: {meta.get('title', 'run')}\n"]
    md.append(f"ATL package: `{meta.get('atl_version', '?')}`; machine architecture `{meta.get('machine')}`; "
              f"time limits scaled by {meta.get('scale')}.\n")
    if results:
        n = len(results)
        win = sum(1 for r in results if r.get("window_seconds") is not None)
        alive = sum(1 for r in results if r.get("outcome") == "window, process alive")
        md.append(f"{n} applications: {win} opened a window, {alive} were still running at the screenshot. "
                  "Whether the window shows a usable first screen is judged from the screenshots, not here.\n")
        rows = []
        for r in results:
            mem = r.get("memory", {}).get("pss_kib")
            first = (r.get("log", {}).get("first_lines") or [""])[0][:110].replace("|", "/")
            rows.append([r["package"], r["group"], r["version_name"], r["nativecode"], r.get("outcome", "?"),
                         r.get("window_seconds", "–"), r.get("stable_seconds", "–"),
                         f"{mem // 1024} MiB" if mem else "–", r.get("colours", "–"),
                         (r.get("a11y") or {}).get("nodes", "–"), (r.get("resize") or {}).get("alive", "–"),
                         f"`{first}`" if first else ""])
        md.append(md_table(["Package", "Kind", "Version", "Native ABIs", "Outcome", "Window (s)", "Picture still (s)",
                            "Memory (PSS)", "Colours", "A11y nodes", "Alive after resize", "First error logged"], rows))
    if probe:
        md.append("### Probe application\n")
        md.append("```json\n" + json.dumps(probe, indent=1)[:60000] + "\n```\n")
    (out / "results.md").write_text("\n".join(md))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--apks", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--lock", action="append", default=[])
    ap.add_argument("--probe")
    ap.add_argument("--probe-phases", default="open,save,view,share,webview,bench")
    ap.add_argument("--scale", type=float, default=1.0)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--title", default="run")
    args = ap.parse_args()
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    meta = {"title": args.title, "scale": args.scale, "machine": os.uname().machine,
            "atl_version": run(["apk", "info", "-v", "android-translation-layer"]).stdout.strip().splitlines()[0:1]}
    results: list[dict] = []
    probe_res = None

    def save() -> None:
        (out / "results.json").write_text(json.dumps({"meta": meta, "apps": results, "probe": probe_res}, indent=1) + "\n")
        write_summary(out, results, probe_res, meta)

    # The probe first: it is small, and its facts are wanted even if the
    # sample later runs out of time.
    if args.probe and pathlib.Path(args.probe).exists():
        try:
            probe_res = run_probe(pathlib.Path(args.probe), out, args.scale, set(args.probe_phases.split(",")))
        except Exception as e:  # noqa: BLE001 - a harness defect is recorded, the sample still runs
            probe_res = {"harness_error": repr(e)}
        save()
    rows = []
    for lock in args.lock:
        if pathlib.Path(lock).exists():
            rows += fdroid_index.read_lock(lock)
    if args.limit:
        rows = rows[: args.limit]
    for i, row in enumerate(rows, 1):
        print(f"[{i}/{len(rows)}] {row['package']} {row['version_name']}", flush=True)
        try:
            res = run_sample_app(row, pathlib.Path(args.apks), out, args.scale)
        except Exception as e:  # noqa: BLE001
            res = {"package": row["package"], "group": row["group"], "version_name": row["version_name"],
                   "nativecode": row["nativecode"], "outcome": f"harness error: {e!r}"}
        print(f"    {res.get('outcome')} window={res.get('window_seconds')} stable={res.get('stable_seconds')}", flush=True)
        results.append(res)
        save()
    save()
    return 0


if __name__ == "__main__":
    sys.exit(main())
