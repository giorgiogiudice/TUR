#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""P@x's guest agent on real Windows, as an experiment
(docs/reports/exp-inwin-1.md, section 3): how much of real applications a
program inside Windows can see and operate.

Each window is read four ways and scored against the controls a person
sees:
  uia            UI Automation, natively (uia.py)
  bridge-win32   the plain window tree       } tur-winbridge.exe of the
  bridge-msaa    MSAA                        } winsee experiment (pull
  bridge-uia     its UI Automation tier      } request 126), unmodified,
                                               built for real Windows
Then actions are sent and each is verified by a route that does not ask
the tier that acted: the application's own log, a control message, the
window's title, or the picture of the screen.

    seer.py --out DIR --raw DIR --bridge DIR --downloads DIR

--out   counts and tables (kept)
--raw   reads and pictures (pictures of Microsoft's applications go to a
        folder that the workflow keeps for one day only)
"""

import argparse
import collections
import ctypes
import json
import os
import pathlib
import queue
import re
import subprocess
import threading
import time
import traceback
import zipfile

import uia
import win

HERE = pathlib.Path(__file__).resolve().parent
ROLE_GROUPS = {
    "push button": "button", "toggle button": "button", "check box": "check", "radio button": "radio", "text": "text",
    "entry": "text", "password text": "text", "combo box": "combo", "page tab": "tab", "list item": "item",
    "table cell": "item", "tree item": "tree item", "menu": "menu", "menu item": "menu", "check menu item": "menu",
    "radio menu item": "menu", "link": "link", "table column header": "header", "label": "label", "static": "label",
    "slider": "slider", "spin button": "spin",
}
INTERACTIVE = {"button", "check", "radio", "text", "combo", "tab", "item", "tree item", "menu", "link", "header", "slider", "spin"}
COLOURS = {"uia": (0, 170, 60), "bridge-win32": (220, 40, 40), "bridge-msaa": (230, 150, 0), "bridge-uia": (40, 90, 230)}
# tw-controls.exe of the winsee experiment: the truth is its source (winsee.py, CONTROLS_TRUTH).
CONTROLS_TRUTH = {"named": (
    [[n, "menu"] for n in ("File", "Options", "Help")] + [[n, "button"] for n in ("Back", "Forward", "Refresh", "Preview", "Save draft", "Help", "Close")]
    + [[n, "text"] for n in ("Name", "Password")] + [["Remember me", "check"]] + [[n, "radio"] for n in ("Small", "Medium", "Large")]
    + [["Colour", "combo"]] + [[n, "item"] for n in ("Apple", "Banana", "Cherry", "Damson", "report.txt", "photo.png", "notes.md")]
    + [[n, "header"] for n in ("File", "Size")] + [[n, "tree item"] for n in ("Settings", "Display", "Sound")] + [[n, "tab"] for n in ("General", "Advanced")])}
FORM_TRUTH = {"named": [["Name", "text"], ["Subscribe", "check"], ["Small", "radio"], ["Large", "radio"], ["Colour", "combo"], ["Send", "button"], ["TUR project", "link"]]}
FILES_TRUTH = {"named": [["alpha.txt", "item"], ["beta.txt", "item"], ["gamma.txt", "item"], ["inner", "item"]]}


def norm(name):
    text = (name or "").replace("&", "").replace("\u2026", "...")
    text = re.sub(r"\s+", " ", text).strip().lower()
    return text.rstrip(":. ").strip()


def group(role):
    return ROLE_GROUPS.get(role or "", role or "")


def visible(read, box=None):
    shown = []
    for element in read.get("elements", []):
        rect = element.get("screen")
        if not rect or "offscreen" in element.get("states", []):
            continue
        if box and (rect[0] + rect[2] <= box[0] or rect[1] + rect[3] <= box[1] or rect[0] >= box[0] + box[2] or rect[1] >= box[1] + box[3]):
            continue
        shown.append(element)
    return shown


def find(read, role=None, name=None, contains=None):
    roles = None if role is None else ({role} if isinstance(role, str) else set(role))
    for element in read.get("elements", []):
        if roles is not None and group(element.get("role")) not in roles and element.get("role") not in roles:
            continue
        if name is not None and norm(element.get("name")) != norm(name):
            continue
        if contains is not None and contains.lower() not in (element.get("name") or "").lower():
            continue
        return element
    return None


def score(truth, read, box=None):
    """winsee's scoring (harness/winsee.py): of the controls a person sees,
    how many a tier found by name, and by name and kind."""
    named = [{"name": name, "group": kind} for name, kind in truth.get("named", [])]
    unnamed = truth.get("unnamed", {})
    shown = [e for e in visible(read, box) if e.get("depth", 0) > 0]
    by_name = by_kind = 0
    missing = []
    used = set()
    for entry in named:
        candidates = [e for e in shown if norm(e.get("name")) == norm(entry["name"])]
        of_kind = [e for e in candidates if group(e.get("role")) == entry["group"] and e["id"] not in used]
        by_name += bool(candidates)
        if of_kind:
            by_kind += 1
            used.add(of_kind[0]["id"])
        else:
            missing.append(f"{entry['name']} ({entry['group']})")
    for kind, count in unnamed.items():
        available = [e for e in shown if group(e.get("role")) == kind and e["id"] not in used]
        got = min(count, len(available))
        used.update(e["id"] for e in available[:got])
        by_name += got
        by_kind += got
        if got < count:
            missing.append(f"{count - got} of {count} without a name ({kind})")
    return {"truth": len(named) + sum(unnamed.values()), "found": by_name, "found_with_kind": by_kind, "missing": missing}


def summary(read, box):
    shown = visible(read, box)
    controls = [e for e in shown if group(e.get("role")) in INTERACTIVE]
    return {
        "elements": len(read.get("elements", [])), "visible": len(shown), "controls": len(controls),
        "named_controls": sum(1 for e in controls if e.get("name")),
        "with_actions": sum(1 for e in shown if e.get("actions")),
        "kinds": dict(collections.Counter(group(e.get("role")) for e in controls)),
        "frameworks": dict(collections.Counter(e.get("framework", "") for e in shown if e.get("framework"))),
        "milliseconds": read.get("milliseconds"), "error": read.get("error"),
    }


class Bridge:
    """tur-winbridge.exe: one JSON object a line (winsee's harness/bridge.py, for real Windows)."""

    def __init__(self, program, log):
        self.process = subprocess.Popen([str(program)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=open(log, "ab"))
        self.lines = queue.Queue()
        self.next = 1
        threading.Thread(target=self._pump, daemon=True).start()
        self.hello = self.ask("hello", 30)

    def _pump(self):
        for raw in self.process.stdout:
            self.lines.put(raw)
        self.lines.put(None)

    def ask(self, op, timeout=60, **fields):
        request = {"id": self.next, "op": op}
        request.update({key: value for key, value in fields.items() if value is not None})
        self.next += 1
        self.process.stdin.write(json.dumps(request).encode("ascii") + b"\n")
        self.process.stdin.flush()
        deadline = time.monotonic() + timeout
        while True:
            raw = self.lines.get(timeout=max(deadline - time.monotonic(), 0.1))
            if raw is None:
                raise RuntimeError("the bridge ended")
            text = raw.decode("utf-8", "replace").strip()
            if not text.startswith("{"):
                continue
            reply = json.loads(text)
            if reply.get("id") != request["id"]:
                continue
            if not reply.get("ok"):
                raise RuntimeError(json.dumps(reply.get("error"))[:300])
            return reply.get("result", {})

    def window_id(self, hwnd):
        for window in self.ask("windows").get("windows", []):
            try:
                if int(window["window"].lstrip("h"), 16) == hwnd:
                    return window["window"]
            except ValueError:
                continue
        return f"h{hwnd:x}"


class Seer:
    def __init__(self, arguments):
        self.out = pathlib.Path(arguments.out)
        self.raw = pathlib.Path(arguments.raw)
        self.downloads = pathlib.Path(arguments.downloads)
        self.build = pathlib.Path(arguments.bridge)
        for directory in (self.out, self.raw / "open", self.raw / "microsoft"):
            directory.mkdir(parents=True, exist_ok=True)
        self.auto = uia.Automation()
        self.bridge = None
        self.notes = []
        self.captures = []
        self.checks = []
        self.truth = json.loads((HERE / "truth-winsee.json").read_text(encoding="utf-8"))
        self.data = pathlib.Path(os.environ.get("RUNNER_TEMP", os.environ.get("TEMP", "."))) / "inwin-data"
        (self.data / "inner").mkdir(parents=True, exist_ok=True)
        for name in ("alpha.txt", "beta.txt", "gamma.txt"):
            (self.data / name).write_text(f"{name}: a file of the inwin experiment\n", encoding="ascii")
        program = self.build / "tur-winbridge.exe"
        if program.exists():
            try:
                self.bridge = Bridge(program, self.out / "bridge.log")
                self.note(f"bridge: {json.dumps(self.bridge.hello)[:300]}")
            except Exception as error:
                self.note(f"the bridge did not start: {error}")
        else:
            self.note("tur-winbridge.exe was not built: its tiers are NOT RUN")

    def note(self, text):
        print(text, flush=True)
        self.notes.append(text)

    def check(self, app, what, ok, detail=""):
        self.checks.append({"app": app, "what": what, "ok": bool(ok), "detail": str(detail)[:300]})
        print(f"  [{'ok' if ok else 'FAILED'}] {app}: {what} {str(detail)[:160]}", flush=True)

    def read_all(self, hwnd):
        reads = {}
        try:
            reads["uia"] = self.auto.read(hwnd)
        except Exception as error:
            reads["uia"] = {"elements": [], "error": f"{type(error).__name__}: {str(error)[:200]}"}
        if self.bridge is not None:
            try:
                identifier = self.bridge.window_id(hwnd)
            except Exception as error:
                identifier = None
                self.note(f"bridge windows: {error}")
            for tier in ("win32", "msaa", "uia"):
                began = time.perf_counter()
                try:
                    read = self.bridge.ask("read", 90, window=identifier, tier=None if tier == "win32" else tier, max_nodes=4000)
                    read.setdefault("milliseconds", round((time.perf_counter() - began) * 1000, 1))
                except Exception as error:
                    read = {"elements": [], "error": f"{type(error).__name__}: {str(error)[:200]}"}
                reads["bridge-" + tier] = read
        return reads

    def capture(self, app, label, hwnd, truth=None, microsoft=False):
        """Read a window every way, draw what each found, count and score."""
        directory = self.raw / ("microsoft" if microsoft else "open") / app
        directory.mkdir(parents=True, exist_ok=True)
        win.SetForegroundWindow(hwnd)
        time.sleep(0.7)
        box = win.frame_of(hwnd) or win.rect_of(hwnd)
        plain = win.grab(box)
        win.save_png(directory / f"{label}.png", plain)
        reads = self.read_all(hwnd)
        row = {"app": app, "label": label, "microsoft": microsoft, "box": box, "title_length": len(win.text_of(hwnd)), "tiers": {}}
        for tier, read in reads.items():
            picture = (plain[0], plain[1], plain[2], plain[3], bytearray(plain[4]))
            for element in visible(read, box):
                if group(element.get("role")) in INTERACTIVE:
                    win.outline(picture, element["screen"], COLOURS[tier])
            win.save_png(directory / f"{label}-{tier}.png", picture)
            (directory / f"{label}-{tier}.json").write_text(json.dumps(read, indent=1), encoding="utf-8")
            row["tiers"][tier] = summary(read, box)
            if truth is not None:
                row["tiers"][tier]["score"] = score(truth, read, box)
        if truth is not None and "bridge-win32" in reads:
            both = {"elements": [dict(e, id="u" + e["id"]) for e in reads["uia"].get("elements", [])] + [dict(e, id="w" + str(e.get("id"))) for e in reads["bridge-win32"].get("elements", [])]}
            row["tiers"]["uia or win32"] = {"score": score(truth, both, box)}
        self.captures.append(row)
        line = "; ".join(f"{tier} {s.get('controls', '-')} controls" + (f" ({s['score']['found_with_kind']}/{s['score']['truth']})" if "score" in s else "") for tier, s in row["tiers"].items())
        self.note(f"{app}/{label}: {line}")
        return reads

    def launch(self, command, **options):
        return subprocess.Popen([str(part) for part in command], **options)

    def close(self, hwnd, process=None):
        if hwnd and win.IsWindow(hwnd):
            win.PostMessageW(hwnd, win.WM_CLOSE, 0, 0)
        time.sleep(1.5)
        if process is not None and process.poll() is None:
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()

    def checked(self, hwnd):
        """A button's check state, asked of the control itself (BM_GETCHECK)."""
        result = ctypes.c_size_t(0)
        win.SendMessageTimeoutW(hwnd, 0xF0, 0, None, win.SMTO_ABORTIFHUNG, 2000, ctypes.byref(result))
        return result.value == 1

    def act(self, app, what, action, verify, wait=1.0):
        """One action and its independent verification."""
        try:
            how = action()
        except Exception as error:
            self.check(app, what, False, f"the action failed: {type(error).__name__}: {str(error)[:160]}")
            return False
        time.sleep(wait)
        try:
            ok, detail = verify()
        except Exception as error:
            ok, detail = False, f"verification failed: {type(error).__name__}: {str(error)[:160]}"
        self.check(app, what, ok, f"{how}; {detail}")
        return ok

    # ------------------------------------------------------------ scenarios

    def scenario_controls(self):
        app = "tw-controls"
        program = self.build / "tw-controls.exe"
        if not program.exists():
            self.note("tw-controls.exe was not built: NOT RUN")
            return
        log = self.data / "tw-controls.log"
        process = self.launch([program, log])
        window = win.wait_window(lambda w: w["exe"] == "tw-controls.exe", 30)
        if window is None:
            self.note("tw-controls.exe showed no window")
            return
        hwnd = window["hwnd"]
        reads = self.capture(app, "tw-controls", hwnd, CONTROLS_TRUTH)
        read = reads["uia"]

        def logged():
            return log.read_text(encoding="utf-8", errors="replace").splitlines() if log.exists() else []

        def grows(before):
            return lambda: (len(logged()) > before, f"the application logged {logged()[before:][:3]}")

        for name, kind, do in (("Save draft", "button", self.auto.press), ("Remember me", "check", self.auto.toggle), ("Large", "radio", self.auto.select), ("Advanced", "tab", self.auto.select)):
            element = find(read, kind, name)
            if element is None:
                self.check(app, f"UI Automation: {name}", False, "not found")
                continue
            before = len(logged())
            verify = grows(before)
            if kind in ("check", "radio") and element.get("hwnd"):
                verify = (lambda h=element["hwnd"]: (self.checked(h), "BM_GETCHECK says it is set"))
            self.act(app, f"UI Automation: {kind} '{name}'", lambda e=element, d=do: d(e["id"]), verify)
        element = find(read, "text", "Name")
        if element is not None and element.get("hwnd"):
            self.act(app, "UI Automation: type into 'Name'", lambda: self.auto.set_value(element["id"], "Ada Lovelace"),
                     lambda: (win.remote_text(element["hwnd"]) == "Ada Lovelace", "WM_GETTEXT returns the text"))
        if self.bridge is not None:
            button = find(reads.get("bridge-win32", {}), "button", "Refresh")
            if button is not None:
                before = len(logged())
                self.act(app, "bridge, Win32 tier: toolbar button 'Refresh'", lambda: json.dumps(self.bridge.ask("act", element=button["id"], action="press"))[:80], grows(before))
        self.close(hwnd, process)

    def scenario_notepad(self):
        app = "notepad"
        process = self.launch(["notepad.exe"])
        window = win.wait_window(lambda w: w["exe"] == "notepad.exe" and w["title"], 40)
        if window is None:
            self.note("Notepad showed no window: NOT RUN")
            return
        hwnd = window["hwnd"]
        reads = self.capture(app, "notepad", hwnd, microsoft=True)
        read = reads["uia"]
        self.note(f"notepad: window class {window['class']}; child classes {sorted({win.class_of(c) for c in win.children(hwnd)})[:12]}")
        editor = find(read, "text")
        text = "TUR inwin 12345"
        if editor is None:
            self.check(app, "UI Automation: type into the editor", False, "no text element")
        else:
            def typed():
                for child in win.children(hwnd):
                    if win.class_of(child) in ("Edit", "RichEditD2DPT", "RICHEDIT50W") and text in win.remote_text(child):
                        return True, f"WM_GETTEXT of the {win.class_of(child)} control returns the text"
                again = self.auto.read(hwnd)
                element = find(again, "text")
                return (element is not None and text in (self.auto.value(element["id"]) or ""), "no control answered WM_GETTEXT; UI Automation reads the text back")
            self.act(app, "UI Automation: type into the editor", lambda: self.auto.set_value(editor["id"], text), typed)
        menu = find(read, "menu", "File")
        if menu is None:
            self.check(app, "UI Automation: open the File menu", False, "no File menu")
        else:
            before = win.grab(win.frame_of(hwnd) or win.rect_of(hwnd))

            def opened():
                popups = [w for w in win.top_windows() if w["class"] == "#32768"]
                after = win.grab(win.frame_of(hwnd) or win.rect_of(hwnd))
                return (bool(popups) or win.differing(before, after) > 0.01, f"{len(popups)} menu window(s); {win.differing(before, after):.1%} of the picture changed")
            if self.act(app, "UI Automation: open the File menu", lambda: self.auto.expand(menu["id"]), opened):
                popup = next((w for w in win.top_windows() if w["class"] == "#32768"), None)
                self.capture(app, "notepad-file-menu", popup["hwnd"] if popup else hwnd, microsoft=True)
            try:
                self.auto.collapse(menu["id"])
            except Exception:
                pass
        win.PostMessageW(hwnd, win.WM_CLOSE, 0, 0)
        time.sleep(2)
        pressed = False
        for candidate in [w for w in win.top_windows() if w["pid"] == window["pid"]]:
            try:
                prompt = self.auto.read(candidate["hwnd"])
            except Exception:
                continue
            button = find(prompt, "button", "Don't Save") or find(prompt, "button", "Don\u2019t save")
            if button is not None:
                self.act(app, "UI Automation: answer the save prompt with Don't Save", lambda b=button: self.auto.press(b["id"]),
                         lambda: (not win.IsWindow(hwnd), "the window is gone"), wait=2)
                pressed = True
                break
        if not pressed:
            self.check(app, "UI Automation: answer the save prompt with Don't Save", False, "no prompt with that button was found")
        if process.poll() is None:
            process.kill()

    def scenario_explorer(self):
        app = "explorer"
        self.launch(["explorer.exe", self.data])
        window = win.wait_window(lambda w: w["class"] == "CabinetWClass" and "inwin-data" in w["title"], 40)
        if window is None:
            self.note("Explorer opened no folder window: NOT RUN")
            return
        hwnd = window["hwnd"]
        time.sleep(2)
        reads = self.capture(app, "explorer", hwnd, FILES_TRUTH, microsoft=True)
        read = reads["uia"]
        item = find(read, "item", "beta.txt")
        if item is not None:
            self.act(app, "UI Automation: select beta.txt", lambda: self.auto.select(item["id"]), lambda: (self.auto.selected(item["id"]), "the item reports itself selected (same tier: weak)"))
        folder = find(read, "item", "inner")
        if folder is not None:
            self.act(app, "UI Automation: open the folder 'inner'", lambda: self.auto.press(folder["id"]),
                     lambda: ("inner" in win.text_of(hwnd) or any(w["class"] == "CabinetWClass" and w["title"].startswith("inner") for w in win.top_windows()), f"the window's title is now '{win.text_of(hwnd)}'"), wait=2.5)
        for other in [w for w in win.top_windows() if w["class"] == "CabinetWClass"]:
            self.close(other["hwnd"])

    def scenario_settings(self):
        app = "settings"
        self.launch(["cmd.exe", "/c", "start", "ms-settings:"])
        window = win.wait_window(lambda w: w["class"] == "ApplicationFrameWindow" and "settings" in w["title"].lower(), 60)
        if window is None:
            self.note("Settings showed no window: NOT RUN")
            return
        hwnd = window["hwnd"]
        time.sleep(4)
        reads = self.capture(app, "settings", hwnd, microsoft=True)
        read = reads["uia"]
        target = None
        for name in ("Apps", "Personalization", "Accounts", "Time & language", "Network & internet"):
            target = find(read, "item", name) or find(read, None, name)
            if target is not None and (target.get("actions")):
                break
        if target is None:
            self.check(app, "UI Automation: open another page", False, "no navigation item found")
        else:
            before = win.grab(win.frame_of(hwnd) or win.rect_of(hwnd))
            names_before = {e["name"] for e in visible(read) if e.get("name")}

            def changed():
                after = win.grab(win.frame_of(hwnd) or win.rect_of(hwnd))
                again = self.auto.read(hwnd)
                new = {e["name"] for e in visible(again) if e.get("name")} - names_before
                return (win.differing(before, after) > 0.03, f"{win.differing(before, after):.1%} of the picture changed; {len(new)} new names on the page")

            def go():
                return self.auto.select(target["id"]) if "select" in target["actions"] else self.auto.press(target["id"])
            self.act(app, f"UI Automation: open the page '{target['name']}'", go, changed, wait=3)
            self.capture(app, "settings-page", hwnd, microsoft=True)
        self.close(hwnd)

    def scenario_browsers(self):
        page = (HERE / "form.html").resolve().as_uri()
        profiles = self.data / "profiles"
        for app, program, arguments, microsoft in (
            ("edge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", ["--user-data-dir={p}", "--no-first-run", "--no-default-browser-check", "--new-window", page], True),
            ("chrome", r"C:\Program Files\Google\Chrome\Application\chrome.exe", ["--user-data-dir={p}", "--no-first-run", "--no-default-browser-check", "--new-window", page], False),
            ("firefox", r"C:\Program Files\Mozilla Firefox\firefox.exe", ["-profile", "{p}", "-no-remote", "-new-instance", page], False),
        ):
            if not os.path.exists(program):
                self.note(f"{app}: not on this runner: NOT RUN")
                continue
            try:
                profile = profiles / app
                profile.mkdir(parents=True, exist_ok=True)
                process = self.launch([program] + [a.replace("{p}", str(profile)) for a in arguments])
                window = win.wait_window(lambda w: "tur inwin form" in w["title"].lower(), 60)
                if window is None:
                    self.note(f"{app}: the page's window did not appear: NOT RUN")
                    process.kill()
                    continue
                hwnd = window["hwnd"]
                time.sleep(3)
                first = score(FORM_TRUTH, self.auto.read(hwnd))
                time.sleep(2)
                reads = self.capture(app, f"{app}-form", hwnd, FORM_TRUTH, microsoft=microsoft)
                read = reads["uia"]
                self.note(f"{app}: the first UI Automation read found {first['found_with_kind']} of {first['truth']} page controls; the second {score(FORM_TRUTH, read)['found_with_kind']}")
                field = find(read, "text", "Name")
                button = find(read, "button", "Send")
                if field is None or button is None:
                    self.check(app, "UI Automation: fill the page's form and send it", False, "the page's controls were not found")
                else:
                    def fill():
                        self.auto.set_value(field["id"], "Ada")
                        time.sleep(0.5)
                        return self.auto.press(button["id"])
                    self.act(app, "UI Automation: fill the page's form and send it", fill, lambda: ("sent:Ada" in win.text_of(hwnd), f"the window's title is now '{win.text_of(hwnd)[:40]}'"), wait=2)
                self.close(hwnd, process)
                subprocess.run(["taskkill.exe", "/f", "/t", "/pid", str(process.pid)], capture_output=True)
            except Exception:
                self.note(f"{app}: {traceback.format_exc()[-600:]}")

    def scenario_ribbon(self):
        app = "wordpad"
        program = r"C:\Program Files\Windows NT\Accessories\wordpad.exe"
        if not os.path.exists(program):
            self.note("WordPad is not part of this Windows: its ribbon is NOT RUN here")
            return
        process = self.launch([program])
        window = win.wait_window(lambda w: w["exe"] == "wordpad.exe" and w["title"], 40)
        if window is None:
            self.note("WordPad showed no window: NOT RUN")
            return
        hwnd = window["hwnd"]
        time.sleep(2)
        reads = self.capture(app, "wordpad", hwnd, microsoft=True)
        read = reads["uia"]
        tab = find(read, "tab", "View")
        if tab is None:
            self.check(app, "UI Automation: switch the ribbon to View", False, "no View tab")
        else:
            before = win.grab(win.frame_of(hwnd) or win.rect_of(hwnd))

            def switched():
                after = win.grab(win.frame_of(hwnd) or win.rect_of(hwnd))
                return (win.differing(before, after) > 0.01, f"{win.differing(before, after):.1%} of the picture changed")
            self.act(app, "UI Automation: switch the ribbon to View", lambda: self.auto.select(tab["id"]), switched, wait=1.5)
            self.capture(app, "wordpad-view", hwnd, microsoft=True)
        document = find(read, "text")
        if document is not None and "type" in document.get("actions", []):
            self.act(app, "UI Automation: type into the document", lambda: self.auto.set_value(document["id"], "TUR ribbon"), lambda: (True, "not verified independently"))
        self.close(hwnd, process)
        if process.poll() is None:
            process.kill()

    def scenario_putty(self):
        app = "putty"
        program = self.downloads / "putty.exe"
        if not program.exists():
            self.note("putty.exe was not downloaded: NOT RUN")
            return
        process = self.launch([program])
        window = win.wait_window(lambda w: w["exe"] == "putty.exe", 40)
        if window is None:
            self.note("PuTTY showed no window")
            return
        hwnd = window["hwnd"]
        time.sleep(1)
        reads = self.capture(app, "putty", hwnd, self.truth["putty"])
        read = reads["uia"]
        radio = find(read, "radio", "Serial")
        if radio is not None and radio.get("hwnd"):
            self.act(app, "UI Automation: choose the radio button 'Serial'", lambda: self.auto.select(radio["id"]) if "select" in radio["actions"] else self.auto.press(radio["id"]),
                     lambda: (self.checked(radio["hwnd"]), "BM_GETCHECK says it is set"))
        else:
            self.check(app, "UI Automation: choose the radio button 'Serial'", False, "not found with a window handle")
        again = self.auto.read(hwnd)
        field = find(again, "text")
        if field is not None and field.get("hwnd"):
            self.act(app, "UI Automation: type into the first field", lambda: self.auto.set_value(field["id"], "COM7"), lambda: (win.remote_text(field["hwnd"]) == "COM7", "WM_GETTEXT returns the text"))
        item = find(again, "tree item", "Colours")
        if item is not None:
            self.act(app, "UI Automation: select the tree row 'Colours'", lambda: self.auto.select(item["id"]),
                     lambda: (find(self.auto.read(hwnd), None, None, "ANSI") is not None or find(self.auto.read(hwnd), None, None, "colour") is not None, "the panel now shows colour options"), wait=1.5)
        self.close(hwnd, process)
        if process.poll() is None:
            process.kill()

    def scenario_notepadpp(self):
        app = "notepadpp"
        archive = next(iter(sorted(self.downloads.glob("npp.*.portable.x64.zip"))), None)
        if archive is None:
            self.note("the Notepad++ archive was not downloaded: NOT RUN")
            return
        target = self.data / "npp"
        if not (target / "notepad++.exe").exists():
            with zipfile.ZipFile(archive) as bundle:
                bundle.extractall(target)
        process = self.launch([target / "notepad++.exe", "-nosession", "-noPlugin", "-multiInst", self.data / "alpha.txt"])
        window = win.wait_window(lambda w: w["exe"] == "notepad++.exe" and w["class"] == "Notepad++", 60)
        if window is None:
            self.note("Notepad++ showed no window")
            return
        hwnd = window["hwnd"]
        time.sleep(2)
        reads = self.capture(app, "notepadpp", hwnd, self.truth["notepadpp"])
        menu = find(reads["uia"], "menu", "Settings")
        if menu is not None:
            try:
                self.auto.expand(menu["id"])
                time.sleep(1)
                popup = next((w for w in win.top_windows() if w["class"] == "#32768"), None)
                entry = find(self.auto.read(popup["hwnd"]), "menu", None, "Preferences") if popup else None
                if entry is None:
                    self.check(app, "UI Automation: Settings, Preferences", False, "the menu entry was not found")
                else:
                    self.act(app, "UI Automation: Settings, Preferences", lambda: self.auto.press(entry["id"]),
                             lambda: (win.wait_window(lambda w: w["exe"] == "notepad++.exe" and "preferences" in w["title"].lower(), 10) is not None, "the Preferences window opened"), wait=1)
                    dialog = win.wait_window(lambda w: w["exe"] == "notepad++.exe" and "preferences" in w["title"].lower(), 5)
                    if dialog is not None:
                        self.capture(app, "notepadpp-preferences", dialog["hwnd"], self.truth["notepadpp-preferences"])
                        self.close(dialog["hwnd"])
            except Exception:
                self.note(f"notepadpp menu: {traceback.format_exc()[-400:]}")
        self.close(hwnd, process)
        if process.poll() is None:
            process.kill()

    def scenario_sevenzip(self):
        app = "sevenzip"
        installer = next(iter(sorted(self.downloads.glob("7z*-x64.exe"))), None)
        program = pathlib.Path(r"C:\Program Files\7-Zip\7zFM.exe")
        if not program.exists():
            if installer is None:
                self.note("7-Zip was not downloaded: NOT RUN")
                return
            subprocess.run([str(installer), "/S"], timeout=300)
        if not program.exists():
            self.note("7-Zip did not install: NOT RUN")
            return
        files = self.data / "seven"
        files.mkdir(exist_ok=True)
        for name in ("alpha.txt", "beta.txt", "gamma.txt"):
            (files / name).write_text(name, encoding="ascii")
        process = self.launch([program, str(files) + "\\"])
        window = win.wait_window(lambda w: w["exe"] == "7zfm.exe", 40)
        if window is None:
            self.note("7-Zip showed no window")
            return
        hwnd = window["hwnd"]
        time.sleep(1.5)
        reads = self.capture(app, "sevenzip", hwnd, self.truth["sevenzip"])
        item = find(reads["uia"], "item", "beta.txt")
        if item is not None:
            self.act(app, "UI Automation: select beta.txt", lambda: self.auto.select(item["id"]),
                     lambda: (find(self.auto.read(hwnd), None, None, "1 / 3") is not None or find(self.auto.read(hwnd), None, None, "1 object") is not None, "the status bar counts one selected object"))
        self.close(hwnd, process)
        if process.poll() is None:
            process.kill()

    def scenario_installer(self):
        app = "innosetup"
        installer = next(iter(sorted(self.downloads.glob("innosetup-*.exe"))), None)
        if installer is None:
            self.note("the Inno Setup installer was not downloaded: NOT RUN")
            return
        process = self.launch([installer])
        scored = False
        for step in range(6):
            window = win.wait_window(lambda w: (w["exe"].startswith("innosetup") or w["exe"].endswith(".tmp")) and w["title"], 40 if step == 0 else 12)
            if window is None:
                break
            hwnd = window["hwnd"]
            time.sleep(1.5)
            read = self.auto.read(hwnd)
            accept = find(read, "radio", "I accept the agreement")
            if accept is not None:
                reads = self.capture(app, "innosetup-licence", hwnd, self.truth["innosetup-2"])
                read = reads["uia"]
                accept = find(read, "radio", "I accept the agreement")
                following = find(read, "button", None, "Next")
                self.act(app, "UI Automation: accept the licence", lambda: self.auto.select(accept["id"]) if "select" in accept["actions"] else self.auto.press(accept["id"]),
                         lambda: (bool(following and following.get("hwnd") and win.IsWindowEnabled(following["hwnd"])), "IsWindowEnabled says Next can now be pressed"))
                scored = True
                break
            self.capture(app, f"innosetup-{step}", hwnd)
            forward = find(read, "button", "OK") or find(read, "button", None, "all users") or find(read, "button", None, "Next")
            if forward is None:
                break
            before = win.grab(win.frame_of(hwnd) or win.rect_of(hwnd))
            self.act(app, f"UI Automation: press '{forward['name']}' in the installer", lambda f=forward: self.auto.press(f["id"]),
                     lambda h=hwnd, b=before: ((not win.IsWindow(h)) or win.differing(b, win.grab(win.frame_of(h) or win.rect_of(h))) > 0.02, "the window closed or its page changed"), wait=2.5)
        if not scored:
            self.note("innosetup: the licence page was not reached")
        subprocess.run(["taskkill.exe", "/f", "/t", "/pid", str(process.pid)], capture_output=True)
        subprocess.run(["taskkill.exe", "/f", "/im", installer.stem + ".tmp"], capture_output=True)

    def scenario_rights(self):
        """Can an agent without administrator rights read and operate a window of a program that has them?"""
        process = self.launch(["notepad.exe"])
        window = win.wait_window(lambda w: w["exe"] == "notepad.exe" and w["title"], 40)
        if window is None:
            self.note("rights: Notepad showed no window: NOT RUN")
            return
        target = pathlib.Path(r"C:\Users\Public\inwin\out\lower.json")
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists():
            target.unlink()
        import sys
        command = f'"{sys.executable}" "{HERE / "lower.py"}" {window["hwnd"]} "{target}"'
        done = subprocess.run(["runas.exe", "/trustlevel:0x20000", command], capture_output=True, text=True, timeout=60)
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline and not target.exists():
            time.sleep(2)
        if target.exists():
            time.sleep(1)
            result = json.loads(target.read_text(encoding="utf-8"))
            (self.out / "lower-rights.json").write_text(json.dumps(result, indent=1), encoding="utf-8")
            self.note(f"rights: an agent with {result.get('rights')} against an elevated Notepad: {json.dumps(result.get('findings'))[:600]}")
        else:
            self.note(f"rights: the restricted agent wrote no result (runas exit {done.returncode}): NOT RUN")
        process.kill()

    def run(self, only):
        scenarios = [
            ("controls", self.scenario_controls), ("notepad", self.scenario_notepad), ("explorer", self.scenario_explorer),
            ("settings", self.scenario_settings), ("browsers", self.scenario_browsers), ("ribbon", self.scenario_ribbon),
            ("putty", self.scenario_putty), ("notepadpp", self.scenario_notepadpp), ("sevenzip", self.scenario_sevenzip),
            ("installer", self.scenario_installer), ("rights", self.scenario_rights),
        ]
        try:
            win.save_png(self.raw / "microsoft" / "desktop-at-start.png", win.grab())
            self.note(f"screen {win.screen_box()}; top-level windows at the start: {[(w['class'], w['exe']) for w in win.top_windows()][:14]}")
        except Exception as error:
            self.note(f"no picture of the desktop: {error}")
        for name, scenario in scenarios:
            if only and not re.search(only, name):
                continue
            print(f"== {name}", flush=True)
            began = time.monotonic()
            try:
                scenario()
            except Exception:
                self.note(f"{name}: the scenario stopped: {traceback.format_exc()[-900:]}")
            print(f"== {name}: {time.monotonic() - began:.0f} s", flush=True)
        self.write()

    def write(self):
        result = {"captures": self.captures, "checks": self.checks, "notes": self.notes}
        (self.out / "see.json").write_text(json.dumps(result, indent=1), encoding="utf-8")
        tiers = ["uia", "bridge-win32", "bridge-msaa", "bridge-uia", "uia or win32"]
        lines = ["### Controls found in each window", "",
                 "Controls a tier reports on screen; where a list of what a person sees exists: found with the right kind / listed.", "",
                 "| Window | " + " | ".join(tiers) + " | UI Automation read (ms) |", "|---|" + "---|" * (len(tiers) + 1)]
        for row in self.captures:
            cells = []
            for tier in tiers:
                s = row["tiers"].get(tier)
                if s is None:
                    cells.append("NOT RUN")
                elif "score" in s:
                    cells.append(f"{s['score']['found_with_kind']} / {s['score']['truth']}" + (f" ({s['controls']} seen)" if "controls" in s else ""))
                else:
                    cells.append(f"{s['controls']} seen" + (" (error)" if s.get("error") else ""))
            lines.append(f"| {row['app']}: {row['label']} | " + " | ".join(cells) + f" | {row['tiers'].get('uia', {}).get('milliseconds')} |")
        lines += ["", "### Actions", "", "| Application | Action | Arrived | How it was verified |", "|---|---|---|---|"]
        for check in self.checks:
            lines.append(f"| {check['app']} | {check['what']} | {'yes' if check['ok'] else '**no**'} | {check['detail'].replace('|', '/')} |")
        lines += ["", f"{sum(1 for c in self.checks if c['ok'])} of {len(self.checks)} actions arrived.", "", "### Notes", ""] + [f"- {note.replace(chr(10), ' ')[:500]}" for note in self.notes]
        (self.out / "see.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
        print("\n".join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True)
    parser.add_argument("--raw", required=True)
    parser.add_argument("--bridge", required=True)
    parser.add_argument("--downloads", required=True)
    parser.add_argument("--only", default="")
    arguments = parser.parse_args()
    win.dpi_aware()
    Seer(arguments).run(arguments.only)


if __name__ == "__main__":
    main()
