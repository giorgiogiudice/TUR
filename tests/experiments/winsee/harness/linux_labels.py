#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The Linux half of the labelled data for the vision test of the winsee
experiment (docs/reports/exp-winsee-1.md).

It shows applications that DO have accessibility trees (a GTK 3 window of
P@x's accessibility test, a GTK 3 dialog, a Qt Quick dialog, Firefox with a
form) on an X server without a screen, photographs each, and reads from the
accessibility bus where each named control is. The names and rectangles
are the labels: nobody drew them by hand. A model is later asked "where is
<name>" and scored against the rectangle (vision/evaluate.py).

The tree is read with libatspi through its GObject bindings: a test tool,
not part of TUR (tur-pax-a11y speaks the bus itself). X is used, not
Wayland, because only there does the accessibility tree give positions on
the screen that a screenshot shares.

    dbus-run-session -- linux_labels.py --evidence DIR --repository DIR

Runs on the CI runner inside a Fedora container, as an ordinary user.
"""

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
import traceback

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from common import Display, choose_targets, crop_box, write_capture  # noqa: E402

# AT-SPI role numbers to the names tur-pax-a11y uses (atspi-constants.h).
ROLES = {
    2: "alert", 7: "check box", 8: "check menu item", 11: "combo box", 16: "dialog", 23: "frame", 27: "image",
    29: "label", 31: "list", 32: "list item", 33: "menu", 34: "menu bar", 35: "menu item", 37: "page tab",
    38: "page tab list", 39: "panel", 40: "password text", 43: "push button", 44: "radio button", 51: "slider",
    52: "spin button", 56: "table cell", 61: "text", 62: "toggle button", 63: "tool bar", 69: "window", 73: "paragraph",
    75: "application", 79: "entry", 82: "document frame", 83: "heading", 85: "section", 88: "link", 91: "tree item",
    95: "document web", 116: "static", 129: "push button", 130: "toggle button",
}
WINDOW_ROLES = {"frame", "dialog", "window", "alert"}


def gdbus(*arguments, timeout=30):
    done = subprocess.run(["gdbus", "call", *arguments], capture_output=True, text=True, timeout=timeout, check=False)
    return done.returncode, (done.stdout or done.stderr).strip()


class Labeller:
    def __init__(self, arguments):
        self.evidence = pathlib.Path(arguments.evidence).resolve()
        self.repository = pathlib.Path(arguments.repository).resolve()
        self.dataset = self.evidence / "dataset"
        self.logs = self.evidence / "logs"
        self.logs.mkdir(parents=True, exist_ok=True)
        self.display = Display(self.logs, number=98)
        self.results = {"windows": [], "notes": []}
        self.atspi = None

    def environment(self, **extra):
        environment = dict(os.environ)
        environment.update({"DISPLAY": self.display.name, "GDK_BACKEND": "x11", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "NO_AT_BRIDGE": "0"})
        environment.pop("WAYLAND_DISPLAY", None)
        environment.update(extra)
        return environment

    def accessibility_up(self):
        code, out = gdbus("--session", "--dest", "org.a11y.Bus", "--object-path", "/org/a11y/bus", "--method", "org.a11y.Bus.GetAddress")
        self.results["accessibility_bus"] = out[:200]
        code, out = gdbus("--session", "--dest", "org.a11y.Bus", "--object-path", "/org/a11y/bus", "--method",
                          "org.freedesktop.DBus.Properties.Set", "org.a11y.Status", "IsEnabled", "<true>")
        self.results["accessibility_enabled"] = code == 0
        os.environ["DISPLAY"] = self.display.name
        import gi

        gi.require_version("Atspi", "2.0")
        from gi.repository import Atspi

        Atspi.init()
        self.atspi = Atspi

    def frames(self):
        """Every window on the accessibility bus: (application, accessible)."""
        Atspi = self.atspi
        found = []
        try:
            desktop = Atspi.get_desktop(0)
            for index in range(desktop.get_child_count()):
                application = desktop.get_child_at_index(index)
                if application is None:
                    continue
                for child_index in range(application.get_child_count()):
                    child = application.get_child_at_index(child_index)
                    if child is not None and ROLES.get(int(child.get_role()), "") in WINDOW_ROLES:
                        found.append((application.get_name() or "", child))
        except Exception as error:  # noqa: BLE001
            self.results["notes"].append(f"reading the desktop failed: {error!r}")
        return found

    def extents(self, accessible):
        Atspi = self.atspi
        try:
            rect = accessible.get_extents(Atspi.CoordType.SCREEN)
        except Exception:  # noqa: BLE001
            try:
                rect = accessible.get_component_iface().get_extents(Atspi.CoordType.SCREEN)
            except Exception:  # noqa: BLE001
                return None
        if rect is None or rect.width <= 0 or rect.height <= 0:
            return None
        return [rect.x, rect.y, rect.width, rect.height]

    def walk(self, accessible, out, depth=0, budget=None):
        Atspi = self.atspi
        budget = budget if budget is not None else [3000]
        if depth > 40 or budget[0] <= 0:
            return
        budget[0] -= 1
        try:
            states = accessible.get_state_set()
            showing = states.contains(Atspi.StateType.SHOWING) and states.contains(Atspi.StateType.VISIBLE)
            role = ROLES.get(int(accessible.get_role()), f"role {int(accessible.get_role())}")
            name = accessible.get_name() or ""
            count = accessible.get_child_count()
        except Exception:  # noqa: BLE001
            return
        if depth > 0 and not showing:
            return
        rect = self.extents(accessible)
        if rect is not None:
            out.append({"name": name, "role": role, "rect": rect, "depth": depth})
        for index in range(min(count, 300)):
            try:
                child = accessible.get_child_at_index(index)
            except Exception:  # noqa: BLE001
                continue
            if child is not None:
                self.walk(child, out, depth + 1, budget)

    def photograph(self, label, source, match, settle=3.0, timeout=90):
        """Wait for a window whose application or title matches, then write
        its screenshot and labels."""
        deadline = time.monotonic() + timeout
        frame = None
        while time.monotonic() < deadline and frame is None:
            for application, accessible in self.frames():
                try:
                    title = accessible.get_name() or ""
                except Exception:  # noqa: BLE001
                    continue
                if match(application, title) and self.extents(accessible):
                    frame = (application, title, accessible)
                    break
            if frame is None:
                time.sleep(1)
        record = {"label": label, "source": source, "on_the_bus": frame is not None}
        if frame is None:
            record["seen"] = [(application, (accessible.get_name() or "")[:60]) for application, accessible in self.frames()][:20]
            self.results["windows"].append(record)
            print(f"  {label}: no such window on the accessibility bus; there: {record['seen']}", flush=True)
            return
        time.sleep(settle)
        application, title, accessible = frame
        shot = self.display.shot(self.evidence / "linux" / f"{label}.png")
        nodes = []
        self.walk(accessible, nodes)
        (self.evidence / "linux" / f"{label}-tree.json").write_text(json.dumps(nodes, indent=1))
        rect = self.extents(accessible)
        record.update({"application": application, "title": title, "window_rect": rect, "elements": len(nodes), "screenshot": bool(shot)})
        if shot and rect:
            box = crop_box(rect, margin=30)
            targets, seen = choose_targets([node for node in nodes if node["depth"] > 0], box)
            record["named_controls"] = seen
            record["targets"] = len(targets)
            if targets:
                write_capture(self.dataset, f"linux-{label}", shot, box, targets, source, "the accessibility tree (AT-SPI), read with libatspi", title, seen)
        self.results["windows"].append(record)
        print(f"  {label}: {record}", flush=True)

    def start(self, name, command, **extra):
        log = open(self.logs / f"{name}.log", "wb")
        process = subprocess.Popen([str(part) for part in command], env=self.environment(**extra), stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
        log.close()
        return process

    @staticmethod
    def stop(process):
        if process is None:
            return
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()

    def run(self):
        self.display.up()
        self.results["window_manager"] = self.display.window_manager
        try:
            self.accessibility_up()
        except Exception as error:  # noqa: BLE001
            traceback.print_exc()
            self.results["notes"].append(f"no accessibility bus: {error!r}")
            self.finish()
            return 0
        work = pathlib.Path(os.environ.get("HOME", "/tmp")) / "winsee-linux"
        work.mkdir(parents=True, exist_ok=True)

        print("== GTK 3: the mail window of P@x's accessibility test", flush=True)
        process = self.start("gtk-mail", ["python3", self.repository / "tests/pax/a11y/mail_app.py", work / "mail.log"])
        self.photograph("gtk3-mail", "linux:gtk3", lambda application, title: "TUR test mail" in title or "tur-a11y-mail" in application)
        self.stop(process)

        print("== GTK 3: a dialog", flush=True)
        process = self.start("gtk-dialog", ["python3", HERE / "gtk_dialog.py"])
        self.photograph("gtk3-dialog", "linux:gtk3", lambda application, title: title == "Network connection")
        self.stop(process)

        print("== Qt Quick: a dialog", flush=True)
        runner = next((path for path in (shutil.which("qml6"), shutil.which("qml-qt6"), "/usr/lib64/qt6/bin/qml", shutil.which("qml")) if path and os.path.exists(path)), None)
        if runner is None:
            self.results["notes"].append("no qml runtime was found: the Qt Quick dialog was NOT RUN")
        else:
            process = self.start("qt-dialog", [runner, HERE / "qt_dialog.qml"], QT_QPA_PLATFORM="xcb", QT_QUICK_BACKEND="software",
                                 QT_LINUX_ACCESSIBILITY_ALWAYS_ON="1", QT_QUICK_CONTROLS_STYLE="Basic")
            self.photograph("qt-dialog", "linux:qtquick", lambda application, title: title == "Printer options")
            self.stop(process)

        print("== Firefox: a form", flush=True)
        if shutil.which("firefox") is None:
            self.results["notes"].append("no firefox: NOT RUN")
        else:
            profile = work / "firefox-profile"
            profile.mkdir(parents=True, exist_ok=True)
            (profile / "user.js").write_text("\n".join([
                'user_pref("browser.shell.checkDefaultBrowser", false);',
                'user_pref("browser.startup.homepage_override.mstone", "ignore");',
                'user_pref("browser.aboutwelcome.enabled", false);',
                'user_pref("datareporting.policy.dataSubmissionEnabled", false);',
                'user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);',
                'user_pref("app.update.disabledForTesting", true);',
                'user_pref("browser.tabs.warnOnClose", false);',
                'user_pref("accessibility.force_disabled", 0);',
                "",
            ]))
            process = self.start("firefox", ["firefox", "--no-remote", "--profile", profile, "--new-instance", "--width", "1000", "--height", "700",
                                             f"file://{HERE / 'form.html'}"],
                                 GNOME_ACCESSIBILITY="1", MOZ_CRASHREPORTER_DISABLE="1", MOZ_ENABLE_WAYLAND="0")
            self.photograph("firefox-form", "linux:firefox", lambda application, title: "TUR winsee form" in title, settle=6.0, timeout=150)
            self.stop(process)
        self.finish()
        return 0

    def finish(self):
        self.display.down()
        (self.evidence / "linux-labels.json").write_text(json.dumps(self.results, indent=1))
        lines = ["### winsee: labelled windows from the accessibility tree (Linux)", "",
                 f"Accessibility bus: {self.results.get('accessibility_bus')}; window manager: {self.results.get('window_manager')}", "",
                 "| Window | On the bus | Elements with a position | Named controls | Asked about |", "|---|---|---|---|---|"]
        for window in self.results["windows"]:
            lines.append(f"| {window['label']} ({window['source']}) | {'yes' if window['on_the_bus'] else 'no'} | {window.get('elements', '')} | "
                         f"{window.get('named_controls', '')} | {window.get('targets', '')} |")
        for note in self.results["notes"]:
            lines.append(f"- note: {note}")
        (self.evidence / "linux-labels.md").write_text("\n".join(lines) + "\n")
        print("\n".join(lines), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--repository", required=True)
    return Labeller(parser.parse_args()).run()


if __name__ == "__main__":
    sys.exit(main())
